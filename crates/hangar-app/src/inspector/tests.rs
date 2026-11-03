//! What the inspector decides without drawing: when git is asked about,
//! how the grip turns a drag into a width, how sections fold, and which
//! requests leave for other packages.
//!
//! The store itself is P0b's (its `apply` changes nothing yet), so what these
//! check is the part that is the inspector's own: the functions the patches
//! it sends are made of, and the state it keeps.

use iced::keyboard::{self, Key, key::Named};
use iced::{Event, event, window};

use super::*;
use crate::state::types::{AppState, Pane, Workspace};

fn pane(id: &str, cwd: Option<&str>) -> Pane {
    Pane {
        id: id.into(),
        name: id.to_uppercase(),
        cwd: cwd.map(str::to_string),
        ..Pane::default()
    }
}

/// A model with one workspace at `/work/app` holding panes `a` (in a folder
/// of its own) and `b`.
fn model() -> Model {
    let mut m = Model::default();
    m.hydrate(AppState {
        workspaces: vec![Workspace {
            id: "w".into(),
            name: "app".into(),
            cwd: "/work/app".into(),
            panes: vec![pane("a", Some("/work/app/packages/ui")), pane("b", None)],
            ..Workspace::default()
        }],
        active_workspace_id: Some("w".into()),
        ..AppState::default()
    });
    m.ui.focus_by_workspace.insert("w".into(), "a".into());
    m
}

/// Sends a message and drops the task it answers with: what the task would
/// do is the shell's.
fn send(st: &mut State, m: &mut Model, msg: Msg) {
    let _task = reduce(st, m, msg);
}

fn key_pressed(named: Named, modifiers: keyboard::Modifiers) -> Event {
    let key = Key::Named(named);
    Event::Keyboard(keyboard::Event::KeyPressed {
        key: key.clone(),
        modified_key: key,
        physical_key: keyboard::key::Physical::Unidentified(
            keyboard::key::NativeCode::Unidentified,
        ),
        location: keyboard::Location::Standard,
        modifiers,
        text: None,
        repeat: false,
    })
}

mod width {
    use super::*;

    #[test]
    fn a_width_is_kept_between_240_and_440_points_and_whole() {
        assert_eq!(clamp_width(100.0), 240.0);
        assert_eq!(clamp_width(296.4), 296.0);
        assert_eq!(clamp_width(296.5), 297.0);
        assert_eq!(clamp_width(900.0), 440.0);
    }

    #[test]
    fn dragging_left_widens_the_column_and_right_narrows_it() {
        // Pressed at x = 600 on a column 296 wide.
        assert_eq!(dragged_width(296.0, 600.0, 560.0), 336.0);
        assert_eq!(dragged_width(296.0, 600.0, 640.0), 256.0);
        assert_eq!(dragged_width(296.0, 600.0, 600.0), 296.0);
        // Past the bounds it stops.
        assert_eq!(dragged_width(296.0, 600.0, 100.0), 440.0);
        assert_eq!(dragged_width(296.0, 600.0, 900.0), 240.0);
    }

    #[test]
    fn a_drag_follows_the_pointer_only_between_the_press_and_the_release() {
        let mut st = State::default();
        let mut m = model();
        send(&mut st, &mut m, Msg::GripMoved(500.0));
        assert!(st.drag.is_none(), "moving without pressing starts nothing");

        send(&mut st, &mut m, Msg::GripPressed(600.0));
        let drag = st.drag.expect("pressed");
        assert_eq!((drag.from_x, drag.from_width), (600.0, 296.0));

        send(&mut st, &mut m, Msg::GripReleased);
        assert!(st.drag.is_none());
    }

    #[test]
    fn leaving_the_window_ends_a_drag() {
        let mut st = State::default();
        let mut m = model();
        send(&mut st, &mut m, Msg::GripPressed(600.0));
        send(&mut st, &mut m, Msg::Away(true));
        assert!(st.drag.is_none());
        assert!(st.away);
    }

    #[test]
    fn the_arrows_resize_by_sixteen_while_the_region_has_the_keyboard() {
        let none = keyboard::Modifiers::empty();
        let left = arrows(
            key_pressed(Named::ArrowLeft, none),
            event::Status::Ignored,
            window::Id::unique(),
        );
        let right = arrows(
            key_pressed(Named::ArrowRight, none),
            event::Status::Ignored,
            window::Id::unique(),
        );
        assert!(matches!(left, Some(Message::Inspector(Msg::Nudge(16)))));
        assert!(matches!(right, Some(Message::Inspector(Msg::Nudge(-16)))));
        // A field that used the key keeps it, and a chord is not a resize.
        assert!(
            arrows(
                key_pressed(Named::ArrowLeft, none),
                event::Status::Captured,
                window::Id::unique()
            )
            .is_none()
        );
        assert!(
            arrows(
                key_pressed(Named::ArrowLeft, keyboard::Modifiers::CTRL),
                event::Status::Ignored,
                window::Id::unique()
            )
            .is_none()
        );
        assert!(
            arrows(
                key_pressed(Named::ArrowUp, none),
                event::Status::Ignored,
                window::Id::unique()
            )
            .is_none()
        );
    }
}

mod folding {
    use super::*;

    #[test]
    fn folding_twice_in_a_row_loses_neither() {
        let mut folded = Vec::new();
        fold(&mut folded, "files");
        fold(&mut folded, "task");
        assert_eq!(folded, ["files", "task"]);
        fold(&mut folded, "files");
        assert_eq!(folded, ["task"]);
    }

