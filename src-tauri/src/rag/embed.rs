//! Text embeddings via a dedicated local `llama-server` instance.
//!
//! ## Why a second server process
//!
//! `llama-server` can serve embeddings, but only when started with
//! `--embedding`, and a server in that mode will not do chat generation.
//! Sharing one process would mean serialising every embed behind every token
//! of a chat reply. Two small processes is simpler and faster; the embedding
//! model is ~90 MB, so the memory cost is negligible even at Tier T1.
//!
//! ## Model choice
//!
//! `bge-small-en-v1.5`, 384 dimensions, Q8_0 GGUF, MIT licence. Reasons:
//! * 384 dims keeps the vector table small — 100k chunks is ~150 MB, not 600 MB
//!   as it would be with a 1536-dim model.
//! * It runs acceptably on CPU, which matters because Tier T0/T1 machines have
//!   no usable GPU.
//! * MIT means we can bundle the weights; under the M1 licence gate, non-
//!   redistributable weights may never ship inside the installer.
//!
//! BGE models expect an asymmetric prefix: queries get an instruction prefix,
//! passages do not. Getting this backwards silently costs several points of
//! recall, which is exactly the kind of bug that is invisible without an eval
//! harness — hence `EmbedKind`.

use serde::{Deserialize, Serialize};

use crate::error::{OrionError, Result};

/// Dimensionality of the embedding model. Stored alongside vectors so a model
/// change is detected rather than silently producing garbage similarities.
pub const EMBED_DIM: usize = 384;

/// Identifier persisted with every vector. If this does not match the running
/// model, the index must be rebuilt.
pub const EMBED_MODEL_ID: &str = "bge-small-en-v1.5-q8_0";

/// Instruction prefix BGE expects on the *query* side only.
const QUERY_PREFIX: &str = "Represent this sentence for searching relevant passages: ";

/// The embedding model's context window, in tokens.
///
/// `bge-small-en-v1.5` is a BERT model with 512 learned position
/// embeddings. A longer sequence cannot be encoded at all: `llama-server`
/// rejects it with HTTP 500 rather than truncating it. The window is a hard
/// limit, not a guideline.
pub const EMBED_WINDOW_TOKENS: usize = 512;

/// Tokens held back for what the model adds itself: `[CLS]`, `[SEP]`, and
/// the BGE query instruction prefix.
pub const EMBED_RESERVED_TOKENS: usize = 16;

/// Content tokens one text may occupy.
pub const EMBED_MAX_TOKENS: usize = EMBED_WINDOW_TOKENS - EMBED_RESERVED_TOKENS;

/// The densest text measured in Orion's own corpus, in characters per token,
/// times ten so the arithmetic stays in integers.
///
/// Measured against the real model with `scripts/measure-embed-tokens.py`:
/// 1200-character pieces cost 53-446 tokens, and the densest 200-character
/// region -- a numeric table, which WordPiece splits into roughly one token
/// per character -- cost 1.63 characters per token. Ordinary prose costs
/// about 3.5. A character budget is only as trustworthy as this number, so
/// it is taken from the measurement rather than from an intuition about
/// how long English is.
pub const DENSEST_CHARS_PER_TOKEN_X10: usize = 15; // 1.5

/// Maximum characters sent in one embedding request.
///
/// 744 characters is 496 tokens even for the densest text measured, and
/// about 210 tokens for prose. The old budget of 1800 characters was
/// described as "well below" the model's window; against the measurement
/// above it was not, and one dense chunk in a document was enough to make
/// the server reject the whole batch.
pub const MAX_EMBED_CHARS: usize = EMBED_MAX_TOKENS * DENSEST_CHARS_PER_TOKEN_X10 / 10;

/// The shortest text the shrink-and-retry path will try. Below this a text
/// is more likely to be meaningless than to be too long.
pub const MIN_EMBED_CHARS: usize = 64;

/// How many texts to send per HTTP request. Larger batches are faster but a
/// failure costs more work, and the request body must stay modest on 8 GB.
pub const BATCH_SIZE: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbedKind {
    /// A user's question.
    Query,
    /// A document chunk being indexed.
    Passage,
}

impl EmbedKind {
    pub fn apply_prefix(&self, text: &str) -> String {
        match self {
            EmbedKind::Query => format!("{QUERY_PREFIX}{text}"),
            EmbedKind::Passage => text.to_string(),
        }
    }
}

