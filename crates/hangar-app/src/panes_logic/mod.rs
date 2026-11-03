//! P4: Pane logic and live stores.
//!
//! Ports lib/{agentState,paneInsight,bridge,activity,hibernate,hibernateStore,
//! useHibernation,agents,agentPane,context,shells,paneBranch,fileDrop}.ts and
//! strings/paneBranch.ts, with their tests. Read inventory-frontend.md
//! §4/1.5–1.7, 1.10, 1.13, §4/4 and crates/hangar-app/PACKAGES.md.
//!
//! # What this package is
//!
//! The state machines the terminal pane used to keep in React `useState`s and
//! module-level maps, as plain data with no clock and no model inside:
//!
//! | file | ports | holds |
//! | --- | --- | --- |
//! | `agent_state` | agentState.ts | published activity, plan limits, overlap marks |
//! | `pane_insight` | paneInsight.ts | model, context, cost, events, edited files |
//! | `pane_agent` | TerminalPane.tsx (state half) | one machine per mounted pane; the reducer of the mod's reports |
//! | `bridge_events` | bridge.ts, PaneStatus.ts | queue carried over a restart, text helpers |
//! | `activity` | activity.ts | output bursts and silences |
//! | `hibernate` | hibernate.ts | the verdicts: may this workspace sleep? |
//! | `hibernation` | hibernateStore.ts, useHibernation.ts | rest data, asleep, the sweep |
//! | `agents` | agents.ts | launch commands, resume, the trust question |
//! | `session_watch` | agents.ts (`watchForSession`) | following a pane's session |
//! | `agent_pane` | agentPane.ts | which pane signs a board task |
//! | `context_gauge` | context.ts | the gauge and token counts |
//! | `shells` | shells.ts | which shell a pane runs |
//! | `pane_branch` | paneBranch.ts | the branch cache |
//! | `file_drop` | fileDrop.ts | paths typed for a dropped file |
//!
//! Every moment is an `EpochMs` handed in as `now`; the only clock read in
//! this directory is `wall_clock_ms`, at the edge, where `update`,
//! `on_event` and `on_intent` take the instant of a message.
//!
//! # What other packages read
//!
//! `Model.live.panes_logic` (`Live`): activity (`pane_activity`,
//! `workspace_activity`, `agents_in`, `plan_limits`, `conflict`), insight
//! (`insight`), the shell's life (`pane_status`, `state_glyph_state`),
//! attention (`needs_attention`), hibernation (`is_asleep`, `asleep`), the
//! branch (`pane_branch`). The shell feeds the snapshot from `activity_map`
//! and `asleep_ids`.
//!
//! # What the terminal's package tells it (P5)
//!
//! Through `Message::PanesLogic(Msg::…)`: `Mounted` when a pane's terminal
//! exists (before the PTY spawns, so no report of the mod goes unheard),
//! `Status` as the shell starts, runs and ends, `Output` for each chunk
//! written to the terminal, `Bell`, `Notification` (OSC 9 and 777, read with
//! `osc9_notification` and `osc777_notification`), `Key`, `Typed` (the person
//! used the workspace), `WatchSession` once the agent is launched,
//! `Unmounted` when the terminal goes. Everything that comes from core
//! (`bridge:event`, `bridge:queue`, the shell's end) arrives through
//! `on_event` and needs nothing from the terminal's package.
//!
//! What a pane reports is worded and decided here: the activity, the badge,
//! the notification (an `Intent::Notify`), the history, the context gauge
//! (the mod's, or the transcript's polled here), the session to store with
//! the pane (`StoreAction::UpdatePane`), the sleep.

pub mod activity;
pub mod agent_pane;
pub mod agent_state;
pub mod agents;
pub mod bridge_events;
pub mod context_gauge;
pub mod file_drop;
pub mod hibernate;
pub mod hibernation;
pub mod pane_agent;
pub mod pane_branch;
pub mod pane_insight;
pub mod session_watch;
pub mod shells;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hangar_core::git::RepoInfo;
use hangar_core::sessions::AgentSession;
use hangar_i18n::{Key, Translator, args};
use iced::{Subscription, Task, window};

// What other packages name at `panes_logic::*`; several have no user until the
// packages land.
#[allow(unused_imports)]
pub use self::agent_pane::{PaneTarget, find_agent_pane};
#[allow(unused_imports)]
pub use self::agent_state::{OVERLAP_SHOWN_MS, activity_at_hello};
#[allow(unused_imports)]
pub use self::bridge_events::{
    carry_key, file_base_name, first_line, newly_queued, session_id_label, short_session_id,
};
#[allow(unused_imports)]
pub use self::context_gauge::{CONTEXT_GAUGE_CONFIG, compute_context_gauge, format_tokens};
#[allow(unused_imports)]
pub use self::hibernate::{
    Awake, Blocker, HIBERNATE_DELAYS, HIBERNATE_TICK_MS, Verdict, sleep_refusal,
};
#[allow(unused_imports)]
pub use self::hibernation::Asleep;
#[allow(unused_imports)]
pub use self::pane_agent::{
    Ambient, Effect, PaneAgent, PaneInfo, osc9_notification, osc777_notification,
};
#[allow(unused_imports)]
pub use self::pane_branch::PaneBranch;
#[allow(unused_imports)]
pub use self::pane_insight::{MAX_EDITED_FILES, MAX_EVENTS, pane_event_text, queued_by};
#[allow(unused_imports)]
pub use crate::state::live::EpochMs;

use self::agent_state::AgentStates;
use self::agents::{Claims, is_resumable};
use self::bridge_events::CarriedQueues;
use self::hibernation::{
    ForcedOutcome, Hibernation, Reading, SleepEnv, SleepMode, Slept, Step, begin, finish, prune,
    put_to_sleep, settle_forced,
};
use self::pane_branch::{
    BranchCache, Lookup, PANE_BRANCH_FOCUS_MS, PANE_BRANCH_REFRESH_MS, PANE_BRANCH_TTL_MS,
    pane_branch_of,
};
use self::pane_insight::Insights;
use self::session_watch::SessionWatch;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::{Notice, Notification, Region, View, WindowKind};
use crate::state::live::{
    AgentActivity, AgentRoster, BridgeEvent, GlyphState, PaneActivity, PaneInsight, PaneStatus,
    PlanLimits, WorkspaceActivity,
};
use crate::state::types::{AgentId, AppState, Pane, PaneId, Workspace, WsId};
use crate::state::{Model, Patch, Snapshot, StoreAction};

