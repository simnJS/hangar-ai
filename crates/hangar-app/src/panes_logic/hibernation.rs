//! What the decision in `hibernate` is made from, and what it decided
//! (`lib/hibernateStore.ts`), and the sweep that asks and acts on it
//! (`lib/useHibernation.ts`).
//!
//! The panes write into the store as things happen (a key pressed, a chunk
//! printed, a turn over). Nothing reads it but the window's timer, twice a
//! minute, and a workspace's menu when it opens on "Put to sleep". The
//! workspaces asleep are the one thing on screen, and only they notify.
//!
//! Everything is in memory: a restart forgets which workspaces were asleep,
//! and they simply count as never opened.
//!
//! The decision is made against a `SleepEnv`, which the live model implements
//! (`LiveEnv` in `panes_logic`) and a test implements with a handful of
//! fields. Asking the system about a shell's processes takes a moment, during
//! which anything can change (the person opens the workspace, an agent starts
//! a turn), so after a probe everything is read again with the answer in hand
//! and only that second reading decides. In the app the probe is a task
//! (`begin`, then `finish` when the answer is in); `decide` is the same two
//! steps with the probe at hand, for the tests and for anything that can
//! wait.

use std::collections::{HashMap, HashSet};

use hangar_i18n::{Key, Translator, args};

use super::hibernate::{
    Awake, Blocker, Candidate, ForcedSleepInput, HibernateInput, PaneRest, StepAsideInput, Verdict,
    forced_sleep_verdict, hibernation_verdict, step_aside,
};
use crate::state::live::{AgentActivity, EpochMs};
use crate::state::types::{AgentId, AppState, PaneId, Workspace, WsId};

/// A workspace asleep: since when, and how many agents it stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Asleep {
    pub at: EpochMs,
    pub agents: usize,
}

/// What the store keeps of a pane's rest; `queued` and `has_children` are
/// read when asked (`Omit<PaneRest, "queued" | "hasChildren">`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rest {
    pub last_output_at: EpochMs,
    pub busy: bool,
    pub resting_since: EpochMs,
    pub agent_exited: bool,
}

#[derive(Debug, Default)]
pub struct Hibernation {
    /// When each workspace was last used, by workspace id.
    touched: HashMap<WsId, EpochMs>,
    /// By pane id, from the moment its terminal mounts until it unmounts.
    rest: HashMap<PaneId, Rest>,
    /// Panes put to sleep that have not started again yet. Each resumes its
    /// session on its next launch whatever the auto-resume setting says: a
    /// person who turned it off still never lost a conversation to a
    /// workspace going to sleep, behind their back or at their word. Taken
    /// once, by the launch it was meant for.
    ///
    /// The web app's workspace windows were handed these in their address
    /// (`?resume=`); here every window is a window of one process.
    resume_once: HashSet<PaneId>,
    asleep: HashMap<WsId, Asleep>,
}

/// Working or waiting for an answer: what keeps a workspace awake.
pub(super) fn is_busy(activity: AgentActivity) -> bool {
    matches!(activity, AgentActivity::Working | AgentActivity::Waiting)
}

impl Hibernation {
    /// Someone typed into the workspace, clicked it, or sent it something.
    pub fn touch_workspace(&mut self, workspace_id: &str, at: EpochMs) {
        self.touched.insert(workspace_id.to_string(), at);
    }

    pub fn last_touch(&self, workspace_id: &str) -> Option<EpochMs> {
        self.touched.get(workspace_id).copied()
    }

    /// A pane that just mounted has done nothing yet, which counts from now.
    pub fn note_pane_mounted(&mut self, pane_id: &str, now: EpochMs) {
        self.rest.insert(
            pane_id.to_string(),
            Rest {
                last_output_at: now,
                busy: false,
                resting_since: now,
                agent_exited: false,
            },
        );
    }

    /// Called for every chunk a pane prints, so it does no more than a write.
    pub fn note_pane_output(&mut self, pane_id: &str, now: EpochMs) {
        if let Some(entry) = self.rest.get_mut(pane_id) {
            entry.last_output_at = now;
        }
    }

    /// The resting clock starts when the agent stops working or waiting.
    pub fn note_pane_activity(&mut self, pane_id: &str, activity: AgentActivity, now: EpochMs) {
        let Some(entry) = self.rest.get_mut(pane_id) else {
            return;
        };
        let busy = is_busy(activity);
        if busy == entry.busy {
            return;
        }
        entry.busy = busy;
        if !busy {
            entry.resting_since = now;
        }
    }

    /// The agent quit back to its shell, or a new one said hello in it.
    pub fn note_agent_exited(&mut self, pane_id: &str, exited: bool) {
        if let Some(entry) = self.rest.get_mut(pane_id) {
            entry.agent_exited = exited;
        }
    }

    pub fn forget_pane(&mut self, pane_id: &str) {
        self.rest.remove(pane_id);
    }

    pub fn pane_rest(&self, pane_id: &str) -> Option<&Rest> {
        self.rest.get(pane_id)
    }

