//! The worktree panel's state machine: what each answer does to the panel,
//! which are dropped, what is armed, kept, disabled.
//!
//! git itself is not run: every task a message returns is dropped, and the
//! answers are sent in by hand, which is also how the runtime delivers them.

use hangar_core::git::MergedWorktree;

use super::*;
use crate::state::types::{AppState, Pane, Workspace};

fn repo(main_root: &str) -> RepoInfo {
    RepoInfo {
        installed: true,
        is_repo: true,
        root: Some(main_root.into()),
        main_root: Some(main_root.into()),
        is_linked_worktree: false,
        branch: Some("main".into()),
        head: Some("abc1234".into()),
    }
}

fn worktree(path: &str, branch: &str) -> Worktree {
    Worktree {
        path: path.into(),
        branch: Some(branch.into()),
        head: Some("abc1234".into()),
        ..Worktree::default()
    }
}

fn branch(kind: &str, name: &str, held: Option<&str>) -> Branch {
    Branch {
        name: name.into(),
        kind: kind.into(),
        is_current: false,
        worktree_path: held.map(str::to_string),
        head: Some("abc1234".into()),
        committer_date: Some(1_700_000_000),
        author_name: Some("Ada".into()),
        subject: Some("A commit".into()),
        upstream: None,
        track: None,
    }
}

fn model() -> Model {
    let mut m = Model::default();
    m.hydrate(AppState {
        workspaces: vec![Workspace {
            id: "w".into(),
            name: "app".into(),
            cwd: "/work/app".into(),
            panes: vec![Pane {
                id: "p".into(),
                name: "Ava".into(),
                ..Pane::default()
            }],
            ..Workspace::default()
        }],
        active_workspace_id: Some("w".into()),
        ..AppState::default()
    });
    m
}

/// Sends a message and drops what it asks for.
fn send(panel: &mut Panel, m: &mut Model, msg: WtMsg) {
    let _task = update(panel, m, msg);
}

/// A panel that has read a repository with `main` and a worktree for `feat/x`.
fn panel() -> (Panel, Model) {
    let mut m = model();
    let (mut panel, _first) = Panel::open("w".into(), "/work/app".into(), 1);
    let loaded = Loaded::Ready {
        info: repo("/work/app"),
        list: Ok(vec![
            Worktree {
                is_main: true,
                is_current: true,
                ..worktree("/work/app", "main")
            },
            worktree("/work/app-worktrees/feat-x", "feat/x"),
        ]),
        refs: Ok(vec![
            branch("local", "main", Some("/work/app")),
            branch("local", "feat/x", Some("/work/app-worktrees/feat-x")),
            branch("local", "dev", None),
            branch("remote", "origin/main", None),
        ]),
    };
    send(
        &mut panel,
        &mut m,
        WtMsg::Loaded(1, Box::new(loaded), After::Nothing),
    );
    (panel, m)
}

mod reading {
    use super::*;

    #[test]
    fn what_git_listed_is_kept_and_the_panel_stops_loading() {
        let (panel, _) = panel();
        assert!(!panel.loading);
        assert_eq!(panel.worktrees.len(), 2);
        assert_eq!(panel.branches.len(), 4);
        assert_eq!(panel.error, None);
    }

    #[test]
    fn a_new_panel_is_loading_until_git_answers() {
        let (panel, _) = Panel::open("w".into(), "/work/app".into(), 1);
        assert!(panel.loading);
        assert!(panel.worktrees.is_empty());
    }

    #[test]
    fn a_folder_that_is_not_a_repository_is_a_state_not_a_failure() {
        let (mut panel, mut m) = panel();
        let plain = RepoInfo {
            installed: true,
            is_repo: false,
            ..RepoInfo::default()
        };
        send(
            &mut panel,
            &mut m,
            WtMsg::Loaded(1, Box::new(Loaded::Unavailable(plain)), After::Nothing),
        );
        assert!(panel.worktrees.is_empty() && panel.branches.is_empty());
        assert_eq!(panel.error, None);
        assert!(!panel.info.as_ref().unwrap().is_repo);
    }

