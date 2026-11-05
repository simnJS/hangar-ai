//! workerControl.test.ts, case for case.
//!
//! The TS called the ops through `runControl`, which first resolved the
//! caller's workspace (P18's `workspaceFor`); here they are called with that
//! workspace, read from the world as `workspaceFor` would have found it.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use hangar_core::board::Task;
use hangar_core::git::{
    ChangedFile, Changes, CopyResult, DeleteBranchResult, HookReport, RemoveResult, RepoInfo,
    Worktree,
};
use serde_json::{Value, json};

use super::*;
use crate::orchestration::events::{DeliveryOutcome, JournalEntry, OrchestratorEvent};
use crate::orchestration::testkit::{pane, shell, strings, task};
use crate::state::types::{
    AppState, OrchestrationSettings, Pane, PaneRole, Workspace, WorktreeHooks,
};
use crate::state::{AgentActivity, PaneActivity};

const REPO: &str = "C:\\dev\\shop";
const SIBLINGS: &str = "C:\\dev\\shop-worktrees";

fn sibling(folder: &str) -> String {
    format!("{SIBLINGS}\\{folder}")
}

fn job(id: &str, title: &str, owns: &[&str]) -> Task {
    Task {
        title: title.into(),
        description: format!("Do {title}. Acceptance: it works."),
        owns: strings(owns),
        ..task(id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RepoFail {
    None,
    Detached,
    NoGit,
}

#[derive(Debug, Clone)]
enum HookFail {
    Report(HookReport),
    Throws,
}

#[derive(Debug, Clone, Default)]
struct Fail {
    worktree_add: Option<String>,
    add_pane: bool,
    enqueue: Option<String>,
    repo: Option<RepoFail>,
    hook: Option<HookFail>,
    remove: Option<String>,
    branch_kept: bool,
}

#[derive(Default)]
struct Options {
    tasks: Vec<Task>,
    orchestration: Option<OrchestrationSettings>,
    panes: Option<Vec<Pane>>,
    cwd: Option<String>,
    post_create: Option<String>,
}

/// A Hangar with a workspace on a git repository, its orchestrator Ava and a
/// shell, and a second workspace whose pane already holds the first block of
/// ports. The board's claim follows board.rs's rules closely enough to
/// refuse what it refuses — the overlap itself is read as identical
/// patterns, the real rule being tested in hangar-core.
struct World {
    state: Mutex<AppState>,
    /// The workspaces opened in the main window during this run.
    opened: Mutex<HashSet<String>>,
    log: Mutex<Vec<String>>,
    tasks: Mutex<Vec<Task>>,
    branches: Mutex<Vec<String>>,
    folders: Mutex<HashSet<String>>,
    trees: Mutex<Vec<Worktree>>,
    /// Uncommitted files per worktree, and commits ahead per branch.
    dirty: Mutex<HashMap<String, Vec<ChangedFile>>>,
    ahead: Mutex<HashMap<String, u32>>,
    fail: Mutex<Fail>,
    next: Mutex<u32>,
    briefs: Mutex<HashMap<String, String>>,
    events: Mutex<Vec<(String, OrchestratorEvent)>>,
    journal: Mutex<HashMap<String, Vec<JournalEntry>>>,
    starting: Mutex<Starting>,
    /// Lets a concurrent start run while this one waits on git, as the TS's
    /// awaits did.
    yields: bool,
}

fn orchestrator_pane() -> Pane {
    pane("p1", "Ava", Some(PaneRole::Orchestrator))
}

fn world(options: Options) -> World {
    let shop = Workspace {
        id: "w1".into(),
        name: "Shop".into(),
        cwd: options.cwd.clone().unwrap_or_else(|| REPO.into()),
        panes: options
            .panes
            .unwrap_or_else(|| vec![orchestrator_pane(), shell("p2", "Max")]),
        orchestration: options.orchestration,
        worktree_hooks: options.post_create.map(|command| WorktreeHooks {
            post_create: Some(command),
            pre_remove: None,
        }),
        ..Workspace::default()
    };
    let blog = Workspace {
        id: "w2".into(),
        name: "Blog".into(),
        cwd: "C:\\dev\\blog".into(),
        panes: vec![Pane {
            port_base: Some(PORT_POOL_START),
            ..pane("p9", "Leo", None)
        }],
        ..Workspace::default()
    };
    World {
        state: Mutex::new(AppState {
            workspaces: vec![shop, blog],
            active_workspace_id: Some("w1".into()),
            ..AppState::default()
        }),
        opened: Mutex::new(HashSet::from(["w1".to_string(), "w2".to_string()])),
        log: Mutex::new(Vec::new()),
        tasks: Mutex::new(options.tasks),
        branches: Mutex::new(strings(&["main"])),
        folders: Mutex::new(HashSet::from([REPO.to_string()])),
        trees: Mutex::new(vec![Worktree {
            path: "C:/dev/shop".into(),
            head: Some("abc1234".into()),
            branch: Some("main".into()),
            is_main: true,
            ..Worktree::default()
        }]),
        dirty: Mutex::new(HashMap::new()),
        ahead: Mutex::new(HashMap::new()),
        fail: Mutex::new(Fail::default()),
        next: Mutex::new(10),
        briefs: Mutex::new(HashMap::new()),
        events: Mutex::new(Vec::new()),
        journal: Mutex::new(HashMap::new()),
        starting: Mutex::new(Starting::default()),
        yields: false,
    }
}

fn orchestrator_caller() -> Caller {
    Caller {
        pane_id: Some("p1".into()),
        pane_name: Some("Ava".into()),
        cwd: Some(REPO.into()),
    }
}

impl World {
    fn push(&self, line: impl Into<String>) {
        self.log.lock().unwrap().push(line.into());
    }

    fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }

    fn clear_log(&self) {
        self.log.lock().unwrap().clear();
    }

    fn shop(&self) -> Workspace {
        self.state.lock().unwrap().workspaces[0].clone()
    }

    fn pane_named(&self, name: &str) -> Option<Pane> {
        self.shop().panes.into_iter().find(|pane| pane.name == name)
    }

    fn task(&self, id: &str) -> Task {
        self.tasks
            .lock()
            .unwrap()
            .iter()
            .find(|t| t.id == id)
            .cloned()
            .unwrap()
    }

    fn brief(&self, name: &str) -> String {
        let id = self.pane_named(name).unwrap().id;
        self.briefs.lock().unwrap().get(&id).cloned().unwrap()
    }

    fn events(&self) -> Vec<(String, OrchestratorEvent)> {
        self.events.lock().unwrap().clone()
    }

    fn fail(&self) -> std::sync::MutexGuard<'_, Fail> {
        self.fail.lock().unwrap()
    }

    async fn call(&self, op: &str, args: Value) -> Result<Value, String> {
        self.call_as(op, args, orchestrator_caller()).await
    }

    async fn call_as(&self, op: &str, args: Value, caller: Caller) -> Result<Value, String> {
        let ws = self.shop();
        worker_control(op, &ws, &args, &caller, self).await
    }

    fn map_shop(&self, map: impl FnOnce(&mut Workspace)) {
        map(&mut self.state.lock().unwrap().workspaces[0]);
    }
}

impl WorkerPort for World {
    fn state(&self) -> Arc<AppState> {
        Arc::new(self.state.lock().unwrap().clone())
    }

    /// The live port's own rule, over this world.
    fn is_open(&self, workspace_id: &str) -> bool {
        let snapshot = crate::state::Snapshot {
            state: WorkerPort::state(self),
            opened: self.opened.lock().unwrap().clone(),
            ..crate::state::Snapshot::default()
        };
        crate::orchestration::live::open_in(&snapshot, workspace_id)
    }

    fn activity(&self, _pane: &str) -> Option<PaneActivity> {
        Some(PaneActivity {
            workspace_id: "w1".into(),
            activity: AgentActivity::Working,
            bridged: true,
        })
    }

    fn starting(&self) -> &Mutex<Starting> {
        &self.starting
    }

    fn journal(&self, workspace_id: &str) -> Vec<JournalEntry> {
        self.journal
            .lock()
            .unwrap()
            .get(workspace_id)
            .cloned()
            .unwrap_or_default()
    }

    fn post_event(&self, workspace_id: &str, event: OrchestratorEvent) {
        self.events
            .lock()
            .unwrap()
            .push((workspace_id.into(), event));
    }

    fn announce(&self, a: Announcement) {
        self.push(format!(
            "announce:{}:{}:{}",
            a.kind(),
            a.subject(),
            a.by().unwrap_or("null")
        ));
    }

    async fn add_pane(&self, workspace_id: &str, new: NewPane) -> Option<PaneId> {
        let full = self
            .state
            .lock()
            .unwrap()
            .workspaces
            .iter()
            .find(|ws| ws.id == workspace_id)?
            .panes
            .len()
            >= MAX_PANES;
        if self.fail().add_pane || full {
            return None;
        }
        let id = {
            let mut next = self.next.lock().unwrap();
            let id = format!("p{next}");
            *next += 1;
            id
        };
        self.push(format!("add:{}", serde_json::to_string(&new).unwrap()));
        let mut added = Pane {
            agent: new.agent,
            cwd: Some(new.cwd.clone()),
            ..pane(&id, &new.name, None)
        };
        if let Some(init) = &new.init {
            self.push(format!(
                "update:{id}:{}",
                serde_json::to_string(init).unwrap()
            ));
            added.role = Some(init.role);
            added.task_id = Some(init.task_id.clone());
            added.model = Some(init.model.clone());
            added.port_base = Some(init.port_base);
            added.shared_folder = init.shared_folder;
        }
        self.map_shop(|ws| ws.panes.push(added));
        Some(id)
    }

    async fn close_pane(&self, _workspace_id: &str, pane: &str) {
        self.push(format!("close:{pane}"));
        self.map_shop(|ws| ws.panes.retain(|entry| entry.id != pane));
    }

    fn touch(&self, _workspace_id: &str) {}

    fn open_workspace(&self, workspace_id: &str) {
        self.push(format!("open:{workspace_id}"));
    }

    async fn kill_pane(&self, pane: &str) -> Result<(), String> {
        self.push(format!("kill:{pane}"));
        Ok(())
    }

    async fn alive(&self, _pane: &str) -> bool {
        true
    }

    async fn enqueue(&self, pane: &str, text: &str, from: &str) -> Result<(), String> {
        if let Some(error) = self.fail().enqueue.clone() {
            return Err(error);
        }
        self.push(format!("enqueue:{pane}:{from}"));
        self.briefs.lock().unwrap().insert(pane.into(), text.into());
        Ok(())
    }

    async fn dir_exists(&self, path: &str) -> bool {
        self.folders.lock().unwrap().contains(path)
    }

    async fn sleep(&self, _ms: u64) {}

    async fn board_load(&self, _cwd: &str) -> Result<Vec<Task>, String> {
        Ok(self.tasks.lock().unwrap().clone())
    }

    async fn claim(
        &self,
        _cwd: &str,
        id: &str,
        worker: &str,
        shared: bool,
    ) -> Result<Task, String> {
        self.push(format!(
            "claim:{id}:{worker}{}",
            if shared { ":shared" } else { "" }
        ));
        let mut tasks = self.tasks.lock().unwrap();
        let Some(entry) = tasks.iter().find(|t| t.id == id).cloned() else {
            return Err(format!("No task {id} on this board."));
        };
        if entry.column != "todo" {
            return Err(format!(
                "\"{}\" is in {} already.",
                entry.title, entry.column
            ));
        }
        let waiting: Vec<&str> = entry
            .depends_on
            .iter()
            .filter(|dep| {
                tasks
                    .iter()
                    .find(|t| &t.id == *dep)
                    .map(|t| t.column.as_str())
                    != Some("done")
            })
            .map(String::as_str)
            .collect();
        if !waiting.is_empty() {
            return Err(format!(
                "\"{}\" depends on tasks that are not done: {}.",
                entry.title,
                waiting.join(", ")
            ));
        }
        let clash = tasks.iter().find(|other| {
            other.id != id
                && other.column == "doing"
                && other.owns.iter().any(|path| entry.owns.contains(path))
        });
        if let Some(clash) = clash {
            return Err(format!(
                "Cannot start \"{}\": it owns files that \"{}\" owns too.",
                entry.title, clash.title
            ));
        }
        let held = tasks.iter_mut().find(|t| t.id == id).unwrap();
        held.column = "doing".into();
        held.assignee = Some(worker.into());
        held.attempts += 1;
        Ok(held.clone())
    }

    async fn release(
        &self,
        _cwd: &str,
        id: &str,
        worker: &str,
        undo: bool,
    ) -> Result<Task, String> {
        self.push(format!("release:{id}:{worker}:{undo}"));
        let mut tasks = self.tasks.lock().unwrap();
        let entry = tasks.iter_mut().find(|t| t.id == id).unwrap();
        if entry.assignee.as_deref() != Some(worker) {
            return Ok(entry.clone());
        }
        if undo {
            entry.attempts = entry.attempts.saturating_sub(1);
        }
        if entry.column == "todo" || entry.column == "doing" {
            entry.column = "todo".into();
            entry.assignee = None;
        }
        Ok(entry.clone())
    }

    async fn repo_info(&self, _path: &str) -> RepoInfo {
        if self.yields {
            tokio::task::yield_now().await;
        }
        let fail = self.fail().repo;
        let none = matches!(fail, Some(RepoFail::None | RepoFail::NoGit));
        RepoInfo {
            installed: fail != Some(RepoFail::NoGit),
            is_repo: !none,
            root: (!none).then(|| REPO.into()),
            main_root: (!none).then(|| REPO.into()),
            is_linked_worktree: false,
            branch: (fail != Some(RepoFail::Detached)).then(|| "main".into()),
            head: Some("abc1234".into()),
        }
    }

    async fn worktree_list(&self, _path: &str) -> Result<Vec<Worktree>, String> {
        Ok(self.trees.lock().unwrap().clone())
    }

    async fn worktree_add(
        &self,
        _path: &str,
        worktree_path: &str,
        branch: &str,
        create_branch: bool,
        base_ref: Option<&str>,
    ) -> Result<(), String> {
        self.push(format!(
            "worktree:{worktree_path}:{branch}:{}:{create_branch}",
            base_ref.unwrap_or("null")
        ));
        if let Some(error) = self.fail().worktree_add.clone() {
            return Err(error);
        }
        if self
            .branches
            .lock()
            .unwrap()
            .iter()
            .any(|known| known == branch)
        {
            return Err(format!("a branch named '{branch}' already exists"));
        }
        self.branches.lock().unwrap().push(branch.into());
        self.folders.lock().unwrap().insert(worktree_path.into());
        let mut trees = self.trees.lock().unwrap();
        let tree = Worktree {
            path: worktree_path.replace('\\', "/"),
            branch: Some(branch.into()),
            is_main: false,
            ..trees[0].clone()
        };
        trees.push(tree);
        Ok(())
    }

    async fn worktree_remove(
        &self,
        _path: &str,
        worktree_path: &str,
        force: bool,
    ) -> Result<RemoveResult, String> {
        self.push(format!("unworktree:{worktree_path}:{force}"));
        if let Some(error) = self.fail().remove.clone() {
            return Err(error);
        }
        self.folders.lock().unwrap().remove(worktree_path);
        let slashed = worktree_path.replace('\\', "/");
        self.trees
            .lock()
            .unwrap()
            .retain(|tree| tree.path != slashed);
        Ok(RemoveResult {
            removed: true,
            needs_force: false,
            message: None,
            hook_blocked: false,
            hook: None,
        })
    }

    async fn worktree_hook(
        &self,
        _path: &str,
        worktree_path: &str,
        command: &str,
    ) -> Result<Option<HookReport>, String> {
        self.push(format!("hook:{worktree_path}:{command}"));
        match self.fail().hook.clone() {
            Some(HookFail::Throws) => Err("the shell is gone".into()),
            Some(HookFail::Report(report)) => Ok(Some(report)),
            None => Ok(Some(HookReport {
                ok: true,
                timed_out: false,
                exit_code: Some(0),
                output: String::new(),
                error: None,
            })),
        }
    }

    async fn branch_delete(
        &self,
        _path: &str,
        branch: &str,
        force: bool,
    ) -> Result<DeleteBranchResult, String> {
        self.push(format!("unbranch:{branch}:{force}"));
        if self.fail().branch_kept {
            return Ok(DeleteBranchResult {
                deleted: false,
                needs_force: false,
                message: Some("checked out".into()),
            });
        }
        self.branches
            .lock()
            .unwrap()
            .retain(|known| known != branch);
        Ok(DeleteBranchResult {
            deleted: true,
            needs_force: false,
            message: None,
        })
    }

    async fn copy_ignored(
        &self,
        from: &str,
        to: &str,
        _patterns: &[&str],
    ) -> Result<CopyResult, String> {
        self.push(format!("copy:{from}:{to}"));
        Ok(CopyResult {
            copied: 1,
            failed: 0,
        })
    }

    async fn changes(&self, path: &str) -> Result<Changes, String> {
        Ok(Changes {
            root: path.into(),
            base: Some("abc1234".into()),
            files: self
                .dirty
                .lock()
                .unwrap()
                .get(path)
                .cloned()
                .unwrap_or_default(),
            truncated: false,
        })
    }

    async fn commits_ahead(&self, _path: &str, base: &str, head: &str) -> Result<u32, String> {
        self.push(format!("ahead:{base}..{head}"));
        Ok(self.ahead.lock().unwrap().get(head).copied().unwrap_or(0))
    }
}

fn tasks_one_two() -> Vec<Task> {
    vec![job("t1", "One", &["a/**"]), job("t2", "Two", &["b/**"])]
}

fn err(result: Result<Value, String>) -> String {
    result.expect_err("refused")
}

/* ── worker_spawn ──────────────────────────────────────────────── */

#[tokio::test]
async fn claims_the_task_checks_out_a_worktree_from_the_target_branch_opens_the_worker_and_queues_its_brief()
 {
    let app = world(Options {
        tasks: vec![job("login-1", "Login form", &["src/login/**"])],
        orchestration: Some(OrchestrationSettings {
            check_command: Some("pnpm test".into()),
            ..Default::default()
        }),
        ..Options::default()
    });

    let spawned = app
        .call("worker_spawn", json!({"task": "login-1"}))
        .await
        .unwrap();

    let branch = "hangar/login-form-login1";
    let path = sibling("hangar-login-form-login1");
    assert_eq!(spawned["worker"], "Mia");
    assert_eq!(
        spawned["task"],
        json!({"id": "login-1", "title": "Login form", "attempt": 1})
    );
    assert_eq!(spawned["model"], "sonnet");
    assert_eq!(spawned["branch"], branch);
    assert_eq!(spawned["base"], "main");
    assert_eq!(spawned["worktree"], path.as_str());
    assert_eq!(
        spawned["ports"],
        json!({"base": PORT_POOL_START + 20, "count": 20})
    );
    let worker = app.pane_named("Mia").unwrap();
    assert_eq!(worker.agent, AgentId::Claude);
    assert_eq!(worker.cwd.as_deref(), Some(path.as_str()));
    assert_eq!(worker.role, Some(PaneRole::Worker));
    assert_eq!(worker.task_id.as_deref(), Some("login-1"));
    assert_eq!(worker.model.as_deref(), Some("sonnet"));
    assert_eq!(worker.port_base, Some(PORT_POOL_START + 20));
    let held = app.task("login-1");
    assert_eq!(
        (
            held.column.as_str(),
            held.assignee.as_deref(),
            held.attempts
        ),
        ("doing", Some("Mia"), 1)
    );
    // In that order: nothing is created before the claim holds.
    let add = json!({"agent": "claude", "cwd": path, "name": "Mia", "near": "p1"}).to_string();
    // Spelled out: JSON.stringify keeps the keys in the order the TS wrote them.
    let update = format!(
        r#"{{"role":"worker","taskId":"login-1","model":"sonnet","portBase":{}}}"#,
        PORT_POOL_START + 20
    );
    assert_eq!(
        app.log()[..7],
        [
            "claim:login-1:Mia".to_string(),
            format!("worktree:{path}:{branch}:main:true"),
            format!("copy:{REPO}:{path}"),
            format!("add:{add}"),
            format!("update:{}:{update}", worker.id),
            "open:w1".to_string(),
            format!("enqueue:{}:Ava", worker.id),
        ]
    );
    assert!(app.log().contains(&"announce:created:Mia:Ava".to_string()));
    assert_eq!(
        app.events(),
        [(
            "w1".to_string(),
            OrchestratorEvent::WorkerStarted {
                worker: "Mia".into(),
                task_id: Some("login-1".into())
            }
        )]
    );

    let brief = app.brief("Mia");
    for needed in [
        "## Task login-1: Login form".to_string(),
        "Do Login form. Acceptance: it works.".to_string(),
        "`src/login/**`. Change nothing outside them.".to_string(),
        format!("({} to {};", PORT_POOL_START + 20, PORT_POOL_START + 39),
        "`pnpm test` passes in your worktree".to_string(),
        format!("on branch {branch}, made from main"),
        "delivery_submit".to_string(),
        "Never mock, stub or guess".to_string(),
    ] {
        assert!(brief.contains(&needed), "{needed}\n---\n{brief}");
    }
}

#[tokio::test]
async fn runs_the_workspaces_post_create_command_in_the_new_worktree_after_the_copy_and_never_otherwise()
 {
    let app = world(Options {
        tasks: tasks_one_two(),
        post_create: Some("  pnpm install  ".into()),
        ..Options::default()
    });
    app.call("worker_spawn", json!({"task": "t1"}))
        .await
        .unwrap();
    let path = sibling("hangar-one-t1");
    let lines: Vec<String> = app
        .log()
        .into_iter()
        .filter(|line| {
            line.starts_with("copy:") || line.starts_with("hook:") || line.starts_with("add:")
        })
        .collect();
    assert_eq!(
        lines[..2],
        [
            format!("copy:{REPO}:{path}"),
            format!("hook:{path}:pnpm install")
        ]
    );
    assert!(!app.brief("Mia").contains("postCreate"));

    let plain = world(Options {
        tasks: vec![job("t1", "One", &["a/**"])],
        post_create: Some("  ".into()),
        ..Options::default()
    });
    plain
        .call("worker_spawn", json!({"task": "t1"}))
        .await
        .unwrap();
    assert!(!plain.log().iter().any(|line| line.starts_with("hook:")));
}

#[tokio::test]
async fn starts_the_worker_anyway_when_its_post_create_command_fails_and_tells_it_in_the_brief() {
    let app = world(Options {
        tasks: vec![job("t1", "One", &["a/**"])],
        post_create: Some("pnpm install".into()),
        ..Options::default()
    });
    app.fail().hook = Some(HookFail::Report(HookReport {
        ok: false,
        timed_out: false,
        exit_code: Some(3),
        output: "ERR_PNPM_NO_LOCKFILE".into(),
        error: None,
    }));

    let spawned = app
        .call("worker_spawn", json!({"task": "t1"}))
        .await
        .unwrap();

    assert_eq!(spawned["worker"], "Mia");
    assert!(
        spawned["note"]
            .as_str()
            .unwrap()
            .contains("postCreate command (pnpm install) failed (exit 3): ERR_PNPM_NO_LOCKFILE")
    );
    assert!(app
        .brief("Mia")
        .contains("postCreate command (`pnpm install`) ran in your worktree and failed (exit 3): ERR_PNPM_NO_LOCKFILE"));
    assert!(!app.log().iter().any(|line| line.starts_with("unworktree:")));
    let held = app.task("t1");
    assert_eq!(
        (held.column.as_str(), held.assignee.as_deref()),
        ("doing", Some("Mia"))
    );

    // A command that cannot even be asked for is the same report.
    let broken = world(Options {
        tasks: vec![job("t1", "One", &["a/**"])],
        post_create: Some("pnpm install".into()),
        ..Options::default()
    });
    broken.fail().hook = Some(HookFail::Throws);
    broken
        .call("worker_spawn", json!({"task": "t1"}))
        .await
        .unwrap();
    assert!(
        broken
            .brief("Mia")
            .contains("could not run: the shell is gone")
    );
}

#[tokio::test]
async fn starts_the_worker_with_the_model_asked_for_and_refuses_one_that_is_not_a_model_name() {
    let app = world(Options {
        tasks: tasks_one_two(),
        orchestration: Some(OrchestrationSettings {
            worker_model: "opus".into(),
            ..Default::default()
        }),
        ..Options::default()
    });

    assert_eq!(
        app.call("worker_spawn", json!({"task": "t1"}))
            .await
            .unwrap()["model"],
        "opus"
    );
    let quick = app
        .call(
            "worker_spawn",
            json!({"task": "t2", "model": "haiku", "name": "Quick"}),
        )
        .await
        .unwrap();
    assert_eq!(quick["model"], "haiku");
    assert_eq!(
        app.pane_named("Quick").unwrap().model.as_deref(),
        Some("haiku")
    );

    let refused = err(app
        .call(
            "worker_spawn",
            json!({"task": "t2", "model": "haiku; rm -rf /"}),
        )
        .await);
    assert!(refused.contains("not a model name"), "{refused}");
}

#[tokio::test]
async fn gives_each_worker_a_block_of_ports_no_pane_of_any_workspace_has() {
    let app = world(Options {
        tasks: tasks_one_two(),
        ..Options::default()
    });
    let first = app
        .call("worker_spawn", json!({"task": "t1"}))
        .await
        .unwrap();
    let second = app
        .call("worker_spawn", json!({"task": "t2"}))
        .await
        .unwrap();

    // 21000 is the Blog workspace's.
    assert_eq!(
        [
            first["ports"]["base"].clone(),
            second["ports"]["base"].clone()
        ],
        [json!(PORT_POOL_START + 20), json!(PORT_POOL_START + 40)]
    );
    assert_eq!(
        allocate_port_base(&app.state().workspaces, []),
        Some(PORT_POOL_START + 60)
    );
    // A block another pane overlaps is skipped too, aligned or not.
    let odd = Workspace {
        panes: vec![Pane {
            port_base: Some(PORT_POOL_START + 5),
            ..shell("a", "a")
        }],
        ..Workspace::default()
    };
    assert_eq!(allocate_port_base(&[odd], []), Some(PORT_POOL_START + 40));
}

/// Hangar sets no limit on how many workers run at once: the orchestrator
/// decides. A fourth, past the 3 of 1.x's default, starts like the first.
#[tokio::test]
async fn starts_every_worker_it_is_asked_for_a_fourth_and_more_included() {
    let app = world(Options {
        tasks: (1..=5)
            .map(|n| Task {
                owns: vec![format!("part{n}/**")],
                ..job(&format!("t{n}"), &format!("Part {n}"), &[])
            })
            .collect(),
        ..Options::default()
    });
    for n in 1..=5 {
        let spawned = app
            .call("worker_spawn", json!({"task": format!("t{n}")}))
            .await
            .unwrap();
        assert_eq!(spawned["task"]["id"], format!("t{n}"));
    }

    assert_eq!(workers_of(&app.shop()).len(), 5);
    for n in 1..=5 {
        assert_eq!(app.task(&format!("t{n}")).column, "doing");
    }
    assert_eq!(app.events().len(), 5);
}

#[tokio::test]
async fn counts_a_start_still_under_way_against_the_room_left_and_never_hands_two_the_same_name_or_ports()
 {
    // One pane short of full: two starts at once cannot both take it.
    let mut crowded = vec![orchestrator_pane()];
    crowded.extend((2..MAX_PANES).map(|at| shell(&format!("s{at}"), &format!("S{at}"))));
    let limited = World {
        yields: true,
        ..world(Options {
            tasks: tasks_one_two(),
            panes: Some(crowded),
            ..Options::default()
        })
    };
    let (one, two) = tokio::join!(
        limited.call("worker_spawn", json!({"task": "t1"})),
        limited.call("worker_spawn", json!({"task": "t2"}))
    );
    assert!(one.is_ok());
    assert!(two.unwrap_err().contains("is full"));
    assert_eq!(limited.task("t2").column, "todo");

    let roomy = World {
        yields: true,
        ..world(Options {
            tasks: tasks_one_two(),
            ..Options::default()
        })
    };
    let (one, two) = tokio::join!(
        roomy.call("worker_spawn", json!({"task": "t1"})),
        roomy.call("worker_spawn", json!({"task": "t2"}))
    );
    let (one, two) = (one.unwrap(), two.unwrap());
    assert_ne!(one["worker"], two["worker"]);
    assert_ne!(one["ports"]["base"], two["ports"]["base"]);
}

#[tokio::test]
async fn passes_on_every_refusal_of_the_claim_and_leaves_nothing_behind() {
    let app = world(Options {
        tasks: vec![
            Task {
                column: "doing".into(),
                assignee: Some("Leo".into()),
                ..job("busy", "Busy", &["src/shared/**"])
            },
            Task {
                depends_on: strings(&["busy"]),
                ..job("later", "Later", &["c/**"])
            },
            job("clash", "Clash", &["src/shared/**"]),
        ],
        ..Options::default()
    });
    let cases = [
        ("nope", "No task nope"),
        ("busy", "in doing already"),
        ("later", "depends on tasks that are not done: busy"),
        ("clash", "owns files that \"Busy\" owns too"),
    ];
    for (id, refusal) in cases {
        let message = err(app.call("worker_spawn", json!({"task": id})).await);
        assert!(message.contains(refusal), "{id}: {message}");
    }
    assert!(app.log().iter().all(|line| line.starts_with("claim:")));
    assert_eq!(app.shop().panes.len(), 2);
    assert!(app.events().is_empty());
}

#[tokio::test]
async fn starts_workers_on_tasks_with_disjoint_files_and_refuses_the_third_that_overlaps_one_of_them()
 {
    let app = world(Options {
        tasks: vec![
            job("api", "API", &["src-tauri/src/board.rs"]),
            job("ui", "UI", &["src/components/BoardView.tsx"]),
            job(
                "both",
                "Both",
                &["src/components/BoardView.tsx", "docs/board.md"],
            ),
        ],
        ..Options::default()
    });
    app.call("worker_spawn", json!({"task": "api"}))
        .await
        .unwrap();
    app.call("worker_spawn", json!({"task": "ui"}))
        .await
        .unwrap();

    assert!(
        err(app.call("worker_spawn", json!({"task": "both"})).await).contains("\"UI\" owns too")
    );
    let both = app.task("both");
    assert_eq!(
        (
            both.column.as_str(),
            both.assignee.as_deref(),
            both.attempts
        ),
        ("todo", None, 0)
    );
}

/// Hangar sets no limit on the workers one task gets: the orchestrator
/// decides when to stop. A task 1.x's default would have handed back after
/// two starts as many as it is asked for, each told which attempt it is.
#[tokio::test]
async fn starts_a_task_again_however_many_workers_it_had_and_tells_it_which_attempt_it_is() {
    let app = world(Options {
        tasks: vec![Task {
            attempts: 4,
            ..job("t1", "Again", &["a/**"])
        }],
        ..Options::default()
    });
    let spawned = app
        .call("worker_spawn", json!({"task": "t1"}))
        .await
        .unwrap();

    assert_eq!(spawned["task"]["attempt"], 5);
    assert!(spawned["task"].get("maxAttempts").is_none());
    assert_eq!(app.task("t1").attempts, 5);
    let brief = app.brief(spawned["worker"].as_str().unwrap());
    assert!(
        brief.contains("This is attempt 5: an earlier worker did not finish it."),
        "{brief}"
    );
    assert!(!brief.contains("attempt 5 of"), "{brief}");
}

#[tokio::test]
async fn refuses_a_workspace_with_no_room_or_a_repository_with_no_branch_to_start_from() {
    let crowded: Vec<Pane> = (0..MAX_PANES)
        .map(|at| {
            if at == 0 {
                pane("c0", "C0", Some(PaneRole::Orchestrator))
            } else {
                shell(&format!("c{at}"), &format!("C{at}"))
            }
        })
        .collect();
    let full = world(Options {
        tasks: vec![task("t1")],
        panes: Some(crowded),
        ..Options::default()
    });
    let caller = Caller {
        pane_id: Some("c0".into()),
        ..Caller::default()
    };
    assert!(
        err(full
            .call_as("worker_spawn", json!({"task": "t1"}), caller)
            .await)
        .contains("is full")
    );

    let detached = world(Options {
        tasks: vec![task("t1")],
        ..Options::default()
    });
    detached.fail().repo = Some(RepoFail::Detached);
    assert!(
        err(detached.call("worker_spawn", json!({"task": "t1"})).await).contains("detached HEAD")
    );
    assert!(detached.log().is_empty());
}

#[tokio::test]
async fn undoes_the_claim_when_the_worktree_cannot_be_made() {
    let app = world(Options {
        tasks: vec![job("t1", "One", &["a/**"])],
        ..Options::default()
    });
    app.fail().worktree_add = Some("fatal: invalid reference: develop".into());

    let message = err(app.call("worker_spawn", json!({"task": "t1"})).await);
    let pattern =
        regex::Regex::new(r"Could not create the worker's worktree.*invalid reference: develop.*Nothing was left behind")
            .unwrap();
    assert!(pattern.is_match(&message), "{message}");
    let back = app.task("t1");
    assert_eq!(
        (
            back.column.as_str(),
            back.assignee.as_deref(),
            back.attempts
        ),
        ("todo", None, 0)
    );
    assert_eq!(app.shop().panes.len(), 2);
    assert!(app.log().contains(&"release:t1:Mia:true".to_string()));
}

#[tokio::test]
async fn removes_the_worktree_and_its_branch_when_the_pane_cannot_be_opened() {
    let app = world(Options {
        tasks: vec![job("t1", "One", &["a/**"])],
        ..Options::default()
    });
    app.fail().add_pane = true;

    let message = err(app.call("worker_spawn", json!({"task": "t1"})).await);
    assert!(
        regex::Regex::new("is full.*Nothing was left behind")
            .unwrap()
            .is_match(&message),
        "{message}"
    );
    let log = app.log();
    assert_eq!(
        log[log.len() - 3..],
        [
            format!("unworktree:{}:true", sibling("hangar-one-t1")),
            "unbranch:hangar/one-t1:true".to_string(),
            "release:t1:Mia:true".to_string(),
        ]
    );
    assert_eq!(*app.branches.lock().unwrap(), ["main"]);
    let back = app.task("t1");
    assert_eq!((back.column.as_str(), back.attempts), ("todo", 0));
}

#[tokio::test]
async fn closes_the_pane_removes_the_worktree_and_gives_the_task_back_when_the_brief_cannot_be_queued()
 {
    let app = world(Options {
        tasks: vec![job("t1", "One", &["a/**"])],
        ..Options::default()
    });
    app.fail().enqueue = Some("the bridge is gone".into());

    assert!(
        err(app.call("worker_spawn", json!({"task": "t1"})).await).contains("the bridge is gone")
    );
    assert!(app.pane_named("Mia").is_none());
    let undone: Vec<String> = app
        .log()
        .into_iter()
        .filter(|line| {
            ["kill:", "close:", "unworktree:", "unbranch:", "release:"]
                .iter()
                .any(|p| line.starts_with(p))
        })
        .collect();
    assert_eq!(
        undone,
        [
            "kill:p10".to_string(),
            "close:p10".to_string(),
            format!("unworktree:{}:true", sibling("hangar-one-t1")),
            "unbranch:hangar/one-t1:true".to_string(),
            "release:t1:Mia:true".to_string(),
        ]
    );
    let back = app.task("t1");
    assert_eq!(
        (
            back.column.as_str(),
            back.assignee.as_deref(),
            back.attempts
        ),
        ("todo", None, 0)
    );
    assert!(app.events().is_empty());

    // Whatever it held — the name, the place, the ports — is free again.
    app.fail().enqueue = None;
    let retried = app
        .call("worker_spawn", json!({"task": "t1"}))
        .await
        .unwrap();
    assert_eq!(retried["worker"], "Mia");
    assert_eq!(retried["ports"]["base"], PORT_POOL_START + 20);
}

#[tokio::test]
async fn takes_the_next_branch_name_when_an_earlier_attempts_branch_is_still_there() {
    let app = world(Options {
        tasks: vec![job("t1", "One", &["a/**"])],
        ..Options::default()
    });
    app.branches.lock().unwrap().push("hangar/one-t1".into());

    let spawned = app
        .call("worker_spawn", json!({"task": "t1"}))
        .await
        .unwrap();
    assert_eq!(spawned["branch"], "hangar/one-t1-2");
    assert_eq!(spawned["worktree"], sibling("hangar-one-t1-2").as_str());
}

#[tokio::test]
async fn starts_the_worker_in_the_same_subfolder_of_its_worktree_as_the_workspace_is_of_the_repository()
 {
    let app = world(Options {
        tasks: vec![job("t1", "One", &["a/**"])],
        cwd: Some(format!("{REPO}\\packages\\App")),
        ..Options::default()
    });
    let spawned = app
        .call("worker_spawn", json!({"task": "t1"}))
        .await
        .unwrap();

    assert_eq!(spawned["worktree"], sibling("hangar-one-t1").as_str());
    assert_eq!(
        app.pane_named("Mia").unwrap().cwd.as_deref(),
        Some(format!("{}\\packages\\App", sibling("hangar-one-t1")).as_str())
    );
    let listed = app.call("worker_list", json!({})).await.unwrap();
    assert_eq!(
        listed["workers"][0]["worktree"],
        "C:/dev/shop-worktrees/hangar-one-t1"
    );

    let stopped = app
        .call("worker_stop", json!({"worker": "Mia"}))
        .await
        .unwrap();
    assert_eq!(
        stopped["worktree"]["path"],
        "C:/dev/shop-worktrees/hangar-one-t1"
    );
    assert_eq!(stopped["worktree"]["removed"], true);
}

#[tokio::test]
async fn does_not_hand_a_worker_the_name_of_one_its_orchestrator_still_hears_about() {
    let app = world(Options {
        tasks: vec![job("t1", "One", &["a/**"])],
        ..Options::default()
    });
    let landed = OrchestratorEvent::Delivery {
        task_id: "t0".into(),
        title: "Zero".into(),
        worker: "Mia".into(),
        outcome: DeliveryOutcome::Landed,
        detail: String::new(),
    };
    app.journal.lock().unwrap().insert(
        "w1".into(),
        vec![JournalEntry {
            at: 0,
            event: landed,
            quiet: false,
        }],
    );
    assert_ne!(
        app.call("worker_spawn", json!({"task": "t1"}))
            .await
            .unwrap()["worker"],
        "Mia"
    );
}

#[tokio::test]
async fn refuses_a_name_another_pane_has_anywhere_in_hangar() {
    let app = world(Options {
        tasks: vec![job("t1", "One", &["a/**"])],
        ..Options::default()
    });
    let message = err(app
        .call("worker_spawn", json!({"task": "t1", "name": "leo"}))
        .await);
    assert!(
        message.contains("already named \"leo\" (in \"Blog\")"),
        "{message}"
    );
    assert!(app.log().is_empty());
}

#[tokio::test]
async fn refuses_anyone_but_the_workspaces_orchestrator() {
    let app = world(Options {
        tasks: vec![job("t1", "One", &["a/**"])],
        ..Options::default()
    });
    let max = Caller {
        pane_id: Some("p2".into()),
        pane_name: Some("Max".into()),
        cwd: Some(REPO.into()),
    };
    assert!(
        err(app
            .call_as("worker_spawn", json!({"task": "t1"}), max)
            .await)
        .contains("orchestrator of \"Shop\", which is \"Ava\", not you")
    );
    let nobody = Caller {
        pane_id: None,
        pane_name: None,
        cwd: Some(REPO.into()),
    };
    assert!(
        err(app.call_as("worker_list", json!({}), nobody).await)
            .contains("which is \"Ava\", not you")
    );

    let spawned = app
        .call("worker_spawn", json!({"task": "t1"}))
        .await
        .unwrap();
    let worker = app.pane_named(spawned["worker"].as_str().unwrap()).unwrap();
    let mia = Caller {
        pane_id: Some(worker.id.clone()),
        pane_name: Some("Mia".into()),
        cwd: worker.cwd.clone(),
    };
    assert!(
        err(app
            .call_as("worker_stop", json!({"worker": "Mia"}), mia)
            .await)
        .contains("you are one of its workers")
    );

    let unpinned = world(Options {
        tasks: vec![task("t1")],
        panes: Some(vec![pane("p1", "Ava", None), shell("p2", "Max")]),
        ..Options::default()
    });
    assert!(
        err(unpinned.call("worker_spawn", json!({"task": "t1"})).await)
            .contains("\"Shop\" has none")
    );
    assert!(unpinned.log().is_empty());
}

#[tokio::test]
async fn refuses_to_start_a_worker_that_could_never_receive_its_brief() {
    let app = world(Options {
        tasks: vec![task("t1")],
        ..Options::default()
    });
    app.state.lock().unwrap().settings.claude_bridge = false;
    assert!(
        err(app.call("worker_spawn", json!({"task": "t1"})).await).contains("bridge is turned off")
    );
}

/* ── a workspace that is not a git repository ──────────────────── */

#[tokio::test]
async fn starts_the_worker_in_its_folder_no_worktree_no_branch_and_a_brief_that_says_so() {
    let app = world(Options {
        tasks: vec![job("t1", "Arena timer", &["Content/Arena/**"])],
        ..Options::default()
    });
    app.fail().repo = Some(RepoFail::None);

    let spawned = app
        .call("worker_spawn", json!({"task": "t1"}))
        .await
        .unwrap();

    assert_eq!(spawned["worker"], "Mia");
    assert_eq!(spawned["folder"], REPO);
    assert_eq!(spawned["branch"], Value::Null);
    assert_eq!(spawned["base"], Value::Null);
    assert_eq!(spawned["worktree"], Value::Null);
    let note = spawned["note"].as_str().unwrap();
    assert!(
        regex::Regex::new(r#""Shop" is not a git repository.*its task is done when it delivers"#)
            .unwrap()
            .is_match(note)
    );
    let mia = app.pane_named("Mia").unwrap();
    assert_eq!(mia.cwd.as_deref(), Some(REPO));
    assert_eq!(mia.role, Some(PaneRole::Worker));
    assert_eq!(mia.task_id.as_deref(), Some("t1"));
    assert_eq!(mia.shared_folder, Some(true));
    let made: Vec<String> = app
        .log()
        .into_iter()
        .filter(|line| {
            ["claim:", "worktree:", "copy:"]
                .iter()
                .any(|p| line.starts_with(p))
        })
        .collect();
    assert_eq!(made, ["claim:t1:Mia:shared"]);
    let brief = app.brief("Mia");
    assert!(brief.contains(&format!(
        "You work in the workspace's own folder: {REPO}. It is not a git repository"
    )));
    assert!(brief.contains("Files you own: `Content/Arena/**`"));
    assert!(brief.contains("Nothing is merged or reviewed here"));
    assert!(!brief.contains("worktree") && !brief.contains("Commit everything"));

    let listed = app.call("worker_list", json!({})).await.unwrap();
    let first = &listed["workers"][0];
    assert_eq!(
        (
            first["name"].clone(),
            first["folder"].clone(),
            first["branch"].clone(),
            first["worktree"].clone()
        ),
        (json!("Mia"), json!(REPO), Value::Null, Value::Null)
    );

    let stopped = app
        .call("worker_stop", json!({"worker": "Mia"}))
        .await
        .unwrap();
    assert_eq!(stopped["worktree"]["path"], REPO);
    assert_eq!(stopped["worktree"]["removed"], false);
    assert_eq!(stopped["worktree"]["branchDeleted"], false);
    let kept = stopped["worktree"]["kept"].as_str().unwrap();
    assert!(
        regex::Regex::new("workspace's own folder.*stays in the folder")
            .unwrap()
            .is_match(kept)
    );
    assert!(!app.log().iter().any(|line| {
        ["unworktree:", "unbranch:", "ahead:"]
            .iter()
            .any(|p| line.starts_with(p))
    }));
    let back = app.task("t1");
    assert_eq!(
        (back.column.as_str(), back.assignee.as_deref()),
        ("todo", None)
    );
}

/* ── worktree: false in a git repository ───────────────────────── */

#[tokio::test]
async fn starts_the_worker_in_the_workspace_folder_when_asked_for_no_worktree_even_in_a_repository()
{
    let app = world(Options {
        tasks: vec![job("t1", "Arena timer", &["Content/Arena/**"])],
        ..Options::default()
    });

    let spawned = app
        .call("worker_spawn", json!({"task": "t1", "worktree": false}))
        .await
        .unwrap();

    assert_eq!(spawned["folder"], REPO);
    assert_eq!(spawned["branch"], Value::Null);
    assert_eq!(spawned["base"], Value::Null);
    assert_eq!(spawned["worktree"], Value::Null);
    assert!(
        spawned["note"]
            .as_str()
            .unwrap()
            .contains("without a worktree, as asked")
    );
    let mia = app.pane_named("Mia").unwrap();
    assert_eq!(mia.cwd.as_deref(), Some(REPO));
    assert_eq!(mia.shared_folder, Some(true));
    let made: Vec<String> = app
        .log()
        .into_iter()
        .filter(|line| {
            ["claim:", "worktree:", "copy:", "branch:"]
                .iter()
                .any(|p| line.starts_with(p))
        })
        .collect();
    assert_eq!(made, ["claim:t1:Mia:shared"]);
    let brief = app.brief("Mia");
    assert!(brief.contains("You work in the shared checkout of the workspace"));
    assert!(brief.contains("`git add <those paths>`, never `git add -A`"));
    assert!(brief.contains("never checkout, reset, stash or rebase"));
    assert!(!brief.contains("git worktree of its own") && !brief.contains("on branch"));
}

#[tokio::test]
async fn still_gives_a_worktree_by_default_or_when_asked_for_one_and_refuses_a_value_that_is_no_boolean()
 {
    for args in [
        json!({"task": "t1"}),
        json!({"task": "t1", "worktree": true}),
    ] {
        let app = world(Options {
            tasks: vec![job("t1", "One", &["a/**"])],
            ..Options::default()
        });
        let spawned = app.call("worker_spawn", args).await.unwrap();
        assert!(spawned["worktree"].is_string());
        assert!(spawned["branch"].is_string());
    }
    let app = world(Options {
        tasks: vec![job("t1", "One", &["a/**"])],
        ..Options::default()
    });
    let refused = err(app
        .call("worker_spawn", json!({"task": "t1", "worktree": "no"}))
        .await);
    assert!(refused.contains("worktree is true or false"), "{refused}");
    assert_eq!(app.task("t1").column, "todo");
}

#[tokio::test]
async fn does_the_same_when_git_is_not_installed_at_all() {
    let app = world(Options {
        tasks: vec![job("t1", "One", &["a/**"])],
        ..Options::default()
    });
    app.fail().repo = Some(RepoFail::NoGit);
    let spawned = app
        .call("worker_spawn", json!({"task": "t1"}))
        .await
        .unwrap();
    assert_eq!(
        (spawned["folder"].clone(), spawned["worktree"].clone()),
        (json!(REPO), Value::Null)
    );
}

#[tokio::test]
async fn undoes_a_start_that_fails_without_leaving_anything_in_the_folder_to_remove() {
    let app = world(Options {
        tasks: vec![job("t1", "One", &["a/**"])],
        ..Options::default()
    });
    app.fail().repo = Some(RepoFail::None);
    app.fail().enqueue = Some("the bridge is gone".into());

    let message = err(app.call("worker_spawn", json!({"task": "t1"})).await);
    assert!(
        regex::Regex::new("the bridge is gone.*Nothing was left behind")
            .unwrap()
            .is_match(&message)
    );
    let undone: Vec<String> = app
        .log()
        .into_iter()
        .filter(|line| {
            ["kill:", "close:", "unworktree:", "release:"]
                .iter()
                .any(|p| line.starts_with(p))
        })
        .collect();
    assert_eq!(undone, ["kill:p10", "close:p10", "release:t1:Mia:true"]);
}

/* ── worker_list ───────────────────────────────────────────────── */

#[tokio::test]
async fn lists_each_worker_with_its_task_branch_ports_and_delivery() {
    let app = world(Options {
        tasks: tasks_one_two(),
        ..Options::default()
    });
    app.call("worker_spawn", json!({"task": "t1"}))
        .await
        .unwrap();
    app.call("worker_spawn", json!({"task": "t2", "model": "haiku"}))
        .await
        .unwrap();
    app.tasks.lock().unwrap()[1].labels = strings(&["ready-to-land"]);

    let listed = app.call("worker_list", json!({})).await.unwrap();
    assert_eq!(listed["workspace"], "Shop");
    // No limit to tell: the orchestrator decides how many run.
    assert!(listed.get("maxWorkers").is_none());
    assert!(listed.get("maxAttempts").is_none());
    assert_eq!(
        listed["workers"][0],
        json!({
            "name": "Mia",
            "task": {"id": "t1", "title": "One", "column": "doing", "attempts": 1},
            "activity": "working",
            "running": true,
            "branch": "hangar/one-t1",
            "worktree": sibling("hangar-one-t1"),
            "model": "sonnet",
            "ports": {"base": PORT_POOL_START + 20, "count": 20},
            "delivery": null,
        })
    );
    let second = &listed["workers"][1];
    assert_eq!(
        (
            second["name"].clone(),
            second["model"].clone(),
            second["delivery"].clone()
        ),
        (json!("Sam"), json!("haiku"), json!("ready"))
    );
}

#[tokio::test]
async fn lists_the_workers_of_a_workspace_in_a_window_of_its_own_as_running() {
    let app = world(Options {
        tasks: tasks_one_two(),
        ..Options::default()
    });
    app.call("worker_spawn", json!({"task": "t1"}))
        .await
        .unwrap();
    // Never opened in the main window this run: its panes are not up.
    app.opened.lock().unwrap().clear();
    let listed = app.call("worker_list", json!({})).await.unwrap();
    assert_eq!(listed["workers"][0]["running"], false);
    // Detached when Hangar last quit: its window draws them.
    app.map_shop(|ws| ws.detached = Some(true));
    let listed = app.call("worker_list", json!({})).await.unwrap();
    assert_eq!(listed["workers"][0]["running"], true);
}

/* ── worker_stop ───────────────────────────────────────────────── */

async fn started() -> (World, Pane, String) {
    let app = world(Options {
        tasks: vec![job("t1", "One", &["a/**"])],
        ..Options::default()
    });
    let spawned = app
        .call("worker_spawn", json!({"task": "t1"}))
        .await
        .unwrap();
    let worker = app.pane_named(spawned["worker"].as_str().unwrap()).unwrap();
    app.clear_log();
    app.events.lock().unwrap().clear();
    (
        app,
        worker,
        spawned["worktree"].as_str().unwrap().to_string(),
    )
}

#[tokio::test]
async fn ends_its_processes_closes_it_gives_the_task_back_and_removes_a_worktree_that_holds_nothing()
 {
    let (app, worker, path) = started().await;

    let stopped = app
        .call("worker_stop", json!({"worker": "mia"}))
        .await
        .unwrap();

    assert_eq!(
        stopped,
        json!({
            "stopped": "Mia",
            "task": {"id": "t1", "title": "One", "column": "todo"},
            "worktree": {"path": path, "branch": "hangar/one-t1", "removed": true, "branchDeleted": true},
        })
    );
    assert_eq!(
        app.log(),
        [
            format!("kill:{}", worker.id),
            format!("close:{}", worker.id),
            "release:t1:Mia:false".to_string(),
            "ahead:main..hangar/one-t1".to_string(),
            format!("unworktree:{path}:true"),
            "unbranch:hangar/one-t1:true".to_string(),
            "announce:closed:Mia:Ava".to_string(),
        ]
    );
    // The attempt still counts.
    let back = app.task("t1");
    assert_eq!(
        (
            back.column.as_str(),
            back.assignee.as_deref(),
            back.attempts
        ),
        ("todo", None, 1)
    );
    assert_eq!(
        app.events(),
        [(
            "w1".to_string(),
            OrchestratorEvent::WorkerStopped {
                worker: "Mia".into(),
                task_id: Some("t1".into())
            }
        )]
    );
}

#[tokio::test]
async fn keeps_a_worktree_with_uncommitted_changes_and_says_which() {
    let (app, _, path) = started().await;
    app.dirty.lock().unwrap().insert(
        path,
        vec![ChangedFile {
            path: "a/x.ts".into(),
            old_path: None,
            status: "M".into(),
            additions: Some(1),
            deletions: Some(0),
            binary: false,
        }],
    );

    let stopped = app
        .call("worker_stop", json!({"worker": "Mia"}))
        .await
        .unwrap();
    assert_eq!(stopped["worktree"]["removed"], false);
    assert_eq!(stopped["worktree"]["branchDeleted"], false);
    assert!(
        stopped["worktree"]["kept"]
            .as_str()
            .unwrap()
            .contains("1 uncommitted change(s): a/x.ts")
    );
    assert!(!app.log().iter().any(|line| line.starts_with("unworktree:")));
}

#[tokio::test]
async fn keeps_a_branch_with_commits_the_target_does_not_have() {
    let (app, _, _) = started().await;
    app.ahead.lock().unwrap().insert("hangar/one-t1".into(), 2);

    let stopped = app
        .call("worker_stop", json!({"worker": "Mia"}))
        .await
        .unwrap();
    assert!(
        stopped["worktree"]["kept"]
            .as_str()
            .unwrap()
            .contains("hangar/one-t1 has 2 commit(s) that main does not")
    );
    assert!(
        app.branches
            .lock()
            .unwrap()
            .contains(&"hangar/one-t1".to_string())
    );
}

#[tokio::test]
async fn says_in_one_plain_line_what_holds_a_worktree_that_would_not_go() {
    let (app, _, _) = started().await;
    app.fail().remove =
        Some("C:/wt/node_modules/x.node cannot be removed, a program still uses it.".into());

    let stopped = app
        .call("worker_stop", json!({"worker": "Mia"}))
        .await
        .unwrap();
    assert_eq!(stopped["worktree"]["removed"], false);
    assert_eq!(stopped["worktree"]["branchDeleted"], false);
    let kept = stopped["worktree"]["kept"].as_str().unwrap();
    assert!(kept.contains("x.node cannot be removed"));
    assert!(!kept.contains("not a working tree") && !kept.contains('\n'));
}

#[tokio::test]
async fn does_not_claim_the_branch_went_when_it_did_not() {
    let (app, _, _) = started().await;
    app.fail().branch_kept = true;

    let stopped = app
        .call("worker_stop", json!({"worker": "Mia"}))
        .await
        .unwrap();
    assert_eq!(stopped["worktree"]["removed"], true);
    assert_eq!(stopped["worktree"]["branchDeleted"], false);
    assert!(
        stopped["worktree"]["kept"]
            .as_str()
            .unwrap()
            .contains("branch hangar/one-t1 could not be deleted")
    );
}

#[tokio::test]
async fn keeps_the_worktree_when_asked_to() {
    let (app, _, _) = started().await;
    let stopped = app
        .call(
            "worker_stop",
            json!({"worker": "Mia", "keepWorktree": true}),
        )
        .await
        .unwrap();
    assert_eq!(stopped["worktree"]["removed"], false);
    assert_eq!(stopped["worktree"]["kept"], "keepWorktree was asked for");
}

#[tokio::test]
async fn only_stops_workers() {
    let (app, _, _) = started().await;
    assert!(
        err(app.call("worker_stop", json!({"worker": "Max"})).await)
            .contains("\"Max\" is not a worker")
    );
    let message = err(app.call("worker_stop", json!({"worker": "Nobody"})).await);
    assert!(
        regex::Regex::new(r#"No worker "Nobody".*"Mia""#)
            .unwrap()
            .is_match(&message),
        "{message}"
    );
}

#[tokio::test]
async fn stops_every_worker_of_a_workspace_for_the_person() {
    let app = world(Options {
        tasks: tasks_one_two(),
        ..Options::default()
    });
    app.call("worker_spawn", json!({"task": "t1"}))
        .await
        .unwrap();
    app.call("worker_spawn", json!({"task": "t2"}))
        .await
        .unwrap();

    assert_eq!(stop_all_workers("w1", &app).await, 2);
    let names: Vec<String> = app.shop().panes.into_iter().map(|pane| pane.name).collect();
    assert_eq!(names, ["Ava", "Max"]);
    assert!(app.log().contains(&"announce:closed:Sam:null".to_string()));
    let columns: Vec<String> = app
        .tasks
        .lock()
        .unwrap()
        .iter()
        .map(|t| t.column.clone())
        .collect();
    assert_eq!(columns, ["todo", "todo"]);
}

/* ── names ─────────────────────────────────────────────────────── */

#[test]
fn makes_a_branch_from_the_tasks_title_and_id() {
    assert_eq!(
        task_slug("Écran de connexion : mot de passe oublié"),
        "ecran-de-connexion-mot-de-passe"
    );
    assert_eq!(task_slug("ログイン"), "task");
    assert_eq!(
        worker_branch("3f2a9c41-0000", "Fix the login form"),
        "hangar/fix-the-login-form-3f2a9c"
    );
}

fn facts<'a>(owns: &'a [String]) -> BriefFacts<'a> {
    BriefFacts {
        task_id: "t",
        title: "T",
        description: "",
        owns,
        worker: "Sam",
        orchestrator: None,
        workspace: "Shop",
        folder: "/w",
        branch: Some("hangar/t-t"),
        target: Some("main"),
        worktree: Some("/w"),
        port_base: PORT_POOL_START,
        build_cache: Vec::new(),
        check_command: None,
        attempt: 1,
        hook_failure: None,
        shared_checkout: false,
    }
}

#[test]
fn the_brief_names_the_build_cache_variable_only_when_it_is_set() {
    let plain = worker_brief(&facts(&strings(&["a"])));
    assert!(!plain.contains("build cache"));
    let owns = strings(&["a"]);
    let mut cached = facts(&owns);
    cached.build_cache = vec!["CARGO_TARGET_DIR".into()];
    let brief = worker_brief(&cached);
    assert!(brief.contains("`CARGO_TARGET_DIR` is set in your shell"));
    assert!(brief.contains("Do not override or unset it"));
}

#[test]
fn writes_a_brief_that_says_what_to_do_when_a_task_owns_no_files() {
    let brief = worker_brief(&facts(&[]));
    assert!(brief.contains("The person started you"));
    assert!(brief.contains("No files were assigned to this task"));
    assert!(brief.contains("(No description beyond the title.)"));
    assert!(!brief.contains("attempt 1"));
    // Spelled out: a worker that guesses them calls with task_id and comment.
    assert!(brief.contains("board_comment_task {id: \"t\", text}"));
    assert!(brief.contains("delivery_submit {summary}"));
}

#[test]
fn writes_a_brief_for_a_folder_without_git_that_never_mentions_a_branch() {
    let owns = strings(&["Content/**"]);
    let brief = worker_brief(&BriefFacts {
        description: "Do it.",
        orchestrator: Some("Ava"),
        workspace: "Island",
        folder: "C:\\uefn\\Island",
        branch: None,
        target: None,
        worktree: None,
        check_command: Some("verse-check"),
        ..facts(&owns)
    });
    assert!(brief.contains("Its orchestrator, Ava, started you"));
    assert!(brief.contains("- `verse-check` passes in the folder."));
    assert!(brief.contains("never revert, move, reformat or delete a file that is not yours"));
    for absent in ["worktree", "on branch", "Commit everything"] {
        assert!(!brief.contains(absent), "{absent}");
    }
}

#[test]
fn sanitizes_branch_names_and_places_worktrees_beside_the_checkout() {
    assert_eq!(sanitize_branch_name("  -feat/login.lock"), "feat/login-");
    assert_eq!(sanitize_branch_name(".env"), "env");
    assert_eq!(sanitize_branch_name("   "), "");
    assert_eq!(
        worktree_folder("/home/me/shop/", "hangar/x"),
        "/home/me/shop-worktrees/hangar-x"
    );
    assert_eq!(worktree_folder(REPO, "a b"), sibling("a-b"));
}
