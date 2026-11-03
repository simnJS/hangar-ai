//! The toolbar of a window other than the main one: the bar draws the
//! workspace of its own window, its menus and questions open in the window
//! they were asked in, and what a control hands on names that window's
//! workspace. Also the shortcut hints of its tooltips, which the keymap fills.
#![allow(clippy::field_reassign_with_default)]

use std::time::Duration;

use hangar_i18n::{Key, Locale, Translator};

use super::*;
use crate::app::Packages;
use crate::navigator::testing::{
    Drive, ctx, ctx_in, detach, model_of, pane, simulate, workspace,
};
use crate::state::live::WindowUi;
use crate::state::types::{AgentId, AppState};

const EN: Translator = Translator::new(Locale::En);

/// Workspace `a` (one pane) in the main window, workspace `b` (two panes) in
/// a window of its own. The person is in the main window.
fn two_windows() -> (Model, window::Id, window::Id) {
    let mut model = model_of(AppState {
        workspaces: vec![
            workspace("a", "alpha", None, vec![pane("a1", "Ava", AgentId::Claude)]),
            workspace(
                "b",
                "beta",
                None,
                vec![
                    pane("b1", "Bo", AgentId::Claude),
                    pane("b2", "Cy", AgentId::Shell),
                ],
            ),
        ],
        active_workspace_id: Some("a".into()),
        ..AppState::default()
    });
    model.ui.view = View::Terminals;
    let main = model.main_window().unwrap();
    let own = detach(&mut model, "b");
    model.ui.focused_window = Some(main);
    model.ui.parked.insert(
        own,
        WindowUi {
            view: View::Terminals,
            ..WindowUi::default()
        },
    );
    (model, main, own)
}

