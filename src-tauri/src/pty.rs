use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

/// One live pseudo-terminal, owned by the manager for the lifetime of a pane.
struct PtySession {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    /// Cleared when the pane is killed. The reader thread outlives the kill by
    /// a moment — it can be holding a chunk read just before it — and this is
    /// what stops those last events from landing in whatever holds the id next.
    live: Arc<AtomicBool>,
    /// What the pane printed last, for a window that takes it over.
    history: Arc<Mutex<History>>,
}

/// How much of a pane's output is kept for a window that takes the pane over
/// — a workspace moved to a window of its own, or a window reloaded. Enough
/// for a screenful of an agent's interface and the scrollback above it.
const HISTORY_BYTES: usize = 256 * 1024;

/// The tail of a pane's output, and how many bytes it has printed in all.
///
/// `end` is what lets a window that subscribed to the live output before
/// asking for the history put the two together: every `pty:output` carries
/// the total it brings the pane to, so a chunk the history already holds is
/// recognised and dropped instead of printed twice.
#[derive(Default)]
struct History {
    text: String,
    end: u64,
}

impl History {
    fn push(&mut self, chunk: &str) -> u64 {
        self.text.push_str(chunk);
        self.end += chunk.len() as u64;
        // Trimmed in batches rather than on every chunk, and at a line break
        // so the replay does not start halfway through an escape sequence.
        if self.text.len() > HISTORY_BYTES * 2 {
            let mut cut = self.text.len() - HISTORY_BYTES;
            while !self.text.is_char_boundary(cut) {
                cut += 1;
            }
            let cut = self.text[cut..].find('\n').map_or(cut, |at| cut + at + 1);
            self.text.drain(..cut);
        }
        self.end
    }
}

#[derive(Clone, Serialize)]
pub struct PtyHistory {
    data: String,
    end: u64,
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
    /// Bytes the pane has printed in all, this chunk included.
    seq: u64,
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

/// The arguments a shell is started with: the ones recorded for it, plus
/// `-NoLogo` for PowerShell.
///
/// PowerShell greets every new pane with its version and, once the profile
/// takes more than half a second, with how long it took: two lines of noise in
/// a pane an agent is about to take over. `-NoLogo` drops both and changes
/// nothing else. It is added here rather than when shells are detected, so
/// panes whose shell was recorded before get it too, and it goes where it
/// cannot change the meaning of the rest: after a leading `-Login`, which
/// PowerShell honours only as its very first argument, and before everything
/// else, since all that follows `-Command` or `-File` belongs to them.
fn shell_args(program: &str, recorded: &[String]) -> Vec<String> {
    // Both separators, whatever the system: a recorded path is a string, and
    // `Path` reads a backslash as part of a name everywhere but on Windows.
    let name = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .to_ascii_lowercase();
    let stem = name.strip_suffix(".exe").unwrap_or(&name);
    let is_powershell = stem == "pwsh" || stem == "powershell";
    // PowerShell takes any unambiguous start of a parameter's name, after
    // `-`, `--` or `/`; "nol" is already NoLogo's alone.
    let names_nologo = |arg: &String| {
        let name = arg.trim_start_matches(['-', '/']).to_ascii_lowercase();
        name.len() >= 3 && "nologo".starts_with(&name)
    };
    let mut args = recorded.to_vec();
    if is_powershell && !recorded.iter().any(names_nologo) {
        let login = recorded.first().is_some_and(|first| {
            let name = first.trim_start_matches('-').to_ascii_lowercase();
            first.starts_with('-') && (name == "l" || name == "login")
        });
        args.insert(usize::from(login), "-NoLogo".to_string());
    }
    args
}

/// How many ports a worker gets from its `HANGAR_PORT_BASE` (lib/workerControl.ts).
const PORT_COUNT: u16 = 20;

/// A worker's own ports: `HANGAR_PORT_BASE` and the nineteen after it, which
/// no other pane of any workspace is given, so two workers can each run a dev
/// server without fighting over one port. Only workers have them; every other
/// pane starts without, even under a Hangar launched from inside a worker.
fn apply_ports(cmd: &mut CommandBuilder, port_base: Option<u16>) {
    match port_base {
        Some(base) => {
            cmd.env("HANGAR_PORT_BASE", base.to_string());
            cmd.env("HANGAR_PORT_COUNT", PORT_COUNT.to_string());
        }
        None => {
            cmd.env_remove("HANGAR_PORT_BASE");
            cmd.env_remove("HANGAR_PORT_COUNT");
        }
    }
}

// The arguments are the command's IPC fields, named as the frontend sends them.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub fn pty_spawn(
    app: AppHandle,
    manager: State<'_, PtyManager>,
    id: String,
    cwd: String,
    shell: Option<crate::shells::ShellInfo>,
    cols: u16,
    rows: u16,
    name: Option<String>,
    bridge: Option<bool>,
    port_base: Option<u16>,
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

