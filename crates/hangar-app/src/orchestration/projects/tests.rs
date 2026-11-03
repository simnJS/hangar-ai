//! projects.test.ts, case for case.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::TimeZone;
use hangar_core::board::Task;
use hangar_core::memory::MemoryEntry;

use super::*;
use crate::orchestration::brief::{BriefingInputs, build_briefing};
use crate::orchestration::brief_text::PLAN_TAG;
use crate::orchestration::events::OrchestratorEvent;
use crate::orchestration::testkit::{pane, shell, strings, task, workspace};
use crate::state::roles::{hq_of, manager_of, orchestration_of};
use crate::state::types::{AgentId, AppState, OrchestrationSettings, Pane, PaneRole, Workspace};
use crate::state::{AgentActivity, PaneActivity};

fn now() -> u64 {
    chrono::Local
        .with_ymd_and_hms(2026, 10, 28, 18, 30, 0)
        .unwrap()
        .timestamp_millis() as u64
}

fn plan(title: &str, content: &str) -> MemoryEntry {
    MemoryEntry {
        id: format!("m-{title}"),
        title: title.into(),
        content: content.into(),
        tags: strings(&[PLAN_TAG]),
        workspace: "C:\\dev\\shop".into(),
        agent: "orch".into(),
        created_at: now() - 3_600_000,
        updated_at: now() - 60_000,
    }
}

fn hq() -> Workspace {
    let mut hq = workspace("hq", "HQ", vec![pane("m", "boss", Some(PaneRole::Manager))]);
    hq.hq = Some(true);
    hq
}

fn task_in(id: &str, title: &str) -> Task {
    Task {
        title: title.into(),
        ..task(id)
    }
}

/// Shop has an orchestration at work; Blog has agents and a board but no
/// orchestrator; Wiki sleeps.
struct World {
    state: Mutex<AppState>,
    boards: Mutex<HashMap<String, Result<Vec<Task>, String>>>,
    memory: Mutex<Result<Vec<MemoryEntry>, String>>,
    repo_broken: bool,
    activities: HashMap<String, AgentActivity>,
    journal: Vec<JournalEntry>,
    reads: Mutex<Vec<String>>,
}

impl ProjectsPort for World {
    fn state(&self) -> Arc<AppState> {
        Arc::new(self.state.lock().unwrap().clone())
    }

    async fn load_board(&self, cwd: &str) -> Result<Vec<Task>, String> {
        self.reads.lock().unwrap().push(cwd.into());
        self.boards
            .lock()
            .unwrap()
            .get(cwd)
            .cloned()
            .unwrap_or(Ok(Vec::new()))
    }

    async fn load_memory(&self) -> Result<Vec<MemoryEntry>, String> {
        self.memory.lock().unwrap().clone()
    }

    async fn is_repo(&self, cwd: &str) -> Result<Option<bool>, String> {
        if self.repo_broken {
            return Err("no git".into());
        }
        Ok(Some(cwd != "C:\\dev\\blog"))
    }

    fn activity(&self, pane: &str) -> Option<PaneActivity> {
        self.activities.get(pane).map(|activity| PaneActivity {
            workspace_id: "?".into(),
            activity: *activity,
            bridged: true,
        })
    }

    fn asleep(&self, workspace_id: &str) -> bool {
        workspace_id == "wiki"
    }

    fn is_open(&self, workspace_id: &str) -> bool {
        workspace_id == "shop" || workspace_id == "blog"
    }

    fn journal(&self, workspace_id: &str) -> Vec<JournalEntry> {
        if workspace_id == "shop" {
            self.journal.clone()
        } else {
            Vec::new()
        }
    }
}

