//! remoteDelivery.test.ts, case for case: the delivery ops run through
//! P18's real `land_delivery` against a delivery core in memory.

use std::pin::Pin;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tokio::sync::oneshot;

use hangar_core::accounts::Account;
use hangar_core::board::{Board, Comment, Task, TaskPatch};
use hangar_core::delivery::{Cleanup, Delivery, Kind, Outcome, Plan, Running, Tested};
use hangar_core::git::{ChangedFile, Changes, FileDiff};
use hangar_core::shells::ShellInfo;

use crate::control::delivery::delivery_state;
use crate::control::port::{BoxFuture, DeliveryDeps, Port};
use crate::orchestration::testkit::{comment, strings};
use crate::state::types::{OrchestrationSettings, PaneRole};
use crate::state::{AgentId, AppState, Pane, Workspace};

use super::super::delivery::{PATCH_MAX_BYTES, cap_patch, delivery_history, run as run_delivery};
use super::super::engine::{Mobile, locked, run};
use super::super::ops::Tell;
use super::fake::{App, Fakes, Wait, assert_equal, assert_matches, ready};

/// What deliveryControl writes on the board as a delivery fails, then passes.
fn failed_check() -> Comment {
    comment(
        "c1",
        "hangar",
        "Delivery failed (checkFailed, 1 of 3): The check `pnpm test` failed (exit 1) on hangar/qr merged onto main. It \
         runs in a fresh checkout of that merge.\n```\nFAIL src/qr.test.ts\n```\nFix it on your branch, then call \
         delivery_submit again.",
        1_000,
    )
}

fn changes_requested() -> Comment {
    comment(
        "c2",
        "hangar",
        "Delivery failed (changesRequested, 2 of 3): The reviewer asked for changes before hangar/qr lands:\n\
         - src/qr.ts:12: the code is drawn before it is checked\n- src/i18n.ts: a French string is missing\n\
         Notes: Close.\nFix these on your branch, commit, then call delivery_submit again.",
        2_000,
    )
}

fn passed() -> Comment {
    comment(
        "c3",
        "hangar",
        "Passed: waits for the person to land it on main (tested as abc1234).\n\
         Summary: Settings › Mobile pairs a phone by QR code.\nThe code is single use.\n\
         Check: `pnpm test` passed in 84 s.\n\
         Review (sonnet): approved. Notes: Clean split.\nMinor: memoize the canvas.\n\
         Changed outside the files it owns, accepted by the review: src/i18n.ts.",
        3_000,
    )
}

fn task() -> Task {
    Task {
        id: "t1".into(),
        title: "Pair a phone by QR code".into(),
        description: String::new(),
        column: "review".into(),
        priority: 1,
        assignee: Some("Dana".into()),
        labels: strings(&["ui", "ready-to-land"]),
        comments: vec![
            failed_check(),
            comment("c-you", "user", "Looks close.", 1),
            changes_requested(),
            passed(),
        ],
        depends_on: Vec::new(),
        created_at: 0,
        updated_at: 0,
        order: 1.0,
        doing_since: None,
        owns: Vec::new(),
        attempts: 0,
        blocked: None,
    }
}

fn outcome(kind: Kind) -> Outcome {
    Outcome {
        outcome: kind,
        detail: String::new(),
        files: Vec::new(),
        in_checkout: false,
        changed: Vec::new(),
        outside_owns: Vec::new(),
        check: None,
        check_skipped: None,
        review: None,
        merge: Some("abc1234def567".into()),
        base: Some("0123456789ab".into()),
        landed_in: Some("C:\\dev\\app".into()),
    }
}

fn changed(
    path: &str,
    status: &str,
    additions: Option<u32>,
    deletions: Option<u32>,
) -> ChangedFile {
    ChangedFile {
        path: path.into(),
        old_path: None,
        status: status.into(),
        additions,
        deletions,
        binary: additions.is_none(),
    }
}

