//! What happened in the projects, kept on disk (a file a day for 30 days, in
//! the app data dir) so the phone's "Since your last visit" still answers
//! after Hangar restarted, a week away included (lib/activityLog.ts). This is
//! the window's side: it writes through core's `activity_append` and reads
//! through `activity_read` (core's `remote/activity.rs`, where the rotation,
//! the pruning and the cap live).
//!
//! It detects nothing. It reads what the app already knows: the project
//! manager's journal (a pane that waits, a task that needs the person, a
//! delivery ready, a board done), the orchestrators' journals (deliveries
//! landed or done, tasks moved to done), the files shared with the phone,
//! and the workspaces falling asleep or waking. A pane that waited is written
//! again when its wait ends, with how long it lasted. Nothing from the HQ.
//!
//! Each line is {at, workspace, kind, text}, its text in the language of the
//! PC at the time it happened.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use hangar_core::remote::{ActivityEntry, Share};
use hangar_i18n::{Arg, Key, Locale, t, t_with};

use crate::orchestration::events::{DeliveryOutcome, JournalEntry, OrchestratorEvent};
use crate::orchestration::manager_feed::{BlockEvent, ManagerEvent, ManagerNews, ManagerNewsEntry};
use crate::state::{AppState, PaneId, WsId};

use super::push::{folder_name, workspace_of_share};

/// How often the journals are read.
pub const TICK_MS: u64 = 5_000;
/// A line the disk refused is kept, this many, and tried again.
const MAX_UNWRITTEN: usize = 200;
/// The same task asking for the person twice within this is one line.
const SAME_ASK_MS: u64 = 10 * 60_000;

/// "45 s", "12 min", "3 h 05": the same in both languages (`lasted`).
pub fn lasted(ms: u64) -> String {
    let seconds = (ms as f64 / 1000.0 + 0.5).floor() as u64;
    if seconds < 60 {
        return format!("{seconds} s");
    }
    let minutes = (seconds as f64 / 60.0 + 0.5).floor() as u64;
    if minutes < 60 {
        return format!("{minutes} min");
    }
    format!("{} h {:02}", minutes / 60, minutes % 60)
}

/// What the log reads (`ActivityDeps`).
pub trait ActivityIo {
    fn state(&self) -> Arc<AppState>;
    /// The manager's journal, block events included (`managerNews`).
    fn manager_news(&self) -> Vec<ManagerNewsEntry>;
    fn orchestrator_journal(&self, workspace_id: &str) -> Vec<JournalEntry>;
    fn is_asleep(&self, workspace_id: &str) -> bool;
    fn is_waiting(&self, pane: &str) -> bool;
}

fn locale_of(state: &AppState) -> Locale {
    state.settings.locale.unwrap_or_else(Locale::system)
}

fn say(locale: Locale, key: Key, vars: &[(&str, &str)]) -> String {
    let args: Vec<(&str, Arg)> = vars
        .iter()
        .map(|(name, value)| (*name, Arg::from(*value)))
        .collect();
    t_with(locale, key, &args).into_owned()
}

/// Who blocked a task, as a line says it: "user" is the person.
fn by_of(by: &str, locale: Locale) -> String {
    if by == "user" {
        t(locale, Key::ActivityThePerson).into_owned()
    } else {
        by.to_string()
    }
}

#[derive(Debug, Clone)]
struct Wait {
    at: u64,
    pane: String,
    workspace: String,
}

/// The log (`createActivityLog`).
#[derive(Debug)]
pub struct ActivityLog {
    /// Written down already, among what the journals still hold.
    seen: HashSet<String>,
    asked: HashMap<String, u64>,
    waits: HashMap<PaneId, Wait>,
    slept: HashMap<WsId, bool>,
    baseline: bool,
    unwritten: Vec<ActivityEntry>,
    stopped: bool,
}

impl Default for ActivityLog {
    fn default() -> Self {
        ActivityLog {
            seen: HashSet::new(),
            asked: HashMap::new(),
            waits: HashMap::new(),
            slept: HashMap::new(),
            baseline: true,
            unwritten: Vec::new(),
            stopped: false,
        }
    }
}

