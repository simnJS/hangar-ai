//! orchestratorEvents.test.ts, case for case.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use hangar_core::board::Task;

use super::*;
use crate::orchestration::testkit::{
    START, Timed, advance, blocked, comment, pane, strings, task, worker, workspace,
};
use crate::state::types::{AppState, Pane, PaneRole};
use crate::state::{AgentActivity, PaneActivity};

#[derive(Debug, Clone, PartialEq)]
pub struct Sent {
    pub pane: String,
    pub text: String,
    pub from: String,
}

/// The engine's world: the state, the activities, one board for every
/// folder, and what was queued.
pub struct World {
    pub state: AppState,
    pub activities: HashMap<String, AgentActivity>,
    pub tasks: Vec<Task>,
    pub sent: RefCell<Vec<Sent>>,
}

impl EngineIo for World {
    fn state(&self) -> Arc<AppState> {
        Arc::new(self.state.clone())
    }

    fn activity(&self, pane: &str) -> Option<PaneActivity> {
        self.activities.get(pane).map(|activity| PaneActivity {
            workspace_id: "ws".into(),
            activity: *activity,
            bridged: true,
        })
    }

    fn load_board(&self, _cwd: &str) -> Result<Vec<Task>, String> {
        Ok(self.tasks.clone())
    }

    fn enqueue(&self, pane: &str, text: &str, from: &str) -> Result<(), String> {
        self.sent.borrow_mut().push(Sent {
            pane: pane.into(),
            text: text.into(),
            from: from.into(),
        });
        Ok(())
    }
}

struct Rig {
    engine: OrchestratorEngine,
    io: World,
    t: u64,
}

impl Timed for Rig {
    fn next_wake(&self) -> Option<u64> {
        self.engine.next_wake()
    }

    fn fire_due(&mut self, now: u64) {
        self.engine.fire_due(&self.io, now);
    }
}

impl Rig {
    fn post(&mut self, ws: &str, event: OrchestratorEvent) {
        self.engine.post(&self.io, ws, event, false, self.t);
    }

    fn advance(&mut self, ms: u64) {
        let mut t = self.t;
        advance(&mut t, ms, self);
        self.t = t;
    }

    fn tick(&mut self) {
        self.engine.tick(&self.io, self.t);
    }

    fn bridge(&mut self, pane: &str, event: BridgeEvent) {
        self.engine.bridge_event(&self.io, pane, &event, self.t);
    }

    fn sent(&self) -> Vec<Sent> {
        self.io.sent.borrow().clone()
    }

    fn set_activity(&mut self, pane: &str, activity: AgentActivity) {
        self.io.activities.insert(pane.into(), activity);
    }

    fn shop(&mut self) -> &mut Vec<Pane> {
        &mut self.io.state.workspaces[0].panes
    }
}

fn rig_with(panes: Vec<Pane>) -> Rig {
    let mut shop = workspace("ws", "Shop", panes);
    shop.cwd = "C:\\dev\\ws".into();
    Rig {
        engine: OrchestratorEngine::new(),
        io: World {
            state: AppState {
                workspaces: vec![
                    shop,
                    workspace("plain", "plain", vec![pane("p", "solo", None)]),
                ],
                active_workspace_id: Some("ws".into()),
                ..AppState::default()
            },
            activities: HashMap::new(),
            tasks: vec![
                Task {
                    column: "doing".into(),
                    assignee: Some("worker-1".into()),
                    ..task("t1")
                },
                task("t2"),
            ],
            sent: RefCell::new(Vec::new()),
        },
        t: START,
    }
}

fn rig() -> Rig {
    rig_with(vec![
        pane("o", "orch", Some(PaneRole::Orchestrator)),
        worker("w1", "worker-1", "t1"),
    ])
}

fn delivery(outcome: DeliveryOutcome, task_id: &str) -> OrchestratorEvent {
    OrchestratorEvent::Delivery {
        task_id: task_id.into(),
        title: format!("Task {task_id}"),
        worker: "worker-1".into(),
        outcome,
        detail: if outcome == DeliveryOutcome::Landed {
            "check passed"
        } else {
            "pnpm test exited 1"
        }
        .into(),
    }
}