    // The pane inherits our environment, so the PATH it starts with is the one
    // we were fixed with — which matters for the shells that are not opened as
    // login shells and would otherwise see the bare system PATH.
    crate::path_env::ensure();

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
    for arg in shell_args(&shell.program, &shell.args) {
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
    // Who this pane is, for an agent in it that asks Hangar to act on its own
    // pane, or signs its board claims. Set whether or not the bridge mod comes
    // along, and never inherited from a Hangar started inside another pane.
    cmd.env("HANGAR_PANE_ID", &id);
    match name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        Some(name) => cmd.env("HANGAR_PANE_NAME", name),
        None => cmd.env_remove("HANGAR_PANE_NAME"),
    }
    // Lets a Claude Code started in this pane load the hangar-bridge mod.
    if bridge != Some(false) {
        crate::bridge::apply_env(&mut cmd, &id, name.as_deref());
    }
    apply_ports(&mut cmd, port_base);

    let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

    // Dropping the slave handle is what lets the reader see EOF on exit.
    drop(pair.slave);

    let live = Arc::new(AtomicBool::new(true));
    let history = Arc::new(Mutex::new(History::default()));
    manager.lock().sessions.insert(
        id.clone(),
        PtySession {
            master: pair.master,
            writer,
            child,
            live: live.clone(),
            history: history.clone(),
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
                        // Recorded before it is sent: a window reading the
                        // history in between finds the chunk there, and drops
                        // the event when it arrives.
                        let seq = history.lock().unwrap().push(&text);
                        let _ = reader_app.emit(
                            "pty:output",
                            PtyOutput {
                                id: reader_id.clone(),
                                data: text,
                                seq,
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

/// `tree` also ends every process below the shell, which is what a pane put
/// to sleep wants: the memory is the point. On Windows the shell's own end
/// does not reach them — a process there outlives its parent — and an agent's
/// helpers (its MCP servers, a build it started) are not always tied to the
/// pane's console either. A pane closed by hand keeps the gentler kill, which
/// leaves alone what the user deliberately started out of it, an editor or a
/// browser.
#[tauri::command]
pub fn pty_kill(
    app: AppHandle,
    manager: State<'_, PtyManager>,
    id: String,
    tree: Option<bool>,
) -> Result<(), String> {
    crate::bridge::forget(&id);
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

    // Off this thread, and not as a courtesy: a Tauri command that is not
    // `async` runs on the main thread, so every millisecond spent in `wait`
    // here is a millisecond the window does not repaint, scroll or answer a
    // key. That bill is never zero on unix — `kill` sends SIGHUP first and
    // waits out a grace period, and an interactive shell installs a handler
    // for SIGHUP rather than dying on it — and a shell that hangs on the way
    // out froze the window for good.
    //
    // Nothing waits on this thread. `dying` already holds the id, and the
    // spawn that wants it back blocks on the condvar until the reap ends.
    std::thread::spawn(move || {
        let manager = app.state::<PtyManager>();
        let _reaping = Reaping {
            manager: manager.inner(),
            id,
        };

        // Listed before the shell goes, while the tree still hangs off it.
        let below = tree
            .filter(|&tree| tree)
            .and_then(|_| Some((session.child.process_id()?, crate::procs::table()?)))
            .map(|(pid, table)| crate::procs::descendants(&table, pid, crate::procs::started))
            .unwrap_or_default();
        let _ = session.child.kill();
        crate::procs::terminate(&below);
        reap_process_group(&session);
        let _ = session.child.wait();
        // Closing the master is what ends the reader thread, so it happens
        // before the id is handed back rather than at the end of the scope.
        drop(session);
    });

    Ok(())
}

/// Kills whatever the shell left running behind it.
///
/// `Child::kill` signals the shell alone, and an agent it started outlives it:
/// re-parented, still holding the pty open, still burning cpu on a pane that is
/// gone from screen. The shell owns a session of its own — the pty gave it one
/// — so its process group is exactly the pane and nothing else.
///
/// Guarded against ever naming our own group: a pty that somehow did not get
/// its own session would otherwise make this the last thing the app does.
#[cfg(unix)]
fn reap_process_group(session: &PtySession) {
    let Some(pid) = session.child.process_id() else {
        return;
    };
    unsafe {
        let group = libc::getpgid(pid as libc::pid_t);
        if group > 0 && group != libc::getpgrp() {
            libc::killpg(group, libc::SIGKILL);
        }
    }
}

#[cfg(not(unix))]
fn reap_process_group(_session: &PtySession) {}

/// The tail of a live pane's output, for a window taking the pane over.
#[tauri::command]
pub fn pty_history(manager: State<'_, PtyManager>, id: String) -> Result<PtyHistory, String> {
    let registry = manager.lock();
    let session = registry.sessions.get(&id).ok_or("pty not found")?;
    let history = session.history.lock().unwrap();
    Ok(PtyHistory {
        data: history.text.clone(),
        end: history.end,
    })
}

#[tauri::command]
pub fn pty_alive(manager: State<'_, PtyManager>, id: String) -> bool {
    manager.lock().sessions.contains_key(&id)
}

/// The panes among `ids` whose shell still runs a process of its own — a dev
/// server, a watcher, a build — which closing the pane would end. A pane that
/// is not running has nothing to lose and is left out.
///
/// Off the main thread: reading the process table is a few milliseconds on
/// Windows and a `ps` on unix, and a command that is not `async` would spend
/// them where the window repaints.
#[tauri::command]
pub async fn pty_has_children(
    manager: State<'_, PtyManager>,
    ids: Vec<String>,
) -> Result<Vec<String>, String> {
    let shells: Vec<(String, u32)> = {
        let registry = manager.lock();
        ids.into_iter()
            .filter_map(|id| {
                let pid = registry.sessions.get(&id)?.child.process_id()?;
                Some((id, pid))
            })
            .collect()
    };
    if shells.is_empty() {
        return Ok(Vec::new());
    }
    tauri::async_runtime::spawn_blocking(move || {
        // Unreadable is not empty: answering "nothing runs" without having
        // looked would let a dev server be closed.
        let table = crate::procs::table().ok_or("the process table could not be read")?;
        Ok(shells
            .into_iter()
            .filter(|(_, pid)| crate::procs::has_descendants(&table, *pid))
            .map(|(id, _)| id)
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Kills every live pane. Used on window close so no shell is left behind.
pub fn kill_all(manager: &PtyManager) {
    let mut registry = manager.lock();
    for (_, mut session) in registry.sessions.drain() {
        session.live.store(false, Ordering::Relaxed);
        let _ = session.child.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn powershell_starts_without_its_banner() {
        assert_eq!(
            shell_args(r"C:\Program Files\PowerShell\7\pwsh.exe", &[]),
            ["-NoLogo"]
        );
        assert_eq!(
            shell_args(
                r"C:\Windows\System32\WindowsPowerShell\v1.0\PowerShell.exe",
                &[]
            ),
            ["-NoLogo"]
        );
        assert_eq!(shell_args("/usr/local/bin/pwsh", &[]), ["-NoLogo"]);
    }

    #[test]
    fn nologo_never_changes_what_the_recorded_arguments_mean() {
        // Everything after -Command or -File is theirs: it goes first.
        assert_eq!(
            shell_args("pwsh", &args(&["-NoExit", "-Command", "echo hi"])),
            ["-NoLogo", "-NoExit", "-Command", "echo hi"]
        );
        // -Login counts only as the very first argument.
        assert_eq!(
            shell_args("/usr/bin/pwsh", &args(&["-l"])),
            ["-l", "-NoLogo"]
        );
        assert_eq!(
            shell_args("/usr/bin/pwsh", &args(&["-Login", "-NoExit"])),
            ["-Login", "-NoLogo", "-NoExit"]
        );
        // Asked for already, in any spelling PowerShell takes: left as it is.
        for spelling in ["-nologo", "-NoLogo", "--nologo", "/NoLogo", "-nol"] {
            assert_eq!(
                shell_args("pwsh", &args(&[spelling])),
                [spelling],
                "{spelling}"
            );
        }
        // Not a PowerShell: untouched.
        assert_eq!(
            shell_args(
                r"C:\Program Files\Git\bin\bash.exe",
                &args(&["--login", "-i"])
            ),
            ["--login", "-i"]
        );
        assert!(shell_args(r"C:\Windows\System32\cmd.exe", &[]).is_empty());
    }

    #[test]
    fn only_a_worker_starts_with_ports_of_its_own() {
        let mut worker = CommandBuilder::new("sh");
        apply_ports(&mut worker, Some(41020));
        assert_eq!(
            worker.get_env("HANGAR_PORT_BASE"),
            Some(std::ffi::OsStr::new("41020"))
        );
        assert_eq!(
            worker.get_env("HANGAR_PORT_COUNT"),
            Some(std::ffi::OsStr::new("20"))
        );

        // Inherited from a Hangar started inside a worker: not passed on.
        let mut other = CommandBuilder::new("sh");
        other.env("HANGAR_PORT_BASE", "41000");
        other.env("HANGAR_PORT_COUNT", "20");
        apply_ports(&mut other, None);
        assert_eq!(other.get_env("HANGAR_PORT_BASE"), None);
        assert_eq!(other.get_env("HANGAR_PORT_COUNT"), None);
    }

    #[test]
    fn history_keeps_a_bounded_tail_cut_at_a_line() {
        let mut history = History::default();
        let line = "é".repeat(50) + "\n";
        let mut total = 0u64;
        for _ in 0..(HISTORY_BYTES * 3 / line.len()) {
            total = history.push(&line);
        }

        assert_eq!(total, history.end);
        assert_eq!(
            history.end as usize,
            line.len() * (HISTORY_BYTES * 3 / line.len())
        );
        assert!(history.text.len() <= HISTORY_BYTES * 2);
        assert!(history.text.len() >= HISTORY_BYTES - line.len());
        // Whole lines only, so a replay never opens mid-sequence.
        assert!(history.text.starts_with('é'));
        assert!(history.text.ends_with('\n'));
    }
}
