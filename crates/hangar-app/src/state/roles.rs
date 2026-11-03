//! Orchestration roles (`src/lib/orchestration.ts`), and the pin rule of
//! `lib/orchestrationActions.ts` (`pinRefusal`, `canCreateOrchestrator`),
//! which the command table needs to know what is live.
//!
//! A workspace's orchestration is one Claude Code pane pinned as its
//! orchestrator, which plans on the board and hands each task to a worker pane
//! of its own. The limits here are Hangar's to hold, not the orchestrator's: a
//! prompt can ask a model to start no more than three workers, only code can
//! refuse the fourth. P10 (`orchestration_ui`) and P19 (`orchestration`) build
//! on these rather than writing their own.

use super::types::{AgentId, AppState, OrchestrationSettings, Pane, PaneRole, Workspace};

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
/// its default (`orchestrationOf`): a count below one, a blank model name, a
/// permission mode not offered. Names and optional texts come back trimmed.
pub fn orchestration_of(ws: &Workspace) -> OrchestrationSettings {
    let defaults = OrchestrationSettings::default();
    let Some(saved) = &ws.orchestration else {
        return defaults;
    };
    let count = |value: u32, fallback: u32| if value >= 1 { value } else { fallback };
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

/// Why a pane cannot be pinned as its workspace's orchestrator (`PinRefusal`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PinRefusal {
    /// No such pane in the workspace.
    Missing,
    /// It runs something else than Claude Code: the briefing is its mod's.
    NotClaude,
    /// It is a worker already, on a task of its own.
    Worker,
    /// It is the orchestrator already.
    Pinned,
    /// Another pane of the workspace is.
    Taken,
    /// It is the project manager, whose role is its own.
    Manager,
    /// The workspace is the HQ, where no project is planned.
    Hq,
}

/// Why `pane` cannot be pinned, or `None` when it can (`pinRefusal`).
pub fn pin_refusal(ws: &Workspace, pane: &str) -> Option<PinRefusal> {
    let Some(pane) = ws.panes.iter().find(|entry| entry.id == pane) else {
        return Some(PinRefusal::Missing);
    };
    if pane.role == Some(PaneRole::Manager) {
        return Some(PinRefusal::Manager);
    }
    if ws.hq == Some(true) {
        return Some(PinRefusal::Hq);
    }
    match pane.role {
        Some(PaneRole::Orchestrator) => return Some(PinRefusal::Pinned),
        Some(PaneRole::Worker) => return Some(PinRefusal::Worker),
        _ => {}
    }
    if pane.agent != AgentId::Claude {
        return Some(PinRefusal::NotClaude);
    }
    if orchestrator_of(ws).is_some() {
        return Some(PinRefusal::Taken);
    }
    None
}

/// Whether a new orchestrator pane may be made in the workspace
/// (`canCreateOrchestrator`): only where there is none, never in the HQ.
pub fn can_create_orchestrator(ws: &Workspace) -> bool {
    ws.hq != Some(true) && orchestrator_of(ws).is_none()
}

#[cfg(test)]
mod tests {
    //! The cases of orchestrationActions.test.ts for the pin rule, of
    //! projects.test.ts for the manager role, and of orchestrationForm.test.ts
    //! for what `orchestrationOf` reads back.

    use super::*;

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
    fn pins_a_claude_code_pane_in_a_workspace_that_has_no_orchestrator() {
        let w = ws(vec![claude("a"), pane("b", AgentId::Codex, None)]);
        assert_eq!(pin_refusal(&w, "a"), None);
    }

    #[test]
    fn keeps_to_one_orchestrator_per_workspace() {
        let w = ws(vec![
            pane("a", AgentId::Claude, Some(PaneRole::Orchestrator)),
            claude("b"),
        ]);
        assert_eq!(pin_refusal(&w, "b"), Some(PinRefusal::Taken));
        assert_eq!(pin_refusal(&w, "a"), Some(PinRefusal::Pinned));
    }

    #[test]
    fn pins_claude_code_only() {
        for agent in [
            AgentId::Shell,
            AgentId::Codex,
            AgentId::Gemini,
            AgentId::Opencode,
        ] {
            let w = ws(vec![pane("x", agent, None)]);
            assert_eq!(
                pin_refusal(&w, "x"),
                Some(PinRefusal::NotClaude),
                "{agent:?}"
            );
        }
    }

    #[test]
    fn never_turns_a_worker_into_the_orchestrator() {
        let w = ws(vec![pane("w", AgentId::Claude, Some(PaneRole::Worker))]);
        assert_eq!(pin_refusal(&w, "w"), Some(PinRefusal::Worker));
    }

    #[test]
    fn says_so_when_the_pane_is_gone() {
        assert_eq!(
            pin_refusal(&ws(vec![claude("a")]), "zz"),
            Some(PinRefusal::Missing)
        );
    }

    #[test]
    fn creates_one_only_where_there_is_none_workers_or_not() {
        assert!(can_create_orchestrator(&ws(vec![
            claude("a"),
            pane("w", AgentId::Claude, Some(PaneRole::Worker)),
        ])));
        assert!(!can_create_orchestrator(&ws(vec![pane(
            "a",
            AgentId::Claude,
            Some(PaneRole::Orchestrator)
        )])));
    }

    #[test]
    fn refuses_an_orchestrator_in_the_hq_and_a_manager_pane_as_one() {
        let mut hq = ws(vec![claude("a")]);
        hq.hq = Some(true);
        assert_eq!(pin_refusal(&hq, "a"), Some(PinRefusal::Hq));
        let mut empty_hq = ws(vec![]);
        empty_hq.hq = Some(true);
        assert!(!can_create_orchestrator(&empty_hq));
        let manager = ws(vec![pane("m", AgentId::Claude, Some(PaneRole::Manager))]);
        assert_eq!(pin_refusal(&manager, "m"), Some(PinRefusal::Manager));
        let mut manager_in_hq = manager.clone();
        manager_in_hq.hq = Some(true);
        assert_eq!(pin_refusal(&manager_in_hq, "m"), Some(PinRefusal::Manager));
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
    fn reads_back_what_was_stored_and_defaults_what_is_off() {
        let stored = OrchestrationSettings {
            max_workers: 4,
            auto_merge: false,
            check_command: Some("make check".into()),
            branch: Some("develop".into()),
            ..OrchestrationSettings::default()
        };
        let mut w = ws(vec![]);
        w.orchestration = Some(stored.clone());
        assert_eq!(orchestration_of(&w), stored);

        w.orchestration = Some(OrchestrationSettings {
            max_workers: 0,
            worker_model: "  ".into(),
            reviewer_model: " haiku ".into(),
            check_command: Some("   ".into()),
            permission_mode: Some("yolo".into()),
            ..OrchestrationSettings::default()
        });
        let read = orchestration_of(&w);
        assert_eq!(read.max_workers, 3);
        assert_eq!(read.worker_model, "sonnet");
        assert_eq!(read.reviewer_model, "haiku");
        assert_eq!(read.check_command, None);
        assert_eq!(read.permission_mode, None);
    }
}
