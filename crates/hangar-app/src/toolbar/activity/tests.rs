//! `ToolbarActivity.tsx` had no tests; these hold the rules its comments
//! state: only the latest event can be the question, a state word alone says
//! it all for a waiting pane, and the counters walk the panes in reading
//! order, wrapping.
#![allow(clippy::field_reassign_with_default)]

use serde_json::json;

use super::*;
use crate::state::types::{SplitDir, SplitNode};

fn report(value: serde_json::Value) -> BridgeEvent {
    BridgeEvent::parse(&value).expect("a report the mod can send")
}

fn permission(tool: Option<&str>) -> BridgeEvent {
    report(json!({"kind": "permission", "waiting": true, "tool": tool}))
}

#[test]
fn a_pane_asking_for_a_permission_names_the_tool() {
    let mut glance = Glance::default();
    glance.note(&permission(Some("Bash")));
    assert_eq!(glance.asking, Some(Some("Bash".to_string())));
    assert_eq!(
        detail(AgentActivity::Waiting, &glance, None),
        Detail::Tool("Bash".into())
    );
}

#[test]
fn a_question_with_no_tool_says_nothing_more_than_the_state_word() {
    let mut glance = Glance::default();
    glance.note(&permission(None));
    assert_eq!(detail(AgentActivity::Waiting, &glance, None), Detail::None);
    glance.note(&permission(Some("")));
    assert_eq!(detail(AgentActivity::Waiting, &glance, None), Detail::None);
}

#[test]
fn only_the_latest_event_can_be_the_question() {
    let mut glance = Glance::default();
    glance.note(&permission(Some("Bash")));
    // The agent was answered and started its next turn: the question is old.
    glance.note(&report(json!({"kind": "turn", "phase": "start"})));
    assert_eq!(glance.asking, None);
    assert_eq!(detail(AgentActivity::Waiting, &glance, None), Detail::None);
}

#[test]
fn the_end_of_a_permission_or_a_tool_is_not_an_event_of_the_history() {
    let mut glance = Glance::default();
    glance.note(&permission(Some("Edit")));
    glance.note(&report(
        json!({"kind": "tool", "phase": "start", "toolUseId": "1", "tool": "Edit", "input": {}}),
    ));
    glance.note(&report(json!({"kind": "permission", "waiting": false})));
    assert_eq!(glance.asking, Some(Some("Edit".to_string())));
}

#[test]
fn the_last_answer_is_the_first_line_that_says_something() {
    let mut glance = Glance::default();
    glance.note(&report(json!({
        "kind": "turn", "phase": "end", "reason": "answer", "durationMs": 10,
        "answer": "\n  Done: the build passes.\nDetails below."
    })));
    assert_eq!(
        glance.last_answer.as_deref(),
        Some("Done: the build passes.")
    );
    assert_eq!(
        detail(AgentActivity::Idle, &glance, None),
        Detail::LastAnswer("Done: the build passes.".into())
    );
}

#[test]
fn an_empty_answer_clears_the_last_one() {
    let mut glance = Glance::default();
    glance.last_answer = Some("old".into());
    glance.note(&report(json!({
        "kind": "turn", "phase": "end", "reason": "aborted", "durationMs": 10, "answer": ""
    })));
    assert_eq!(glance.last_answer, None);
}

#[test]
fn a_task_reads_in_full_strength_and_wins_over_the_last_answer() {
    let mut glance = Glance::default();
    glance.last_answer = Some("an old answer".into());
    assert_eq!(
        detail(AgentActivity::Working, &glance, Some("Fix the login")),
        Detail::Working("Fix the login".into())
    );
    assert_eq!(
        detail(AgentActivity::Yours, &glance, Some("Fix the login")),
        Detail::Done("Fix the login".into())
    );
    // Idle with a task is not on it: it falls back to what it last said.
    assert_eq!(
        detail(AgentActivity::Idle, &glance, Some("Fix the login")),
        Detail::LastAnswer("an old answer".into())
    );
}

