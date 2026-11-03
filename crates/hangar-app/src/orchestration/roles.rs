//! The parts of `lib/orchestration.ts` and `lib/paneNames.ts` the engine reads
//! and the shell has not ported yet: `orchestrationOf` (each setting read as
//! its default when it makes no sense), `workersOf`, `managerOf`, `hqOf`,
//! `pickPaneName` and `allPaneNames`.
//!
//! They belong in `state::roles` and `state::pane_names`, which P0b fills in
//! with their own tests; until then the engine keeps its own copies here, so
//! that nothing outside this directory changes. When P0b lands, these become
//! re-exports of the shell's.

use crate::state::roles::orchestrator_of;
use crate::state::types::{AppState, OrchestrationSettings, Pane, PaneRole, Workspace};

/// The permission modes an orchestration's agents may be given
/// (`ORCHESTRATION_PERMISSION_MODES`); anything else is the person's own.
pub const PERMISSION_MODES: [&str; 3] = ["auto", "acceptEdits", "bypassPermissions"];

/// A workspace's settings, each one that makes no sense read as its default
/// (`orchestrationOf`). The file was read leniently already (a field of the
/// wrong type is its default by now); what is left is what serde accepts and
/// the TS still refused: a count of zero, a blank model name, a branch of
/// spaces, a mode this version does not offer.
pub fn orchestration_of(ws: &Workspace) -> OrchestrationSettings {
    let defaults = OrchestrationSettings::default();
    let Some(saved) = &ws.orchestration else {
        return defaults;
    };
    let count = |value: u32, fallback: u32| if value >= 1 { value } else { fallback };
    let name = |value: &str, fallback: &str| {
        let value = value.trim();
        if value.is_empty() { fallback } else { value }.to_string()
    };
    let optional = |value: &Option<String>| {
        value
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    OrchestrationSettings {
        max_workers: count(saved.max_workers, defaults.max_workers),
        max_attempts: count(saved.max_attempts, defaults.max_attempts),
        auto_merge: saved.auto_merge,
        check_command: optional(&saved.check_command),
        orchestrator_model: name(&saved.orchestrator_model, &defaults.orchestrator_model),
        worker_model: name(&saved.worker_model, &defaults.worker_model),
        reviewer_model: name(&saved.reviewer_model, &defaults.reviewer_model),
        branch: optional(&saved.branch),
        permission_mode: saved
            .permission_mode
            .clone()
            .filter(|mode| PERMISSION_MODES.contains(&mode.as_str())),
    }
}

/// The workspace's orchestrator pane (`orchestratorOf`).
pub fn orchestrator(ws: &Workspace) -> Option<&Pane> {
    orchestrator_of(ws)
}

/// The workspace's worker panes, in pane order (`workersOf`).
pub fn workers_of(ws: &Workspace) -> Vec<&Pane> {
    ws.panes
        .iter()
        .filter(|pane| pane.role == Some(PaneRole::Worker))
        .collect()
}

/// The manager pane, wherever it is, and its workspace (`managerOf`): one for
/// the whole of Hangar, which follows every workspace from the HQ.
pub fn manager_of(state: &AppState) -> Option<(&Workspace, &Pane)> {
    state.workspaces.iter().find_map(|ws| {
        ws.panes
            .iter()
            .find(|pane| pane.role == Some(PaneRole::Manager))
            .map(|pane| (ws, pane))
    })
}

/// The HQ workspace, the manager's, if Hangar has created it (`hqOf`).
pub fn hq_of(state: &AppState) -> Option<&Workspace> {
    state.workspaces.iter().find(|ws| ws.hq == Some(true))
}

/// Short, unambiguous names, easy to say out loud (`PANE_NAMES`). The first
/// sixteen are the ones panes were named with before names became unique
/// across workspaces, kept first and in order.
pub const PANE_NAMES: [&str; 48] = [
    "Ava", "Max", "Leo", "Mia", "Sam", "Zoe", "Eli", "Nora", "Finn", "Ivy", "Theo", "Luna",
    "Jude", "Cleo", "Rex", "Wren", "Ada", "Ben", "Cora", "Dex", "Emma", "Gus", "Hugo", "Iris",
    "Jack", "Kai", "Lola", "Milo", "Otto", "Quinn", "Rosa", "Seth", "Tara", "Vera", "Will",
    "Yara", "Zack", "Bram", "Ezra", "Faye", "Hana", "Juno", "Kit", "Lars", "Omar", "Ruby",
    "Tess", "Vic",
];

/// The first free name; "Ava 2" and so on once the list runs out
/// (`pickPaneName`).
pub fn pick_pane_name<'a>(taken: impl IntoIterator<Item = &'a str>) -> String {
    let used: std::collections::HashSet<&str> = taken.into_iter().collect();
    if let Some(free) = PANE_NAMES.iter().find(|name| !used.contains(*name)) {
        return (*free).to_string();
    }
    (2..)
        .flat_map(|round| PANE_NAMES.iter().map(move |name| format!("{name} {round}")))
        .find(|candidate| !used.contains(candidate.as_str()))
        .unwrap_or_default()
}

/// Every pane name in use, in every workspace (`allPaneNames`).
pub fn all_pane_names(workspaces: &[Workspace]) -> Vec<&str> {
    workspaces
        .iter()
        .flat_map(|ws| ws.panes.iter())
        .map(|pane| pane.name.as_str())
        .filter(|name| !name.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_setting_that_makes_no_sense_reads_as_its_default() {
        let ws = Workspace {
            orchestration: Some(OrchestrationSettings {
                max_workers: 0,
                worker_model: "  ".into(),
                branch: Some("  develop ".into()),
                check_command: Some("   ".into()),
                permission_mode: Some("yolo".into()),
                ..OrchestrationSettings::default()
            }),
            ..Workspace::default()
        };
        let settings = orchestration_of(&ws);
        assert_eq!(settings.max_workers, 3);
        assert_eq!(settings.worker_model, "sonnet");
        assert_eq!(settings.branch.as_deref(), Some("develop"));
        assert_eq!(settings.check_command, None);
        assert_eq!(settings.permission_mode, None);
        assert_eq!(
            orchestration_of(&Workspace::default()),
            OrchestrationSettings::default()
        );
    }

    #[test]
    fn names_run_through_the_list_then_count_rounds() {
        assert_eq!(pick_pane_name(["Ava", "Max"]), "Leo");
        assert_eq!(pick_pane_name(PANE_NAMES), "Ava 2");
    }
}