    #[test]
    fn a_failed_branch_listing_degrades_the_panel_and_says_why() {
        let (mut panel, mut m) = panel();
        let loaded = Loaded::Ready {
            info: repo("/work/app"),
            list: Ok(vec![worktree("/work/app", "main")]),
            refs: Err("git is too old".into()),
        };
        send(
            &mut panel,
            &mut m,
            WtMsg::Loaded(1, Box::new(loaded), After::Nothing),
        );
        assert_eq!(panel.worktrees.len(), 1);
        assert!(panel.branches.is_empty());
        assert_eq!(panel.error.as_deref(), Some("git is too old"));
    }

    #[test]
    fn a_failed_worktree_listing_keeps_what_was_there_and_says_why() {
        let (mut panel, mut m) = panel();
        let loaded = Loaded::Ready {
            info: repo("/work/app"),
            list: Err("fatal".into()),
            refs: Ok(vec![]),
        };
        send(
            &mut panel,
            &mut m,
            WtMsg::Loaded(1, Box::new(loaded), After::Nothing),
        );
        assert_eq!(panel.worktrees.len(), 2);
        assert_eq!(panel.error.as_deref(), Some("fatal"));
    }

    #[test]
    fn an_answer_for_an_earlier_opening_is_dropped() {
        let (mut panel, mut m) = panel();
        let other = Loaded::Ready {
            info: repo("/elsewhere"),
            list: Ok(vec![]),
            refs: Ok(vec![]),
        };
        send(
            &mut panel,
            &mut m,
            WtMsg::Loaded(7, Box::new(other), After::Nothing),
        );
        assert_eq!(panel.worktrees.len(), 2);
    }
}

mod the_form {
    use super::*;

    #[test]
    fn the_folder_follows_the_branch_name_until_it_is_edited() {
        let (mut panel, mut m) = panel();
        assert_eq!(panel.folder_text(), "");
        send(&mut panel, &mut m, WtMsg::NewBranch("feat/search".into()));
        assert_eq!(panel.folder_text(), "/work/app-worktrees/feat-search");
        send(&mut panel, &mut m, WtMsg::NewBranch("feat/other".into()));
        assert_eq!(panel.folder_text(), "/work/app-worktrees/feat-other");

        send(&mut panel, &mut m, WtMsg::Folder("/mine".into()));
        send(&mut panel, &mut m, WtMsg::NewBranch("feat/third".into()));
        assert_eq!(panel.folder_text(), "/mine");
    }

    #[test]
    fn the_name_is_cleaned_before_it_is_used() {
        let (mut panel, mut m) = panel();
        send(&mut panel, &mut m, WtMsg::NewBranch("my feature..x".into()));
        assert_eq!(panel.resolved().plan.unwrap().branch, "my-feature-x");
    }

    #[test]
    fn create_needs_a_branch_a_folder_and_nothing_in_flight() {
        let (mut panel, mut m) = panel();
        assert!(!panel.can_create());
        send(&mut panel, &mut m, WtMsg::NewBranch("feat/search".into()));
        assert!(panel.can_create());
        send(&mut panel, &mut m, WtMsg::Folder("   ".into()));
        assert!(!panel.can_create(), "a blank folder is no folder");
        send(&mut panel, &mut m, WtMsg::Folder("/wt/x".into()));
        assert!(panel.can_create());
        panel.busy = Some(Busy::Fetch);
        assert!(!panel.can_create());
    }

    #[test]
    fn a_name_that_exists_is_refused_with_its_reason() {
        let (mut panel, mut m) = panel();
        send(&mut panel, &mut m, WtMsg::NewBranch("dev".into()));
        let resolved = panel.resolved();
        assert_eq!(resolved.plan, None);
        assert_eq!(
            resolved.error,
            Some(create::PlanError::NameTaken("dev".into()))
        );
        assert!(!panel.can_create());
    }

    #[test]
    fn an_existing_branch_is_picked_by_its_key_and_the_menu_closes() {
        let (mut panel, mut m) = panel();
        send(&mut panel, &mut m, WtMsg::SetMode(Mode::Existing));
        send(&mut panel, &mut m, WtMsg::TogglePicker(Picker::Existing));
        assert_eq!(panel.picker, Some(Picker::Existing));
        send(&mut panel, &mut m, WtMsg::PickExisting("local:dev".into()));
        assert_eq!(panel.picker, None);
        assert_eq!(panel.resolved().plan.unwrap().branch, "dev");
        assert_eq!(panel.suggested, "/work/app-worktrees/dev");
    }

