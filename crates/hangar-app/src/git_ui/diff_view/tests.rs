//! What the diff view decides without drawing: which answers it accepts,
//! what it selects, what it caches, which rows are in view.

use super::*;

fn info(is_repo: bool, linked: bool, branch: Option<&str>) -> RepoInfo {
    RepoInfo {
        installed: true,
        is_repo,
        root: Some("/work/app".into()),
        main_root: Some("/work/app".into()),
        is_linked_worktree: linked,
        branch: branch.map(str::to_string),
        head: Some("abc1234".into()),
    }
}

fn file(path: &str, status: &str) -> ChangedFile {
    ChangedFile {
        path: path.into(),
        old_path: None,
        status: status.into(),
        additions: Some(3),
        deletions: Some(1),
        binary: false,
    }
}

fn changes(files: Vec<ChangedFile>) -> Arc<Changes> {
    Arc::new(Changes {
        root: "/work/app".into(),
        base: Some("abc1234".into()),
        files,
        truncated: false,
    })
}

/// A state that has been through its setup, on opening number 1.
fn ready(main_branch: Option<&str>) -> DiffState {
    let (mut state, _first) = DiffState::open("/work/app".into(), Some("Ava".into()), 1);
    let _ = state.update(DiffMsg::Setup(
        1,
        Box::new((
            info(true, main_branch.is_some(), Some("feat/x")),
            main_branch.map(str::to_string),
        )),
    ));
    state
}

/// The state with a listing in it, as the first load answered.
fn listed(files: Vec<ChangedFile>) -> DiffState {
    let mut state = ready(None);
    let seq = state.load_seq;
    let _ = state.update(DiffMsg::Listed(1, seq, Ok(changes(files))));
    state
}

fn read(text: &str) -> Arc<FileDiff> {
    Arc::new(FileDiff {
        text: text.into(),
        binary: false,
        truncated: false,
    })
}

mod setup {
    use super::*;

    #[test]
    fn a_folder_that_is_not_a_repository_asks_for_no_listing() {
        let (mut state, _) = DiffState::open("/tmp".into(), None, 1);
        let _ = state.update(DiffMsg::Setup(
            1,
            Box::new((info(false, false, None), None)),
        ));
        assert!(!state.ready());
        assert!(!state.loading);
        assert_eq!(state.load_seq, 0);
    }

    #[test]
    fn a_repository_is_read_at_once() {
        let state = ready(None);
        assert!(state.ready());
        assert!(state.loading);
        assert_eq!(state.load_seq, 1);
        assert_eq!(state.base, None);
    }

    #[test]
    fn a_worktree_on_its_own_branch_starts_compared_with_the_main_one() {
        let state = ready(Some("main"));
        assert_eq!(state.base.as_deref(), Some("main"));
        assert!(matches!(
            &state.setup,
            Setup::Ready { main_branch: Some(main), .. } if main == "main"
        ));
    }

    #[test]
    fn an_answer_for_another_opening_is_not_this_one_s() {
        let (mut state, _) = DiffState::open("/work/app".into(), None, 2);
        let _ = state.update(DiffMsg::Setup(1, Box::new((info(true, false, None), None))));
        assert!(matches!(state.setup, Setup::Loading));
    }
}

mod listing {
    use super::*;

    #[test]
    fn the_first_file_is_selected_when_nothing_was() {
        let state = listed(vec![file("a.rs", "M"), file("b.rs", "A")]);
        assert_eq!(state.selected.as_deref(), Some("\u{0}a.rs"));
        assert!(!state.loading);
        assert_eq!(state.files().len(), 2);
    }

    #[test]
    fn a_reload_keeps_the_selection_while_its_file_is_still_listed() {
        let mut state = listed(vec![file("a.rs", "M"), file("b.rs", "A")]);
        let _ = state.update(DiffMsg::Select("\u{0}b.rs".into()));
        let _ = state.update(DiffMsg::Reload);
        let seq = state.load_seq;
        let _ = state.update(DiffMsg::Listed(
            1,
            seq,
            Ok(changes(vec![file("b.rs", "M"), file("c.rs", "A")])),
        ));
        assert_eq!(state.selected.as_deref(), Some("\u{0}b.rs"));
    }

    #[test]
    fn a_reload_that_lost_the_file_falls_to_the_first() {
        let mut state = listed(vec![file("a.rs", "M"), file("b.rs", "A")]);
        let _ = state.update(DiffMsg::Select("\u{0}b.rs".into()));
        let _ = state.update(DiffMsg::Reload);
        let seq = state.load_seq;
        let _ = state.update(DiffMsg::Listed(
            1,
            seq,
            Ok(changes(vec![file("z.rs", "M")])),
        ));
        assert_eq!(state.selected.as_deref(), Some("\u{0}z.rs"));
    }

    #[test]
    fn an_empty_listing_selects_nothing() {
        let state = listed(vec![]);
        assert_eq!(state.selected, None);
        assert!(state.current().is_none());
        assert!(state.shown.is_none());
    }

