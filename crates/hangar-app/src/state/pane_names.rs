//! Pane names (`src/lib/paneNames.ts`, tests in paneNames.test.ts).
//!
//! Panes are called by name rather than numbered. A number is a position, and
//! positions move: split, close or drag a pane and every index after it
//! shifts. A name stays with the pane, which is what makes it usable in a
//! sentence ("Leo is done, Ava is still running").
//!
//! A name is unique across every workspace, not only its own: Claude Code
//! sessions reach each other by name, and two "Ava" in two workspaces could
//! not be told apart.

use std::collections::HashSet;

use super::types::{Pane, Workspace};

/// Short, unambiguous, easy to say. The first sixteen are those panes were
/// named with before names became unique across workspaces, kept first and in
/// order so existing panes keep theirs.
pub const PANE_NAMES: [&str; 48] = [
    "Ava", "Max", "Leo", "Mia", "Sam", "Zoe", "Eli", "Nora", "Finn", "Ivy", "Theo", "Luna", "Jude",
    "Cleo", "Rex", "Wren", "Vadim", "Samuel", "Lenny", "Sasha", "Gabriel", "Simon", "Soren",
    "Phileas", "Ada", "Ben", "Cora", "Emma", "Hugo", "Iris", "Jack", "Kai", "Lola", "Milo",
    "Quinn", "Rosa", "Tara", "Vera", "Will", "Yara", "Zack", "Ezra", "Faye", "Hana", "Juno",
    "Omar", "Ruby", "Tess",
];

/// The first free name; "Ava 2" and on once the list runs out
/// (`pickPaneName`).
pub fn pick_pane_name<S: AsRef<str>>(taken: impl IntoIterator<Item = S>) -> String {
    let used: HashSet<String> = taken.into_iter().map(|s| s.as_ref().to_string()).collect();
    first_free(&used)
}

fn first_free(used: &HashSet<String>) -> String {
    if let Some(free) = PANE_NAMES.iter().find(|name| !used.contains(**name)) {
        return (*free).to_string();
    }
    (2..)
        .find_map(|round| {
            PANE_NAMES
                .iter()
                .map(|name| format!("{name} {round}"))
                .find(|candidate| !used.contains(candidate))
        })
        .expect("an endless supply of rounds always has a free name")
}

/// The next `count` free names, in the order panes would get them
/// (`pickPaneNames`).
pub fn pick_pane_names<S: AsRef<str>>(
    count: usize,
    taken: impl IntoIterator<Item = S>,
) -> Vec<String> {
    let mut used: HashSet<String> = taken.into_iter().map(|s| s.as_ref().to_string()).collect();
    (0..count)
        .map(|_| {
            let name = first_free(&used);
            used.insert(name.clone());
            name
        })
        .collect()
}

/// Every name in use, in every workspace; empty names left out
/// (`allPaneNames`).
pub fn all_pane_names<'a>(workspaces: impl IntoIterator<Item = &'a Workspace>) -> Vec<String> {
    workspaces
        .into_iter()
        .flat_map(|ws| ws.panes.iter())
        .map(|pane| pane.name.clone())
        .filter(|name| !name.is_empty())
        .collect()
}

/// The names of these panes (`paneNames`).
pub fn pane_names(panes: &[Pane]) -> Vec<String> {
    panes.iter().map(|pane| pane.name.clone()).collect()
}

/// Gives every pane a name no other pane has, in any workspace
/// (`uniquePaneNames`, run at hydration). A name already stored is kept where
/// it is the first of its kind, in the order the workspaces are listed; only
/// the later duplicates and the unnamed get a new one. Answers whether
/// anything changed.
pub fn unique_pane_names(workspaces: &mut [Workspace]) -> bool {
    let mut seen = HashSet::new();
    let mut clashing = Vec::new();
    for (w, ws) in workspaces.iter().enumerate() {
        for (p, pane) in ws.panes.iter().enumerate() {
            if pane.name.is_empty() || !seen.insert(pane.name.clone()) {
                clashing.push((w, p));
            }
        }
    }
    for &(w, p) in &clashing {
        let name = first_free(&seen);
        seen.insert(name.clone());
        workspaces[w].panes[p].name = name;
    }
    !clashing.is_empty()
}

