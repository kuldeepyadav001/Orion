//! Audio buffering and WAV encoding for speech capture.
//!
//! This module is deliberately free of any device code. Everything here is a
//! pure function over samples, so it can be tested without a microphone —
//! which matters because the dev sandbox has no audio device at all, and the
//! last four milestones each shipped a crash that came from untested
//! integration glue.
//!
//! The device layer lives in `capture.rs` behind a trait, and does as little
//! as possible.

use std::collections::VecDeque;

/// Whisper requires 16 kHz mono. Everything is resampled to this before it
/// reaches the transcriber; feeding it anything else produces silence or
/// gibberish rather than an error, which is a miserable failure mode.
pub const TARGET_RATE: u32 = 16_000;

/// Hard ceiling on a single utterance, in seconds.
///
/// Without this, a stuck VAD or a noisy room grows the buffer until the
/// machine swaps. On a 6 GiB laptop already running a 2.4 GB model, that is
/// not a theoretical concern.
pub const MAX_UTTERANCE_SECS: usize = 30;

/// Samples in one full-length utterance.
pub const MAX_SAMPLES: usize = TARGET_RATE as usize * MAX_UTTERANCE_SECS;

// 30 s of f32 is ~1.9 MB. Checked at compile time because the machine this
// targets has ~1 GB free with the chat model loaded, and a careless bump here
// would be felt immediately.
const _: () = assert!(MAX_SAMPLES * 4 < 3_000_000);

/// How much audio to keep from *before* the wake word fires.
///
/// People run the wake word into the request without pausing — "hey Orion
/// what's the weather" arrives as one breath. Without a pre-roll the first
/// word or two of the actual question is lost, which reads as the assistant
/// mishearing.
pub const PREROLL_SECS: f32 = 1.5;

/// Samples of pre-roll.
pub const PREROLL_SAMPLES: usize = (TARGET_RATE as f32 * PREROLL_SECS) as usize;

/// A fixed-size ring of recent audio, kept so the moment before a trigger is
/// not lost.
///
/// Memory is bounded and constant: 1.5 s at 16 kHz of `f32` is 96 KB, which
/// is the entire cost of always-on listening at the buffer level.
pub struct PreRoll {
    samples: VecDeque<f32>,
    capacity: usize,
}

impl PreRoll {
    pub fn new(capacity: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub fn push(&mut self, chunk: &[f32]) {
        for &s in chunk {
            if self.samples.len() == self.capacity {
                self.samples.pop_front();
            }
            self.samples.push_back(s);
        }
    }

    /// Everything currently buffered, oldest first.
    pub fn drain_all(&mut self) -> Vec<f32> {
        self.samples.drain(..).collect()
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn clear(&mut self) {
        self.samples.clear();
    }
}

/// Mix interleaved multi-channel audio down to mono.
///
/// Laptop arrays are frequently stereo, and taking only the left channel
/// throws away half the signal — audible as a quieter, noisier recording on
/// machines where the two mics are meaningfully apart.
pub fn to_mono(interleaved: &[f32], channels: u16) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    let ch = channels as usize;
    interleaved
        .chunks_exact(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect()
}

/// Resample to 16 kHz by linear interpolation.
///
/// Deliberately not a windowed-sinc resampler. Whisper's own front end
/// downsamples to a mel spectrogram, so the aliasing that linear
/// interpolation introduces above ~8 kHz is discarded anyway. A proper
/// resampler would add a dependency and CPU cost for no measurable accuracy
/// gain on speech.
pub fn resample_to_16k(input: &[f32], from_rate: u32) -> Vec<f32> {
    if from_rate == TARGET_RATE || input.is_empty() {
        return input.to_vec();
    }

    let ratio = TARGET_RATE as f64 / from_rate as f64;
    let out_len = ((input.len() as f64) * ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);

    for i in 0..out_len {
        let src = i as f64 / ratio;
        let lo = src.floor() as usize;
        let hi = (lo + 1).min(input.len() - 1);
        let frac = (src - lo as f64) as f32;
        let a = input[lo.min(input.len() - 1)];
        let b = input[hi];
        out.push(a + (b - a) * frac);
    }
    out
}

/// Root mean square of a block, used as a cheap loudness estimate.
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f32 = samples.iter().map(|s| s * s).sum();
    (sum / samples.len() as f32).sqrt()
}

/// Encode mono `f32` samples as a 16-bit PCM WAV file in memory.
///
/// whisper-cli reads WAV from disk and whisper-server takes it as multipart
/// form data, so the encoder is shared. Written by hand rather than pulling
/// in the `hound` crate: the format is 44 bytes of header and a cast, and one
/// fewer dependency is one less thing to audit in an offline product.
pub fn encode_wav_16k_mono(samples: &[f32]) -> Vec<u8> {
    let data_len = samples.len() * 2; // i16
    let mut out = Vec::with_capacity(44 + data_len);

    // RIFF header
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");

    // fmt chunk
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&TARGET_RATE.to_le_bytes());
    out.extend_from_slice(&(TARGET_RATE * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample

    // data chunk
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_len as u32).to_le_bytes());