fn setup() -> World {
    let shop = workspace(
        "shop",
        "Shop",
        vec![
            pane("o", "orch", Some(PaneRole::Orchestrator)),
            Pane {
                task_id: Some("t1".into()),
                ..pane("w1", "worker-1", Some(PaneRole::Worker))
            },
            Pane {
                task_id: Some("t2".into()),
                ..pane("w2", "worker-2", Some(PaneRole::Worker))
            },
            pane("p", "notes", None),
            shell("sh", "term"),
        ],
    );
    let blog = workspace(
        "blog",
        "Blog",
        vec![pane("b1", "writer", None), pane("b2", "cx", None)],
    );
    let wiki = workspace("wiki", "Wiki", vec![pane("k", "wiki-agent", None)]);
    let boards = HashMap::from([
        (
            "C:\\dev\\shop".to_string(),
            Ok(vec![
                Task {
                    column: "doing".into(),
                    assignee: Some("worker-1".into()),
                    order: 1.0,
                    ..task_in("t1", "Login form")
                },
                Task {
                    column: "doing".into(),
                    assignee: Some("worker-2".into()),
                    priority: 2,
                    order: 2.0,
                    ..task_in("t2", "Payment page")
                },
                Task {
                    column: "review".into(),
                    labels: strings(&["ready-to-land"]),
                    ..task_in("t3", "Signup")
                },
                Task {
                    labels: strings(&["needs-you"]),
                    order: 1.0,
                    ..task_in("t4", "Pick a mailer")
                },
                Task {
                    labels: strings(&["needs-you"]),
                    priority: 3,
                    order: 2.0,
                    ..task_in("t5", "Pick a payment provider")
                },
                Task {
                    column: "done".into(),
                    labels: strings(&["needs-you", "ready-to-land"]),
                    ..task_in("t6", "Old question")
                },
                Task {
                    column: "done".into(),
                    ..task_in("t0", "Scaffold")
                },
            ]),
        ),
        (
            "C:\\dev\\blog".to_string(),
            Ok(vec![
                task_in("b-1", "Write about Rust"),
                Task {
                    column: "doing".into(),
                    assignee: Some("writer".into()),
                    ..task_in("b-2", "Draft the intro")
                },
                Task {
                    column: "done".into(),
                    ..task_in("b-3", "Pick a theme")
                },
            ]),
        ),
        (
            "C:\\dev\\wiki".to_string(),
            Ok(vec![Task {
                column: "done".into(),
                ..task_in("k-1", "Import pages")
            }]),
        ),
    ]);
    let activities = HashMap::from(
        [
            ("o", AgentActivity::Working),
            ("w1", AgentActivity::Working),
            ("w2", AgentActivity::Waiting),
            ("p", AgentActivity::Waiting),
            ("sh", AgentActivity::Waiting),
            ("b1", AgentActivity::Idle),
            ("b2", AgentActivity::Waiting),
            ("m", AgentActivity::Waiting),
        ]
        .map(|(k, v)| (k.to_string(), v)),
    );
    let journal = (0..7)
        .map(|n| JournalEntry {
            at: now() - (7 - n) * 60_000,
            quiet: false,
            event: OrchestratorEvent::WorkerStarted {
                worker: format!("worker-{n}"),
                task_id: Some("t1".into()),
            },
        })
        .collect();
    World {
        state: Mutex::new(AppState {
            workspaces: vec![hq(), shop, blog, wiki],
            active_workspace_id: Some("shop".into()),
            ..AppState::default()
        }),
        boards: Mutex::new(boards),
        memory: Mutex::new(Ok(vec![plan(
            "Plan — Shop",
            "Objective: sell things.\nTasks:\n- t1 doing\nStatus: T1 and T2 running; T3 waits to land.",
        )])),
        repo_broken: false,
        activities,
        journal,
        reads: Mutex::new(Vec::new()),
    }
}

fn by_name<'a>(projects: &'a [ProjectSummary], name: &str) -> &'a ProjectSummary {
    projects
        .iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("no project {name}"))
}

fn tasks(items: &[(&str, &str)]) -> Vec<ProjectTask> {
    items
        .iter()
        .map(|(id, title)| ProjectTask {
            id: id.to_string(),
            title: title.to_string(),
        })
        .collect()
}

/* ── the manager role ──────────────────────────────────────────── */

#[test]
fn is_neither_an_orchestrator_nor_a_worker_and_has_a_briefing_of_its_own() {
    let world = setup();
    let state = world.state.lock().unwrap().clone();
    let home = &state.workspaces[0];
    assert!(orchestrator_of(home).is_none());
    assert!(workers_of(home).is_empty());
    assert_eq!(orchestration_of(home), OrchestrationSettings::default());
    // A manager pane listed first does not hide a workspace's orchestrator.
    let mixed = workspace(
        "x",
        "X",
        vec![
            pane("m2", "boss", Some(PaneRole::Manager)),
            pane("o2", "orch", Some(PaneRole::Orchestrator)),
        ],
    );
    assert_eq!(orchestrator_of(&mixed).map(|p| p.id.as_str()), Some("o2"));
    assert!(workers_of(&mixed).is_empty());
    let briefing = build_briefing(&BriefingInputs {
        state: &state,
        pane_id: "m",
        tasks: &[],
        memory: &[],
        journal: &[],
        activity: &|_| None,
        now: now(),
        git: true,
        projects: &[],
        manager_events: &[],
    });
    assert_eq!(briefing.map(|b| b.role), Some(PaneRole::Manager));
}

