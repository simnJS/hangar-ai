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

use std::cell::RefCell;
use std::collections::HashMap;

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

/// What [`find_agent_pane`] reads of one workspace: its id and name, and the
/// id, name and agent of each of its panes. Nothing else changes its answer.
#[derive(Debug, Clone, PartialEq)]
struct Seen {
    id: WsId,
    name: String,
    panes: Vec<(PaneId, String, AgentId)>,
}

fn same_inputs(seen: &[Seen], workspaces: &[Workspace]) -> bool {
    seen.len() == workspaces.len()
        && seen.iter().zip(workspaces).all(|(seen, ws)| {
            seen.id == ws.id
                && seen.name == ws.name
                && seen.panes.len() == ws.panes.len()
                && seen
                    .panes
                    .iter()
                    .zip(&ws.panes)
                    .all(|((id, name, agent), pane)| {
                        *id == pane.id && *name == pane.name && *agent == pane.agent
                    })
        })
}

/// The answers for one active workspace, by assignee.
type Answers = HashMap<String, Option<PaneTarget>>;

/// Past answers of [`find_agent_pane`], kept while the workspaces it read
/// stay the same.
///
/// The inspector, the board and the board dock ask which pane each assignee
/// of the board is in from their views, and a view is rebuilt after every
/// message: during a flood, every frame. Each answer splits and lowercases
/// the name of every pane of every workspace, so a board of a hundred open
/// tasks cost about half a millisecond a frame, with nothing changed. The
/// memo is checked against the workspaces once per batch of questions (a
/// comparison, no allocation) and answers each from a map after that.
#[derive(Debug, Default)]
pub struct PaneFinder {
    seen: Vec<Seen>,
    /// By active workspace (a handful), then by assignee.
    answers: Vec<(Option<WsId>, Answers)>,
}

/// More assignees than any board has: past it the memo starts over, so names
/// that came and went do not pile up.
const MEMO_LIMIT: usize = 4096;

impl PaneFinder {
    /// The finder, ready to answer for `workspaces`: what it knew is dropped
    /// if they are not the ones it last answered for.
    pub fn over<'a>(&'a mut self, workspaces: &'a [Workspace]) -> Finding<'a> {
        if !same_inputs(&self.seen, workspaces) {
            self.seen = workspaces
                .iter()
                .map(|ws| Seen {
                    id: ws.id.clone(),
                    name: ws.name.clone(),
                    panes: ws
                        .panes
                        .iter()
                        .map(|pane| (pane.id.clone(), pane.name.clone(), pane.agent))
                        .collect(),
                })
                .collect();
            self.answers.clear();
        }
        Finding {
            finder: self,
            workspaces,
        }
    }
}

/// A [`PaneFinder`] checked against the workspaces it answers for.
pub struct Finding<'a> {
    finder: &'a mut PaneFinder,
    workspaces: &'a [Workspace],
}

impl Finding<'_> {
    /// What [`find_agent_pane`] answers, from the memo when it can.
    pub fn find(&mut self, assignee: &str, active_id: Option<&str>) -> Option<PaneTarget> {
        let answers = &mut self.finder.answers;
        let at = match answers
            .iter()
            .position(|(id, _)| id.as_deref() == active_id)
        {
            Some(at) => at,
            None => {
                answers.push((active_id.map(str::to_owned), HashMap::new()));
                answers.len() - 1
            }
        };
        let memo = &mut answers[at].1;
        if let Some(answer) = memo.get(assignee) {
            return answer.clone();
        }
        if memo.len() >= MEMO_LIMIT {
            memo.clear();
        }
        let answer = find_agent_pane(assignee, self.workspaces, active_id);
        memo.insert(assignee.to_owned(), answer.clone());
        answer
    }
}

thread_local! {
    /// The UI thread's finder (views only ever run there). A thread of its
    /// own for each test, which is as right: it is keyed by what it read.
    static FINDER: RefCell<PaneFinder> = RefCell::new(PaneFinder::default());
}

/// Runs `f` with this thread's [`PaneFinder`], checked against `workspaces`.
pub fn with_finder<R>(workspaces: &[Workspace], f: impl FnOnce(&mut Finding<'_>) -> R) -> R {
    FINDER.with(|finder| {
        let mut finder = finder.borrow_mut();
        let mut finding = finder.over(workspaces);
        f(&mut finding)
    })
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

    #[test]
    fn the_memo_answers_what_find_agent_pane_answers() {
        let all = fixtures();
        let mut finder = PaneFinder::default();
        let names = [
            "Max",
            "shell:max",
            "ava-2",
            "ava",
            "Leo",
            "hangar-ava",
            "claude",
            "nobody",
        ];
        for active in [Some("front"), Some("back"), None] {
            for name in names {
                // Twice: once computed, once from the memo.
                for _ in 0..2 {
                    assert_eq!(
                        finder.over(&all).find(name, active),
                        find_agent_pane(name, &all, active),
                        "{name} from {active:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_memo_starts_over_when_a_pane_is_renamed_or_runs_another_agent() {
        let mut all = fixtures();
        let mut finder = PaneFinder::default();
        assert_eq!(
            finder
                .over(&all)
                .find("Max", Some("front"))
                .map(|t| t.pane_id),
            Some("front:Max".to_string())
        );
        all[0].panes[2].name = "Zoe".into();
        assert_eq!(finder.over(&all).find("Max", Some("front")), None);
        assert_eq!(
            finder
                .over(&all)
                .find("Zoe", Some("front"))
                .map(|t| t.pane_id),
            Some("front:Max".to_string())
        );

        // The only Claude Code pane of "back" answers "claude"; a second one
        // makes it ambiguous.
        assert_eq!(
            finder
                .over(&all)
                .find("claude", Some("back"))
                .map(|t| t.pane_id),
            Some("back:Ava".to_string())
        );
        all[1].panes[1].agent = AgentId::Claude;
        assert_eq!(finder.over(&all).find("claude", Some("back")), None);
    }

    #[test]
    fn the_memo_starts_over_when_workspaces_come_go_or_are_renamed() {
        let mut all = fixtures();
        let mut finder = PaneFinder::default();
        assert_eq!(
            finder
                .over(&all)
                .find("hangar-ava", Some("front"))
                .map(|t| t.pane_id),
            Some("back:Ava".to_string())
        );
        all[1].name = "Dock".into();
        assert_eq!(
            finder.over(&all).find("hangar-ava", Some("front")),
            find_agent_pane("hangar-ava", &all, Some("front"))
        );
        all.pop();
        assert_eq!(finder.over(&all).find("Leo", Some("front")), None);
    }

    #[test]
    fn the_thread_finder_answers_like_the_function() {
        let all = fixtures();
        for name in ["Max", "Leo", "ava"] {
            assert_eq!(
                with_finder(&all, |finder| finder.find(name, Some("front"))),
                find_agent_pane(name, &all, Some("front"))
            );
        }
    }
}