    #[test]
    fn pressing_a_picker_twice_closes_it() {
        let (mut panel, mut m) = panel();
        send(&mut panel, &mut m, WtMsg::TogglePicker(Picker::Base));
        send(&mut panel, &mut m, WtMsg::TogglePicker(Picker::Base));
        assert_eq!(panel.picker, None);
        send(&mut panel, &mut m, WtMsg::TogglePicker(Picker::Base));
        send(&mut panel, &mut m, WtMsg::ClosePicker);
        assert_eq!(panel.picker, None);
    }

    fn made(notes: &[&str], hook_failure: Option<&str>) -> Created {
        Created {
            target: "/work/app-worktrees/feat-x".into(),
            branch: "feat/x".into(),
            notes: notes.iter().map(|note| note.to_string()).collect(),
            warn: false,
            hook_failure: hook_failure.map(str::to_string),
        }
    }

    #[test]
    fn a_created_worktree_resets_the_form_but_keeps_the_choices_about_what_comes_with_it() {
        let (mut panel, mut m) = panel();
        send(&mut panel, &mut m, WtMsg::NewBranch("feat/x".into()));
        send(&mut panel, &mut m, WtMsg::Folder("/wt".into()));
        send(&mut panel, &mut m, WtMsg::CopyUntracked(false));
        send(&mut panel, &mut m, WtMsg::OpenAfter(false));
        panel.busy = Some(Busy::Create);
        send(
            &mut panel,
            &mut m,
            WtMsg::Created(1, Box::new(Ok(made(&[], None)))),
        );
        assert_eq!(panel.busy, None);
        assert_eq!(panel.form.new_branch, "");
        assert!(!panel.form.folder_touched);
        assert!(!panel.form.copy_untracked);
        assert!(!panel.form.open_after);
    }

    #[test]
    fn what_came_with_the_worktree_is_said_in_one_line_that_clears_itself() {
        let (mut panel, mut m) = panel();
        send(
            &mut panel,
            &mut m,
            WtMsg::Created(
                1,
                Box::new(Ok(made(
                    &["2 files copied.", "The postCreate command ran."],
                    None,
                ))),
            ),
        );
        let said = panel.notice.clone().expect("a notice");
        assert_eq!(said.text, "2 files copied. The postCreate command ran.");
        assert!(!said.warn);
        // The timer of an older notice does not clear a newer one.
        send(&mut panel, &mut m, WtMsg::NoticeExpired(said.seq + 10));
        assert!(panel.notice.is_some());
        send(&mut panel, &mut m, WtMsg::NoticeExpired(said.seq));
        assert!(panel.notice.is_none());
    }

    #[test]
    fn a_hook_failure_stays_on_screen_until_the_next_action() {
        let (mut panel, mut m) = panel();
        send(
            &mut panel,
            &mut m,
            WtMsg::Created(
                1,
                Box::new(Ok(made(
                    &[],
                    Some("The postCreate command failed (exit 1). The worktree was kept."),
                ))),
            ),
        );
        assert!(panel.error.as_deref().unwrap().contains("failed (exit 1)"));
        send(&mut panel, &mut m, WtMsg::Refresh);
        assert_eq!(panel.error, None);
    }

    #[test]
    fn a_failure_to_create_says_why_and_makes_nothing() {
        let (mut panel, mut m) = panel();
        panel.busy = Some(Busy::Create);
        send(
            &mut panel,
            &mut m,
            WtMsg::Created(1, Box::new(Err("fatal: already exists".into()))),
        );
        assert_eq!(panel.busy, None);
        assert_eq!(panel.error.as_deref(), Some("fatal: already exists"));
    }
}

mod removing {
    use super::*;

    const PATH: &str = "/work/app-worktrees/feat-x";

    #[test]
    fn the_first_press_arms_the_button_and_the_second_removes() {
        let (mut panel, mut m) = panel();
        send(&mut panel, &mut m, WtMsg::RemovePressed(PATH.into()));
        assert_eq!(
            panel.armed,
            Some(Armed {
                path: PATH.into(),
                branch: Some("feat/x".into())
            })
        );
        assert_eq!(panel.busy, None);
        send(&mut panel, &mut m, WtMsg::RemovePressed(PATH.into()));
        assert_eq!(panel.busy, Some(Busy::Remove(PATH.into())));
    }