    // Automatic peak normalization for speech clarity:
    // Raw microphone signals often have very low amplitude (e.g. 0.05-0.15 peak).
    // Scaling speech to ~85% full scale (-1.4 dBFS) ensures whisper receives clean,
    // audible audio without boosting digital silence into noise.
    let peak = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    let gain = if peak > 0.005 {
        (0.85 / peak).clamp(1.0, 12.0)
    } else {
        1.0
    };

    for &s in samples {
        // Apply gain and clamp before casting. A sample above 1.0 wraps to a
        // large negative i16 otherwise, which sounds like a loud click and can
        // confuse the transcriber into emitting noise tokens.
        let amplified = s * gain;
        let clamped = amplified.clamp(-1.0, 1.0);
        let v = (clamped * i16::MAX as f32) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /* ---------- pre-roll ---------- */

    #[test]
    fn preroll_keeps_only_the_most_recent_audio() {
        let mut p = PreRoll::new(4);
        p.push(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        assert_eq!(p.drain_all(), vec![3.0, 4.0, 5.0, 6.0]);
    }

    #[test]
    fn preroll_memory_is_bounded_however_much_is_pushed() {
        // Always-on listening pushes audio forever. If this grows, the
        // machine eventually swaps.
        let mut p = PreRoll::new(100);
        for _ in 0..1000 {
            p.push(&[0.5; 50]);
        }
        assert_eq!(p.len(), 100);
    }

    #[test]
    fn draining_the_preroll_empties_it() {
        let mut p = PreRoll::new(8);
        p.push(&[1.0; 8]);
        assert_eq!(p.drain_all().len(), 8);
        assert!(p.is_empty(), "a second trigger must not replay old audio");
    }

    #[test]
    fn preroll_holds_about_a_second_and_a_half() {
        // The point of the pre-roll is to catch speech that runs straight out
        // of the wake word. Too short and the first words are lost.
        assert!(PREROLL_SAMPLES >= TARGET_RATE as usize);
        assert!(PREROLL_SAMPLES <= TARGET_RATE as usize * 3);
    }

    /* ---------- channel mixing ---------- */

    #[test]
    fn stereo_is_averaged_not_truncated() {
        // Taking one channel would discard half the signal.
        let stereo = [1.0, 0.0, 0.5, 0.5, -1.0, 1.0];
        assert_eq!(to_mono(&stereo, 2), vec![0.5, 0.5, 0.0]);
    }

    #[test]
    fn mono_passes_through_untouched() {
        let mono = [0.1, 0.2, 0.3];
        assert_eq!(to_mono(&mono, 1), mono.to_vec());
    }

    #[test]
    fn four_channel_arrays_are_handled() {
        // Some laptops expose 4-mic arrays.
        let quad = [1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0];
        assert_eq!(to_mono(&quad, 4), vec![1.0, 0.0]);
    }

    /* ---------- resampling ---------- */

    #[test]
    fn resampling_hits_the_rate_whisper_requires() {
        // 48 kHz is the near-universal laptop default. Getting this wrong
        // produces gibberish rather than an error.
        let one_sec_48k = vec![0.0f32; 48_000];
        let out = resample_to_16k(&one_sec_48k, 48_000);
        let expected = TARGET_RATE as usize;
        assert!(
            (out.len() as i64 - expected as i64).abs() <= 1,
            "expected ~{expected} samples, got {}",
            out.len()
        );
    }

    #[test]
    fn already_16k_audio_is_not_touched() {
        let input = vec![0.25f32; 1000];
        assert_eq!(resample_to_16k(&input, TARGET_RATE), input);
    }

    #[test]
    fn resampling_preserves_a_constant_signal() {
        // Interpolation of a flat line must stay flat; a bug in the index
        // maths shows up as spikes at the boundaries.
        let input = vec![0.5f32; 4800];
        let out = resample_to_16k(&input, 48_000);
        for (i, s) in out.iter().enumerate() {
            assert!(
                (s - 0.5).abs() < 1e-6,
                "sample {i} drifted to {s}, interpolation is wrong"
            );
        }
    }

    #[test]
    fn resampling_upward_also_works() {
        // 8 kHz inputs exist on some bluetooth headsets.
        let input = vec![0.1f32; 8_000];
        let out = resample_to_16k(&input, 8_000);
        assert!((out.len() as i64 - 16_000).abs() <= 1);
    }

    #[test]
    fn resampling_empty_input_does_not_panic() {
        assert!(resample_to_16k(&[], 44_100).is_empty());
    }

    /* ---------- wav encoding ---------- */

    #[test]
    fn wav_header_is_well_formed() {
        let wav = encode_wav_16k_mono(&[0.0; 100]);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[36..40], b"data");

        // 44-byte header + 2 bytes per sample
        assert_eq!(wav.len(), 44 + 200);

        let rate = u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]);
        assert_eq!(rate, 16_000, "whisper only accepts 16 kHz");

