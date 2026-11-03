//! The project_* control operations (managerControl.ts): the project manager
//! (the pane with role `manager`, in the HQ workspace) seeing every project
//! and passing the person's instructions on to each project's orchestrator.
//!
//! Only the manager calls them, and it acts only when the person asks. The
//! projects themselves are read by P19 (`orchestration::projects`), through
//! the port's orchestration hooks.

use serde_json::{Value, json};

use crate::state::Workspace;
use crate::state::roles::orchestrator_of;
use crate::state::types::PaneRole;

use super::panes::tell;
use super::port::{ModelCmd, Port, Projects, is_asleep, is_open};
use super::run::unknown;
use super::types::{Caller, normalize, text};

/// How the orchestrator reads what the manager passes on.
pub const RELAY_PREFIX: &str = "Relayed by the project manager from the person:";

/// The manager pane's name, when it is the one calling; refused otherwise.
fn manager_calling(port: &Port, caller: &Caller, op: &str) -> Result<String, String> {
    let snapshot = port.snapshot();
    let me = caller.pane_id.as_deref().and_then(|id| {
        snapshot
            .state
            .workspaces
            .iter()
            .flat_map(|ws| &ws.panes)
            .find(|pane| pane.id == id)
    });
    match me {
        Some(pane) if pane.role == Some(PaneRole::Manager) => Ok(pane.name.clone()),
        _ => Err(format!(
            "{op} is for the project manager, the pane of the HQ workspace, and you are not it. Work on your \
             own workspace with your own tools; if the person wants something from another project, tell them \
             to ask the project manager."
        )),
    }
}

/// The project asked for, by id, name or folder; the HQ is not one.
fn project_for(port: &Port, reference: Option<&Value>) -> Result<Workspace, String> {
    let snapshot = port.snapshot();
    let workspaces = &snapshot.state.workspaces;
    let names = workspaces
        .iter()
        .filter(|ws| ws.hq != Some(true))
        .map(|ws| format!("\"{}\"", ws.name))
        .collect::<Vec<_>>()
        .join(", ");
    let names = if names.is_empty() {
        "none".into()
    } else {
        names
    };
    let Some(wanted) = text(reference) else {
        return Err(format!(
            "Name the project (its name or folder). Projects: {names}."
        ));
    };
    let lower = wanted.to_lowercase();
    let found = workspaces
        .iter()
        .find(|ws| ws.id == wanted)
        .or_else(|| workspaces.iter().find(|ws| ws.name.to_lowercase() == lower))
        .or_else(|| {
            workspaces
                .iter()
                .find(|ws| normalize(&ws.cwd) == normalize(&wanted))
        });
    let Some(found) = found else {
        return Err(format!("No project \"{wanted}\". Projects: {names}."));
    };
    if found.hq == Some(true) {
        return Err(format!(
            "\"{}\" is the HQ, your own workspace, not a project. Projects: {names}.",
            found.name
        ));
    }
    Ok(found.clone())
}

async fn summaries(port: &Port) -> Result<Projects, String> {
    match port.orchestration() {
        Some(engine) => engine.projects(port.clone()).await,
        None => Err("The projects cannot be read in this Hangar window.".into()),
    }
}

pub async fn control(
    op: &str,
    args: &Value,
    caller: &Caller,
    port: &Port,
) -> Result<Value, String> {
    let me = manager_calling(port, caller, op)?;
    match op {
        "project_list" => {
            let projects = summaries(port).await?;
            Ok(json!({
                "projects": projects.projects.iter().map(|project| &project.summary).collect::<Vec<_>>(),
                "text": projects.overview,
            }))
        }
        "project_status" => {
            let ws = project_for(port, args.get("workspace"))?;
            let projects = summaries(port).await?;
            let Some(project) = projects.projects.iter().find(|entry| entry.id == ws.id) else {
                return Err(format!("\"{}\" is gone. Call project_list again.", ws.name));
            };
            Ok(json!({ "project": project.summary, "text": project.text }))
        }
        "project_send" => {
            let ws = project_for(port, args.get("workspace"))?;
            let Some(body) = text(args.get("text")) else {
                return Err("Nothing to send: give the person's instruction in text.".into());
            };
            let Some(orchestrator) = orchestrator_of(&ws) else {
                return Err(format!(
                    "\"{}\" has no orchestrator, so there is nobody to relay to. Only the person can pin one, \
                     from the menu of a Claude Code pane; tell them, and ask whether they want one.",
                    ws.name
                ));
            };
            // An agent acting on a workspace is using it: its sleep waits.
            tell(port, ModelCmd::Touch(ws.id.clone())).await;
            port.enqueue(
                &orchestrator.id,
                &format!("{RELAY_PREFIX} {body}"),
                Some(&me),
            )
            .await?;
            // Held for a Claude Code that is not running, it would wait for
            // good: an asleep or unopened workspace is opened, as pane_send
            // does.
            let snapshot = port.snapshot();
            let woke = is_asleep(&snapshot, &ws.id);
            let starting = !is_open(&snapshot, &ws);
            if starting {
                tell(port, ModelCmd::OpenWorkspace(ws.id.clone())).await;
            }
            let mut answer = json!({
                "queued": true,
                "workspace": ws.name,
                "orchestrator": orchestrator.name,
                "note": format!(
                    "Queued for \"{}\", the orchestrator of \"{}\", until it is idle. Its progress comes back to \
                     you as events; do not poll.",
                    orchestrator.name, ws.name
                ),
            });
            if woke {
                answer["woke"] = json!(true);
            } else if starting {
                answer["starting"] = json!(true);
            }
            Ok(answer)
        }
        _ => Err(unknown(op)),
    }
}