    #[test]
    fn arming_another_row_moves_the_arm() {
        let (mut panel, mut m) = panel();
        send(&mut panel, &mut m, WtMsg::RemovePressed(PATH.into()));
        send(
            &mut panel,
            &mut m,
            WtMsg::RemovePressed("/work/app-worktrees/other".into()),
        );
        assert_eq!(
            panel.armed.as_ref().unwrap().path,
            "/work/app-worktrees/other"
        );
        assert_eq!(panel.busy, None);
    }

    fn answer(
        removed: bool,
        needs_force: bool,
        hook_blocked: bool,
        message: Option<&str>,
    ) -> RemoveResult {
        RemoveResult {
            removed,
            needs_force,
            message: message.map(str::to_string),
            hook_blocked,
            hook: None,
        }
    }

    fn removed(panel: &mut Panel, m: &mut Model, result: Result<RemoveResult, String>) {
        panel.busy = Some(Busy::Remove(PATH.into()));
        panel.armed = Some(Armed {
            path: PATH.into(),
            branch: Some("feat/x".into()),
        });
        send(
            panel,
            m,
            WtMsg::Removed(1, PATH.into(), Some("feat/x".into()), Box::new(result)),
        );
    }

    #[test]
    fn a_removal_that_went_through_disarms_clears_the_block_and_reads_again() {
        let (mut panel, mut m) = panel();
        panel.blocked = Some(Blocked {
            path: PATH.into(),
            message: None,
            hook: false,
        });
        removed(&mut panel, &mut m, Ok(answer(true, false, false, None)));
        assert_eq!(panel.armed, None);
        assert_eq!(panel.blocked, None);
        assert_eq!(panel.busy, None);
        assert!(panel.loading, "the lists are read again");
    }

    #[test]
    fn git_refusing_a_dirty_worktree_offers_the_forced_run_on_its_row() {
        let (mut panel, mut m) = panel();
        removed(
            &mut panel,
            &mut m,
            Ok(answer(false, true, false, Some("contains modified files"))),
        );
        assert_eq!(
            panel.blocked,
            Some(Blocked {
                path: PATH.into(),
                message: Some("contains modified files".into()),
                hook: false
            })
        );
        assert_eq!(panel.armed, None);
        assert_eq!(panel.busy, None);
        assert_eq!(panel.error, None);
    }

    #[test]
    fn a_hook_that_stopped_the_removal_offers_to_remove_anyway() {
        let (mut panel, mut m) = panel();
        removed(
            &mut panel,
            &mut m,
            Ok(answer(false, false, true, Some("preRemove failed"))),
        );
        assert!(panel.blocked.as_ref().unwrap().hook);
    }

    #[test]
    fn a_refusal_that_is_not_a_question_is_an_error() {
        let (mut panel, mut m) = panel();
        removed(
            &mut panel,
            &mut m,
            Ok(answer(false, false, false, Some("fatal: not a worktree"))),
        );
        assert_eq!(panel.error.as_deref(), Some("fatal: not a worktree"));
        assert_eq!(panel.blocked, None);
        let (mut again, mut m) = super::panel();
        removed(&mut again, &mut m, Err("could not run git".into()));
        assert_eq!(again.error.as_deref(), Some("could not run git"));
    }

    #[test]
    fn once_nothing_holds_the_branch_the_panel_offers_to_delete_it() {
        let (mut panel, _m) = panel();
        let fresh = Loaded::Ready {
            info: repo("/work/app"),
            list: Ok(vec![worktree("/work/app", "main")]),
            // The worktree is gone; "feat/x" is held by nothing.
            refs: Ok(vec![branch("local", "feat/x", None)]),
        };
        let _ = loaded_in(&mut panel, fresh, After::Orphan("feat/x".into()));
        assert_eq!(
            panel.orphan,
            Some(Orphan {
                name: "feat/x".into(),
                needs_force: false,
                message: None
            })
        );
    }

    #[test]
    fn no_offer_while_a_worktree_still_holds_the_branch() {
        let (mut panel, _m) = panel();
        let fresh = Loaded::Ready {
            info: repo("/work/app"),
            list: Ok(vec![]),
            refs: Ok(vec![branch("local", "feat/x", Some("/elsewhere"))]),
        };
        let _ = loaded_in(&mut panel, fresh, After::Orphan("feat/x".into()));
        assert_eq!(panel.orphan, None);
    }