    pub fn resume_on_wake<'a>(&mut self, pane_ids: impl IntoIterator<Item = &'a str>) {
        self.resume_once
            .extend(pane_ids.into_iter().map(str::to_string));
    }

    /// Whether the pane's next launch must resume; `take` uses the promise up.
    pub fn must_resume(&mut self, pane_id: &str, take: bool) -> bool {
        if take {
            self.resume_once.remove(pane_id)
        } else {
            self.resume_once.contains(pane_id)
        }
    }

    /// A pane restarted under a new id keeps what its old id was owed.
    pub fn carry_resume(&mut self, from_id: &str, to_id: &str) {
        if self.must_resume(from_id, true) {
            self.resume_once.insert(to_id.to_string());
        }
    }

    pub fn mark_asleep(&mut self, workspace_id: &str, info: Asleep) {
        self.asleep.insert(workspace_id.to_string(), info);
    }

    /// Awake again: opened by hand, by an agent, or sent to a window.
    /// Whether it was asleep.
    pub fn mark_awake(&mut self, workspace_id: &str) -> bool {
        self.asleep.remove(workspace_id).is_some()
    }

    pub fn is_asleep(&self, workspace_id: &str) -> bool {
        self.asleep.contains_key(workspace_id)
    }

    /// Every workspace asleep, by id.
    pub fn asleep(&self) -> &HashMap<WsId, Asleep> {
        &self.asleep
    }

    /// Forgets the workspaces asleep that are not in `existing`: deleted
    /// since. Whether any went.
    pub fn prune_asleep(&mut self, existing: &HashSet<WsId>) -> bool {
        let before = self.asleep.len();
        self.asleep.retain(|id, _| existing.contains(id));
        self.asleep.len() != before
    }

    /// Mounted again: awake, and as good as used just now. For the
    /// workspaces the window has mounted. Whether any woke.
    pub fn wake_mounted(&mut self, opened_ids: &[WsId], now: EpochMs) -> bool {
        let mut woke = false;
        for id in opened_ids {
            if self.mark_awake(id) {
                self.touch_workspace(id, now);
                woke = true;
            }
        }
        woke
    }
}

/// What a decision reads of the app, as it stands. Apart from `SleepEnv` so
/// that something which only wants the verdict (the navigator's menu, drawn
/// from a shared model) needs nothing it could change.
pub trait SleepRead {
    fn state(&self) -> &AppState;
    /// The workspace the main window shows.
    fn active(&self) -> Option<&str>;
    /// The workspaces whose terminals the main window has mounted.
    fn opened_ids(&self) -> &[WsId];
    /// The workspaces the main window showed, the latest first.
    fn recent(&self) -> &[WsId];
    fn store(&self) -> &Hibernation;
    /// Prompts held for a pane (`getPaneInsight(id).queue.length`).
    fn queued(&self, pane_id: &str) -> usize;
    /// One of its deliveries is being checked, reviewed or landed:
    /// `isDelivering(id) || deliveringIn(id)`.
    fn delivering(&self, workspace_id: &str) -> bool;
}

/// What a workspace is put to sleep from: the app as it stands, read when
/// asked, and the few things a sleep does to it.
pub trait SleepEnv: SleepRead {
    fn store_mut(&mut self) -> &mut Hibernation;
    /// Unmounts a workspace's terminals, which ends their PTYs.
    fn unmount(&mut self, workspace_id: &str);
    /// Shows another workspace in the main window.
    fn show(&mut self, workspace_id: &str);
    /// Ends a pane's shell and everything under it (`pty_kill(id, tree)`).
    fn kill_tree(&mut self, pane_id: &str);
}

/// Left alone for the delay, or asked for by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepMode {
    Idle { delay_ms: u64 },
    Forced,
}

/// One workspace, and what the rules said about it.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub workspace: Workspace,
    pub verdict: Verdict,
}

fn unknown_processes() -> Awake {
    Awake {
        reason: Blocker::UnknownProcesses,
        pane_id: None,
    }
}

/// The workspaces as the navigator lists them: the ones at the top level,
/// then each folder's in folder order (`sidebarOrder` of lib/folders.ts;
/// `state::folders` will own it when it is ported).
fn navigator_order(state: &AppState) -> Vec<Candidate> {
    let known: HashSet<&str> = state.folders.iter().map(|f| f.id.as_str()).collect();
    let top = state.workspaces.iter().filter(|ws| {
        ws.folder_id
            .as_deref()
            .is_none_or(|folder| !known.contains(folder))
    });
    let filed = state.folders.iter().flat_map(|folder| {
        state
            .workspaces
            .iter()
            .filter(|ws| ws.folder_id.as_deref() == Some(folder.id.as_str()))
    });
    top.chain(filed)
        .map(|ws| Candidate {
            id: ws.id.clone(),
            detached: ws.detached.unwrap_or(false),
        })
        .collect()
}

/// Where the main window would go if `workspace_id` left it.
fn step_aside_from<E: SleepRead + ?Sized>(env: &E, workspace_id: &str) -> Option<WsId> {
    let workspaces = navigator_order(env.state());
    let mounted: Vec<WsId> = env
        .opened_ids()
        .iter()
        .filter(|id| !env.store().is_asleep(id))
        .cloned()
        .collect();
    step_aside(&StepAsideInput {
        current: workspace_id,
        workspaces: &workspaces,
        recent: env.recent(),
        mounted: &mounted,
    })
}

fn rest_of<E: SleepRead + ?Sized>(
    env: &E,
    pane_id: &str,
    probed: Option<&HashMap<PaneId, bool>>,
) -> Option<PaneRest> {
    let rest = env.store().pane_rest(pane_id)?;
    Some(PaneRest {
        last_output_at: rest.last_output_at,
        busy: rest.busy,
        resting_since: rest.resting_since,
        queued: env.queued(pane_id),
        agent_exited: rest.agent_exited,
        has_children: probed.and_then(|probed| probed.get(pane_id).copied()),
    })
}