#[test]
fn is_found_across_the_workspaces_beside_the_hq() {
    let world = setup();
    let state = world.state.lock().unwrap().clone();
    let (ws, pane) = manager_of(&state).unwrap();
    assert_eq!((ws.id.as_str(), pane.id.as_str()), ("hq", "m"));
    assert_eq!(hq_of(&state).map(|ws| ws.id.as_str()), Some("hq"));
    let shop_only = AppState {
        workspaces: vec![state.workspaces[1].clone()],
        ..AppState::default()
    };
    assert!(manager_of(&shop_only).is_none());
    assert!(hq_of(&shop_only).is_none());
}

/* ── loadProjects ──────────────────────────────────────────────── */

#[tokio::test]
async fn leaves_the_hq_out_and_reads_nothing_of_it() {
    let world = setup();
    let projects = load_projects(&world).await;
    let names: Vec<&str> = projects.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Shop", "Blog", "Wiki"]);
    assert!(
        !world
            .reads
            .lock()
            .unwrap()
            .contains(&"C:\\dev\\hq".to_string())
    );
}

#[tokio::test]
async fn summarizes_a_workspace_without_an_orchestrator_from_its_panes_and_board() {
    let projects = load_projects(&setup()).await;
    let blog = by_name(&projects, "Blog");
    assert_eq!(blog.id, "blog");
    assert_eq!(blog.cwd, "C:\\dev\\blog");
    assert!(blog.open);
    assert!(!blog.asleep);
    assert_eq!(blog.git, Some(false));
    assert_eq!(blog.orchestrator, None);
    assert!(blog.workers.is_empty());
    assert_eq!(
        blog.waiting,
        [WaitingPane {
            name: "cx".into(),
            agent: AgentId::Claude
        }]
    );
    assert_eq!(
        blog.board,
        BoardCounts {
            todo: 1,
            doing: 1,
            review: 0,
            done: 1
        }
    );
    assert_eq!(blog.board_error, None);
    assert_eq!(
        blog.doing,
        [DoingTask {
            id: "b-2".into(),
            title: "Draft the intro".into(),
            assignee: Some("writer".into())
        }]
    );
    assert!(blog.needs_you.is_empty());
    assert!(blog.ready_to_land.is_empty());
    assert!(!blog.all_done);
    assert_eq!(blog.plan_status, None);
    assert!(blog.last_events.is_empty());
}

#[tokio::test]
async fn gives_the_orchestrator_the_workers_and_what_they_are_doing() {
    let projects = load_projects(&setup()).await;
    let shop = by_name(&projects, "Shop");
    assert_eq!(shop.git, Some(true));
    assert_eq!(
        shop.orchestrator,
        Some(ProjectPane {
            name: "orch".into(),
            activity: Some(AgentActivity::Working)
        })
    );
    assert_eq!(
        shop.workers,
        [
            ProjectWorker {
                name: "worker-1".into(),
                task_id: Some("t1".into()),
                activity: Some(AgentActivity::Working)
            },
            ProjectWorker {
                name: "worker-2".into(),
                task_id: Some("t2".into()),
                activity: Some(AgentActivity::Waiting)
            },
        ]
    );
    // Higher priority first.
    let ids: Vec<&str> = shop.doing.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["t2", "t1"]);
    assert_eq!(
        shop.doing[0],
        DoingTask {
            id: "t2".into(),
            title: "Payment page".into(),
            assignee: Some("worker-2".into())
        }
    );
}

#[tokio::test]
async fn lists_every_agent_pane_waiting_for_the_person_workers_or_not_and_no_shell() {
    let projects = load_projects(&setup()).await;
    assert_eq!(
        by_name(&projects, "Shop").waiting,
        [
            WaitingPane {
                name: "worker-2".into(),
                agent: AgentId::Claude
            },
            WaitingPane {
                name: "notes".into(),
                agent: AgentId::Claude
            },
        ]
    );
}

