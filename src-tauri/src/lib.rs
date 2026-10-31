mod accounts;
mod board;
mod bridge;
mod chat;
mod context;
mod control;
mod delivery;
mod discord;
mod endpoint;
mod git;
mod install;
mod instructions;
mod kit_scheme;
mod kits;
mod mcp;
mod mcp_scope;
mod memory;
mod model;
mod notify;
mod orchestrator;
mod path_env;
mod preview;
mod pricing;
mod procs;
mod pty;
mod remote;
mod server;
mod sessions;
mod shells;
mod stats;
mod store;
mod voice;
mod windows;
mod workspace_file;

use tauri::{Manager, RunEvent, WindowEvent};

/// Entry point for `hangar-ai --mcp`: no window, no Tauri runtime, just the
/// stdio protocol loop talking to whatever app instance is running.
pub fn run_mcp() {
    mcp::run_stdio();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        // Hangar at login: an option of the Mobile settings (remote/).
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(pty::PtyManager::default())
        .manage(discord::DiscordPresence::default())
        .manage(voice::VoiceManager::default())
        // The preview's files — models and their textures, images, video,
        // text, pages — read from the folders the window allowed and nowhere
        // else (model.rs, preview.rs).
        .register_asynchronous_uri_scheme_protocol(model::SCHEME, model::serve)
        // The pages of kits' tabs, isolated, and the SDK they talk to Hangar
        // with (kit_scheme.rs, docs/kits.md).
        .register_asynchronous_uri_scheme_protocol(kit_scheme::SCHEME, kit_scheme::serve)
        .setup(|app| {
            // Warmed here rather than on the first detection call: asking the
            // login shell for its PATH takes a moment, and doing it now means
            // the window is never the thing waiting on it.
            std::thread::spawn(path_env::ensure);
            // Written before the first pane asks for it, for the same reason.
            std::thread::spawn(|| {
                bridge::plugin_dir();
            });
            // `kits:changed` when a linked kit's manifest is edited on disk.
            kits::watch(app.handle().clone());
            // `accounts:changed` when a Claude account logs in or out.
            accounts::watch(app.handle().clone());

            let handle = app.handle().clone();
            let token = endpoint::generate_token();
            let advertised = token.clone();

            tauri::async_runtime::spawn(async move {
                match server::start(handle, ui_store(), token).await {
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

            // Hangar Mobile: a listener of its own, off until the person turns
            // it on, and on again at launch once they have (remote/).
            match remote::Remote::open(app.handle()) {
                Ok(remote) => {
                    app.manage(remote.clone());
                    tauri::async_runtime::spawn(async move { remote.resume().await });
                }
                Err(err) => eprintln!("Hangar Mobile is unavailable: {err}"),
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            pty::pty_spawn,
            pty::pty_write,
            pty::pty_resize,
            pty::pty_kill,
            pty::pty_alive,
            pty::pty_has_children,
            accounts::accounts_list,
            accounts::account_add,
            accounts::account_rename,
            accounts::account_remove,
            accounts::accounts_support,
            bridge::bridge_enqueue,
            bridge::bridge_cancel,
            bridge::bridge_queue,
            control::control_reply,
            windows::open_workspace_window,
            windows::focus_workspace_window,
            windows::close_workspace_window,
            pty::pty_history,
            sessions::list_sessions,
            sessions::session_exists,
            sessions::detect_agents,
            context::context_usage,
            chat::chat_page,
            stats::usage_stats,
            shells::detect_shells,
            store::load_state,
            store::save_state,
            store::dir_exists,
            store::home_dir,
            hq_dir,
            workspace_file::read_code_workspace,
            workspace_file::find_code_workspace,
            board_load,
            board_create,
            board_update,
            board_delete,
            board_claim,
            board_claim_worker,
            board_release_worker,
            board_comment,
            git::git_repo_info,
            git::git_worktree_list,
            git::git_branches,
            git::git_worktree_add,
            git::git_worktree_remove,
            git::git_worktree_prune,
            git::git_worktree_lock,
            git::git_worktree_unlock,
            git::git_fetch,
            git::git_branch_delete,
            git::git_copy_untracked,
            git::git_commits_ahead,
            git::git_changes,
            git::git_file_diff,
            delivery::delivery_preflight,
            delivery::delivery_run,
            delivery::delivery_cleanup,
            delivery::delivery_running,
            memory::memory_load,
            memory::memory_create,
            memory::memory_update,
            memory::memory_delete,
            model::model_allow_roots,
            model::model_stat,
            model::model_save_png,
            preview::preview_inspect,
            install::mcp_targets,
            install::mcp_install,
            install::mcp_manual_commands,
            mcp_scope::mcp_servers,
            mcp_scope::mcp_scope_write,
            mcp_scope::mcp_scope_clear,
            kits::kits_list,
            kits::kit_add_folder,
            kits::kit_add_git,
            kits::kit_remove,
            kits::kit_check_update,
            kits::kit_update,
            kits::kit_inspect,
            kits::kit_setup_status,
            kits::kit_setup_mark,
            kit_scheme::files::kit_file_read,
            kit_scheme::files::kit_workspace_path,
            instructions::write_agent_instructions,
            instructions::agent_instructions_status,
            notify::notify_send,
            discord::discord_presence_set,
            discord::discord_presence_status,
            voice::voice_start,
            voice::voice_stop,
            voice::voice_recording,
            voice::voice_unload,
            voice::models::voice_models,
            voice::models::voice_model_download,
            voice::key::voice_key_status,
            voice::key::voice_key_set,
            voice::key::voice_key_clear,
            remote::remote_status,
            remote::remote_set_enabled,
            remote::remote_pair_start,
            remote::remote_devices,
            remote::remote_revoke,
            remote::remote_tailscale,
            remote::remote_tailscale_serve,
            remote::remote_push,
            remote::remote_push_subscribers,
            remote::remote_options,
            remote::remote_set_keep_awake,
            remote::remote_set_autostart,
            remote::remote_ports_listening,
            remote::remote_app_slots,
            remote::activity_append,
            remote::activity_read,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app, event| {
            let RunEvent::WindowEvent { label, event, .. } = event else {
                return;
            };
            match event {
                // A workspace window hands its panes back before it goes.
                WindowEvent::CloseRequested { api, .. } if windows::is_workspace_window(&label) => {
                    api.prevent_close();
                    windows::on_close_requested(app, &label);
                }
                // Only the main window's end is the app's: it owns the store,
                // so the workspace windows go with it, and nothing is left to
                // draw the terminals. A workspace window closing leaves its
                // panes running for the main window to take back.
                WindowEvent::Destroyed if label == "main" => {
                    pty::kill_all(&app.state::<pty::PtyManager>());
                    // The history says Hangar stopped; the PC may sleep again.
                    if let Some(remote) = app.try_state::<std::sync::Arc<remote::Remote>>() {
                        remote.activity_stopped();
                    }
                    // A check or a review still running goes with the app.
                    delivery::cancel_all();
                    // Stale endpoint files would make agents try to reach a dead port.
                    endpoint::clear();
                    // Discord drops the presence when the pipe dies anyway; this
                    // just takes it down before the window is gone from screen.
                    app.state::<discord::DiscordPresence>().stop();
                    app.exit(0);
                }
                _ => {}
            }
        });
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
#[tauri::command]
fn hq_dir(app: tauri::AppHandle) -> Result<String, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|err| err.to_string())?
        .join("hq");
    std::fs::create_dir_all(&dir)
        .map_err(|err| format!("could not create {}: {err}", dir.display()))?;
    Ok(dir.to_string_lossy().into_owned())
}

#[tauri::command]
fn board_load(cwd: String) -> board::Board {
    ui_store().read(&cwd)
}

#[tauri::command]
fn board_create(cwd: String, task: board::NewTask) -> Result<board::Task, String> {
    ui_store().update(&cwd, |b| board::create_task(b, task))
}

#[tauri::command]
fn board_update(cwd: String, id: String, patch: board::TaskPatch) -> Result<board::Task, String> {
    ui_store().update(&cwd, |b| board::patch_task(b, &id, patch))
}

#[tauri::command]
fn board_delete(cwd: String, id: String) -> Result<(), String> {
    ui_store().update(&cwd, |b| board::delete_task(b, &id))
}

#[tauri::command]
fn board_claim(cwd: String, id: String, agent: String) -> Result<board::Task, String> {
    ui_store().update(&cwd, |b| board::claim_task(b, &id, &agent))
}

/// A worker's start and end (lib/workerControl.ts). Done by the window rather
/// than by an agent over the API, so the board views are told here.
#[tauri::command]
fn board_claim_worker(
    app: tauri::AppHandle,
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
    let _ = tauri::Emitter::emit(&app, "board:changed", &cwd);
    Ok(task)
}

#[tauri::command]
fn board_release_worker(
    app: tauri::AppHandle,
    cwd: String,
    id: String,
    worker: String,
    undo: bool,
) -> Result<board::Task, String> {
    let task = ui_store().update(&cwd, |b| board::release_worker(b, &id, &worker, undo))?;
    let _ = tauri::Emitter::emit(&app, "board:changed", &cwd);
    Ok(task)
}

#[tauri::command]
fn board_comment(
    cwd: String,
    id: String,
    author: String,
    text: String,
) -> Result<board::Task, String> {
    ui_store().update(&cwd, |b| board::add_comment(b, &id, &author, &text))
}
