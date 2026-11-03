//! Where the rows are, and when the list has to scroll to one: checked against
//! the arithmetic, and against the layout the view really makes.

use super::*;
use crate::navigator::model::{NavInput, build_nav_tree};
use crate::navigator::testing::{folder, model_of, workspace};
use crate::state::Model;
use crate::state::types::{AppState, Workspace, WorkspaceFolder};

struct World {
    workspaces: Vec<Workspace>,
    folders: Vec<WorkspaceFolder>,
}

impl World {
    fn projects(&self) -> Vec<&Workspace> {
        self.workspaces.iter().collect()
    }
}

/// Loose: a, b. In "work" (open): c, d. "perso" is folded over e, "empty"
/// holds nothing, and "last" is folded.
fn world() -> World {
    let ws =
        |id: &str, folder: Option<&str>| workspace(id, &format!("Project {id}"), folder, vec![]);
    World {
        workspaces: vec![
            ws("a", None),
            ws("c", Some("work")),
            ws("b", None),
            ws("d", Some("work")),
            ws("e", Some("perso")),
        ],
        folders: vec![
            folder("work", "Work", false),
            folder("perso", "Perso", true),
            folder("empty", "Empty", false),
            folder("last", "Last", true),
        ],
    }
}

fn tree_of(world: &World) -> crate::navigator::model::NavTree<'_> {
    build_nav_tree(&NavInput {
        workspaces: &world.projects(),
        folders: &world.folders,
        query: "",
        attention_only: false,
        wants_you: &|_| false,
    })
}

fn model_of_world(world: &World) -> Model {
    model_of(AppState {
        workspaces: world.workspaces.clone(),
        folders: world.folders.clone(),
        ..AppState::default()
    })
}

fn span<'a>(spans: &'a [Span], key: &str) -> &'a Span {
    spans
        .iter()
        .find(|span| span.key == key)
        .unwrap_or_else(|| panic!("no row {key}"))
}

#[test]
fn rows_follow_one_another_with_a_pixel_between() {
    let world = world();
    let tree = tree_of(&world);
    let spans = spans(&tree);
    // Loose: a, b.
    assert_eq!(span(&spans, "ws:a").top, LIST_PAD_TOP);
    assert_eq!(span(&spans, "ws:a").bottom, LIST_PAD_TOP + ROW_HEIGHT);
    assert_eq!(span(&spans, "ws:b").top, LIST_PAD_TOP + ROW_HEIGHT + GAP);
}

#[test]
fn a_folders_heading_has_its_gap_above_and_its_rows_below() {
    let world = world();
    let tree = tree_of(&world);
    let spans = spans(&tree);
    let after_loose = span(&spans, "ws:b").bottom;
    let head = span(&spans, "folder:work");
    // The column's spacing, the folder's own gap, and the spacing under it.
    assert_eq!(head.top, after_loose + GAP + FOLDER_GAP + GAP);
    assert_eq!(head.bottom, head.top + ROW_HEIGHT);
    assert_eq!(span(&spans, "ws:c").top, head.bottom + GAP);
    assert_eq!(span(&spans, "ws:d").top, span(&spans, "ws:c").bottom + GAP);
}

#[test]
fn a_folded_folder_has_no_rows_under_its_heading() {
    let world = world();
    let tree = tree_of(&world);
    let spans = spans(&tree);
    assert!(spans.iter().all(|span| span.key != "ws:e"));
    let perso = span(&spans, "folder:perso");
    // After the last row of the open folder and its padding.
    let after_work = span(&spans, "ws:d").bottom + BODY_PAD;
    assert_eq!(perso.top, after_work + GAP + FOLDER_GAP + GAP);
}

#[test]
fn an_empty_folder_leaves_room_for_its_placeholder() {
    let world = world();
    let tree = tree_of(&world);
    let spans = spans(&tree);
    let empty = span(&spans, "folder:empty");
    let perso = span(&spans, "folder:perso");
    // perso is folded: nothing under it but the space the next folder takes.
    assert_eq!(empty.top, perso.bottom + GAP + FOLDER_GAP + GAP);
    // The empty one is open: its placeholder and the padding under it are
    // between it and the folder after.
    let last = span(&spans, "folder:last");
    assert_eq!(
        last.top,
        empty.bottom
            + GAP
            + PLACEHOLDER_HEIGHT
            + PLACEHOLDER_GAP
            + BODY_PAD
            + GAP
            + FOLDER_GAP
            + GAP
    );
}

#[test]
fn a_row_already_in_view_needs_no_scroll() {
    let row = Span {
        key: "ws:a".into(),
        top: 100.0,
        bottom: 128.0,
    };
    assert_eq!(reveal(&row, 0.0, 400.0), None);
    assert_eq!(reveal(&row, 90.0, 400.0), None);
}

#[test]
fn a_row_above_the_view_is_scrolled_to_just_under_its_top() {
    let row = Span {
        key: "ws:a".into(),
        top: 100.0,
        bottom: 128.0,
    };
    assert_eq!(reveal(&row, 300.0, 400.0), Some(94.0));
    // Never past the start of the list.
    let first = Span {
        key: "ws:a".into(),
        top: 2.0,
        bottom: 30.0,
    };
    assert_eq!(reveal(&first, 300.0, 400.0), Some(0.0));
}

#[test]
fn a_row_below_the_view_is_scrolled_to_just_above_its_bottom() {
    let row = Span {
        key: "ws:z".into(),
        top: 700.0,
        bottom: 728.0,
    };
    assert_eq!(reveal(&row, 0.0, 400.0), Some(334.0));
}

#[test]
fn a_list_that_has_not_said_how_tall_it_is_is_left_alone() {
    let row = Span {
        key: "ws:z".into(),
        top: 700.0,
        bottom: 728.0,
    };
    assert_eq!(reveal(&row, 0.0, 0.0), None);
}

/// The places the arithmetic gives are the places the view puts its rows: the
/// centre of each name, drawn by a real layout, sits in the middle of its span.
#[test]
fn the_rows_are_where_the_view_draws_them() {
    let world = world();
    let model = model_of_world(&world);
    let pkg = crate::app::Packages::default();
    let st = crate::navigator::State::default();
    let ctx = crate::navigator::testing::ctx(&model, &pkg);
    let framed = iced::widget::container(crate::navigator::view(&st, ctx))
        .width(hangar_theme::tokens::size::SIDEBAR_WIDTH)
        .height(iced::Length::Fill);
    let mut ui = crate::navigator::testing::simulate(framed);

    let tree = build_nav_tree(&NavInput {
        workspaces: &world.projects(),
        folders: &world.folders,
        query: "",
        attention_only: false,
        wants_you: &|_| false,
    });
    let spans = spans(&tree);

    // The list starts below the top, the manager's door and the heading: find
    // where by the first row, then every other must be where the arithmetic
    // says relative to it.
    let first = ui.find("Project a").expect("the row is shown");
    let first_centre = first.visible_bounds().unwrap().center().y;
    let origin = first_centre - (span(&spans, "ws:a").top + ROW_HEIGHT / 2.0);
    for (key, name) in [
        ("ws:b", "Project b"),
        ("ws:c", "Project c"),
        ("ws:d", "Project d"),
    ] {
        let shown = ui.find(name).expect("the row is shown");
        let centre = shown.visible_bounds().unwrap().center().y;
        let expected = origin + span(&spans, key).top + ROW_HEIGHT / 2.0;
        assert!(
            (centre - expected).abs() < 2.0,
            "{key}: drawn at {centre}, computed at {expected}"
        );
    }
}
