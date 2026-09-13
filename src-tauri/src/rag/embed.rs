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

/// Maximum characters sent in one embedding request. The model's window is
/// 512 tokens; we cut well below it because truncation is silent server-side
/// and would quietly drop the end of a chunk.
pub const MAX_EMBED_CHARS: usize = 1800;

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
    pub async fn embed(&self, texts: &[String], kind: EmbedKind) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let mut out: Vec<Vec<f32>> = Vec::with_capacity(texts.len());

        for batch in texts.chunks(BATCH_SIZE) {
            let input: Vec<String> = batch
                .iter()
                .map(|t| kind.apply_prefix(&truncate_chars(t, MAX_EMBED_CHARS)))
                .collect();

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
                .map_err(|e| OrionError::Engine(format!("embedding request failed: {e}")))?;

            if !resp.status().is_success() {
                return Err(OrionError::Engine(format!(
                    "embedding server returned {}",
                    resp.status()
                )));
            }

            let parsed: EmbedResponse = resp
                .json()
                .await
                .map_err(|e| OrionError::Engine(format!("bad embedding response: {e}")))?;

            let mut data = parsed.data;
            if data.len() != batch.len() {
                return Err(OrionError::Engine(format!(
                    "embedding count mismatch: sent {}, got {}",
                    batch.len(),
                    data.len()
                )));
            }
            data.sort_by_key(|d| d.index);

            for d in data {
                if d.embedding.len() != EMBED_DIM {
                    return Err(OrionError::Engine(format!(
                        "expected {}-dim embeddings, got {}",
                        EMBED_DIM,
                        d.embedding.len()
                    )));
                }
                out.push(normalize(d.embedding));
            }
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
    if !b.len().is_multiple_of(4) {
        return Err(OrionError::Db(format!(
            "vector blob length {} is not a multiple of 4",
            b.len()
        )));
    }
    let (quads, _) = b.as_chunks::<4>();
    Ok(quads.iter().map(|c| f32::from_le_bytes(*c)).collect())
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
        // ChunkConfig::default().max is 1800; if chunking grows past what the
        // embedder accepts, chunk tails would be silently dropped.
        assert!(
            MAX_EMBED_CHARS >= crate::ingest::ChunkConfig::default().max,
            "embedder would silently truncate the largest chunk the chunker emits"
        );
    }
}
