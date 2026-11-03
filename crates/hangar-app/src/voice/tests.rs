//! `lib/voice.ts` had no tests; these hold the gesture it describes: only the
//! press starts, any release ends, Escape cancels, nothing queues behind a
//! transcription.
#![allow(clippy::field_reassign_with_default)]

use hangar_core::voice::EngineKind;

use super::*;

#[test]
fn a_press_starts_a_recording_and_only_when_idle() {
    let mut st = State::default();
    assert!(st.press());
    assert_eq!(st.phase(), Phase::Recording);
    // The keyboard repeating the held key is not another dictation.
    assert!(!st.press());
}

#[test]
fn nothing_queues_behind_a_transcription() {
    let mut st = State::default();
    assert!(st.press());
    st.starting = false;
    assert_eq!(st.stop(false), Some(false));
    assert_eq!(st.phase(), Phase::Transcribing);
    assert!(!st.press());
}

#[test]
fn a_release_ends_the_recording_and_transcribes_it() {
    let mut st = State::default();
    st.press();
    st.starting = false;
    assert_eq!(st.stop(false), Some(false));
    assert_eq!(st.phase(), Phase::Transcribing);
}

#[test]
fn escape_cancels_and_goes_back_to_idle_at_once() {
    let mut st = State::default();
    st.press();
    st.starting = false;
    assert_eq!(st.stop(true), Some(true));
    assert_eq!(st.phase(), Phase::Idle);
    // A press right after is a new dictation: the round trip to the Rust side
    // cannot start a second recording behind it, since the phase already left
    // `Recording`, but it can start a fresh one once idle.
    assert!(st.press());
}

#[test]
fn a_release_with_nothing_recording_does_nothing() {
    let mut st = State::default();
    assert_eq!(st.stop(false), None);
    st.press();
    st.starting = false;
    st.stop(false);
    // A second release while it transcribes is not a second stop.
    assert_eq!(st.stop(false), None);
}

#[test]
fn a_key_up_before_the_microphone_opened_is_remembered() {
    let mut st = State::default();
    st.press();
    assert!(st.starting);
    st.stop(false);
    assert!(st.aborted);
}

#[test]
fn the_meter_scrolls_with_the_latest_level_newest_on_the_right() {
    let mut st = State::default();
    st.level = 0.5;
    st.tick();
    st.level = 1.0;
    st.tick();
    assert_eq!(st.bars()[BARS - 2], 0.5);
    assert_eq!(st.bars()[BARS - 1], 1.0);
    assert_eq!(st.bars()[0], 0.0);
}

#[test]
fn a_steady_voice_still_moves_the_trace() {
    // The same peak twice in a row: the trace still advances, which is why it
    // is driven by a timer and not by the level changing.
    let mut st = State::default();
    st.level = 0.3;
    st.tick();
    st.tick();
    assert_eq!(&st.bars()[BARS - 2..], &[0.3, 0.3]);
}

#[test]
fn the_meter_empties_when_recording_ends() {
    let mut st = State::default();
    st.press();
    st.level = 0.8;
    st.tick();
    st.starting = false;
    st.stop(false);
    assert_eq!(st.bars(), &[0.0; BARS]);
    assert_eq!(st.level, 0.0);
}

#[test]
fn reads_the_phases_core_reports() {
    assert_eq!(Phase::of("recording"), Phase::Recording);
    assert_eq!(Phase::of("transcribing"), Phase::Transcribing);
    assert_eq!(Phase::of("idle"), Phase::Idle);
    assert_eq!(Phase::of("something newer"), Phase::Idle);
}

// ---- voiceConfig ------------------------------------------------------------------------------

#[test]
fn a_local_dictation_uses_the_local_model() {
    let settings = Settings {
        voice_engine: VoiceEngine::Local,
        voice_model: "whisper-small-q5_1".into(),
        voice_cloud_model: "whisper-large-v3-turbo".into(),
        ..Settings::default()
    };
    let config = voice_config(&settings);
    assert!(matches!(config.engine, EngineKind::Local));
    assert_eq!(config.model, "whisper-small-q5_1");
}

#[test]
fn a_cloud_dictation_uses_the_cloud_model() {
    let settings = Settings {
        voice_engine: VoiceEngine::Groq,
        ..Settings::default()
    };
    let config = voice_config(&settings);
    assert!(matches!(config.engine, EngineKind::Groq));
    assert_eq!(config.model, "whisper-large-v3-turbo");
}

#[test]
fn an_empty_language_lets_the_model_decide() {
    let mut settings = Settings::default();
    assert_eq!(voice_config(&settings).language, None);
    settings.voice_language = "fr".into();
    assert_eq!(voice_config(&settings).language.as_deref(), Some("fr"));
}

#[test]
fn the_cleanup_settings_travel_with_the_config() {
    let settings = Settings {
        voice_cleanup: true,
        voice_cleanup_model: "llama".into(),
        voice_cleanup_hint: "Hangar, Groq".into(),
        ..Settings::default()
    };
    let config = voice_config(&settings);
    assert!(config.cleanup);
    assert_eq!(config.cleanup_model, "llama");
    assert_eq!(config.cleanup_hint, "Hangar, Groq");
}
