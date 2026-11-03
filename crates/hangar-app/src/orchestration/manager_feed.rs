//! What waits for the person, in any workspace but the HQ, as the project
//! manager hears it (`lib/managerEvents.ts`). Four things and nothing else: an
//! agent pane waiting for a permission or a question, a task labelled
//! needs-you, a delivery ready to land, a board whose tasks are all done.
//! Moves, comments, worker starts and landings stay with the orchestrators
//! (events.rs).
//!
//! The batching is the orchestrators' own: a few seconds to gather what comes
//! together, a gap between two messages, held while the manager is in a turn,
//! read again just before it goes (a wait that ended is no longer news),
//! capped. The workspaces need no orchestrator. The manager's end of turn is
//! what raises the desktop notification, so nothing is sent to the desktop
//! from here.
//!
//! The phone's push notifications (P20's remotePush) are a second sink of the
//! same detection: it runs while a manager runs or a phone is subscribed, and
//! each sink gets what it is owed whether the other exists or not — the push
//! takes every event as it is seen and keeps its own pace, the manager's batch
//! is not held for it, nor it for the manager's turn.
//!
//! Like the orchestrators' engine, the feed is a state machine driven with
//! the time (`next_wake`, `fire_due`) over `EngineIo`.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use hangar_core::board::Task;

use super::events::{EngineIo, timing::*};
use super::roles::manager_of;
use super::text::{MAX_TEXT, is_hangar_author, one_line, task_ref};
use crate::state::AgentActivity;
use crate::state::roles::{NEEDS_YOU_LABEL, READY_TO_LAND_LABEL};
use crate::state::types::{AppState, Pane, PaneId, PaneRole, Workspace, WsId};

/// What waits for the person (`ManagerEvent`): the four things the phone's
/// push knows too.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ManagerEvent {
    #[serde(rename_all = "camelCase")]
    PaneWaiting {
        workspace_id: WsId,
        workspace: String,
        pane_id: PaneId,
        pane: String,
        for_ms: u64,
    },
    #[serde(rename_all = "camelCase")]
    TaskNeedsYou {
        workspace_id: WsId,
        workspace: String,
        task_id: String,
        title: String,
        reason: String,
    },
    #[serde(rename_all = "camelCase")]
    ReadyToLand {
        workspace_id: WsId,
        workspace: String,
        task_id: String,
        title: String,
    },
    #[serde(rename_all = "camelCase")]
    BoardDone {
        workspace_id: WsId,
        workspace: String,
        tasks: usize,
    },
}

/// A task blocked for the person (human only), then unblocked or reminded
/// (`BlockEvent`). Told to the manager like the rest, but kept apart from
/// `ManagerEvent`: the phone push knows only those four, and a block is not a
/// notification it was asked to make.
// The names are the kinds the TS wrote, which the JSON carries.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BlockEvent {
    #[serde(rename_all = "camelCase")]
    TaskBlocked {
        workspace_id: WsId,
        workspace: String,
        task_id: String,
        title: String,
        by: String,
        reason: String,
    },
    #[serde(rename_all = "camelCase")]
    TaskUnblocked {
        workspace_id: WsId,
        workspace: String,
        task_id: String,
        title: String,
        how: String,
    },
    #[serde(rename_all = "camelCase")]
    TaskRecheckDue {
        workspace_id: WsId,
        workspace: String,
        task_id: String,
        title: String,
        reason: String,
    },
}

/// Either kind (`ManagerNews`). Written as the inner event, kind and all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ManagerNews {
    Event(ManagerEvent),
    Block(BlockEvent),
}

impl ManagerNews {
    pub fn is_block(&self) -> bool {
        matches!(self, ManagerNews::Block(_))
    }

