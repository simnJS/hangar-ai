//! What the inspector decides without drawing: when git is asked about, in
//! which windows, how the grip turns a drag into a width and how far, how
//! sections fold, and which requests leave for other packages; and what the
//! column shows of what the other packages know of a pane.
//!
//! The store itself is P0b's (its `apply` changes nothing yet), so what these
//! check is the part that is the inspector's own: the functions the patches
//! it sends are made of, and the state it keeps.

use iced::keyboard::{self, Key, key::Named};
use iced::{Event, event, window};

use super::*;
use crate::app::Packages;
use crate::navigator::testing::{ctx, simulate};
use crate::panes_logic;
use crate::shell::WindowKind;
use crate::state::live::{AgentActivity, PaneEventDetail, PaneMcp, PaneMcpMode};
use crate::state::types::{AgentId, AppState, Pane, Workspace};

fn pane(id: &str, cwd: Option<&str>) -> Pane {
    Pane {
        id: id.into(),
        name: id.to_uppercase(),
        cwd: cwd.map(str::to_string),
        ..Pane::default()
    }
}

/// A model with one workspace at `/work/app` holding panes `a` (in a folder
/// of its own) and `b`, shown in the main window.
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
    m.live
        .windows
        .insert(window::Id::unique(), WindowKind::Main);
    m
}

/// The main window of a model that has one.
fn main(m: &Model) -> window::Id {
    m.main_window().expect("a main window")
}

/// `model`, plus a workspace `d` at `/work/site` in a window of its own,
/// holding panes `d1` and `d2`; the main window shows `w`.
fn with_detached() -> (Model, window::Id) {
    let mut m = model();
    let mut state = m.state().clone();
    state.workspaces.push(Workspace {
        id: "d".into(),
        name: "site".into(),
        cwd: "/work/site".into(),
        panes: vec![pane("d1", None), pane("d2", Some("/work/site/web"))],
        detached: Some(true),
        ..Workspace::default()
    });
    m.hydrate(state);
    m.ui.focus_by_workspace.insert("d".into(), "d2".into());
    let own = window::Id::unique();
    m.live
        .windows
        .insert(own, WindowKind::Workspace("d".into()));
    (m, own)
}