        let channels = u16::from_le_bytes([wav[22], wav[23]]);
        assert_eq!(channels, 1, "whisper only accepts mono");

        let bits = u16::from_le_bytes([wav[34], wav[35]]);
        assert_eq!(bits, 16);
    }

    #[test]
    fn declared_sizes_match_the_actual_payload() {
        // A wrong length field makes some decoders read past the buffer or
        // truncate the audio silently.
        let wav = encode_wav_16k_mono(&[0.5; 50]);
        let riff_len = u32::from_le_bytes([wav[4], wav[5], wav[6], wav[7]]) as usize;
        let data_len = u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]) as usize;

        assert_eq!(riff_len, wav.len() - 8);
        assert_eq!(data_len, 100);
        assert_eq!(data_len, wav.len() - 44);
    }

    #[test]
    fn loud_samples_clamp_instead_of_wrapping() {
        // Without clamping, +1.5 wraps to a large negative i16 — an audible
        // click that the transcriber may turn into spurious tokens.
        let wav = encode_wav_16k_mono(&[1.5, -1.5]);
        let a = i16::from_le_bytes([wav[44], wav[45]]);
        let b = i16::from_le_bytes([wav[46], wav[47]]);
        assert!(a > 32_000, "positive clipping wrapped: {a}");
        assert!(b < -32_000, "negative clipping wrapped: {b}");
    }

    #[test]
    fn silence_encodes_to_zeroes() {
        let wav = encode_wav_16k_mono(&[0.0; 10]);
        assert!(wav[44..].iter().all(|&b| b == 0));
    }

    #[test]
    fn an_empty_recording_still_produces_a_valid_file() {
        let wav = encode_wav_16k_mono(&[]);
        assert_eq!(wav.len(), 44);
        assert_eq!(&wav[0..4], b"RIFF");
    }

    /* ---------- loudness ---------- */

    #[test]
    fn rms_separates_silence_from_speech() {
        assert_eq!(rms(&[]), 0.0);
        assert_eq!(rms(&[0.0; 100]), 0.0);
        assert!(rms(&[0.5; 100]) > 0.4);
        assert!(rms(&[0.001; 100]) < 0.01);
    }

    /* ---------- limits ---------- */

    #[test]
    fn an_utterance_is_capped_at_a_sane_length() {
        // A stuck VAD must not grow the buffer without bound.
        assert_eq!(MAX_SAMPLES, 480_000);
    }
}
