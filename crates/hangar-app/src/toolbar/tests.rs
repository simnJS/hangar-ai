//! `Toolbar.tsx` had no tests; these hold what its comments promise about the
//! questions it asks: a preset only asks when a pane it would close is busy,
//! and a menu that lists panes reads them as a sentence.

use hangar_i18n::{Locale, Translator};

use super::menus::{agent_pane_count, name_list, restart_all, worker_count};
use super::*;
use crate::state::types::{AgentId, Pane, PaneRole};

fn pane(id: &str, agent: AgentId, role: Option<PaneRole>) -> Pane {
    Pane {
        id: id.into(),
        name: id.to_uppercase(),
        agent,
        role,
        ..Pane::default()
    }
}

fn closing() -> Vec<(PaneId, String)> {
    ["a", "b", "c"]
        .iter()
        .map(|id| (id.to_string(), id.to_uppercase()))
        .collect()
}

#[test]
fn a_preset_that_closes_only_idle_panes_does_not_ask() {
    assert!(busy_among(&closing(), |_| AgentActivity::Idle).is_empty());
}

#[test]
fn a_preset_asks_when_an_agent_is_working_waiting_or_has_an_answer_unread() {
    for activity in [
        AgentActivity::Working,
        AgentActivity::Waiting,
        AgentActivity::Yours,
    ] {
        let busy = busy_among(&closing(), |id| {
            if id == "b" {
                activity
            } else {
                AgentActivity::Idle
            }
        });
        assert_eq!(busy, vec![("B".to_string(), activity)]);
    }
}

#[test]
fn the_question_lists_every_busy_pane_in_reading_order() {
    let busy = busy_among(&closing(), |id| match id.as_str() {
        "a" => AgentActivity::Working,
        "c" => AgentActivity::Yours,
        _ => AgentActivity::Idle,
    });
    assert_eq!(
        busy,
        vec![
            ("A".to_string(), AgentActivity::Working),
            ("C".to_string(), AgentActivity::Yours)
        ]
    );
}

#[test]
fn names_read_as_a_sentence() {
    let en = Translator::new(Locale::En);
    let names = |list: &[&str]| -> Vec<String> { list.iter().map(|s| s.to_string()).collect() };
    assert_eq!(name_list(en, &names(&[])), "");
    assert_eq!(name_list(en, &names(&["Ben"])), "Ben");
    assert_eq!(name_list(en, &names(&["Ben", "Cleo"])), "Ben and Cleo");
    assert_eq!(
        name_list(en, &names(&["Ben", "Cleo", "Dan"])),
        "Ben, Cleo and Dan"
    );
    let fr = Translator::new(Locale::Fr);
    assert_eq!(name_list(fr, &names(&["Ben", "Cleo"])), "Ben et Cleo");
}

#[test]
fn a_reset_counts_the_agents_not_the_shells() {
    let ws = Workspace {
        panes: vec![
            pane("a", AgentId::Claude, None),
            pane("b", AgentId::Shell, None),
            pane("c", AgentId::Codex, None),
        ],
        ..Workspace::default()
    };
    assert_eq!(agent_pane_count(&ws), 2);
}

#[test]
fn stopping_counts_the_workers_only() {
    let ws = Workspace {
        panes: vec![
            pane("o", AgentId::Claude, Some(PaneRole::Orchestrator)),
            pane("w1", AgentId::Claude, Some(PaneRole::Worker)),
            pane("w2", AgentId::Claude, Some(PaneRole::Worker)),
            pane("s", AgentId::Shell, None),
        ],
        ..Workspace::default()
    };
    assert_eq!(worker_count(&ws), 2);
}

#[test]
fn restarting_all_names_every_pane() {
    let ws = Workspace {
        id: "w".into(),
        panes: vec![
            pane("a", AgentId::Claude, None),
            pane("b", AgentId::Shell, None),
        ],
        ..Workspace::default()
    };
    match restart_all(&ws) {
        Intent::RestartPanes { ws, panes } => {
            assert_eq!(ws, "w");
            assert_eq!(panes, ["a", "b"]);
        }
        other => panic!("expected a restart, got {other:?}"),
    }
}

#[test]
fn the_toolbars_breakpoints_are_in_em_of_its_font() {
    // 80 em of 12 px: the width under which the name goes.
    assert_eq!(80.0 * EM, 960.0);
    assert_eq!(activity::EXTRAS_FROM_EM * EM, 852.0);
    assert_eq!(64.0 * EM, 768.0);
    assert_eq!(46.0 * EM, 552.0);
}