/// The main window is too narrow for the column and the terminals together:
/// it can only be a drawer. `width` is the row the terminals share.
fn narrow(m: &mut Model, width: f32) {
    let w = main(m);
    m.ui.body_width.insert(w, width);
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
        assert_eq!(clamp_width(100.0, INSPECTOR_MAX), 240.0);
        assert_eq!(clamp_width(296.4, INSPECTOR_MAX), 296.0);
        assert_eq!(clamp_width(296.5, INSPECTOR_MAX), 297.0);
        assert_eq!(clamp_width(900.0, INSPECTOR_MAX), 440.0);
    }

    #[test]
    fn the_room_a_window_has_caps_the_width_but_never_below_the_minimum() {
        assert_eq!(clamp_width(400.0, 350.0), 350.0);
        assert_eq!(clamp_width(300.0, 350.0), 300.0);
        assert_eq!(clamp_width(100.0, 350.0), 240.0);
        // A room too small for the minimum leaves the minimum, as the TS did.
        assert_eq!(clamp_width(300.0, 100.0), 240.0);
        // And a room wider than the setting's bound is still the bound.
        assert_eq!(clamp_width(500.0, 900.0), 440.0);
    }

    #[test]
    fn dragging_left_widens_the_column_and_right_narrows_it() {
        // Pressed at x = 600 on a column 296 wide.
        let max = INSPECTOR_MAX;
        assert_eq!(dragged_width(296.0, 600.0, 560.0, max), 336.0);
        assert_eq!(dragged_width(296.0, 600.0, 640.0, max), 256.0);
        assert_eq!(dragged_width(296.0, 600.0, 600.0, max), 296.0);
        // Past the bounds it stops.
        assert_eq!(dragged_width(296.0, 600.0, 100.0, max), 440.0);
        assert_eq!(dragged_width(296.0, 600.0, 900.0, max), 240.0);
    }

    /// The width the frame is told to draw the column at while the grip is
    /// held, if it is the inspector's grip.
    fn held(m: &Model) -> Option<f32> {
        m.live
            .signals
            .panel_drag
            .filter(|drag| drag.panel == Drawer::Inspector)
            .map(|drag| drag.width)
    }

    #[test]
    fn a_drag_follows_the_pointer_only_between_the_press_and_the_release() {
        let mut st = State::default();
        let mut m = model();
        send(&mut st, &mut m, Msg::GripMoved(500.0));
        assert!(st.drag.is_none(), "moving without pressing starts nothing");
        assert_eq!(held(&m), None);

        send(
            &mut st,
            &mut m,
            Msg::GripPressed {
                x: 600.0,
                max: 440.0,
            },
        );
        let drag = st.drag.expect("pressed");
        assert_eq!(
            (drag.from_x, drag.from_width, drag.max),
            (600.0, 296.0, 440.0)
        );

        let revision = m.revision();
        send(&mut st, &mut m, Msg::GripMoved(580.0));
        send(&mut st, &mut m, Msg::GripMoved(560.0));
        // The frame draws the column where the pointer is; nothing is saved.
        assert_eq!(held(&m), Some(336.0));
        assert_eq!(Panels::of(&m, main(&m)).inspector.width, 336.0);
        assert_eq!(m.settings().inspector_width, 296.0);
        assert_eq!(m.revision(), revision, "no store action while it is held");

        send(&mut st, &mut m, Msg::GripReleased);
        assert!(st.drag.is_none());
        assert_eq!(held(&m), None);
        assert_eq!(m.settings().inspector_width, 336.0);
        assert_eq!(m.revision(), revision + 1, "written once, on release");
        send(&mut st, &mut m, Msg::GripMoved(500.0));
        assert_eq!(held(&m), None);
        assert_eq!(m.settings().inspector_width, 336.0, "released: it stays");
    }

    #[test]
    fn a_release_where_the_press_was_writes_nothing() {
        let mut st = State::default();
        let mut m = model();
        let revision = m.revision();
        send(
            &mut st,
            &mut m,
            Msg::GripPressed {
                x: 600.0,
                max: 440.0,
            },
        );
        send(&mut st, &mut m, Msg::GripMoved(560.0));
        send(&mut st, &mut m, Msg::GripMoved(600.0));
        send(&mut st, &mut m, Msg::GripReleased);
        assert_eq!(m.revision(), revision);
        assert_eq!(held(&m), None);
    }

    #[test]
    fn escape_drops_the_drag_and_the_column_keeps_its_width() {
        let mut st = State::default();
        let mut m = model();
        let revision = m.revision();
        send(
            &mut st,
            &mut m,
            Msg::GripPressed {
                x: 600.0,
                max: 440.0,
            },
        );
        send(&mut st, &mut m, Msg::GripMoved(500.0));
        assert_eq!(held(&m), Some(396.0));
        send(&mut st, &mut m, Msg::GripCancelled);
        assert!(st.drag.is_none());
        assert_eq!(held(&m), None);
        assert_eq!(Panels::of(&m, main(&m)).inspector.width, 296.0);
        assert_eq!(m.revision(), revision);
        // The release that follows is nobody's.
        send(&mut st, &mut m, Msg::GripReleased);
        assert_eq!(m.settings().inspector_width, 296.0);
    }

    #[test]
    fn letting_go_leaves_another_panel_s_held_grip_alone() {
        let mut st = State::default();
        let mut m = model();
        let dock = crate::state::PanelDrag {
            panel: Drawer::Dock,
            width: 500.0,
        };
        m.live.signals.panel_drag = Some(dock);
        send(&mut st, &mut m, Msg::GripReleased);
        send(&mut st, &mut m, Msg::GripCancelled);
        send(&mut st, &mut m, Msg::ResetWidth);
        assert_eq!(m.live.signals.panel_drag, Some(dock));
    }

    #[test]
    fn a_drag_stops_at_the_room_the_window_had_when_the_grip_was_pressed() {
        let mut st = State::default();
        let mut m = model();
        send(
            &mut st,
            &mut m,
            Msg::GripPressed {
                x: 600.0,
                max: 330.0,
            },
        );
        send(&mut st, &mut m, Msg::GripMoved(100.0));
        assert_eq!(held(&m), Some(330.0));
        send(&mut st, &mut m, Msg::GripMoved(900.0));
        assert_eq!(held(&m), Some(240.0));
        send(&mut st, &mut m, Msg::GripReleased);
        assert_eq!(m.settings().inspector_width, 240.0);
    }

    #[test]
    fn a_drag_starts_from_the_width_as_drawn_not_the_one_saved() {
        let mut st = State::default();
        let mut m = model();
        let mut state = m.state().clone();
        state.settings.inspector_width = 440.0;
        m.hydrate(state);
        // The room only allows 300: that is what the person sees and takes
        // hold of.
        send(
            &mut st,
            &mut m,
            Msg::GripPressed {
                x: 600.0,
                max: 300.0,
            },
        );
        assert_eq!(st.drag.expect("pressed").from_width, 300.0);
        send(&mut st, &mut m, Msg::GripMoved(610.0));
        assert_eq!(held(&m), Some(290.0));
        send(&mut st, &mut m, Msg::GripReleased);
        assert_eq!(m.settings().inspector_width, 290.0);
    }

    #[test]
    fn a_double_click_goes_back_to_the_default_width() {
        let mut st = State::default();
        let mut m = model();
        send(
            &mut st,
            &mut m,
            Msg::GripPressed {
                x: 600.0,
                max: 440.0,
            },
        );
        send(&mut st, &mut m, Msg::GripMoved(500.0));
        send(&mut st, &mut m, Msg::ResetWidth);
        assert!(st.drag.is_none());
        assert_eq!(held(&m), None);
        assert_eq!(
            m.settings().inspector_width,
            Settings::default().inspector_width
        );
    }

    #[test]
    fn the_app_going_to_the_back_ends_a_drag() {
        let mut st = State::default();
        let mut m = model();
        let w = main(&m);
        send(&mut st, &mut m, Msg::Focus(w, true));
        send(
            &mut st,
            &mut m,
            Msg::GripPressed {
                x: 600.0,
                max: 440.0,
            },
        );
        send(&mut st, &mut m, Msg::GripMoved(500.0));
        send(&mut st, &mut m, Msg::Focus(w, false));
        assert!(st.drag.is_none());
        assert!(st.away);
        // As Escape: dropped, not saved.
        assert_eq!(held(&m), None);
        assert_eq!(m.settings().inspector_width, 296.0);
    }

    #[test]
    fn the_arrows_nudge_inside_the_room_the_focused_window_has() {
        let mut st = State::default();
        let mut m = model();
        // A row of 940 leaves the terminals their 640 beside a column of at
        // most 300.
        narrow(&mut m, 940.0);
        send(&mut st, &mut m, Msg::Nudge(16));
        assert_eq!(m.settings().inspector_width, 300.0);
        send(&mut st, &mut m, Msg::Nudge(-16));
        assert_eq!(m.settings().inspector_width, 284.0);
    }

    #[test]
    fn a_nudge_in_a_drawer_starts_from_the_width_drawn() {
        let mut st = State::default();
        let mut m = model();
        let mut state = m.state().clone();
        state.settings.inspector_width = 400.0;
        m.hydrate(state);
        // A row of 300 leaves a drawer 252 wide, whatever was saved.
        narrow(&mut m, 300.0);
        send(&mut st, &mut m, Msg::Nudge(-16));
        assert_eq!(m.settings().inspector_width, 240.0, "252 - 16, held at 240");
        send(&mut st, &mut m, Msg::Nudge(16));
        assert_eq!(m.settings().inspector_width, 252.0, "never past the room");
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

    fn folders(st: &State, m: &Model) -> Vec<String> {
        poll_keys(st, m).into_iter().map(|key| key.folder).collect()
    }

    fn key_of(st: &State, m: &Model) -> Option<poll::PollKey> {
        poll_keys(st, m).into_iter().next()
    }

    #[test]
    fn git_is_asked_about_the_focused_pane_s_own_folder() {
        let (st, m) = (State::default(), model());
        let keys = poll_keys(&st, &m);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].pane, "a");
        assert_eq!(keys[0].folder, "/work/app/packages/ui");
    }

    #[test]
    fn a_pane_without_a_folder_of_its_own_uses_the_workspace_s() {
        let st = State::default();
        let mut m = model();
        m.ui.focus_by_workspace.insert("w".into(), "b".into());
        assert_eq!(key_of(&st, &m).unwrap().folder, "/work/app");
    }

    #[test]
    fn nothing_is_asked_while_the_app_is_away() {
        let mut st = State::default();
        let mut m = model();
        let w = main(&m);
        send(&mut st, &mut m, Msg::Focus(w, false));
        assert!(poll_keys(&st, &m).is_empty());
        send(&mut st, &mut m, Msg::Focus(w, true));
        assert!(!poll_keys(&st, &m).is_empty());
    }

    #[test]
    fn one_window_handing_the_focus_to_another_is_not_leaving() {
        let (mut m, own) = with_detached();
        let (first, second) = (main(&m), own);
        // The old window loses it just before the new one gains it...
        let mut st = State::default();
        send(&mut st, &mut m, Msg::Focus(first, true));
        send(&mut st, &mut m, Msg::Focus(first, false));
        assert!(st.away);
        send(&mut st, &mut m, Msg::Focus(second, true));
        assert!(!st.away);

        // ...or just after: the order of the two events is the system's.
        let mut st = State::default();
        send(&mut st, &mut m, Msg::Focus(first, true));
        send(&mut st, &mut m, Msg::Focus(second, true));
        send(&mut st, &mut m, Msg::Focus(first, false));
        assert!(!st.away, "the second window still has the focus");
        send(&mut st, &mut m, Msg::Focus(second, false));
        assert!(st.away);
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
        assert!(poll_keys(&st, &m).is_empty());
    }

    #[test]
    fn nothing_is_asked_while_the_column_is_closed() {
        let st = State::default();
        let mut m = model();
        let mut state = m.state().clone();
        state.settings.inspector_open = false;
        m.hydrate(state);
        assert!(poll_keys(&st, &m).is_empty());
        // The drawer flag means nothing to a column that has the room.
        m.ui.drawer = Some(Drawer::Inspector);
        assert!(poll_keys(&st, &m).is_empty());
    }

    #[test]
    fn a_column_the_room_turned_into_a_closed_drawer_is_not_on_screen() {
        let st = State::default();
        let mut m = model();
        let w = main(&m);
        // The setting says open, but 300 leaves no room beside the terminals.
        narrow(&mut m, 300.0);
        assert!(m.settings().inspector_open);
        assert!(!shown_in(&m, w));
        assert!(poll_keys(&st, &m).is_empty(), "no look at git for nobody");
        assert!(!sections::counts_time(&m, w));
        assert!(!focused_shows(&m));
        assert!(focus_entry(&st, &m, Region::Inspector).is_none());

        // Opened, it is on screen all the same.
        m.ui.drawer = Some(Drawer::Inspector);
        assert!(shown_in(&m, w));
        assert_eq!(poll_keys(&st, &m).len(), 1);
        assert!(focus_entry(&st, &m, Region::Inspector).is_some());
    }

    #[test]
    fn nothing_is_asked_without_a_workspace() {
        let st = State::default();
        let mut m = Model::default();
        m.live
            .windows
            .insert(window::Id::unique(), WindowKind::Main);
        assert!(poll_keys(&st, &m).is_empty());
        assert!(poll_keys(&st, &Model::default()).is_empty());
    }

    #[test]
    fn a_window_of_its_own_is_asked_about_its_own_workspace() {
        let st = State::default();
        let (m, _) = with_detached();
        // The main window shows `w`, the other window `d`: each has its pane.
        let mut asked = folders(&st, &m);
        asked.sort();
        assert_eq!(
            asked,
            ["/work/app/packages/ui", "/work/site/web"].map(String::from)
        );
    }

    #[test]
    fn the_main_window_never_borrows_a_workspace_shown_in_another() {
        let st = State::default();
        let (mut m, _) = with_detached();
        // `d` is the active workspace, but it lives in its own window.
        let mut state = m.state().clone();
        state.active_workspace_id = Some("d".into());
        m.hydrate(state);
        assert_eq!(folders(&st, &m), vec!["/work/site/web".to_string()]);
    }

    #[test]
    fn a_window_that_is_hidden_is_not_asked_for() {
        let st = State::default();
        let (mut m, own) = with_detached();
        // The workspace window has a drawer-only room, and it is closed.
        m.ui.body_width.insert(own, 300.0);
        assert_eq!(folders(&st, &m), vec!["/work/app/packages/ui".to_string()]);
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
        let kept = st.changes.get("/work/app/packages/ui").expect("kept");
        assert!(Arc::ptr_eq(&kept.changes, &changes));

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
        assert!(Arc::ptr_eq(
            &st.changes.get("/work/app/packages/ui").unwrap().changes,
            &changes
        ));

        // A failure empties the section's git half.
        send(
            &mut st,
            &mut m,
            Msg::Changes(current, Err("not a repository".into())),
        );
        assert!(st.changes.is_empty());
    }

    #[test]
    fn two_windows_keep_their_own_answers_side_by_side() {
        let mut st = State::default();
        let (mut m, _) = with_detached();
        let keys = poll_keys(&st, &m);
        assert_eq!(keys.len(), 2);
        for key in &keys {
            let changes = Arc::new(Changes {
                root: key.folder.clone(),
                ..Changes::default()
            });
            send(&mut st, &mut m, Msg::Changes(key.clone(), Ok(changes)));
        }
        assert_eq!(st.changes.len(), 2);
        for key in &keys {
            assert_eq!(st.changes[&key.folder].root, key.folder);
        }
    }

    #[test]
    fn what_no_window_looks_at_any_more_is_let_go() {
        let mut st = State::default();
        let (mut m, _) = with_detached();
        for key in poll_keys(&st, &m) {
            let changes = Arc::new(Changes::default());
            send(&mut st, &mut m, Msg::Changes(key, Ok(changes)));
        }
        assert_eq!(st.changes.len(), 2);
        // The workspace window closes; the next answer prunes its folder.
        m.live.windows.retain(|_, kind| *kind == WindowKind::Main);
        let key = key_of(&st, &m).unwrap();
        send(
            &mut st,
            &mut m,
            Msg::Changes(key, Ok(Arc::new(Changes::default()))),
        );
        assert_eq!(
            st.changes.keys().collect::<Vec<_>>(),
            ["/work/app/packages/ui"]
        );
    }

    #[test]
    fn the_windows_coming_and_going_are_told_to_the_inspector() {
        let id = window::Id::unique();
        let status = event::Status::Ignored;
        assert!(matches!(
            window_focus(Event::Window(window::Event::Focused), status, id),
            Some(Message::Inspector(Msg::Focus(who, true))) if who == id
        ));
        assert!(matches!(
            window_focus(Event::Window(window::Event::Unfocused), status, id),
            Some(Message::Inspector(Msg::Focus(who, false))) if who == id
        ));
        // A window that closes has the focus no more.
        assert!(matches!(
            window_focus(Event::Window(window::Event::Closed), status, id),
            Some(Message::Inspector(Msg::Focus(who, false))) if who == id
        ));
        assert!(
            window_focus(
                Event::Window(window::Event::RedrawRequested(std::time::Instant::now())),
                status,
                id
            )
            .is_none()
        );
    }
}

