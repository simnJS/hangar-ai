//! Where each pane's agent stands (`lib/agentState.ts`).
//!
//! The pane works its own state out, from the hangar-bridge mod when its
//! Claude Code loaded it and from the shape of its output otherwise
//! (`pane_agent`), and publishes it here. The navigator, the toolbar, the
//! needs-you list and the engines read it from nowhere near the pane.
//!
//! - `working`: a turn is running.
//! - `waiting`: a permission dialog (or a question) is waiting for an answer.
//! - `yours`: the agent handed control back and nobody has looked since.
//! - `idle`: nothing to report.
//!
//! The web app kept a version counter, a cache of summaries and a relay to
//! the other windows. None of that is needed here: every window is a window
//! of one process over one `Model`, and a summary is a count over a handful
//! of entries, taken when asked.

use std::collections::HashMap;

use crate::state::live::{
    AgentActivity, AgentRoster, EpochMs, PaneActivity, PlanLimits, RateLimit, WorkspaceActivity,
};
use crate::state::types::{PaneId, WsId};

/// How long the mark of an overlapping edit shows: long enough to be noticed
/// on a glance at the grid, short of becoming noise.
pub const OVERLAP_SHOWN_MS: u64 = 2 * 60 * 1000;

/// A pane's activity once its Claude Code's mod says hello. The session has
/// just started and runs no turn yet, so a "working" guessed from its
/// startup output goes: guesses stop once the mod speaks, and nothing else
/// would take it back before the first turn. A wait or a hand-back is left
/// as it is.
pub fn activity_at_hello(activity: AgentActivity) -> AgentActivity {
    if activity == AgentActivity::Working {
        AgentActivity::Idle
    } else {
        activity
    }
}

/// Panes whose file another pane just edited too. Nothing was stopped (both
/// edits went through), so the mark only says it, and clears itself.
#[derive(Debug, Clone)]
struct Conflict {
    /// The name of the pane that followed.
    by: String,
    until: EpochMs,
}

/// The published activity of every pane that reports one, the plan's limits,
/// and the overlap marks.
#[derive(Debug, Default)]
pub struct AgentStates {
    panes: HashMap<PaneId, PaneActivity>,
    /// Insertion order, as the `Map` of the TS kept it: the navigator names
    /// the agents of a workspace in a stable order.
    order: Vec<PaneId>,
    /// The plan's rate-limit windows, from whichever session reported last.
    plan_limits: Option<PlanLimits>,
    conflicts: HashMap<PaneId, Conflict>,
}

impl AgentStates {
    /// Publishes a pane's activity. Whether anything changed: the same entry
    /// again changes nothing.
    pub fn set(&mut self, pane: &str, value: PaneActivity) -> bool {
        match self.panes.get_mut(pane) {
            Some(current) if *current == value => false,
            Some(current) => {
                *current = value;
                true
            }
            None => {
                self.order.push(pane.to_string());
                self.panes.insert(pane.to_string(), value);
                true
            }
        }
    }

    /// Takes a pane out. Whether it was there.
    pub fn clear(&mut self, pane: &str) -> bool {
        if self.panes.remove(pane).is_none() {
            return false;
        }
        self.order.retain(|id| id != pane);
        true
    }

    pub fn get(&self, pane: &str) -> Option<&PaneActivity> {
        self.panes.get(pane)
    }

    /// Every entry, for the snapshot the engines read.
    pub fn all(&self) -> &HashMap<PaneId, PaneActivity> {
        &self.panes
    }

    /// The pane's agent is waiting for an answer, rather than at work or done.
    pub fn is_waiting(&self, pane: &str) -> bool {
        self.get(pane)
            .is_some_and(|entry| entry.activity == AgentActivity::Waiting)
    }

    /// Marks `pane` as overlapped by `by` for a couple of minutes. Whether
    /// the mark shown changed.
    pub fn flag_overlap(&mut self, pane: &str, by: &str, now: EpochMs) -> bool {
        let until = now + OVERLAP_SHOWN_MS;
        match self.conflicts.get_mut(pane) {
            Some(conflict) if conflict.by == by => {
                conflict.until = until;
                false
            }
            Some(conflict) => {
                *conflict = Conflict {
                    by: by.to_string(),
                    until,
                };
                true
            }
            None => {
                self.conflicts.insert(
                    pane.to_string(),
                    Conflict {
                        by: by.to_string(),
                        until,
                    },
                );
                true
            }
        }
    }

