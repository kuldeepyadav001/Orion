//! Text to speech via Piper TTS.
//!
//! ## Why Piper
//!
//! Piper is a fast, local neural text-to-speech system that runs as a
//! standalone C++ binary without Python. On target CPU hardware (including
//! 8 GB / 5.7 GB usable laptops), Piper synthesizes high-quality audio at
//! ~30-50x realtime, requiring only ~25 MB of resident RAM during synthesis
//! and releasing it immediately when finished.
//!
//! ## Output Delivery
//!
//! Synthesized audio is produced as standard 16-bit or 22.05 kHz WAV audio.
//! In the desktop app, audio is passed to the webview via `voice://speak`
//! (base64 encoded), allowing the browser's native Web Audio API to play it
//! with zero ALSA/DirectSound driver contention, while driving the reactive
//! `speaking` state in `VoiceButton`.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::error::{OrionError, Result};

/// Default lightweight English voice model for Piper (~15 MB, low memory footprint).
pub const MODEL_LESSAC_LOW: &str = "en_US-lessac-low.onnx";
/// Standard English voice model (~28 MB).
pub const MODEL_LESSAC_MEDIUM: &str = "en_US-lessac-medium.onnx";

/// Hard timeout on synthesis to prevent hangs on stalled subprocesses.
pub const TTS_TIMEOUT_SECS: u64 = 30;

/// A completed synthesis result.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeechResult {
    pub wav_bytes: Vec<u8>,
    pub elapsed_ms: u128,
}

/// Locate a Piper voice model in the models directory.
pub fn find_piper_model(models_dir: &Path) -> Option<PathBuf> {
    if let Ok(p) = std::env::var("ORION_PIPER_MODEL_PATH") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }

    // Prefer the low/compact model on resource-constrained systems.
    for name in [MODEL_LESSAC_LOW, MODEL_LESSAC_MEDIUM] {
        let p = models_dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }

    // Fall back to any .onnx model present in the models directory.
    std::fs::read_dir(models_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.extension()
                .map(|ext| ext.eq_ignore_ascii_case("onnx"))
                .unwrap_or(false)
        })
}

/// Where the piper executable lives.
pub fn find_piper_binary() -> PathBuf {
    if let Ok(p) = std::env::var("ORION_PIPER_PATH") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return p;
        }
    }

    let exe = if cfg!(windows) { "piper.exe" } else { "piper" };

    // 1. Beside running executable
    if let Ok(p) = std::env::current_exe() {
        if let Some(d) = p.parent() {
            let candidate = d.join(exe);
            if candidate.is_file() {
                return candidate;
            }
        }
    }

    // 2. Relative to CARGO_MANIFEST_DIR (dev/test)
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        let candidate = PathBuf::from(manifest).join("binaries").join(exe);
        if candidate.is_file() {
            return candidate;
        }
    }

    // 3. Project binaries directory
    for prefix in &["src-tauri/binaries", "binaries", "../src-tauri/binaries"] {
        let candidate = PathBuf::from(prefix).join(exe);
        if candidate.is_file() {
            return candidate;
        }
    }

    // 4. PATH fallback
    PathBuf::from(exe)
}

/// Clean markdown, code blocks, URLs, and formatting so speech flows naturally.
pub fn clean_for_speech(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_code_block = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            continue;
        }

        // Drop heading hashes
        let content = trimmed.trim_start_matches('#').trim();
        if content.is_empty() {
            continue;
        }

        let mut cleaned_line = String::new();
        let mut chars = content.chars().peekable();

        while let Some(c) = chars.next() {
            // Drop inline backticks
            if c == '`' {
                continue;
            }
            // Drop asterisks and underscores (bold/italic)
            if c == '*' || c == '_' {
                continue;
            }
            // Strip citation brackets e.g. [chunk:12] or [1]
            if c == '[' {
                let mut bracketed = String::new();
                let mut closed = false;
                for inner in chars.by_ref() {
                    if inner == ']' {
                        closed = true;
                        break;
                    }
                    bracketed.push(inner);
                }
                if closed {
                    // Check if markdown link [title](url)
                    if chars.peek() == Some(&'(') {
                        chars.next(); // consume '('
                        for link_c in chars.by_ref() {
                            if link_c == ')' {
                                break;
                            }
                        }
                        cleaned_line.push_str(&bracketed);
                    } else if !bracketed.contains("chunk:") && !bracketed.contains("citation:") {
                        cleaned_line.push_str(&bracketed);
                    }
                    continue;
                }
            }

            cleaned_line.push(c);
        }

        if !cleaned_line.is_empty() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(&cleaned_line);
        }
    }

    // Collapse multiple consecutive spaces
    let collapsed: Vec<&str> = out.split_whitespace().collect();
    collapsed.join(" ")
}

