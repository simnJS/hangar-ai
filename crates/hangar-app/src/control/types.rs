//! The values the control bus and the engines share (inventory A6, "Shared
//! enums"): who calls, what the window says on screen, what a delivery or a
//! worker tells the orchestration engine.
//!
//! They serialize as the TS objects did, kind-tagged and camelCase, because
//! some of them are written where a model or the phone reads them (the
//! journal feeds the briefing and the activity log).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::state::PaneId;

/// Who is asking, from the environment of the pane the agent runs in
/// (`Caller` of control.ts). Core's MCP server sets `args.caller` from
/// `HANGAR_PANE_ID`/`HANGAR_PANE_NAME`; `args.callerCwd` is the fallback for
/// an agent Hangar did not start.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Caller {
    pub pane_id: Option<PaneId>,
    pub pane_name: Option<String>,
    pub cwd: Option<String>,
}

impl Caller {
    /// The caller of a request, read from its arguments (`readCaller`):
    /// blank strings are no value at all.
    pub fn of(args: &Value) -> Caller {
        let raw = args.get("caller");
        let field = |name: &str| raw.and_then(|raw| text(raw.get(name)));
        Caller {
            pane_id: field("paneId"),
            pane_name: field("paneName"),
            cwd: field("cwd").or_else(|| text(args.get("callerCwd"))),
        }
    }

    /// Nobody: the person, acting from the phone or the window.
    pub fn person() -> Caller {
        Caller::default()
    }
}

/// What an agent did that the window says on screen (`ControlEvent`), as the
/// notice `control.<kind>` with these fields as its placeholders. `by` is
/// the agent's pane name; `None` reads as `control.someone`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ControlEvent {
    Created {
        by: Option<String>,
        pane: String,
        workspace: String,
    },
    Closed {
        by: Option<String>,
        pane: String,
        workspace: String,
    },
    Restarted {
        by: Option<String>,
        pane: String,
        workspace: String,
    },
    Reset {
        by: Option<String>,
        pane: String,
        workspace: String,
    },
    WorkspaceCreated {
        by: Option<String>,
        pane: String,
        workspace: String,
    },
    CheckSet {
        by: Option<String>,
        workspace: String,
        command: String,
    },
    /// In the workspace on screen.
    ModelShown {
        by: Option<String>,
        file: String,
        workspace: String,
    },
    ModelClosed {
        by: Option<String>,
        file: String,
        workspace: String,
    },
    /// In a workspace that is not the one on screen, which the line names.
    ModelShownElsewhere {
        by: Option<String>,
        file: String,
        workspace: String,
    },
    ModelClosedElsewhere {
        by: Option<String>,
        file: String,
        workspace: String,
    },
}

