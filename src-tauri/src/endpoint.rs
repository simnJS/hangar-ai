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
    Some(dirs::home_dir()?.join(".iabench").join("endpoint.json"))
}

pub fn publish(port: u16, token: &str) -> Result<(), String> {
    let path = endpoint_path().ok_or("no home directory")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_string_pretty(&Endpoint {
        port,
        token: token.to_string(),
        pid: std::process::id(),
    })
    .map_err(|e| e.to_string())?;
    fs::write(&path, body).map_err(|e| e.to_string())
}

pub fn read() -> Option<Endpoint> {
    let path = endpoint_path()?;
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

pub fn clear() {
    if let Some(path) = endpoint_path() {
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
