use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Mutex;

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

/// One live pseudo-terminal, owned by the manager for the lifetime of a pane.
struct PtySession {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

#[derive(Default)]
pub struct PtyManager {
    sessions: Mutex<HashMap<String, PtySession>>,
}

#[derive(Clone, Serialize)]
struct PtyOutput {
    id: String,
    data: String,
}

#[derive(Clone, Serialize)]
struct PtyExit {
    id: String,
    code: Option<u32>,
}

/// Pulls every complete UTF-8 sequence out of `pending`, leaving a partial
/// trailing codepoint in place so it can be completed by the next read.
fn take_utf8(pending: &mut Vec<u8>) -> String {
    match std::str::from_utf8(pending) {
        Ok(s) => {
            let out = s.to_string();
            pending.clear();
            out
        }
        Err(err) => {
            let valid = err.valid_up_to();
            let out = String::from_utf8_lossy(&pending[..valid]).into_owned();
            match err.error_len() {
                // Genuinely invalid bytes: drop them and move on.
                Some(len) => {
                    pending.drain(..valid + len);
                    out + "\u{FFFD}"
                }
                // Truncated codepoint: keep the tail for the next read.
                None => {
                    pending.drain(..valid);
                    out
                }
            }
        }
    }
}

/// Session markers an agent CLI leaves in the environment of anything it spawns.
///
/// If IaBench is itself launched from inside an agent session, these leak all
/// the way down into the panes. `CLAUDE_CODE_CHILD_SESSION` in particular tells
/// Claude Code it is a nested run and to skip writing a transcript — which
/// silently breaks session capture and resume. A pane must look like a fresh
/// terminal, so we strip the markers without touching user config such as
/// `CLAUDE_CONFIG_DIR`.
const INHERITED_SESSION_MARKERS: &[&str] = &[
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDECODE",
    "CLAUDE_PID",
];

#[tauri::command]
pub fn pty_spawn(
    app: AppHandle,
    manager: State<'_, PtyManager>,
    id: String,
    cwd: String,
    shell: Option<crate::shells::ShellInfo>,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    // Re-spawning into a live id would orphan the old process.
    if manager.sessions.lock().unwrap().contains_key(&id) {
        return Err(format!("pty {id} already running"));
    }

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;

    // Fall back to the best detected shell when none was given, or when the
    // recorded one has since been uninstalled.
    let shell = shell
        .filter(|s| crate::shells::program_exists(&s.program))
        .or_else(crate::shells::default_shell)
        .ok_or("no usable shell found on this system")?;

    let mut cmd = CommandBuilder::new(&shell.program);
    for arg in &shell.args {
        cmd.arg(arg);
    }
    if std::path::Path::new(&cwd).is_dir() {
        cmd.cwd(&cwd);
    }
    // Agents render much better when they know they are on a real terminal.
    cmd.env("TERM", "xterm-256color");
    for marker in INHERITED_SESSION_MARKERS {
        cmd.env_remove(marker);
    }

    let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

    // Dropping the slave handle is what lets the reader see EOF on exit.
    drop(pair.slave);

    manager.sessions.lock().unwrap().insert(
        id.clone(),
        PtySession {
            master: pair.master,
            writer,
            child,
        },
    );

    let reader_app = app.clone();
    let reader_id = id.clone();
    std::thread::spawn(move || {
        let mut pending: Vec<u8> = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    pending.extend_from_slice(&buf[..n]);
                    let text = take_utf8(&mut pending);
                    if !text.is_empty() {
                        let _ = reader_app.emit(
                            "pty:output",
                            PtyOutput {
                                id: reader_id.clone(),
                                data: text,
                            },
                        );
                    }
                }
                Err(_) => break,
            }
        }
        let _ = reader_app.emit(
            "pty:exit",
            PtyExit {
                id: reader_id,
                code: None,
            },
        );
    });

    Ok(())
}

#[tauri::command]
pub fn pty_write(manager: State<'_, PtyManager>, id: String, data: String) -> Result<(), String> {
    let mut sessions = manager.sessions.lock().unwrap();
    let session = sessions.get_mut(&id).ok_or("pty not found")?;
    session
        .writer
        .write_all(data.as_bytes())
        .map_err(|e| e.to_string())?;
    session.writer.flush().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn pty_resize(
    manager: State<'_, PtyManager>,
    id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let sessions = manager.sessions.lock().unwrap();
    let session = sessions.get(&id).ok_or("pty not found")?;
    session
        .master
        .resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn pty_kill(manager: State<'_, PtyManager>, id: String) -> Result<(), String> {
    let mut sessions = manager.sessions.lock().unwrap();
    if let Some(mut session) = sessions.remove(&id) {
        let _ = session.child.kill();
        let _ = session.child.wait();
    }
    Ok(())
}

#[tauri::command]
pub fn pty_alive(manager: State<'_, PtyManager>, id: String) -> bool {
    manager.sessions.lock().unwrap().contains_key(&id)
}

/// Kills every live pane. Used on window close so no shell is left behind.
pub fn kill_all(manager: &PtyManager) {
    let mut sessions = manager.sessions.lock().unwrap();
    for (_, mut session) in sessions.drain() {
        let _ = session.child.kill();
    }
}
