//! How Hangar names and places the worktrees it creates: the ones the person
//! makes in the worktree panel, and the one each worker gets
//! (`lib/worktrees.ts`, which `lib/workerControl.ts` also calls).
//!
//! The inventory (§8.1) puts this in `hangar-core/src/worktree_naming.rs`,
//! because the orchestration engine (P19) names its workers' worktrees the
//! same way. This package may not edit `hangar-core`, so it lives here and
//! is public to the crate; moving it is a rename of this file and the import
//! of whoever calls it. Nothing in it touches the window.

use std::sync::LazyLock;

use regex::Regex;

/// A branch name is not a folder name: `feat/login` would nest a level.
pub fn folder_name(branch: &str) -> String {
    static UNFIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"[\\/:*?"<>|\s]+"#).unwrap());
    UNFIT.replace_all(branch, "-").into_owned()
}

/// Ported from microsoft/vscode, `sanitizeBranchName` in
/// extensions/git/src/commands.ts:650 (MIT). Everything git would refuse in a
/// ref name — and everything a shell would fight over — becomes a dash.
///
/// The pass can itself leave a leading dash (".env" → "-env"), which git
/// refuses as an option lookalike, so dashes are stripped again after it. An
/// input with nothing else in it comes out empty, and an empty name is what
/// keeps the Create button off.
pub fn sanitize_branch_name(name: &str) -> String {
    static REFUSED: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^\.|/\.|\.\.|~|\^|:|/$|\.lock$|\.lock/|\\|\*|\s|^\s*$|\.$|\[|\]$").unwrap()
    });
    let trimmed = name.trim().trim_start_matches('-');
    let dashed = REFUSED.replace_all(trimmed, "-");
    dashed.trim_start_matches('-').to_string()
}

/// The files a checkout leaves behind because git ignores them. Defaults taken
/// from Git Worktree Manager's copy-on-create list, trimmed to what actually
/// stops a project from starting: secrets and local editor settings.
pub const COPY_PATTERNS: [&str; 4] = [".env", ".env.*", "*.local", ".vscode/**"];

/// `<parent of the main repo>/<repo>-worktrees/<branch>`: siblings of the
/// checkout rather than children of it, so git never sees them as content.
pub fn worktree_folder(main_root: &str, branch: &str) -> String {
    let separator = if main_root.contains('\\') { "\\" } else { "/" };
    let trimmed = main_root.trim_end_matches(['\\', '/']);
    let mut segments: Vec<&str> = trimmed.split(['\\', '/']).collect();
    let repo = segments.pop().unwrap_or("");
    let home = format!("{repo}-worktrees");
    let folder = folder_name(branch);
    segments.push(&home);
    segments.push(&folder);
    segments.join(separator)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_branch_name_becomes_a_single_folder() {
        assert_eq!(folder_name("feat/login"), "feat-login");
        assert_eq!(folder_name("fix: the \"bug\" <now>"), "fix-the-bug-now-");
        assert_eq!(folder_name("a\\b|c*d?e"), "a-b-c-d-e");
        assert_eq!(folder_name("plain"), "plain");
    }

    #[test]
    fn what_git_refuses_in_a_ref_name_becomes_a_dash() {
        assert_eq!(sanitize_branch_name("feat/login"), "feat/login");
        assert_eq!(sanitize_branch_name("  feat/login  "), "feat/login");
        assert_eq!(sanitize_branch_name("my feature"), "my-feature");
        assert_eq!(sanitize_branch_name("a..b"), "a-b");
        assert_eq!(sanitize_branch_name("a~1^2:b"), "a-1-2-b");
        assert_eq!(sanitize_branch_name("a\\b*c"), "a-b-c");
        assert_eq!(sanitize_branch_name("a/.hidden"), "a-hidden");
        assert_eq!(sanitize_branch_name("name.lock"), "name-");
        assert_eq!(sanitize_branch_name("dir.lock/x"), "dir-x");
        assert_eq!(sanitize_branch_name("trailing/"), "trailing-");
        assert_eq!(sanitize_branch_name("trailing."), "trailing-");
        assert_eq!(sanitize_branch_name("[x]"), "x-");
    }

    #[test]
    fn a_leading_dash_is_never_left_because_git_reads_it_as_an_option() {
        assert_eq!(sanitize_branch_name("--force"), "force");
        // The pass itself makes one out of a leading dot.
        assert_eq!(sanitize_branch_name(".env"), "env");
        assert_eq!(sanitize_branch_name("-.x"), "x");
    }

    #[test]
    fn nothing_but_blanks_comes_out_empty() {
        assert_eq!(sanitize_branch_name(""), "");
        assert_eq!(sanitize_branch_name("   "), "");
        assert_eq!(sanitize_branch_name("-"), "");
        assert_eq!(sanitize_branch_name("..."), "");
    }

    #[test]
    fn a_worktree_lives_next_to_its_repository() {
        assert_eq!(
            worktree_folder("/home/ada/dev/app", "feat/login"),
            "/home/ada/dev/app-worktrees/feat-login"
        );
        assert_eq!(
            worktree_folder("/home/ada/dev/app/", "main"),
            "/home/ada/dev/app-worktrees/main"
        );
        assert_eq!(
            worktree_folder("C:\\dev\\app", "feat/login"),
            "C:\\dev\\app-worktrees\\feat-login"
        );
        assert_eq!(worktree_folder("/app", "x"), "/app-worktrees/x");
    }

    #[test]
    fn the_files_a_checkout_leaves_behind_are_copied_by_pattern() {
        assert_eq!(COPY_PATTERNS, [".env", ".env.*", "*.local", ".vscode/**"]);
    }
}