#[tokio::test]
async fn counts_the_boards_columns() {
    let projects = load_projects(&setup()).await;
    assert_eq!(
        by_name(&projects, "Shop").board,
        BoardCounts {
            todo: 2,
            doing: 2,
            review: 1,
            done: 2
        }
    );
}

#[tokio::test]
async fn lists_the_open_tasks_that_need_the_person_and_those_ready_to_land() {
    let projects = load_projects(&setup()).await;
    let shop = by_name(&projects, "Shop");
    // The done one waits for nobody, whatever its labels say.
    assert_eq!(
        shop.needs_you,
        tasks(&[("t5", "Pick a payment provider"), ("t4", "Pick a mailer")])
    );
    assert_eq!(shop.ready_to_land, tasks(&[("t3", "Signup")]));
}

#[tokio::test]
async fn is_all_done_only_for_a_board_with_tasks_every_one_of_them_done() {
    let world = setup();
    assert!(by_name(&load_projects(&world).await, "Wiki").all_done);
    assert!(!by_name(&load_projects(&world).await, "Shop").all_done);
    world
        .boards
        .lock()
        .unwrap()
        .insert("C:\\dev\\wiki".into(), Ok(Vec::new()));
    assert!(!by_name(&load_projects(&world).await, "Wiki").all_done);
}

#[tokio::test]
async fn takes_the_status_line_of_the_orchestrators_plan() {
    let world = setup();
    let status =
        |projects: &[ProjectSummary], name: &str| by_name(projects, name).plan_status.clone();
    assert_eq!(
        status(&load_projects(&world).await, "Shop").as_deref(),
        Some("T1 and T2 running; T3 waits to land.")
    );
    // Renamed since: the plan is still found from its folder.
    world.state.lock().unwrap().workspaces[1].name = "Store".into();
    assert_eq!(
        status(&load_projects(&world).await, "Store").as_deref(),
        Some("T1 and T2 running; T3 waits to land.")
    );
    // No status written: none, rather than something else of the plan.
    *world.memory.lock().unwrap() = Ok(vec![plan("Plan — Store", "Objective: sell things.")]);
    assert_eq!(status(&load_projects(&world).await, "Store"), None);
}

#[tokio::test]
async fn tells_a_workspace_asleep_and_one_not_open() {
    let projects = load_projects(&setup()).await;
    let wiki = by_name(&projects, "Wiki");
    assert!(wiki.asleep);
    assert!(!wiki.open);
    assert!(
        render_overview(std::slice::from_ref(wiki), now())
            .contains("Wiki — C:\\dev\\wiki · asleep")
    );
}

#[tokio::test]
async fn gives_an_unreadable_board_an_empty_part_and_still_summarizes_everything_else() {
    let world = setup();
    world
        .boards
        .lock()
        .unwrap()
        .insert("C:\\dev\\wiki".into(), Err("locked".into()));
    world.boards.lock().unwrap().insert(
        "C:\\dev\\blog".into(),
        Err("Error: board.json is not JSON".into()),
    );
    let projects = load_projects(&world).await;
    let blog = by_name(&projects, "Blog");
    assert_eq!(blog.board_error.as_deref(), Some("board.json is not JSON"));
    assert_eq!(blog.board, BoardCounts::default());
    assert!(blog.doing.is_empty());
    assert!(!blog.all_done);
    assert_eq!(
        blog.waiting,
        [WaitingPane {
            name: "cx".into(),
            agent: AgentId::Claude
        }]
    );
    assert_eq!(
        by_name(&projects, "Wiki").board_error.as_deref(),
        Some("locked")
    );
    assert_eq!(by_name(&projects, "Shop").board.doing, 2);
    assert!(render_project(blog).contains("Board: could not be read (board.json is not JSON)."));
}

#[tokio::test]
async fn does_without_the_memory_and_the_repository_when_they_cannot_be_read() {
    let mut world = setup();
    *world.memory.lock().unwrap() = Err("memory locked".into());
    world.repo_broken = true;
    let projects = load_projects(&world).await;
    assert_eq!(projects.len(), 3);
    assert!(
        projects
            .iter()
            .all(|p| p.plan_status.is_none() && p.git.is_none())
    );
}

