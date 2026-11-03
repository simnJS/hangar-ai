//! The sheet driven with real events, through the simulator of `iced_test`:
//! what a click, a key or some typing says to the settings.

use std::borrow::Cow;

use iced::keyboard::key::Named;
use iced::{Settings as WindowSettings, Size};
use iced_test::Simulator;

use super::*;
use crate::app::Packages;
use crate::ctx::{Ctx, Metrics};
use crate::state::types::{AppState, Workspace};

fn model(category: SettingsCategory) -> Model {
    let mut model = Model::default();
    model.hydrate(AppState {
        workspaces: vec![Workspace {
            id: "w1".into(),
            name: "Shop".into(),
            ..Workspace::default()
        }],
        active_workspace_id: Some("w1".into()),
        ..AppState::default()
    });
    model.ui.overlays = vec![Overlay::Settings(category)];
    model
}

/// What the sheet says after `act`, as words: a message of the app has no
/// equality, and these read well in a failure.
fn said(
    category: SettingsCategory,
    st: State,
    act: impl FnOnce(&mut Simulator<'_, Message>),
) -> Vec<String> {
    let model = model(category);
    let pkg = Packages::default();
    let ctx = Ctx {
        model: &model,
        pkg: &pkg,
        i18n: Translator::new(hangar_i18n::Locale::En),
        theme: hangar_theme::default_theme(),
        metrics: Metrics::default(),
        keymap: &model.live.keymap.index,
        window: iced::window::Id::unique(),
        now: std::time::Instant::now(),
    };
    let window = WindowSettings {
        fonts: vec![Cow::Borrowed(hangar_theme::fonts::INTER_VARIABLE)],
        default_font: hangar_theme::fonts::ui(),
        ..WindowSettings::default()
    };
    let mut ui = Simulator::with_size(window, Size::new(1360.0, 860.0), view(&st, ctx, category));
    act(&mut ui);
    ui.into_messages()
        .map(|message| format!("{message:?}"))
        .collect()
}

#[test]
fn clicking_a_category_of_the_list_picks_it() {
    let words = said(SettingsCategory::General, State::default(), |ui| {
        ui.click("Appearance").expect("the entry is on the list");
    });
    assert_eq!(words, ["Settings(Pick(Appearance))"]);
}

#[test]
fn clicking_a_switch_flips_it() {
    let words = said(SettingsCategory::General, State::default(), |ui| {
        ui.click("Auto resume").expect("the row is on the page");
    });
    // Auto resume is on by default: the click turns it off. Whether it came
    // from the switch or from the text beside it, it is one change.
    assert_eq!(
        words,
        ["Settings(Change(Set(AutoResume, Bool(false))))"]
    );
}

#[test]
fn clicking_a_theme_card_picks_the_theme() {
    let words = said(SettingsCategory::Appearance, State::default(), |ui| {
        ui.click("Nord").expect("the card is on the page");
    });
    assert_eq!(
        words,
        [r#"Settings(Change(Set(Theme, Text("nord"))))"#]
    );
}

#[test]
fn clicking_a_font_card_picks_the_stack() {
    let words = said(SettingsCategory::Terminal, State::default(), |ui| {
        ui.click("Hack").expect("the card is on the page");
    });
    assert!(
        words
            .iter()
            .any(|word| word.contains("FontFamily") && word.contains("Hack")),
        "{words:?}"
    );
}

#[test]
fn an_armed_reset_offers_to_cancel_or_to_confirm() {
    let armed = || State {
        armed: Some("reset".into()),
        ..State::default()
    };
    // Cancel is the second "Cancel" nowhere else on the page: the first
    // "Cancel" of an armed row.
    let cancelled = said(SettingsCategory::About, armed(), |ui| {
        ui.click("Cancel").expect("the armed row offers to cancel");
    });
    assert_eq!(cancelled, ["Settings(Disarm)"]);
    let confirmed = said(SettingsCategory::About, armed(), |ui| {
        ui.click("Confirm").expect("the armed row offers to confirm");
    });
    // The one button that does it.
    assert_eq!(confirmed, ["Settings(Act(ResetSettings))"]);
}

#[test]
fn a_reset_that_is_not_armed_has_no_confirm_button() {
    let words = said(SettingsCategory::About, State::default(), |ui| {
        assert!(ui.find("Confirm").is_err());
        assert!(ui.find("Cancel").is_err());
    });
    assert!(words.is_empty());
}

// ---- Escape ------------------------------------------------------------------------------------------

#[test]
fn escape_is_the_sheets_own() {
    let words = said(SettingsCategory::General, State::default(), |ui| {
        ui.tap_key(Named::Escape);
    });
    assert_eq!(words, ["Settings(Escape)"]);
}

#[test]
fn escape_is_the_recorders_while_a_shortcut_is_being_recorded() {
    let mut st = State::default();
    let index = crate::keymap::KeymapIndex::default();
    shortcuts::step(
        &mut st.shortcuts,
        &index,
        shortcuts::Msg::Record(crate::commands::CommandId::PaneSplit),
    );
    assert!(st.shortcuts.is_recording());
    let words = said(SettingsCategory::Shortcuts, st, |ui| {
        ui.tap_key(Named::Escape);
    });
    // The recorder takes it (as a key to record, which its own rule turns into
    // a cancellation); the sheet's own Escape is not sent, so the search is not
    // cleared nor the sheet closed under the recording.
    assert!(
        words
            .iter()
            .any(|word| word.starts_with("Settings(Shortcuts(Input(Press")),
        "{words:?}"
    );
    assert!(!words.iter().any(|word| word == "Settings(Escape)"), "{words:?}");
}

#[test]
fn up_and_down_walk_the_list_only_after_it_was_used() {
    let before = said(SettingsCategory::General, State::default(), |ui| {
        ui.tap_key(Named::ArrowDown);
    });
    assert!(before.is_empty(), "{before:?}");
    let used = State {
        nav_active: true,
        ..State::default()
    };
    let after = said(SettingsCategory::General, used, |ui| {
        ui.tap_key(Named::ArrowDown);
    });
    assert_eq!(after, ["Settings(Step(Next))"]);
}

#[test]
fn typing_in_the_search_filters() {
    let words = said(SettingsCategory::General, State::default(), |ui| {
        // The sheet opens with the keyboard on its search field.
        ui.tap_key(Named::Shift);
        ui.typewrite("th");
    });
    assert!(
        words.iter().any(|word| word.starts_with("Settings(Query(")),
        "{words:?}"
    );
}
