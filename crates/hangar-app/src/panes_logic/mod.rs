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
//! (`insight`, whose `mcp` is what the pane's Claude Code was launched
//! with), whether a pane is inside a turn (`is_mid_turn`), the shell's life
//! (`pane_status`, `state_glyph_state`), attention (`needs_attention`),
//! hibernation (`is_asleep`, `asleep`, and for a menu `sleep_line` /
//! `sleep_verdict`, the rules of a sleep asked for by hand read without
//! changing anything), the branch (`pane_branch`). The shell feeds the
//! snapshot from `activity_map` and `asleep_ids`.
//!
//! # What the terminal's package tells it (P5)
//!
//! Through `Message::PanesLogic(Msg::…)`: `Mounted` when a pane's terminal
//! exists (before the PTY spawns, so no report of the mod goes unheard),
//! `Status` once the shell runs (or when it could not start), `Output` for
//! each chunk written to the terminal and not for a replayed history,
//! `Bell`, `Notification` (OSC 9 and 777, read with `osc9_notification` and
//! `osc777_notification`, which also tell the parser whether to handle the
//! code), `Key` (a key pressed, not the terminal answering its program),
//! `Typed` (the person used a workspace on screen), `WatchSession` once the
//! agent is launched, `Launched` when its command was typed (it dates the
//! session of an agent that has no mod to say so), `Unmounted` when the
//! terminal goes. Everything that
//! comes from core (`bridge:event`, `bridge:queue`) and the shell's end
//! (`CoreEvent::PtyExit`, which P5 sends when a pane's shell ends by itself)
//! arrives through `on_event` and needs nothing more from the terminal's
//! package.
//!
//! A launch reads and takes from `Live` (`claim_session`, `must_resume`,
//! `carry_resume`) and tells it the servers it started Claude Code with
//! (`set_launch_mcp`), through the few methods documented there.
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
mod pane_index;
pub mod pane_insight;
pub mod remote_transcripts;
pub mod session_watch;
pub mod shells;

