//! The pane operations: `workspace_list`, `pane_list`, `pane_create`,
//! `pane_close`, `pane_restart`, `pane_send`, `pane_read`, `pane_focus`,
//! `workspace_window` (control.ts).

use serde_json::{Value, json};

use crate::state::types::MAX_PANES;
use crate::state::{AgentId, AppState, Pane, Patch, Snapshot, SplitDir, Workspace};

use super::paths::resolve_folder;
use super::port::{ModelCmd, Port, activity_of, is_asleep, is_open, object, workspace_in};
use super::run::{
    ASLEEP, agent_arg, caller_workspace, effective_cwd, has_pane, key_sequence, ordered, pane_in,
};
use super::types::{Caller, ControlEvent, text};

/// What `pane_read` gives without `lines`, and the most it gives.
const READ_DEFAULT: f64 = 60.0;
const READ_MAX: f64 = 1000.0;

/// Between a paste and the Enter that submits it: Claude Code's interface
/// must see the paste end before the Enter, or it takes the Enter as part of
/// it (inventory A6, problem 3).
pub const PASTE_GAP_MS: u64 = 120;
/// Between two keys.
const KEY_GAP_MS: u64 = 40;

/// Carries out a model write whose answer the operation does not need: the
/// TS called these synchronously and went on.
pub async fn tell(port: &Port, cmd: ModelCmd) {
    let _ = port.apply(cmd).await;
}

pub async fn announce(port: &Port, event: ControlEvent) {
    tell(port, ModelCmd::Announce(event)).await;
}

/// The workspace as it stands now, else as the request first saw it.
fn current(snapshot: &Snapshot, ws: &Workspace) -> Workspace {
    workspace_in(snapshot, &ws.id)
        .cloned()
        .unwrap_or_else(|| ws.clone())
}

pub fn workspace_list(snapshot: &Snapshot, caller: &Caller) -> Value {
    let state = &snapshot.state;
    let mine = caller_workspace(state, caller).map(|ws| ws.id.clone());
    let workspaces = state
        .workspaces
        .iter()
        .map(|ws| {
            object([
                ("name", json!(ws.name)),
                ("cwd", json!(ws.cwd)),
                ("panes", json!(ws.panes.len())),
                (
                    "active",
                    json!(state.active_workspace_id.as_deref() == Some(ws.id.as_str())),
                ),
                ("open", json!(is_open(snapshot, ws))),
                ("asleep", json!(is_asleep(snapshot, &ws.id))),
                ("yours", json!(mine.as_deref() == Some(ws.id.as_str()))),
                ("id", json!(ws.id)),
            ])
        })
        .collect::<Vec<_>>();
    json!({ "workspaces": workspaces })
}

pub async fn pane_list(ws: &Workspace, caller: &Caller, port: &Port) -> Result<Value, String> {
    let snapshot = port.snapshot();
    let open = is_open(&snapshot, &current(&snapshot, ws));
    let mut panes = Vec::new();
    for pane in ordered(ws) {
        let running = open && port.alive(&pane.id).await;
        let activity = activity_of(&snapshot, &pane.id).map(|seen| json!(seen.activity));
        panes.push(object([
            ("name", json!(pane.name)),
            ("agent", json!(pane.agent)),
            ("cwd", json!(effective_cwd(ws, pane))),
            ("running", json!(running)),
            ("activity", activity.unwrap_or(Value::Null)),
            (
                "you",
                json!(caller.pane_id.as_deref() == Some(pane.id.as_str())),
            ),
            ("id", json!(pane.id)),
        ]));
    }
    let asleep = !open && is_asleep(&snapshot, &ws.id);
    let mut workspace = json!({ "name": ws.name, "cwd": ws.cwd, "open": open });
    if asleep {
        workspace["asleep"] = json!(true);
    }
    let mut answer = json!({ "workspace": workspace, "panes": panes });
    // Not mounted yet: its terminals start the first time it is shown, or as
    // soon as an agent creates, sends to or restarts one of its panes.
    if !open {
        answer["note"] = json!(if asleep {
            format!("This workspace is asleep, so none of its panes is running. {ASLEEP}")
        } else {
            "This workspace is not open yet, so none of its panes is running.".to_string()
        });
    }
    Ok(answer)
}

