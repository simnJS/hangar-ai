//! activityLog.test.ts, case for case.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use hangar_core::remote::{ActivityEntry, Share, ShareKind};
use hangar_i18n::Locale;

use crate::orchestration::events::{DeliveryOutcome, JournalEntry, OrchestratorEvent};
use crate::orchestration::manager_feed::{BlockEvent, ManagerEvent, ManagerNews, ManagerNewsEntry};
use crate::state::{AppState, Workspace};

use super::super::activity_log::{ActivityIo, ActivityLog, lasted};
use super::super::engine::locked;
use super::super::ops::text_of;
use super::fake::claude;

fn workspace(id: &str, name: &str) -> Workspace {
    Workspace {
        id: id.into(),
        name: name.into(),
        cwd: format!("C:\\dev\\{id}"),
        panes: vec![claude(&format!("{id}-p"), "Ava", None)],
        ..Workspace::default()
    }
}

struct Io {
    state: Arc<AppState>,
    manager: Vec<ManagerNewsEntry>,
    orchestrators: HashMap<String, Vec<JournalEntry>>,
    asleep: HashSet<String>,
    waiting: HashSet<String>,
}

impl ActivityIo for Io {
    fn state(&self) -> Arc<AppState> {
        self.state.clone()
    }

    fn manager_news(&self) -> Vec<ManagerNewsEntry> {
        self.manager.clone()
    }

    fn orchestrator_journal(&self, workspace_id: &str) -> Vec<JournalEntry> {
        self.orchestrators
            .get(workspace_id)
            .cloned()
            .unwrap_or_default()
    }

    fn is_asleep(&self, workspace_id: &str) -> bool {
        self.asleep.contains(workspace_id)
    }

    fn is_waiting(&self, pane: &str) -> bool {
        self.waiting.contains(pane)
    }
}

struct Rig {
    io: Io,
    log: ActivityLog,
    clock: u64,
    written: Mutex<Vec<Vec<ActivityEntry>>>,
    failing: bool,
}

fn rig(workspaces: Vec<Workspace>, locale: Locale) -> Rig {
    let mut hq = workspace("hq", "HQ");
    hq.hq = Some(true);
    let mut all = vec![hq];
    all.extend(workspaces);
    let mut state = AppState {
        workspaces: all,
        ..AppState::default()
    };
    state.settings.locale = Some(locale);
    Rig {
        io: Io {
            state: Arc::new(state),
            manager: Vec::new(),
            orchestrators: HashMap::new(),
            asleep: HashSet::new(),
            waiting: HashSet::new(),
        },
        log: ActivityLog::default(),
        clock: 10_000,
        written: Mutex::new(Vec::new()),
        failing: false,
    }
}

impl Rig {
    /// Reads the journals once and writes what is new (`tick`).
    fn tick(&mut self) {
        self.log.tick(&self.io, self.clock);
        self.flush();
    }

    fn flush(&mut self) {
        let (written, failing) = (&self.written, self.failing);
        self.log.write(&|entries: &[ActivityEntry]| {
            if failing {
                return Err("disk".into());
            }
            locked(written).push(entries.to_vec());
            Ok(())
        });
    }

    fn flat(&self) -> Vec<ActivityEntry> {
        locked(&self.written).concat()
    }

    fn kinds_and_texts(&self) -> Vec<(String, String)> {
        self.flat()
            .into_iter()
            .map(|entry| (entry.kind, entry.text))
            .collect()
    }
}

fn entry(at: u64, workspace: &str, kind: &str, text: &str) -> ActivityEntry {
    ActivityEntry {
        at,
        workspace: workspace.into(),
        kind: kind.into(),
        text: text.into(),
    }
}

fn delivery(outcome: DeliveryOutcome, task: &str, title: &str) -> OrchestratorEvent {
    OrchestratorEvent::Delivery {
        task_id: task.into(),
        title: title.into(),
        worker: "Ben".into(),
        outcome,
        detail: String::new(),
    }
}

fn journal(at: u64, event: OrchestratorEvent) -> JournalEntry {
    JournalEntry {
        at,
        event,
        quiet: false,
    }
}

