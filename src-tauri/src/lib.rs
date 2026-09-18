//! Orion — application core.
//!
//! M0 scope: spawn the `llama-server` sidecar, persist chat to SQLite, and
//! stream tokens to the webview over Tauri events.
//!
//! Trust boundary: the webview is treated as untrusted. It receives rendered
//! tokens and status, never the sidecar port, the auth token, or filesystem
//! paths. Every capability it has is an explicit `#[tauri::command]`.

pub mod db;
pub mod documents;
pub mod engine;
pub mod error;
pub mod hashing;
pub mod ingest;
pub mod models;
pub mod profiler;
pub mod rag;
pub mod sidecars;

use std::sync::Arc;

use tauri::{Emitter, Manager, State};
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;
use tokio::sync::Mutex;

use db::Db;
use engine::{ChatMessage, Engine, EngineConfig, EngineState, EngineStatus};
use error::{OrionError, Result};
use models::{ModelManager, ModelStatus};
use profiler::{HardwareProfile, Tier, TierRecommendation};

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
    pub profile: HardwareProfile,
    pub models: ModelManager,
    /// Tier actually in use, after any user override.
    pub active_tier: Mutex<Tier>,
    /// Embedding sidecar backing semantic search over the user's documents.
    pub embed: Arc<documents::EmbedService>,
    /// Every spawned child process, so they can be killed on exit.
    pub sidecars: Arc<sidecars::SidecarRegistry>,
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

/// The measured (or simulated) hardware profile.
#[tauri::command]
async fn hardware_profile(state: State<'_, AppState>) -> Result<HardwareProfile> {
    Ok(state.profile.clone())
}

/// Tier recommendation with the reasoning behind it.
#[tauri::command]
async fn tier_recommendation(state: State<'_, AppState>) -> Result<TierRecommendation> {
    Ok(state.profile.recommend())
}

/// Catalogue with per-model installation state.
#[tauri::command]
async fn list_models(state: State<'_, AppState>) -> Result<Vec<ModelStatus>> {
    Ok(state.models.all_statuses())
}

/// Tier currently in use.
#[tauri::command]
async fn active_tier(state: State<'_, AppState>) -> Result<Tier> {
    Ok(*state.active_tier.lock().await)
}

/* ------------------------------------------------------------------ */
/* documents                                                           */
/* ------------------------------------------------------------------ */

/// Add files to the searchable library.
///
/// Each file is ingested independently so one unreadable document does not
/// abort the batch.
#[tauri::command]
async fn add_documents(
    paths: Vec<String>,
    state: State<'_, AppState>,
) -> Result<Vec<documents::IngestSummary>> {
    let mut out = Vec::new();
    let mut failures = Vec::new();

    for p in paths {
        let path = std::path::PathBuf::from(&p);
        match documents::ingest_file(&path, &state.db, &state.embed).await {
            Ok(summary) => out.push(summary),
            Err(e) => {
                tracing::warn!(path = %p, error = %e, "could not index document");
                failures.push(format!("{}: {e}", path.display()));
            }
        }
    }

    // A partial batch still succeeds: indexing four of five files and saying
    // so beats discarding all five because one was a scanned image.
    if out.is_empty() && !failures.is_empty() {
        return Err(OrionError::Config(failures.join("; ")));
    }
    Ok(out)
}

#[derive(serde::Serialize)]
pub struct LibraryStatus {
    pub documents: usize,
    pub chunks: usize,
    pub embed_state: documents::EmbedState,
    pub embed_detail: String,
}

#[tauri::command]
async fn library_status(state: State<'_, AppState>) -> Result<LibraryStatus> {
    let (documents, chunks) = {
        let db = state.db.lock().await;
        let store = rag::store::RagStore::new(db.conn());
        store.migrate()?;
        (store.document_count()?, store.chunk_count()?)
    };
    Ok(LibraryStatus {
        documents,
        chunks,
        embed_state: state.embed.state().await,
        embed_detail: state.embed.detail().await,
    })
}

#[tauri::command]
async fn forget_document(document_id: String, state: State<'_, AppState>) -> Result<()> {
    let db = state.db.lock().await;
    let store = rag::store::RagStore::new(db.conn());
    store.delete_document(&document_id)?;
    tracing::info!(document_id, "document removed from the library");
    Ok(())
}

/// Override the recommended tier. Takes effect on the next engine start.
#[tauri::command]
async fn set_active_tier(tier: String, state: State<'_, AppState>) -> Result<Tier> {
    let tier =
        Tier::parse(&tier).ok_or_else(|| OrionError::Config(format!("unknown tier: {tier}")))?;
    *state.active_tier.lock().await = tier;
    tracing::info!(tier = tier.as_str(), "active tier overridden by user");
    Ok(tier)
}

/* ------------------------------------------------------------------ */
/* sidecar                                                             */
/* ------------------------------------------------------------------ */