fn entry(at: u64, workspace: &str, kind: &str, text: String) -> ActivityEntry {
    ActivityEntry {
        at,
        workspace: workspace.to_string(),
        kind: kind.to_string(),
        text,
    }
}

fn is_hq(state: &AppState, workspace_id: &str) -> bool {
    state
        .workspaces
        .iter()
        .any(|ws| ws.id == workspace_id && ws.hq == Some(true))
}

impl ActivityLog {
    fn add(&mut self, line: ActivityEntry) {
        self.unwritten.push(line);
        if self.unwritten.len() > MAX_UNWRITTEN {
            let over = self.unwritten.len() - MAX_UNWRITTEN;
            self.unwritten.drain(..over);
        }
    }

    /// Once for a task asking twice, from the manager's journal and the
    /// orchestrator's.
    fn ask(&mut self, kind: &str, workspace_id: &str, task_id: &str, at: u64) -> bool {
        let key = format!("{kind}\n{workspace_id}\n{task_id}");
        if let Some(last) = self.asked.get(&key)
            && at.abs_diff(*last) < SAME_ASK_MS
        {
            return false;
        }
        self.asked.insert(key, at);
        true
    }

    fn read_manager(
        &mut self,
        io: &dyn ActivityIo,
        state: &AppState,
        current: &mut HashSet<String>,
        locale: Locale,
    ) {
        for ManagerNewsEntry { at, event } in io.manager_news() {
            let (kind, workspace_id, workspace, what) = match &event {
                ManagerNews::Event(ManagerEvent::PaneWaiting {
                    workspace_id,
                    workspace,
                    pane_id,
                    ..
                }) => ("paneWaiting", workspace_id, workspace, pane_id.as_str()),
                ManagerNews::Event(ManagerEvent::TaskNeedsYou {
                    workspace_id,
                    workspace,
                    task_id,
                    ..
                }) => ("taskNeedsYou", workspace_id, workspace, task_id.as_str()),
                ManagerNews::Event(ManagerEvent::ReadyToLand {
                    workspace_id,
                    workspace,
                    task_id,
                    ..
                }) => ("readyToLand", workspace_id, workspace, task_id.as_str()),
                ManagerNews::Event(ManagerEvent::BoardDone {
                    workspace_id,
                    workspace,
                    ..
                }) => ("boardDone", workspace_id, workspace, ""),
                ManagerNews::Block(BlockEvent::TaskBlocked {
                    workspace_id,
                    workspace,
                    task_id,
                    ..
                }) => ("taskBlocked", workspace_id, workspace, task_id.as_str()),
                ManagerNews::Block(BlockEvent::TaskUnblocked {
                    workspace_id,
                    workspace,
                    task_id,
                    ..
                }) => ("taskUnblocked", workspace_id, workspace, task_id.as_str()),
                ManagerNews::Block(BlockEvent::TaskRecheckDue {
                    workspace_id,
                    workspace,
                    task_id,
                    ..
                }) => ("taskRecheckDue", workspace_id, workspace, task_id.as_str()),
            };
            let key = format!("m\n{at}\n{kind}\n{workspace_id}\n{what}");
            let fresh = !self.seen.contains(&key);
            current.insert(key);
            if !fresh || is_hq(state, workspace_id) {
                continue;
            }
            match &event {
                ManagerNews::Event(ManagerEvent::PaneWaiting { pane_id, pane, .. }) => {
                    self.waits.insert(
                        pane_id.clone(),
                        Wait {
                            at,
                            pane: pane.clone(),
                            workspace: workspace.clone(),
                        },
                    );
                    let text = say(locale, Key::ActivityPaneWaiting, &[("pane", pane)]);
                    self.add(entry(at, workspace, "paneWaiting", text));
                }
                ManagerNews::Event(ManagerEvent::TaskNeedsYou { task_id, title, .. }) => {
                    if self.ask("needsYou", workspace_id, task_id, at) {
                        let text = say(locale, Key::ActivityNeedsYou, &[("title", title)]);
                        self.add(entry(at, workspace, "needsYou", text));
                    }
                }
                ManagerNews::Event(ManagerEvent::ReadyToLand { task_id, title, .. }) => {
                    if self.ask("readyToLand", workspace_id, task_id, at) {
                        let text = say(locale, Key::ActivityReadyToLand, &[("title", title)]);
                        self.add(entry(at, workspace, "readyToLand", text));
                    }
                }
                ManagerNews::Block(BlockEvent::TaskBlocked {
                    task_id,
                    title,
                    by,
                    reason,
                    ..
                }) => {
                    if self.ask("taskBlocked", workspace_id, task_id, at) {
                        let by = by_of(by, locale);
                        let text = say(
                            locale,
                            Key::ActivityTaskBlocked,
                            &[("by", &by), ("title", title), ("reason", reason)],
                        );
                        self.add(entry(at, workspace, "taskBlocked", text));
                    }
                }
                ManagerNews::Block(BlockEvent::TaskUnblocked {
                    task_id,
                    title,
                    how,
                    ..
                }) => {
                    if self.ask("taskUnblocked", workspace_id, task_id, at) {
                        let text = say(
                            locale,
                            Key::ActivityTaskUnblocked,
                            &[("title", title), ("how", how)],
                        );
                        self.add(entry(at, workspace, "taskUnblocked", text));
                    }
                }
                ManagerNews::Event(ManagerEvent::BoardDone { tasks, .. }) => {
                    let text = say(locale, Key::ActivityBoardDone, &[("n", &tasks.to_string())]);
                    self.add(entry(at, workspace, "boardDone", text));
                }
                ManagerNews::Block(BlockEvent::TaskRecheckDue { .. }) => {}
            }
        }
    }

