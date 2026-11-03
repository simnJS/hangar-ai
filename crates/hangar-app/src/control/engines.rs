//! P19's engine on the bus: the port's orchestration hooks over its
//! `OrchestrationHandle`, which `Model.live.orchestration` holds once the
//! runtime has started. `control::update` installs it as soon as it is
//! there (`State::sync_engines`).

use serde_json::Value;

use crate::orchestration::OrchestrationHandle;
use crate::orchestration::projects::{render_overview, render_project};
use crate::state::{Workspace, WsId};

use super::port::{BoxFuture, OrchestrationHooks, Port, ProjectView, Projects, json};
use super::types::{Caller, OrchestratorEvent, now_ms};

impl OrchestrationHooks for OrchestrationHandle {
    fn worker_control(
        &self,
        _port: Port,
        op: String,
        ws: Workspace,
        args: Value,
        caller: Caller,
    ) -> BoxFuture<Result<Value, String>> {
        let handle = self.clone();
        Box::pin(async move { handle.worker_control(&op, &ws, &args, &caller).await })
    }

    fn briefing(&self, _port: Port, args: Value) -> BoxFuture<Result<Value, String>> {
        let handle = self.clone();
        Box::pin(async move { handle.briefing(&args).await })
    }

    /// The engine tells a worker's own start or stop apart itself; what the
    /// bus posts (deliveries) is never quiet.
    fn post(&self, ws: &WsId, event: OrchestratorEvent, _quiet: bool) {
        OrchestrationHandle::post(self, ws, event);
    }

    fn projects(&self, _port: Port) -> BoxFuture<Result<Projects, String>> {
        let handle = self.clone();
        Box::pin(async move {
            let summaries = handle.load_projects().await;
            let projects = summaries
                .iter()
                .map(|summary| ProjectView {
                    id: summary.id.clone(),
                    summary: json(summary),
                    text: render_project(summary),
                })
                .collect();
            Ok(Projects {
                projects,
                overview: render_overview(&summaries, now_ms()),
            })
        })
    }
}
