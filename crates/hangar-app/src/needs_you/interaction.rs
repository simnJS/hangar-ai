//! The button and its panel driven with real pointer events.

use serde_json::json;

use super::model::Boards;
use super::*;
use crate::app::Packages;
use crate::navigator::testing::{ctx, model_of, pane, simulate, workspace};
use crate::state::types::{AgentId, AppState, PaneRole};
use hangar_core::board::Task as BoardTask;

fn task(id: &str, title: &str, labels: &[&str]) -> BoardTask {
    serde_json::from_value(json!({
        "id": id, "title": title, "column": "review", "priority": 2,
        "assignee": "Ben", "labels": labels,
        "comments": [{"id": "c", "author": "Ava", "text": "Stripe or Adyen?\nDetails.", "created_at": 1}],
        "created_at": 1, "updated_at": 5,
    }))
    .unwrap()
}

/// An orchestrated workspace whose board holds a question and a delivery.
fn world() -> (Model, State) {
    let mut ws = workspace(
        "w",
        "checkout",
        None,
        vec![
            pane("iris", "Iris", AgentId::Claude),
            pane("ben", "Ben", AgentId::Claude),
        ],
    );
    ws.panes[0].role = Some(PaneRole::Orchestrator);
    ws.panes[1].role = Some(PaneRole::Worker);
    let cwd = ws.cwd.clone();
    let mut model = model_of(AppState {
        workspaces: vec![ws],
        active_workspace_id: Some("w".into()),
        ..AppState::default()
    });
    let mut st = State::default();
    let boards: Boards = [(
        cwd,
        vec![
            task("1", "Choose the payment provider", &["needs-you"]),
            task("2", "Add the webhook", &["ready-to-land"]),
        ],
    )]
    .into_iter()
    .collect();
    let seen = look(&model, &HashSet::new(), &boards);
    let _ = settle(&mut st, &mut model, seen);
    (model, st)
}

fn run(
    st: &State,
    model: &Model,
    drive: impl FnOnce(&mut iced_test::Simulator<'_, Message>),
) -> Vec<Message> {
    let pkg = Packages::default();
    let ctx = ctx(model, &pkg);
    let mut ui = simulate(button(st, ctx));
    drive(&mut ui);
    ui.into_messages().collect()
}

fn needs_you(messages: &[Message]) -> Vec<&Msg> {
    messages
        .iter()
        .filter_map(|message| match message {
            Message::NeedsYou(msg) => Some(msg),
            _ => None,
        })
        .collect()
}

#[test]
fn nothing_shows_while_nothing_waits() {
    let model = model_of(AppState {
        workspaces: vec![workspace(
            "w",
            "plain",
            None,
            vec![pane("a", "Ava", AgentId::Claude)],
        )],
        active_workspace_id: Some("w".into()),
        ..AppState::default()
    });
    let pkg = Packages::default();
    let st = State::default();
    let mut ui = simulate(button(&st, ctx(&model, &pkg)));
    assert!(ui.find("Needs you").is_err());
}

#[test]
fn the_button_counts_what_waits_and_opens_the_panel() {
    let (model, st) = world();
    // The window is too narrow for the button's words: it is its flag and its
    // count, which says two things wait (a question and a delivery).
    let messages = run(&st, &model, |ui| {
        ui.click("2").expect("the button is shown");
    });
    assert!(
        needs_you(&messages)
            .iter()
            .any(|msg| matches!(msg, Msg::Toggle)),
        "{messages:?}"
    );
}

#[test]
fn the_panel_groups_what_waits_by_kind() {
    let (mut model, st) = world();
    model.ui.needs_you_open = true;
    let pkg = Packages::default();
    let mut ui = simulate(button(&st, ctx(&model, &pkg)));
    for words in [
        "Choose the payment provider",
        "Add the webhook",
        "Stripe or Adyen?",
        "checkout · Ben",
    ] {
        assert!(ui.find(words).is_ok(), "{words} is missing");
    }
}

#[test]
fn answering_goes_to_the_orchestrator_of_the_workspace() {
    let (mut model, st) = world();
    model.ui.needs_you_open = true;
    let messages = run(&st, &model, |ui| {
        ui.click("Answer").expect("the question can be answered");
    });
    assert!(
        needs_you(&messages)
            .iter()
            .any(|msg| matches!(msg, Msg::GoTo { ws, pane } if ws == "w" && pane == "iris")),
        "{messages:?}"
    );
}

#[test]
fn a_delivery_that_is_ready_is_merged_from_its_row() {
    let (mut model, st) = world();
    model.ui.needs_you_open = true;
    let messages = run(&st, &model, |ui| {
        ui.click("Merge").expect("the delivery can be merged");
    });
    let request = needs_you(&messages)
        .into_iter()
        .find_map(|msg| match msg {
            Msg::Land(request) => Some(request.clone()),
            _ => None,
        })
        .expect("a landing was asked for");
    assert_eq!(request.ws, "w");
    assert_eq!(request.task, "2");
}

#[test]
fn a_delivery_being_merged_cannot_be_merged_twice() {
    let (mut model, mut st) = world();
    model.ui.needs_you_open = true;
    let cwd = model.workspace("w").unwrap().cwd.clone();
    st.landing
        .insert(format!("task:{cwd}:2:ready-to-land"), Instant::now());
    let messages = run(&st, &model, |ui| {
        ui.click("Merging…")
            .expect("the button says what it is doing");
    });
    assert!(
        !needs_you(&messages)
            .iter()
            .any(|msg| matches!(msg, Msg::Land(_))),
        "{messages:?}"
    );
}

#[test]
fn a_merge_that_failed_says_why_on_its_row() {
    let (mut model, mut st) = world();
    model.ui.needs_you_open = true;
    let cwd = model.workspace("w").unwrap().cwd.clone();
    st.failures.insert(
        format!("task:{cwd}:2:ready-to-land"),
        "the branch has diverged".into(),
    );
    let pkg = Packages::default();
    let mut ui = simulate(button(&st, ctx(&model, &pkg)));
    assert!(
        ui.find("Could not merge: the branch has diverged").is_ok(),
        "the failure is not on the row"
    );
}
