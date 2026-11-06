//! The phone's operations on the panes and the projects (lib/remoteControl.ts):
//! what waits for the person, a pane's screen, typing into it as the person,
//! a permission answered, the manager told, a project woken, the history.
//!
//! The person is the one asking, from their phone, so what is sent is typed
//! as them, through P18's `pane_send` and `pane_read` run as the person:
//! agents' pane control being turned off does not stop it.
//!
//! Claude Code's permission prompt takes one key: Enter accepts the
//! highlighted "1. Yes", Escape is "No, and tell Claude what to do
//! differently". The hangar-bridge mod reports what the call would do, never
//! which choices the prompt offers, so those two keys are all that is sent.

use serde_json::{Map, Value, json};

use hangar_i18n::{Key, Locale, t};

use crate::control::port::{ModelCmd, Port, activity_of, is_asleep, is_open};
use crate::control::run_as_person;
use crate::control::types::same;
use crate::state::roles::{manager_of, orchestrator_of};
use crate::state::{AgentActivity, AgentId, AppState, Pane, Snapshot, Workspace};

use super::contract::{
    ChatPending, NamedActivity, REMOTE_KEYS, RemoteHistory, RemoteManager, RemotePane,
    RemoteProject, RemoteScreen, RemoteSent, RemoteState, RemoteWaiting, RemoteWoken,
    RemoteWorkspace, WaitingKind,
};
use super::desk::pending_now;
use super::engine::Mobile;
use crate::orchestration::manager_feed::{ManagerEvent, ManagerJournalEntry};

const SCREEN_DEFAULT: f64 = 80.0;
pub const NO_MANAGER: &str = "Open the project manager on the PC";
/// A closed or asleep workspace gets this long to start a pane's shell for a
/// send: within the 10 s the mobile server gives the window, which is within
/// the 12 s the phone gives the server, past which the phone tries again.
pub const OPEN_WAIT_MS: u64 = 8_000;
const OPEN_POLL_MS: u64 = 250;
/// Past this, a Claude Code woken from the phone that never said hello is
/// taken to run without the mod.
pub const RESUME_MS: u64 = 120_000;

/// A string argument, trimmed; empty when it is anything else.
fn text_arg(args: &Value, key: &str) -> String {
    args.get(key)
        .and_then(Value::as_str)
        .map(|text| text.trim().to_string())
        .unwrap_or_default()
}

/// A pane by id or by name, wherever it is (`findPane`).
pub fn find_pane(state: &AppState, wanted: Option<&Value>) -> Result<(Workspace, Pane), String> {
    let wanted = wanted
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if wanted.is_empty() {
        return Err("Name the pane, by id or by name.".into());
    }
    let found = |hit: &dyn Fn(&Pane) -> bool| {
        state.workspaces.iter().find_map(|ws| {
            ws.panes
                .iter()
                .find(|pane| hit(pane))
                .map(|pane| (ws.clone(), pane.clone()))
        })
    };
    found(&|pane| pane.id == wanted)
        .or_else(|| found(&|pane| same(&pane.name, wanted)))
        .ok_or_else(|| format!("No pane \"{wanted}\" in Hangar."))
}

/// A project by id or by name (`findWorkspace`).
pub fn find_workspace(state: &AppState, wanted: Option<&Value>) -> Result<Workspace, String> {
    let wanted = wanted
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if wanted.is_empty() {
        return Err("Name the project, by id or by name.".into());
    }
    state
        .workspaces
        .iter()
        .find(|ws| ws.id == wanted)
        .or_else(|| state.workspaces.iter().find(|ws| same(&ws.name, wanted)))
        .cloned()
        .ok_or_else(|| format!("No project \"{wanted}\" in Hangar."))
}

fn activity(snapshot: &Snapshot, pane: &str) -> Option<AgentActivity> {
    activity_of(snapshot, &pane.to_string()).map(|entry| entry.activity)
}

fn bridged(snapshot: &Snapshot, pane: &str) -> bool {
    activity_of(snapshot, &pane.to_string()).is_some_and(|entry| entry.bridged)
}

