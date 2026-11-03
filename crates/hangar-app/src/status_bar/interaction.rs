//! The status bar driven with real pointer and key events: the broadcast
//! field, its two buttons, and the readouts.

use iced::keyboard::{Modifiers, key::Named};

use super::*;
use crate::app::Packages;
use crate::navigator::testing::{Drive, ctx, ctx_in, detach, model_of, pane, simulate, workspace};
use crate::state::types::{AgentId, AppState};

fn model(view: View) -> Model {
    let mut model = model_of(AppState {
        workspaces: vec![workspace(
            "w",
            "hangar",
            None,
            vec![
                pane("p1", "Ava", AgentId::Claude),
                pane("p2", "Ben", AgentId::Shell),
            ],
        )],
        active_workspace_id: Some("w".into()),
        ..AppState::default()
    });
    model.ui.view = view;
    model
}

/// A state whose main window's field holds `text`.
fn typed(model: &Model, text: &str) -> State {
    let mut st = State::default();
    st.set_broadcast_text(model.main_window().unwrap(), text.into());
    st
}

fn run(
    st: &State,
    model: &Model,
    drive: impl FnOnce(&mut iced_test::Simulator<'_, Message>),
) -> Vec<Message> {
    let pkg = Packages::default();
    let ctx = ctx(model, &pkg);
    let mut ui = simulate(iced::widget::container(view(st, ctx)).height(36));
    drive(&mut ui);
    ui.into_messages().collect()
}

fn sends(messages: &[Message]) -> Vec<bool> {
    messages
        .iter()
        .filter_map(|message| match message {
            Message::StatusBar(Msg::Send { when_free, .. }) => Some(*when_free),
            _ => None,
        })
        .collect()
}

#[test]
fn over_the_terminals_the_field_says_how_many_panes_it_reaches() {
    let model = model(View::Terminals);
    let pkg = Packages::default();
    let st = State::default();
    let mut ui = simulate(view(&st, ctx(&model, &pkg)));
    assert!(ui.find("Send the same instruction to all 2 panes…").is_ok());
    assert!(ui.find("2 panes").is_ok());
}

#[test]
fn an_empty_field_has_nothing_to_send() {
    let messages = run(&State::default(), &model(View::Terminals), |ui| {
        ui.click("Broadcast").expect("the button is shown");
        ui.click("When free").expect("the button is shown");
    });
    assert!(sends(&messages).is_empty(), "{messages:?}");
}

#[test]
fn a_line_is_sent_now_or_when_free_by_its_two_buttons() {
    let model = model(View::Terminals);
    let now = run(&typed(&model, "run the tests"), &model, |ui| {
        ui.click("Broadcast").unwrap();
    });
    assert_eq!(sends(&now), [false]);
    let later = run(&typed(&model, "run the tests"), &model, |ui| {
        ui.click("When free").unwrap();
    });
    assert_eq!(sends(&later), [true]);
}

#[test]
fn enter_sends_now_and_alt_enter_holds_the_line_until_each_agent_is_free() {
    let model = model(View::Terminals);
    let messages = run(&typed(&model, "run the tests"), &model, |ui| {
        ui.click("run the tests").expect("the field is shown");
        ui.key(Named::Enter, Modifiers::empty());
    });
    assert_eq!(sends(&messages), [false], "{messages:?}");

    let held = run(&typed(&model, "run the tests"), &model, |ui| {
        ui.click("run the tests").unwrap();
        ui.key(Named::Enter, Modifiers::ALT);
    });
    // Alt+Enter is the key's own message, and the field's Enter does not also
    // send the line at once.
    assert_eq!(sends(&held), [true], "{held:?}");
}

#[test]
fn a_shortcut_typed_in_the_field_runs_and_does_not_also_send_the_line() {
    let model = model(View::Terminals);
    // Ctrl+Shift+Enter splits the pane; the field would submit on it.
    let messages = run(&typed(&model, "run the tests"), &model, |ui| {
        ui.click("run the tests").expect("the field is shown");
        ui.key(Named::Enter, Modifiers::CTRL | Modifiers::SHIFT);
    });
    assert!(sends(&messages).is_empty(), "{messages:?}");

    // Without the shortcut's modifiers the same key sends, as before.
    let plain = run(&typed(&model, "run the tests"), &model, |ui| {
        ui.click("run the tests").unwrap();
        ui.key(Named::Enter, Modifiers::empty());
    });
    assert_eq!(sends(&plain), [false], "{plain:?}");
}

#[test]
fn typing_goes_to_the_broadcast_text() {
    let messages = run(&State::default(), &model(View::Terminals), |ui| {
        ui.click("Send the same instruction to all 2 panes…")
            .unwrap();
        ui.typewrite("ls");
    });
    assert!(
        messages.iter().any(
            |message| matches!(message, Message::StatusBar(Msg::Input(_, text)) if text == "l")
        ),
        "{messages:?}"
    );
}

#[test]
fn the_other_views_show_the_folder_and_that_the_agents_are_quiet() {
    let model = model(View::Board);
    let pkg = Packages::default();
    let st = State::default();
    let mut ui = simulate(view(&st, ctx(&model, &pkg)));
    assert!(ui.find("/dev/w").is_ok());
    assert!(ui.find("No agent at work").is_ok());
    // And no broadcast field.
    assert!(ui.find("Broadcast").is_err());
}

#[test]
fn the_branch_shows_with_its_worktree_tag_when_the_folder_is_a_linked_one() {
    let model = model(View::Terminals);
    let mut st = State::default();
    st.take_repo(
        "/dev/w",
        Some(Repo {
            branch: Some("feature/login".into()),
            head: Some("a1b2c3d".into()),
            root: Some("/dev/w".into()),
            is_linked_worktree: true,
        }),
    );
    let pkg = Packages::default();
    let mut ui = simulate(view(&st, ctx(&model, &pkg)));
    assert!(ui.find("feature/login").is_ok());
    assert!(ui.find("worktree").is_ok());
}

#[test]
fn a_detached_head_shows_its_sha_in_place_of_a_branch() {
    let model = model(View::Terminals);
    let mut st = State::default();
    st.take_repo(
        "/dev/w",
        Some(Repo {
            branch: None,
            head: Some("a1b2c3d".into()),
            root: Some("/dev/w".into()),
            is_linked_worktree: false,
        }),
    );
    let pkg = Packages::default();
    let mut ui = simulate(view(&st, ctx(&model, &pkg)));
    assert!(ui.find("a1b2c3d").is_ok());
}

#[test]
fn a_branch_read_for_another_folder_is_not_shown() {
    let model = model(View::Terminals);
    let mut st = State::default();
    st.take_repo(
        "/dev/elsewhere",
        Some(Repo {
            branch: Some("main".into()),
            head: None,
            root: None,
            is_linked_worktree: false,
        }),
    );
    let pkg = Packages::default();
    let mut ui = simulate(view(&st, ctx(&model, &pkg)));
    assert!(ui.find("main").is_err());
}

// ---- A bar for each window ---------------------------------------------------------------------

/// Workspace `a` (one pane) in the main window, workspace `b` (three panes) in
/// a window of its own.
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
                    pane("b3", "Di", AgentId::Shell),
                ],
            ),
        ],
        active_workspace_id: Some("a".into()),
        ..AppState::default()
    });
    model.ui.view = View::Terminals;
    let main = model.main_window().unwrap();
    let own = detach(&mut model, "b");
    // The person is in the main window, so the workspace window's own view
    // waits in the parked state, as the shell keeps it.
    model.ui.focused_window = Some(main);
    model.ui.parked.insert(
        own,
        crate::state::live::WindowUi {
            view: View::Terminals,
            ..Default::default()
        },
    );
    (model, main, own)
}

