//! Which pane an agent signing the board as `assignee` runs in
//! (`src/lib/agentPane.ts`, tested by `agentPane.test.ts`).
//!
//! The board only knows the free-form name an agent chose when it claimed, and
//! nothing ties that name to a pane. So this guesses, and only answers when the
//! guess is unambiguous: focusing the wrong agent is worse than offering no
//! shortcut at all.
//!
//! 1. a workspace and one of its panes, in either order ("hangar-ava", or
//!    "Ava (Hangar)", the name a Claude Code session runs under), which also
//!    tells apart two panes a board from before names were unique names alike;
//! 2. a pane whose name is in the assignee ("Ava", "claude:ava"), in the active
//!    workspace first, then in any other, since pane names are unique across
//!    workspaces; the longest name wins, so "Ava 2" beats "Ava";
//! 3. the only pane of the active workspace running the agent the assignee
//!    names ("claude", "codex-1").
//!
//! TODO(P4): `lib/agentPane.ts` belongs to the pane-logic package
//! (`panes_logic/agent_pane.rs`), which has not landed. This is the same
//! function with the same tests, kept here so the board does not wait for it;
//! replace the body with a call to P4's when it is merged.

use crate::state::{PaneId, Workspace, WsId};

/// The pane an assignee leads to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneTarget {
    pub workspace_id: WsId,
    pub pane_id: PaneId,
}

/// The lower-case words of a name, split on anything that is not a letter or a
/// digit (the TS split on `[^\p{L}\p{N}]+`).
fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Whether `run` appears in `all` as consecutive entries.
fn contains_run(all: &[String], run: &[String]) -> bool {
    if run.is_empty() || run.len() > all.len() {
        return false;
    }
    all.windows(run.len()).any(|window| window == run)
}

/// The pane whose name is the longest one the assignee contains, if exactly
/// one pane has it.
fn named(said: &[String], candidates: &[&Workspace]) -> Option<PaneTarget> {
    let mut best: Vec<(PaneTarget, usize)> = Vec::new();
    for ws in candidates {
        for pane in &ws.panes {
            let name = words(&pane.name);
            if !contains_run(said, &name) {
                continue;
            }
            if best.first().is_none_or(|(_, length)| name.len() > *length) {
                best.clear();
            }
            if best.first().is_none_or(|(_, length)| name.len() == *length) {
                best.push((
                    PaneTarget {
                        workspace_id: ws.id.clone(),
                        pane_id: pane.id.clone(),
                    },
                    name.len(),
                ));
            }
        }
    }
    match best.as_slice() {
        [(target, _)] => Some(target.clone()),
        _ => None,
    }
}