/// The workspace as it stands now, or as it was given.
fn current(snapshot: &Snapshot, ws: &Workspace) -> Workspace {
    snapshot
        .state
        .workspaces
        .iter()
        .find(|entry| entry.id == ws.id)
        .cloned()
        .unwrap_or_else(|| ws.clone())
}

fn open(snapshot: &Snapshot, ws: &Workspace) -> bool {
    is_open(snapshot, &current(snapshot, ws))
}

/* ── remote_state ─────────────────────────────────────────────── */

/// Since when something waits: the journal's time when it has one, else the
/// first time it was seen (`since`).
fn since(mobile: &Mobile, key: &str, journaled: Option<u64>, at: u64) -> u64 {
    let mut book = mobile.book();
    match journaled {
        Some(journaled) => {
            book.first_seen.insert(key.to_string(), journaled);
            journaled
        }
        None => *book.first_seen.entry(key.to_string()).or_insert(at),
    }
}

/// The latest time the manager's journal recorded an event that matches.
fn journaled_at(
    journal: &[ManagerJournalEntry],
    hit: impl Fn(&ManagerEvent) -> bool,
) -> Option<u64> {
    journal
        .iter()
        .filter(|entry| hit(&entry.event))
        .map(|entry| entry.at)
        .max()
}

/// `remote_state {}`: every project, the manager, and what waits for the
/// person, oldest first. Each phone asks every 2 s.
///
/// The summaries come from `Sources::projects`, which the app keeps for a few
/// seconds (live.rs): what moves faster than that — whether a project is
/// open or asleep, what its agents are doing — is read from the snapshot of
/// now rather than from the summary.
pub async fn remote_state(port: &Port, mobile: &Mobile) -> RemoteState {
    let at = port.now_ms();
    let summaries = mobile.sources().projects().await;
    let journal = mobile.sources().manager_journal();
    let snapshot = port.snapshot();
    let state = &snapshot.state;

    let mut waiting = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for ws in &state.workspaces {
        for pane in &ws.panes {
            if pane.agent == AgentId::Shell
                || activity(&snapshot, &pane.id) != Some(AgentActivity::Waiting)
            {
                continue;
            }
            let key = format!("pane:{}", pane.id);
            let journaled = journaled_at(
                &journal,
                |event| matches!(event, ManagerEvent::PaneWaiting { pane_id, .. } if *pane_id == pane.id),
            );
            waiting.push(RemoteWaiting {
                kind: WaitingKind::Pane,
                workspace_id: ws.id.clone(),
                workspace: ws.name.clone(),
                pane_id: Some(pane.id.clone()),
                pane: Some(pane.name.clone()),
                task_id: None,
                title: None,
                since: since(mobile, &key, journaled, at),
            });
            seen.insert(key);
        }
    }
    for project in &summaries {
        for (kind, name, tasks) in [
            (WaitingKind::NeedsYou, "needsYou", &project.needs_you),
            (
                WaitingKind::ReadyToLand,
                "readyToLand",
                &project.ready_to_land,
            ),
        ] {
            for task in tasks {
                let key = format!("{name}:{}:{}", project.id, task.id);
                let journaled = journaled_at(&journal, |event| match event {
                    ManagerEvent::TaskNeedsYou { task_id, .. } => {
                        kind == WaitingKind::NeedsYou && *task_id == task.id
                    }
                    ManagerEvent::ReadyToLand { task_id, .. } => {
                        kind == WaitingKind::ReadyToLand && *task_id == task.id
                    }
                    _ => false,
                });
                waiting.push(RemoteWaiting {
                    kind,
                    workspace_id: project.id.clone(),
                    workspace: project.name.clone(),
                    pane_id: None,
                    pane: None,
                    task_id: Some(task.id.clone()),
                    title: Some(task.title.clone()),
                    since: since(mobile, &key, journaled, at),
                });
                seen.insert(key);
            }
        }
    }
    {
        // What is kept of panes closed since goes with them (MOB-16).
        let panes: std::collections::HashSet<&str> = state
            .workspaces
            .iter()
            .flat_map(|ws| ws.panes.iter().map(|pane| pane.id.as_str()))
            .collect();
        let mut book = mobile.book();
        book.first_seen.retain(|key, _| seen.contains(key));
        book.resuming
            .retain(|pane, _| panes.contains(pane.as_str()));
        book.chat.keep_only(&panes);
    }
    waiting.sort_by_key(|entry| entry.since);

    let manager = manager_of(state).map(|(_, pane)| RemoteManager {
        pane_id: pane.id.clone(),
        name: pane.name.clone(),
        activity: activity(&snapshot, &pane.id),
    });
    let projects = summaries
        .iter()
        .map(|project| {
            let ws = state.workspaces.iter().find(|ws| ws.id == project.id);
            RemoteProject {
                id: project.id.clone(),
                name: project.name.clone(),
                open: ws.map_or(project.open, |ws| is_open(&snapshot, ws)),
                asleep: ws.map_or(project.asleep, |ws| is_asleep(&snapshot, &ws.id)),
                git: project.git,
                orchestrator: match ws {
                    Some(ws) => orchestrator_of(ws).map(|pane| NamedActivity {
                        name: pane.name.clone(),
                        activity: activity(&snapshot, &pane.id),
                    }),
                    None => project.orchestrator.as_ref().map(|pane| NamedActivity {
                        name: pane.name.clone(),
                        activity: pane.activity,
                    }),
                },
                panes: ws
                    .map(|ws| ws.panes.as_slice())
                    .unwrap_or_default()
                    .iter()
                    .map(|pane| RemotePane {
                        id: pane.id.clone(),
                        name: pane.name.clone(),
                        agent: pane.agent.as_str().into(),
                        role: pane.role,
                        activity: activity(&snapshot, &pane.id),
                    })
                    .collect(),
                board: project.board,
                plan_status: project.plan_status.clone(),
                needs_you: project.needs_you.clone(),
                ready_to_land: project.ready_to_land.clone(),
            }
        })
        .collect();
    RemoteState {
        at,
        manager,
        waiting,
        projects,
    }
}

