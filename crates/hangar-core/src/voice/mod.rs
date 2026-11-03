//! Dictation: microphone in, text out, for whichever pane has the focus.
//!
//! The shape follows what the feature actually is. Recording is a long-lived
//! thing the user holds open, so it lives behind a handle rather than a call.
//! Transcription is a one-shot job that can take a second, so it runs on a
//! thread and reports back with events instead of making the command wait.
//! Nothing here touches the terminal: the frontend decides where a transcript
//! lands, because only it knows what has the focus.

mod capture;
mod cleanup;
mod engine;
mod groq;
/// Public for the same reason as `models`, and so `store.rs` can move a key
/// left in state.json by an older version.
pub mod key;
mod local;
/// Public: the app calls the catalogue's commands as `voice::models::…`,
/// where the window invoked them.
pub mod models;
mod wav;

use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::host::{Event, Host};

use capture::Recorder;
use engine::SttEngine;

pub use engine::{EngineKind, VoiceConfig};

/// Below this, the recording is silence: a key pressed and released without
/// speaking, or a microphone that is muted in hardware and reports nothing but
/// a quiet hiss. Sending it anywhere is a wasted second and, on Whisper, an
/// invented sentence.
const SILENCE_RMS: f32 = 0.004;

/// Trim threshold, deliberately lower than the silence gate: the aim is to keep
/// every syllable and drop only the room, so it errs towards keeping audio.
const TRIM_RMS: f32 = 0.008;

/// Kept either side of the speech that survived trimming. Models need a moment
/// of room tone to settle; cutting exactly on the first syllable clips it.
const TRIM_MARGIN_MS: usize = 120;

/// A recording shorter than this was a mis-hit on the shortcut, not a sentence.
const MIN_SAMPLES: usize = capture::SAMPLE_RATE as usize / 4;

/// What the interface shows about the dictation: `Event::VoicePhase`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Phase {
    /// `idle`, `recording` or `transcribing`.
    pub phase: &'static str,
    /// Set when a dictation ended badly, and shown as a toast. `None` on every
    /// ordinary transition.
    pub error: Option<String>,
}

impl Phase {
    fn of(phase: &'static str) -> Self {
        Self { phase, error: None }
    }

    fn failed(error: String) -> Self {
        Self {
            phase: "idle",
            error: Some(error),
        }
    }
}

/// The microphone's level while recording: `Event::VoiceLevel`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Level {
    /// Peak since the last frame, 0..1.
    pub level: f32,
}

/// What was said: `Event::VoiceText`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    pub text: String,
}

/// A model loaded and kept in memory between dictations.
///
/// This is the whole reason the manager holds state at all. A local checkpoint
/// takes a second or two to read off disk and lay out; paying that on every
/// push of the key would cost more than the transcription itself.
struct Loaded {
    key: String,
    engine: Box<dyn SttEngine>,
}

#[derive(Default)]
pub struct VoiceManager {
    /// The live recording, if any. Separate from `loaded` so that arming the
    /// microphone never waits on a model being read off disk.
    session: Mutex<Option<Recorder>>,
    loaded: Mutex<Option<Loaded>>,
}

