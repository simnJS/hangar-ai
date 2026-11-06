//! The sheet's place on the overlay stack and what it asks the schema for.

use super::*;
use crate::state::types::AppState;

mod drawn;
mod driven;
mod guarded;

fn model_with(overlays: Vec<Overlay>) -> Model {
    let mut model = Model::default();
    model.ui.overlays = overlays;
    model
}

#[test]
fn no_sheet_is_open_until_one_is_on_the_stack() {
    assert_eq!(open_category(&Model::default()), None);
    let model = model_with(vec![Overlay::Palette]);
    assert_eq!(open_category(&model), None);
}

#[test]
fn the_sheet_is_found_under_the_dialog_it_opened() {
    let model = model_with(vec![
        Overlay::Settings(SettingsCategory::Voice),
        Overlay::KitActivation {
            kit: "k".into(),
            ws: None,
        },
    ]);
    assert_eq!(open_category(&model), Some(SettingsCategory::Voice));
}

#[test]
fn picking_a_category_changes_the_one_the_overlay_carries() {
    let mut model = model_with(vec![
        Overlay::Settings(SettingsCategory::General),
        Overlay::Palette,
    ]);
    go_to(&mut model, SettingsCategory::Terminal);
    assert_eq!(
        model.ui.overlays,
        [
            Overlay::Settings(SettingsCategory::Terminal),
            Overlay::Palette
        ]
    );
}

#[test]
fn asking_for_a_category_while_the_sheet_is_up_moves_to_it_and_stacks_nothing() {
    // The shell pushes the new sheet over the old one; the settings keep one.
    let mut model = model_with(vec![
        Overlay::Settings(SettingsCategory::General),
        Overlay::Settings(SettingsCategory::Shortcuts),
    ]);
    collapse_sheets(&mut model);
    assert_eq!(
        model.ui.overlays,
        [Overlay::Settings(SettingsCategory::Shortcuts)]
    );
    assert_eq!(open_category(&model), Some(SettingsCategory::Shortcuts));
    // One sheet is left as it is.
    collapse_sheets(&mut model);
    assert_eq!(model.ui.overlays.len(), 1);
}

#[test]
fn other_overlays_keep_their_place_when_sheets_collapse() {
    let mut model = model_with(vec![
        Overlay::Settings(SettingsCategory::General),
        Overlay::Palette,
        Overlay::Settings(SettingsCategory::About),
    ]);
    collapse_sheets(&mut model);
    assert_eq!(
        model.ui.overlays,
        [Overlay::Palette, Overlay::Settings(SettingsCategory::About)]
    );
}

#[test]
fn the_category_shown_is_the_wanted_one_unless_the_search_took_it_away() {
    let model = Model::default();
    let mut st = State::default();
    assert_eq!(
        shown_category(&st, &model, SettingsCategory::Voice),
        Some(SettingsCategory::Voice)
    );
    // Searching for the shortcuts leaves Shortcuts alone on the list.
    st.query = "raccourcis clavier".into();
    assert_eq!(
        shown_category(&st, &model, SettingsCategory::Voice),
        Some(SettingsCategory::Shortcuts)
    );
    st.query = "no such setting anywhere".into();
    assert_eq!(shown_category(&st, &model, SettingsCategory::Voice), None);
}

