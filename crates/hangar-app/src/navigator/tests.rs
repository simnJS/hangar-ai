//! The navigator's keyboard, rename and filter (`Sidebar.tsx`, which had no
//! tests of its own). They drive `step` with a model read from a state, and
//! look at what the navigator keeps; what it asks of the store is the store's
//! to test.
#![allow(clippy::field_reassign_with_default)]

use iced::keyboard::{Key, Modifiers, key::Named};

use super::*;
use crate::state::types::{AgentId, AppState, Pane, WorkspaceFolder};

fn ws(id: &str, folder: Option<&str>) -> Workspace {
    Workspace {
        id: id.into(),
        name: format!("Project {id}"),
        cwd: format!("/dev/{id}"),
        folder_id: folder.map(str::to_string),
        panes: vec![Pane {
            id: format!("{id}-p"),
            name: format!("Agent {id}"),
            agent: AgentId::Claude,
            ..Pane::default()
        }],
        ..Workspace::default()
    }
}

fn folder(id: &str, collapsed: bool) -> WorkspaceFolder {
    WorkspaceFolder {
        id: id.into(),
        name: id.into(),
        collapsed,
        ..WorkspaceFolder::default()
    }
}

/// Loose: a, b. In "work" (open): c, d. In "perso" (folded): e.
fn model() -> Model {
    let mut model = Model::default();
    model.hydrate(AppState {
        workspaces: vec![
            ws("a", None),
            ws("c", Some("work")),
            ws("b", None),
            ws("d", Some("work")),
            ws("e", Some("perso")),
        ],
        folders: vec![folder("work", false), folder("perso", true)],
        active_workspace_id: Some("b".into()),
        ..AppState::default()
    });
    model
}

fn key(st: &mut State, m: &mut Model, key: Key) {
    step_key(st, m, key, Modifiers::empty());
}

fn step_key(st: &mut State, m: &mut Model, key: Key, modifiers: Modifiers) {
    let _ = step(st, m, Msg::Key(key, modifiers));
}

fn named(named: Named) -> Key {
    Key::Named(named)
}

fn at(st: &State) -> &str {
    st.cursor.as_deref().unwrap()
}

// Screen order: ws:a, ws:b, folder:work, ws:c, ws:d, folder:perso.

#[test]
fn the_tab_stop_is_the_active_workspace_until_the_keyboard_has_been_elsewhere() {
    let m = model();
    let mut st = State::default();
    let rows = rows_of(&st, &m);
    assert_eq!(tab_key(&st, &m, &rows).as_deref(), Some("ws:b"));
    st.cursor = Some("folder:work".into());
    assert_eq!(tab_key(&st, &m, &rows).as_deref(), Some("folder:work"));
    // A row that is not on screen any more cannot hold it.
    st.cursor = Some("ws:gone".into());
    assert_eq!(tab_key(&st, &m, &rows).as_deref(), Some("ws:b"));
}

#[test]
fn with_no_active_workspace_the_first_row_holds_it() {
    let mut m = Model::default();
    m.hydrate(AppState {
        workspaces: vec![ws("a", None), ws("b", None)],
        ..AppState::default()
    });
    let st = State::default();
    let rows = rows_of(&st, &m);
    assert_eq!(tab_key(&st, &m, &rows).as_deref(), Some("ws:a"));
}

#[test]
fn entering_the_tree_lands_on_the_tab_stop() {
    let mut m = model();
    let mut st = State::default();
    let _ = step(&mut st, &mut m, Msg::EnterTree);
    assert_eq!(at(&st), "ws:b");
    // Entering again does not take the keyboard off the row it is on.
    st.cursor = Some("ws:c".into());
    let _ = step(&mut st, &mut m, Msg::EnterTree);
    assert_eq!(at(&st), "ws:c");
}

#[test]
fn the_arrows_walk_the_rows_in_screen_order() {
    let mut m = model();
    let mut st = State::default();
    st.cursor = Some("ws:a".into());
    key(&mut st, &mut m, named(Named::ArrowDown));
    assert_eq!(at(&st), "ws:b");
    key(&mut st, &mut m, named(Named::ArrowDown));
    assert_eq!(at(&st), "folder:work");
    key(&mut st, &mut m, named(Named::ArrowDown));
    assert_eq!(at(&st), "ws:c");
    key(&mut st, &mut m, named(Named::ArrowUp));
    assert_eq!(at(&st), "folder:work");
}