    #[test]
    fn the_ids_are_the_ones_the_settings_store() {
        let ids = [
            SectionId::Session,
            SectionId::Terminal,
            SectionId::Context,
            SectionId::Task,
            SectionId::Queue,
            SectionId::Files,
            SectionId::Activity,
        ]
        .map(SectionId::as_str);
        assert_eq!(
            ids,
            [
                "session", "terminal", "context", "task", "queue", "files", "activity"
            ]
        );
    }
}

mod polling {
    use super::*;

    fn key_of(st: &State, m: &Model) -> Option<poll::PollKey> {
        poll_key(st, m)
    }

    #[test]
    fn git_is_asked_about_the_focused_pane_s_own_folder() {
        let (st, m) = (State::default(), model());
        let key = key_of(&st, &m).expect("a pane is in focus");
        assert_eq!(key.pane, "a");
        assert_eq!(key.folder, "/work/app/packages/ui");
    }

    #[test]
    fn a_pane_without_a_folder_of_its_own_uses_the_workspace_s() {
        let st = State::default();
        let mut m = model();
        m.ui.focus_by_workspace.insert("w".into(), "b".into());
        assert_eq!(key_of(&st, &m).unwrap().folder, "/work/app");
    }

    #[test]
    fn nothing_is_asked_while_the_window_is_away() {
        let mut st = State::default();
        let m = model();
        send(&mut st, &mut model(), Msg::Away(true));
        assert!(key_of(&st, &m).is_none());
        send(&mut st, &mut model(), Msg::Away(false));
        assert!(key_of(&st, &m).is_some());
    }

    #[test]
    fn nothing_is_asked_while_the_files_section_is_folded() {
        let st = State::default();
        let mut m = model();
        // The settings go through the store; the model's own copy is edited
        // directly here, which only a test may do.
        let mut state = m.state().clone();
        state.settings.inspector_collapsed = vec!["files".into()];
        m.hydrate(state);
        assert!(key_of(&st, &m).is_none());
    }

    #[test]
    fn nothing_is_asked_while_the_column_is_hidden() {
        let st = State::default();
        let mut m = model();
        let mut state = m.state().clone();
        state.settings.inspector_open = false;
        m.hydrate(state);
        assert!(key_of(&st, &m).is_none());
        // Opened as a drawer, it is shown all the same.
        m.ui.drawer = Some(Drawer::Inspector);
        assert!(key_of(&st, &m).is_some());
    }

    #[test]
    fn nothing_is_asked_without_a_workspace() {
        let st = State::default();
        assert!(key_of(&st, &Model::default()).is_none());
    }

    #[test]
    fn an_answer_is_kept_for_the_current_look_and_dropped_for_an_old_one() {
        let mut st = State::default();
        let mut m = model();
        let current = key_of(&st, &m).unwrap();
        let changes = Arc::new(Changes {
            root: "/work/app".into(),
            ..Changes::default()
        });

        send(
            &mut st,
            &mut m,
            Msg::Changes(current.clone(), Ok(changes.clone())),
        );
        let (folder, kept) = st.changes.clone().expect("kept");
        assert_eq!(folder, "/work/app/packages/ui");
        assert!(Arc::ptr_eq(&kept, &changes));

        // The pane changed while git was thinking: that answer is not this
        // pane's.
        let stale = poll::PollKey {
            pane: "b".into(),
            ..current.clone()
        };
        let other = Arc::new(Changes {
            root: "/elsewhere".into(),
            ..Changes::default()
        });
        send(&mut st, &mut m, Msg::Changes(stale, Ok(other)));
        assert!(Arc::ptr_eq(&st.changes.as_ref().unwrap().1, &changes));

        // A failure empties the section's git half.
        send(
            &mut st,
            &mut m,
            Msg::Changes(current, Err("not a repository".into())),
        );
        assert!(st.changes.is_none());
    }

    #[test]
    fn the_window_coming_and_going_is_told_to_the_inspector() {
        let id = window::Id::unique();
        let status = event::Status::Ignored;
        assert!(matches!(
            window_focus(Event::Window(window::Event::Focused), status, id),
            Some(Message::Inspector(Msg::Away(false)))
        ));
        assert!(matches!(
            window_focus(Event::Window(window::Event::Unfocused), status, id),
            Some(Message::Inspector(Msg::Away(true)))
        ));
        assert!(window_focus(Event::Window(window::Event::Closed), status, id).is_none());
    }
}

mod requests {
    use super::*;

    #[test]
    fn what_other_packages_do_is_asked_for_as_an_intent() {
        assert!(matches!(
            request(&Msg::OpenBoard),
            Some(Intent::SetView(View::Board))
        ));
        assert!(matches!(
            request(&Msg::OpenSessions("w".into(), "a".into())),
            Some(Intent::OpenSessions { ws, pane }) if ws == "w" && pane == "a"
        ));
        assert!(matches!(
            request(&Msg::OpenMcpServers("w".into())),
            Some(Intent::OpenMcpServers(ws)) if ws == "w"
        ));
        assert!(matches!(
            request(&Msg::OpenDiff("w".into(), "a".into())),
            Some(Intent::OpenDiff { ws, pane: Some(pane) }) if ws == "w" && pane == "a"
        ));
    }

    #[test]
    fn what_is_the_inspector_s_own_is_not_an_intent() {
        for own in [
            Msg::Clock,
            Msg::Away(true),
            Msg::GripReleased,
            Msg::Toggle(SectionId::Files),
            Msg::CancelQueued("a".into(), "q1".into()),
        ] {
            assert!(request(&own).is_none(), "{own:?}");
        }
    }
}
