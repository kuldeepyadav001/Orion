//! Wiring between the document engine (M2) and the running application.
//!
//! M2 built ingestion, chunking, hybrid retrieval and citation assembly, and
//! tested all of it to 220 tests — but never connected any of it to the app.
//! The library existed; nothing called it. This module is that connection.
//!
//! What lives here:
//!
//! * the **embedding sidecar**: a second `llama-server` process running
//!   bge-small, separate from the chat model
//! * **ingestion**: file path in, chunks embedded and stored
//! * **retrieval**: question in, grounded context with citations out
//!
//! ## Why a second process
//!
//! One `llama-server` serves one model. The chat model and the embedding
//! model are different models, so they need different processes. The
//! alternative — embedding with the chat model — produces vectors that are
//! not trained for retrieval and would quietly degrade search quality.
//!
//! The cost is real and worth stating: bge-small-en-v1.5 Q8_0 is ~130 MB of
//! weights, plus process overhead. On the 5.7 GiB machine this was developed
//! against, that is roughly 2% of RAM on top of the 2.4 GiB chat model. If
//! the embedder cannot start, ingestion and search still work — they fall
//! back to keyword-only retrieval, which the M2 eval measured at 0.731
//! recall@4 against 0.769 for hybrid.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;
use tokio::sync::Mutex;

use crate::db::Db;
use crate::engine::{free_port, random_token};
use crate::error::{OrionError, Result};
use crate::ingest::{self, chunker::ChunkConfig};
use crate::rag::context::{build_context, GroundedContext};
use crate::rag::embed::{EmbedConfig, EmbedKind, Embedder};
use crate::rag::store::RagStore;
use crate::sidecars::SidecarRegistry;

/// Model file for the embedder, looked up in the same models directory as
/// the chat model.
pub const EMBED_MODEL_FILE: &str = "bge-small-en-v1.5-q8_0.gguf";

/// Where to download it from, if missing.
pub const EMBED_MODEL_REPO: &str = "CompendiumLabs/bge-small-en-v1.5-gguf";

/// Vector width of bge-small. Stored alongside each chunk; a mismatch means
/// the index was built with a different model and must be rebuilt.
pub const EMBED_DIM: usize = 384;

/// How many chunks to retrieve per question before assembling context.
const SEARCH_LIMIT: usize = 8;

/// How many of those actually reach the prompt.
const CONTEXT_TOP_K: usize = 4;

// Asking for more context chunks than were retrieved would silently truncate,
// making the prompt smaller than intended. Checked at compile time.
const _: () = assert!(CONTEXT_TOP_K <= SEARCH_LIMIT);

/// State of the embedding sidecar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EmbedState {
    /// Not started yet.
    Idle,
    Starting,
    Ready,
    /// Unavailable. Search degrades to keyword-only rather than failing.
    Unavailable,
}

/// Runtime handle for the embedding sidecar.
pub struct EmbedService {
    state: Mutex<EmbedState>,
    detail: Mutex<String>,
    embedder: Mutex<Option<Arc<Embedder>>>,
}

impl Default for EmbedService {
    fn default() -> Self {
        Self::new()
    }
}

impl EmbedService {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(EmbedState::Idle),
            detail: Mutex::new(String::new()),
            embedder: Mutex::new(None),
        }
    }

    pub async fn state(&self) -> EmbedState {
        *self.state.lock().await
    }

    pub async fn detail(&self) -> String {
        self.detail.lock().await.clone()
    }

    async fn set(&self, state: EmbedState, detail: impl Into<String>) {
        *self.state.lock().await = state;
        *self.detail.lock().await = detail.into();
        tracing::info!(?state, "embedding service status changed");
    }

    /// The embedder, if it is ready.
    pub async fn get(&self) -> Option<Arc<Embedder>> {
        self.embedder.lock().await.clone()
    }

    /// Embed one query, or `None` when the embedder is unavailable.
    ///
    /// Returning `None` rather than an error is deliberate: a missing
    /// embedder should degrade search to keyword-only, not break it.
    pub async fn embed_query(&self, text: &str) -> Option<Vec<f32>> {
        let embedder = self.get().await?;
        match embedder.embed(&[text.to_string()], EmbedKind::Query).await {
            Ok(mut v) if !v.is_empty() => Some(v.remove(0)),
            Ok(_) => None,
            Err(e) => {
                tracing::warn!(error = %e, "query embedding failed; using keyword search only");
                None
            }
        }
    }

    /// Embed document chunks. Errors propagate here, because silently
    /// indexing a document with no vectors would make it permanently
    /// invisible to semantic search with no indication anything was wrong.
    pub async fn embed_passages(&self, texts: &[String]) -> Result<Option<Vec<Vec<f32>>>> {
        let Some(embedder) = self.get().await else {
            return Ok(None);
        };
        let vectors = embedder.embed(texts, EmbedKind::Passage).await?;
        Ok(Some(vectors))
    }
}