fn commented(task_id: &str, title: &str, by: &str, text: &str) -> OrchestratorEvent {
    OrchestratorEvent::TaskCommented {
        task_id: task_id.into(),
        title: title.into(),
        by: by.into(),
        text: text.into(),
    }
}

fn moved(task_id: &str, title: &str, from: &str, to: &str) -> OrchestratorEvent {
    OrchestratorEvent::TaskMoved {
        task_id: task_id.into(),
        title: title.into(),
        from: from.into(),
        to: to.into(),
        by: None,
    }
}

fn started(worker: &str, task_id: Option<&str>) -> OrchestratorEvent {
    OrchestratorEvent::WorkerStarted {
        worker: worker.into(),
        task_id: task_id.map(str::to_string),
    }
}

fn exited(worker: &str, task_id: Option<&str>) -> OrchestratorEvent {
    OrchestratorEvent::WorkerExited {
        worker: worker.into(),
        task_id: task_id.map(str::to_string),
    }
}

fn stopped(worker: &str, task_id: Option<&str>) -> OrchestratorEvent {
    OrchestratorEvent::WorkerStopped {
        worker: worker.into(),
        task_id: task_id.map(str::to_string),
    }
}

/* ── batching ──────────────────────────────────────────────────── */

#[test]
fn sends_the_events_of_a_few_seconds_as_one_message_from_hangar() {
    let mut r = rig();
    r.post("ws", delivery(DeliveryOutcome::CheckFailed, "t1"));
    r.advance(2_000);
    r.post(
        "ws",
        commented("t1", "Task t1", "worker-1", "Fixing the test"),
    );
    r.advance(2_000);
    assert!(r.sent().is_empty());
    r.advance(timing::DEBOUNCE_MS);
    let sent = r.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].pane, "o");
    assert_eq!(sent[0].from, "hangar");
    let lines: Vec<&str> = sent[0].text.split('\n').collect();
    assert_eq!(lines[0], "2 updates in workspace \"Shop\":");
    assert!(lines[1].contains("Delivery of \"Task t1\" [t1] by worker-1 failed the check"));
    assert!(lines[1].contains("pnpm test exited 1"));
    assert_eq!(
        lines[2],
        "- worker-1 commented on \"Task t1\" [t1]: \"Fixing the test\""
    );
}

#[test]
fn does_not_let_a_steady_stream_postpone_the_first_event_for_good() {
    let mut r = rig();
    for i in 0..12 {
        r.post(
            "ws",
            commented("t1", "Task t1", &format!("w{i}"), &format!("note {i}")),
        );
        r.advance(3_000);
    }
    let sent = r.sent();
    assert!(!sent.is_empty());
    assert!(sent[0].text.contains("note 0"));
}

#[test]
fn holds_the_batch_while_the_orchestrator_is_in_a_turn_and_folds_what_comes_meanwhile_into_it() {
    let mut r = rig();
    r.set_activity("o", AgentActivity::Working);
    r.post("ws", delivery(DeliveryOutcome::CheckFailed, "t1"));
    r.advance(timing::DEBOUNCE_MS + 1);
    r.post("ws", exited("worker-1", Some("t1")));
    r.advance(60_000);
    assert!(r.sent().is_empty());
    r.set_activity("o", AgentActivity::Yours);
    r.advance(timing::HOLD_MS + timing::DEBOUNCE_MS);
    let sent = r.sent();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].text.starts_with("2 updates"));
}

#[test]
fn believes_the_mod_over_a_pane_still_showing_the_working_of_its_startup() {
    let mut r = rig();
    r.set_activity("o", AgentActivity::Working);
    r.bridge(
        "o",
        BridgeEvent::Hello {
            session_id: "s".into(),
            model: "sonnet".into(),
            version: "2".into(),
        },
    );
    r.post("ws", delivery(DeliveryOutcome::CheckFailed, "t1"));
    r.advance(timing::DEBOUNCE_MS);
    assert_eq!(r.sent().len(), 1);

    r.bridge("o", BridgeEvent::Turn(TurnPhase::Start));
    r.post("ws", delivery(DeliveryOutcome::Landed, "t1"));
    r.advance(timing::MIN_GAP_MS + timing::DEBOUNCE_MS);
    assert_eq!(r.sent().len(), 1);
    r.bridge(
        "o",
        BridgeEvent::Permission {
            waiting: true,
            tool: None,
            input: None,
        },
    );
    r.bridge(
        "o",
        BridgeEvent::Turn(TurnPhase::End {
            reason: "answer".into(),
            duration_ms: 1,
            answer: String::new(),
        }),
    );
    r.advance(timing::HOLD_MS);
    assert_eq!(r.sent().len(), 2);
}

