//! Pictures of the board without a window, for whoever builds it and cannot
//! capture the screen, and a check that every state of it lays out.
//!
//! `HANGAR_BOARD_SNAPSHOTS=/some/folder cargo test -p hangar-app board::snapshot`
//! writes one PNG per scene, in each language and a dark and a light theme
//! (the folder is cleared of the old ones first). Without the variable the
//! scenes are drawn and thrown away, which is the check.

use std::borrow::Cow;
use std::sync::Arc;
use std::time::Instant;

use hangar_core::board::{Blocked, Comment};
use hangar_i18n::{Locale, Translator};
use iced::{Element, Settings, Size, window};
use iced_test::Simulator;

use crate::app::Packages;
use crate::ctx::{Ctx, Metrics};
use crate::message::Message;
use crate::shell::BoardVariant;
use crate::state::{AgentId, AppState, Model, Pane, Workspace};

use super::State;
use super::drag::Press;
use super::format::now_ms;
use super::model::{Column, Task, test_task};
use super::task_dialog::Dialog;

pub(super) const CWD: &str = "/work/hangar";

fn pane(id: &str, name: &str, agent: AgentId) -> Pane {
    Pane {
        id: id.into(),
        name: name.into(),
        agent,
        ..Pane::default()
    }
}

pub(super) fn tasks() -> Vec<Task> {
    let now = now_ms();
    let task = |id: &str, title: &str, column: &str, edit: &dyn Fn(&mut Task)| {
        let mut task = test_task(id);
        task.id = format!("{id}0000-aaaa-bbbb-cccc-dddddddddddd");
        task.title = title.into();
        task.column = column.into();
        task.created_at = now - 86_400_000;
        task.order = 1.0;
        edit(&mut task);
        task
    };
    let release = task("5b3c", "Release 1.13", "todo", &|t| {
        t.blocked = Some(Blocked {
            reason: "Waiting for the npm release".into(),
            by: "user".into(),
            since: now - 3_600_000,
            recheck: Some("2027-01-10".into()),
            human_only: true,
            reminded: None,
        });
    });
    vec![
        task("1a2b", "Wire the board into the dock", "todo", &|t| {
            t.priority = 2;
            t.labels = vec!["ui".into(), "board".into()];
        }),
        task("3d4e", "Port the task dialog", "doing", &|t| {
            t.assignee = Some("Ava (Hangar)".into());
            t.doing_since = Some(now - 72 * 60_000);
            t.description = "Edit, block, thread, delete.".into();
            t.owns = vec!["crates/hangar-app/src/board/**".into()];
            t.comments = vec![
                Comment {
                    id: "c1".into(),
                    author: "Ava".into(),
                    text: "Started with the cache.".into(),
                    created_at: now - 3_000_000,
                },
                Comment {
                    id: "c2".into(),
                    author: "user".into(),
                    text: "Keep the drag as it was.".into(),
                    created_at: now - 1_000_000,
                },
            ];
        }),
        task("6f7a", "Review the boards cache", "review", &|t| {
            t.assignee = Some("Ben".into());
            t.priority = 0;
        }),
        task("8b9c", "Write the docs", "done", &|t| {
            t.assignee = Some("Ava".into());
            t.labels = vec!["docs".into()];
        }),
        release.clone(),
        task("7d8e", "Ship once the release is out", "todo", &|t| {
            t.depends_on = vec![release.id.clone(), "gone-0000".into()];
        }),
        task(
            "9e0f",
            "Make the very long title of this one wrap over several lines instead of running out of its card",
            "doing",
            &|t| {
                t.assignee = Some("some-bot".into());
                t.priority = 3;
                t.doing_since = Some(now - 5 * 3_600_000 - 7 * 60_000);
            },
        ),
    ]
}

pub(super) fn model(locale: Locale, theme: &str, loaded: Option<Vec<Task>>) -> Model {
    let mut state = AppState::default();
    state.settings.locale = Some(locale);
    state.settings.theme_id = theme.into();
    state.workspaces = vec![Workspace {
        id: "ws".into(),
        name: "Hangar".into(),
        cwd: CWD.into(),
        panes: vec![
            pane("p1", "Ava", AgentId::Claude),
            pane("p2", "Ben", AgentId::Codex),
        ],
        ..Workspace::default()
    }];
    state.active_workspace_id = Some("ws".into());
    let mut m = Model::default();
    m.hydrate(state);
    m.live.board.follow(&[CWD.to_owned()]);
    if let Some(tasks) = loaded {
        m.live.board.request(CWD);
        m.live.board.answer(CWD, Some(Arc::new(tasks)));
    }
    m
}