#[derive(Debug, Serialize)]
struct EmbedRequest {
    input: Vec<String>,
    model: String,
}

#[derive(Debug, Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedDatum>,
}

#[derive(Debug, Deserialize)]
struct EmbedDatum {
    embedding: Vec<f32>,
    #[serde(default)]
    index: usize,
}

/// Connection details for the embedding sidecar. Mirrors `EngineConfig`:
/// loopback only, ephemeral port, per-session bearer token. The webview never
/// sees any of this.
#[derive(Debug, Clone)]
pub struct EmbedConfig {
    pub host: String,
    pub port: u16,
    pub auth_token: String,
}

impl EmbedConfig {
    pub fn base_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
}

/// Why a batch failed.
///
/// Only a *rejection* is worth retrying with a shorter text. A transport
/// failure or a malformed response will fail again however short the text
/// is, and pretending otherwise would turn one dead server into hundreds
/// of doomed requests.
#[derive(Debug)]
enum BatchError {
    /// The server refused the input — for an embedding server, that means
    /// the text exceeded the model's token window.
    Rejected,
    /// Anything else.
    Fatal(OrionError),
}

impl From<OrionError> for BatchError {
    fn from(e: OrionError) -> Self {
        BatchError::Fatal(e)
    }
}

pub struct Embedder {
    config: EmbedConfig,
    http: reqwest::Client,
}

impl Embedder {
    pub fn new(config: EmbedConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("failed to build http client"),
        }
    }

    /// Embed a batch of texts, preserving input order.
    ///
    /// The server may return results out of order, so we sort by the `index`
    /// field rather than trusting array position — mismatching a vector to
    /// the wrong chunk would corrupt the index in a way no test would catch
    /// except an end-to-end eval.
    ///
    /// A batch the server refuses is retried one text at a time. One chunk
    /// that is too long for the model's window must not cost the document
    /// its semantic index: the caller degrades a whole document to
    /// keyword-only search when embedding fails, and that is a far worse
    /// outcome than one chunk being embedded from its first few hundred
    /// characters.
    pub async fn embed(&self, texts: &[String], kind: EmbedKind) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let mut out: Vec<Vec<f32>> = Vec::with_capacity(texts.len());

        for batch in texts.chunks(BATCH_SIZE) {
            match self.embed_batch(batch, kind).await {
                Ok(mut v) => out.append(&mut v),
                Err(BatchError::Rejected) => {
                    for text in batch {
                        out.push(self.embed_one(text, kind).await?);
                    }
                }
                Err(BatchError::Fatal(e)) => return Err(e),
            }
        }

        Ok(out)
    }

    /// Embed one text, splitting it when the model cannot take it whole.
    ///
    /// The server is the oracle: it knows the model's real token window and
    /// refuses anything longer rather than truncating it. When that happens
    /// the text is split and the pieces are pooled, so no part of the chunk
    /// is silently dropped. A truncated embedding would make the tail of a
    /// chunk invisible to semantic search while still looking like a
    /// successful index.
    fn embed_one<'a>(
        &'a self,
        text: &'a str,
        kind: EmbedKind,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<f32>>> + Send + 'a>> {
        Box::pin(async move {
        if text.chars().count() > MAX_EMBED_CHARS {
            let pieces = split_for_embedding(text, MAX_EMBED_CHARS);
            let mut acc = vec![0.0f32; EMBED_DIM];
            for piece in &pieces {
                let v = self.embed_one(piece, kind).await?;
                for (a, b) in acc.iter_mut().zip(v.iter()) {
                    *a += b;
                }
            }
            return Ok(normalize(acc));
        }

        match self.embed_batch(std::slice::from_ref(&text.to_string()), kind).await {
            Ok(mut v) => v.pop().ok_or_else(|| {
                OrionError::Engine("embedding server returned nothing".into())
            }),
            Err(BatchError::Rejected) => {
                if text.chars().count() <= MIN_EMBED_CHARS {
                    return Err(OrionError::Engine(
                        "embedding server rejected the text even at the minimum length".into(),
                    ));
                }
                let (head, tail) = split_in_half(text);
                let a = self.embed_one(head, kind).await?;
                let b = self.embed_one(tail, kind).await?;
                Ok(pool(&a, &b))
            }
            Err(BatchError::Fatal(e)) => Err(e),
        }
        })
    }

    /// One HTTP round trip.
    async fn embed_batch(
        &self,
        texts: &[String],
        kind: EmbedKind,
    ) -> std::result::Result<Vec<Vec<f32>>, BatchError> {
        // A text over the character budget cannot go in a plain request, so
        // it takes the split-and-pool path. Doing it here rather than in
        // the caller keeps the returned vectors in the same order as the
        // input, which the store relies on when it zips embeddings to
        // chunks.
        if texts.iter().any(|t| t.chars().count() > MAX_EMBED_CHARS) {
            let mut out = Vec::with_capacity(texts.len());
            for t in texts {
                out.push(self.embed_one(t, kind).await?);
            }
            return Ok(out);
        }

        let input: Vec<String> = texts.iter().map(|t| kind.apply_prefix(t)).collect();

        let resp = self
            .http
            .post(format!("{}/v1/embeddings", self.config.base_url()))
            .bearer_auth(&self.config.auth_token)
            .json(&EmbedRequest {
                input,
                model: EMBED_MODEL_ID.to_string(),
            })
            .send()
            .await
            .map_err(|e| {
                BatchError::Fatal(OrionError::Engine(format!(
                    "embedding request failed: {e}"
                )))
            })?;

        // 5xx is the server saying "I will not encode this": for an
        // embedding server that means the text exceeded the model's window.
        // Retrying it shorter is worth trying. Any other failure is fatal.
        if resp.status().is_server_error() {
            return Err(BatchError::Rejected);
        }
        if !resp.status().is_success() {
            return Err(BatchError::Fatal(OrionError::Engine(format!(
                "embedding server returned {}",
                resp.status()
            ))));
        }

        let parsed: EmbedResponse = resp.json().await.map_err(|e| {
            BatchError::Fatal(OrionError::Engine(format!("bad embedding response: {e}")))
        })?;

        let mut data = parsed.data;
        if data.len() != texts.len() {
            return Err(BatchError::Fatal(OrionError::Engine(format!(
                "embedding count mismatch: sent {}, got {}",
                texts.len(),
                data.len()
            ))));
        }
        data.sort_by_key(|d| d.index);

        let mut out = Vec::with_capacity(data.len());
        for d in data {
            if d.embedding.len() != EMBED_DIM {
                return Err(BatchError::Fatal(OrionError::Engine(format!(
                    "expected {}-dim embeddings, got {}",
                    EMBED_DIM,
                    d.embedding.len()
                ))));
            }
            out.push(normalize(d.embedding));
        }
        Ok(out)
    }

    pub async fn embed_query(&self, text: &str) -> Result<Vec<f32>> {
        let mut v = self.embed(&[text.to_string()], EmbedKind::Query).await?;
        v.pop()
            .ok_or_else(|| OrionError::Engine("embedding server returned nothing".into()))
    }
}