/* ── remote_screen ────────────────────────────────────────────── */

/// `remote_screen {pane, lines?}`: the pane's last lines; empty while its
/// workspace sleeps or is closed, which is not the pane being gone. While it
/// waits on a permission the PC can name, `prompt` says which, for the
/// phone's answer to name it back (MOB-02).
pub async fn remote_screen(
    port: &Port,
    mobile: &Mobile,
    args: &Value,
) -> Result<RemoteScreen, String> {
    let snapshot = port.snapshot();
    let (ws, pane) = find_pane(&snapshot.state, args.get("pane"))?;
    let lines = args
        .get("lines")
        .and_then(Value::as_f64)
        .unwrap_or(SCREEN_DEFAULT);
    let read = run_as_person(
        "pane_read",
        json!({ "workspace": ws.id, "pane": pane.id, "lines": lines }),
        port,
    )
    .await;
    let text = match read {
        Ok(read) => read["text"].as_str().unwrap_or("").to_string(),
        Err(error) if port.terminal(&pane.id).is_some() => return Err(error),
        Err(_) => String::new(),
    };
    let prompt = if activity(&snapshot, &pane.id) == Some(AgentActivity::Waiting) {
        match pending_now(port, mobile, &ws, &pane).await {
            Ok(Some(ChatPending::Permission { prompt, .. })) => Some(prompt),
            _ => None,
        }
    } else {
        None
    };
    Ok(RemoteScreen {
        prompt,
        pane_id: pane.id.clone(),
        pane: pane.name.clone(),
        workspace: ws.name.clone(),
        agent: pane.agent.as_str().into(),
        activity: activity(&snapshot, &pane.id),
        text,
    })
}

/* ── Typing as the person ─────────────────────────────────────── */

/// The hangar-bridge queue drains only where the mod runs (Settings › Agents).
fn queues(port: &Port) -> bool {
    port.snapshot().state.settings.claude_bridge
}