    pub fn workspace(&self) -> &str {
        match self {
            ManagerNews::Event(
                ManagerEvent::PaneWaiting { workspace, .. }
                | ManagerEvent::TaskNeedsYou { workspace, .. }
                | ManagerEvent::ReadyToLand { workspace, .. }
                | ManagerEvent::BoardDone { workspace, .. },
            )
            | ManagerNews::Block(
                BlockEvent::TaskBlocked { workspace, .. }
                | BlockEvent::TaskUnblocked { workspace, .. }
                | BlockEvent::TaskRecheckDue { workspace, .. },
            ) => workspace,
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            ManagerNews::Event(ManagerEvent::PaneWaiting { .. }) => "paneWaiting",
            ManagerNews::Event(ManagerEvent::TaskNeedsYou { .. }) => "taskNeedsYou",
            ManagerNews::Event(ManagerEvent::ReadyToLand { .. }) => "readyToLand",
            ManagerNews::Event(ManagerEvent::BoardDone { .. }) => "boardDone",
            ManagerNews::Block(BlockEvent::TaskBlocked { .. }) => "taskBlocked",
            ManagerNews::Block(BlockEvent::TaskUnblocked { .. }) => "taskUnblocked",
            ManagerNews::Block(BlockEvent::TaskRecheckDue { .. }) => "taskRecheckDue",
        }
    }

    fn rank(&self) -> f64 {
        match self.kind() {
            "paneWaiting" => 0.0,
            "taskNeedsYou" => 1.0,
            "taskRecheckDue" => 1.5,
            "taskBlocked" => 1.6,
            "taskUnblocked" => 1.7,
            "readyToLand" => 2.0,
            _ => 3.0,
        }
    }

