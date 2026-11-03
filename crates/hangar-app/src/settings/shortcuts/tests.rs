//! The editor, driven without a window: what each message asks of the
//! settings and of the dispatcher, what the rows list for a query, and which
//! keys they flag.

use super::*;
use hangar_i18n::{Locale, Translator};
use hangar_ui::shortcut::{Chord, Held};
use serde_json::json;

fn editor(overrides: serde_json::Value) -> (State, Overrides, Keymap) {
    let overrides = overrides.as_object().cloned().unwrap_or_default();
    let keymap = resolve(&overrides);
    (State::default(), overrides, keymap)
}

fn press(key: &str) -> Input {
    Input::Press {
        chord: Some(Chord::new(key)),
        held: Held::default(),
    }
}

fn run(
    st: &mut State,
    overrides: &Overrides,
    keymap: &Keymap,
    msgs: impl IntoIterator<Item = Msg>,
) -> Vec<Effect> {
    msgs.into_iter()
        .map(|msg| step(st, overrides, keymap, msg))
        .collect()
}

// ---- Recording ---------------------------------------------------------------------------------------

#[test]
fn recording_takes_the_keyboard_from_the_dispatcher_and_gives_it_back() {
    let (mut st, overrides, keymap) = editor(json!({}));
    let effects = run(
        &mut st,
        &overrides,
        &keymap,
        [Msg::Record(CommandId::PaneSplit), Msg::Cancel],
    );
    assert_eq!(effects[0].suspend, Some(true));
    assert!(effects[0].write.is_none());
    assert_eq!(effects[1].suspend, Some(false));
    assert!(!st.is_recording());
}

#[test]
fn moving_to_another_row_keeps_the_one_suspension() {
    let (mut st, overrides, keymap) = editor(json!({}));
    let effects = run(
        &mut st,
        &overrides,
        &keymap,
        [
            Msg::Record(CommandId::PaneSplit),
            Msg::Record(CommandId::PaneClose),
        ],
    );
    assert_eq!(effects[0].suspend, Some(true));
    assert_eq!(effects[1].suspend, None);
    assert_eq!(
        st.recording.as_ref().map(|(id, _)| *id),
        Some(CommandId::PaneClose)
    );
}

#[test]
fn cancelling_with_nothing_recording_lets_go_of_nothing() {
    let (mut st, overrides, keymap) = editor(json!({}));
    let effect = step(&mut st, &overrides, &keymap, Msg::Cancel);
    assert_eq!(effect, Effect::default());
}

#[test]
fn a_recorded_key_is_added_next_to_the_defaults_on_enter() {
    let (mut st, overrides, keymap) = editor(json!({}));
    let effects = run(
        &mut st,
        &overrides,
        &keymap,
        [
            Msg::Record(CommandId::PaneSplit),
            Msg::Input(press("F9")),
            Msg::Input(press("Enter")),
        ],
    );
    assert_eq!(effects[1], Effect::default());
    let done = &effects[2];
    assert_eq!(done.suspend, Some(false));
    let written = done.write.as_ref().expect("a write");
    let mut expected = keymap::default_bindings_on(
        hangar_ui::shortcut::Platform::current(),
        CommandId::PaneSplit,
    );
    expected.push("F9".into());
    assert_eq!(written["pane.split"], json!(expected));
    assert!(!st.is_recording());
}

#[test]
fn the_check_button_confirms_what_was_recorded() {
    let (mut st, overrides, keymap) = editor(json!({}));
    let effects = run(
        &mut st,
        &overrides,
        &keymap,
        [
            Msg::Record(CommandId::PaneClose),
            Msg::Input(press("F10")),
            Msg::Confirm,
        ],
    );
    assert!(effects[2].write.is_some());
    assert_eq!(effects[2].suspend, Some(false));
}

#[test]
fn nothing_is_confirmed_before_a_key_was_pressed() {
    let (mut st, overrides, keymap) = editor(json!({}));
    let effects = run(
        &mut st,
        &overrides,
        &keymap,
        [
            Msg::Record(CommandId::PaneClose),
            Msg::Confirm,
            Msg::Input(press("Enter")),
        ],
    );
    assert_eq!(effects[1], Effect::default());
    assert_eq!(effects[2], Effect::default());
    assert!(st.is_recording());
}

#[test]
fn escape_while_recording_cancels_without_writing() {
    let (mut st, overrides, keymap) = editor(json!({}));
    let effects = run(
        &mut st,
        &overrides,
        &keymap,
        [
            Msg::Record(CommandId::PaneClose),
            Msg::Input(press("F10")),
            Msg::Input(press("Escape")),
        ],
    );
    assert_eq!(effects[2].write, None);
    assert_eq!(effects[2].suspend, Some(false));
    assert!(!st.is_recording());
}

