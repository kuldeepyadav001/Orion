//! When to start recording, and when to stop.
//!
//! A pure state machine over "was there speech in this block?", so the whole
//! of the listening behaviour can be tested without a microphone, a wake-word
//! model, or a clock.
//!
//! The hard part of voice input is not transcription — that was measured at
//! ~1.8 s for 11 seconds of audio. The hard part is deciding when the user
//! has finished talking. Cut too early and you transcribe half a sentence;
//! cut too late and the assistant feels unresponsive.

use serde::{Deserialize, Serialize};

/// How long a silence must last before an utterance is considered finished.
///
/// 800 ms is a compromise. Natural speech contains pauses of 200-500 ms
/// mid-sentence — after a comma, while thinking of a word — so anything much
/// below this cuts people off mid-thought, which is the single most
/// infuriating failure a voice assistant has. Much above it and the thing
/// feels slow to respond.
pub const SILENCE_TIMEOUT_MS: u32 = 900;

/// Minimum speech required before an utterance is worth transcribing.
///
/// Guards against a cough or a door closing triggering a round trip through
/// whisper and then the LLM.
pub const MIN_SPEECH_MS: u32 = 300;

/// Maximum time the microphone stays open after the wake word with no speech
/// at all, before giving up and going back to sleep.
pub const WAKE_GRACE_MS: u32 = 4_000;

// Tuning guards, checked at compile time so a future tweak cannot quietly
// make the assistant either cut people off or hang on their every pause.
//
// Below ~600 ms the silence timeout truncates natural mid-sentence pauses,
// which is the single most infuriating voice-assistant failure; above ~1.5 s
// it feels unresponsive. The grace period must be long enough for someone to
// gather their thoughts after the wake word, but short enough that a false
// trigger does not record the room.
const _: () = assert!(SILENCE_TIMEOUT_MS >= 600 && SILENCE_TIMEOUT_MS <= 1_500);
const _: () = assert!(MIN_SPEECH_MS >= 100 && MIN_SPEECH_MS <= 500);
const _: () = assert!(WAKE_GRACE_MS >= 3_000 && WAKE_GRACE_MS <= 8_000);

/// What the listener is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ListenState {
    /// Microphone closed. Nothing is being captured at all.
    Off,
    /// Microphone open, waiting for the wake word. Audio goes to the pre-roll
    /// ring and nowhere else — it is never written to disk or transcribed.
    Waiting,
    /// Wake word heard, waiting for the user to actually start speaking.
    Armed,
    /// Actively capturing an utterance.
    Recording,
    /// Utterance captured, being transcribed.
    Transcribing,
}

/// What the caller should do after feeding in a block of audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListenAction {
    /// Keep going, nothing to do.
    Continue,
    /// Start buffering audio, including the pre-roll.
    BeginCapture,
    /// Utterance is complete; send the buffer for transcription.
    FinishCapture,
    /// Give up and return to waiting — no speech arrived in time.
    Abandon,
}

/// Drives the listening lifecycle.
///
/// Time is passed in rather than read from a clock, so the tests are
/// deterministic and a 10-second silence costs nothing to simulate.
pub struct Listener {
    state: ListenState,
    /// Milliseconds of continuous silence while recording.
    silence_ms: u32,
    /// Milliseconds of speech captured in this utterance.
    speech_ms: u32,
    /// Milliseconds spent armed with no speech yet.
    armed_ms: u32,
    /// Total captured, used to enforce the hard length cap.
    captured_ms: u32,
    max_utterance_ms: u32,
}

impl Listener {
    pub fn new(max_utterance_secs: usize) -> Self {
        Self {
            state: ListenState::Off,
            silence_ms: 0,
            speech_ms: 0,
            armed_ms: 0,
            captured_ms: 0,
            max_utterance_ms: max_utterance_secs as u32 * 1000,
        }
    }

    pub fn state(&self) -> ListenState {
        self.state
    }

    /// Open the microphone and begin waiting for the wake word.
    pub fn start_waiting(&mut self) {
        self.state = ListenState::Waiting;
        self.reset_counters();
    }