    /// Two events with the same key are one (`keyOf`): the kind, the
    /// workspace, and the pane or the task.
    fn key(&self) -> String {
        let (workspace_id, what) = match self {
            ManagerNews::Event(ManagerEvent::PaneWaiting {
                workspace_id,
                pane_id,
                ..
            }) => (workspace_id, pane_id.as_str()),
            ManagerNews::Event(
                ManagerEvent::TaskNeedsYou {
                    workspace_id,
                    task_id,
                    ..
                }
                | ManagerEvent::ReadyToLand {
                    workspace_id,
                    task_id,
                    ..
                },
            )
            | ManagerNews::Block(
                BlockEvent::TaskBlocked {
                    workspace_id,
                    task_id,
                    ..
                }
                | BlockEvent::TaskUnblocked {
                    workspace_id,
                    task_id,
                    ..
                }
                | BlockEvent::TaskRecheckDue {
                    workspace_id,
                    task_id,
                    ..
                },
            ) => (workspace_id, task_id.as_str()),
            ManagerNews::Event(ManagerEvent::BoardDone { workspace_id, .. }) => (workspace_id, ""),
        };
        format!("{}\n{workspace_id}\n{what}", self.kind())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManagerJournalEntry {
    pub at: u64,
    pub event: ManagerEvent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManagerNewsEntry {
    pub at: u64,
    pub event: ManagerNews,
}

/// A sink beside the manager's pane (`ManagerSink`): it hears every event
/// while `active` says it listens. P20's phone push is one.
pub trait ManagerSink: Send {
    fn active(&self) -> bool;
    /// A sink's trouble is its own: an error here never costs the manager
    /// its event.
    fn post(&mut self, event: &ManagerEvent) -> Result<(), String>;
}

const JOURNAL_SIZE: usize = 50;
const MAX_PENDING: usize = 200;
const MAX_LINES: usize = 12;

/// One event as a line a model reads (`describeManagerEvent`).
pub fn describe_manager_event(event: &ManagerNews) -> String {
    let place = format!("Workspace \"{}\"", event.workspace());
    match event {
        ManagerNews::Block(BlockEvent::TaskBlocked {
            task_id,
            title,
            by,
            reason,
            ..
        }) => format!(
            "{place}: {} blocked {} until a person lifts it: {reason}",
            if by == "user" { "the person" } else { by },
            task_ref(Some(title), task_id)
        ),
        ManagerNews::Block(BlockEvent::TaskUnblocked {
            task_id,
            title,
            how,
            ..
        }) => {
            format!(
                "{place}: {} unblocked: {how}",
                task_ref(Some(title), task_id)
            )
        }
        ManagerNews::Block(BlockEvent::TaskRecheckDue {
            task_id,
            title,
            reason,
            ..
        }) => format!(
            "{place}: {}: its recheck day passed, it waits for a person ({reason})",
            task_ref(Some(title), task_id)
        ),
        ManagerNews::Event(ManagerEvent::PaneWaiting { pane, for_ms, .. }) => format!(
            "{place}: pane \"{pane}\" has been waiting for the person for {} min (a permission or a question in its pane)",
            ((*for_ms as f64 / 60_000.0).round() as u64).max(1)
        ),
        ManagerNews::Event(ManagerEvent::TaskNeedsYou {
            task_id,
            title,
            reason,
            ..
        }) => {
            format!(
                "{place}: {} needs the person: {reason}",
                task_ref(Some(title), task_id)
            )
        }
        ManagerNews::Event(ManagerEvent::ReadyToLand { task_id, title, .. }) => format!(
            "{place}: the delivery of {} is ready to land, it waits for the person",
            task_ref(Some(title), task_id)
        ),
        ManagerNews::Event(ManagerEvent::BoardDone { tasks, .. }) => {
            format!("{place}: every task on the board is done ({tasks})")
        }
    }
}

/// The events of a batch worth sending: repeats folded, waits that ended
/// dropped, most pressing first (`settleManager`).
pub fn settle_manager(
    batch: &[ManagerNewsEntry],
    is_waiting: &dyn Fn(&str) -> bool,
) -> Vec<ManagerNews> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for entry in batch {
        if let ManagerNews::Event(ManagerEvent::PaneWaiting { pane_id, .. }) = &entry.event
            && !is_waiting(pane_id)
        {
            continue;
        }
        if !seen.insert(entry.event.key()) {
            continue;
        }
        out.push(entry.event.clone());
    }
    out.sort_by(|a, b| a.rank().total_cmp(&b.rank()));
    out
}

/// The message a batch becomes (`renderManagerDelta`).
pub fn render_manager_delta(events: &[ManagerNews]) -> String {
    let shown = &events[..events.len().min(MAX_LINES)];
    let mut lines: Vec<String> = shown
        .iter()
        .map(|event| format!("- {}", one_line(&describe_manager_event(event), 400)))
        .collect();
    let hidden = events.len() - shown.len();
    if hidden > 0 {
        lines.push(format!(
            "- …and {hidden} more: the HQ overview has the rest."
        ));
    }
    let head = if events.len() == 1 {
        "1 thing waits for the person:".to_string()
    } else {
        format!("{} things wait for the person:", events.len())
    };
    let mut out = vec![head];
    out.extend(lines);
    out.join("\n")
}

#[derive(Debug, Clone, Default)]
struct BoardWatch {
    /// Labels of each task as last read.
    labels: HashMap<String, Vec<String>>,
    /// Tasks blocked for the person, and whether the recheck reminder went out.
    blocked: HashMap<String, bool>,
    all_done: bool,
}

#[derive(Debug, Clone, Copy)]
struct WaitWatch {
    since: u64,
    sent: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Timer {
    at: u64,
    seq: u64,
}

/// The feed (`createManagerFeed`).
#[derive(Default)]
pub struct ManagerFeed {
    boards: HashMap<WsId, BoardWatch>,
    waits: HashMap<PaneId, WaitWatch>,
    journal: Vec<ManagerNewsEntry>,
    pending: Vec<ManagerNewsEntry>,
    first_pending_at: Option<u64>,
    last_sent_at: u64,
    timer: Option<Timer>,
    holding: bool,
    board_timer: Option<Timer>,
    seq: u64,
    stopped: bool,
    /// The phone's notifications (P20's remotePush).
    push: Option<Box<dyn ManagerSink>>,
}

impl std::fmt::Debug for ManagerFeed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ManagerFeed")
            .field("journal", &self.journal.len())
            .field("pending", &self.pending.len())
            .field("push", &self.push.is_some())
            .finish()
    }
}

impl ManagerFeed {
    pub fn new(push: Option<Box<dyn ManagerSink>>) -> Self {
        ManagerFeed {
            push,
            ..ManagerFeed::default()
        }
    }

