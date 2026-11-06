//! The tasks panel headless (`ICED_TEST_BACKEND=tiny-skia`): the global
//! progress and the rows from a fixture board, with and without reported
//! progress, the section of what waits for the person, the empty board, and
//! the panel kept between frames while nothing it reads changes.

use std::sync::Arc;

use hangar_core::board::{Blocked, Comment, Progress, Task};

use super::model::{Overall, Waiting, groups, overall, panel_of};
use super::*;
use crate::app::Packages;
use crate::board::Column;
use crate::board::format::now_ms;
use crate::board::model::test_task;
use crate::navigator::testing::{ctx, model_of, pane, simulate, workspace};
use crate::state::AgentActivity;
use crate::state::roles::{NEEDS_YOU_LABEL, READY_TO_LAND_LABEL};
use crate::state::types::{AgentId, AppState, PaneRole};

fn task(id: &str, title: &str, column: &str, order: f64) -> Task {
    Task {
        title: title.into(),
        column: column.into(),
        order,
        ..test_task(id)
    }
}

fn reported(percent: u8, note: &str, at: u64) -> Option<Progress> {
    Some(Progress {
        percent,
        note: note.into(),
        at,
        by: Some("Finn".into()),
    })
}

/// The demo's board: one task in each column, none reported on.
fn plain_board() -> Vec<Task> {
    vec![
        task("t-done", "Recipe model", "done", 1.0),
        Task {
            assignee: Some("Finn".into()),
            ..task("t-doing", "Search page", "doing", 2.0)
        },
        Task {
            assignee: Some("Mia".into()),
            ..task("t-review", "Import from a URL", "review", 3.0)
        },
        task("t-todo", "Shopping list", "todo", 4.0),
    ]
}

/// The same board as its agents reported on it: done at 100, doing at 60
/// three minutes ago, review at 95; the todo task blocked.
fn reported_board() -> Vec<Task> {
    let mut tasks = plain_board();
    let three_minutes_ago = now_ms() - 3 * 60_000 - 5_000;
    tasks[0].progress = reported(100, "", 1);
    tasks[1].progress = reported(60, "index built", three_minutes_ago);
    tasks[1].description = "Full-text search over the recipes.".into();
    tasks[1].comments = vec![Comment {
        id: "c1".into(),
        author: "Leo".into(),
        text: "Keep the old filters.".into(),
        created_at: 1,
    }];
    tasks[2].progress = reported(95, "tests pass", three_minutes_ago);
    tasks[3].blocked = Some(Blocked {
        reason: "npm release".into(),
        by: "user".into(),
        since: 1,
        recheck: None,
        human_only: false,
        reminded: None,
    });
    tasks
}

/// A workspace with an orchestrator, Leo, and a worker, Finn; its board held
/// in the cache as `tasks`, and shown as the chat view.
fn shop(tasks: Option<Vec<Task>>) -> Model {
    let mut leo = pane("leo", "Leo", AgentId::Shell);
    leo.role = Some(PaneRole::Orchestrator);
    let mut finn = pane("finn", "Finn", AgentId::Shell);
    finn.role = Some(PaneRole::Worker);
    let mut m = model_of(AppState {
        workspaces: vec![workspace("shop", "Shop", None, vec![leo, finn])],
        active_workspace_id: Some("shop".into()),
        ..AppState::default()
    });
    if let Some(tasks) = tasks {
        let cwd = m.workspace("shop").unwrap().cwd.clone();
        m.live.board.hold(&cwd, tasks);
    }
    m
}

/// The texts the panel shows, among `wanted`.
fn on_screen(m: &Model, pkg: &Packages, wanted: &[&str]) -> Vec<String> {
    let ws = m.workspace("shop").unwrap();
    let ctx = ctx(m, pkg);
    let mut ui = simulate(view(&pkg.chat_view.tasks, ctx, ws));
    let _ = ui
        .snapshot(&hangar_theme::to_iced(ctx.theme))
        .expect("it draws");
    wanted
        .iter()
        .filter(|text| ui.find(**text).is_ok())
        .map(|text| text.to_string())
        .collect()
}

/// The messages a click on `label` in the panel sends.
fn click(m: &Model, pkg: &Packages, label: &str) -> Vec<Message> {
    let ws = m.workspace("shop").unwrap();
    let mut ui = simulate(view(&pkg.chat_view.tasks, ctx(m, pkg), ws));
    ui.click(label).expect("it is on screen");
    ui.into_messages().collect()
}