    #[test]
    fn deleting_the_orphan_unforced_may_turn_into_a_question() {
        let (mut panel, mut m) = panel();
        panel.orphan = Some(Orphan {
            name: "feat/x".into(),
            needs_force: false,
            message: None,
        });
        let refused = DeleteBranchResult {
            deleted: false,
            needs_force: true,
            message: Some("error: the branch 'feat/x' is not fully merged".into()),
        };
        send(
            &mut panel,
            &mut m,
            WtMsg::OrphanDeleted(1, Box::new(Ok(refused))),
        );
        let orphan = panel.orphan.clone().expect("still offered");
        assert!(orphan.needs_force);
        assert!(orphan.message.unwrap().contains("not fully merged"));

        let deleted = DeleteBranchResult {
            deleted: true,
            needs_force: false,
            message: None,
        };
        send(
            &mut panel,
            &mut m,
            WtMsg::OrphanDeleted(1, Box::new(Ok(deleted))),
        );
        assert_eq!(panel.orphan, None);
        assert!(panel.loading);
    }

    #[test]
    fn keeping_the_branch_withdraws_the_offer() {
        let (mut panel, mut m) = panel();
        panel.orphan = Some(Orphan {
            name: "feat/x".into(),
            needs_force: false,
            message: None,
        });
        send(&mut panel, &mut m, WtMsg::OrphanKeep);
        assert_eq!(panel.orphan, None);
    }
}

mod migrating {
    use super::*;

    const PATH: &str = "/work/app-worktrees/feat-x";

    fn result(outcome: &'static str, stash: Option<&str>) -> MigrateResult {
        MigrateResult {
            outcome,
            files: Vec::new(),
            more: 0,
            stash: stash.map(str::to_string),
            message: None,
        }
    }

    #[test]
    fn the_button_asks_first_and_asking_again_withdraws_the_question() {
        let (mut panel, mut m) = panel();
        send(&mut panel, &mut m, WtMsg::MigrateAsk(PATH.into()));
        assert_eq!(panel.migrating.as_deref(), Some(PATH));
        send(&mut panel, &mut m, WtMsg::MigrateAsk(PATH.into()));
        assert_eq!(panel.migrating, None);
        send(&mut panel, &mut m, WtMsg::MigrateAsk(PATH.into()));
        send(&mut panel, &mut m, WtMsg::MigrateCancel);
        assert_eq!(panel.migrating, None);
    }

    #[test]
    fn confirming_runs_it_and_clears_the_question_and_the_old_report() {
        let (mut panel, mut m) = panel();
        panel.migrated = Some((PATH.into(), result("nothing", None)));
        panel.migrating = Some(PATH.into());
        send(&mut panel, &mut m, WtMsg::MigrateRun(PATH.into()));
        assert_eq!(panel.migrating, None);
        assert!(panel.migrated.is_none());
        assert_eq!(panel.busy, Some(Busy::Migrate(PATH.into())));
    }

    #[test]
    fn a_clean_move_is_a_line_that_clears_itself() {
        let (mut panel, mut m) = panel();
        panel.busy = Some(Busy::Migrate(PATH.into()));
        send(
            &mut panel,
            &mut m,
            WtMsg::Migrated(1, PATH.into(), Box::new(Ok(result("moved", None)))),
        );
        assert_eq!(panel.busy, None);
        assert!(panel.notice.is_some());
        assert!(panel.migrated.is_none());
    }

    #[test]
    fn anything_else_stays_on_the_row_until_dismissed() {
        for outcome in ["nothing", "conflict", "applyFailed"] {
            let (mut panel, mut m) = panel();
            send(
                &mut panel,
                &mut m,
                WtMsg::Migrated(1, PATH.into(), Box::new(Ok(result(outcome, Some("abc"))))),
            );
            assert_eq!(
                panel.migrated.as_ref().map(|(path, _)| path.as_str()),
                Some(PATH),
                "{outcome}"
            );
            send(&mut panel, &mut m, WtMsg::DismissMigrated);
            assert!(panel.migrated.is_none());
        }
        // A move whose stash could not be dropped is worth reading too.
        let (mut panel, mut m) = panel();
        send(
            &mut panel,
            &mut m,
            WtMsg::Migrated(1, PATH.into(), Box::new(Ok(result("moved", Some("abc"))))),
        );
        assert!(panel.migrated.is_some());
    }

