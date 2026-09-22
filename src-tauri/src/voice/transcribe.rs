//! Speech to text via whisper.cpp.
//!
//! ## Why the CLI and not the server
//!
//! `whisper-server` exists and speaks HTTP, which would match the
//! `llama-server` pattern already used here. The probe in
//! `docs/M4-VOICE-FINDINGS.md` rejected it on security grounds, and the
//! reasoning is worth keeping next to the code:
//!
//! * It has **no authentication at all**. `llama-server` takes `--api-key`
//!   and Orion generates a per-session token; `whisper-server --help` lists
//!   nothing for api, key or auth, and a request carrying a junk bearer token
//!   was answered with HTTP 200.
//! * It **does not honour `--host`**. Started with `--host 127.0.0.1` it also
//!   bound the LAN interface, and a POST to that address from off-loopback
//!   returned a transcript.
//!
//! Together that is an unauthenticated speech-to-text service exposed to the
//! local network — on café or campus Wi-Fi, anyone could send audio to it.
//! For a product whose entire claim is that your data never leaves the
//! machine, that cannot ship.
//!
//! `whisper-cli` opens no socket. The measured cost is ~75 ms of model load
//! per utterance against a ~1.8 s transcription, so roughly 4% overhead to
//! remove a network attack surface entirely. That is a good trade.
//!
//! If upstream gains real auth and a correct bind, revisit: the server keeps
//! the model warm, which would matter for a back-and-forth conversation.

use std::path::{Path, PathBuf};

use crate::error::{OrionError, Result};

/// Whisper models by tier, smallest first.
///
/// tiny.en is the T1 default. Measured at ~5-6x realtime on CPU with
/// transcription of the standard test clip exactly correct. base.en is a
/// tier-2 upgrade rather than a default: nearly double the memory for a
/// modest accuracy gain that speech commands rarely need.
pub const MODEL_TINY_EN: &str = "ggml-tiny.en.bin";
pub const MODEL_BASE_EN: &str = "ggml-base.en.bin";

/// Silero VAD, shipped as a whisper.cpp model.
///
/// The plan listed VAD as a separate component. It is not: whisper.cpp has it
/// built in via `--vad`. On the test clip it discarded 25% of the audio as
/// silence before transcription, which is a direct latency saving.
pub const MODEL_VAD: &str = "ggml-silero-v5.1.2.bin";

/// Where the models are downloaded from.
pub const WHISPER_MODEL_REPO: &str = "ggerganov/whisper.cpp";
pub const VAD_MODEL_REPO: &str = "ggml-org/whisper-vad";

/// Hard ceiling on how long a transcription may take before it is abandoned.
///
/// A 30-second utterance at the measured ~5x realtime should finish in about
/// 6 s. 60 s allows for a heavily loaded machine while still guaranteeing the
/// UI is never stuck forever.
pub const TRANSCRIBE_TIMEOUT_SECS: u64 = 60;

/// A completed transcription.
#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    pub text: String,
    /// Wall-clock time, so the UI can be honest about latency and the
    /// measured figures in the docs can be checked against real hardware.
    pub elapsed_ms: u128,
}

/// Locate a whisper model in the models directory.
pub fn find_whisper_model(models_dir: &Path) -> Option<PathBuf> {
    if let Ok(p) = std::env::var("ORION_WHISPER_MODEL_PATH") {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }

    // Prefer the smaller model. On the machine this targets the chat model
    // already holds 2.4 GB, so defaulting to something larger would be the
    // wrong call even though it is more accurate.
    for name in [MODEL_TINY_EN, MODEL_BASE_EN] {
        let p = models_dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }

    // Accept any ggml whisper model the user has fetched themselves, so a
    // deliberate choice is not overridden by a missing-file error.
    std::fs::read_dir(models_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            let n = p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            n.starts_with("ggml-") && n.ends_with(".bin") && !n.contains("silero")
        })
}

/// Locate the VAD model, which is optional.
pub fn find_vad_model(models_dir: &Path) -> Option<PathBuf> {
    let exact = models_dir.join(MODEL_VAD);
    if exact.is_file() {
        return Some(exact);
    }
    std::fs::read_dir(models_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.contains("silero"))
                .unwrap_or(false)
        })
}

/// Build the argument list for `whisper-cli`.
///
/// Separated from execution so the flags can be asserted in tests. Getting
/// these wrong fails quietly: a wrong sample rate or a missing `-nt` produces
/// plausible-looking but useless output rather than an error.
pub fn build_args(
    model: &Path,
    wav: &Path,
    vad_model: Option<&Path>,
    threads: usize,
) -> Vec<String> {
    let mut args = vec![
        "-m".into(),
        model.to_string_lossy().to_string(),
        "-f".into(),
        wav.to_string_lossy().to_string(),
        // No timestamps: the output is fed straight to the chat model as a
        // user message, and "[00:00:00.000 --> ...]" prefixes would be read
        // as part of the question.
        "-nt".into(),
        // English only. Auto-detection costs an extra pass and the models
        // shipped are the .en variants, which cannot do anything else.
        "-l".into(),
        "en".into(),
        "-t".into(),
        threads.to_string(),
        // Print to stdout only; no side-effect files next to the audio.
        "-np".into(),
    ];

    if let Some(vad) = vad_model {
        args.push("--vad".into());
        args.push("-vm".into());
        args.push(vad.to_string_lossy().to_string());
    }

    args
}

