//! The sleep line of a workspace's menu: what the rules say, what the system
//! said of the shells, and what a menu does with an answer that came late.
//! The rules themselves, and the Ask they end on, are the pane logic's.

use hangar_i18n::{Key, Locale, Translator};
use iced::Vector;

use super::*;
use crate::navigator::model::RowKind;
use crate::navigator::testing::{model_of, pane, workspace};
use crate::navigator::{MenuState, Msg, State, step};
use crate::state::types::{AgentId, AppState};

fn model() -> Model {
    model_of(AppState {
        workspaces: vec![
            workspace("a", "alpha", None, vec![pane("a1", "Ava", AgentId::Shell)]),
            workspace("b", "beta", None, vec![pane("b1", "Bo", AgentId::Shell)]),
        ],
        active_workspace_id: Some("a".into()),
        ..AppState::default()
    })
}

fn open_menu_on(st: &mut State, ws: &str) {
    st.menu = Some(MenuState {
        kind: RowKind::Ws,
        id: ws.into(),
        at: Vector::new(0.0, 0.0),
        from_keyboard: false,
    });
}

#[test]
fn a_workspace_that_is_not_open_is_greyed_with_the_reason() {
    let m = model();
    // `b` was never opened: its shells run nowhere, so there is nothing to put
    // to sleep, and the line says so.
    match line(&m, None, "b") {
        Some(SleepLine::Refused(reason)) => {
            let said = Translator::new(Locale::En)
                .t(Key::SleepNotMounted)
                .into_owned();
            assert_eq!(reason, said);
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn a_workspace_that_is_gone_has_no_line() {
    let m = model();
    assert_eq!(line(&m, None, "gone"), None);
    assert_eq!(request(&m, "gone"), None);
}

#[test]
fn a_refusal_of_the_rules_stands_whatever_the_system_said() {
    let m = model();
    let answered = Probe::answered("b".into(), &["b1".to_string()], Ok(vec![]));
    let failed = Probe::answered("b".into(), &["b1".to_string()], Err("no ps".into()));
    let rules = line(&m, None, "b");
    assert!(matches!(rules, Some(SleepLine::Refused(_))));
    assert_eq!(line(&m, Some(&answered), "b"), rules);
    assert_eq!(line(&m, Some(&failed), "b"), rules);
}

#[test]
fn the_answer_names_which_of_the_asked_shells_run_something() {
    let asked = ["s1".to_string(), "s2".to_string(), "s3".to_string()];
    let probe = Probe::answered("b".into(), &asked, Ok(vec!["s2".to_string()]));
    match &probe.answer {
        Answer::Running(known) => {
            assert_eq!(known.get("s1"), Some(&false));
            assert_eq!(known.get("s2"), Some(&true));
            assert_eq!(known.get("s3"), Some(&false));
        }
        other => panic!("expected an answer, got {other:?}"),
    }
    assert_eq!(probe.ws(), "b");
    // A system that could not say is not an answer.
    let failed = Probe::answered("b".into(), &asked, Err("gone".into()));
    assert_eq!(failed.answer, Answer::Failed);
}

#[test]
fn not_knowing_what_a_shell_runs_is_a_reason_to_leave_it_running() {
    let m = model();
    let words = Translator::new(Locale::En)
        .t(Key::SleepUnknownProcesses)
        .into_owned();
    assert_eq!(unknown_processes(&m), SleepLine::Refused(words));
}

// ---- What the menu does with an answer ---------------------------------------------------------

fn probed(ws: &str) -> Msg {
    Msg::SleepProbed {
        ws: ws.into(),
        asked: vec!["b1".into()],
        answer: Ok(vec![]),
    }
}

#[test]
fn an_answer_is_kept_for_the_menu_that_asked() {
    let mut m = model();
    let mut st = State::default();
    open_menu_on(&mut st, "b");
    let _ = step(&mut st, &mut m, probed("b"));
    assert_eq!(st.probe.as_ref().map(Probe::ws), Some("b"));
}

#[test]
fn an_answer_for_a_menu_that_closed_or_moved_on_is_dropped() {
    let mut m = model();
    let mut st = State::default();
    // No menu open.
    let _ = step(&mut st, &mut m, probed("b"));
    assert!(st.probe.is_none());
    // Another workspace's menu is open.
    open_menu_on(&mut st, "a");
    let _ = step(&mut st, &mut m, probed("b"));
    assert!(st.probe.is_none());
}