/// A workspace as the rules are fed it, read once.
struct Seen {
    workspace: Workspace,
    asleep: bool,
    mounted: bool,
    on_screen: bool,
    detached: bool,
    delivering: bool,
    rest: HashMap<PaneId, PaneRest>,
}

/// Everything both modes of the rules read of a workspace. `None` when it is
/// gone.
fn see<E: SleepRead + ?Sized>(
    env: &E,
    workspace_id: &str,
    probed: Option<&HashMap<PaneId, bool>>,
) -> Option<Seen> {
    let workspace = env
        .state()
        .workspaces
        .iter()
        .find(|ws| ws.id == workspace_id)?
        .clone();
    let asleep = env.store().is_asleep(workspace_id);
    // Put to sleep, a workspace is unmounted on the window's next render: from
    // the moment it is marked asleep it no longer counts as mounted.
    let mounted = env.opened_ids().iter().any(|id| id == workspace_id) && !asleep;
    let on_screen = env.active() == Some(workspace_id);
    let detached = workspace.detached.unwrap_or(false);
    // A worker's delivery_submit and a "Merge" are both tracked elsewhere:
    // either keeps the workspace up.
    let delivering = env.delivering(workspace_id);
    let rest: HashMap<PaneId, PaneRest> = workspace
        .panes
        .iter()
        .filter_map(|pane| Some((pane.id.clone(), rest_of(env, &pane.id, probed)?)))
        .collect();
    Some(Seen {
        workspace,
        asleep,
        mounted,
        on_screen,
        detached,
        delivering,
        rest,
    })
}

/// The rules of a sleep asked for by hand. They need no clock, so reading
/// them changes nothing: this is what the navigator's menu shows as it opens
/// (`sleep.verdict` of `useHibernation`) and what `read_workspace` decides on
/// in `Forced` mode. `probed` is what the system said the shells run, once
/// asked. `None` when the workspace is gone.
pub fn read_forced<E: SleepRead + ?Sized>(
    env: &E,
    workspace_id: &str,
    probed: Option<&HashMap<PaneId, bool>>,
) -> Option<Reading> {
    let seen = see(env, workspace_id, probed)?;
    let can_step_aside = !seen.on_screen || step_aside_from(env, workspace_id).is_some();
    let verdict = forced_sleep_verdict(&ForcedSleepInput {
        panes: &seen.workspace.panes,
        asleep: seen.asleep,
        mounted: seen.mounted,
        on_screen: seen.on_screen,
        can_step_aside,
        detached: seen.detached,
        delivering: seen.delivering,
        rest: &seen.rest,
    });
    Some(Reading {
        workspace: seen.workspace,
        verdict,
    })
}

/// One workspace as it stands, put to the rules of `mode`; `probed` is what
/// the system said its shells run, once asked. `None` when it is gone.
pub fn read_workspace(
    env: &mut dyn SleepEnv,
    workspace_id: &str,
    mode: SleepMode,
    probed: Option<&HashMap<PaneId, bool>>,
    now: EpochMs,
) -> Option<Reading> {
    let delay_ms = match mode {
        SleepMode::Forced => return read_forced(&*env, workspace_id, probed),
        SleepMode::Idle { delay_ms } => delay_ms,
    };
    let seen = see(&*env, workspace_id, probed)?;
    // Never seen before (mounted by an agent, say): its delay starts now. Not
    // before it is mounted, or it would start the delay already spent.
    if seen.mounted && (seen.on_screen || env.store().last_touch(workspace_id).is_none()) {
        env.store_mut().touch_workspace(workspace_id, now);
    }
    let last_interaction = env.store().last_touch(workspace_id).unwrap_or(now);
    let verdict = hibernation_verdict(&HibernateInput {
        now,
        delay_ms,
        panes: &seen.workspace.panes,
        mounted: seen.mounted,
        on_screen: seen.on_screen,
        detached: seen.detached,
        delivering: seen.delivering,
        last_interaction,
        rest: &seen.rest,
    });
    Some(Reading {
        workspace: seen.workspace,
        verdict,
    })
}

/// Where a decision stands after its first reading.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Decided, or gone (`None`).
    Done(Option<Reading>),
    /// Only the shells' processes are left to ask about.
    Probe {
        workspace: Workspace,
        pane_ids: Vec<PaneId>,
    },
}

/// The first reading of a decision.
pub fn begin(env: &mut dyn SleepEnv, workspace_id: &str, mode: SleepMode, now: EpochMs) -> Step {
    match read_workspace(env, workspace_id, mode, None, now) {
        Some(Reading {
            workspace,
            verdict: Verdict::Probe { pane_ids },
        }) => Step::Probe {
            workspace,
            pane_ids,
        },
        other => Step::Done(other),
    }
}

/// The second reading, once the system answered for the shells that were
/// asked about (`asked`). An error, or shells that came while the others were
/// being asked, is `UnknownProcesses`: not knowing what a shell runs is
/// reason enough to leave it running.
pub fn finish(
    env: &mut dyn SleepEnv,
    workspace: &Workspace,
    mode: SleepMode,
    now: EpochMs,
    asked: &[PaneId],
    answer: Result<Vec<PaneId>, String>,
) -> Option<Reading> {
    let Ok(running) = answer else {
        return Some(Reading {
            workspace: workspace.clone(),
            verdict: Verdict::Awake(unknown_processes()),
        });
    };
    let probed: HashMap<PaneId, bool> = asked
        .iter()
        .map(|id| (id.clone(), running.contains(id)))
        .collect();
    let reading = read_workspace(env, &workspace.id, mode, Some(&probed), now)?;
    if matches!(reading.verdict, Verdict::Probe { .. }) {
        return Some(Reading {
            workspace: reading.workspace,
            verdict: Verdict::Awake(unknown_processes()),
        });
    }
    Some(reading)
}

