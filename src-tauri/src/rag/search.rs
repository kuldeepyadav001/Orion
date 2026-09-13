//! Hybrid retrieval: BM25 keyword search fused with dense vector search.
//!
//! Serina used dense vectors only. That fails exactly where users ask the
//! most precise questions — "what is the policy number", "who signed on
//! 14 March" — because embeddings capture topic, not exact tokens. Meanwhile
//! pure BM25 misses paraphrases entirely.
//!
//! Running both and fusing the rankings is the standard fix and costs very
//! little: SQLite gives us FTS5 for BM25 and sqlite-vec for vectors in the
//! same file, so there is no second service and no network hop.
//!
//! Fusion uses **Reciprocal Rank Fusion**, which combines rankings rather
//! than scores. That matters because BM25 scores and cosine similarities are
//! not on comparable scales, and normalising them is fragile.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A retrieved chunk with provenance for citation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hit {
    pub chunk_id: i64,
    pub document_id: String,
    pub document_name: String,
    pub text: String,
    pub page: Option<u32>,
    pub breadcrumb: String,
    /// Fused relevance. Only meaningful relative to other hits.
    pub score: f32,
    /// Which retriever(s) found this, for debugging and the eval harness.
    pub sources: Vec<String>,
}

/// A ranked candidate from one retriever.
#[derive(Debug, Clone, Copy)]
pub struct Ranked {
    pub chunk_id: i64,
    pub rank: usize,
}

/// RRF damping constant. 60 is the value from the original paper and is
/// what most production systems use; it stops rank-1 from dominating.
const RRF_K: f32 = 60.0;

/// Fuse several ranked lists into one.
///
/// `weights` biases a retriever without changing the fusion maths — useful
/// when one side is known to be stronger for a corpus.
pub fn reciprocal_rank_fusion(
    lists: &[(&'static str, Vec<Ranked>, f32)],
    limit: usize,
) -> Vec<(i64, f32, Vec<&'static str>)> {
    let mut scores: HashMap<i64, f32> = HashMap::new();
    let mut provenance: HashMap<i64, Vec<&'static str>> = HashMap::new();

    for (name, list, weight) in lists {
        for r in list {
            // rank is 0-based; +1 so the top result contributes 1/(k+1).
            let contribution = weight / (RRF_K + (r.rank as f32) + 1.0);
            *scores.entry(r.chunk_id).or_insert(0.0) += contribution;
            provenance.entry(r.chunk_id).or_default().push(*name);
        }
    }

    let mut fused: Vec<(i64, f32, Vec<&'static str>)> = scores
        .into_iter()
        .map(|(id, s)| {
            let mut src = provenance.remove(&id).unwrap_or_default();
            src.sort_unstable();
            src.dedup();
            (id, s, src)
        })
        .collect();

    // Sort by score desc, then id asc so results are deterministic.
    fused.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    fused.truncate(limit);
    fused
}

/// Cosine similarity between two equal-length vectors.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

/// Prepare user text for an FTS5 MATCH query.
///
/// **Security-relevant.** FTS5 has its own query syntax; passing raw user
/// input lets a question containing `"` or `NEAR` or `*` either error out or
/// change the query's meaning. We tokenise to alphanumerics and quote every
/// term, so the input is always data and never syntax.
pub fn sanitize_fts_query(q: &str) -> String {
    let terms: Vec<String> = q
        .split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '-')
        .map(|t| t.trim_matches(|c: char| c == '\'' || c == '-'))
        .filter(|t| t.len() > 1)
        .filter(|t| !is_stopword(t))
        .take(32) // bound the query size
        .map(|t| format!("\"{}\"", t.replace('"', "")))
        .collect();

    terms.join(" OR ")
}

/// A small stopword list. Kept short on purpose — aggressive removal hurts
/// phrase-like questions such as "who is the owner of record".
fn is_stopword(t: &str) -> bool {
    matches!(
        t.to_ascii_lowercase().as_str(),
        "the"
            | "a"
            | "an"
            | "of"
            | "to"
            | "in"
            | "is"
            | "it"
            | "and"
            | "or"
            | "for"
            | "on"
            | "at"
            | "by"
            | "be"
            | "as"
            | "that"
            | "this"
            | "with"
            | "from"
            | "was"
            | "are"
    )
}

/// Simple in-memory BM25, used by the eval harness and tests so retrieval
/// quality can be measured without standing up SQLite FTS5.
pub struct Bm25 {
    docs: Vec<(i64, Vec<String>)>,
    df: HashMap<String, usize>,
    avg_len: f32,
    k1: f32,
    b: f32,
}

impl Bm25 {
    pub fn new(docs: Vec<(i64, String)>) -> Self {
        let tokenised: Vec<(i64, Vec<String>)> =
            docs.into_iter().map(|(id, t)| (id, tokenize(&t))).collect();

        let mut df: HashMap<String, usize> = HashMap::new();
        for (_, toks) in &tokenised {
            let mut seen: Vec<&String> = toks.iter().collect();
            seen.sort();
            seen.dedup();
            for t in seen {
                *df.entry(t.clone()).or_insert(0) += 1;
            }
        }

        let avg_len = if tokenised.is_empty() {
            0.0
        } else {
            tokenised.iter().map(|(_, t)| t.len() as f32).sum::<f32>() / tokenised.len() as f32
        };

        Self {
            docs: tokenised,
            df,
            avg_len,
            k1: 1.5,
            b: 0.75,
        }
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<Ranked> {
        let q = tokenize(query);
        let n = self.docs.len() as f32;
        let mut scored: Vec<(i64, f32)> = Vec::new();

        for (id, toks) in &self.docs {
            let len = toks.len() as f32;
            let mut score = 0.0f32;

            for term in &q {
                let tf = toks.iter().filter(|t| *t == term).count() as f32;
                if tf == 0.0 {
                    continue;
                }
                let df = *self.df.get(term).unwrap_or(&0) as f32;
                // Robertson/Sparck-Jones IDF with the +1 smoothing that keeps
                // it non-negative for terms present in every document.
                let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
                let denom = tf + self.k1 * (1.0 - self.b + self.b * len / self.avg_len.max(1.0));
                score += idf * (tf * (self.k1 + 1.0)) / denom;
            }

            if score > 0.0 {
                scored.push((*id, score));
            }
        }

        scored.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.0.cmp(&b.0))
        });
        scored
            .into_iter()
            .take(limit)
            .enumerate()
            .map(|(rank, (chunk_id, _))| Ranked { chunk_id, rank })
            .collect()
    }
}