/// Wakes a workspace asleep, or opens one closed, as a click on the PC does:
/// its terminals mount, each Claude Code resuming its session, and it counts
/// as used, so hibernation leaves it alone for its whole delay.
pub async fn wake(port: &Port, mobile: &Mobile, ws: &Workspace) {
    let snapshot = port.snapshot();
    let starting = is_asleep(&snapshot, &ws.id) || !open(&snapshot, ws);
    let _ = port.apply(ModelCmd::OpenWorkspace(ws.id.clone())).await;
    let _ = port.apply(ModelCmd::Touch(ws.id.clone())).await;
    if !starting || !queues(port) {
        return;
    }
    let at = port.now_ms();
    let mut book = mobile.book();
    for pane in &ws.panes {
        if pane.agent == AgentId::Claude
            && !book.resuming.contains_key(&pane.id)
            && !bridged(&snapshot, &pane.id)
        {
            book.resuming.insert(pane.id.clone(), at);
        }
    }
}

/// A Claude Code pane not up yet: no terminal — its workspace asleep or
/// closed — or one woken from the phone whose Claude Code has not said hello.
pub fn resumes(port: &Port, mobile: &Mobile, pane: &Pane) -> bool {
    if pane.agent != AgentId::Claude {
        return false;
    }
    if port.terminal(&pane.id).is_none() {
        return true;
    }
    let mut book = mobile.book();
    let Some(since) = book.resuming.get(&pane.id).copied() else {
        return false;
    };
    if bridged(&port.snapshot(), &pane.id) || port.now_ms().saturating_sub(since) >= RESUME_MS {
        book.resuming.remove(&pane.id);
        return false;
    }
    true
}

/// Nothing was sent to a Claude Code not up yet: why, said before `wake`
/// changes it.
fn not_up(port: &Port, ws: &Workspace, pane: &Pane) -> String {
    let snapshot = port.snapshot();
    let why = if is_asleep(&snapshot, &ws.id) {
        format!(
            "\"{}\" was asleep: it is waking up now, its agents resuming their sessions.",
            ws.name
        )
    } else if !open(&snapshot, ws) {
        format!("\"{}\" was not open: its panes are starting now.", ws.name)
    } else {
        format!("\"{}\" is resuming its session.", pane.name)
    };
    format!("{why} Nothing was sent; send it again in a few seconds.")
}

/// A pane whose terminal or shell did not start in time: nothing was typed.
fn not_started(pane: &Pane) -> String {
    format!(
        "\"{}\" has not started yet: nothing was sent; send it again in a few seconds.",
        pane.name
    )
}

/// What the person sent a Claude Code not up yet, put in the hangar-bridge
/// queue, which the session takes once it is ready, as their own words; the
/// workspace is woken for it. Without text to queue, or without the mod to
/// take it, nothing is sent (`hold`).
pub async fn hold(
    port: &Port,
    mobile: &Mobile,
    ws: &Workspace,
    pane: &Pane,
    text: Option<&str>,
) -> Result<(), String> {
    let Some(text) = text.filter(|_| queues(port)) else {
        let error = not_up(port, ws, pane);
        wake(port, mobile, ws).await;
        return Err(error);
    };
    port.enqueue(&pane.id, text, None).await?;
    wake(port, mobile, ws).await;
    Ok(())
}

/// What `type` sends.
#[derive(Debug, Clone, Default)]
pub struct Input {
    pub text: Option<String>,
    pub keys: Vec<String>,
    /// `Some(false)`: the text is pasted without Enter.
    pub submit: Option<bool>,
}