/// Clean up raw whisper output.
///
/// whisper emits leading spaces, newlines mid-sentence, and bracketed
/// annotations such as `[BLANK_AUDIO]`, `(silence)` or `[MUSIC]` for
/// non-speech. Passing those to the chat model produces replies about
/// blank audio, so they are stripped here rather than confusing the model.
pub fn clean_transcript(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut depth_square = 0usize;
    let mut depth_round = 0usize;

    for c in raw.chars() {
        match c {
            '[' => depth_square += 1,
            ']' => depth_square = depth_square.saturating_sub(1),
            '(' => depth_round += 1,
            ')' => depth_round = depth_round.saturating_sub(1),
            _ if depth_square == 0 && depth_round == 0 => out.push(c),
            _ => {}
        }
    }

    // Collapse all whitespace, including the newlines whisper inserts at
    // segment boundaries mid-sentence.
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// True when a transcript contains nothing worth sending to the model.
///
/// whisper returns an empty string, a lone piece of punctuation, or a
/// non-speech annotation for silence. Sending any of those wakes a 2.4 GB
/// model to answer nothing.
pub fn is_empty_transcript(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return true;
    }
    !t.chars().any(|c| c.is_alphanumeric())
}

/// Run whisper-cli over a WAV file and return the transcript.
///
/// The binary is invoked directly rather than through Tauri's sidecar API
/// because this is a short-lived one-shot process, not a long-running service
/// that needs registering for shutdown.
pub async fn transcribe_file(
    whisper_cli: &Path,
    model: &Path,
    wav: &Path,
    vad_model: Option<&Path>,
    threads: usize,
) -> Result<Transcript> {
    let started = std::time::Instant::now();
    let args = build_args(model, wav, vad_model, threads);

    let mut cmd = tokio::process::Command::new(whisper_cli);
    cmd.args(&args);

    // Keep the console window hidden on Windows. Without this, every spoken
    // sentence flashes a black box on screen.
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    // The DLLs and .so files sit beside the binary.
    if let Some(dir) = whisper_cli.parent() {
        let key = if cfg!(windows) {
            "PATH"
        } else {
            "LD_LIBRARY_PATH"
        };
        let existing = std::env::var(key).unwrap_or_default();
        let sep = if cfg!(windows) { ";" } else { ":" };
        cmd.env(key, format!("{}{}{}", dir.display(), sep, existing));
    }

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(TRANSCRIBE_TIMEOUT_SECS),
        cmd.output(),
    )
    .await
    .map_err(|_| {
        OrionError::Engine(format!(
            "transcription took longer than {TRANSCRIBE_TIMEOUT_SECS}s and was abandoned"
        ))
    })?
    .map_err(|e| OrionError::Engine(format!("could not run whisper-cli: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // whisper is extremely chatty on stderr even on success, so only the
        // tail is useful in an error message.
        let tail: String = stderr.lines().rev().take(3).collect::<Vec<_>>().join(" | ");
        return Err(OrionError::Engine(format!("whisper-cli failed: {tail}")));
    }

    let text = clean_transcript(&String::from_utf8_lossy(&output.stdout));

    Ok(Transcript {
        text,
        elapsed_ms: started.elapsed().as_millis(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /* ---------- argument construction ---------- */

    #[test]
    fn args_request_no_timestamps() {
        // Timestamps would be sent to the chat model as part of the question.
        let args = build_args(Path::new("m.bin"), Path::new("a.wav"), None, 4);
        assert!(args.contains(&"-nt".to_string()));
    }

    #[test]
    fn args_include_the_model_and_the_audio() {
        let args = build_args(Path::new("model.bin"), Path::new("audio.wav"), None, 4);
        assert!(args.contains(&"model.bin".to_string()));
        assert!(args.contains(&"audio.wav".to_string()));
    }

    #[test]
    fn vad_is_only_requested_when_a_model_exists() {
        // Passing --vad without -vm makes whisper-cli fail outright, so an
        // optional model must not half-enable the flag.
        let without = build_args(Path::new("m.bin"), Path::new("a.wav"), None, 4);
        assert!(!without.contains(&"--vad".to_string()));

        let with = build_args(
            Path::new("m.bin"),
            Path::new("a.wav"),
            Some(Path::new("silero.bin")),
            4,
        );
        assert!(with.contains(&"--vad".to_string()));
        assert!(with.contains(&"-vm".to_string()));
        assert!(with.contains(&"silero.bin".to_string()));
    }

    #[test]
    fn thread_count_is_passed_through() {
        let args = build_args(Path::new("m.bin"), Path::new("a.wav"), None, 3);
        let i = args.iter().position(|a| a == "-t").expect("-t missing");
        assert_eq!(args[i + 1], "3");
    }

    /* ---------- transcript cleaning ---------- */

    #[test]
    fn leading_space_and_newlines_are_removed() {
        // Exactly the shape whisper returned in the probe.
        let raw = " And so my fellow Americans ask not what your country can do\n for you.\n";
        assert_eq!(
            clean_transcript(raw),
            "And so my fellow Americans ask not what your country can do for you."
        );
    }

    #[test]
    fn non_speech_annotations_are_stripped() {
        // Otherwise the model is asked to respond to "[BLANK_AUDIO]".
        assert_eq!(clean_transcript("[BLANK_AUDIO]"), "");
        assert_eq!(clean_transcript("(silence)"), "");
        assert_eq!(clean_transcript("[MUSIC] hello there"), "hello there");
        assert_eq!(
            clean_transcript("what time is it [BLANK_AUDIO]"),
            "what time is it"
        );
    }

    #[test]
    fn ordinary_punctuation_survives() {
        // Over-eager stripping would mangle real speech.
        let s = clean_transcript("What's the weather? It's 20 degrees - warm!");
        assert_eq!(s, "What's the weather? It's 20 degrees - warm!");
    }

    #[test]
    fn unbalanced_brackets_do_not_swallow_the_rest() {
        // A stray bracket must not discard the whole transcript.
        assert_eq!(clean_transcript("hello ) world"), "hello world");
    }

    #[test]
    fn cleaning_empty_input_is_safe() {
        assert_eq!(clean_transcript(""), "");
        assert_eq!(clean_transcript("   \n  \n "), "");
    }

    /* ---------- empty detection ---------- */

    #[test]
    fn silence_is_recognised_as_empty() {
        // Each of these wakes a 2.4 GB model for nothing if not caught.
        assert!(is_empty_transcript(""));
        assert!(is_empty_transcript("   "));
        assert!(is_empty_transcript("."));
        assert!(is_empty_transcript("..."));
        assert!(is_empty_transcript("[ ]"));
        assert!(is_empty_transcript("- -"));
    }

    #[test]
    fn real_speech_is_not_discarded() {
        assert!(!is_empty_transcript("hello"));
        assert!(!is_empty_transcript("yes"));
        assert!(!is_empty_transcript("42"));
        // Short answers matter: "no." must reach the model.
        assert!(!is_empty_transcript("no."));
    }

    /* ---------- model discovery ---------- */

    #[test]
    fn a_missing_model_is_reported_not_assumed() {
        std::env::set_var("ORION_WHISPER_MODEL_PATH", "/nonexistent/model.bin");
        assert!(find_whisper_model(Path::new("/tmp")).is_none());
        std::env::remove_var("ORION_WHISPER_MODEL_PATH");
    }

    #[test]
    fn the_smaller_model_is_preferred() {
        // On a 6 GiB machine already holding 2.4 GB of chat model, defaulting
        // to the larger model would be the wrong call even though it is more
        // accurate.
        let dir = std::env::temp_dir().join(format!("orion-wm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(MODEL_BASE_EN), b"x").unwrap();
        std::fs::write(dir.join(MODEL_TINY_EN), b"x").unwrap();

        let found = find_whisper_model(&dir).unwrap();
        assert_eq!(found.file_name().unwrap(), MODEL_TINY_EN);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_vad_model_is_not_mistaken_for_a_speech_model() {
        // Both are ggml-*.bin in the same directory. Passing silero as the
        // transcription model produces garbage rather than an error.
        let dir = std::env::temp_dir().join(format!("orion-vm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(MODEL_VAD), b"x").unwrap();

        assert!(
            find_whisper_model(&dir).is_none(),
            "silero must never be selected as the speech model"
        );
        assert!(find_vad_model(&dir).is_some());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /* ---------- the security decision ---------- */

    #[test]
    fn transcription_does_not_use_the_http_server() {
        // whisper-server has no authentication and binds beyond loopback
        // despite --host; see the module docs and M4-VOICE-FINDINGS.md.
        // Structural guard so a future change cannot quietly reintroduce an
        // unauthenticated network service into an offline-first product.
        // Production code only, comments stripped. The module docs explain
        // at length why the HTTP server was rejected, and this test's own
        // failure message names it — both matched in earlier versions.
        let full = include_str!("transcribe.rs");
        let prod = full.split("mod tests").next().unwrap_or(full);
        let code: String = prod
            .lines()
            .filter(|l| {
                let t = l.trim_start();
                !t.starts_with("//") && !t.starts_with("///")
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            !code.contains("whisper-server"),
            "the transcriber must not spawn whisper-server: it has no auth \
             and does not honour --host"
        );
        assert!(
            !code.contains("/inference"),
            "no HTTP endpoint should be called for transcription"
        );
    }
}