#[test]
fn the_stack_names_the_category_on_screen_and_goes_back_when_the_search_is_cleared() {
    let mut model = model_with(vec![Overlay::Settings(SettingsCategory::Kits)]);
    let mut st = State {
        chosen: SettingsCategory::Kits,
        ..State::default()
    };
    // Nothing searched: the chosen category is the one on screen.
    assert_eq!(follow_search(&st, &mut model), Some(SettingsCategory::Kits));
    // A search that finds only the shortcuts moves the page, and the stack
    // with it, so that nothing believes the kits are on screen.
    st.query = "raccourcis clavier".into();
    assert_eq!(
        follow_search(&st, &mut model),
        Some(SettingsCategory::Shortcuts)
    );
    assert_eq!(open_category(&model), Some(SettingsCategory::Shortcuts));
    // A search that finds nothing leaves the stack alone.
    st.query = "no such setting anywhere".into();
    assert_eq!(follow_search(&st, &mut model), None);
    assert_eq!(open_category(&model), Some(SettingsCategory::Shortcuts));
    // Cleared: back on what the person chose.
    st.query.clear();
    assert_eq!(follow_search(&st, &mut model), Some(SettingsCategory::Kits));
    assert_eq!(open_category(&model), Some(SettingsCategory::Kits));
}

#[test]
fn the_wanted_category_stays_while_the_search_still_finds_something_in_it() {
    let model = Model::default();
    let st = State {
        query: "microphone".into(),
        ..State::default()
    };
    assert_eq!(
        shown_category(&st, &model, SettingsCategory::Voice),
        Some(SettingsCategory::Voice)
    );
}

#[test]
fn the_schema_is_built_from_the_model_the_way_the_page_sees_it() {
    let mut model = Model::default();
    model.hydrate(AppState {
        workspaces: vec![crate::state::Workspace {
            id: "w1".into(),
            name: "Shop".into(),
            ..crate::state::Workspace::default()
        }],
        active_workspace_id: Some("w1".into()),
        ..AppState::default()
    });
    let built = categories(&model);
    let ids: Vec<&str> = built
        .iter()
        .flat_map(|c| &c.sections)
        .flat_map(|s| &s.items)
        .map(|item| item.id.as_str())
        .collect();
    // An active workspace brings its two rows.
    assert!(ids.contains(&"themeScope"));
    assert!(ids.contains(&"shellWorkspace"));
    // And the version is this build's.
    assert!(ids.contains(&"version"));
}

#[test]
fn every_category_of_the_shell_has_a_page() {
    // Experimental is left out while the registry is empty; it holds the
    // chat view, so every category of the shell is there.
    let built: Vec<SettingsCategory> = categories(&Model::default()).iter().map(|c| c.id).collect();
    for category in schema::ORDER {
        assert!(built.contains(&category), "{category:?}");
    }
}

#[test]
fn a_category_is_named_by_the_id_the_ts_used() {
    for category in schema::ORDER {
        assert_eq!(
            schema::ORDER
                .iter()
                .copied()
                .find(|other| other.id() == category.id()),
            Some(category)
        );
    }
    assert_eq!(SettingsCategory::Shortcuts.id(), "shortcuts");
}

#[test]
fn the_settings_asks_nothing_of_the_environment_in_a_release_build() {
    // `dev_category` reads the environment only in debug builds, so a release
    // never opens a sheet by itself.
    if !cfg!(debug_assertions) {
        assert_eq!(dev_category(), None);
    }
}

#[test]
fn a_sheet_that_left_the_stack_lets_go_of_the_keyboard_but_keeps_a_download() {
    // `Intent::FocusPane` clears the stack without `CloseOverlay`: the
    // reconcile that `on_intent` now runs for every intent ends up here.
    let mut st = State {
        open: true,
        shown: Some(SettingsCategory::Shortcuts),
        ..State::default()
    };
    let index = crate::keymap::KeymapIndex::default();
    shortcuts::step(
        &mut st.shortcuts,
        &index,
        shortcuts::Msg::Record(crate::commands::CommandId::PaneSplit),
    );
    st.voice
        .download_event(hangar_core::voice::models::Progress {
            id: "a".into(),
            received: 1,
            total: 2,
            done: false,
        });
    assert!(st.shortcuts.is_recording());
    let _release = close(&mut st);
    assert!(!st.open && st.shown.is_none());
    assert!(!st.shortcuts.is_recording());
    assert!(st.voice.is_downloading("a"));
    // Nothing open: nothing to do.
    let _ = close(&mut st);
}
