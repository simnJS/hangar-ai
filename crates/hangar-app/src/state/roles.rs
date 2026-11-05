//! Orchestration roles (`src/lib/orchestration.ts`). The pin rule of
//! `lib/orchestrationActions.ts` (`pinRefusal`, `canCreateOrchestrator`) is
//! P10's, in `orchestration_ui::actions`, which the command table asks.
//!
//! A workspace's orchestration is one Claude Code pane pinned as its
//! orchestrator, which plans on the board and hands each task to a worker pane
//! of its own. How many workers run at once, and how many times a task is
//! tried, are the orchestrator's call: Hangar sets no limit on either. P10
//! (`orchestration_ui`) and P19 (`orchestration`) build on these rather than
//! writing their own.

use super::types::{AppState, OrchestrationSettings, Pane, PaneRole, Workspace};

/// The board label of a task only the person can move forward.
pub const NEEDS_YOU_LABEL: &str = "needs-you";
/// The board label of a delivery that passed its check and review, waiting
/// to land.
pub const READY_TO_LAND_LABEL: &str = "ready-to-land";

/// The permission modes offered for an orchestration's agents; none means
/// the person's own (`ORCHESTRATION_PERMISSION_MODES`).
pub const ORCHESTRATION_PERMISSION_MODES: [&str; 3] = ["auto", "acceptEdits", "bypassPermissions"];

/// What an orchestration pane may use without asking, whatever its mode: the
/// tools of Hangar's own server, each of which checks who calls it
/// (`ORCHESTRATION_ALLOWED_TOOLS`).
pub const ORCHESTRATION_ALLOWED_TOOLS: [&str; 1] = ["mcp__hangar"];

/// `ORCHESTRATION_DEFAULTS`: the `Default` of `OrchestrationSettings`.
pub fn orchestration_defaults() -> OrchestrationSettings {
    OrchestrationSettings::default()
}