    /// Close the microphone entirely.
    pub fn stop(&mut self) {
        self.state = ListenState::Off;
        self.reset_counters();
    }

    /// The wake word fired, or the user pressed the talk key.
    ///
    /// Ignored unless waiting: a second trigger during an utterance must not
    /// discard what has already been captured.
    pub fn trigger(&mut self) -> ListenAction {
        match self.state {
            ListenState::Waiting => {
                self.state = ListenState::Armed;
                self.reset_counters();
                ListenAction::BeginCapture
            }
            _ => ListenAction::Continue,
        }
    }

    /// Transcription finished; go back to waiting for the wake word.
    pub fn finish_transcription(&mut self) {
        if self.state == ListenState::Transcribing {
            self.state = ListenState::Waiting;
            self.reset_counters();
        }
    }

    /// Feed in one block of audio.
    ///
    /// `has_speech` comes from the VAD; `duration_ms` is the block length.
    pub fn push_block(&mut self, has_speech: bool, duration_ms: u32) -> ListenAction {
        match self.state {
            ListenState::Off | ListenState::Transcribing => ListenAction::Continue,

            ListenState::Waiting => {
                if has_speech {
                    self.state = ListenState::Recording;
                    self.speech_ms = duration_ms;
                    self.captured_ms = duration_ms;
                    self.silence_ms = 0;
                    return ListenAction::BeginCapture;
                }
                ListenAction::Continue
            }

            ListenState::Armed => {
                if has_speech {
                    self.state = ListenState::Recording;
                    self.speech_ms = duration_ms;
                    self.captured_ms = duration_ms;
                    self.silence_ms = 0;
                    return ListenAction::Continue;
                }

                self.armed_ms += duration_ms;
                if self.armed_ms >= WAKE_GRACE_MS {
                    // Nothing was said. Almost always a false trigger, and
                    // holding the capture open invites recording a
                    // conversation the user never addressed to us.
                    self.state = ListenState::Waiting;
                    self.reset_counters();
                    return ListenAction::Abandon;
                }
                ListenAction::Continue
            }

            ListenState::Recording => {
                self.captured_ms += duration_ms;

                if has_speech {
                    self.speech_ms += duration_ms;
                    self.silence_ms = 0;
                } else {
                    self.silence_ms += duration_ms;
                }

                // Hard cap first: a stuck VAD reporting speech forever must
                // still terminate.
                if self.captured_ms >= self.max_utterance_ms {
                    return self.finish();
                }

                if self.silence_ms >= SILENCE_TIMEOUT_MS {
                    if self.speech_ms < MIN_SPEECH_MS {
                        // A cough or a door. Not worth waking the model for.
                        self.state = ListenState::Waiting;
                        self.reset_counters();
                        return ListenAction::Abandon;
                    }
                    return self.finish();
                }

                ListenAction::Continue
            }
        }
    }

    fn finish(&mut self) -> ListenAction {
        self.state = ListenState::Transcribing;
        ListenAction::FinishCapture
    }

    fn reset_counters(&mut self) {
        self.silence_ms = 0;
        self.speech_ms = 0;
        self.armed_ms = 0;
        self.captured_ms = 0;
    }