    /// The name of the pane that just edited a file this one had edited, if
    /// any (`useConflict`). Marks wear off in `expire`, which the host calls
    /// now and then.
    pub fn conflict(&self, pane: &str) -> Option<&str> {
        self.conflicts
            .get(pane)
            .map(|conflict| conflict.by.as_str())
    }

    pub fn has_conflicts(&self) -> bool {
        !self.conflicts.is_empty()
    }

    /// Drops the marks that have been shown long enough. Whether any went.
    pub fn expire(&mut self, now: EpochMs) -> bool {
        let before = self.conflicts.len();
        self.conflicts.retain(|_, conflict| conflict.until > now);
        self.conflicts.len() != before
    }

    /// Keeps the limits of the session that reported last. A report with no
    /// window says nothing and changes nothing.
    pub fn set_plan_limits(&mut self, limits: Vec<RateLimit>, now: EpochMs) -> bool {
        if limits.is_empty() {
            return false;
        }
        self.plan_limits = Some(PlanLimits { limits, at: now });
        true
    }

    pub fn plan_limits(&self) -> Option<&PlanLimits> {
        self.plan_limits.as_ref()
    }

    /// How many of a workspace's agents are working, waiting, or done.
    pub fn summarize(&self, workspace_id: &str) -> WorkspaceActivity {
        let mut value = WorkspaceActivity::default();
        for pane in self.panes.values() {
            if pane.workspace_id != workspace_id {
                continue;
            }
            match pane.activity {
                AgentActivity::Working => value.working += 1,
                AgentActivity::Waiting => value.waiting += 1,
                AgentActivity::Yours => value.yours += 1,
                AgentActivity::Idle => {}
            }
        }
        value
    }

    /// Which panes of these workspaces are working, waiting, or done: the
    /// navigator's summary of a workspace, or of a folded folder, names them.
    pub fn roster(&self, workspace_ids: &[WsId]) -> AgentRoster {
        let mut roster = AgentRoster::default();
        if workspace_ids.is_empty() {
            return roster;
        }
        for id in &self.order {
            let Some(pane) = self.panes.get(id) else {
                continue;
            };
            if !workspace_ids.contains(&pane.workspace_id) {
                continue;
            }
            match pane.activity {
                AgentActivity::Working => roster.working.push(id.clone()),
                AgentActivity::Waiting => roster.waiting.push(id.clone()),
                AgentActivity::Yours => roster.yours.push(id.clone()),
                AgentActivity::Idle => {}
            }
        }
        roster
    }

