//! `StatusBar.tsx` had no tests; these hold what its comments promise: the
//! repository object is kept while nothing moved, an answer for another
//! workspace is not taken for this one's, and a pane counts as bridged from
//! the mod's first word to its last.

use serde_json::json;

use super::*;

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
    assert_eq!(st.repo_cwd, "/dev/app/sub");
}

#[test]
fn leaving_a_repository_is_a_change() {
    let mut st = State::default();
    st.take_repo("/dev/app", Some(repo(Some("main"))));
    assert!(st.take_repo("/dev/app", None));
    assert_eq!(st.repo, None);
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

fn pane(id: &str) -> crate::state::Pane {
    crate::state::Pane {
        id: id.into(),
        ..crate::state::Pane::default()
    }
}

#[test]
fn a_pane_is_bridged_from_the_mods_first_word_to_its_last() {
    let mut st = State::default();
    let panes = [pane("a"), pane("b")];
    assert_eq!(st.bridged_among(&panes), 0);

    st.note_bridge(
        &"a".to_string(),
        &json!({"kind": "hello", "sessionId": "s", "model": "m", "version": "2"}),
    );
    assert_eq!(st.bridged_among(&panes), 1);

    st.note_bridge(&"b".to_string(), &json!({"kind": "turn", "phase": "start"}));
    assert_eq!(st.bridged_among(&panes), 2);

    st.note_bridge(&"a".to_string(), &json!({"kind": "end", "reason": "exit"}));
    assert_eq!(st.bridged_among(&panes), 1);
}

#[test]
fn a_report_this_version_does_not_know_says_nothing() {
    let mut st = State::default();
    st.note_bridge(&"a".to_string(), &json!({"kind": "from-the-future"}));
    assert_eq!(st.bridged_among(&[pane("a")]), 0);
}