mod clock {
    use super::*;

    fn since(m: &mut Model, pane: &str) {
        m.live
            .panes_logic
            .seed_insight(pane, |insight| insight.session_since = Some(1_000));
    }

    fn agent(m: &mut Model, ws: &str, pane: &str) {
        let mut state = m.state().clone();
        let found = state
            .workspaces
            .iter_mut()
            .find(|w| w.id == ws)
            .and_then(|w| w.panes.iter_mut().find(|p| p.id == pane))
            .expect("the pane");
        found.agent = AgentId::Claude;
        m.hydrate(state);
    }

    #[test]
    fn the_clock_runs_for_a_window_that_shows_a_session_start() {
        let mut m = model();
        agent(&mut m, "w", "a");
        let w = main(&m);
        assert!(!sections::counts_time(&m, w), "nothing counts time yet");
        since(&mut m, "a");
        assert!(sections::counts_time(&m, w));
    }

    #[test]
    fn each_window_counts_for_the_pane_it_shows() {
        let (mut m, own) = with_detached();
        agent(&mut m, "w", "a");
        agent(&mut m, "d", "d2");
        let main = main(&m);
        // The detached window's focused pane has the start time, the main
        // window's has not.
        since(&mut m, "d2");
        assert!(sections::counts_time(&m, own));
        assert!(!sections::counts_time(&m, main));
        // So the clock is wanted, and by the window that needs it.
        assert_eq!(windows_showing(&m).len(), 2);
    }