impl VoiceManager {
    /// Drops the resident model. Used when the settings pick another one, so
    /// the next dictation loads the new checkpoint instead of the old one, and
    /// a machine that has finished dictating gets its memory back.
    pub fn unload(&self) {
        *self.loaded.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// Starts recording. Fails if there is no microphone, if the chosen engine
/// cannot run, or if a recording is already open — which the frontend prevents,
/// but a stuck key would not.
pub async fn voice_start(
    host: Host,
    manager: Arc<VoiceManager>,
    config: VoiceConfig,
) -> Result<(), String> {
    {
        let session = manager.session.lock().unwrap_or_else(|e| e.into_inner());
        if session.is_some() {
            return Err("already recording".into());
        }
    }

    // Before the microphone rather than after: an undownloaded model or a
    // missing key is a fine thing to be told, and a terrible thing to be told
    // once you have already said your sentence.
    usable(&host, &config)?;

    // Opening a device waits on the audio backend, which is a stall the window
    // would be seen to take if this ran where commands run by default.
    let recorder = tokio::task::spawn_blocking(Recorder::start)
        .await
        .map_err(|err| format!("capture thread failed: {err}"))??;

    *manager.session.lock().unwrap_or_else(|e| e.into_inner()) = Some(recorder);
    let _ = host.emit(Event::VoicePhase(Phase::of("recording")));
    spawn_meter(host, manager);
    Ok(())
}

/// Ends the recording. With `cancel`, the audio is dropped and nothing is
/// transcribed — the escape hatch for a dictation you thought better of.
pub fn voice_stop(host: &Host, manager: &Arc<VoiceManager>, config: VoiceConfig, cancel: bool) {
    let Some(recorder) = manager
        .session
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
    else {
        return;
    };

    if cancel {
        // Dropping the recorder stops the thread and closes the device.
        drop(recorder);
        let _ = host.emit(Event::VoicePhase(Phase::of("idle")));
        return;
    }

    let _ = host.emit(Event::VoicePhase(Phase::of("transcribing")));

    // Off the command thread from here on. Reading the tail of the recording,
    // loading a model and running inference are all measured in hundreds of
    // milliseconds, and the window has a spinner to keep turning.
    let host = host.clone();
    let manager = manager.clone();
    std::thread::Builder::new()
        .name("voice-transcribe".into())
        .spawn(
            move || match transcribe(&host, &manager, recorder, &config) {
                Ok(None) => {
                    // Nothing was said. Not an error worth a toast — the user
                    // knows they said nothing — so it just ends quietly.
                    let _ = host.emit(Event::VoicePhase(Phase::of("idle")));
                }
                Ok(Some(transcript)) => {
                    let _ = host.emit(Event::VoicePhase(Phase::of("idle")));
                    let _ = host.emit(Event::VoiceText(transcript));
                }
                Err(err) => {
                    let _ = host.emit(Event::VoicePhase(Phase::failed(err)));
                }
            },
        )
        .ok();
}

/// True while a recording is open, so the interface can recover its own state
/// after a reload rather than showing a microphone that is not running.
pub fn voice_recording(manager: &VoiceManager) -> bool {
    manager
        .session
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_some()
}

/// Drops the resident model, freeing its memory until the next dictation.
pub fn voice_unload(manager: &VoiceManager) {
    manager.unload();
}

/// `Ok(None)` when there was nothing to transcribe.
fn transcribe(
    host: &Host,
    manager: &VoiceManager,
    recorder: Recorder,
    config: &VoiceConfig,
) -> Result<Option<Transcript>, String> {
    let samples = recorder.finish();
    if samples.len() < MIN_SAMPLES || capture::rms(&samples) < SILENCE_RMS {
        return Ok(None);
    }

    let speech = capture::trim(&samples, TRIM_RMS, TRIM_MARGIN_MS);
    if speech.len() < MIN_SAMPLES {
        return Ok(None);
    }

    let text = {
        let mut loaded = manager.loaded.lock().unwrap_or_else(|e| e.into_inner());
        let key = config.engine_key();

        // Rebuilt only when the settings now point somewhere else. The common
        // path — the same model as last time — reuses what is already resident.
        if loaded.as_ref().map(|l| l.key.as_str()) != Some(key.as_str()) {
            // Dropped before the new one is built, so switching between two
            // local checkpoints never holds both in memory at once.
            *loaded = None;
            *loaded = Some(Loaded {
                key,
                engine: build(host, config)?,
            });
        }

        loaded
            .as_mut()
            .expect("engine was just built")
            .engine
            .transcribe(speech, config.language())?
    };

    let text = text.trim().to_string();
    if text.is_empty() {
        return Ok(None);
    }

    // The cleanup pass is allowed to fail without taking the dictation with it:
    // a rate limit or a bad model name should cost the polish, never the words.
    let text = if config.cleanup {
        cleanup::run(config, &text).unwrap_or(text)
    } else {
        text
    };

    Ok(Some(Transcript { text }))
}

/// Whether this configuration could transcribe anything, without loading
/// anything to find out.
fn usable(host: &Host, config: &VoiceConfig) -> Result<(), String> {
    match config.engine {
        EngineKind::Local if !models::is_installed(host, &config.model) => {
            Err("the local model is not downloaded yet — Settings → Voice".into())
        }
        EngineKind::Groq => key::require().map(|_| ()),
        _ => Ok(()),
    }
}

fn build(host: &Host, config: &VoiceConfig) -> Result<Box<dyn SttEngine>, String> {
    match config.engine {
        EngineKind::Local => {
            let path = models::model_file(host, &config.model)?;
            Ok(Box::new(local::LocalEngine::load(&path)?))
        }
        EngineKind::Groq => Ok(Box::new(groq::GroqEngine::new(&config.model))),
    }
}

/// How often the level meter is pushed to the interface. Fast enough to look
/// like a response to the voice, slow enough to be free.
const METER_INTERVAL: std::time::Duration = std::time::Duration::from_millis(60);

/// Feeds the level meter for as long as the recording lasts.
///
/// A thread rather than the frontend polling: the alternative is an IPC round
/// trip fifteen times a second for a number the Rust side already has.
fn spawn_meter(host: Host, manager: Arc<VoiceManager>) {
    std::thread::Builder::new()
        .name("voice-meter".into())
        .spawn(move || loop {
            let level = {
                let session = manager.session.lock().unwrap_or_else(|e| e.into_inner());
                match session.as_ref() {
                    Some(recorder) => recorder.level(),
                    // The recording ended — by the user, or by hitting the
                    // length cap. Either way this thread is done.
                    None => return,
                }
            };
            // Nobody listening is a window that is gone.
            if host.emit(Event::VoiceLevel(Level { level })).is_err() {
                return;
            }
            std::thread::sleep(METER_INTERVAL);
        })
        .ok();
}