#[tokio::test]
async fn reads_a_folder_two_workspaces_share_once() {
    let world = setup();
    let mut old = workspace("shop2", "Shop (old)", vec![]);
    old.cwd = "c:/dev/shop/".into();
    world.state.lock().unwrap().workspaces.push(old);
    let projects = load_projects(&world).await;
    let shop_reads = world
        .reads
        .lock()
        .unwrap()
        .iter()
        .filter(|cwd| cwd.to_lowercase().contains("shop"))
        .count();
    assert_eq!(shop_reads, 1);
    assert_eq!(
        by_name(&projects, "Shop (old)").board,
        by_name(&projects, "Shop").board
    );
}

#[tokio::test]
async fn keeps_the_latest_few_events_worded_with_the_boards_titles() {
    let projects = load_projects(&setup()).await;
    let shop = by_name(&projects, "Shop");
    assert_eq!(shop.last_events.len(), 5);
    assert_eq!(
        shop.last_events[0].text,
        "worker-2 started on \"Login form\" [t1]"
    );
    assert_eq!(
        shop.last_events[4].text,
        "worker-6 started on \"Login form\" [t1]"
    );
}

/* ── planStatusOf ──────────────────────────────────────────────── */

#[test]
fn reads_the_status_on_its_line_or_under_it() {
    let status = |text: &str| plan_status_of(text);
    assert_eq!(
        status("Objective: x\nStatus: T2 running, T3 next.\n").as_deref(),
        Some("T2 running, T3 next.")
    );
    assert_eq!(
        status("  Status: indented as the role shows it").as_deref(),
        Some("indented as the role shows it")
    );
    assert_eq!(
        status("**Status:** all landed").as_deref(),
        Some("all landed")
    );
    assert_eq!(status("- Status: in a list").as_deref(), Some("in a list"));
    assert_eq!(
        status("Status:\n- T2 running\n- T3 next\n\nNotes").as_deref(),
        Some("- T2 running / - T3 next")
    );
    assert_eq!(
        status("## Status\nAll landed.\n## Notes").as_deref(),
        Some("All landed.")
    );
    assert_eq!(status("Status:\nTasks:\n- t1"), None);
    assert_eq!(status("Objective: the status quo"), None);
    assert_eq!(
        status(&format!("Status: {}", "x".repeat(400))).map(|s| s.chars().count()),
        Some(300)
    );
}

/* ── Texts ─────────────────────────────────────────────────────── */

fn summary(n: usize) -> ProjectSummary {
    ProjectSummary {
        id: format!("p{n}"),
        name: format!("Project {n}"),
        cwd: format!("C:\\dev\\project-{n}"),
        open: true,
        asleep: false,
        git: Some(true),
        orchestrator: None,
        workers: Vec::new(),
        waiting: Vec::new(),
        board: BoardCounts::default(),
        board_error: None,
        needs_you: Vec::new(),
        ready_to_land: Vec::new(),
        doing: Vec::new(),
        all_done: false,
        plan_status: None,
        last_events: Vec::new(),
    }
}

fn uuid(n: usize, k: usize) -> String {
    format!("{n:08}-1ce9-461d-9521-{k:012}")
}

/// A project with all an overview can say about it, at length.
fn busy(n: usize) -> ProjectSummary {
    let named = |k: usize, what: &str| -> Vec<ProjectTask> {
        (0..k)
            .map(|i| ProjectTask {
                id: uuid(n, i),
                title: format!("{what} {i} ").repeat(12),
            })
            .collect()
    };
    ProjectSummary {
        orchestrator: Some(ProjectPane {
            name: "orch".into(),
            activity: Some(AgentActivity::Working),
        }),
        workers: vec![
            ProjectWorker {
                name: "worker-1".into(),
                task_id: Some(uuid(n, 0)),
                activity: Some(AgentActivity::Working),
            },
            ProjectWorker {
                name: "worker-2".into(),
                task_id: Some(uuid(n, 1)),
                activity: Some(AgentActivity::Waiting),
            },
            ProjectWorker {
                name: "worker-3".into(),
                task_id: None,
                activity: None,
            },
        ],
        waiting: (0..5)
            .map(|i| WaitingPane {
                name: format!("pane-with-a-long-name-{i}"),
                agent: AgentId::Claude,
            })
            .collect(),
        board: BoardCounts {
            todo: 12,
            doing: 3,
            review: 2,
            done: 30,
        },
        needs_you: named(5, "Decide something"),
        ready_to_land: named(4, "Land something"),
        doing: named(3, "Do something")
            .into_iter()
            .map(|t| DoingTask {
                id: t.id,
                title: t.title,
                assignee: Some("worker-1".into()),
            })
            .collect(),
        plan_status: Some("Status words go on and on. ".repeat(20)),
        last_events: vec![LastEvent {
            at: now(),
            text: "Something happened. ".repeat(30),
        }],
        ..summary(n)
    }
}