    #[test]
    fn an_older_answer_arriving_late_is_dropped() {
        let mut state = ready(None);
        let first = state.load_seq;
        // Toggling the base twice quickly races two listings; only the last
        // one asked for may describe what the toggle shows.
        let _ = state.update(DiffMsg::SetBase(Some("main".into())));
        assert_eq!(state.load_seq, first + 1);
        let _ = state.update(DiffMsg::Listed(
            1,
            first,
            Ok(changes(vec![file("old.rs", "M")])),
        ));
        assert!(state.changes.is_none(), "the older listing was kept");
        assert!(state.loading);
        let latest = state.load_seq;
        let _ = state.update(DiffMsg::Listed(
            1,
            latest,
            Ok(changes(vec![file("new.rs", "M")])),
        ));
        assert_eq!(state.files()[0].path, "new.rs");
        assert!(!state.loading);
    }

    #[test]
    fn a_failure_shows_its_message_and_no_files() {
        let mut state = ready(None);
        let seq = state.load_seq;
        let _ = state.update(DiffMsg::Listed(1, seq, Err("fatal: bad object".into())));
        assert_eq!(state.error.as_deref(), Some("fatal: bad object"));
        assert!(state.changes.is_none());
        assert!(!state.loading);
    }

    #[test]
    fn choosing_the_base_already_chosen_reads_nothing() {
        let mut state = ready(None);
        let seq = state.load_seq;
        let _ = state.update(DiffMsg::SetBase(None));
        assert_eq!(state.load_seq, seq);
        let _ = state.update(DiffMsg::SetBase(Some("main".into())));
        assert_eq!(state.load_seq, seq + 1);
        assert_eq!(state.base.as_deref(), Some("main"));
    }

    #[test]
    fn refresh_waits_for_the_reading_in_progress() {
        let mut state = ready(None);
        let seq = state.load_seq;
        let _ = state.update(DiffMsg::Reload);
        assert_eq!(state.load_seq, seq, "a refresh while loading is ignored");
        let _ = state.update(DiffMsg::Listed(1, seq, Ok(changes(vec![]))));
        let _ = state.update(DiffMsg::Reload);
        assert_eq!(state.load_seq, seq + 1);
    }
}

mod keys {
    use super::*;

    fn three() -> DiffState {
        listed(vec![
            file("a.rs", "M"),
            file("b.rs", "M"),
            file("c.rs", "M"),
        ])
    }

    #[test]
    fn down_and_up_walk_the_list_and_stop_at_its_ends() {
        let mut state = three();
        let path = |state: &DiffState| state.current().map(|file| file.path.clone());
        assert_eq!(path(&state), Some("a.rs".into()));
        let _ = state.update(DiffMsg::Step(1));
        assert_eq!(path(&state), Some("b.rs".into()));
        let _ = state.update(DiffMsg::Step(1));
        let _ = state.update(DiffMsg::Step(1));
        assert_eq!(path(&state), Some("c.rs".into()));
        let _ = state.update(DiffMsg::Step(-1));
        let _ = state.update(DiffMsg::Step(-1));
        let _ = state.update(DiffMsg::Step(-1));
        assert_eq!(path(&state), Some("a.rs".into()));
    }

    #[test]
    fn stepping_without_files_does_nothing() {
        let mut state = listed(vec![]);
        let _ = state.update(DiffMsg::Step(1));
        assert_eq!(state.selected, None);
    }
}

mod reading_a_file {
    use super::*;

    const TEXT: &str =
        "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,2 +1,2 @@\n keep\n-old\n+new\n";

    #[test]
    fn a_diff_that_arrives_is_shown_when_it_is_the_selected_file_s() {
        let mut state = listed(vec![file("a.rs", "M")]);
        let key = state.wanted_key().expect("a file is selected");
        let seq = state.load_seq;
        assert!(state.shown.is_none());
        let _ = state.update(DiffMsg::FileRead(1, seq, key.clone(), Ok(read(TEXT))));
        let (shown_key, shown) = state.shown.as_ref().expect("shown");
        assert_eq!(shown_key, &key);
        // The file's header is not a row of the table.
        assert_eq!(shown.rows.len(), 4);
        assert!(shown.has_lines());
    }

    #[test]
    fn a_diff_for_a_file_no_longer_selected_is_cached_but_not_shown() {
        let mut state = listed(vec![file("a.rs", "M"), file("b.rs", "M")]);
        let first = state.wanted_key().unwrap();
        let _ = state.update(DiffMsg::Select("\u{0}b.rs".into()));
        let seq = state.load_seq;
        let _ = state.update(DiffMsg::FileRead(1, seq, first.clone(), Ok(read(TEXT))));
        assert!(state.shown.is_none());
        assert!(state.cache.contains_key(&first));
        // Coming back to it is free.
        let _ = state.update(DiffMsg::Select("\u{0}a.rs".into()));
        assert_eq!(state.shown.as_ref().map(|(key, _)| key), Some(&first));
    }

    #[test]
    fn an_answer_for_an_earlier_listing_is_dropped() {
        let mut state = listed(vec![file("a.rs", "M")]);
        let key = state.wanted_key().unwrap();
        let stale = state.load_seq;
        let _ = state.update(DiffMsg::Reload);
        let _ = state.update(DiffMsg::FileRead(1, stale, key.clone(), Ok(read(TEXT))));
        assert!(!state.cache.contains_key(&key));
        assert!(state.shown.is_none());
    }

