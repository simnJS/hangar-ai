//! The shell's own calls into hangar-core: starting and stopping it, the
//! state file, the one-time detections, notifications.
//!
//! Packages call hangar-core directly, in the module of each former command
//! (docs/native/core-api.md lists them: `board_load` is
//! `hangar_core::board_load`, `git_*` is `hangar_core::git::*`…), with the
//! `Host` and the managers `Services::core()` hands them. What is here is only
//! what the shell itself needs, wrapped for `Task::perform`: the sync ones
//! that touch the disk run on Tokio's blocking pool so the UI thread never
//! waits on them (core-api.md, "Conventions").

use std::sync::Arc;

use serde_json::Value;

use hangar_core::{Core, Host};

use crate::services::events::Bus;
use crate::shell::Notification;
use crate::state::AppState;
use crate::state::types::ShellInfo;

/// `hangar-ai --mcp`: the stdio MCP server agent CLIs start. No window, no
/// `Core`.
pub fn run_mcp() {
    hangar_core::run_mcp();
}

/// The running core, shared by the shell and every package.
#[derive(Clone)]
pub struct CoreHandles {
    core: Arc<Core>,
}

impl CoreHandles {
    /// What the old Tauri `setup` did: the watchers, Hangar Mobile, the
    /// agents' API and its endpoint file (core-api.md, "Starting and
    /// stopping"). Returns the bus, subscribed before anything could send on
    /// it.
    ///
    /// Must run inside the app's Tokio runtime, which outlives the core: iced
    /// calls `App::boot` inside it.
    pub fn start() -> (CoreHandles, Bus) {
        // `Host::new` reads `HANGAR_DATA_ROOT` (paths.rs), which keeps a
        // development build off the installed app's data (main.rs).
        let host = Host::new();
        let bus = Bus::new(host.subscribe());
        let core = Core::start(host, &tokio::runtime::Handle::current());
        (
            CoreHandles {
                core: Arc::new(core),
            },
            bus,
        )
    }

    pub fn core(&self) -> &Core {
        &self.core
    }

    pub fn host(&self) -> &Host {
        self.core.host()
    }

    /// The panes' terminals (`pty_spawn`, `pty_write`…, P5).
    pub fn pty(&self) -> &Arc<hangar_core::pty::PtyManager> {
        self.core.pty()
    }

    /// What the Tauri app did when the main window went: kill the panes'
    /// shells, tell Hangar Mobile, cancel deliveries, clear the endpoint,
    /// drop the Discord presence. The app exits right after.
    pub fn shutdown(&self) {
        self.core.shutdown();
    }
}

/// `load_state`: the raw JSON of `state.json`, `None` when there is none yet.
/// Core moves a plaintext Groq key to the keychain and quarantines an
/// unreadable file (`state.json.corrupt.*`) before answering.
pub async fn load_state(host: Host) -> Result<Option<Value>, String> {
    blocking(move || hangar_core::store::load_state(&host)).await
}

/// `save_state`: the whole state, written to `state.json.tmp` then renamed.
pub async fn save_state(host: Host, state: Arc<AppState>) -> Result<(), String> {
    blocking(move || {
        let value = serde_json::to_value(&*state).map_err(|err| err.to_string())?;
        hangar_core::store::save_state(&host, value)
    })
    .await
}

/// The same, on the calling thread: the last save on the way out, when there
/// is no runtime left to wait on.
pub fn save_state_now(host: &Host, state: &AppState) -> Result<(), String> {
    let value = serde_json::to_value(state).map_err(|err| err.to_string())?;
    hangar_core::store::save_state(host, value)
}

/// `detect_agents` and `detect_shells`, once at start (inventory §1.6).
pub async fn detect() -> (Vec<String>, Vec<ShellInfo>) {
    blocking(|| {
        Ok((
            hangar_core::sessions::detect_agents(),
            hangar_core::shells::detect_shells(),
        ))
    })
    .await
    .unwrap_or_default()
}

/// `notify_send`: a desktop notification; one that names a pane comes back,
/// clicked, as `CoreEvent::NotificationActivated` (Windows).
pub async fn notify(host: Host, notification: Notification) -> Result<(), String> {
    blocking(move || {
        let (workspace, pane) = notification.target.unwrap_or_default();
        hangar_core::notify::notify_send(
            &host,
            notification.title,
            notification.body,
            workspace,
            pane,
        )
    })
    .await
}

/// `f` on Tokio's blocking pool.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|err| format!("the task stopped: {err}"))?
}