/// A project as most are: an orchestrator, one worker, one question, a status.
fn typical(n: usize) -> ProjectSummary {
    ProjectSummary {
        orchestrator: Some(ProjectPane {
            name: "orch".into(),
            activity: Some(AgentActivity::Idle),
        }),
        workers: vec![ProjectWorker {
            name: "worker-1".into(),
            task_id: Some(uuid(n, 1)),
            activity: Some(AgentActivity::Working),
        }],
        board: BoardCounts {
            todo: 3,
            doing: 1,
            review: 0,
            done: 5,
        },
        needs_you: vec![ProjectTask {
            id: uuid(n, 2),
            title: "Choose between the two layouts for the settings page".into(),
        }],
        doing: vec![DoingTask {
            id: uuid(n, 1),
            title: "Add CSV export to the orders page".into(),
            assignee: Some("worker-1".into()),
        }],
        plan_status: Some("T2 runs; T3 and T4 start once it lands, then the release notes.".into()),
        ..summary(n)
    }
}

#[test]
fn says_each_project_in_a_few_lines_the_ones_with_something_for_the_person_first() {
    let text = render_overview(
        &[
            ProjectSummary {
                name: "Quiet".into(),
                git: Some(false),
                open: false,
                ..summary(1)
            },
            ProjectSummary {
                name: "Shop".into(),
                orchestrator: Some(ProjectPane {
                    name: "orch".into(),
                    activity: Some(AgentActivity::Working),
                }),
                workers: vec![
                    ProjectWorker {
                        name: "worker-1".into(),
                        task_id: Some("t1".into()),
                        activity: Some(AgentActivity::Working),
                    },
                    ProjectWorker {
                        name: "worker-2".into(),
                        task_id: Some("t2".into()),
                        activity: Some(AgentActivity::Waiting),
                    },
                ],
                waiting: vec![WaitingPane {
                    name: "worker-2".into(),
                    agent: AgentId::Claude,
                }],
                board: BoardCounts {
                    todo: 2,
                    doing: 2,
                    review: 1,
                    done: 2,
                },
                needs_you: tasks(&[("t5", "Pick a payment provider")]),
                ready_to_land: tasks(&[("t3", "Signup")]),
                plan_status: Some("T1 and T2 running.".into()),
                ..summary(2)
            },
            ProjectSummary {
                name: "Done".into(),
                board: BoardCounts {
                    done: 4,
                    ..BoardCounts::default()
                },
                all_done: true,
                ..summary(3)
            },
        ],
        now(),
    );
    assert_eq!(
        text,
        [
            "# Hangar projects at 18:30",
            "3 projects: 1 with something for the person (Shop), 0 at work, 2 quiet.",
            "",
            "Shop — C:\\dev\\project-2 · open · git repository",
            "  Orchestrator \"orch\" working · 2 workers: 1 working, 1 waiting for the person · board: 2 todo, 2 doing, 1 review, 2 done.",
            "  Needs you: \"Pick a payment provider\" [t5].",
            "  Ready to land: \"Signup\" [t3].",
            "  Waiting for the person: \"worker-2\" (claude).",
            "  Plan status: T1 and T2 running.",
            "",
            "Quiet — C:\\dev\\project-1 · not open · no git repository · no orchestrator · board: empty.",
            "",
            "Done — C:\\dev\\project-3 · open · git repository · no orchestrator · board: all 4 tasks done.",
        ]
        .join("\n")
    );
}

#[test]
fn fits_twenty_typical_projects_whole() {
    let projects: Vec<_> = (0..20).map(typical).collect();
    let text = render_overview(&projects, now());
    assert!(text.chars().count() <= 6_000);
    assert!(!text.contains("not shown"));
    for n in 0..20 {
        assert!(text.contains(&format!("Project {n} — ")));
    }
}

