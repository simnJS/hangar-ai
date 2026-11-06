//! The `OrchestrationRuntime`: one tokio task, started on hydration, that
//! owns what the TS kept in module state (inventory A6, "Async shape", 5) —
//! the orchestrators' event engine, the manager feed, the starts under way —
//! and drives the engines with the real clock.
//!
//! Its inputs are the core events the package hands it (`EngineMsg`: bridge
//! reports, a pane's shell ending, a board written), the engines' own
//! deadlines, and two ticks: every 10 s the boards are read and the workers
//! and panes looked at (`useOrchestrator`'s `TICK_MS`), and every 5 s what
//! Hangar Mobile plugs in runs (its activity log, P20). The engines are
//! synchronous state machines over `EngineIo`; every call that may read a
//! board goes to the blocking pool, so the async threads never wait on a
//! file, and reads its boards before it locks the engines, so they never
//! wait on one through the lock either.
//!
//! The rest of the app reaches the runtime through `OrchestrationHandle`,
//! which `orchestration::Live` holds: P18's control bus for the `worker_*`
//! ops, the briefing and `postOrchestratorEvent` (deliveries), P20 for the
//! journals, the projects and the push sink, this package for the person's
//! « Arrêter les workers » and a role given or taken.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde_json::Value;
use tokio::sync::{Notify, mpsc};

use hangar_core::board::Task;

use super::brief;
use super::events::{EngineIo, JournalEntry, OrchestratorEngine, OrchestratorEvent};
use super::live::LivePort;
use super::manager_feed::{ManagerFeed, ManagerJournalEntry, ManagerNewsEntry, ManagerSink};
use super::port::Caller;
use super::projects::{self, ProjectSummary};
use super::text::now_ms;
use super::worker::{self, Starting};
use crate::services::Services;
use crate::state::PaneActivity;
use crate::state::live::BridgeEvent;
use crate::state::types::{AppState, PaneId, PaneRole, Workspace};

/// How often the watched boards are read and the workers looked at.
const TICK: Duration = Duration::from_secs(10);
/// How often what Hangar Mobile plugged in runs (its activity log).
const FIVE_SECONDS: Duration = Duration::from_secs(5);
/// Timers fired at one wake, at most: one that re-arms itself at once would
/// otherwise hold the blocking thread for good.
const MAX_FIRED: usize = 64;

/// What the package hands the runtime from the core events it is offered.
#[derive(Debug, Clone)]
pub enum EngineMsg {
    Bridge {
        pane: PaneId,
        event: BridgeEvent,
    },
    PtyExit(PaneId),
    /// An agent or a worker's claim wrote a board.
    BoardChanged,
}

/// Something run every 5 s with the handle, off the engines' lock: P20's
/// activity log reads the journals with it.
pub type Ticker = Box<dyn FnMut(&OrchestrationHandle, u64) + Send>;

/// Both engines, behind one lock.
#[derive(Debug, Default)]
pub struct Engines {
    pub events: OrchestratorEngine,
    pub manager: ManagerFeed,
}

impl Engines {
    /// Every board a tick or a timer of either engine reads, once each.
    fn watched_boards(&self, io: &dyn EngineIo) -> Vec<String> {
        let mut cwds = self.events.watched_boards(io);
        for cwd in self.manager.watched_boards(io) {
            if !cwds.contains(&cwd) {
                cwds.push(cwd);
            }
        }
        cwds
    }
}

/// Whether a piece of the engines' work reads boards.
#[derive(Debug, Clone, Copy)]
enum Reads {
    Nothing,
    Boards,
}

/// The engines' io for one piece of their work: the boards it reads, read
/// before the engines were locked, and the rest of `io` as it is. A board
/// nobody expected (a workspace pinned an orchestrator in between) is read
/// as before, under the lock: late, never missed.
struct Prefetched<'a> {
    io: &'a dyn EngineIo,
    boards: HashMap<String, Result<Vec<Task>, String>>,
}

