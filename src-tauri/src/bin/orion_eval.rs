//! Live retrieval evaluation against a **real** embedding model.
//!
//! `cargo test --lib` proves the retrieval *plumbing* behaves. It cannot
//! prove retrieval *quality*, because the in-memory harness embeds with a
//! hashed bag-of-words projection: it matches vocabulary, never meaning.
//!
//! This binary closes that gap. It talks to a live `llama-server` running
//! `bge-small-en-v1.5-q8_0` — the same model Orion ships and the same client
//! (`rag::embed::Embedder`) the app uses — and scores the same 30-question
//! suite three ways:
//!
//! * BM25 only (keyword),
//! * dense only (semantic),
//! * hybrid (reciprocal-rank fusion of the two).
//!
//! It then reports Wilson confidence intervals for each configuration and a
//! **paired** comparison against BM25, because two averages are not evidence
//! that one retriever is better than another — the questions must be matched
//! one-for-one before any claim is made.
//!
//! ## Running it
//!
//! `scripts/run-eval.sh` starts the embedding server, runs this binary and
//! stops the server again. To run it by hand:
//!
//! ```text
//! llama-server -m ~/.local/share/orion/models/bge-small-en-v1.5-q8_0.gguf \
//!     --embedding --pooling cls --host 127.0.0.1 --port 8082 \
//!     --api-key <token> -c 512
//!
//! ORION_EVAL_URL=http://127.0.0.1:8082 \
//! ORION_EVAL_TOKEN=<token> \
//!   cargo run --bin orion_eval
//! ```
//!
//! The binary exits non-zero if the embedding server cannot be reached. It
//! never falls back to the pseudo-embedder: a silent fallback would produce
//! numbers that look real and are not.

use std::collections::HashMap;
use std::process::ExitCode;

use orion_lib::rag::embed::{EmbedConfig, Embedder, EmbedKind, EMBED_DIM, EMBED_MODEL_ID};
use orion_lib::rag::eval::{corpus_docs, questions, run_eval, EvalReport, Metric, Probe};
use orion_lib::rag::stats::{compare_paired, wilson_interval};

/// 95% two-sided normal quantile.
const Z_95: f64 = 1.959_963_984_540_054;