fn full(ws: &Workspace) -> String {
    format!(
        "\"{}\" is full: a workspace holds at most {MAX_PANES} panes.",
        ws.name
    )
}

pub async fn pane_create(
    ws: &Workspace,
    args: &Value,
    caller: &Caller,
    state: &AppState,
    port: &Port,
) -> Result<Value, String> {
    if ws.panes.len() >= MAX_PANES {
        return Err(full(ws));
    }
    let agent = agent_arg(args.get("agent"))?;
    let name = text(args.get("name"));
    // Unique across every workspace: other sessions reach a pane by name.
    if let Some(name) = &name
        && let Some(holder) = state.workspaces.iter().find(|entry| has_pane(entry, name))
    {
        return Err(format!(
            "A pane is already named \"{name}\" (in \"{}\"); pane names are unique across Hangar.",
            holder.name
        ));
    }
    let mut cwd = None;
    if let Some(folder) = text(args.get("cwd")) {
        let resolved = resolve_folder(&ws.cwd, &folder);
        if !port.dir_exists(&resolved).await {
            return Err(format!("No such folder: {resolved}"));
        }
        cwd = Some(resolved);
    }
    let near = match text(args.get("near")) {
        Some(_) => Some(pane_in(ws, args.get("near"))?),
        None => None,
    };
    let model = near.or_else(|| ordered(ws).last().copied());
    let final_agent = agent
        .or(model.map(|pane| pane.agent))
        .unwrap_or(AgentId::Shell);
    let prompt = args
        .get("prompt")
        .and_then(Value::as_str)
        .filter(|prompt| !prompt.trim().is_empty())
        .map(str::to_string);
    if prompt.is_some() && final_agent != AgentId::Claude {
        return Err(
            "A prompt can only be handed to a Claude Code pane at creation. Create the pane without it, \
             then send it with pane_send once pane_read shows the agent has started."
                .into(),
        );
    }

    let added = port
        .apply(ModelCmd::AddPane {
            ws: ws.id.clone(),
            agent,
            near: near.map(|pane| pane.id.clone()),
            dir: Some(
                if args.get("direction").and_then(Value::as_str) == Some("down") {
                    SplitDir::Col
                } else {
                    SplitDir::Row
                },
            ),
            cwd: cwd.clone(),
            name: name.clone(),
            init: None,
        })
        .await?;
    let Some(id) = added.pane else {
        return Err(full(ws));
    };
    let snapshot = port.snapshot();
    let created: Option<Pane> = workspace_in(&snapshot, &ws.id)
        .and_then(|entry| entry.panes.iter().find(|pane| pane.id == id))
        .cloned();
    tell(port, ModelCmd::OpenWorkspace(ws.id.clone())).await;
    // An agent's words, never the person's: the new session reads who sent them.
    if let Some(prompt) = &prompt {
        port.enqueue(
            &id,
            prompt,
            Some(caller.pane_name.as_deref().unwrap_or("unknown")),
        )
        .await?;
    }
    if args.get("focus") == Some(&Value::Bool(true)) {
        tell(
            port,
            ModelCmd::Activate {
                ws: ws.id.clone(),
                pane: id.clone(),
            },
        )
        .await;
    }
    let pane_name = created
        .as_ref()
        .map(|pane| pane.name.clone())
        .or(name)
        .unwrap_or_else(|| id.clone());
    announce(
        port,
        ControlEvent::Created {
            by: caller.pane_name.clone(),
            pane: pane_name.clone(),
            workspace: ws.name.clone(),
        },
    )
    .await;
    let mut answer = json!({
        "workspace": ws.name,
        "pane": {
            "name": pane_name,
            "agent": created.as_ref().map_or(final_agent, |pane| pane.agent),
            "cwd": match &created {
                Some(pane) => effective_cwd(ws, pane),
                None => cwd.unwrap_or_else(|| ws.cwd.clone()),
            },
            "id": id,
        },
    });
    if prompt.is_some() {
        answer["prompt"] = json!("queued: submitted as soon as the Claude Code session is ready");
    }
    Ok(answer)
}

