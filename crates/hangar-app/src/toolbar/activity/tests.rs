//! `ToolbarActivity.tsx` had no tests; these hold the rules its comments
//! state: only the latest event can be the question, a state word alone says
//! it all for a waiting pane, and the counters walk the panes in reading
//! order, wrapping. What the capsule is drawn from is the pane logic's
//! insight (which has the tests of how the mod's reports become one), so the
//! cases here start from an insight.
#![allow(clippy::field_reassign_with_default)]

use super::*;
use crate::state::live::{ContextGauge, PaneEvent};
use crate::state::types::{SplitDir, SplitNode};

fn event(detail: PaneEventDetail) -> PaneEvent {
    PaneEvent { at: 1, detail }
}

fn asking(tool: Option<&str>) -> PaneEvent {
    event(PaneEventDetail::Permission {
        tool: tool.map(str::to_string),
    })
}

fn insight(events: Vec<PaneEvent>) -> PaneInsight {
    PaneInsight {
        events,
        ..PaneInsight::default()
    }
}

#[test]
fn a_pane_asking_for_a_permission_names_the_tool() {
    assert_eq!(
        detail(
            AgentActivity::Waiting,
            &insight(vec![asking(Some("Bash"))]),
            None
        ),
        Detail::Tool("Bash".into())
    );
}

#[test]
fn a_question_with_no_tool_says_nothing_more_than_the_state_word() {
    assert_eq!(
        detail(AgentActivity::Waiting, &insight(vec![asking(None)]), None),
        Detail::None
    );
    assert_eq!(
        detail(
            AgentActivity::Waiting,
            &insight(vec![asking(Some(""))]),
            None
        ),
        Detail::None
    );
}

#[test]
fn only_the_latest_event_can_be_the_question() {
    // Most recent first: the agent was answered and started its next turn, so
    // the permission it asked for is old.
    let history = insight(vec![
        event(PaneEventDetail::TurnStart),
        asking(Some("Bash")),
    ]);
    assert_eq!(detail(AgentActivity::Waiting, &history, None), Detail::None);
    assert_eq!(
        detail(AgentActivity::Waiting, &PaneInsight::default(), None),
        Detail::None
    );
}

#[test]
fn the_last_answer_is_what_an_idle_pane_has_to_say() {
    let mut said = PaneInsight::default();
    said.last_answer = Some("Done: the build passes.".into());
    assert_eq!(
        detail(AgentActivity::Idle, &said, None),
        Detail::LastAnswer("Done: the build passes.".into())
    );
    said.last_answer = Some(String::new());
    assert_eq!(detail(AgentActivity::Idle, &said, None), Detail::None);
}

#[test]
fn a_task_reads_in_full_strength_and_wins_over_the_last_answer() {
    let mut said = PaneInsight::default();
    said.last_answer = Some("an old answer".into());
    assert_eq!(
        detail(AgentActivity::Working, &said, Some("Fix the login")),
        Detail::Working("Fix the login".into())
    );
    assert_eq!(
        detail(AgentActivity::Yours, &said, Some("Fix the login")),
        Detail::Done("Fix the login".into())
    );
    // Idle with a task is not on it: it falls back to what it last said.
    assert_eq!(
        detail(AgentActivity::Idle, &said, Some("Fix the login")),
        Detail::LastAnswer("an old answer".into())
    );
}

fn gauge(pct: f64) -> ContextGauge {
    ContextGauge {
        pct,
        used_tokens: 1,
        window: 10,
        model: None,
    }
}

#[test]
fn the_context_used_is_whole_percent() {
    let mut used = PaneInsight::default();
    assert_eq!(context_pct(&used), None);
    used.context = Some(gauge(41.6));
    assert_eq!(context_pct(&used), Some(42));
    used.context = Some(gauge(25.0));
    assert_eq!(context_pct(&used), Some(25));
}

#[test]
fn the_context_is_held_between_zero_and_a_hundred() {
    let mut used = PaneInsight::default();
    used.context = Some(gauge(140.0));
    assert_eq!(context_pct(&used), Some(100));
    used.context = Some(gauge(-4.0));
    assert_eq!(context_pct(&used), Some(0));
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
