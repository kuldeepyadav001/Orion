//! Orion — application core.
//!
//! M0 scope: spawn the `llama-server` sidecar, persist chat to SQLite, and
//! stream tokens to the webview over Tauri events.
//!
//! Trust boundary: the webview is treated as untrusted. It receives rendered
//! tokens and status, never the sidecar port, the auth token, or filesystem
//! paths. Every capability it has is an explicit `#[tauri::command]`.

pub mod db;
pub mod engine;
pub mod error;
pub mod hashing;
pub mod ingest;
pub mod rag;

use std::sync::Arc;

use tauri::{Emitter, Manager, State};
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;
use tokio::sync::Mutex;

use db::Db;
use engine::{ChatMessage, Engine, EngineConfig, EngineState, EngineStatus};
use error::{OrionError, Result};

/// How many past turns to replay into the model. Kept small deliberately:
/// on an 8 GB machine the KV cache for a long history is a real memory cost.
const HISTORY_LIMIT: usize = 20;

const SYSTEM_PROMPT: &str = "You are Orion, a private AI assistant running entirely on the \
user's own computer. Be direct, accurate and concise. If you do not know something, say so \
plainly rather than guessing.";

pub struct AppState {
    pub engine: Arc<Engine>,
    pub db: Arc<Mutex<Db>>,
    pub session_id: Mutex<String>,
}

/* ------------------------------------------------------------------ */
/* commands                                                            */
/* ------------------------------------------------------------------ */

#[tauri::command]
async fn engine_status(state: State<'_, AppState>) -> Result<EngineStatus> {
    Ok(state.engine.status().await)
}

/// Send a user message and stream the reply back as `chat://token` events.
#[tauri::command]
async fn send_message(
    message: String,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<()> {
    let message = message.trim().to_string();
    if message.is_empty() {
        return Err(OrionError::Config("message is empty".into()));
    }

    let session_id = state.session_id.lock().await.clone();

    // Persist the user's turn *before* generating. If the app dies mid-stream
    // the question survives — Serina lost it in exactly this window.
    {
        let db = state.db.lock().await;
        db.add_message(&session_id, "user", &message)?;
    }

    let history = {
        let db = state.db.lock().await;
        db.recent_messages(&session_id, HISTORY_LIMIT)?
    };

    let mut msgs = vec![ChatMessage::system(SYSTEM_PROMPT)];
    msgs.extend(history.into_iter().map(|m| ChatMessage {
        role: m.role,
        content: m.content,
    }));

    let engine = state.engine.clone();
    let db = state.db.clone();
    let app_for_task = app.clone();

    // Generation runs detached so the command returns immediately and the UI
    // stays responsive; progress arrives purely via events.
    tauri::async_runtime::spawn(async move {
        let emitter = app_for_task.clone();
        let result = engine
            .chat_stream(msgs, move |tok| {
                let _ = emitter.emit("chat://token", tok);
            })
            .await;

        match result {
            Ok(reply) => {
                if !reply.is_empty() {
                    let db = db.lock().await;
                    if let Err(e) = db.add_message(&session_id, "assistant", &reply) {
                        tracing::error!(error = %e, "failed to persist assistant reply");
                    }
                }
                let _ = app_for_task.emit("chat://done", ());
            }
            Err(e) => {
                tracing::error!(error = %e, "generation failed");
                let _ = app_for_task.emit("chat://error", e.to_string());
            }
        }
    });

    Ok(())
}

#[tauri::command]
async fn cancel_generation(state: State<'_, AppState>) -> Result<()> {
    state.engine.cancel().await;
    Ok(())
}

/* ------------------------------------------------------------------ */
/* sidecar                                                             */
/* ------------------------------------------------------------------ */

/// Locate a GGUF model. M1 replaces this with the hardware-aware model
/// manager; for now we look in the app data dir and honour an env override.
fn find_model() -> Result<std::path::PathBuf> {
    if let Ok(p) = std::env::var("ORION_MODEL_PATH") {
        let p = std::path::PathBuf::from(p);
        if p.is_file() {
            return Ok(p);
        }
        return Err(OrionError::Config(format!(
            "ORION_MODEL_PATH does not point at a file: {}",
            p.display()
        )));
    }

    let dir = db::data_dir()?.join("models");
    if dir.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("gguf"))
            .collect();
        entries.sort();
        if let Some(first) = entries.into_iter().next() {
            return Ok(first);
        }
    }

    Err(OrionError::NoModel)
}