/// The folder of the second workspace of [`two_windows`].
pub(super) const SITE: &str = "/work/site";

/// Two workspaces in two windows, as after a detach: `ws` (on `CWD`) in the
/// main window, where it is the active workspace, and `site` (on `SITE`) in a
/// window of its own, which has the keyboard. Both boards are read, and the
/// site's holds one task.
pub(super) fn two_windows() -> (Model, window::Id, window::Id) {
    use crate::shell::WindowKind;
    let mut m = model(Locale::En, "graphite", Some(tasks()));
    let mut state = m.state().clone();
    state.workspaces.push(Workspace {
        id: "site".into(),
        name: "Site".into(),
        cwd: SITE.into(),
        detached: Some(true),
        panes: vec![pane("s1", "Zed", AgentId::Claude)],
        ..Workspace::default()
    });
    m.hydrate(state);
    m.live.board.follow(&[CWD.to_owned(), SITE.to_owned()]);
    m.live.board.request(SITE);
    let mut one = test_task("site-task");
    one.title = "Fix the footer".into();
    m.live.board.answer(SITE, Some(Arc::new(vec![one])));
    let (main, site) = (window::Id::unique(), window::Id::unique());
    m.live.windows.insert(main, WindowKind::Main);
    m.live
        .windows
        .insert(site, WindowKind::Workspace("site".into()));
    m.ui.focused_window = Some(site);
    (m, main, site)
}

fn ctx<'a>(m: &'a Model, packages: &'a Packages) -> Ctx<'a> {
    Ctx {
        model: m,
        pkg: packages,
        i18n: Translator::new(m.locale()),
        theme: m.theme(),
        metrics: Metrics::default(),
        keymap: &m.live.keymap.index,
        window: window::Id::unique(),
        now: Instant::now(),
    }
}