pub fn find_agent_pane(
    assignee: &str,
    workspaces: &[Workspace],
    active_id: Option<&str>,
) -> Option<PaneTarget> {
    let said = words(assignee);
    if said.is_empty() {
        return None;
    }
    let active = workspaces
        .iter()
        .find(|ws| Some(ws.id.as_str()) == active_id);

    for ws in workspaces {
        let prefix = words(&ws.name);
        if prefix.is_empty() {
            continue;
        }
        // Longest name first, so "Ava 2" is not taken for "Ava".
        let mut panes: Vec<_> = ws.panes.iter().collect();
        panes.sort_by_key(|pane| std::cmp::Reverse(words(&pane.name).len()));
        for pane in panes {
            let name = words(&pane.name);
            let before: Vec<String> = prefix.iter().chain(&name).cloned().collect();
            let after: Vec<String> = name.iter().chain(&prefix).cloned().collect();
            if contains_run(&said, &before) || contains_run(&said, &after) {
                return Some(PaneTarget {
                    workspace_id: ws.id.clone(),
                    pane_id: pane.id.clone(),
                });
            }
        }
    }

    if let Some(here) = active.and_then(|ws| named(&said, &[ws])) {
        return Some(here);
    }
    let everywhere: Vec<&Workspace> = workspaces.iter().collect();
    if let Some(anywhere) = named(&said, &everywhere) {
        return Some(anywhere);
    }

    if let Some(ws) = active {
        let running: Vec<_> = ws
            .panes
            .iter()
            .filter(|pane| {
                pane.agent != crate::state::AgentId::Shell
                    && said.iter().any(|word| word == pane.agent.as_str())
            })
            .collect();
        if let [only] = running.as_slice() {
            return Some(PaneTarget {
                workspace_id: ws.id.clone(),
                pane_id: only.id.clone(),
            });
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{AgentId, Pane};

    fn workspace(id: &str, name: &str, panes: &[(&str, AgentId)]) -> Workspace {
        Workspace {
            id: id.into(),
            name: name.into(),
            cwd: format!("/repo/{id}"),
            panes: panes
                .iter()
                .map(|(pane_name, agent)| Pane {
                    id: format!("{id}:{pane_name}"),
                    name: (*pane_name).into(),
                    agent: *agent,
                    ..Pane::default()
                })
                .collect(),
            ..Workspace::default()
        }
    }

    fn fixture() -> Vec<Workspace> {
        vec![
            workspace(
                "front",
                "Storefront",
                &[
                    ("Ava", AgentId::Claude),
                    ("Ava 2", AgentId::Claude),
                    ("Max", AgentId::Codex),
                ],
            ),
            workspace(
                "back",
                "Hangar",
                &[("Ava", AgentId::Claude), ("Leo", AgentId::Shell)],
            ),
        ]
    }

    fn pane_of(assignee: &str, active: &str) -> Option<String> {
        find_agent_pane(assignee, &fixture(), Some(active)).map(|target| target.pane_id)
    }

    #[test]
    fn finds_a_pane_of_the_active_workspace_by_its_name_inside_the_assignee() {
        assert_eq!(pane_of("Max", "front").as_deref(), Some("front:Max"));
        assert_eq!(pane_of("codex:max", "front").as_deref(), Some("front:Max"));
    }

    #[test]
    fn prefers_the_longest_pane_name() {
        assert_eq!(pane_of("ava-2", "front").as_deref(), Some("front:Ava 2"));
        assert_eq!(pane_of("ava", "front").as_deref(), Some("front:Ava"));
    }

    #[test]
    fn finds_a_name_that_only_one_workspace_has_wherever_it_is() {
        let all = fixture();
        let expected = PaneTarget {
            workspace_id: "back".into(),
            pane_id: "back:Leo".into(),
        };
        assert_eq!(
            find_agent_pane("Leo", &all, Some("front")),
            Some(expected.clone())
        );
        assert_eq!(
            find_agent_pane("hangar-leo", &all, Some("front")),
            Some(expected)
        );
    }

    #[test]
    fn reads_a_claude_code_session_name_workspace_and_all() {
        // The fixture still has an "Ava" in both workspaces, as a board from
        // before names were unique might: the workspace settles it.
        let all = fixture();
        assert_eq!(
            find_agent_pane("Ava (Hangar)", &all, Some("front")),
            Some(PaneTarget {
                workspace_id: "back".into(),
                pane_id: "back:Ava".into(),
            })
        );
        assert_eq!(
            pane_of("Ava 2 (Storefront)", "back").as_deref(),
            Some("front:Ava 2")
        );
    }

    #[test]
    fn falls_back_on_the_agent_kind_only_when_one_pane_runs_it() {
        assert_eq!(pane_of("codex-1", "front").as_deref(), Some("front:Max"));
        assert_eq!(pane_of("claude", "front"), None);
        assert_eq!(pane_of("claude", "back").as_deref(), Some("back:Ava"));
    }

    #[test]
    fn does_not_match_a_word_that_only_contains_a_pane_name() {
        assert_eq!(pane_of("maximilian", "front"), None);
        assert_eq!(pane_of("agent", "front"), None);
    }

    #[test]
    fn an_assignee_without_words_leads_nowhere() {
        assert_eq!(pane_of("  - ", "front"), None);
    }
}
