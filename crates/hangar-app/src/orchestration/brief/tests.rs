//! orchestratorBrief.test.ts, case for case.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::TimeZone;
use hangar_core::board::Task;
use hangar_core::memory::MemoryEntry;

use super::*;
use crate::orchestration::events::{DeliveryOutcome, OrchestratorEvent};
use crate::orchestration::projects::{ProjectsPort, load_projects};
use crate::orchestration::testkit::{comment, pane, shell, strings, task, workspace};
use crate::state::types::{AgentId, AppState, OrchestrationSettings, Pane, PaneRole, Workspace};
use crate::state::{AgentActivity, PaneActivity};

fn now() -> u64 {
    chrono::Local
        .with_ymd_and_hms(2026, 10, 27, 14, 5, 0)
        .unwrap()
        .timestamp_millis() as u64
}

fn memory(title: &str, content: &str) -> MemoryEntry {
    MemoryEntry {
        id: format!("m-{}", title.chars().count()),
        title: title.into(),
        content: content.into(),
        tags: strings(&[PLAN_TAG]),
        workspace: "C:\\dev\\shop".into(),
        agent: "orch".into(),
        created_at: now() - 3_600_000,
        updated_at: now() - 12 * 60_000,
    }
}

/// What `setup()` of the TS test held.
#[derive(Clone)]
struct Setup {
    state: AppState,
    pane_id: String,
    tasks: Vec<Task>,
    memory: Vec<MemoryEntry>,
    journal: Vec<JournalEntry>,
    git: bool,
    projects: Vec<ProjectSummary>,
}

fn activity(id: &str) -> Option<PaneActivity> {
    (id == "w1").then(|| PaneActivity {
        workspace_id: "shop".into(),
        activity: AgentActivity::Working,
        bridged: true,
    })
}

fn setup() -> Setup {
    let mut shop = workspace(
        "shop",
        "Shop",
        vec![
            pane("o", "orch", Some(PaneRole::Orchestrator)),
            Pane {
                task_id: Some("t1".into()),
                port_base: Some(41_000),
                ..pane("w1", "worker-1", Some(PaneRole::Worker))
            },
            pane("p", "plain", None),
            Pane {
                role: Some(PaneRole::Worker),
                ..shell("s", "shell-ish")
            },
        ],
    );
    shop.orchestration = Some(OrchestrationSettings {
        check_command: Some("pnpm test".into()),
        ..OrchestrationSettings::default()
    });
    let state = AppState {
        workspaces: vec![
            shop,
            workspace("blog", "Blog", vec![pane("b", "blogger", None)]),
        ],
        active_workspace_id: Some("shop".into()),
        ..AppState::default()
    };
    let tasks = vec![
        Task {
            title: "Login form".into(),
            column: "doing".into(),
            assignee: Some("worker-1".into()),
            attempts: 1,
            owns: strings(&["src/login/**", "src/login.test.ts"]),
            description: "Goal: a login form.\nAcceptance:\n- submits\nCheck: pnpm test".into(),
            comments: vec![comment(
                "c1",
                "worker-1",
                "Form done,\ntests next",
                now() - 60_000,
            )],
            ..task("t1")
        },
        Task {
            title: "Settings page".into(),
            depends_on: strings(&["t1"]),
            owns: strings(&["src/settings/**"]),
            ..task("t2")
        },
        Task {
            title: "Docs".into(),
            priority: 3,
            ..task("t3")
        },
        Task {
            title: "Scaffold".into(),
            column: "done".into(),
            ..task("t0")
        },
    ];
    let journal = vec![
        JournalEntry {
            at: now() - 5 * 60_000,
            quiet: true,
            event: OrchestratorEvent::WorkerStarted {
                worker: "worker-1".into(),
                task_id: Some("t1".into()),
            },
        },
        JournalEntry {
            at: now() - 60_000,
            quiet: false,
            event: OrchestratorEvent::Delivery {
                task_id: "t1".into(),
                title: "Login form".into(),
                worker: "worker-1".into(),
                outcome: DeliveryOutcome::CheckFailed,
                detail: "2 tests failing".into(),
            },
        },
    ];
    Setup {
        state,
        pane_id: "o".into(),
        tasks,
        memory: vec![memory(
            "Plan — Shop",
            "Objective: let people log in.\nValidation criteria:\n1. a user logs in",
        )],
        journal,
        git: true,
        projects: Vec::new(),
    }
}

