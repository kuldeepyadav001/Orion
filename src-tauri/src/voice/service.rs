//! The voice service: owns the microphone, the listener state machine and
//! the transcriber, and reports what it is doing so the UI can show it.
//!
//! ## Why the status matters as much as the feature
//!
//! Voice is the one part of this product where the user cannot tell whether
//! it is working. A chat reply either appears or it does not. A microphone
//! that is not hearing you looks exactly like a microphone that is hearing
//! you and choosing not to respond — and an assistant that might be listening
//! is worse than one that clearly is not.
//!
//! So every state transition here is published, and the UI renders it:
//! whether the microphone is open, whether speech is being detected right
//! now, the live input level, and whether a transcription is running.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::error::{OrionError, Result};

use super::audio::{self, MAX_UTTERANCE_SECS};
use super::capture::{CaptureState, MicStream};
use super::listener::{ListenAction, ListenState, Listener};
use super::transcribe;

/// How much audio arrives per callback, used to advance the listener clock.
const BLOCK_MS: u32 = 100;

/// Everything the UI needs to render the voice indicator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceStatus {
    /// Off, waiting, armed, recording, transcribing.
    pub state: ListenState,
    /// True when the microphone is open at all.
    pub mic_open: bool,
    /// True when speech is being detected in this instant. This is what makes
    /// "it can hear me" visible rather than a matter of faith.
    pub hearing: bool,
    /// Input level, 0.0 to 1.0, for a live meter.
    pub level: f32,
    /// Speech captured so far in this utterance, milliseconds.
    pub speech_ms: u32,
    /// Human-readable explanation, shown on hover.
    pub detail: String,
    /// False when the binary or models are missing, so the UI can say why
    /// rather than showing a button that does nothing.
    pub available: bool,
}

impl Default for VoiceStatus {
    fn default() -> Self {
        Self {
            state: ListenState::Off,
            mic_open: false,
            hearing: false,
            level: 0.0,
            speech_ms: 0,
            detail: "Microphone off".into(),
            available: false,
        }
    }
}

/// Runtime state for voice input.
pub struct VoiceService {
    listener: Mutex<Listener>,
    capture: Arc<CaptureState>,
    status: Mutex<VoiceStatus>,
    /// Holding the stream keeps the microphone open; dropping it closes it.
    stream: Mutex<Option<MicStream>>,
}

impl Default for VoiceService {
    fn default() -> Self {
        Self::new()
    }
}

impl VoiceService {
    pub fn new() -> Self {
        Self {
            listener: Mutex::new(Listener::new(MAX_UTTERANCE_SECS)),
            capture: Arc::new(CaptureState::new()),
            status: Mutex::new(VoiceStatus::default()),
            stream: Mutex::new(None),
        }
    }

    pub async fn status(&self) -> VoiceStatus {
        self.status.lock().await.clone()
    }

    /// Is the whisper binary and at least one model present?
    ///
    /// Checked before the microphone is opened so a missing download produces
    /// an explanation rather than a button that silently does nothing.
    pub fn availability(models_dir: &std::path::Path, bin: &std::path::Path) -> (bool, String) {
        if !bin.is_file() {
            return (
                false,
                "Speech recognition is not installed. Run scripts/fetch-voice.sh".into(),
            );
        }
        if transcribe::find_whisper_model(models_dir).is_none() {
            return (
                false,
                "No speech model found. Run scripts/fetch-voice.sh".into(),
            );
        }
        (true, "Ready".into())
    }

    async fn set(&self, f: impl FnOnce(&mut VoiceStatus)) -> VoiceStatus {
        let mut s = self.status.lock().await;
        f(&mut s);
        s.clone()
    }

    /// Open the microphone and start listening.
    pub async fn start(&self, available: bool, detail: String) -> Result<VoiceStatus> {
        if !available {
            return Err(OrionError::Config(detail));
        }

        {
            let mut st = self.stream.lock().await;
            if st.is_none() {
                *st = Some(MicStream::open(Arc::clone(&self.capture))?);
            }
        }

        self.listener.lock().await.start_waiting();
        Ok(self
            .set(|s| {
                s.state = ListenState::Waiting;
                s.mic_open = true;
                s.available = true;
                s.detail = "Listening — speak to chat".into();
            })
            .await)
    }

    /// Close the microphone entirely.
    ///
    /// Dropping the stream is what actually releases the device, so the OS
    /// microphone indicator goes out. Anything less would leave the user
    /// unsure whether they are still being heard.
    pub async fn stop(&self) -> VoiceStatus {
        *self.stream.lock().await = None;
        self.listener.lock().await.stop();
        self.capture.abandon_utterance();
        self.set(|s| {
            s.state = ListenState::Off;
            s.mic_open = false;
            s.hearing = false;
            s.level = 0.0;
            s.speech_ms = 0;
            s.detail = "Microphone off".into();
        })
        .await
    }

    /// Begin capturing, as if the wake word had fired.
    pub async fn trigger(&self) -> VoiceStatus {
        let action = self.listener.lock().await.trigger();
        if action == ListenAction::BeginCapture {
            self.capture.begin_utterance();
            return self
                .set(|s| {
                    s.state = ListenState::Armed;
                    s.detail = "Go ahead".into();
                })
                .await;
        }
        self.status().await
    }