fn moved(task: &str, title: &str, from: &str, to: &str, by: Option<&str>) -> OrchestratorEvent {
    OrchestratorEvent::TaskMoved {
        task_id: task.into(),
        title: title.into(),
        from: from.into(),
        to: to.into(),
        by: by.map(str::to_string),
    }
}

fn news(at: u64, event: ManagerEvent) -> ManagerNewsEntry {
    ManagerNewsEntry {
        at,
        event: ManagerNews::Event(event),
    }
}

fn needs_you(task: &str, title: &str) -> ManagerEvent {
    ManagerEvent::TaskNeedsYou {
        workspace_id: "shop".into(),
        workspace: "Shop".into(),
        task_id: task.into(),
        title: title.into(),
        reason: "?".into(),
    }
}

/* ── What is written ──────────────────────────────────────────── */

#[test]
fn a_delivery_landed_or_done_and_a_task_moved_to_done_once_not_twice() {
    let mut r = rig(vec![workspace("shop", "Shop")], Locale::En);
    r.io.orchestrators.insert(
        "shop".into(),
        vec![
            journal(100, delivery(DeliveryOutcome::Landed, "t1", "Cart")),
            journal(101, moved("t1", "Cart", "review", "done", None)),
            journal(200, delivery(DeliveryOutcome::Done, "t2", "Search")),
            journal(300, delivery(DeliveryOutcome::Conflict, "t3", "Pay")),
            journal(400, moved("t4", "Docs", "doing", "done", Some("Ava"))),
            journal(500, moved("t5", "Wip", "todo", "doing", None)),
        ],
    );
    r.tick();
    assert_eq!(
        r.flat(),
        [
            entry(100, "Shop", "landed", "Landed: Cart"),
            entry(200, "Shop", "delivered", "Delivered: Search"),
            entry(400, "Shop", "taskDone", "Done: Docs"),
        ]
    );
}

#[test]
fn what_the_project_manager_is_told_a_task_that_needs_the_person_a_delivery_ready_a_board_done() {
    let mut r = rig(vec![workspace("shop", "Shop")], Locale::En);
    r.io.manager = vec![
        news(100, needs_you("t1", "Pay")),
        news(
            200,
            ManagerEvent::ReadyToLand {
                workspace_id: "shop".into(),
                workspace: "Shop".into(),
                task_id: "t2".into(),
                title: "Cart".into(),
            },
        ),
        news(
            300,
            ManagerEvent::BoardDone {
                workspace_id: "shop".into(),
                workspace: "Shop".into(),
                tasks: 7,
            },
        ),
    ];
    r.tick();
    assert_eq!(
        r.kinds_and_texts(),
        [
            ("needsYou".to_string(), "Needs you: Pay".to_string()),
            (
                "readyToLand".to_string(),
                "Ready to merge: Cart".to_string()
            ),
            (
                "boardDone".to_string(),
                "Every task is done (7)".to_string()
            ),
        ]
    );
}

#[test]
fn a_task_that_needs_the_person_is_one_line_whoever_saw_it() {
    let mut r = rig(vec![workspace("shop", "Shop")], Locale::En);
    r.io.manager = vec![news(100, needs_you("t1", "Pay"))];
    r.io.orchestrators.insert(
        "shop".into(),
        vec![journal(
            150,
            OrchestratorEvent::TaskNeedsYou {
                task_id: "t1".into(),
                title: "Pay".into(),
                reason: "?".into(),
            },
        )],
    );
    r.tick();
    assert_eq!(
        r.flat()
            .iter()
            .filter(|entry| entry.kind == "needsYou")
            .count(),
        1
    );
}

