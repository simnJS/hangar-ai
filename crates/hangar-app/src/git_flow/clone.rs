//! What the dialog and the flow need around a clone (`lib/gitClone.ts`).
//!
//! The commands themselves are `hangar_core::git_clone`, called directly:
//! its types are the ones the TS declared by hand (`RepoRef`, `Ownership`,
//! `CloneResult`…), so they are re-exported here rather than described twice.
//! What stays in the app is the small logic around them: how an address is
//! cleaned, what a clone job is called, and how git's phases become one bar.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub use hangar_core::git_clone::{
    CloneOutcome, CloneProgress, DiscardOutcome, Ownership, ParseResult, RepoRef,
};

/// What the person typed or pasted, ready for `git_repo_parse`: the ends
/// trimmed (a paste brings a newline). Rust refuses any whitespace left
/// inside.
pub fn clean_repo_input(raw: &str) -> &str {
    raw.trim()
}

/// `git_repo_parse` of the address as typed.
pub fn parse_repo_input(input: &str) -> ParseResult {
    hangar_core::git_clone::git_repo_parse(clean_repo_input(input).to_string())
}

/// The folder a clone lands in by default: the repository's name.
pub fn default_clone_folder(repo_name: &str) -> &str {
    repo_name
}

/// A job id for one clone, to follow its progress and cancel it.
pub fn new_clone_job_id() -> String {
    // No random-number crate is needed for an id that only has to differ from
    // the ones before it in this run (and from another run's, which share
    // nothing): the clock, a counter, and the entropy `RandomState` seeds
    // each hasher with.
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    use std::hash::{BuildHasher, Hasher};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(nanos);
    hasher.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    format!("clone-{nanos:x}-{:08x}", hasher.finish() & 0xffff_ffff)
}

/// `Receiving objects 45 %`, or just the phase when git gave no percentage.
/// The space before the percent sign is a plain one, as the log line it
/// stands for was.
pub fn progress_label(phase: &str, percent: Option<u8>) -> String {
    match percent {
        None => phase.to_string(),
        Some(percent) => format!("{phase} {percent} %"),
    }
}

/// The share of the whole clone, 0 to 100, from the phase git is in. Git
/// counts each phase from 0 to 100 on its own; receiving is by far the
/// longest, so it takes the first 80 %, resolving deltas the rest and the
/// others nothing. `None` when the phase is unknown.
pub fn overall_percent(phase: &str, percent: Option<u8>) -> Option<u8> {
    let percent = f64::from(percent?.min(100));
    let phase = phase.to_lowercase();
    if phase.starts_with("receiving") {
        Some((percent * 0.8).round() as u8)
    } else if phase.starts_with("resolving") {
        Some((80.0 + percent * 0.2).round() as u8)
    } else if phase.starts_with("updating files") {
        Some(100)
    } else {
        None
    }
}

/// Whether `progress` belongs to the clone `job_id` (the bus carries every
/// clone's, and a dialog follows only its own).
pub fn is_progress_of(progress: &CloneProgress, job_id: &str) -> bool {
    progress.job_id == job_id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_repo_input_trims_the_ends_and_leaves_the_inside_alone() {
        assert_eq!(clean_repo_input("  octo/hello\n"), "octo/hello");
        assert_eq!(clean_repo_input("octo/hel lo"), "octo/hel lo");
    }

    #[test]
    fn the_default_folder_is_the_repositorys_name() {
        assert_eq!(default_clone_folder("hello"), "hello");
    }

    #[test]
    fn a_job_id_is_unique_and_prefixed() {
        let a = new_clone_job_id();
        assert!(a.starts_with("clone-") && a.len() > "clone-".len());
        assert_ne!(new_clone_job_id(), a);
    }

    #[test]
    fn the_label_shows_the_percentage_when_there_is_one() {
        assert_eq!(
            progress_label("Receiving objects", Some(45)),
            "Receiving objects 45 %"
        );
        assert_eq!(
            progress_label("Receiving objects", None),
            "Receiving objects"
        );
    }

    #[test]
    fn the_phases_map_onto_one_bar() {
        assert_eq!(overall_percent("Receiving objects", Some(0)), Some(0));
        assert_eq!(overall_percent("Receiving objects", Some(50)), Some(40));
        assert_eq!(overall_percent("Receiving objects", Some(100)), Some(80));
        assert_eq!(overall_percent("Resolving deltas", Some(50)), Some(90));
        assert_eq!(overall_percent("Resolving deltas", Some(100)), Some(100));
        assert_eq!(overall_percent("Updating files", Some(10)), Some(100));
    }

    #[test]
    fn an_unknown_phase_or_no_percentage_gives_nothing_and_a_percentage_is_clamped() {
        assert_eq!(overall_percent("Counting objects", Some(100)), None);
        assert_eq!(overall_percent("Receiving objects", None), None);
        assert_eq!(overall_percent("Receiving objects", Some(250)), Some(80));
    }

    #[test]
    fn the_address_is_cleaned_before_it_is_read() {
        assert!(matches!(
            parse_repo_input("  octo/hello \n"),
            ParseResult::Ok { repo } if repo.owner == "octo" && repo.name == "hello"
        ));
        assert!(matches!(
            parse_repo_input("octo/hel lo"),
            ParseResult::Refused { .. }
        ));
    }

    #[test]
    fn only_the_progress_of_its_own_job_is_handed_over() {
        let mine = CloneProgress {
            job_id: "job-1".into(),
            phase: "Receiving objects".into(),
            percent: Some(3),
        };
        let other = CloneProgress {
            job_id: "job-2".into(),
            phase: "x".into(),
            percent: Some(1),
        };
        assert!(is_progress_of(&mine, "job-1"));
        assert!(!is_progress_of(&other, "job-1"));
    }
}
