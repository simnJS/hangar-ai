//! `GET /node/fs?path=`: a folder's entries, for the desktop to pick the
//! folder of a workspace on the node.
//!
//! No path is the home directory; `~` and `~/…` are in it; any other relative
//! path is taken from the home directory and must stay inside it. An absolute
//! path is the desktop asking for that folder explicitly, and is listed
//! wherever it is.
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
}

#[derive(Debug, Serialize)]
pub struct Listing {
    /// The folder listed, resolved.
    pub path: String,
    /// Its parent, absent at `/`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Folders first, then by name.
    pub entries: Vec<Entry>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A relative path that leaves the home directory.
    OutsideHome,
    /// Not there, not a folder, or not readable: the reason.
    Unreadable(String),
}

/// The folder `asked` names, before it is resolved.
fn requested(home: &Path, asked: Option<&str>) -> (PathBuf, bool) {
    match asked.map(str::trim).filter(|asked| !asked.is_empty()) {
        None | Some("~") => (home.to_path_buf(), true),
        Some(asked) => match asked.strip_prefix("~/") {
            Some(rest) => (home.join(rest), true),
            None if Path::new(asked).is_absolute() => (PathBuf::from(asked), false),
            None => (home.join(asked), true),
        },
    }
}

pub fn list(home: &Path, asked: Option<&str>) -> Result<Listing, Refusal> {
    let (path, in_home) = requested(home, asked);
    let path = path
        .canonicalize()
        .map_err(|err| Refusal::Unreadable(err.to_string()))?;
    if in_home {
        let home = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
        if !path.starts_with(&home) {
            return Err(Refusal::OutsideHome);
        }
    }
    let read = std::fs::read_dir(&path).map_err(|err| Refusal::Unreadable(err.to_string()))?;
    let mut entries: Vec<Entry> = read
        .flatten()
        .map(|entry| Entry {
            name: entry.file_name().to_string_lossy().into_owned(),
            // Through a symbolic link: a link to a folder opens as one.
            dir: entry.path().is_dir(),
        })
        .collect();
    entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.cmp(&b.name)));
    Ok(Listing {
        parent: path.parent().map(|p| p.to_string_lossy().into_owned()),
        path: path.to_string_lossy().into_owned(),
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_folders_first_and_keeps_relative_paths_home() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        std::fs::create_dir_all(home.join("b-dir")).unwrap();
        std::fs::create_dir_all(home.join("z-dir")).unwrap();
        std::fs::write(home.join("a-file"), "").unwrap();
        std::fs::create_dir_all(root.path().join("elsewhere")).unwrap();

        let listing = list(&home, None).unwrap();
        let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["b-dir", "z-dir", "a-file"]);
        assert!(listing.entries[0].dir && !listing.entries[2].dir);
        assert_eq!(list(&home, Some("~")).unwrap().path, listing.path);
        assert!(list(&home, Some("~/b-dir")).unwrap().entries.is_empty());
        assert!(list(&home, Some("b-dir")).is_ok());

        assert_eq!(
            list(&home, Some("../elsewhere")).unwrap_err(),
            Refusal::OutsideHome
        );
        assert_eq!(
            list(&home, Some("~/../elsewhere")).unwrap_err(),
            Refusal::OutsideHome
        );
        let explicit = root.path().join("elsewhere");
        assert!(list(&home, Some(explicit.to_str().unwrap())).is_ok());
        assert!(matches!(
            list(&home, Some("missing")),
            Err(Refusal::Unreadable(_))
        ));
        assert!(matches!(
            list(&home, Some("a-file")),
            Err(Refusal::Unreadable(_))
        ));
    }
}