struct Args {
    url: String,
    token: String,
    k: usize,
    use_headings: bool,
    verbose: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut url = std::env::var("ORION_EVAL_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8082".to_string());
    let mut token =
        std::env::var("ORION_EVAL_TOKEN").unwrap_or_else(|_| "orion-eval-token".to_string());
    let mut k = 4usize;
    let mut use_headings = true;
    let mut verbose = false;

    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--url" => url = it.next().ok_or("--url needs a value")?,
            "--token" => token = it.next().ok_or("--token needs a value")?,
            "--k" => {
                k = it
                    .next()
                    .ok_or("--k needs a value")?
                    .parse()
                    .map_err(|_| "--k must be a number")?
            }
            "--no-headings" => use_headings = false,
            "--verbose" => verbose = true,
            "-h" | "--help" => {
                println!(
                    "usage: orion_eval [--url URL] [--token TOKEN] [--k N] \
                     [--no-headings] [--verbose]"
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok(Args {
        url,
        token,
        k,
        use_headings,
        verbose,
    })
}

fn split_url(url: &str) -> Result<(String, u16), String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or("only http:// URLs are supported")?;
    let (host, port) = rest
        .split_once(':')
        .ok_or("URL must be host:port, e.g. http://127.0.0.1:8082")?;
    let port = port
        .parse::<u16>()
        .map_err(|_| "port must be a number".to_string())?;
    Ok((host.to_string(), port))
}

/// Wilson interval for the "did it find any relevant chunk" question, which
/// is the recall metric expressed as a proportion over the answerable set.
fn recall_ci(report: &EvalReport) -> (f64, f64) {
    let successes = report.outcomes.iter().filter(|o| o.recall > 0.5).count() as u32;
    wilson_interval(successes, report.scored as u32, Z_95)
}

fn print_report(name: &str, report: &EvalReport) {
    println!("=== {name} ===");
    print!("{}", report.render());
    let (lo, hi) = recall_ci(report);
    println!(
        "recall Wilson 95% CI: [{lo:.3}, {hi:.3}]  (n = {} answerable)",
        report.scored
    );
    println!();
}

fn print_probe_table(reports: &[(&str, &EvalReport)]) {
    println!("=== recall by probe ===");
    let probes = [
        Probe::Paraphrase,
        Probe::Exact,
        Probe::HeadingScoped,
        Probe::Disambiguation,
    ];
    println!("{:<20}", "probe");
    for (name, _) in reports {
        print!("{:<12}", name);
    }
    println!();

    for p in probes {
        println!("{:<20}", format!("{p:?}"));
        for (_, r) in reports {
            // The unanswerable probe is excluded from this table on purpose:
            // it has no relevant chunk, so recall is undefined for it.
            let scored: Vec<_> = r.outcomes.iter().filter(|o| o.probe == p).collect();
            if scored.is_empty() {
                print!("{:<12}", "n/a");
                continue;
            }
            let v: f32 = scored.iter().map(|o| o.recall).sum::<f32>() / scored.len() as f32;
            print!("{v:<12.3}");
        }
        println!();
    }
    println!();
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("orion_eval: {e}");
            return ExitCode::from(2);
        }
    };

    let (host, port) = match split_url(&args.url) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("orion_eval: {e}");
            return ExitCode::from(2);
        }
    };

    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("orion_eval: cannot start tokio runtime: {e}");
            return ExitCode::FAILURE;
        }
    };

    let embedder = Embedder::new(EmbedConfig {
        host,
        port,
        auth_token: args.token,
    });

    // --- embed the corpus -----------------------------------------------
    let docs = corpus_docs(args.use_headings);
    let corpus_texts: Vec<String> = docs.iter().map(|(_, t)| t.clone()).collect();

    let vectors = match rt.block_on(embedder.embed(&corpus_texts, EmbedKind::Passage)) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "orion_eval: cannot reach the embedding server at {}: {e}\n\
                 Start it with scripts/run-eval.sh, or see the header of this file.",
                args.url
            );
            return ExitCode::FAILURE;
        }
    };

    if vectors.len() != docs.len() {
        eprintln!(
            "orion_eval: embedding count mismatch: {} docs, {} vectors",
            docs.len(),
            vectors.len()
        );
        return ExitCode::FAILURE;
    }
    if let Some(v) = vectors.first() {
        if v.len() != EMBED_DIM {
            eprintln!(
                "orion_eval: embedding server returned {}-dim vectors, expected {EMBED_DIM}",
                v.len()
            );
            return ExitCode::FAILURE;
        }
    }

    // --- embed the questions --------------------------------------------
    let qs = questions();
    let query_texts: Vec<String> = qs
        .iter()
        // The embedder applies the BGE query prefix itself; doing it here
        // as well would send the instruction twice.
        .map(|q| q.question.to_string())
        .collect();
    let query_vectors = match rt.block_on(embedder.embed(&query_texts, EmbedKind::Query)) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("orion_eval: embedding the questions failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut qmap: HashMap<String, Vec<f32>> = HashMap::new();
    for (q, v) in qs.iter().zip(query_vectors.iter()) {
        qmap.insert(q.question.to_string(), v.clone());
    }

    // --- build the harness and score ------------------------------------
    let doc_vectors: Vec<(i64, Vec<f32>)> =
        docs.iter().map(|(id, _)| *id).zip(vectors.into_iter()).collect();
    let harness = orion_lib::rag::eval::Harness::from_vectors(docs, doc_vectors, qmap, args.use_headings);

    let missing = harness.missing_query_vectors(qs);
    if !missing.is_empty() {
        eprintln!(
            "orion_eval: {} questions have no pre-computed query vector; refusing to \
             mix vector spaces",
            missing.len()
        );
        for q in missing {
            eprintln!("  - {q}");
        }
        return ExitCode::FAILURE;
    }

    let k = args.k;

    // Each variant is a candidate retrieval strategy. They are all scored
    // on the same 26 answerable questions so that any comparison between
    // them can be made pairwise.
    let variants: Vec<(&str, EvalReport)> = vec![
        ("bm25", run_eval(k, |q, kk| harness.bm25_only(q, kk))),
        ("dense", run_eval(k, |q, kk| harness.vector_only(q, kk))),
        (
            "PRODUCTION dense-3 + lexical-1",
            run_eval(k, |q, kk| harness.dense_mixed(q, kk, 1)),
        ),
        (
            "dense-first (bm25 fills only)",
            run_eval(k, |q, kk| harness.dense_first(q, kk)),
        ),
        (
            "dense-2 + lexical-2",
            run_eval(k, |q, kk| harness.dense_mixed(q, kk, 2)),
        ),
        (
            "rrf (previous default: bm25 0.8 / vector 1.0)",
            run_eval(k, |q, kk| harness.hybrid(q, kk)),
        ),
        (
            "rrf (vector-leaning: bm25 0.5 / vector 2.0)",
            run_eval(k, |q, kk| harness.rrf(q, kk, 0.5, 2.0)),
        ),
        (
            "rrf (dense-only weighting: bm25 0.1 / vector 1.0)",
            run_eval(k, |q, kk| harness.rrf(q, kk, 0.1, 1.0)),
        ),
    ];

    println!("model:          {EMBED_MODEL_ID} ({EMBED_DIM} dims, live server)");
    println!("corpus:         {} chunks", variants[0].1.scored);
    println!("k:              {k}");
    println!(
        "heading trail:  {}",
        if args.use_headings { "on" } else { "OFF (ablation)" }
    );
    println!();

    for (name, report) in &variants {
        print_report(name, report);
    }

    let table: Vec<(&str, &EvalReport)> = variants.iter().map(|(n, r)| (*n, r)).collect();
    print_probe_table(&table);

    // --- headline table ---------------------------------------------------
    println!("=== headline ===");
    println!(
        "{:<46} {:>8} {:>8} {:>10}  {}",
        "strategy", "recall", "MRR", "precision", "recall 95% CI"
    );
    for (name, r) in &variants {
        let (lo, hi) = recall_ci(r);
        println!(
            "{name:<46} {:>8.3} {:>8.3} {:>10.3}  [{lo:.3}, {hi:.3}]",
            r.recall_at_k, r.mrr, r.precision_at_k
        );
    }
    println!();

    // --- paired comparisons ---------------------------------------------
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

    // --- paired comparisons against the production configuration ---------
    let production = &variants[2].1;
    println!("=== paired comparisons (vs production dense-3 + lexical-1) ===");
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

    ExitCode::SUCCESS
}
