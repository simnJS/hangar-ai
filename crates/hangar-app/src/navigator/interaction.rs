//! The navigator driven with real pointer and key events, on the widgets
//! themselves: what a press, a double click, a right click or a drag over its
//! rows says to the app.
#![allow(clippy::field_reassign_with_default)]

use iced::keyboard::{Modifiers, key::Named};
use iced::mouse;

use super::testing::{Drive, ctx, folder, model_of, pane, simulate, workspace};
use super::*;
use crate::app::Packages;
use crate::state::types::{AgentId, AppState};

fn model() -> Model {
    model_of(AppState {
        workspaces: vec![
            workspace("a", "alpha", None, vec![pane("a1", "Ava", AgentId::Claude)]),
            workspace(
                "b",
                "beta",
                Some("work"),
                vec![pane("b1", "Ben", AgentId::Shell)],
            ),
            workspace("c", "gamma", None, vec![]),
        ],
        folders: vec![folder("work", "Work", false)],
        active_workspace_id: Some("c".into()),
        ..AppState::default()
    })
}

/// Everything a drive of the view sent, as the messages the app would get.
fn run(
    st: &State,
    model: &Model,
    drive: impl FnOnce(&mut iced_test::Simulator<'_, Message>),
) -> Vec<Message> {
    let pkg = Packages::default();
    let ctx = ctx(model, &pkg);
    // The frame gives the navigator its width and the window's height.
    let framed = iced::widget::container(view(st, ctx))
        .width(hangar_theme::tokens::size::SIDEBAR_WIDTH)
        .height(iced::Length::Fill);
    let mut ui = simulate(framed);
    drive(&mut ui);
    ui.into_messages().collect()
}

fn navigator_messages(messages: &[Message]) -> Vec<&Msg> {
    messages
        .iter()
        .filter_map(|message| match message {
            Message::Navigator(msg) => Some(msg),
            _ => None,
        })
        .collect()
}

#[test]
fn a_click_on_a_workspace_presses_then_releases_its_row() {
    let messages = run(&State::default(), &model(), |ui| {
        ui.click("alpha").expect("the row is shown");
    });
    let seen = navigator_messages(&messages);
    assert!(
        seen.iter().any(|msg| matches!(
            msg,
            Msg::Press(DragItem { kind: DragKind::Workspace, id }) if id == "a"
        )),
        "{seen:?}"
    );
    assert!(
        seen.iter()
            .any(|msg| matches!(msg, Msg::Release(key) if key == "ws:a")),
        "{seen:?}"
    );
}

#[test]
fn a_click_on_a_folders_heading_presses_and_releases_the_folder() {
    let messages = run(&State::default(), &model(), |ui| {
        ui.click("WORK").expect("the heading is shown");
    });
    let seen = navigator_messages(&messages);
    assert!(seen.iter().any(|msg| matches!(
        msg,
        Msg::Press(DragItem { kind: DragKind::Folder, id }) if id == "work"
    )));
    assert!(
        seen.iter()
            .any(|msg| matches!(msg, Msg::Release(key) if key == "folder:work"))
    );
}

#[test]
fn two_clicks_in_a_row_are_a_double_click_that_renames() {
    let messages = run(&State::default(), &model(), |ui| {
        ui.click("alpha").unwrap();
        ui.click("alpha").unwrap();
    });
    assert!(
        navigator_messages(&messages)
            .iter()
            .any(|msg| matches!(msg, Msg::DoublePress(id) if id == "a")),
        "{messages:?}"
    );
}

#[test]
fn a_right_click_asks_for_the_rows_menu_at_the_pointer() {
    let messages = run(&State::default(), &model(), |ui| {
        let target = ui.find("beta").expect("the row is shown");
        let at = target.visible_bounds().expect("it is on screen").center();
        ui.move_to(at.x, at.y);
        ui.press(mouse::Button::Right);
    });
    let asked = navigator_messages(&messages)
        .into_iter()
        .find_map(|msg| match msg {
            Msg::OpenMenu {
                kind,
                id,
                from_keyboard,
                ..
            } => Some((*kind, id.clone(), *from_keyboard)),
            _ => None,
        })
        .expect("a menu was asked for");
    assert_eq!(asked, (RowKind::Ws, "b".to_string(), false));
}

#[test]
fn the_pointer_over_a_row_says_which() {
    let messages = run(&State::default(), &model(), |ui| {
        let target = ui.find("alpha").unwrap();
        let at = target.visible_bounds().unwrap().center();
        ui.move_to(at.x, at.y);
    });
    assert!(
        navigator_messages(&messages)
            .iter()
            .any(|msg| matches!(msg, Msg::Hover(Some(key)) if key == "ws:a")),
        "{messages:?}"
    );
}

#[test]
fn rows_say_nothing_of_the_pointers_travel_unless_something_is_being_dragged() {
    let messages = run(&State::default(), &model(), |ui| {
        let target = ui.find("alpha").unwrap();
        let at = target.visible_bounds().unwrap().center();
        ui.move_to(at.x, at.y);
        ui.move_to(at.x + 3.0, at.y + 1.0);
    });
    assert!(
        !navigator_messages(&messages)
            .iter()
            .any(|msg| matches!(msg, Msg::RowMove { .. })),
        "{messages:?}"
    );
}

#[test]
fn while_a_row_is_pressed_each_row_reports_where_the_pointer_is_in_it() {
    let mut st = State::default();
    st.drag = Some(Drag::pressed(DragItem {
        kind: DragKind::Workspace,
        id: "a".into(),
    }));
    let messages = run(&st, &model(), |ui| {
        let target = ui.find("gamma").unwrap();
        let at = target.visible_bounds().unwrap().center();
        // The first move into a row is its `on_enter`; the ones after it are
        // what a drag follows.
        ui.move_to(at.x, at.y);
        ui.move_to(at.x + 1.0, at.y);
    });
    let moved = navigator_messages(&messages)
        .into_iter()
        .find_map(|msg| match msg {
            Msg::RowMove { row, hit, point } => Some((row.clone(), hit.clone(), *point)),
            _ => None,
        })
        .expect("the row reported the pointer");
    assert_eq!(moved.0, "ws:c");
    assert_eq!(moved.1, Hit::Workspace("c".into()));
    // Inside a 28 px row, near its middle.
    assert!(moved.2.y > 5.0 && moved.2.y < 23.0, "{:?}", moved.2);
}

#[test]
fn a_release_in_the_list_while_dragging_is_the_rows_to_catch() {
    let mut st = State::default();
    st.drag = Some(Drag::pressed(DragItem {
        kind: DragKind::Workspace,
        id: "a".into(),
    }));
    let messages = run(&st, &model(), |ui| {
        ui.click("gamma").unwrap();
    });
    assert!(
        navigator_messages(&messages)
            .iter()
            .any(|msg| matches!(msg, Msg::Release(key) if key == "ws:c")),
        "{messages:?}"
    );
}

#[test]
fn down_in_the_filter_goes_to_the_list_and_escape_clears_it() {
    let messages = run(&State::default(), &model(), |ui| {
        ui.click("Filter").expect("the field is shown");
        ui.key(Named::ArrowDown, Modifiers::empty());
        ui.key(Named::Escape, Modifiers::empty());
    });
    let seen = navigator_messages(&messages);
    assert!(
        seen.iter().any(|msg| matches!(msg, Msg::FilterDown)),
        "{seen:?}"
    );
    assert!(
        seen.iter().any(|msg| matches!(msg, Msg::FilterEscape)),
        "{seen:?}"
    );
}

#[test]
fn the_filter_leaves_the_arrows_and_escape_alone_while_a_pane_has_the_keyboard() {
    // Nothing clicked in the sidebar: the keys are a pane's, typed into Claude
    // Code's choices, and must not take the keyboard to the list.
    let messages = run(&State::default(), &model(), |ui| {
        ui.key(Named::ArrowDown, Modifiers::empty());
        ui.key(Named::Escape, Modifiers::empty());
    });
    let seen = navigator_messages(&messages);
    assert!(
        !seen
            .iter()
            .any(|msg| matches!(msg, Msg::FilterDown | Msg::FilterEscape)),
        "{seen:?}"
    );
}

#[test]
fn typing_in_the_filter_sends_the_text() {
    let messages = run(&State::default(), &model(), |ui| {
        ui.click("Filter").unwrap();
        ui.typewrite("be");
    });
    assert!(
        navigator_messages(&messages)
            .iter()
            .any(|msg| matches!(msg, Msg::Query(text) if text == "b")),
        "{messages:?}"
    );
}

#[test]
fn the_new_workspace_button_opens_the_workspace_dialog() {
    // The button is an icon: its tip is its name, and it is the left one of the
    // filter bar, at the bottom of the navigator.
    let messages = run(&State::default(), &model(), |ui| {
        ui.move_to(19.0, WINDOW_BOTTOM_BAR);
        ui.press(mouse::Button::Left);
        ui.release(mouse::Button::Left);
    });
    assert!(
        messages.iter().any(|message| matches!(
            message,
            Message::Intent(Intent::OpenOverlay(crate::shell::Overlay::WorkspaceDialog))
        )),
        "{messages:?}"
    );
}

#[test]
fn the_settings_button_opens_the_settings() {
    let messages = run(&State::default(), &model(), |ui| {
        ui.move_to(233.0, WINDOW_BOTTOM_BAR);
        ui.press(mouse::Button::Left);
        ui.release(mouse::Button::Left);
    });
    assert!(
        messages
            .iter()
            .any(|message| matches!(message, Message::Intent(Intent::OpenSettings(None)))),
        "{messages:?}"
    );
}

#[test]
fn the_project_manager_door_opens_it() {
    let messages = run(&State::default(), &model(), |ui| {
        ui.click("Open the project manager").unwrap();
    });
    assert!(
        messages
            .iter()
            .any(|message| matches!(message, Message::Intent(Intent::OpenManager))),
        "{messages:?}"
    );
}

#[test]
fn a_rename_in_place_is_settled_with_enter_and_called_off_with_escape() {
    let mut st = State::default();
    st.rename = Some(Rename {
        kind: RowKind::Ws,
        id: "a".into(),
        draft: "alpha".into(),
    });
    let messages = run(&st, &model(), |ui| {
        ui.click("alpha").expect("the field is shown");
        ui.key(Named::Enter, Modifiers::empty());
        ui.key(Named::Escape, Modifiers::empty());
    });
    let seen = navigator_messages(&messages);
    assert!(
        seen.iter().any(|msg| matches!(msg, Msg::RenameSubmit)),
        "{seen:?}"
    );
    assert!(
        seen.iter().any(|msg| matches!(msg, Msg::RenameCancel)),
        "{seen:?}"
    );
}

/// The y of the filter bar's middle in the test window (700 px tall, bar 36).
const WINDOW_BOTTOM_BAR: f32 = 700.0 - 18.0;