impl ControlEvent {
    /// The `kind` the TS logged and keyed its i18n string by.
    pub fn kind(&self) -> &'static str {
        match self {
            ControlEvent::Created { .. } => "created",
            ControlEvent::Closed { .. } => "closed",
            ControlEvent::Restarted { .. } => "restarted",
            ControlEvent::Reset { .. } => "reset",
            ControlEvent::WorkspaceCreated { .. } => "workspaceCreated",
            ControlEvent::CheckSet { .. } => "checkSet",
            ControlEvent::ModelShown { .. } => "modelShown",
            ControlEvent::ModelClosed { .. } => "modelClosed",
            ControlEvent::ModelShownElsewhere { .. } => "modelShownElsewhere",
            ControlEvent::ModelClosedElsewhere { .. } => "modelClosedElsewhere",
        }
    }

    pub fn by(&self) -> Option<&str> {
        match self {
            ControlEvent::Created { by, .. }
            | ControlEvent::Closed { by, .. }
            | ControlEvent::Restarted { by, .. }
            | ControlEvent::Reset { by, .. }
            | ControlEvent::WorkspaceCreated { by, .. }
            | ControlEvent::CheckSet { by, .. }
            | ControlEvent::ModelShown { by, .. }
            | ControlEvent::ModelClosed { by, .. }
            | ControlEvent::ModelShownElsewhere { by, .. }
            | ControlEvent::ModelClosedElsewhere { by, .. } => by.as_deref(),
        }
    }

    /// What the event is about: the pane, the file, or the command.
    pub fn subject(&self) -> &str {
        match self {
            ControlEvent::Created { pane, .. }
            | ControlEvent::Closed { pane, .. }
            | ControlEvent::Restarted { pane, .. }
            | ControlEvent::Reset { pane, .. }
            | ControlEvent::WorkspaceCreated { pane, .. } => pane,
            ControlEvent::CheckSet { command, .. } => command,
            ControlEvent::ModelShown { file, .. }
            | ControlEvent::ModelClosed { file, .. }
            | ControlEvent::ModelShownElsewhere { file, .. }
            | ControlEvent::ModelClosedElsewhere { file, .. } => file,
        }
    }

    pub fn workspace(&self) -> &str {
        match self {
            ControlEvent::Created { workspace, .. }
            | ControlEvent::Closed { workspace, .. }
            | ControlEvent::Restarted { workspace, .. }
            | ControlEvent::Reset { workspace, .. }
            | ControlEvent::WorkspaceCreated { workspace, .. }
            | ControlEvent::CheckSet { workspace, .. }
            | ControlEvent::ModelShown { workspace, .. }
            | ControlEvent::ModelClosed { workspace, .. }
            | ControlEvent::ModelShownElsewhere { workspace, .. }
            | ControlEvent::ModelClosedElsewhere { workspace, .. } => workspace,
        }
    }
}

/// How a delivery ended (`DeliveryOutcomeKind`): core's `delivery::Kind`, and
/// `done` for a worker in a folder that is not a git repository, which has
/// nothing to merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeliveryOutcomeKind {
    Landed,
    Done,
    Ready,
    Conflict,
    CheckFailed,
    ChangesRequested,
    Dirty,
    Stale,
    Error,
}

impl DeliveryOutcomeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DeliveryOutcomeKind::Landed => "landed",
            DeliveryOutcomeKind::Done => "done",
            DeliveryOutcomeKind::Ready => "ready",
            DeliveryOutcomeKind::Conflict => "conflict",
            DeliveryOutcomeKind::CheckFailed => "checkFailed",
            DeliveryOutcomeKind::ChangesRequested => "changesRequested",
            DeliveryOutcomeKind::Dirty => "dirty",
            DeliveryOutcomeKind::Stale => "stale",
            DeliveryOutcomeKind::Error => "error",
        }
    }
}

impl From<hangar_core::delivery::Kind> for DeliveryOutcomeKind {
    fn from(kind: hangar_core::delivery::Kind) -> Self {
        use hangar_core::delivery::Kind;
        match kind {
            Kind::Landed => DeliveryOutcomeKind::Landed,
            Kind::Ready => DeliveryOutcomeKind::Ready,
            Kind::Conflict => DeliveryOutcomeKind::Conflict,
            Kind::CheckFailed => DeliveryOutcomeKind::CheckFailed,
            Kind::ChangesRequested => DeliveryOutcomeKind::ChangesRequested,
            Kind::Dirty => DeliveryOutcomeKind::Dirty,
            Kind::Stale => DeliveryOutcomeKind::Stale,
            Kind::Error => DeliveryOutcomeKind::Error,
        }
    }
}

