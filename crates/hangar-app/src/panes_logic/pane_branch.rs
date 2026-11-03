//! The git branch a pane's header shows beside its name (`lib/paneBranch.ts`
//! and `strings/paneBranch.ts`).
//!
//! One `git_repo_info` call per folder, however many panes sit on it: the
//! answers live in a cache keyed by folder, a call in flight is shared, and
//! an answer stays good for `PANE_BRANCH_TTL_MS`. A pane asks once on mount,
//! again every `PANE_BRANCH_REFRESH_MS`, and when it takes the focus; no file
//! watcher.
//!
//! The cache is the pure half: `lookup` says whether to ask git, `answered`
//! takes the answer in. The asking itself is in `panes_logic::update`, which
//! runs `git_repo_info` off the UI thread. The four strings of the web app's
//! own dictionary (`paneBranch.*`) are `hangar-i18n` keys now.

use std::collections::HashMap;

use hangar_core::git::RepoInfo;

use crate::state::live::EpochMs;

/// How long an answer is trusted before the next ask goes to git.
pub const PANE_BRANCH_TTL_MS: u64 = 30_000;
/// How often a mounted pane asks again.
pub const PANE_BRANCH_REFRESH_MS: u64 = 45_000;
/// A focus right after an answer does not ask again: panes focus in bursts.
pub const PANE_BRANCH_FOCUS_MS: u64 = 2_000;

/// What the header says; `None` means it says nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneBranch {
    Branch { name: String, linked_worktree: bool },
    Detached { head: String },
}

/// Reads the header's branch off git's answer. Outside a repo, or without
/// git: `None`.
pub fn pane_branch_of(info: Option<&RepoInfo>) -> Option<PaneBranch> {
    let info = info?;
    if !info.installed || !info.is_repo {
        return None;
    }
    if let Some(name) = info.branch.as_deref().filter(|name| !name.is_empty()) {
        return Some(PaneBranch::Branch {
            name: name.to_string(),
            linked_worktree: info.is_linked_worktree,
        });
    }
    // No branch name: a detached head, which the short sha stands for.
    info.head
        .as_deref()
        .filter(|head| !head.is_empty())
        .map(|head| PaneBranch::Detached {
            head: head.to_string(),
        })
}

fn is_drive(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic()) && chars.next() == Some(':')
}

/// The same folder written two ways is one cache entry.
pub fn folder_key(folder: &str) -> String {
    let mut key = folder.trim().replace('\\', "/");
    // A trailing slash goes, except the root of a drive (`c:/`) and of the
    // file system (`/`).
    while key.len() > 1 && key.ends_with('/') && !(key.len() == 3 && is_drive(&key)) {
        key.pop();
    }
    // A drive path is case-insensitive.
    if is_drive(&key) {
        key = key.to_lowercase();
    }
    key
}

#[derive(Debug, Default)]
struct Entry {
    branch: Option<PaneBranch>,
    /// When the answer came in; `None` until the first one.
    at: Option<EpochMs>,
    /// A call is on its way and will answer for everyone who asks meanwhile.
    pending: bool,
}

/// What a lookup found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lookup {
    /// The cached answer is young enough.
    Cached,
    /// Another ask is already on its way; its answer will do.
    InFlight,
    /// Ask git now, then call `answered`. The entry is marked in flight.
    Ask,
}

#[derive(Debug, Default)]
pub struct BranchCache {
    entries: HashMap<String, Entry>,
}

impl BranchCache {
    /// The branch of a folder, asking git only when the cached answer is older
    /// than `max_age_ms` and no call is already on its way.
    pub fn lookup(&mut self, folder: &str, now: EpochMs, max_age_ms: u64) -> Lookup {
        let entry = self.entries.entry(folder_key(folder)).or_default();
        if entry.pending {
            return Lookup::InFlight;
        }
        if entry
            .at
            .is_some_and(|at| now.saturating_sub(at) < max_age_ms)
        {
            return Lookup::Cached;
        }
        entry.pending = true;
        Lookup::Ask
    }

