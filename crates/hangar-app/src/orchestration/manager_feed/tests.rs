//! managerEvents.test.ts, case for case.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use hangar_core::board::Task;

use super::*;
use crate::orchestration::testkit::{START, Timed, advance, blocked, comment, pane, strings, task, workspace};
use crate::state::types::{AppState, PaneRole};
use crate::state::{AgentActivity, PaneActivity};

#[derive(Debug, Default)]
struct Push {
    on: bool,
    posted: Vec<ManagerEvent>,
    broken: bool,
}

struct Sink(Arc<Mutex<Push>>);

impl ManagerSink for Sink {
    fn active(&self) -> bool {
        self.0.lock().unwrap().on
    }

    fn post(&mut self, event: &ManagerEvent) -> Result<(), String> {
        let mut push = self.0.lock().unwrap();
        if push.broken {
            return Err("no phone".into());
        }
        push.posted.push(event.clone());
        Ok(())
    }
}

struct World {
    state: AppState,
    activities: HashMap<String, AgentActivity>,
    boards: HashMap<String, Vec<Task>>,
    sent: RefCell<Vec<(String, String, String)>>,
    reads: Cell<usize>,
}

impl EngineIo for World {
    fn state(&self) -> Arc<AppState> {
        Arc::new(self.state.clone())
    }

    fn activity(&self, pane: &str) -> Option<PaneActivity> {
        self.activities.get(pane).map(|activity| PaneActivity {
            workspace_id: "x".into(),
            activity: *activity,
            bridged: true,
        })
    }

    fn load_board(&self, cwd: &str) -> Result<Vec<Task>, String> {
        self.reads.set(self.reads.get() + 1);
        Ok(self.boards.get(cwd).cloned().unwrap_or_default())
    }

    fn enqueue(&self, pane: &str, text: &str, from: &str) -> Result<(), String> {
        self.sent.borrow_mut().push((pane.into(), text.into(), from.into()));
        Ok(())
    }
}

struct Rig {
    feed: ManagerFeed,
    io: World,
    push: Arc<Mutex<Push>>,
    t: u64,
}

impl Timed for Rig {
    fn next_wake(&self) -> Option<u64> {
        self.feed.next_wake()
    }

    fn fire_due(&mut self, now: u64) {
        self.feed.fire_due(&self.io, now);
    }
}

impl Rig {
    fn tick(&mut self) {
        self.feed.tick(&self.io, self.t);
    }

    fn advance(&mut self, ms: u64) {
        let mut t = self.t;
        advance(&mut t, ms, self);
        self.t = t;
    }

    fn sent(&self) -> Vec<(String, String, String)> {
        self.io.sent.borrow().clone()
    }

    fn board(&mut self, id: &str, tasks: Vec<Task>) {
        self.io.boards.insert(format!("C:\\dev\\{id}"), tasks);
    }

    fn activity(&mut self, pane: &str, activity: AgentActivity) {
        self.io.activities.insert(pane.into(), activity);
    }

    fn posted(&self) -> Vec<ManagerEvent> {
        self.push.lock().unwrap().posted.clone()
    }

    fn push_on(&self, on: bool) {
        self.push.lock().unwrap().on = on;
    }
}

fn rig_with(manager: bool, push: bool) -> Rig {
    let mut hq = workspace(
        "hq",
        "HQ",
        vec![
            pane("m", "manager", manager.then_some(PaneRole::Manager)),
            pane("hw", "hq-agent", None),
        ],
    );
    hq.hq = Some(true);
    let state = AppState {
        workspaces: vec![
            hq,
            workspace(
                "a",
                "Shop",
                vec![
                    pane("o", "orch", Some(PaneRole::Orchestrator)),
                    pane("w1", "worker-1", Some(PaneRole::Worker)),
                ],
            ),
            workspace("b", "Blog", vec![pane("p", "solo", None)]),
        ],
        active_workspace_id: Some("a".into()),
        ..AppState::default()
    };
    let shared = Arc::new(Mutex::new(Push { on: push, ..Push::default() }));
    Rig {
        feed: ManagerFeed::new(Some(Box::new(Sink(shared.clone())))),
        io: World {
            state,
            activities: HashMap::from([("m".to_string(), AgentActivity::Idle)]),
            boards: HashMap::from([
                ("C:\\dev\\hq".to_string(), vec![task("h1")]),
                (
                    "C:\\dev\\a".to_string(),
                    vec![Task { column: "doing".into(), ..task("t1") }, task("t2")],
                ),
                ("C:\\dev\\b".to_string(), vec![Task { column: "doing".into(), ..task("u1") }]),
            ]),
            sent: RefCell::new(Vec::new()),
            reads: Cell::new(0),
        },
        push: shared,
        t: START,
    }
}