#[test]
fn keeps_two_messages_apart() {
    let mut r = rig();
    r.post("ws", delivery(DeliveryOutcome::CheckFailed, "t1"));
    r.advance(timing::DEBOUNCE_MS);
    assert_eq!(r.sent().len(), 1);
    r.post("ws", delivery(DeliveryOutcome::Landed, "t1"));
    r.advance(timing::DEBOUNCE_MS);
    assert_eq!(r.sent().len(), 1);
    r.advance(timing::MIN_GAP_MS);
    assert_eq!(r.sent().len(), 2);
}

#[test]
fn says_nothing_and_keeps_nothing_for_a_workspace_without_an_orchestrator() {
    let mut r = rig_with(vec![worker("w1", "worker-1", "t1")]);
    r.post("ws", delivery(DeliveryOutcome::Landed, "t1"));
    r.post("plain", delivery(DeliveryOutcome::Landed, "t1"));
    r.post("nowhere", delivery(DeliveryOutcome::Landed, "t1"));
    r.advance(60_000);
    r.tick();
    assert!(r.sent().is_empty());
    assert!(r.engine.journal("ws").is_empty());
    assert!(r.engine.journal("plain").is_empty());
}

#[test]
fn drops_what_is_pending_when_the_orchestrator_is_unpinned_before_it_goes() {
    let mut r = rig();
    r.post("ws", delivery(DeliveryOutcome::Landed, "t1"));
    r.shop()[0].role = None;
    r.advance(60_000);
    assert!(r.sent().is_empty());
}

#[test]
fn keeps_what_the_orchestrator_did_itself_out_of_its_messages_but_in_its_journal() {
    let mut r = rig();
    r.set_activity("o", AgentActivity::Working);
    r.post("ws", started("worker-1", Some("t1")));
    r.set_activity("o", AgentActivity::Yours);
    r.advance(60_000);
    assert!(r.sent().is_empty());
    let journal = r.engine.journal("ws");
    assert_eq!(journal.len(), 1);
    assert!(journal[0].quiet);
    assert_eq!(journal[0].event.kind(), EventKind::WorkerStarted);
}

#[test]
fn keeps_the_last_fifty_events_of_a_workspace() {
    let mut r = rig();
    for i in 0..70 {
        r.post(
            "ws",
            commented("t1", "Task t1", "worker-1", &format!("n{i}")),
        );
    }
    let journal = r.engine.journal("ws");
    assert_eq!(journal.len(), 50);
    assert!(
        matches!(&journal[0].event, OrchestratorEvent::TaskCommented { text, .. } if text == "n20")
    );
}

/* ── the board ─────────────────────────────────────────────────── */

#[test]
fn takes_the_first_read_as_it_is_then_tells_moves_comments_and_needs_you() {
    let mut r = rig();
    r.tick();
    r.advance(60_000);
    assert!(r.sent().is_empty());

    r.io.tasks = vec![
        Task {
            column: "review".into(),
            assignee: Some("worker-1".into()),
            comments: vec![
                comment("c1", "worker-1", "Done.\nRan pnpm test: 12 passed", 2),
                comment("c2", "orch", "Thanks", 3),
                comment("c3", "hangar", "Landed", 4),
            ],
            ..task("t1")
        },
        Task {
            labels: strings(&["needs-you"]),
            comments: vec![comment("c4", "user", "Which API?", 5)],
            ..task("t2")
        },
        task("t3"),
    ];
    r.engine.board_changed(r.t);
    r.advance(300);
    r.advance(timing::DEBOUNCE_MS);
    let sent = r.sent();
    assert_eq!(sent.len(), 1);
    let text = &sent[0].text;
    // The person's question is told once, as what the task needs.
    assert!(text.starts_with("3 updates"), "{text}");
    assert!(text.contains("\"Task t2\" [t2] needs the person: user: Which API?"));
    assert!(
        text.contains(
            "worker-1 commented on \"Task t1\" [t1]: \"Done. / Ran pnpm test: 12 passed\""
        )
    );
    assert!(!text.contains("The person commented"));
    assert!(text.contains("\"Task t1\" [t1] moved doing → review"));
    assert!(!text.contains("Thanks"));
    assert!(!text.contains("Landed"));
    assert!(!text.contains("t3"));
}