#[test]
fn a_sequence_of_two_chords_is_recorded_as_one_binding() {
    let (mut st, overrides, keymap) = editor(json!({}));
    let ctrl = |key: &str| Input::Press {
        chord: Some(Chord::new(key).ctrl()),
        held: Held {
            ctrl: true,
            ..Held::default()
        },
    };
    let effects = run(
        &mut st,
        &overrides,
        &keymap,
        [
            Msg::Record(CommandId::PaneClose),
            Msg::Input(ctrl("J")),
            Msg::Input(ctrl("L")),
            Msg::Input(press("Enter")),
        ],
    );
    let written = effects[3].write.as_ref().expect("a write");
    assert!(
        written["pane.close"]
            .as_array()
            .is_some_and(|list| list.iter().any(|b| b == "Ctrl+J Ctrl+L"))
    );
}

#[test]
fn typing_in_the_search_ends_a_recording() {
    for ending in [Msg::Query("split".into()), Msg::SearchPressed] {
        let (mut st, overrides, keymap) = editor(json!({}));
        let effects = run(
            &mut st,
            &overrides,
            &keymap,
            [Msg::Record(CommandId::PaneClose), ending],
        );
        assert_eq!(effects[1].suspend, Some(false));
        assert!(!st.is_recording());
    }
}

#[test]
fn leaving_the_page_lets_go_only_if_something_was_recording() {
    let mut st = State::default();
    st.recording = Some((CommandId::PaneClose, Recorder::new()));
    st.query = "x".into();
    let _ = st.leave();
    assert!(!st.is_recording());
    assert!(st.query.is_empty());
}

// ---- Editing -----------------------------------------------------------------------------------------

#[test]
fn the_cross_takes_a_key_off_a_command_and_keeps_it_an_override() {
    let (mut st, overrides, keymap) = editor(json!({}));
    let effect = step(
        &mut st,
        &overrides,
        &keymap,
        Msg::Remove(CommandId::PaneSplit, 0),
    );
    let written = effect.write.expect("a write");
    assert_eq!(written["pane.split"], json!([]));
}

#[test]
fn a_cross_on_a_key_that_is_not_there_does_nothing() {
    let (mut st, overrides, keymap) = editor(json!({}));
    let effect = step(
        &mut st,
        &overrides,
        &keymap,
        Msg::Remove(CommandId::PaneSplit, 7),
    );
    assert_eq!(effect, Effect::default());
}

#[test]
fn reset_puts_one_command_back_on_its_defaults() {
    let (mut st, overrides, keymap) = editor(json!({"pane.split": ["F9"], "pane.close": []}));
    let effect = step(
        &mut st,
        &overrides,
        &keymap,
        Msg::Reset(CommandId::PaneSplit),
    );
    assert_eq!(
        effect.write.expect("a write"),
        json!({"pane.close": []}).as_object().cloned().unwrap()
    );
}

#[test]
fn freeing_a_key_takes_it_from_the_other_commands_that_answer_to_it() {
    let (mut st, overrides, keymap) = editor(json!({"pane.split": ["F9"], "pane.close": ["F9"]}));
    let effect = step(
        &mut st,
        &overrides,
        &keymap,
        Msg::Free(CommandId::PaneSplit, 0),
    );
    let written = effect.write.expect("a write");
    assert_eq!(written["pane.split"], json!(["F9"]));
    assert_eq!(written["pane.close"], json!([]));
}

#[test]
fn reset_all_asks_twice_and_clears_every_override() {
    let (mut st, overrides, keymap) = editor(json!({"pane.split": ["F9"], "pane.close": []}));
    assert_eq!(
        step(&mut st, &overrides, &keymap, Msg::AskResetAll(true)),
        Effect::default()
    );
    assert!(st.confirm_reset);
    assert_eq!(
        step(&mut st, &overrides, &keymap, Msg::AskResetAll(false)),
        Effect::default()
    );
    assert!(!st.confirm_reset);
    st.confirm_reset = true;
    let effect = step(&mut st, &overrides, &keymap, Msg::ResetAll);
    assert_eq!(effect.write, Some(Overrides::new()));
    assert!(!st.confirm_reset);
}

#[test]
fn the_rows_follow_the_settings_when_they_change() {
    let mut st = State::default();
    let mut settings = Settings::default();
    st.refresh(&settings);
    let first = &st.cache.as_ref().expect("cache").keymap;
    assert_eq!(bindings_of(first, CommandId::PaneSplit).len(), 1);
    settings
        .keybindings
        .insert("pane.split".into(), json!(["F9", "F10"]));
    st.refresh(&settings);
    let second = &st.cache.as_ref().expect("cache").keymap;
    assert_eq!(bindings_of(second, CommandId::PaneSplit), ["F9", "F10"]);
}

