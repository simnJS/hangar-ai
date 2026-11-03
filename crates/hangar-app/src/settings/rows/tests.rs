//! What a row reads and reports: the value a slider settles on, the text a
//! field is given.

use super::*;
use crate::settings::experiments::Platform;
use crate::settings::schema::{Context, build_categories};
use crate::state::types::AgentId;
use hangar_i18n::{Locale, Translator};
use hangar_ui::shortcut::Platform as KeyboardPlatform;
use serde_json::Value as Json;

#[test]
fn a_slider_settles_on_a_multiple_of_its_step_without_the_noise_of_f32() {
    // 1.0 + 3 * 0.05 in single precision is 1.1500000238…
    let raw = 1.0_f32 + 3.0 * 0.05;
    assert_ne!(f64::from(raw), 1.15);
    assert_eq!(snap(raw, 0.05), 1.15);
    assert_eq!(snap(1.3, 0.05), 1.3);
    assert_eq!(snap(-0.5, 0.5), -0.5);
    assert_eq!(snap(2.5, 0.5), 2.5);
    assert_eq!(snap(13.0, 1.0), 13.0);
    assert_eq!(snap(700.0, 100.0), 700.0);
    assert_eq!(snap(10000.0, 1000.0), 10000.0);
    assert_eq!(snap(97.4999, 5.0), 97.0);
}

#[test]
fn the_decimals_of_a_step() {
    assert_eq!(decimals(1.0), 0);
    assert_eq!(decimals(100.0), 0);
    assert_eq!(decimals(0.5), 1);
    assert_eq!(decimals(0.05), 2);
}

#[test]
fn every_text_field_of_the_schema_shows_what_its_row_holds() {
    let mut settings = Settings::default();
    settings
        .agent_commands
        .insert("codex".into(), Json::String("codex --yolo".into()));
    settings.voice_cleanup_model = "llama".into();
    settings.voice_cleanup_hint = "Hangar, Tauri".into();
    settings.discord_app_id = "1234".into();
    let categories = build_categories(&Context {
        t: Translator::new(Locale::En),
        settings: &settings,
        shells: &[],
        workspace: None,
        version: "1",
        experiments: &[],
        platform: Some(Platform::Linux),
        keyboard: KeyboardPlatform::Other,
    });
    let mut seen = 0;
    for item in categories
        .iter()
        .flat_map(|c| &c.sections)
        .flat_map(|s| &s.items)
    {
        if let Kind::Input { value, target, .. } = &item.kind {
            assert_eq!(text_of(&settings, target), value, "{}", item.id);
            seen += 1;
        }
    }
    // Four launch commands, the cleanup model and vocabulary, the application.
    assert_eq!(seen, 7);
}

#[test]
fn a_launch_command_not_set_reads_as_nothing_and_not_as_the_default() {
    let settings = Settings::default();
    assert_eq!(
        text_of(&settings, &Target::AgentCommand(AgentId::Claude)),
        ""
    );
}

#[test]
fn a_target_that_is_no_text_field_reads_nothing() {
    assert_eq!(text_of(&Settings::default(), &Target::AutoResume), "");
}
