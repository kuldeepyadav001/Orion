//! Voice input (M4).
//!
//! ```text
//! microphone -> pre-roll ring -> wake word -> capture -> whisper -> chat
//! ```
//!
//! ## What is testable here, and what is not
//!
//! The dev sandbox has no audio device, so this milestone is split so that
//! the parts which *can* be verified are verified, and the part which cannot
//! is kept as small as possible:
//!
//! * `audio` — buffering, mixing, resampling, WAV encoding. Pure functions,
//!   fully tested.
//! * `listener` — when to start and stop recording. A pure state machine over
//!   "was there speech?", fully tested without a clock or a microphone.
//! * `transcribe` — argument construction and output cleaning are tested;
//!   the actual process spawn is not.
//! * `capture` — the device layer. Behind a trait, deliberately thin, and
//!   **unverified** until someone runs it on real hardware.
//!
//! The previous four milestones each shipped a crash originating in exactly
//! this kind of untested integration glue, which is why the boundary is drawn
//! this tightly.
//!
//! ## Privacy
//!
//! Always-on listening is the mode chosen for this milestone, and it is a
//! real privacy surface in a product whose entire claim is that nothing
//! leaves the machine. Three rules follow from that, and they are enforced in
//! code rather than left as intentions:
//!
//! 1. **Off by default.** The microphone is never opened until the user asks.
//! 2. **Nothing is retained.** While waiting for the wake word, audio lives
//!    only in a 96 KB ring buffer and is overwritten continuously. It is
//!    never written to disk and never transcribed.
//! 3. **Nothing is transmitted.** Transcription runs as a local process with
//!    no socket — see `transcribe` for why the HTTP server was rejected.

pub mod audio;
pub mod capture;
pub mod listener;
pub mod service;
pub mod transcribe;
pub mod tts;

pub use listener::{ListenAction, ListenState, Listener};
pub use service::{VoiceService, VoiceStatus};
pub use transcribe::Transcript;
pub use tts::SpeechResult;