impl<'a> Prefetched<'a> {
    fn read(io: &'a dyn EngineIo, cwds: Vec<String>) -> Self {
        let mut boards = HashMap::new();
        for cwd in cwds {
            if let std::collections::hash_map::Entry::Vacant(entry) = boards.entry(cwd) {
                let read = io.load_board(entry.key());
                entry.insert(read);
            }
        }
        Prefetched { io, boards }
    }
}

impl EngineIo for Prefetched<'_> {
    fn state(&self) -> Arc<AppState> {
        self.io.state()
    }

    fn activity(&self, pane: &str) -> Option<PaneActivity> {
        self.io.activity(pane)
    }

    fn load_board(&self, cwd: &str) -> Result<Vec<Task>, String> {
        match self.boards.get(cwd) {
            Some(read) => read.clone(),
            None => self.io.load_board(cwd),
        }
    }

    fn enqueue(&self, pane: &str, text: &str, from: &str) -> Result<(), String> {
        self.io.enqueue(pane, text, from)
    }
}

struct Shared {
    services: Services,
    engines: Mutex<Engines>,
    starting: Mutex<Starting>,
    /// Rung when a deadline may have moved closer (an event posted).
    wake: Notify,
    tickers: Mutex<Vec<Ticker>>,
    /// The workspace folders whose worktrees were swept already.
    swept: Mutex<std::collections::HashSet<String>>,
}

/// Every folder a pane runs in, in any workspace (a pane without a folder of
/// its own runs in its workspace's), plus the workspaces' own folders: a
/// worktree under one of them is in use.
fn panes_folders(state: &AppState) -> Vec<String> {
    let mut folders = Vec::new();
    for ws in &state.workspaces {
        folders.push(ws.cwd.clone());
        folders.extend(ws.extra_roots.iter().cloned());
        for pane in &ws.panes {
            folders.push(pane.cwd.clone().unwrap_or_else(|| ws.cwd.clone()));
        }
    }
    folders.retain(|folder| !folder.is_empty());
    folders
}

/// The way into the runtime, cloned wherever it is needed.
#[derive(Clone)]
pub struct OrchestrationHandle {
    shared: Arc<Shared>,
    tx: mpsc::UnboundedSender<EngineMsg>,
}

impl std::fmt::Debug for OrchestrationHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OrchestrationHandle(..)")
    }
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// Starts the runtime on the current tokio runtime. Called once, on
/// hydration, from the package's `update`.
pub fn start(services: Services) -> OrchestrationHandle {
    let (tx, rx) = mpsc::unbounded_channel();
    let handle = OrchestrationHandle {
        shared: Arc::new(Shared {
            services,
            engines: Mutex::new(Engines::default()),
            starting: Mutex::new(Starting::default()),
            wake: Notify::new(),
            tickers: Mutex::new(Vec::new()),
            swept: Mutex::new(Default::default()),
        }),
        tx,
    };
    tokio::spawn(run(handle.clone(), rx));
    handle
}