fn build(setup: &Setup) -> Option<Briefing> {
    build_briefing(&BriefingInputs {
        state: &setup.state,
        pane_id: &setup.pane_id,
        tasks: &setup.tasks,
        memory: &setup.memory,
        journal: &setup.journal,
        activity: &activity,
        now: now(),
        git: setup.git,
        projects: &setup.projects,
        manager_events: &[],
    })
}

fn with_pane(pane_id: &str) -> Setup {
    Setup {
        pane_id: pane_id.into(),
        ..setup()
    }
}

/// The deps of `briefingFor`, recording what was read.
struct Fake {
    setup: Setup,
    reads: Mutex<Vec<String>>,
    memory_broken: bool,
    /// `None`: no `isRepo` given; `Some(answer)`: what it answers.
    is_repo: Option<Option<bool>>,
    /// Whether `loadProjects` is given.
    projects: bool,
    sent: Mutex<Vec<(String, String)>>,
}

fn fake(setup: Setup) -> Fake {
    Fake {
        setup,
        reads: Mutex::new(Vec::new()),
        memory_broken: false,
        is_repo: None,
        projects: false,
        sent: Mutex::new(Vec::new()),
    }
}

impl BriefingPort for Fake {
    fn state(&self) -> Arc<AppState> {
        Arc::new(self.setup.state.clone())
    }

    async fn load_board(&self, cwd: &str) -> Result<Vec<Task>, String> {
        self.reads.lock().unwrap().push(format!("board {cwd}"));
        Ok(self.setup.tasks.clone())
    }

    async fn load_memory(&self) -> Result<Vec<MemoryEntry>, String> {
        self.reads.lock().unwrap().push("memory".into());
        if self.memory_broken {
            return Err("locked".into());
        }
        Ok(self.setup.memory.clone())
    }

    fn journal(&self, _workspace_id: &str) -> Vec<JournalEntry> {
        self.setup.journal.clone()
    }

    fn activity(&self, pane: &str) -> Option<PaneActivity> {
        activity(pane)
    }

    fn now(&self) -> u64 {
        now()
    }

    async fn is_repo(&self, cwd: &str) -> Result<Option<bool>, String> {
        match self.is_repo {
            Some(answer) => {
                self.reads.lock().unwrap().push(format!("repo {cwd}"));
                Ok(answer)
            }
            // No `isRepo` in the deps: the TS read that as a repository.
            None => Ok(None),
        }
    }

    async fn load_projects(&self) -> Result<Vec<ProjectSummary>, String> {
        if !self.projects {
            return Ok(Vec::new());
        }
        Ok(load_projects(self).await)
    }

    fn manager_journal(&self) -> Vec<crate::orchestration::manager_feed::ManagerJournalEntry> {
        Vec::new()
    }

    fn enqueue(&self, pane: &str, text: &str, _from: &str) -> Result<(), String> {
        self.sent.lock().unwrap().push((pane.into(), text.into()));
        Ok(())
    }
}

/// The TS manager tests' `loadProjects` deps: every board the same, every
/// folder a repository, every workspace open and awake.
impl ProjectsPort for Fake {
    fn state(&self) -> Arc<AppState> {
        Arc::new(self.setup.state.clone())
    }

    async fn load_board(&self, cwd: &str) -> Result<Vec<Task>, String> {
        self.reads.lock().unwrap().push(format!("board {cwd}"));
        Ok(self.setup.tasks.clone())
    }

    async fn load_memory(&self) -> Result<Vec<MemoryEntry>, String> {
        Ok(self.setup.memory.clone())
    }

    async fn is_repo(&self, _cwd: &str) -> Result<Option<bool>, String> {
        Ok(Some(true))
    }

    fn activity(&self, pane: &str) -> Option<PaneActivity> {
        activity(pane)
    }

