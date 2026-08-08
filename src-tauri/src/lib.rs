mod board;
mod discord;
mod endpoint;
mod install;
mod instructions;
mod mcp;
mod path_env;
mod pty;
mod server;
mod sessions;
mod shells;
mod store;
mod voice;
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
        .manage(pty::PtyManager::default())
        .manage(discord::DiscordPresence::default())
        .manage(voice::VoiceManager::default())
        .setup(|app| {
            // Warmed here rather than on the first detection call: asking the
            // login shell for its PATH takes a moment, and doing it now means
            // the window is never the thing waiting on it.
            std::thread::spawn(path_env::ensure);

            let handle = app.handle().clone();
            let token = endpoint::generate_token();
            let advertised = token.clone();

            tauri::async_runtime::spawn(async move {
                match server::start(handle, ui_store(), token).await {
                    Ok(port) => {
                        // Agents discover the live port/token here rather than
                        // having them baked into their config files.
                        if let Err(err) = endpoint::publish(port, &advertised) {
                            eprintln!("could not publish endpoint: {err}");
                        }
                    }
                    Err(err) => eprintln!("board API failed to start: {err}"),
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            pty::pty_spawn,
            pty::pty_write,
            pty::pty_resize,
            pty::pty_kill,
            pty::pty_alive,
            sessions::list_sessions,
            sessions::detect_agents,
            shells::detect_shells,
            store::load_state,
            store::save_state,
            store::dir_exists,
            store::home_dir,
            workspace_file::read_code_workspace,
            workspace_file::find_code_workspace,
            board_load,
            board_create,
            board_update,
            board_delete,
            board_claim,
            board_comment,
            install::mcp_targets,
            install::mcp_install,
            install::mcp_manual_commands,
            instructions::write_agent_instructions,
            instructions::agent_instructions_status,
            discord::discord_presence_set,
            discord::discord_presence_status,
            voice::voice_start,
            voice::voice_stop,
            voice::voice_recording,
            voice::voice_unload,
            voice::models::voice_models,
            voice::models::voice_model_download,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app, event| {
            if let RunEvent::WindowEvent {
                event: WindowEvent::Destroyed,
                ..
            } = event
            {
                pty::kill_all(&app.state::<pty::PtyManager>());
                // Stale endpoint files would make agents try to reach a dead port.
                endpoint::clear();
                // Discord drops the presence when the pipe dies anyway; this
                // just takes it down before the window is gone from screen.
                app.state::<discord::DiscordPresence>().stop();
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

#[tauri::command]
fn board_comment(
    cwd: String,
    id: String,
    author: String,
    text: String,
) -> Result<board::Task, String> {
    ui_store().update(&cwd, |b| board::add_comment(b, &id, &author, &text))
}
