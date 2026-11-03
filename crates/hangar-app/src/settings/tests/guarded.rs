//! The sheet's text fields, each given Mod+Shift+Enter: the keymap's shortcut
//! must be spent before the field sees it, so that it runs without also
//! typing or submitting in the field (`keymap::guard`).

use super::*;
use crate::settings::guard_testing::Fixture;

fn fixture() -> Fixture {
    let mut fixture = Fixture::new();
    fixture.model.ui.overlays = vec![Overlay::Settings(SettingsCategory::General)];
    fixture
}

#[test]
fn the_search_field_leaves_the_shortcut_to_the_keymap() {
    let fixture = fixture();
    // Appearance has no text field of its own: the one guard is the search's.
    let said = fixture.split_chord(view(
        &State::default(),
        fixture.ctx(),
        SettingsCategory::Appearance,
    ));
    assert_eq!(said, ["Noop"]);
}

#[test]
fn the_text_rows_leave_it_too() {
    let fixture = fixture();
    let st = State::default();
    // The categories with a row of text (the commands of the agents, the
    // Discord application, the cleanup model) hold more guards than the
    // search alone.
    let mut with_fields = 0;
    for category in schema::ORDER {
        let said = fixture.split_chord(view(&st, fixture.ctx(), category));
        assert!(
            said.iter().all(|word| word == "Noop") && !said.is_empty(),
            "{category:?}: {said:?}"
        );
        if said.len() > 1 {
            with_fields += 1;
        }
    }
    assert!(with_fields > 0, "no category has a text row to guard");
}

#[test]
fn the_projects_folder_field_leaves_it() {
    let fixture = fixture();
    let said = fixture.split_chord(previews::projects_folder(fixture.ctx()));
    assert_eq!(said, ["Noop"]);
}

#[test]
fn the_shortcut_search_leaves_it() {
    let fixture = fixture();
    fixture.assert_spends_the_split_chord(shortcuts::view(
        &shortcuts::State::default(),
        fixture.ctx(),
    ));
}

#[test]
fn the_voice_key_field_leaves_it() {
    let fixture = fixture();
    let st = voice_card::State::asking_for_a_key();
    let said = fixture.split_chord(voice_card::key_view(&st, fixture.ctx()));
    assert_eq!(said, ["Noop"]);
}
