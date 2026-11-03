//! The cases of `needsYou.test.ts`, with the same workspaces, boards and
//! expectations.

use serde_json::json;

use super::*;
use crate::state::types::{AgentId, Pane};

fn pane(id: &str, agent: AgentId, role: Option<PaneRole>, task_id: Option<&str>) -> Pane {
    Pane {
        id: id.into(),
        name: id.to_uppercase(),
        agent,
        role,
        task_id: task_id.map(str::to_string),
        ..Pane::default()
    }
}

fn plain(id: &str) -> Pane {
    pane(id, AgentId::Claude, None, None)
}

fn orchestrator(id: &str) -> Pane {
    pane(id, AgentId::Claude, Some(PaneRole::Orchestrator), None)
}

fn worker(id: &str, task_id: &str) -> Pane {
    pane(id, AgentId::Claude, Some(PaneRole::Worker), Some(task_id))
}

fn workspace(id: &str, panes: Vec<Pane>, cwd: Option<&str>) -> Workspace {
    Workspace {
        id: id.into(),
        name: id.into(),
        cwd: cwd.map_or_else(|| format!("C:\\dev\\{id}"), str::to_string),
        panes,
        ..Workspace::default()
    }
}

/// A task as the board file would hold it, with the fields the tests vary.
struct Spec {
    column: &'static str,
    priority: u8,
    labels: Vec<&'static str>,
    updated_at: u64,
}

impl Default for Spec {
    fn default() -> Self {
        Spec {
            column: "todo",
            priority: 1,
            labels: vec![],
            updated_at: 1_000,
        }
    }
}

fn task(id: &str, spec: Spec) -> Task {
    serde_json::from_value(json!({
        "id": id,
        "title": format!("Task {id}"),
        "description": "",
        "column": spec.column,
        "priority": spec.priority,
        "assignee": null,
        "labels": spec.labels,
        "comments": [],
        "depends_on": [],
        "created_at": 1_000,
        "updated_at": spec.updated_at,
        "order": 0.0,
    }))
    .unwrap()
}