/// Typed as the person: pane_send's path, without when_idle (`type`).
///
/// A Claude Code pane that is not up — its workspace asleep or closed, or
/// resuming after the phone woke it — has a shell first and Claude Code
/// seconds later: typed then, the text runs in the shell, or reaches no
/// process at all. It is held in the queue instead (`hold`); keys, or text
/// left unsubmitted, cannot wait there. Any other pane is typed into once
/// its terminal and its shell are up.
pub async fn type_into(
    port: &Port,
    mobile: &Mobile,
    ws: &Workspace,
    pane: &Pane,
    input: Input,
) -> Result<RemoteSent, String> {
    // Keys typed while an answer is being typed into the same dialog would
    // land in the middle of it (MOB-18).
    if mobile.book().answering.contains(&pane.id) {
        return Err(format!(
            "An answer is being typed into \"{}\": wait for it. Nothing was sent.",
            pane.name
        ));
    }
    if resumes(port, mobile, pane) {
        let text = input.text.as_deref().unwrap_or("");
        let holdable =
            !text.trim().is_empty() && input.keys.is_empty() && input.submit != Some(false);
        hold(port, mobile, ws, pane, holdable.then_some(text)).await?;
        return Ok(RemoteSent::QUEUED);
    }
    if port.terminal(&pane.id).is_none() {
        wake(port, mobile, ws).await;
        // A terminal mounts before its shell is spawned: what it takes until
        // then reaches no process.
        let mut waited = 0;
        while waited < OPEN_WAIT_MS {
            port.sleep(OPEN_POLL_MS).await;
            if port.terminal(&pane.id).is_some() && port.alive(&pane.id).await {
                break;
            }
            waited += OPEN_POLL_MS;
        }
        // Still no shell: what is typed would reach no process, and the
        // phone be told it was sent (MOB-17).
        if port.terminal(&pane.id).is_none() || !port.alive(&pane.id).await {
            return Err(not_started(pane));
        }
    }
    let mut args = Map::new();
    args.insert("workspace".into(), json!(ws.id));
    args.insert("pane".into(), json!(pane.id));
    if let Some(text) = &input.text {
        args.insert("text".into(), json!(text));
    }
    args.insert("keys".into(), json!(input.keys));
    if let Some(submit) = input.submit {
        args.insert("submit".into(), json!(submit));
    }
    run_as_person("pane_send", Value::Object(args), port).await?;
    Ok(RemoteSent::SENT)
}

/// How a delivery's words reach its worker (remoteDelivery's `TellWorker`).
pub trait Tell: Send + Sync {
    fn tell<'a>(
        &'a self,
        port: &'a Port,
        mobile: &'a Mobile,
        ws: &'a Workspace,
        pane: &'a Pane,
        text: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>>;
}

/// The person's words for a worker sent back from the phone: through the
/// queue, which its Claude Code session takes once it is ready, its project
/// woken for it; typed as `remote_send` does where the mod does not run
/// (`toWorker`).
pub struct ToWorker;

impl Tell for ToWorker {
    fn tell<'a>(
        &'a self,
        port: &'a Port,
        mobile: &'a Mobile,
        ws: &'a Workspace,
        pane: &'a Pane,
        text: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            if !queues(port) {
                let input = Input {
                    text: Some(text.to_string()),
                    ..Input::default()
                };
                return type_into(port, mobile, ws, pane, input).await.map(|_| ());
            }
            port.enqueue(&pane.id, text, None).await?;
            wake(port, mobile, ws).await;
            Ok(())
        })
    }
}

/* ── The operations ───────────────────────────────────────────── */

/// `remote_send {pane, text?, keys?, submit?}`.
pub async fn remote_send(port: &Port, mobile: &Mobile, args: &Value) -> Result<RemoteSent, String> {
    let (ws, pane) = find_pane(&port.snapshot().state, args.get("pane"))?;
    let keys: Vec<String> = args
        .get("keys")
        .and_then(Value::as_array)
        .map(|keys| {
            keys.iter()
                .map(|key| match key {
                    Value::String(name) => name.to_lowercase(),
                    other => other.to_string().to_lowercase(),
                })
                .collect()
        })
        .unwrap_or_default();
    if let Some(bad) = keys.iter().find(|key| !REMOTE_KEYS.contains(&key.as_str())) {
        return Err(format!(
            "Unknown key \"{bad}\". One of: {}.",
            REMOTE_KEYS.join(", ")
        ));
    }
    let input = Input {
        text: args.get("text").and_then(Value::as_str).map(str::to_string),
        keys,
        submit: (args.get("submit") == Some(&Value::Bool(false))).then_some(false),
    };
    type_into(port, mobile, &ws, &pane, input).await
}

