//! The dispatch of an agent's operation (`runControl`), the gate in front of
//! it, and which workspace and pane a request means.

use serde_json::Value;

use crate::state::layout::reading_order;
use crate::state::types::AGENTS;
use crate::state::{AgentId, AppState, Pane, Workspace};

use super::port::{ModelCmd, Port};
use super::types::{Caller, normalize, same, text};

/// Every operation, while the setting is off.
pub const DISABLED: &str = "Pane control is turned off in Hangar.AI (Settings › General › Agents). Ask the user to turn it on.";

/// Said wherever a sleeping workspace is the reason nothing runs.
pub const ASLEEP: &str = "Hangar put it to sleep to free memory, as nobody had used it for a while: its agents were stopped, \
and resume their sessions as soon as it opens. pane_send, pane_create and pane_restart open it.";

/// Who an operation runs for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// An agent, through the MCP server: refused while the setting is off.
    Agent,
    /// The person, from the phone: never refused for the setting.
    Person,
}

/// An agent's operation, run to its answer (`runControl`).
pub async fn run(op: &str, args: Value, port: &Port, gate: Gate) -> Result<Value, String> {
    let snapshot = port.snapshot();
    if gate == Gate::Agent && !snapshot.state.settings.agent_pane_control {
        return Err(DISABLED.into());
    }
    let caller = Caller::of(&args);
    let state = &snapshot.state;

    if op == "workspace_list" {
        return Ok(super::panes::workspace_list(&snapshot, &caller));
    }
    if op == "dev_command" && cfg!(debug_assertions) {
        return dev_command(&args, port).await;
    }
    // The project manager works across workspaces: manager.rs finds its own.
    if op.starts_with("project_") {
        return super::manager::control(op, &args, &caller, port).await;
    }
    if op.starts_with("kit_tab_") {
        return super::kit_tabs::control(op, &args, &caller, port).await;
    }
    if op == "workspace_create" {
        return super::workspace::create(&args, &caller, state, port).await;
    }
    if op == "workspace_set_check" {
        return super::workspace::set_check(&args, &caller, state, port).await;
    }

    let ws = workspace_for(state, &args, &caller)?.clone();
    // What an agent does to a workspace's panes counts as using it, like
    // typing into them would; reading them does not.
    if matches!(op, "pane_send" | "pane_create" | "pane_restart") {
        let _ = port.apply(ModelCmd::Touch(ws.id.clone())).await;
    }

    // Orchestration has modules of its own: P19's engine, and delivery.rs.
    if op.starts_with("worker_") {
        return match port.orchestration() {
            Some(engine) => {
                engine
                    .worker_control(port.clone(), op.to_string(), ws, args, caller)
                    .await
            }
            None => Err("Workers cannot be started from this Hangar window.".into()),
        };
    }
    if op.starts_with("delivery_") {
        return super::delivery::control(op, &ws, &args, &caller, port).await;
    }

    match op {
        "pane_list" => super::panes::pane_list(&ws, &caller, port).await,
        "pane_create" => super::panes::pane_create(&ws, &args, &caller, state, port).await,
        "pane_close" => super::panes::pane_close(&ws, &args, &caller, port).await,
        "pane_restart" => super::panes::pane_restart(&ws, &args, &caller, port).await,
        "pane_send" => super::panes::pane_send(&ws, &args, &caller, port).await,
        "pane_read" => super::panes::pane_read(&ws, &args, port).await,
        "workspace_window" => super::panes::workspace_window(&ws, &args, port).await,
        "pane_focus" => super::panes::pane_focus(&ws, &args, port).await,
        "model_show" => super::preview::model_show(&ws, &args, &caller, port).await,
        "model_snapshot" => super::preview::model_snapshot(&ws, &args, &caller, port).await,
        "model_close" => super::preview::model_close(&ws, &args, &caller, port).await,
        "preview_show" => super::preview::preview_show(&ws, &args, &caller, port).await,
        "preview_close" => super::preview::preview_close(&ws, &args, &caller, port).await,
        _ => Err(unknown(op)),
    }
}