fn labelled(id: &str, labels: &[&'static str]) -> Task {
    task(
        id,
        Spec {
            labels: labels.to_vec(),
            ..Spec::default()
        },
    )
}

struct World {
    workspaces: Vec<Workspace>,
    waiting: HashSet<String>,
    boards: Boards,
}

impl World {
    fn items(&self) -> Vec<NeedsYouItem> {
        let refs: Vec<&Workspace> = self.workspaces.iter().collect();
        needs_you_items(&refs, &self.waiting, &self.boards)
    }
}

/// One orchestrated workspace and one that is not, as most machines would be.
fn world(tasks: Vec<Task>, waiting: &[&str]) -> World {
    let orchestrated = workspace(
        "checkout",
        vec![
            orchestrator("iris"),
            worker("jade", "31"),
            pane("dan", AgentId::Shell, None, None),
        ],
        None,
    );
    let site = workspace("site", vec![plain("leo"), plain("eve")], None);
    let boards: Boards = [(orchestrated.cwd.clone(), tasks), (site.cwd.clone(), vec![])]
        .into_iter()
        .collect();
    World {
        workspaces: vec![orchestrated, site],
        waiting: waiting.iter().map(|id| id.to_string()).collect(),
        boards,
    }
}

fn kinds(items: &[NeedsYouItem]) -> Vec<Kind> {
    items.iter().map(NeedsYouItem::kind).collect()
}

fn task_ids(items: &[NeedsYouItem]) -> Vec<String> {
    items
        .iter()
        .filter_map(|item| item.task().map(|task| task.id.clone()))
        .collect()
}

// ---- needsYouItems ------------------------------------------------------------------------------

#[test]
fn is_empty_for_anyone_who_never_pinned_an_orchestrator_whatever_waits() {
    let site = workspace("site", vec![plain("leo"), plain("eve")], None);
    let boards: Boards = [(site.cwd.clone(), vec![labelled("1", &[NEEDS_YOU_LABEL])])]
        .into_iter()
        .collect();
    let waiting: HashSet<String> = ["leo".to_string()].into();
    assert!(needs_you_items(&[&site], &waiting, &boards).is_empty());
}

#[test]
fn lists_agents_waiting_for_an_answer_in_every_workspace_once_one_orchestrates() {
    let items = world(vec![], &["eve", "jade"]).items();
    let names: Vec<&str> = items
        .iter()
        .filter_map(|item| match item {
            NeedsYouItem::Waiting { pane_name, .. } => Some(pane_name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(names, ["JADE", "EVE"]);
    match &items[1] {
        NeedsYouItem::Waiting { base, pane_id, .. } => {
            assert_eq!(base.workspace_id, "site");
            assert_eq!(base.workspace_name, "site");
            assert_eq!(pane_id, "eve");
        }
        other => panic!("expected a waiting item, got {other:?}"),
    }
}

#[test]
fn lists_the_questions_and_the_deliveries_of_the_orchestrated_boards_nothing_else() {
    let items = world(
        vec![
            labelled("30", &[NEEDS_YOU_LABEL]),
            task(
                "33",
                Spec {
                    column: "review",
                    labels: vec![READY_TO_LAND_LABEL],
                    ..Spec::default()
                },
            ),
            task(
                "31",
                Spec {
                    column: "doing",
                    labels: vec!["bug"],
                    ..Spec::default()
                },
            ),
        ],
        &[],
    )
    .items();
    assert_eq!(kinds(&items), [Kind::Question, Kind::Land]);
    match &items[0] {
        NeedsYouItem::Question {
            orchestrator_id,
            cwd,
            ..
        } => {
            assert_eq!(orchestrator_id.as_deref(), Some("iris"));
            assert_eq!(cwd, "C:\\dev\\checkout");
        }
        other => panic!("expected a question, got {other:?}"),
    }
    assert_eq!(task_ids(&items), ["30", "33"]);
}

#[test]
fn puts_what_blocks_an_agent_first_then_the_questions_then_the_merges() {
    let items = world(
        vec![
            task(
                "33",
                Spec {
                    column: "review",
                    labels: vec![READY_TO_LAND_LABEL],
                    ..Spec::default()
                },
            ),
            labelled("30", &[NEEDS_YOU_LABEL]),
        ],
        &["eve"],
    )
    .items();
    assert_eq!(kinds(&items), [Kind::Waiting, Kind::Question, Kind::Land]);
}

#[test]
fn asks_the_most_urgent_question_first_and_lands_the_oldest_delivery_first() {
    let question = |id: &str, priority: u8, updated_at: u64| {
        task(
            id,
            Spec {
                labels: vec![NEEDS_YOU_LABEL],
                priority,
                updated_at,
                ..Spec::default()
            },
        )
    };
    let delivery = |id: &str, updated_at: u64| {
        task(
            id,
            Spec {
                column: "review",
                labels: vec![READY_TO_LAND_LABEL],
                updated_at,
                ..Spec::default()
            },
        )
    };
    let items = world(
        vec![
            question("a", 1, 5),
            question("b", 3, 1),
            question("c", 1, 9),
            delivery("new", 50),
            delivery("old", 10),
        ],
        &[],
    )
    .items();
    assert_eq!(task_ids(&items), ["b", "c", "a", "old", "new"]);
}

#[test]
fn leaves_done_tasks_out_whatever_labels_they_kept() {
    let done = |id: &str, label: &'static str| {
        task(
            id,
            Spec {
                column: "done",
                labels: vec![label],
                ..Spec::default()
            },
        )
    };
    let items = world(
        vec![done("1", NEEDS_YOU_LABEL), done("2", READY_TO_LAND_LABEL)],
        &[],
    )
    .items();
    assert!(items.is_empty());
}

#[test]
fn lists_a_task_labelled_both_ways_once_as_the_question_it_still_is() {
    let items = world(
        vec![task(
            "1",
            Spec {
                column: "review",
                labels: vec![READY_TO_LAND_LABEL, NEEDS_YOU_LABEL],
                ..Spec::default()
            },
        )],
        &[],
    )
    .items();
    assert_eq!(kinds(&items), [Kind::Question]);
}

#[test]
fn waits_for_a_board_that_is_still_being_read_rather_than_guessing() {
    let mut w = world(vec![], &[]);
    w.boards.clear();
    assert!(w.items().is_empty());
}

#[test]
fn reads_a_board_two_workspaces_share_once_under_the_first_one() {
    let a = workspace("a", vec![orchestrator("o1")], Some("C:\\dev\\shared"));
    let b = workspace("b", vec![worker("w1", "1")], Some("C:\\dev\\shared"));
    let boards: Boards = [(
        "C:\\dev\\shared".to_string(),
        vec![labelled("1", &[NEEDS_YOU_LABEL])],
    )]
    .into_iter()
    .collect();
    let items = needs_you_items(&[&a, &b], &HashSet::new(), &boards);
    let owners: Vec<&str> = items
        .iter()
        .map(|item| item.base().workspace_id.as_str())
        .collect();
    assert_eq!(owners, ["a"]);
}

#[test]
fn names_no_orchestrator_to_answer_when_a_worker_is_all_that_is_left() {
    let left = workspace("left", vec![worker("w", "1")], None);
    let boards: Boards = [(left.cwd.clone(), vec![labelled("1", &[NEEDS_YOU_LABEL])])]
        .into_iter()
        .collect();
    let items = needs_you_items(&[&left], &HashSet::new(), &boards);
    match &items[0] {
        NeedsYouItem::Question {
            orchestrator_id, ..
        } => assert_eq!(*orchestrator_id, None),
        other => panic!("expected a question, got {other:?}"),
    }
}

#[test]
fn keys_each_item_by_what_it_stands_for_the_same_from_one_read_to_the_next() {
    let tasks = vec![labelled("30", &[NEEDS_YOU_LABEL])];
    let first = world(tasks.clone(), &["eve"]).items();
    let again = world(tasks, &["eve"]).items();
    let keys = |items: &[NeedsYouItem]| -> Vec<String> {
        items.iter().map(|item| item.key().to_string()).collect()
    };
    assert_eq!(keys(&again), keys(&first));
    let unique: HashSet<String> = keys(&first).into_iter().collect();
    assert_eq!(unique.len(), first.len());
}

// ---- watchedBoards ------------------------------------------------------------------------------

#[test]
fn reads_only_the_boards_of_the_workspaces_that_orchestrate_once_each() {
    let a = workspace("a", vec![orchestrator("o1")], Some("C:\\dev\\shared"));
    let b = workspace("b", vec![worker("w1", "1")], Some("C:\\dev\\shared"));
    let c = workspace("c", vec![plain("x")], None);
    let d = workspace("d", vec![orchestrator("o2")], None);
    assert_eq!(
        watched_boards(&[&a, &b, &c, &d]),
        ["C:\\dev\\shared", "C:\\dev\\d"]
    );
}

// ---- latestNote ---------------------------------------------------------------------------------

#[test]
fn says_what_the_last_comment_says_on_one_line() {
    let mut t = labelled("1", &[]);
    t.comments = serde_json::from_value(json!([
        {"id": "1", "author": "Iris", "text": "First thought", "created_at": 1},
        {"id": "2", "author": "Iris", "text": "\n  Stripe or Adyen?\nDetails follow.", "created_at": 2},
    ]))
    .unwrap();
    assert_eq!(latest_note(&t).as_deref(), Some("Stripe or Adyen?"));
    t.comments.clear();
    assert_eq!(latest_note(&t), None);
}

// ---- noticeDelta ----------------------------------------------------------------------------------

const CWD: &str = "C:\\dev\\checkout";

fn question(id: &str) -> Task {
    labelled(id, &[NEEDS_YOU_LABEL])
}

fn delivery(id: &str) -> Task {
    task(
        id,
        Spec {
            column: "review",
            labels: vec![READY_TO_LAND_LABEL],
            ..Spec::default()
        },
    )
}

fn loaded() -> HashSet<String> {
    [CWD.to_string()].into()
}

#[test]
fn says_nothing_about_what_was_already_waiting_when_a_board_is_first_read() {
    let items = world(vec![question("1")], &[]).items();
    let (memory, fresh) = notice_delta(&NoticeMemory::default(), &items, &loaded());
    assert!(fresh.is_empty());
    assert_eq!(memory.primed, loaded());
}

#[test]
fn tells_of_a_question_or_a_delivery_that_appears_once_the_board_is_known() {
    let mut memory = notice_delta(&NoticeMemory::default(), &world(vec![], &[]).items(), &loaded()).0;
    let both = world(vec![question("1"), delivery("2")], &[]).items();
    let (next, fresh) = notice_delta(&memory, &both, &loaded());
    assert_eq!(kinds(&fresh), [Kind::Question, Kind::Land]);
    memory = next;
    // Read again, unchanged: no news.
    let again = world(vec![question("1"), delivery("2")], &[]).items();
    assert!(notice_delta(&memory, &again, &loaded()).1.is_empty());
}

#[test]
fn tells_again_of_an_item_that_left_and_came_back() {
    let mut memory = notice_delta(&NoticeMemory::default(), &world(vec![], &[]).items(), &loaded()).0;
    memory = notice_delta(&memory, &world(vec![question("1")], &[]).items(), &loaded()).0;
    memory = notice_delta(&memory, &world(vec![], &[]).items(), &loaded()).0;
    let (_, fresh) = notice_delta(&memory, &world(vec![question("1")], &[]).items(), &loaded());
    assert_eq!(fresh.len(), 1);
}

#[test]
fn leaves_the_agents_waiting_in_a_dialog_to_their_own_panes_notification() {
    let memory = notice_delta(&NoticeMemory::default(), &world(vec![], &[]).items(), &loaded()).0;
    let (_, fresh) = notice_delta(&memory, &world(vec![], &["eve"]).items(), &loaded());
    assert!(fresh.is_empty());
}