    #[test]
    fn a_failure_is_an_error_not_a_report() {
        let (mut panel, mut m) = panel();
        send(
            &mut panel,
            &mut m,
            WtMsg::Migrated(1, PATH.into(), Box::new(Err("boom".into()))),
        );
        assert_eq!(panel.error.as_deref(), Some("boom"));
        assert!(panel.migrated.is_none());
    }
}

mod cleaning_up {
    use super::*;

    fn merged(path: &str, branch: &str) -> MergedWorktree {
        MergedWorktree {
            path: path.into(),
            branch: branch.into(),
            locked: None,
            dirty: Some(0),
            no_own_commit: false,
            held: false,
            preselected: true,
        }
    }

    fn open(panel: &mut Panel, m: &mut Model) {
        send(panel, m, WtMsg::OpenCleanup);
        let items = vec![merged("/wt/a", "a"), merged("/wt/b", "b")];
        send(panel, m, WtMsg::CleanupListed(1, Ok(items)));
    }

    #[test]
    fn opening_it_waits_for_git_and_then_ticks_the_likely_ones() {
        let (mut panel, mut m) = panel();
        send(&mut panel, &mut m, WtMsg::OpenCleanup);
        assert!(panel.cleanup.as_ref().unwrap().items.is_none());
        send(
            &mut panel,
            &mut m,
            WtMsg::CleanupListed(1, Ok(vec![merged("/wt/a", "a")])),
        );
        let cleanup = panel.cleanup.as_ref().unwrap();
        assert_eq!(cleanup.items.as_ref().unwrap().len(), 1);
        assert_eq!(cleanup.picked().len(), 1);
    }

    #[test]
    fn a_failure_to_list_closes_it_and_says_why() {
        let (mut panel, mut m) = panel();
        send(&mut panel, &mut m, WtMsg::OpenCleanup);
        send(
            &mut panel,
            &mut m,
            WtMsg::CleanupListed(1, Err("fatal".into())),
        );
        assert!(panel.cleanup.is_none());
        assert_eq!(panel.error.as_deref(), Some("fatal"));
    }

    #[test]
    fn the_first_press_arms_removal_and_the_second_runs_it() {
        let (mut panel, mut m) = panel();
        open(&mut panel, &mut m);
        send(&mut panel, &mut m, WtMsg::CleanupRemovePressed);
        assert!(panel.cleanup.as_ref().unwrap().confirming);
        assert_eq!(panel.busy, None);
        send(&mut panel, &mut m, WtMsg::CleanupRemovePressed);
        assert_eq!(panel.busy, Some(Busy::Cleanup));
    }

    #[test]
    fn with_nothing_ticked_there_is_nothing_to_run() {
        let (mut panel, mut m) = panel();
        open(&mut panel, &mut m);
        send(&mut panel, &mut m, WtMsg::CleanupToggle("/wt/a".into()));
        send(&mut panel, &mut m, WtMsg::CleanupToggle("/wt/b".into()));
        send(&mut panel, &mut m, WtMsg::CleanupRemovePressed);
        send(&mut panel, &mut m, WtMsg::CleanupRemovePressed);
        assert_eq!(panel.busy, None);
    }

    #[test]
    fn when_the_removals_finish_the_lists_are_read_again_before_the_branches_are_offered() {
        let (mut panel, mut m) = panel();
        open(&mut panel, &mut m);
        panel.busy = Some(Busy::Cleanup);
        let run = Run {
            gone: ["/wt/a".to_string()].into_iter().collect(),
            branches: vec!["a".into()],
            ..Run::default()
        };
        send(
            &mut panel,
            &mut m,
            WtMsg::CleanupRan(1, Box::new(run.clone())),
        );
        assert_eq!(panel.busy, None);
        assert!(panel.loading);
        // The reading comes in: "a" is held by nothing now.
        let fresh = Loaded::Ready {
            info: repo("/work/app"),
            list: Ok(vec![]),
            refs: Ok(vec![branch("local", "a", None)]),
        };
        let _ = loaded_in(&mut panel, fresh, After::CleanupRan(run));
        let cleanup = panel.cleanup.as_ref().unwrap();
        assert_eq!(cleanup.removed, 1);
        assert_eq!(cleanup.branches, ["a"]);
    }

