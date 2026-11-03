//! What a worktree row says and how paths and branch names are compared
//! (the small helpers at the top of `WorktreePanel.tsx`), as pure functions.

use hangar_core::git::Branch;
use hangar_i18n::{Key, Translator, args};

/// Two paths are the same folder. Windows is the target here: the separators
/// go both ways, the case carries no meaning, and a trailing one is
/// decoration.
pub fn normalize(path: &str) -> String {
    path.trim_end_matches(['\\', '/'])
        .replace('\\', "/")
        .to_lowercase()
}

pub fn same_path(a: &str, b: &str) -> bool {
    normalize(a) == normalize(b)
}

/// The folder's own name: the last segment, or the path when it has none.
pub fn last_segment(path: &str) -> String {
    let last = path
        .trim_end_matches(['\\', '/'])
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or("");
    if last.is_empty() { path } else { last }.to_string()
}

/// Cuts from the head: the tail is what tells two worktrees of a repo apart.
/// Cut at a separator, so a half-named folder never starts the path.
pub fn shorten_path(path: &str, max: usize) -> String {
    let chars: Vec<char> = path.chars().collect();
    if chars.len() <= max {
        return path.to_string();
    }
    let tail: String = chars[chars.len() - max..].iter().collect();
    match tail.find(['\\', '/']) {
        Some(at) => format!("…{}", &tail[at..]),
        None => format!("…{tail}"),
    }
}

/// What a checkout of a remote branch is called locally: the remote is a
/// coordinate, not part of the name — "origin/feat/x" is checked out as
/// "feat/x". Only the first segment goes.
pub fn local_name_of(remote: &str) -> String {
    match remote.find('/') {
        Some(at) => remote[at + 1..].to_string(),
        None => remote.to_string(),
    }
}

/// Locals and remotes can share a name, so the pickers key on both.
pub fn ref_key(branch: &Branch) -> String {
    format!("{}:{}", branch.kind, branch.name)
}

/// git's own wording for how a branch stands against its upstream, turned
/// into something a badge can hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Track {
    pub gone: bool,
    pub ahead: u32,
    pub behind: u32,
}

/// "[ahead 1, behind 2]", "[gone]"; `None` for a branch that is level.
pub fn parse_track(track: Option<&str>) -> Option<Track> {
    let track = track?;
    if track.to_lowercase().contains("gone") {
        return Some(Track {
            gone: true,
            ahead: 0,
            behind: 0,
        });
    }
    let number_after = |word: &str| {
        track
            .find(word)
            .map(|at| &track[at + word.len()..])
            .and_then(|rest| {
                let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
                digits.parse::<u32>().ok()
            })
            .unwrap_or(0)
    };
    let (ahead, behind) = (number_after("ahead "), number_after("behind "));
    (ahead > 0 || behind > 0).then_some(Track {
        gone: false,
        ahead,
        behind,
    })
}

impl Track {
    /// "1↑ 2↓": the badge's text for a branch that is not level.
    pub fn arrows(&self) -> String {
        [
            (self.ahead > 0).then(|| format!("{}↑", self.ahead)),
            (self.behind > 0).then(|| format!("{}↓", self.behind)),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ")
    }
}

/// How long ago a commit was, in the panel's own abbreviations (no locale
/// data to ship): seconds, minutes, hours, days, weeks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Age {
    Seconds(u64),
    Minutes(u64),
    Hours(u64),
    Days(u64),
    Weeks(u64),
}

/// `committed` and `now` are Unix seconds.
pub fn relative_age(now: i64, committed: i64) -> Age {
    let elapsed = (now - committed).max(0) as u64;
    if elapsed < 60 {
        return Age::Seconds(elapsed);
    }
    let minutes = elapsed / 60;
    if minutes < 60 {
        return Age::Minutes(minutes);
    }
    let hours = minutes / 60;
    if hours < 24 {
        return Age::Hours(hours);
    }
    let days = hours / 24;
    if days < 7 {
        return Age::Days(days);
    }
    Age::Weeks(days / 7)
}

