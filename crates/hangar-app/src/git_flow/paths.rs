//! Folder arithmetic for « From a Git repository » (the path half of
//! `lib/workspaceFromGit.ts`).
//!
//! Paths here are plain strings, not `PathBuf`s, on purpose: the agent that
//! asks may name a Windows folder (`E:\`) to a Hangar running on Linux, or the
//! other way round, and the answer must keep the asker's own spelling of the
//! separator. So nothing here touches the disk or the platform's rules; Rust's
//! git clone judges the real paths again (`hangar_core::git_clone`).

use crate::state::Settings;

use super::clone::default_clone_folder;

fn separator_of(path: &str) -> char {
    if path.contains('\\') { '\\' } else { '/' }
}

fn is_separator(ch: char) -> bool {
    ch == '\\' || ch == '/'
}

fn is_drive(text: &str) -> bool {
    let mut chars = text.chars();
    matches!((chars.next(), chars.next(), chars.next()), (Some(letter), Some(':'), None) if letter.is_ascii_alphabetic())
}

/// The folder holding `path`, or `None` when it is a root or has none.
pub fn parent_folder(path: &str) -> Option<String> {
    let trimmed = path.trim().trim_end_matches(is_separator);
    let cut = trimmed.rfind(is_separator)?;
    if cut == 0 {
        return None;
    }
    let parent = &trimmed[..cut];
    // `C:` alone is a drive's current folder, not its root.
    if is_drive(parent) {
        Some(format!("{parent}{}", separator_of(path)))
    } else {
        Some(parent.to_string())
    }
}

pub fn join_folder(parent: &str, name: &str) -> String {
    let separator = separator_of(parent);
    format!("{}{separator}{name}", parent.trim_end_matches(is_separator))
}

/// The same repository address: a trailing slash or `.git` does not tell two
/// repositories apart.
pub fn same_repo_url(a: &str, b: &str) -> bool {
    fn flat(url: &str) -> String {
        let url = url.trim().trim_end_matches('/');
        let cut = url
            .len()
            .checked_sub(4)
            .filter(|at| url.is_char_boundary(*at) && url[*at..].eq_ignore_ascii_case(".git"));
        match cut {
            Some(at) => url[..at].to_string(),
            None => url.to_string(),
        }
    }
    flat(a) == flat(b)
}

/// Same folder, the way Windows compares them: either separator, any case.
pub fn same_folder(a: &str, b: &str) -> bool {
    fn flat(path: &str) -> String {
        path.strip_prefix("\\\\?\\")
            .unwrap_or(path)
            .trim_end_matches(is_separator)
            .replace('\\', "/")
            .to_lowercase()
    }
    flat(a) == flat(b)
}

/// The path split at its separators, a run of them counting as one, as
/// `path.split(/[\\/]+/)` does: a leading separator gives an empty first part
/// and a trailing one an empty last part.
fn parts_of(path: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut in_run = false;
    for (at, ch) in path.char_indices() {
        if is_separator(ch) {
            if !in_run {
                parts.push(&path[start..at]);
                in_run = true;
            }
            start = at + ch.len_utf8();
        } else {
            in_run = false;
        }
    }
    parts.push(&path[start..]);
    parts
}