/// The package's UI state; lives in `App.pkg.panes_logic`. What is in flight,
/// not what anybody reads.
#[derive(Debug, Default)]
pub struct State {
    /// Workspaces whose shells are being asked about (a sweep or a sleep by
    /// hand): the system's answer is awaited, and nobody asks twice.
    probing: HashSet<(WsId, bool)>,
    /// When the transcripts' context was last read, and the branches asked.
    last_context_poll: EpochMs,
    last_branch_poll: EpochMs,
}

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.panes_logic`.
///
/// Activity per pane, plan limits, insight per pane, hibernation (`touched`,
/// `rest`, `asleep`), branch cache: the shared value types are in
/// `state::live`. The terminal's package writes through `Msg`, never into it.
#[derive(Debug, Default)]
pub struct Live {
    agents: AgentStates,
    insights: Insights,
    /// One machine per mounted pane.
    machines: HashMap<PaneId, PaneAgent>,
    /// The shell's life, per mounted pane (`status` of the header).
    status: HashMap<PaneId, PaneStatus>,
    hibernation: Hibernation,
    carried: CarriedQueues,
    claims: Claims,
    branches: BranchCache,
    /// The transcript hunt of every pane whose agent can resume.
    watches: HashMap<PaneId, SessionWatch>,
    /// The window of the app that has the system's focus, if one does.
    focused_window: Option<window::Id>,
    /// The workspaces the main window showed, the latest first.
    recent: Vec<WsId>,
    /// The workspace it showed when `sync_active` last looked.
    last_active: Option<WsId>,
}

/// The package's messages, routed by the shell as `Message::PanesLogic(Msg)`.
///
/// The first group is what the terminal's package (P5) reports about a pane
/// (see the module documentation); the second is this package's own timers
/// and answers.
#[derive(Debug, Clone)]
pub enum Msg {
    // ----- from the terminal's package
    /// The pane's terminal exists. Before the PTY is spawned, so that no
    /// report of the mod is lost; the queue a restart carried over is
    /// handed back to core here.
    Mounted {
        pane: PaneId,
    },
    /// The terminal is gone: closed, restarted under a new id, or taken
    /// over by another window (`handing_over`: its state stays).
    Unmounted {
        pane: PaneId,
        handing_over: bool,
    },
    /// The pane's shell: starting, running, exited.
    Status {
        pane: PaneId,
        status: PaneStatus,
    },
    /// A chunk of output was written to the terminal.
    Output {
        pane: PaneId,
    },
    Bell {
        pane: PaneId,
    },
    /// OSC 9 or OSC 777 raised a notification; `message` is what
    /// `osc9_notification` or `osc777_notification` read (empty: say that the
    /// agent finished).
    Notification {
        pane: PaneId,
        message: String,
    },
    /// A key was pressed in the pane, not the terminal answering its
    /// program.
    Key {
        pane: PaneId,
    },
    /// Typing, clicking, a broadcast or a saved command: the workspace was
    /// used. Counted for the hibernation delay.
    Typed {
        workspace: WsId,
    },
    /// The agent is launched in the pane: follow its session. `known` is
    /// what `agents::known_session_ids` read before the launch.
    WatchSession {
        pane: PaneId,
        agent: AgentId,
        cwd: String,
        known: HashSet<String>,
    },

    // ----- the package's own
    /// A window of the app gained or lost the system's focus.
    WindowFocus {
        window: window::Id,
        focused: bool,
    },
    /// Every `activity::TICK_MS` while a pane is being watched.
    Tick,
    /// Every 1.5 s while a pane is mounted.
    Slow,
    HibernateTick,
    /// The system answered which of a workspace's shells run something.
    Probed {
        workspace: Box<Workspace>,
        mode: SleepMode,
        asked: Vec<PaneId>,
        answer: Result<Vec<PaneId>, String>,
    },
    /// The transcripts of an agent, listed for a round of the session hunt.
    Sessions {
        pane: PaneId,
        sessions: Option<Vec<AgentSession>>,
    },
    /// The transcript's reading of a pane's context.
    Context {
        pane: PaneId,
        usage: Option<hangar_core::context::ContextUsage>,
    },
    /// git's answer for a folder.
    Branch {
        folder: String,
        info: RepoInfo,
    },
}

/// Milliseconds since the Unix epoch, now. The one clock of this directory,
/// read where a message arrives and handed down as `now`.
pub fn wall_clock_ms() -> EpochMs {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as EpochMs)
}

// ----- what other packages read

impl Live {
    /// A pane's published activity (`getPaneActivity`): only the panes that
    /// report one, which is an agent's, or a shell's with a Claude Code in it
    /// that loaded the mod.
    pub fn pane_activity(&self, pane: &str) -> Option<&PaneActivity> {
        self.agents.get(pane)
    }

    /// The pane's activity, `Idle` for one that reports none.
    pub fn activity_or_idle(&self, pane: &str) -> AgentActivity {
        self.agents
            .get(pane)
            .map_or(AgentActivity::Idle, |entry| entry.activity)
    }

    /// Every published activity: what the snapshot carries for the engines.
    pub fn activity_map(&self) -> &HashMap<PaneId, PaneActivity> {
        self.agents.all()
    }

    /// The pane's agent is waiting for an answer, rather than at work or done.
    pub fn is_waiting(&self, pane: &str) -> bool {
        self.agents.is_waiting(pane)
    }

    /// How many of a workspace's agents are working, waiting, or done.
    pub fn workspace_activity(&self, workspace_id: &str) -> WorkspaceActivity {
        self.agents.summarize(workspace_id)
    }

    /// Which panes of these workspaces are working, waiting, or done.
    pub fn agents_in(&self, workspace_ids: &[WsId]) -> AgentRoster {
        self.agents.roster(workspace_ids)
    }

    /// How many of these panes have a Claude Code that loaded the mod.
    pub fn bridged_count<'a>(&self, pane_ids: impl IntoIterator<Item = &'a PaneId>) -> usize {
        self.agents.bridged_count(pane_ids)
    }

    /// The plan's rate-limit windows, from whichever session reported last.
    pub fn plan_limits(&self) -> Option<&PlanLimits> {
        self.agents.plan_limits()
    }

    /// The name of the pane that just edited a file this one had edited.
    pub fn conflict(&self, pane: &str) -> Option<&str> {
        self.agents.conflict(pane)
    }

    /// What the pane knows of its agent: the empty insight for a pane that
    /// never reported.
    pub fn insight(&self, pane: &str) -> &PaneInsight {
        self.insights.get(pane)
    }

    /// The shell of a mounted pane: starting, running, exited.
    pub fn pane_status(&self, pane: &str) -> Option<PaneStatus> {
        self.status.get(pane).copied()
    }

    /// The pane handed control back while the person was looking elsewhere:
    /// its outline, and a bell on a shell's header.
    pub fn needs_attention(&self, pane: &str) -> bool {
        self.machines
            .get(pane)
            .is_some_and(|machine| machine.attention)
    }

    /// Whether a workspace was put to sleep and not woken since.
    pub fn is_asleep(&self, workspace_id: &str) -> bool {
        self.hibernation.is_asleep(workspace_id)
    }

    /// Every workspace asleep, with since when and how many agents stopped.
    pub fn asleep(&self) -> &HashMap<WsId, Asleep> {
        self.hibernation.asleep()
    }

    /// The ids of the workspaces asleep: what the snapshot carries.
    pub fn asleep_ids(&self) -> HashSet<WsId> {
        self.hibernation.asleep().keys().cloned().collect()
    }

    /// The branch the header of a pane on this folder shows.
    pub fn pane_branch(&self, folder: &str) -> Option<&PaneBranch> {
        self.branches.peek(folder)
    }

    /// Whether a pane's terminal is mounted.
    pub fn is_pane_mounted(&self, pane: &str) -> bool {
        self.machines.contains_key(pane)
    }

    // ----- the few doors the terminal's package writes through

    /// A session id handed to a pane for this app run, so that no other pane
    /// latches onto it (`claim`). The launch of a resumed session claims it.
    pub fn claim_session(&mut self, session_id: &str) {
        self.claims.claim(session_id);
    }

    pub fn release_session(&mut self, session_id: Option<&str>) {
        self.claims.release(session_id);
    }

    /// Whether the pane's next launch must resume whatever the auto-resume
    /// setting says (a workspace was put to sleep under it); `take` uses the
    /// promise up. Called by the launch it was meant for.
    pub fn must_resume(&mut self, pane: &str, take: bool) -> bool {
        self.hibernation.must_resume(pane, take)
    }

    /// A pane restarted under a new id keeps what its old id was owed.
    pub fn carry_resume(&mut self, from: &str, to: &str) {
        self.hibernation.carry_resume(from, to);
    }

    /// Panes put to sleep that resume on their next launch (`resumeOnWake`),
    /// for a workspace that moves to a window while asleep.
    pub fn resume_on_wake<'a>(&mut self, panes: impl IntoIterator<Item = &'a str>) {
        self.hibernation.resume_on_wake(panes);
    }

    /// Awake again: a workspace sent to a window of its own or opened by an
    /// agent. Whether it was asleep.
    pub fn mark_awake(&mut self, workspace_id: &str) -> bool {
        self.hibernation.mark_awake(workspace_id)
    }
}

/// The glyph a pane shows: its shell's life until it runs, then its agent's
/// activity (`paneGlyphState`). Used by everything that draws a glyph.
///
/// A pane that is not mounted has no life of its own to show, and rests as
/// idle (a workspace never opened, or asleep); so does one that reports no
/// activity, a plain shell.
pub fn state_glyph_state(m: &Model, pane: &PaneId) -> GlyphState {
    let live = &m.live.panes_logic;
    match live.pane_status(pane) {
        Some(PaneStatus::Starting) => GlyphState::Starting,
        Some(PaneStatus::Exited) => GlyphState::Exited,
        Some(PaneStatus::Running) | None => match live.activity_or_idle(pane) {
            AgentActivity::Working => GlyphState::Working,
            AgentActivity::Waiting => GlyphState::Waiting,
            AgentActivity::Yours => GlyphState::Yours,
            AgentActivity::Idle => GlyphState::Idle,
        },
    }
}

// ----- the model, read for the machines

/// A pane and its workspace, from the persisted state.
fn locate(m: &Model, pane: &str) -> Option<(PaneInfo, Pane)> {
    for ws in &m.state().workspaces {
        if let Some(found) = ws.panes.iter().find(|p| p.id == pane) {
            return Some((info_of(ws, found), found.clone()));
        }
    }
    None
}

fn info_of(ws: &Workspace, pane: &Pane) -> PaneInfo {
    PaneInfo {
        id: pane.id.clone(),
        workspace_id: ws.id.clone(),
        name: pane.name.clone(),
        agent: pane.agent,
        session_id: pane.session_id.clone(),
    }
}

/// The folder a pane's shell opens in.
fn folder_of(ws: &Workspace, pane: &Pane) -> String {
    pane.cwd
        .as_deref()
        .filter(|cwd| !cwd.is_empty())
        .unwrap_or(&ws.cwd)
        .to_string()
}

/// The window that draws a workspace's panes: its own when it is in one, the
/// main window otherwise.
fn host_window(m: &Model, ws: &Workspace) -> Option<window::Id> {
    m.live.windows.iter().find_map(|(id, kind)| match kind {
        WindowKind::Workspace(open) if ws.detached == Some(true) && *open == ws.id => Some(*id),
        WindowKind::Main if ws.detached != Some(true) => Some(*id),
        _ => None,
    })
}

/// The pane is the focused one of its workspace and the workspace is shown
/// (`focused && visible` of TerminalPane): in the main window the active
/// workspace of the terminals view, or a workspace window's own.
fn attended(m: &Model, ws: &Workspace, pane: &str) -> bool {
    let focused =
        m.ui.focus_by_workspace
            .get(&ws.id)
            .is_some_and(|id| id == pane);
    let shown = if ws.detached == Some(true) {
        host_window(m, ws).is_some()
    } else {
        m.state().active_workspace_id.as_deref() == Some(ws.id.as_str())
            && m.ui.view == View::Terminals
    };
    focused && shown
}

/// What a decision about a pane depends on, read from the window.
fn ambient(m: &Model, ws: &Workspace, pane: &str, now: EpochMs) -> Ambient {
    let settings = m.settings();
    let window_focused =
        host_window(m, ws).is_some_and(|host| m.live.panes_logic.focused_window == Some(host));
    Ambient {
        now,
        looking: window_focused && attended(m, ws, pane),
        window_focused,
        notify_on_idle: settings.notify_on_idle,
        notify_only_when_away: settings.notify_only_when_away,
        t: Translator::new(m.locale()),
    }
}

/// A pane's machine inputs, or `None` when the pane is not in the state.
fn inputs(m: &Model, pane: &str, now: EpochMs) -> Option<(PaneInfo, Ambient)> {
    let (ws, found) = m
        .state()
        .workspaces
        .iter()
        .find_map(|ws| ws.panes.iter().find(|p| p.id == pane).map(|p| (ws, p)))?;
    Some((info_of(ws, found), ambient(m, ws, pane, now)))
}

fn bump(m: &mut Model) {
    m.live.version += 1;
}

/// What the machines asked the window to do.
fn run_effects(m: &mut Model, info: &PaneInfo, effects: Vec<Effect>) -> Task<Message> {
    let mut tasks = Vec::new();
    for effect in effects {
        match effect {
            Effect::Notify { title, body } => {
                tasks.push(Task::done(Message::Intent(Intent::Notify(Notification {
                    title,
                    body,
                    target: Some((info.workspace_id.clone(), info.id.clone())),
                }))));
            }
            Effect::CaptureSession(session_id) => {
                m.apply(StoreAction::UpdatePane {
                    ws: info.workspace_id.clone(),
                    pane: info.id.clone(),
                    patch: Patch::new(move |pane: &mut Pane| {
                        pane.session_id = Some(session_id.clone());
                    }),
                });
            }
        }
    }
    Task::batch(tasks)
}

// ----- the active workspace, and who is looking

/// The workspace on screen counts as used when it arrives and when it leaves,
/// so the delay of the one the person looks away from starts as they leave
/// it; and the main window remembers what it showed (`recent`), where a
/// sleep steps back to. Panes that become the focused one of a shown
/// workspace are looked at, which takes their hand-back.
fn sync_active(m: &mut Model, now: EpochMs) {
    let active = m.state().active_workspace_id.clone();
    let live = &mut m.live.panes_logic;
    if active != live.last_active {
        if let Some(leaving) = live.last_active.take() {
            live.hibernation.touch_workspace(&leaving, now);
        }
        if let Some(arriving) = &active {
            live.hibernation.touch_workspace(arriving, now);
            live.recent.retain(|id| id != arriving);
            live.recent.insert(0, arriving.clone());
        }
        live.last_active = active;
    }
    // Mounted again: awake, and as good as used just now.
    let opened = m.ui.opened_ids.clone();
    m.live.panes_logic.hibernation.wake_mounted(&opened, now);
}

/// Looking at a pane clears its badge. `window_back`: the window of these
/// panes just came to the front (only those it draws), with the pane
/// already the one looked at.
fn refresh_attention(m: &mut Model, window_back: Option<window::Id>, now: EpochMs) {
    let mut looks: Vec<(PaneInfo, bool, bool)> = Vec::new();
    for ws in &m.state().workspaces {
        let back = window_back.is_some_and(|window| host_window(m, ws) == Some(window));
        for pane in &ws.panes {
            if m.live.panes_logic.is_pane_mounted(&pane.id) {
                looks.push((info_of(ws, pane), attended(m, ws, &pane.id), back));
            }
        }
    }
    let mut changed = false;
    for (info, attended, back) in looks {
        let before = m.live.panes_logic.pane_state_mark(&info.id);
        m.live.panes_logic.attend(&info, attended, back, now);
        changed |= before != m.live.panes_logic.pane_state_mark(&info.id);
    }
    if changed {
        bump(m);
    }
}

impl Live {
    /// What `attend` can change, to tell whether it did.
    fn pane_state_mark(&self, pane: &str) -> Option<(AgentActivity, bool)> {
        self.machines
            .get(pane)
            .map(|machine| (machine.activity, machine.attention))
    }

    /// What a tick of the watchers can change in what is read, every pane's.
    fn state_marks(&self) -> Vec<(PaneId, AgentActivity, bool)> {
        let mut marks: Vec<_> = self
            .machines
            .iter()
            .map(|(id, machine)| (id.clone(), machine.activity, machine.attention))
            .collect();
        marks.sort_by(|a, b| a.0.cmp(&b.0));
        marks
    }
}

// ----- hibernation, against the live model

/// The model as a sleep reads and changes it: borrowed apart from the
/// shell's model so the rules can run on it, and the things a sleep asks the
/// window to do (kills, showing another workspace) gathered for after.
struct LiveEnv<'a> {
    state: &'a AppState,
    active: Option<String>,
    opened_ids: &'a mut Vec<WsId>,
    recent: &'a [WsId],
    store: &'a mut Hibernation,
    insights: &'a Insights,
    snapshot: Arc<Snapshot>,
    kills: Vec<PaneId>,
    shown: Vec<WsId>,
}

impl SleepEnv for LiveEnv<'_> {
    fn state(&self) -> &AppState {
        self.state
    }
    fn active(&self) -> Option<&str> {
        self.active.as_deref()
    }
    fn opened_ids(&self) -> &[WsId] {
        self.opened_ids
    }
    fn recent(&self) -> &[WsId] {
        self.recent
    }
    fn store(&self) -> &Hibernation {
        self.store
    }
    fn store_mut(&mut self) -> &mut Hibernation {
        self.store
    }
    fn queued(&self, pane_id: &str) -> usize {
        self.insights.get(pane_id).queue.len()
    }
    fn delivering(&self, workspace_id: &str) -> bool {
        self.snapshot
            .delivering
            .get(workspace_id)
            .is_some_and(|runs| !runs.is_empty())
    }
    fn unmount(&mut self, workspace_id: &str) {
        self.opened_ids.retain(|id| id != workspace_id);
    }
    fn show(&mut self, workspace_id: &str) {
        self.active = Some(workspace_id.to_string());
        self.shown.push(workspace_id.to_string());
    }
    fn kill_tree(&mut self, pane_id: &str) {
        self.kills.push(pane_id.to_string());
    }
}

/// Runs `body` on the model as a sleep environment; what it asked the window
/// to do comes back as a task.
fn with_env<T>(
    m: &mut Model,
    sv: &Services,
    body: impl FnOnce(&mut LiveEnv) -> T,
) -> (T, Task<Message>) {
    let (result, kills, shown) = env_run(m, sv.snapshot(), body);
    (result, after_sleep(sv, kills, shown))
}

/// `with_env` without the window: the result, the shells to end and the
/// workspaces the main window stepped aside to.
fn env_run<T>(
    m: &mut Model,
    snapshot: Arc<Snapshot>,
    body: impl FnOnce(&mut LiveEnv) -> T,
) -> (T, Vec<PaneId>, Vec<WsId>) {
    let state = m.state_arc();
    let active = state.active_workspace_id.clone();
    let live = &mut m.live.panes_logic;
    let mut env = LiveEnv {
        state: &state,
        active,
        opened_ids: &mut m.ui.opened_ids,
        recent: &live.recent,
        store: &mut live.hibernation,
        insights: &live.insights,
        snapshot,
        kills: Vec::new(),
        shown: Vec::new(),
    };
    let result = body(&mut env);
    (result, env.kills, env.shown)
}

/// What a sleep leaves to do: end the shells, show the workspace the window
/// stepped aside to.
fn after_sleep(sv: &Services, kills: Vec<PaneId>, shown: Vec<WsId>) -> Task<Message> {
    let mut tasks: Vec<Task<Message>> = shown
        .into_iter()
        .map(|ws| Task::done(Message::Intent(Intent::ActivateWorkspace(ws))))
        .collect();
    if !kills.is_empty() {
        let manager = sv.core().pty().clone();
        tasks.push(Task::perform(
            async move {
                // The whole tree under each shell, so the agents' helpers go
                // too; the unmount ends the shells again, which costs nothing.
                let _ = tokio::task::spawn_blocking(move || {
                    for id in kills {
                        let _ = hangar_core::pty::pty_kill(&manager, id, Some(true));
                    }
                })
                .await;
            },
            |()| Message::Noop,
        ));
    }
    Task::batch(tasks)
}

fn delay_ms(m: &Model) -> u64 {
    // At least a minute, whatever a hand-edited state.json says.
    let minutes = m.settings().hibernate_after_min;
    let minutes = if minutes.is_finite() {
        minutes.max(1.0)
    } else {
        1.0
    };
    (minutes * 60_000.0) as u64
}

/// The notice for workspaces just put to sleep.
fn announce_slept(m: &mut Model, slept: &[Slept]) -> Task<Message> {
    if slept.is_empty() {
        return Task::none();
    }
    let workspaces: Vec<Workspace> = slept.iter().map(|s| s.workspace.clone()).collect();
    let agents = slept.iter().map(|s| s.agents).sum();
    let text = hibernation::announcement(Translator::new(m.locale()), &workspaces, agents);
    crate::shell::notice::show(m, Notice::Asleep(text))
}

/// What the notice says when a sleep asked for by hand finds the workspace
/// must stay awake. Asleep already is what was asked, so it says nothing.
fn refuse_forced(m: &mut Model, workspace: &Workspace, verdict: &Awake) -> Task<Message> {
    if verdict.reason == Blocker::Asleep {
        return Task::none();
    }
    let t = Translator::new(m.locale());
    let reason = {
        let live = &m.live.panes_logic;
        sleep_refusal(t, verdict, &workspace.panes, &|pane| live.is_waiting(pane))
    };
    let text = t
        .t_with(
            Key::SleepRefused,
            &args!("name" => workspace.name.as_str(), "reason" => reason.as_str()),
        )
        .into_owned();
    crate::shell::notice::show(m, Notice::Problem(text))
}

/// One decision, ready to be acted on, or the probe it waits for.
fn decide_next(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    workspace_id: &str,
    mode: SleepMode,
    now: EpochMs,
) -> Task<Message> {
    // The sweep and a sleep asked for by hand each ask once; the second of
    // the same kind finds the first on its way.
    let key = (workspace_id.to_string(), mode == SleepMode::Forced);
    if st.probing.contains(&key) {
        return Task::none();
    }
    let (step, io) = with_env(m, sv, |env| begin(env, workspace_id, mode, now));
    match step {
        Step::Done(Some(reading)) => Task::batch([io, act_on(m, sv, reading, mode, now)]),
        Step::Done(None) => io,
        Step::Probe {
            workspace,
            pane_ids,
        } => {
            st.probing.insert(key);
            let manager = sv.core().pty().clone();
            let asked = pane_ids.clone();
            Task::batch([
                io,
                Task::perform(
                    hangar_core::pty::pty_has_children(manager, pane_ids),
                    move |answer| {
                        Message::PanesLogic(Msg::Probed {
                            workspace: Box::new(workspace.clone()),
                            mode,
                            asked: asked.clone(),
                            answer,
                        })
                    },
                ),
            ])
        }
    }
}

/// What a decision does: the sweep puts a workspace to sleep when the rules
/// say so, a sleep asked for refuses or goes through.
fn act_on(
    m: &mut Model,
    sv: &Services,
    reading: Reading,
    mode: SleepMode,
    now: EpochMs,
) -> Task<Message> {
    match mode {
        SleepMode::Idle { .. } => {
            let Reading {
                workspace,
                verdict: Verdict::Sleep { agents },
            } = reading
            else {
                return Task::none();
            };
            let ((), io) = with_env(m, sv, |env| put_to_sleep(env, &workspace, agents, now));
            bump(m);
            Task::batch([io, announce_slept(m, &[Slept { workspace, agents }])])
        }
        SleepMode::Forced => {
            let (outcome, io) = with_env(m, sv, |env| settle_forced(env, reading, now));
            bump(m);
            let said = match outcome {
                ForcedOutcome::Slept {
                    workspace, agents, ..
                } => announce_slept(m, &[Slept { workspace, agents }]),
                ForcedOutcome::Refused { workspace, verdict } => {
                    refuse_forced(m, &workspace, &verdict)
                }
            };
            Task::batch([io, said])
        }
    }
}

// ----- the package

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    let now = wall_clock_ms();
    match msg {
        Msg::Mounted { pane } => {
            let Some((ws, found)) = m.state().workspaces.iter().find_map(|ws| {
                ws.panes
                    .iter()
                    .find(|p| p.id == pane)
                    .map(|p| (ws.clone(), p.clone()))
            }) else {
                return Task::none();
            };
            let live = &mut m.live.panes_logic;
            live.mount_pane(&pane, &ws.id, &found.name, now);
            // A restart gave this pane a new id: the prompts still queued for
            // the old one come along.
            for carried in live.carried.take(&carry_key(&ws.id, &found.name), now) {
                hangar_core::bridge::bridge_enqueue(
                    sv.core().host(),
                    pane.clone(),
                    carried.text,
                    carried.from,
                );
            }
            bump(m);
            // A pane asks once on mount.
            ask_branch(m, &folder_of(&ws, &found), now, PANE_BRANCH_TTL_MS)
        }
        Msg::Unmounted { pane, handing_over } => {
            let live = &m.live.panes_logic;
            let carry = live.machines.get(&pane).and_then(|machine| {
                // Restarted under a new id, its prompts still queued come
                // along; closed for real, they go with it.
                let successor = m.workspace(&machine.workspace_id).is_some_and(|ws| {
                    ws.panes
                        .iter()
                        .any(|p| p.name == machine.name && p.id != pane)
                });
                successor.then(|| {
                    (
                        carry_key(&machine.workspace_id, &machine.name),
                        live.insights.get(&pane).queue.clone(),
                    )
                })
            });
            let live = &mut m.live.panes_logic;
            if let Some((key, queue)) = carry {
                live.carried.stash(&key, queue, now);
            }
            live.unmount_pane(&pane, handing_over);
            bump(m);
            Task::none()
        }
        Msg::Status { pane, status } => {
            let live = &mut m.live.panes_logic;
            if live.machines.contains_key(&pane) {
                live.status.insert(pane, status);
                bump(m);
            }
            Task::none()
        }
        Msg::Output { pane } => {
            // The hot path: a write per chunk and nothing to redraw for it.
            m.live.panes_logic.note_output(&pane, now);
            Task::none()
        }
        Msg::Bell { pane } => with_pane(m, &pane, now, |live, info, amb| live.bell(info, amb)),
        Msg::Notification { pane, message } => with_pane(m, &pane, now, |live, info, amb| {
            live.terminal_notification(info, &message, amb)
        }),
        Msg::Key { pane } => {
            if let Some((info, _)) = inputs(m, &pane, now) {
                m.live.panes_logic.key_pressed(&info, now);
                bump(m);
            }
            Task::none()
        }
        Msg::Typed { workspace } => {
            m.live
                .panes_logic
                .hibernation
                .touch_workspace(&workspace, now);
            Task::none()
        }
        Msg::WatchSession {
            pane,
            agent,
            cwd,
            known,
        } => {
            if is_resumable(agent) {
                m.live
                    .panes_logic
                    .watches
                    .insert(pane, SessionWatch::new(agent, cwd, known, now));
            }
            Task::none()
        }
        Msg::WindowFocus { window, focused } => {
            let live = &mut m.live.panes_logic;
            if focused {
                live.focused_window = Some(window);
            } else if live.focused_window == Some(window) {
                live.focused_window = None;
            }
            if focused {
                refresh_attention(m, Some(window), now);
            }
            Task::none()
        }
        Msg::Tick => tick(m, sv, now),
        Msg::Slow => slow(st, m, sv, now),
        Msg::HibernateTick => hibernate_tick(st, m, sv, now),
        Msg::Probed {
            workspace,
            mode,
            asked,
            answer,
        } => {
            st.probing
                .remove(&(workspace.id.clone(), mode == SleepMode::Forced));
            let (reading, io) = with_env(m, sv, |env| {
                finish(env, &workspace, mode, now, &asked, answer)
            });
            let acted = match reading {
                Some(reading) => act_on(m, sv, reading, mode, now),
                None => Task::none(),
            };
            Task::batch([io, acted])
        }
        Msg::Sessions { pane, sessions } => {
            let live = &mut m.live.panes_logic;
            let Some(found) = live
                .watches
                .get_mut(&pane)
                .and_then(|watch| watch.round_done(sessions.as_deref(), now, &mut live.claims))
            else {
                return Task::none();
            };
            // The mod names the session outright; the transcript hunt is for
            // the agents that have none.
            let bridged = live
                .machines
                .get(&pane)
                .is_none_or(|machine| machine.bridged);
            match (bridged, inputs(m, &pane, now)) {
                (false, Some((info, _))) => {
                    run_effects(m, &info, vec![Effect::CaptureSession(found)])
                }
                _ => Task::none(),
            }
        }
        Msg::Context { pane, usage } => {
            let agent = locate(m, &pane).map(|(info, _)| info.agent);
            let gauge = agent
                .zip(usage)
                .and_then(|(agent, usage)| compute_context_gauge(agent, &usage));
            m.live.panes_logic.set_transcript_context(&pane, gauge);
            bump(m);
            Task::none()
        }
        Msg::Branch { folder, info } => {
            let branch = pane_branch_of(Some(&info));
            if m.live.panes_logic.branches.answered(&folder, branch, now) {
                bump(m);
            }
            Task::none()
        }
    }
}

/// Gives a pane's inputs to a machine step and runs what it asks.
fn with_pane(
    m: &mut Model,
    pane: &str,
    now: EpochMs,
    step: impl FnOnce(&mut Live, &PaneInfo, &Ambient) -> Vec<Effect>,
) -> Task<Message> {
    let Some((info, amb)) = inputs(m, pane, now) else {
        return Task::none();
    };
    let effects = step(&mut m.live.panes_logic, &info, &amb);
    bump(m);
    run_effects(m, &info, effects)
}

/// The watchers' checks that came due, and the trust question to look for.
fn tick(m: &mut Model, sv: &Services, now: EpochMs) -> Task<Message> {
    let idle_ms = m.settings().notify_idle_ms;
    let panes: Vec<PaneId> = m.live.panes_logic.machines.keys().cloned().collect();
    let before = m.live.panes_logic.state_marks();
    let mut tasks = Vec::new();
    for pane in panes {
        let Some((info, amb)) = inputs(m, &pane, now) else {
            continue;
        };
        let mut effects = m.live.panes_logic.poll_pane(&info, idle_ms, &amb);
        if m.live.panes_logic.trust_look_due(&pane, now) {
            // The rows of the screen, read a moment after the last output.
            let screen = sv
                .terminals()
                .read_lines(&pane, TRUST_LOOK_LINES)
                .map(|lines| lines.join("\n"))
                .unwrap_or_default();
            effects.extend(
                m.live
                    .panes_logic
                    .look_for_trust_question(&info, &screen, &amb),
            );
        }
        if !effects.is_empty() {
            tasks.push(run_effects(m, &info, effects));
        }
    }
    // A burst of output ticks every quarter second; what the readers see
    // changes only when a pane starts working or hands back.
    if m.live.panes_logic.state_marks() != before {
        bump(m);
    }
    Task::batch(tasks)
}

/// How many lines of the screen the trust question is looked for in.
const TRUST_LOOK_LINES: usize = 60;

/// How often the transcripts' context is read.
const CONTEXT_POLL_MS: u64 = 5000;

/// What happens every second and a half while panes are mounted: the overlap
/// marks wear off, the session hunt takes a round, the transcripts' context
/// and the branches are read when it is time.
fn slow(st: &mut State, m: &mut Model, sv: &Services, now: EpochMs) -> Task<Message> {
    let mut tasks = Vec::new();
    if m.live.panes_logic.agents.expire(now) {
        bump(m);
    }

    // The transcript hunt.
    let due: Vec<PaneId> = m
        .live
        .panes_logic
        .watches
        .iter()
        .filter(|(_, watch)| watch.due(now))
        .map(|(pane, _)| pane.clone())
        .collect();
    for pane in due {
        let live = &mut m.live.panes_logic;
        let Some(watch) = live.watches.get_mut(&pane) else {
            continue;
        };
        if !watch.begin_round(now) {
            continue;
        }
        let (agent, cwd) = (watch.agent.as_str().to_string(), watch.cwd.clone());
        tasks.push(Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    hangar_core::sessions::list_sessions(agent, cwd)
                })
                .await
                .ok()
            },
            move |sessions| {
                Message::PanesLogic(Msg::Sessions {
                    pane: pane.clone(),
                    sessions,
                })
            },
        ));
    }

    // The transcripts' context: while the pane is shown, its shell alive and
    // the mod silent. A pane that goes hidden keeps its last honest reading.
    if now.saturating_sub(st.last_context_poll) >= CONTEXT_POLL_MS {
        st.last_context_poll = now;
        let mut reads = Vec::new();
        let mut clears = Vec::new();
        let live = &m.live.panes_logic;
        for ws in &m.state().workspaces {
            for pane in &ws.panes {
                let Some(machine) = live.machines.get(&pane.id) else {
                    continue;
                };
                if machine.bridged {
                    continue;
                }
                let session = pane.session_id.clone().filter(|id| !id.is_empty());
                match session {
                    Some(session) if is_resumable(pane.agent) => {
                        if attended_workspace(m, ws)
                            && live.pane_status(&pane.id) == Some(PaneStatus::Running)
                        {
                            reads.push((pane.id.clone(), pane.agent, session, folder_of(ws, pane)));
                        }
                    }
                    // Gemini and opencode keep no readable transcript, and
                    // before a session is captured there is nothing to read:
                    // no gauge beats a made-up zero.
                    _ => clears.push(pane.id.clone()),
                }
            }
        }
        for pane in clears {
            m.live.panes_logic.set_transcript_context(&pane, None);
        }
        for (pane, agent, session, cwd) in reads {
            tasks.push(Task::perform(
                hangar_core::context::context_usage(agent.as_str().to_string(), session, cwd),
                move |usage| {
                    Message::PanesLogic(Msg::Context {
                        pane: pane.clone(),
                        usage,
                    })
                },
            ));
        }
    }

    // The branches: a pane asks again every 45 s.
    if now.saturating_sub(st.last_branch_poll) >= PANE_BRANCH_REFRESH_MS {
        st.last_branch_poll = now;
        let folders: Vec<String> = m
            .state()
            .workspaces
            .iter()
            .flat_map(|ws| ws.panes.iter().map(move |pane| (ws, pane)))
            .filter(|(_, pane)| m.live.panes_logic.is_pane_mounted(&pane.id))
            .map(|(ws, pane)| folder_of(ws, pane))
            .collect();
        for folder in folders {
            tasks.push(ask_branch(m, &folder, now, PANE_BRANCH_TTL_MS));
        }
    }
    Task::batch(tasks)
}

/// Whether the workspace's panes are drawn at all (`visible` of TerminalPane
/// before the zoom).
fn attended_workspace(m: &Model, ws: &Workspace) -> bool {
    if ws.detached == Some(true) {
        host_window(m, ws).is_some()
    } else {
        m.state().active_workspace_id.as_deref() == Some(ws.id.as_str())
            && m.ui.view == View::Terminals
    }
}

/// Asks git for a folder's branch, unless the answer at hand is young enough
/// or someone is asking already.
fn ask_branch(m: &mut Model, folder: &str, now: EpochMs, max_age_ms: u64) -> Task<Message> {
    if folder.is_empty() {
        return Task::none();
    }
    if m.live.panes_logic.branches.lookup(folder, now, max_age_ms) != Lookup::Ask {
        return Task::none();
    }
    let folder = folder.to_string();
    Task::perform(
        hangar_core::git::git_repo_info(folder.clone()),
        move |info| {
            Message::PanesLogic(Msg::Branch {
                folder: folder.clone(),
                info,
            })
        },
    )
}

/// The sweep: one window-wide timer, every thirty seconds.
fn hibernate_tick(st: &mut State, m: &mut Model, sv: &Services, now: EpochMs) -> Task<Message> {
    if !m.live.hydrated || !m.settings().hibernate_idle {
        return Task::none();
    }
    sync_active(m, now);
    let delay = delay_ms(m);
    let ids: Vec<WsId> = m
        .state()
        .workspaces
        .iter()
        .map(|ws| ws.id.clone())
        .collect();
    let mut tasks = Vec::new();
    for id in ids {
        tasks.push(decide_next(
            st,
            m,
            sv,
            &id,
            SleepMode::Idle { delay_ms: delay },
            now,
        ));
    }
    // A workspace deleted in its sleep has nothing left to wake.
    let ((), io) = with_env(m, sv, |env| {
        prune(env);
    });
    tasks.push(io);
    bump(m);
    Task::batch(tasks)
}

pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    let _ = st;
    let live = &m.live.panes_logic;
    let mut subscriptions = vec![iced::event::listen_with(window_focus)];
    let watching = live
        .machines
        .values()
        .any(|machine| machine.watcher.is_ticking() || machine.trust_look_at.is_some());
    if watching {
        subscriptions.push(
            iced::time::every(Duration::from_millis(activity::TICK_MS))
                .map(|_| Message::PanesLogic(Msg::Tick)),
        );
    }
    if !live.machines.is_empty() || live.agents.has_conflicts() {
        subscriptions.push(
            iced::time::every(Duration::from_millis(1500)).map(|_| Message::PanesLogic(Msg::Slow)),
        );
    }
    if m.live.hydrated && m.settings().hibernate_idle {
        subscriptions.push(
            iced::time::every(Duration::from_millis(HIBERNATE_TICK_MS))
                .map(|_| Message::PanesLogic(Msg::HibernateTick)),
        );
    }
    Subscription::batch(subscriptions)
}

/// Which window of the app has the system's focus (`document.hasFocus()`).
fn window_focus(
    event: iced::Event,
    _status: iced::event::Status,
    window: window::Id,
) -> Option<Message> {
    match event {
        iced::Event::Window(window::Event::Focused) => {
            Some(Message::PanesLogic(Msg::WindowFocus {
                window,
                focused: true,
            }))
        }
        iced::Event::Window(window::Event::Unfocused) => {
            Some(Message::PanesLogic(Msg::WindowFocus {
                window,
                focused: false,
            }))
        }
        _ => None,
    }
}

/// What takes the focus when F6 reaches a region this package owns. It owns
/// none, so the shell never calls it.
pub fn focus_entry(st: &State, m: &Model, region: Region) -> Option<Task<Message>> {
    let _ = (st, m, region);
    None
}

/// Every intent, after the shell's own handling of it: `Some` when this
/// package acted on it.
pub fn on_intent(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    let now = wall_clock_ms();
    // Whatever the intent was, the workspace on screen may be another, a
    // pane may have taken the focus, a workspace may have been mounted again.
    sync_active(m, now);
    match intent {
        // Sent text counts as using the workspace; it can only reach the one
        // on screen, or one an agent writes to over MCP, which P18 counts.
        Intent::Broadcast { ws, .. } | Intent::RunSavedCommand { ws, .. } => {
            m.live.panes_logic.hibernation.touch_workspace(ws, now);
        }
        // Sent to a window of its own, a workspace asleep wakes there: the
        // sessions its panes are owed stay owed, since the window that
        // starts them is still this process.
        Intent::Detach(ws) => {
            m.live.panes_logic.hibernation.mark_awake(ws);
        }
        _ => {}
    }
    refresh_attention(m, None, now);
    match intent {
        Intent::Sleep(ws) => {
            // Asked by hand: the same rules without the clock, and the same
            // way to sleep. Everything is read and asked at the click.
            Some(decide_next(st, m, sv, ws, SleepMode::Forced, now))
        }
        // A pane that takes the focus shows the branch it is on as soon as
        // it is looked at.
        Intent::FocusPane { ws, pane, .. } => {
            let folder = m.workspace(ws).and_then(|w| {
                w.panes
                    .iter()
                    .find(|p| p.id == *pane)
                    .map(|p| folder_of(w, p))
            })?;
            Some(ask_branch(m, &folder, now, PANE_BRANCH_FOCUS_MS))
        }
        _ => None,
    }
}

/// Every core event, after the shell's own handling of it.
pub fn on_event(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    event: &CoreEvent,
) -> Option<Task<Message>> {
    let _ = (st, sv);
    reduce_core_event(m, event, wall_clock_ms())
}

/// The reducer of core's events into `Live`: the reports of the mod, the
/// queue, the shell's end. Everything it needs is the model and the moment.
fn reduce_core_event(m: &mut Model, event: &CoreEvent, now: EpochMs) -> Option<Task<Message>> {
    match event {
        CoreEvent::Bridge { pane, event } => {
            // A kind this version does not know (a newer mod) is ignored.
            let report = BridgeEvent::parse(event)?;
            Some(with_pane(m, pane, now, |live, info, amb| {
                live.on_bridge(info, report, amb)
            }))
        }
        CoreEvent::BridgeQueue { pane, pending } => {
            m.live.panes_logic.queue_changed(pane, pending.clone(), now);
            bump(m);
            Some(Task::none())
        }
        CoreEvent::PtyExit { pane } => {
            let (info, _) = inputs(m, pane, now)?;
            m.live.panes_logic.shell_exited(&info, now);
            bump(m);
            Some(Task::none())
        }
        _ => None,
    }
}