fn rig() -> Rig {
    rig_with(true, false)
}

fn labelled(id: &str, labels: &[&str]) -> Task {
    Task { labels: strings(labels), ..task(id) }
}

/* ── manager feed ──────────────────────────────────────────────── */

#[test]
fn tells_a_pane_waiting_for_the_person_once_with_workspace_and_pane() {
    let mut r = rig();
    r.tick();
    r.activity("w1", AgentActivity::Waiting);
    r.tick();
    r.advance(WAITING_MS);
    r.tick();
    r.advance(MAX_WAIT_MS);
    let sent = r.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!((sent[0].0.as_str(), sent[0].2.as_str()), ("m", "hangar"));
    assert!(sent[0].1.contains("Workspace \"Shop\""));
    assert!(sent[0].1.contains("\"worker-1\""));
    r.tick();
    r.advance(MAX_WAIT_MS);
    assert_eq!(r.sent().len(), 1);
}

#[test]
fn covers_an_orchestrator_and_a_workspace_that_has_none() {
    let mut r = rig();
    r.tick();
    r.activity("o", AgentActivity::Waiting);
    r.activity("p", AgentActivity::Waiting);
    r.tick();
    r.advance(WAITING_MS);
    r.tick();
    r.advance(MAX_WAIT_MS);
    let sent = r.sent();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].1.contains("\"orch\""));
    assert!(sent[0].1.contains("Workspace \"Blog\": pane \"solo\""));
}

#[test]
fn does_not_say_a_wait_shorter_than_the_threshold() {
    let mut r = rig();
    r.tick();
    r.activity("w1", AgentActivity::Waiting);
    r.tick();
    r.advance(WAITING_MS - 1_000);
    r.tick();
    r.advance(MAX_WAIT_MS);
    assert!(r.sent().is_empty());
}

#[test]
fn drops_a_wait_that_ended_before_the_batch_goes() {
    let mut r = rig();
    r.tick();
    r.activity("w1", AgentActivity::Waiting);
    r.tick();
    r.advance(WAITING_MS);
    r.tick();
    r.activity("w1", AgentActivity::Working);
    r.advance(MAX_WAIT_MS);
    assert!(r.sent().is_empty());
}

#[test]
fn tells_a_task_that_gets_needs_you_a_delivery_ready_to_land_and_a_board_that_is_done() {
    let mut r = rig();
    r.tick();
    r.board(
        "a",
        vec![
            Task { column: "review".into(), ..labelled("t1", &["ready-to-land"]) },
            Task { comments: vec![comment("c1", "orch", "Which db?", 1)], ..labelled("t2", &["needs-you"]) },
        ],
    );
    r.board("b", vec![Task { column: "done".into(), ..task("u1") }]);
    r.tick();
    r.advance(MAX_WAIT_MS);
    let sent = r.sent();
    assert_eq!(sent.len(), 1);
    let text = &sent[0].1;
    assert!(text.contains("3 things wait for the person"));
    assert!(text.contains("Workspace \"Shop\": \"Task t2\" [t2] needs the person: orch: Which db?"));
    assert!(text.contains("Workspace \"Shop\": the delivery of \"Task t1\" [t1] is ready to land"));
    assert!(text.contains("Workspace \"Blog\": every task on the board is done (1)"));
    // Once: the same labels read again are no news.
    r.tick();
    r.advance(MAX_WAIT_MS);
    assert_eq!(r.sent().len(), 1);
}