    /// Takes in git's answer; a failing call reads as nothing to show.
    /// Whether the branch shown changed.
    pub fn answered(&mut self, folder: &str, branch: Option<PaneBranch>, now: EpochMs) -> bool {
        let entry = self.entries.entry(folder_key(folder)).or_default();
        let changed = entry.branch != branch;
        entry.branch = branch;
        entry.at = Some(now);
        entry.pending = false;
        changed
    }

    /// The cached branch of a folder, without asking.
    pub fn peek(&self, folder: &str) -> Option<&PaneBranch> {
        self.entries.get(&folder_key(folder))?.branch.as_ref()
    }

    /// Forgets every answer. For tests.
    pub fn reset(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hangar_i18n::{Key, Locale, Translator, args};

    fn info(patch: impl FnOnce(&mut RepoInfo)) -> RepoInfo {
        let mut info = RepoInfo {
            installed: true,
            is_repo: true,
            root: Some("C:/r".into()),
            main_root: Some("C:/r".into()),
            is_linked_worktree: false,
            branch: Some("main".into()),
            head: Some("abc1234".into()),
        };
        patch(&mut info);
        info
    }

    fn named(name: &str) -> Option<PaneBranch> {
        Some(PaneBranch::Branch {
            name: name.into(),
            linked_worktree: false,
        })
    }

    // paneBranchOf

    #[test]
    fn names_the_branch() {
        assert_eq!(
            pane_branch_of(Some(&info(|i| i.is_linked_worktree = true))),
            Some(PaneBranch::Branch {
                name: "main".into(),
                linked_worktree: true
            })
        );
    }

    #[test]
    fn gives_the_short_sha_of_a_detached_head() {
        assert_eq!(
            pane_branch_of(Some(&info(|i| i.branch = None))),
            Some(PaneBranch::Detached {
                head: "abc1234".into()
            })
        );
    }

    #[test]
    fn says_nothing_outside_a_repo_without_git_or_on_an_unborn_head() {
        assert_eq!(
            pane_branch_of(Some(&info(|i| {
                i.is_repo = false;
                i.branch = None;
            }))),
            None
        );
        assert_eq!(
            pane_branch_of(Some(&info(|i| {
                i.installed = false;
                i.is_repo = false;
            }))),
            None
        );
        assert_eq!(
            pane_branch_of(Some(&info(|i| {
                i.branch = None;
                i.head = None;
            }))),
            None
        );
        assert_eq!(pane_branch_of(None), None);
    }

    // folderKey

    #[test]
    fn merges_the_spellings_of_one_folder() {
        assert_eq!(folder_key("C:\\Repo\\app\\"), folder_key("c:/repo/app"));
        assert_eq!(folder_key("/home/a/"), "/home/a");
        assert_eq!(folder_key("C:\\"), "c:/");
        assert_eq!(folder_key("/"), "/");
    }

    // loadPaneBranch

    /// A stand-in for the asking: counts the calls git would have got and
    /// answers each with the next branch it was given.
    struct Git {
        cache: BranchCache,
        calls: usize,
    }

    impl Git {
        fn new() -> Git {
            Git {
                cache: BranchCache::default(),
                calls: 0,
            }
        }

        /// `loadPaneBranch(folder, max_age)` with git answering `answer`.
        fn load(
            &mut self,
            folder: &str,
            now: EpochMs,
            max_age: u64,
            answer: Option<PaneBranch>,
        ) -> Option<PaneBranch> {
            if self.cache.lookup(folder, now, max_age) == Lookup::Ask {
                self.calls += 1;
                self.cache.answered(folder, answer, now);
            }
            self.cache.peek(folder).cloned()
        }
    }

    #[test]
    fn makes_one_call_for_several_panes_on_a_folder() {
        let mut git = Git::new();
        // Three spellings of one folder, asked while the first is in flight.
        assert_eq!(
            git.cache.lookup("C:\\r", 0, PANE_BRANCH_TTL_MS),
            Lookup::Ask
        );
        assert_eq!(
            git.cache.lookup("c:/r/", 0, PANE_BRANCH_TTL_MS),
            Lookup::InFlight
        );
        assert_eq!(
            git.cache.lookup("C:/r", 0, PANE_BRANCH_TTL_MS),
            Lookup::InFlight
        );
        assert!(git.cache.answered("C:\\r", named("main"), 5));
        for spelling in ["C:\\r", "c:/r/", "C:/r"] {
            assert_eq!(git.cache.peek(spelling), named("main").as_ref());
        }
        // And later asks find it fresh.
        assert_eq!(
            git.cache.lookup("C:/r", 10, PANE_BRANCH_TTL_MS),
            Lookup::Cached
        );
    }

    #[test]
    fn asks_once_per_folder() {
        let mut git = Git::new();
        let a = git.load("C:/one", 0, PANE_BRANCH_TTL_MS, named("a"));
        let b = git.load("C:/two", 0, PANE_BRANCH_TTL_MS, named("b"));
        assert_eq!(git.calls, 2);
        assert_eq!(a, named("a"));
        assert_eq!(b, named("b"));
    }

    #[test]
    fn asks_again_once_the_answer_is_stale_and_on_a_focus_after_the_short_delay() {
        let mut git = Git::new();
        git.load("C:/r", 0, PANE_BRANCH_TTL_MS, named("main"));
        git.load(
            "C:/r",
            PANE_BRANCH_FOCUS_MS - 1,
            PANE_BRANCH_FOCUS_MS,
            named("x"),
        );
        assert_eq!(git.calls, 1);

        let t = PANE_BRANCH_FOCUS_MS + 1;
        git.load("C:/r", t, PANE_BRANCH_FOCUS_MS, named("other"));
        assert_eq!(git.calls, 2);
        assert_eq!(git.cache.peek("C:/r"), named("other").as_ref());
        git.load("C:/r", t, PANE_BRANCH_TTL_MS, named("x"));
        assert_eq!(git.calls, 2);

        git.load(
            "C:/r",
            t + PANE_BRANCH_TTL_MS,
            PANE_BRANCH_TTL_MS,
            named("later"),
        );
        assert_eq!(git.calls, 3);
    }

    #[test]
    fn reads_a_failing_call_as_nothing_to_show() {
        let mut git = Git::new();
        git.load("C:/r", 0, PANE_BRANCH_TTL_MS, named("main"));
        // The next ask fails: the header says nothing rather than the old branch.
        assert_eq!(
            git.load("C:/r", PANE_BRANCH_TTL_MS, PANE_BRANCH_TTL_MS, None),
            None
        );
        assert_eq!(git.calls, 2);
    }

    #[test]
    fn says_whether_the_answer_changed_what_is_shown() {
        let mut cache = BranchCache::default();
        assert_eq!(cache.lookup("/r", 0, PANE_BRANCH_TTL_MS), Lookup::Ask);
        assert!(cache.answered("/r", named("main"), 0));
        assert_eq!(
            cache.lookup("/r", PANE_BRANCH_TTL_MS, PANE_BRANCH_TTL_MS),
            Lookup::Ask
        );
        assert!(!cache.answered("/r", named("main"), PANE_BRANCH_TTL_MS));
        assert!(cache.answered("/r", None, 2 * PANE_BRANCH_TTL_MS));
    }

    // strings

    #[test]
    fn has_the_four_strings_in_both_languages_and_fills_the_parameters() {
        let fr = Translator::new(Locale::Fr);
        let en = Translator::new(Locale::En);
        for key in [
            Key::PaneBranchBranch,
            Key::PaneBranchBranchWorktree,
            Key::PaneBranchDetached,
            Key::PaneBranchDetachedMark,
        ] {
            assert!(!fr.t(key).is_empty() && !en.t(key).is_empty());
        }
        assert_eq!(
            fr.t_with(Key::PaneBranchDetached, &args!("head" => "abc")),
            "HEAD détachée sur abc"
        );
        assert_eq!(
            en.t_with(Key::PaneBranchBranch, &args!("branch" => "x")),
            "Git branch x"
        );
    }
}
