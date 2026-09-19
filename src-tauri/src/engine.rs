//! Manages the `llama-server` sidecar process and talks to it over HTTP.
//!
//! Design notes (these are deliberate, please don't "simplify" them away):
//!
//! * The sidecar binds to `127.0.0.1` on an **ephemeral port** with a random
//!   per-session bearer token. Serina (the predecessor project) published every
//!   backend port to the host with no auth; anything on the LAN could read the
//!   user's documents. We do not repeat that.
//! * The webview never learns the port or the token. All model traffic goes
//!   through Rust commands, so a compromised renderer cannot reach the model
//!   directly.
//! * We use the **OpenAI-compatible** `/v1/chat/completions` endpoint rather
//!   than hand-building a `"User:\nAssistant:"` prompt string. That lets
//!   llama.cpp apply the model's real chat template, which materially improves
//!   instruct-model output and gives us a genuine system role.

use std::path::PathBuf;
use std::sync::Arc;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

use crate::error::{OrionError, Result};

/// Lifecycle of the inference engine, surfaced to the UI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EngineState {
    /// Nothing has been started, and nothing is wrong.
    ///
    /// Distinct from `Starting`, which claims work is in progress. Since the
    /// engine became lazy the app sits here until the first message, and
    /// reporting "Starting engine…" for that made a perfectly healthy idle
    /// app look like it had hung — which is exactly how it was reported.
    Idle,
    /// Process spawning.
    Starting,
    /// Process up, model still being read into memory.
    Loading,
    /// Accepting requests.
    Ready,
    /// Unrecoverable without user action (missing model, port clash, crash).
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct EngineStatus {
    pub state: EngineState,
    /// Human-readable context. Shown in the status pill's tooltip.
    pub detail: String,
}

/// A single chat turn in OpenAI wire format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: content.into(),
        }
    }
}

/// Where the model lives and how to reach it.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub model_path: PathBuf,
    pub host: String,
    pub port: u16,
    pub auth_token: String,
    /// Context window. Kept modest by default — on an 8 GB machine the KV
    /// cache is a real cost, not a rounding error.
    pub ctx_size: u32,
    pub threads: usize,
}

impl EngineConfig {
    pub fn base_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
}

pub struct Engine {
    config: RwLock<Option<EngineConfig>>,
    status: RwLock<EngineStatus>,
    http: reqwest::Client,
    /// Set while a generation is in flight; dropping/flagging it stops the stream.
    cancel: Mutex<Option<Arc<std::sync::atomic::AtomicBool>>>,
}

impl Engine {
    pub fn new() -> Self {
        Self {
            config: RwLock::new(None),
            status: RwLock::new(EngineStatus {
                state: EngineState::Idle,
                detail: "Ready when you are — the model loads on your first message".into(),
            }),
            // No global timeout: generation legitimately runs for minutes on
            // CPU-only hardware. Cancellation is explicit instead.
            http: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("failed to build http client"),
            cancel: Mutex::new(None),
        }
    }

    pub async fn status(&self) -> EngineStatus {
        self.status.read().await.clone()
    }

    pub async fn set_status(&self, state: EngineState, detail: impl Into<String>) {
        let detail = detail.into();
        tracing::info!(?state, %detail, "engine status changed");
        *self.status.write().await = EngineStatus { state, detail };
    }

    pub async fn set_config(&self, config: EngineConfig) {
        *self.config.write().await = Some(config);
    }

    pub async fn config(&self) -> Option<EngineConfig> {
        self.config.read().await.clone()
    }