    fn asleep(&self, _workspace_id: &str) -> bool {
        false
    }

    fn is_open(&self, _workspace_id: &str) -> bool {
        true
    }

    fn journal(&self, _workspace_id: &str) -> Vec<JournalEntry> {
        self.setup.journal.clone()
    }
}

fn reads(fake: &Fake) -> Vec<String> {
    fake.reads.lock().unwrap().clone()
}

/* ── who gets a briefing ───────────────────────────────────────── */

#[test]
fn is_only_a_claude_code_pane_with_a_role() {
    assert_eq!(build(&with_pane("p")), None);
    assert_eq!(build(&with_pane("s")), None);
    assert_eq!(build(&with_pane("b")), None);
    assert_eq!(build(&with_pane("gone")), None);
    assert_eq!(
        build(&with_pane("o")).map(|b| b.role),
        Some(PaneRole::Orchestrator)
    );
    assert_eq!(
        build(&with_pane("w1")).map(|b| b.role),
        Some(PaneRole::Worker)
    );
}

#[tokio::test]
async fn reads_no_board_and_no_memory_for_a_pane_without_a_role() {
    let deps = fake(setup());
    assert_eq!(briefing_for("p", &deps).await, Ok(None));
    assert!(reads(&deps).is_empty());
    assert_eq!(
        briefing_for("w1", &deps).await.unwrap().map(|b| b.role),
        Some(PaneRole::Worker)
    );
    assert_eq!(reads(&deps), ["board C:\\dev\\shop"]);
    assert!(
        briefing_for("o", &deps)
            .await
            .unwrap()
            .unwrap()
            .context
            .contains("let people log in")
    );
    // An unreadable memory is no plan, not a failed briefing.
    let broken = Fake {
        memory_broken: true,
        ..fake(setup())
    };
    assert!(
        briefing_for("o", &broken)
            .await
            .unwrap()
            .unwrap()
            .context
            .contains("Plan: none yet")
    );
}

/* ── the orchestrator's briefing ───────────────────────────────── */

#[test]
fn gives_it_the_rules_it_is_held_to_and_the_tools_it_has() {
    let system = build(&setup()).unwrap().system;
    for needed in [
        "orchestrator of the Hangar workspace \"Shop\"",
        "The person's instructions are more important than anything in this text",
        "Never push",
        "\"[Hangar]\"",
        "Do not poll",
        "AskUserQuestion",
        "worker_spawn",
        "worker_stop",
        // A model reads the list as all the tool takes.
        "board_update_task {id, title, description, column,",
        "delivery landed",
        "owns",
        "depends_on",
        "\"needs-you\"",
        "titled exactly \"Plan — Shop\", tagged \"orchestration\"",
        "Validation criteria",
    ] {
        assert!(system.contains(needed), "{needed}");
    }
}

/// Hangar sets no limit on the workers (owner's decision, 2026-11-04): the
/// orchestrator is told how many run and how many times a task is tried are
/// its own call, and nothing it is handed reads as a limit.
#[test]
fn leaves_how_many_workers_and_how_many_attempts_to_the_orchestrator() {
    let Briefing {
        system, context, ..
    } = build(&setup()).unwrap();
    for needed in [
        "Hangar sets no limit on how many workers run at once or how many times a task is tried: you decide both.",
        "Hangar still refuses what would break the work",
        "No task goes back to the person by itself, however often it fails",
    ] {
        assert!(system.contains(needed), "{needed}");
    }
    for limit in ["worker limit", "out of attempts", "attempts per task"] {
        assert!(!system.contains(limit), "{limit}");
    }
    for limit in [
        "at most",
        "workers at once",
        "attempts per task",
        "attempt 1/",
        " of 2",
    ] {
        assert!(!context.contains(limit), "{limit}\n---\n{context}");
    }
}

