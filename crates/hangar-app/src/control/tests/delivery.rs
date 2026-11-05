//! deliveryControl.test.ts, case for case.

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tokio::sync::oneshot;

use hangar_core::accounts::Account;
use hangar_core::board::{Board, Comment, Task, TaskPatch};
use hangar_core::delivery::{
    Blocking, Check, Cleanup, Delivery, Kind, Outcome, Plan, Running, Tested, Verdict,
};
use hangar_core::shells::ShellInfo;

use hangar_i18n::Locale;

use super::super::delivery::{control, delivering_in, delivery_state, land_delivery};
use super::super::fake::{Fake, lock, pane, workspace};
use super::super::land;
use super::super::port::{BoxFuture, DeliveryDeps, OrchestrationHooks, Port, Projects};
use super::super::types::{Caller, OrchestratorEvent};
use super::{contains, matches};
use crate::message::{Intent, Message};
use crate::needs_you;
use crate::shell::Notice;
use crate::state::types::{OrchestrationSettings, PaneRole};
use crate::state::{AgentId, AppState, Pane, Workspace, WsId};

/// What the next delivery run answers: at once, or once `finish` says.
enum Next {
    Now(Box<Result<Outcome, String>>),
    Hold,
}

struct FakeDelivery {
    log: Arc<Mutex<Vec<String>>>,
    tasks: Mutex<Vec<Task>>,
    runs: Mutex<Vec<Delivery>>,
    next: Mutex<Next>,
    held: Mutex<Option<oneshot::Sender<Outcome>>>,
    preflight_error: Mutex<Option<String>>,
    /// What preflight says waits for the person, when asked about a task.
    tested: Mutex<Option<Tested>>,
    preflight_tasks: Mutex<Vec<Option<String>>>,
    running: Mutex<Vec<Running>>,
    /// How many times the accounts were listed.
    listed_accounts: Mutex<usize>,
}

fn ready<T: Send + 'static>(value: T) -> BoxFuture<T> {
    Box::pin(std::future::ready(value))
}

impl FakeDelivery {
    fn push(&self, line: String) {
        lock(&self.log).push(line);
    }

    fn change(&self, id: &str, edit: impl FnOnce(&mut Task)) -> Result<Task, String> {
        let mut tasks = lock(&self.tasks);
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
        target: Option<String>,
        landing: bool,
        task: Option<String>,
    ) -> BoxFuture<Result<Plan, String>> {
        self.push(format!(
            "preflight:{worktree}:{}:{landing}",
            target.as_deref().unwrap_or("null")
        ));
        let asked = task.is_some();
        lock(&self.preflight_tasks).push(task);
        if let Some(error) = lock(&self.preflight_error).clone() {
            return ready(Err(error));
        }
        ready(Ok(Plan {
            repo: "C:\\dev\\app".into(),
            queue: "c:/dev/app/.git".into(),
            branch: "worker/t1".into(),
            target: "main".into(),
            ahead: 2,
            tested: if asked {
                lock(&self.tested).clone()
            } else {
                None
            },
        }))
    }

    fn run(&self, delivery: Delivery) -> BoxFuture<Result<Outcome, String>> {
        lock(&self.runs).push(delivery);
        match &*lock(&self.next) {
            Next::Now(outcome) => ready((**outcome).clone()),
            Next::Hold => {
                let (tx, rx) = oneshot::channel();
                *lock(&self.held) = Some(tx);
                Box::pin(async move { rx.await.map_err(|_| "never finished".to_string()) })
            }
        }
    }

    fn cleanup(
        &self,
        worktree: String,
        branch: String,
        target: String,
        _task_id: String,
    ) -> BoxFuture<Result<Cleanup, String>> {
        self.push(format!("cleanup:{worktree}:{branch}:{target}"));
        ready(Ok(Cleanup {
            worktree_removed: true,
            branch_deleted: true,
            kept: vec![],
        }))
    }

    fn running(&self) -> BoxFuture<Result<Vec<Running>, String>> {
        ready(Ok(lock(&self.running).clone()))
    }

    fn accounts(&self) -> BoxFuture<Result<Vec<Account>, String>> {
        *lock(&self.listed_accounts) += 1;
        ready(Ok(vec![Account {
            id: "work".into(),
            label: "Work".into(),
            is_main: false,
            logged_in: true,
            email: None,
            plan: None,
        }]))
    }

    fn shells(&self) -> Vec<ShellInfo> {
        vec![ShellInfo {
            id: "pwsh".into(),
            label: "PowerShell 7".into(),
            program: "pwsh.exe".into(),
            args: vec![],
        }]
    }

    fn board_load(&self, _cwd: &str) -> BoxFuture<Result<Board, String>> {
        ready(Ok(Board {
            tasks: lock(&self.tasks).clone(),
        }))
    }

    fn board_update(
        &self,
        _cwd: &str,
        id: &str,
        patch: TaskPatch,
    ) -> BoxFuture<Result<Task, String>> {
        self.push(format!(
            "update:{id}:{}:{}",
            patch.column.as_deref().unwrap_or(""),
            patch
                .labels
                .as_ref()
                .map(|labels| labels.join(","))
                .unwrap_or_default()
        ));
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
        self.push(format!("comment:{id}:{author}:{text}"));
        ready(self.change(id, |task| {
            task.comments.push(Comment {
                id: "c".into(),
                author: author.into(),
                text: text.into(),
                created_at: 0,
            })
        }))
    }

    fn tell(&self, pane: &crate::state::PaneId, text: &str) -> BoxFuture<Result<(), String>> {
        self.push(format!("tell:{pane}:{text}"));
        ready(Ok(()))
    }
}

/// The orchestration engine, as far as deliveries reach it: the events.
#[derive(Default)]
struct Events(Mutex<Vec<(WsId, OrchestratorEvent)>>);

