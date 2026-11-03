//! How the package sets its overlays up from the intents the shell hands it.

use super::*;
use crate::state::types::{AppState, Pane, Workspace};

fn model() -> Model {
    let mut m = Model::default();
    m.hydrate(AppState {
        workspaces: vec![Workspace {
            id: "w".into(),
            name: "storefront".into(),
            cwd: "/work/storefront".into(),
            panes: vec![
                Pane {
                    id: "a".into(),
                    name: "Ava".into(),
                    cwd: Some("/work/storefront-worktrees/feat-x".into()),
                    ..Pane::default()
                },
                Pane {
                    id: "b".into(),
                    name: "Ben".into(),
                    ..Pane::default()
                },
            ],
            ..Workspace::default()
        }],
        active_workspace_id: Some("w".into()),
        ..AppState::default()
    });
    m
}

mod what_a_diff_is_about {
    use super::*;

    fn target(pane: Option<&str>) -> Option<(String, String)> {
        diff_target(
            &model(),
            &"w".to_string(),
            pane.map(str::to_string).as_ref(),
        )
    }

    #[test]
    fn a_pane_with_a_folder_of_its_own_is_read_there_and_named_after_itself() {
        assert_eq!(
            target(Some("a")),
            Some(("/work/storefront-worktrees/feat-x".into(), "Ava".into()))
        );
    }

    #[test]
    fn a_pane_without_one_is_read_in_the_workspace_s_folder() {
        assert_eq!(
            target(Some("b")),
            Some(("/work/storefront".into(), "Ben".into()))
        );
    }

    #[test]
    fn no_pane_is_the_workspace_itself() {
        assert_eq!(
            target(None),
            Some(("/work/storefront".into(), "storefront".into()))
        );
        // A pane that no longer exists is not an error either.
        assert_eq!(
            target(Some("gone")),
            Some(("/work/storefront".into(), "storefront".into()))
        );
    }

    #[test]
    fn a_workspace_that_is_gone_has_no_changes_to_show() {
        let m = model();
        assert_eq!(diff_target(&m, &"nobody".to_string(), None), None);
    }
}

mod opening {
    use super::*;

    #[test]
    fn every_opening_is_a_new_generation_so_old_answers_are_told() {
        let (m, mut st) = (model(), State::default());
        let _ = open_diff(&mut st, &m, &"w".to_string(), None);
        let first = st.generations;
        let _ = open_diff(&mut st, &m, &"w".to_string(), Some(&"a".to_string()));
        assert!(st.generations > first);
        assert_eq!(
            st.diff.as_ref().unwrap().cwd,
            "/work/storefront-worktrees/feat-x"
        );
        let _ = open_worktrees(&mut st, &m, &"w".to_string());
        assert!(st.generations > first + 1);
        let panel = st.worktrees.as_ref().unwrap();
        assert_eq!(panel.cwd, "/work/storefront");
        assert_eq!(panel.ws, "w");
    }

    #[test]
    fn a_workspace_that_is_gone_opens_nothing() {
        let (m, mut st) = (model(), State::default());
        let _ = open_diff(&mut st, &m, &"nobody".to_string(), None);
        assert!(st.diff.is_none());
        let _ = open_worktrees(&mut st, &m, &"nobody".to_string());
        assert!(st.worktrees.is_none());
    }

    #[test]
    fn closing_the_diff_takes_its_overlay_only_when_it_is_in_front() {
        let mut m = model();
        m.ui.overlays = vec![
            Overlay::Worktrees("w".into()),
            Overlay::Diff {
                ws: "w".into(),
                pane: None,
            },
        ];
        close_diff(&mut m);
        assert_eq!(m.ui.overlays, vec![Overlay::Worktrees("w".into())]);
        // The panel is in front now: a late "close the diff" leaves it alone.
        close_diff(&mut m);
        assert_eq!(m.ui.overlays, vec![Overlay::Worktrees("w".into())]);
    }
}

/// The overlays as the shell draws them (`shell::overlay::view`): in the
/// window's stack, over the page, asking for themselves to be closed when
/// Escape is pressed.
mod as_the_shell_draws_them {
    use iced::keyboard::{self, Key, key::Named};
    use iced::{Event, Settings, Size, window};
    use iced_test::Simulator;

    use super::*;
    use crate::app::Packages;
    use crate::ctx::{Ctx, Metrics};

    fn escape() -> Event {
        let key = Key::Named(Named::Escape);
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
            text: None,
            repeat: false,
        })
    }

    /// What the overlay answers to Escape.
    fn after_escape(packages: &Packages, m: &Model, overlay: &Overlay) -> Vec<Message> {
        let ctx = Ctx {
            model: m,
            pkg: packages,
            i18n: hangar_i18n::Translator::new(hangar_i18n::Locale::En),
            theme: hangar_theme::default_theme(),
            metrics: Metrics::default(),
            keymap: &m.live.keymap.index,
            window: window::Id::unique(),
            now: std::time::Instant::now(),
        };
        let mut ui = Simulator::with_size(
            Settings::default(),
            Size::new(900.0, 700.0),
            crate::shell::overlay::view(ctx, overlay),
        );
        let _ = ui.simulate([escape()]);
        ui.into_messages().collect()
    }

    #[test]
    fn escape_closes_the_changes_overlay() {
        let m = model();
        let mut packages = Packages::default();
        let _ = open_diff(&mut packages.git_ui, &m, &"w".to_string(), None);
        let overlay = Overlay::Diff {
            ws: "w".into(),
            pane: None,
        };
        let answered = after_escape(&packages, &m, &overlay);
        assert!(
            answered
                .iter()
                .any(|message| matches!(message, Message::GitUi(Msg::CloseDiff))),
            "{answered:?}"
        );
    }

    #[test]
    fn escape_closes_the_worktree_panel() {
        let m = model();
        let mut packages = Packages::default();
        let _ = open_worktrees(&mut packages.git_ui, &m, &"w".to_string());
        let answered = after_escape(&packages, &m, &Overlay::Worktrees("w".into()));
        assert!(
            answered
                .iter()
                .any(|message| matches!(message, Message::GitUi(Msg::Wt(WtMsg::Close)))),
            "{answered:?}"
        );
    }

    #[test]
    fn escape_over_a_row_s_changes_gives_the_panel_back_instead_of_closing_it() {
        let m = model();
        let mut packages = Packages::default();
        let _ = open_worktrees(&mut packages.git_ui, &m, &"w".to_string());
        let panel = packages.git_ui.worktrees.as_mut().unwrap();
        let _ = worktrees::update(
            panel,
            &mut model(),
            WtMsg::ShowChanges("/work/storefront-worktrees/feat-x".into()),
        );
        let answered = after_escape(&packages, &m, &Overlay::Worktrees("w".into()));
        assert!(
            answered
                .iter()
                .any(|message| matches!(message, Message::GitUi(Msg::Wt(WtMsg::CloseDiff)))),
            "{answered:?}"
        );
        assert!(
            !answered
                .iter()
                .any(|message| matches!(message, Message::GitUi(Msg::Wt(WtMsg::Close)))),
            "{answered:?}"
        );
    }

    #[test]
    fn an_overlay_whose_state_is_not_set_up_draws_nothing_and_does_not_panic() {
        let m = model();
        let packages = Packages::default();
        let answered = after_escape(&packages, &m, &Overlay::Worktrees("w".into()));
        assert!(answered.is_empty());
    }
}