    fn read_orchestrators(
        &mut self,
        io: &dyn ActivityIo,
        state: &AppState,
        current: &mut HashSet<String>,
        locale: Locale,
    ) {
        for ws in state.workspaces.iter().filter(|ws| ws.hq != Some(true)) {
            let journal = io.orchestrator_journal(&ws.id);
            let delivered: HashSet<&str> = journal
                .iter()
                .filter_map(|entry| match &entry.event {
                    OrchestratorEvent::Delivery {
                        task_id,
                        outcome: DeliveryOutcome::Landed | DeliveryOutcome::Done,
                        ..
                    } => Some(task_id.as_str()),
                    _ => None,
                })
                .collect();
            for JournalEntry { at, event, .. } in &journal {
                let key = format!(
                    "o\n{}\n{at}\n{:?}\n{}",
                    ws.id,
                    event.kind(),
                    event.task_id().unwrap_or("")
                );
                let fresh = !self.seen.contains(&key);
                current.insert(key);
                if !fresh {
                    continue;
                }
                let at = *at;
                let place = ws.name.as_str();
                match event {
                    OrchestratorEvent::Delivery {
                        title,
                        outcome: outcome @ (DeliveryOutcome::Landed | DeliveryOutcome::Done),
                        ..
                    } => {
                        let landed = *outcome == DeliveryOutcome::Landed;
                        let (kind, key) = if landed {
                            ("landed", Key::ActivityLanded)
                        } else {
                            ("delivered", Key::ActivityDelivered)
                        };
                        let text = say(locale, key, &[("title", title)]);
                        self.add(entry(at, place, kind, text));
                    }
                    OrchestratorEvent::TaskMoved {
                        task_id, title, to, ..
                    } if to == "done" && !delivered.contains(task_id.as_str()) => {
                        let text = say(locale, Key::ActivityTaskDone, &[("title", title)]);
                        self.add(entry(at, place, "taskDone", text));
                    }
                    OrchestratorEvent::TaskBlocked {
                        task_id,
                        title,
                        by,
                        reason,
                        ..
                    } if self.ask("taskBlocked", &ws.id, task_id, at) => {
                        let by = by_of(by, locale);
                        let text = say(
                            locale,
                            Key::ActivityTaskBlocked,
                            &[("by", &by), ("title", title), ("reason", reason)],
                        );
                        self.add(entry(at, place, "taskBlocked", text));
                    }
                    OrchestratorEvent::TaskUnblocked {
                        task_id,
                        title,
                        how,
                        ..
                    } if self.ask("taskUnblocked", &ws.id, task_id, at) => {
                        let text = say(
                            locale,
                            Key::ActivityTaskUnblocked,
                            &[("title", title), ("how", how)],
                        );
                        self.add(entry(at, place, "taskUnblocked", text));
                    }
                    OrchestratorEvent::TaskNeedsYou { task_id, title, .. }
                        if self.ask("needsYou", &ws.id, task_id, at) =>
                    {
                        let text = say(locale, Key::ActivityNeedsYou, &[("title", title)]);
                        self.add(entry(at, place, "needsYou", text));
                    }
                    _ => {}
                }
            }
        }
    }