/// `remote_wake {workspace}`: wakes a project asleep; one awake is left as
/// it is.
pub async fn remote_wake(
    port: &Port,
    mobile: &Mobile,
    args: &Value,
) -> Result<RemoteWoken, String> {
    let snapshot = port.snapshot();
    let ws = find_workspace(&snapshot.state, args.get("workspace"))?;
    if !is_asleep(&snapshot, &ws.id) {
        return Ok(RemoteWoken { waking: false });
    }
    wake(port, mobile, &ws).await;
    Ok(RemoteWoken { waking: true })
}

/// Refuses, with nothing typed, unless the prompt open now is the one the
/// phone showed (`prompt`, from `remote_chat` or `remote_screen`): Enter
/// approves whatever is open, and the phone's look at the pane is a second or
/// more old — the person may have answered at the PC, Claude Code moved on to
/// its next call (MOB-02). A question is not a permission: Enter would pick
/// its highlighted option.
///
/// A pane the PC can name no prompt of (an agent other than Claude Code, a
/// session without a transcript or the mod) is answered as asked, as
/// before: the phone has nothing to name either, and showed only its screen.
async fn shown_prompt(
    port: &Port,
    mobile: &Mobile,
    ws: &Workspace,
    pane: &Pane,
    shown: Option<&str>,
) -> Result<(), String> {
    let changed = || {
        format!(
            "What \"{}\" asks has changed since the phone showed it: look again. Nothing was sent.",
            pane.name
        )
    };
    match pending_now(port, mobile, ws, pane).await? {
        Some(ChatPending::Question { .. }) => Err(format!(
            "\"{}\" waits on a question, not a permission: answer it in the chat. Nothing was sent.",
            pane.name
        )),
        Some(ChatPending::Permission { prompt, .. }) if shown == Some(prompt.as_str()) => Ok(()),
        Some(ChatPending::Permission { .. }) => Err(changed()),
        None if shown.is_some() => Err(changed()),
        None => Ok(()),
    }
}

/// `remote_permission {pane, allow, prompt?}`: Enter or Escape to a pane
/// that waits, on the prompt the phone showed (`permit_pane`).
pub async fn remote_permission(
    port: &Port,
    mobile: &Mobile,
    args: &Value,
) -> Result<super::contract::RemoteAnswered, String> {
    let Some(allow) = args.get("allow").and_then(Value::as_bool) else {
        return Err("Say whether to allow: \"allow\" is true or false.".into());
    };
    let (ws, pane) = find_pane(&port.snapshot().state, args.get("pane"))?;
    let shown = args.get("prompt").and_then(Value::as_str);
    permit_pane(port, mobile, &ws, &pane, allow, shown).await
}

/// Enter (`allow`) or Escape to a pane that waits, on the prompt shown
/// (`shown`, `ChatPending::Permission`'s `prompt`; see `shown_prompt`).
pub async fn permit_pane(
    port: &Port,
    mobile: &Mobile,
    ws: &Workspace,
    pane: &Pane,
    allow: bool,
    shown: Option<&str>,
) -> Result<super::contract::RemoteAnswered, String> {
    let snapshot = port.snapshot();
    if activity(&snapshot, &pane.id) != Some(AgentActivity::Waiting) {
        // A sleep stops the agents: no dialog outlives it, and Enter or
        // Escape typed while it resumes would reach the shell.
        if is_asleep(&snapshot, &ws.id) {
            wake(port, mobile, ws).await;
            return Err(format!(
                "\"{}\" was asleep, so \"{}\" no longer waits for a permission. It is waking up now; nothing was sent.",
                ws.name, pane.name
            ));
        }
        return Err(format!(
            "\"{}\" is not waiting for a permission: nothing was sent.",
            pane.name
        ));
    }
    shown_prompt(port, mobile, ws, pane, shown).await?;
    let key = if allow { "enter" } else { "escape" };
    let input = Input {
        keys: vec![key.into()],
        ..Input::default()
    };
    type_into(port, mobile, ws, pane, input).await?;
    Ok(super::contract::RemoteAnswered {
        answered: true,
        queued: None,
    })
}