fn messages_of(
    st: &State,
    model: &Model,
    window: window::Id,
    width: f32,
    drive: impl FnOnce(&mut iced_test::Simulator<'_, Message>),
) -> Vec<Message> {
    let pkg = Packages::default();
    let mut ui = simulate(
        iced::widget::container(view(st, ctx_in(model, &pkg, window)))
            .width(width)
            .height(44),
    );
    drive(&mut ui);
    ui.into_messages().collect()
}

#[test]
fn every_windows_toolbar_names_its_own_workspace() {
    let (model, main, own) = two_windows();
    let pkg = Packages::default();
    let st = State::default();

    let mut mine = simulate(view(&st, ctx_in(&model, &pkg, own)));
    assert!(mine.find("beta").is_ok());
    assert!(mine.find("alpha").is_err());
    drop(mine);

    let mut main_bar = simulate(view(&st, ctx_in(&model, &pkg, main)));
    assert!(main_bar.find("alpha").is_ok());
    assert!(main_bar.find("beta").is_err());
}

#[test]
fn a_main_window_whose_workspace_is_in_a_window_of_its_own_has_no_bar() {
    let (mut model, main, _) = two_windows();
    let mut state = model.state().clone();
    state.active_workspace_id = Some("b".into());
    model.hydrate(state);
    let pkg = Packages::default();
    let st = State::default();
    let mut ui = simulate(view(&st, ctx_in(&model, &pkg, main)));
    assert!(ui.find("beta").is_err());
    assert!(ui.find("Terminals").is_err());
}

#[test]
fn a_menu_opened_in_one_window_is_not_open_in_the_other() {
    let (mut model, main, own) = two_windows();
    let mut st = State::default();
    let _ = step(&mut st, &mut model, Msg::ToggleMore(own));
    assert_eq!(st.more_open, Some(own));

    // The other window's button takes the menu over instead of opening a
    // second one, and pressing the open one's closes it.
    let _ = step(&mut st, &mut model, Msg::ToggleMore(main));
    assert_eq!(st.more_open, Some(main));
    let _ = step(&mut st, &mut model, Msg::ToggleMore(main));
    assert_eq!(st.more_open, None);

    // Kits menus are per window and per workspace.
    let _ = step(&mut st, &mut model, Msg::ToggleKits(own, "b".into()));
    assert_eq!(st.kits_for, Some((own, "b".to_string())));
    let _ = step(&mut st, &mut model, Msg::ToggleKits(main, "a".into()));
    assert_eq!(st.kits_for, Some((main, "a".to_string())));
    let _ = step(&mut st, &mut model, Msg::Pick(Vec::new()));
    assert_eq!((st.more_open, st.kits_for), (None, None));
}

#[test]
fn restart_all_from_a_workspace_window_restarts_only_its_workspaces_panes() {
    let (model, _, own) = two_windows();
    let pkg = Packages::default();
    let ctx = ctx_in(&model, &pkg, own);
    // The « … » menu of a narrow bar lists the terminal actions: New
    // terminal, Tidy, then Restart all (26 px lines after 5 px of padding).
    let entries = menus::more_entries(ctx, ctx.workspace().unwrap(), true, true, false, true);
    let mut ui = simulate(ui_menu::menu(ctx.theme, entries));
    ui.move_to(40.0, 70.0);
    ui.press(iced::mouse::Button::Left);
    ui.release(iced::mouse::Button::Left);
    let messages: Vec<Message> = ui.into_messages().collect();
    let picked: Vec<&Vec<Intent>> = messages
        .iter()
        .filter_map(|message| match message {
            Message::Toolbar(Msg::Pick(list)) => Some(list),
            _ => None,
        })
        .collect();
    match picked.as_slice() {
        [list] => match list.as_slice() {
            [Intent::RestartPanes { ws, panes }] => {
                assert_eq!(ws, "b");
                assert_eq!(panes, &["b1".to_string(), "b2".to_string()]);
            }
            other => panic!("expected one restart, got {other:?}"),
        },
        other => panic!("expected one pick, got {other:?}"),
    }
}

#[test]
fn an_armed_reset_is_asked_in_the_window_it_was_armed_in_about_that_windows_workspace() {
    let (mut model, main, own) = two_windows();
    model.ui.confirm_reset = true;
    let mut st = State::default();
    st.asking_in = Some(own);
    let pkg = Packages::default();
    let title = EN
        .t_with(Key::ToolbarResetTitle, &hangar_i18n::args!("n" => 1))
        .into_owned();

    // The window the command ran in asks, and a confirmation resets its own
    // workspace.
    let mut mine = simulate(view(&st, ctx_in(&model, &pkg, own)));
    assert!(mine.find(title.as_str()).is_ok(), "{title}");
    drop(mine);
    let confirm = EN
        .t_with(Key::TopbarResetAllConfirm, &hangar_i18n::args!("n" => 1))
        .into_owned();
    let confirmed = messages_of(&st, &model, own, 1100.0, |ui| {
        ui.click(confirm.as_str()).expect("the question is shown");
    });
    assert!(
        confirmed.iter().any(|message| matches!(
            message,
            Message::Intent(Intent::ResetAgents(ws)) if ws == "b"
        )),
        "{confirmed:?}"
    );

    // The other window does not ask about its own workspace.
    let mut other = simulate(view(&st, ctx_in(&model, &pkg, main)));
    assert!(other.find(title.as_str()).is_err());
}

#[test]
fn a_reset_armed_by_a_command_is_asked_where_the_command_ran() {
    let (mut model, main, own) = two_windows();
    model.ui.confirm_reset = true;
    // The shell did not say which window (nothing recorded): the focused one.
    let st = State::default();
    let pkg = Packages::default();
    let title = EN
        .t_with(Key::ToolbarResetTitle, &hangar_i18n::args!("n" => 1))
        .into_owned();
    let mut focused = simulate(view(&st, ctx_in(&model, &pkg, main)));
    assert!(focused.find(title.as_str()).is_ok());
    drop(focused);
    let mut other = simulate(view(&st, ctx_in(&model, &pkg, own)));
    assert!(other.find(title.as_str()).is_err());
}

#[test]
fn a_preset_of_a_workspace_window_applies_to_that_workspace() {
    let (model, _, _) = two_windows();
    // The main window's active workspace is `a`; the press came from `b`'s bar.
    match preset_step(&model, "b", 1) {
        Some(PresetStep::Apply(Intent::ApplyPreset { ws, size })) => {
            assert_eq!(ws, "b");
            assert_eq!(size, 1);
        }
        other => panic!("expected the preset to apply, got {other:?}"),
    }
    assert!(preset_step(&model, "gone", 1).is_none());
}

#[test]
fn a_counter_brings_the_terminals_of_its_own_windows_forward() {
    let (mut model, main, own) = two_windows();
    // The workspace window shows the board; the main window the terminals.
    model.ui.parked.get_mut(&own).unwrap().view = View::Board;
    let from_own = go_to(&model, "b".into(), "b1".into());
    assert!(matches!(from_own.as_slice(), [
        Intent::SetView(View::Terminals),
        Intent::FocusPane { ws, pane, .. },
    ] if ws == "b" && pane == "b1"));
    // Its own window's view decides, not the focused window's.
    let from_main = go_to(&model, "a".into(), "a1".into());
    assert!(matches!(from_main.as_slice(), [Intent::FocusPane { ws, .. }] if ws == "a"));
    let _ = main;
}

// ---- The shortcut hints ------------------------------------------------------------------------

#[test]
fn the_toolbar_asks_the_keymap_for_a_commands_shortcut() {
    let (model, main, _) = two_windows();
    let pkg = Packages::default();
    let ctx = ctx_in(&model, &pkg, main);
    assert_eq!(
        reads::binding(&ctx, CommandId::ViewBoard),
        "Mod+Alt+B",
        "the binding is handed over as the settings write it"
    );
    assert_eq!(reads::binding(&ctx, CommandId::ViewSettings), "Mod+,");
}

/// The toolbar with the pointer resting on its Board button until the tip is
/// up, drawn, as a hash of the pixels: kept at `at` the first time, compared
/// with it after.
///
/// The tip is an overlay that iced offers no way to read the text of, so the
/// frame itself is what a test can compare.
fn tip_frame_matches(model: &Model, at: &std::path::Path) -> bool {
    let pkg = Packages::default();
    let st = State::default();
    let ctx = ctx(model, &pkg);
    let theme = hangar_theme::to_iced(ctx.theme);
    let mut ui = simulate(view(&st, ctx));
    let board = ui.find("Board").expect("the view is offered").bounds();
    ui.move_to(board.center_x(), board.center_y());
    // The tip shows once the pointer has rested on the control for a while,
    // on the toolkit's own clock, and the next event is what notices.
    std::thread::sleep(Duration::from_millis(600));
    ui.move_to(board.center_x() + 1.0, board.center_y());
    ui.snapshot(&theme)
        .expect("the interface can be drawn")
        .matches_hash(at)
        .expect("the hash can be kept")
}

fn one_workspace() -> Model {
    model_of(AppState {
        workspaces: vec![workspace("a", "alpha", None, vec![])],
        active_workspace_id: Some("a".into()),
        ..AppState::default()
    })
}

#[test]
fn a_view_buttons_tooltip_ends_with_its_shortcut() {
    let model = one_workspace();
    // Board is bound by default. The same bar with the key taken off it is
    // another picture, since its tip is shorter by the shortcut.
    let mut unbound = one_workspace();
    let overrides = match serde_json::json!({ "view.board": [] }) {
        serde_json::Value::Object(map) => map,
        _ => unreachable!(),
    };
    unbound.live.keymap.index = std::sync::Arc::new(crate::keymap::KeymapIndex::new(&overrides));
    assert_eq!(
        unbound
            .live
            .keymap
            .index
            .first_binding(CommandId::ViewBoard),
        None
    );

    let folder = std::env::temp_dir().join(format!("hangar-toolbar-tip-{}", std::process::id()));
    let frame = folder.join("board-tip");
    let first = tip_frame_matches(&model, &frame);
    let again = tip_frame_matches(&model, &frame);
    let without = tip_frame_matches(&unbound, &frame);
    let _ = std::fs::remove_dir_all(&folder);
    assert!(first, "the first frame is the one kept");
    assert!(again, "the same bar draws the same tip");
    assert!(!without, "the tip loses its shortcut with the binding");
}
