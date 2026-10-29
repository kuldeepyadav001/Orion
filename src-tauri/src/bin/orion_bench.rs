//! Retrieval performance at scale, and the headless half of the cold-start budget.
//!
//! ## What this replaces
//!
//! Orion stores vectors as BLOBs in the same SQLite file as everything else
//! and scans them in Rust. That decision was justified by *arithmetic* — a
//! few thousand chunks times 384 floats is a few megabytes, so a linear scan
//! is cheaper than running a vector database — but arithmetic is not a
//! measurement, and the whole point of this project is that claims get
//! measured. This binary measures it.
//!
//! It also measures the two startup phases that do not need a window:
//! opening the database and running the migration ladder, and profiling the
//! hardware. The remaining three phases (`runtime init`, `tray and hotkey`,
//! `window show`) need a desktop session and are reported as unmeasured
//! rather than guessed.
//!
//! ## Running it
//!
//! ```text
//! cargo run --release --bin orion_bench
//! cargo run --release --bin orion_bench -- --chunks 1000,10000 --queries 40
//! ```
//!
//! It writes to a temporary directory and removes it afterwards. It never
//! touches the user's real library.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use orion_lib::db::Db;
use orion_lib::ingest::{Chunk, Format, IngestedDocument};
use orion_lib::profiler::HardwareProfile;
use orion_lib::rag::embed::{normalize, EMBED_DIM};
use orion_lib::rag::store::RagStore;

/// The M3 per-phase budget for opening the database and migrating it.
const DATABASE_BUDGET_MS: u128 = 150;
/// The M3 per-phase budget for reading config and profiling the hardware.
const CONFIG_BUDGET_MS: u128 = 100;

/* ------------------------------------------------------------------ */
/* Deterministic synthetic corpus                                     */
/* ------------------------------------------------------------------ */

struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

/// Vocabulary chosen so that the text is compressible by FTS5 but not
/// degenerate: real documents are English with repeated terms of art.
const WORDS: &[&str] = &[
    "annual", "leave", "accrual", "policy", "employee", "contract", "termination",
    "notice", "period", "salary", "expenses", "travel", "reimbursement", "invoice",
    "confidential", "security", "incident", "response", "audit", "compliance",
    "deployment", "pipeline", "release", "rollback", "migration", "database",
    "index", "latency", "throughput", "monitoring", "alert", "threshold",
    "customer", "support", "warranty", "liability", "insurance", "renewal",
    "supplier", "procurement", "purchase", "order", "delivery", "warehouse",
    "research", "methodology", "results", "analysis", "discussion", "appendix",
    "quarterly", "revenue", "margin", "forecast", "budget", "headcount",
    "onboarding", "offboarding", "payroll", "benefits", "pension", "equity",
];

const SENTENCES: &[&str] = &[
    "The annual leave accrual rate is recorded in the handbook and applies to all full-time staff.",
    "Either party may terminate this agreement by giving thirty days notice in writing.",
    "Reimbursable travel expenses require a receipt submitted within thirty days of the journey.",
    "A security incident must be reported to the platform owner within one hour of discovery.",
    "The deployment pipeline runs the migration ladder before the release is promoted.",
    "Index latency is measured at the ninety-fifth percentile over a rolling window.",
    "The warranty period is twelve months from the date of delivery to the customer.",
    "The purchase order is issued once procurement has approved the supplier quotation.",
    "The appendix contains the full methodology and the results of the analysis.",
    "The forecast assumes headcount and revenue grow at the same rate each quarter.",
    "Payroll is processed monthly and benefits are provided from the employee's start date.",
];

/// Build a deterministic corpus of `chunks` chunks spread over `docs`
/// documents, with a semantic topic vector per document.
struct Corpus {
    chunks: Vec<(i64, String, String)>,
    vectors: Vec<(i64, Vec<f32>)>,
    queries: Vec<(String, Vec<f32>)>,
}