const WORKTREE: &str = "C:\\dev\\app-wt\\qr";

fn worker() -> Pane {
    Pane {
        id: "p-dana".into(),
        name: "Dana".into(),
        agent: AgentId::Claude,
        session_id: Some("s1".into()),
        cwd: Some(WORKTREE.into()),
        role: Some(PaneRole::Worker),
        task_id: Some("t1".into()),
        ..Pane::default()
    }
}

/// What the next delivery run answers: at once, or once `finish` says.
enum Next {
    Now(Box<Outcome>),
    Hold,
}

struct FakeDelivery {
    app: Arc<App>,
    log: Arc<Mutex<Vec<String>>>,
    tasks: Mutex<Vec<Task>>,
    next: Mutex<Next>,
    held: Mutex<Option<oneshot::Sender<Outcome>>>,
    /// Whether the PC showed the landing while it ran.
    shown: Mutex<Option<bool>>,
    tested: Mutex<Option<Tested>>,
    running: Mutex<Vec<Running>>,
}

impl FakeDelivery {
    fn push(&self, line: String) {
        locked(&self.log).push(line);
    }

    fn change(&self, id: &str, edit: impl FnOnce(&mut Task)) -> Result<Task, String> {
        let mut tasks = locked(&self.tasks);
        let task = tasks
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or("task not found")?;
        edit(task);
        Ok(task.clone())
    }
}

impl DeliveryDeps for FakeDelivery {
    fn preflight(
        &self,
        _workspace: String,
        worktree: String,
        _target: Option<String>,
        landing: bool,
        task: Option<String>,
    ) -> BoxFuture<Result<Plan, String>> {
        self.push(format!(
            "preflight:{worktree}:{landing}:{}",
            task.as_deref().unwrap_or("")
        ));
        ready(Ok(Plan {
            repo: "C:\\dev\\app".into(),
            queue: "c:/dev/app/.git".into(),
            branch: "hangar/qr".into(),
            target: "main".into(),
            ahead: 3,
            tested: if task.is_some() {
                locked(&self.tested).clone()
            } else {
                None
            },
        }))
    }

    fn run(&self, delivery: Delivery) -> BoxFuture<Result<Outcome, String>> {
        self.push(format!("run:{}:{}", delivery.land, delivery.reuse));
        *locked(&self.shown) = Some(self.app.is_delivering("w1"));
        match &*locked(&self.next) {
            Next::Now(outcome) => ready(Ok((**outcome).clone())),
            Next::Hold => {
                let (tx, rx) = oneshot::channel();
                *locked(&self.held) = Some(tx);
                Box::pin(async move { rx.await.map_err(|_| "never finished".to_string()) })
            }
        }
    }

    fn cleanup(
        &self,
        _worktree: String,
        _branch: String,
        _target: String,
        _task_id: String,
    ) -> BoxFuture<Result<Cleanup, String>> {
        ready(Ok(Cleanup {
            worktree_removed: true,
            branch_deleted: true,
            kept: Vec::new(),
        }))
    }

    fn running(&self) -> BoxFuture<Result<Vec<Running>, String>> {
        ready(Ok(locked(&self.running).clone()))
    }

    fn accounts(&self) -> BoxFuture<Result<Vec<Account>, String>> {
        ready(Ok(Vec::new()))
    }

    fn shells(&self) -> Vec<ShellInfo> {
        Vec::new()
    }

    fn board_load(&self, _cwd: &str) -> BoxFuture<Result<Board, String>> {
        ready(Ok(Board {
            tasks: locked(&self.tasks).clone(),
        }))
    }

    fn board_update(
        &self,
        _cwd: &str,
        id: &str,
        patch: TaskPatch,
    ) -> BoxFuture<Result<Task, String>> {
        ready(self.change(id, |task| {
            if let Some(column) = patch.column {
                task.column = column;
            }
            if let Some(labels) = patch.labels {
                task.labels = labels;
            }
        }))
    }

