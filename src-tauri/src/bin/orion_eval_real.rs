//! Retrieval evaluation over a **real** corpus, at realistic scale.
//!
//! `orion_eval` scores the M2 suite: 30 questions against 22 chunks. That
//! corpus is deliberately adversarial but *small*, and a 22-chunk corpus is
//! easy for every retriever — a system that returns the four nearest
//! neighbours out of twenty-two is not being tested at all.
//!
//! This binary scores a second suite: Orion's own documentation, chunked
//! with the real chunker into a few hundred chunks of real prose, with
//! questions whose ground truth is a **text anchor** rather than a chunk
//! id, so the suite survives the documents being edited.
//!
//! It answers the question the small suite cannot: does dense-first
//! ordering still beat keyword retrieval when the corpus is two hundred
//! chunks instead of twenty-two?
//!
//! ## Usage
//!
//! ```text
//! cargo run --release --bin orion_eval_real -- \\
//!     --corpus docs/PROJECT-STATE.md,docs/ORION-PLAN.md,README.md \\
//!     --questions eval/real-corpus-questions.json \\
//!     --url http://127.0.0.1:8082 --token orion-eval-token
//! ```
//!
//! `scripts/run-eval.sh --real` wraps this.

use std::collections::HashMap;
use std::process::ExitCode;

use orion_lib::ingest::chunker::{chunk_blocks, ChunkConfig};
use orion_lib::ingest::extract::extract;
use orion_lib::ingest::Format;
use orion_lib::rag::embed::{EmbedConfig, Embedder, EmbedKind, EMBED_DIM, EMBED_MODEL_ID};
use orion_lib::rag::eval::{
    indexed_text, run_questions, EvalQuestion, EvalReport, Metric, Probe,
};
use orion_lib::rag::stats::{compare_paired, wilson_interval};

const Z_95: f64 = 1.959_963_984_540_054;

/// A question as it appears in the suite file.
struct RawQuestion {
    id: u32,
    q: String,
    /// Distinctive phrases that identify the chunk that answers the
    /// question. All of them must appear in the same chunk.
    anchors: Vec<String>,
    probe: Probe,
    /// True when the corpus does not contain the answer at all.
    unanswerable: bool,
    /// Restricts the anchor search to chunks from files whose path contains
    /// this text. Used by the disambiguation questions, where two documents
    /// legitimately say different things and the question asks about one of
    /// them specifically.
    source: Option<String>,
}

/// The suite file.
struct RawSuite {
    name: String,
    corpus: Vec<String>,
    questions: Vec<RawQuestion>,
}

fn parse_probe(s: &str) -> Probe {
    match s.to_ascii_lowercase().as_str() {
        "paraphrase" => Probe::Paraphrase,
        "exact" => Probe::Exact,
        "heading" | "headingscoped" => Probe::HeadingScoped,
        "disambiguation" => Probe::Disambiguation,
        _ => Probe::Unanswerable,
    }
}