/// L2-normalise so cosine similarity reduces to a dot product. Doing it once
/// at write time saves the square roots on every query.
pub fn normalize(mut v: Vec<f32>) -> Vec<f32> {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
    v
}

/// Truncate on a character boundary. `&str[..n]` panics mid-codepoint, and a
/// document containing an emoji must not crash ingestion.
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect()
}

/// Split text into pieces of at most `budget` characters.
///
/// Line boundaries are preferred, then word boundaries. A piece that is
/// still too long is cut hard, because the alternative -- dropping the
/// tail -- loses content the chunk was indexed for.
fn split_for_embedding(s: &str, budget: usize) -> Vec<String> {
    let budget = budget.max(1);
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();

    for line in s.lines() {
        if line.chars().count() > budget {
            if !cur.trim().is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            let mut rest = line;
            while rest.chars().count() > budget {
                let cut = rest
                    .char_indices()
                    .nth(budget)
                    .map(|(i, _)| i)
                    .unwrap_or(rest.len());
                let (head, tail) = rest.split_at(cut);
                out.push(head.trim_end().to_string());
                rest = tail.trim_start();
            }
            if !rest.trim().is_empty() {
                cur = rest.to_string();
            }
            continue;
        }
        if cur.chars().count() + line.chars().count() + 1 > budget && !cur.trim().is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push('\n');
        }
        cur.push_str(line);
    }

    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// Split text in two, preferring a whitespace boundary.
fn split_in_half(s: &str) -> (&str, &str) {
    let mut mid = s.len() / 2;
    while mid < s.len() && !s.is_char_boundary(mid) {
        mid += 1;
    }
    let mut cut = mid;
    while cut > 0 {
        if s[..cut].ends_with(char::is_whitespace) {
            break;
        }
        cut -= 1;
    }
    if cut == 0 {
        cut = mid;
    }
    (&s[..cut], &s[cut..])
}

