//! The folder half of the new-workspace form: the names it derives, the
//! agents assigned to panes, and the draft it hands the store
//! (`WorkspaceDialog.tsx`).

use crate::state::types::{AGENTS, AgentId, Workspace};
use crate::state::WorkspaceDraft;

use super::hooks::HooksDraft;

/// The pane counts offered (`LayoutSize`).
pub const LAYOUTS: [u8; 4] = [1, 2, 4, 8];
pub const DEFAULT_LAYOUT: u8 = 4;

/// Grid shape per layout, mirrored from the window's grid so the preview
/// matches reality.
pub fn preview_columns(layout: u8) -> usize {
    match layout {
        1 => 1,
        2 | 4 => 2,
        _ => 4,
    }
}

/// One folder of a `.code-workspace` (`WorkspaceRoot`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root {
    pub name: String,
    pub path: String,
    /// A workspace file outlives the folders it points at; a missing one is
    /// shown, and cannot be the main one.
    pub exists: bool,
}

/// The last part of a path (the path itself when it has none).
pub fn basename(path: &str) -> String {
    let trimmed = path.trim_end_matches(['\\', '/']);
    match trimmed.rsplit(['\\', '/']).next() {
        Some(last) if !last.is_empty() => last.to_string(),
        _ => path.to_string(),
    }
}

/// `My project.code-workspace` names a workspace `My project`.
pub fn workspace_file_name(path: &str) -> String {
    let name = basename(path);
    let suffix = ".code-workspace";
    match name.len().checked_sub(suffix.len()) {
        Some(at) if name.is_char_boundary(at) && name[at..].eq_ignore_ascii_case(suffix) => {
            name[..at].to_string()
        }
        _ => name,
    }
}

/// The first existing root is the one the terminals open in, when the file
/// has no better idea: terminals need a folder that is really there, so a
/// file whose first entry is missing on this machine falls through to the
/// first one that is not.
pub fn main_root(roots: &[Root]) -> Option<&Root> {
    roots.iter().find(|root| root.exists)
}

// ── the agents assigned to the panes ──────────────────────────────────

/// One entry per pane: the source of truth the counters and the preview
/// share.
pub fn initial_assignments(layout: u8) -> Vec<AgentId> {
    vec![AgentId::Shell; usize::from(layout)]
}

/// The assignments for another number of panes: cut, or padded with shells.
pub fn resize(assignments: &mut Vec<AgentId>, layout: u8) {
    assignments.resize(usize::from(layout), AgentId::Shell);
}

/// Gives the first free pane (a plain shell) to `agent`.
pub fn add_agent(assignments: &mut [AgentId], agent: AgentId) {
    if let Some(slot) = assignments.iter_mut().find(|slot| **slot == AgentId::Shell) {
        *slot = agent;
    }
}

/// Takes the last pane of `agent` back to a shell.
pub fn remove_agent(assignments: &mut [AgentId], agent: AgentId) {
    if let Some(slot) = assignments.iter_mut().rev().find(|slot| **slot == agent) {
        *slot = AgentId::Shell;
    }
}

/// Clicking a preview cell cycles that single pane through the agents.
pub fn cycle_cell(assignments: &mut [AgentId], index: usize) {
    let Some(slot) = assignments.get_mut(index) else {
        return;
    };
    let at = AGENTS.iter().position(|agent| agent.id == *slot).unwrap_or(0);
    *slot = AGENTS[(at + 1) % AGENTS.len()].id;
}

pub fn count_of(assignments: &[AgentId], agent: AgentId) -> usize {
    assignments.iter().filter(|assigned| **assigned == agent).count()
}

// ── pane names ────────────────────────────────────────────────────────

/// Short, unambiguous, easy to say out loud (`PANE_NAMES` of
/// `lib/paneNames.ts`). The first sixteen are the ones panes were named with
/// before names became unique across workspaces.
///
/// TODO(P0b): `state::pane_names` is to hold this list with `pickPaneNames`;
/// until it does the preview keeps a copy so it can name the cells as the
/// store will. Replace `pick_pane_names` below by that module's.
const PANE_NAMES: [&str; 48] = [
    "Ava", "Max", "Leo", "Mia", "Sam", "Zoe", "Eli", "Nora", "Finn", "Ivy", "Theo", "Luna",
    "Jude", "Cleo", "Rex", "Wren", "Ada", "Ben", "Cora", "Dex", "Emma", "Gus", "Hugo", "Iris",
    "Jack", "Kai", "Lola", "Milo", "Otto", "Quinn", "Rosa", "Seth", "Tara", "Vera", "Will",
    "Yara", "Zack", "Bram", "Ezra", "Faye", "Hana", "Juno", "Kit", "Lars", "Omar", "Ruby",
    "Tess", "Vic",
];

