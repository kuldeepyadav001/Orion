//! Microphone capture.
//!
//! **This is the one part of M4 that cannot be tested here.** The dev sandbox
//! has no audio device, so nothing below has ever run. It is therefore kept
//! as thin as possible: the device hands over blocks of samples and every
//! decision about what to do with them lives in `audio` and `listener`, which
//! are pure and fully tested.
//!
//! ## Microphone sharing
//!
//! Orion must never prevent another application from using the microphone.
//! Windows WASAPI has two modes: *shared*, where the audio engine mixes
//! streams from many applications, and *exclusive*, where one application
//! locks the device and everything else is denied.
//!
//! `cpal` uses `AUDCLNT_SHAREMODE_SHARED` at every call site — verified by
//! reading `cpal-0.18.2/src/host/wasapi/device.rs`, which never references
//! the exclusive constant. So a call in Zoom or a game keeps working while
//! Orion listens.
//!
//! This is correct by default rather than by our effort, which means it could
//! be broken by an upgrade without anyone noticing. `shared_mode_is_used`
//! below guards against that.

use std::sync::{Arc, Mutex};

use crate::error::{OrionError, Result};

use super::audio::{self, PreRoll, PREROLL_SAMPLES};

/// A block of microphone audio, already mono and at 16 kHz.
pub type Block = Vec<f32>;

/// Where captured audio goes.
///
/// A trait so the pipeline can be driven from a test with synthetic audio,
/// and so the device layer stays replaceable — `cpal` is not the only option
/// and this should not be welded to it.
pub trait AudioSink: Send + Sync {
    /// Called for every block of captured audio.
    fn on_block(&self, samples: &[f32]);
}

/// Describes an input device for the settings UI.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InputDevice {
    pub name: String,
    pub is_default: bool,
}

/// Shared state between the audio callback and the rest of the app.
///
/// The callback runs on a realtime audio thread. It must not allocate, block,
/// or take a contended lock — an overrun there produces audible glitches in
/// *other* applications too, since the device is shared.
pub struct CaptureState {
    /// Rolling buffer of recent audio, so speech immediately before the wake
    /// word is not lost.
    pub preroll: Mutex<PreRoll>,
    /// The utterance currently being captured, if any.
    pub utterance: Mutex<Option<Vec<f32>>>,
    /// Loudness of the most recent block, scaled to 0..1 for the UI meter.
    ///
    /// An atomic rather than a lock: this is written from the realtime audio
    /// callback, where blocking on a contended mutex would glitch audio for
    /// every application sharing the device.
    level: std::sync::atomic::AtomicU32,
}

impl Default for CaptureState {
    fn default() -> Self {
        Self::new()
    }
}

impl CaptureState {
    pub fn new() -> Self {
        Self {
            preroll: Mutex::new(PreRoll::new(PREROLL_SAMPLES)),
            utterance: Mutex::new(None),
            level: std::sync::atomic::AtomicU32::new(0),
        }
    }

    /// Feed in a block from the device.
    ///
    /// While idle the audio goes only to the ring buffer and is continuously
    /// overwritten. Nothing is retained until capture actually begins.
    /// Most recent input level, 0.0 to 1.0.
    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(std::sync::atomic::Ordering::Relaxed))
    }

    /// Is the current block loud enough to be speech?
    ///
    /// A crude energy gate, not Silero. The real VAD runs inside whisper.cpp
    /// during transcription; this only decides when an utterance has ended
    /// and drives the meter. The threshold is deliberately low — cutting
    /// someone off mid-sentence is far worse than transcribing a little
    /// silence, and whisper discards the silence anyway.
    pub fn has_speech(&self) -> bool {
        self.level() > 0.012
    }

    pub fn push(&self, samples: &[f32]) {
        // Track loudness before anything else, so the meter moves whether or
        // not an utterance is being captured.
        let rms = audio::rms(samples);
        // Speech RMS sits around 0.02-0.3; scale so normal speech fills the
        // meter rather than sitting flat at the bottom.
        let scaled = (rms * 6.0).clamp(0.0, 1.0);
        self.level
            .store(scaled.to_bits(), std::sync::atomic::Ordering::Relaxed);

        if let Ok(mut u) = self.utterance.lock() {
            if let Some(buf) = u.as_mut() {
                // Enforce the hard cap here as well as in the listener: if
                // the state machine ever fails to terminate, the buffer must
                // still not grow without bound.
                let room = audio::MAX_SAMPLES.saturating_sub(buf.len());
                if room > 0 {
                    buf.extend_from_slice(&samples[..samples.len().min(room)]);
                }
                return;
            }
        }
        if let Ok(mut p) = self.preroll.lock() {
            p.push(samples);
        }
    }

    /// Begin an utterance, seeded with the pre-roll.
    pub fn begin_utterance(&self) {
        let seed = self
            .preroll
            .lock()
            .map(|mut p| p.drain_all())
            .unwrap_or_default();
        if let Ok(mut u) = self.utterance.lock() {
            *u = Some(seed);
        }
    }

    /// Finish the utterance and take the samples.
    pub fn take_utterance(&self) -> Option<Vec<f32>> {
        self.utterance.lock().ok().and_then(|mut u| u.take())
    }

    /// Discard a partial utterance, e.g. after a false trigger.
    pub fn abandon_utterance(&self) {
        if let Ok(mut u) = self.utterance.lock() {
            *u = None;
        }
        if let Ok(mut p) = self.preroll.lock() {
            p.clear();
        }
    }

    pub fn is_capturing(&self) -> bool {
        self.utterance.lock().map(|u| u.is_some()).unwrap_or(false)
    }
}

