//! Everything Hangar does that is not drawing: the panes' terminals, the
//! board, the memory, git, orchestration, the agents' API and MCP server,
//! kits, Hangar Mobile.
//!
//! This was the Tauri app's backend. Each `#[tauri::command]` the window
//! invoked is a plain function now, in the module it always lived in, and the
//! app calls it directly; docs/native/core-api.md lists every one with what
//! the window called it. What took a Tauri handle takes what it used of it:
//! a `Host` for the folders and the events (host.rs), the `Core`'s managers
//! for Tauri's managed state.
//!
//! Async functions run their slow part on Tokio's blocking pool, as they did
//! on Tauri's runtime, and are to be awaited on a Tokio runtime. A function
//! that is not async does its work where it is called: one that reads the
//! disk or asks git was run on Tauri's main thread before, and is best called
//! off the app's own.

pub mod accounts;
pub mod autostart;
pub mod board;
pub mod bridge;
pub mod chat;
pub mod context;
pub mod control;
pub mod delivery;
pub mod discord;
pub mod endpoint;
pub mod git;
pub mod git_clone;
pub mod host;
pub mod install;
pub mod instructions;
pub mod kit_scheme;
pub mod kits;
pub mod mcp;
pub mod mcp_scope;
pub mod memory;
pub mod model;
pub mod notify;
pub mod open_editor;
pub mod opener;
pub mod orchestrator;
pub mod path_env;
pub mod paths;
pub mod preview;
pub mod pricing;
pub mod procs;
pub mod pty;
pub mod remote;
pub mod server;
pub mod sessions;
pub mod shells;
pub mod stats;
pub mod store;
pub mod voice;
pub mod workspace_file;

pub use host::{Event, Host};
pub use paths::Paths;

use std::sync::Arc;

/// Entry point for `hangar-ai --mcp`: no window, no runtime of the app's, just
/// the stdio protocol loop talking to whatever app instance is running.
pub fn run_mcp() {
    mcp::run_stdio();
}

/// What the Tauri app set up around the window and took down after it: the
/// state its commands were handed (`State<…>`), and the work its `setup` hook
/// started — the agents' API, Hangar Mobile, the watchers.
///
/// One per process, made by `start` once the window can listen, ended by
/// `shutdown` when the main window goes.
pub struct Core {
    host: Host,
    pty: Arc<pty::PtyManager>,
    discord: discord::DiscordPresence,
    voice: Arc<voice::VoiceManager>,
    /// `None` when Hangar Mobile could not open its store: its commands are
    /// then unavailable, as they were in the Tauri app, which never managed
    /// the state they asked for.
    remote: Option<Arc<remote::Remote>>,
}

impl Core {
    /// What the Tauri `setup` hook did: the slow lookups warmed on threads of
    /// their own, the watchers, Hangar Mobile opened and resumed, and the
    /// agents' API started, its port and token published for the agents to
    /// find.
    ///
    /// Subscribe to `host` before calling this: the watchers and the API send
    /// events from the moment they start. The API and Hangar Mobile run on
    /// `runtime`, which must outlive the `Core`.
    pub fn start(host: Host, runtime: &tokio::runtime::Handle) -> Self {
        // Warmed here rather than on the first detection call: asking the
        // login shell for its PATH takes a moment, and doing it now means
        // the window is never the thing waiting on it.
        std::thread::spawn(path_env::ensure);
        // Written before the first pane asks for it, for the same reason.
        std::thread::spawn(|| {
            bridge::plugin_dir();
        });
        // `Event::KitsChanged` when a linked kit's manifest is edited on disk.
        kits::watch(host.clone());
        // `Event::AccountsChanged` when a Claude account logs in or out.
        accounts::watch(host.clone());

        let pty = Arc::new(pty::PtyManager::default());

        // Hangar Mobile: a listener of its own, off until the person turns
        // it on, and on again at launch once they have (remote/). Opened
        // before the API starts, which hands an agent's shares to it.
        let remote = match remote::Remote::open(&host) {
            Ok(remote) => {
                let resumed = remote.clone();
                runtime.spawn(async move { resumed.resume().await });
                Some(remote)
            }
            Err(err) => {
                eprintln!("Hangar Mobile is unavailable: {err}");
                None
            }
        };

        let token = endpoint::generate_token();
        let advertised = token.clone();
        let api = server::Reach {
            host: host.clone(),
            pty: pty.clone(),
            remote: remote.clone(),
        };
        runtime.spawn(async move {
            match server::start(api, ui_store(), token).await {
                Ok(port) => {
                    bridge::set_endpoint(port, &advertised);
                    // Agents discover the live port/token here rather than
                    // having them baked into their config files.
                    if let Err(err) = endpoint::publish(port, &advertised) {
                        eprintln!("could not publish endpoint: {err}");
                    }
                }
                Err(err) => eprintln!("board API failed to start: {err}"),
            }
        });

        Self {
            host,
            pty,
            discord: discord::DiscordPresence::default(),
            voice: Arc::default(),
            remote,
        }
    }