impl OrchestrationHooks for Events {
    fn worker_control(
        &self,
        _port: Port,
        _op: String,
        _ws: Workspace,
        _args: Value,
        _caller: Caller,
    ) -> BoxFuture<Result<Value, String>> {
        ready(Err("not here".into()))
    }

    fn briefing(&self, _port: Port, _args: Value) -> BoxFuture<Result<Value, String>> {
        ready(Ok(Value::Null))
    }

    fn post(&self, ws: &WsId, event: OrchestratorEvent, _quiet: bool) {
        lock(&self.0).push((ws.clone(), event));
    }

    fn projects(&self, _port: Port) -> BoxFuture<Result<Projects, String>> {
        ready(Ok(Projects::default()))
    }
}

fn task() -> Task {
    Task {
        id: "t1".into(),
        title: "Add the export button".into(),
        description: "A button that exports the board as CSV.".into(),
        column: "doing".into(),
        priority: 2,
        assignee: Some("Bo".into()),
        labels: vec!["ui".into()],
        comments: vec![],
        depends_on: vec![],
        created_at: 0,
        updated_at: 0,
        order: 1.0,
        doing_since: None,
        owns: vec!["src/export".into()],
        attempts: 0,
        blocked: None,
    }
}

fn outcome(kind: Kind, detail: &str) -> Outcome {
    Outcome {
        outcome: kind,
        detail: detail.into(),
        files: vec![],
        in_checkout: false,
        changed: vec!["src/export/button.tsx".into()],
        outside_owns: vec![],
        check: None,
        check_skipped: None,
        review: None,
        merge: Some("abcdef1234567890".into()),
        base: Some("1234567abcdef".into()),
        landed_in: Some("C:\\dev\\app".into()),
    }
}

fn worker() -> Pane {
    let mut bo = pane("p2", "Bo", AgentId::Claude, Some("C:\\dev\\app-wt\\t1"));
    bo.session_id = None;
    bo.role = Some(PaneRole::Worker);
    bo.task_id = Some("t1".into());
    bo.port_base = Some(41020);
    bo
}

struct Setup {
    fake: Arc<Fake>,
    delivery: Arc<FakeDelivery>,
    events: Arc<Events>,
}

fn setup() -> Setup {
    let mut ava = pane("p1", "Ava", AgentId::Claude, None);
    ava.role = Some(PaneRole::Orchestrator);
    let mut app = workspace(
        "w1",
        "App",
        "C:\\dev\\app",
        vec![ava, worker(), pane("p3", "Cy", AgentId::Claude, None)],
    );
    app.shell_id = Some("pwsh".into());
    app.orchestration = Some(OrchestrationSettings {
        check_command: Some("pnpm test".into()),
        ..OrchestrationSettings::default()
    });
    let fake = Fake::with_state(AppState {
        workspaces: vec![app],
        active_workspace_id: Some("w1".into()),
        ..AppState::default()
    });
    let delivery = Arc::new(FakeDelivery {
        log: fake.log.clone(),
        tasks: Mutex::new(vec![task()]),
        runs: Mutex::new(vec![]),
        next: Mutex::new(Next::Now(Box::new(Ok(outcome(
            Kind::Landed,
            "worker landed on main (abcdef1).",
        ))))),
        held: Mutex::new(None),
        preflight_error: Mutex::new(None),
        tested: Mutex::new(None),
        preflight_tasks: Mutex::new(vec![]),
        running: Mutex::new(vec![]),
        listed_accounts: Mutex::new(0),
    });
    let events = Arc::new(Events::default());
    *lock(&fake.delivery) = Some(delivery.clone());
    *lock(&fake.engine) = Some(events.clone());
    Setup {
        fake,
        delivery,
        events,
    }
}

impl Setup {
    fn ws(&self) -> Workspace {
        lock(&self.fake.state).workspaces[0].clone()
    }

    fn port(&self) -> Port {
        self.fake.port()
    }

    async fn submit_as(&self, args: Value, pane: Option<&str>) -> Result<Value, String> {
        let caller = Caller {
            pane_id: pane.map(str::to_string),
            pane_name: None,
            cwd: Some("C:\\dev\\app".into()),
        };
        control("delivery_submit", &self.ws(), &args, &caller, &self.port()).await
    }

    async fn submit(&self, args: Value) -> Result<Value, String> {
        self.submit_as(args, Some("p2")).await
    }

    fn next(&self, next: Next) {
        *lock(&self.delivery.next) = next;
    }

    fn now(&self, kind: Kind, detail: &str, edit: impl FnOnce(&mut Outcome)) {
        let mut answer = outcome(kind, detail);
        edit(&mut answer);
        self.next(Next::Now(Box::new(Ok(answer))));
    }

    /// Ends the delivery held in its run.
    fn finish(&self, answer: Outcome) {
        let held = lock(&self.delivery.held)
            .take()
            .expect("no delivery is held");
        let _ = held.send(answer);
    }

    fn runs(&self) -> Vec<Delivery> {
        lock(&self.delivery.runs).clone()
    }

    fn task(&self) -> Task {
        lock(&self.delivery.tasks)[0].clone()
    }

    fn set_tasks(&self, tasks: Vec<Task>) {
        *lock(&self.delivery.tasks) = tasks;
    }

    fn line(&self, prefix: &str) -> String {
        self.fake
            .logged(prefix)
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("nothing logged as {prefix}"))
    }

    fn events(&self) -> Vec<Value> {
        lock(&self.events.0)
            .iter()
            .map(|(_, event)| serde_json::to_value(event).unwrap())
            .collect()
    }

    fn state(&self) -> Option<String> {
        delivery_state(&self.port(), "t1").map(|state| state.state().to_string())
    }

    fn settings(&self, edit: impl FnOnce(&mut OrchestrationSettings)) {
        self.fake
            .patch("w1", |ws| edit(ws.orchestration.as_mut().unwrap()));
    }
}

