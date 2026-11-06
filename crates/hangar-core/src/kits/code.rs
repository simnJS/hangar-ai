//! The files a kit's hooks, servers and commands run, by their content.
//!
//! A grant hashes what a kit writes it runs: `node ${CLAUDE_PLUGIN_ROOT}/guard.js`,
//! `["node", "{kitDir}/x.js"]`. The words alone would let an update change
//! `guard.js` under a grant given for another one, so the grant also covers the
//! content of every file such a line names inside the kit — a folder named is
//! every file in it — and of every file in the plugin's `bin/`, which is on the
//! PATH of Claude's shell. A file named and not there is recorded as missing, so
//! one that appears later is a change as well.
//!
//! What a named script loads in turn is covered when it sits in a folder the line
//! names or in `bin/`; a kit that wants all its code covered names its folder.

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use sha2::{Digest, Sha256};

use super::manifest;

/// How a plugin's hooks, servers and monitors name its folder.
pub const PLUGIN_ROOT: [&str; 2] = ["${CLAUDE_PLUGIN_ROOT}", "$CLAUDE_PLUGIN_ROOT"];
/// How a kit's commands name its folder.
pub const KIT_DIR: [&str; 1] = ["{kitDir}"];

/// Deep enough for any `bin/` or scripts folder, short of walking a stray
/// `node_modules`.
const MAX_DEPTH: usize = 6;
/// Past this many files under one folder, the rest is not read: the count of
/// them stands in its place, so adding one is still a change.
const MAX_FILES: usize = 2000;

/// Where a path named after a marker ends: the shell's and JSON's separators.
fn ends_path(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '"' | '\'' | '`' | ';' | '|' | '&' | '(' | ')' | '<' | '>' | ',' | '}' | ']'
        )
}

/// The paths `text` names after one of `markers`, relative to the folder the
/// marker stands for. The folder itself, named alone, is not one of them.
pub fn named<'a>(text: &'a str, markers: &[&str]) -> Vec<&'a str> {
    let mut found = Vec::new();
    for marker in markers {
        let mut rest = text;
        while let Some(at) = rest.find(marker) {
            let after = &rest[at + marker.len()..];
            rest = after;
            // `$CLAUDE_PLUGIN_ROOT` is also the start of `${CLAUDE_PLUGIN_ROOT}`'s
            // neighbour `$CLAUDE_PLUGIN_ROOTS`: a name goes on with a letter.
            if marker.starts_with('$')
                && !marker.ends_with('}')
                && after.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
            {
                continue;
            }
            // In quotes, the path is all the quotes hold; bare, it ends where
            // the shell would end a word.
            let quote = text[..text.len() - after.len() - marker.len()]
                .chars()
                .next_back()
                .filter(|c| matches!(c, '"' | '\''));
            let end = match quote {
                Some(quote) => after.find(quote),
                None => after.find(ends_path),
            }
            .unwrap_or(after.len());
            let path = after[..end].trim_start_matches(['/', '\\']);
            if !path.is_empty() {
                found.push(path);
            }
        }
    }
    found
}

/// `(path in the folder, what it holds)` of every file `texts` name after
/// `markers` in `root`, and of every file in `root/<always>`, sorted and each
/// once. Empty when nothing is named and those folders are not there.
pub fn files<'a>(
    root: &Path,
    texts: impl IntoIterator<Item = &'a str>,
    markers: &[&str],
    always: &[&str],
) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let Ok(root) = dunce::canonicalize(root) else {
        return found;
    };
    let mut wanted: Vec<String> = texts
        .into_iter()
        .flat_map(|text| named(text, markers))
        .map(str::to_string)
        .collect();
    wanted.extend(
        always
            .iter()
            .filter(|dir| root.join(dir).is_dir())
            .map(|dir| dir.to_string()),
    );
    for raw in wanted {
        let Ok(relative) = manifest::normalize(&raw) else {
            found.push((raw, "outside".into()));
            continue;
        };
        if relative == "." {
            continue;
        }
        match manifest::inside(&root, &relative) {
            Ok(path) => walk(&root, &path, 0, &mut found),
            Err(_) => found.push((relative, "missing".into())),
        }
    }
    found.sort();
    found.dedup();
    found
}

fn walk(root: &Path, path: &Path, depth: usize, found: &mut Vec<(String, String)>) {
    let name = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    let Ok(meta) = fs::symlink_metadata(path) else {
        found.push((name, "missing".into()));
        return;
    };
    if meta.file_type().is_symlink() {
        // A link: what it points at, not followed.
        let target = fs::read_link(path).unwrap_or_default();
        found.push((name, format!("link:{}", target.to_string_lossy())));
    } else if meta.is_file() {
        found.push((name, hash_file(path, &meta)));
    } else if meta.is_dir() {
        if depth >= MAX_DEPTH {
            found.push((name, "deeper".into()));
            return;
        }
        let mut children: Vec<PathBuf> = match fs::read_dir(path) {
            Ok(entries) => entries.flatten().map(|entry| entry.path()).collect(),
            Err(err) => {
                found.push((name, err.to_string()));
                return;
            }
        };
        children.sort();
        if children.len() > MAX_FILES {
            found.push((name.clone(), format!("{} entries", children.len())));
            children.truncate(MAX_FILES);
        }
        for child in children {
            walk(root, &child, depth + 1, found);
        }
    }
}

