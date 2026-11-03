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