    pub fn host(&self) -> &Host {
        &self.host
    }

    /// The panes' terminals (pty.rs).
    pub fn pty(&self) -> &Arc<pty::PtyManager> {
        &self.pty
    }

    /// The Discord presence (discord.rs).
    pub fn discord(&self) -> &discord::DiscordPresence {
        &self.discord
    }

    /// The dictation (voice/).
    pub fn voice(&self) -> &Arc<voice::VoiceManager> {
        &self.voice
    }

    /// Hangar Mobile (remote/), when it could be opened.
    pub fn remote(&self) -> Option<&Arc<remote::Remote>> {
        self.remote.as_ref()
    }

    /// What the Tauri app did when the main window was destroyed, before it
    /// exited. Only the main window's end is the app's: it owns the store, so
    /// the workspace windows go with it, and nothing is left to draw the
    /// terminals. The app exits once this returns.
    pub fn shutdown(&self) {
        pty::kill_all(&self.pty);
        // The history says Hangar stopped; the PC may sleep again.
        if let Some(remote) = &self.remote {
            remote.activity_stopped();
        }
        // A check or a review still running goes with the app.
        delivery::cancel_all();
        // Stale endpoint files would make agents try to reach a dead port.
        endpoint::clear();
        // Discord drops the presence when the pipe dies anyway; this just
        // takes it down before the window is gone from screen.
        self.discord.stop();
    }
}

// The UI goes through the same store as the HTTP API, so a card dragged in the
// window and a task claimed by an agent contend for the same lock.
use board::BoardStore;
use std::sync::OnceLock;

fn ui_store() -> &'static BoardStore {
    static STORE: OnceLock<BoardStore> = OnceLock::new();
    STORE.get_or_init(BoardStore::default)
}

/// The project manager's folder: `<app data dir>/hq`, created if missing. Its
/// Claude Code works from here rather than from any project.
pub fn hq_dir(host: &Host) -> Result<String, String> {
    let dir = host
        .paths()
        .app_data_dir()
        .map_err(|err| err.to_string())?
        .join("hq");
    std::fs::create_dir_all(&dir)
        .map_err(|err| format!("could not create {}: {err}", dir.display()))?;
    Ok(dir.to_string_lossy().into_owned())
}

pub fn board_load(cwd: String) -> board::Board {
    ui_store().read(&cwd)
}

pub fn board_create(cwd: String, task: board::NewTask) -> Result<board::Task, String> {
    ui_store().update(&cwd, |b| board::create_task(b, task))
}

pub fn board_update(
    cwd: String,
    id: String,
    patch: board::TaskPatch,
) -> Result<board::Task, String> {
    ui_store().update(&cwd, |b| board::patch_task(b, &id, patch))
}