/// First free name; falls back to « Ava 2 » once the list runs out.
fn pick_pane_name(taken: &std::collections::HashSet<String>) -> String {
    if let Some(free) = PANE_NAMES.iter().find(|name| !taken.contains(**name)) {
        return (*free).to_string();
    }
    (2_u32..)
        .flat_map(|round| PANE_NAMES.iter().map(move |name| format!("{name} {round}")))
        .find(|candidate| !taken.contains(candidate))
        .unwrap_or_default()
}

/// The next `count` free names, in the order panes would be given them.
pub fn pick_pane_names(count: usize, taken: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut used: std::collections::HashSet<String> = taken.into_iter().collect();
    (0..count)
        .map(|_| {
            let name = pick_pane_name(&used);
            used.insert(name.clone());
            name
        })
        .collect()
}

/// Every name in use, in every workspace.
pub fn all_pane_names(workspaces: &[Workspace]) -> Vec<String> {
    workspaces
        .iter()
        .flat_map(|ws| ws.panes.iter())
        .map(|pane| pane.name.clone())
        .filter(|name| !name.is_empty())
        .collect()
}

// ── the draft ─────────────────────────────────────────────────────────

pub struct FolderForm<'a> {
    pub name: &'a str,
    pub cwd: &'a str,
    pub roots: &'a [Root],
    pub layout: u8,
    pub assignments: &'a [AgentId],
    pub shell_id: Option<&'a str>,
    pub kits: &'a [crate::state::types::WorkspaceKit],
    pub claude_account: Option<&'a str>,
}

/// What `addWorkspace` is given, or `None` without a folder.
pub fn draft_of(form: &FolderForm) -> Option<WorkspaceDraft> {
    if form.cwd.is_empty() {
        return None;
    }
    Some(WorkspaceDraft {
        name: match form.name.trim() {
            "" => basename(form.cwd),
            name => name.to_string(),
        },
        cwd: form.cwd.to_string(),
        // `cwd` is the root the terminals get; the others only ride along for
        // the agents that can read outside it. Missing folders are never
        // passed on.
        extra_roots: form
            .roots
            .iter()
            .filter(|root| root.exists && root.path != form.cwd)
            .map(|root| root.path.clone())
            .collect(),
        layout: form.layout,
        agents: form.assignments.to_vec(),
        shell_id: form.shell_id.map(str::to_string),
        folder_id: None,
        // Written only when set: absent rather than empty.
        kits: (!form.kits.is_empty()).then(|| form.kits.to_vec()),
        claude_account: form.claude_account.map(str::to_string),
    })
}