    /// Puts the phone's push in (P20 installs it once it runs).
    pub fn set_push(&mut self, push: Option<Box<dyn ManagerSink>>) {
        self.push = push;
    }

    fn timer(&mut self, at: u64) -> Timer {
        self.seq += 1;
        Timer { at, seq: self.seq }
    }

    /// The manager, when it exists and its session runs.
    fn manager(io: &dyn EngineIo, state: &AppState) -> Option<(Workspace, Pane)> {
        let (ws, pane) = manager_of(state)?;
        io.activity(&pane.id)?;
        Some((ws.clone(), pane.clone()))
    }

    fn pushing(&self) -> bool {
        self.push.as_ref().is_some_and(|push| push.active())
    }

    /// Someone to tell: the detection runs only then.
    fn listened(&self, io: &dyn EngineIo) -> bool {
        self.pushing() || Self::manager(io, &io.state()).is_some()
    }

    fn post(&mut self, io: &dyn EngineIo, event: ManagerNews, now: u64) {
        if self.stopped {
            return;
        }
        let entry = ManagerNewsEntry { at: now, event };
        self.journal.push(entry.clone());
        if self.journal.len() > JOURNAL_SIZE {
            let over = self.journal.len() - JOURNAL_SIZE;
            self.journal.drain(..over);
        }
        if self.pushing()
            && let ManagerNews::Event(event) = &entry.event
            && let Some(push) = self.push.as_mut()
        {
            // The push's trouble is not the manager's.
            let _ = push.post(event);
        }
        if Self::manager(io, &io.state()).is_none() {
            return;
        }
        self.first_pending_at.get_or_insert(entry.at);
        self.pending.push(entry);
        if self.pending.len() > MAX_PENDING {
            let over = self.pending.len() - MAX_PENDING;
            self.pending.drain(..over);
        }
        if !self.holding {
            self.schedule(false, now);
        }
    }

    fn schedule(&mut self, hold: bool, now: u64) {
        self.holding = hold;
        let t = now as i128;
        let gap = self.last_sent_at as i128 + MIN_GAP_MS as i128 - t;
        let deadline = self.first_pending_at.map_or(t, |at| at as i128) + MAX_WAIT_MS as i128 - t;
        let wanted = if hold {
            HOLD_MS as i128
        } else {
            (DEBOUNCE_MS as i128).min(deadline)
        };
        let delay = wanted.max(gap).max(0) as u64;
        self.timer = Some(self.timer(now + delay));
    }

    fn fire(&mut self, io: &dyn EngineIo, now: u64) {
        if self.stopped {
            return;
        }
        let state = io.state();
        let Some((_, manager)) = Self::manager(io, &state) else {
            self.pending.clear();
            self.first_pending_at = None;
            return;
        };
        let activity = io.activity(&manager.id).map(|a| a.activity);
        if matches!(
            activity,
            Some(AgentActivity::Working | AgentActivity::Waiting)
        ) {
            self.schedule(true, now);
            return;
        }
        self.send(io, now);
    }

    fn send(&mut self, io: &dyn EngineIo, now: u64) -> Option<String> {
        let state = io.state();
        let found = Self::manager(io, &state);
        let batch = std::mem::take(&mut self.pending);
        self.first_pending_at = None;
        let (_, manager) = found?;
        if batch.is_empty() {
            return None;
        }
        let waiting =
            |pane: &str| io.activity(pane).map(|a| a.activity) == Some(AgentActivity::Waiting);
        let events = settle_manager(&batch, &waiting);
        if events.is_empty() {
            return None;
        }
        let text = render_manager_delta(&events);
        self.last_sent_at = now;
        let _ = io.enqueue(&manager.id, &text, "hangar");
        Some(text)
    }