/// A deliberately small JSON reader.
///
/// Pulling in a JSON crate for a benchmark would add a dependency to the
/// shipping crate's dependency graph for no product reason, so the suite
/// file uses a line-oriented format that this reads directly.
fn parse_suite(text: &str) -> Result<RawSuite, String> {
    let mut name = String::new();
    let mut corpus: Vec<String> = Vec::new();
    let mut questions: Vec<RawQuestion> = Vec::new();

    #[derive(PartialEq)]
    enum Mode {
        None,
        Corpus,
        Questions,
    }
    let mut mode = Mode::None;
    let mut cur: Option<RawQuestion> = None;

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("name:") {
            name = rest.trim().trim_matches('"').to_string();
            continue;
        }
        if line == "[corpus]" {
            mode = Mode::Corpus;
            continue;
        }
        if line == "[questions]" {
            mode = Mode::Questions;
            continue;
        }
        match mode {
            Mode::None => {}
            Mode::Corpus => corpus.push(line.trim_matches('"').to_string()),
            Mode::Questions => {
                if line.starts_with("question ") {
                    if let Some(q) = cur.take() {
                        questions.push(q);
                    }
                    // `question 12 | text here`
                    let body = line["question ".len()..].trim();
                    let (id, text) = body
                        .split_once('|')
                        .ok_or_else(|| format!("malformed question line: {line}"))?;
                    cur = Some(RawQuestion {
                        id: id.trim().parse().map_err(|_| "bad question id".to_string())?,
                        q: text.trim().to_string(),
                        anchors: Vec::new(),
                        probe: Probe::Paraphrase,
                        unanswerable: false,
                        source: None,
                    });
                } else if let Some(rest) = line.strip_prefix("anchor ") {
                    let q = cur
                        .as_mut()
                        .ok_or_else(|| "anchor before question".to_string())?;
                    q.anchors.push(rest.trim().to_string());
                } else if let Some(rest) = line.strip_prefix("probe ") {
                    let q = cur
                        .as_mut()
                        .ok_or_else(|| "probe before question".to_string())?;
                    q.probe = parse_probe(rest.trim());
                } else if let Some(rest) = line.strip_prefix("source ") {
                    let q = cur
                        .as_mut()
                        .ok_or_else(|| "source before question".to_string())?;
                    q.source = Some(rest.trim().to_string());
                } else if let Some(rest) = line.strip_prefix("unanswerable") {
                    let q = cur
                        .as_mut()
                        .ok_or_else(|| "unanswerable before question".to_string())?;
                    q.unanswerable = rest.trim() != "false";
                }
            }
        }
    }
    if let Some(q) = cur.take() {
        questions.push(q);
    }
    if name.is_empty() {
        return Err("suite has no name".into());
    }
    if questions.is_empty() {
        return Err("suite has no questions".into());
    }
    Ok(RawSuite {
        name,
        corpus,
        questions,
    })
}

struct Args {
    corpus: Vec<String>,
    questions: String,
    url: String,
    token: String,
    k: usize,
    verbose: bool,
    /// Override the chunker's maximum chunk size, so the effect of chunk
    /// granularity on retrieval quality can be measured rather than guessed.
    chunk_max: Option<usize>,
    /// Print the relevant and retrieved chunk texts for these question ids.
    show: Vec<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut corpus: Vec<String> = Vec::new();
    let mut questions = "eval/real-corpus-questions.txt".to_string();
    let mut url = std::env::var("ORION_EVAL_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8082".to_string());
    let mut token =
        std::env::var("ORION_EVAL_TOKEN").unwrap_or_else(|_| "orion-eval-token".to_string());
    let mut k = 4usize;
    let mut verbose = false;
    let mut chunk_max: Option<usize> = None;
    let mut show: Vec<String> = Vec::new();

    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--corpus" => {
                let raw = it.next().ok_or("--corpus needs a value")?;
                corpus = raw
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            "--questions" => questions = it.next().ok_or("--questions needs a value")?,
            "--url" => url = it.next().ok_or("--url needs a value")?,
            "--token" => token = it.next().ok_or("--token needs a value")?,
            "--k" => {
                k = it
                    .next()
                    .ok_or("--k needs a value")?
                    .parse()
                    .map_err(|_| "--k must be a number")?
            }
            "--chunk-max" => {
                chunk_max = Some(
                    it.next()
                        .ok_or("--chunk-max needs a value")?
                        .parse()
                        .map_err(|_| "--chunk-max must be a number")?,
                )
            }
            "--show" => {
                let raw = it.next().ok_or("--show needs a value")?;
                show.extend(raw.split(',').map(|s| s.trim().to_string()));
            }
            "--verbose" => verbose = true,
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(Args {
        corpus,
        questions,
        url,
        token,
        k,
        verbose,
        chunk_max,
        show,
    })
}