/// Why `parent` can never receive a clone, seen before anyone is asked: not
/// absolute, `..` in it, or an operating system folder. Rust judges again, with
/// the real paths of the machine, and also refuses Hangar's own data.
///
/// The sentences are for the model that asked (they say what to call again
/// with), so they stay in English whatever the person's language is.
pub fn parent_problem(parent: &str) -> Option<String> {
    let path = parent.trim();
    let absolute = {
        let mut chars = path.chars();
        let first = chars.next();
        let second = chars.next();
        let third = chars.next();
        matches!(first, Some('/'))
            || path.starts_with("\\\\")
            || matches!((first, second, third), (Some(letter), Some(':'), Some(sep))
                if letter.is_ascii_alphabetic() && is_separator(sep))
    };
    if !absolute {
        return Some(format!(
            "parent must be an absolute folder, such as E:\\ or /mnt/data (got \"{path}\"). Call again with an absolute parent; find the drive yourself."
        ));
    }
    let parts = parts_of(path);
    if parts.contains(&"..") {
        return Some(
            "parent cannot hold `..`. Call again with the folder written out in full.".into(),
        );
    }
    let flat: Vec<String> = parts.iter().map(|part| part.to_lowercase()).collect();
    let second = flat.get(1).map(String::as_str).unwrap_or("");
    let windows = flat.first().is_some_and(|first| is_drive(first)) || path.starts_with("\\\\");
    let system = if windows {
        second == "windows"
            || second == "program files"
            || second == "program files (x86)"
            || flat.iter().any(|part| part == "system volume information")
    } else {
        [
            "bin", "sbin", "usr", "etc", "boot", "dev", "proc", "sys", "lib", "lib64", "system",
            "library",
        ]
        .contains(&second)
    };
    if system {
        return Some(format!(
            "{path} is a folder of the operating system: nothing is cloned there. Call again with another parent, such as a data drive or a projects folder."
        ));
    }
    None
}

/// The « Projects folder » setting, as a folder: absent when blank.
pub fn projects_folder_of(settings: &Settings) -> Option<String> {
    settings
        .projects_folder
        .as_deref()
        .map(str::trim)
        .filter(|folder| !folder.is_empty())
        .map(str::to_string)
}

/// The folder repositories are cloned into: the one typed in the dialog, else
/// the « Projects folder » setting, else the parent of the current workspace's
/// folder. `None` when none of them gives one.
pub fn resolve_parent(
    edited: Option<&str>,
    projects_folder: Option<&str>,
    asker_cwd: Option<&str>,
) -> Option<String> {
    let typed = edited.map(str::trim).unwrap_or("");
    if !typed.is_empty() {
        return Some(typed.to_string());
    }
    let configured = projects_folder.map(str::trim).unwrap_or("");
    if !configured.is_empty() {
        return Some(configured.to_string());
    }
    asker_cwd
        .filter(|cwd| !cwd.is_empty())
        .and_then(parent_folder)
}

/// Where a clone of `repo_name` would land in `parent`; `None` until both are
/// known.
pub fn clone_destination(parent: Option<&str>, repo_name: Option<&str>) -> Option<String> {
    let folder = parent.map(str::trim).unwrap_or("");
    let name = repo_name.filter(|name| !name.is_empty())?;
    if folder.is_empty() {
        return None;
    }
    Some(join_folder(folder, default_clone_folder(name)))
}

