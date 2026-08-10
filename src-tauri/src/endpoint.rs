use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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
fn endpoint_paths() -> Vec<PathBuf> {
    [endpoint_path(), legacy_endpoint_path()]
        .into_iter()
        .flatten()
        .collect()
}

pub fn publish(port: u16, token: &str) -> Result<(), String> {
    let body = serde_json::to_string_pretty(&Endpoint {
        port,
        token: token.to_string(),
        pid: std::process::id(),
    })
    .map_err(|e| e.to_string())?;

    let paths = endpoint_paths();
    if paths.is_empty() {
        return Err("no home directory".into());
    }

    // The current location is the one that has to succeed; failing to refresh
    // the legacy copy only costs older binaries their connection.
    let mut result = Ok(());
    for (nth, path) in paths.iter().enumerate() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let outcome = fs::write(path, &body).map_err(|e| e.to_string());
        if nth == 0 {
            result = outcome;
        }
    }
    result
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