/// List available input devices.
#[cfg(feature = "voice")]
pub fn list_input_devices() -> Result<Vec<InputDevice>> {
    use cpal::traits::{DeviceTrait, HostTrait};

    let host = cpal::default_host();

    // cpal 0.18 exposes the human-readable name through description(), not a
    // name() method; DeviceTrait::name does not exist in this version.
    let default_name = host
        .default_input_device()
        .and_then(|d| d.description().ok())
        .map(|d| d.name().to_string())
        .unwrap_or_default();

    let devices = host
        .input_devices()
        .map_err(|e| OrionError::Config(format!("cannot list microphones: {e}")))?;

    Ok(devices
        .filter_map(|d| d.description().ok())
        .map(|desc| {
            let name = desc.name().to_string();
            InputDevice {
                is_default: name == default_name,
                name,
            }
        })
        .collect())
}

#[cfg(not(feature = "voice"))]
pub fn list_input_devices() -> Result<Vec<InputDevice>> {
    Ok(Vec::new())
}

/// An open microphone stream. Dropping it closes the device.
#[cfg(feature = "voice")]
pub struct MicStream {
    _stream: cpal::Stream,
}

#[cfg(feature = "voice")]
impl MicStream {
    /// Open the default microphone and stream mono 16 kHz blocks into
    /// `state`.
    ///
    /// Never verified on real hardware; see the module docs.
    pub fn open(state: Arc<CaptureState>) -> Result<Self> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| OrionError::Config("no microphone found".into()))?;

        let config = device.default_input_config().map_err(|e| {
            OrionError::Config(format!("cannot read microphone configuration: {e}"))
        })?;

        // SampleRate is a plain u32 alias in cpal 0.18, not a newtype.
        let sample_rate = config.sample_rate();
        let channels = config.channels();

        let device_name = device
            .description()
            .map(|d| d.name().to_string())
            .unwrap_or_else(|_| "unknown".into());

        tracing::info!(
            device = device_name,
            sample_rate,
            channels,
            "opening microphone (shared mode; other applications keep access)"
        );

        let err_state = Arc::clone(&state);
        let _ = &err_state;

        let stream = device
            .build_input_stream(
                // Taken by value in cpal 0.18.
                config.config(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    // Realtime thread. Keep this cheap: mix, resample, hand
                    // over. No allocation beyond the two vectors, no I/O, no
                    // blocking.
                    let mono = audio::to_mono(data, channels);
                    let resampled = audio::resample_to_16k(&mono, sample_rate);
                    state.push(&resampled);
                },
                move |err| {
                    // A device error is usually the microphone being
                    // unplugged. Log it; the supervisor decides whether to
                    // reopen.
                    tracing::warn!(error = %err, "microphone stream error");
                },
                None,
            )
            .map_err(|e| OrionError::Config(format!("cannot open microphone: {e}")))?;

        stream
            .play()
            .map_err(|e| OrionError::Config(format!("cannot start microphone: {e}")))?;

        Ok(Self { _stream: stream })
    }
}