    #[test]
    fn a_folded_section_counts_nothing() {
        let mut m = model();
        agent(&mut m, "w", "a");
        since(&mut m, "a");
        let w = main(&m);
        let mut state = m.state().clone();
        state.settings.inspector_collapsed = vec!["session".into()];
        m.hydrate(state);
        assert!(!sections::counts_time(&m, w));
    }

    #[test]
    fn a_column_the_room_hides_counts_nothing() {
        let mut m = model();
        agent(&mut m, "w", "a");
        since(&mut m, "a");
        narrow(&mut m, 300.0);
        let w = main(&m);
        assert!(!sections::counts_time(&m, w), "the drawer is closed");
        m.ui.drawer = Some(Drawer::Inspector);
        assert!(sections::counts_time(&m, w));
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
            Msg::Focus(window::Id::unique(), true),
            Msg::GripReleased,
            Msg::GripCancelled,
            Msg::Toggle(SectionId::Files),
            Msg::CancelQueued("a".into(), "q1".into()),
        ] {
            assert!(request(&own).is_none(), "{own:?}");
        }
    }
}

/// The column drawn from the model, with what the other packages know of a
/// pane: nothing here is handed to the sections directly.
mod drawn {
    use super::*;

    fn claude_model() -> Model {
        let mut m = model();
        let mut state = m.state().clone();
        state.workspaces[0].panes[0].agent = AgentId::Claude;
        state.workspaces[0].panes[1].agent = AgentId::Shell;
        m.hydrate(state);
        m
    }

