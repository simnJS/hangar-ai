//! The « … » menu of a pane header: what a pane is to the orchestration (the
//! orchestration panels' rule decides), and the Claude account block it ends
//! with, which is set off by one separator and one heading, not two.

use hangar_i18n::Locale;

use super::*;
use crate::navigator::testing::pane;
use crate::shell::MenuEntry;
use crate::state::types::AgentId;
use crate::workspace_dialog::testing::Harness;

fn workspace(panes: Vec<Pane>) -> Workspace {
    Workspace {
        panes,
        ..Workspace::default()
    }
}

fn with_role(id: &str, agent: AgentId, role: PaneRole) -> Pane {
    Pane {
        role: Some(role),
        ..pane(id, id, agent)
    }
}

fn part(ws: &Workspace, id: &str) -> Option<Part> {
    let entry = ws.panes.iter().find(|entry| entry.id == id).unwrap();
    part_in_orchestration(ws, entry)
}

#[test]
fn only_a_free_claude_code_pane_can_become_the_orchestrator() {
    let ws = workspace(vec![
        pane("claude", "Claude", AgentId::Claude),
        pane("codex", "Codex", AgentId::Codex),
        pane("shell", "Shell", AgentId::Shell),
        with_role("worker", AgentId::Claude, PaneRole::Worker),
        with_role("manager", AgentId::Claude, PaneRole::Manager),
    ]);
    assert_eq!(part(&ws, "claude"), Some(Part::Free));
    for other in ["codex", "shell", "worker", "manager"] {
        assert_eq!(part(&ws, other), None, "{other}");
    }
}

#[test]
fn one_orchestrator_per_workspace_and_none_in_the_hq() {
    let mut taken = workspace(vec![
        pane("claude", "Claude", AgentId::Claude),
        with_role("lead", AgentId::Claude, PaneRole::Orchestrator),
    ]);
    // The orchestrator offers to let go; the others can no longer be one.
    assert_eq!(part(&taken, "lead"), Some(Part::Orchestrator));
    assert_eq!(part(&taken, "claude"), None);

    taken.panes.pop();
    taken.hq = Some(true);
    assert_eq!(part(&taken, "claude"), None);
}

#[test]
fn the_claude_account_block_comes_with_its_own_separator_and_heading() {
    let harness = Harness::new(Locale::En, "graphite");
    let ctx = harness.ctx();
    // The accounts package starts its block with a separator and its heading:
    // what this menu appends is that block, not that block under a second pair.
    let block = crate::accounts::menu_items(ctx, &"w1".to_string(), Some(&"p1".to_string()));
    assert!(matches!(
        block.as_slice(),
        [MenuEntry::Separator, MenuEntry::Heading(_), ..]
    ));
    let separators = block
        .iter()
        .filter(|entry| matches!(entry, MenuEntry::Separator))
        .count();
    assert_eq!(separators, 1);
    let headings = block
        .iter()
        .filter(|entry| matches!(entry, MenuEntry::Heading(_)))
        .count();
    assert_eq!(headings, 1);
    assert_eq!(set_off(block.clone()).len(), block.len());
}

#[test]
fn a_block_that_comes_without_a_separator_gets_one_and_an_empty_block_none() {
    let heading = || MenuEntry::Heading("Account".into());
    assert!(matches!(
        set_off(vec![heading()]).as_slice(),
        [MenuEntry::Separator, MenuEntry::Heading(_)]
    ));
    assert!(matches!(
        set_off(vec![MenuEntry::Separator, heading()]).as_slice(),
        [MenuEntry::Separator, MenuEntry::Heading(_)]
    ));
    assert!(set_off(Vec::new()).is_empty());
}

#[test]
fn a_pane_that_does_not_run_claude_code_has_no_account_block() {
    let harness = Harness::new(Locale::En, "graphite");
    // `p2` is a shell.
    let block = crate::accounts::menu_items(harness.ctx(), &"w1".to_string(), Some(&"p2".to_string()));
    assert!(block.is_empty());
}