/// How a probe is made when it can be made on the spot: the environment (a
/// probe takes time, and the answer can change what is read after it) and the
/// shells to ask about; the ids with a process under them, or why it failed.
pub type Probe<'a> = &'a mut dyn FnMut(&mut dyn SleepEnv, &[PaneId]) -> Result<Vec<PaneId>, String>;

/// Whether one workspace goes to sleep now: `begin`, the probe, `finish`.
/// `None` when the workspace is gone.
pub fn decide(
    env: &mut dyn SleepEnv,
    workspace_id: &str,
    mode: SleepMode,
    now: EpochMs,
    probe: Probe,
) -> Option<Reading> {
    match begin(env, workspace_id, mode, now) {
        Step::Done(reading) => reading,
        Step::Probe {
            workspace,
            pane_ids,
        } => {
            let answer = probe(env, &pane_ids);
            finish(env, &workspace, mode, now, &pane_ids, answer)
        }
    }
}

/// The one way a workspace goes to sleep, whoever decided it: every pane's
/// process tree killed, its agents owed their sessions on the next launch,
/// the workspace marked asleep and its terminals unmounted.
pub fn put_to_sleep(env: &mut dyn SleepEnv, workspace: &Workspace, agents: usize, now: EpochMs) {
    // Every agent pane has a session by now (`hibernate`): each resumes it.
    env.store_mut().resume_on_wake(
        workspace
            .panes
            .iter()
            .filter(|pane| pane.agent != AgentId::Shell)
            .map(|pane| pane.id.as_str()),
    );
    // The whole tree under each shell, so the agents' helpers go too. The
    // unmount below ends the shells again, which costs nothing.
    for pane in &workspace.panes {
        env.kill_tree(&pane.id);
    }
    env.store_mut()
        .mark_asleep(&workspace.id, Asleep { at: now, agents });
    env.unmount(&workspace.id);
}

/// A workspace the sweep put to sleep.
#[derive(Debug, Clone, PartialEq)]
pub struct Slept {
    pub workspace: Workspace,
    pub agents: usize,
}

/// Every workspace, asked in turn; the ones put to sleep come back.
pub fn sweep(env: &mut dyn SleepEnv, delay_ms: u64, now: EpochMs, probe: Probe) -> Vec<Slept> {
    let mut slept = Vec::new();
    let ids: Vec<WsId> = env
        .state()
        .workspaces
        .iter()
        .map(|ws| ws.id.clone())
        .collect();
    for id in ids {
        let reading = decide(env, &id, SleepMode::Idle { delay_ms }, now, probe);
        if let Some(Reading {
            workspace,
            verdict: Verdict::Sleep { agents },
        }) = reading
        {
            put_to_sleep(env, &workspace, agents, now);
            slept.push(Slept { workspace, agents });
        }
    }
    prune(env);
    slept
}

/// A workspace deleted in its sleep has nothing left to wake.
pub fn prune(env: &mut dyn SleepEnv) -> bool {
    let existing: HashSet<WsId> = env
        .state()
        .workspaces
        .iter()
        .map(|ws| ws.id.clone())
        .collect();
    env.store_mut().prune_asleep(&existing)
}

#[derive(Debug, Clone, PartialEq)]
pub enum ForcedOutcome {
    /// `shown` is the workspace the main window moved on to, if it had to.
    Slept {
        workspace: Workspace,
        agents: usize,
        shown: Option<WsId>,
    },
    Refused {
        workspace: Workspace,
        verdict: Awake,
    },
}

/// What a sleep asked for does with its reading: the main window moves on
/// before, and only once the answer is yes (a refusal leaves the person where
/// they were), then the workspace is put to sleep the way the sweep does it.
pub fn settle_forced(env: &mut dyn SleepEnv, reading: Reading, now: EpochMs) -> ForcedOutcome {
    let Reading { workspace, verdict } = reading;
    let agents = match verdict {
        Verdict::Sleep { agents } => agents,
        Verdict::Awake(verdict) => return ForcedOutcome::Refused { workspace, verdict },
        // `decide` never ends on a probe; were it to, the shells were not asked.
        Verdict::Probe { .. } => {
            return ForcedOutcome::Refused {
                workspace,
                verdict: unknown_processes(),
            };
        }
    };
    let mut shown = None;
    if env.active() == Some(workspace.id.as_str()) {
        shown = step_aside_from(env, &workspace.id);
        let Some(next) = shown.clone() else {
            return ForcedOutcome::Refused {
                workspace,
                verdict: Awake {
                    reason: Blocker::Alone,
                    pane_id: None,
                },
            };
        };
        env.show(&next);
    }
    put_to_sleep(env, &workspace, agents, now);
    ForcedOutcome::Slept {
        workspace,
        agents,
        shown,
    }
}

/// One workspace put to sleep because the person asked: decided like the
/// sweep, without the clock, then put to sleep the same way. `None` when the
/// workspace is gone.
pub fn force_sleep(
    env: &mut dyn SleepEnv,
    workspace_id: &str,
    now: EpochMs,
    probe: Probe,
) -> Option<ForcedOutcome> {
    let reading = decide(env, workspace_id, SleepMode::Forced, now, probe)?;
    Some(settle_forced(env, reading, now))
}