// ---- What the rows list ------------------------------------------------------------------------------

fn listed(
    locale: Locale,
    overrides: serde_json::Value,
    query: &str,
) -> Vec<(Section, Vec<CommandId>)> {
    let (_, _, keymap) = editor(overrides);
    with_ctx(locale, |ctx| groups(ctx, &keymap, query))
}

/// A `Ctx` for a test: the shell builds one per window; this one is made of
/// the same parts around an empty model.
fn with_ctx<T>(locale: Locale, work: impl FnOnce(Ctx<'_>) -> T) -> T {
    use crate::app::Packages;
    use crate::ctx::Metrics;
    use crate::keymap::KeymapIndex;
    let model = Model::default();
    let pkg = Packages::default();
    let index = KeymapIndex::default();
    let ctx = Ctx {
        model: &model,
        pkg: &pkg,
        i18n: Translator::new(locale),
        theme: hangar_theme::default_theme(),
        metrics: Metrics::default(),
        keymap: &index,
        window: iced::window::Id::unique(),
        now: std::time::Instant::now(),
    };
    work(ctx)
}

#[test]
fn without_a_query_every_command_is_listed_by_section() {
    let all = listed(Locale::En, json!({}), "");
    let sections: Vec<Section> = all.iter().map(|(section, _)| *section).collect();
    assert_eq!(sections, Section::ALL);
    let total: usize = all.iter().map(|(_, commands)| commands.len()).sum();
    assert_eq!(total, 88);
    assert!(all[0].1.contains(&CommandId::PaneSplit));
}

#[test]
fn a_query_keeps_the_commands_whose_label_keys_or_section_match() {
    let by_label = listed(Locale::En, json!({}), "fill the grid");
    let ids: Vec<CommandId> = by_label.iter().flat_map(|(_, c)| c.clone()).collect();
    assert!(ids.contains(&CommandId::PaneZoom));
    assert!(!ids.contains(&CommandId::PaneClose));
    let text = listed(Locale::En, json!({}), "BIGGER text");
    let ids: Vec<CommandId> = text.iter().flat_map(|(_, c)| c.clone()).collect();
    assert_eq!(ids, [CommandId::AppZoomIn]);

    let by_key = listed(Locale::En, json!({"pane.close": ["F9"]}), "f9");
    let ids: Vec<CommandId> = by_key.iter().flat_map(|(_, c)| c.clone()).collect();
    assert!(ids.contains(&CommandId::PaneClose));

    let by_section = listed(Locale::En, json!({}), "terminal");
    assert!(
        by_section
            .iter()
            .any(|(section, _)| *section == Section::Terminal)
    );
    let terminal = by_section
        .iter()
        .find(|(section, _)| *section == Section::Terminal)
        .map(|(_, commands)| commands.len());
    // All seven, and any other command with the word in its label.
    assert!(terminal.is_some_and(|count| count >= 7));
}

#[test]
fn accents_and_case_are_ignored_in_the_query() {
    // "Réglages" in French has an accent a keyboard without one cannot type.
    let accented = listed(Locale::Fr, json!({}), "RÉGLAGES");
    let plain = listed(Locale::Fr, json!({}), "reglages");
    assert!(!plain.is_empty());
    assert_eq!(accented, plain);
}

#[test]
fn a_query_nothing_matches_lists_nothing() {
    assert!(listed(Locale::En, json!({}), "no such command anywhere").is_empty());
}

#[test]
fn the_digit_families_share_one_label_with_their_number() {
    with_ctx(Locale::En, |ctx| {
        let three = command_label(ctx, CommandId::PaneFocus3);
        let four = command_label(ctx, CommandId::PaneFocus4);
        assert!(three.contains('3'), "{three}");
        assert!(four.contains('4'), "{four}");
        assert_ne!(three, four);
        assert_eq!(
            command_label(ctx, CommandId::PaneSplit),
            ctx.t(Key::CmdPaneSplit)
        );
    });
}

#[test]
fn a_key_two_commands_answer_to_flags_both_rows() {
    let (_, _, keymap) = editor(json!({"pane.split": ["F9"], "pane.close": ["F9"]}));
    assert_eq!(
        rivals(&keymap, CommandId::PaneSplit),
        [(0, vec![CommandId::PaneClose])]
    );
    assert_eq!(
        rivals(&keymap, CommandId::PaneClose),
        [(0, vec![CommandId::PaneSplit])]
    );
    assert!(rivals(&keymap, CommandId::PaneNext).is_empty());
}
