//! Making a worktree: what the form's choices come to (the three arguments
//! git needs), and the steps of creating it (`create` of `WorktreePanel.tsx`,
//! `hookProblem` of `lib/git.ts`).
//!
//! The resolution is pure and has tests. The flow is one async function the
//! panel runs as a task and answers with a report; it could live in
//! hangar-core as the inventory suggests (`create_worktree_flow`), and the
//! orchestration engine would then share it. Until then it is here, built on
//! the same core calls a worker's worktree uses.

use hangar_core::git::{self, Branch, HookReport};
use hangar_i18n::{Key, Translator, args};

use super::naming::COPY_PATTERNS;
use super::rows::{local_name_of, ref_key};

// ---- What the form comes to -------------------------------------------------------------------

/// New branch, or one that exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    New,
    Existing,
}

/// What `create` is about to ask git for, once the selection is resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub branch: String,
    pub create_branch: bool,
    pub base_ref: Option<String>,
}

/// Why the selection cannot be made into a plan, with what the sentence needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    NameTaken(String),
    /// A local branch already checked out elsewhere.
    TakenBy(String),
    /// The local branch tracking this remote is checked out elsewhere.
    TrackingTaken {
        name: String,
        path: String,
    },
    LocalExists(String),
}

impl PlanError {
    pub fn text(&self, t: Translator) -> String {
        match self {
            PlanError::NameTaken(name) => t
                .t_with(Key::WorktreesNameTaken, &args!("name" => name.as_str()))
                .into_owned(),
            PlanError::TakenBy(path) => t
                .t_with(Key::WorktreesTakenBy, &args!("path" => path.as_str()))
                .into_owned(),
            PlanError::TrackingTaken { name, path } => t
                .t_with(
                    Key::WorktreesTrackingTaken,
                    &args!("name" => name.as_str(), "path" => path.as_str()),
                )
                .into_owned(),
            PlanError::LocalExists(name) => t
                .t_with(Key::WorktreesLocalExists, &args!("name" => name.as_str()))
                .into_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Resolved {
    pub plan: Option<Plan>,
    pub error: Option<PlanError>,
}

/// Turns the selection into the three arguments git needs. The remote case is
/// Git Worktree Manager's `checkoutBranch`: reuse the branch already tracking
/// it, otherwise create the local name it would get, based on the remote.
///
/// `sanitized` is the typed name once `sanitize_branch_name` has had it;
/// `new_base` and `existing` are `ref_key`s of the pickers.
pub fn resolve(
    mode: Mode,
    sanitized: &str,
    new_base: Option<&str>,
    existing: Option<&str>,
    branches: &[Branch],
) -> Resolved {
    let by_key = |key: &str| branches.iter().find(|branch| ref_key(branch) == key);
    let local = |name: &str| {
        branches
            .iter()
            .find(|branch| branch.kind == "local" && branch.name == name)
    };

    let ok = |plan| Resolved {
        plan: Some(plan),
        error: None,
    };
    let refused = |error| Resolved {
        plan: None,
        error: Some(error),
    };

    if mode == Mode::New {
        if sanitized.is_empty() {
            return Resolved::default();
        }
        if local(sanitized).is_some() {
            return refused(PlanError::NameTaken(sanitized.to_string()));
        }
        let base = new_base.and_then(by_key).map(|branch| branch.name.clone());
        return ok(Plan {
            branch: sanitized.to_string(),
            create_branch: true,
            base_ref: base,
        });
    }

    let Some(picked) = existing.and_then(by_key) else {
        return Resolved::default();
    };

    if picked.kind == "local" {
        return match &picked.worktree_path {
            Some(path) => refused(PlanError::TakenBy(path.clone())),
            None => ok(Plan {
                branch: picked.name.clone(),
                create_branch: false,
                base_ref: None,
            }),
        };
    }

    // A remote branch: the local one already tracking it, if there is one.
    let tracking = branches
        .iter()
        .find(|branch| branch.kind == "local" && branch.upstream.as_deref() == Some(&picked.name));
    if let Some(tracking) = tracking {
        return match &tracking.worktree_path {
            Some(path) => refused(PlanError::TrackingTaken {
                name: tracking.name.clone(),
                path: path.clone(),
            }),
            None => ok(Plan {
                branch: tracking.name.clone(),
                create_branch: false,
                base_ref: None,
            }),
        };
    }

    let derived = local_name_of(&picked.name);
    if local(&derived).is_some() {
        return refused(PlanError::LocalExists(derived));
    }
    // git wires the upstream up on its own when the base is a remote branch.
    ok(Plan {
        branch: derived,
        create_branch: true,
        base_ref: Some(picked.name.clone()),
    })
}

// ---- Folders ----------------------------------------------------------------------------------

/// A hand-typed relative folder, resolved where git will resolve it: against
/// the workspace `git -C` runs in. Probing it against the app's own folder
/// would answer for the wrong directory.
pub fn resolve_folder(cwd: &str, path: &str) -> String {
    let mut chars = path.chars();
    let absolute = match (chars.next(), chars.next(), chars.next()) {
        (Some('/' | '\\'), _, _) => true,
        (Some(drive), Some(':'), Some('/' | '\\')) => drive.is_ascii_alphabetic(),
        _ => false,
    };
    if absolute {
        return path.to_string();
    }
    let separator = if cwd.contains('\\') { '\\' } else { '/' };
    format!("{}{separator}{path}", cwd.trim_end_matches(['\\', '/']))
}

/// VS Code gives up after 20 suffixes too (repository.ts:1962).
pub const FOLDER_TRIES: usize = 20;

/// The first free `<path>`, `<path>-1`, `<path>-2`… or `None` when even
/// those are taken — VS Code's loop when the folder of a new worktree
/// already exists.
pub fn free_folder(base: &str, is_dir: impl Fn(&str) -> bool) -> Option<String> {
    if !is_dir(base) {
        return Some(base.to_string());
    }
    (1..=FOLDER_TRIES)
        .map(|index| format!("{base}-{index}"))
        .find(|candidate| !is_dir(candidate))
}

// ---- Hooks ------------------------------------------------------------------------------------

/// One line for a failed hook, with its output after it; `None` when it
/// passed (`hookProblem`).
pub fn hook_problem(report: &HookReport) -> Option<String> {
    if report.ok {
        return None;
    }
    let why = if let Some(error) = &report.error {
        format!("could not run: {error}")
    } else if report.timed_out {
        "timed out and was stopped".to_string()
    } else {
        match report.exit_code {
            None => "was stopped by a signal".to_string(),
            Some(code) => format!("failed (exit {code})"),
        }
    };
    Some(if report.output.is_empty() {
        why
    } else {
        format!("{why}: {}", report.output)
    })
}

// ---- The flow ---------------------------------------------------------------------------------

/// Everything the flow needs, gathered when Create was pressed.
#[derive(Debug, Clone)]
pub struct CreateRequest {
    /// The workspace's folder, where `git -C` runs.
    pub cwd: String,
    /// The repository's root, which the untracked files are copied from.
    pub root: Option<String>,
    pub plan: Plan,
    /// The folder as the form had it, relative or not.
    pub wanted: String,
    pub copy_untracked: bool,
    /// The person's `postCreate` command; blank runs nothing.
    pub post_create: Option<String>,
}

/// What came of a worktree that was made: where it is, and what happened
/// around it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Created {
    pub target: String,
    pub branch: String,
    /// One sentence each, for the line that says what came with it.
    pub notes: Vec<String>,
    /// Half a failure: the worktree is there, something around it is not.
    pub warn: bool,
    /// The postCreate command's failure, which stays on screen until the next
    /// action; the worktree is kept all the same.
    pub hook_failure: Option<String>,
}

/// The copy is a convenience: it reports what it did and never undoes the
/// worktree, which exists by then whatever happens here.
async fn copy_report(from: String, to: String, t: Translator) -> (String, bool) {
    let patterns = COPY_PATTERNS.iter().map(|p| p.to_string()).collect();
    match git::git_copy_untracked(from, to, patterns, None).await {
        Ok(result) => {
            let copied = t.tn(Key::WorktreesCopied, result.copied, &[]).into_owned();
            if result.failed == 0 {
                return (copied, false);
            }
            let failed = t
                .tn(Key::WorktreesCopyFailed, result.failed, &[])
                .into_owned();
            (format!("{copied} {failed}"), true)
        }
        Err(error) => (
            t.t_with(Key::WorktreesCopyError, &args!("error" => error.as_str()))
                .into_owned(),
            true,
        ),
    }
}

/// Makes the worktree and what goes with it: the free folder, `git worktree
/// add`, the untracked files, the person's command. An `Err` means no
/// worktree was made.
pub async fn run(request: CreateRequest, t: Translator) -> Result<Created, String> {
    let wanted = resolve_folder(&request.cwd, request.wanted.trim());
    let target = free_folder(&wanted, |candidate| {
        hangar_core::store::dir_exists(candidate.to_string())
    })
    .ok_or_else(|| {
        t.t_with(Key::WorktreesFolderTaken, &args!("path" => wanted.as_str()))
            .into_owned()
    })?;

    git::git_worktree_add(
        request.cwd.clone(),
        target.clone(),
        request.plan.branch.clone(),
        request.plan.create_branch,
        request.plan.base_ref.clone(),
    )
    .await?;

    // One line for the whole aftermath: where it landed, what came with it.
    let mut notes = Vec::new();
    let mut warn = false;
    if target != wanted {
        notes.push(
            t.t_with(
                Key::WorktreesFolderSuffixed,
                &args!("path" => target.as_str()),
            )
            .into_owned(),
        );
    }
    if request.copy_untracked {
        let from = request.root.clone().unwrap_or_else(|| request.cwd.clone());
        let (text, warned) = copy_report(from, target.clone(), t).await;
        notes.push(text);
        warn |= warned;
    }

    // After the copy, so the command finds the .env it may need. A failure
    // keeps the worktree and stays on screen until the next action.
    let mut hook_failure = None;
    if let Some(command) = &request.post_create {
        match git::git_worktree_hook(request.cwd.clone(), target.clone(), Some(command.clone()))
            .await
        {
            Ok(Some(report)) => match hook_problem(&report) {
                Some(problem) => hook_failure = Some(problem),
                None => notes.push(t.t(Key::WorktreesHookPostDone).into_owned()),
            },
            Ok(None) => {}
            Err(error) => hook_failure = Some(error),
        }
    }
    let hook_failure = hook_failure.map(|problem| {
        t.t_with(
            Key::WorktreesHookPostFailed,
            &args!("problem" => problem.as_str()),
        )
        .into_owned()
    });

    Ok(Created {
        target,
        branch: request.plan.branch,
        notes,
        warn,
        hook_failure,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn local(name: &str, worktree: Option<&str>, upstream: Option<&str>) -> Branch {
        Branch {
            name: name.into(),
            kind: "local".into(),
            is_current: false,
            worktree_path: worktree.map(str::to_string),
            head: None,
            committer_date: None,
            author_name: None,
            subject: None,
            upstream: upstream.map(str::to_string),
            track: None,
        }
    }

    fn remote(name: &str) -> Branch {
        Branch {
            kind: "remote".into(),
            ..local(name, None, None)
        }
    }

    fn branches() -> Vec<Branch> {
        vec![
            local("main", Some("/work/app"), Some("origin/main")),
            local(
                "feat/login",
                Some("/work/app-worktrees/feat-login"),
                Some("origin/feat/login"),
            ),
            local("dev", None, Some("origin/dev")),
            remote("origin/main"),
            remote("origin/feat/login"),
            remote("origin/dev"),
            remote("origin/feat/pay"),
        ]
    }

    fn plan(branch: &str, create: bool, base: Option<&str>) -> Option<Plan> {
        Some(Plan {
            branch: branch.into(),
            create_branch: create,
            base_ref: base.map(str::to_string),
        })
    }

    mod new_branch {
        use super::*;

        #[test]
        fn a_new_name_makes_a_branch_from_the_current_head() {
            let found = resolve(Mode::New, "feat/search", None, None, &branches());
            assert_eq!(found.plan, plan("feat/search", true, None));
            assert_eq!(found.error, None);
        }

        #[test]
        fn nothing_typed_is_no_plan_and_no_complaint() {
            assert_eq!(
                resolve(Mode::New, "", None, None, &branches()),
                Resolved::default()
            );
        }

        #[test]
        fn a_name_that_is_a_local_branch_is_refused() {
            let found = resolve(Mode::New, "dev", None, None, &branches());
            assert_eq!(found.plan, None);
            assert_eq!(found.error, Some(PlanError::NameTaken("dev".into())));
            // A remote of that name is not a local branch.
            assert!(
                resolve(Mode::New, "origin/dev", None, None, &branches())
                    .plan
                    .is_some()
            );
        }

        #[test]
        fn the_base_is_the_named_ref_local_or_remote() {
            let from_local = resolve(Mode::New, "x", Some("local:dev"), None, &branches());
            assert_eq!(from_local.plan, plan("x", true, Some("dev")));
            let from_remote = resolve(Mode::New, "x", Some("remote:origin/dev"), None, &branches());
            assert_eq!(from_remote.plan, plan("x", true, Some("origin/dev")));
            // A base that no longer exists falls back to the current head.
            let gone = resolve(Mode::New, "x", Some("local:vanished"), None, &branches());
            assert_eq!(gone.plan, plan("x", true, None));
        }
    }

    mod existing_branch {
        use super::*;

        #[test]
        fn nothing_picked_is_no_plan() {
            assert_eq!(
                resolve(Mode::Existing, "", None, None, &branches()),
                Resolved::default()
            );
            assert_eq!(
                resolve(
                    Mode::Existing,
                    "",
                    None,
                    Some("local:vanished"),
                    &branches()
                ),
                Resolved::default()
            );
        }

        #[test]
        fn a_free_local_branch_is_checked_out_as_it_is() {
            let found = resolve(Mode::Existing, "", None, Some("local:dev"), &branches());
            assert_eq!(found.plan, plan("dev", false, None));
        }

        #[test]
        fn a_local_branch_held_elsewhere_says_where() {
            let found = resolve(
                Mode::Existing,
                "",
                None,
                Some("local:feat/login"),
                &branches(),
            );
            assert_eq!(found.plan, None);
            assert_eq!(
                found.error,
                Some(PlanError::TakenBy("/work/app-worktrees/feat-login".into()))
            );
        }

        #[test]
        fn a_remote_branch_reuses_the_local_one_that_tracks_it() {
            let found = resolve(
                Mode::Existing,
                "",
                None,
                Some("remote:origin/dev"),
                &branches(),
            );
            assert_eq!(found.plan, plan("dev", false, None));
        }

        #[test]
        fn a_remote_whose_tracking_branch_is_held_says_so() {
            let found = resolve(
                Mode::Existing,
                "",
                None,
                Some("remote:origin/feat/login"),
                &branches(),
            );
            assert_eq!(
                found.error,
                Some(PlanError::TrackingTaken {
                    name: "feat/login".into(),
                    path: "/work/app-worktrees/feat-login".into()
                })
            );
        }

        #[test]
        fn a_remote_with_no_local_branch_gets_one_named_without_its_remote() {
            let found = resolve(
                Mode::Existing,
                "",
                None,
                Some("remote:origin/feat/pay"),
                &branches(),
            );
            assert_eq!(found.plan, plan("feat/pay", true, Some("origin/feat/pay")));
        }

        #[test]
        fn a_derived_name_that_is_taken_by_an_unrelated_branch_is_refused() {
            let mut all = branches();
            // "feat/pay" exists locally but tracks nothing.
            all.push(local("feat/pay", None, None));
            let found = resolve(
                Mode::Existing,
                "",
                None,
                Some("remote:origin/feat/pay"),
                &all,
            );
            assert_eq!(found.error, Some(PlanError::LocalExists("feat/pay".into())));
        }
    }

    mod folders {
        use super::*;

        #[test]
        fn a_relative_folder_is_resolved_against_the_workspace() {
            assert_eq!(resolve_folder("/work/app", "../wt/x"), "/work/app/../wt/x");
            assert_eq!(resolve_folder("/work/app/", "wt"), "/work/app/wt");
            assert_eq!(resolve_folder("C:\\work\\app", "wt"), "C:\\work\\app\\wt");
        }

        #[test]
        fn an_absolute_folder_is_left_alone() {
            assert_eq!(resolve_folder("/work/app", "/elsewhere/x"), "/elsewhere/x");
            assert_eq!(resolve_folder("C:\\work", "D:\\wt\\x"), "D:\\wt\\x");
            assert_eq!(resolve_folder("C:\\work", "d:/wt/x"), "d:/wt/x");
            assert_eq!(
                resolve_folder("C:\\work", "\\\\server\\share"),
                "\\\\server\\share"
            );
            // "c:x" is a drive-relative path, not an absolute one.
            assert_eq!(resolve_folder("/work", "c:x"), "/work/c:x");
        }

        #[test]
        fn the_first_free_folder_takes_a_numeric_suffix() {
            let taken: HashSet<&str> = ["/wt/x", "/wt/x-1", "/wt/x-2"].into_iter().collect();
            let exists = |path: &str| taken.contains(path);
            assert_eq!(free_folder("/wt/y", exists), Some("/wt/y".into()));
            assert_eq!(free_folder("/wt/x", exists), Some("/wt/x-3".into()));
        }

        #[test]
        fn twenty_suffixes_and_then_it_gives_up() {
            assert_eq!(free_folder("/wt/x", |_| true), None);
            // The twentieth is still tried.
            let taken = |path: &str| path != "/wt/x-20";
            assert_eq!(free_folder("/wt/x", taken), Some("/wt/x-20".into()));
        }
    }

    mod hooks {
        use super::*;

        fn report(ok: bool) -> HookReport {
            HookReport {
                ok,
                ..HookReport::default()
            }
        }

        #[test]
        fn a_hook_that_passed_has_no_problem() {
            assert_eq!(hook_problem(&report(true)), None);
        }

        #[test]
        fn a_failure_says_how_it_ended_and_what_it_printed() {
            let failed = HookReport {
                exit_code: Some(3),
                output: "boom".into(),
                ..report(false)
            };
            assert_eq!(
                hook_problem(&failed).as_deref(),
                Some("failed (exit 3): boom")
            );
            let silent = HookReport {
                exit_code: Some(1),
                ..report(false)
            };
            assert_eq!(hook_problem(&silent).as_deref(), Some("failed (exit 1)"));
            let signalled = report(false);
            assert_eq!(
                hook_problem(&signalled).as_deref(),
                Some("was stopped by a signal")
            );
            let slow = HookReport {
                timed_out: true,
                ..report(false)
            };
            assert_eq!(
                hook_problem(&slow).as_deref(),
                Some("timed out and was stopped")
            );
            let missing = HookReport {
                error: Some("no such shell".into()),
                ..report(false)
            };
            assert_eq!(
                hook_problem(&missing).as_deref(),
                Some("could not run: no such shell")
            );
        }
    }
}