/// Build CLI arguments for Piper.
pub fn build_args(model: &Path, output_wav: &Path) -> Vec<String> {
    vec![
        "--model".into(),
        model.to_string_lossy().to_string(),
        "--output_file".into(),
        output_wav.to_string_lossy().to_string(),
    ]
}

/// Synthesize text to WAV audio using Piper.
pub async fn synthesize(
    binary: &Path,
    model: &Path,
    text: &str,
) -> Result<SpeechResult> {
    let clean = clean_for_speech(text);
    if clean.trim().is_empty() {
        return Err(OrionError::Config("nothing to speak after cleaning".into()));
    }

    let temp_dir = std::env::temp_dir();
    let temp_id = uuid::Uuid::new_v4();
    let out_path = temp_dir.join(format!("orion-tts-{}.wav", temp_id));

    let args = build_args(model, &out_path);
    let start = std::time::Instant::now();

    let mut cmd = Command::new(binary);
    cmd.args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }

    let mut child = cmd.spawn().map_err(|e| {
        OrionError::Config(format!(
            "could not launch Piper TTS at {}: {e}",
            binary.display()
        ))
    })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(clean.as_bytes())
            .await
            .map_err(|e| OrionError::Config(format!("failed to send text to Piper: {e}")))?;
        let _ = stdin.flush().await;
        drop(stdin);
    }

    let timeout_duration = std::time::Duration::from_secs(TTS_TIMEOUT_SECS);
    let status = match tokio::time::timeout(timeout_duration, child.wait()).await {
        Ok(res) => res.map_err(|e| OrionError::Config(format!("Piper execution error: {e}")))?,
        Err(_) => {
            let _ = child.kill().await;
            let _ = std::fs::remove_file(&out_path);
            return Err(OrionError::Config(format!(
                "Piper TTS timed out after {TTS_TIMEOUT_SECS} seconds"
            )));
        }
    };

    if !status.success() {
        let _ = std::fs::remove_file(&out_path);
        return Err(OrionError::Config(format!(
            "Piper TTS failed with exit code {:?}",
            status.code()
        )));
    }

    let wav_bytes = std::fs::read(&out_path)
        .map_err(|e| OrionError::Config(format!("could not read synthesized audio: {e}")))?;
    let _ = std::fs::remove_file(&out_path);

    Ok(SpeechResult {
        wav_bytes,
        elapsed_ms: start.elapsed().as_millis(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_code_blocks_from_speech() {
        let input = "Here is the code:\n```rust\nfn main() {}\n```\nIt works!";
        let cleaned = clean_for_speech(input);
        assert_eq!(cleaned, "Here is the code: It works!");
    }

    #[test]
    fn cleans_markdown_links_and_formatting() {
        let input = "Check out [Orion docs](https://example.com) for **important** details.";
        let cleaned = clean_for_speech(input);
        assert_eq!(cleaned, "Check out Orion docs for important details.");
    }

    #[test]
    fn drops_internal_citation_tags() {
        let input = "The revenue grew by 15% [chunk:42] in Q3.";
        let cleaned = clean_for_speech(input);
        assert_eq!(cleaned, "The revenue grew by 15% in Q3.");
    }

    #[test]
    fn builds_correct_cli_arguments() {
        let model = PathBuf::from("/models/voice.onnx");
        let out = PathBuf::from("/tmp/out.wav");
        let args = build_args(&model, &out);
        assert_eq!(
            args,
            vec!["--model", "/models/voice.onnx", "--output_file", "/tmp/out.wav"]
        );
    }
}