/// Draws one scene, and writes it when asked to.
fn draw<'a>(
    name: &str,
    m: &'a Model,
    size: Size,
    build: impl FnOnce(Ctx<'a>) -> Element<'a, Message>,
) {
    // Leaked: a view borrows the packages' states for as long as it lives,
    // and a test is short.
    let packages: &'a Packages = Box::leak(Box::default());
    let element = build(ctx(m, packages));
    let settings = Settings {
        fonts: vec![Cow::Borrowed(hangar_theme::fonts::INTER_VARIABLE)],
        default_font: hangar_theme::fonts::ui(),
        ..Settings::default()
    };
    let mut ui = Simulator::with_size(settings, size, element);
    let snapshot = ui
        .snapshot(&hangar_theme::to_iced(m.theme()))
        .expect("the board can be drawn");
    let Some(folder) = std::env::var_os("HANGAR_BOARD_SNAPSHOTS") else {
        return;
    };
    let folder = std::path::PathBuf::from(folder);
    let stem = format!("{name}-{}-{}", m.theme().id, m.locale().id());
    if let Ok(entries) = std::fs::read_dir(&folder) {
        for entry in entries.flatten() {
            let file = entry.file_name().to_string_lossy().into_owned();
            if file.starts_with(&format!("{stem}-")) && file.ends_with(".png") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    snapshot
        .matches_image(folder.join(stem))
        .expect("the picture can be written");
}

/// Each scene in English and French, a dark theme and a light one.
fn each(scene: impl Fn(&Model)) {
    for (locale, theme) in [
        (Locale::En, "graphite"),
        (Locale::Fr, "graphite"),
        (Locale::En, "github-light"),
    ] {
        let m = model(locale, theme, Some(tasks()));
        scene(&m);
    }
}

fn full<'a>(m: &'a Model, st: &'a State, name: &str, size: Size) {
    draw(name, m, size, |ctx| {
        super::view(st, ctx, BoardVariant::Full)
    });
}

#[test]
fn the_full_board_in_each_language_and_theme() {
    each(|m| full(m, &State::default(), "full", Size::new(1200.0, 760.0)));
}

#[test]
fn the_full_board_stacks_when_it_is_narrow() {
    each(|m| full(m, &State::default(), "stacked", Size::new(560.0, 760.0)));
}

#[test]
fn an_empty_board_says_what_it_is_for() {
    let m = model(Locale::En, "graphite", Some(Vec::new()));
    let st = State::default();
    full(&m, &st, "empty", Size::new(1000.0, 600.0));
    draw("empty-dock", &m, Size::new(420.0, 600.0), |ctx| {
        super::view(
            &st,
            ctx,
            BoardVariant::Dock {
                width: 420.0,
                drawer: false,
            },
        )
    });
}

#[test]
fn a_board_still_being_read_is_an_outline() {
    let m = model(Locale::En, "graphite", None);
    full(&m, &State::default(), "skeleton", Size::new(1000.0, 600.0));
}

#[test]
fn the_dock_folds_its_sections() {
    each(|m| {
        let st = State::default();
        draw("dock", m, Size::new(420.0, 760.0), |ctx| {
            super::view(
                &st,
                ctx,
                BoardVariant::Dock {
                    width: 420.0,
                    drawer: false,
                },
            )
        });
    });
}

#[test]
fn the_dock_with_its_filters_open() {
    let m = model(Locale::En, "graphite", Some(tasks()));
    let st = State {
        filter_bar: true,
        folded: Vec::new(),
        filters: super::select::Filters {
            label: "ui".into(),
            ..Default::default()
        },
        ..State::default()
    };
    draw("dock-filters", &m, Size::new(420.0, 900.0), |ctx| {
        super::view(
            &st,
            ctx,
            BoardVariant::Dock {
                width: 420.0,
                drawer: false,
            },
        )
    });
}

#[test]
fn a_filter_menu_and_a_card_being_dragged() {
    let m = model(Locale::En, "graphite", Some(tasks()));
    let mut press = Press::new(
        "3d4e0000-aaaa-bbbb-cccc-dddddddddddd".to_owned(),
        Column::Doing,
        iced::Point::new(10.0, 10.0),
    )
    .on(CWD);
    press.pointer_moved(iced::Point::new(60.0, 10.0));
    press.enter(Column::Review);
    let st = State {
        press: Some(press),
        open_filter: Some(super::FilterKind::Assignee),
        adding: Some((CWD.to_owned(), Column::Todo)),
        drafts: [((CWD.to_owned(), Column::Todo), "Draft of a card".to_owned())].into(),
        toast: Some((1, "“Release 1.13” is blocked and stays in To do.".into())),
        ..State::default()
    };
    full(&m, &st, "dragging", Size::new(1200.0, 760.0));
}

#[test]
fn the_task_dialog_edits_a_task() {
    each(|m| dialog(m, "3d4e0000-aaaa-bbbb-cccc-dddddddddddd", "dialog"));
}

#[test]
fn the_task_dialog_shows_a_block_and_the_dependencies() {
    let m = model(Locale::En, "graphite", Some(tasks()));
    dialog(&m, "5b3c0000-aaaa-bbbb-cccc-dddddddddddd", "dialog-blocked");
    dialog(&m, "7d8e0000-aaaa-bbbb-cccc-dddddddddddd", "dialog-deps");
}

#[test]
fn the_task_dialog_writes_a_new_task() {
    let m = model(Locale::En, "graphite", Some(tasks()));
    let mut st = State::default();
    let mut open = Dialog::open(CWD, None, m.live.board.tasks(CWD).unwrap_or(&[]));
    open.seed(Column::Doing, "A card typed in the board".into());
    st.dialog = Some(open);
    draw("dialog-new", &m, Size::new(900.0, 800.0), |ctx| {
        super::task_view(&st, ctx, CWD, None)
    });
}

fn dialog<'a>(m: &'a Model, id: &'a str, name: &str) {
    let st = State {
        dialog: Some(Dialog::open(
            CWD,
            Some(id),
            m.live.board.tasks(CWD).unwrap_or(&[]),
        )),
        ..State::default()
    };
    draw(name, m, Size::new(900.0, 800.0), |ctx| {
        super::task_view(&st, ctx, CWD, Some(id))
    });
}

#[test]
fn the_views_are_the_ones_the_shell_calls() {
    // The shell's own slot for a task asks for a title that is not in the
    // cache: the dialog draws nothing rather than a half task.
    let m = model(Locale::En, "graphite", Some(tasks()));
    let st = State::default();
    draw("no-dialog", &m, Size::new(400.0, 300.0), |ctx| {
        super::task_view(&st, ctx, CWD, Some("missing"))
    });
}