/// The name a Claude Code session runs under, which other sessions list and
/// message it by: the pane's, then its workspace's (`sessionName`). Characters
/// a shell would read as quoting or expansion are left out of the workspace
/// part, so the name can always go on a command line.
pub fn session_name(pane_name: &str, workspace_name: &str) -> String {
    let cleaned: String = workspace_name
        .chars()
        .filter(|c| {
            !matches!(
                c,
                '"' | '\'' | '`' | '$' | '\\' | '%' | '!' | '^' | '&' | '|' | '<' | '>'
            )
        })
        .collect();
    let workspace = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if workspace.is_empty() {
        pane_name.to_string()
    } else {
        format!("{pane_name} ({workspace})")
    }
}

#[cfg(test)]
mod tests {
    //! paneNames.test.ts, case by case.

    use super::*;
    use crate::state::types::AgentId;

    fn pane(id: &str, name: &str) -> Pane {
        Pane {
            id: id.into(),
            name: name.into(),
            agent: AgentId::Claude,
            ..Pane::default()
        }
    }

    fn workspace(id: &str, panes: Vec<Pane>) -> Workspace {
        Workspace {
            id: id.into(),
            name: id.into(),
            cwd: format!("/repo/{id}"),
            panes,
            ..Workspace::default()
        }
    }

    #[test]
    fn keeps_the_names_panes_already_had_first_in_the_list() {
        assert_eq!(PANE_NAMES[..4], ["Ava", "Max", "Leo", "Mia"]);
        let unique: HashSet<&str> = PANE_NAMES.iter().copied().collect();
        assert_eq!(unique.len(), PANE_NAMES.len());
    }

    #[test]
    fn picks_names_nobody_uses_yet_in_order() {
        assert_eq!(pick_pane_names(3, ["Ava", "Leo"]), ["Max", "Mia", "Sam"]);
    }

    #[test]
    fn renames_only_the_later_duplicates_across_workspaces_and_the_unnamed() {
        let mut list = vec![
            workspace("one", vec![pane("a", "Ava"), pane("b", "Max")]),
            workspace(
                "two",
                vec![pane("c", "Ava"), pane("d", ""), pane("e", "Leo")],
            ),
        ];
        assert!(unique_pane_names(&mut list));
        assert_eq!(pane_names(&list[0].panes), ["Ava", "Max"]);
        assert_eq!(pane_names(&list[1].panes), ["Mia", "Sam", "Leo"]);
        // Ids, agents and sessions are untouched: only the name moves.
        assert_eq!(list[1].panes[0].id, "c");
        assert_eq!(list[1].panes[0].agent, AgentId::Claude);
    }

    #[test]
    fn changes_nothing_when_every_name_is_already_unique() {
        let mut list = vec![
            workspace("one", vec![pane("a", "Ava")]),
            workspace("two", vec![pane("b", "Max")]),
        ];
        let before = list.clone();
        assert!(!unique_pane_names(&mut list));
        assert_eq!(list, before);
    }

    #[test]
    fn names_a_claude_code_session_after_its_pane_and_workspace_safe_to_quote() {
        assert_eq!(session_name("Ava", "Hangar.AI"), "Ava (Hangar.AI)");
        assert_eq!(session_name("Ava", r#"My "big" $app"#), "Ava (My big app)");
        assert_eq!(session_name("Ava", "  "), "Ava");
    }

    #[test]
    fn goes_on_to_numbered_rounds_once_the_list_runs_out() {
        assert_eq!(pick_pane_name(PANE_NAMES), "Ava 2");
        let mut taken: Vec<String> = PANE_NAMES.iter().map(|s| s.to_string()).collect();
        taken.push("Ava 2".into());
        assert_eq!(pick_pane_name(&taken), "Max 2");
    }

    #[test]
    fn counts_every_workspace_s_names_but_the_empty_ones() {
        let list = [
            workspace("one", vec![pane("a", "Ava"), pane("b", "")]),
            workspace("two", vec![pane("c", "Leo")]),
        ];
        assert_eq!(all_pane_names(&list), ["Ava", "Leo"]);
    }
}
