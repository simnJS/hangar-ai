//! The editor, driven without a window: what each message asks of the keymap
//! and of the dispatcher, what the rows list for a query, and which keys they
//! flag. The keymap itself (resolving, editing, conflicts) is P7's, with its
//! own tests.

use super::*;
use crate::keymap::Msg as Asked;
use hangar_i18n::{Locale, Translator};
use hangar_ui::shortcut::{Chord, Held, Platform};
use serde_json::json;

fn index(overrides: serde_json::Value) -> KeymapIndex {
    let map = overrides.as_object().cloned().unwrap_or_default();
    KeymapIndex::on(Platform::Other, &map)
}

fn press(key: &str) -> Input {
    Input::Press {
        chord: Some(Chord::new(key)),
        held: Held::default(),
    }
}

fn run(st: &mut State, index: &KeymapIndex, msgs: impl IntoIterator<Item = Msg>) -> Vec<Effect> {
    msgs.into_iter().map(|msg| step(st, index, msg)).collect()
}

/// An effect as the words of its messages, since the keymap's `Msg` is not
/// comparable.
fn said(effect: &Effect) -> Vec<String> {
    effect.iter().map(|msg| format!("{msg:?}")).collect()
}

// ---- Recording ---------------------------------------------------------------------------------------

#[test]
fn recording_takes_the_keyboard_from_the_dispatcher_and_gives_it_back() {
    let (mut st, index) = (State::default(), index(json!({})));
    let effects = run(
        &mut st,
        &index,
        [Msg::Record(CommandId::PaneSplit), Msg::Cancel],
    );
    assert!(matches!(effects[0][..], [Asked::Suspend(RECORDING)]));
    assert!(matches!(effects[1][..], [Asked::Release(RECORDING)]));
    assert!(!st.is_recording());
}

#[test]
fn moving_to_another_row_keeps_the_one_suspension() {
    let (mut st, index) = (State::default(), index(json!({})));
    let effects = run(
        &mut st,
        &index,
        [
            Msg::Record(CommandId::PaneSplit),
            Msg::Record(CommandId::PaneClose),
        ],
    );
    assert_eq!(effects[0].len(), 1);
    assert!(effects[1].is_empty());
    assert_eq!(
        st.recording.as_ref().map(|(id, _)| *id),
        Some(CommandId::PaneClose)
    );
}

#[test]
fn cancelling_with_nothing_recording_lets_go_of_nothing() {
    let (mut st, index) = (State::default(), index(json!({})));
    assert!(step(&mut st, &index, Msg::Cancel).is_empty());
}

#[test]
fn a_recorded_key_is_bound_to_the_command_on_enter_and_the_dispatcher_let_go() {
    let (mut st, index) = (State::default(), index(json!({})));
    let effects = run(
        &mut st,
        &index,
        [
            Msg::Record(CommandId::PaneSplit),
            Msg::Input(press("F9")),
            Msg::Input(press("Enter")),
        ],
    );
    assert!(effects[1].is_empty());
    assert_eq!(
        said(&effects[2]),
        [
            r#"Bind { command: PaneSplit, binding: "F9" }"#.to_string(),
            format!("Release({RECORDING})")
        ]
    );
    assert!(!st.is_recording());
}

#[test]
fn the_check_button_confirms_what_was_recorded() {
    let (mut st, index) = (State::default(), index(json!({})));
    let effects = run(
        &mut st,
        &index,
        [
            Msg::Record(CommandId::PaneClose),
            Msg::Input(press("F10")),
            Msg::Confirm,
        ],
    );
    assert!(matches!(
        effects[2][..],
        [Asked::Bind { .. }, Asked::Release(_)]
    ));
}

#[test]
fn nothing_is_confirmed_before_a_key_was_pressed() {
    let (mut st, index) = (State::default(), index(json!({})));
    let effects = run(
        &mut st,
        &index,
        [
            Msg::Record(CommandId::PaneClose),
            Msg::Confirm,
            Msg::Input(press("Enter")),
        ],
    );
    assert!(effects[1].is_empty());
    assert!(effects[2].is_empty());
    assert!(st.is_recording());
}

#[test]
fn escape_while_recording_cancels_without_binding_anything() {
    let (mut st, index) = (State::default(), index(json!({})));
    let effects = run(
        &mut st,
        &index,
        [
            Msg::Record(CommandId::PaneClose),
            Msg::Input(press("F10")),
            Msg::Input(press("Escape")),
        ],
    );
    assert!(matches!(effects[2][..], [Asked::Release(_)]));
    assert!(!st.is_recording());
}