    fn waits_ended(&mut self, io: &dyn ActivityIo, locale: Locale, now: u64) {
        let ended: Vec<PaneId> = self
            .waits
            .keys()
            .filter(|pane| !io.is_waiting(pane))
            .cloned()
            .collect();
        for pane in ended {
            let Some(wait) = self.waits.remove(&pane) else {
                continue;
            };
            let time = lasted(now.saturating_sub(wait.at));
            let text = say(
                locale,
                Key::ActivityPaneWaited,
                &[("pane", &wait.pane), ("time", &time)],
            );
            self.add(entry(now, &wait.workspace, "paneWaited", text));
        }
    }

    fn sleeps(&mut self, io: &dyn ActivityIo, state: &AppState, locale: Locale, now: u64) {
        let workspaces: Vec<_> = state
            .workspaces
            .iter()
            .filter(|ws| ws.hq != Some(true))
            .collect();
        self.slept
            .retain(|id, _| workspaces.iter().any(|ws| ws.id == *id));
        for ws in workspaces {
            let asleep = io.is_asleep(&ws.id);
            let was = self.slept.insert(ws.id.clone(), asleep);
            // What is asleep when Hangar starts is not news; what changes
            // after is.
            if self.baseline || was.is_none_or(|was| was == asleep) {
                continue;
            }
            let (kind, key) = if asleep {
                ("asleep", Key::ActivityAsleep)
            } else {
                ("awake", Key::ActivityAwake)
            };
            let text = say(locale, key, &[("name", &ws.name)]);
            self.add(entry(now, &ws.name, kind, text));
        }
    }

    /// Reads the journals once, and keeps what is new to write.
    pub fn tick(&mut self, io: &dyn ActivityIo, now: u64) {
        if self.stopped {
            return;
        }
        let state = io.state();
        let locale = locale_of(&state);
        let mut current = HashSet::new();
        self.read_manager(io, &state, &mut current, locale);
        self.read_orchestrators(io, &state, &mut current, locale);
        self.seen = current;
        self.waits_ended(io, locale, now);
        self.sleeps(io, &state, locale, now);
        self.baseline = false;
    }

    /// An agent shared a file with the phones.
    pub fn shared(&mut self, share: &Share, state: &AppState, now: u64) {
        if self.stopped {
            return;
        }
        let ws = workspace_of_share(state, share);
        if ws.is_some_and(|ws| ws.hq == Some(true)) {
            return;
        }
        let workspace = ws.map_or_else(|| folder_name(&share.workspace), |ws| ws.name.clone());
        let text = say(
            locale_of(state),
            Key::ActivityShared,
            &[("pane", &share.pane), ("name", &share.name)],
        );
        let at = if share.at > 0 { share.at } else { now };
        self.add(entry(at, &workspace, "shared", text));
    }

    /// Writes what waits to be written. Called with the log locked, so the
    /// lines reach the file in the order they came; a refused write keeps its
    /// lines for the next.
    pub fn write(&mut self, append: &dyn Fn(&[ActivityEntry]) -> Result<(), String>) {
        if self.unwritten.is_empty() {
            return;
        }
        let batch = std::mem::take(&mut self.unwritten);
        if append(&batch).is_err() {
            let mut kept = batch;
            kept.append(&mut self.unwritten);
            let over = kept.len().saturating_sub(MAX_UNWRITTEN);
            kept.drain(..over);
            self.unwritten = kept;
        }
    }

    pub fn stop(&mut self) {
        self.stopped = true;
    }
}

/// "Hangar started", in the PC's language: the first line of a run.
pub fn app_started(state: &AppState, now: u64) -> ActivityEntry {
    entry(
        now,
        "",
        "appStarted",
        t(locale_of(state), Key::ActivityAppStarted).into_owned(),
    )
}
