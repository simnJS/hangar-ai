//! The drag gesture and where it lands (`Sidebar.tsx`'s `beginDrag`,
//! `targetAt` and `commit`, which had no tests of their own).

use super::*;

fn ws(id: &str, folder: Option<&str>) -> Workspace {
    Workspace {
        id: id.into(),
        folder_id: folder.map(str::to_string),
        ..Workspace::default()
    }
}

fn folder(id: &str) -> WorkspaceFolder {
    WorkspaceFolder {
        id: id.into(),
        name: id.into(),
        ..WorkspaceFolder::default()
    }
}

fn item(kind: DragKind, id: &str) -> DragItem {
    DragItem {
        kind,
        id: id.into(),
    }
}

fn row(id: &str, after: bool) -> DropTarget {
    DropTarget::Row {
        id: id.into(),
        after,
    }
}

// ---- Where the pointer is ---------------------------------------------------------------------

#[test]
fn a_workspace_dropped_beside_another_lands_before_or_after_it() {
    let dragged = item(DragKind::Workspace, "a");
    let over_b = Hit::Workspace("b".into());
    assert_eq!(target_at(&dragged, &over_b, false), Some(row("b", false)));
    assert_eq!(target_at(&dragged, &over_b, true), Some(row("b", true)));
}

#[test]
fn a_workspace_over_itself_lands_nowhere() {
    let dragged = item(DragKind::Workspace, "a");
    assert_eq!(target_at(&dragged, &Hit::Workspace("a".into()), true), None);
}

#[test]
fn a_workspace_over_a_folder_or_its_empty_placeholder_goes_into_it() {
    let dragged = item(DragKind::Workspace, "a");
    let into = Some(DropTarget::Into { folder: "f".into() });
    assert_eq!(target_at(&dragged, &Hit::Folder("f".into()), false), into);
    assert_eq!(
        target_at(&dragged, &Hit::FolderBody("f".into()), true),
        into
    );
}

#[test]
fn a_folder_lands_beside_another_folder_only() {
    let dragged = item(DragKind::Folder, "f");
    assert_eq!(
        target_at(&dragged, &Hit::Folder("g".into()), true),
        Some(row("g", true))
    );
    assert_eq!(target_at(&dragged, &Hit::Folder("f".into()), true), None);
    assert_eq!(target_at(&dragged, &Hit::Workspace("a".into()), true), None);
    assert_eq!(
        target_at(&dragged, &Hit::FolderBody("g".into()), true),
        None
    );
}

#[test]
fn the_root_takes_either_kind() {
    let root = Hit::Root { heading: true };
    let expected = Some(DropTarget::Root { heading: true });
    assert_eq!(
        target_at(&item(DragKind::Workspace, "a"), &root, false),
        expected
    );
    assert_eq!(
        target_at(&item(DragKind::Folder, "f"), &root, false),
        expected
    );
}

// ---- The gesture ------------------------------------------------------------------------------

const H: f32 = 28.0;

#[test]
fn a_press_is_not_a_drag_until_the_pointer_travels() {
    let mut drag = Drag::pressed(item(DragKind::Workspace, "a"));
    // The first sighting only marks where the pointer is.
    assert!(!drag.moved(
        "ws:a",
        Point::new(10.0, 14.0),
        H,
        Some(Hit::Workspace("a".into()))
    ));
    assert!(!drag.active);
    // Under five pixels is still a click.
    assert!(!drag.moved(
        "ws:a",
        Point::new(12.0, 15.0),
        H,
        Some(Hit::Workspace("a".into()))
    ));
    assert!(!drag.active);
    assert_eq!(drag.target, None);
}

#[test]
fn five_pixels_of_travel_make_it_a_drag() {
    let mut drag = Drag::pressed(item(DragKind::Workspace, "a"));
    drag.moved("ws:a", Point::new(10.0, 14.0), H, None);
    drag.moved("ws:a", Point::new(10.0, 20.0), H, None);
    assert!(drag.active);
}

#[test]
fn leaving_the_row_it_was_pressed_on_is_travel_enough() {
    let mut drag = Drag::pressed(item(DragKind::Workspace, "a"));
    drag.moved("ws:a", Point::new(10.0, 26.0), H, None);
    let changed = drag.moved(
        "ws:b",
        Point::new(10.0, 3.0),
        H,
        Some(Hit::Workspace("b".into())),
    );
    assert!(drag.active);
    assert!(changed);
    assert_eq!(drag.target, Some(row("b", false)));
}

#[test]
fn the_half_of_the_row_decides_before_or_after() {
    let mut drag = Drag::pressed(item(DragKind::Workspace, "a"));
    drag.moved("ws:a", Point::new(0.0, 0.0), H, None);
    drag.moved(
        "ws:b",
        Point::new(10.0, 13.0),
        H,
        Some(Hit::Workspace("b".into())),
    );
    assert_eq!(drag.target, Some(row("b", false)));
    drag.moved(
        "ws:b",
        Point::new(10.0, 15.0),
        H,
        Some(Hit::Workspace("b".into())),
    );
    assert_eq!(drag.target, Some(row("b", true)));
}

#[test]
fn leaving_the_list_drops_nothing() {
    let mut drag = Drag::pressed(item(DragKind::Workspace, "a"));
    drag.moved("ws:a", Point::new(0.0, 0.0), H, None);
    drag.moved(
        "ws:b",
        Point::new(10.0, 3.0),
        H,
        Some(Hit::Workspace("b".into())),
    );
    drag.left_list();
    assert_eq!(drag.target, None);
}

