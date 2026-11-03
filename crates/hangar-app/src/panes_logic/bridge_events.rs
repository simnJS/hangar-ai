//! What the hangar-bridge mod reports from inside a pane's Claude Code
//! (`lib/bridge.ts`), and the small helpers of PaneStatus.ts the reports are
//! worded with.
//!
//! `BridgeEvent` itself, with `BridgeEvent::parse`, is declared in
//! `state::live`. The web app subscribed each pane to a global listener
//! (`subscribeBridge`); here core's events reach `panes_logic::on_event`,
//! which finds the pane and gives the report to its machine
//! (`pane_agent`). What is left of `lib/bridge.ts` is the queue carried over
//! a restart.

use std::collections::HashMap;

use crate::state::live::{EpochMs, QueuedPrompt};

/// Prompts queued for a pane that is restarting under a new id. The backend
/// keeps the queue per id and drops it with the old pane, so the old pane
/// stashes what it still holds and the new one (same workspace, same name)
/// takes it. It wears off: a pane closed for real must not pass its queue to
/// one created later under the same name.
pub const CARRY_MS: u64 = 10_000;

/// The key a restart's queue is stashed under.
pub fn carry_key(workspace_id: &str, pane_name: &str) -> String {
    format!("{workspace_id}/{pane_name}")
}

#[derive(Debug, Default)]
pub struct CarriedQueues {
    carried: HashMap<String, (EpochMs, Vec<QueuedPrompt>)>,
}

impl CarriedQueues {
    pub fn stash(&mut self, key: &str, prompts: Vec<QueuedPrompt>, now: EpochMs) {
        if prompts.is_empty() {
            self.carried.remove(key);
        } else {
            self.carried
                .insert(key.to_string(), (now + CARRY_MS, prompts));
        }
    }

    /// The stashed prompts, once: taking them empties the stash.
    pub fn take(&mut self, key: &str, now: EpochMs) -> Vec<QueuedPrompt> {
        match self.carried.remove(key) {
            Some((until, prompts)) if until >= now => prompts,
            _ => Vec::new(),
        }
    }
}

/// The first line with something on it, trimmed: an event is one line
/// (`firstLine`).
pub fn first_line(text: &str, max: usize) -> String {
    let line = text
        .split('\n')
        .map(|entry| entry.strip_suffix('\r').unwrap_or(entry))
        .find(|entry| !entry.trim().is_empty())
        .map_or("", str::trim);
    if line.chars().count() > max {
        let cut: String = line.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    } else {
        line.to_string()
    }
}

/// How long an event's line may run in the history.
pub const EVENT_LINE_MAX: usize = 120;

/// `src/lib/layout.ts` becomes `layout.ts`, whichever separator the agent
/// used (`fileBaseName`).
pub fn file_base_name(path: &str) -> String {
    let trimmed = path.trim_end_matches(['\\', '/']);
    match trimmed.rsplit(['\\', '/']).next() {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => trimmed.to_string(),
    }
}

/// Session ids are long; six characters tell two apart on screen
/// (`shortSessionId`).
pub fn short_session_id(id: &str) -> String {
    id.chars().take(6).collect()
}

/// The short id, with an ellipsis saying there is more of it
/// (`sessionIdLabel`).
pub fn session_id_label(id: &str) -> String {
    let short = short_session_id(id);
    if short.chars().count() < id.chars().count() {
        format!("{short}…")
    } else {
        id.to_string()
    }
}