    fn read_board(&mut self, io: &dyn EngineIo, ws: &Workspace, now: u64) -> Result<(), String> {
        let tasks: Vec<Task> = io.load_board(&ws.cwd)?;
        let next = BoardWatch {
            labels: tasks
                .iter()
                .map(|task| (task.id.clone(), task.labels.clone()))
                .collect(),
            blocked: tasks
                .iter()
                .filter_map(|task| {
                    let blocked = task.blocked.as_ref().filter(|blocked| blocked.human_only)?;
                    Some((task.id.clone(), blocked.reminded.is_some_and(|at| at != 0)))
                })
                .collect(),
            all_done: !tasks.is_empty() && tasks.iter().all(|task| task.column == "done"),
        };
        let before = self.boards.insert(ws.id.clone(), next.clone());
        // The first read is the baseline: what already waited is not news.
        let Some(before) = before else {
            return Ok(());
        };
        let labelled = |labels: &[String], label: &str| labels.iter().any(|l| l == label);
        for task in &tasks {
            let Some(was) = before.labels.get(&task.id) else {
                continue;
            };
            let was_blocked = before.blocked.get(&task.id).copied();
            let human = task.blocked.as_ref().filter(|blocked| blocked.human_only);
            let (workspace_id, workspace, task_id, title) = (
                ws.id.clone(),
                ws.name.clone(),
                task.id.clone(),
                task.title.clone(),
            );
            if let (Some(blocked), None) = (human, was_blocked) {
                let event = BlockEvent::TaskBlocked {
                    workspace_id,
                    workspace,
                    task_id,
                    title,
                    by: if blocked.by.is_empty() {
                        "an agent".into()
                    } else {
                        blocked.by.clone()
                    },
                    reason: one_line(&blocked.reason, MAX_TEXT),
                };
                self.post(io, ManagerNews::Block(event), now);
            } else if task.blocked.is_none() && was_blocked.is_some() {
                let how = match task.comments.last() {
                    Some(last) if is_hangar_author(&last.author) => one_line(&last.text, MAX_TEXT),
                    _ => "unblocked".into(),
                };
                let event = BlockEvent::TaskUnblocked {
                    workspace_id,
                    workspace,
                    task_id,
                    title,
                    how,
                };
                self.post(io, ManagerNews::Block(event), now);
            } else if let (Some(blocked), Some(false)) = (human, was_blocked)
                && blocked.reminded.is_some_and(|at| at != 0)
            {
                let event = BlockEvent::TaskRecheckDue {
                    workspace_id,
                    workspace,
                    task_id,
                    title,
                    reason: one_line(&blocked.reason, MAX_TEXT),
                };
                self.post(io, ManagerNews::Block(event), now);
            }
            if labelled(&task.labels, NEEDS_YOU_LABEL) && !labelled(was, NEEDS_YOU_LABEL) {
                let reason = match task.comments.last() {
                    Some(last) => format!("{}: {}", last.author, one_line(&last.text, MAX_TEXT)),
                    None => format!("labelled {NEEDS_YOU_LABEL}"),
                };
                let event = ManagerEvent::TaskNeedsYou {
                    workspace_id: ws.id.clone(),
                    workspace: ws.name.clone(),
                    task_id: task.id.clone(),
                    title: task.title.clone(),
                    reason,
                };
                self.post(io, ManagerNews::Event(event), now);
            }
            if labelled(&task.labels, READY_TO_LAND_LABEL) && !labelled(was, READY_TO_LAND_LABEL) {
                let event = ManagerEvent::ReadyToLand {
                    workspace_id: ws.id.clone(),
                    workspace: ws.name.clone(),
                    task_id: task.id.clone(),
                    title: task.title.clone(),
                };
                self.post(io, ManagerNews::Event(event), now);
            }
        }
        if next.all_done && !before.all_done {
            let event = ManagerEvent::BoardDone {
                workspace_id: ws.id.clone(),
                workspace: ws.name.clone(),
                tasks: tasks.len(),
            };
            self.post(io, ManagerNews::Event(event), now);
        }
        Ok(())
    }

    fn watched(state: &AppState) -> Vec<Workspace> {
        state
            .workspaces
            .iter()
            .filter(|ws| ws.hq != Some(true))
            .cloned()
            .collect()
    }

    fn read_all(&mut self, io: &dyn EngineIo, now: u64) {
        let spaces = Self::watched(&io.state());
        let ids: HashSet<&str> = spaces.iter().map(|ws| ws.id.as_str()).collect();
        self.boards.retain(|id, _| ids.contains(id.as_str()));
        for ws in &spaces {
            let _ = self.read_board(io, ws, now);
        }
    }

