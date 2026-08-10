//! Transcription on this machine, with no network and no key.
//!
//! whisper.cpp on the CPU. The GPU is left out deliberately: a dictation is ten
//! seconds of audio, a model the size of this one clears that in a fraction of
//! it on any recent processor, and linking CUDA or Metal would buy a wait
//! nobody can feel at the price of vendor libraries in the installer and a
//! class of driver-specific failures this feature has no need to own.
//!
//! Which makes how the CPU is asked to do the work the whole performance story,
//! and the two constants below are most of it.

use std::path::Path;

use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
};

use super::engine::SttEngine;

/// Threads whisper.cpp is given.
///
/// Left alone it uses `min(4, cores)`, a default from when four was a lot, and
/// a sixteen-core machine then transcribes at a quarter of its speed.
///
/// Physical cores, not logical. The two hyperthreads of one core share the
/// arithmetic units this work is made of, so the second adds nothing to compute
/// and everything to contention: the same machine at thirty-two threads takes
/// *seventy-six* seconds on a model it clears in four and a half at sixteen.
/// More threads than cores to run them on is not a smaller win, it is a large
/// loss.
fn threads() -> std::ffi::c_int {
    num_cpus::get_physical().max(1) as std::ffi::c_int
}

/// The encoder always runs over a 30-second window: whisper pads anything
/// shorter and charges full price for the padding. `audio_ctx` shortens the
/// window to what was actually said, and a ten-second dictation is two thirds
/// padding — measured here, 17.3s becomes 8.0s, with the same words out.
///
/// The `+ 128` is slack for the encoder's own downsampling, and comes from
/// whisper.cpp#1855 along with the ratio. Below that floor the tail of a
/// sentence starts falling outside the window it is being read from.
const AUDIO_CTX_FULL: i32 = 1500;
const AUDIO_CTX_SLACK: i32 = 128;

fn audio_ctx(samples: usize) -> std::ffi::c_int {
    let seconds = samples as f32 / super::capture::SAMPLE_RATE as f32;
    let scaled = (seconds / 30.0 * AUDIO_CTX_FULL as f32) as i32 + AUDIO_CTX_SLACK;
    // Zero means "the whole window", which is also what anything past it would
    // get: asking for more context than the encoder has is not an error, it is
    // just the default spelled out.
    if scaled >= AUDIO_CTX_FULL {
        0
    } else {
        scaled as std::ffi::c_int
    }
}

pub struct LocalEngine {
    state: WhisperState,
    /// Never read, and load-bearing: it owns the C memory `state` points into.
    #[allow(dead_code)]
    context: WhisperContext,
}

impl LocalEngine {
    /// Loads a downloaded checkpoint. Slow — a second or two of reading and
    /// laying out weights — which is exactly why the caller keeps the result
    /// alive between dictations instead of calling this per sentence.
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.is_file() {
            return Err("the local model is not downloaded yet".into());
        }
        let path = path
            .to_str()
            .ok_or("the model path is not valid unicode")?
            .to_string();

        // No GPU backend is compiled in, so `use_gpu` only decides whether
        // loading spends its time looking for one that cannot be there.
        let params = WhisperContextParameters {
            use_gpu: false,
            ..Default::default()
        };

        let context = WhisperContext::new_with_params(&path, params)
            .map_err(|err| format!("could not load the local model: {err}"))?;
        let state = context
            .create_state()
            .map_err(|err| format!("could not start the local model: {err}"))?;

        Ok(Self { state, context })
    }
}

impl SttEngine for LocalEngine {
    fn transcribe(&mut self, samples: &[f32], language: Option<&str>) -> Result<String, String> {
        // Beam search over greedy, as before: it costs about a twentieth of the
        // run and is what keeps whisper from committing to a bad first guess.
        let mut params = FullParams::new(SamplingStrategy::BeamSearch {
            beam_size: 3,
            patience: -1.0,
        });
        params.set_language(language);
        params.set_n_threads(threads());
        params.set_audio_ctx(audio_ctx(samples.len()));
        params.set_translate(false);
        params.set_suppress_blank(true);
        params.set_suppress_nst(true);
        // Nothing here has a console to print to, and whisper.cpp's default is
        // to write the transcript to one as it goes.
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        self.state
            .full(params, samples)
            .map_err(|err| format!("transcription failed: {err}"))?;

        let mut text = String::new();
        for i in 0..self.state.full_n_segments() {
            let Some(segment) = self.state.get_segment(i) else {
                continue;
            };
            text.push_str(segment.to_str().map_err(|err| err.to_string())?);
        }
        Ok(text.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: usize = super::super::capture::SAMPLE_RATE as usize;

    #[test]
    fn audio_ctx_shrinks_with_the_clip() {
        // Ten seconds: a third of the window, plus the slack.
        assert_eq!(audio_ctx(10 * RATE), 628);
        assert_eq!(audio_ctx(RATE / 2), 153);
    }

    #[test]
    fn audio_ctx_is_the_full_window_once_it_would_exceed_it() {
        // Zero is how whisper.cpp spells "all of it", and anything from here up
        // has to reach that rather than ask for a window past the encoder's.
        assert_eq!(audio_ctx(30 * RATE), 0);
        assert_eq!(audio_ctx(300 * RATE), 0);
    }

    #[test]
    fn threads_is_at_least_one() {
        assert!(threads() >= 1);
    }

    /// The real thing, skipped unless pointed at a checkpoint and a 16 kHz mono
    /// WAV — neither of which belongs in a repository or in CI.
    ///
    ///   HANGAR_TEST_MODEL=…/ggml-small-q5_1.bin \
    ///   HANGAR_TEST_WAV=…/jfk.wav cargo test -- --nocapture transcribes
    #[test]
    fn transcribes_real_audio() {
        let (Ok(model), Ok(wav)) = (
            std::env::var("HANGAR_TEST_MODEL"),
            std::env::var("HANGAR_TEST_WAV"),
        ) else {
            return;
        };

        let bytes = std::fs::read(&wav).expect("read the wav");
        let samples: Vec<f32> = bytes[44..]
            .chunks_exact(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]) as f32 / 32768.0)
            .collect();

        let started = std::time::Instant::now();
        let mut engine = LocalEngine::load(Path::new(&model)).expect("load the model");
        let text = engine
            .transcribe(&samples, Some("en"))
            .expect("transcribe the audio");
        println!("{:.2?}: {text}", started.elapsed());
        assert!(text.to_lowercase().contains("ask not what your country"));
    }
}