#[test]
fn says_nothing_of_what_the_boards_already_held_nor_an_empty_board() {
    let mut r = rig();
    r.board("a", vec![labelled("t1", &["needs-you"])]);
    r.board("b", vec![]);
    r.tick();
    r.tick();
    r.advance(MAX_WAIT_MS);
    assert!(r.sent().is_empty());
}

#[test]
fn says_nothing_of_the_hq() {
    let mut r = rig();
    r.tick();
    r.board("hq", vec![Task { column: "done".into(), ..labelled("h1", &["needs-you", "ready-to-land"]) }]);
    r.activity("hw", AgentActivity::Waiting);
    r.tick();
    r.advance(WAITING_MS);
    r.tick();
    r.advance(MAX_WAIT_MS);
    assert!(r.sent().is_empty());
}

#[test]
fn says_nothing_else_moves_comments_worker_starts() {
    let mut r = rig();
    r.tick();
    r.board(
        "a",
        vec![
            Task { column: "review".into(), assignee: Some("worker-1".into()), ..task("t1") },
            Task { column: "doing".into(), comments: vec![comment("c1", "worker-1", "done", 1)], ..task("t2") },
            task("t3"),
        ],
    );
    r.activity("w1", AgentActivity::Working);
    r.activity("o", AgentActivity::Yours);
    r.tick();
    r.advance(MAX_WAIT_MS);
    assert!(r.sent().is_empty());
    assert!(r.feed.journal().is_empty());
}

#[test]
fn sends_nothing_without_a_manager_or_one_that_does_not_run() {
    let mut none = rig_with(false, false);
    none.tick();
    none.board("a", vec![labelled("t1", &["needs-you"]), task("t2")]);
    none.tick();
    none.advance(MAX_WAIT_MS);
    assert!(none.sent().is_empty());
    assert!(none.feed.journal().is_empty());

    let mut asleep = rig();
    asleep.tick();
    asleep.io.activities.remove("m");
    asleep.board("a", vec![labelled("t1", &["needs-you"]), task("t2")]);
    asleep.tick();
    asleep.advance(MAX_WAIT_MS);
    assert!(asleep.sent().is_empty());
}

#[test]
fn holds_the_batch_while_the_manager_is_in_a_turn() {
    let mut r = rig();
    r.tick();
    r.activity("m", AgentActivity::Working);
    r.board("a", vec![labelled("t1", &["needs-you"]), task("t2")]);
    r.tick();
    r.advance(MAX_WAIT_MS);
    assert!(r.sent().is_empty());
    r.activity("m", AgentActivity::Yours);
    r.advance(HOLD_MS + MIN_GAP_MS);
    assert_eq!(r.sent().len(), 1);
}

#[test]
fn batches_what_comes_together_keeps_a_journal() {
    let mut r = rig();
    r.tick();
    r.board("a", vec![labelled("t1", &["needs-you"]), labelled("t2", &["ready-to-land"])]);
    r.tick();
    r.advance(DEBOUNCE_MS - 1_000);
    r.board("b", vec![Task { column: "done".into(), ..task("u1") }]);
    r.tick();
    r.advance(MAX_WAIT_MS);
    assert_eq!(r.sent().len(), 1);
    let kinds: Vec<String> = r
        .feed
        .journal()
        .iter()
        .map(|e| ManagerNews::Event(e.event.clone()).kind().to_string())
        .collect();
    assert_eq!(kinds, ["taskNeedsYou", "readyToLand", "boardDone"]);
}

#[test]
fn caps_the_lines_of_a_message_and_counts_the_rest() {
    let mut r = rig();
    let many: Vec<Task> = (0..20).map(|i| task(&format!("n{i}"))).collect();
    r.board("a", many.clone());
    r.tick();
    r.board("a", many.iter().map(|t| Task { labels: strings(&["needs-you"]), ..t.clone() }).collect());
    r.tick();
    r.advance(MAX_WAIT_MS);
    let sent = r.sent();
    assert_eq!(sent.len(), 1);
    let lines: Vec<&str> = sent[0].1.split('\n').collect();
    assert_eq!(lines[0], "20 things wait for the person:");
    assert_eq!(lines.len(), 1 + 12 + 1);
    assert!(lines.last().unwrap().contains("…and 8 more"));
}