#[test]
fn stays_under_its_cap_with_many_busy_projects_and_says_how_many_it_left_out() {
    let mut projects: Vec<_> = (0..40).map(busy).collect();
    projects.push(ProjectSummary {
        name: "Idle one".into(),
        ..summary(99)
    });
    let text = render_overview(&projects, now());
    assert!(text.chars().count() <= 6_000);
    assert!(
        text.contains("Shortest form, for room: tasks counted rather than named, no plan status.")
    );
    assert!(
        text.contains(
            "For the person: 5 tasks needing you, 4 tasks ready to land, 5 panes waiting."
        )
    );
    assert!(!text.contains("Plan status"));
    let left = regex::Regex::new(
        r"…and (\d+) more projects not shown, (\d+) of them with something for the person: (.*)\.$",
    )
    .unwrap();
    let caps = left
        .captures(&text)
        .expect("a line saying what was left out");
    let shown = regex::Regex::new(r"(?m)^Project \d+ — ")
        .unwrap()
        .find_iter(&text)
        .count();
    let left_out: usize = caps[1].parse().unwrap();
    assert!(shown > 0);
    assert_eq!(shown + left_out, projects.len());
    // The quiet one is left out before any busy one.
    assert_eq!(caps[2].parse::<usize>().unwrap(), left_out - 1);
    assert!(!text.contains("Idle one — "));
    assert!(
        regex::Regex::new(r"^Project \d+, ")
            .unwrap()
            .is_match(&caps[3])
    );
}

#[test]
fn names_fewer_tasks_before_it_leaves_a_project_out() {
    let overview = |n: usize| render_overview(&(0..n).map(typical).collect::<Vec<_>>(), now());
    let full = overview(12);
    assert!(full.contains("Plan status: T2 runs"));
    assert!(!full.contains("form, for room"));
    let short = overview(20);
    assert!(short.chars().count() <= 6_000);
    assert!(short.contains("Short form, for room"));
    assert!(!short.contains("not shown"));
    let counted = overview(28);
    assert!(counted.chars().count() <= 6_000);
    assert!(counted.contains("For the person: 1 task needing you."));
    assert!(!counted.contains("not shown"));
}

#[test]
fn says_so_when_there_is_no_project() {
    assert_eq!(
        render_overview(&[], now()),
        "# Hangar projects at 18:30\nNo project: Hangar has no workspace besides the HQ."
    );
}

/* ── renderProject ─────────────────────────────────────────────── */

#[tokio::test]
async fn gives_one_project_in_detail() {
    let projects = load_projects(&setup()).await;
    let text = render_project(by_name(&projects, "Shop"));
    for needed in [
        "Shop — C:\\dev\\shop · open · git repository",
        "Orchestrator \"orch\" working.",
        "Workers (2):\n- \"worker-1\" on \"Login form\" [t1]: working\n- \"worker-2\" on \"Payment page\" [t2]: waiting for the person",
        "Board: 2 todo, 2 doing, 1 review, 2 done.",
        "Doing:\n- \"Payment page\" [t2] · worker-2\n- \"Login form\" [t1] · worker-1",
        "Needs you:\n- \"Pick a payment provider\" [t5]\n- \"Pick a mailer\" [t4]",
        "Ready to land:\n- \"Signup\" [t3]",
        "Waiting for the person: \"worker-2\" (claude), \"notes\" (claude).",
        "Plan status: T1 and T2 running; T3 waits to land.",
        "Latest events, oldest first:\n- 18:25 worker-2 started on \"Login form\" [t1]",
    ] {
        assert!(text.contains(needed), "{needed}\n---\n{text}");
    }
}

#[tokio::test]
async fn says_what_a_quiet_project_lacks() {
    let projects = load_projects(&setup()).await;
    let text = render_project(by_name(&projects, "Wiki"));
    for needed in [
        "No orchestrator.",
        "Workers: none running.",
        "Board: all 1 task done.",
        "No pane waits for the person.",
        "Plan status: none found.",
    ] {
        assert!(text.contains(needed), "{needed}");
    }
    assert!(!text.contains("Latest events"));
}

#[test]
fn a_summary_reads_as_the_ts_wrote_it() {
    let json = serde_json::to_value(typical(1)).unwrap();
    assert_eq!(json["boardError"], serde_json::Value::Null);
    assert_eq!(json["orchestrator"]["activity"], "idle");
    assert_eq!(json["workers"][0]["taskId"], uuid(1, 1));
    assert_eq!(json["board"]["todo"], 3);
    assert!(json.get("readyToLand").is_some());
}