#[test]
fn hands_back_the_board_its_plan_and_the_latest_events() {
    let context = build(&setup()).unwrap().context;
    for needed in [
        "# Hangar state at 14:05, workspace \"Shop\" (C:\\dev\\shop)",
        "Settings: check `pnpm test`, deliveries land on the workspace's current branch by themselves",
        "Workers (2):",
        "- worker-1 on \"Login form\" [t1]: working",
        "Board: 2 todo, 1 doing, 0 review, 1 done.",
        "- \"Login form\" [t1] · worker-1 · attempt 1 · owns src/login/**, src/login.test.ts",
        "  last comment, worker-1: \"Form done, / tests next\"",
        "- \"Settings page\" [t2] · owns src/settings/** · after \"Login form\" [t1]",
        "- \"Docs\" [t3] · priority 3 · owns nothing yet · ready",
        "Done: \"Scaffold\".",
        "Plan (memory \"Plan — Shop\" [m-11], updated 12 min ago):\nObjective: let people log in.",
        "Latest events, oldest first:",
        "- 14:04 Delivery of \"Login form\" [t1] by worker-1 failed the check; Hangar sent the worker the output — 2 tests failing",
    ] {
        assert!(context.contains(needed), "{needed}\n---\n{context}");
    }
    // Priority first within a column.
    assert!(
        context.find("\"Docs\" [t3]").unwrap() < context.find("\"Settings page\" [t2]").unwrap()
    );
}

#[test]
fn says_where_the_plan_goes_when_there_is_none_yet() {
    let context = build(&Setup {
        memory: Vec::new(),
        ..setup()
    })
    .unwrap()
    .context;
    assert!(context.contains(
        "Plan: none yet. Write it with memory_write, title \"Plan — Shop\", tag \"orchestration\", before creating tasks."
    ));
}

#[test]
fn stays_compact_on_a_big_board_with_a_long_plan() {
    let tasks: Vec<Task> = (0..120)
        .map(|i| Task {
            title: format!("A task with a fairly long title number {i}"),
            column: if i % 3 == 0 { "done" } else { "todo" }.into(),
            owns: vec![format!("src/area{i}/**")],
            description: "x".repeat(2_000),
            comments: vec![comment(&format!("c{i}"), "w", &"y".repeat(2_000), 1)],
            ..task(&format!("t{i}"))
        })
        .collect();
    let journal: Vec<JournalEntry> = (0..50)
        .map(|i| JournalEntry {
            at: now() - i,
            quiet: false,
            event: OrchestratorEvent::TaskCommented {
                task_id: "t1".into(),
                title: "T".into(),
                by: "w".into(),
                text: "z".repeat(1_000),
            },
        })
        .collect();
    let context = build(&Setup {
        tasks,
        journal,
        memory: vec![memory("Plan — Shop", &"p".repeat(20_000))],
        ..setup()
    })
    .unwrap()
    .context;
    assert!(context.chars().count() < 16_000);
    assert!(context.contains("…and 55 more open tasks: board_list_tasks."));
    assert!(context.contains("(cut here: memory_read m-11 for the rest, and shorten it)"));
    let timed = regex::Regex::new(r"(?m)^- \d\d:\d\d ").unwrap();
    assert_eq!(timed.find_iter(&context).count(), 20);
}

/* ── a workspace that is not a git repository ──────────────────── */

#[test]
fn tells_the_orchestrator_its_workers_share_the_folder_rather_than_to_ask_for_a_repository() {
    let briefing = build(&Setup {
        git: false,
        ..setup()
    })
    .unwrap();
    assert!(
        briefing
            .system
            .contains("in the workspace's folder, beside the other workers")
    );
    assert!(
        briefing
            .system
            .contains("do not ask the person to make it one")
    );
    assert!(briefing.system.contains("delivery is in"));
    assert!(briefing.system.contains("worker_stop {worker}"));
    for absent in [
        "git worktree",
        "git log",
        "delivery landed",
        "merge branches",
        "keepWorktree",
    ] {
        assert!(!briefing.system.contains(absent), "{absent}");
    }
    assert!(
        briefing
            .context
            .contains("Not a git repository: workers share the workspace's folder")
    );
    assert!(
        briefing
            .context
            .contains("check `pnpm test`, run by the workers")
    );
}

