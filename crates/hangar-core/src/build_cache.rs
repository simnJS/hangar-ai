//! One build cache per workspace for its workers (off unless the person turns it on),
//! and one folder of its own for its delivery checks (`check_dir_for`).
//!
//! Why:
//! - **Dependencies once.** A worker used to build in its own target folder and
//!   a delivery check in a brand-new scratch worktree, so every one of them
//!   compiled all the dependencies again from nothing. Pointed at one folder
//!   per workspace, they compile once and every later build only compiles what
//!   changed.
//! - **MAX_PATH.** On Windows, a scratch worktree under `%TEMP%\hangar-deliveries`
//!   puts a build path past 260 characters, which `whisper-rs-sys` cannot
//!   build in. The cache therefore lives at a short path:
//!   `%LOCALAPPDATA%\hangar\b\<8 hex>`.
//! - **Disk.** One target folder per worker reached 30 GB. One per workspace
//!   is bounded, and the person can empty it from the orchestration panel.
//!
//! Only worker panes and delivery checks use it, never the person's terminals.
//! Cargo is the only toolchain for now; `env_for` is where others would go.

use std::io;
use std::path::{Path, PathBuf};

/// A stable 32-bit FNV-1a: `DefaultHasher` makes no promise across releases,
/// and the folder must stay the same from one version of Hangar to the next.
fn hash(text: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in text.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// The 8 hex digits that name a workspace's cache folder.
pub fn key_for(workspace_id: &str) -> String {
    format!("{:08x}", hash(workspace_id))
}

/// The workspace's build cache folder: short on purpose. Under
/// `HANGAR_DATA_ROOT` when that is set (a run kept off the installed app's
/// files), in the system's local-data (Windows) or cache folder otherwise.
pub fn dir_for(workspace_id: &str) -> PathBuf {
    let base = match std::env::var_os("HANGAR_DATA_ROOT").filter(|value| !value.is_empty()) {
        Some(root) => PathBuf::from(root),
        None => system_base(),
    };
    base.join("hangar").join("b").join(key_for(workspace_id))
}

#[cfg(windows)]
fn system_base() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(std::env::temp_dir)
}

#[cfg(not(windows))]
fn system_base() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir)
}

/// The environment a worker pane or a check starts with so that its builds go
/// to the workspace's cache: empty when `enabled` is false or the root holds
/// nothing a cache is known for.
pub fn env_for(workspace_root: &Path, workspace_id: &str, enabled: bool) -> Vec<(String, String)> {
    let mut env = Vec::new();
    if !enabled {
        return env;
    }
    if workspace_root.join("Cargo.toml").is_file() {
        env.push((
            "CARGO_TARGET_DIR".to_string(),
            dir_for(workspace_id)
                .join("target")
                .to_string_lossy()
                .into_owned(),
        ));
    }
    env
}

/// The folder the delivery checks of a workspace build in: a sibling of
/// `dir_for`'s, `%LOCALAPPDATA%\hangar\k\<8 hex>`, short for the same reason.
///
/// Never the workers' folder. Worktrees of one workspace that share a target
/// folder give their workspace crates the same artifact names, and Cargo only
/// compares source mtimes by package-relative path, so a tree can silently
/// reuse another branch's build of the same crate. The check runs in one
/// folder of its own that no worker builds in, always on the same path.
pub fn check_dir_for(workspace_id: &str) -> PathBuf {
    let base = match std::env::var_os("HANGAR_DATA_ROOT").filter(|value| !value.is_empty()) {
        Some(root) => PathBuf::from(root),
        None => system_base(),
    };
    base.join("hangar").join("k").join(key_for(workspace_id))
}

/// What a delivery check starts with so that it builds into its own folder
/// (`check_dir_for`): used always, whatever the workers' cache toggle says,
/// because that folder is shared with nothing. Empty when the root holds
/// nothing a cache is known for.
pub fn check_env_for(workspace_root: &Path, workspace_id: &str) -> Vec<(String, String)> {
    let mut env = Vec::new();
    if workspace_root.join("Cargo.toml").is_file() {
        env.push((
            "CARGO_TARGET_DIR".to_string(),
            check_dir_for(workspace_id)
                .join("target")
                .to_string_lossy()
                .into_owned(),
        ));
    }
    env
}

/// The bytes under `dir`, 0 when it does not exist. Walks the folder: call it
/// off the UI thread. Links are not followed.
pub fn size_of(dir: &Path) -> u64 {
    let mut total = 0;
    let mut pending = vec![dir.to_path_buf()];
    while let Some(folder) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                pending.push(entry.path());
            } else if let Ok(meta) = entry.metadata() {
                total += meta.len();
            }
        }
    }
    total
}

/// Deletes everything in the cache folder. A folder that is not there is
/// already empty.
pub fn empty(dir: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(dir) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_check_builds_in_a_folder_the_workers_never_share() {
        let root = crate::testdir::scratch("check-env");
        assert!(check_env_for(&root, "w1").is_empty());
        std::fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
        let env = check_env_for(&root, "w1");
        assert_eq!(env.len(), 1);
        assert_eq!(env[0].0, "CARGO_TARGET_DIR");
        let target = PathBuf::from(&env[0].1);
        assert!(target.starts_with(check_dir_for("w1")));
        assert_ne!(check_dir_for("w1"), dir_for("w1"));
        assert!(!target.starts_with(dir_for("w1")));
        assert_ne!(check_dir_for("w1"), check_dir_for("w2"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_key_is_stable_and_eight_hex() {
        assert_eq!(key_for("ws-1"), key_for("ws-1"));
        let key = key_for("ws-1");
        assert_eq!(key.len(), 8);
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
        // Fixed: the folder must not move between releases.
        assert_eq!(key_for(""), "811c9dc5");
    }

    #[test]
    fn the_folder_is_short_and_differs_per_workspace() {
        let a = dir_for("workspace-a");
        assert!(a.to_string_lossy().len() <= 80, "{}", a.display());
        assert_ne!(a, dir_for("workspace-b"));
        assert_eq!(a, dir_for("workspace-a"));
        assert!(a.ends_with(Path::new("b").join(key_for("workspace-a"))));
    }

    #[test]
    fn only_a_cargo_root_that_is_enabled_gets_the_variable() {
        let root = crate::testdir::scratch("bc");
        assert!(env_for(&root, "w", true).is_empty());
        std::fs::write(root.join("Cargo.toml"), "").unwrap();
        let env = env_for(&root, "w", true);
        assert_eq!(env.len(), 1);
        assert_eq!(env[0].0, "CARGO_TARGET_DIR");
        assert!(Path::new(&env[0].1).starts_with(dir_for("w")));
        assert!(env_for(&root, "w", false).is_empty());
    }

    #[test]
    fn size_and_empty() {
        let dir = crate::testdir::scratch("bc-size").join("folder");
        assert_eq!(size_of(&dir), 0);
        std::fs::create_dir_all(dir.join("a/b")).unwrap();
        std::fs::write(dir.join("a/b/x"), [0u8; 10]).unwrap();
        std::fs::write(dir.join("y"), [0u8; 5]).unwrap();
        assert_eq!(size_of(&dir), 15);
        empty(&dir).unwrap();
        assert_eq!(size_of(&dir), 0);
        empty(&dir).unwrap();
    }
}