/// The last part of a folder's path: the only part of it the activity log
/// keeps.
pub fn folder_name(path: &str) -> String {
    path.trim_end_matches(is_separator)
        .rsplit(is_separator)
        .next()
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_arithmetic_works_for_both_separators() {
        assert_eq!(parent_folder("C:\\dev\\hangar").as_deref(), Some("C:\\dev"));
        assert_eq!(
            parent_folder("C:\\dev\\hangar\\").as_deref(),
            Some("C:\\dev")
        );
        assert_eq!(parent_folder("C:\\dev").as_deref(), Some("C:\\"));
        assert_eq!(parent_folder("C:\\"), None);
        assert_eq!(parent_folder("/home/me/x").as_deref(), Some("/home/me"));
        assert_eq!(parent_folder("/x"), None);
        assert_eq!(join_folder("C:\\dev", "shop"), "C:\\dev\\shop");
        assert_eq!(join_folder("C:\\", "shop"), "C:\\shop");
        assert_eq!(join_folder("/home/me/", "shop"), "/home/me/shop");
        assert!(same_folder("C:\\Dev\\Shop\\", "c:/dev/shop"));
        assert!(!same_folder("C:\\dev\\shop", "C:\\dev\\shop2"));
    }

    #[test]
    fn a_trailing_slash_or_dot_git_does_not_tell_two_repositories_apart() {
        assert!(same_repo_url(
            "https://github.com/x/a.git",
            "https://github.com/x/a"
        ));
        assert!(same_repo_url(
            "https://github.com/x/a.GIT/",
            " https://github.com/x/a "
        ));
        // The comparison is exact otherwise: a name's case tells two apart.
        assert!(!same_repo_url(
            "https://github.com/x/a/",
            "https://github.com/x/A.git"
        ));
        assert!(!same_repo_url(
            "https://github.com/good/theirs.git",
            "https://github.com/evil/theirs.git"
        ));
    }

    #[test]
    fn the_verbatim_prefix_of_a_windows_path_is_ignored() {
        assert!(same_folder("\\\\?\\C:\\dev\\shop", "c:/dev/shop/"));
    }

    #[test]
    fn a_parent_that_can_never_take_a_clone_is_told_why() {
        for parent in [
            "projects",
            "..\\projects",
            "E:\\a\\..\\b",
            "/home/me/../x",
            "C:\\Windows\\System32",
            "C:\\Program Files (x86)\\x",
            "E:\\System Volume Information",
            "/etc/cron.d",
            "/usr",
        ] {
            let problem = parent_problem(parent);
            assert!(problem.is_some(), "{parent}");
            assert!(
                problem.is_some_and(|text| text.contains("Call again")),
                "{parent}"
            );
        }
        for parent in ["E:\\", "/mnt/ssd/code", "D:\\Projects", "/home/me/code"] {
            assert_eq!(parent_problem(parent), None, "{parent}");
        }
    }

    #[test]
    fn the_projects_folder_is_read_only_when_the_settings_have_one() {
        let mut settings = Settings::default();
        assert_eq!(projects_folder_of(&settings), None);
        settings.projects_folder = Some("  ".into());
        assert_eq!(projects_folder_of(&settings), None);
        settings.projects_folder = Some(" D:\\Projects ".into());
        assert_eq!(
            projects_folder_of(&settings).as_deref(),
            Some("D:\\Projects")
        );
    }

    #[test]
    fn a_projects_folder_that_is_not_text_reads_as_none() {
        // `state.json` is hand-edited: the lenient reader drops a value of
        // the wrong type, so the setting reads as unset (TS: `typeof` guard).
        let state = crate::state::read_state(serde_json::json!({
            "settings": { "projectsFolder": 3 }
        }));
        assert_eq!(projects_folder_of(&state.settings), None);
    }

    #[test]
    fn clones_go_to_the_typed_folder_else_the_setting_else_beside_the_workspace() {
        assert_eq!(
            resolve_parent(
                Some(" E:\\Repos "),
                Some("D:\\Projects"),
                Some("C:\\Work\\app")
            )
            .as_deref(),
            Some("E:\\Repos")
        );
        assert_eq!(
            resolve_parent(Some(""), Some("D:\\Projects"), Some("C:\\Work\\app")).as_deref(),
            Some("D:\\Projects")
        );
        assert_eq!(
            resolve_parent(None, Some("  "), Some("C:\\Work\\app")).as_deref(),
            Some("C:\\Work")
        );
        assert_eq!(
            resolve_parent(None, None, Some("/home/me/code/app/")).as_deref(),
            Some("/home/me/code")
        );
        assert_eq!(resolve_parent(None, None, None), None);
        assert_eq!(resolve_parent(None, None, Some("C:\\")), None);
    }

    #[test]
    fn the_destination_is_named_from_the_folder_and_the_repository() {
        assert_eq!(
            clone_destination(Some("D:\\Projects"), Some("shop")).as_deref(),
            Some("D:\\Projects\\shop")
        );
        assert_eq!(
            clone_destination(Some("/home/me/code/"), Some("shop")).as_deref(),
            Some("/home/me/code/shop")
        );
        assert_eq!(clone_destination(Some("  "), Some("shop")), None);
        assert_eq!(clone_destination(Some("D:\\Projects"), None), None);
    }

    #[test]
    fn a_folder_keeps_only_its_last_part_for_the_log() {
        assert_eq!(folder_name("/home/me/code/theirs/"), "theirs");
        assert_eq!(folder_name("C:\\a\\b"), "b");
    }
}