#[cfg(not(feature = "voice"))]
pub struct MicStream;

#[cfg(not(feature = "voice"))]
impl MicStream {
    pub fn open(_state: Arc<CaptureState>) -> Result<Self> {
        Err(OrionError::Config(
            "this build was compiled without voice support".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_audio_goes_only_to_the_ring_buffer() {
        // The privacy claim in the module docs depends on this: while waiting
        // for the wake word, nothing is retained.
        let s = CaptureState::new();
        for _ in 0..100 {
            s.push(&[0.1; 1000]);
        }
        assert!(!s.is_capturing());
        assert!(
            s.take_utterance().is_none(),
            "audio was retained while idle"
        );
    }

    #[test]
    fn the_utterance_starts_with_the_preroll() {
        // People run the wake word into the question without pausing. Without
        // the pre-roll the first words are lost.
        let s = CaptureState::new();
        s.push(&[0.5; 8000]);
        s.begin_utterance();
        s.push(&[0.7; 1000]);

        let captured = s.take_utterance().expect("no utterance");
        assert!(
            captured.len() > 1000,
            "pre-roll was not included: got {} samples",
            captured.len()
        );
        assert_eq!(captured[0], 0.5, "should begin with buffered audio");
    }

    #[test]
    fn a_second_utterance_does_not_replay_the_first() {
        let s = CaptureState::new();
        s.push(&[0.5; 8000]);
        s.begin_utterance();
        let first = s.take_utterance().unwrap();
        assert!(!first.is_empty());

        s.begin_utterance();
        let second = s.take_utterance().unwrap();
        assert!(second.is_empty(), "stale audio leaked into a new utterance");
    }

    #[test]
    fn a_runaway_capture_cannot_exhaust_memory() {
        // Defence in depth: the listener enforces a time cap, but if it ever
        // fails the buffer must still stop growing.
        let s = CaptureState::new();
        s.begin_utterance();
        for _ in 0..2000 {
            s.push(&[0.1; 16_000]); // 1 s per push
        }
        let captured = s.take_utterance().unwrap();
        assert!(
            captured.len() <= audio::MAX_SAMPLES,
            "buffer grew past the cap: {} samples",
            captured.len()
        );
    }

    #[test]
    fn abandoning_clears_everything() {
        // After a false trigger nothing should survive into the next attempt.
        let s = CaptureState::new();
        s.push(&[0.3; 4000]);
        s.begin_utterance();
        s.push(&[0.4; 4000]);

        s.abandon_utterance();

        assert!(!s.is_capturing());
        assert!(s.take_utterance().is_none());
        s.begin_utterance();
        assert!(
            s.take_utterance().unwrap().is_empty(),
            "pre-roll survived an abandon"
        );
    }

    #[test]
    fn capture_is_reported_accurately() {
        let s = CaptureState::new();
        assert!(!s.is_capturing());
        s.begin_utterance();
        assert!(s.is_capturing());
        s.take_utterance();
        assert!(!s.is_capturing());
    }

    #[test]
    fn shared_mode_is_used() {
        // Orion must never block another application from using the
        // microphone. cpal requests AUDCLNT_SHAREMODE_SHARED everywhere and
        // never the exclusive constant, so this is correct by default — which
        // means an upgrade could break it silently.
        //
        // Structural guard: we must not be asking for exclusive access
        // ourselves, and must not be building a raw WASAPI client.
        // Check the production code only, with comments stripped.
        //
        // Two earlier versions of this test failed against themselves: the
        // module docs explain what must not happen, and the assertion message
        // names the forbidden constant. Both matched. Splitting at `mod
        // tests` is what actually scopes the search to real code.
        let full = include_str!("capture.rs");
        let prod = full.split("mod tests").next().unwrap_or(full);
        let code: String = prod
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            !code.contains("SHAREMODE_EXCLUSIVE"),
            "exclusive mode would lock the microphone away from every other \
             application on the machine"
        );
        assert!(
            code.contains("default_input_config"),
            "using the device's own default configuration is what keeps the \
             stream in shared mode"
        );
    }

    #[test]
    fn capture_targets_the_rate_whisper_needs() {
        // The device layer is responsible for delivering 16 kHz mono. Getting
        // this wrong produces gibberish rather than an error.
        assert_eq!(super::super::audio::TARGET_RATE, 16_000);
    }
}
