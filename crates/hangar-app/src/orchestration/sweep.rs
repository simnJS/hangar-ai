//! The worktree sweep a workspace gets the first time the runtime sees it:
//! what it may remove, and what it must leave alone.
//!
//! `hangar_core::git::sweep_orphan_worktrees` removes the merged, clean,
//! unlocked worktrees of the repository that no folder of `held` is in. The
//! panes this Hangar knows are not enough to fill `held`: another Hangar on
//! the same machine (another data root, a debug instance a worker started
//! with a workspace inside its worktree) has panes this one cannot see, and
//! a worker whose branch has no commit of its own yet is "merged" as soon as
//! the target moves. Both together removed a running worker's worktree and
//! branch. So a worktree goes only when this workspace's board says its
//! task is done, and no live process stands in it; every other one is held.

use std::path::{Path, PathBuf};

use hangar_core::board::Task;
use hangar_core::git::{OrphanSweep, Worktree};

/// The first characters of a task's id that name its worker's branch, as
/// `worker::worker_branch` takes them.
fn branch_id(task: &Task) -> String {
    task.id
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .take(6)
        .collect()
}

/// Whether `branch` is the branch Hangar made for a worker on `task`:
/// `hangar/<slug>-<id>`, or `hangar/<slug>-<id>-<n>` when the first name was
/// taken.
fn names_task(branch: &str, task: &Task) -> bool {
    let id = branch_id(task);
    let Some(rest) = branch.strip_prefix("hangar/") else {
        return false;
    };
    if id.is_empty() {
        return false;
    }
    let tail = format!("-{id}");
    if rest.ends_with(&tail) {
        return true;
    }
    match rest.rsplit_once('-') {
        Some((head, round)) if !round.is_empty() && round.bytes().all(|b| b.is_ascii_digit()) => {
            head.ends_with(&tail)
        }
        _ => false,
    }
}

/// Whether the board says the work of the worktree on `branch` is over: its
/// branch names exactly one task of `tasks`, and that task is done.
fn finished(branch: Option<&str>, tasks: &[Task]) -> bool {
    let Some(branch) = branch else {
        return false;
    };
    let mut named = tasks.iter().filter(|task| names_task(branch, task));
    match (named.next(), named.next()) {
        (Some(task), None) => task.column == "done",
        _ => false,
    }
}

/// Whether a live process stands in `path`. `None` (the system does not
/// say): the board alone decides.
fn occupied(path: &str, dirs: Option<&[(u32, PathBuf)]>) -> bool {
    dirs.is_some_and(|dirs| {
        !hangar_core::delivery::occupied::standing_in(dirs, Path::new(path)).is_empty()
    })
}

/// The linked worktrees of `listed` the sweep must leave alone: every one
/// whose task is not done on `tasks` (the workspace's board), or that a
/// live process of `dirs` stands in.
pub(super) fn guarded(
    listed: &[Worktree],
    tasks: &[Task],
    dirs: Option<&[(u32, PathBuf)]>,
) -> Vec<String> {
    listed
        .iter()
        .filter(|tree| !tree.is_main)
        .filter(|tree| !finished(tree.branch.as_deref(), tasks) || occupied(&tree.path, dirs))
        .map(|tree| tree.path.clone())
        .collect()
}

