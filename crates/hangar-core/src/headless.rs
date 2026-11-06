//! What a headless Hangar runs: the panes' terminals and the agents' API,
//! with no window around them (docs/native/nodes.md).
//!
//! `Core` (lib.rs) is the desktop's setup and needs the `desktop` feature;
//! this is the part of it a node on a server needs, and builds without it: no
//! dictation, no notifications, no Discord, no login item, and no Hangar
//! Mobile — a phone reaches a node through the desktop it is attached to, not
//! through a server of its own.
//!
//! `start` sets up, in this order:
//!
//! - the login shell's PATH (path_env.rs) and the hangar-bridge mod's folder
//!   (bridge.rs), warmed on threads of their own as `Core` does, so the first
//!   pane does not wait on them;
//! - the accounts and kits watchers (accounts.rs, kits.rs), which poll files
//!   on a thread and send `Event::AccountsChanged` / `Event::KitsChanged`;
//! - an empty `PtyManager` for the panes;
//! - the agents' HTTP API (server.rs) on an ephemeral loopback port, with a
//!   fresh app token, made known to the panes (`bridge::set_endpoint`) and
//!   published to the endpoint file (endpoint.rs) for the MCP proxies.
//!
//! `Headless::shutdown` takes down what outlives the process otherwise: the
//! panes' process groups, a check or review still running, the endpoint file.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::runtime::Handle;

use crate::{accounts, bridge, delivery, endpoint, kits, path_env, pty, server, Host};

/// What a running node holds, handed back by `start`.
pub struct Headless {
    host: Host,
    pty: Arc<pty::PtyManager>,
    port: u16,
    token: String,
    endpoint_file: Option<PathBuf>,
}

/// Starts a node's backend. Subscribe to `host` before calling this, as for
/// `Core::start`: the watchers and the API send events from the moment they
/// start.
///
/// `endpoint_file` is where the API's port and outside token are published:
/// `None` for the usual place (`~/.hangar/endpoint.json`, or
/// `HANGAR_ENDPOINT_FILE`, endpoint.rs), a path to keep a node apart from a
/// desktop Hangar running for the same account.
///
/// The API runs on `runtime`, which must outlive the node. This waits for it
/// to listen, so it is called from outside the runtime (a plain thread, `main`
/// before it enters the runtime): blocking on it from one of its own tasks
/// panics. An error is the API that could not start, or an endpoint that could
/// not be published — either leaves the node unreachable, so nothing is left
/// running but the watchers.
pub fn start(
    host: Host,
    runtime: &Handle,
    endpoint_file: Option<PathBuf>,
) -> Result<Headless, String> {
    std::thread::spawn(path_env::ensure);
    std::thread::spawn(|| {
        bridge::plugin_dir();
    });
    accounts::watch(host.clone());
    kits::watch(host.clone());

    let pty = Arc::new(pty::PtyManager::default());
    let token = endpoint::generate_token();
    let port = runtime.block_on(serve(&host, &pty, token.clone()))?;
    bridge::set_endpoint(port, &token);
    publish(endpoint_file.as_deref(), port)?;
    Ok(Headless {
        host,
        pty,
        port,
        token,
        endpoint_file,
    })
}

/// The agents' API, on the board store the rest of hangar-core shares.
async fn serve(host: &Host, pty: &Arc<pty::PtyManager>, token: String) -> Result<u16, String> {
    let reach = server::Reach {
        host: host.clone(),
        pty: pty.clone(),
        // Built with `desktop`, the node still has no Hangar Mobile.
        #[cfg(feature = "desktop")]
        remote: None,
    };
    server::start(reach, crate::ui_store(), token)
        .await
        .map_err(|err| format!("the agents' API could not start: {err}"))
}

/// The outside token, never the app's: the file is readable by any process of
/// the account's, as it is for the desktop (lib.rs).
fn publish(file: Option<&std::path::Path>, port: u16) -> Result<(), String> {
    let token = server::outside_token();
    match file {
        Some(file) => endpoint::publish_to(file, port, token),
        None => endpoint::publish(port, token),
    }
    .map_err(|err| format!("could not publish the endpoint: {err}"))
}

impl Headless {
    pub fn host(&self) -> &Host {
        &self.host
    }

    /// The panes' terminals (pty.rs).
    pub fn pty(&self) -> &Arc<pty::PtyManager> {
        &self.pty
    }

    /// The loopback port the agents' API listens on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Hangar's own token for the API (`Caller::App`, server.rs): whoever
    /// drives the node speaks with it. Never handed to a pane or written to
    /// the endpoint file.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// The panes killed, process groups and all, a check or review still
    /// running cancelled, and the endpoint file removed so no agent tries a
    /// dead port. The API stops with the runtime it runs on.
    pub fn shutdown(&self) {
        pty::kill_all(&self.pty);
        delivery::cancel_all();
        match &self.endpoint_file {
            Some(file) => endpoint::clear_at(file),
            None => endpoint::clear(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    /// The API `start` brings up, and the endpoint it publishes to a file of
    /// the caller's. `start` itself also starts the watchers and writes the
    /// mod beside the person's real `~/.hangar`, which a test must not touch.
    #[test]
    fn the_api_listens_and_its_endpoint_lands_in_the_chosen_file() {
        let root = crate::testdir::scratch("headless");
        let host = Host::with_paths(Paths::rooted(&root.join("data")));
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let pty = Arc::new(pty::PtyManager::default());
        let token = endpoint::generate_token();
        let port = runtime.block_on(serve(&host, &pty, token)).unwrap();

        let file = root.join("node").join("endpoint.json");
        publish(Some(&file), port).unwrap();
        let written: endpoint::Endpoint =
            serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(written.port, port);
        assert_eq!(written.token, server::outside_token());
        assert_eq!(written.pid, std::process::id());

        // Asked over a plain socket: an HTTP client could go through a proxy
        // the environment names.
        use std::io::{Read, Write};
        let mut socket = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        socket
            .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .unwrap();
        let mut answer = String::new();
        socket.read_to_string(&mut answer).unwrap();
        assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
        assert!(answer.ends_with(r#"{"ok":true}"#), "{answer}");

        endpoint::clear_at(&file);
        assert!(!file.exists());
    }
}
