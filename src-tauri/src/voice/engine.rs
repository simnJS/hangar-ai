//! What every transcription backend has in common.
//!
//! One trait, so the manager holds a model without knowing whether it runs on
//! this machine or in a datacentre, and so adding a backend is a file rather
//! than a branch through the recording path.

use serde::{Deserialize, Serialize};

/// Where a dictation is transcribed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineKind {
    /// On this machine, with a model downloaded once and kept in memory.
    Local,
    /// Groq's hosted Whisper. Kept as the fallback rather than the default:
    /// it needs a key and a connection, and it is the only path that sends
    /// anything recorded here off the machine.
    Groq,
}

/// The whole voice configuration, as the frontend settings describe it. Passed
/// with every call rather than mirrored in the manager: settings change while a
/// recording is in flight, and the dictation should finish under the rules it
/// started with.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceConfig {
    pub engine: EngineKind,
    /// Model id within the engine — a checkpoint name locally, an API model
    /// name for Groq.
    pub model: String,
    /// ISO code, or `None` to let the model decide. Naming the language helps
    /// every engine: it saves Whisper a detection pass and stops it switching
    /// language mid-sentence on a bilingual speaker.
    pub language: Option<String>,
    pub api_key: String,
    /// Send the transcript through a small model to clean it up.
    pub cleanup: bool,
    pub cleanup_model: String,
    /// Extra instruction appended to the cleanup prompt — project jargon, the
    /// spelling of names an ASR model has never met.
    pub cleanup_hint: String,
}

impl VoiceConfig {
    /// Identifies the loaded model, so the manager can tell a call it can serve
    /// from the model it already has from one that needs a different file.
    pub fn engine_key(&self) -> String {
        format!("{:?}:{}", self.engine, self.model)
    }

    pub fn language(&self) -> Option<&str> {
        self.language.as_deref().filter(|code| !code.is_empty())
    }
}

pub trait SttEngine: Send {
    /// Transcribes 16 kHz mono samples.
    ///
    /// `&mut self` because a local model owns mutable inference state; the
    /// manager serialises calls behind its own lock either way, since one
    /// person only dictates one thing at a time.
    fn transcribe(&mut self, samples: &[f32], language: Option<&str>) -> Result<String, String>;
}