    /// Whether `words` is anywhere in the column drawn in `window`.
    fn draws(m: &Model, window: window::Id, words: &str) -> bool {
        let (st, pkg) = (State::default(), Packages::default());
        let ctx = crate::ctx::Ctx {
            window,
            ..ctx(m, &pkg)
        };
        simulate(view(&st, ctx)).find(words).is_ok()
    }

    fn servers(names: &[&str]) -> PaneMcp {
        PaneMcp {
            mode: PaneMcpMode::Custom {
                servers: names.iter().map(|name| (*name).to_owned()).collect(),
            },
            hangar: Some(true),
        }
    }

    #[test]
    fn the_mcp_row_shows_the_servers_the_launch_recorded() {
        let mut m = claude_model();
        let w = main(&m);
        // Nothing launched yet: no row at all.
        assert!(!draws(&m, w, "MCP"));

        m.live
            .panes_logic
            .set_launch_mcp("a", Some(servers(&["hangar", "github"])));
        assert!(draws(&m, w, "MCP"));
        assert!(draws(&m, w, "Change…"));
        // Its own servers, not Claude Code's: the servers are the value, and
        // they read as the launch's list.
        assert!(!draws(&m, w, "Claude Code's own"));
        let recorded = sources::insight(&m, &"a".to_string()).mcp.clone().unwrap();
        assert_eq!(
            sections::mcp_servers(&recorded).as_deref(),
            Some("hangar, github")
        );
    }