#[test]
fn the_global_progress_without_reports_counts_only_what_is_done() {
    assert_eq!(
        overall(&plain_board()),
        Overall {
            percent: 25,
            done: 1,
            total: 4
        }
    );
    assert_eq!(overall(&[]), Overall::default());
}

#[test]
fn the_global_progress_is_the_mean_of_the_reported_percents() {
    // (100 + 60 + 95 + 0) / 4 = 63.75.
    assert_eq!(
        overall(&reported_board()),
        Overall {
            percent: 64,
            done: 1,
            total: 4
        }
    );
    // A task in a column this version does not know counts for nothing.
    let mut tasks = reported_board();
    tasks.push(task("t-odd", "Odd", "later", 5.0));
    assert_eq!(overall(&tasks).total, 4);
}

#[test]
fn the_rows_go_by_column_in_the_docks_order_with_what_their_agent_said() {
    let groups = groups(&reported_board());
    let columns: Vec<Column> = groups.iter().map(|group| group.column).collect();
    assert_eq!(columns, Column::DOCK_ORDER);
    let doing = &groups[0].rows[0];
    assert_eq!(doing.title, "Search page");
    assert_eq!(doing.assignee.as_deref(), Some("Finn"));
    let progress = doing.progress.as_ref().unwrap();
    assert_eq!(
        (progress.percent, progress.note.as_str()),
        (60, "index built")
    );
    assert_eq!(
        doing.last_comment.as_ref().unwrap().text,
        "Keep the old filters."
    );
    assert_eq!(groups[2].rows[0].blocked.as_deref(), Some("npm release"));
    assert!(groups[2].rows[0].progress.is_none());

    // Without reports: no bar to draw, the columns all the same.
    let plain = super::model::groups(&plain_board());
    assert!(plain.iter().all(|group| group.rows[0].progress.is_none()));

    // In a column, the most urgent first, then in the order they came.
    let mut tasks = vec![
        task("a", "A", "todo", 1.0),
        task("b", "B", "todo", 2.0),
        Task {
            priority: 3,
            ..task("c", "C", "todo", 3.0)
        },
    ];
    tasks.reverse();
    let ids: Vec<String> = super::model::groups(&tasks)[0]
        .rows
        .iter()
        .map(|row| row.id.clone())
        .collect();
    assert_eq!(ids, ["c", "a", "b"]);
}

#[test]
fn the_panel_draws_the_progress_the_rows_and_a_blocked_task() {
    let m = shop(Some(reported_board()));
    let pkg = Packages::default();
    let wanted = [
        "Tasks",
        "64 %",
        "Done 1/4",
        "IN PROGRESS",
        "Search page",
        "Finn · In progress",
        "60 %",
        "index built · 3\u{a0}min ago",
        "Unassigned · To do",
        "Blocked: npm release",
        "Full-text search over the recipes.",
    ];
    let shown = on_screen(&m, &pkg, &wanted);
    assert_eq!(shown, &wanted[..10], "closed rows: no description");
}

#[test]
fn a_board_without_reports_draws_no_bar_on_its_rows() {
    let m = shop(Some(plain_board()));
    let pkg = Packages::default();
    assert_eq!(
        on_screen(
            &m,
            &pkg,
            &["25 %", "Done 1/4", "Search page", "0 %", "60 %"]
        ),
        ["25 %", "Done 1/4", "Search page"]
    );
}

#[test]
fn a_click_on_a_row_opens_its_description_and_last_comment() {
    let mut m = shop(Some(reported_board()));
    let mut pkg = Packages::default();
    let asked: Vec<String> = click(&m, &pkg, "Search page")
        .into_iter()
        .filter_map(|message| match message {
            Message::ChatView(crate::chat_view::Msg::Tasks(Msg::Toggle(task))) => Some(task),
            _ => None,
        })
        .collect();
    assert_eq!(asked, ["t-doing"]);

    let _ = update(
        &mut pkg.chat_view.tasks,
        &mut m,
        Msg::Toggle("t-doing".into()),
    );
    let wanted = [
        "Full-text search over the recipes.",
        "Leo",
        "Keep the old filters.",
    ];
    assert_eq!(on_screen(&m, &pkg, &wanted), wanted);

    let _ = update(
        &mut pkg.chat_view.tasks,
        &mut m,
        Msg::Toggle("t-doing".into()),
    );
    assert!(on_screen(&m, &pkg, &wanted).is_empty(), "closed again");
}

