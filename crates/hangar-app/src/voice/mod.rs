//! P6: Voice dictation and its HUD (`VoiceHud.tsx`, `lib/voice.ts`).
//!
//! The Rust half (hangar-core's `voice`) owns the microphone and the model;
//! this owns the gesture, and the gesture is push-to-talk and nothing else:
//! the key is down, it listens; the key comes up, it transcribes. Nothing to
//! learn and nothing to leave running by accident, since the microphone cannot
//! be open unless a finger is on it.
//!
//! That the shortcut only ever *starts* is what makes it safe. Holding a key
//! repeats its press, so a press that also stopped would cut every dictation
//! short at the auto-repeat delay.
//!
//! The shortcut itself is the keymap's: the shell's `commands::run` must turn
//! `CommandId::VoiceDictate` into `Message::Voice(Msg::Press)`.

pub mod hud;

use std::time::{Duration, Instant};

use iced::keyboard::{self, Key, key::Named};
use iced::{Element, Event, Subscription, Task, window};

use hangar_core::voice::{EngineKind, VoiceConfig};
use hangar_i18n::{Key as Text, Translator, args};

use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::{Notice, Region};
use crate::state::Model;
use crate::state::types::{Settings, VoiceEngine};

/// Bars in the meter. Enough to read as a waveform, few enough to stay a pill.
pub const BARS: usize = 9;

/// How often the meter scrolls. Deliberately not driven by the level changing:
/// a steady voice reports the same peak twice in a row, and the trace would
/// freeze exactly while someone is holding a note.
const TICK: Duration = Duration::from_millis(70);

/// How often the pill is redrawn while the bars mark time, which is a little
/// faster than the trace so the wave runs smoothly.
const WAVE_TICK: Duration = Duration::from_millis(40);

/// Where a dictation is (`VoicePhase`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    Recording,
    Transcribing,
}

impl Phase {
    /// The phase hangar-core reports (`idle`, `recording`, `transcribing`).
    fn of(name: &str) -> Phase {
        match name {
            "recording" => Phase::Recording,
            "transcribing" => Phase::Transcribing,
            _ => Phase::Idle,
        }
    }
}

/// What the Rust side is given for a dictation (`voiceConfig`). No key: it
/// reads the Groq one from the system keychain itself.
pub fn voice_config(settings: &Settings) -> VoiceConfig {
    let local = settings.voice_engine == VoiceEngine::Local;
    VoiceConfig {
        engine: match settings.voice_engine {
            VoiceEngine::Local => EngineKind::Local,
            VoiceEngine::Groq => EngineKind::Groq,
        },
        model: if local {
            settings.voice_model.clone()
        } else {
            settings.voice_cloud_model.clone()
        },
        language: Some(settings.voice_language.clone()).filter(|language| !language.is_empty()),
        cleanup: settings.voice_cleanup,
        cleanup_model: settings.voice_cleanup_model.clone(),
        cleanup_hint: settings.voice_cleanup_hint.clone(),
    }
}

/// The package's UI state; lives in `App.pkg.voice`.
#[derive(Debug)]
pub struct State {
    phase: Phase,
    /// The latest peak, 0 to 1.
    level: f32,
    /// The trace: newest on the right, so it reads the way a waveform is
    /// written.
    bars: [f32; BARS],
    /// When the pill appeared, for the wave that marks time while the audio is
    /// being transcribed.
    since: Option<Instant>,
    /// `voice_start` has been asked and has not answered: the microphone is
    /// opening.
    starting: bool,
    /// The key came up while it was opening.
    aborted: bool,
}