fn build_corpus(total_chunks: usize, chunk_size: usize) -> Corpus {
    let mut rng = Rng(0xA5A5_1234_9E37_0001);
    let doc_count = total_chunks.div_ceil(chunk_size).max(1);

    // One topic vector per document. Chunks inherit most of it, so the
    // corpus has the same clustering a real library has: a document's
    // chunks are more like each other than like anything else.
    let mut doc_vectors: HashMap<usize, Vec<f32>> = HashMap::new();
    for d in 0..doc_count {
        let v: Vec<f32> = (0..EMBED_DIM).map(|_| rng.next_u64() as f32 / u64::MAX as f32).collect();
        doc_vectors.insert(d, normalize(v));
    }

    let mut chunks = Vec::with_capacity(total_chunks);
    let mut vectors = Vec::with_capacity(total_chunks);
    let mut id = 0i64;

    for d in 0..doc_count {
        let topic = &doc_vectors[&d];
        for c in 0..chunk_size {
            if chunks.len() >= total_chunks {
                break;
            }
            let sentence = SENTENCES[rng.below(SENTENCES.len())];
            let mut body = String::from(sentence);
            for _ in 0..6 {
                body.push(' ');
                body.push_str(WORDS[rng.below(WORDS.len())]);
            }
            let heading = format!("Section {} › Clause {}", d + 1, c + 1);
            // 80% topic, 20% noise: close enough to a real embedding's
            // within-document similarity that the ranking work is realistic.
            let mut v: Vec<f32> = (0..EMBED_DIM)
                .map(|i| {
                    let noise = rng.next_u64() as f32 / u64::MAX as f32 - 0.5;
                    topic[i] * 0.8 + noise * 0.2
                })
                .collect();
            v = normalize(v);

            chunks.push((id, heading.clone(), body.clone()));
            vectors.push((id, v));
            id += 1;
        }
    }

    // Queries: a phrase taken from a real chunk, so lexical retrieval has
    // something to find, plus the chunk's own vector so semantic retrieval
    // has something to find. This is the easy case; it is the *latency*
    // that is being measured, not the quality.
    let queries = chunks
        .iter()
        .enumerate()
        .filter(|(i, _)| i % 37 == 0)
        .map(|(i, (_, _, body))| {
            let terms: Vec<&str> = body.split_whitespace().take(4).collect();
            (terms.join(" "), vectors[i].1.clone())
        })
        .collect();

    Corpus {
        chunks,
        vectors,
        queries,
    }
}

/* ------------------------------------------------------------------ */
/* Timing helpers                                                      */
/* ------------------------------------------------------------------ */

struct Stats {
    samples: Vec<Duration>,
}

impl Stats {
    fn new() -> Self {
        Self {
            samples: Vec::new(),
        }
    }

    fn push(&mut self, d: Duration) {
        self.samples.push(d);
    }

    fn sorted(&self) -> Vec<Duration> {
        let mut s = self.samples.clone();
        s.sort();
        s
    }

    fn p(&self, q: f64) -> Duration {
        let s = self.sorted();
        if s.is_empty() {
            return Duration::ZERO;
        }
        let idx = ((s.len() as f64 - 1.0) * q).round() as usize;
        s[idx.min(s.len() - 1)]
    }

    fn mean(&self) -> Duration {
        if self.samples.is_empty() {
            return Duration::ZERO;
        }
        let total: Duration = self.samples.iter().sum();
        total / self.samples.len() as u32
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Resident set size of this process, in kibibytes.
fn rss_kib() -> u64 {
    let Ok(s) = std::fs::read_to_string("/proc/self/status") else {
        return 0;
    };
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            return rest
                .trim()
                .trim_end_matches(" kB")
                .trim()
                .parse()
                .unwrap_or(0);
        }
    }
    0
}

/* ------------------------------------------------------------------ */
/* Cold start: the phases that do not need a window                   */
/* ------------------------------------------------------------------ */