    #[test]
    fn a_launch_on_claude_codes_own_servers_says_so() {
        let mut m = claude_model();
        let w = main(&m);
        m.live.panes_logic.set_launch_mcp(
            "a",
            Some(PaneMcp {
                mode: PaneMcpMode::Inherit,
                hangar: Some(false),
            }),
        );
        assert!(draws(&m, w, "MCP"));
        assert!(draws(&m, w, "Claude Code's own"));
    }

    #[test]
    fn only_claude_code_has_servers_to_show() {
        let mut m = claude_model();
        let w = main(&m);
        // The shell pane, in focus, has no MCP row whatever was recorded.
        m.ui.focus_by_workspace.insert("w".into(), "b".into());
        m.live
            .panes_logic
            .set_launch_mcp("b", Some(servers(&["hangar"])));
        assert!(!draws(&m, w, "MCP"));
    }

    #[test]
    fn a_relaunch_replaces_the_row_and_one_that_could_not_say_takes_it_away() {
        let mut m = claude_model();
        let w = main(&m);
        m.live
            .panes_logic
            .set_launch_mcp("a", Some(servers(&["hangar", "github"])));
        assert!(draws(&m, w, "MCP") && !draws(&m, w, "Claude Code's own"));
        // The next launch was on Claude Code's own servers.
        m.live.panes_logic.set_launch_mcp(
            "a",
            Some(PaneMcp {
                mode: PaneMcpMode::Inherit,
                hangar: Some(false),
            }),
        );
        assert!(draws(&m, w, "Claude Code's own"));
        // And one that could not say leaves no row.
        m.live.panes_logic.set_launch_mcp("a", None);
        assert!(!draws(&m, w, "MCP"));
    }