#[test]
fn the_ends_of_the_list_do_not_wrap() {
    let mut m = model();
    let mut st = State::default();
    st.cursor = Some("ws:a".into());
    key(&mut st, &mut m, named(Named::ArrowUp));
    assert_eq!(at(&st), "ws:a");
    key(&mut st, &mut m, named(Named::End));
    assert_eq!(at(&st), "folder:perso");
    key(&mut st, &mut m, named(Named::ArrowDown));
    assert_eq!(at(&st), "folder:perso");
    key(&mut st, &mut m, named(Named::Home));
    assert_eq!(at(&st), "ws:a");
}

#[test]
fn right_on_an_open_folder_steps_into_what_it_holds() {
    let mut m = model();
    let mut st = State::default();
    st.cursor = Some("folder:work".into());
    key(&mut st, &mut m, named(Named::ArrowRight));
    assert_eq!(at(&st), "ws:c");
}

#[test]
fn right_on_a_workspace_does_nothing() {
    let mut m = model();
    let mut st = State::default();
    st.cursor = Some("ws:a".into());
    key(&mut st, &mut m, named(Named::ArrowRight));
    assert_eq!(at(&st), "ws:a");
}

#[test]
fn left_on_a_workspace_in_a_folder_goes_to_the_folder() {
    let mut m = model();
    let mut st = State::default();
    st.cursor = Some("ws:d".into());
    key(&mut st, &mut m, named(Named::ArrowLeft));
    assert_eq!(at(&st), "folder:work");
}

#[test]
fn left_on_a_loose_workspace_stays_put() {
    let mut m = model();
    let mut st = State::default();
    st.cursor = Some("ws:a".into());
    key(&mut st, &mut m, named(Named::ArrowLeft));
    assert_eq!(at(&st), "ws:a");
}

#[test]
fn a_chord_with_control_or_alt_is_not_the_trees() {
    let mut m = model();
    let mut st = State::default();
    st.cursor = Some("ws:a".into());
    step_key(&mut st, &mut m, named(Named::ArrowDown), Modifiers::CTRL);
    step_key(&mut st, &mut m, named(Named::ArrowDown), Modifiers::ALT);
    assert_eq!(at(&st), "ws:a");
}

#[test]
fn f2_renames_the_row_the_keyboard_is_on() {
    let mut m = model();
    let mut st = State::default();
    st.cursor = Some("ws:c".into());
    key(&mut st, &mut m, named(Named::F2));
    let rename = st.rename.as_ref().expect("a rename");
    assert_eq!(rename.kind, RowKind::Ws);
    assert_eq!(rename.id, "c");
    assert_eq!(rename.draft, "Project c");
}

#[test]
fn f2_on_a_folder_renames_the_folder() {
    let mut m = model();
    let mut st = State::default();
    st.cursor = Some("folder:perso".into());
    key(&mut st, &mut m, named(Named::F2));
    let rename = st.rename.as_ref().unwrap();
    assert_eq!(
        (rename.kind, rename.id.as_str()),
        (RowKind::Folder, "perso")
    );
}

#[test]
fn the_menu_key_opens_the_rows_menu_for_the_keyboard() {
    let mut m = model();
    let mut st = State::default();
    st.cursor = Some("ws:c".into());
    key(&mut st, &mut m, named(Named::ContextMenu));
    let menu = st.menu.as_ref().expect("a menu");
    assert_eq!((menu.kind, menu.id.as_str()), (RowKind::Ws, "c"));
    assert!(menu.from_keyboard);
}

#[test]
fn shift_f10_does_the_same_and_plain_f10_does_not() {
    let mut m = model();
    let mut st = State::default();
    st.cursor = Some("ws:a".into());
    key(&mut st, &mut m, named(Named::F10));
    assert!(st.menu.is_none());
    step_key(&mut st, &mut m, named(Named::F10), Modifiers::SHIFT);
    assert!(st.menu.is_some());
}

#[test]
fn closing_the_menu_closes_it() {
    let mut m = model();
    let mut st = State::default();
    let _ = step(
        &mut st,
        &mut m,
        Msg::OpenMenu {
            kind: RowKind::Ws,
            id: "a".into(),
            at: Vector::new(3.0, 4.0),
            from_keyboard: false,
        },
    );
    assert_eq!(
        st.menu.as_ref().map(|menu| menu.at),
        Some(Vector::new(3.0, 4.0))
    );
    let _ = step(&mut st, &mut m, Msg::CloseMenu);
    assert!(st.menu.is_none());
}