impl Age {
    pub fn text(self, t: Translator) -> String {
        let (key, n) = match self {
            Age::Seconds(n) => (Key::WorktreesAgeSec, n),
            Age::Minutes(n) => (Key::WorktreesAgeMin, n),
            Age::Hours(n) => (Key::WorktreesAgeHour, n),
            Age::Days(n) => (Key::WorktreesAgeDay, n),
            Age::Weeks(n) => (Key::WorktreesAgeWeek, n),
        };
        t.t_with(key, &args!("n" => n)).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hangar_i18n::Locale;

    #[test]
    fn two_spellings_of_a_windows_folder_are_the_same_folder() {
        assert!(same_path("C:\\Dev\\App\\", "c:/dev/app"));
        assert!(same_path("/home/ada/app/", "/home/ada/app"));
        assert!(!same_path("/home/ada/app", "/home/ada/app2"));
    }

    #[test]
    fn a_folder_is_named_by_its_last_segment() {
        assert_eq!(
            last_segment("/home/ada/app-worktrees/feat-login"),
            "feat-login"
        );
        assert_eq!(last_segment("C:\\dev\\app\\"), "app");
        assert_eq!(last_segment("/"), "/");
        assert_eq!(last_segment("app"), "app");
    }

    #[test]
    fn a_long_path_loses_its_head_at_a_separator() {
        let path = "/home/ada/dev/clients/storefront-worktrees/feat-checkout-discount-rules";
        let short = shorten_path(path, 40);
        assert!(short.starts_with('…'));
        assert!(short.chars().count() <= 41);
        assert!(short.ends_with("feat-checkout-discount-rules"));
        // The cut lands on a separator, not in the middle of a folder name.
        assert!(short.starts_with("…/"));
        assert_eq!(shorten_path("/short/path", 54), "/short/path");
        // No separator in the tail at all: it is kept as it is.
        assert_eq!(
            shorten_path(&"x".repeat(60), 54),
            format!("…{}", "x".repeat(54))
        );
    }

    #[test]
    fn a_remote_branch_is_checked_out_without_its_remote() {
        assert_eq!(local_name_of("origin/feat/x"), "feat/x");
        assert_eq!(local_name_of("origin/main"), "main");
        assert_eq!(local_name_of("main"), "main");
    }

    #[test]
    fn a_ref_is_keyed_by_its_kind_and_name() {
        let branch = |kind: &str, name: &str| Branch {
            name: name.into(),
            kind: kind.into(),
            is_current: false,
            worktree_path: None,
            head: None,
            committer_date: None,
            author_name: None,
            subject: None,
            upstream: None,
            track: None,
        };
        assert_eq!(ref_key(&branch("local", "main")), "local:main");
        assert_ne!(
            ref_key(&branch("local", "main")),
            ref_key(&branch("remote", "main"))
        );
    }

    #[test]
    fn git_s_gap_to_the_upstream_becomes_a_badge() {
        assert_eq!(parse_track(None), None);
        assert_eq!(parse_track(Some("")), None);
        assert_eq!(
            parse_track(Some("[gone]")),
            Some(Track {
                gone: true,
                ahead: 0,
                behind: 0
            })
        );
        let both = parse_track(Some("[ahead 1, behind 2]")).unwrap();
        assert_eq!((both.ahead, both.behind), (1, 2));
        assert_eq!(both.arrows(), "1↑ 2↓");
        let ahead = parse_track(Some("[ahead 12]")).unwrap();
        assert_eq!(ahead.arrows(), "12↑");
        let behind = parse_track(Some("[behind 3]")).unwrap();
        assert_eq!(behind.arrows(), "3↓");
        // Level with its upstream: nothing to show.
        assert_eq!(parse_track(Some("[ahead 0]")), None);
    }

    #[test]
    fn a_commit_s_age_is_counted_in_the_largest_unit_that_fits() {
        let now = 1_000_000;
        assert_eq!(relative_age(now, now - 5), Age::Seconds(5));
        assert_eq!(relative_age(now, now - 90), Age::Minutes(1));
        assert_eq!(relative_age(now, now - 3 * 3600), Age::Hours(3));
        assert_eq!(relative_age(now, now - 3 * 86_400), Age::Days(3));
        assert_eq!(relative_age(now, now - 15 * 86_400), Age::Weeks(2));
        // A commit dated in the future (a clock set back) is "now".
        assert_eq!(relative_age(now, now + 500), Age::Seconds(0));
    }

    #[test]
    fn an_age_is_spoken_in_the_language_asked() {
        let en = Translator::new(Locale::En);
        assert!(Age::Days(3).text(en).contains('3'));
        assert!(
            Age::Weeks(2)
                .text(Translator::new(Locale::Fr))
                .contains('2')
        );
    }
}