#[test]
fn what_waits_for_the_person_comes_first_with_its_action() {
    let mut tasks = reported_board();
    tasks[2].labels = vec![READY_TO_LAND_LABEL.into()];
    tasks[1].labels = vec![NEEDS_YOU_LABEL.into()];
    tasks[1].comments.push(Comment {
        id: "c2".into(),
        author: "Leo".into(),
        text: "Which index, sqlite or tantivy?".into(),
        created_at: 2,
    });
    let mut m = shop(Some(tasks));
    m.live
        .panes_logic
        .seed_activity("finn", "shop", AgentActivity::Waiting);
    let pkg = Packages::default();

    let ws = m.workspace("shop").unwrap();
    let panel = panel_of(
        ws,
        m.live.board.tasks(&ws.cwd),
        &super::model::waiting_panes(&m, ws),
    );
    let kinds: Vec<&str> = panel
        .waiting
        .iter()
        .map(|item| match item {
            Waiting::Pane { name, .. } => name.as_str(),
            Waiting::Question { title, .. } => title.as_str(),
            Waiting::Land { task, .. } => task.as_str(),
        })
        .collect();
    assert_eq!(kinds, ["Finn", "Search page", "t-review"]);

    let wanted = [
        "NEEDS YOU",
        "Finn",
        "waits for your answer",
        "Which index, sqlite or tantivy?",
        "Answer",
        "Merge",
    ];
    assert_eq!(on_screen(&m, &pkg, &wanted), wanted);

    // « Merge » lands through the needs-you package, as its own list does.
    let landed: Vec<(String, String)> = click(&m, &pkg, "Merge")
        .into_iter()
        .filter_map(|message| match message {
            Message::NeedsYou(crate::needs_you::Msg::Land(request)) => {
                Some((request.ws, request.task))
            }
            _ => None,
        })
        .collect();
    assert_eq!(landed, [("shop".to_string(), "t-review".to_string())]);

    // « Show » goes to the waiting pane, « Answer » to the orchestrator.
    let goes = |label: &str| -> Vec<(String, String)> {
        click(&m, &pkg, label)
            .into_iter()
            .filter_map(|message| match message {
                Message::ChatView(crate::chat_view::Msg::Tasks(Msg::GoTo { ws, pane })) => {
                    Some((ws, pane))
                }
                _ => None,
            })
            .collect()
    };
    assert_eq!(goes("Show"), [("shop".to_string(), "finn".to_string())]);
    assert_eq!(goes("Answer"), [("shop".to_string(), "leo".to_string())]);

    // Going there shows the panes again: the pane lives in the grid.
    let mut st = State::default();
    m.apply(StoreAction::SetChatView {
        ws: "shop".into(),
        on: true,
    });
    let _ = update(
        &mut st,
        &mut m,
        Msg::GoTo {
            ws: "shop".into(),
            pane: "finn".into(),
        },
    );
    assert_eq!(m.workspace("shop").unwrap().chat_view, None);
}

#[test]
fn nothing_waiting_means_no_section() {
    let m = shop(Some(reported_board()));
    let pkg = Packages::default();
    assert!(on_screen(&m, &pkg, &["NEEDS YOU", "Merge", "Answer"]).is_empty());
}

#[test]
fn a_board_without_a_task_says_so() {
    let m = shop(Some(Vec::new()));
    let pkg = Packages::default();
    assert_eq!(
        on_screen(
            &m,
            &pkg,
            &["Tasks", "No task on this board yet", "0 %", "NEEDS YOU"]
        ),
        ["Tasks", "No task on this board yet"]
    );

    // A board not read yet is not an empty one.
    let m = shop(None);
    assert_eq!(
        on_screen(
            &m,
            &pkg,
            &["No task on this board yet", "Reading the board…"]
        ),
        ["Reading the board…"]
    );
}

#[test]
fn the_panel_is_worked_out_again_only_when_what_it_reads_changes() {
    let mut m = shop(Some(plain_board()));
    let cache = super::model::Cache::default();
    let ws = m.workspace("shop").unwrap().clone();
    let first = cache.panel(&m, &ws);
    assert!(Arc::ptr_eq(&first, &cache.panel(&m, &ws)), "a frame later");

    // A pane that starts waiting.
    m.live
        .panes_logic
        .seed_activity("finn", "shop", AgentActivity::Waiting);
    let waiting = cache.panel(&m, &ws);
    assert!(!Arc::ptr_eq(&first, &waiting));
    assert_eq!(waiting.waiting.len(), 1);

    // A new read of the board.
    m.live.board.hold(&ws.cwd, reported_board());
    let read = cache.panel(&m, &ws);
    assert!(!Arc::ptr_eq(&waiting, &read));
    assert_eq!(read.overall.percent, 64);
    assert!(Arc::ptr_eq(&read, &cache.panel(&m, &ws)));
}