    #[test]
    fn going_back_closes_it() {
        let (mut panel, mut m) = panel();
        open(&mut panel, &mut m);
        send(&mut panel, &mut m, WtMsg::CleanupBack);
        assert!(panel.cleanup.is_none());
    }
}

mod around_it {
    use super::*;

    #[test]
    fn a_busy_panel_is_told_which_write_is_running() {
        let (mut panel, mut m) = panel();
        send(&mut panel, &mut m, WtMsg::Fetch);
        assert_eq!(panel.busy, Some(Busy::Fetch));
        send(&mut panel, &mut m, WtMsg::Done(1, Ok(())));
        assert_eq!(panel.busy, None);
        assert!(panel.loading, "the lists are read again after a fetch");
        send(&mut panel, &mut m, WtMsg::Prune);
        assert_eq!(panel.busy, Some(Busy::Prune));
    }

    #[test]
    fn a_failed_fetch_says_so_and_still_reads_again() {
        let (mut panel, mut m) = panel();
        send(&mut panel, &mut m, WtMsg::Fetch);
        send(
            &mut panel,
            &mut m,
            WtMsg::Done(1, Err("could not resolve host".into())),
        );
        // The reading that follows must not swallow it.
        assert_eq!(panel.busy, None);
        assert!(panel.loading);
        assert_eq!(panel.error.as_deref(), Some("could not resolve host"));
        // It stays until the next action.
        send(&mut panel, &mut m, WtMsg::Refresh);
        assert_eq!(panel.error, None);
    }

    #[test]
    fn locking_asks_git_for_the_opposite_of_what_the_row_shows() {
        let (mut panel, mut m) = panel();
        panel.worktrees[1].locked = Some(String::new());
        send(
            &mut panel,
            &mut m,
            WtMsg::ToggleLock("/work/app-worktrees/feat-x".into()),
        );
        assert_eq!(
            panel.busy,
            Some(Busy::Lock("/work/app-worktrees/feat-x".into()))
        );
    }

    #[test]
    fn closing_the_panel_takes_its_overlay_and_no_other() {
        let (mut panel, mut m) = panel();
        m.ui.overlays = vec![Overlay::Worktrees("w".into())];
        send(&mut panel, &mut m, WtMsg::Close);
        assert!(m.ui.overlays.is_empty());
        // Something else in front stays: the panel is not the top.
        m.ui.overlays = vec![Overlay::Worktrees("w".into()), Overlay::Palette];
        send(&mut panel, &mut m, WtMsg::Close);
        assert_eq!(m.ui.overlays.len(), 2);
    }

    #[test]
    fn no_pane_could_be_added_so_the_workspace_is_said_to_be_full() {
        // The store's `AddPane` answers no id until P0b lands, as it does at
        // sixteen panes: either way the panel says there is no room.
        let (mut panel, mut m) = panel();
        send(
            &mut panel,
            &mut m,
            WtMsg::OpenPane("/work/app-worktrees/feat-x".into()),
        );
        assert!(panel.error.is_some());
    }

    #[test]
    fn the_hooks_are_the_persons_own_and_blank_ones_run_nothing() {
        let mut m = model();
        let mut state = m.state().clone();
        state.workspaces[0].worktree_hooks = Some(crate::state::types::WorktreeHooks {
            post_create: Some("  pnpm install  ".into()),
            pre_remove: Some("   ".into()),
        });
        m.hydrate(state);
        assert_eq!(hooks(&m, "w"), (Some("pnpm install".to_string()), None));
        assert_eq!(hooks(&m, "nobody"), (None, None));
    }

    #[test]
    fn every_folder_a_workspace_or_a_pane_is_open_in_counts_as_held() {
        let mut m = model();
        let mut state = m.state().clone();
        state.workspaces[0].panes[0].cwd = Some("/work/app-worktrees/feat-x".into());
        m.hydrate(state);
        assert_eq!(
            held_folders(&m),
            ["/work/app", "/work/app-worktrees/feat-x"]
        );
    }
}
