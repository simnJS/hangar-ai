//! `StatusBar.tsx` had no tests; these hold what its comments promise: the
//! repository object is kept while nothing moved, an answer for another
//! workspace is not taken for this one's, and, now that every window has a bar
//! of its own, what is typed, sent and read stays with its window.

use super::*;
use crate::navigator::testing::{detach, model_of, pane, workspace};
use crate::state::types::{AgentId, AppState};

fn repo(branch: Option<&str>) -> Repo {
    Repo {
        branch: branch.map(str::to_string),
        head: Some("a1b2c3d".into()),
        root: Some("/dev/app".into()),
        is_linked_worktree: false,
    }
}

fn info(installed: bool, is_repo: bool) -> RepoInfo {
    RepoInfo {
        installed,
        is_repo,
        root: Some("/dev/app".into()),
        branch: Some("main".into()),
        head: Some("a1b2c3d".into()),
        ..RepoInfo::default()
    }
}

#[test]
fn a_folder_in_a_repository_has_one() {
    let found = Repo::of(info(true, true)).unwrap();
    assert_eq!(found.branch.as_deref(), Some("main"));
    assert_eq!(found.root.as_deref(), Some("/dev/app"));
}

#[test]
fn a_folder_outside_any_repository_or_without_git_has_none() {
    assert_eq!(Repo::of(info(true, false)), None);
    assert_eq!(Repo::of(info(false, false)), None);
}

#[test]
fn the_same_branch_head_and_root_change_nothing() {
    let mut st = State::default();
    assert!(st.take_repo("/dev/app", Some(repo(Some("main")))));
    assert!(!st.take_repo("/dev/app", Some(repo(Some("main")))));
    assert!(st.take_repo("/dev/app", Some(repo(Some("feature")))));
}

#[test]
fn the_same_repository_for_another_folder_is_another_answer() {
    let mut st = State::default();
    st.take_repo("/dev/app", Some(repo(Some("main"))));
    assert!(st.take_repo("/dev/app/sub", Some(repo(Some("main")))));
    // Each folder keeps its own answer: two windows on two folders do not
    // take turns overwriting one.
    assert_eq!(st.repos.len(), 2);
    assert!(!st.take_repo("/dev/app", Some(repo(Some("main")))));
}

#[test]
fn leaving_a_repository_is_a_change() {
    let mut st = State::default();
    st.take_repo("/dev/app", Some(repo(Some("main"))));
    assert!(st.take_repo("/dev/app", None));
    assert_eq!(st.repos.get("/dev/app"), Some(&None));
}

#[test]
fn a_detached_head_has_no_branch() {
    let found = Repo::of(RepoInfo {
        branch: None,
        ..info(true, true)
    })
    .unwrap();
    assert_eq!(found.branch, None);
    assert_eq!(found.head.as_deref(), Some("a1b2c3d"));
}

// ---- One bar per window ------------------------------------------------------------------------

/// Workspace `a` in the main window, workspace `b` in a window of its own.
fn two_windows() -> (Model, window::Id, window::Id) {
    let mut model = model_of(AppState {
        workspaces: vec![
            workspace("a", "alpha", None, vec![pane("a1", "Ava", AgentId::Claude)]),
            workspace("b", "beta", None, vec![pane("b1", "Bo", AgentId::Claude)]),
        ],
        active_workspace_id: Some("a".into()),
        ..AppState::default()
    });
    let main = model.main_window().unwrap();
    let own = detach(&mut model, "b");
    (model, main, own)
}

#[test]
fn each_window_holds_its_own_line() {
    let (_, main, own) = two_windows();
    let mut st = State::default();
    st.set_broadcast_text(main, "run the tests".into());
    assert_eq!(st.broadcast_text(main), "run the tests");
    assert_eq!(st.broadcast_text(own), "");

    st.set_broadcast_text(own, "ls -la".into());
    assert_eq!(st.broadcast_text(main), "run the tests");
    assert_eq!(st.broadcast_text(own), "ls -la");
}

#[test]
fn a_send_from_a_workspace_window_reaches_that_workspace_and_empties_only_its_field() {
    let (model, main, own) = two_windows();
    let mut st = State::default();
    st.set_broadcast_text(main, "main's line".into());
    st.set_broadcast_text(own, "  the window's line  ".into());

    match send(&mut st, &model, own, "b".into(), true) {
        Some(Intent::Broadcast {
            ws,
            text,
            when_free,
        }) => {
            // The workspace of the window, not the main window's active one.
            assert_eq!(ws, "b");
            assert_eq!(text, "the window's line");
            assert!(when_free);
        }
        other => panic!("expected a broadcast, got {other:?}"),
    }
    assert_eq!(st.broadcast_text(own), "");
    assert_eq!(st.broadcast_text(main), "main's line");
}

#[test]
fn an_empty_line_or_a_workspace_that_is_gone_sends_nothing_and_keeps_the_field() {
    let (model, main, _) = two_windows();
    let mut st = State::default();
    assert!(send(&mut st, &model, main, "a".into(), false).is_none());
    st.set_broadcast_text(main, "   ".into());
    assert!(send(&mut st, &model, main, "a".into(), false).is_none());

    st.set_broadcast_text(main, "hello".into());
    assert!(send(&mut st, &model, main, "gone".into(), false).is_none());
    assert_eq!(st.broadcast_text(main), "hello");
}

#[test]
fn the_branch_is_read_for_the_folder_of_each_window_that_has_the_focus() {
    let (model, main, own) = two_windows();
    let mut st = State::default();
    let both: Vec<String> = beats(&st, &model).into_iter().collect();
    assert_eq!(both, ["/dev/a", "/dev/b"]);

    // The main window lost the focus: its folder is not asked about.
    st.unfocused.insert(main);
    let only_own: Vec<String> = beats(&st, &model).into_iter().collect();
    assert_eq!(only_own, ["/dev/b"]);

    st.unfocused.insert(own);
    assert!(beats(&st, &model).is_empty());
}

#[test]
fn the_main_window_asks_about_nothing_while_its_workspace_is_in_a_window_of_its_own() {
    let (mut model, _, _) = two_windows();
    // The workspace the main window is on is the detached one: the main
    // window draws none, and its folder is asked about once, for the window
    // that does.
    let mut state = model.state().clone();
    state.active_workspace_id = Some("b".into());
    model.hydrate(state);
    let st = State::default();
    let folders: Vec<String> = beats(&st, &model).into_iter().collect();
    assert_eq!(folders, ["/dev/b"]);
}