/// Runs a command of the palette by its id (`view.board`), or fills the open
/// palette's field with `query` (debug builds: the captures of the README and
/// the site).
async fn dev_command(args: &Value, port: &Port) -> Result<Value, String> {
    let id = args.get("command").and_then(Value::as_str);
    let command = id
        .map(|id| {
            id.parse::<crate::commands::CommandId>()
                .map_err(|_| format!("No command is called \"{id}\"."))
        })
        .transpose()?;
    let text = |key: &str| args.get(key).and_then(Value::as_str).map(str::to_string);
    let (query, stats_project) = (text("query"), text("stats_project"));
    let number = |key: &str| {
        args.get("capture")
            .and_then(|capture| capture.get(key))
            .and_then(Value::as_f64)
    };
    let capture = number("width")
        .zip(number("height"))
        .map(|(width, height)| iced::Size::new(width as f32, height as f32));
    let pane_menu = text("pane_menu");
    if command.is_none()
        && query.is_none()
        && stats_project.is_none()
        && capture.is_none()
        && pane_menu.is_none()
    {
        return Err(
            "Give command (a command id, such as view.board), query, stats_project or capture."
                .into(),
        );
    }
    port.apply(ModelCmd::DevCommand {
        command,
        query,
        stats_project,
        capture,
        pane_menu,
    })
    .await?;
    Ok(serde_json::json!({ "ran": id }))
}

/// An operation nobody answers.
pub fn unknown(op: &str) -> String {
    format!("Unknown operation \"{op}\".")
}

/// A pane's folder: its own, else its workspace's.
pub fn effective_cwd(ws: &Workspace, pane: &Pane) -> String {
    match pane.cwd.as_deref() {
        Some(cwd) if !cwd.is_empty() => cwd.to_string(),
        _ => ws.cwd.clone(),
    }
}

/// Panes in reading order, which is what "the last one" means on screen
/// (`leafIds(normalizeTree(...))`): the shell's `reading_order`.
pub fn ordered(ws: &Workspace) -> Vec<&Pane> {
    reading_order(&ws.panes, ws.tree.as_ref())
}

/// The workspace the agent is working in: its pane if Hangar launched it,
/// otherwise the folder it runs in (`callerWorkspace`).
pub fn caller_workspace<'a>(state: &'a AppState, caller: &Caller) -> Option<&'a Workspace> {
    if let Some(pane) = &caller.pane_id
        && let Some(owner) = state
            .workspaces
            .iter()
            .find(|ws| ws.panes.iter().any(|entry| &entry.id == pane))
    {
        return Some(owner);
    }
    let here = normalize(caller.cwd.as_deref()?);
    let exact: Vec<&Workspace> = state
        .workspaces
        .iter()
        .filter(|ws| {
            normalize(&ws.cwd) == here
                || ws.panes.iter().any(|pane| {
                    pane.cwd
                        .as_deref()
                        .is_some_and(|cwd| !cwd.is_empty() && normalize(cwd) == here)
                })
        })
        .collect();
    if exact.len() > 1 {
        // Two workspaces on one folder: the pane name tells them apart, then
        // whichever the person is looking at.
        let named = caller.pane_name.as_deref().and_then(|name| {
            exact
                .iter()
                .find(|ws| ws.panes.iter().any(|pane| same(&pane.name, name)))
        });
        return named
            .or_else(|| {
                exact
                    .iter()
                    .find(|ws| state.active_workspace_id.as_deref() == Some(ws.id.as_str()))
            })
            .or(exact.first())
            .copied();
    }
    if let [only] = exact.as_slice() {
        return Some(only);
    }
    let mut holders: Vec<&Workspace> = state
        .workspaces
        .iter()
        .filter(|ws| here.starts_with(&format!("{}/", normalize(&ws.cwd))))
        .collect();
    holders.sort_by_key(|ws| std::cmp::Reverse(ws.cwd.len()));
    holders.first().copied()
}

/// The quoted names of every workspace, for a refusal.
fn workspace_names(state: &AppState) -> String {
    let names = state
        .workspaces
        .iter()
        .map(|ws| format!("\"{}\"", ws.name))
        .collect::<Vec<_>>()
        .join(", ");
    if names.is_empty() {
        "none".into()
    } else {
        names
    }
}