pub async fn pane_close(
    ws: &Workspace,
    args: &Value,
    caller: &Caller,
    port: &Port,
) -> Result<Value, String> {
    let pane = pane_in(ws, args.get("pane"))?;
    if ws.panes.len() <= 1 {
        return Err(format!(
            "\"{}\" is the last pane of \"{}\" and cannot be closed.",
            pane.name, ws.name
        ));
    }
    tell(
        port,
        ModelCmd::ClosePane {
            ws: ws.id.clone(),
            pane: pane.id.clone(),
        },
    )
    .await;
    announce(
        port,
        ControlEvent::Closed {
            by: caller.pane_name.clone(),
            pane: pane.name.clone(),
            workspace: ws.name.clone(),
        },
    )
    .await;
    Ok(json!({ "closed": pane.name, "workspace": ws.name }))
}

pub async fn pane_restart(
    ws: &Workspace,
    args: &Value,
    caller: &Caller,
    port: &Port,
) -> Result<Value, String> {
    let pane = pane_in(ws, args.get("pane"))?;
    let agent = agent_arg(args.get("agent"))?;
    let fresh = args.get("fresh") == Some(&Value::Bool(true)) || agent.is_some();
    let patch = match agent {
        Some(agent) => Patch::new(move |pane: &mut Pane| {
            pane.agent = agent;
            pane.session_id = None;
        }),
        None if fresh => Patch::new(|pane: &mut Pane| pane.session_id = None),
        None => Patch::none(),
    };
    let respawned = port
        .apply(ModelCmd::RespawnPane {
            ws: ws.id.clone(),
            pane: pane.id.clone(),
            patch,
            fresh,
        })
        .await?;
    let Some(id) = respawned.pane else {
        return Err(format!("\"{}\" was closed in the meantime.", pane.name));
    };
    tell(port, ModelCmd::OpenWorkspace(ws.id.clone())).await;
    let by = caller.pane_name.clone();
    let (pane_name, workspace) = (pane.name.clone(), ws.name.clone());
    announce(
        port,
        if fresh {
            ControlEvent::Reset {
                by,
                pane: pane_name,
                workspace,
            }
        } else {
            ControlEvent::Restarted {
                by,
                pane: pane_name,
                workspace,
            }
        },
    )
    .await;
    Ok(json!({
        "pane": { "name": pane.name, "agent": agent.unwrap_or(pane.agent), "id": id },
        "fresh": fresh,
    }))
}

/// A key name as JavaScript's `String(key).toLowerCase()` read it.
fn key_name(key: &Value) -> String {
    match key {
        Value::String(name) => name.to_lowercase(),
        other => other.to_string().to_lowercase(),
    }
}