/// Locate the embedding model in the models directory.
pub fn find_embed_model() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("ORION_EMBED_MODEL_PATH") {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }

    let dir = crate::db::data_dir().ok()?.join("models");
    let exact = dir.join(EMBED_MODEL_FILE);
    if exact.is_file() {
        return Some(exact);
    }

    // Accept any bge-* GGUF, so a user who fetched a different quantisation
    // is not told the file is missing when it plainly is not.
    std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            let name = p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            name.starts_with("bge-") && name.ends_with(".gguf")
        })
}

/// Start the embedding sidecar.
///
/// Never fatal. A failure here leaves search working on keywords alone,
/// which is worse but far better than refusing to open documents at all.
pub async fn start_embedder(
    registry: Arc<SidecarRegistry>,
    app: tauri::AppHandle,
    service: Arc<EmbedService>,
) {
    let Some(model_path) = find_embed_model() else {
        service
            .set(
                EmbedState::Unavailable,
                format!(
                    "No embedding model found. Semantic search is off; keyword search still \
                     works. Download {EMBED_MODEL_FILE} to enable it."
                ),
            )
            .await;
        return;
    };

    let port = match free_port() {
        Ok(p) => p,
        Err(e) => {
            service
                .set(EmbedState::Unavailable, format!("no free port: {e}"))
                .await;
            return;
        }
    };

    let token = random_token();
    service
        .set(EmbedState::Starting, "Starting semantic search…")
        .await;

    let sidecar = match app.shell().sidecar("llama-server") {
        Ok(c) => c,
        Err(e) => {
            service
                .set(EmbedState::Unavailable, format!("sidecar missing: {e}"))
                .await;
            return;
        }
    };

    // `--embedding` puts llama-server in embedding mode, where /v1/embeddings
    // works and /v1/chat/completions does not. Two threads is plenty: chunks
    // are short and the chat model needs the remaining cores far more.
    let spawned = sidecar
        .args([
            "--model".into(),
            model_path.to_string_lossy().to_string(),
            "--embedding".into(),
            "--host".into(),
            "127.0.0.1".into(),
            "--port".into(),
            port.to_string(),
            "--api-key".into(),
            token.clone(),
            "--ctx-size".into(),
            "512".to_string(),
            "--threads".into(),
            "2".to_string(),
        ])
        .spawn();

    let (mut rx, child) = match spawned {
        Ok(v) => v,
        Err(e) => {
            service
                .set(EmbedState::Unavailable, format!("could not start: {e}"))
                .await;
            return;
        }
    };

    // Same reasoning as the chat sidecar: an unregistered child is never
    // killed and outlives the app.
    registry.register("llama-server (embeddings)", child);

    tauri::async_runtime::spawn(async move {
        while let Some(ev) = rx.recv().await {
            match ev {
                CommandEvent::Stderr(b) | CommandEvent::Stdout(b) => {
                    tracing::debug!(target: "embed", "{}", String::from_utf8_lossy(&b).trim())
                }
                CommandEvent::Terminated(p) => {
                    tracing::warn!(?p, "embedding sidecar exited");
                    break;
                }
                _ => {}
            }
        }
    });

    let config = EmbedConfig {
        host: "127.0.0.1".into(),
        port,
        auth_token: token,
    };
    let embedder = Arc::new(Embedder::new(config.clone()));

    // Poll until it answers. bge-small is ~130 MB so this is quick, but the
    // process still has to start and map the file.
    let http = reqwest::Client::new();
    let url = format!("{}/health", config.base_url());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);

    while std::time::Instant::now() < deadline {
        if let Ok(r) = http.get(&url).bearer_auth(&config.auth_token).send().await {
            if r.status().is_success() {
                *service.embedder.lock().await = Some(embedder);
                service
                    .set(EmbedState::Ready, "Semantic search ready")
                    .await;
                return;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    service
        .set(
            EmbedState::Unavailable,
            "Embedding model did not start in time; keyword search only.",
        )
        .await;
}

/// Summary of one ingested document, for the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestSummary {
    pub document_id: String,
    pub name: String,
    pub chunks: usize,
    pub pages: Option<u32>,
    /// False when the embedder was unavailable, so this document is
    /// searchable by keyword but not semantically.
    pub embedded: bool,
}

/// Ingest one file: extract, chunk, embed, store.
pub async fn ingest_file(
    path: &Path,
    db: &Arc<Mutex<Db>>,
    embed: &Arc<EmbedService>,
) -> Result<IngestSummary> {
    // Parsing is CPU-bound and synchronous. Run it off the async runtime so a
    // large PDF cannot stall the UI's event loop.
    let owned = path.to_path_buf();
    let doc = tauri::async_runtime::spawn_blocking(move || {
        ingest::ingest_path(&owned, &ChunkConfig::default())
    })
    .await
    .map_err(|e| OrionError::Config(format!("ingestion task failed: {e}")))??;

    // `Chunk::text` already carries the heading trail prefix, which the M2
    // eval showed is worth +0.22 recall. Embed that, not the bare body.
    let texts: Vec<String> = doc.chunks.iter().map(|c| c.text.clone()).collect();
    let vectors = embed.embed_passages(&texts).await?;
    let embedded = vectors.is_some();

    // The store requires one vector per chunk. When the embedder is
    // unavailable, store zero vectors of the right width so the document is
    // still keyword-searchable: a zero vector has no direction, so cosine
    // similarity scores it at zero and it simply never wins on the dense
    // side. The alternative — refusing to index — would lose the document
    // entirely over a missing optional component.
    let vectors = vectors.unwrap_or_else(|| vec![vec![0.0f32; EMBED_DIM]; texts.len()]);

    let name = doc.name.clone();
    let document_id = doc.id.clone();
    let chunk_count = doc.chunks.len();
    let pages = doc.chunks.iter().filter_map(|c| c.page).max();

    {
        let db = db.lock().await;
        let store = RagStore::new(db.conn());
        store.migrate()?;
        store.upsert_document(&doc, &vectors)?;
    }

    tracing::info!(
        document = %name,
        chunks = chunk_count,
        embedded,
        "document indexed"
    );

    Ok(IngestSummary {
        document_id,
        name,
        chunks: chunk_count,
        pages,
        embedded,
    })
}

/// Retrieve grounded context for a question.
///
/// Returns `None` when the library is empty, so the caller can skip the
/// grounded prompt entirely and answer as an ordinary chat.
pub async fn retrieve(
    question: &str,
    db: &Arc<Mutex<Db>>,
    embed: &Arc<EmbedService>,
) -> Result<Option<GroundedContext>> {
    // Embed first, outside the database lock: the HTTP round trip is orders
    // of magnitude slower than the query, and holding the lock across it
    // would stall every other database user.
    let query_vec = embed.embed_query(question).await.unwrap_or_default();

    let hits = {
        let db = db.lock().await;
        let store = RagStore::new(db.conn());
        store.migrate()?;
        if store.chunk_count()? == 0 {
            return Ok(None);
        }
        store.search(question, &query_vec, SEARCH_LIMIT)?
    };

    if hits.is_empty() {
        return Ok(None);
    }

    Ok(Some(build_context(&hits, CONTEXT_TOP_K)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embed_dim_matches_the_model() {
        // bge-small-en-v1.5 is a 384-dimension model. If this constant and
        // the model ever disagree, every stored vector is wrong and search
        // silently returns nonsense.
        assert_eq!(EMBED_DIM, 384);
    }

    #[tokio::test]
    async fn a_fresh_service_is_idle_and_has_no_embedder() {
        let s = EmbedService::new();
        assert_eq!(s.state().await, EmbedState::Idle);
        assert!(s.get().await.is_none());
    }

    #[tokio::test]
    async fn query_embedding_degrades_instead_of_failing() {
        // With no embedder the search must fall back to keywords, not error.
        // The M2 eval measured keyword-only at 0.731 recall@4 versus 0.769
        // for hybrid: worse, but far better than refusing to search.
        let s = EmbedService::new();
        assert!(s.embed_query("anything").await.is_none());
    }

    #[tokio::test]
    async fn passage_embedding_reports_absence_rather_than_inventing_vectors() {
        let s = EmbedService::new();
        let out = s.embed_passages(&["text".into()]).await.unwrap();
        assert!(
            out.is_none(),
            "must report no embedder rather than returning empty vectors, \
             which would be stored as a valid but meaningless index"
        );
    }

    #[tokio::test]
    async fn status_transitions_are_recorded() {
        let s = EmbedService::new();
        s.set(EmbedState::Starting, "starting").await;
        assert_eq!(s.state().await, EmbedState::Starting);
        assert_eq!(s.detail().await, "starting");

        s.set(EmbedState::Unavailable, "no model").await;
        assert_eq!(s.state().await, EmbedState::Unavailable);
        assert!(s.detail().await.contains("no model"));
    }

    #[test]
    fn missing_embed_model_is_detected_not_assumed() {
        // With the override pointing at a nonexistent path, lookup must fail
        // rather than silently falling through to the models directory.
        std::env::set_var("ORION_EMBED_MODEL_PATH", "/nonexistent/model.gguf");
        assert!(find_embed_model().is_none());
        std::env::remove_var("ORION_EMBED_MODEL_PATH");
    }
}