pub fn board_delete(cwd: String, id: String) -> Result<(), String> {
    ui_store().update(&cwd, |b| board::delete_task(b, &id))
}

pub fn board_claim(cwd: String, id: String, agent: String) -> Result<board::Task, String> {
    ui_store().update(&cwd, |b| board::claim_task(b, &id, &agent))
}

/// A worker's start and end (lib/workerControl.ts). Done by the window rather
/// than by an agent over the API, so the board views are told here.
pub fn board_claim_worker(
    host: &Host,
    cwd: String,
    id: String,
    worker: String,
    max_attempts: u32,
    shared_folder: Option<bool>,
) -> Result<board::Task, String> {
    let shared_folder = shared_folder.unwrap_or(false);
    let task = ui_store().update(&cwd, |b| {
        board::claim_for_worker(b, &id, &worker, max_attempts, shared_folder)
    })?;
    let _ = host.emit(Event::BoardChanged { cwd });
    Ok(task)
}

pub fn board_release_worker(
    host: &Host,
    cwd: String,
    id: String,
    worker: String,
    undo: bool,
) -> Result<board::Task, String> {
    let task = ui_store().update(&cwd, |b| board::release_worker(b, &id, &worker, undo))?;
    let _ = host.emit(Event::BoardChanged { cwd });
    Ok(task)
}

pub fn board_comment(
    cwd: String,
    id: String,
    author: String,
    text: String,
) -> Result<board::Task, String> {
    ui_store().update(&cwd, |b| board::add_comment(b, &id, &author, &text))
}

#[cfg(test)]
mod tests {
    use std::future::Future;

    /// What the app needs of an async command: a future it can hand to its
    /// executor (`iced::Task::perform` wants `Send + 'static`), which owns
    /// what it was given rather than borrowing from the caller.
    fn sendable<F: Future + Send + 'static>(_: impl FnOnce() -> F) {}