// ---- Renaming ---------------------------------------------------------------------------------

#[test]
fn a_name_is_typed_over_the_rows_and_settled_by_enter() {
    let mut m = model();
    let mut st = State::default();
    let _ = step(&mut st, &mut m, Msg::DoublePress("a".into()));
    assert!(st.rename.is_some());
    let _ = step(&mut st, &mut m, Msg::RenameInput("Renamed".into()));
    assert_eq!(st.rename.as_ref().unwrap().draft, "Renamed");
    let _ = step(&mut st, &mut m, Msg::RenameSubmit);
    assert!(st.rename.is_none());
}

#[test]
fn escape_settles_a_rename_without_saving() {
    let mut m = model();
    let mut st = State::default();
    let _ = step(&mut st, &mut m, Msg::DoublePress("a".into()));
    let before = m.revision();
    let _ = step(&mut st, &mut m, Msg::RenameInput("Nope".into()));
    let _ = step(&mut st, &mut m, Msg::RenameCancel);
    assert!(st.rename.is_none());
    assert_eq!(m.revision(), before);
}

#[test]
fn a_press_on_another_row_settles_the_rename_first() {
    let mut m = model();
    let mut st = State::default();
    let _ = step(&mut st, &mut m, Msg::DoublePress("a".into()));
    let _ = step(
        &mut st,
        &mut m,
        Msg::Press(DragItem {
            kind: DragKind::Workspace,
            id: "b".into(),
        }),
    );
    assert!(st.rename.is_none());
    assert!(st.drag.is_some());
}

#[test]
fn a_name_of_blanks_saves_nothing() {
    let mut m = model();
    let mut st = State::default();
    let _ = step(&mut st, &mut m, Msg::DoublePress("a".into()));
    let _ = step(&mut st, &mut m, Msg::RenameInput("   ".into()));
    let before = m.revision();
    let _ = step(&mut st, &mut m, Msg::RenameSubmit);
    assert!(st.rename.is_none());
    assert_eq!(m.revision(), before);
}

// ---- The filter -------------------------------------------------------------------------------

#[test]
fn escape_in_the_filter_clears_the_text_then_the_toggle() {
    let mut m = model();
    let mut st = State::default();
    st.query = "eve".into();
    st.attention_only = true;
    let _ = step(&mut st, &mut m, Msg::FilterEscape);
    assert_eq!(st.query, "");
    assert!(st.attention_only);
    let _ = step(&mut st, &mut m, Msg::FilterEscape);
    assert!(!st.attention_only);
}

#[test]
fn escape_in_an_empty_filter_goes_to_the_list() {
    let mut m = model();
    let mut st = State::default();
    let _ = step(&mut st, &mut m, Msg::FilterEscape);
    assert_eq!(at(&st), "ws:b");
}

#[test]
fn clearing_the_filter_clears_both_the_text_and_the_toggle() {
    let mut m = model();
    let mut st = State::default();
    st.query = "x".into();
    st.attention_only = true;
    let _ = step(&mut st, &mut m, Msg::ClearFilter);
    assert!(!filtering(&st));
}

#[test]
fn the_keyboard_walks_only_what_the_filter_lets_through() {
    let mut m = model();
    let mut st = State::default();
    st.query = "project c".into();
    // A folder shows for what it holds: the list is the folder and c.
    let keys: Vec<String> = rows_of(&st, &m).into_iter().map(|row| row.key).collect();
    assert_eq!(keys, ["folder:work", "ws:c"]);
    st.cursor = Some("folder:work".into());
    key(&mut st, &mut m, named(Named::ArrowDown));
    assert_eq!(at(&st), "ws:c");
    key(&mut st, &mut m, named(Named::ArrowDown));
    assert_eq!(at(&st), "ws:c");
}

#[test]
fn the_project_managers_workspace_is_not_in_the_list() {
    let mut m = Model::default();
    m.hydrate(AppState {
        workspaces: vec![
            ws("a", None),
            Workspace {
                hq: Some(true),
                ..ws("hq", None)
            },
        ],
        ..AppState::default()
    });
    let st = State::default();
    let keys: Vec<String> = rows_of(&st, &m).into_iter().map(|row| row.key).collect();
    assert_eq!(keys, ["ws:a"]);
}

