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
pub mod presence;

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

/// Assess files the webview reports as dropped, without ingesting them yet.
/// The renderer never gets to decide what is readable; that is policy.
#[tauri::command]
async fn assess_dropped_files(paths: Vec<String>) -> Result<DropSummary> {
    let paths: Vec<std::path::PathBuf> = paths.into_iter().map(Into::into).collect();
    let a = presence::assess(&paths, &presence::RealFs);
    Ok(DropSummary {
        summary: a.summary(),
        accepted: a
            .accepted
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect(),
        rejected: a
            .rejected
            .iter()
            .map(|(p, why)| {
                (
                    p.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    why.clone(),
                )
            })
            .collect(),
        truncated: a.truncated,
    })
}

#[derive(serde::Serialize)]
pub struct DropSummary {
    pub summary: String,
    pub accepted: Vec<String>,
    /// (file name, reason) — the path is deliberately not sent to the
    /// renderer, which has no need for the user's directory layout.
    pub rejected: Vec<(String, String)>,
    pub truncated: bool,
}

/// The hotkey label to show in the UI, formatted for this platform.
#[tauri::command]
fn hotkey_label() -> String {
    presence::Hotkey::default_global().display_for(presence::Platform::current())
}

/// Files handed to us on the command line, filtered through the drop policy.
fn files_from_args(args: &[String]) -> Vec<std::path::PathBuf> {
    let paths: Vec<std::path::PathBuf> = args
        .iter()
        .skip(1) // argv[0] is the executable
        .filter(|a| !a.starts_with('-'))
        .map(std::path::PathBuf::from)
        .collect();

    if paths.is_empty() {
        return Vec::new();
    }
    presence::assess(&paths, &presence::RealFs).accepted
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("ORION_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let boot = std::time::Instant::now();
    let mut trace = presence::StartupTrace::new();

    tauri::Builder::default()
        // Single instance must be registered first, so a second launch is
        // short-circuited before it does any setup work of its own.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            tracing::info!("second instance launched");
            presence::tauri_glue::activate(app, presence::Activation::SecondInstance);

            let files = files_from_args(&args);
            if !files.is_empty() {
                use tauri::Emitter;
                presence::tauri_glue::activate(app, presence::Activation::FileDrop);
                let _ = app.emit("files://opened", files);
            }
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(move |app| {
            use presence::tauri_glue::timed;

            trace.record(presence::Phase::RuntimeInit, boot.elapsed());

            let database = timed(&mut trace, presence::Phase::Database, || -> Result<Db> {
                let data_dir = db::data_dir()?;
                Db::open(&data_dir.join("orion.db"))
            })?;

            let session_id = timed(&mut trace, presence::Phase::Config, || {
                database.create_session("New chat")
            })?;

            let engine = Arc::new(Engine::new());

            app.manage(AppState {
                engine: engine.clone(),
                db: Arc::new(Mutex::new(database)),
                session_id: Mutex::new(session_id),
            });

            timed(&mut trace, presence::Phase::SystemIntegration, || {
                if let Err(e) = presence::tauri_glue::build_tray(app.handle()) {
                    // A missing tray is survivable; the window still works.
                    tracing::error!(error = %e, "tray icon could not be created");
                }
                presence::tauri_glue::register_hotkey(app.handle(), None)
            });

            timed(&mut trace, presence::Phase::WindowShow, || {
                if let Some(w) = app.get_webview_window(presence::tauri_glue::MAIN_WINDOW) {
                    let _ = w.show();
                }
            });

            // Files passed on the command line, e.g. "Open with Orion".
            let files = files_from_args(&std::env::args().collect::<Vec<_>>());
            if !files.is_empty() {
                use tauri::Emitter;
                let _ = app.emit("files://opened", files);
            }

            tracing::info!("\n{}", trace.render());
            if !trace.within_gate() {
                tracing::warn!(
                    "cold start exceeded the {:?} gate",
                    presence::startup::COLD_START_BUDGET
                );
            }

            // Everything below here is deliberately AFTER the window is up.
            // See presence::startup::DeferredWork for why.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                start_engine(handle, engine).await;
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Orion lives in the tray; closing hides it so the global
                // hotkey keeps working. Quit is in the tray menu.
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            engine_status,
            send_message,
            cancel_generation,
            assess_dropped_files,
            hotkey_label
        ])
        .run(tauri::generate_context!())
        .expect("error while running Orion");
}