impl Default for State {
    /// `HANGAR_DEV_VOICE=recording` or `transcribing` shows the pill in that
    /// phase, with a voice drawn on the meter, to see it in a theme and a
    /// language (debug builds only).
    fn default() -> Self {
        let mut st = State {
            phase: Phase::Idle,
            level: 0.0,
            bars: [0.0; BARS],
            since: None,
            starting: false,
            aborted: false,
        };
        let forced = if cfg!(debug_assertions) {
            std::env::var("HANGAR_DEV_VOICE").ok()
        } else {
            None
        };
        match forced.as_deref() {
            Some("recording") => {
                st.set_phase(Phase::Recording);
                st.bars = [0.02, 0.1, 0.4, 0.8, 0.5, 0.9, 0.3, 0.1, 0.04];
            }
            Some("transcribing") => st.set_phase(Phase::Transcribing),
            _ => {}
        }
        st
    }
}

impl State {
    pub fn phase(&self) -> Phase {
        self.phase
    }

    fn set_phase(&mut self, phase: Phase) {
        if phase == self.phase {
            return;
        }
        self.phase = phase;
        if phase != Phase::Recording {
            self.level = 0.0;
            self.bars = [0.0; BARS];
        }
        self.since = (phase != Phase::Idle).then(Instant::now);
    }

    /// The shortcut went down. It only ever starts: every later press while
    /// the key is still held is the keyboard repeating itself, and a
    /// dictation still being transcribed is not something to queue another
    /// one behind, since it would race the first into the same pane.
    fn press(&mut self) -> bool {
        if self.phase != Phase::Idle {
            return false;
        }
        self.set_phase(Phase::Recording);
        self.starting = true;
        self.aborted = false;
        true
    }

    /// Ends a recording. `Some(cancel)` when the Rust side has to be told.
    /// Moved out of `Recording` here rather than on the event coming back, so
    /// a second press during the round trip cannot start a new recording.
    fn stop(&mut self, cancel: bool) -> Option<bool> {
        if self.phase != Phase::Recording {
            return None;
        }
        self.aborted = self.starting;
        self.set_phase(if cancel {
            Phase::Idle
        } else {
            Phase::Transcribing
        });
        Some(cancel)
    }

    /// The meter scrolls one step with the latest level.
    fn tick(&mut self) {
        self.bars.rotate_left(1);
        self.bars[BARS - 1] = self.level;
    }

    pub(crate) fn bars(&self) -> &[f32; BARS] {
        &self.bars
    }

