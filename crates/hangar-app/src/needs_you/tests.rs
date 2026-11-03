//! What `NeedsYou.tsx` keeps between renders: a failure belongs to the row it
//! happened on, and a landing that never answers must not hold its button for
//! ever. And what this package tells the shell: how many things wait.

use std::time::Duration;

use serde_json::json;

use super::*;
use crate::commands::CommandId;
use crate::commands::run::is_available;
use crate::navigator::testing::{model_of, pane, workspace};
use crate::state::types::{AgentId, AppState, PaneRole};
use model::{Boards, ItemBase, NeedsYouItem};

fn land(key: &str) -> NeedsYouItem {
    NeedsYouItem::Land {
        base: ItemBase {
            key: key.into(),
            workspace_id: "w".into(),
            workspace_name: "w".into(),
        },
        cwd: "/p".into(),
        task: serde_json::from_value(json!({
            "id": "1", "title": "Ship it", "column": "review",
            "created_at": 1, "updated_at": 1
        }))
        .unwrap(),
    }
}

#[test]
fn a_row_that_left_the_list_takes_its_failure_and_its_landing_with_it() {
    let mut st = State::default();
    st.failures
        .insert("task:/p:1:ready-to-land".into(), "conflict".into());
    st.failures
        .insert("task:/p:2:ready-to-land".into(), "conflict".into());
    st.landing
        .insert("task:/p:2:ready-to-land".into(), Instant::now());

    st.forget_gone(&[land("task:/p:1:ready-to-land")]);

    assert_eq!(st.failure("task:/p:1:ready-to-land"), Some("conflict"));
    assert_eq!(st.failure("task:/p:2:ready-to-land"), None);
    assert!(st.landing.is_empty());
}

#[test]
fn a_delivery_asked_to_land_says_so_for_a_while() {
    let mut st = State::default();
    let m = Model::default();
    let now = Instant::now();
    st.landing.insert("k".into(), now);
    assert!(st.is_landing(&m, land("k").base(), "1", now));
    assert!(st.is_landing(&m, land("k").base(), "1", now + Duration::from_secs(5)));
    assert!(!st.is_landing(&m, land("other").base(), "1", now));
}

#[test]
fn a_landing_that_never_shows_lets_the_person_try_again() {
    let mut st = State::default();
    let m = Model::default();
    let now = Instant::now();
    st.landing.insert("k".into(), now);
    assert!(!st.is_landing(
        &m,
        land("k").base(),
        "1",
        now + LAND_GRACE + Duration::from_secs(1)
    ));
}

#[test]
fn the_panel_is_closed_until_asked() {
    // The flag is the shell's (`Model.ui.needs_you_open`); this package only
    // flips it, so a fresh model starts closed.
    let model = Model::default();
    assert!(!model.ui.needs_you_open);
}

// ---- What the shell is told ---------------------------------------------------------------------

/// A workspace with an orchestrator, a worker and a plain agent.
fn orchestrated() -> Model {
    let mut ws = workspace(
        "w",
        "checkout",
        None,
        vec![
            pane("iris", "Iris", AgentId::Claude),
            pane("ben", "Ben", AgentId::Claude),
            pane("leo", "Leo", AgentId::Claude),
        ],
    );
    ws.panes[0].role = Some(PaneRole::Orchestrator);
    ws.panes[1].role = Some(PaneRole::Worker);
    model_of(AppState {
        workspaces: vec![ws],
        active_workspace_id: Some("w".into()),
        ..AppState::default()
    })
}

fn board_of(m: &Model, tasks: &[(&str, &[&str])]) -> Boards {
    let cwd = m.workspace("w").unwrap().cwd.clone();
    let tasks = tasks
        .iter()
        .map(|(id, labels)| {
            serde_json::from_value(json!({
                "id": id, "title": format!("Task {id}"), "column": "review",
                "labels": labels, "created_at": 1, "updated_at": 1
            }))
            .unwrap()
        })
        .collect();
    [(cwd, tasks)].into_iter().collect()
}

fn waiting(ids: &[&str]) -> HashSet<String> {
    ids.iter().map(|id| id.to_string()).collect()
}