#[test]
fn takes_a_move_made_during_the_orchestrators_turn_for_its_own() {
    let before = snap_board(&[
        task("a"),
        Task {
            column: "doing".into(),
            ..task("b")
        },
    ]);
    let after = [
        Task {
            column: "doing".into(),
            ..task("a")
        },
        Task {
            column: "doing".into(),
            labels: strings(&["needs-you"]),
            ..task("b")
        },
    ];
    let own = diff_board(
        &before,
        &after,
        Reader {
            name: "orch",
            busy: true,
        },
    );
    assert_eq!(own.len(), 2);
    assert!(
        matches!(&own[0], (OrchestratorEvent::TaskMoved { by: Some(by), .. }, true) if by == "orch")
    );
    assert!(
        matches!(&own[1], (OrchestratorEvent::TaskNeedsYou { reason, .. }, true) if reason == "labelled needs-you")
    );
    let theirs = diff_board(
        &before,
        &after,
        Reader {
            name: "orch",
            busy: false,
        },
    );
    assert_eq!(
        theirs.iter().map(|(_, quiet)| *quiet).collect::<Vec<_>>(),
        [false, false]
    );
    assert!(matches!(
        &theirs[0].0,
        OrchestratorEvent::TaskMoved { by: None, .. }
    ));
}

/* ── settling a batch ──────────────────────────────────────────── */

fn entry(event: OrchestratorEvent, at: u64, quiet: bool) -> JournalEntry {
    JournalEntry { at, event, quiet }
}

/// `at` around a base far from zero, so the TS's negative offsets fit.
const BASE: u64 = 10_000_000;

struct FakeLive {
    activity: HashMap<String, AgentActivity>,
    columns: HashMap<String, String>,
}

impl LiveView for FakeLive {
    fn activity(&self, worker: &str) -> Option<AgentActivity> {
        self.activity.get(worker).copied()
    }

    fn column(&self, task_id: &str) -> Option<String> {
        self.columns.get(task_id).cloned()
    }
}

fn live(activity: &[(&str, AgentActivity)], columns: &[(&str, &str)]) -> FakeLive {
    FakeLive {
        activity: activity.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        columns: columns
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
    }
}

#[test]
fn folds_the_moves_of_one_task_into_one_and_drops_a_round_trip() {
    let moves = [
        entry(moved("a", "A", "todo", "doing"), BASE, false),
        entry(moved("a", "A", "doing", "review"), BASE, false),
        entry(moved("b", "B", "todo", "doing"), BASE, false),
        entry(moved("b", "B", "doing", "todo"), BASE, false),
    ];
    assert_eq!(
        settle(&moves, &moves, &live(&[], &[])),
        [moved("a", "A", "todo", "review")]
    );
}

#[test]
fn drops_what_a_more_telling_event_already_says() {
    let started = entry(started("w", Some("a")), BASE, true);
    let landed = entry(delivery(DeliveryOutcome::Landed, "b"), BASE, false);
    let stopped = entry(stopped("x", Some("c")), BASE, false);
    let batch = vec![
        entry(moved("a", "A", "todo", "doing"), BASE + 2_000, false),
        entry(moved("b", "B", "doing", "done"), BASE + 3_000, false),
        entry(exited("x", Some("c")), BASE + 1_000, false),
    ];
    let mut journal = vec![started.clone(), landed, stopped];
    journal.extend(batch.clone());
    assert!(settle(&batch, &journal, &live(&[], &[])).is_empty());
    // …unless it is about something else, or long ago.
    let old = JournalEntry {
        at: BASE - 10 * timing::NEAR_MS,
        ..started
    };
    let mut journal = vec![old];
    journal.extend(batch.clone());
    assert_eq!(settle(&batch[..1], &journal, &live(&[], &[])).len(), 1);
}