#[test]
fn the_context_used_is_whole_percent_from_the_report_or_the_token_count() {
    let mut glance = Glance::default();
    glance.note(&report(json!({
        "kind": "usage", "context": {"tokens": 1, "window": 10, "percent": 41.6},
        "rateLimits": [], "costUsd": null
    })));
    assert_eq!(glance.context_pct, Some(42));

    glance.note(&report(json!({
        "kind": "usage", "context": {"tokens": 50_000, "window": 200_000, "percent": null},
        "rateLimits": [], "costUsd": null
    })));
    assert_eq!(glance.context_pct, Some(25));

    glance.note(&report(json!({
        "kind": "usage", "context": {"tokens": null, "window": 200_000, "percent": null},
        "rateLimits": [], "costUsd": null
    })));
    assert_eq!(glance.context_pct, None);
}

#[test]
fn the_context_is_held_between_zero_and_a_hundred() {
    let mut glance = Glance::default();
    glance.note(&report(json!({
        "kind": "usage", "context": {"tokens": 1, "window": 10, "percent": 140.0},
        "rateLimits": [], "costUsd": null
    })));
    assert_eq!(glance.context_pct, Some(100));
}

#[test]
fn an_agent_that_exits_takes_its_gauge_and_its_question_with_it() {
    let mut glance = Glance::default();
    glance.context_pct = Some(60);
    glance.note(&permission(Some("Bash")));
    glance.note(&report(json!({"kind": "end", "reason": "exit"})));
    assert_eq!(glance.context_pct, None);
    assert_eq!(glance.asking, None);
}

// ---- The counters ------------------------------------------------------------------------------

fn pane(id: &str) -> Pane {
    Pane {
        id: id.into(),
        name: id.to_uppercase(),
        ..Pane::default()
    }
}

fn workspace() -> Workspace {
    // Read: c, a, b, d.
    Workspace {
        id: "w".into(),
        panes: vec![pane("a"), pane("b"), pane("c"), pane("d")],
        tree: Some(SplitNode::Split {
            dir: SplitDir::Row,
            ratio: 0.5,
            a: Box::new(SplitNode::Split {
                dir: SplitDir::Col,
                ratio: 0.5,
                a: Box::new(SplitNode::Pane { id: "c".into() }),
                b: Box::new(SplitNode::Pane { id: "a".into() }),
            }),
            b: Box::new(SplitNode::Split {
                dir: SplitDir::Col,
                ratio: 0.5,
                a: Box::new(SplitNode::Pane { id: "b".into() }),
                b: Box::new(SplitNode::Pane { id: "d".into() }),
            }),
        }),
        ..Workspace::default()
    }
}

fn wanted(ids: &'static [&'static str]) -> impl Fn(&PaneId) -> bool {
    move |id| ids.contains(&id.as_str())
}

#[test]
fn the_first_click_lands_on_the_first_pane_in_reading_order() {
    let ws = workspace();
    let target = next_in(&ws, None, wanted(&["a", "b", "d"])).unwrap();
    assert_eq!(target.id, "a");
}

#[test]
fn the_next_clicks_walk_through_the_rest_and_wrap() {
    let ws = workspace();
    let all = wanted(&["a", "b", "d"]);
    let after_a = next_in(&ws, Some(&"a".to_string()), &all).unwrap();
    assert_eq!(after_a.id, "b");
    let after_b = next_in(&ws, Some(&"b".to_string()), &all).unwrap();
    assert_eq!(after_b.id, "d");
    let after_d = next_in(&ws, Some(&"d".to_string()), &all).unwrap();
    assert_eq!(after_d.id, "a");
}

#[test]
fn a_focused_pane_that_is_not_in_the_state_starts_from_the_top() {
    let ws = workspace();
    let target = next_in(&ws, Some(&"c".to_string()), wanted(&["b", "d"])).unwrap();
    assert_eq!(target.id, "b");
}

#[test]
fn nothing_in_the_state_has_nowhere_to_go() {
    assert!(next_in(&workspace(), None, wanted(&[])).is_none());
}