pub fn tokenize(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_ascii_lowercase())
        .collect()
}

/// Rank vectors by cosine similarity to a query vector.
pub fn vector_rank(query: &[f32], corpus: &[(i64, Vec<f32>)], limit: usize) -> Vec<Ranked> {
    let mut scored: Vec<(i64, f32)> = corpus
        .iter()
        .map(|(id, v)| (*id, cosine(query, v)))
        .filter(|(_, s)| *s > 0.0)
        .collect();

    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    scored
        .into_iter()
        .take(limit)
        .enumerate()
        .map(|(rank, (chunk_id, _))| Ranked { chunk_id, rank })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(ids: &[i64]) -> Vec<Ranked> {
        ids.iter()
            .enumerate()
            .map(|(rank, id)| Ranked {
                chunk_id: *id,
                rank,
            })
            .collect()
    }

    /* ---------- fusion ---------- */

    #[test]
    fn fusion_rewards_agreement_between_retrievers() {
        // 2 is mid-ranked in both lists; 1 and 9 are top of only one.
        let fused = reciprocal_rank_fusion(
            &[("bm25", r(&[1, 2, 3]), 1.0), ("vec", r(&[9, 2, 8]), 1.0)],
            3,
        );
        assert_eq!(fused[0].0, 2, "a chunk found by both should win: {fused:?}");
        assert_eq!(fused[0].2, vec!["bm25".to_string(), "vec".to_string()]);
    }

    #[test]
    fn fusion_is_deterministic_on_ties() {
        let a = reciprocal_rank_fusion(&[("bm25", r(&[5, 3, 1]), 1.0)], 3);
        let b = reciprocal_rank_fusion(&[("bm25", r(&[5, 3, 1]), 1.0)], 3);
        assert_eq!(
            a.iter().map(|x| x.0).collect::<Vec<_>>(),
            b.iter().map(|x| x.0).collect::<Vec<_>>()
        );
    }

    #[test]
    fn fusion_respects_weights() {
        let low = reciprocal_rank_fusion(&[("bm25", r(&[1]), 0.1), ("vec", r(&[2]), 1.0)], 2);
        assert_eq!(low[0].0, 2, "the heavier retriever should lead");
    }

    #[test]
    fn fusion_handles_empty_and_single_lists() {
        assert!(reciprocal_rank_fusion(&[], 5).is_empty());
        assert!(reciprocal_rank_fusion(&[("bm25", vec![], 1.0)], 5).is_empty());
        assert_eq!(
            reciprocal_rank_fusion(&[("bm25", r(&[7]), 1.0)], 5).len(),
            1
        );
    }

    #[test]
    fn fusion_respects_the_limit() {
        let fused = reciprocal_rank_fusion(&[("bm25", r(&[1, 2, 3, 4, 5]), 1.0)], 2);
        assert_eq!(fused.len(), 2);
    }

    /* ---------- cosine ---------- */

    #[test]
    fn cosine_basics() {
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        assert!(
            (cosine(&[1.0, 1.0], &[2.0, 2.0]) - 1.0).abs() < 1e-6,
            "scale invariant"
        );
    }

    #[test]
    fn cosine_is_safe_on_degenerate_input() {
        assert_eq!(cosine(&[], &[]), 0.0);
        assert_eq!(cosine(&[1.0], &[1.0, 2.0]), 0.0, "length mismatch");
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 1.0]), 0.0, "zero vector");
    }

    /* ---------- FTS sanitisation (security) ---------- */

    #[test]
    fn fts_query_is_tokenised_and_quoted() {
        let q = sanitize_fts_query("termination notice period");
        assert!(q.contains("\"termination\""));
        assert!(q.contains(" OR "));
    }

    #[test]
    fn fts_query_neutralises_syntax_characters() {
        // These would otherwise be FTS5 operators or a syntax error.
        for evil in [
            "foo\" OR bar",
            "NEAR(a b)",
            "col:value",
            "wild*card",
            "a AND b NOT c",
            "((((",
            "\"\"\"\"",
        ] {
            let q = sanitize_fts_query(evil);
            // Every emitted term is individually quoted, so nothing can act
            // as an operator. Quotes only ever appear as delimiters.
            for tok in q.split(" OR ").filter(|s| !s.is_empty()) {
                assert!(
                    tok.starts_with('"') && tok.ends_with('"'),
                    "unquoted token {tok:?} from {evil:?}"
                );
                assert_eq!(
                    tok.matches('"').count(),
                    2,
                    "embedded quote survived in {tok:?} from {evil:?}"
                );
            }
        }
    }

    #[test]
    fn fts_query_drops_stopwords_and_single_chars() {
        let q = sanitize_fts_query("what is the a b c of it");
        assert!(!q.contains("\"the\""));
        assert!(!q.contains("\"a\""));
        assert!(!q.contains("\"b\""), "single characters are noise");
    }

    #[test]
    fn fts_query_on_empty_input_is_empty() {
        assert_eq!(sanitize_fts_query(""), "");
        assert_eq!(sanitize_fts_query("!!! ??? ..."), "");
    }

    #[test]
    fn fts_query_is_length_bounded() {
        let long = (0..500)
            .map(|i| format!("term{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let q = sanitize_fts_query(&long);
        assert!(q.split(" OR ").count() <= 32);
    }

    /* ---------- BM25 ---------- */

    fn corpus() -> Vec<(i64, String)> {
        vec![
            (
                1,
                "The termination clause requires thirty days written notice.".into(),
            ),
            (
                2,
                "Payment terms are net thirty from the invoice date.".into(),
            ),
            (
                3,
                "The policy number is AB-99312 and covers fire damage.".into(),
            ),
            (
                4,
                "Either party may cancel the agreement by giving notice.".into(),
            ),
        ]
    }

    #[test]
    fn bm25_finds_exact_tokens() {
        let idx = Bm25::new(corpus());
        let hits = idx.search("policy number AB-99312", 3);
        assert_eq!(hits[0].chunk_id, 3, "exact identifier must rank first");
    }

    #[test]
    fn bm25_ranks_by_relevance() {
        let idx = Bm25::new(corpus());
        let hits = idx.search("termination notice", 4);
        assert_eq!(hits[0].chunk_id, 1);
    }

    #[test]
    fn bm25_returns_nothing_for_absent_terms() {
        let idx = Bm25::new(corpus());
        assert!(idx.search("zebra helicopter", 5).is_empty());
    }

    #[test]
    fn bm25_handles_an_empty_corpus() {
        let idx = Bm25::new(vec![]);
        assert!(idx.search("anything", 5).is_empty());
    }

    #[test]
    fn bm25_ranks_are_sequential_from_zero() {
        let idx = Bm25::new(corpus());
        let hits = idx.search("notice", 5);
        for (i, h) in hits.iter().enumerate() {
            assert_eq!(h.rank, i);
        }
    }

    /* ---------- vector ranking ---------- */

    #[test]
    fn vector_rank_orders_by_similarity() {
        let corpus = vec![
            (1, vec![1.0, 0.0, 0.0]),
            (2, vec![0.9, 0.1, 0.0]),
            (3, vec![0.0, 1.0, 0.0]),
        ];
        let hits = vector_rank(&[1.0, 0.0, 0.0], &corpus, 3);
        assert_eq!(hits[0].chunk_id, 1);
        assert_eq!(hits[1].chunk_id, 2);
    }

    #[test]
    fn vector_rank_skips_orthogonal_entries() {
        let corpus = vec![(1, vec![1.0, 0.0]), (2, vec![0.0, 1.0])];
        let hits = vector_rank(&[1.0, 0.0], &corpus, 5);
        assert_eq!(hits.len(), 1, "zero-similarity results should be dropped");
    }

    /* ---------- the point of the whole module ---------- */

    #[test]
    fn hybrid_beats_vectors_alone_on_exact_identifiers() {
        // A dense retriever that understands topic but not identifiers: it
        // ranks the "contract-ish" chunks and never surfaces the policy number.
        let vec_hits = r(&[1, 4, 2]);
        assert!(
            !vec_hits.iter().any(|h| h.chunk_id == 3),
            "premise of the test: dense search misses chunk 3"
        );

        let bm = Bm25::new(corpus());
        let bm_hits = bm.search("policy number AB-99312", 5);
        assert_eq!(bm_hits[0].chunk_id, 3);

        let fused = reciprocal_rank_fusion(&[("bm25", bm_hits, 1.0), ("vec", vec_hits, 1.0)], 4);
        let ids: Vec<i64> = fused.iter().map(|f| f.0).collect();
        assert!(
            ids.contains(&3),
            "hybrid must surface the exact match dense search missed: {ids:?}"
        );
        // It should also rank at the very top, since nothing else was found
        // by two retrievers and it is rank 1 of its own list.
        assert!(
            ids.iter().position(|i| *i == 3).unwrap() <= 1,
            "the exact match should be at or near the top: {ids:?}"
        );
    }

    #[test]
    fn hybrid_keeps_semantic_hits_when_keywords_miss() {
        // Query paraphrases the text, so BM25 contributes little or nothing.
        let bm = Bm25::new(corpus());
        let bm_hits = bm.search("ending the contract early", 5);

        // Dense retrieval understands "cancel the agreement" (chunk 4).
        let vec_hits = r(&[4, 1]);

        let fused = reciprocal_rank_fusion(&[("bm25", bm_hits, 1.0), ("vec", vec_hits, 1.0)], 4);
        let ids: Vec<i64> = fused.iter().map(|f| f.0).collect();
        assert!(
            ids.contains(&4),
            "the semantic match must survive fusion: {ids:?}"
        );
    }

    #[test]
    fn a_chunk_found_by_both_retrievers_outranks_one_found_by_either() {
        // The central guarantee of RRF, stated directly.
        let fused =
            reciprocal_rank_fusion(&[("bm25", r(&[10, 7]), 1.0), ("vec", r(&[11, 7]), 1.0)], 3);
        assert_eq!(fused[0].0, 7);
        assert_eq!(fused[0].2.len(), 2);
    }
}