#[test]
fn a_sequence_of_two_chords_is_recorded_as_one_binding() {
    let (mut st, index) = (State::default(), index(json!({})));
    let ctrl = |key: &str| Input::Press {
        chord: Some(Chord::new(key).ctrl()),
        held: Held {
            ctrl: true,
            ..Held::default()
        },
    };
    let effects = run(
        &mut st,
        &index,
        [
            Msg::Record(CommandId::PaneClose),
            Msg::Input(ctrl("J")),
            Msg::Input(ctrl("L")),
            Msg::Input(press("Enter")),
        ],
    );
    assert_eq!(
        said(&effects[3])[0],
        r#"Bind { command: PaneClose, binding: "Ctrl+J Ctrl+L" }"#
    );
}

#[test]
fn typing_in_the_search_ends_a_recording() {
    for ending in [Msg::Query("split".into()), Msg::SearchPressed] {
        let (mut st, index) = (State::default(), index(json!({})));
        let effects = run(&mut st, &index, [Msg::Record(CommandId::PaneClose), ending]);
        assert!(matches!(effects[1][..], [Asked::Release(_)]));
        assert!(!st.is_recording());
    }
}

#[test]
fn leaving_the_page_forgets_the_search_and_the_recording() {
    let mut st = State {
        recording: Some((CommandId::PaneClose, Recorder::new())),
        query: "x".into(),
        ..State::default()
    };
    let _ = st.leave();
    assert!(!st.is_recording());
    assert!(st.query.is_empty());
}

// ---- Editing -----------------------------------------------------------------------------------------

#[test]
fn the_cross_takes_a_key_off_a_command() {
    let (mut st, index) = (
        State::default(),
        index(json!({"pane.split": ["F9", "F10"]})),
    );
    let effect = step(&mut st, &index, Msg::Remove(CommandId::PaneSplit, 1));
    assert_eq!(
        said(&effect),
        [r#"Unbind { command: PaneSplit, binding: "F10" }"#]
    );
}

#[test]
fn a_cross_on_a_key_that_is_not_there_does_nothing() {
    let (mut st, index) = (State::default(), index(json!({})));
    assert!(step(&mut st, &index, Msg::Remove(CommandId::PaneSplit, 7)).is_empty());
}

#[test]
fn reset_puts_one_command_back_on_its_defaults() {
    let (mut st, index) = (State::default(), index(json!({"pane.split": ["F9"]})));
    let effect = step(&mut st, &index, Msg::Reset(CommandId::PaneSplit));
    assert_eq!(said(&effect), ["Reset(PaneSplit)"]);
}

#[test]
fn freeing_a_key_takes_it_from_the_other_commands_that_answer_to_it() {
    let (mut st, index) = (
        State::default(),
        index(json!({"pane.split": ["F9"], "pane.close": ["F9"]})),
    );
    let effect = step(&mut st, &index, Msg::Free(CommandId::PaneSplit, 0));
    assert_eq!(
        said(&effect),
        [r#"Free { binding: "F9", commands: [PaneClose] }"#]
    );
}

#[test]
fn reset_all_asks_twice_and_clears_every_override() {
    let (mut st, index) = (
        State::default(),
        index(json!({"pane.split": ["F9"], "pane.close": []})),
    );
    assert!(step(&mut st, &index, Msg::AskResetAll(true)).is_empty());
    assert!(st.confirm_reset);
    assert!(step(&mut st, &index, Msg::AskResetAll(false)).is_empty());
    assert!(!st.confirm_reset);
    st.confirm_reset = true;
    let effect = step(&mut st, &index, Msg::ResetAll);
    assert_eq!(said(&effect), ["ResetAll"]);
    assert!(!st.confirm_reset);
}

// ---- What the rows list ------------------------------------------------------------------------------

fn listed(
    locale: Locale,
    overrides: serde_json::Value,
    query: &str,
) -> Vec<(Section, Vec<CommandId>)> {
    let index = index(overrides);
    with_ctx(locale, |ctx| groups(ctx, &index, query))
}

/// A `Ctx` for a test: the shell builds one per window; this one is made of
/// the same parts around an empty model.
fn with_ctx<T>(locale: Locale, work: impl FnOnce(Ctx<'_>) -> T) -> T {
    use crate::app::Packages;
    use crate::ctx::Metrics;
    let model = crate::state::Model::default();
    let pkg = Packages::default();
    let keymap = KeymapIndex::default();
    let ctx = Ctx {
        model: &model,
        pkg: &pkg,
        i18n: Translator::new(locale),
        theme: hangar_theme::default_theme(),
        metrics: Metrics::default(),
        keymap: &keymap,
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
    assert_eq!(total, CommandId::ALL.len());
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
    let index = index(json!({"pane.split": ["F9"], "pane.close": ["F9"]}));
    assert_eq!(
        rivals(&index, CommandId::PaneSplit),
        [(0, vec![CommandId::PaneClose])]
    );
    assert_eq!(
        rivals(&index, CommandId::PaneClose),
        [(0, vec![CommandId::PaneSplit])]
    );
    assert!(rivals(&index, CommandId::PaneNext).is_empty());
}