/// `remote_manager {text}`: typed into the project manager's pane.
pub async fn remote_manager(
    port: &Port,
    mobile: &Mobile,
    args: &Value,
) -> Result<RemoteSent, String> {
    let text = args.get("text").and_then(Value::as_str).unwrap_or("");
    if text.trim().is_empty() {
        return Err("Nothing to send: give text.".into());
    }
    let snapshot = port.snapshot();
    let Some((ws, pane)) = manager_of(&snapshot.state) else {
        return Err(NO_MANAGER.into());
    };
    let input = Input {
        text: Some(text.to_string()),
        ..Input::default()
    };
    type_into(port, mobile, ws, pane, input).await
}

/// A line written without words (Hangar stopping is written by core itself)
/// gets them here, in the PC's language (`textOf`).
pub fn text_of(entry: &hangar_core::remote::ActivityEntry, locale: Locale) -> String {
    if !entry.text.is_empty() {
        return entry.text.clone();
    }
    match entry.kind.as_str() {
        "appStarted" => t(locale, Key::ActivityAppStarted).into_owned(),
        "appStopped" => t(locale, Key::ActivityAppStopped).into_owned(),
        other => other.to_string(),
    }
}

/// `remote_history {since?}`: what happened, from the log kept on disk
/// (activity_log.rs), newest first. The journals in memory forget with every
/// restart; the disk does not.
pub async fn remote_history(
    port: &Port,
    mobile: &Mobile,
    args: &Value,
) -> Result<RemoteHistory, String> {
    let from = args
        .get("since")
        .and_then(Value::as_f64)
        .filter(|since| *since > 0.0)
        .unwrap_or(0.0);
    let locale = port
        .snapshot()
        .state
        .settings
        .locale
        .unwrap_or_else(Locale::system);
    let mut entries: Vec<_> = mobile
        .sources()
        .history(from as u64)
        .await?
        .into_iter()
        .filter(|entry| entry.at as f64 > from)
        .map(|mut entry| {
            entry.text = text_of(&entry, locale);
            entry
        })
        .collect();
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.at));
    Ok(RemoteHistory { entries })
}

/// A folder as the Windows way compares it: either separator, any case, no
/// trailing one.
fn folder_key(path: &str) -> String {
    let mut key = String::new();
    let mut separator = false;
    for c in path.trim().chars() {
        if c == '/' || c == '\\' {
            if !separator {
                key.push('/');
            }
            separator = true;
        } else {
            key.push(c);
            separator = false;
        }
    }
    key.trim_end_matches('/').to_lowercase()
}

/// `remote_workspace {workspace? | folder?}`: a workspace by id, or by folder.
/// Not one of the phone's calls (core's `CALLS` leaves it out): the mobile
/// server asks it itself, for the folder an upload lands in.
pub fn remote_workspace(port: &Port, args: &Value) -> Result<RemoteWorkspace, String> {
    let id = text_arg(args, "workspace");
    let folder = text_arg(args, "folder");
    if id.is_empty() && folder.is_empty() {
        return Err("Name the workspace, by id or by folder.".into());
    }
    let snapshot = port.snapshot();
    let workspaces = &snapshot.state.workspaces;
    let found = if id.is_empty() {
        workspaces
            .iter()
            .find(|ws| folder_key(&ws.cwd) == folder_key(&folder))
    } else {
        workspaces.iter().find(|ws| ws.id == id)
    };
    let Some(ws) = found else {
        let which = if id.is_empty() {
            format!("at \"{folder}\"")
        } else {
            format!("with id \"{id}\"")
        };
        return Err(format!("No workspace {which} in Hangar."));
    };
    Ok(RemoteWorkspace {
        id: ws.id.clone(),
        name: ws.name.clone(),
        cwd: ws.cwd.clone(),
    })
}