/* ── manager feed, two sinks ───────────────────────────────────── */

fn needs_you() -> Vec<Task> {
    vec![labelled("t1", &["needs-you"]), task("t2")]
}

fn kinds(events: &[ManagerEvent]) -> Vec<&'static str> {
    events.iter().map(|e| ManagerNews::Event(e.clone()).kind()).collect()
}

#[test]
fn pushes_to_the_phone_with_no_manager_at_all() {
    let mut r = rig_with(false, true);
    r.tick();
    r.board("a", needs_you());
    r.activity("p", AgentActivity::Waiting);
    r.tick();
    r.advance(WAITING_MS);
    r.tick();
    r.advance(MAX_WAIT_MS);
    assert_eq!(kinds(&r.posted()), ["taskNeedsYou", "paneWaiting"]);
    assert!(r.sent().is_empty());
    // What waited is in the journal all the same, for the phone's history.
    assert_eq!(r.feed.journal().len(), 2);
}

#[test]
fn tells_the_manager_without_pushing_when_no_phone_listens() {
    let mut r = rig_with(true, false);
    r.tick();
    r.board("a", needs_you());
    r.tick();
    r.advance(MAX_WAIT_MS);
    assert_eq!(r.sent().len(), 1);
    assert!(r.posted().is_empty());
}

#[test]
fn gives_each_sink_every_event_the_push_at_once() {
    let mut r = rig_with(true, true);
    r.tick();
    r.board("a", needs_you());
    r.board("b", vec![Task { column: "done".into(), ..task("u1") }]);
    r.tick();
    // The push is not held for the manager's batch…
    assert_eq!(kinds(&r.posted()), ["taskNeedsYou", "boardDone"]);
    assert!(r.sent().is_empty());
    r.advance(MAX_WAIT_MS);
    // …nor the manager for the push.
    let sent = r.sent();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].1.contains("2 things wait for the person"));
}

#[test]
fn does_not_hold_the_push_while_the_manager_is_in_a_turn() {
    let mut r = rig_with(true, true);
    r.tick();
    r.activity("m", AgentActivity::Working);
    r.board("a", needs_you());
    r.tick();
    r.advance(MAX_WAIT_MS);
    assert_eq!(r.posted().len(), 1);
    assert!(r.sent().is_empty());
}

#[test]
fn keeps_pushing_when_the_manager_stops_and_telling_it_when_the_phone_does() {
    let mut r = rig_with(true, true);
    r.tick();
    r.io.activities.remove("m");
    r.board("a", needs_you());
    r.tick();
    r.advance(MAX_WAIT_MS);
    assert_eq!(r.posted().len(), 1);
    assert!(r.sent().is_empty());

    r.activity("m", AgentActivity::Idle);
    r.push_on(false);
    r.board("a", vec![labelled("t1", &["needs-you"]), labelled("t2", &["ready-to-land"])]);
    r.tick();
    r.advance(MAX_WAIT_MS);
    assert_eq!(r.posted().len(), 1);
    let sent = r.sent();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].1.contains("ready to land"));
}

#[test]
fn a_push_that_throws_leaves_the_manager_its_event() {
    let mut r = rig_with(true, true);
    r.tick();
    r.push.lock().unwrap().broken = true;
    r.board("a", needs_you());
    r.tick();
    r.advance(MAX_WAIT_MS);
    assert_eq!(r.sent().len(), 1);
}

#[test]
fn watches_nothing_without_a_manager_or_a_subscription() {
    let mut r = rig_with(false, false);
    r.tick();
    r.board("a", needs_you());
    r.activity("p", AgentActivity::Waiting);
    r.tick();
    let t = r.t;
    r.feed.board_changed(&r.io, t);
    r.advance(WAITING_MS + MAX_WAIT_MS);
    r.tick();
    assert_eq!(r.io.reads.get(), 0);
    assert!(r.sent().is_empty());
    assert!(r.posted().is_empty());
    assert!(r.feed.journal().is_empty());
}