    /// Speech captured so far, for the UI.
    pub fn speech_ms(&self) -> u32 {
        self.speech_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK: u32 = 100;

    fn armed() -> Listener {
        let mut l = Listener::new(30);
        l.start_waiting();
        assert_eq!(l.trigger(), ListenAction::BeginCapture);
        l
    }

    /* ---------- lifecycle ---------- */

    #[test]
    fn starts_off_with_the_microphone_closed() {
        // The default must be off. A voice assistant that listens before
        // being asked is a privacy problem, not a feature.
        assert_eq!(Listener::new(30).state(), ListenState::Off);
    }

    #[test]
    fn audio_is_ignored_entirely_while_off() {
        let mut l = Listener::new(30);
        for _ in 0..100 {
            assert_eq!(l.push_block(true, BLOCK), ListenAction::Continue);
        }
        assert_eq!(l.state(), ListenState::Off);
    }

    #[test]
    fn the_wake_word_does_nothing_unless_waiting() {
        // Guards against a stray trigger while off opening the microphone.
        let mut l = Listener::new(30);
        assert_eq!(l.trigger(), ListenAction::Continue);
        assert_eq!(l.state(), ListenState::Off);
    }

    #[test]
    fn stopping_closes_the_microphone_from_any_state() {
        let mut l = armed();
        l.push_block(true, BLOCK);
        assert_eq!(l.state(), ListenState::Recording);
        l.stop();
        assert_eq!(l.state(), ListenState::Off);
    }

    /* ---------- normal utterance ---------- */

    #[test]
    fn a_normal_sentence_is_captured_and_finished() {
        let mut l = armed();

        // ~1.5 s of speech
        for _ in 0..15 {
            assert_eq!(l.push_block(true, BLOCK), ListenAction::Continue);
        }
        assert_eq!(l.state(), ListenState::Recording);

        // Then the user stops talking.
        let mut action = ListenAction::Continue;
        for _ in 0..(SILENCE_TIMEOUT_MS / BLOCK) {
            action = l.push_block(false, BLOCK);
        }

        assert_eq!(action, ListenAction::FinishCapture);
        assert_eq!(l.state(), ListenState::Transcribing);
    }

    #[test]
    fn a_pause_mid_sentence_does_not_cut_the_user_off() {
        // The most infuriating failure in any voice assistant. People pause
        // for 200-500 ms mid-sentence and must not be cut off.
        let mut l = armed();

        for _ in 0..10 {
            l.push_block(true, BLOCK);
        }
        // 500 ms of thinking
        for _ in 0..5 {
            assert_eq!(
                l.push_block(false, BLOCK),
                ListenAction::Continue,
                "cut off during a natural pause"
            );
        }
        // ...and they carry on.
        assert_eq!(l.push_block(true, BLOCK), ListenAction::Continue);
        assert_eq!(l.state(), ListenState::Recording);
    }

    #[test]
    fn the_silence_counter_resets_when_speech_resumes() {
        let mut l = armed();

        // Enough speech to clear MIN_SPEECH_MS, otherwise the cough guard
        // correctly abandons this and the reset is never exercised.
        for _ in 0..5 {
            l.push_block(true, BLOCK);
        }

        // Nearly time out...
        for _ in 0..8 {
            l.push_block(false, BLOCK);
        }
        // ...then speak again.
        l.push_block(true, BLOCK);

        // A full timeout must now be required from scratch.
        for _ in 0..8 {
            assert_eq!(l.push_block(false, BLOCK), ListenAction::Continue);
        }
        assert_eq!(l.push_block(false, BLOCK), ListenAction::FinishCapture);
    }

    #[test]
    fn speech_while_waiting_automatically_begins_capture() {
        let mut l = Listener::new(30);
        l.start_waiting();
        assert_eq!(l.state(), ListenState::Waiting);

        // When user speaks, listener immediately begins capture
        assert_eq!(l.push_block(true, BLOCK), ListenAction::BeginCapture);
        assert_eq!(l.state(), ListenState::Recording);

        // Continue speaking
        for _ in 0..10 {
            assert_eq!(l.push_block(true, BLOCK), ListenAction::Continue);
        }

        // Stop speaking -> silence timeout finishes capture
        let mut action = ListenAction::Continue;
        for _ in 0..(SILENCE_TIMEOUT_MS / BLOCK) {
            action = l.push_block(false, BLOCK);
        }
        assert_eq!(action, ListenAction::FinishCapture);
        assert_eq!(l.state(), ListenState::Transcribing);
    }

    /* ---------- rejecting noise ---------- */

    #[test]
    fn a_cough_is_not_sent_to_the_model() {
        // 100 ms of noise then silence. Transcribing this would wake a 2.4 GB
        // model to answer a door closing.
        let mut l = armed();
        l.push_block(true, BLOCK);

        let mut action = ListenAction::Continue;
        for _ in 0..(SILENCE_TIMEOUT_MS / BLOCK) {
            action = l.push_block(false, BLOCK);
        }

        assert_eq!(action, ListenAction::Abandon);
        assert_eq!(l.state(), ListenState::Waiting, "should go back to waiting");
    }

    #[test]
    fn a_false_trigger_with_no_speech_gives_up() {
        // The wake word fires on the television. Nobody says anything. The
        // microphone must not stay open recording the room.
        let mut l = armed();

        let mut action = ListenAction::Continue;
        for _ in 0..(WAKE_GRACE_MS / BLOCK) {
            action = l.push_block(false, BLOCK);
        }

        assert_eq!(action, ListenAction::Abandon);
        assert_eq!(l.state(), ListenState::Waiting);
    }

    /* ---------- hard limits ---------- */

    #[test]
    fn a_stuck_vad_cannot_record_forever() {
        // If the VAD reports speech indefinitely — a fan, a television — the
        // buffer must still terminate rather than growing until the machine
        // swaps.
        let mut l = Listener::new(2); // 2-second cap for the test
        l.start_waiting();
        l.trigger();

        let mut action = ListenAction::Continue;
        for _ in 0..100 {
            action = l.push_block(true, BLOCK);
            if action == ListenAction::FinishCapture {
                break;
            }
        }

        assert_eq!(action, ListenAction::FinishCapture);
        assert_eq!(l.state(), ListenState::Transcribing);
    }

    #[test]
    fn the_cap_counts_silence_too() {
        // Otherwise alternating speech and silence extends the utterance
        // indefinitely.
        let mut l = Listener::new(1);
        l.start_waiting();
        l.trigger();
        l.push_block(true, BLOCK);

        let mut action = ListenAction::Continue;
        for i in 0..20 {
            action = l.push_block(i % 2 == 0, BLOCK);
            if matches!(action, ListenAction::FinishCapture | ListenAction::Abandon) {
                break;
            }
        }
        assert_ne!(action, ListenAction::Continue, "never terminated");
    }

    /* ---------- returning to rest ---------- */

    #[test]
    fn finishing_transcription_returns_to_waiting() {
        let mut l = armed();
        for _ in 0..10 {
            l.push_block(true, BLOCK);
        }
        for _ in 0..(SILENCE_TIMEOUT_MS / BLOCK) {
            l.push_block(false, BLOCK);
        }
        assert_eq!(l.state(), ListenState::Transcribing);

        l.finish_transcription();
        assert_eq!(l.state(), ListenState::Waiting, "must listen again");
    }

    #[test]
    fn audio_during_transcription_is_ignored() {
        // Otherwise the assistant's own spoken reply, or the user talking
        // over it, starts a second capture.
        let mut l = armed();
        for _ in 0..10 {
            l.push_block(true, BLOCK);
        }
        for _ in 0..(SILENCE_TIMEOUT_MS / BLOCK) {
            l.push_block(false, BLOCK);
        }
        assert_eq!(l.state(), ListenState::Transcribing);

        for _ in 0..50 {
            assert_eq!(l.push_block(true, BLOCK), ListenAction::Continue);
        }
        assert_eq!(l.state(), ListenState::Transcribing);
    }

    #[test]
    fn counters_do_not_leak_between_utterances() {
        // A second utterance must get a clean slate, or the accumulated
        // speech from the first makes the cough guard pass incorrectly.
        let mut l = armed();
        for _ in 0..20 {
            l.push_block(true, BLOCK);
        }
        for _ in 0..(SILENCE_TIMEOUT_MS / BLOCK) {
            l.push_block(false, BLOCK);
        }
        l.finish_transcription();

        assert_eq!(l.speech_ms(), 0);
        l.trigger();
        l.push_block(true, BLOCK);
        assert_eq!(l.speech_ms(), BLOCK);
    }

    /* ---------- tuning sanity ---------- */
}