#[test]
fn lets_a_delivery_say_why_its_task_went_back_to_doing() {
    let back = entry(moved("b", "B", "review", "doing"), BASE + 1_000, false);
    let stale = entry(delivery(DeliveryOutcome::Stale, "b"), BASE + 1_200, false);
    let both = [back, stale.clone()];
    assert_eq!(settle(&both, &both, &live(&[], &[])), [stale.event]);
}

#[test]
fn drops_a_wait_or_an_idle_the_moment_has_overtaken() {
    let waiting = |w: &str, t: &str| OrchestratorEvent::WorkerWaiting {
        worker: w.into(),
        task_id: Some(t.into()),
        for_ms: 150_000,
    };
    let idle = |w: &str, t: &str| OrchestratorEvent::WorkerIdle {
        worker: w.into(),
        task_id: Some(t.into()),
        for_ms: 700_000,
    };
    let batch = [
        entry(waiting("w1", "a"), BASE, false),
        entry(waiting("w2", "b"), BASE, false),
        entry(idle("w3", "c"), BASE, false),
        entry(idle("w4", "d"), BASE, false),
    ];
    let now = live(
        &[
            ("w1", AgentActivity::Waiting),
            ("w2", AgentActivity::Working),
            ("w3", AgentActivity::Yours),
            ("w4", AgentActivity::Yours),
        ],
        &[("c", "doing"), ("d", "done")],
    );
    let workers: Vec<_> = settle(&batch, &batch, &now)
        .iter()
        .map(|e| e.worker().unwrap().to_string())
        .collect();
    assert_eq!(workers, ["w1", "w3"]);
}

#[test]
fn drops_the_exit_of_a_worker_whose_task_left_doing_as_a_landing_closes_its_pane() {
    let batch = [
        entry(exited("w1", Some("a")), BASE, false),
        entry(exited("w2", Some("b")), BASE, false),
        entry(exited("w3", None), BASE, false),
    ];
    let now = live(&[], &[("a", "done"), ("b", "doing")]);
    let workers: Vec<_> = settle(&batch, &batch, &now)
        .iter()
        .map(|e| e.worker().unwrap().to_string())
        .collect();
    assert_eq!(workers, ["w2", "w3"]);
}

#[test]
fn removes_repeats_keeps_a_workers_last_two_comments_and_puts_deliveries_first() {
    let note = |text: &str| commented("a", "A", "w1", text);
    let batch = [
        entry(started("w2", Some("b")), BASE, false),
        entry(note("one"), BASE, false),
        entry(note("two"), BASE, false),
        entry(note("two"), BASE, false),
        entry(note("three"), BASE, false),
        entry(delivery(DeliveryOutcome::CheckFailed, "c"), BASE, false),
    ];
    assert_eq!(
        settle(&batch, &batch, &live(&[], &[])),
        [
            delivery(DeliveryOutcome::CheckFailed, "c"),
            note("two"),
            note("three"),
            started("w2", Some("b"))
        ]
    );
}

/* ── the message ───────────────────────────────────────────────── */

#[test]
fn lists_a_dozen_lines_at_most_and_counts_the_rest() {
    let events: Vec<_> = (0..15).map(|i| started(&format!("w{i}"), None)).collect();
    let text = render_delta(&events, &[], "Shop");
    let lines: Vec<&str> = text.split('\n').collect();
    assert_eq!(lines.len(), 14);
    assert_eq!(
        lines[13],
        "- …and 3 more: board_list_tasks and worker_list have the rest."
    );
}

#[test]
fn names_the_tasks_a_landing_made_ready_or_says_the_board_is_done() {
    let tasks = vec![
        Task {
            column: "done".into(),
            ..task("a")
        },
        Task {
            depends_on: strings(&["a"]),
            priority: 2,
            ..task("b")
        },
        Task {
            depends_on: strings(&["a", "z"]),
            ..task("c")
        },
        Task {
            labels: strings(&["needs-you"]),
            ..task("d")
        },
        Task {
            assignee: Some("w1".into()),
            ..task("e")
        },
    ];
    let ready: Vec<&str> = ready_tasks(&tasks).iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ready, ["b"]);
    assert!(
        render_delta(&[delivery(DeliveryOutcome::Landed, "a")], &tasks, "Shop")
            .contains("Ready to start: \"Task b\" [b].")
    );
    assert!(
        !render_delta(
            &[delivery(DeliveryOutcome::CheckFailed, "a")],
            &tasks,
            "Shop"
        )
        .contains("Ready to start")
    );
    assert!(
        render_delta(
            &[delivery(DeliveryOutcome::Landed, "a")],
            &[Task {
                column: "done".into(),
                ..task("a")
            }],
            "Shop"
        )
        .contains("Every task on the board is done")
    );
}