/// The runtime's loop. It ends when the package's handle and every clone of
/// it are gone, which is when the app is.
async fn run(handle: OrchestrationHandle, mut rx: mpsc::UnboundedReceiver<EngineMsg>) {
    let mut tick = tokio::time::interval(TICK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut five = tokio::time::interval(FIVE_SECONDS);
    five.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        let wake = handle.next_wake();
        let deadline = async {
            match wake {
                Some(at) => {
                    tokio::time::sleep(Duration::from_millis(at.saturating_sub(now_ms()))).await
                }
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            message = rx.recv() => match message {
                Some(message) => handle.on_message(message).await,
                None => break,
            },
            () = deadline => handle.fire_due().await,
            // The first tick is at once: the TS ticked on start too.
            _ = tick.tick() => handle.tick().await,
            _ = five.tick() => handle.every_five_seconds(),
            () = handle.shared.wake.notified() => {}
        }
    }
}

impl OrchestrationHandle {
    pub(super) fn services(&self) -> &Services {
        &self.shared.services
    }

    pub(super) fn starting(&self) -> &Mutex<Starting> {
        &self.shared.starting
    }

    /// The ports the engine's code reads and acts through.
    pub fn port(&self) -> LivePort {
        LivePort::new(self.clone())
    }

    /// The ports for an operation on workspace `ws`: on its node, when it
    /// lives on one.
    fn port_for(&self, ws: &Workspace) -> LivePort {
        LivePort::on(self.clone(), ws.node.clone())
    }

    /// Hands a core event over (from the package's `on_event`).
    pub fn send(&self, message: EngineMsg) {
        // The runtime gone, the app is closing: nothing to tell.
        let _ = self.tx.send(message);
    }

    fn engines(&self) -> MutexGuard<'_, Engines> {
        locked(&self.shared.engines)
    }

    fn next_wake(&self) -> Option<u64> {
        let engines = self.engines();
        [engines.events.next_wake(), engines.manager.next_wake()]
            .into_iter()
            .flatten()
            .min()
    }

    /// Runs `work` on the engines, on the blocking pool. With `Reads::Boards`
    /// the boards it reads are read first, with the engines' lock let go:
    /// `post`, the journals and the briefings take that lock from the async
    /// threads, and would otherwise wait out every board of every workspace
    /// on each tick.
    async fn with_engines(
        &self,
        reads: Reads,
        work: impl FnOnce(&mut Engines, &dyn EngineIo, u64) + Send + 'static,
    ) {
        let handle = self.clone();
        let done = tokio::task::spawn_blocking(move || {
            let port = handle.port();
            let wanted = match reads {
                // Asked under the lock, which reads nothing, and let go.
                Reads::Boards => handle.engines().watched_boards(&port),
                Reads::Nothing => Vec::new(),
            };
            // The remote boards are read from their nodes here, on the
            // blocking pool, for the engines' synchronous reads.
            port.refresh_remote_boards(&wanted);
            let io = Prefetched::read(&port, wanted);
            let mut engines = handle.engines();
            work(&mut engines, &io, now_ms());
        })
        .await;
        if let Err(error) = done {
            eprintln!("the orchestration engine stopped on an error: {error}");
        }
    }

    async fn on_message(&self, message: EngineMsg) {
        // A report or an exit posts events and sets timers; no board is read.
        self.with_engines(Reads::Nothing, move |engines, port, now| match message {
            EngineMsg::Bridge { pane, event } => {
                engines.events.bridge_event(port, &pane, &event, now)
            }
            EngineMsg::PtyExit(pane) => engines.events.pty_exited(port, &pane, now),
            EngineMsg::BoardChanged => {
                engines.events.board_changed(now);
                engines.manager.board_changed(port, now);
            }
        })
        .await;
    }

    async fn fire_due(&self) {
        self.with_engines(Reads::Boards, |engines, port, now| {
            for _ in 0..MAX_FIRED {
                if !engines.events.fire_due(port, now) {
                    break;
                }
            }
            for _ in 0..MAX_FIRED {
                if !engines.manager.fire_due(port, now) {
                    break;
                }
            }
        })
        .await;
    }

    /// Every 10 s: the orchestrators' boards and workers, then what waits
    /// for the person (`engine.tick()`, then `manager.tick`).
    async fn tick(&self) {
        self.with_engines(Reads::Boards, |engines, port, now| {
            engines.events.tick(port, now);
            engines.manager.tick(port, now);
        })
        .await;
        self.sweep_new_workspaces().await;
    }

    /// Removes the worktrees nobody uses in each workspace folder seen for the
    /// first time: the first tick after the state was loaded covers every
    /// workspace of the app, a later one the workspace that was opened since.
    /// Only the worktrees of tasks done on the workspace's board that no live
    /// process stands in can go (`sweep::sweep_workspace`).
    async fn sweep_new_workspaces(&self) {
        let state = self.port().state();
        let (fresh, held) = {
            let mut done = locked(&self.shared.swept);
            let fresh: Vec<String> = state
                .workspaces
                .iter()
                // A remote workspace's folder is a path on its node.
                .filter(|ws| ws.node.is_none())
                .filter(|ws| !ws.cwd.is_empty() && done.insert(ws.cwd.clone()))
                .map(|ws| ws.cwd.clone())
                .collect();
            (fresh, panes_folders(&state))
        };
        if fresh.is_empty() {
            return;
        }
        // Not a repository, or git is missing: nothing listed, nothing swept.
        let mut listed = Vec::with_capacity(fresh.len());
        for cwd in fresh {
            let trees = hangar_core::git::git_worktree_list(cwd.clone())
                .await
                .unwrap_or_default();
            listed.push((cwd, trees));
        }
        let swept = tokio::task::spawn_blocking(move || {
            // Who stands where, once for every workspace: a pane of another
            // Hangar is in no `held` of this one.
            let dirs = hangar_core::delivery::occupied::current_dirs();
            let mut seen = Vec::new();
            for (cwd, trees) in listed {
                let tasks = hangar_core::board_load(cwd.clone()).tasks;
                if let Ok(sweep) =
                    super::sweep::sweep_workspace(&cwd, &trees, &held, &tasks, dirs.as_deref())
                {
                    // Two workspaces of one repository say it once.
                    for line in sweep.removed.into_iter().chain(sweep.kept) {
                        if !seen.contains(&line) {
                            eprintln!("worktree sweep ({cwd}): {line}");
                            seen.push(line);
                        }
                    }
                }
            }
        })
        .await;
        if let Err(error) = swept {
            eprintln!("the worktree sweep stopped on an error: {error}");
        }
    }

    fn every_five_seconds(&self) {
        // Taken out so a ticker may use the handle, which locks nothing it holds.
        let mut tickers = std::mem::take(&mut *locked(&self.shared.tickers));
        let now = now_ms();
        for ticker in &mut tickers {
            ticker(self, now);
        }
        locked(&self.shared.tickers).extend(tickers);
    }

    /* ── What the rest of the app calls ─────────────────────────── */

    /// Hands `event` to the workspace's orchestrator, if it has one
    /// (`postOrchestratorEvent`): the deliveries post theirs here (P18).
    pub fn post(&self, workspace_id: &str, event: OrchestratorEvent) {
        let port = self.port();
        self.engines()
            .events
            .post(&port, workspace_id, event, false, now_ms());
        self.shared.wake.notify_one();
    }

    /// What happened lately in the workspace, oldest first
    /// (`orchestratorJournal`).
    pub fn journal(&self, workspace_id: &str) -> Vec<JournalEntry> {
        self.engines().events.journal(workspace_id)
    }

    /// The workspace's board as the engine last read it.
    pub fn tasks(&self, workspace_id: &str) -> Vec<Task> {
        self.engines().events.tasks(workspace_id)
    }

    /// What lately waited for the person (`managerJournal`).
    pub fn manager_journal(&self) -> Vec<ManagerJournalEntry> {
        self.engines().manager.journal()
    }

    /// The same with the block events (`managerNews`).
    pub fn manager_news(&self) -> Vec<ManagerNewsEntry> {
        self.engines().manager.news()
    }

    /// Puts the phone's push beside the manager (P20's remotePush). The sink
    /// is called with the engines locked: it queues what it is given and
    /// returns, and never calls back into this handle.
    pub fn set_push(&self, push: Box<dyn ManagerSink>) {
        self.engines().manager.set_push(Some(push));
    }

    /// Runs `ticker` every 5 s from now on (P20's activity log).
    pub fn add_ticker(&self, ticker: Ticker) {
        locked(&self.shared.tickers).push(ticker);
    }

    /// A `worker_*` op, for the workspace P18's dispatch resolved.
    pub async fn worker_control(
        &self,
        op: &str,
        ws: &Workspace,
        args: &Value,
        caller: &Caller,
    ) -> Result<Value, String> {
        worker::worker_control(op, ws, args, caller, &self.port_for(ws)).await
    }

    /// The person's « Arrêter les workers »: how many were stopped.
    pub async fn stop_all_workers(&self, workspace_id: &str) -> usize {
        let state = self.port().state();
        let port = match state.workspaces.iter().find(|ws| ws.id == workspace_id) {
            Some(ws) => self.port_for(ws),
            None => self.port(),
        };
        worker::stop_all_workers(workspace_id, &port).await
    }

    /// The answer to the internal request `orchestrator_briefing {paneId}`:
    /// a pane of a remote workspace is briefed from its node's board and
    /// memory.
    pub async fn briefing(&self, args: &Value) -> Result<Value, String> {
        let pane = args
            .get("paneId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let node = crate::nodes::engine::node_of_pane(&self.port().state(), pane);
        brief::answer_briefing(args, &LivePort::on(self.clone(), node)).await
    }

    /// Tells a running session the role its pane was just given or lost.
    pub async fn announce_role(
        &self,
        workspace_id: &str,
        pane_id: &str,
        previous: Option<PaneRole>,
    ) -> Result<bool, String> {
        brief::announce_role(workspace_id, pane_id, previous, &self.port()).await
    }

    /// Every project but the HQ, summarized (P18's `project_*`, P20's
    /// `remote_state`).
    pub async fn load_projects(&self) -> Vec<ProjectSummary> {
        projects::load_projects(&self.port()).await
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::orchestration::testkit::{START, pane, task, workspace};
    use crate::state::AgentActivity;

    /// Boards on disk, counted as they are read.
    struct Disk {
        state: AppState,
        reads: Cell<usize>,
    }

    impl EngineIo for Disk {
        fn state(&self) -> Arc<AppState> {
            Arc::new(self.state.clone())
        }

        /// The manager's session runs, so the feed listens.
        fn activity(&self, pane: &str) -> Option<PaneActivity> {
            (pane == "m").then(|| PaneActivity {
                workspace_id: "hq".into(),
                activity: AgentActivity::Idle,
                bridged: true,
            })
        }

        fn load_board(&self, _cwd: &str) -> Result<Vec<Task>, String> {
            self.reads.set(self.reads.get() + 1);
            Ok(vec![task("t1")])
        }

        fn enqueue(&self, _pane: &str, _text: &str, _from: &str) -> Result<(), String> {
            Ok(())
        }
    }

    fn disk() -> Disk {
        let hq = Workspace {
            hq: Some(true),
            ..workspace("hq", "HQ", vec![pane("m", "Max", Some(PaneRole::Manager))])
        };
        Disk {
            state: AppState {
                workspaces: vec![
                    workspace(
                        "shop",
                        "Shop",
                        vec![pane("o", "Ava", Some(PaneRole::Orchestrator))],
                    ),
                    workspace("blog", "Blog", vec![pane("x", "Leo", None)]),
                    hq,
                ],
                ..AppState::default()
            },
            reads: Cell::new(0),
        }
    }

    #[test]
    fn reads_every_board_a_tick_needs_before_the_engines_are_locked() {
        let disk = disk();
        let mut engines = Engines::default();
        // The orchestrator's board and every project's but the HQ's, once.
        let wanted = engines.watched_boards(&disk);
        assert_eq!(wanted, ["C:\\dev\\shop", "C:\\dev\\blog"]);
        let io = Prefetched::read(&disk, wanted);
        assert_eq!(disk.reads.get(), 2);
        // What runs with the engines locked reads nothing from the disk.
        engines.events.tick(&io, START);
        engines.manager.tick(&io, START);
        engines.events.board_changed(START);
        engines.manager.board_changed(&io, START);
        let later = START + 60_000;
        while engines.events.fire_due(&io, later) {}
        while engines.manager.fire_due(&io, later) {}
        assert_eq!(disk.reads.get(), 2);
        // And the engines did read them.
        assert_eq!(engines.events.tasks("shop").len(), 1);
    }

    #[test]
    fn reads_a_board_nobody_expected_late_rather_than_never() {
        let disk = disk();
        let io = Prefetched::read(&disk, Vec::new());
        assert_eq!(
            io.load_board("C:\\dev\\shop").map(|tasks| tasks.len()),
            Ok(1)
        );
        assert_eq!(disk.reads.get(), 1);
    }

    #[test]
    fn a_stopped_engine_and_a_feed_nobody_hears_want_no_board() {
        let mut disk = disk();
        disk.state.workspaces.retain(|ws| ws.hq != Some(true));
        let mut engines = Engines::default();
        assert_eq!(engines.watched_boards(&disk), ["C:\\dev\\shop"]);
        engines.events.stop();
        assert!(engines.watched_boards(&disk).is_empty());
    }
}