/// Average two unit vectors and renormalise.
///
/// Pooling is what makes split-and-pool lossless: every part of the text
/// contributes to the vector, rather than the tail being dropped.
pub fn pool(a: &[f32], b: &[f32]) -> Vec<f32> {
    let v: Vec<f32> = a.iter().zip(b.iter()).map(|(x, y)| (x + y) * 0.5).collect();
    normalize(v)
}

/// Truncate to at most `max` characters, preferring to cut on a word
/// boundary.
///
/// A cut mid-word leaves the last token half-formed, and the embedding of
/// a half word is noise. When there is no whitespace to cut on — a run of
/// digits, say — this falls back to a plain character cut.
pub fn truncate_at_word(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    match cut.rfind(|c: char| c.is_whitespace()) {
        // Keep at least three quarters of the budget so a cut never
        // discards most of the text.
        Some(i) if i > max * 3 / 4 => cut[..i].trim_end().to_string(),
        _ => cut,
    }
}

/// Pack a vector for BLOB storage: little-endian f32, matching sqlite-vec's
/// expected layout.
pub fn to_blob(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

/// Unpack a BLOB back into a vector. Returns an error rather than panicking
/// on a truncated or corrupt row.
pub fn from_blob(b: &[u8]) -> Result<Vec<f32>> {
    // `% 4`, not `is_multiple_of`: the latter needs Rust 1.87, above this
    // crate's declared MSRV of 1.77.
    if b.len() % 4 != 0 {
        return Err(OrionError::Db(format!(
            "vector blob length {} is not a multiple of 4",
            b.len()
        )));
    }
    Ok(b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_and_passage_are_prefixed_differently() {
        let q = EmbedKind::Query.apply_prefix("what is the notice period");
        let p = EmbedKind::Passage.apply_prefix("what is the notice period");
        assert!(q.starts_with(QUERY_PREFIX));
        assert_eq!(p, "what is the notice period", "passages take no prefix");
        assert_ne!(q, p);
    }

    #[test]
    fn normalize_gives_unit_length() {
        let v = normalize(vec![3.0, 4.0]);
        let len: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((len - 1.0).abs() < 1e-6);
        assert!((v[0] - 0.6).abs() < 1e-6);
    }

    #[test]
    fn normalize_leaves_a_zero_vector_alone() {
        assert_eq!(normalize(vec![0.0, 0.0]), vec![0.0, 0.0]);
    }

    #[test]
    fn normalized_vectors_make_cosine_a_dot_product() {
        let a = normalize(vec![1.0, 2.0, 3.0]);
        let b = normalize(vec![2.0, 1.0, 0.5]);
        let dot: f32 = a.iter().zip(&b).map(|(x, y)| x * y).sum();
        let cos = crate::rag::search::cosine(&a, &b);
        assert!((dot - cos).abs() < 1e-6);
    }

    #[test]
    fn blob_roundtrips_exactly() {
        let v = normalize(vec![0.1, -0.2, 0.35, 0.0]);
        let back = from_blob(&to_blob(&v)).unwrap();
        assert_eq!(v.len(), back.len());
        for (a, b) in v.iter().zip(&back) {
            assert_eq!(a, b, "f32 roundtrip must be bit-exact, not approximate");
        }
    }

    #[test]
    fn blob_roundtrips_special_values() {
        let v = vec![f32::MIN, f32::MAX, 0.0, -0.0, f32::EPSILON];
        let back = from_blob(&to_blob(&v)).unwrap();
        assert_eq!(v, back);
    }

    #[test]
    fn corrupt_blob_errors_instead_of_panicking() {
        assert!(from_blob(&[1, 2, 3]).is_err());
        assert!(from_blob(&[1, 2, 3, 4, 5]).is_err());
        assert!(
            from_blob(&[]).is_ok(),
            "empty is degenerate but not corrupt"
        );
    }

    #[test]
    fn truncation_never_splits_a_codepoint() {
        // Each emoji is 4 bytes; a naive byte slice at 10 would panic.
        let s = "🙂🙂🙂🙂🙂";
        let t = truncate_chars(s, 3);
        assert_eq!(t.chars().count(), 3);
        assert!(s.starts_with(&t));
    }

    #[test]
    fn truncation_is_a_noop_for_short_text() {
        assert_eq!(truncate_chars("short", 100), "short");
    }

    #[test]
    fn embed_dim_matches_the_declared_model() {
        // Guard against someone swapping the model id without the dimension,
        // which would make every stored vector silently incomparable.
        assert_eq!(EMBED_DIM, 384);
        assert!(EMBED_MODEL_ID.contains("bge-small"));
    }

    #[test]
    fn max_embed_chars_fits_the_largest_chunk() {
        // ChunkConfig::default().max is MAX_EMBED_CHARS; if chunking grows
        // past what the embedder accepts, chunk tails would be silently
        // dropped.
        assert!(
            MAX_EMBED_CHARS >= crate::ingest::ChunkConfig::default().max,
            "embedder would silently truncate the largest chunk the chunker emits"
        );
    }

    #[test]
    fn the_character_budget_fits_the_model_window() {
        // The constant that matters. At the densest text measured, the
        // character budget must still fit inside the model's token window,
        // because the server refuses anything longer instead of truncating.
        let worst_tokens = MAX_EMBED_CHARS * 10 / DENSEST_CHARS_PER_TOKEN_X10;
        assert!(
            worst_tokens <= EMBED_MAX_TOKENS,
            "worst-case text costs {worst_tokens} tokens, window allows {EMBED_MAX_TOKENS}"
        );
        assert_eq!(EMBED_MAX_TOKENS + EMBED_RESERVED_TOKENS, EMBED_WINDOW_TOKENS);
    }

    #[test]
    fn the_chunk_budget_never_exceeds_the_embedder_budget() {
        let cfg = crate::ingest::ChunkConfig::default();
        assert!(
            cfg.max <= MAX_EMBED_CHARS,
            "the chunker may emit a chunk the embedder cannot encode"
        );
        assert!(cfg.target <= cfg.max);
    }

    #[test]
    fn truncate_at_word_cuts_on_a_boundary() {
        let s = "the quick brown fox jumps over the lazy dog and keeps running";
        let t = truncate_at_word(s, 20);
        assert!(s.starts_with(&t));
        assert!(t.chars().count() <= 20);
        assert_eq!(t, "the quick brown fox", "the cut should land on a word boundary");
        assert!(!t.ends_with(' '), "a word-boundary cut leaves no trailing space");
    }

    #[test]
    fn truncate_at_word_keeps_most_of_the_text() {
        // A cut must not discard most of the budget.
        let s = "alpha beta gamma delta epsilon zeta eta theta";
        let t = truncate_at_word(s, 30);
        assert!(t.chars().count() > 30 * 3 / 4);
    }

    #[test]
    fn truncate_at_word_falls_back_when_there_is_no_whitespace() {
        // A run of digits has no word boundary to cut on.
        let s = "1234567890123456789012345678901234567890";
        let t = truncate_at_word(s, 20);
        assert_eq!(t.chars().count(), 20);
    }

    #[test]
    fn splitting_for_embedding_loses_nothing() {
        let text = "first line\nsecond line\n\nthird line that is quite a lot longer than the others";
        let pieces = split_for_embedding(text, 20);
        assert!(pieces.len() > 1);
        let joined: String = pieces.join(" ");
        for word in ["first", "second", "third", "longer"] {
            assert!(joined.contains(word), "{word} was lost while splitting");
        }
    }

    #[test]
    fn splitting_for_embedding_keeps_every_piece_within_budget() {
        let text = "word ".repeat(200);
        for piece in split_for_embedding(&text, 64) {
            assert!(piece.chars().count() <= 64, "piece too long: {}", piece.len());
        }
    }

    #[test]
    fn splitting_in_half_covers_the_whole_text() {
        let text = "the quick brown fox jumps over the lazy dog";
        let (a, b) = split_in_half(text);
        assert_eq!(format!("{a}{b}"), text);
        assert!(!a.is_empty() && !b.is_empty());
    }

    #[test]
    fn pooling_averages_and_renormalises() {
        let a = normalize(vec![1.0, 0.0]);
        let b = normalize(vec![0.0, 1.0]);
        let p = pool(&a, &b);
        let len: f32 = p.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((len - 1.0).abs() < 1e-6, "pooled vector must be unit length");
        assert!((p[0] - p[1]).abs() < 1e-6, "the pooled vector is the mean");
    }

    #[test]
    fn truncate_at_word_is_a_noop_for_short_text() {
        assert_eq!(truncate_at_word("short", 100), "short");
    }
}