#[test]
fn a_pane_that_waited_when_it_starts_and_when_it_ends_with_how_long() {
    let mut r = rig(vec![workspace("shop", "Shop")], Locale::En);
    r.io.waiting.insert("shop-p".into());
    r.io.manager = vec![news(
        10_000,
        ManagerEvent::PaneWaiting {
            workspace_id: "shop".into(),
            workspace: "Shop".into(),
            pane_id: "shop-p".into(),
            pane: "Ava".into(),
            for_ms: 0,
        },
    )];
    r.tick();
    r.clock = 10_000 + 12 * 60_000;
    r.tick();
    let kinds: Vec<String> = r.flat().into_iter().map(|entry| entry.kind).collect();
    assert_eq!(kinds, ["paneWaiting"]);
    r.io.waiting.clear();
    r.tick();
    assert_eq!(
        r.flat().last().cloned(),
        Some(entry(
            10_000 + 12 * 60_000,
            "Shop",
            "paneWaited",
            "Ava waited for you for 12 min"
        ))
    );
    r.tick();
    assert_eq!(r.flat().len(), 2);
}

#[test]
fn a_workspace_falling_asleep_or_waking_what_is_asleep_at_the_start_is_not_news() {
    let mut r = rig(
        vec![workspace("shop", "Shop"), workspace("blog", "Blog")],
        Locale::En,
    );
    r.io.asleep.insert("blog".into());
    r.tick();
    assert!(r.flat().is_empty());
    r.io.asleep.insert("shop".into());
    r.io.asleep.remove("blog");
    r.clock = 20_000;
    r.tick();
    let lines: Vec<(String, String, String)> = r
        .flat()
        .into_iter()
        .map(|e| (e.workspace, e.kind, e.text))
        .collect();
    assert_eq!(
        lines,
        [
            (
                "Shop".to_string(),
                "asleep".to_string(),
                "Shop fell asleep".to_string()
            ),
            (
                "Blog".to_string(),
                "awake".to_string(),
                "Blog woke up".to_string()
            ),
        ]
    );
}

fn share(workspace: &str, pane: &str, name: &str, at: u64) -> Share {
    Share {
        id: "s1".into(),
        at,
        workspace: workspace.into(),
        pane: pane.into(),
        name: name.into(),
        mime: "application/pdf".into(),
        kind: ShareKind::default(),
        size: 10,
        caption: None,
    }
}

#[test]
fn a_file_shared_with_the_phone_from_the_workspace_it_came_from() {
    let mut r = rig(vec![workspace("shop", "Shop")], Locale::En);
    let state = r.io.state.clone();
    r.log.shared(
        &share("C:\\dev\\shop", "Ava", "report.pdf", 777),
        &state,
        r.clock,
    );
    r.flush();
    assert_eq!(
        r.flat(),
        [entry(777, "Shop", "shared", "Ava shared report.pdf")]
    );
}

#[test]
fn nothing_from_the_hq() {
    let mut r = rig(Vec::new(), Locale::En);
    r.io.manager = vec![news(
        1,
        ManagerEvent::BoardDone {
            workspace_id: "hq".into(),
            workspace: "HQ".into(),
            tasks: 1,
        },
    )];
    r.io.orchestrators.insert(
        "hq".into(),
        vec![journal(2, delivery(DeliveryOutcome::Landed, "t1", "Cart"))],
    );
    let state = r.io.state.clone();
    r.log
        .shared(&share("C:\\dev\\hq", "Pm", "x", 3), &state, r.clock);
    r.tick();
    assert!(r.flat().is_empty());
}

#[test]
fn writes_in_the_language_of_the_pc() {
    let mut r = rig(vec![workspace("shop", "Shop")], Locale::Fr);
    r.io.orchestrators.insert(
        "shop".into(),
        vec![journal(
            100,
            delivery(DeliveryOutcome::Landed, "t1", "Cart"),
        )],
    );
    r.tick();
    assert_eq!(r.flat()[0].text, "Intégrée\u{202f}: Cart");
}

/* ── The journals are read again and again ────────────────────── */