#[test]
fn the_shell_is_told_how_many_things_wait() {
    let mut m = orchestrated();
    let mut st = State::default();
    assert_eq!(m.live.signals.needs_you, 0);

    let boards = board_of(&m, &[("1", &["needs-you"]), ("2", &["ready-to-land"])]);
    let seen = look(&m, &waiting(&["leo"]), &boards);
    let _ = settle(&mut st, &mut m, seen);

    // An agent in a dialog, a question and a delivery.
    assert_eq!(m.live.signals.needs_you, 3);
    assert_eq!(st.items().len(), 3);
}

#[test]
fn a_waiting_pane_and_a_needs_you_task_make_the_command_live_and_nothing_kills_it() {
    let mut m = orchestrated();
    let mut st = State::default();
    let w = iced::window::Id::unique();
    assert!(!is_available(&m, w, CommandId::ViewNeedsYou));

    let boards = board_of(&m, &[("1", &["needs-you"])]);
    let seen = look(&m, &waiting(&["leo"]), &boards);
    let _ = settle(&mut st, &mut m, seen);
    assert!(is_available(&m, w, CommandId::ViewNeedsYou));

    // The question is answered and the agent moves on: the command goes dead
    // and the shell has nothing to keep the panel open for.
    let seen = look(&m, &waiting(&[]), &board_of(&m, &[]));
    let _ = settle(&mut st, &mut m, seen);
    assert_eq!(m.live.signals.needs_you, 0);
    assert!(!is_available(&m, w, CommandId::ViewNeedsYou));
}

#[test]
fn the_panel_stays_open_after_the_command_opens_it() {
    // The shell's `Action::OpenNeedsYou` sets the flag, and after the message
    // closes the panel when `signals.needs_you` is 0 (`app/shell.rs`). With the
    // list worked out the count is not 0, so that rule leaves the panel be.
    let mut m = orchestrated();
    let mut st = State::default();
    let boards = board_of(&m, &[("1", &["needs-you"])]);
    let seen = look(&m, &waiting(&[]), &boards);
    let _ = settle(&mut st, &mut m, seen);
    m.ui.needs_you_open = true;
    let shell_closes = m.live.signals.needs_you == 0;
    assert!(!shell_closes);
}

#[test]
fn a_press_on_the_button_replaces_a_stale_count_before_it_opens() {
    // The shell closes the panel after the message while the count is 0, so
    // the count must be the truth by then: here the stale 7 becomes the real 0.
    let mut m = orchestrated();
    let mut st = State::default();
    m.live.signals.needs_you = 7;
    let _ = toggle(&mut st, &mut m);
    assert_eq!(m.live.signals.needs_you, 0);
    assert!(m.ui.needs_you_open, "the press itself is obeyed");
    let _ = toggle(&mut st, &mut m);
    assert!(!m.ui.needs_you_open);
}

#[test]
fn a_workspace_that_orchestrates_nothing_has_nothing_to_list() {
    let mut m = model_of(AppState {
        workspaces: vec![workspace(
            "w",
            "plain",
            None,
            vec![pane("a", "Ava", AgentId::Claude)],
        )],
        active_workspace_id: Some("w".into()),
        ..AppState::default()
    });
    let mut st = State::default();
    let seen = look(&m, &waiting(&["a"]), &Boards::new());
    let _ = settle(&mut st, &mut m, seen);
    assert_eq!(m.live.signals.needs_you, 0);
}

#[test]
fn a_board_not_read_yet_lists_nothing_and_is_not_a_baseline() {
    let m = orchestrated();
    let seen = look(&m, &waiting(&[]), &Boards::new());
    assert!(seen.items.is_empty());
    assert!(seen.loaded.is_empty());
    let seen = look(&m, &waiting(&[]), &board_of(&m, &[]));
    assert_eq!(seen.loaded.len(), 1);
}

#[test]
fn the_boards_come_from_the_board_packages_cache() {
    // `board::Live` is what the app reads; with nothing read yet it has no
    // tasks, so the list is empty rather than an error. No pane reports a
    // dialog either, since none has an agent bridge in a test.
    let m = orchestrated();
    assert!(waiting_panes(&m).is_empty());
    let seen = look(&m, &waiting_panes(&m), &m.live.board);
    assert!(seen.items.is_empty());
}

#[test]
fn nothing_is_watched_until_a_workspace_orchestrates() {
    assert!(watched(&model_of(AppState::default())).is_empty());
    assert_eq!(watched(&orchestrated()).len(), 1);
}