    fn board_comment(
        &self,
        _cwd: &str,
        id: &str,
        author: &str,
        text: &str,
    ) -> BoxFuture<Result<Task, String>> {
        self.push(format!("comment:{author}:{text}"));
        ready(self.change(id, |task| {
            let next = format!("c{}", task.comments.len() + 1);
            task.comments.push(comment(&next, author, text, 9_000));
        }))
    }

    fn tell(&self, pane: &String, text: &str) -> BoxFuture<Result<(), String>> {
        self.push(format!("tell:{pane}:{text}"));
        ready(Ok(()))
    }
}

/// `tell` of the tests: what the person's words for a worker would be.
#[derive(Default)]
struct Told(Mutex<Vec<String>>);

impl Tell for Told {
    fn tell<'a>(
        &'a self,
        _port: &'a Port,
        _mobile: &'a Mobile,
        ws: &'a Workspace,
        pane: &'a Pane,
        text: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
        locked(&self.0).push(format!("{}:{}:{text}", ws.id, pane.id));
        Box::pin(async { Ok(()) })
    }
}

struct Rig {
    app: Arc<App>,
    fakes: Arc<Fakes>,
    delivery: Arc<FakeDelivery>,
    mobile: Arc<Mobile>,
    port: Port,
    told: Told,
}

fn rig() -> Rig {
    let ava = Pane {
        id: "p-ava".into(),
        name: "Ava".into(),
        role: Some(PaneRole::Orchestrator),
        task_id: None,
        ..worker()
    };
    let ws = Workspace {
        id: "w1".into(),
        name: "App".into(),
        cwd: "C:\\dev\\app".into(),
        panes: vec![ava, worker()],
        orchestration: Some(OrchestrationSettings {
            auto_merge: false,
            check_command: Some("pnpm test".into()),
            orchestrator_model: "opus".into(),
            worker_model: "sonnet".into(),
            reviewer_model: "sonnet".into(),
            ..OrchestrationSettings::default()
        }),
        ..Workspace::default()
    };
    let app = App::new(AppState {
        workspaces: vec![ws],
        active_workspace_id: Some("w1".into()),
        ..AppState::default()
    });
    let fakes = Fakes::new(app.clone());
    let delivery = Arc::new(FakeDelivery {
        app: app.clone(),
        log: fakes.log.clone(),
        tasks: Mutex::new(vec![task()]),
        next: Mutex::new(Next::Now(Box::new(outcome(Kind::Landed)))),
        held: Mutex::new(None),
        shown: Mutex::new(None),
        tested: Mutex::new(Some(Tested {
            base: "0123456789ab".into(),
            merge: "abc1234def567".into(),
            current: true,
        })),
        running: Mutex::new(Vec::new()),
    });
    *locked(&app.delivery) = Some(delivery.clone() as Arc<dyn DeliveryDeps>);
    *locked(&fakes.changes) = Changes {
        root: WORKTREE.into(),
        base: Some("0123456".into()),
        truncated: false,
        files: vec![
            changed("assets/qr-frame.png", "A", None, None),
            changed("notes.txt", "?", Some(3), Some(0)),
            changed("src/i18n.ts", "M", Some(24), Some(3)),
            ChangedFile {
                old_path: Some("src/mobile/Pairing.tsx".into()),
                ..changed("src/mobile/Settings.tsx", "R", Some(4), Some(1))
            },
            changed("src/old.ts", "D", Some(0), Some(40)),
            changed("src/qr.ts", "A", Some(88), Some(0)),
        ],
    };
    locked(&fakes.diffs).extend([
        (
            "src/qr.ts".to_string(),
            FileDiff {
                text: "diff --git a/src/qr.ts b/src/qr.ts\n@@ -0,0 +1,2 @@\n+export const qr = 1;\n+\n".into(),
                binary: false,
                truncated: false,
            },
        ),
        (
            "assets/qr-frame.png".to_string(),
            FileDiff {
                text: "Binary files /dev/null and b/assets/qr-frame.png differ\n".into(),
                binary: true,
                truncated: false,
            },
        ),
    ]);
    let mobile = fakes.mobile();
    let port = app.port();
    Rig {
        app,
        fakes,
        delivery,
        mobile,
        port,
        told: Told::default(),
    }
}