#[tokio::test]
async fn asks_only_for_the_orchestrator_and_takes_a_folder_it_cannot_tell_for_a_repository() {
    let deps = Fake {
        is_repo: Some(Some(false)),
        ..fake(setup())
    };
    assert!(
        briefing_for("o", &deps)
            .await
            .unwrap()
            .unwrap()
            .system
            .contains("## No git here")
    );
    assert!(
        briefing_for("w1", &deps)
            .await
            .unwrap()
            .unwrap()
            .system
            .contains("in a git worktree")
    );
    let asked: Vec<String> = reads(&deps)
        .into_iter()
        .filter(|r| r.starts_with("repo "))
        .collect();
    assert_eq!(asked, ["repo C:\\dev\\shop"]);
    let unknown = Fake {
        is_repo: Some(None),
        ..fake(setup())
    };
    assert!(
        !briefing_for("o", &unknown)
            .await
            .unwrap()
            .unwrap()
            .system
            .contains("## No git here")
    );
}

#[test]
fn gives_a_worker_in_the_shared_folder_rules_that_keep_it_off_the_others_files() {
    let mut inputs = with_pane("w1");
    inputs.state.workspaces[0].panes[1].shared_folder = Some(true);
    let system = build(&inputs).unwrap().system;
    assert!(
        system.contains("You work in the workspace's own folder, not in a checkout of your own")
    );
    assert!(system.contains("Never revert, move, reformat or delete a file that is not yours"));
    assert!(system.contains("Nothing is merged or reviewed here"));
    for absent in ["worktree", "branch", "Commit"] {
        assert!(!system.contains(absent), "{absent}");
    }
}

/* ── a worker's briefing ───────────────────────────────────────── */

#[test]
fn gives_it_its_task_its_files_its_check_and_its_ports() {
    let Briefing {
        system, context, ..
    } = build(&with_pane("w1")).unwrap();
    for needed in [
        "to do one board task, \"Login form\" [t1]",
        "orchestrator (pane \"orch\")",
        "delivery_submit",
        "Never push",
        "HANGAR_PORT_BASE",
    ] {
        assert!(system.contains(needed), "{needed}");
    }
    for needed in [
        "Your task \"Login form\" [t1]: in doing, attempt 1.",
        "Owns: src/login/**, src/login.test.ts.",
        "Orchestrator: pane \"orch\". Check: `pnpm test`. Ports: 41000 to 41019 (HANGAR_PORT_BASE).",
        "Description:\nGoal: a login form.",
        "worker-1: \"Form done, / tests next\"",
        "2 tests failing",
    ] {
        assert!(context.contains(needed), "{needed}\n---\n{context}");
    }
    assert!(!context.contains("workerStarted"));
    assert!(!context.contains("Plan"));
}

/// Builds are the cost of a compiled project: the worker iterates cheaply and
/// leaves the whole check to the delivery, which runs it anyway.
#[test]
fn tells_a_worker_to_iterate_cheaply_and_leave_the_whole_check_to_the_delivery() {
    let system = build(&with_pane("w1")).unwrap().system;
    for needed in [
        "Iterate with the fastest command that proves your change",
        "build only the package or crate you touched",
        "Never run the workspace's whole check: Hangar runs it on delivery_submit",
        "CARGO_TARGET_DIR, when set), never a private one of your own",
        "run the formatter and the linter on what you touched",
    ] {
        assert!(system.contains(needed), "{needed}");
    }
    assert!(!system.contains("Run the check from the brief"));
}

/// Without git nobody else runs the check, so the worker still does.
#[test]
fn keeps_the_shared_folder_worker_running_the_check_itself() {
    let mut inputs = with_pane("w1");
    inputs.state.workspaces[0].panes[1].shared_folder = Some(true);
    let system = build(&inputs).unwrap().system;
    assert!(system.contains("Run the check from the brief, and the tests near your change"));
    assert!(!system.contains("Never run the workspace's whole check"));
}