    fn watch_panes(&mut self, io: &dyn EngineIo, now: u64) {
        let mut present = HashSet::new();
        for ws in Self::watched(&io.state()) {
            for pane in &ws.panes {
                if pane.role == Some(PaneRole::Manager) {
                    continue;
                }
                if io.activity(&pane.id).map(|a| a.activity) != Some(AgentActivity::Waiting) {
                    continue;
                }
                present.insert(pane.id.clone());
                let w = self.waits.entry(pane.id.clone()).or_insert(WaitWatch {
                    since: now,
                    sent: false,
                });
                if !w.sent && now.saturating_sub(w.since) >= WAITING_MS {
                    w.sent = true;
                    let event = ManagerEvent::PaneWaiting {
                        workspace_id: ws.id.clone(),
                        workspace: ws.name.clone(),
                        pane_id: pane.id.clone(),
                        pane: pane.name.clone(),
                        for_ms: now - w.since,
                    };
                    self.post(io, ManagerNews::Event(event), now);
                }
            }
        }
        self.waits.retain(|id, _| present.contains(id));
    }

    /// Reads the boards, looks at the panes: called on every tick.
    pub fn tick(&mut self, io: &dyn EngineIo, now: u64) {
        if self.stopped {
            return;
        }
        if !self.listened(io) {
            // Nobody to tell: forget what was gathered, and start over when
            // there is.
            self.pending.clear();
            self.first_pending_at = None;
            self.boards.clear();
            self.waits.clear();
            return;
        }
        self.read_all(io, now);
        self.watch_panes(io, now);
    }

    /// An agent changed a board: they are read again shortly.
    pub fn board_changed(&mut self, io: &dyn EngineIo, now: u64) {
        if self.stopped || self.board_timer.is_some() || !self.listened(io) {
            return;
        }
        self.board_timer = Some(self.timer(now + BOARD_SETTLE_MS));
    }

    /// Sends what is pending now, waits and holds aside (tests, `flush`).
    pub fn flush(&mut self, io: &dyn EngineIo, now: u64) -> Option<String> {
        self.timer = None;
        self.send(io, now)
    }

    /// What lately waited for the person, oldest first, block events left
    /// out: the manager's briefing and the phone's state (`managerJournal`).
    pub fn journal(&self) -> Vec<ManagerJournalEntry> {
        self.journal
            .iter()
            .filter_map(|entry| match &entry.event {
                ManagerNews::Event(event) => Some(ManagerJournalEntry {
                    at: entry.at,
                    event: event.clone(),
                }),
                ManagerNews::Block(_) => None,
            })
            .collect()
    }

    /// The same with the block events: the activity log tells them too
    /// (`managerNews`).
    pub fn news(&self) -> Vec<ManagerNewsEntry> {
        self.journal.clone()
    }

    pub fn stop(&mut self) {
        self.stopped = true;
        self.timer = None;
        self.board_timer = None;
        self.boards.clear();
        self.waits.clear();
    }

    pub fn next_wake(&self) -> Option<u64> {
        [self.timer, self.board_timer]
            .into_iter()
            .flatten()
            .min_by_key(|timer| (timer.at, timer.seq))
            .map(|timer| timer.at)
    }

    /// Fires the earliest timer due by `now`; whether one fired.
    pub fn fire_due(&mut self, io: &dyn EngineIo, now: u64) -> bool {
        let batch = self.timer.filter(|timer| timer.at <= now);
        let board = self.board_timer.filter(|timer| timer.at <= now);
        match (batch, board) {
            (Some(batch), Some(board)) if (board.at, board.seq) < (batch.at, batch.seq) => {
                self.board_timer = None;
                self.read_all(io, now);
            }
            (Some(_), _) => {
                self.timer = None;
                self.holding = false;
                self.fire(io, now);
            }
            (None, Some(_)) => {
                self.board_timer = None;
                self.read_all(io, now);
            }
            (None, None) => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests;