#[test]
fn a_line_is_written_once_however_many_times_the_journal_is_read() {
    let mut r = rig(vec![workspace("shop", "Shop")], Locale::En);
    r.io.orchestrators.insert(
        "shop".into(),
        vec![journal(
            100,
            delivery(DeliveryOutcome::Landed, "t1", "Cart"),
        )],
    );
    r.tick();
    r.tick();
    r.tick();
    assert_eq!(r.flat().len(), 1);
    r.io.orchestrators.get_mut("shop").unwrap().push(journal(
        200,
        delivery(DeliveryOutcome::Done, "t2", "Search"),
    ));
    r.tick();
    let ats: Vec<u64> = r.flat().iter().map(|entry| entry.at).collect();
    assert_eq!(ats, [100, 200]);
    // Gone from the journal and back is not new either.
    let kept = r.io.orchestrators["shop"][1].clone();
    r.io.orchestrators.insert("shop".into(), vec![kept]);
    r.tick();
    assert_eq!(r.flat().len(), 2);
}

#[test]
fn a_write_the_disk_refused_is_tried_again_with_the_next_in_order() {
    let mut r = rig(vec![workspace("shop", "Shop")], Locale::En);
    r.failing = true;
    r.io.orchestrators.insert(
        "shop".into(),
        vec![journal(
            100,
            delivery(DeliveryOutcome::Landed, "t1", "Cart"),
        )],
    );
    r.tick();
    assert!(r.flat().is_empty());
    r.failing = false;
    r.io.orchestrators.get_mut("shop").unwrap().push(journal(
        200,
        delivery(DeliveryOutcome::Done, "t2", "Search"),
    ));
    r.tick();
    let ats: Vec<u64> = r.flat().iter().map(|entry| entry.at).collect();
    assert_eq!(ats, [100, 200]);
}

#[test]
fn nothing_is_written_once_it_is_stopped() {
    let mut r = rig(vec![workspace("shop", "Shop")], Locale::En);
    r.io.orchestrators.insert(
        "shop".into(),
        vec![journal(
            100,
            delivery(DeliveryOutcome::Landed, "t1", "Cart"),
        )],
    );
    r.log.stop();
    r.tick();
    assert!(r.flat().is_empty());
}

/* ── Words ────────────────────────────────────────────────────── */

#[test]
fn says_how_long_the_same_in_both_languages() {
    assert_eq!(lasted(0), "0 s");
    assert_eq!(lasted(45_000), "45 s");
    assert_eq!(lasted(12 * 60_000), "12 min");
    assert_eq!(lasted(3 * 3_600_000 + 5 * 60_000), "3 h 05");
}

#[test]
fn words_a_line_the_pc_wrote_without_text() {
    assert_eq!(
        text_of(&entry(1, "", "appStopped", ""), Locale::En),
        "Hangar stopped"
    );
    assert_eq!(
        text_of(&entry(1, "", "landed", "Landed: x"), Locale::En),
        "Landed: x"
    );
    assert_eq!(text_of(&entry(1, "", "mystery", ""), Locale::En), "mystery");
}

/* ── Blocked tasks in the log ─────────────────────────────────── */

#[test]
fn writes_a_block_and_an_unblock_once_whoever_saw_them() {
    let mut r = rig(vec![workspace("shop", "Shop")], Locale::En);
    r.io.manager = vec![ManagerNewsEntry {
        at: 100,
        event: ManagerNews::Block(BlockEvent::TaskBlocked {
            workspace_id: "shop".into(),
            workspace: "Shop".into(),
            task_id: "t1".into(),
            title: "Pay".into(),
            by: "Eli".into(),
            reason: "waiting".into(),
        }),
    }];
    r.io.orchestrators.insert(
        "shop".into(),
        vec![
            journal(
                120,
                OrchestratorEvent::TaskBlocked {
                    task_id: "t1".into(),
                    title: "Pay".into(),
                    by: "Eli".into(),
                    reason: "waiting".into(),
                    human_only: false,
                },
            ),
            journal(
                900_000,
                OrchestratorEvent::TaskUnblocked {
                    task_id: "t1".into(),
                    title: "Pay".into(),
                    how: "recheck passed".into(),
                },
            ),
        ],
    );
    r.tick();
    assert_eq!(
        r.kinds_and_texts(),
        [
            (
                "taskBlocked".to_string(),
                "Eli blocked Pay: waiting".to_string()
            ),
            (
                "taskUnblocked".to_string(),
                "Pay unblocked (recheck passed)".to_string()
            ),
        ]
    );
}