fn measure_cold_start(path: &PathBuf) {
    println!("=== cold start: headless phases ===");

    // Runtime init is measured by the application itself: `run()` records
    // the elapsed time from process start to the Tauri runtime being ready.
    // It cannot be reproduced here, because that needs the event loop.

    // Database: open a fresh file, run the migration ladder, and bring the
    // retrieval schema up to date. The application does all three inside
    // its "database" phase.
    let fresh = path.join("cold-fresh.db");
    let _ = std::fs::remove_file(&fresh);
    let t = Instant::now();
    let db = Db::open(&fresh).expect("cold database");
    RagStore::new(db.conn()).migrate().expect("rag schema");
    let cold = t.elapsed();
    drop(db);
    println!(
        "  database (fresh file + migrations): {:.1} ms  [budget {DATABASE_BUDGET_MS} ms]",
        ms(cold)
    );

    // Reopen the same file: the realistic case, where the schema is current
    // and only the integrity check runs.
    let t = Instant::now();
    let db = Db::open(&fresh).expect("warm database");
    let warm = t.elapsed();
    drop(db);
    println!(
        "  database (existing file): {:.1} ms  [budget {DATABASE_BUDGET_MS} ms]",
        ms(warm)
    );

    // Config: hardware profiling.
    let t = Instant::now();
    let profile = HardwareProfile::detect_or_forced();
    let config = t.elapsed();
    println!(
        "  config (hardware profile): {:.1} ms  [budget {CONFIG_BUDGET_MS} ms]  \
         tier={} ram={:.1} GiB cores={}",
        ms(config),
        profile.recommend().tier.as_str(),
        profile.total_ram_gib,
        profile.cpu_cores
    );

    println!(
        "  tray and hotkey, window show: NOT MEASURED — these need a desktop session."
    );
    let _ = std::fs::remove_file(&fresh);
    println!();
}

/* ------------------------------------------------------------------ */
/* Retrieval at scale                                                  */
/* ------------------------------------------------------------------ */

