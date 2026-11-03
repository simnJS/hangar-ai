//! The end-of-cycle cleanup (`Cleanup` and its functions in
//! `WorktreePanel.tsx`): worktrees whose branch is already merged, listed
//! with the reasons to leave some of them alone, removed one after the other,
//! with what git held back offered for a forced second try, and the branches
//! they leave orphaned offered for deletion.
//!
//! The state and every step of it that is not a call to git are here and
//! tested; the calls are three async functions the panel runs as tasks.

use std::collections::HashSet;

use hangar_core::git::{self, Branch, MergedWorktree};

/// What the cleanup shows: what is on offer, what is ticked, how it went.
#[derive(Debug, Clone, Default)]
pub struct Cleanup {
    /// `None` while git is still being asked.
    pub items: Option<Vec<MergedWorktree>>,
    pub selected: HashSet<String>,
    /// The inline « are you sure », armed by the first click.
    pub confirming: bool,
    /// Refused without force (dirty): git's reason, one confirmation per row.
    pub forced: Vec<(MergedWorktree, Option<String>)>,
    pub failed: Vec<(MergedWorktree, String)>,
    pub removed: usize,
    /// Branches the removals left with no worktree, offered for deletion.
    pub branches: Vec<String>,
    /// The branches git said are not fully merged: a second, explicit question.
    pub branches_force: bool,
    pub branches_deleted: usize,
}

/// What removing the ticked worktrees one after the other came to.
#[derive(Debug, Clone, Default)]
pub struct Run {
    /// The paths that went.
    pub gone: HashSet<String>,
    /// Their branches, in order.
    pub branches: Vec<String>,
    pub forced: Vec<(MergedWorktree, Option<String>)>,
    pub failed: Vec<(MergedWorktree, String)>,
}

/// What deleting the offered branches came to.
#[derive(Debug, Clone, Default)]
pub struct BranchesRun {
    /// The ones git called not fully merged: asked about again.
    pub left: Vec<String>,
    pub deleted: usize,
    /// The last failure that was not a question.
    pub error: Option<String>,
}

/// The branches of `names` that no worktree holds any more, by what git says
/// now. Only those can be offered for deletion.
fn orphaned(names: &[String], fresh: Option<&[Branch]>) -> Vec<String> {
    let Some(fresh) = fresh else {
        return Vec::new();
    };
    names
        .iter()
        .filter(|name| {
            fresh.iter().any(|branch| {
                branch.kind == "local" && &branch.name == *name && branch.worktree_path.is_none()
            })
        })
        .cloned()
        .collect()
}

impl Cleanup {
    /// Open, and waiting for git.
    pub fn loading() -> Cleanup {
        Cleanup::default()
    }

    /// git answered: what is on offer, the likely ones ticked.
    pub fn listed(&mut self, items: Vec<MergedWorktree>) {
        self.selected = items
            .iter()
            .filter(|item| item.preselected)
            .map(|item| item.path.clone())
            .collect();
        self.items = Some(items);
    }

    /// Ticks or unticks a worktree. Changing what is ticked disarms the
    /// confirmation.
    pub fn toggle(&mut self, path: &str) {
        if !self.selected.remove(path) {
            self.selected.insert(path.to_string());
        }
        self.confirming = false;
    }

    /// The worktrees that are ticked.
    pub fn picked(&self) -> Vec<&MergedWorktree> {
        self.items
            .iter()
            .flatten()
            .filter(|item| self.selected.contains(&item.path))
            .collect()
    }

    /// How many of the ticked ones hold uncommitted work, or may: a state git
    /// could not read counts, since it is never taken for clean.
    pub fn picked_dirty(&self) -> usize {
        self.picked()
            .iter()
            .filter(|item| item.dirty != Some(0))
            .count()
    }

    /// The worktrees a removal is run on: ticked and not locked.
    pub fn todo(&self) -> Vec<MergedWorktree> {
        self.picked()
            .into_iter()
            .filter(|item| item.locked.is_none())
            .cloned()
            .collect()
    }

    /// The removals finished and the lists were read again.
    pub fn after_run(&mut self, run: Run, fresh: Option<&[Branch]>) {
        if let Some(items) = &mut self.items {
            items.retain(|item| !run.gone.contains(&item.path));
        }
        self.selected.clear();
        self.confirming = false;
        self.forced = run.forced;
        self.failed = run.failed;
        self.removed += run.gone.len();
        self.branches = orphaned(&run.branches, fresh);
        self.branches_force = false;
    }