    /// Checked when the tests compile; the closures are never called, which
    /// is what lets `todo!()` stand for every argument.
    #[allow(unreachable_code)]
    #[test]
    fn every_async_command_is_a_future_the_app_can_spawn() {
        sendable(|| crate::chat::chat_page(todo!(), todo!(), todo!(), todo!()));
        sendable(|| crate::context::context_usage(todo!(), todo!(), todo!()));
        sendable(|| crate::delivery::delivery_cleanup(todo!(), todo!(), todo!(), todo!()));
        sendable(|| {
            crate::delivery::delivery_preflight(todo!(), todo!(), todo!(), todo!(), todo!())
        });
        sendable(|| crate::delivery::delivery_run(todo!()));
        sendable(|| crate::git::git_branch_delete(todo!(), todo!(), todo!()));
        sendable(|| crate::git::git_branches(todo!()));
        sendable(|| crate::git::git_changes(todo!(), todo!()));
        sendable(|| crate::git_clone::git_clone(todo!(), todo!(), todo!(), todo!(), todo!()));
        sendable(|| crate::git_clone::git_clone_discard(todo!()));
        sendable(|| crate::git::git_commits_ahead(todo!(), todo!(), todo!()));
        sendable(|| crate::git::git_copy_untracked(todo!(), todo!(), todo!(), todo!()));
        sendable(|| crate::git::git_fetch(todo!()));
        sendable(|| crate::git::git_file_diff(todo!(), todo!(), todo!(), todo!(), todo!()));
        sendable(|| crate::git::git_repo_info(todo!()));
        sendable(|| crate::git_clone::git_repo_ownership(todo!(), todo!()));
        sendable(|| crate::git::git_worktree_add(todo!(), todo!(), todo!(), todo!(), todo!()));
        sendable(|| crate::git::git_worktree_hook(todo!(), todo!(), todo!()));
        sendable(|| crate::git::git_worktree_list(todo!()));
        sendable(|| crate::git::git_worktree_lock(todo!(), todo!()));
        sendable(|| crate::git::git_worktree_migrate(todo!(), todo!()));
        sendable(|| crate::git::git_worktree_prune(todo!()));
        sendable(|| crate::git::git_worktree_remove(todo!(), todo!(), todo!(), todo!()));
        sendable(|| crate::git::git_worktree_unlock(todo!(), todo!()));
        sendable(|| crate::git::git_worktrees_merged(todo!(), todo!()));
        sendable(|| crate::kits::kit_add_folder(todo!(), todo!()));
        sendable(|| crate::kits::kit_add_git(todo!(), todo!()));
        sendable(|| crate::kits::kit_check_update(todo!(), todo!()));
        sendable(|| {
            crate::kits::commands::kit_command_run(todo!(), todo!(), todo!(), todo!(), todo!())
        });
        sendable(|| crate::kits::data::kit_data_read(todo!(), todo!()));
        sendable(|| crate::kits::data::kit_data_write(todo!(), todo!(), todo!()));
        sendable(|| crate::kit_scheme::files::kit_file_read(todo!(), todo!()));
        sendable(|| crate::kits::files::kit_files_list(todo!(), todo!(), todo!()));
        sendable(|| crate::kits::files::kit_files_read(todo!(), todo!()));
        sendable(|| crate::kits::files::kit_files_read_binary(todo!(), todo!(), todo!()));
        sendable(|| crate::kits::files::kit_files_write(todo!(), todo!(), todo!(), todo!()));
        sendable(|| crate::kits::files::kit_files_write_binary(todo!(), todo!(), todo!()));
        sendable(crate::kits::gallery::kit_gallery_list);
        sendable(|| crate::kits::kit_inspect(todo!()));
        sendable(|| {
            crate::kits::loopback::kit_loopback_fetch(todo!(), todo!(), todo!(), todo!(), todo!())
        });
        sendable(|| crate::kits::kit_remove(todo!(), todo!()));
        sendable(|| crate::kits::kit_setup_mark(todo!(), todo!()));
        sendable(|| crate::kits::kit_setup_status(todo!()));
        sendable(|| crate::kits::kit_update(todo!(), todo!()));
        sendable(|| crate::kit_scheme::files::kit_workspace_path(todo!(), todo!()));
        sendable(crate::kits::kits_list);
        sendable(|| crate::model::model_allow_roots(todo!()));
        sendable(|| crate::model::model_save_png(todo!(), todo!()));
        sendable(|| crate::model::model_stat(todo!()));
        sendable(|| crate::open_editor::open_in_vscode(todo!(), todo!()));
        sendable(|| crate::preview::preview_inspect(todo!()));
        sendable(|| crate::pty::pty_has_children(todo!(), todo!()));
        sendable(|| crate::remote::remote_app_slots(todo!()));
        sendable(|| crate::remote::remote_pair_start(todo!()));
        sendable(|| crate::remote::remote_ports_listening(todo!()));
        sendable(|| crate::remote::remote_push(todo!(), todo!(), todo!(), todo!()));
        sendable(|| crate::remote::remote_push_subscribers(todo!()));
        sendable(|| crate::remote::remote_set_enabled(todo!(), todo!()));
        sendable(|| crate::remote::remote_status(todo!()));
        sendable(|| crate::remote::remote_tailscale(todo!()));
        sendable(|| crate::remote::remote_tailscale_serve(todo!()));
        sendable(|| crate::stats::usage_stats(todo!(), todo!(), todo!(), todo!()));
        sendable(crate::voice::key::voice_key_clear);
        sendable(|| crate::voice::key::voice_key_set(todo!()));
        sendable(crate::voice::key::voice_key_status);
        sendable(|| crate::voice::models::voice_model_download(todo!(), todo!()));
        sendable(|| crate::voice::voice_start(todo!(), todo!(), todo!()));
    }
}