    #[test]
    fn the_row_lists_the_names_a_custom_launch_loaded_and_nothing_for_claude_codes_own() {
        let custom = servers(&["hangar", "github", "docs"]);
        assert_eq!(
            sections::mcp_servers(&custom).as_deref(),
            Some("hangar, github, docs")
        );
        let own = PaneMcp {
            mode: PaneMcpMode::Inherit,
            hangar: None,
        };
        assert_eq!(sections::mcp_servers(&own), None);
    }

    #[test]
    fn the_state_and_the_history_come_from_the_pane_read_model() {
        let mut m = claude_model();
        let w = main(&m);
        let a = "a".to_string();
        // The head's glyph and word come from the activity P4 publishes.
        assert!(sources::activity(&m, &a).is_none());
        m.live
            .panes_logic
            .seed_activity("a", "w", AgentActivity::Working);
        let seen = sources::activity(&m, &a).expect("published");
        assert_eq!(seen.activity, AgentActivity::Working);
        assert!(seen.bridged);

        let line = panes_logic::pane_event_text(
            &PaneEventDetail::Edit {
                file: "Cart.tsx".into(),
                overlap: None,
            },
            hangar_i18n::Translator::new(hangar_i18n::Locale::En),
        );
        m.live.panes_logic.seed_insight("a", |insight| {
            insight.events.push(crate::state::live::PaneEvent {
                at: 1_000,
                detail: PaneEventDetail::Edit {
                    file: "Cart.tsx".into(),
                    overlap: None,
                },
            });
        });
        assert!(draws(&m, w, &line));
    }

    #[test]
    fn each_window_draws_the_pane_of_its_own_workspace() {
        let (mut m, own) = with_detached();
        let mut state = m.state().clone();
        // `w`'s focused pane is `a`, a Claude Code; the detached workspace's
        // is `d2`, a shell.
        state.workspaces[0].panes[0].agent = AgentId::Claude;
        state.workspaces[1].panes[1].agent = AgentId::Shell;
        m.hydrate(state);
        let main = main(&m);
        let t = hangar_i18n::Translator::new(hangar_i18n::Locale::En);
        let session = t.t(hangar_i18n::Key::InspectorSectionSession);
        let terminal = t.t(hangar_i18n::Key::InspectorSectionTerminal);
        assert!(draws(&m, main, &session));
        assert!(!draws(&m, main, &terminal));
        assert!(draws(&m, own, &terminal));
        assert!(!draws(&m, own, &session));
    }

    #[test]
    fn the_column_is_told_the_window_s_room_for_its_grip() {
        // The grip carries the widest the window allows, read where the
        // column is drawn: here a window of 940 leaves 300.
        let mut m = claude_model();
        narrow(&mut m, 940.0);
        let w = main(&m);
        let panels = Panels::of(&m, w);
        assert_eq!(panels.inspector.max, 300.0);
        assert_eq!(panels.inspector.width, 296.0);
    }
}
