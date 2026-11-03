//! The status bar driven with real pointer and key events: the broadcast
//! field, its two buttons, and the readouts.

use iced::keyboard::{Modifiers, key::Named};

use super::*;
use crate::app::Packages;
use crate::navigator::testing::{Drive, ctx, model_of, pane, simulate, workspace};
use crate::state::types::{AgentId, AppState};

fn model(broadcast: &str, view: View) -> Model {
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
    model.ui.broadcast = broadcast.into();
    model.ui.view = view;
    model
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
            Message::StatusBar(Msg::Send { when_free }) => Some(*when_free),
            _ => None,
        })
        .collect()
}

#[test]
fn over_the_terminals_the_field_says_how_many_panes_it_reaches() {
    let model = model("", View::Terminals);
    let pkg = Packages::default();
    let st = State::default();
    let mut ui = simulate(view(&st, ctx(&model, &pkg)));
    assert!(ui.find("Send the same instruction to all 2 panes…").is_ok());
    assert!(ui.find("2 panes").is_ok());
}

#[test]
fn an_empty_field_has_nothing_to_send() {
    let messages = run(&State::default(), &model("", View::Terminals), |ui| {
        ui.click("Broadcast").expect("the button is shown");
        ui.click("When free").expect("the button is shown");
    });
    assert!(sends(&messages).is_empty(), "{messages:?}");
}

#[test]
fn a_line_is_sent_now_or_when_free_by_its_two_buttons() {
    let now = run(
        &State::default(),
        &model("run the tests", View::Terminals),
        |ui| {
            ui.click("Broadcast").unwrap();
        },
    );
    assert_eq!(sends(&now), [false]);
    let later = run(
        &State::default(),
        &model("run the tests", View::Terminals),
        |ui| {
            ui.click("When free").unwrap();
        },
    );
    assert_eq!(sends(&later), [true]);
}

#[test]
fn enter_sends_now_and_alt_enter_holds_the_line_until_each_agent_is_free() {
    let messages = run(
        &State::default(),
        &model("run the tests", View::Terminals),
        |ui| {
            ui.click("run the tests").expect("the field is shown");
            ui.key(Named::Enter, Modifiers::empty());
        },
    );
    assert_eq!(sends(&messages), [false], "{messages:?}");

    let held = run(
        &State::default(),
        &model("run the tests", View::Terminals),
        |ui| {
            ui.click("run the tests").unwrap();
            ui.key(Named::Enter, Modifiers::ALT);
        },
    );
    // Alt+Enter is the key's own message, and the field's Enter does not also
    // send the line at once.
    assert_eq!(sends(&held), [true], "{held:?}");
}

#[test]
fn typing_goes_to_the_broadcast_text() {
    let messages = run(&State::default(), &model("", View::Terminals), |ui| {
        ui.click("Send the same instruction to all 2 panes…")
            .unwrap();
        ui.typewrite("ls");
    });
    assert!(
        messages
            .iter()
            .any(|message| matches!(message, Message::StatusBar(Msg::Input(text)) if text == "l")),
        "{messages:?}"
    );
}

#[test]
fn the_other_views_show_the_folder_and_that_the_agents_are_quiet() {
    let model = model("", View::Board);
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
    let model = model("", View::Terminals);
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
    let model = model("", View::Terminals);
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
    let model = model("", View::Terminals);
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