#[test]
fn names_a_workers_task_by_its_title() {
    let text = render_delta(
        &[OrchestratorEvent::WorkerWaiting {
            worker: "w1".into(),
            task_id: Some("a".into()),
            for_ms: 185_000,
        }],
        &[Task {
            title: "Login form".into(),
            ..task("a")
        }],
        "Shop",
    );
    assert!(text.contains("w1 has been waiting for the person for 3 min on \"Login form\" [a]"));
}

/* ── the workers ───────────────────────────────────────────────── */

#[test]
fn tells_a_wait_for_the_person_once_it_passes_two_minutes_once() {
    let mut r = rig();
    r.set_activity("w1", AgentActivity::Working);
    r.tick();
    r.set_activity("w1", AgentActivity::Waiting);
    r.tick();
    r.advance(timing::WAITING_MS - 1_000);
    r.tick();
    r.advance(timing::DEBOUNCE_MS);
    assert!(r.sent().is_empty());
    r.advance(2_000);
    r.tick();
    r.tick();
    r.advance(timing::DEBOUNCE_MS);
    let sent = r.sent();
    assert_eq!(sent.len(), 1);
    assert!(
        sent[0]
            .text
            .contains("worker-1 has been waiting for the person for 2 min")
    );
    r.advance(5 * 60_000);
    r.tick();
    r.advance(timing::DEBOUNCE_MS);
    assert_eq!(r.sent().len(), 1);
}

#[test]
fn tells_a_turn_ended_ten_minutes_ago_without_a_delivery() {
    let mut r = rig();
    r.set_activity("w1", AgentActivity::Working);
    r.tick();
    r.set_activity("w1", AgentActivity::Yours);
    r.tick();
    r.advance(timing::IDLE_MS + 1_000);
    r.tick();
    r.advance(timing::DEBOUNCE_MS);
    let sent = r.sent();
    assert_eq!(sent.len(), 1);
    assert!(
        sent[0]
            .text
            .contains("worker-1 ended its turn 10 min ago without delivering on \"Task t1\" [t1]")
    );
}

#[test]
fn does_not_call_a_worker_idle_that_delivered_or_that_never_had_a_turn() {
    let mut delivered = rig();
    delivered.set_activity("w1", AgentActivity::Working);
    delivered.tick();
    delivered.set_activity("w1", AgentActivity::Yours);
    delivered.tick();
    delivered.post("ws", delivery(DeliveryOutcome::CheckFailed, "t1"));
    delivered.advance(timing::IDLE_MS + 20_000);
    delivered.tick();
    delivered.advance(timing::DEBOUNCE_MS);
    let all: Vec<String> = delivered.sent().into_iter().map(|m| m.text).collect();
    assert!(!all.join("\n").contains("without delivering"));

    let mut fresh = rig();
    fresh.set_activity("w1", AgentActivity::Idle);
    fresh.tick();
    fresh.advance(timing::IDLE_MS + 20_000);
    fresh.tick();
    fresh.advance(timing::DEBOUNCE_MS);
    assert!(fresh.sent().is_empty());
}

#[test]
fn tells_an_exit_once_whether_claude_code_or_its_shell_said_it() {
    let mut r = rig();
    r.tick();
    r.bridge(
        "w1",
        BridgeEvent::End {
            reason: "exit".into(),
        },
    );
    let t = r.t;
    r.engine.pty_exited(&r.io, "w1", t);
    r.bridge(
        "o",
        BridgeEvent::End {
            reason: "exit".into(),
        },
    );
    r.advance(timing::DEBOUNCE_MS);
    let sent = r.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].text,
        "1 update in workspace \"Shop\":\n- worker-1's session ended on \"Task t1\" [t1]; the task is still assigned to it"
    );
}