#[test]
fn sizes_tasks_by_what_a_build_and_a_delivery_check_cost() {
    let system = build(&setup()).unwrap().system;
    for needed in [
        "Each task costs one worker build and one full delivery check",
        "the briefing may give the last check's duration",
        "fewer, larger tasks along module lines",
        "never a task of their own",
        "Check: the fastest targeted command that proves it",
        "not the workspace check: Hangar runs that on every delivery",
        "worker_stop its worker (Hangar removes the landed worktree and branch)",
        "checkFailed whose output points at the environment, not the branch",
        "workspace_set_check",
        "A delivery submitted before the check changed ran the old one",
    ] {
        assert!(system.contains(needed), "{needed}");
    }
}

#[test]
fn leaves_the_check_of_a_task_in_a_shared_folder_as_the_command_that_proves_it() {
    let system = build(&Setup {
        git: false,
        ..setup()
    })
    .unwrap()
    .system;
    assert!(system.contains("Check: the command that proves it."));
    assert!(!system.contains("delivery check"));
}

/* ── the plan document ─────────────────────────────────────────── */

#[test]
fn is_named_after_the_workspace_and_its_folder_when_another_has_that_name() {
    let a = workspace("a", "Shop", vec![]);
    let b = workspace("b", "shop", vec![]);
    assert_eq!(plan_title(&a, std::slice::from_ref(&a)), "Plan — Shop");
    assert_eq!(plan_title(&a, &[a.clone(), b]), "Plan — Shop (C:\\dev\\a)");
}

#[test]
fn is_found_by_its_title_or_by_the_folder_it_was_written_from() {
    let ws = workspace("shop", "Shop", vec![]);
    let renamed = MemoryEntry {
        workspace: "c:/dev/shop/".into(),
        ..memory("Plan — Old name", "old")
    };
    let other = MemoryEntry {
        workspace: "C:\\dev\\blog".into(),
        ..memory("Plan — Elsewhere", "x")
    };
    let untagged = MemoryEntry {
        tags: Vec::new(),
        ..memory("Plan — Notes", "n")
    };
    let found =
        |entries: &[MemoryEntry]| find_plan(entries, "Plan — Shop", &ws).map(|e| e.content.clone());
    assert_eq!(
        found(&[renamed.clone(), other.clone()]).as_deref(),
        Some("old")
    );
    assert_eq!(found(&[other, untagged]), None);
    let exact = memory("Plan — Shop", "exact");
    assert_eq!(found(&[renamed, exact]).as_deref(), Some("exact"));
}

/* ── a role given mid-session ──────────────────────────────────── */

#[test]
fn carries_the_role_and_the_state_in_one_message() {
    let briefing = build(&setup()).unwrap();
    let text = role_message(briefing.role, &briefing.system, &briefing.context, "Shop");
    assert!(
        text.starts_with("Hangar has just pinned you as the orchestrator of workspace \"Shop\".")
    );
    assert!(text.contains(&briefing.system));
    assert!(text.contains(&briefing.context));
    assert!(release_message(PaneRole::Orchestrator, "Shop").contains("no longer the orchestrator"));
}

#[tokio::test]
async fn announces_a_role_to_a_bridged_session_only() {
    let deps = fake(setup());
    // w1 is bridged in this world, and a worker.
    assert_eq!(announce_role("shop", "w1", None, &deps).await, Ok(true));
    assert!(
        deps.sent.lock().unwrap()[0]
            .1
            .starts_with("Hangar has just made you a worker in workspace \"Shop\".")
    );
    // The orchestrator has no bridged session here: its next one reads the role.
    assert_eq!(announce_role("shop", "o", None, &deps).await, Ok(false));
    let mut unpinned = setup();
    unpinned.state.workspaces[0].panes[1].role = None;
    let deps = fake(unpinned);
    assert_eq!(
        announce_role("shop", "w1", Some(PaneRole::Worker), &deps).await,
        Ok(true)
    );
    assert!(
        deps.sent.lock().unwrap()[0]
            .1
            .contains("no longer a worker")
    );
    assert_eq!(announce_role("shop", "w1", None, &deps).await, Ok(false));
}

/* ── the manager's briefing ────────────────────────────────────── */

fn manager_setup() -> Setup {
    let mut inputs = setup();
    let mut hq = workspace(
        "hq",
        "HQ",
        vec![pane("m", "manager", Some(PaneRole::Manager))],
    );
    hq.hq = Some(true);
    inputs.state.workspaces.insert(0, hq);
    inputs.pane_id = "m".into();
    inputs
}

