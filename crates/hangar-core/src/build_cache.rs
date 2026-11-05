//! One build cache per workspace, shared by its workers and its delivery checks.
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

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

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

fn registry() -> &'static Mutex<HashMap<String, Vec<(String, String)>>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, Vec<(String, String)>>>> = OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}

/// Remembers the environment of a worker pane, to be applied when its shell
/// starts (`pty_spawn`) and again if it is restarted.
pub fn assign_pane(pane_id: &str, env: Vec<(String, String)>) {
    let mut map = registry().lock().unwrap_or_else(|p| p.into_inner());
    if env.is_empty() {
        map.remove(pane_id);
    } else {
        map.insert(pane_id.to_string(), env);
    }
}

/// What `assign_pane` was given for this pane.
pub fn pane_env(pane_id: &str) -> Vec<(String, String)> {
    let map = registry().lock().unwrap_or_else(|p| p.into_inner());
    map.get(pane_id).cloned().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let root = std::env::temp_dir().join(format!("hangar-bc-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        assert!(env_for(&root, "w", true).is_empty());
        std::fs::write(root.join("Cargo.toml"), "").unwrap();
        let env = env_for(&root, "w", true);
        assert_eq!(env.len(), 1);
        assert_eq!(env[0].0, "CARGO_TARGET_DIR");
        assert!(Path::new(&env[0].1).starts_with(dir_for("w")));
        assert!(env_for(&root, "w", false).is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn size_and_empty() {
        let dir = std::env::temp_dir().join(format!("hangar-bc-size-{}", std::process::id()));
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