fn measure_scale(total_chunks: usize, chunk_size: usize, query_count: usize) {
    println!("=== retrieval at scale: {total_chunks} chunks ===");

    let dir = PathBuf::from(format!("/tmp/orion-bench-{total_chunks}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("bench dir");
    let db_path = dir.join("bench.db");

    let corpus = build_corpus(total_chunks, chunk_size);
    let rss_before = rss_kib();

    let db = Db::open(&db_path).expect("bench db");
    let store = RagStore::new(db.conn());
    store.migrate().expect("rag schema");

    // --- ingest -------------------------------------------------------
    let mut per_doc: Vec<IngestedDocument> = Vec::new();
    let mut per_doc_vectors: Vec<Vec<Vec<f32>>> = Vec::new();
    let mut doc_id = 0i64;
    while (doc_id as usize) * chunk_size < corpus.chunks.len() {
        let start = (doc_id as usize) * chunk_size;
        let end = (start + chunk_size).min(corpus.chunks.len());
        let slice = &corpus.chunks[start..end];
        let chunks: Vec<Chunk> = slice
            .iter()
            .enumerate()
            .map(|(i, (_, heading, body))| Chunk {
                text: format!("{heading}\n{body}"),
                body: body.clone(),
                page: None,
                headings: vec![heading.clone()],
                index: i,
            })
            .collect();
        let embeds: Vec<Vec<f32>> = corpus.vectors[start..end]
            .iter()
            .map(|(_, v)| v.clone())
            .collect();
        per_doc.push(IngestedDocument {
            id: format!("doc-{doc_id}"),
            name: format!("Document {doc_id}.md"),
            format: Format::Markdown,
            chunks,
        });
        per_doc_vectors.push(embeds);
        doc_id += 1;
    }

    let t = Instant::now();
    for (doc, embeds) in per_doc.iter().zip(per_doc_vectors.iter()) {
        store.upsert_document(doc, embeds).expect("upsert");
    }
    let ingest = t.elapsed();
    let rss_after = rss_kib();

    let db_bytes = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);
    println!(
        "  ingest: {:.0} ms total, {:.3} ms/chunk, {:.0} chunks/s",
        ms(ingest),
        ms(ingest) / total_chunks as f64,
        (total_chunks as f64 / ingest.as_secs_f64()) as u64
    );
    println!(
        "  database on disk: {:.1} MiB ({:.0} bytes/chunk); RSS +{} MiB",
        db_bytes as f64 / (1024.0 * 1024.0),
        db_bytes as f64 / total_chunks as f64,
        (rss_after.saturating_sub(rss_before)) as f64 / 1024.0
    );

    // --- query latency -------------------------------------------------
    let queries: Vec<(String, Vec<f32>)> = corpus
        .queries
        .iter()
        .take(query_count.max(1))
        .cloned()
        .collect();

    let mut total = Stats::new();
    let mut lex = Stats::new();
    let mut dense = Stats::new();
    let mut legacy = Stats::new();

    for (q, qv) in &queries {
        let t = Instant::now();
        let hits = store.search(q, qv, 4).expect("search");
        total.push(t.elapsed());
        if hits.is_empty() {
            continue;
        }
        let t = Instant::now();
        let _ = store.bm25_only(q, 4);
        lex.push(t.elapsed());
        let t = Instant::now();
        let _ = store.dense_only(qv, 4);
        dense.push(t.elapsed());

        // The previous implementation, for comparison: materialise the whole
        // corpus, compute a full cosine for every vector, then sort the
        // whole thing. Same machine, same data, same binary — the only
        // difference is the algorithm.
        let t = Instant::now();
        let corpus = store.all_vectors().expect("all vectors");
        let mut scored: Vec<(i64, f32)> = corpus
            .iter()
            .map(|(id, v)| (*id, orion_lib::rag::search::cosine(qv, v)))
            .filter(|(_, s)| *s > 0.0)
            .collect();
        scored.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.0.cmp(&b.0))
        });
        let _ = scored.iter().take(4).count();
        legacy.push(t.elapsed());
    }

    println!(
        "  search (BM25 + dense + order): p50 {:.1} ms, p95 {:.1} ms, mean {:.1} ms  \
         (n = {})",
        ms(total.p(0.50)),
        ms(total.p(0.95)),
        ms(total.mean()),
        total.samples.len()
    );
    println!(
        "    lexical only: p50 {:.1} ms | semantic only: p50 {:.1} ms | combined: p50 {:.1} ms",
        ms(lex.p(0.50)),
        ms(dense.p(0.50)),
        ms(total.p(0.50))
    );
    println!(
        "    previous scan (materialise + cosine + full sort): p50 {:.1} ms  \
         -> {:.1}x slower",
        ms(legacy.p(0.50)),
        if legacy.p(0.50) > Duration::ZERO {
            legacy.p(0.50).as_secs_f64() / dense.p(0.50).max(Duration::from_nanos(1)).as_secs_f64()
        } else {
            0.0
        }
    );

    let _ = std::fs::remove_dir_all(dir);
    println!();
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut sizes: Vec<usize> = vec![1_000, 10_000, 25_000];
    let mut queries = 40usize;
    let mut chunk_size = 20usize;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--chunks" => {
                i += 1;
                let raw = args.get(i).cloned().unwrap_or_default();
                sizes = raw
                    .split(',')
                    .filter_map(|s| s.trim().parse::<usize>().ok())
                    .filter(|n| *n > 0)
                    .collect();
                if sizes.is_empty() {
                    sizes = vec![1_000];
                }
            }
            "--queries" => {
                i += 1;
                queries = args
                    .get(i)
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(40);
            }
            "--chunk-size" => {
                i += 1;
                chunk_size = args
                    .get(i)
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(20);
            }
            "-h" | "--help" => {
                println!("usage: orion_bench [--chunks 1000,10000] [--queries 40] [--chunk-size 20]");
                return;
            }
            other => eprintln!("orion_bench: ignoring unknown argument {other}"),
        }
        i += 1;
    }

    println!("Orion retrieval benchmark");
    println!("host: {} cores, {} MiB RSS", std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0), rss_kib() / 1024);
    println!();

    let dir = PathBuf::from("/tmp/orion-bench-cold");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("bench dir");
    measure_cold_start(&dir);
    let _ = std::fs::remove_dir_all(&dir);

    for size in sizes {
        measure_scale(size, chunk_size, queries);
    }
}
