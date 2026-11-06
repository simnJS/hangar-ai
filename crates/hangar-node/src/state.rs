//! `~/.local/state/hangar-node/`: what the desktop reads over SSH to reach the
//! node (`node.json`), the daemon's log, the agents' endpoint file, and the
//! lock that keeps the node to one daemon per account.
#![cfg(target_os = "linux")]

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// `node.json`: how the desktop reaches the node through its tunnel.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeFile {
    pub version: String,
    pub port: u16,
    pub token: String,
    pub pid: u32,
}

/// The node's folder, from the home directory: `~/.local/state/hangar-node`,
/// where the desktop looks for it whatever `XDG_STATE_HOME` says.
pub fn dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("no home directory")?;
    Ok(home.join(".local").join("state").join("hangar-node"))
}

pub fn node_file(dir: &Path) -> PathBuf {
    dir.join("node.json")
}

pub fn log_file(dir: &Path) -> PathBuf {
    dir.join("node.log")
}

/// The agents' API endpoint (hangar-core's endpoint.rs), the node's own.
pub fn endpoint_file(dir: &Path) -> PathBuf {
    dir.join("endpoint.json")
}

fn lock_file(dir: &Path) -> PathBuf {
    dir.join("node.lock")
}

/// The folder, created if missing and made the owner's alone (0700): the
/// token in `node.json` lets whoever reads it type into the node's shells.
pub fn ensure_dir(dir: &Path) -> Result<(), String> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(|err| format!("{}: {err}", dir.display()))?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
        .map_err(|err| format!("{}: {err}", dir.display()))
}

/// Written whole or not at all: to a private temporary file beside it, then
/// renamed over it, so a reader never sees half a file or a moment of 0644.
pub fn write(dir: &Path, node: &NodeFile) -> Result<(), String> {
    let path = node_file(dir);
    let temp = dir.join(format!(".node.json.{}", std::process::id()));
    let body = serde_json::to_vec_pretty(node).map_err(|err| err.to_string())?;
    let written = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temp)?;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        file.write_all(&body)?;
        file.sync_all()?;
        fs::rename(&temp, &path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written.map_err(|err| format!("{}: {err}", path.display()))
}

pub fn read(dir: &Path) -> Option<NodeFile> {
    let raw = fs::read_to_string(node_file(dir)).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Removed on the way out, only while it is still this process's.
pub fn clear(dir: &Path) {
    if read(dir).is_some_and(|node| node.pid == std::process::id()) {
        let _ = fs::remove_file(node_file(dir));
    }
}

/// Whether a process `pid` exists (one of another user's counts too).
pub fn alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
    // SAFETY: signal 0 checks the process without signalling it.
    let found = unsafe { libc::kill(pid, 0) } == 0;
    found || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// The daemon `node.json` names, when it is alive and answers its token.
pub fn running(dir: &Path) -> Option<NodeFile> {
    let node = read(dir)?;
    (alive(node.pid) && answers(&node)).then_some(node)
}

/// `GET /node/info` with the node's token, over a plain socket on loopback:
/// a 200 within two seconds.
pub fn answers(node: &NodeFile) -> bool {
    let attempt = || -> std::io::Result<bool> {
        let address = std::net::SocketAddr::from(([127, 0, 0, 1], node.port));
        let timeout = Duration::from_secs(2);
        let mut socket = std::net::TcpStream::connect_timeout(&address, timeout)?;
        socket.set_read_timeout(Some(timeout))?;
        socket.set_write_timeout(Some(timeout))?;
        write!(
            socket,
            "GET /node/info HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",
            node.token
        )?;
        let mut head = [0u8; 12];
        socket.read_exact(&mut head)?;
        Ok(head.starts_with(b"HTTP/1.1 200"))
    };
    attempt().unwrap_or(false)
}

/// The lock a daemon holds for as long as it runs: the one that gets it is
/// the account's node, and any other `serve` waits for that one instead.
pub struct Lock {
    _file: File,
}

/// `None` when another process holds it.
pub fn try_lock(dir: &Path) -> Result<Option<Lock>, String> {
    let path = lock_file(dir);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&path)
        .map_err(|err| format!("{}: {err}", path.display()))?;
    // SAFETY: flock on a descriptor this function owns; released when the
    // file closes, the process's end included.
    let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
    if locked {
        return Ok(Some(Lock { _file: file }));
    }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::EWOULDBLOCK) => Ok(None),
        _ => Err(format!(
            "{}: {}",
            path.display(),
            std::io::Error::last_os_error()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> NodeFile {
        NodeFile {
            version: "1.0.0".into(),
            port: 4242,
            token: "t".into(),
            pid: std::process::id(),
        }
    }

    #[test]
    fn node_file_is_the_owners_alone_and_reads_back() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("state").join("hangar-node");
        ensure_dir(&dir).unwrap();
        write(&dir, &sample()).unwrap();
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&node_file(&dir)), 0o600);
        assert_eq!(read(&dir), Some(sample()));
        clear(&dir);
        assert!(!node_file(&dir).exists());
    }

    #[test]
    fn one_lock_at_a_time() {
        let root = tempfile::tempdir().unwrap();
        let first = try_lock(root.path()).unwrap();
        assert!(first.is_some());
        assert!(try_lock(root.path()).unwrap().is_none());
        drop(first);
        assert!(try_lock(root.path()).unwrap().is_some());
    }

    #[test]
    fn a_dead_pid_is_not_alive() {
        assert!(alive(std::process::id()));
        assert!(!alive(0));
        assert!(!alive(u32::MAX));
    }
}