#[test]
fn starts_from_a_fresh_baseline_when_a_phone_subscribes() {
    let mut r = rig_with(false, false);
    r.board("a", needs_you());
    r.tick();
    r.push_on(true);
    // What already waited when the phone subscribed is not news.
    r.tick();
    r.tick();
    assert!(r.posted().is_empty());
    r.board("a", vec![labelled("t1", &["needs-you"]), labelled("t2", &["needs-you"])]);
    r.tick();
    let posted = r.posted();
    assert_eq!(posted.len(), 1);
    assert!(matches!(
        &posted[0],
        ManagerEvent::TaskNeedsYou { task_id, workspace, .. } if task_id == "t2" && workspace == "Shop"
    ));
}

#[test]
fn pushes_nothing_of_the_hq() {
    let mut r = rig_with(false, true);
    r.tick();
    r.board("hq", vec![Task { column: "done".into(), ..labelled("h1", &["needs-you", "ready-to-land"]) }]);
    r.activity("hw", AgentActivity::Waiting);
    r.tick();
    r.advance(WAITING_MS);
    r.tick();
    assert!(r.posted().is_empty());
}

/* ── settleManager ─────────────────────────────────────────────── */

fn waiting(pane: &str) -> ManagerNewsEntry {
    ManagerNewsEntry {
        at: 1,
        event: ManagerNews::Event(ManagerEvent::PaneWaiting {
            workspace_id: "a".into(),
            workspace: "Shop".into(),
            pane_id: pane.into(),
            pane: pane.into(),
            for_ms: 120_000,
        }),
    }
}

#[test]
fn folds_repeats_and_ranks_the_waits_first() {
    let done = ManagerNewsEntry {
        at: 1,
        event: ManagerNews::Event(ManagerEvent::BoardDone { workspace_id: "a".into(), workspace: "Shop".into(), tasks: 2 }),
    };
    let out = settle_manager(&[done, waiting("w1"), waiting("w1")], &|_| true);
    let kinds: Vec<&str> = out.iter().map(ManagerNews::kind).collect();
    assert_eq!(kinds, ["paneWaiting", "boardDone"]);
}

#[test]
fn drops_a_wait_that_ended() {
    assert!(settle_manager(&[waiting("w1")], &|_| false).is_empty());
}

/* ── manager feed, blocked tasks ───────────────────────────────── */

#[test]
fn tells_a_block_only_the_person_can_lift_and_its_unblock_but_not_the_phone() {
    let mut r = rig_with(true, true);
    r.tick();
    r.board(
        "a",
        vec![
            Task { column: "doing".into(), ..task("t1") },
            Task { blocked: Some(blocked("pick a license", "Eli", true)), ..task("t2") },
        ],
    );
    r.tick();
    r.board(
        "a",
        vec![
            Task { column: "doing".into(), ..task("t1") },
            Task { comments: vec![comment("c", "hangar", "Unblocked by user", 2)], ..task("t2") },
        ],
    );
    r.tick();
    let news: Vec<&str> = r.feed.news().iter().map(|e| e.event.kind()).collect();
    assert_eq!(news, ["taskBlocked", "taskUnblocked"]);
    assert!(r.feed.journal().is_empty());
    assert!(r.posted().is_empty());
    r.advance(MAX_WAIT_MS);
    assert!(r.sent()[0].1.contains("Eli blocked \"Task t2\" [t2] until a person lifts it: pick a license"));
}

#[test]
fn leaves_a_block_an_agent_can_lift_to_the_orchestrator() {
    let mut r = rig();
    r.tick();
    r.board(
        "a",
        vec![
            Task { column: "doing".into(), ..task("t1") },
            Task { blocked: Some(blocked("pick a license", "Eli", false)), ..task("t2") },
        ],
    );
    r.tick();
    assert!(r.feed.news().is_empty());
}

#[test]
fn news_reads_as_the_ts_wrote_it() {
    let json = serde_json::to_value(&waiting("w1").event).unwrap();
    assert_eq!(json["kind"], "paneWaiting");
    assert_eq!(json["paneId"], "w1");
    assert_eq!(json["forMs"], 120_000);
}