/// Spawn `llama-server` bound to loopback on an ephemeral port with a random
/// bearer token, then wait for it to report healthy.
async fn start_engine(app: tauri::AppHandle, engine: Arc<Engine>) {
    let model_path = match find_model() {
        Ok(p) => p,
        Err(e) => {
            engine
                .set_status(
                    EngineState::Error,
                    format!(
                        "{e}. Place a .gguf file in the models folder or set ORION_MODEL_PATH."
                    ),
                )
                .await;
            return;
        }
    };

    let port = match engine::free_port() {
        Ok(p) => p,
        Err(e) => {
            engine.set_status(EngineState::Error, e.to_string()).await;
            return;
        }
    };

    let token = engine::random_token();
    // Leave a core for the UI on small machines; never go below one.
    let threads = std::thread::available_parallelism()
        .map(|n| (n.get().saturating_sub(1)).max(1))
        .unwrap_or(2);

    let cfg = EngineConfig {
        model_path: model_path.clone(),
        host: "127.0.0.1".into(),
        port,
        auth_token: token.clone(),
        ctx_size: 4096,
        threads,
    };
    engine.set_config(cfg.clone()).await;

    tracing::info!(model = %model_path.display(), port, threads, "starting llama-server");
    engine
        .set_status(EngineState::Starting, "Starting inference engine…")
        .await;

    let sidecar = app.shell().sidecar("llama-server");
    let sidecar = match sidecar {
        Ok(c) => c,
        Err(e) => {
            engine
                .set_status(
                    EngineState::Error,
                    format!("llama-server sidecar missing: {e}. Run scripts/fetch-sidecars.sh"),
                )
                .await;
            return;
        }
    };

    let spawned = sidecar
        .args([
            "--model".into(),
            model_path.to_string_lossy().to_string(),
            "--host".into(),
            "127.0.0.1".into(),
            "--port".into(),
            port.to_string(),
            "--api-key".into(),
            token,
            "--ctx-size".into(),
            cfg.ctx_size.to_string(),
            "--threads".into(),
            threads.to_string(),
        ])
        .spawn();

    let (mut rx, _child) = match spawned {
        Ok(v) => v,
        Err(e) => {
            engine
                .set_status(EngineState::Error, format!("could not start engine: {e}"))
                .await;
            return;
        }
    };

    // Drain sidecar output into our logs; llama-server is chatty on stderr.
    tauri::async_runtime::spawn(async move {
        while let Some(ev) = rx.recv().await {
            match ev {
                CommandEvent::Stderr(b) => {
                    tracing::debug!(target: "llama", "{}", String::from_utf8_lossy(&b).trim())
                }
                CommandEvent::Stdout(b) => {
                    tracing::debug!(target: "llama", "{}", String::from_utf8_lossy(&b).trim())
                }
                CommandEvent::Terminated(p) => {
                    tracing::warn!(?p, "llama-server exited");
                    break;
                }
                _ => {}
            }
        }
    });

    // Cold-loading a 4B model from disk on a slow laptop can genuinely take
    // a couple of minutes the first time.
    if let Err(e) = engine
        .wait_until_ready(std::time::Duration::from_secs(180))
        .await
    {
        tracing::error!(error = %e, "engine failed to become ready");
    }
}

/* ------------------------------------------------------------------ */
/* entrypoint                                                          */
/* ------------------------------------------------------------------ */

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("ORION_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let data_dir = db::data_dir()?;
            let database = Db::open(&data_dir.join("orion.db"))?;
            // M0 uses a single rolling session; M1 adds the history sidebar.
            let session_id = database.create_session("New chat")?;

            let engine = Arc::new(Engine::new());

            app.manage(AppState {
                engine: engine.clone(),
                db: Arc::new(Mutex::new(database)),
                session_id: Mutex::new(session_id),
            });

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                start_engine(handle, engine).await;
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            engine_status,
            send_message,
            cancel_generation
        ])
        .run(tauri::generate_context!())
        .expect("error while running Orion");
}