#[test]
fn a_move_that_changes_nothing_says_so() {
    let mut drag = Drag::pressed(item(DragKind::Workspace, "a"));
    drag.moved("ws:a", Point::new(0.0, 0.0), H, None);
    assert!(drag.moved(
        "ws:b",
        Point::new(10.0, 3.0),
        H,
        Some(Hit::Workspace("b".into()))
    ));
    assert!(!drag.moved(
        "ws:b",
        Point::new(11.0, 4.0),
        H,
        Some(Hit::Workspace("b".into()))
    ));
}

#[test]
fn only_the_row_a_drag_would_land_beside_wears_the_marker() {
    let mut drag = Drag::pressed(item(DragKind::Workspace, "a"));
    drag.moved("ws:a", Point::new(0.0, 0.0), H, None);
    drag.moved(
        "ws:b",
        Point::new(10.0, 20.0),
        H,
        Some(Hit::Workspace("b".into())),
    );
    assert_eq!(marker(Some(&drag), DragKind::Workspace, "b"), Some(true));
    assert_eq!(marker(Some(&drag), DragKind::Workspace, "c"), None);
    // A folder with that id is not the row.
    assert_eq!(marker(Some(&drag), DragKind::Folder, "b"), None);
    assert_eq!(marker(None, DragKind::Workspace, "b"), None);
}

#[test]
fn a_drag_that_has_not_started_marks_nothing() {
    let mut drag = Drag::pressed(item(DragKind::Workspace, "a"));
    drag.target = Some(row("b", true));
    assert_eq!(marker(Some(&drag), DragKind::Workspace, "b"), None);
}

// ---- What a release does ----------------------------------------------------------------------

fn workspaces() -> Vec<Workspace> {
    // Loose: a, b. In "work": c, d, e. In "perso": f.
    vec![
        ws("a", None),
        ws("c", Some("work")),
        ws("b", None),
        ws("d", Some("work")),
        ws("f", Some("perso")),
        ws("e", Some("work")),
    ]
}

fn moved_workspace(id: &str, target: DropTarget) -> Option<(Option<String>, Option<String>)> {
    match commit(
        &item(DragKind::Workspace, id),
        &target,
        &workspaces(),
        &[folder("work"), folder("perso")],
    )? {
        StoreAction::MoveWorkspace {
            id: moved,
            folder,
            before,
        } => {
            assert_eq!(moved, id);
            Some((folder, before))
        }
        other => panic!("expected a move of a workspace, got {other:?}"),
    }
}

#[test]
fn dropping_a_workspace_before_another_puts_it_in_that_ones_folder_ahead_of_it() {
    assert_eq!(
        moved_workspace("a", row("d", false)),
        Some((Some("work".into()), Some("d".into())))
    );
}

#[test]
fn after_means_before_the_next_one_of_the_same_folder() {
    assert_eq!(
        moved_workspace("a", row("c", true)),
        Some((Some("work".into()), Some("d".into())))
    );
}

#[test]
fn after_the_last_one_of_a_folder_means_last_in_it() {
    assert_eq!(
        moved_workspace("a", row("e", true)),
        Some((Some("work".into()), None))
    );
}

#[test]
fn the_dragged_workspace_does_not_count_among_its_neighbours() {
    // d dropped after c: the next in "work" without d is e.
    assert_eq!(
        moved_workspace("d", row("c", true)),
        Some((Some("work".into()), Some("e".into())))
    );
}

#[test]
fn dropping_beside_a_loose_workspace_takes_it_out_of_its_folder() {
    assert_eq!(
        moved_workspace("c", row("b", false)),
        Some((None, Some("b".into())))
    );
    assert_eq!(moved_workspace("c", row("b", true)), Some((None, None)));
}

#[test]
fn dropping_on_a_folder_files_the_workspace_last_in_it() {
    assert_eq!(
        moved_workspace(
            "a",
            DropTarget::Into {
                folder: "perso".into()
            }
        ),
        Some((Some("perso".into()), None))
    );
}

#[test]
fn dropping_on_the_root_makes_it_loose_and_last() {
    assert_eq!(
        moved_workspace("c", DropTarget::Root { heading: false }),
        Some((None, None))
    );
}

#[test]
fn a_workspace_dropped_beside_one_that_vanished_moves_nothing() {
    assert_eq!(moved_workspace("a", row("gone", false)), None);
}

fn moved_folder(id: &str, target: DropTarget) -> Option<Option<String>> {
    let folders = [folder("work"), folder("perso"), folder("misc")];
    match commit(
        &item(DragKind::Folder, id),
        &target,
        &workspaces(),
        &folders,
    )? {
        StoreAction::MoveFolder { id: moved, before } => {
            assert_eq!(moved, id);
            Some(before)
        }
        other => panic!("expected a move of a folder, got {other:?}"),
    }
}

#[test]
fn a_folder_dropped_before_another_goes_ahead_of_it() {
    assert_eq!(
        moved_folder("misc", row("work", false)),
        Some(Some("work".into()))
    );
}

#[test]
fn a_folder_dropped_after_another_goes_before_the_next_one() {
    assert_eq!(
        moved_folder("misc", row("work", true)),
        Some(Some("perso".into()))
    );
}

#[test]
fn a_folder_dropped_after_the_last_goes_last() {
    assert_eq!(moved_folder("work", row("misc", true)), Some(None));
}

#[test]
fn a_folder_dropped_on_the_root_goes_last() {
    assert_eq!(
        moved_folder("work", DropTarget::Root { heading: false }),
        Some(None)
    );
}

#[test]
fn a_folder_cannot_go_into_a_folder() {
    assert!(
        commit(
            &item(DragKind::Folder, "work"),
            &DropTarget::Into {
                folder: "perso".into()
            },
            &workspaces(),
            &[folder("work"), folder("perso")],
        )
        .is_none()
    );
}