    #[test]
    fn a_failure_for_the_selected_file_is_its_own_message() {
        let mut state = listed(vec![file("a.rs", "M")]);
        let key = state.wanted_key().unwrap();
        let seq = state.load_seq;
        let _ = state.update(DiffMsg::FileRead(1, seq, key, Err("could not read".into())));
        assert_eq!(
            state.diff_error.as_ref().map(|(_, error)| error.as_str()),
            Some("could not read")
        );
        assert!(state.shown.is_none());
    }

    #[test]
    fn a_reload_empties_the_cache() {
        let mut state = listed(vec![file("a.rs", "M")]);
        let key = state.wanted_key().unwrap();
        let seq = state.load_seq;
        let _ = state.update(DiffMsg::FileRead(1, seq, key, Ok(read(TEXT))));
        assert_eq!(state.cache.len(), 1);
        let _ = state.update(DiffMsg::Reload);
        assert!(state.cache.is_empty());
    }

    #[test]
    fn the_base_is_part_of_what_a_diff_is_cached_under() {
        let mut state = listed(vec![file("a.rs", "M")]);
        let uncommitted = state.wanted_key().unwrap();
        state.base = Some("main".into());
        assert_ne!(state.wanted_key().unwrap(), uncommitted);
    }

    #[test]
    fn a_renamed_file_is_told_from_the_one_it_replaced() {
        let renamed = ChangedFile {
            old_path: Some("old.rs".into()),
            ..file("new.rs", "R")
        };
        assert_ne!(file_key(&renamed), file_key(&file("new.rs", "R")));
    }
}

mod the_diff_shown {
    use super::*;

    #[test]
    fn rows_exclude_the_header_and_expand_tabs() {
        let text = "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-\told\n+\tnew\tmore\n";
        let shown = Shown::of(&FileDiff {
            text: text.into(),
            ..FileDiff::default()
        });
        let texts: Vec<_> = shown.rows.iter().map(|row| row.text.as_str()).collect();
        assert_eq!(texts, ["@@ -1 +1 @@", "    old", "    new    more"]);
        assert_eq!(shown.widest, "    new    more".chars().count());
        assert_eq!(shown.last_line, 1);
    }

    #[test]
    fn a_diff_with_only_a_header_has_no_lines() {
        let shown = Shown::of(&FileDiff {
            text: "diff --git a/a b/a\nold mode 100644\nnew mode 100755\n".into(),
            ..FileDiff::default()
        });
        assert!(!shown.has_lines());
    }

    #[test]
    fn the_widest_line_and_the_last_number_size_the_table() {
        let text = "@@ -98,2 +98,3 @@\n a\n+bbbbbb\n c\n";
        let shown = Shown::of(&FileDiff {
            text: text.into(),
            ..FileDiff::default()
        });
        assert_eq!(shown.widest, "@@ -98,2 +98,3 @@".chars().count());
        assert_eq!(shown.last_line, 100);
    }
}

mod rows_in_view {
    use super::*;

    #[test]
    fn at_the_top_the_first_screen_and_a_margin_are_built() {
        let (first, last) = visible_range(0.0, 400.0, 10_000);
        assert_eq!(first, 0);
        assert_eq!(last, (400.0 / ROW).ceil() as usize + OVERSCAN);
    }

    #[test]
    fn scrolled_down_only_what_is_near_the_viewport_is_built() {
        let (first, last) = visible_range(ROW * 1000.0, 400.0, 10_000);
        assert_eq!(first, 1000 - OVERSCAN);
        assert!(last > 1000 && last < 1100);
        assert!(last - first < 80, "{} rows built", last - first);
    }

    #[test]
    fn the_end_of_the_diff_is_never_overrun() {
        let (first, last) = visible_range(ROW * 9_990.0, 400.0, 10_000);
        assert_eq!(last, 10_000);
        assert!(first < last);
        // A scroll past the end (a resize) still gives a valid range.
        let (first, last) = visible_range(ROW * 50_000.0, 400.0, 10_000);
        assert_eq!((first, last), (10_000, 10_000));
    }

    #[test]
    fn before_the_first_scroll_a_tall_viewport_is_assumed() {
        let (_, last) = visible_range(0.0, 0.0, 10_000);
        assert!(last as f32 * ROW >= FIRST_VIEWPORT);
    }

    #[test]
    fn a_short_diff_is_built_whole() {
        assert_eq!(visible_range(0.0, 400.0, 5), (0, 5));
        assert_eq!(visible_range(0.0, 400.0, 0), (0, 0));
    }
}

mod letters {
    use super::*;

    #[test]
    fn untracked_reads_u_and_a_conflict_reads_a_bang() {
        assert_eq!(status_mark("?"), "U");
        assert_eq!(status_mark("U"), "!");
        assert_eq!(status_mark("M"), "M");
        assert_eq!(status_mark("R"), "R");
    }
}
