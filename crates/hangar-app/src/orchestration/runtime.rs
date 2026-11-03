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
//! file.
//!
//! The rest of the app reaches the runtime through `OrchestrationHandle`,
//! which `orchestration::Live` holds: P18's control bus for the `worker_*`
//! ops, the briefing and `postOrchestratorEvent` (deliveries), P20 for the
//! journals, the projects and the push sink, this package for the person's
//! « Arrêter les workers » and a role given or taken.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde_json::Value;
use tokio::sync::{Notify, mpsc};

use hangar_core::board::Task;

use super::brief;
use super::events::{JournalEntry, OrchestratorEngine, OrchestratorEvent};
use super::live::LivePort;
use super::manager_feed::{ManagerFeed, ManagerJournalEntry, ManagerNewsEntry, ManagerSink};
use super::port::Caller;
use super::projects::{self, ProjectSummary};
use super::text::now_ms;
use super::worker::{self, Starting};
use crate::services::Services;
use crate::state::live::BridgeEvent;
use crate::state::types::{PaneId, PaneRole, Workspace};

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
    Bridge { pane: PaneId, event: BridgeEvent },
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

struct Shared {
    services: Services,
    engines: Mutex<Engines>,
    starting: Mutex<Starting>,
    /// Rung when a deadline may have moved closer (an event posted).
    wake: Notify,
    tickers: Mutex<Vec<Ticker>>,
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
                Some(at) => tokio::time::sleep(Duration::from_millis(at.saturating_sub(now_ms()))).await,
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

    /// Runs `work` on the engines, on the blocking pool: it may read boards.
    async fn with_engines(&self, work: impl FnOnce(&mut Engines, &LivePort, u64) + Send + 'static) {
        let handle = self.clone();
        let done = tokio::task::spawn_blocking(move || {
            let port = handle.port();
            let mut engines = handle.engines();
            work(&mut engines, &port, now_ms());
        })
        .await;
        if let Err(error) = done {
            eprintln!("the orchestration engine stopped on an error: {error}");
        }
    }

    async fn on_message(&self, message: EngineMsg) {
        self.with_engines(move |engines, port, now| match message {
            EngineMsg::Bridge { pane, event } => engines.events.bridge_event(port, &pane, &event, now),
            EngineMsg::PtyExit(pane) => engines.events.pty_exited(port, &pane, now),
            EngineMsg::BoardChanged => {
                engines.events.board_changed(now);
                engines.manager.board_changed(port, now);
            }
        })
        .await;
    }

    async fn fire_due(&self) {
        self.with_engines(|engines, port, now| {
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
        self.with_engines(|engines, port, now| {
            engines.events.tick(port, now);
            engines.manager.tick(port, now);
        })
        .await;
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
        self.engines().events.post(&port, workspace_id, event, false, now_ms());
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
    pub async fn worker_control(&self, op: &str, ws: &Workspace, args: &Value, caller: &Caller) -> Result<Value, String> {
        worker::worker_control(op, ws, args, caller, &self.port()).await
    }

    /// The person's « Arrêter les workers »: how many were stopped.
    pub async fn stop_all_workers(&self, workspace_id: &str) -> usize {
        worker::stop_all_workers(workspace_id, &self.port()).await
    }

    /// The answer to the internal request `orchestrator_briefing {paneId}`.
    pub async fn briefing(&self, args: &Value) -> Result<Value, String> {
        brief::answer_briefing(args, &self.port()).await
    }

    /// Tells a running session the role its pane was just given or lost.
    pub async fn announce_role(&self, workspace_id: &str, pane_id: &str, previous: Option<PaneRole>) -> Result<bool, String> {
        brief::announce_role(workspace_id, pane_id, previous, &self.port()).await
    }

    /// Every project but the HQ, summarized (P18's `project_*`, P20's
    /// `remote_state`).
    pub async fn load_projects(&self) -> Vec<ProjectSummary> {
        projects::load_projects(&self.port()).await
    }
}