#[cfg(test)]
mod remote_tests;
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
    carry_key, file_base_name, first_line, newly_queued, short_session_id,
};
#[allow(unused_imports)]
pub use self::context_gauge::{CONTEXT_GAUGE_CONFIG, compute_context_gauge, format_tokens};
#[allow(unused_imports)]
pub use self::hibernate::{Awake, Blocker, HIBERNATE_TICK_MS, Verdict, sleep_refusal};
#[allow(unused_imports)]
pub use self::hibernation::Asleep;
#[allow(unused_imports)]
pub use self::pane_agent::{Ambient, Effect, PaneAgent, PaneInfo};
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
    ForcedOutcome, Hibernation, Reading, SleepEnv, SleepMode, SleepRead, Slept, Step, begin,
    finish, prune, put_to_sleep, read_forced, settle_forced,
};
use self::pane_branch::{
    BranchCache, Lookup, PANE_BRANCH_FOCUS_MS, PANE_BRANCH_REFRESH_MS, PANE_BRANCH_TTL_MS,
    pane_branch_of,
};
use self::pane_index::{PaneIndex, info_of};
use self::pane_insight::Insights;
use self::session_watch::SessionWatch;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::{Focus, Notice, Notification, View, WindowKind};
use crate::state::live::{
    AgentActivity, AgentRoster, BridgeEvent, GlyphState, PaneActivity, PaneInsight, PaneMcp,
    PaneStatus, PlanLimits, QueuedPrompt, WorkspaceActivity,
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
    /// The saved sessions a launch did not find on disk, this run: one miss
    /// keeps the id (a transcript under a root not looked in, a read that
    /// failed), a second forgets it.
    missed: HashSet<String>,
    branches: BranchCache,
    /// The transcript hunt of every pane whose agent can resume.
    watches: HashMap<PaneId, SessionWatch>,
    /// The window of the app that has the system's focus, if one does.
    focused_window: Option<window::Id>,
    /// The workspaces the main window showed, the latest first.
    recent: Vec<WsId>,
    /// The workspace it showed when `sync_active` last looked.
    last_active: Option<WsId>,
    /// Where each pane is in the state (`index_panes`).
    index: PaneIndex,
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
    /// The agent's command was typed into the pane's shell. `resumed` is the
    /// session id it was told to resume, `None` for a new conversation.
    ///
    /// This is what dates the session and puts its line in the history for
    /// an agent that cannot say so itself: every agent but a Claude Code with
    /// the hangar-bridge mod, whose hello does it with the id it really
    /// landed on. So the terminal's package may send it for every launch;
    /// that one is ignored here.
    Launched {
        pane: PaneId,
        resumed: Option<String>,
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

/// Pane `pane` wrote output to its terminal, from `first` to `last`, since
/// the panes last looked: they look once a frame at the panes on screen and
/// with the tick at those off it, and tell it here, in the same update, rather
/// than in a message of its own (each message builds the whole window again).
/// The hot path: a write per pane and nothing to redraw for it.
pub fn note_printed(m: &mut Model, pane: &str, first: EpochMs, last: EpochMs) {
    m.live.panes_logic.note_printed(pane, first, last);
}

/// Pane `pane` of workspace `ws` mounted, for a test of another package.
#[cfg(test)]
pub(crate) fn mount_for_test(m: &mut Model, pane: &str, ws: &str, name: &str, now: EpochMs) {
    m.live.panes_logic.mount_pane(pane, ws, name, now);
}

/// The burst of pane `pane` the activity follows, from its first byte to its
/// last, and when hibernation last saw it print, for a test of another
/// package.
#[cfg(test)]
pub(crate) fn output_seen(m: &Model, pane: &str) -> (Option<(EpochMs, EpochMs)>, Option<EpochMs>) {
    let live = &m.live.panes_logic;
    (
        live.machines
            .get(pane)
            .and_then(|machine| machine.watcher.burst()),
        live.hibernation
            .pane_rest(pane)
            .map(|rest| rest.last_output_at),
    )
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

    /// The pane's agent is inside a turn: at work, or waiting for an answer
    /// the turn needs. What must not be restarted from under its person, and
    /// what keeps a workspace awake (`hibernation::is_busy`).
    pub fn is_mid_turn(&self, pane: &str) -> bool {
        hibernation::is_busy(self.activity_or_idle(pane))
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

    /// Hands a session id back: another pane may latch onto it again.
    pub fn release_session(&mut self, session_id: Option<&str>) {
        self.claims.release(session_id);
    }

    /// A launch looked for `session_id`'s transcript: whether the pane is to
    /// forget it, which only a second miss in a row decides.
    pub fn session_looked_up(&mut self, session_id: &str, exists: Option<bool>) -> bool {
        match exists {
            Some(false) => !self.missed.insert(session_id.to_owned()),
            Some(true) => {
                self.missed.remove(session_id);
                false
            }
            None => false,
        }
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

    /// The MCP servers the pane's Claude Code was launched with
    /// (`updatePaneInsight(pane, { mcp })` of `mcpLaunchConfig`). Written by
    /// the launch, once the servers' file is known and before the agent
    /// starts, and read back by the inspector's MCP row and by the servers
    /// panel, which restarts the panes that run with something else.
    ///
    /// `None` forgets it, which is what a launch that could not say leaves:
    /// the panel then counts the pane as out of date, as the TS did for a
    /// launch it never saw. It goes with the pane when its terminal goes
    /// (`unmount_pane`), so a restart under a new id starts without. Whether
    /// anything changed.
    pub fn set_launch_mcp(&mut self, pane: &str, mcp: Option<PaneMcp>) -> bool {
        self.insights.update(pane, |insight| insight.mcp = mcp)
    }
}

/// What another package's test needs to put a pane in a state: the reducer
/// that normally does it is driven by core's events and the terminals.
#[cfg(test)]
impl Live {
    /// The pane reports this activity, as an agent with the mod would.
    pub(crate) fn seed_activity(&mut self, pane: &str, workspace: &str, activity: AgentActivity) {
        self.agents.set(
            pane,
            PaneActivity {
                workspace_id: workspace.to_string(),
                activity,
                bridged: true,
            },
        );
    }

    /// The pane has reported this much about its agent.
    pub(crate) fn seed_insight(&mut self, pane: &str, edit: impl FnOnce(&mut PaneInsight)) {
        self.insights.update(pane, edit);
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

// ----- the prompts a restart carried over

/// Hands prompts back to core's queue for a pane, in the order they were held.
/// Off the UI thread: the queue sits behind a lock that the bridge's own
/// threads take for as long as a mod's request lasts, and a pane mounting
/// must not wait on one.
fn requeue(host: hangar_core::Host, pane: PaneId, carried: Vec<QueuedPrompt>) -> Task<Message> {
    if carried.is_empty() {
        return Task::none();
    }
    Task::perform(
        async move {
            // A blocking task that panicked has nobody to tell: the prompts
            // are what is lost, as when the queue itself fails.
            let _ = tokio::task::spawn_blocking(move || enqueue_all(&host, &pane, carried)).await;
        },
        |()| Message::Noop,
    )
}

/// The blocking half of `requeue`.
fn enqueue_all(host: &hangar_core::Host, pane: &str, carried: Vec<QueuedPrompt>) {
    for prompt in carried {
        hangar_core::bridge::bridge_enqueue(host, pane.to_string(), prompt.text, prompt.from);
    }
}

// ----- the sleep verdict, for a menu

/// The model as a sleep environment that can only be read: what a menu asks
/// as it opens, drawn from the shared model (`sleep.verdict` of
/// `useHibernation`).
struct ModelRead<'a>(&'a Model);

impl SleepRead for ModelRead<'_> {
    fn state(&self) -> &AppState {
        self.0.state()
    }
    fn active(&self) -> Option<&str> {
        self.0.state().active_workspace_id.as_deref()
    }
    fn opened_ids(&self) -> &[WsId] {
        &self.0.ui.opened_ids
    }
    fn recent(&self) -> &[WsId] {
        &self.0.live.panes_logic.recent
    }
    fn store(&self) -> &Hibernation {
        &self.0.live.panes_logic.hibernation
    }
    fn queued(&self, pane_id: &str) -> usize {
        self.0.live.panes_logic.insights.get(pane_id).queue.len()
    }
    fn delivering(&self, workspace_id: &str) -> bool {
        self.0.live.control.is_delivering(workspace_id)
    }
}

/// What a workspace's « Put to sleep » line is, as a menu draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SleepLine {
    /// The rules say it may go.
    Ready,
    /// The rules say it may go, but for these shells: whether they run
    /// something is for the system to say (`pty_has_children`). The line
    /// stays live meanwhile, as in the TS; ask them as the menu opens and
    /// read again with the answer in `probed`.
    Ask(Vec<PaneId>),
    /// It stays awake, and this is why, worded for the line's tooltip.
    Refused(String),
}

/// Whether the workspace may be put to sleep now, by the rules of a sleep
/// asked for by hand (`sleep.verdict(id)`): read from the model as it stands,
/// changing nothing. `probed` is what the system said the shells run, once
/// asked. `None` when the workspace is gone.
#[cfg(test)]
pub fn sleep_verdict(
    m: &Model,
    workspace_id: &str,
    probed: Option<&HashMap<PaneId, bool>>,
) -> Option<Verdict> {
    read_forced(&ModelRead(m), workspace_id, probed).map(|reading| reading.verdict)
}

/// The verdict as the navigator's menu wants it (`sleepEntry` of Sidebar.tsx):
/// greyed with the reason when a rule says no, live otherwise. `None` when
/// the workspace is gone.
pub fn sleep_line(
    m: &Model,
    workspace_id: &str,
    probed: Option<&HashMap<PaneId, bool>>,
) -> Option<SleepLine> {
    let reading = read_forced(&ModelRead(m), workspace_id, probed)?;
    Some(match reading.verdict {
        Verdict::Sleep { .. } => SleepLine::Ready,
        Verdict::Probe { pane_ids } => SleepLine::Ask(pane_ids),
        Verdict::Awake(awake) => {
            let live = &m.live.panes_logic;
            SleepLine::Refused(sleep_refusal(
                Translator::new(m.locale()),
                &awake,
                &reading.workspace.panes,
                &|pane| live.is_waiting(pane),
            ))
        }
    })
}

// ----- the model, read for the machines

impl Model {
    /// The pane `pane` and its workspace, wherever it is: one lookup in the
    /// index of the panes (`index_panes`) rather than a walk through every
    /// workspace.
    pub fn find_pane(&self, pane: &str) -> Option<(&Workspace, &Pane)> {
        let (ws, found, _) = self.live.panes_logic.index.find(self.state(), pane)?;
        Some((ws, found))
    }

    /// `workspace`, through the same index.
    pub fn find_workspace(&self, id: &str) -> Option<&Workspace> {
        self.live.panes_logic.index.workspace(self.state(), id)
    }
}

/// Brings the index of the panes level with the state, when the state
/// changed since it was built. A pane that left the state was closed for
/// good — closed by hand, restarted under a new id, gone with its workspace —
/// and its API token goes with it; one asleep or restarted on the same id is
/// still in the state and keeps its own. Run before every message of this
/// package and of the terminals' (`panes::update`).
pub fn index_panes(m: &mut Model) {
    let state = m.state_arc();
    let revision = m.revision();
    let tokens = hangar_core::server::pane_tokens();
    for gone in m.live.panes_logic.index.refresh(&state, revision) {
        tokens.revoke(&gone);
    }
}

/// A pane, its workspace and what the machines know it as.
fn lookup<'a>(m: &'a Model, pane: &str) -> Option<(&'a Workspace, &'a Pane, Arc<PaneInfo>)> {
    let (ws, found, info) = m.live.panes_logic.index.find(m.state(), pane)?;
    let info = info.unwrap_or_else(|| Arc::new(info_of(ws, found)));
    Some((ws, found, info))
}

/// The folder a pane's shell opens in: its own, else its workspace's. Public
/// so that the terminals' package asks here rather than keeping a copy.
pub fn folder_of(ws: &Workspace, pane: &Pane) -> String {
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
    // The pane the workspace remembers as its own, or the one that has the
    // keyboard right now.
    let focused =
        m.ui.focus_by_workspace
            .get(&ws.id)
            .is_some_and(|id| id == pane)
            || matches!(&m.ui.focus, Some(Focus::Terminal(focus)) if focus == pane);
    focused && attended_workspace(m, ws)
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
fn inputs(m: &Model, pane: &str, now: EpochMs) -> Option<(Arc<PaneInfo>, Ambient)> {
    let (ws, _, info) = lookup(m, pane)?;
    Some((info, ambient(m, ws, pane, now)))
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
                // The session the pane was on is nobody's any more.
                release_unheld_sessions(m);
            }
        }
    }
    Task::batch(tasks)
}