    /// How many of these panes have a Claude Code that loaded the mod.
    pub fn bridged_count<'a>(&self, pane_ids: impl IntoIterator<Item = &'a PaneId>) -> usize {
        pane_ids
            .into_iter()
            .filter(|id| self.panes.get(*id).is_some_and(|pane| pane.bridged))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(ws: &str, activity: AgentActivity, bridged: bool) -> PaneActivity {
        PaneActivity {
            workspace_id: ws.into(),
            activity,
            bridged,
        }
    }

    #[test]
    fn drops_the_work_guessed_from_a_starting_sessions_output() {
        assert_eq!(
            activity_at_hello(AgentActivity::Working),
            AgentActivity::Idle
        );
    }

    #[test]
    fn leaves_a_wait_a_hand_back_and_an_idle_pane_as_they_are() {
        assert_eq!(
            activity_at_hello(AgentActivity::Waiting),
            AgentActivity::Waiting
        );
        assert_eq!(
            activity_at_hello(AgentActivity::Yours),
            AgentActivity::Yours
        );
        assert_eq!(activity_at_hello(AgentActivity::Idle), AgentActivity::Idle);
    }

    #[test]
    fn publishing_the_same_entry_again_changes_nothing() {
        let mut states = AgentStates::default();
        assert!(states.set("a", entry("w", AgentActivity::Working, false)));
        assert!(!states.set("a", entry("w", AgentActivity::Working, false)));
        assert!(states.set("a", entry("w", AgentActivity::Working, true)));
        assert!(states.set("a", entry("w2", AgentActivity::Working, true)));
        assert!(states.clear("a"));
        assert!(!states.clear("a"));
        assert!(states.get("a").is_none());
    }

    #[test]
    fn counts_a_workspaces_agents_by_state() {
        let mut states = AgentStates::default();
        states.set("a", entry("w", AgentActivity::Working, true));
        states.set("b", entry("w", AgentActivity::Working, false));
        states.set("c", entry("w", AgentActivity::Waiting, false));
        states.set("d", entry("w", AgentActivity::Yours, false));
        states.set("e", entry("w", AgentActivity::Idle, false));
        states.set("f", entry("other", AgentActivity::Working, false));
        assert_eq!(
            states.summarize("w"),
            WorkspaceActivity {
                working: 2,
                waiting: 1,
                yours: 1
            }
        );
        assert_eq!(states.summarize("nobody"), WorkspaceActivity::default());
    }

    #[test]
    fn names_the_agents_of_some_workspaces_in_the_order_they_reported() {
        let mut states = AgentStates::default();
        states.set("a", entry("w1", AgentActivity::Working, false));
        states.set("b", entry("w2", AgentActivity::Waiting, false));
        states.set("c", entry("w1", AgentActivity::Working, false));
        states.set("d", entry("w3", AgentActivity::Yours, false));
        states.set("e", entry("w1", AgentActivity::Idle, false));
        // A pane that changes state keeps its place.
        states.set("a", entry("w1", AgentActivity::Working, true));

        let roster = states.roster(&["w1".into(), "w2".into()]);
        assert_eq!(roster.working, ["a", "c"]);
        assert_eq!(roster.waiting, ["b"]);
        assert!(roster.yours.is_empty());
        assert_eq!(states.roster(&[]), AgentRoster::default());
    }

    #[test]
    fn counts_the_panes_whose_claude_code_loaded_the_mod() {
        let mut states = AgentStates::default();
        states.set("a", entry("w", AgentActivity::Idle, true));
        states.set("b", entry("w", AgentActivity::Idle, false));
        let ids: Vec<PaneId> = vec!["a".into(), "b".into(), "never".into()];
        assert_eq!(states.bridged_count(&ids), 1);
    }

    #[test]
    fn knows_a_pane_that_waits() {
        let mut states = AgentStates::default();
        states.set("a", entry("w", AgentActivity::Waiting, false));
        states.set("b", entry("w", AgentActivity::Working, false));
        assert!(states.is_waiting("a"));
        assert!(!states.is_waiting("b"));
        assert!(!states.is_waiting("never"));
    }

    #[test]
    fn an_overlap_mark_shows_for_two_minutes_and_clears_itself() {
        let mut states = AgentStates::default();
        assert!(states.flag_overlap("a", "Ava", 1000));
        assert_eq!(states.conflict("a"), Some("Ava"));
        // The same pane again extends the mark without announcing a change.
        assert!(!states.flag_overlap("a", "Ava", 61_000));
        assert!(!states.expire(61_000 + OVERLAP_SHOWN_MS - 1));
        assert_eq!(states.conflict("a"), Some("Ava"));
        assert!(states.expire(61_000 + OVERLAP_SHOWN_MS));
        assert_eq!(states.conflict("a"), None);
        assert!(!states.has_conflicts());
        // Another pane taking over replaces the name.
        states.flag_overlap("a", "Ava", 0);
        assert!(states.flag_overlap("a", "Bob", 10));
        assert_eq!(states.conflict("a"), Some("Bob"));
    }

    #[test]
    fn a_report_without_a_window_leaves_the_plan_limits_as_they_were() {
        let mut states = AgentStates::default();
        assert!(!states.set_plan_limits(Vec::new(), 5));
        assert!(states.plan_limits().is_none());
        let limit = RateLimit {
            kind: "five_hour".into(),
            percent_used: 42.0,
            resets_at: None,
        };
        assert!(states.set_plan_limits(vec![limit.clone()], 10));
        assert!(!states.set_plan_limits(Vec::new(), 20));
        let plan = states.plan_limits().unwrap();
        assert_eq!((plan.limits.as_slice(), plan.at), (&[limit][..], 10));
    }
}