/// Chunk a corpus file exactly the way ingestion does.
fn chunk_file(path: &str, cfg: &ChunkConfig) -> Result<Vec<(i64, String)>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    let format = if path.ends_with(".md") {
        Format::Markdown
    } else if path.ends_with(".html") {
        Format::Html
    } else {
        Format::Text
    };
    let blocks = extract(format, &bytes, path).map_err(|e| format!("cannot extract {path}: {e}"))?;
    let chunks = chunk_blocks(&blocks, cfg);
    Ok(chunks
        .into_iter()
        .enumerate()
        .map(|(i, c)| (i as i64, indexed_text(&c.breadcrumb(), &c.body)))
        .collect())
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("orion_eval_real: {e}");
            return ExitCode::from(2);
        }
    };

    let suite_text = match std::fs::read_to_string(&args.questions) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("orion_eval_real: cannot read {}: {e}", args.questions);
            return ExitCode::FAILURE;
        }
    };
    let suite = match parse_suite(&suite_text) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("orion_eval_real: {e}");
            return ExitCode::FAILURE;
        }
    };

    // --- corpus ----------------------------------------------------------
    // `--corpus` overrides the list in the suite file; the file's list is
    // the default so that a suite is self-contained.
    let corpus_paths: Vec<String> = if args.corpus.is_empty() {
        suite.corpus.clone()
    } else {
        args.corpus.clone()
    };
    if corpus_paths.is_empty() {
        eprintln!("orion_eval_real: no corpus given (--corpus, or a [corpus] section in the suite file)");
        return ExitCode::from(2);
    }
    // The chunk size is the one retrieval knob that is cheap to change and
    // expensive to get wrong: it decides how much context a hit carries.
    // It is also bounded by the embedding model's token window.
    let base_cfg = ChunkConfig::default();
    let chunk_cfg = match args.chunk_max {
        Some(max) => ChunkConfig {
            target: max * 4 / 5,
            max,
            ..base_cfg
        },
        None => base_cfg,
    };
    let mut docs: Vec<(i64, String)> = Vec::new();
    // Which file each chunk came from, so a question can be scoped to one
    // document.
    let mut doc_source: Vec<usize> = Vec::new();
    for (file_index, path) in corpus_paths.iter().enumerate() {
        match chunk_file(path, &chunk_cfg) {
            Ok(mut part) => {
                let base = docs.len();
                for (id, _text) in part.iter_mut() {
                    *id += base as i64;
                }
                docs.extend(part);
                doc_source.extend(std::iter::repeat(file_index).take(docs.len() - base));
            }
            Err(e) => {
                eprintln!("orion_eval_real: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    if docs.is_empty() {
        eprintln!("orion_eval_real: the corpus is empty");
        return ExitCode::FAILURE;
    }

    // --- resolve anchors to chunk ids ------------------------------------
    let mut questions: Vec<EvalQuestion> = Vec::with_capacity(suite.questions.len());
    let mut unresolved = 0usize;
    let mut ambiguous = 0usize;
    for rq in &suite.questions {
        let mut relevant: Vec<i64> = Vec::new();
        if !rq.unanswerable {
            // Every chunk containing *all* the anchors is an acceptable
            // answer: where the same fact appears twice in the corpus, a
            // retriever that finds either copy has found the answer.
            for (index, (id, text)) in docs.iter().enumerate() {
                if let Some(src) = &rq.source {
                    let path = &corpus_paths[doc_source[index]];
                    if !path.contains(src.as_str()) {
                        continue;
                    }
                }
                if rq.anchors.iter().all(|a| text.contains(a)) {
                    relevant.push(*id);
                }
            }
            if relevant.is_empty() {
                unresolved += 1;
                eprintln!(
                    "orion_eval_real: question {} has no chunk matching its anchors {:?}",
                    rq.id, rq.anchors
                );
            } else if relevant.len() > 1 {
                ambiguous += 1;
                eprintln!(
                    "orion_eval_real: question {} matches {} chunks; all are accepted",
                    rq.id,
                    relevant.len()
                );
            }
        }
        questions.push(EvalQuestion {
            id: rq.id,
            question: Box::leak(rq.q.clone().into_boxed_str()),
            relevant: Box::leak(relevant.into_boxed_slice()),
            probe: rq.probe,
        });
    }
    if unresolved > 0 {
        eprintln!(
            "orion_eval_real: {unresolved} question(s) could not be grounded in the corpus; \
             refusing to score a suite whose ground truth is broken"
        );
        return ExitCode::FAILURE;
    }
    if args.verbose {
        eprintln!("orion_eval_real: {ambiguous} question(s) matched more than one chunk");
    }

    // --- embed -----------------------------------------------------------
    let (host, port) = match args.url.strip_prefix("http://").and_then(|r| r.split_once(':')) {
        Some((h, p)) => (h.to_string(), p.parse::<u16>().unwrap_or(0)),
        None => {
            eprintln!("orion_eval_real: --url must be http://host:port");
            return ExitCode::from(2);
        }
    };
    let embedder = Embedder::new(EmbedConfig {
        host,
        port,
        auth_token: args.token,
    });
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("orion_eval_real: cannot start tokio runtime: {e}");
            return ExitCode::FAILURE;
        }
    };

    let corpus_texts: Vec<String> = docs.iter().map(|(_, t)| t.clone()).collect();
    let vectors = match rt.block_on(embedder.embed(&corpus_texts, EmbedKind::Passage)) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("orion_eval_real: cannot reach the embedding server: {e}");
            return ExitCode::FAILURE;
        }
    };
    if vectors.len() != docs.len() {
        eprintln!("orion_eval_real: embedding count mismatch");
        return ExitCode::FAILURE;
    }
    if let Some(v) = vectors.first() {
        if v.len() != EMBED_DIM {
            eprintln!("orion_eval_real: expected {EMBED_DIM}-dim embeddings");
            return ExitCode::FAILURE;
        }
    }

    let query_texts: Vec<String> = questions
        .iter()
        // The embedder applies the BGE query prefix itself; doing it here
        // as well would send the instruction twice.
        .map(|q| q.question.to_string())
        .collect();
    let query_vectors = match rt.block_on(embedder.embed(&query_texts, EmbedKind::Query)) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("orion_eval_real: embedding the questions failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut qmap: HashMap<String, Vec<f32>> = HashMap::new();
    for (q, v) in questions.iter().zip(query_vectors.iter()) {
        qmap.insert(q.question.to_string(), v.clone());
    }

    let doc_vectors: Vec<(i64, Vec<f32>)> =
        docs.iter().map(|(id, _)| *id).zip(vectors.into_iter()).collect();
    // Kept for the inspection dump: the harness takes ownership of the
    // corpus, but a failed question is only diagnosable by reading the
    // chunk that should have matched.
    let doc_texts = docs.clone();
    let harness = orion_lib::rag::eval::Harness::from_vectors(docs, doc_vectors, qmap, true);

    // --- score -----------------------------------------------------------
    let k = args.k;
    let variants: Vec<(&str, EvalReport)> = vec![
        ("bm25", run_questions(&questions, k, |q, kk| harness.bm25_only(q, kk))),
        ("dense", run_questions(&questions, k, |q, kk| harness.vector_only(q, kk))),
        (
            "PRODUCTION dense-first",
            run_questions(&questions, k, |q, kk| harness.dense_mixed(q, kk, 1)),
        ),
        (
            "rrf (previous default 0.8/1.0)",
            run_questions(&questions, k, |q, kk| harness.hybrid(q, kk)),
        ),
    ];

    let scored = variants[0].1.scored;
    println!("suite:          {}", suite.name);
    println!("model:          {EMBED_MODEL_ID} ({EMBED_DIM} dims, live server)");
    {
        let total: usize = corpus_texts.iter().map(|t| t.chars().count()).sum();
        let longest = corpus_texts.iter().map(|t| t.chars().count()).max().unwrap_or(0);
        let mean = if corpus_texts.is_empty() {
            0
        } else {
            total / corpus_texts.len()
        };
        println!(
            "corpus text:    {total} chars, mean {mean} chars/chunk, longest {longest}"
        );
    }
    println!(
        "corpus:         {} chunks from {} files",
        corpus_texts.len(),
        corpus_paths.len()
    );
    println!("questions:      {} ({} answerable)", variants[0].1.total, scored);
    println!("k:              {k}");
    println!();

    for (name, report) in &variants {
        let successes = report.outcomes.iter().filter(|o| o.recall > 0.5).count() as u32;
        let (lo, hi) = wilson_interval(successes, report.scored as u32, Z_95);
        println!(
            "{name:<36} recall {:.3} [{lo:.3}, {hi:.3}]   MRR {:.3}   precision {:.3}",
            report.recall_at_k, report.mrr, report.precision_at_k
        );
    }
    println!();

    println!("=== recall by probe ===");
    let probes = [
        Probe::Paraphrase,
        Probe::Exact,
        Probe::HeadingScoped,
        Probe::Disambiguation,
    ];
    println!("{:<36}", "probe");
    for (name, _) in &variants {
        print!("{name:<36}");
    }
    println!();
    for p in probes {
        print!("{:<36}", format!("{p:?}"));
        for (_, r) in &variants {
            let s: Vec<_> = r.outcomes.iter().filter(|o| o.probe == p).collect();
            if s.is_empty() {
                print!("{:<36}", "n/a");
                continue;
            }
            let v: f32 = s.iter().map(|o| o.recall).sum::<f32>() / s.len() as f32;
            print!("{v:<36.3}");
        }
        println!();
    }
    println!();

    println!("=== paired comparisons (vs bm25) ===");
    for (name, cand) in &variants {
        if *name == "bm25" {
            continue;
        }
        for m in [Metric::Recall, Metric::Mrr, Metric::Precision] {
            let c = compare_paired(&variants[0].1, cand, m);
            println!("{}", c.render("bm25", name));
        }
        println!();
    }

    println!("=== paired comparisons (vs production) ===");
    let production = &variants[2].1;
    for (name, cand) in &variants {
        if *name == "bm25" || name.starts_with("PRODUCTION") {
            continue;
        }
        for m in [Metric::Recall, Metric::Mrr, Metric::Precision] {
            let c = compare_paired(production, cand, m);
            println!("{}", c.render("production", name));
        }
        println!();
    }

    if args.verbose {
        for (name, r) in &variants {
            println!("=== per-question outcomes: {name} ===");
            print!("{}", r.render_outcomes());
        }
    }

    // --- inspection -------------------------------------------------------
    // "Why did this question fail?" is a question about the *text*, not
    // about the score, so the chunk text is printed rather than its id.
    if !args.show.is_empty() {
        println!();
        println!("=== inspection ===");
        for q in &questions {
            let id = q.id.to_string();
            if !args.show.iter().any(|s| s == &id) {
                continue;
            }
            println!();
            println!("Q{} [{}] {}", q.id, format!("{:?}", q.probe), q.question);
            println!("  relevant chunks ({}):", q.relevant.len());
            for r in q.relevant {
                let text = doc_texts
                    .iter()
                    .find(|(cid, _)| cid == r)
                    .map(|(_, t)| t.as_str())
                    .unwrap_or("<missing>");
                println!("    [{r}] {}", first_lines(text, 8));
            }
            for (name, r) in &variants {
                let outcome = r.outcomes.iter().find(|o| o.id == q.id);
                let Some(o) = outcome else { continue };
                println!("  {name}:");
                for (rank, cid) in o.retrieved.iter().enumerate() {
                    let text = doc_texts
                        .iter()
                        .find(|(did, _)| did == cid)
                        .map(|(_, t)| t.as_str())
                        .unwrap_or("<missing>");
                    let mark = if q.relevant.contains(cid) { "HIT " } else { "    " };
                    println!("    {mark}#{rank} [{cid}] {}", first_lines(text, 8));
                }
            }
        }
    }

    ExitCode::SUCCESS
}

/// The first `n` lines of a chunk, collapsed to one line and clipped, so an
/// inspection dump stays readable.
fn first_lines(text: &str, n: usize) -> String {
    let joined: Vec<&str> = text.lines().take(n).collect();
    let s = joined.join(" / ");
    if s.chars().count() > 400 {
        let cut: String = s.chars().take(400).collect();
        format!("{cut}…")
    } else {
        s
    }
}