/// Releases the claims no pane holds any more: a pane closed, or moved on to
/// another session, gives its session back, so that a pane opened later on
/// that conversation (the transcript hunt) can latch onto it again. A pane
/// put to sleep or restarted under a new id still holds its session in the
/// state, so its claim stands.
fn release_unheld_sessions(m: &mut Model) {
    let held: HashSet<&str> = m
        .state()
        .workspaces
        .iter()
        .flat_map(|ws| &ws.panes)
        .filter_map(|pane| pane.session_id.as_deref())
        .collect();
    let unheld: Vec<String> = m
        .live
        .panes_logic
        .claims
        .ids()
        .filter(|id| !held.contains(id))
        .map(str::to_owned)
        .collect();
    for id in unheld {
        m.live.panes_logic.release_session(Some(&id));
    }
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
    m.live
        .panes_logic
        .hibernation
        .wake_mounted(&m.ui.opened_ids, now);
}

/// Looking at a pane clears its badge. `window_back`: the window of these
/// panes just came to the front (only those it draws), with the pane
/// already the one looked at.
fn refresh_attention(m: &mut Model, window_back: Option<window::Id>, now: EpochMs) {
    let live = &m.live.panes_logic;
    let mut looks: Vec<(Arc<PaneInfo>, bool, bool)> = Vec::with_capacity(live.machines.len());
    for ws in &m.state().workspaces {
        let back = window_back.is_some_and(|window| host_window(m, ws) == Some(window));
        for pane in &ws.panes {
            if live.is_pane_mounted(&pane.id) {
                looks.push((live.index.info(ws, pane), attended(m, ws, &pane.id), back));
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

impl SleepRead for LiveEnv<'_> {
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
    fn queued(&self, pane_id: &str) -> usize {
        self.insights.get(pane_id).queue.len()
    }
    fn delivering(&self, workspace_id: &str) -> bool {
        self.snapshot
            .delivering
            .get(workspace_id)
            .is_some_and(|runs| !runs.is_empty())
    }
}

impl SleepEnv for LiveEnv<'_> {
    fn store_mut(&mut self) -> &mut Hibernation {
        self.store
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
                // A remote workspace's shells are killed on their node.
                let mut local = Vec::new();
                for id in kills {
                    if crate::nodes::remote::kill_pane(&id).await.is_none() {
                        local.push(id);
                    }
                }
                let kills = local;
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
                    crate::nodes::remote::has_children(manager, pane_ids),
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
    index_panes(m);
    match msg {
        Msg::Mounted { pane } => {
            let Some((ws, found, info)) = lookup(m, &pane) else {
                return Task::none();
            };
            let folder = folder_of(ws, found);
            let live = &mut m.live.panes_logic;
            live.mount_pane(&pane, &info.workspace_id, &info.name, now);
            // A restart gave this pane a new id: the prompts still queued for
            // the old one come along.
            let carried: Vec<QueuedPrompt> = live
                .carried
                .take(&carry_key(&info.workspace_id, &info.name), now);
            let requeued = requeue(sv.core().host().clone(), pane.clone(), carried);
            bump(m);
            // A pane asks once on mount.
            Task::batch([requeued, ask_branch(m, &folder, now, PANE_BRANCH_TTL_MS)])
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
            if !handing_over {
                release_unheld_sessions(m);
            }
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
                    .insert(pane, SessionWatch::new(cwd, known, now));
            }
            Task::none()
        }
        Msg::Launched { pane, resumed } => {
            let watch = watch_remote(m, &pane, resumed.as_deref());
            launched(m, &pane, resumed.as_deref(), now);
            watch
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
        Msg::Slow => slow(st, m, now),
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
            // The mod names the session outright; the transcript hunt is for
            // the agents that have none. Its answer is not even claimed.
            let bridged = live
                .machines
                .get(&pane)
                .is_none_or(|machine| machine.bridged);
            if bridged {
                if let Some(watch) = live.watches.get_mut(&pane) {
                    watch.skip_round(now);
                }
                return Task::none();
            }
            let Some(found) = live
                .watches
                .get_mut(&pane)
                .and_then(|watch| watch.round_done(sessions.as_deref(), now, &mut live.claims))
            else {
                return Task::none();
            };
            match (bridged, inputs(m, &pane, now)) {
                (false, Some((info, _))) => {
                    run_effects(m, &info, vec![Effect::CaptureSession(found)])
                }
                _ => Task::none(),
            }
        }
        Msg::Context { pane, usage } => {
            // A pane switched to a shell since the read has no gauge.
            let claude = m
                .find_pane(&pane)
                .is_some_and(|(_, found)| found.agent == AgentId::Claude);
            let gauge = usage
                .filter(|_| claude)
                .and_then(|usage| compute_context_gauge(&usage));
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

/// The session hunt of a pane of a remote workspace, which its controller
/// leaves out (remote_transcripts.rs): started once the agent's command is
/// typed, with what the node already holds (and the session resumed) as
/// known.
fn watch_remote(m: &Model, pane: &str, resumed: Option<&str>) -> Task<Message> {
    let Some((ws, found)) = m.find_pane(pane) else {
        return Task::none();
    };
    let Some(node) = crate::nodes::away::node_of(ws, found) else {
        return Task::none();
    };
    let agent = found.agent;
    if !is_resumable(agent) {
        return Task::none();
    }
    let cwd = folder_of(ws, found);
    let pane = pane.to_owned();
    let resumed = resumed.map(str::to_owned);
    Task::perform(
        remote_transcripts::known_session_ids(node, cwd.clone()),
        move |mut known| {
            known.extend(resumed.clone());
            Message::PanesLogic(Msg::WatchSession {
                pane: pane.clone(),
                agent,
                cwd: cwd.clone(),
                known,
            })
        },
    )
}

/// The agent's command was typed into a pane's shell: dates its session and
/// words it in the history, for an agent that has no mod to do it.
fn launched(m: &mut Model, pane: &str, resumed: Option<&str>, now: EpochMs) {
    let Some((info, _)) = inputs(m, pane, now) else {
        return;
    };
    // A Claude Code with the mod says it all in its hello, with the id it
    // really landed on; a plain shell launched nothing.
    let says_itself = info.agent == AgentId::Claude && m.settings().claude_bridge;
    if says_itself || info.agent == AgentId::Shell {
        return;
    }
    m.live.panes_logic.launched(&info, resumed, now);
    bump(m);
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
    // The mounted panes as the index made them once: no id or name copied
    // per tick.
    let panes: Vec<Arc<PaneInfo>> = m
        .live
        .panes_logic
        .machines
        .keys()
        .filter_map(|pane| lookup(m, pane).map(|(_, _, info)| info))
        .collect();
    let mut changed = false;
    let mut tasks = Vec::new();
    for pane in panes {
        let Some((info, amb)) = inputs(m, &pane.id, now) else {
            continue;
        };
        // A step changes the machine of its own pane and no other.
        let before = m.live.panes_logic.pane_state_mark(&info.id);
        let mut effects = m.live.panes_logic.poll_pane(&info, idle_ms, &amb);
        if m.live.panes_logic.trust_look_due(&info.id, now) {
            // The rows of the screen, read a moment after the last output.
            let screen = screen_of(sv.terminals().as_ref(), &info.id);
            effects.extend(
                m.live
                    .panes_logic
                    .look_for_trust_question(&info, &screen, &amb),
            );
        }
        changed |= m.live.panes_logic.pane_state_mark(&info.id) != before;
        if !effects.is_empty() {
            tasks.push(run_effects(m, &info, effects));
        }
    }
    // A burst of output ticks every quarter second; what the readers see
    // changes only when a pane starts working or hands back.
    if changed {
        bump(m);
    }
    Task::batch(tasks)
}

/// The screen the trust question is looked for in: the rows the program
/// drew, as the old terminal read them (`viewportY` to `viewportY + rows`),
/// and not the history behind them. A question answered and scrolled off
/// would otherwise be found again by every look until the history forgot it,
/// and the pane would read as waiting for an answer it was given.
fn screen_of(access: &dyn crate::services::TerminalAccess, pane: &PaneId) -> String {
    access
        .screen_text(pane)
        .map(|rows| rows.join("\n"))
        .unwrap_or_default()
}

/// How often the transcripts' context is read.
const CONTEXT_POLL_MS: u64 = 5000;

/// What happens every second and a half while panes are mounted: the overlap
/// marks wear off, the session hunt takes a round, the transcripts' context
/// and the branches are read when it is time.
fn slow(st: &mut State, m: &mut Model, now: EpochMs) -> Task<Message> {
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
        // A bridged pane's session is named by the mod: no listing at all.
        let bridged = live
            .machines
            .get(&pane)
            .is_none_or(|machine| machine.bridged);
        let Some(watch) = live.watches.get_mut(&pane) else {
            continue;
        };
        if bridged {
            watch.skip_round(now);
            continue;
        }
        if !watch.begin_round(now) {
            continue;
        }
        let cwd = watch.cwd.clone();
        // A remote pane's transcripts are on its node.
        let node = m
            .find_pane(&pane)
            .and_then(|(ws, entry)| crate::nodes::away::node_of(ws, entry));
        tasks.push(Task::perform(
            async move {
                match node {
                    Some(node) => remote_transcripts::hunt_listing(node, cwd).await,
                    None => tokio::task::spawn_blocking(move || session_watch::hunt_listing(cwd))
                        .await
                        .ok(),
                }
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
                            reads.push((
                                pane.id.clone(),
                                session,
                                folder_of(ws, pane),
                                crate::nodes::away::node_of(ws, pane),
                            ));
                        }
                    }
                    // A shell keeps no transcript, and before a session is
                    // captured there is nothing to read: no gauge beats a
                    // made-up zero.
                    _ => clears.push(pane.id.clone()),
                }
            }
        }
        for pane in clears {
            m.live.panes_logic.set_transcript_context(&pane, None);
        }
        for (pane, session, cwd, node) in reads {
            let read = async move {
                match node {
                    Some(node) => remote_transcripts::context_usage(node, session, cwd).await,
                    None => Ok(hangar_core::context::context_usage(session, cwd).await),
                }
            };
            tasks.push(Task::perform(read, move |usage| match usage {
                Ok(usage) => Message::PanesLogic(Msg::Context {
                    pane: pane.clone(),
                    usage,
                }),
                // The node did not answer: the last reading stands.
                Err(_) => Message::Noop,
            }));
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
    // The tick is also when the panes off screen are looked at, while one of
    // them prints (`panes::look_off_screen`, called before the tick).
    if watching || m.live.panes.looking_off_screen() {
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

/// Every intent, after the shell's own handling of it: `Some` when this
/// package acted on it.
pub fn on_intent(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    let now = wall_clock_ms();
    index_panes(m);
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
        _ => focused_branch(m, intent, now),
    }
}

/// A pane that takes the focus shows the branch it is on as soon as it is
/// looked at: the full landing (`FocusPane`: a notification, the board,
/// Needs-you, the palette) and the keyboard's move from pane to pane
/// (`SelectPane`: `pane.focus1`…, `pane.next`, the arrows) alike. Everything
/// else the focus drives here is read from the model by `sync_active`, which
/// runs before every intent.
fn focused_branch(m: &mut Model, intent: &Intent, now: EpochMs) -> Option<Task<Message>> {
    let (Intent::FocusPane { ws, pane, .. } | Intent::SelectPane { ws, pane }) = intent else {
        return None;
    };
    let folder = m.workspace(ws).and_then(|w| {
        w.panes
            .iter()
            .find(|p| p.id == *pane)
            .map(|p| folder_of(w, p))
    })?;
    Some(ask_branch(m, &folder, now, PANE_BRANCH_FOCUS_MS))
}

/// Every core event, after the shell's own handling of it.
pub fn on_event(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    event: &CoreEvent,
) -> Option<Task<Message>> {
    let _ = (st, sv);
    index_panes(m);
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
