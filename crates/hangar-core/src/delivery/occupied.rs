//! Who stands in a folder: the live processes whose current directory is in
//! it. A pane of any Hangar, the person's own terminal, an agent working
//! elsewhere but started there: all of them keep their start folder as
//! their current directory, so this is what tells a worktree in use from an
//! orphan when the panes that use it belong to another Hangar (another data
//! root, a debug instance a worker started) that this one cannot see.
//!
//! Read from `/proc/<pid>/cwd` on Linux, for the processes this user may
//! look at. Elsewhere there is no cheap way to ask, and `None` says so.

use std::path::{Path, PathBuf};

/// The processes, other than this one, whose current directory is `folder`
/// or a folder in it. `None`: the system does not say (not Linux, or `/proc`
/// could not be read). A folder that does not exist has no one in it.
pub fn occupants(folder: &Path) -> Option<Vec<u32>> {
    let dirs = current_dirs()?;
    Some(standing_in(&dirs, folder))
}

/// The processes of `dirs` (as `current_dirs` reads them), other than this
/// one, standing in `folder` or a folder in it.
pub fn standing_in(dirs: &[(u32, PathBuf)], folder: &Path) -> Vec<u32> {
    let Ok(folder) = dunce::canonicalize(folder) else {
        return Vec::new();
    };
    let mine = std::process::id();
    dirs.iter()
        .filter(|(pid, dir)| *pid != mine && dir.starts_with(&folder))
        .map(|(pid, _)| *pid)
        .collect()
}

/// Every folder a live process of this user stands in, as `(pid, folder)`.
/// A folder deleted under a process reads `<path> (deleted)`: it is kept
/// under its path, which no longer exists and so matches nothing.
#[cfg(target_os = "linux")]
pub fn current_dirs() -> Option<Vec<(u32, PathBuf)>> {
    let entries = std::fs::read_dir("/proc").ok()?;
    let mut dirs = Vec::new();
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        // Another user's process, or one that just ended: not readable.
        if let Ok(dir) = std::fs::read_link(entry.path().join("cwd")) {
            dirs.push((pid, dir));
        }
    }
    Some(dirs)
}

#[cfg(not(target_os = "linux"))]
pub fn current_dirs() -> Option<Vec<(u32, PathBuf)>> {
    None
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    #[test]
    fn a_process_standing_in_a_folder_or_below_it_occupies_it() {
        let root = crate::testdir::scratch("occupied");
        let folder = root.join("tree");
        let below = folder.join("src");
        std::fs::create_dir_all(&below).unwrap();
        let other = root.join("other");
        std::fs::create_dir_all(&other).unwrap();
        assert_eq!(occupants(&folder), Some(vec![]));

        let mut child = Command::new("sleep")
            .arg("30")
            .current_dir(&below)
            .stdin(Stdio::null())
            .spawn()
            .expect("sleep runs");
        let found = occupants(&folder);
        let none = occupants(&other);
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&root);

        assert_eq!(found, Some(vec![child.id()]));
        assert_eq!(none, Some(vec![]));
        // Gone, so is the folder: no one stands in it.
        assert_eq!(occupants(&folder), Some(vec![]));
    }
}
