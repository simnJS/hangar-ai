use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

/// One live pseudo-terminal, owned by the manager for the lifetime of a pane.
struct PtySession {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    /// Cleared when the pane is killed. The reader thread outlives the kill by
    /// a moment — it can be holding a chunk read just before it — and this is
    /// what stops those last events from landing in whatever holds the id next.
    live: Arc<AtomicBool>,
}

/// Everything the manager tracks, behind one lock: an id is live, being reaped,
/// or free, and it can never be seen as free while it is still one of the
/// other two.
#[derive(Default)]
struct Registry {
    sessions: HashMap<String, PtySession>,
    /// Ids pulled out of `sessions` whose shell is still being reaped. They own
    /// their id as much as a live session does: until the old child is really
    /// gone, spawning into that id would put two shells on one pane.
    dying: HashSet<String>,
}

#[derive(Default)]
pub struct PtyManager {
    registry: Mutex<Registry>,
    /// Signalled when an id leaves `dying`, so a spawn queued behind a kill
    /// wakes as soon as the old shell is reaped instead of polling for it.
    reaped: Condvar,
}

impl PtyManager {
    fn lock(&self) -> MutexGuard<'_, Registry> {
        self.registry.lock().unwrap()
    }
}

/// Holds an id in `dying` for as long as its shell is being reaped and hands it
/// back on the way out — early return, error, or a panic inside `wait` included
/// — so a reap that goes wrong cannot strand the id for good.
struct Reaping<'a> {
    manager: &'a PtyManager,
    id: String,
}

impl Drop for Reaping<'_> {
    fn drop(&mut self) {
        // Deliberately not `unwrap`: panicking here while already unwinding
        // would abort the process instead of just failing the pane.
        if let Ok(mut registry) = self.manager.registry.lock() {
            registry.dying.remove(&self.id);
        }
        self.manager.reaped.notify_all();
    }
}

/// How long `pty_spawn` waits for the previous shell on the same id to be
/// reaped. Killing and reaping a shell is near instant; this is only here so a
/// shell that refuses to die fails one pane instead of hanging the command.
const REAP_TIMEOUT: Duration = Duration::from_secs(5);

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
/// If Hangar.AI is itself launched from inside an agent session, these leak all
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
    // An id belongs to one shell at a time, and one being reaped by `pty_kill`
    // still counts: its child is alive and its reader thread still speaks under
    // that id. A live session is a caller mistake and fails, but a dying one is
    // routine — the frontend kills and respawns a pane without awaiting the
    // kill — so it is waited out. The lock is released while waiting, here and
    // during the reap itself, so no other pane stalls behind it.
    {
        let registry = manager.lock();
        if registry.sessions.contains_key(&id) {
            return Err(format!("pty {id} already running"));
        }
        let (registry, wait) = manager
            .reaped
            .wait_timeout_while(registry, REAP_TIMEOUT, |r| r.dying.contains(&id))
            .unwrap();
        if wait.timed_out() {
            return Err(format!("pty {id} is still shutting down"));
        }
        if registry.sessions.contains_key(&id) {
            return Err(format!("pty {id} already running"));
        }
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

    let live = Arc::new(AtomicBool::new(true));
    manager.lock().sessions.insert(
        id.clone(),
        PtySession {
            master: pair.master,
            writer,
            child,
            live: live.clone(),
        },
    );

    let reader_app = app.clone();
    let reader_id = id.clone();
    let reader_live = live;
    std::thread::spawn(move || {
        let mut pending: Vec<u8> = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    // The pane may have been killed while this chunk was in
                    // flight. It belongs to nobody now, and the id may already
                    // be back in use by another pane.
                    if !reader_live.load(Ordering::Relaxed) {
                        break;
                    }
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
        // Only a shell that ended on its own has an exit to report: a killed
        // pane is already gone as far as the frontend is concerned, and the id
        // may have been handed to a new shell in the meantime.
        if reader_live.load(Ordering::Relaxed) {
            let _ = reader_app.emit(
                "pty:exit",
                PtyExit {
                    id: reader_id,
                    code: None,
                },
            );
        }
    });

    Ok(())
}

#[tauri::command]
pub fn pty_write(manager: State<'_, PtyManager>, id: String, data: String) -> Result<(), String> {
    let mut registry = manager.lock();
    let session = registry.sessions.get_mut(&id).ok_or("pty not found")?;
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
    let registry = manager.lock();
    let session = registry.sessions.get(&id).ok_or("pty not found")?;
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
    // Taken out under the lock, reaped outside it: `wait` blocks until the
    // shell is really gone, and a shell that takes its time — one still
    // tearing down a child agent — would otherwise hold the manager and
    // freeze every write and resize in the other panes. The id is booked in
    // `dying` for that window instead, so it stays taken without the lock
    // being taken with it, and a `pty_spawn` on the same id waits for the old
    // shell rather than running alongside it.
    let mut session = {
        let mut registry = manager.lock();
        // Unknown or already reaped: nothing to kill, nothing to book.
        let Some(session) = registry.sessions.remove(&id) else {
            return Ok(());
        };
        registry.dying.insert(id.clone());
        session
    };
    session.live.store(false, Ordering::Relaxed);
    let _reaping = Reaping {
        manager: &manager,
        id,
    };

    let _ = session.child.kill();
    let _ = session.child.wait();
    // Closing the master is what ends the reader thread, so it happens before
    // the id is handed back rather than at the end of the scope.
    drop(session);
    Ok(())
}

#[tauri::command]
pub fn pty_alive(manager: State<'_, PtyManager>, id: String) -> bool {
    manager.lock().sessions.contains_key(&id)
}

/// Kills every live pane. Used on window close so no shell is left behind.
pub fn kill_all(manager: &PtyManager) {
    let mut registry = manager.lock();
    for (_, mut session) in registry.sessions.drain() {
        session.live.store(false, Ordering::Relaxed);
        let _ = session.child.kill();
    }
}
