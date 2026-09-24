//! Automated in-app resource downloader and setup engine.
//!
//! Downloads missing LLM weights, Whisper speech models, and Piper TTS voices
//! directly into the user data `models/` directory with real-time streaming
//! progress events (`download://progress`).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tauri::Emitter;

use crate::error::{OrionError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceItem {
    pub id: String,
    pub name: String,
    pub category: String, // "llm", "coder", "whisper", "piper"
    pub url: String,
    pub filename: String,
    pub approx_size_mb: u64,
    pub installed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceStatus {
    pub all_ready: bool,
    pub has_general_llm: bool,
    pub has_dedicated_llm: bool,
    pub has_whisper: bool,
    pub has_piper: bool,
    pub missing_items: Vec<ResourceItem>,
    pub installed_items: Vec<ResourceItem>,
    pub total_download_mb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadProgressPayload {
    pub item_id: String,
    pub item_name: String,
    pub category: String,
    pub current_bytes: u64,
    pub total_bytes: u64,
    pub percentage: f64,
    pub speed_mb_s: f64,
    pub current_step: usize,
    pub total_steps: usize,
    pub status: String, // "downloading", "verifying", "complete", "error"
    pub error_msg: Option<String>,
}

pub struct DownloadManager {
    models_dir: PathBuf,
    is_cancelled: Arc<AtomicBool>,
}

impl DownloadManager {
    pub fn new(models_dir: PathBuf) -> Self {
        Self {
            models_dir,
            is_cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn cancel(&self) {
        self.is_cancelled.store(true, Ordering::SeqCst);
    }

    /// Check which resources are installed and which are missing for the given persona.
    pub fn check_status(&self, persona_id: &str) -> ResourceStatus {
        let items = self.expected_resources(persona_id);
        let mut missing = Vec::new();
        let mut installed = Vec::new();
        let mut total_download_mb = 0;

        let mut has_general = false;
        let mut has_dedicated = false;
        let mut has_whisper = false;
        let mut has_piper = false;

        for mut item in items {
            let path = self.models_dir.join(&item.filename);
            let exists = path.is_file() && std::fs::metadata(&path).map(|m| m.len() > 1024).unwrap_or(false);
            item.installed = exists;

            if exists {
                match item.category.as_str() {
                    "llm" => has_general = true,
                    "coder" => has_dedicated = true,
                    "whisper" => has_whisper = true,
                    "piper" => has_piper = true,
                    _ => {}
                }
                installed.push(item);
            } else {
                total_download_mb += item.approx_size_mb;
                missing.push(item);
            }
        }

        // Dedicated is considered ready if not needed or installed
        let dedicated_needed = persona_id == "developer" || persona_id == "coder";
        let dedicated_ok = !dedicated_needed || has_dedicated;
        let all_ready = has_general && dedicated_ok && has_whisper && has_piper;

        ResourceStatus {
            all_ready,
            has_general_llm: has_general,
            has_dedicated_llm: has_dedicated,
            has_whisper,
            has_piper,
            missing_items: missing,
            installed_items: installed,
            total_download_mb,
        }
    }

    /// Returns list of all target resources based on the selected persona.
    pub fn expected_resources(&self, persona_id: &str) -> Vec<ResourceItem> {
        let mut list = vec![
            ResourceItem {
                id: "qwen2.5-3b-general".into(),
                name: "Qwen2.5 3B Base Intelligence".into(),
                category: "llm".into(),
                url: "https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/resolve/main/qwen2.5-3b-instruct-q4_k_m.gguf?download=true".into(),
                filename: "qwen2.5-3b-instruct-q4_k_m.gguf".into(),
                approx_size_mb: 2100,
                installed: false,
            },
        ];

        // Dedicated specialized model if developer
        if persona_id == "developer" || persona_id == "coder" {
            list.push(ResourceItem {
                id: "qwen2.5-coder-3b".into(),
                name: "Qwen2.5-Coder 3B (Dedicated Coding Engine)".into(),
                category: "coder".into(),
                url: "https://huggingface.co/Qwen/Qwen2.5-Coder-3B-Instruct-GGUF/resolve/main/qwen2.5-coder-3b-instruct-q4_k_m.gguf?download=true".into(),
                filename: "qwen2.5-coder-3b-instruct-q4_k_m.gguf".into(),
                approx_size_mb: 2100,
                installed: false,
            });
        } else if persona_id == "researcher" || persona_id == "analyst" {
            list.push(ResourceItem {
                id: "deepseek-r1-distill-1.5b".into(),
                name: "DeepSeek-R1 Distill (Dedicated Reasoning Engine)".into(),
                category: "coder".into(),
                url: "https://huggingface.co/unsloth/DeepSeek-R1-Distill-Qwen-1.5B-GGUF/resolve/main/DeepSeek-R1-Distill-Qwen-1.5B-Q4_K_M.gguf?download=true".into(),
                filename: "DeepSeek-R1-Distill-Qwen-1.5B-Q4_K_M.gguf".into(),
                approx_size_mb: 1100,
                installed: false,
            });
        }

        // Speech recognition
        list.push(ResourceItem {
            id: "whisper-tiny-en".into(),
            name: "Whisper Speech Recognition (STT)".into(),
            category: "whisper".into(),
            url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.en.bin?download=true".into(),
            filename: "ggml-tiny.en.bin".into(),
            approx_size_mb: 75,
            installed: false,
        });

        // Speech synthesis model
        list.push(ResourceItem {
            id: "piper-lessac-voice".into(),
            name: "Piper Neural Voice Model (TTS)".into(),
            category: "piper".into(),
            url: "https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/en/en_US/lessac/medium/en_US-lessac-medium.onnx?download=true".into(),
            filename: "en_US-lessac-medium.onnx".into(),
            approx_size_mb: 35,
            installed: false,
        });

        list
    }

    /// Asynchronously download all missing resources, emitting progress events.
    pub async fn download_missing<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        persona_id: &str,
    ) -> Result<()> {
        self.is_cancelled.store(false, Ordering::SeqCst);
        std::fs::create_dir_all(&self.models_dir).ok();

        let status = self.check_status(persona_id);
        let items = status.missing_items;
        let total_steps = items.len();

        if total_steps == 0 {
            tracing::info!("all resources already downloaded");
            return Ok(());
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(3600))
            .build()
            .map_err(|e| OrionError::Config(format!("failed to initialize HTTP client: {e}")))?;

        for (idx, item) in items.iter().enumerate() {
            if self.is_cancelled.load(Ordering::SeqCst) {
                tracing::warn!("download cancelled by user");
                return Err(OrionError::Config("download cancelled by user".into()));
            }

            let step_index = idx + 1;
            let dest_path = self.models_dir.join(&item.filename);
            let part_path = self.models_dir.join(format!("{}.part", item.filename));

            tracing::info!(
                item = %item.name,
                url = %item.url,
                dest = %dest_path.display(),
                "downloading resource"
            );

            // Initial notification
            let _ = app.emit(
                "download://progress",
                DownloadProgressPayload {
                    item_id: item.id.clone(),
                    item_name: item.name.clone(),
                    category: item.category.clone(),
                    current_bytes: 0,
                    total_bytes: item.approx_size_mb * 1024 * 1024,
                    percentage: 0.0,
                    speed_mb_s: 0.0,
                    current_step: step_index,
                    total_steps,
                    status: "downloading".into(),
                    error_msg: None,
                },
            );

            let res = match client.get(&item.url).send().await {
                Ok(r) => r,
                Err(e) => {
                    let err = format!("network error connecting to download source: {e}");
                    let _ = app.emit(
                        "download://progress",
                        DownloadProgressPayload {
                            item_id: item.id.clone(),
                            item_name: item.name.clone(),
                            category: item.category.clone(),
                            current_bytes: 0,
                            total_bytes: 0,
                            percentage: 0.0,
                            speed_mb_s: 0.0,
                            current_step: step_index,
                            total_steps,
                            status: "error".into(),
                            error_msg: Some(err.clone()),
                        },
                    );
                    return Err(OrionError::Config(err));
                }
            };

            if !res.status().is_success() {
                let err = format!("HTTP error: {}", res.status());
                return Err(OrionError::Config(err));
            }

            let total_size = res.content_length().unwrap_or(item.approx_size_mb * 1024 * 1024);
            let mut file = tokio::fs::File::create(&part_path).await.map_err(|e| {
                OrionError::Config(format!("failed to create destination file: {e}"))
            })?;

            let mut stream = res.bytes_stream();
            let mut downloaded: u64 = 0;
            let start_time = Instant::now();
            let mut last_emit = Instant::now();

            use tokio::io::AsyncWriteExt;
            while let Some(chunk_res) = stream.next().await {
                if self.is_cancelled.load(Ordering::SeqCst) {
                    let _ = tokio::fs::remove_file(&part_path).await;
                    return Err(OrionError::Config("download cancelled".into()));
                }

                let chunk = chunk_res.map_err(|e| OrionError::Config(format!("stream error: {e}")))?;
                file.write_all(&chunk).await.map_err(|e| {
                    OrionError::Config(format!("write error while downloading: {e}"))
                })?;

                downloaded += chunk.len() as u64;

                // Emit progress every 200 ms to prevent event flood
                if last_emit.elapsed().as_millis() > 200 || downloaded == total_size {
                    last_emit = Instant::now();
                    let elapsed_secs = start_time.elapsed().as_secs_f64().max(0.001);
                    let speed_mb_s = (downloaded as f64 / (1024.0 * 1024.0)) / elapsed_secs;
                    let percentage = ((downloaded as f64 / total_size as f64) * 100.0).clamp(0.0, 100.0);

                    let _ = app.emit(
                        "download://progress",
                        DownloadProgressPayload {
                            item_id: item.id.clone(),
                            item_name: item.name.clone(),
                            category: item.category.clone(),
                            current_bytes: downloaded,
                            total_bytes: total_size,
                            percentage,
                            speed_mb_s,
                            current_step: step_index,
                            total_steps,
                            status: "downloading".into(),
                            error_msg: None,
                        },
                    );
                }
            }

            file.flush().await.ok();
            drop(file);

            // Atomically rename .part to final
            std::fs::rename(&part_path, &dest_path).map_err(|e| {
                OrionError::Config(format!("failed to finalize downloaded file: {e}"))
            })?;

            // If piper onnx, also fetch its tiny config json
            if item.id == "piper-lessac-voice" {
                let json_url = "https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/en/en_US/lessac/medium/en_US-lessac-medium.onnx.json?download=true";
                let json_dest = self.models_dir.join("en_US-lessac-medium.onnx.json");
                if let Ok(resp) = client.get(json_url).send().await {
                    if let Ok(bytes) = resp.bytes().await {
                        std::fs::write(json_dest, bytes).ok();
                    }
                }
            }

            let _ = app.emit(
                "download://progress",
                DownloadProgressPayload {
                    item_id: item.id.clone(),
                    item_name: item.name.clone(),
                    category: item.category.clone(),
                    current_bytes: total_size,
                    total_bytes: total_size,
                    percentage: 100.0,
                    speed_mb_s: 0.0,
                    current_step: step_index,
                    total_steps,
                    status: "complete".into(),
                    error_msg: None,
                },
            );

            tracing::info!(item = %item.name, "resource download complete");
        }

        // Final completion event
        let _ = app.emit("download://all_complete", ());
        Ok(())
    }
}