#[test]
fn tells_a_worker_pane_closed_by_hand_not_one_stopped_or_restarted() {
    let mut closed = rig();
    closed.tick();
    closed.shop().remove(1);
    closed.tick();
    closed.advance(timing::DEBOUNCE_MS);
    assert!(closed.sent()[0].text.contains("worker-1's session ended"));

    let mut r = rig();
    r.tick();
    r.post("ws", stopped("worker-1", Some("t1")));
    r.shop().remove(1);
    r.tick();
    r.advance(timing::DEBOUNCE_MS);
    let sent = r.sent();
    assert_eq!(sent.len(), 1);
    assert!(!sent[0].text.contains("session ended"));

    let mut restarted = rig();
    restarted.tick();
    restarted.shop()[1].id = "w1b".into();
    restarted.tick();
    restarted.advance(timing::DEBOUNCE_MS);
    assert!(restarted.sent().is_empty());
}

/* ── blocked tasks ─────────────────────────────────────────────── */

const ORCH: Reader<'static> = Reader {
    name: "orch",
    busy: false,
};

#[test]
fn tells_a_block_with_its_author_and_reason_and_an_unblock_with_how() {
    let before = snap_board(&[task("a")]);
    let on = [Task {
        blocked: Some(blocked("npm release", "Eli", false)),
        ..task("a")
    }];
    let told = diff_board(&before, &on, ORCH);
    assert!(matches!(
        &told[0].0,
        OrchestratorEvent::TaskBlocked { by, reason, .. } if by == "Eli" && reason == "npm release"
    ));
    assert_eq!(
        describe_event(&told[0].0, &|_| Some("Task a".into())),
        "Eli blocked \"Task a\" [a]: npm release"
    );

    let comments = vec![comment("c", "hangar", "recheck passed (2026-11-05)", 2)];
    let lifted = diff_board(
        &snap_board(&on),
        &[Task {
            comments,
            ..task("a")
        }],
        ORCH,
    );
    assert!(matches!(
        &lifted[0].0,
        OrchestratorEvent::TaskUnblocked { how, .. } if how == "recheck passed (2026-11-05)"
    ));
    assert!(diff_board(&snap_board(&on), &on, ORCH).is_empty());
}

#[test]
fn keeps_the_orchestrators_own_block_quiet_and_says_when_only_a_person_can_lift_one() {
    let own = diff_board(
        &snap_board(&[task("a")]),
        &[Task {
            blocked: Some(blocked("npm release", "orch", false)),
            ..task("a")
        }],
        Reader {
            name: "orch",
            busy: true,
        },
    );
    assert!(own[0].1);
    let human = diff_board(
        &snap_board(&[task("a")]),
        &[Task {
            blocked: Some(blocked("npm release", "Eli", true)),
            ..task("a")
        }],
        ORCH,
    );
    assert!(describe_event(&human[0].0, &|_| None).contains("only the person can unblock it"));
}

#[test]
fn reminds_once_when_a_human_only_recheck_passes() {
    let human = blocked("npm release", "Eli", true);
    let reminded = hangar_core::board::Blocked {
        reminded: Some(5),
        ..human.clone()
    };
    let due = diff_board(
        &snap_board(&[Task {
            blocked: Some(human),
            ..task("a")
        }]),
        &[Task {
            blocked: Some(reminded),
            ..task("a")
        }],
        ORCH,
    );
    assert_eq!(due[0].0.kind(), EventKind::TaskRecheckDue);
}

#[test]
fn does_not_offer_a_blocked_task_as_ready() {
    let tasks = [
        Task {
            blocked: Some(blocked("npm release", "Eli", false)),
            ..task("a")
        },
        task("b"),
    ];
    let ready: Vec<&str> = ready_tasks(&tasks).iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ready, ["b"]);
}

#[test]
fn an_event_reads_as_the_ts_wrote_it() {
    let json = serde_json::to_value(OrchestratorEvent::WorkerWaiting {
        worker: "Mia".into(),
        task_id: Some("t1".into()),
        for_ms: 3,
    })
    .unwrap();
    assert_eq!(
        json,
        serde_json::json!({"kind": "workerWaiting", "worker": "Mia", "taskId": "t1", "forMs": 3})
    );
    let json = serde_json::to_value(delivery(DeliveryOutcome::CheckFailed, "t1")).unwrap();
    assert_eq!(json["outcome"], "checkFailed");
}