/// What a file looked like when its hash was taken, and when that was.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: SystemTime,
    /// Unix: the inode and the status change time, which a rewrite through a
    /// new file (git checkout, a rename) changes even when size and
    /// modification time come out the same.
    extra: (u64, i64, i64),
}

impl Stamp {
    fn of(meta: &fs::Metadata) -> Option<Stamp> {
        let modified = meta.modified().ok()?;
        #[cfg(unix)]
        let extra = {
            use std::os::unix::fs::MetadataExt;
            (meta.ino(), meta.ctime(), meta.ctime_nsec())
        };
        #[cfg(not(unix))]
        let extra = (0, 0, 0);
        Some(Stamp {
            len: meta.len(),
            modified,
            extra,
        })
    }
}

/// A stamp is trusted only when the file had not been touched for this long
/// when its hash was taken: a file written again within the clock's
/// granularity, with the same size, would otherwise keep the old hash (git's
/// "racily clean" files). A grant must never miss a change.
const SETTLED: std::time::Duration = std::time::Duration::from_secs(2);

type Seen = HashMap<PathBuf, (Stamp, SystemTime, String)>;

/// The SHA-256 of a file. Remembered by its size and modification time, as the
/// grants are worked out for every listing of the kits and a `bin/` can hold
/// large programs.
fn hash_file(path: &Path, meta: &fs::Metadata) -> String {
    static SEEN: OnceLock<Mutex<Seen>> = OnceLock::new();
    let seen = SEEN.get_or_init(Mutex::default);
    let stamp = Stamp::of(meta);
    if let (Some(stamp), Some((was, taken, hash))) = (
        stamp,
        seen.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(path),
    ) {
        let settled = taken
            .duration_since(was.modified)
            .is_ok_and(|age| age >= SETTLED);
        if *was == stamp && settled {
            return hash.clone();
        }
    }
    let taken = SystemTime::now();
    let hash: String = match fs::File::open(path) {
        Ok(mut file) => {
            let mut hasher = Sha256::new();
            let mut chunk = vec![0u8; 64 * 1024];
            loop {
                match file.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(read) => hasher.update(&chunk[..read]),
                    Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(err) => return format!("unreadable: {err}"),
                }
            }
            hasher
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect()
        }
        Err(err) => return format!("unreadable: {err}"),
    };
    if let Some(stamp) = stamp {
        seen.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(path.to_path_buf(), (stamp, taken, hash.clone()));
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kits::testing::Folder;

    #[test]
    fn paths_are_read_after_the_markers_in_every_quoting() {
        fn found(text: &str) -> Vec<&str> {
            named(text, &PLUGIN_ROOT)
        }
        assert_eq!(
            found("node ${CLAUDE_PLUGIN_ROOT}/guard.js --x"),
            ["guard.js"]
        );
        assert_eq!(
            found("node \"${CLAUDE_PLUGIN_ROOT}/a b/x.js\" --y"),
            ["a b/x.js"]
        );
        assert_eq!(found("node ${CLAUDE_PLUGIN_ROOT}/a b/x.js"), ["a"]);
        assert_eq!(
            found("sh '$CLAUDE_PLUGIN_ROOT/scripts/run.sh'; $CLAUDE_PLUGIN_ROOT/bin/x|y"),
            ["scripts/run.sh", "bin/x"]
        );
        assert_eq!(
            found(r"${CLAUDE_PLUGIN_ROOT}\srv\main.py"),
            [r"srv\main.py"]
        );
        assert!(found("cd ${CLAUDE_PLUGIN_ROOT} && $CLAUDE_PLUGIN_ROOTS/x").is_empty());
        assert_eq!(
            named("{kitDir}/tools/a.js --root={kitDir}", &KIT_DIR),
            ["tools/a.js"]
        );
    }

    #[test]
    fn the_content_of_what_is_named_and_of_bin_is_what_counts() {
        let kit = Folder::new("code-files");
        kit.file("guard.js", "one")
            .file("bin/tool", "#!/bin/sh")
            .file("scripts/a.sh", "a")
            .file("scripts/lib/b.sh", "b")
            .file("README.md", "not run");
        let texts = [
            "node ${CLAUDE_PLUGIN_ROOT}/guard.js",
            "sh ${CLAUDE_PLUGIN_ROOT}/scripts",
            "${CLAUDE_PLUGIN_ROOT}/gone.js ${CLAUDE_PLUGIN_ROOT}/../out",
        ];
        let read = || files(&kit.root, texts, &PLUGIN_ROOT, &["bin"]);
        let before = read();
        let names: Vec<&str> = before.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            [
                "../out",
                "bin/tool",
                "gone.js",
                "guard.js",
                "scripts/a.sh",
                "scripts/lib/b.sh"
            ]
        );
        assert!(before.contains(&("gone.js".into(), "missing".into())));
        assert!(before.contains(&("../out".into(), "outside".into())));

        kit.file("README.md", "edited");
        assert_eq!(read(), before, "a file nothing runs changes nothing");
        for (path, body) in [
            ("guard.js", "two"),
            ("bin/tool", "#!/bin/bash"),
            ("scripts/lib/b.sh", "c"),
            ("gone.js", "here now"),
        ] {
            let was = read();
            kit.file(path, body);
            assert_ne!(read(), was, "{path}");
        }
        assert!(files(&kit.root, ["node x.js"], &PLUGIN_ROOT, &["bin-missing"]).is_empty());
    }
}
