//! Paths as agents give them: relative to their pane's folder, in Windows or
//! POSIX spelling, sometimes in core's canonical `\\?\` form. Folded and
//! compared here without touching the disk, so that a check on a path cannot
//! be walked out of with `..`.

use super::types::normalize;

/// Whether `path` starts at a root: a drive (`C:\`, `C:/`) or a separator.
fn is_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    let drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    drive || matches!(bytes.first(), Some(b'\\' | b'/'))
}

fn is_separator(c: char) -> bool {
    c == '\\' || c == '/'
}

/// `path` read from `base`, with `.` and `..` folded away (`resolvePath`).
/// A Windows path keeps backslashes; a UNC share keeps its two.
pub fn resolve_path(base: &str, path: &str) -> String {
    let full = if is_absolute(path) {
        path.to_string()
    } else {
        format!("{base}/{path}")
    };
    let mut chars = full.chars();
    let unc = matches!(
        (chars.next(), chars.next(), chars.next()),
        (Some(a), Some(b), Some(c)) if is_separator(a) && is_separator(b) && !is_separator(c)
    );
    let mut pieces = full.split(is_separator).collect::<Vec<_>>();
    // `split` gives an empty piece between two separators, where the TS's
    // split on runs of them gave none; the head is kept as it is.
    let head = pieces.remove(0);
    let head_is_drive =
        head.len() == 2 && head.as_bytes()[0].is_ascii_alphabetic() && head.as_bytes()[1] == b':';
    let separator = if unc || head_is_drive { "\\" } else { "/" };
    let mut parts: Vec<&str> = Vec::new();
    for part in pieces {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    let joined = std::iter::once(head)
        .chain(parts)
        .collect::<Vec<_>>()
        .join(separator);
    if unc {
        format!("{separator}{joined}")
    } else {
        joined
    }
}

/// A folder an agent names for a new pane (`resolveFolder`): an absolute one
/// as it is, a relative one under `root` with `root`'s separator.
pub fn resolve_folder(root: &str, path: &str) -> String {
    if is_absolute(path) {
        return path.to_string();
    }
    let separator = if root.contains('\\') { "\\" } else { "/" };
    let relative = path
        .strip_prefix("./")
        .or_else(|| path.strip_prefix(".\\"))
        .unwrap_or(path);
    let mut out = String::new();
    let mut in_run = false;
    for c in relative.chars() {
        if is_separator(c) {
            if !in_run {
                out.push_str(separator);
            }
            in_run = true;
        } else {
            out.push(c);
            in_run = false;
        }
    }
    format!("{}{separator}{out}", root.trim_end_matches(is_separator))
}

/// For comparing: core's canonical paths carry Windows' `\\?\` prefix
/// (`comparable`).
pub fn comparable(path: &str) -> String {
    let lowered = path.to_ascii_lowercase();
    let stripped = if lowered.starts_with("\\\\?\\unc\\") {
        format!("\\\\{}", &path[8..])
    } else if lowered.starts_with("\\\\?\\") {
        path[4..].to_string()
    } else {
        path.to_string()
    };
    normalize(&stripped)
}

/// Whether `path` is one of `roots` or inside one.
pub fn inside(path: &str, roots: &[String]) -> bool {
    let target = comparable(path);
    roots.iter().any(|root| {
        let base = comparable(root);
        target == base || target.starts_with(&format!("{base}/"))
    })
}

/// The file name of a path, whatever its separators (`basename`).
pub fn basename(path: &str) -> String {
    let trimmed = path.trim_end_matches(is_separator);
    match trimmed.rsplit(is_separator).next() {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => path.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // control.test.ts, "model paths".
    #[test]
    fn reads_a_relative_path_from_a_base_and_folds_dots() {
        assert_eq!(
            resolve_path("C:\\dev\\hangar", "assets/chair.glb"),
            "C:\\dev\\hangar\\assets\\chair.glb"
        );
        assert_eq!(
            resolve_path("C:\\dev\\hangar", ".\\a\\..\\b.glb"),
            "C:\\dev\\hangar\\b.glb"
        );
        assert_eq!(
            resolve_path("C:\\dev\\hangar", "..\\..\\..\\..\\x.glb"),
            "C:\\x.glb"
        );
        assert_eq!(
            resolve_path("/home/me/game", "../art/ship.fbx"),
            "/home/me/art/ship.fbx"
        );
    }

    #[test]
    fn takes_an_absolute_path_as_it_is_a_unc_share_included() {
        assert_eq!(
            resolve_path("C:\\dev\\hangar", "D:/art/ship.glb"),
            "D:\\art\\ship.glb"
        );
        assert_eq!(
            resolve_path("C:\\dev\\hangar", "\\\\nas\\art\\ship.glb"),
            "\\\\nas\\art\\ship.glb"
        );
        assert_eq!(
            resolve_path("/home/me", "/srv/art/ship.glb"),
            "/srv/art/ship.glb"
        );
    }

    #[test]
    fn a_new_panes_folder_is_read_under_the_workspace() {
        assert_eq!(
            resolve_folder("C:\\dev\\shop", "packages/api"),
            "C:\\dev\\shop\\packages\\api"
        );
        assert_eq!(
            resolve_folder("/srv/app/", "./web//src"),
            "/srv/app/web/src"
        );
        assert_eq!(resolve_folder("/srv/app", "/tmp"), "/tmp");
    }

    #[test]
    fn canonical_forms_compare_as_the_everyday_ones() {
        assert_eq!(comparable("\\\\?\\C:\\Dev\\A"), "c:/dev/a");
        assert_eq!(comparable("\\\\?\\UNC\\nas\\share"), "//nas/share");
        assert!(inside("C:\\dev\\hangar\\a.glb", &["c:/DEV/hangar/".into()]));
        assert!(!inside(
            "C:\\dev\\hangar-wt\\a.glb",
            &["C:\\dev\\hangar".into()]
        ));
        assert_eq!(basename("C:\\dev\\a\\m.glb"), "m.glb");
        assert_eq!(basename("/srv/x/"), "x");
    }
}