/// The prompts that joined the queue since it last changed, oldest first
/// (`newlyQueued`).
pub fn newly_queued<'a>(
    before: &[QueuedPrompt],
    after: &'a [QueuedPrompt],
) -> Vec<&'a QueuedPrompt> {
    after
        .iter()
        .filter(|entry| !before.iter().any(|known| known.id == entry.id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brief() -> QueuedPrompt {
        QueuedPrompt {
            id: "q1".into(),
            text: "do the thing".into(),
            from: Some("Jack".into()),
        }
    }

    // queue carried over a restart

    #[test]
    fn hands_the_stashed_prompts_to_the_new_pane_once() {
        let mut carried = CarriedQueues::default();
        let key = carry_key("ws", "Tara");
        carried.stash(&key, vec![brief()], 1000);
        assert_eq!(carried.take(&key, 2000), vec![brief()]);
        assert_eq!(carried.take(&key, 2000), vec![]);
    }

    #[test]
    fn lets_go_of_a_stash_that_wore_off() {
        let mut carried = CarriedQueues::default();
        let key = carry_key("ws", "Max");
        carried.stash(&key, vec![brief()], 1000);
        assert_eq!(carried.take(&key, 1000 + 60_000), vec![]);
    }

    #[test]
    fn keeps_panes_of_different_names_or_workspaces_apart() {
        let mut carried = CarriedQueues::default();
        carried.stash(&carry_key("ws", "Ava"), vec![brief()], 1000);
        assert_eq!(carried.take(&carry_key("ws", "Bob"), 1500), vec![]);
        assert_eq!(carried.take(&carry_key("other", "Ava"), 1500), vec![]);
        assert_eq!(carried.take(&carry_key("ws", "Ava"), 1500), vec![brief()]);
    }

    #[test]
    fn an_empty_queue_stashes_nothing() {
        let mut carried = CarriedQueues::default();
        let key = carry_key("ws", "Eve");
        carried.stash(&key, vec![brief()], 1000);
        carried.stash(&key, vec![], 1100);
        assert_eq!(carried.take(&key, 1200), vec![]);
    }

    #[test]
    fn a_stash_is_still_good_on_the_last_millisecond() {
        let mut carried = CarriedQueues::default();
        let key = carry_key("ws", "Zed");
        carried.stash(&key, vec![brief()], 1000);
        assert_eq!(carried.take(&key, 1000 + CARRY_MS), vec![brief()]);
    }

    // PaneStatus.ts

    #[test]
    fn keeps_the_first_line_that_says_something() {
        assert_eq!(
            first_line("\n  \n  Done: tests pass\nmore", 120),
            "Done: tests pass"
        );
        assert_eq!(first_line("", 120), "");
        assert_eq!(first_line("x".repeat(10).as_str(), 5), "xxxx…");
        assert_eq!(first_line("a\r\nb", 120), "a");
        assert_eq!(first_line("\r\n \r\nfound\r\n", 120), "found");
    }

    #[test]
    fn takes_a_files_name_off_either_kind_of_path() {
        assert_eq!(file_base_name("src/lib/layout.ts"), "layout.ts");
        assert_eq!(file_base_name("C:\\dev\\app\\main.rs"), "main.rs");
        assert_eq!(file_base_name("README.md"), "README.md");
        assert_eq!(file_base_name("src/board/"), "board");
        assert_eq!(file_base_name("/"), "");
    }

    #[test]
    fn shortens_session_ids() {
        assert_eq!(short_session_id("01J8Z4-pty-bridge"), "01J8Z4");
        assert_eq!(short_session_id("abc"), "abc");
        assert_eq!(session_id_label("01J8Z4-pty-bridge"), "01J8Z4…");
        assert_eq!(session_id_label("abc"), "abc");
    }

    #[test]
    fn finds_the_prompts_that_joined_the_queue() {
        let q = |id: &str| QueuedPrompt {
            id: id.into(),
            text: id.into(),
            from: None,
        };
        let (a, b, c) = (q("1"), q("2"), q("3"));
        let ids = |list: Vec<&QueuedPrompt>| -> Vec<String> {
            list.into_iter().map(|entry| entry.id.clone()).collect()
        };
        assert_eq!(ids(newly_queued(&[], &[a.clone(), b.clone()])), ["1", "2"]);
        assert_eq!(
            ids(newly_queued(&[a.clone(), b.clone()], &[b.clone(), c])),
            ["3"]
        );
        assert!(newly_queued(&[a, b.clone()], &[b]).is_empty());
    }
}