fn manager_deps(inputs: Setup) -> Fake {
    Fake {
        projects: true,
        ..fake(inputs)
    }
}

#[test]
fn gives_it_its_role_its_rules_and_its_tools() {
    let briefing = build(&manager_setup()).unwrap();
    assert_eq!(briefing.role, PaneRole::Manager);
    for needed in [
        "project manager",
        "Never write code",
        "project_send",
        "project_list",
        "\"Projects — tracker\"",
    ] {
        assert!(briefing.system.contains(needed), "{needed}");
    }
}

#[tokio::test]
async fn hands_back_the_overview_without_the_hq_and_says_where_the_tracker_goes_when_there_is_none()
{
    let briefing = briefing_for("m", &manager_deps(manager_setup()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(briefing.role, PaneRole::Manager);
    assert!(briefing.context.contains("# Hangar projects at 14:05"));
    assert!(briefing.context.contains("Shop"));
    assert!(briefing.context.contains("Blog"));
    assert!(!briefing.context.contains("HQ —"));
    assert!(briefing.context.contains(
        "Tracker: none yet. Write it with memory_write, title \"Projects — tracker\", tag \"manager\""
    ));
}

#[tokio::test]
async fn hands_back_the_tracker_the_manager_wrote() {
    let mut inputs = manager_setup();
    let tracker = MemoryEntry {
        tags: strings(&["manager"]),
        ..memory("Projects — tracker", "Shop: the person wants login first.")
    };
    inputs.memory.push(tracker);
    let briefing = briefing_for("m", &manager_deps(inputs))
        .await
        .unwrap()
        .unwrap();
    assert!(
        briefing
            .context
            .contains("Tracker (memory \"Projects — tracker\"")
    );
    assert!(briefing.context.contains("the person wants login first"));
    assert!(!briefing.context.contains("Tracker: none yet"));
}

#[tokio::test]
async fn reads_no_board_for_itself_and_leaves_the_orchestrator_and_the_worker_alone() {
    let deps = manager_deps(manager_setup());
    briefing_for("m", &deps).await.unwrap();
    assert!(!reads(&deps).contains(&"board C:\\dev\\hq".to_string()));
    assert!(
        briefing_for("o", &deps)
            .await
            .unwrap()
            .unwrap()
            .context
            .contains("let people log in")
    );
    assert_eq!(
        briefing_for("w1", &deps).await.unwrap().map(|b| b.role),
        Some(PaneRole::Worker)
    );
    let plain = build(&setup()).unwrap().system;
    let with_projects = build(&Setup {
        projects: Vec::new(),
        ..setup()
    })
    .unwrap()
    .system;
    assert_eq!(plain, with_projects);
}

#[tokio::test]
async fn is_told_its_role_mid_session_and_when_it_loses_it() {
    let briefing = briefing_for("m", &manager_deps(manager_setup()))
        .await
        .unwrap()
        .unwrap();
    let text = role_message(briefing.role, &briefing.system, &briefing.context, "HQ");
    assert!(text.starts_with("Hangar has just pinned you as the person's project manager"));
    assert!(text.contains(&briefing.context));
    assert!(
        release_message(PaneRole::Manager, "HQ").contains("no longer the person's project manager")
    );
}

#[tokio::test]
async fn answers_the_internal_request_in_the_shape_core_checks() {
    let deps = fake(setup());
    let answer = answer_briefing(&serde_json::json!({"paneId": "o"}), &deps)
        .await
        .unwrap();
    assert_eq!(answer["role"], "orchestrator");
    assert!(answer["system"].as_str().unwrap().contains("orchestrator"));
    assert!(answer["context"].as_str().unwrap().contains("Board:"));
    let none = answer_briefing(&serde_json::json!({"paneId": "p"}), &deps)
        .await
        .unwrap();
    assert_eq!(none, serde_json::Value::Null);
    let _ = (
        HashMap::<String, String>::new(),
        AgentId::Claude,
        Workspace::default(),
    );
}