/// Resolve which GGUF to load, in priority order:
///   1. `ORION_MODEL_PATH` — explicit override, always wins
///   2. the catalogue model for the active tier, if installed
///   3. the best installed catalogue model of any tier
///   4. any user-supplied .gguf in the models directory
///
/// Falling back rather than refusing to start matters: a user who already has
/// weights should not be blocked because our preferred file is absent.
fn resolve_model(models: &ModelManager, tier: Tier) -> Result<std::path::PathBuf> {
    if let Ok(p) = std::env::var("ORION_MODEL_PATH") {
        let p = std::path::PathBuf::from(p);
        if p.is_file() {
            tracing::info!(path = %p.display(), "using ORION_MODEL_PATH");
            return Ok(p);
        }
        return Err(OrionError::Config(format!(
            "ORION_MODEL_PATH does not point at a file: {}",
            p.display()
        )));
    }

    if let Some(spec) = models.resolve_for_tier(tier) {
        let path = models.local_path(spec);
        tracing::info!(model = %spec.id, tier = spec.tier.as_str(), "resolved catalogue model");
        return Ok(path);
    }

    if let Some(found) = models.foreign_models().into_iter().next() {
        tracing::info!(path = %found.display(), "using a user-supplied model");
        return Ok(found);
    }

    Err(OrionError::NoModel)
}

/// Spawn `llama-server` bound to loopback on an ephemeral port with a random
/// bearer token, then wait for it to report healthy.
async fn start_engine(
    registry: Arc<sidecars::SidecarRegistry>,
    app: tauri::AppHandle,
    engine: Arc<Engine>,
    models: Arc<ModelManager>,
    tier: Tier,
) {
    let model_path = match resolve_model(&models, tier) {
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

    let (mut rx, child) = match spawned {
        Ok(v) => v,
        Err(e) => {
            engine
                .set_status(EngineState::Error, format!("could not start engine: {e}"))
                .await;
            return;
        }
    };

    // Hand the handle to the registry. Dropping it would orphan the process:
    // CommandChild has no Drop impl, so the ~2.4 GB llama-server would
    // outlive the app and lock its own executable against the next build.
    registry.register("llama-server (chat)", child);

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
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = db::data_dir()?;
            let database = Db::open(&data_dir.join("orion.db"))?;

            // Profile the machine before anything else — the tier decides
            // which model we try to load.
            let profile = HardwareProfile::detect_or_forced();
            let recommendation = profile.recommend();
            tracing::info!(
                total_ram = profile.total_ram_gib,
                available_ram = profile.available_ram_gib,
                cores = profile.cpu_cores,
                simulated = profile.simulated,
                tier = recommendation.tier.as_str(),
                "hardware profiled"
            );
            for reason in &recommendation.reasons {
                tracing::info!(target: "profiler", "{reason}");
            }
            for warning in &recommendation.warnings {
                tracing::warn!(target: "profiler", "{warning}");
            }

            let models_dir = data_dir.join("models");
            std::fs::create_dir_all(&models_dir).ok();
            let manager = Arc::new(ModelManager::new(models_dir).with_registry_file());
            let tier = recommendation.tier;
            // M0 uses a single rolling session; M1 adds the history sidebar.
            let session_id = database.create_session("New chat")?;

            let engine = Arc::new(Engine::new());

            let embed = Arc::new(documents::EmbedService::new());
            let sidecars = Arc::new(sidecars::SidecarRegistry::new());

            app.manage(AppState {
                engine: engine.clone(),
                db: Arc::new(Mutex::new(database)),
                session_id: Mutex::new(session_id),
                profile,
                models: ModelManager::new(db::data_dir().unwrap_or_default().join("models"))
                    .with_registry_file(),
                active_tier: Mutex::new(tier),
                embed: embed.clone(),
                sidecars: sidecars.clone(),
            });

            let handle = app.handle().clone();
            let mgr = manager.clone();
            let engine_sidecars = sidecars.clone();
            tauri::async_runtime::spawn(async move {
                start_engine(engine_sidecars, handle, engine, mgr, tier).await;
            });

            // The embedding sidecar warms up independently of the chat model.
            // It is small (~130 MB) and optional: if it never becomes ready,
            // search degrades to keyword-only rather than failing.
            let embed_handle = app.handle().clone();
            let embed_sidecars = sidecars.clone();
            tauri::async_runtime::spawn(async move {
                documents::start_embedder(embed_sidecars, embed_handle, embed).await;
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            engine_status,
            send_message,
            cancel_generation,
            hardware_profile,
            tier_recommendation,
            list_models,
            active_tier,
            set_active_tier,
            add_documents,
            library_status,
            forget_document
        ])
        .build(tauri::generate_context!())
        .expect("error while building Orion")
        .run(|app, event| {
            // Kill the sidecars when the app exits. Without this the
            // llama-server processes outlive Orion: CommandChild has no Drop
            // impl, so a dropped handle simply orphans the child. That leaks
            // ~2.4 GB per launch and locks the executable against the next
            // build on Windows.
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app.try_state::<AppState>() {
                    state.sidecars.shutdown();
                }
            }
        });
}