    /// One forced removal finished and the lists were read again: `failure`
    /// is why it did not go.
    pub fn after_forced(
        &mut self,
        item: &MergedWorktree,
        failure: Option<String>,
        fresh: Option<&[Branch]>,
    ) {
        self.forced.retain(|(held, _)| held.path != item.path);
        if let Some(message) = failure {
            self.failed.push((item.clone(), message));
            return;
        }
        if let Some(items) = &mut self.items {
            items.retain(|listed| listed.path != item.path);
        }
        self.removed += 1;
        if !orphaned(std::slice::from_ref(&item.branch), fresh).is_empty() {
            self.branches.push(item.branch.clone());
        }
    }

    /// The offered branches were deleted, or some of them were not.
    pub fn after_branches(&mut self, run: BranchesRun) {
        self.branches_force = !run.left.is_empty();
        self.branches = run.left;
        self.branches_deleted += run.deleted;
    }

    /// "Keep them": the offer is withdrawn.
    pub fn keep_branches(&mut self) {
        self.branches.clear();
        self.branches_force = false;
    }
}

/// Removes the worktrees one after the other, without force: a dirty one
/// comes back as `needs_force` and waits for its own confirmation, a failure
/// is gathered rather than stopping the batch. The main checkout, the current
/// worktree and the locked ones never get here — they are not on offer, or
/// not tickable.
pub async fn run(cwd: String, todo: Vec<MergedWorktree>, pre_remove: Option<String>) -> Run {
    let mut run = Run::default();
    for item in todo {
        match git::git_worktree_remove(cwd.clone(), item.path.clone(), false, pre_remove.clone())
            .await
        {
            Ok(result) if result.removed => {
                run.gone.insert(item.path.clone());
                run.branches.push(item.branch.clone());
            }
            Ok(result) if result.needs_force => run.forced.push((item, result.message)),
            Ok(result) => {
                let message = result.message.unwrap_or_default();
                run.failed.push((item, message));
            }
            Err(error) => run.failed.push((item, error)),
        }
    }
    run
}

/// The forced removal of one dirty worktree, after its own confirmation.
/// `None` when it went, git's reason when it did not.
pub async fn force_one(cwd: String, path: String) -> Option<String> {
    match git::git_worktree_remove(cwd, path, true, None).await {
        Ok(result) if result.removed => None,
        Ok(result) => Some(result.message.unwrap_or_default()),
        Err(error) => Some(error),
    }
}

/// `-d` for all; the ones git calls not fully merged are asked about again.
pub async fn delete_branches(cwd: String, names: Vec<String>, force: bool) -> BranchesRun {
    let mut run = BranchesRun::default();
    for name in names {
        match git::git_branch_delete(cwd.clone(), name.clone(), force).await {
            Ok(result) if result.deleted => run.deleted += 1,
            Ok(result) if result.needs_force => run.left.push(name),
            Ok(result) => run.error = result.message,
            Err(error) => run.error = Some(error),
        }
    }
    run
}

#[cfg(test)]
mod tests {
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

    fn local(name: &str, worktree: Option<&str>) -> Branch {
        Branch {
            name: name.into(),
            kind: "local".into(),
            is_current: false,
            worktree_path: worktree.map(str::to_string),
            head: None,
            committer_date: None,
            author_name: None,
            subject: None,
            upstream: None,
            track: None,
        }
    }

    fn offered() -> Cleanup {
        let mut cleanup = Cleanup::loading();
        let mut dirty = merged("/wt/b", "b");
        dirty.dirty = Some(3);
        dirty.preselected = false;
        let mut locked = merged("/wt/c", "c");
        locked.locked = Some(String::new());
        locked.preselected = false;
        let mut unknown = merged("/wt/d", "d");
        unknown.dirty = None;
        unknown.preselected = false;
        cleanup.listed(vec![merged("/wt/a", "a"), dirty, locked, unknown]);
        cleanup
    }

    #[test]
    fn what_looks_finished_is_ticked_and_nothing_else() {
        let cleanup = offered();
        assert_eq!(
            cleanup.selected,
            ["/wt/a".to_string()].into_iter().collect()
        );
        assert_eq!(cleanup.picked().len(), 1);
    }

    #[test]
    fn ticking_disarms_the_confirmation() {
        let mut cleanup = offered();
        cleanup.confirming = true;
        cleanup.toggle("/wt/b");
        assert!(!cleanup.confirming);
        assert_eq!(cleanup.picked().len(), 2);
        cleanup.toggle("/wt/b");
        assert_eq!(cleanup.picked().len(), 1);
    }

    #[test]
    fn a_dirty_or_unreadable_worktree_counts_as_holding_work() {
        let mut cleanup = offered();
        assert_eq!(cleanup.picked_dirty(), 0);
        cleanup.toggle("/wt/b"); // 3 files
        cleanup.toggle("/wt/d"); // unknown
        assert_eq!(cleanup.picked_dirty(), 2);
    }