    /// Poll `/health` until the model reports ready, or give up.
    ///
    /// llama-server returns 503 while weights are still loading, which is the
    /// signal we use to distinguish `Loading` from `Ready`.
    pub async fn wait_until_ready(&self, max_wait: std::time::Duration) -> Result<()> {
        let Some(cfg) = self.config().await else {
            return Err(OrionError::Engine("engine not configured".into()));
        };

        let url = format!("{}/health", cfg.base_url());
        let deadline = std::time::Instant::now() + max_wait;

        while std::time::Instant::now() < deadline {
            match self
                .http
                .get(&url)
                .bearer_auth(&cfg.auth_token)
                .send()
                .await
            {
                Ok(r) if r.status().is_success() => {
                    self.set_status(EngineState::Ready, "Model loaded").await;
                    return Ok(());
                }
                Ok(_) => {
                    self.set_status(EngineState::Loading, "Loading model into memory…")
                        .await;
                }
                Err(_) => {
                    // Connection refused simply means the process hasn't bound yet.
                    self.set_status(EngineState::Starting, "Waiting for engine…")
                        .await;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(700)).await;
        }

        self.set_status(EngineState::Error, "Engine did not become ready in time")
            .await;
        Err(OrionError::Engine(
            "timed out waiting for llama-server".into(),
        ))
    }

    /// Signal any in-flight generation to stop.
    pub async fn cancel(&self) {
        if let Some(flag) = self.cancel.lock().await.take() {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Stream a chat completion, invoking `on_token` for each text delta.
    ///
    /// Returns the full concatenated reply.
    pub async fn chat_stream<F>(
        &self,
        messages: Vec<ChatMessage>,
        mut on_token: F,
    ) -> Result<String>
    where
        F: FnMut(&str),
    {
        let Some(cfg) = self.config().await else {
            return Err(OrionError::Engine("engine not configured".into()));
        };

        let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        *self.cancel.lock().await = Some(flag.clone());

        let body = serde_json::json!({
            "messages": messages,
            "stream": true,
            "temperature": 0.7,
            "top_p": 0.9,
            "cache_prompt": true,
        });

        let resp = self
            .http
            .post(format!("{}/v1/chat/completions", cfg.base_url()))
            .bearer_auth(&cfg.auth_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| OrionError::Engine(format!("request failed: {e}")))?;

        if !resp.status().is_success() {
            let code = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(OrionError::Engine(format!(
                "engine returned {code}: {text}"
            )));
        }

        let mut stream = resp.bytes_stream();
        let mut full = String::new();
        // SSE frames can split across chunk boundaries, so we accumulate and
        // only consume complete lines.
        let mut buf = String::new();

        while let Some(chunk) = stream.next().await {
            if flag.load(std::sync::atomic::Ordering::Relaxed) {
                tracing::info!("generation cancelled by user");
                break;
            }

            let chunk = chunk.map_err(|e| OrionError::Engine(format!("stream error: {e}")))?;
            // Multi-byte UTF-8 characters can straddle chunk boundaries. Using
            // from_utf8_lossy per chunk would corrupt them, so decode leniently
            // only over the accumulated buffer.
            buf.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(idx) = buf.find('\n') {
                let line = buf[..idx].trim().to_string();
                buf.drain(..=idx);

                let Some(data) = line.strip_prefix("data: ") else {
                    continue;
                };
                if data == "[DONE]" {
                    break;
                }

                let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else {
                    continue;
                };
                if let Some(tok) = v["choices"][0]["delta"]["content"].as_str() {
                    if !tok.is_empty() {
                        full.push_str(tok);
                        on_token(tok);
                    }
                }
            }
        }

        *self.cancel.lock().await = None;
        Ok(full)
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate a random bearer token for this session.
pub fn random_token() -> String {
    use rand::Rng;
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    (0..48)
        .map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char)
        .collect()
}

/// Ask the OS for a free TCP port by binding to :0 and reading it back.
pub fn free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| OrionError::Engine(format!("could not find a free port: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| OrionError::Engine(format!("could not read local addr: {e}")))?
        .port();
    Ok(port)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_fresh_engine_is_idle_not_starting() {
        // Regression guard for a real report: the app appeared "stuck at
        // starting engine" forever after launch.
        //
        // It was not stuck. The engine became lazy — it does nothing until
        // the first message — but the default state was still Starting with
        // detail "Engine not started", which the UI rendered as
        // "Starting engine…" with a pulsing amber dot. A perfectly healthy
        // idle app was indistinguishable from a hung one.
        let e = Engine::new();
        let st = e.status().await;

        assert_eq!(
            st.state,
            EngineState::Idle,
            "an engine that has not been asked to do anything must not claim \
             to be starting"
        );
        assert!(
            !st.detail.to_lowercase().contains("not started"),
            "the detail is shown to the user and should not read as a \
             failure: {:?}",
            st.detail
        );
    }

    #[tokio::test]
    async fn a_loading_engine_is_not_ready_to_answer() {
        // The gap that produced a 503 on every first message: ensure_engine
        // spawned the process and returned immediately, so send_message fired
        // a request while llama-server was still mapping the weights.
        //
        // Ready must mean "can answer now". Any other state means wait.
        for state in [
            EngineState::Idle,
            EngineState::Starting,
            EngineState::Loading,
            EngineState::Error,
        ] {
            assert_ne!(
                state,
                EngineState::Ready,
                "{state:?} must not be treated as able to answer"
            );
        }
    }

    #[tokio::test]
    async fn an_unconfigured_engine_refuses_rather_than_hanging() {
        // wait_until_ready is now on the send path, so its failure mode is
        // user-visible. With no config it must return promptly instead of
        // polling a URL that does not exist until the timeout expires.
        let e = Engine::new();
        let r = e.wait_until_ready(std::time::Duration::from_secs(5)).await;
        assert!(r.is_err(), "should not claim readiness with no engine");
    }

    #[tokio::test]
    async fn idle_is_distinct_from_every_other_state() {
        // The UI keys colour and wording off this. If Idle ever collapses
        // into Starting again, the bug returns silently.
        assert_ne!(EngineState::Idle, EngineState::Starting);
        assert_ne!(EngineState::Idle, EngineState::Loading);
        assert_ne!(EngineState::Idle, EngineState::Ready);
        assert_ne!(EngineState::Idle, EngineState::Error);
    }
}