/// Sweeps the repository of the workspace folder `cwd`: `listed`, its
/// worktrees as git listed them, `held` the folders this Hangar's panes run
/// in, `tasks` the workspace's board. Nothing is swept when git listed
/// nothing: what is not known is kept.
pub(super) fn sweep_workspace(
    cwd: &str,
    listed: &[Worktree],
    held: &[String],
    tasks: &[Task],
    dirs: Option<&[(u32, PathBuf)]>,
) -> Result<OrphanSweep, String> {
    if listed.is_empty() {
        return Ok(OrphanSweep::default());
    }
    let mut held = held.to_vec();
    held.extend(guarded(listed, tasks, dirs));
    hangar_core::git::sweep_orphan_worktrees(Path::new(cwd), &held)
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;
    use crate::orchestration::testkit::task;

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn commit(dir: &Path, file: &str, message: &str) {
        std::fs::write(dir.join(file), message).unwrap();
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-q", "-m", message]);
    }

    fn task_in(id: &str, column: &str) -> Task {
        Task {
            column: column.into(),
            ..task(id)
        }
    }

    /// What happened to Finn: a repository whose main moved on since two
    /// workers started from it. Finn's branch has no commit of its own (so
    /// git calls it merged), its worktree is clean (Finn works elsewhere),
    /// Finn's agent still stands in it. Cleo's worktree has work in it, and
    /// a debug Hangar Cleo started has its workspace in a folder inside it.
    struct Scene {
        main: PathBuf,
        finn: PathBuf,
        cleo: PathBuf,
        nested: PathBuf,
    }

    const FINN: &str = "hangar/endurance-audit-04f8c6";
    const CLEO: &str = "hangar/audit-rendering-bddfd5";

    fn scene() -> Scene {
        let root = crate::kit_tabs::testdir::scratch("sweep");
        // macOS's temp folder is behind a link; git lists the real path.
        #[cfg(unix)]
        let root = std::fs::canonicalize(&root).unwrap();
        let main = root.join("repo");
        std::fs::create_dir_all(&main).unwrap();
        git(&main, &["init", "-q", "-b", "main"]);
        for (name, value) in [
            ("user.name", "Test"),
            ("user.email", "test@example.com"),
            ("commit.gpgsign", "false"),
        ] {
            git(&main, &["config", name, value]);
        }
        commit(&main, "a.txt", "start");
        let trees = root.join("repo-worktrees");
        let finn = trees.join("hangar-endurance-audit-04f8c6");
        let cleo = trees.join("hangar-audit-rendering-bddfd5");
        for (path, branch) in [(&finn, FINN), (&cleo, CLEO)] {
            git(
                &main,
                &[
                    "worktree",
                    "add",
                    "-q",
                    "-b",
                    branch,
                    &path.to_string_lossy(),
                ],
            );
        }
        // Another delivery lands: both branches are now behind main.
        commit(&main, "b.txt", "landed");
        let nested = cleo.join(".frame-timing/runs/tmpu27aklab/chatcwd");
        std::fs::create_dir_all(&nested).unwrap();
        Scene {
            main,
            finn,
            cleo,
            nested,
        }
    }

    fn list(dir: &Path) -> Vec<Worktree> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime
            .block_on(hangar_core::git::git_worktree_list(
                dir.to_string_lossy().into_owned(),
            ))
            .unwrap()
    }

    fn branch_exists(main: &Path, branch: &str) -> bool {
        Command::new("git")
            .args([
                "rev-parse",
                "--verify",
                "-q",
                &format!("refs/heads/{branch}"),
            ])
            .current_dir(main)
            .status()
            .unwrap()
            .success()
    }

    /// The debug instance knows only its own panes, which run in its
    /// workspace's folder, and its board (in that folder) is empty. Before,
    /// it removed Finn's worktree and branch.
    #[test]
    fn another_hangars_sweep_leaves_a_running_workers_worktree_alone() {
        let scene = scene();
        let cwd = scene.nested.to_string_lossy().into_owned();
        let held = vec![cwd.clone()];
        let listed = list(&scene.nested);
        // Finn's agent is not modelled as a process here: the board alone
        // has to keep it.
        let sweep = sweep_workspace(&cwd, &listed, &held, &[], Some(&[])).expect("swept");
        assert!(sweep.removed.is_empty(), "{sweep:?}");
        assert!(scene.finn.exists(), "{sweep:?}");
        assert!(branch_exists(&scene.main, FINN));
        assert!(scene.cleo.exists());
    }

    /// The real workspace's board: Finn's task is in doing, so it stays even
    /// with no pane of this Hangar in it; a done task's worktree goes.
    #[test]
    fn only_the_worktree_of_a_done_task_goes() {
        let scene = scene();
        let cwd = scene.main.to_string_lossy().into_owned();
        let tasks = vec![
            task_in("04f8c659-14dd", "doing"),
            task_in("bddfd5c6-2d54", "done"),
        ];
        let listed = list(&scene.main);
        let sweep = sweep_workspace(&cwd, &listed, std::slice::from_ref(&cwd), &tasks, Some(&[]))
            .expect("swept");
        assert!(scene.finn.exists(), "{sweep:?}");
        assert!(branch_exists(&scene.main, FINN));
        assert!(!scene.cleo.exists(), "{sweep:?}");
        assert!(!branch_exists(&scene.main, CLEO));
    }

    /// Done on the board, yet a live process stands in it (a pane of a
    /// Hangar this one does not see): kept. Live processes are found
    /// through /proc, so on Linux only (elsewhere the board rule alone holds).
    #[cfg(target_os = "linux")]
    #[test]
    fn a_worktree_a_live_process_stands_in_is_kept() {
        let scene = scene();
        let cwd = scene.main.to_string_lossy().into_owned();
        let tasks = vec![task_in("04f8c659-14dd", "done")];
        let listed = list(&scene.main);
        let dirs = vec![(u32::MAX, scene.finn.join("src"))];
        let sweep = sweep_workspace(
            &cwd,
            &listed,
            std::slice::from_ref(&cwd),
            &tasks,
            Some(&dirs),
        )
        .expect("swept");
        assert!(scene.finn.exists(), "{sweep:?}");
        assert!(branch_exists(&scene.main, FINN));
    }

    #[test]
    fn a_branch_names_its_task_by_the_first_characters_of_its_id() {
        let finn = task("04f8c659-14dd");
        assert!(names_task("hangar/endurance-audit-04f8c6", &finn));
        assert!(names_task("hangar/endurance-audit-04f8c6-2", &finn));
        assert!(!names_task("hangar/endurance-audit-04f8c7", &finn));
        assert!(!names_task("probe/endurance-04f8c6", &finn));
        assert!(!names_task("hangar/x-04f8c6x", &finn));
        // Two tasks the branch could be: neither decides.
        let twins = [task_in("04f8c6aa", "done"), task_in("04f8c6bb", "doing")];
        assert!(!finished(Some("hangar/x-04f8c6"), &twins));
        assert!(finished(Some("hangar/x-04f8c6"), &twins[..1]));
        assert!(!finished(None, &twins[..1]));
    }
}