// ---- The pointer ------------------------------------------------------------------------------

fn press(st: &mut State, m: &mut Model, id: &str) {
    let _ = step(
        st,
        m,
        Msg::Press(DragItem {
            kind: DragKind::Workspace,
            id: id.into(),
        }),
    );
}

#[test]
fn a_press_and_a_release_without_travel_is_a_click_and_no_drag() {
    let mut m = model();
    let mut st = State::default();
    press(&mut st, &mut m, "a");
    assert!(st.drag.is_some());
    let _ = step(&mut st, &mut m, Msg::Release("ws:a".into()));
    assert!(st.drag.is_none());
    assert_eq!(at(&st), "ws:a");
}

#[test]
fn escape_during_a_drag_puts_everything_back() {
    let mut m = model();
    let mut st = State::default();
    press(&mut st, &mut m, "a");
    let _ = step(
        &mut st,
        &mut m,
        Msg::RowMove {
            row: "ws:a".into(),
            point: Point::new(0.0, 0.0),
            hit: Hit::Workspace("a".into()),
        },
    );
    let before = m.revision();
    let _ = step(&mut st, &mut m, Msg::DragCancel);
    assert!(st.drag.is_none());
    assert_eq!(m.revision(), before);
}

#[test]
fn leaving_the_list_forgets_the_target_but_not_the_drag() {
    let mut m = model();
    let mut st = State::default();
    press(&mut st, &mut m, "a");
    for (row, y) in [("ws:a", 0.0), ("ws:b", 3.0)] {
        let _ = step(
            &mut st,
            &mut m,
            Msg::RowMove {
                row: row.into(),
                point: Point::new(5.0, y),
                hit: Hit::Workspace(row.trim_start_matches("ws:").into()),
            },
        );
    }
    assert!(st.drag.as_ref().unwrap().target.is_some());
    let _ = step(&mut st, &mut m, Msg::ListExit);
    let drag = st.drag.as_ref().unwrap();
    assert!(drag.active);
    assert_eq!(drag.target, None);
}

#[test]
fn a_note_goes_when_its_own_time_comes_and_not_another() {
    let mut m = model();
    let mut st = State::default();
    let _ = step(&mut st, &mut m, Msg::EditorDone(Some("first".into())));
    let first = st.note.as_ref().unwrap().0;
    let _ = step(&mut st, &mut m, Msg::EditorDone(Some("second".into())));
    // The first one's expiry must not take the second one's note away.
    let _ = step(&mut st, &mut m, Msg::NoteExpired(first));
    assert_eq!(
        st.note.as_ref().map(|(_, note)| note.as_str()),
        Some("second")
    );
    let second = st.note.as_ref().unwrap().0;
    let _ = step(&mut st, &mut m, Msg::NoteExpired(second));
    assert!(st.note.is_none());
}

#[test]
fn an_editor_that_opened_leaves_no_note() {
    let mut m = model();
    let mut st = State::default();
    let _ = step(&mut st, &mut m, Msg::EditorDone(None));
    assert!(st.note.is_none());
}

#[test]
fn a_global_release_before_the_rows_own_still_lets_the_click_through() {
    // The subscription's release and the row's can arrive in either order.
    let mut m = model();
    let mut st = State::default();
    press(&mut st, &mut m, "a");
    let _ = step(&mut st, &mut m, Msg::DragEnd);
    assert!(st.drag.as_ref().is_some_and(|drag| drag.ended));
    let _ = step(&mut st, &mut m, Msg::Release("ws:a".into()));
    assert!(st.drag.is_none());
    assert_eq!(at(&st), "ws:a");
}

#[test]
fn a_global_release_that_no_row_followed_leaves_nothing_to_drag_with() {
    let mut m = model();
    let mut st = State::default();
    press(&mut st, &mut m, "a");
    let _ = step(&mut st, &mut m, Msg::DragEnd);
    // The pointer then wanders over rows without the button.
    for (row, y) in [("ws:a", 0.0), ("ws:b", 3.0)] {
        let _ = step(
            &mut st,
            &mut m,
            Msg::RowMove {
                row: row.into(),
                point: Point::new(5.0, y),
                hit: Hit::Workspace(row.trim_start_matches("ws:").into()),
            },
        );
    }
    assert!(!st.drag.as_ref().unwrap().active);
}