/// The hooks of the new workspace, when it has any.
pub fn hooks_of(draft: &HooksDraft) -> Option<crate::state::types::WorktreeHooks> {
    draft.clean()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str, path: &str, exists: bool) -> Root {
        Root {
            name: name.into(),
            path: path.into(),
            exists,
        }
    }

    #[test]
    fn a_path_is_named_by_its_last_part() {
        assert_eq!(basename("/home/me/shop"), "shop");
        assert_eq!(basename("/home/me/shop/"), "shop");
        assert_eq!(basename("C:\\dev\\shop\\"), "shop");
        assert_eq!(basename("/"), "/");
        assert_eq!(basename("shop"), "shop");
    }

    #[test]
    fn a_workspace_file_names_its_workspace_without_the_extension() {
        assert_eq!(workspace_file_name("/x/My project.code-workspace"), "My project");
        assert_eq!(workspace_file_name("C:\\x\\App.CODE-WORKSPACE"), "App");
        assert_eq!(workspace_file_name("/x/notes.txt"), "notes.txt");
    }

    #[test]
    fn the_main_root_is_the_first_one_that_exists() {
        let roots = [
            root("gone", "/gone", false),
            root("api", "/api", true),
            root("web", "/web", true),
        ];
        assert_eq!(main_root(&roots).map(|root| root.path.as_str()), Some("/api"));
        assert_eq!(main_root(&[root("gone", "/gone", false)]), None);
        assert_eq!(main_root(&[]), None);
    }

    #[test]
    fn the_preview_has_the_columns_of_the_window_grid() {
        let columns: Vec<_> = LAYOUTS.iter().map(|layout| preview_columns(*layout)).collect();
        assert_eq!(columns, [1, 2, 2, 4]);
    }

    #[test]
    fn a_new_layout_cuts_or_pads_with_shells() {
        let mut assignments = initial_assignments(4);
        assert_eq!(assignments, [AgentId::Shell; 4]);
        add_agent(&mut assignments, AgentId::Claude);
        add_agent(&mut assignments, AgentId::Codex);
        resize(&mut assignments, 2);
        assert_eq!(assignments, [AgentId::Claude, AgentId::Codex]);
        resize(&mut assignments, 8);
        assert_eq!(assignments.len(), 8);
        assert_eq!(count_of(&assignments, AgentId::Shell), 6);
        resize(&mut assignments, 1);
        assert_eq!(assignments, [AgentId::Claude]);
    }

    #[test]
    fn an_agent_takes_the_first_free_pane_and_gives_back_the_last_it_had() {
        let mut assignments = initial_assignments(4);
        add_agent(&mut assignments, AgentId::Claude);
        add_agent(&mut assignments, AgentId::Claude);
        add_agent(&mut assignments, AgentId::Codex);
        assert_eq!(
            assignments,
            [AgentId::Claude, AgentId::Claude, AgentId::Codex, AgentId::Shell]
        );
        remove_agent(&mut assignments, AgentId::Claude);
        assert_eq!(
            assignments,
            [AgentId::Claude, AgentId::Shell, AgentId::Codex, AgentId::Shell]
        );
        // Nothing to take back, nothing to take it: no change.
        remove_agent(&mut assignments, AgentId::Gemini);
        add_agent(&mut assignments, AgentId::Gemini);
        add_agent(&mut assignments, AgentId::Gemini);
        add_agent(&mut assignments, AgentId::Opencode);
        assert_eq!(
            assignments,
            [AgentId::Claude, AgentId::Gemini, AgentId::Codex, AgentId::Gemini]
        );
    }

    #[test]
    fn a_cell_cycles_through_the_agents_in_the_order_of_the_list() {
        let mut assignments = initial_assignments(2);
        let mut seen = Vec::new();
        for _ in 0..AGENTS.len() {
            cycle_cell(&mut assignments, 0);
            seen.push(assignments[0]);
        }
        assert_eq!(
            seen,
            [
                AgentId::Claude,
                AgentId::Codex,
                AgentId::Gemini,
                AgentId::Opencode,
                AgentId::Shell
            ]
        );
        assert_eq!(assignments[1], AgentId::Shell);
        cycle_cell(&mut assignments, 9);
    }

    #[test]
    fn pane_names_are_the_next_free_ones_in_order() {
        assert_eq!(
            pick_pane_names(3, []),
            ["Ava", "Max", "Leo"]
        );
        assert_eq!(
            pick_pane_names(2, ["Ava".to_string(), "Leo".to_string()]),
            ["Max", "Mia"]
        );
        let all: Vec<String> = PANE_NAMES.iter().map(|name| name.to_string()).collect();
        assert_eq!(pick_pane_names(2, all), ["Ava 2", "Max 2"]);
    }

    #[test]
    fn every_name_of_every_workspace_is_taken() {
        let workspace = |names: &[&str]| Workspace {
            panes: names
                .iter()
                .map(|name| crate::state::Pane {
                    name: (*name).into(),
                    ..crate::state::Pane::default()
                })
                .collect(),
            ..Workspace::default()
        };
        let all = [workspace(&["Ava", "", "Max"]), workspace(&["Leo"])];
        assert_eq!(all_pane_names(&all), ["Ava", "Max", "Leo"]);
    }

    fn form<'a>(name: &'a str, cwd: &'a str, roots: &'a [Root]) -> FolderForm<'a> {
        FolderForm {
            name,
            cwd,
            roots,
            layout: 4,
            assignments: &[],
            shell_id: None,
            kits: &[],
            claude_account: None,
        }
    }

    #[test]
    fn without_a_folder_there_is_nothing_to_create() {
        assert!(draft_of(&form("x", "", &[])).is_none());
    }

    #[test]
    fn the_name_defaults_to_the_folders_and_blanks_are_not_a_name() {
        let draft = draft_of(&form("  ", "/home/me/shop", &[])).expect("a draft");
        assert_eq!(draft.name, "shop");
        let draft = draft_of(&form(" The shop ", "/home/me/shop", &[])).expect("a draft");
        assert_eq!(draft.name, "The shop");
    }

    #[test]
    fn only_the_other_existing_roots_ride_along() {
        let roots = [
            root("api", "/api", true),
            root("web", "/web", true),
            root("gone", "/gone", false),
        ];
        let draft = draft_of(&form("", "/api", &roots)).expect("a draft");
        assert_eq!(draft.extra_roots, ["/web"]);
        assert_eq!(draft.cwd, "/api");
    }

    #[test]
    fn kits_and_account_are_written_only_when_set() {
        let mut fields = form("", "/x", &[]);
        let draft = draft_of(&fields).expect("a draft");
        assert!(draft.kits.is_none());
        assert!(draft.claude_account.is_none());
        let kits = [crate::state::types::WorkspaceKit {
            id: "kit-editor".into(),
            granted: vec![],
        }];
        fields.kits = &kits;
        fields.claude_account = Some("perso");
        fields.shell_id = Some("bash");
        let draft = draft_of(&fields).expect("a draft");
        assert_eq!(draft.kits.as_ref().map(Vec::len), Some(1));
        assert_eq!(draft.claude_account.as_deref(), Some("perso"));
        assert_eq!(draft.shell_id.as_deref(), Some("bash"));
    }
}