    /// Advance the listener with the current audio level.
    ///
    /// Returns captured samples when an utterance completes.
    pub async fn poll(&self, level: f32, has_speech: bool) -> (VoiceStatus, Option<Vec<f32>>) {
        let action = self.listener.lock().await.push_block(has_speech, BLOCK_MS);
        let state = self.listener.lock().await.state();
        let speech_ms = self.listener.lock().await.speech_ms();

        let mut captured = None;
        let detail = match action {
            ListenAction::FinishCapture => {
                captured = self.capture.take_utterance();
                "Transcribing…".to_string()
            }
            ListenAction::Abandon => {
                self.capture.abandon_utterance();
                "Listening — speak to chat".to_string()
            }
            ListenAction::BeginCapture => {
                self.capture.begin_utterance();
                "Hearing you…".to_string()
            }
            ListenAction::Continue => match state {
                ListenState::Recording if has_speech => "Hearing you…".to_string(),
                ListenState::Recording => "…".to_string(),
                ListenState::Armed => "Go ahead".to_string(),
                ListenState::Waiting => "Listening — speak to chat".to_string(),
                ListenState::Transcribing => "Transcribing…".to_string(),
                ListenState::Off => "Microphone off".to_string(),
            },
        };

        let status = self
            .set(|s| {
                s.state = state;
                s.hearing = has_speech && state != ListenState::Off;
                s.level = level.clamp(0.0, 1.0);
                s.speech_ms = speech_ms;
                s.detail = detail;
            })
            .await;

        (status, captured)
    }

    /// Transcription finished; go back to waiting.
    pub async fn finish(&self) -> VoiceStatus {
        self.listener.lock().await.finish_transcription();
        self.set(|s| {
            s.state = ListenState::Waiting;
            s.hearing = false;
            s.speech_ms = 0;
            s.detail = "Listening — speak to chat".into();
        })
        .await
    }

    /// Encode a finished utterance as a WAV file ready for whisper.
    pub fn to_wav(samples: &[f32]) -> Vec<u8> {
        audio::encode_wav_16k_mono(samples)
    }

    pub fn capture_state(&self) -> Arc<CaptureState> {
        Arc::clone(&self.capture)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn starts_with_the_microphone_closed() {
        // A voice assistant that opens the microphone before being asked is a
        // privacy problem, not a feature.
        let v = VoiceService::new();
        let s = v.status().await;
        assert_eq!(s.state, ListenState::Off);
        assert!(!s.mic_open);
        assert!(!s.hearing);
    }

    #[tokio::test]
    async fn refuses_to_start_when_whisper_is_missing() {
        // Better an explanation than a button that silently does nothing.
        let v = VoiceService::new();
        let r = v.start(false, "not installed".into()).await;
        assert!(r.is_err());
        assert!(!v.status().await.mic_open);
    }

    #[tokio::test]
    async fn availability_names_what_is_missing() {
        let dir = std::env::temp_dir().join(format!("orion-va-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let (ok, why) = VoiceService::availability(&dir, &dir.join("nope"));
        assert!(!ok);
        assert!(
            why.contains("fetch-voice"),
            "the message must say how to fix it: {why}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn hearing_reflects_speech_not_merely_being_open() {
        // This is the signal the user reads to know the microphone works.
        // If it were true whenever the mic was open it would be useless.
        let v = VoiceService::new();
        v.listener.lock().await.start_waiting();
        v.trigger().await;

        let (loud, _) = v.poll(0.4, true).await;
        assert!(loud.hearing, "speech should register as hearing");

        let (quiet, _) = v.poll(0.001, false).await;
        assert!(!quiet.hearing, "silence must not look like hearing");
    }

    #[tokio::test]
    async fn the_level_meter_is_bounded() {
        // A meter that can exceed 1.0 renders as a bar overflowing its track.
        let v = VoiceService::new();
        v.listener.lock().await.start_waiting();
        let (s, _) = v.poll(9.9, true).await;
        assert!(s.level <= 1.0);
        let (s2, _) = v.poll(-3.0, false).await;
        assert!(s2.level >= 0.0);
    }

    #[tokio::test]
    async fn a_finished_utterance_returns_its_audio() {
        let v = VoiceService::new();
        v.listener.lock().await.start_waiting();
        v.trigger().await;

        // Speak, then fall silent past the timeout.
        for _ in 0..6 {
            v.poll(0.5, true).await;
        }
        let mut got = None;
        for _ in 0..12 {
            let (_, captured) = v.poll(0.0, false).await;
            if captured.is_some() {
                got = captured;
                break;
            }
        }
        assert!(got.is_some(), "utterance was never handed back");
    }

    #[tokio::test]
    async fn stopping_closes_the_microphone_and_clears_audio() {
        let v = VoiceService::new();
        v.listener.lock().await.start_waiting();
        v.trigger().await;
        v.poll(0.5, true).await;

        let s = v.stop().await;
        assert!(!s.mic_open);
        assert!(!s.hearing);
        assert_eq!(s.state, ListenState::Off);
        assert!(
            v.capture.take_utterance().is_none(),
            "audio survived the microphone being closed"
        );
    }

    #[test]
    fn wav_output_is_what_whisper_expects() {
        let wav = VoiceService::to_wav(&[0.1; 1600]);
        assert_eq!(&wav[0..4], b"RIFF");
        let rate = u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]);
        assert_eq!(rate, 16_000, "whisper only accepts 16 kHz");
    }
}
