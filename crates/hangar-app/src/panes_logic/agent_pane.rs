//! Which pane an agent signing the board as `assignee` runs in
//! (`lib/agentPane.ts`).
//!
//! The board only knows the free-form name an agent chose when it claimed,
//! and nothing ties that name to a pane. So this guesses, and only answers
//! when the guess is unambiguous: focusing the wrong agent is worse than
//! offering no shortcut at all.
//!
//! 1. A workspace and one of its panes, in either order ("hangar-ava", or
//!    "Ava (Hangar)", the name a Claude Code session runs under), which also
//!    tells apart two panes a board from before names were unique names
//!    alike.
//! 2. A pane whose name is in the assignee ("Ava", "claude:ava"), in the
//!    active workspace first, then in any other, since pane names are unique
//!    across workspaces; the longest name wins, so "Ava 2" beats "Ava".
//! 3. The only pane of the active workspace running the agent the assignee
//!    names ("claude", "claude-1"); a shell runs none.

use crate::state::types::{AgentId, PaneId, Workspace, WsId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneTarget {
    pub workspace_id: WsId,
    pub pane_id: PaneId,
}

/// Lowercase runs of letters and digits, whatever separates them.
fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !(c.is_alphabetic() || c.is_numeric()))
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

/// Whether `run` appears in `all` as consecutive entries.
fn contains_run(all: &[String], run: &[String]) -> bool {
    if run.is_empty() || run.len() > all.len() {
        return false;
    }
    all.windows(run.len()).any(|window| window == run)
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
    let active = active_id.and_then(|id| workspaces.iter().find(|ws| ws.id == id));

    // The one pane, among these workspaces, with the longest name inside what
    // was said; none when two share that length.
    let named = |candidates: &[&Workspace]| -> Option<PaneTarget> {
        let mut best: Vec<(PaneTarget, usize)> = Vec::new();
        for ws in candidates {
            for pane in &ws.panes {
                let name = words(&pane.name);
                if !contains_run(&said, &name) {
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
    };

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
            let workspace_first: Vec<String> = prefix.iter().chain(&name).cloned().collect();
            let pane_first: Vec<String> = name.iter().chain(&prefix).cloned().collect();
            if contains_run(&said, &workspace_first) || contains_run(&said, &pane_first) {
                return Some(PaneTarget {
                    workspace_id: ws.id.clone(),
                    pane_id: pane.id.clone(),
                });
            }
        }
    }

    if let Some(here) = active.and_then(|ws| named(&[ws])) {
        return Some(here);
    }
    let everywhere: Vec<&Workspace> = workspaces.iter().collect();
    if let Some(anywhere) = named(&everywhere) {
        return Some(anywhere);
    }

    if let Some(active) = active {
        let running: Vec<_> = active
            .panes
            .iter()
            .filter(|pane| {
                pane.agent != AgentId::Shell && said.iter().any(|w| w == pane.agent.as_str())
            })
            .collect();
        if let [only] = running.as_slice() {
            return Some(PaneTarget {
                workspace_id: active.id.clone(),
                pane_id: only.id.clone(),
            });
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::types::Pane;

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

    fn fixtures() -> Vec<Workspace> {
        vec![
            workspace(
                "front",
                "Storefront",
                &[
                    ("Ava", AgentId::Claude),
                    ("Ava 2", AgentId::Claude),
                    ("Max", AgentId::Shell),
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
        find_agent_pane(assignee, &fixtures(), Some(active)).map(|target| target.pane_id)
    }

    #[test]
    fn finds_a_pane_of_the_active_workspace_by_its_name_inside_the_assignee() {
        assert_eq!(pane_of("Max", "front").as_deref(), Some("front:Max"));
        assert_eq!(pane_of("shell:max", "front").as_deref(), Some("front:Max"));
    }

    #[test]
    fn prefers_the_longest_pane_name() {
        assert_eq!(pane_of("ava-2", "front").as_deref(), Some("front:Ava 2"));
        assert_eq!(pane_of("ava", "front").as_deref(), Some("front:Ava"));
    }

    #[test]
    fn finds_a_name_that_only_one_workspace_has_wherever_it_is() {
        let all = fixtures();
        assert_eq!(
            find_agent_pane("Leo", &all, Some("front")),
            Some(PaneTarget {
                workspace_id: "back".into(),
                pane_id: "back:Leo".into()
            })
        );
        assert_eq!(
            find_agent_pane("hangar-leo", &all, Some("front")),
            Some(PaneTarget {
                workspace_id: "back".into(),
                pane_id: "back:Leo".into()
            })
        );
    }

    #[test]
    fn reads_a_claude_code_session_name_workspace_and_all() {
        // The fixture still has an "Ava" in both workspaces, as a board from
        // before names were unique might: the workspace settles it.
        let all = fixtures();
        assert_eq!(
            find_agent_pane("Ava (Hangar)", &all, Some("front")),
            Some(PaneTarget {
                workspace_id: "back".into(),
                pane_id: "back:Ava".into()
            })
        );
        assert_eq!(
            pane_of("Ava 2 (Storefront)", "back").as_deref(),
            Some("front:Ava 2")
        );
    }

    #[test]
    fn falls_back_on_the_agent_kind_only_when_one_pane_runs_it() {
        assert_eq!(pane_of("claude", "front"), None);
        assert_eq!(pane_of("claude-1", "back").as_deref(), Some("back:Ava"));
        // A shell is not an agent anyone signs as.
        assert_eq!(pane_of("shell", "back"), None);
    }

    #[test]
    fn does_not_match_a_word_that_only_contains_a_pane_name() {
        assert_eq!(pane_of("maximilian", "front"), None);
        assert_eq!(pane_of("agent", "front"), None);
    }
}
