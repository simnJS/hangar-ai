use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::store::{write_atomic, Access};

/// Where the running app advertises how to reach its API.
///
/// The port is ephemeral and the token is regenerated every launch, so agent
/// configs must never hardcode them. They point at the binary only, and the
/// binary resolves the live endpoint from this file at startup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Endpoint {
    pub port: u16,
    pub token: String,
    pub pid: u32,
}

pub fn endpoint_path() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".hangar").join("endpoint.json"))
}

/// Where the endpoint lived before the directory was renamed.
fn legacy_endpoint_path() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".iabench").join("endpoint.json"))
}

/// Both locations, current first.
///
/// This file is the handshake between the app and the `--mcp` process, and the
/// two are separate binaries that get updated at different times: an agent tool
/// may still be launching a pre-rename executable long after the app itself has
/// been updated. Writing both locations and reading either keeps every
/// combination working; once only post-rename binaries are in play, the legacy
/// path can go.
///
/// `HANGAR_ENDPOINT_FILE` replaces both, for a development build run next to
/// an installed Hangar: sharing the file would hand every agent of the
/// installed app over to the build, and closing the build would delete it.
fn endpoint_paths() -> Vec<PathBuf> {
    if let Some(file) = std::env::var_os("HANGAR_ENDPOINT_FILE").filter(|v| !v.is_empty()) {
        return vec![PathBuf::from(file)];
    }
    [endpoint_path(), legacy_endpoint_path()]
        .into_iter()
        .flatten()
        .collect()
}

fn body(port: u16, token: &str) -> Result<String, String> {
    serde_json::to_string_pretty(&Endpoint {
        port,
        token: token.to_string(),
        pid: std::process::id(),
    })
    .map_err(|e| e.to_string())
}

/// The endpoint written to one file the caller chose, rather than to the
/// app's own (`publish`): a headless node (headless.rs) next to a desktop
/// Hangar on the same account must not take its agents over. Its folder is
/// created private if missing and otherwise left as it is, as one named by
/// `HANGAR_ENDPOINT_FILE` is.
pub fn publish_to(path: &Path, port: u16, token: &str) -> Result<(), String> {
    let body = body(port, token)?;
    if let Some(parent) = path.parent() {
        private_folder(parent, false);
    }
    write_atomic(path, body.as_bytes(), Access::Private)
}

/// `clear` for a file `publish_to` wrote.
pub fn clear_at(path: &Path) {
    let _ = fs::remove_file(path);
}

pub fn publish(port: u16, token: &str) -> Result<(), String> {
    let body = body(port, token)?;

    let paths = endpoint_paths();
    if paths.is_empty() {
        return Err("no home directory".into());
    }

    // The current location is the one that has to succeed; failing to refresh
    // the legacy copy only costs older binaries their connection.
    let own_folders = std::env::var_os("HANGAR_ENDPOINT_FILE").is_none_or(|v| v.is_empty());
    let mut result = Ok(());
    for (nth, path) in paths.iter().enumerate() {
        if let Some(parent) = path.parent() {
            private_folder(parent, own_folders);
        }
        let outcome = write_atomic(path, body.as_bytes(), Access::Private);
        if nth == 0 {
            result = outcome;
        }
    }
    result
}

/// The token in this file is the outside one (server.rs, `outside_token`):
/// never Hangar's own, and no pane's. It still lets whoever reads it type
/// into the person's shells (`/api/control/pane_send`), so the file is the owner's alone (0600,
/// `Access::Private`), and so is the folder Hangar keeps it in (0700): a
/// home that other users may list (Debian, Fedora) must not hand it out.
///
/// The folder is only tightened when it is Hangar's own (`~/.hangar`,
/// `~/.iabench`, which also hold the accounts and kits): one named by
/// `HANGAR_ENDPOINT_FILE` may be anything, `/tmp` included, and is only
/// created, private, if missing.
///
/// On Windows the file sits in the user's profile, whose ACL already keeps
/// other users out; nothing more is set there.
fn private_folder(dir: &Path, own: bool) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        let _ = fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir);
        if own {
            let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
        }
    }
    #[cfg(not(unix))]
    {
        let _ = own;
        let _ = fs::create_dir_all(dir);
    }
}

pub fn read() -> Option<Endpoint> {
    endpoint_paths().into_iter().find_map(|path| {
        let raw = fs::read_to_string(path).ok()?;
        serde_json::from_str(&raw).ok()
    })
}

pub fn clear() {
    for path in endpoint_paths() {
        let _ = fs::remove_file(path);
    }
}

/// Two v4 UUIDs concatenated: 256 bits from the OS entropy source, and no
/// extra dependency beyond the uuid crate already in use.
pub fn generate_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

// Its one test is about Unix permission bits: on Windows the module would be
// empty and its import unused.
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn the_endpoint_file_and_its_folder_are_the_owner_s_alone() {
        use std::os::unix::fs::PermissionsExt;
        let root = crate::testdir::scratch("endpoint");
        let dir = root.join(".hangar");
        fs::create_dir_all(&dir).unwrap();
        // A folder made by an earlier version, readable by all.
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        private_folder(&dir, true);
        let path = dir.join("endpoint.json");
        fs::write(&path, "{}").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        write_atomic(&path, b"{\"token\":\"t\"}", Access::Private).unwrap();

        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&path), 0o600);

        // A folder Hangar does not own is created private, never changed.
        let shared = root.join("shared");
        fs::create_dir_all(&shared).unwrap();
        fs::set_permissions(&shared, fs::Permissions::from_mode(0o755)).unwrap();
        private_folder(&shared, false);
        assert_eq!(mode(&shared), 0o755);
        let fresh = root.join("fresh");
        private_folder(&fresh, false);
        assert_eq!(mode(&fresh), 0o700);
        fs::remove_dir_all(&root).ok();
    }
}