/// A workspace's orchestration settings, each one absent or malformed read as
/// its default (`orchestrationOf`): a blank model name, a permission mode not
/// offered. Names and optional texts come back trimmed.
pub fn orchestration_of(ws: &Workspace) -> OrchestrationSettings {
    let defaults = OrchestrationSettings::default();
    let Some(saved) = &ws.orchestration else {
        return defaults;
    };
    let name = |value: &str, fallback: &str| {
        let value = value.trim();
        if value.is_empty() {
            fallback.to_string()
        } else {
            value.to_string()
        }
    };
    let optional = |value: &Option<String>| {
        value
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    OrchestrationSettings {
        auto_merge: saved.auto_merge,
        shared_build_cache: saved.shared_build_cache,
        check_command: optional(&saved.check_command),
        orchestrator_model: name(&saved.orchestrator_model, &defaults.orchestrator_model),
        worker_model: name(&saved.worker_model, &defaults.worker_model),
        reviewer_model: name(&saved.reviewer_model, &defaults.reviewer_model),
        branch: optional(&saved.branch),
        permission_mode: saved
            .permission_mode
            .clone()
            .filter(|mode| ORCHESTRATION_PERMISSION_MODES.contains(&mode.as_str())),
    }
}

/// The workspace's orchestrator pane, if it has one (`orchestratorOf`).
pub fn orchestrator_of(ws: &Workspace) -> Option<&Pane> {
    ws.panes
        .iter()
        .find(|pane| pane.role == Some(PaneRole::Orchestrator))
}

/// The workspace's worker panes (`workersOf`).
pub fn workers_of(ws: &Workspace) -> Vec<&Pane> {
    ws.panes
        .iter()
        .filter(|pane| pane.role == Some(PaneRole::Worker))
        .collect()
}

/// The manager pane, wherever it is (`managerOf`): one for the whole of
/// Hangar, following every workspace from the HQ, neither an orchestrator nor
/// a worker of the workspace it sits in.
pub fn manager_of(state: &AppState) -> Option<(&Workspace, &Pane)> {
    state.workspaces.iter().find_map(|ws| {
        ws.panes
            .iter()
            .find(|pane| pane.role == Some(PaneRole::Manager))
            .map(|pane| (ws, pane))
    })
}

/// The HQ workspace, the manager's, once Hangar has created it (`hqOf`).
pub fn hq_of(state: &AppState) -> Option<&Workspace> {
    state.workspaces.iter().find(|ws| ws.hq == Some(true))
}

#[cfg(test)]
mod tests {
    //! The cases of projects.test.ts for the manager role, and of
    //! orchestrationForm.test.ts for what `orchestrationOf` reads back.

    use super::*;
    use crate::state::types::AgentId;

    fn pane(id: &str, agent: AgentId, role: Option<PaneRole>) -> Pane {
        Pane {
            id: id.into(),
            name: id.into(),
            agent,
            role,
            ..Pane::default()
        }
    }
    fn claude(id: &str) -> Pane {
        pane(id, AgentId::Claude, None)
    }
    fn ws(panes: Vec<Pane>) -> Workspace {
        Workspace {
            id: "w".into(),
            panes,
            ..Workspace::default()
        }
    }

    #[test]
    fn the_manager_is_neither_an_orchestrator_nor_a_worker() {
        let home = ws(vec![pane("m", AgentId::Claude, Some(PaneRole::Manager))]);
        assert!(orchestrator_of(&home).is_none());
        assert!(workers_of(&home).is_empty());
        assert_eq!(orchestration_of(&home), orchestration_defaults());
        let mixed = ws(vec![
            pane("m2", AgentId::Claude, Some(PaneRole::Manager)),
            pane("o2", AgentId::Claude, Some(PaneRole::Orchestrator)),
        ]);
        assert_eq!(orchestrator_of(&mixed).map(|p| p.id.as_str()), Some("o2"));
        assert!(workers_of(&mixed).is_empty());
        let state = AppState {
            workspaces: vec![ws(vec![claude("a")]), mixed.clone()],
            ..AppState::default()
        };
        assert_eq!(manager_of(&state).map(|(_, p)| p.id.as_str()), Some("m2"));
    }

    #[test]
    fn a_file_without_the_build_cache_field_reads_true() {
        let saved: OrchestrationSettings = serde_json::from_str(r#"{"autoMerge": false}"#).unwrap();
        assert!(saved.shared_build_cache);
        assert!(!saved.auto_merge);
        let mut w = ws(vec![]);
        w.orchestration = Some(saved);
        assert!(orchestration_of(&w).shared_build_cache);

        w.orchestration = Some(OrchestrationSettings {
            shared_build_cache: false,
            ..OrchestrationSettings::default()
        });
        assert!(!orchestration_of(&w).shared_build_cache);
        let json = serde_json::to_value(orchestration_of(&w)).unwrap();
        assert_eq!(json["sharedBuildCache"], false);
    }

    #[test]
    fn reads_back_what_was_stored_and_defaults_what_is_off() {
        let stored = OrchestrationSettings {
            auto_merge: false,
            check_command: Some("make check".into()),
            branch: Some("develop".into()),
            ..OrchestrationSettings::default()
        };
        let mut w = ws(vec![]);
        w.orchestration = Some(stored.clone());
        assert_eq!(orchestration_of(&w), stored);

        w.orchestration = Some(OrchestrationSettings {
            worker_model: "  ".into(),
            reviewer_model: " haiku ".into(),
            check_command: Some("   ".into()),
            permission_mode: Some("yolo".into()),
            ..OrchestrationSettings::default()
        });
        let read = orchestration_of(&w);
        assert_eq!(read.worker_model, "sonnet");
        assert_eq!(read.reviewer_model, "haiku");
        assert_eq!(read.check_command, None);
        assert_eq!(read.permission_mode, None);
    }
}
