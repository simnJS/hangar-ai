//! Transcription on this machine, with no network and no key.
//!
//! Parakeet TDT rather than Whisper, and CPU rather than GPU, are the same
//! decision seen twice. The model is 0.6B with a non-autoregressive decoder, so
//! it runs at roughly twenty times real time on an ordinary desktop core: a ten
//! second dictation comes back in half a second, on a machine with no graphics
//! card worth the name. Linking CUDA to make that faster would add a gigabyte
//! of driver-specific libraries to the installer, and a class of "it does not
//! work on my machine" that this feature has no need to own.

use std::path::Path;

use transcribe_rs::onnx::parakeet::ParakeetModel;
use transcribe_rs::onnx::Quantization;
use transcribe_rs::{SpeechModel, TranscribeOptions};

use super::engine::SttEngine;

pub struct LocalEngine {
    model: ParakeetModel,
}

impl LocalEngine {
    /// Loads a downloaded checkpoint. Slow — a second or two of reading and
    /// laying out weights — which is exactly why the caller keeps the result
    /// alive between dictations instead of calling this per sentence.
    pub fn load(dir: &Path) -> Result<Self, String> {
        if !dir.is_dir() {
            return Err("the local model is not downloaded yet".into());
        }
        let model = ParakeetModel::load(dir, &Quantization::Int8)
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
        // `transcribe` rather than `transcribe_raw`: it pads the leading
        // silence this model's mel preprocessor needs, without which the first
        // word of every dictation comes back clipped or missing.
        let result = self
            .model
            .transcribe(samples, &options)
            .map_err(|err| format!("transcription failed: {err}"))?;
        Ok(result.text.trim().to_string())
    }
}