/// The workspace a request is for (`workspaceFor`): the one it names, else
/// the one holding the pane it names, else the caller's.
pub fn workspace_for<'a>(
    state: &'a AppState,
    args: &Value,
    caller: &Caller,
) -> Result<&'a Workspace, String> {
    if let Some(wanted) = text(args.get("workspace")) {
        return state
            .workspaces
            .iter()
            .find(|ws| ws.id == wanted)
            .or_else(|| state.workspaces.iter().find(|ws| same(&ws.name, &wanted)))
            .or_else(|| {
                state
                    .workspaces
                    .iter()
                    .find(|ws| normalize(&ws.cwd) == normalize(&wanted))
            })
            .ok_or_else(|| {
                format!(
                    "No workspace \"{wanted}\". Workspaces: {}.",
                    workspace_names(state)
                )
            });
    }
    let mine = caller_workspace(state, caller);
    // Pane names are unique across workspaces, so a pane named without its
    // workspace is found wherever it is, the caller's own workspace first.
    if let Some(pane) = text(args.get("pane"))
        && !mine.is_some_and(|ws| has_pane(ws, &pane))
    {
        let holders: Vec<&Workspace> = state
            .workspaces
            .iter()
            .filter(|ws| has_pane(ws, &pane))
            .collect();
        if let [only] = holders.as_slice() {
            return Ok(only);
        }
    }
    mine.ok_or_else(|| {
        format!(
            "Could not tell which workspace you are in. Pass \"workspace\", one of: {}.",
            workspace_names(state)
        )
    })
}

/// Whether a workspace has the pane, by id or by name.
pub fn has_pane(ws: &Workspace, reference: &str) -> bool {
    ws.panes
        .iter()
        .any(|pane| pane.id == reference || same(&pane.name, reference))
}

/// The pane a request names in `ws` (`paneIn`).
pub fn pane_in<'a>(ws: &'a Workspace, reference: Option<&Value>) -> Result<&'a Pane, String> {
    let names = ordered(ws)
        .iter()
        .map(|pane| format!("\"{}\"", pane.name))
        .collect::<Vec<_>>()
        .join(", ");
    let Some(wanted) = text(reference) else {
        return Err(format!("Name the pane. Panes of \"{}\": {names}.", ws.name));
    };
    ws.panes
        .iter()
        .find(|pane| pane.id == wanted)
        .or_else(|| ws.panes.iter().find(|pane| same(&pane.name, &wanted)))
        .ok_or_else(|| format!("No pane \"{wanted}\" in \"{}\". Panes: {names}.", ws.name))
}

/// The agent a request names, if any (`agentArg`).
pub fn agent_arg(value: Option<&Value>) -> Result<Option<AgentId>, String> {
    let Some(wanted) = text(value) else {
        return Ok(None);
    };
    AGENTS
        .iter()
        .find(|agent| agent.id.as_str() == wanted)
        .map(|agent| Some(agent.id))
        .ok_or_else(|| {
            let ids = AGENTS
                .iter()
                .map(|agent| agent.id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            format!("Unknown agent \"{wanted}\". One of: {ids}.")
        })
}

/// The keys `pane_send` types, as the terminal itself would send them.
const KEYS: [(&str, &str); 7] = [
    ("enter", "\r"),
    ("escape", "\x1b"),
    ("tab", "\t"),
    ("shift+tab", "\x1b[Z"),
    ("backspace", "\x7f"),
    ("ctrl+c", "\x03"),
    ("ctrl+d", "\x04"),
];

/// Arrows: an application that turned on cursor-key mode expects `ESC O`,
/// everything else `ESC [`.
const ARROWS: [(&str, char); 4] = [("up", 'A'), ("down", 'B'), ("right", 'C'), ("left", 'D')];

/// What a key name types (`keySequence`).
pub fn key_sequence(name: &str, application_cursor: bool) -> Result<String, String> {
    if let Some((_, arrow)) = ARROWS.iter().find(|(key, _)| *key == name) {
        let mode = if application_cursor { 'O' } else { '[' };
        return Ok(format!("\x1b{mode}{arrow}"));
    }
    KEYS.iter()
        .find(|(key, _)| *key == name)
        .map(|(_, sequence)| sequence.to_string())
        .ok_or_else(|| {
            let known = KEYS
                .iter()
                .map(|(key, _)| *key)
                .chain(ARROWS.iter().map(|(key, _)| *key))
                .collect::<Vec<_>>()
                .join(", ");
            format!("Unknown key \"{name}\". One of: {known}.")
        })
}

/// A number argument, as JavaScript read one: a number, or a numeric string.
pub fn number_arg(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(number) => number.as_f64(),
        Value::String(raw) => {
            let raw = raw.trim();
            if raw.is_empty() {
                Some(0.0)
            } else {
                raw.parse().ok()
            }
        }
        _ => None,
    }
    .filter(|number: &f64| number.is_finite())
}

/// The workspace on screen: the main window's active one, or one in a window
/// of its own (`onScreen`).
pub fn on_screen(state: &AppState, ws: &Workspace) -> bool {
    ws.detached == Some(true) || state.active_workspace_id.as_deref() == Some(ws.id.as_str())
}