pub async fn pane_send(
    ws: &Workspace,
    args: &Value,
    caller: &Caller,
    port: &Port,
) -> Result<Value, String> {
    let pane = pane_in(ws, args.get("pane"))?;
    let body = args.get("text").and_then(Value::as_str).unwrap_or("");
    let keys: Vec<String> = args
        .get("keys")
        .and_then(Value::as_array)
        .map(|keys| keys.iter().map(key_name).collect())
        .unwrap_or_default();
    if body.is_empty() && keys.is_empty() {
        return Err("Nothing to send: give text, keys, or both.".into());
    }
    let snapshot = port.snapshot();
    let open = is_open(&snapshot, &current(&snapshot, ws));

    if args.get("when_idle") == Some(&Value::Bool(true)) {
        if pane.agent != AgentId::Claude {
            return Err(format!(
                "when_idle only works with Claude Code panes, and \"{}\" runs {}.",
                pane.name,
                pane.agent.as_str()
            ));
        }
        if !keys.is_empty() {
            return Err("Keys cannot be queued: send them without when_idle.".into());
        }
        port.enqueue(
            &pane.id,
            body,
            Some(caller.pane_name.as_deref().unwrap_or("unknown")),
        )
        .await?;
        // Held for a Claude Code that is not running, it would wait for good.
        let mut answer = json!({ "queued": true, "pane": pane.name });
        if !open {
            tell(port, ModelCmd::OpenWorkspace(ws.id.clone())).await;
            answer["starting"] = json!(true);
        }
        return Ok(answer);
    }

    let Some(term) = port.terminal(&pane.id) else {
        if !open {
            let asleep = is_asleep(&snapshot, &ws.id);
            tell(port, ModelCmd::OpenWorkspace(ws.id.clone())).await;
            return Err(if asleep {
                format!(
                    "\"{}\" was asleep: its agents are resuming their sessions now. Send again in a few seconds.",
                    ws.name
                )
            } else {
                format!(
                    "\"{}\" was not open: its panes are starting now. Send again in a few seconds.",
                    ws.name
                )
            });
        }
        return Err(format!(
            "\"{}\" has no terminal yet. Send again in a moment.",
            pane.name
        ));
    };
    // Checked before anything is typed, so a bad key name sends nothing.
    let application_cursor = term.application_cursor();
    let sequences = keys
        .iter()
        .map(|key| key_sequence(key, application_cursor))
        .collect::<Result<Vec<_>, _>>()?;
    let submit = args.get("submit") != Some(&Value::Bool(false));
    // Through the terminal rather than straight to the PTY: it brackets the
    // paste, so several lines land as one message instead of one Enter each.
    if !body.is_empty() {
        term.paste(body).await?;
    }
    if !body.is_empty() && (submit || !sequences.is_empty()) {
        port.sleep(PASTE_GAP_MS).await;
    }
    if !body.is_empty() && submit {
        port.write(&pane.id, "\r").await?;
    }
    for sequence in &sequences {
        port.write(&pane.id, sequence).await?;
        port.sleep(KEY_GAP_MS).await;
    }
    Ok(json!({ "sent": true, "pane": pane.name }))
}

pub async fn pane_read(ws: &Workspace, args: &Value, port: &Port) -> Result<Value, String> {
    let pane = pane_in(ws, args.get("pane"))?;
    let Some(term) = port.terminal(&pane.id) else {
        let snapshot = port.snapshot();
        return Err(if is_open(&snapshot, &current(&snapshot, ws)) {
            format!("\"{}\" has no terminal yet.", pane.name)
        } else if is_asleep(&snapshot, &ws.id) {
            format!(
                "\"{}\" is asleep, so \"{}\" shows nothing. {ASLEEP}",
                ws.name, pane.name
            )
        } else {
            format!(
                "\"{}\" is not open, so \"{}\" has shown nothing yet.",
                ws.name, pane.name
            )
        });
    };
    let wanted = match args.get("lines") {
        Some(Value::Number(n)) => n.as_f64().map_or(READ_DEFAULT, f64::floor),
        _ => READ_DEFAULT,
    };
    let lines = term.read(wanted.clamp(1.0, READ_MAX) as usize);
    Ok(json!({ "pane": pane.name, "lines": lines.len(), "text": lines.join("\n") }))
}

pub async fn workspace_window(ws: &Workspace, args: &Value, port: &Port) -> Result<Value, String> {
    let detach = args.get("detach") != Some(&Value::Bool(false));
    if detach == (ws.detached == Some(true)) {
        return Ok(json!({ "workspace": ws.name, "detached": detach, "changed": false }));
    }
    if detach {
        if !port.can_detach() {
            return Err("This Hangar cannot open other windows.".into());
        }
        tell(port, ModelCmd::Detach(ws.id.clone())).await;
    } else {
        tell(port, ModelCmd::Reattach(ws.id.clone())).await;
    }
    Ok(json!({ "workspace": ws.name, "detached": detach, "changed": true }))
}

pub async fn pane_focus(ws: &Workspace, args: &Value, port: &Port) -> Result<Value, String> {
    let pane = pane_in(ws, args.get("pane"))?;
    tell(
        port,
        ModelCmd::Activate {
            ws: ws.id.clone(),
            pane: pane.id.clone(),
        },
    )
    .await;
    Ok(json!({ "focused": pane.name, "workspace": ws.name }))
}