#[test]
fn a_workspace_windows_bar_is_about_its_own_workspace() {
    let (model, main, own) = two_windows();
    let pkg = Packages::default();
    let st = State::default();

    let mut mine = simulate(view(&st, ctx_in(&model, &pkg, own)));
    assert!(
        mine.find("Send the same instruction to all 3 panes…")
            .is_ok()
    );
    assert!(mine.find("3 panes").is_ok());
    drop(mine);

    let mut main_bar = simulate(view(&st, ctx_in(&model, &pkg, main)));
    assert!(main_bar.find("1 pane").is_ok());
    assert!(main_bar.find("3 panes").is_err());
}

#[test]
fn what_is_typed_in_one_window_is_not_in_the_other() {
    let (model, main, own) = two_windows();
    let pkg = Packages::default();
    let mut st = State::default();
    st.set_broadcast_text(main, "only for the main window".into());

    let mut main_bar = simulate(view(&st, ctx_in(&model, &pkg, main)));
    assert!(main_bar.find("only for the main window").is_ok());
    drop(main_bar);
    let mut own_bar = simulate(view(&st, ctx_in(&model, &pkg, own)));
    assert!(own_bar.find("only for the main window").is_err());
}

#[test]
fn the_buttons_of_a_workspace_window_send_to_its_workspace() {
    let (model, _, own) = two_windows();
    let pkg = Packages::default();
    let mut st = State::default();
    st.set_broadcast_text(own, "run the tests".into());

    let mut ui = simulate(iced::widget::container(view(&st, ctx_in(&model, &pkg, own))).height(36));
    ui.click("Broadcast").expect("the button is shown");
    let messages: Vec<Message> = ui.into_messages().collect();
    let sent: Vec<(window::Id, &str)> = messages
        .iter()
        .filter_map(|message| match message {
            Message::StatusBar(Msg::Send { window, ws, .. }) => Some((*window, ws.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(sent, [(own, "b")], "{messages:?}");
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
    assert!(ui.find("3 panes").is_err());
    assert!(ui.find("1 pane").is_err());
}