/// What the notice says after workspaces went to sleep.
pub fn announcement(t: Translator, workspaces: &[Workspace], agents: usize) -> String {
    match workspaces {
        [] => String::new(),
        [only] => {
            if agents > 0 {
                t.t_with(
                    Key::HibernateNotice,
                    &args!("name" => only.name.as_str(), "n" => agents),
                )
                .into_owned()
            } else {
                t.t_with(
                    Key::HibernateNoticeNoAgent,
                    &args!("name" => only.name.as_str()),
                )
                .into_owned()
            }
        }
        many => t
            .t_with(
                Key::HibernateNoticeMany,
                &args!("count" => many.len(), "n" => agents),
            )
            .into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::types::Pane;
    use hangar_i18n::Locale;

    const MIN: u64 = 60_000;

    fn pane(id: &str, agent: AgentId) -> Pane {
        Pane {
            id: id.into(),
            name: id.into(),
            agent,
            session_id: (agent != AgentId::Shell).then(|| format!("s-{id}")),
            ..Pane::default()
        }
    }

    fn workspace(id: &str, panes: Vec<Pane>) -> Workspace {
        Workspace {
            id: id.into(),
            name: id.into(),
            cwd: format!("/work/{id}"),
            panes,
            ..Workspace::default()
        }
    }

    /// A window that mounted every workspace given, with each pane seen just
    /// now: the environment the vitest suite built with `mainWindow`.
    struct Window {
        state: AppState,
        opened: Vec<WsId>,
        recent: Vec<WsId>,
        store: Hibernation,
        queued: HashMap<PaneId, usize>,
        delivering: HashSet<WsId>,
        calls: Vec<String>,
        kills: Vec<PaneId>,
    }

    impl Window {
        fn new(workspaces: Vec<Workspace>, active: &str, recent: &[&str], now: EpochMs) -> Window {
            let mut store = Hibernation::default();
            for ws in &workspaces {
                for pane in &ws.panes {
                    store.note_pane_mounted(&pane.id, now);
                }
            }
            Window {
                opened: workspaces.iter().map(|ws| ws.id.clone()).collect(),
                state: AppState {
                    workspaces,
                    active_workspace_id: Some(active.into()),
                    ..AppState::default()
                },
                recent: recent.iter().map(|id| (*id).to_string()).collect(),
                store,
                queued: HashMap::new(),
                delivering: HashSet::new(),
                calls: Vec::new(),
                kills: Vec::new(),
            }
        }
    }

    impl SleepRead for Window {
        fn state(&self) -> &AppState {
            &self.state
        }
        fn active(&self) -> Option<&str> {
            self.state.active_workspace_id.as_deref()
        }
        fn opened_ids(&self) -> &[WsId] {
            &self.opened
        }
        fn recent(&self) -> &[WsId] {
            &self.recent
        }
        fn store(&self) -> &Hibernation {
            &self.store
        }
        fn queued(&self, pane_id: &str) -> usize {
            self.queued.get(pane_id).copied().unwrap_or(0)
        }
        fn delivering(&self, workspace_id: &str) -> bool {
            self.delivering.contains(workspace_id)
        }
    }

    impl SleepEnv for Window {
        fn store_mut(&mut self) -> &mut Hibernation {
            &mut self.store
        }
        fn unmount(&mut self, workspace_id: &str) {
            self.calls.push(format!("unmount {workspace_id}"));
            self.opened.retain(|id| id != workspace_id);
        }
        fn show(&mut self, workspace_id: &str) {
            self.calls.push(format!("show {workspace_id}"));
            self.state.active_workspace_id = Some(workspace_id.into());
        }
        fn kill_tree(&mut self, pane_id: &str) {
            self.kills.push(pane_id.into());
        }
    }

    /// The system's answer: no shell runs anything.
    fn nothing_runs(
        probes: &std::cell::RefCell<Vec<Vec<PaneId>>>,
    ) -> impl FnMut(&mut dyn SleepEnv, &[PaneId]) -> Result<Vec<PaneId>, String> + '_ {
        move |_, ids| {
            probes.borrow_mut().push(ids.to_vec());
            Ok(Vec::new())
        }
    }

    /// What a sleep leaves behind, whoever decided it.
    fn slept(window: &mut Window, ws: &Workspace) -> (bool, Vec<PaneId>, Vec<PaneId>) {
        let killed = window
            .kills
            .iter()
            .filter(|id| ws.panes.iter().any(|p| &p.id == *id))
            .cloned()
            .collect();
        let owed = ws
            .panes
            .iter()
            .filter(|p| window.store.must_resume(&p.id, false))
            .map(|p| p.id.clone())
            .collect();
        (window.store.is_asleep(&ws.id), killed, owed)
    }

    #[test]
    fn puts_a_workspace_to_sleep_by_hand_exactly_as_the_timer_does_once_it_is_due() {
        let start = 1_000 * MIN;
        let screen = workspace("screen", vec![pane("gus", AgentId::Claude)]);
        let by_hand = workspace(
            "hand",
            vec![pane("ava", AgentId::Claude), pane("sam", AgentId::Shell)],
        );
        let by_timer = workspace(
            "timer",
            vec![pane("ben", AgentId::Claude), pane("tom", AgentId::Shell)],
        );
        let mut window = Window::new(
            vec![screen, by_hand.clone(), by_timer.clone()],
            "screen",
            &[],
            start,
        );
        let probes = std::cell::RefCell::new(Vec::new());

        // Just mounted: the timer has nothing to do yet, a click does.
        assert_eq!(
            sweep(&mut window, 10 * MIN, start, &mut nothing_runs(&probes)),
            []
        );
        let outcome = force_sleep(&mut window, "hand", start, &mut nothing_runs(&probes));
        assert_eq!(
            outcome,
            Some(ForcedOutcome::Slept {
                workspace: by_hand.clone(),
                agents: 1,
                shown: None
            })
        );

        let later = 1_011 * MIN;
        let swept = sweep(&mut window, 10 * MIN, later, &mut nothing_runs(&probes));
        assert_eq!(
            swept,
            [Slept {
                workspace: by_timer.clone(),
                agents: 1
            }]
        );

        // The same marks on both: every tree killed, the agent owed its
        // session, the workspace asleep and unmounted.
        assert_eq!(
            slept(&mut window, &by_hand),
            (true, vec!["ava".into(), "sam".into()], vec!["ava".into()])
        );
        assert_eq!(
            slept(&mut window, &by_timer),
            (true, vec!["ben".into(), "tom".into()], vec!["ben".into()])
        );
        assert_eq!(window.calls, ["unmount hand", "unmount timer"]);
        // Both asked the system about the shell before deciding.
        assert_eq!(
            *probes.borrow(),
            [vec!["sam".to_string()], vec!["tom".to_string()]]
        );
    }

    #[test]
    fn never_lets_the_timer_put_to_sleep_again_a_workspace_a_click_just_did() {
        let start = 2_000 * MIN;
        let screen = workspace("screen", vec![pane("gus", AgentId::Claude)]);
        let other = workspace("other", vec![pane("ava", AgentId::Claude)]);
        let mut window = Window::new(vec![screen, other], "screen", &[], start);
        let probes = std::cell::RefCell::new(Vec::new());
        force_sleep(&mut window, "other", start, &mut nothing_runs(&probes));

        // The window has not drawn the unmount yet: it is still in opened ids.
        window.opened = vec!["screen".into(), "other".into()];
        let later = 2_011 * MIN;
        assert_eq!(
            sweep(&mut window, 10 * MIN, later, &mut nothing_runs(&probes)),
            []
        );
        assert_eq!(window.kills.len(), 1);
    }

    fn three_workspaces() -> Window {
        let a = workspace("a", vec![pane("ava", AgentId::Claude)]);
        let b = workspace("b", vec![pane("ben", AgentId::Claude)]);
        let c = workspace("c", vec![pane("cleo", AgentId::Claude)]);
        Window::new(vec![a, b, c], "a", &["a", "c", "b"], 0)
    }

    #[test]
    fn force_moves_the_main_window_to_the_workspace_used_last_then_puts_this_one_to_sleep() {
        let mut window = three_workspaces();
        let probes = std::cell::RefCell::new(Vec::new());
        let outcome = force_sleep(&mut window, "a", 0, &mut nothing_runs(&probes));
        assert!(matches!(
            outcome,
            Some(ForcedOutcome::Slept { shown: Some(ref shown), .. }) if shown == "c"
        ));
        assert_eq!(window.calls, ["show c", "unmount a"]);
        assert_eq!(window.active(), Some("c"));
        assert!(window.store.is_asleep("a"));
    }

    #[test]
    fn force_keeps_the_only_workspace_on_screen_and_awake() {
        let only = workspace("only", vec![pane("ava", AgentId::Claude)]);
        let mut window = Window::new(vec![only.clone()], "only", &[], 0);
        let probes = std::cell::RefCell::new(Vec::new());
        assert_eq!(
            force_sleep(&mut window, "only", 0, &mut nothing_runs(&probes)),
            Some(ForcedOutcome::Refused {
                workspace: only,
                verdict: Awake {
                    reason: Blocker::Alone,
                    pane_id: None
                }
            })
        );
        assert!(window.calls.is_empty());
        assert!(window.kills.is_empty());
    }

    #[test]
    fn force_refuses_at_the_click_when_a_shell_turns_out_to_run_a_process_and_moves_nothing() {
        let a = workspace(
            "a",
            vec![pane("ava", AgentId::Claude), pane("leo", AgentId::Shell)],
        );
        let b = workspace("b", vec![pane("ben", AgentId::Claude)]);
        let mut window = Window::new(vec![a.clone(), b], "a", &["a", "b"], 0);
        let mut busy_shell = |_: &mut dyn SleepEnv, _: &[PaneId]| Ok(vec!["leo".to_string()]);

        assert_eq!(
            force_sleep(&mut window, "a", 0, &mut busy_shell),
            Some(ForcedOutcome::Refused {
                workspace: a,
                verdict: Awake {
                    reason: Blocker::ShellChildren,
                    pane_id: Some("leo".into())
                }
            })
        );
        // Still on screen, still running.
        assert!(window.calls.is_empty());
        assert_eq!(window.active(), Some("a"));
        assert!(window.kills.is_empty());
        assert!(!window.store.is_asleep("a"));
    }

    #[test]
    fn force_reads_everything_again_once_the_shells_have_answered() {
        let screen = workspace("screen", vec![pane("gus", AgentId::Claude)]);
        let ws = workspace(
            "ws",
            vec![pane("ava", AgentId::Claude), pane("leo", AgentId::Shell)],
        );
        let mut window = Window::new(vec![screen, ws], "screen", &[], 0);
        // Ava starts a turn while the system is being asked about Leo's shell.
        let mut starts_a_turn = |env: &mut dyn SleepEnv, _: &[PaneId]| {
            env.store_mut()
                .note_pane_activity("ava", AgentActivity::Working, 1);
            Ok(Vec::new())
        };

        let outcome = force_sleep(&mut window, "ws", 0, &mut starts_a_turn);
        let Some(ForcedOutcome::Refused { verdict, .. }) = outcome else {
            panic!("expected a refusal, got {outcome:?}");
        };
        assert_eq!(verdict.reason, Blocker::Busy);
        assert_eq!(verdict.pane_id.as_deref(), Some("ava"));
        assert!(window.kills.is_empty());
    }

    #[test]
    fn force_keeps_a_workspace_awake_when_the_system_cannot_say_what_its_shells_run() {
        let screen = workspace("screen", vec![pane("gus", AgentId::Claude)]);
        let ws = workspace(
            "ws",
            vec![pane("ava", AgentId::Claude), pane("leo", AgentId::Shell)],
        );
        let mut window = Window::new(vec![screen, ws], "screen", &[], 0);
        let mut no_table = |_: &mut dyn SleepEnv, _: &[PaneId]| Err("no process table".to_string());
        let outcome = force_sleep(&mut window, "ws", 0, &mut no_table);
        let Some(ForcedOutcome::Refused { verdict, .. }) = outcome else {
            panic!("expected a refusal, got {outcome:?}");
        };
        assert_eq!(verdict.reason, Blocker::UnknownProcesses);
    }

    #[test]
    fn force_says_an_agent_at_work_keeps_it_awake_and_asks_the_system_nothing() {
        let screen = workspace("screen", vec![pane("gus", AgentId::Claude)]);
        let ws = workspace(
            "ws",
            vec![pane("ava", AgentId::Claude), pane("leo", AgentId::Shell)],
        );
        let mut window = Window::new(vec![screen, ws], "screen", &[], 0);
        window
            .store
            .note_pane_activity("ava", AgentActivity::Waiting, 1);
        let probes = std::cell::RefCell::new(Vec::new());

        let outcome = force_sleep(&mut window, "ws", 0, &mut nothing_runs(&probes));
        let Some(ForcedOutcome::Refused { verdict, .. }) = outcome else {
            panic!("expected a refusal, got {outcome:?}");
        };
        assert_eq!(verdict.reason, Blocker::Busy);
        assert_eq!(verdict.pane_id.as_deref(), Some("ava"));
        assert!(probes.borrow().is_empty());
    }

    #[test]
    fn force_answers_asleep_the_second_time_having_done_nothing_more() {
        let screen = workspace("screen", vec![pane("gus", AgentId::Claude)]);
        let ws = workspace("ws", vec![pane("ava", AgentId::Claude)]);
        let mut window = Window::new(vec![screen, ws], "screen", &[], 0);
        let probes = std::cell::RefCell::new(Vec::new());
        force_sleep(&mut window, "ws", 0, &mut nothing_runs(&probes));
        let again = force_sleep(&mut window, "ws", 0, &mut nothing_runs(&probes));
        let Some(ForcedOutcome::Refused { verdict, .. }) = again else {
            panic!("expected a refusal, got {again:?}");
        };
        assert_eq!(verdict.reason, Blocker::Asleep);
        assert_eq!(window.kills.len(), 1);
    }

    #[test]
    fn force_is_nothing_for_a_workspace_deleted_meanwhile() {
        let mut window = Window::new(
            vec![workspace("screen", vec![pane("gus", AgentId::Claude)])],
            "screen",
            &[],
            0,
        );
        let probes = std::cell::RefCell::new(Vec::new());
        assert_eq!(
            force_sleep(&mut window, "gone", 0, &mut nothing_runs(&probes)),
            None
        );
    }

    // What the store itself promises.

    #[test]
    fn the_rest_clock_starts_when_the_agent_stops_working_or_waiting() {
        let mut store = Hibernation::default();
        store.note_pane_mounted("a", 100);
        // Nothing mounted, nothing noted.
        store.note_pane_activity("ghost", AgentActivity::Working, 5);
        assert!(store.pane_rest("ghost").is_none());

        store.note_pane_activity("a", AgentActivity::Working, 200);
        assert_eq!(
            store.pane_rest("a").map(|r| (r.busy, r.resting_since)),
            Some((true, 100))
        );
        // Waiting is busy as well: the clock does not restart.
        store.note_pane_activity("a", AgentActivity::Waiting, 300);
        assert_eq!(store.pane_rest("a").map(|r| r.resting_since), Some(100));
        store.note_pane_activity("a", AgentActivity::Yours, 400);
        assert_eq!(
            store.pane_rest("a").map(|r| (r.busy, r.resting_since)),
            Some((false, 400))
        );

        store.note_pane_output("a", 450);
        store.note_agent_exited("a", true);
        let rest = store.pane_rest("a").unwrap();
        assert_eq!((rest.last_output_at, rest.agent_exited), (450, true));
        store.forget_pane("a");
        assert!(store.pane_rest("a").is_none());
    }

    #[test]
    fn a_resume_is_owed_once_and_follows_a_pane_restarted_under_a_new_id() {
        let mut store = Hibernation::default();
        store.resume_on_wake(["a", "b"]);
        assert!(store.must_resume("a", false));
        assert!(store.must_resume("a", true));
        assert!(!store.must_resume("a", false));

        store.carry_resume("b", "b2");
        assert!(!store.must_resume("b", false));
        assert!(store.must_resume("b2", false));
        // Nothing owed, nothing carried.
        store.carry_resume("c", "c2");
        assert!(!store.must_resume("c2", false));
    }

    #[test]
    fn waking_a_mounted_workspace_marks_it_awake_and_used_just_now() {
        let mut store = Hibernation::default();
        store.mark_asleep("w1", Asleep { at: 1, agents: 2 });
        store.mark_asleep("w2", Asleep { at: 1, agents: 0 });
        assert!(store.wake_mounted(&["w1".into(), "other".into()], 500));
        assert!(!store.is_asleep("w1"));
        assert_eq!(store.last_touch("w1"), Some(500));
        assert_eq!(store.last_touch("other"), None);
        assert!(store.is_asleep("w2"));
        assert!(!store.wake_mounted(&["w1".into()], 600));

        // A workspace deleted in its sleep is forgotten.
        let existing: HashSet<WsId> = ["w1".to_string()].into();
        assert!(store.prune_asleep(&existing));
        assert!(store.asleep().is_empty());
        assert!(!store.prune_asleep(&existing));
    }

    #[test]
    fn a_workspace_never_seen_starts_its_delay_when_it_is_first_read() {
        let a = workspace("a", vec![pane("ava", AgentId::Claude)]);
        let b = workspace("b", vec![pane("ben", AgentId::Claude)]);
        let mut window = Window::new(vec![a, b], "a", &[], 0);
        assert_eq!(window.store.last_touch("b"), None);
        // Read at minute 100: its delay starts then, not at the epoch.
        let reading = read_workspace(
            &mut window,
            "b",
            SleepMode::Idle { delay_ms: 10 * MIN },
            None,
            100 * MIN,
        );
        assert!(matches!(
            reading,
            Some(Reading {
                verdict: Verdict::Awake(Awake {
                    reason: Blocker::RecentInteraction,
                    ..
                }),
                ..
            })
        ));
        assert_eq!(window.store.last_touch("b"), Some(100 * MIN));
        // And the workspace on screen is touched on every read.
        read_workspace(
            &mut window,
            "a",
            SleepMode::Idle { delay_ms: 10 * MIN },
            None,
            105 * MIN,
        );
        assert_eq!(window.store.last_touch("a"), Some(105 * MIN));
    }

    #[test]
    fn queued_prompts_and_deliveries_keep_a_workspace_awake() {
        let screen = workspace("screen", vec![pane("gus", AgentId::Claude)]);
        let ws = workspace("ws", vec![pane("ava", AgentId::Claude)]);
        let mut window = Window::new(vec![screen, ws], "screen", &[], 0);
        window.queued.insert("ava".into(), 2);
        let reading = read_workspace(&mut window, "ws", SleepMode::Forced, None, 0).unwrap();
        assert_eq!(
            reading.verdict,
            Verdict::Awake(Awake {
                reason: Blocker::Queued,
                pane_id: Some("ava".into())
            })
        );
        window.queued.clear();
        window.delivering.insert("ws".into());
        let reading = read_workspace(&mut window, "ws", SleepMode::Forced, None, 0).unwrap();
        assert_eq!(
            reading.verdict,
            Verdict::Awake(Awake {
                reason: Blocker::Delivering,
                pane_id: None
            })
        );
    }

    #[test]
    fn steps_aside_in_the_navigators_order_folders_after_the_top_level() {
        use crate::state::types::WorkspaceFolder;
        let mut a = workspace("a", vec![pane("p1", AgentId::Claude)]);
        let mut b = workspace("b", vec![pane("p2", AgentId::Claude)]);
        let c = workspace("c", vec![pane("p3", AgentId::Claude)]);
        a.folder_id = Some("f".into());
        // A folder id naming no folder is the top level.
        b.folder_id = Some("gone".into());
        let state = AppState {
            workspaces: vec![a, b, c],
            folders: vec![WorkspaceFolder {
                id: "f".into(),
                name: "F".into(),
                ..WorkspaceFolder::default()
            }],
            ..AppState::default()
        };
        let order: Vec<WsId> = navigator_order(&state).into_iter().map(|c| c.id).collect();
        assert_eq!(order, ["b", "c", "a"]);
    }

    #[test]
    fn words_what_went_to_sleep() {
        let en = Translator::new(Locale::En);
        let one = workspace("w", vec![]);
        let mut two = workspace("v", vec![]);
        two.name = "other".into();
        let says = |key: Key, args: &[(&str, hangar_i18n::Arg)]| en.t_with(key, args).into_owned();
        assert_eq!(
            announcement(en, std::slice::from_ref(&one), 2),
            says(Key::HibernateNotice, &args!("name" => "w", "n" => 2))
        );
        assert_eq!(
            announcement(en, std::slice::from_ref(&one), 1),
            says(Key::HibernateNotice, &args!("name" => "w", "n" => 1))
        );
        assert_eq!(
            announcement(en, std::slice::from_ref(&one), 0),
            says(Key::HibernateNoticeNoAgent, &args!("name" => "w"))
        );
        assert_eq!(
            announcement(en, &[one, two], 3),
            says(Key::HibernateNoticeMany, &args!("count" => 2, "n" => 3))
        );
        // One agent reads as a singular, several as a plural.
        assert_ne!(
            says(Key::HibernateNotice, &args!("name" => "w", "n" => 1)),
            says(Key::HibernateNotice, &args!("name" => "w", "n" => 2))
        );
        assert_eq!(announcement(en, &[], 0), "");
    }
}
