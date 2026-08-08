//! Transcription on this machine, with no network and no key.
//!
//! whisper.cpp on the CPU. The GPU is left out deliberately: a dictation is ten
//! seconds of audio, the turbo checkpoint clears that in a fraction of it on
//! any recent processor, and linking CUDA or Metal would buy a wait nobody can
//! feel at the price of vendor libraries in the installer and a class of
//! driver-specific failures this feature has no need to own.

use std::path::Path;

use transcribe_rs::whisper_cpp::WhisperEngine;
use transcribe_rs::{SpeechModel, TranscribeOptions};

use super::engine::SttEngine;

pub struct LocalEngine {
    model: WhisperEngine,
}

impl LocalEngine {
    /// Loads a downloaded checkpoint. Slow — a second or two of reading and
    /// laying out weights — which is exactly why the caller keeps the result
    /// alive between dictations instead of calling this per sentence.
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.is_file() {
            return Err("the local model is not downloaded yet".into());
        }
        let model = WhisperEngine::load(path)
            .map_err(|err| format!("could not load the local model: {err}"))?;
        Ok(Self { model })
    }
}

impl SttEngine for LocalEngine {
    fn transcribe(&mut self, samples: &[f32], language: Option<&str>) -> Result<String, String> {
        let options = TranscribeOptions {
            language: language.map(str::to_string),
            ..Default::default()
        };
        let result = self
            .model
            .transcribe(samples, &options)
            .map_err(|err| format!("transcription failed: {err}"))?;
        Ok(result.text.trim().to_string())
    }
}