/// What happened in a workspace that its orchestrator hears about
/// (`OrchestratorEvent` of orchestratorEvents.ts). The worker and delivery
/// operations post them (`AppPort::post_event`); P19's event engine makes the
/// rest from the boards and the panes, journals them and tells the
/// orchestrator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum OrchestratorEvent {
    #[serde(rename_all = "camelCase")]
    WorkerStarted {
        worker: String,
        task_id: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    WorkerStopped {
        worker: String,
        task_id: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    WorkerExited {
        worker: String,
        task_id: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    WorkerWaiting {
        worker: String,
        task_id: Option<String>,
        for_ms: u64,
    },
    #[serde(rename_all = "camelCase")]
    WorkerIdle {
        worker: String,
        task_id: Option<String>,
        for_ms: u64,
    },
    #[serde(rename_all = "camelCase")]
    TaskMoved {
        task_id: String,
        title: String,
        from: String,
        to: String,
        by: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    TaskCommented {
        task_id: String,
        title: String,
        by: String,
        text: String,
    },
    #[serde(rename_all = "camelCase")]
    TaskNeedsYou {
        task_id: String,
        title: String,
        reason: String,
    },
    #[serde(rename_all = "camelCase")]
    TaskBlocked {
        task_id: String,
        title: String,
        by: String,
        reason: String,
        human_only: bool,
    },
    #[serde(rename_all = "camelCase")]
    TaskUnblocked {
        task_id: String,
        title: String,
        how: String,
    },
    #[serde(rename_all = "camelCase")]
    TaskRecheckDue {
        task_id: String,
        title: String,
        reason: String,
    },
    #[serde(rename_all = "camelCase")]
    Delivery {
        task_id: String,
        title: String,
        worker: String,
        outcome: DeliveryOutcomeKind,
        detail: String,
    },
}

/// Where a task's delivery stands (`DeliveryState` of deliveryControl.ts),
/// for whoever lists the workers (P19's `worker_list`) or shows the delivery
/// (P20's `remote_delivery`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryState {
    /// `None` while it runs, then its outcome.
    pub outcome: Option<DeliveryOutcomeKind>,
    /// Epoch milliseconds: when it started, or when it ended.
    pub since: u64,
    pub detail: Option<String>,
}

impl DeliveryState {
    /// `running`, or the outcome, as the TS wrote the state.
    pub fn state(&self) -> &'static str {
        self.outcome.map_or("running", DeliveryOutcomeKind::as_str)
    }
}

/// A trimmed string, or nothing for a blank or a value that is not a string
/// (`text` of control.ts): how every argument an agent passes is read.
pub fn text(value: Option<&Value>) -> Option<String> {
    let value = value?.as_str()?.trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// Same name, the way the person reads names: case and edge spaces aside.
pub fn same(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// Same folder, the way Windows compares them: either separator, any case,
/// no trailing separator (`normalize`).
pub fn normalize(path: &str) -> String {
    path.trim_end_matches(['\\', '/'])
        .replace('\\', "/")
        .to_lowercase()
}

/// Epoch milliseconds now, as `Date.now()` gave them.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_caller_comes_from_its_pane_or_from_the_folder_it_runs_in() {
        let args = json!({"caller": {"paneId": " p1 ", "paneName": "", "cwd": null},
                          "callerCwd": "C:\\dev"});
        assert_eq!(
            Caller::of(&args),
            Caller {
                pane_id: Some("p1".into()),
                pane_name: None,
                cwd: Some("C:\\dev".into()),
            }
        );
        assert_eq!(Caller::of(&json!({})), Caller::person());
    }

    #[test]
    fn events_serialize_as_the_ts_objects_did() {
        let event = OrchestratorEvent::Delivery {
            task_id: "t1".into(),
            title: "Fix".into(),
            worker: "Leo".into(),
            outcome: DeliveryOutcomeKind::CheckFailed,
            detail: "x".into(),
        };
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({"kind": "delivery", "taskId": "t1", "title": "Fix", "worker": "Leo",
                   "outcome": "checkFailed", "detail": "x"})
        );
        let shown = ControlEvent::ModelShownElsewhere {
            by: None,
            file: "a.glb".into(),
            workspace: "Shop".into(),
        };
        assert_eq!(shown.kind(), "modelShownElsewhere");
        assert_eq!(
            serde_json::to_value(&shown).unwrap(),
            json!({"kind": "modelShownElsewhere", "by": null, "file": "a.glb", "workspace": "Shop"})
        );
    }

    #[test]
    fn folders_compare_whatever_their_spelling() {
        assert_eq!(normalize("C:\\Dev\\Shop\\"), "c:/dev/shop");
        assert!(same(" Ava", "ava "));
    }
}