impl Rig {
    async fn call(&self, op: &str, args: Value) -> Result<Value, String> {
        run_delivery(op, &args, &self.port, &self.mobile, &self.told).await
    }

    fn log(&self) -> Vec<String> {
        locked(&self.fakes.log).clone()
    }

    fn runs(&self) -> Vec<String> {
        self.log()
            .into_iter()
            .filter(|line| line.starts_with("run:"))
            .collect()
    }

    fn told(&self) -> Vec<String> {
        locked(&self.told.0).clone()
    }

    fn task(&self) -> Task {
        locked(&self.delivery.tasks)[0].clone()
    }
}

/// Lets the landings run until `done` says so, a little at a time.
async fn until(done: impl Fn() -> bool) {
    for _ in 0..1_000 {
        if done() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    panic!("it never happened");
}

#[track_caller]
fn refused(answer: Result<Value, String>, part: &str) {
    let error = answer.expect_err("a refusal");
    assert!(error.contains(part), "{error:?} does not say {part:?}");
}

/* ── The hangar comments ──────────────────────────────────────── */

#[test]
fn comments_say_what_the_check_and_the_review_of_the_delivery_that_waits_found() {
    let (passed, earlier) = delivery_history(&task().comments);
    let passed = passed.unwrap();
    assert_eq!(passed.at, 3_000);
    assert_eq!(passed.target, "main");
    assert_eq!(passed.merge.as_deref(), Some("abc1234"));
    assert_eq!(passed.in_the_way, None);
    assert_eq!(
        passed.summary.as_deref(),
        Some("Settings › Mobile pairs a phone by QR code.\nThe code is single use.")
    );
    assert_equal(
        &json!(passed.check),
        json!({"command": "pnpm test", "passed": true, "seconds": 84}),
    );
    assert_equal(
        &json!(passed.review),
        json!({"approved": true, "notes": "Clean split.\nMinor: memoize the canvas.", "blocking": []}),
    );
    assert_equal(
        &json!(earlier),
        json!([
            {"at": 1_000, "outcome": "checkFailed", "text": "The check `pnpm test` failed (exit 1) on hangar/qr merged onto main."},
            {"at": 2_000, "outcome": "changesRequested", "text": "The reviewer asked for changes before hangar/qr lands:",
             "blocking": [
                {"file": "src/qr.ts", "line": 12, "problem": "the code is drawn before it is checked"},
                {"file": "src/i18n.ts", "problem": "a French string is missing"},
             ]},
        ]),
    );
}

/// A failure counted since a task has no most it may fail (`<n>`, where the
/// boards on disk say `<n> of <max>`) reads as one written before.
#[test]
fn comments_read_a_failed_delivery_counted_with_or_without_a_most() {
    let since = comment(
        "c4",
        "hangar",
        &crate::control::delivery_text::failed_comment(
            "conflict",
            4,
            "Merging hangar/qr onto main conflicts in src/qr.ts.",
        ),
        4_000,
    );
    let (passed, earlier) = delivery_history(&[failed_check(), since]);
    assert!(passed.is_none());
    assert_equal(
        &json!(earlier),
        json!([
            {"at": 1_000, "outcome": "checkFailed", "text": "The check `pnpm test` failed (exit 1) on hangar/qr merged onto main."},
            {"at": 4_000, "outcome": "conflict", "text": "Merging hangar/qr onto main conflicts in src/qr.ts."},
        ]),
    );
}

#[test]
fn comments_read_a_delivery_without_a_summary_a_check_or_a_review() {
    let bare = comment(
        "c",
        "hangar",
        "Passed: waits for the person to land it on dev (tested as 1234567).\nCheck: none set for this workspace.",
        1,
    );
    let (passed, _) = delivery_history(std::slice::from_ref(&bare));
    let passed = passed.unwrap();
    assert_eq!(passed.target, "dev");
    assert_eq!(
        (passed.summary, passed.check, passed.review),
        (None, None, None)
    );
    // Someone else writing the same words is not Hangar.
    let user = Comment {
        author: "user".into(),
        ..bare
    };
    assert_eq!(delivery_history(&[user]).0, None);
}

#[test]
fn comments_know_the_person_s_local_changes_stood_in_the_way_and_keep_what_a_landing_tried_again_did_not_repeat()
 {
    let blocked = comment(
        "c4",
        "hangar",
        "Passed, but main's checkout has local changes in the way (src/a.ts, src/b.ts): waits for the person to land \
         it once they are committed or set aside (tested as abc1234).\nLanded as it was checked and reviewed when it was submitted.",
        4_000,
    );
    let mut comments = task().comments;
    comments.push(blocked);
    let (passed, earlier) = delivery_history(&comments);
    let passed = passed.unwrap();
    assert_eq!(passed.at, 4_000);
    assert_eq!(passed.in_the_way, Some(strings(&["src/a.ts", "src/b.ts"])));
    assert_eq!(
        passed.summary.as_deref(),
        Some("Settings › Mobile pairs a phone by QR code.\nThe code is single use.")
    );
    assert_matches(
        &json!(passed.check),
        json!({"command": "pnpm test", "passed": true, "seconds": 84}),
    );
    assert_matches(&json!(passed.review), json!({"approved": true}));
    assert_eq!(earlier.len(), 2);
}

/* ── remote_delivery ──────────────────────────────────────────── */

#[tokio::test]
async fn delivery_says_what_it_brings_what_the_check_and_the_review_said_and_where_it_lands() {
    let r = rig();
    let answer = r
        .call("remote_delivery", json!({"task": "t1"}))
        .await
        .unwrap();
    let mut expected = json!({
        "taskId": "t1", "title": "Pair a phone by QR code", "workspace": "App", "workspaceId": "w1", "worker": "Dana",
        "branch": "hangar/qr", "target": "main",
        "summary": "Settings › Mobile pairs a phone by QR code.\nThe code is single use.",
        "check": {"command": "pnpm test", "passed": true, "seconds": 84},
        "review": {"approved": true, "notes": "Clean split.\nMinor: memoize the canvas.", "blocking": []},
        // The untracked notes.txt was never delivered.
        "files": [
            {"path": "assets/qr-frame.png", "status": "added", "additions": 0, "deletions": 0, "binary": true},
            {"path": "src/i18n.ts", "status": "modified", "additions": 24, "deletions": 3},
            {"path": "src/mobile/Settings.tsx", "oldPath": "src/mobile/Pairing.tsx", "status": "renamed", "additions": 4, "deletions": 1},
            {"path": "src/old.ts", "status": "deleted", "additions": 0, "deletions": 40},
            {"path": "src/qr.ts", "status": "added", "additions": 88, "deletions": 0},
        ],
        "waiting": true, "stage": null, "merge": "abc1234", "passedAt": 3_000, "moved": false, "inTheWay": null,
    });
    expected["earlier"] = answer["earlier"].clone();
    assert!(answer["earlier"].is_array());
    assert_equal(&answer, expected);
    // The worktree against the branch it lands on, as delivery.rs names them.
    assert_eq!(
        r.log(),
        [
            format!("preflight:{WORKTREE}:true:t1"),
            format!("changes:{WORKTREE}:main")
        ]
    );
}

#[tokio::test]
async fn delivery_says_when_the_worker_committed_since_and_when_a_landing_runs_for_it() {
    let r = rig();
    locked(&r.delivery.tested).as_mut().unwrap().current = false;
    *locked(&r.delivery.running) = vec![Running {
        task_id: "t1".into(),
        queue: "q".into(),
        stage: "checking".into(),
        since: 1,
    }];
    locked(&r.fakes.changes).truncated = true;
    let answer = r
        .call("remote_delivery", json!({"task": "t1"}))
        .await
        .unwrap();
    assert_matches(
        &answer,
        json!({"moved": true, "waiting": false, "stage": "checking", "moreFiles": true}),
    );
}

#[tokio::test]
async fn delivery_refuses_a_task_that_does_not_wait_to_land_or_that_no_worker_holds() {
    let r = rig();
    *locked(&r.delivery.tasks) = vec![Task {
        column: "doing".into(),
        labels: strings(&["ui"]),
        ..task()
    }];
    for op in [
        "remote_delivery",
        "remote_delivery_file",
        "remote_land",
        "remote_rework",
    ] {
        refused(
            r.call(
                op,
                json!({"task": "t1", "file": "src/qr.ts", "text": "Fix it"}),
            )
            .await,
            "Nothing of \"Pair a phone by QR code\" waits to land any more",
        );
    }
    r.app
        .change(|state| state.workspaces[0].panes.retain(|pane| pane.id != "p-dana"));
    refused(
        r.call("remote_delivery", json!({"task": "t1"})).await,
        "No worker holds this task",
    );
    refused(r.call("remote_delivery", json!({})).await, "Name the task");
    assert!(r.runs().is_empty());
    assert!(r.told().is_empty());
}

/* ── remote_delivery_file ─────────────────────────────────────── */

#[tokio::test]
async fn delivery_file_gives_one_file_s_patch_against_the_branch_it_lands_on() {
    let r = rig();
    let text = locked(&r.fakes.diffs)["src/qr.ts"].text.clone();
    assert_equal(
        &r.call(
            "remote_delivery_file",
            json!({"task": "t1", "file": "src/qr.ts"}),
        )
        .await
        .unwrap(),
        json!({"path": "src/qr.ts", "patch": text, "binary": false, "truncated": false}),
    );
    assert!(r.log().contains(&format!("diff:{WORKTREE}:src/qr.ts:main")));
    assert_equal(
        &r.call(
            "remote_delivery_file",
            json!({"task": "t1", "file": "assets/qr-frame.png"}),
        )
        .await
        .unwrap(),
        json!({"path": "assets/qr-frame.png", "patch": "", "binary": true, "truncated": false}),
    );
}

#[tokio::test]
async fn delivery_file_cuts_a_big_patch_at_200_kb_and_says_so() {
    let r = rig();
    locked(&r.fakes.diffs).insert(
        "src/i18n.ts".into(),
        FileDiff {
            text: format!(
                "@@ -1,1 +1,9000 @@\n{}",
                "+ a line of the dictionary\n".repeat(9_000)
            ),
            binary: false,
            truncated: false,
        },
    );
    let big = r
        .call(
            "remote_delivery_file",
            json!({"task": "t1", "file": "src/i18n.ts"}),
        )
        .await
        .unwrap();
    assert_eq!(big["truncated"], true);
    let patch = big["patch"].as_str().unwrap();
    assert!(patch.len() <= PATCH_MAX_BYTES);
    assert!(patch.ends_with("+ a line of the dictionary\n"));
}

#[tokio::test]
async fn delivery_file_refuses_a_file_the_delivery_does_not_change() {
    let r = rig();
    refused(
        r.call(
            "remote_delivery_file",
            json!({"task": "t1", "file": "notes.txt"}),
        )
        .await,
        "\"notes.txt\" is not one of the files",
    );
    refused(
        r.call("remote_delivery_file", json!({"task": "t1"})).await,
        "Name the file",
    );
}

#[test]
fn delivery_file_never_cuts_inside_a_line_nor_inside_a_character() {
    assert_eq!(cap_patch("ab\ncd\n", 5), ("ab\n".to_string(), true));
    assert_eq!(cap_patch("ab\ncd\n", 6), ("ab\ncd\n".to_string(), false));
    // One line longer than the cut: it stops before the "é" it would split.
    assert_eq!(cap_patch("abé", 3), ("ab".to_string(), true));
}

/* ── remote_land ──────────────────────────────────────────────── */

#[tokio::test]
async fn land_lands_what_waits_as_the_pc_s_fusionner_does() {
    let r = rig();
    assert_equal(
        &r.call("remote_land", json!({"task": "t1", "merge": "abc1234"}))
            .await
            .unwrap(),
        json!({"landed": true, "target": "main"}),
    );
    // Tested back then, landed as it was.
    assert!(r.log().contains(&"run:true:true".to_string()));
    assert_eq!(*locked(&r.delivery.shown), Some(true));
    assert!(!r.app.is_delivering("w1"));
    let landed = r.task();
    assert_eq!(
        (landed.column.as_str(), landed.labels),
        ("done", strings(&["ui"]))
    );
    assert!(r.app.logged("close:").contains(&"w1:p-dana".to_string()));
}

#[tokio::test]
async fn land_relays_why_it_did_not_land_in_land_delivery_s_words() {
    let r = rig();
    *locked(&r.delivery.next) = Next::Now(Box::new(Outcome {
        in_checkout: true,
        files: strings(&["src/a.ts"]),
        detail: "passed, but…".into(),
        landed_in: None,
        ..outcome(Kind::Dirty)
    }));
    assert_eq!(
        r.call("remote_land", json!({"task": "t1"})).await,
        Err("Local changes in main's checkout are in the way (src/a.ts): commit them or set them aside, then land it again. \
             Nothing was touched."
            .into())
    );
    // Told once: the next « Fusionner » is a landing of its own.
    *locked(&r.delivery.next) = Next::Now(Box::new(outcome(Kind::Landed)));
    assert_equal(
        &r.call("remote_land", json!({"task": "t1"})).await.unwrap(),
        json!({"landed": true, "target": "main"}),
    );
    assert_eq!(r.runs().len(), 2);
}

#[tokio::test]
async fn land_refuses_to_land_a_delivery_other_than_the_one_the_phone_showed() {
    let r = rig();
    locked(&r.delivery.tested).as_mut().unwrap().merge = "fedcba9876543".into();
    let error = r
        .call("remote_land", json!({"task": "t1", "merge": "abc1234"}))
        .await
        .unwrap_err();
    assert!(
        error.contains("delivered again since you opened it (now fedcba9, you saw abc1234)"),
        "{error}"
    );
    assert!(error.ends_with("Nothing was done."), "{error}");
    assert!(r.runs().is_empty());
}

#[tokio::test]
async fn land_answers_while_a_long_landing_runs_and_the_same_call_waits_for_that_landing() {
    let r = rig();
    *locked(&r.delivery.next) = Next::Hold;
    *locked(&r.fakes.wait) = Wait::Now;
    *locked(&r.delivery.running) = vec![Running {
        task_id: "t1".into(),
        queue: "q".into(),
        stage: "checking".into(),
        since: 1,
    }];

    assert_equal(
        &r.call("remote_land", json!({"task": "t1", "merge": "abc1234"}))
            .await
            .unwrap(),
        json!({"landed": false, "stage": "checking"}),
    );
    assert_matches(
        &r.call("remote_delivery", json!({"task": "t1"}))
            .await
            .unwrap(),
        json!({"waiting": false, "stage": "checking"}),
    );
    refused(
        r.call("remote_rework", json!({"task": "t1", "text": "Fix it"}))
            .await,
        "being landed or tested right now",
    );
    locked(&r.delivery.running).clear();
    assert_equal(
        &r.call("remote_land", json!({"task": "t1"})).await.unwrap(),
        json!({"landed": false, "stage": "queued"}),
    );

    until(|| locked(&r.delivery.held).is_some()).await;
    let finish = locked(&r.delivery.held).take().unwrap();
    finish.send(outcome(Kind::Landed)).unwrap();
    let port = r.port.clone();
    until(|| {
        delivery_state(&port, "t1").is_some_and(|state| state.state() == "landed")
            && !r.app.is_delivering("w1")
    })
    .await;
    assert_equal(
        &r.call("remote_land", json!({"task": "t1"})).await.unwrap(),
        json!({"landed": true, "target": "main"}),
    );
    assert_eq!(r.runs(), ["run:true:true"]);
}

/// MOB-07: two taps at once — two phones, or a retry — start one landing,
/// and the outcome is the one landing's.
#[tokio::test]
async fn land_tapped_twice_at_once_starts_one_landing_and_tells_its_outcome() {
    let r = rig();
    *locked(&r.delivery.next) = Next::Hold;
    *locked(&r.fakes.wait) = Wait::Now;
    let (first, second) = tokio::join!(
        r.call("remote_land", json!({"task": "t1", "merge": "abc1234"})),
        r.call("remote_land", json!({"task": "t1", "merge": "abc1234"})),
    );
    for answer in [first, second] {
        assert_equal(&answer.unwrap()["landed"], json!(false));
    }
    until(|| locked(&r.delivery.held).is_some()).await;
    let finish = locked(&r.delivery.held).take().unwrap();
    finish.send(outcome(Kind::Landed)).unwrap();
    let port = r.port.clone();
    until(|| {
        delivery_state(&port, "t1").is_some_and(|state| state.state() == "landed")
            && !r.app.is_delivering("w1")
    })
    .await;
    assert_equal(
        &r.call("remote_land", json!({"task": "t1"})).await.unwrap(),
        json!({"landed": true, "target": "main"}),
    );
    assert_eq!(r.runs(), ["run:true:true"]);
}

/* ── remote_rework ────────────────────────────────────────────── */

#[tokio::test]
async fn rework_sends_the_person_s_words_to_the_worker_and_records_them_on_the_task() {
    let r = rig();
    assert_equal(
        &r.call("remote_rework", json!({"task": "t1", "text": "  The QR code is too small on an iPhone SE.  ", "merge": "abc1234"}))
            .await
            .unwrap(),
        json!({"sent": true}),
    );
    assert_eq!(
        r.told(),
        ["w1:p-dana:The QR code is too small on an iPhone SE."]
    );
    assert!(
        r.log()
            .contains(&"comment:user:Sent back to Dana from the phone, to fix before landing: The QR code is too small on an iPhone SE.".to_string())
    );
    // What waits stays until the worker's next delivery replaces it.
    let kept = r.task();
    assert_eq!(
        (kept.column.as_str(), kept.labels),
        ("review", strings(&["ui", "ready-to-land"]))
    );
}

#[tokio::test]
async fn rework_refuses_an_empty_instruction_or_one_for_a_delivery_since_replaced() {
    let r = rig();
    refused(
        r.call("remote_rework", json!({"task": "t1", "text": "  "}))
            .await,
        "Say what to fix",
    );
    locked(&r.delivery.tested).as_mut().unwrap().merge = "fedcba9876543".into();
    refused(
        r.call(
            "remote_rework",
            json!({"task": "t1", "text": "Fix it", "merge": "abc1234"}),
        )
        .await,
        "delivered again",
    );
    assert!(r.told().is_empty());
}

#[tokio::test]
async fn rework_reaches_the_worker_through_the_queue_as_the_person_its_project_woken_for_it() {
    let r = rig();
    r.app.sleep_ws("w1");
    let answer = run(
        "remote_rework",
        &json!({"task": "t1", "text": "Use the accent colour"}),
        &r.port,
        &r.mobile,
    )
    .await;
    assert_eq!(answer, Ok(json!({"sent": true})));
    // No `from`: the session reads it as the person's.
    assert_eq!(r.app.queued(), ["p-dana:Use the accent colour"]);
    assert_eq!(r.app.logged("open:"), ["w1"]);
}