    pub(crate) fn since(&self) -> Option<Instant> {
        self.since
    }
}

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.voice`.
#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::Voice(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The dictation shortcut went down (`CommandId::VoiceDictate`).
    Press,
    /// A key went up: the push-to-talk is over. Any release ends it, not just
    /// the shortcut's own key: letting go of a three-key chord sends them up
    /// in whatever order the fingers leave, and waiting for one in particular
    /// would keep listening through the others.
    Release,
    /// Escape: the dictation was thought better of.
    Cancel,
    /// The window lost the keyboard mid-hold (a switch of window, a
    /// notification stealing it) and never sees the release: ended rather
    /// than cancelled, since the words were still said.
    Blur,
    /// The meter scrolls, or the wave moves.
    Tick,
    /// The microphone opened, or did not.
    Started(Result<(), String>),
}

fn notice(text: String) -> Task<Message> {
    Task::done(Message::Intent(Intent::Notice(Notice::Problem(text))))
}

fn failed(m: &Model, error: &str) -> Task<Message> {
    let tr = Translator::new(m.locale());
    notice(
        tr.t_with(Text::VoiceFailed, &args!("error" => error))
            .into_owned(),
    )
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    match msg {
        Msg::Press => {
            // Dictation is an opt-in: the shortcut is only live with the
            // setting on (`voice.dictate` was only in the handlers then).
            if !m.settings().voice_enabled || !st.press() {
                return Task::none();
            }
            let host = sv.core().host().clone();
            let manager = sv.core().core().voice().clone();
            let config = voice_config(m.settings());
            Task::perform(
                hangar_core::voice::voice_start(host, manager, config),
                |result| Message::Voice(Msg::Started(result)),
            )
        }
        Msg::Started(Ok(())) => {
            st.starting = false;
            // Let go before the microphone had opened (a tap on the key): the
            // stop found nothing to stop, so the recording that has just
            // begun is dropped here rather than left open with nobody's
            // finger on it.
            if !std::mem::take(&mut st.aborted) {
                return Task::none();
            }
            st.set_phase(Phase::Idle);
            send_stop(m, sv, true)
        }
        Msg::Started(Err(error)) => {
            st.starting = false;
            st.aborted = false;
            st.set_phase(Phase::Idle);
            failed(m, &error)
        }
        Msg::Release | Msg::Blur => stop(st, m, sv, false),
        Msg::Cancel => stop(st, m, sv, true),
        Msg::Tick => {
            st.tick();
            Task::none()
        }
    }
}

/// `voice_stop` returns at once, but dropping the recorder joins the capture
/// thread, so it runs off the interface's thread.
fn stop(st: &mut State, m: &Model, sv: &Services, cancel: bool) -> Task<Message> {
    match st.stop(cancel) {
        Some(cancel) => send_stop(m, sv, cancel),
        None => Task::none(),
    }
}

fn send_stop(m: &Model, sv: &Services, cancel: bool) -> Task<Message> {
    let host = sv.core().host().clone();
    let manager = sv.core().core().voice().clone();
    let config = voice_config(m.settings());
    Task::perform(
        async move {
            let _ = tokio::task::spawn_blocking(move || {
                hangar_core::voice::voice_stop(&host, &manager, config, cancel);
            })
            .await;
        },
        |()| Message::Noop,
    )
}

/// While recording: the release, Escape and a lost window end it; the meter
/// scrolls on its own beat. While transcribing: the wave. Idle: nothing, so no
/// timer runs and no key is read for a feature that is not in use.
pub fn subscription(st: &State, _m: &Model) -> Subscription<Message> {
    match st.phase {
        Phase::Idle => Subscription::none(),
        Phase::Recording => Subscription::batch([
            iced::event::listen_with(|event, _status, _window| match event {
                Event::Keyboard(keyboard::Event::KeyReleased { .. }) => {
                    Some(Message::Voice(Msg::Release))
                }
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key: Key::Named(Named::Escape),
                    ..
                }) => Some(Message::Voice(Msg::Cancel)),
                Event::Window(window::Event::Unfocused) => Some(Message::Voice(Msg::Blur)),
                _ => None,
            }),
            iced::time::every(TICK).map(|_| Message::Voice(Msg::Tick)),
        ]),
        Phase::Transcribing => iced::time::every(WAVE_TICK).map(|_| Message::Noop),
    }
}

/// What takes the focus when F6 reaches a region this package owns. It owns
/// none, so the shell never calls it.
pub fn focus_entry(_st: &State, _m: &Model, _region: Region) -> Option<Task<Message>> {
    None
}

/// Every intent, after the shell's own handling of it: `Some` when this
/// package acted on it.
pub fn on_intent(
    _st: &mut State,
    _m: &mut Model,
    _sv: &Services,
    _intent: &Intent,
) -> Option<Task<Message>> {
    None
}

/// The dictation's events: its phase (and what went wrong), its level, and
/// what was said.
pub fn on_event(
    st: &mut State,
    m: &mut Model,
    _sv: &Services,
    event: &CoreEvent,
) -> Option<Task<Message>> {
    match event {
        CoreEvent::VoicePhase(phase) => {
            // The "recording" of a microphone that opened after the key had
            // already come up is not a state to show.
            if !st.aborted {
                st.set_phase(Phase::of(phase.phase));
            }
            phase.error.as_deref().map(|error| failed(m, error))
        }
        CoreEvent::VoiceLevel(level) => {
            st.level = level.level.clamp(0.0, 1.0);
            None
        }
        CoreEvent::VoiceText(transcript) => (!transcript.text.is_empty()).then(|| {
            Task::done(Message::Intent(Intent::InsertTranscript(
                transcript.text.clone(),
            )))
        }),
        _ => None,
    }
}

/// The dictation pill at the bottom; nothing while idle.
pub fn hud<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    hud::view(st, ctx)
}

#[cfg(test)]
mod tests;