    #[test]
    fn a_locked_worktree_is_never_removed_even_if_ticked() {
        let mut cleanup = offered();
        cleanup.toggle("/wt/c");
        assert_eq!(cleanup.picked().len(), 2);
        let todo: Vec<_> = cleanup.todo().into_iter().map(|item| item.path).collect();
        assert_eq!(todo, ["/wt/a"]);
    }

    #[test]
    fn after_the_removals_what_went_leaves_the_list_and_its_branches_are_offered() {
        let mut cleanup = offered();
        let run = Run {
            gone: ["/wt/a".to_string()].into_iter().collect(),
            branches: vec!["a".into()],
            forced: vec![(merged("/wt/b", "b"), Some("contains modified files".into()))],
            failed: vec![(merged("/wt/d", "d"), "boom".into())],
        };
        // git no longer ties "a" to a worktree; "b" never went.
        let fresh = [local("a", None), local("b", Some("/wt/b"))];
        cleanup.after_run(run, Some(&fresh));

        let left: Vec<_> = cleanup
            .items
            .iter()
            .flatten()
            .map(|item| item.path.as_str())
            .collect();
        assert_eq!(left, ["/wt/b", "/wt/c", "/wt/d"]);
        assert!(cleanup.selected.is_empty());
        assert_eq!(cleanup.removed, 1);
        assert_eq!(cleanup.branches, ["a"]);
        assert!(!cleanup.branches_force);
        assert_eq!(cleanup.forced.len(), 1);
        assert_eq!(cleanup.failed.len(), 1);
    }

    #[test]
    fn a_branch_is_not_offered_while_a_worktree_still_holds_it_or_when_git_could_not_be_read() {
        let names = vec!["a".to_string(), "b".to_string(), "gone".to_string()];
        let fresh = [local("a", None), local("b", Some("/wt/b"))];
        assert_eq!(orphaned(&names, Some(&fresh)), ["a"]);
        assert!(orphaned(&names, None).is_empty());
        // A remote of the same name is not the local branch.
        let remote = Branch {
            kind: "remote".into(),
            ..local("gone", None)
        };
        assert!(orphaned(&names, Some(&[remote])).is_empty());
    }

    #[test]
    fn a_forced_removal_that_worked_leaves_the_list_and_may_orphan_its_branch() {
        let mut cleanup = offered();
        let held = merged("/wt/b", "b");
        cleanup.forced.push((held.clone(), Some("dirty".into())));
        let fresh = [local("b", None)];
        cleanup.after_forced(&held, None, Some(&fresh));
        assert!(cleanup.forced.is_empty());
        assert_eq!(cleanup.removed, 1);
        assert_eq!(cleanup.branches, ["b"]);
        assert!(
            cleanup
                .items
                .iter()
                .flatten()
                .all(|item| item.path != "/wt/b")
        );
    }

    #[test]
    fn a_forced_removal_that_failed_is_moved_to_the_failures() {
        let mut cleanup = offered();
        let held = merged("/wt/b", "b");
        cleanup.forced.push((held.clone(), Some("dirty".into())));
        cleanup.after_forced(&held, Some("still locked".into()), None);
        assert!(cleanup.forced.is_empty());
        assert_eq!(cleanup.failed.len(), 1);
        assert_eq!(cleanup.failed[0].1, "still locked");
        assert_eq!(cleanup.removed, 0);
        assert!(
            cleanup
                .items
                .iter()
                .flatten()
                .any(|item| item.path == "/wt/b")
        );
    }

    #[test]
    fn the_branches_git_will_not_delete_unforced_are_asked_about_a_second_time() {
        let mut cleanup = offered();
        cleanup.branches = vec!["a".into(), "b".into()];
        cleanup.after_branches(BranchesRun {
            left: vec!["b".into()],
            deleted: 1,
            error: None,
        });
        assert_eq!(cleanup.branches, ["b"]);
        assert!(cleanup.branches_force);
        assert_eq!(cleanup.branches_deleted, 1);

        cleanup.after_branches(BranchesRun {
            left: Vec::new(),
            deleted: 1,
            error: None,
        });
        assert!(cleanup.branches.is_empty());
        assert!(!cleanup.branches_force);
        assert_eq!(cleanup.branches_deleted, 2);
    }

    #[test]
    fn keeping_the_branches_withdraws_the_offer() {
        let mut cleanup = offered();
        cleanup.branches = vec!["a".into()];
        cleanup.branches_force = true;
        cleanup.keep_branches();
        assert!(cleanup.branches.is_empty());
        assert!(!cleanup.branches_force);
    }
}