/// Until `cond` holds, letting what runs in the background go on meanwhile.
async fn until(cond: impl Fn() -> bool) {
    for _ in 0..1000 {
        if cond() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("never came");
}

fn delivery_json(delivery: &Delivery) -> Value {
    json!({
        "workspace": delivery.workspace,
        "worktree": delivery.worktree,
        "branch": delivery.branch,
        "target": delivery.target,
        "task": {"id": delivery.task.id, "title": delivery.task.title, "owns": delivery.task.owns},
        "summary": delivery.summary,
        "checkCommand": delivery.check_command,
        "reviewerModel": delivery.reviewer_model,
        "claudeAccount": delivery.claude_account,
        "land": delivery.land,
        "reuse": delivery.reuse,
        "portBase": delivery.port_base,
        "shell": delivery.shell.as_ref().map(|shell| json!({"id": shell.id})),
    })
}

/* ── delivery_submit ─────────────────────────────────────────── */

#[tokio::test]
async fn is_only_for_a_worker_pane_on_a_task() {
    let s = setup();
    let refused = s.submit_as(json!({}), Some("p1")).await.unwrap_err();
    contains(&refused, "for worker panes");
    contains(&refused, "worker_spawn");
    contains(
        &s.submit_as(json!({}), Some("p3")).await.unwrap_err(),
        "for worker panes",
    );
    contains(
        &s.submit_as(json!({}), None).await.unwrap_err(),
        "for worker panes",
    );
    assert!(s.runs().is_empty());
}

#[tokio::test]
async fn refuses_a_task_that_is_not_being_worked_on_or_already_passed() {
    let s = setup();
    for (column, refused) in [
        ("done", "already done"),
        ("review", "is in review, not doing"),
        ("todo", "is in todo, not doing"),
    ] {
        let mut held = task();
        held.column = column.into();
        s.set_tasks(vec![held]);
        contains(&s.submit(json!({})).await.unwrap_err(), refused);
    }
    s.set_tasks(vec![]);
    contains(
        &s.submit(json!({})).await.unwrap_err(),
        "no longer on the board",
    );
    assert!(s.runs().is_empty());
}

#[tokio::test]
async fn hands_the_worker_gits_refusal_as_it_is() {
    let s = setup();
    *lock(&s.delivery.preflight_error) =
        Some("Your worktree has uncommitted changes: a.ts. Only commits are delivered.".into());
    contains(
        &s.submit(json!({})).await.unwrap_err(),
        "Your worktree has uncommitted changes: a.ts.",
    );
    assert!(s.runs().is_empty());
    assert_eq!(s.state(), None);
}

#[tokio::test]
async fn answers_at_once_and_runs_the_delivery_with_the_workspaces_settings() {
    let s = setup();
    s.next(Next::Hold);
    let answer = s
        .submit(json!({"summary": "  Added the button.  "}))
        .await
        .unwrap();
    matches(
        &answer,
        json!({"submitted": true, "branch": "worker/t1", "target": "main", "commits": 2}),
    );
    let next = answer["next"].as_str().unwrap();
    contains(next, "runs `pnpm test` there");
    contains(next, "message from Hangar");
    contains(next, "End your turn");
    assert_eq!(
        s.fake.logged("preflight:")[0],
        "preflight:C:\\dev\\app-wt\\t1:null:false"
    );
    until(|| !s.runs().is_empty()).await;
    matches(
        &delivery_json(&s.runs()[0]),
        json!({
            "workspace": "C:\\dev\\app",
            "worktree": "C:\\dev\\app-wt\\t1",
            "branch": "worker/t1",
            "target": "main",
            "task": {"id": "t1", "title": "Add the export button", "owns": ["src/export"]},
            "summary": "Added the button.",
            "checkCommand": "pnpm test",
            "reviewerModel": "sonnet",
            "land": true,
            "reuse": false,
            "portBase": 41020,
            "shell": {"id": "pwsh"},
        }),
    );
    assert_eq!(s.state().as_deref(), Some("running"));
    assert!(delivering_in(&s.port(), "w1"));
    contains(
        &s.submit(json!({})).await.unwrap_err(),
        "already being merged, checked and reviewed",
    );
    assert_eq!(s.runs().len(), 1);

    s.finish(outcome(Kind::Landed, "landed"));
    s.fake.settle().await;
    assert_eq!(s.state().as_deref(), Some("landed"));
    assert!(!delivering_in(&s.port(), "w1"));
    // The model heard of it too: hibernation reads it there.
    assert_eq!(s.fake.logged("begin:"), vec!["begin:w1:t1"]);
    assert_eq!(s.fake.logged("end:"), vec!["end:w1:t1"]);
}

#[tokio::test]
async fn reviews_on_the_workspaces_claude_account_and_on_nothing_when_it_is_the_default_one() {
    let s = setup();
    // A delivery that does not land keeps the worker's pane for the next one.
    s.now(Kind::Conflict, "conflict", |_| {});
    for (account, expected) in [
        (None, None),
        (Some("work"), Some("work")),
        (Some("gone"), None),
        (Some("main"), None),
    ] {
        s.fake
            .patch("w1", |ws| ws.claude_account = account.map(str::to_string));
        s.submit(json!({})).await.unwrap();
        s.fake.settle().await;
        assert_eq!(
            s.runs().last().unwrap().claude_account.as_deref(),
            expected,
            "{account:?}"
        );
    }
}

#[tokio::test]
async fn asks_for_the_accounts_only_when_a_level_names_one_other_than_main() {
    let s = setup();
    s.now(Kind::Conflict, "conflict", |_| {});
    // Nothing named, or main: the review runs on main without a listing.
    for account in [None, Some("main")] {
        s.fake
            .patch("w1", |ws| ws.claude_account = account.map(str::to_string));
        s.submit(json!({})).await.unwrap();
        s.fake.settle().await;
    }
    assert_eq!(*lock(&s.delivery.listed_accounts), 0);
    // The person's default names one: it has to be found, and the
    // workspace's main still wins over it.
    lock(&s.fake.state).settings.claude_account = Some("work".into());
    s.fake
        .patch("w1", |ws| ws.claude_account = Some("main".into()));
    s.submit(json!({})).await.unwrap();
    s.fake.settle().await;
    assert_eq!(*lock(&s.delivery.listed_accounts), 1);
    assert_eq!(s.runs().last().unwrap().claude_account, None);
    s.fake.patch("w1", |ws| ws.claude_account = None);
    s.submit(json!({})).await.unwrap();
    s.fake.settle().await;
    assert_eq!(
        s.runs().last().unwrap().claude_account.as_deref(),
        Some("work")
    );
}

#[tokio::test]
async fn does_not_count_a_review_stopped_by_a_usage_limit_as_a_failed_delivery() {
    let s = setup();
    s.now(
        Kind::Error,
        "The review could not run: the Claude account \"Work\" has reached its usage limit, so the review could not run",
        |answer| answer.merge = None,
    );
    s.submit(json!({})).await.unwrap();
    s.fake.settle().await;
    let comment = s.line("comment:");
    assert!(!comment.contains("Delivery failed"));
    contains(&comment, "usage limit");
    contains(&s.line("tell:p2:"), "not a problem with your work");
    let last = s.events().pop().unwrap();
    matches(&last, json!({"outcome": "error"}));
    assert!(!last.to_string().contains("failed delivery"));
}

#[tokio::test]
async fn leaves_a_landing_to_the_person_when_automatic_landing_is_off() {
    let s = setup();
    s.settings(|settings| settings.auto_merge = false);
    s.submit(json!({})).await.unwrap();
    s.fake.settle().await;
    assert!(!s.runs()[0].land);
}

/* ── a worker in a workspace without git ─────────────────────── */

#[tokio::test]
async fn is_done_when_it_delivers_nothing_runs_the_orchestrator_hears_it_and_its_pane_goes_after_the_answer()
 {
    let s = setup();
    s.fake.patch("w1", |ws| {
        let bo = &mut ws.panes[1];
        bo.cwd = Some("C:\\dev\\app".into());
        bo.shared_folder = Some(true);
    });
    let mut held = task();
    held.labels = vec!["ui".into(), "needs-you".into()];
    s.set_tasks(vec![held]);

    let answer = s
        .submit(json!({"summary": " Added the button. "}))
        .await
        .unwrap();

    matches(
        &answer,
        json!({"submitted": true, "done": true, "task": "Add the export button"}),
    );
    let next = answer["next"].as_str().unwrap();
    contains(next, "nothing to merge");
    contains(next, "End your turn");
    assert!(s.fake.logged("preflight:").is_empty());
    assert!(s.runs().is_empty());
    assert_eq!(s.task().column, "done");
    assert_eq!(s.task().labels, vec!["ui"]);
    let comment = s.line("comment:");
    contains(
        &comment,
        "comment:t1:hangar:Done by Bo in the workspace's folder.",
    );
    contains(&comment, "Summary: Added the button.");
    assert_eq!(s.state().as_deref(), Some("done"));
    assert!(!delivering_in(&s.port(), "w1"));
    assert_eq!(
        s.events(),
        vec![
            json!({"kind": "delivery", "taskId": "t1", "title": "Add the export button",
                    "worker": "Bo", "outcome": "done", "detail": "summary: Added the button."})
        ]
    );
    assert!(!s.fake.log().contains(&"close:w1:p2".into()));
    s.fake.settle().await;
    assert!(s.fake.log().contains(&"close:w1:p2".into()));
}

/* ── what follows an outcome ─────────────────────────────────── */

#[tokio::test]
async fn a_landed_delivery_finishes_the_task_and_lets_the_worker_go() {
    let s = setup();
    let mut held = task();
    held.labels = vec!["ui".into(), "needs-you".into()];
    s.set_tasks(vec![held]);
    s.now(
        Kind::Landed,
        "worker/t1 landed on main (abcdef1).",
        |answer| {
            answer.check = Some(Check {
                command: "pnpm test".into(),
                passed: true,
                exit_code: Some(0),
                timed_out: false,
                seconds: 42,
                tail: "ok".into(),
            });
            answer.review = Some(Verdict {
                approve: true,
                blocking: vec![],
                notes: "Clean.".into(),
            });
            answer.outside_owns = vec!["src/i18n.ts".into()];
        },
    );

    s.submit(json!({"summary": "Added the button."}))
        .await
        .unwrap();
    s.fake.settle().await;

    assert_eq!(s.task().column, "done");
    assert_eq!(s.task().labels, vec!["ui"]);
    let log = s.fake.log();
    assert!(log.contains(&"close:w1:p2".into()));
    assert!(log.contains(&"cleanup:C:\\dev\\app-wt\\t1:worker/t1:main".into()));
    let comment = s.line("comment:");
    contains(
        &comment,
        "comment:t1:hangar:Landed on main (abcdef1), fast-forwarded in C:\\dev\\app.",
    );
    contains(&comment, "Summary: Added the button.");
    contains(&comment, "Check: `pnpm test` passed in 42 s.");
    contains(&comment, "Review (sonnet): approved. Notes: Clean.");
    // The briefing's Settings line reads what the check cost from here.
    assert!(crate::orchestration::live::last_check("w1").is_some());
    contains(
        &comment,
        "Changed outside the files it owns, accepted by the review: src/i18n.ts.",
    );
    assert!(s.fake.logged("tell:").is_empty());
    assert_eq!(
        s.events(),
        vec![
            json!({"kind": "delivery", "taskId": "t1", "title": "Add the export button",
                    "worker": "Bo", "outcome": "landed", "detail": "landed on main (abcdef1)"})
        ]
    );
}

#[tokio::test]
async fn a_failure_goes_back_to_the_worker_with_what_to_do_and_the_task_stays_its_own() {
    let s = setup();
    let detail = "Merging worker/t1 onto the current main conflicts in src/a.ts, src/b.ts. Merge main into your branch \
                  (`git merge main`), resolve the conflicts, commit, then call delivery_submit again.";
    s.now(Kind::Conflict, detail, |answer| {
        answer.files = vec!["src/a.ts".into(), "src/b.ts".into()]
    });

    s.submit(json!({})).await.unwrap();
    s.fake.settle().await;

    assert_eq!(s.task().column, "doing");
    assert!(s.fake.logged("close:").is_empty());
    assert!(s.fake.log().contains(&format!(
        "comment:t1:hangar:Delivery failed (conflict, 1): {detail}"
    )));
    let told = s.line("tell:p2:");
    contains(
        &told,
        "Your delivery of \"Add the export button\" did not land.",
    );
    contains(&told, "conflicts in src/a.ts, src/b.ts");
    contains(&told, "Failed deliveries so far: 1.");
    let events = s.events();
    assert_eq!(events.len(), 1);
    matches(
        &events[0],
        json!({"kind": "delivery", "outcome": "conflict",
               "detail": "conflict with main in src/a.ts, src/b.ts (failed delivery 1)"}),
    );
}

/// Hangar sets no limit on a task's failed deliveries (owner's decision,
/// 2026-11-04): the third, which 1.x handed to the person, goes back to the
/// worker like the first, and so does every one after it. The orchestrator
/// hears of each, counted, and decides when to stop.
#[tokio::test]
async fn a_third_failure_and_every_one_after_it_go_back_to_the_worker_not_to_the_person() {
    let s = setup();
    // Two as boards on disk carry them, written while there was a most.
    let earlier = |n: u32| Comment {
        id: format!("c{n}"),
        author: "hangar".into(),
        text: format!("Delivery failed (checkFailed, {n} of 3): x"),
        created_at: 0,
    };
    let mut held = task();
    held.comments = vec![earlier(1), earlier(2)];
    s.set_tasks(vec![held]);
    let detail = "The check `pnpm test` failed (exit 1) on worker/t1 merged onto main.";

    for n in 3..=5 {
        lock(&s.events.0).clear();
        s.now(Kind::CheckFailed, detail, |_| {});
        s.submit(json!({})).await.unwrap();
        s.fake.settle().await;

        assert_eq!(s.task().column, "doing");
        assert_eq!(s.task().labels, vec!["ui"], "failure {n}");
        assert!(s.fake.log().contains(&format!(
            "comment:t1:hangar:Delivery failed (checkFailed, {n}): {detail}"
        )));
        let told = s.fake.logged("tell:p2:").pop().unwrap();
        contains(&told, &format!("Failed deliveries so far: {n}."));
        assert!(!told.contains("stop working on it"), "{told}");
        let events = s.events();
        assert_eq!(events.len(), 1, "failure {n}: {events:?}");
        matches(&events[0], json!({"kind": "delivery", "taskId": "t1"}));
        let said = events[0]["detail"].as_str().unwrap();
        assert!(said.ends_with(&format!("(failed delivery {n})")), "{said}");
    }
}

#[tokio::test]
async fn tells_the_orchestrator_the_fact_it_acts_on_not_the_workers_instructions() {
    let s = setup();
    s.now(
        Kind::CheckFailed,
        "The check `pnpm test` failed (exit 1) on worker/t1 merged onto main. It runs in a fresh checkout.",
        |answer| {
            answer.check = Some(Check {
                command: "pnpm test".into(),
                passed: false,
                exit_code: Some(1),
                timed_out: false,
                seconds: 12,
                tail: " RUN  v5\n FAIL  src/export.test.ts > exports a row per task\nAssertionError: expected 2 to be 3\n Tests  1 failed".into(),
            })
        },
    );
    s.submit(json!({})).await.unwrap();
    s.fake.settle().await;
    matches(
        &s.events()[0],
        json!({"detail": "`pnpm test` exited 1: FAIL  src/export.test.ts > exports a row per task (failed delivery 1)"}),
    );

    lock(&s.events.0).clear();
    s.now(
        Kind::ChangesRequested,
        "The reviewer asked for changes before worker/t1 lands:",
        |answer| {
            answer.review = Some(Verdict {
                approve: false,
                blocking: vec![
                    Blocking {
                        file: "src/export/csv.ts".into(),
                        line: Some(14),
                        problem: "Commas inside a title are not quoted.".into(),
                    },
                    Blocking {
                        file: "src/export/button.tsx".into(),
                        line: None,
                        problem: "Nothing happens on an empty board.".into(),
                    },
                ],
                notes: String::new(),
            })
        },
    );
    s.submit(json!({})).await.unwrap();
    s.fake.settle().await;
    matches(
        &s.events()[0],
        json!({"detail": "review: src/export/csv.ts:14: Commas inside a title are not quoted. (+1 more) (failed delivery 2)"}),
    );
}

#[tokio::test]
async fn an_error_from_the_app_itself_is_a_failed_delivery_too() {
    let s = setup();
    s.next(Next::Now(Box::new(Err("the window went away".into()))));
    s.submit(json!({})).await.unwrap();
    s.fake.settle().await;
    assert_eq!(s.state().as_deref(), Some("error"));
    let told = s.line("tell:p2:");
    contains(
        &told,
        "The delivery could not run: the window went away. Nothing was merged.",
    );
    contains(&told, "If it fails again the same way, stop");
}

#[tokio::test]
async fn one_that_passed_waits_for_the_person_and_lands_when_they_say_so() {
    let s = setup();
    s.settings(|settings| settings.auto_merge = false);
    s.now(
        Kind::Ready,
        "worker/t1 passed its check and its review. Automatic landing is off in this workspace, so it waits.",
        |answer| answer.landed_in = None,
    );

    s.submit(json!({})).await.unwrap();
    s.fake.settle().await;

    assert_eq!(s.task().column, "review");
    assert_eq!(s.task().labels, vec!["ui", "ready-to-land"]);
    contains(&s.line("tell:p2:"), "Automatic landing is off");
    matches(
        &s.events()[0],
        json!({"outcome": "ready", "detail": "ready to land on main"}),
    );
    assert!(s.fake.logged("close:").is_empty());

    s.now(Kind::Landed, "landed", |_| {});
    land_delivery(&s.ws(), "t1", &s.port()).await.unwrap();

    let run = &s.runs()[1];
    assert!(run.land && run.reuse);
    assert!(
        s.fake
            .log()
            .contains(&"preflight:C:\\dev\\app-wt\\t1:null:true".into())
    );
    assert_eq!(s.task().column, "done");
    assert_eq!(s.task().labels, vec!["ui"]);
    assert!(s.fake.log().contains(&"close:w1:p2".into()));
    contains(
        &s.task().comments.last().unwrap().text,
        "Landed as it was checked and reviewed when it was submitted.",
    );
}

#[tokio::test]
async fn the_persons_local_changes_in_the_way_make_it_wait_for_them_not_the_worker() {
    let s = setup();
    s.now(
        Kind::Dirty,
        "worker/t1 passed its review, but main is checked out in C:\\dev\\app, which has local changes.",
        |answer| {
            answer.in_checkout = true;
            answer.files = vec!["src/a.ts".into()];
            answer.landed_in = None;
        },
    );
    s.submit(json!({})).await.unwrap();
    s.fake.settle().await;
    assert_eq!(s.task().column, "review");
    assert_eq!(s.task().labels, vec!["ui", "ready-to-land"]);
    contains(&s.line("comment:"), "local changes in the way (src/a.ts)");
    assert!(
        s.fake
            .logged("comment:t1:hangar:Delivery failed")
            .is_empty()
    );
}

fn waiting_task() -> Task {
    let mut held = task();
    held.column = "review".into();
    held.labels = vec!["ui".into(), "ready-to-land".into()];
    held
}

#[tokio::test]
async fn landing_what_waited_says_why_when_it_no_longer_can_and_the_worker_gets_it_back() {
    let s = setup();
    s.set_tasks(vec![waiting_task()]);
    s.now(
        Kind::Conflict,
        "Merging worker/t1 onto the current main conflicts in src/a.ts.",
        |answer| answer.files = vec!["src/a.ts".into()],
    );
    assert_eq!(
        land_delivery(&s.ws(), "t1", &s.port()).await.unwrap_err(),
        "It no longer lands as it was tested (conflict with main in src/a.ts), so Bo was told what to fix."
    );
    assert_eq!(s.task().column, "doing");
    assert_eq!(s.task().labels, vec!["ui"]);
    contains(&s.line("tell:p2:"), "conflicts in src/a.ts");
}

#[tokio::test]
async fn landing_tells_the_person_when_their_own_changes_are_in_the_way() {
    let s = setup();
    s.set_tasks(vec![waiting_task()]);
    s.now(Kind::Dirty, "passed, but…", |answer| {
        answer.in_checkout = true;
        answer.files = vec!["src/a.ts".into()];
        answer.landed_in = None;
    });
    assert_eq!(
        land_delivery(&s.ws(), "t1", &s.port()).await.unwrap_err(),
        "Local changes in main's checkout are in the way (src/a.ts): commit them or set them aside, then land it \
         again. Nothing was touched."
    );
    assert_eq!(s.task().column, "review");
    assert_eq!(s.task().labels, vec!["ui", "ready-to-land"]);
}

#[tokio::test]
async fn landing_needs_the_worker_that_holds_the_branch() {
    let s = setup();
    s.fake
        .patch("w1", |ws| ws.panes.retain(|pane| pane.id != "p2"));
    contains(
        &land_delivery(&s.ws(), "t1", &s.port()).await.unwrap_err(),
        "No worker holds",
    );
    assert!(s.runs().is_empty());
}

#[tokio::test]
async fn landing_needs_a_delivery_that_still_waits() {
    let s = setup();
    contains(
        &land_delivery(&s.ws(), "t1", &s.port()).await.unwrap_err(),
        "Nothing of \"Add the export button\" waits to land any more",
    );
    assert!(s.runs().is_empty());
    assert_eq!(s.state(), None);
    assert!(!delivering_in(&s.port(), "w1"));
}

#[tokio::test]
async fn a_landing_the_person_asked_for_that_fails_says_so_on_its_row_and_in_a_notice() {
    let s = setup();
    let said = land(s.ws(), "t1".into(), s.port(), Locale::En).await;
    let [
        Message::Intent(Intent::Notice(Notice::Problem(notice))),
        Message::NeedsYou(needs_you::Msg::LandFailed { key, error }),
    ] = said.as_slice()
    else {
        panic!("a notice and the row's failure, not {said:?}");
    };
    assert_eq!(key, "task:C:\\dev\\app:t1:ready-to-land");
    contains(
        error,
        "Nothing of \"Add the export button\" waits to land any more",
    );
    contains(notice, error);
    // The landing held the workspace awake while it ran, and let go: the
    // person's hold and the delivery's own end alike.
    let begun = s.fake.logged("begin:");
    assert!(!begun.is_empty());
    assert_eq!(s.fake.logged("end:").len(), begun.len());
}

#[tokio::test]
async fn a_landing_the_person_asked_for_that_lands_only_says_so() {
    let s = setup();
    s.settings(|settings| settings.auto_merge = false);
    s.now(Kind::Ready, "passed, it waits.", |answer| {
        answer.landed_in = None
    });
    s.submit(json!({})).await.unwrap();
    s.fake.settle().await;
    s.now(Kind::Landed, "landed", |_| {});
    let said = land(s.ws(), "t1".into(), s.port(), Locale::En).await;
    let [Message::Intent(Intent::Notice(Notice::Agent(notice)))] = said.as_slice() else {
        panic!("a notice alone, not {said:?}");
    };
    contains(notice, "Add the export button");
}

/* ── a delivery that waits for the person, submitted again ───── */

fn failed_before() -> Comment {
    Comment {
        id: "c0".into(),
        author: "hangar".into(),
        text: "Delivery failed (conflict, 1 of 3): x".into(),
        created_at: 0,
    }
}

fn waiting() -> Setup {
    let s = setup();
    s.settings(|settings| settings.auto_merge = false);
    let mut held = waiting_task();
    held.comments = vec![failed_before()];
    s.set_tasks(vec![held]);
    s
}

fn tested(current: bool) -> Option<Tested> {
    Some(Tested {
        base: "1234567abcdef".into(),
        merge: "abcdef1234567890".into(),
        current,
    })
}

fn failures(task: &Task) -> Vec<String> {
    task.comments
        .iter()
        .filter(|comment| comment.text.starts_with("Delivery failed"))
        .map(|comment| comment.text.clone())
        .collect()
}

#[tokio::test]
async fn unchanged_still_waits_the_worker_is_told_so_without_an_error_and_nothing_runs() {
    let s = waiting();
    *lock(&s.delivery.tested) = tested(true);

    let answer = s.submit(json!({})).await.unwrap();

    assert_eq!(
        *lock(&s.delivery.preflight_tasks),
        vec![Some("t1".to_string())]
    );
    matches(
        &answer,
        json!({"submitted": false, "waiting": true, "testedAs": "abcdef1", "target": "main"}),
    );
    let next = answer["next"].as_str().unwrap();
    contains(next, "already passed its check and its review, as abcdef1");
    contains(next, "stop here");
    contains(
        next,
        "commit the change on worker/t1, then call delivery_submit again",
    );
    assert!(s.runs().is_empty());
    assert_eq!(s.task().column, "review");
    assert_eq!(s.task().labels, vec!["ui", "ready-to-land"]);
    assert_eq!(failures(&s.task()), vec![failed_before().text]);
    assert_eq!(s.task().comments.len(), 1);
    assert_eq!(
        s.events(),
        vec![
            json!({"kind": "delivery", "taskId": "t1", "title": "Add the export button",
                    "worker": "Bo", "outcome": "ready",
                    "detail": "submitted again without a new commit: still waits to land on main as tested (abcdef1)"})
        ]
    );
    assert_eq!(s.state(), None);
    assert!(!delivering_in(&s.port(), "w1"));
}

#[tokio::test]
async fn with_new_commits_replaces_it_and_takes_the_whole_way_again_only_the_new_one_lands() {
    let s = waiting();
    *lock(&s.delivery.tested) = tested(false);
    s.next(Next::Hold);

    let answer = s
        .submit(json!({"summary": "Fixed the empty board."}))
        .await
        .unwrap();

    matches(&answer, json!({"submitted": true, "replaces": "abcdef1"}));
    until(|| !s.runs().is_empty()).await;
    let run = &s.runs()[0];
    assert!(!run.reuse && !run.land);
    assert_eq!(run.summary.as_deref(), Some("Fixed the empty board."));
    // Withdrawn at once: nothing offers to land the old one meanwhile.
    assert_eq!(s.task().column, "doing");
    assert_eq!(s.task().labels, vec!["ui"]);
    assert!(s.fake.log().contains(
        &"comment:t1:hangar:Submitted again with new commits on worker/t1: the delivery that waited for the person \
          (tested as abcdef1) is withdrawn, and this one is merged, checked and reviewed before it can land."
            .to_string()
    ));
    contains(
        &land_delivery(&s.ws(), "t1", &s.port()).await.unwrap_err(),
        "being tested or landed already",
    );

    let mut passed = outcome(
        Kind::Ready,
        "worker/t1 passed its check and its review. It waits for the person.",
    );
    passed.merge = Some("fedcba9876543210".into());
    passed.landed_in = None;
    s.finish(passed);
    s.fake.settle().await;

    assert_eq!(s.task().column, "review");
    assert_eq!(s.task().labels, vec!["ui", "ready-to-land"]);
    contains(&s.task().comments.last().unwrap().text, "tested as fedcba9");
    let events = s.events();
    assert_eq!(events.len(), 1);
    matches(
        &events[0],
        json!({"kind": "delivery", "outcome": "ready", "detail": "ready to land on main"}),
    );

    s.now(Kind::Landed, "landed", |answer| {
        answer.merge = Some("fedcba9876543210".into())
    });
    land_delivery(&s.ws(), "t1", &s.port()).await.unwrap();
    let run = &s.runs()[1];
    assert!(run.land && run.reuse);
    assert_eq!(s.task().column, "done");
    assert_eq!(s.task().labels, vec!["ui"]);
    // Not one failed delivery more than before.
    assert_eq!(failures(&s.task()), vec![failed_before().text]);
}

#[tokio::test]
async fn with_new_commits_that_fail_counts_that_failure_once_and_nothing_waits_any_more() {
    let s = waiting();
    *lock(&s.delivery.tested) = tested(false);
    s.now(
        Kind::Conflict,
        "Merging worker/t1 onto main conflicts in src/a.ts.",
        |answer| answer.files = vec!["src/a.ts".into()],
    );

    s.submit(json!({})).await.unwrap();
    s.fake.settle().await;

    assert_eq!(s.task().column, "doing");
    assert_eq!(s.task().labels, vec!["ui"]);
    assert!(s.fake.log().contains(
        &"comment:t1:hangar:Delivery failed (conflict, 2): Merging worker/t1 onto main conflicts in src/a.ts."
            .to_string()
    ));
    contains(&s.line("tell:p2:"), "Failed deliveries so far: 2.");
    let events = s.events();
    assert_eq!(events.len(), 1);
    matches(
        &events[0],
        json!({"kind": "delivery", "outcome": "conflict",
               "detail": "conflict with main in src/a.ts (failed delivery 2)"}),
    );
}

#[tokio::test]
async fn is_never_landed_past_the_workers_newer_commits_the_worker_is_asked_to_submit_again() {
    let s = waiting();
    s.now(
        Kind::Stale,
        "worker/t1 has commits that were not part of the delivery waiting to land on main, which was checked and \
         reviewed as abcdef1: it was not landed. Once that work is finished and committed, call delivery_submit again.",
        |answer| answer.landed_in = None,
    );

    contains(
        &land_delivery(&s.ws(), "t1", &s.port()).await.unwrap_err(),
        "Bo committed on worker/t1 after it was checked and reviewed, so it would not land what is there now. \
         Nothing was landed: Bo was asked to submit it again",
    );

    let run = &s.runs()[0];
    assert!(run.land && run.reuse);
    assert_eq!(s.task().column, "doing");
    assert_eq!(s.task().labels, vec!["ui"]);
    assert!(s.fake.log().contains(
        &"comment:t1:hangar:Not landed: worker/t1 has commits that were not in the delivery that waited (tested as \
          abcdef1). It is withdrawn; Bo was asked to submit again."
            .to_string()
    ));
    let told = s.line("tell:p2:");
    contains(&told, "\"Add the export button\" was not landed.");
    contains(&told, "call delivery_submit again");
    assert!(s.fake.logged("close:").is_empty() && s.fake.logged("cleanup:").is_empty());
    assert_eq!(failures(&s.task()), vec![failed_before().text]);
    let events = s.events();
    assert_eq!(events.len(), 1);
    matches(
        &events[0],
        json!({"kind": "delivery", "outcome": "stale",
               "detail": "the person asked to land it on main; it was tested as abcdef1"}),
    );
    assert_eq!(s.state().as_deref(), Some("stale"));

    // Submitted again, it is a delivery like any other.
    *lock(&s.delivery.tested) = None;
    s.now(Kind::Ready, "passed", |answer| answer.landed_in = None);
    matches(
        &s.submit(json!({})).await.unwrap(),
        json!({"submitted": true}),
    );
    s.fake.settle().await;
    assert_eq!(s.task().column, "review");
    assert_eq!(s.task().labels, vec!["ui", "ready-to-land"]);
}

#[tokio::test]
async fn the_person_landing_and_the_worker_submitting_never_run_side_by_side() {
    let s = waiting();
    s.next(Next::Hold);
    let (ws, port) = (s.ws(), s.port());
    let landing = tokio::spawn(async move { land_delivery(&ws, "t1", &port).await });
    until(|| !s.runs().is_empty()).await;

    contains(
        &s.submit(json!({})).await.unwrap_err(),
        "being landed right now",
    );
    s.finish(outcome(Kind::Landed, "landed"));
    landing.await.unwrap().unwrap();
    assert_eq!(s.runs().len(), 1);
}

/* ── delivery_status ─────────────────────────────────────────── */

#[tokio::test]
async fn tells_a_worker_where_its_own_delivery_stands() {
    let s = setup();
    s.next(Next::Hold);
    *lock(&s.delivery.running) = vec![Running {
        task_id: "t1".into(),
        queue: "c:/dev/app/.git".into(),
        stage: "reviewing".into(),
        since: 1_700_000_000_000,
    }];
    s.submit(json!({})).await.unwrap();
    until(|| !s.runs().is_empty()).await;

    let as_pane = |pane: &str| Caller {
        pane_id: Some(pane.into()),
        pane_name: None,
        cwd: Some("C:\\dev\\app".into()),
    };
    let status = control(
        "delivery_status",
        &s.ws(),
        &json!({}),
        &as_pane("p2"),
        &s.port(),
    )
    .await
    .unwrap();
    assert_eq!(
        status["deliveries"],
        json!([{"worker": "Bo", "task": "t1", "state": "reviewing",
                "since": "2023-11-14T22:13:20.000Z"}])
    );

    s.finish(outcome(
        Kind::ChangesRequested,
        "The reviewer asked for changes before worker/t1 lands:",
    ));
    s.fake.settle().await;
    lock(&s.delivery.running).clear();
    let after = control(
        "delivery_status",
        &s.ws(),
        &json!({}),
        &as_pane("p1"),
        &s.port(),
    )
    .await
    .unwrap();
    let rows = after["deliveries"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    matches(
        &rows[0],
        json!({"worker": "Bo", "state": "changesRequested",
               "detail": "The reviewer asked for changes before worker/t1 lands:"}),
    );
}

#[tokio::test]
async fn needs_a_window_that_delivers() {
    let s = setup();
    *lock(&s.fake.delivery) = None;
    assert_eq!(
        s.submit(json!({})).await.unwrap_err(),
        "Deliveries are not available in this Hangar window."
    );
}
