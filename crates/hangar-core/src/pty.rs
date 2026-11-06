//! The panes' pseudo-terminals: one shell each, spawned, written to, resized
//! and killed here, its output read on a thread of its own.
//!
//! # Where a pane's output goes
//!
//! The Tauri app emitted every chunk a pane printed as a `pty:output` event to
//! every window, and each window's terminal picked its own out by id. Here a
//! pane is spawned with a sink of its own (`PtySink`), and its reader thread
//! hands the sink each chunk, then the shell's end:
//!
//! - not the host's broadcast bus (host.rs): a pane's output is most of what
//!   the backend ever says — a build log, an agent redrawing its interface
//!   sixty times a second — and is only ever for that pane. On the bus, every
//!   subscriber would be handed a copy of every pane's every chunk, and a
//!   burst of it would push the rarer events, a control request among them,
//!   out of a bus that keeps a bounded number;
//! - a callback rather than a channel made here: the app decides what a
//!   chunk turns into — bytes fed to the pane's terminal, a wake-up of the
//!   renderer, a message to its event loop — with no channel type of ours to
//!   go through first, and no queue of ours between the shell and the screen.
//!
//! The sink is called on the reader thread, so it must not block for long:
//! while it runs, the shell's output waits in the pty. It is set once, at
//! spawn: a pane belongs to the one app that spawned it, whichever of its
//! windows draws it, so a window taking a pane over routes the output inside
//! the app and reads `pty_history` for what came before.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;

mod drain;
mod scope;

/// A pane's input, shared out of the registry: `pty_write` takes a clone of
/// it under the registry's lock and writes once that lock is released, so a
/// program that does not read its input — a long paste into a busy agent, a
/// shell running `sleep` — blocks the writes to its own pane and nothing else.
type SharedWriter = Arc<Mutex<Box<dyn Write + Send>>>;

/// One live pseudo-terminal, owned by the manager for the lifetime of a pane.
struct PtySession {
    master: Box<dyn MasterPty + Send>,
    writer: SharedWriter,
    child: Box<dyn Child + Send + Sync>,
    /// Cleared when the pane is killed. The reader thread outlives the kill by
    /// a moment — it can be holding a chunk read just before it — and this is
    /// what stops those last events from landing in whatever holds the id next.
    live: Arc<AtomicBool>,
    /// What the pane printed last, for a window that takes it over.
    history: Arc<Mutex<History>>,
    /// The Claude account its Claude Code was started on (accounts.rs);
    /// `None` for a pane that is not Claude Code.
    account: Option<String>,
}

/// How much of a pane's output is kept for a window that takes the pane over
/// — a workspace moved to a window of its own, or a window reloaded. Enough
/// for a screenful of an agent's interface and the scrollback above it.
const HISTORY_BYTES: usize = 256 * 1024;

/// The tail of a pane's output, and how many bytes it has printed in all.
///
/// `end` is what lets a window that subscribed to the live output before
/// asking for the history put the two together: every `PtyOutput` carries
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

#[derive(Debug, Clone, Serialize)]
pub struct PtyHistory {
    pub data: String,
    pub end: u64,
    /// The Claude account the pane's Claude Code was started on, for a window
    /// taking it over; absent for a pane that is not Claude Code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

/// Everything the manager tracks, behind one lock: an id is starting, live,
/// being reaped, or free, and it can never be seen as free while it is still
/// one of the other three.
#[derive(Default)]
struct Registry {
    sessions: HashMap<String, PtySession>,
    /// Ids pulled out of `sessions` whose shell is still being reaped. They own
    /// their id as much as a live session does: until the old child is really
    /// gone, spawning into that id would put two shells on one pane.
    dying: HashSet<String>,
    /// Ids a `pty_spawn` has booked and is still starting a shell for, with
    /// the lock released: a second spawn of one of them fails instead of
    /// starting a second shell that would replace the first in `sessions`.
    starting: HashSet<String>,
}

#[derive(Default)]
pub struct PtyManager {
    registry: Mutex<Registry>,
    /// Signalled when an id leaves `dying`, so a spawn queued behind a kill
    /// wakes as soon as the old shell is reaped instead of polling for it.
    reaped: Condvar,
}

impl PtyManager {
    /// Never a panic for a poisoned lock: one pane's thread that panicked
    /// while holding it would otherwise fail every pane from then on.
    fn lock(&self) -> MutexGuard<'_, Registry> {
        self.registry.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Holds an id in `starting` while `pty_spawn` starts its shell, and hands it
/// back on the way out, whether the spawn succeeded or failed.
struct Starting<'a> {
    manager: &'a PtyManager,
    id: String,
}

impl Starting<'_> {
    /// The shell is up: the id goes from `starting` to `sessions` under one
    /// lock, so it is never seen free in between.
    fn install(self, session: PtySession) {
        let mut registry = self.manager.lock();
        registry.starting.remove(&self.id);
        registry.sessions.insert(self.id.clone(), session);
    }
}

impl Drop for Starting<'_> {
    fn drop(&mut self) {
        self.manager.lock().starting.remove(&self.id);
    }
}

/// Books `id` for a spawn, in one go under the lock: once booked, it is the
/// caller's until the `Starting` is installed or dropped. One still being
/// reaped is waited out (see `pty_spawn_with`); one live or being started
/// fails.
fn reserve<'a>(manager: &'a PtyManager, id: &str) -> Result<Starting<'a>, String> {
    let taken = |registry: &Registry| {
        if registry.sessions.contains_key(id) {
            Err(format!("pty {id} already running"))
        } else if registry.starting.contains(id) {
            Err(format!("pty {id} is already starting"))
        } else {
            Ok(())
        }
    };
    let registry = manager.lock();
    taken(&registry)?;
    let (mut registry, wait) = manager
        .reaped
        .wait_timeout_while(registry, REAP_TIMEOUT, |r| r.dying.contains(id))
        .unwrap_or_else(PoisonError::into_inner);
    if wait.timed_out() {
        return Err(format!("pty {id} is still shutting down"));
    }
    taken(&registry)?;
    registry.starting.insert(id.to_string());
    Ok(Starting {
        manager,
        id: id.to_string(),
    })
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

/// A chunk of what a pane printed, whole UTF-8 sequences only: the Tauri
/// window's `pty:output`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PtyOutput {
    pub id: String,
    pub data: String,
    /// Bytes the pane has printed in all, this chunk included.
    pub seq: u64,
}

/// A shell that ended on its own: the Tauri window's `pty:exit`. Never sent
/// for a pane killed with `pty_kill`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PtyExit {
    pub id: String,
    /// Always `None`: the reader sees the end of the output, not the status.
    pub code: Option<u32>,
}

/// What a pane's reader thread hands its sink.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PtyEvent {
    Output(PtyOutput),
    /// The last event a sink is handed.
    Exit(PtyExit),
}

/// Where a pane's output goes: called on the pane's reader thread, in order,
/// for as long as the shell prints (see the top of this file).
pub type PtySink = Box<dyn FnMut(PtyEvent) + Send + 'static>;

/// Decodes all of `pending`, each invalid sequence turned into U+FFFD as
/// `String::from_utf8_lossy` does, and leaves in place only a codepoint cut
/// off at the very end, for the next read to complete. What follows an
/// invalid byte is never held back: it is shown with the chunk it came in.
fn take_utf8(pending: &mut Vec<u8>) -> String {
    // The usual case, a chunk of valid text: handed over without a copy.
    let bytes = match String::from_utf8(std::mem::take(pending)) {
        Ok(text) => return text,
        Err(err) => err.into_bytes(),
    };
    let mut out = String::with_capacity(bytes.len());
    let mut rest = &bytes[..];
    loop {
        match std::str::from_utf8(rest) {
            Ok(text) => {
                out.push_str(text);
                rest = &[];
                break;
            }
            Err(err) => {
                let (valid, after) = rest.split_at(err.valid_up_to());
                out.push_str(&String::from_utf8_lossy(valid));
                match err.error_len() {
                    // Genuinely invalid bytes: one replacement, and on.
                    Some(len) => {
                        out.push('\u{FFFD}');
                        rest = &after[len..];
                    }
                    // A codepoint cut off by the end of the read.
                    None => {
                        rest = after;
                        break;
                    }
                }
            }
        }
    }
    *pending = rest.to_vec();
    out
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

/// Variables Hangar set in its own environment for itself alone, which no
/// pane inherits: the app's `WGPU_BACKEND` (hangar-app's app/gpu.rs), a
/// choice for its own drawing. One the person exported before starting
/// Hangar is never listed here, and reaches the panes as before.
static APP_ONLY_ENV: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

/// Keeps `name` out of every pane spawned from now on: the app set it for
/// itself, the person did not.
pub fn keep_from_panes(name: &'static str) {
    let mut names = APP_ONLY_ENV.lock().unwrap_or_else(PoisonError::into_inner);
    if !names.contains(&name) {
        names.push(name);
    }
}

/// Removes from a pane's environment what only the app was meant to have.
fn apply_app_only_env(cmd: &mut CommandBuilder, names: &[&str]) {
    for name in names {
        cmd.env_remove(name);
    }
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

/// The variables of the workspace's shared build cache (build_cache.rs), for a
/// worker only: it is the pane that has ports of its own. Nothing is removed
/// from the others: the person's own `CARGO_TARGET_DIR` stays theirs.
fn apply_build_cache(cmd: &mut CommandBuilder, port_base: Option<u16>, env: &[(String, String)]) {
    if port_base.is_some() {
        for (name, value) in env {
            cmd.env(name, value);
        }
    }
}

/// The `--mcp-config` file a Claude Code pane starts with, written just
/// before (lib/mcpScope): the command typed into the shell names this variable
/// instead of the path, which holds the user's name and would stay on screen.
/// Never inherited from a Hangar started inside a pane.
fn apply_mcp_config(cmd: &mut CommandBuilder, path: Option<&str>) {
    match path.filter(|path| !path.is_empty()) {
        Some(path) => cmd.env("HANGAR_MCP_CONFIG", path),
        None => cmd.env_remove("HANGAR_MCP_CONFIG"),
    }
}

/// What a pane's Claude Code loads as plugins: the hangar-bridge mod when it
/// comes along, then the plugins of the workspace's kits — which do not
/// depend on the bridge, and come with it off too. A machine whose policy
/// forbids sideloading gets neither (bridge.rs); the kits it kept out are
/// reported, and the pane starts all the same.
///
/// A pane with kits also learns where their data lives, for a hook to find
/// `$HANGAR_KITS_DATA/<kit id>`; any other pane starts without the variable,
/// even under a Hangar launched from inside a pane.
fn apply_plugins(
    cmd: &mut CommandBuilder,
    bridge_dir: Option<&std::path::Path>,
    kits: crate::kits::Launch,
    forbidden: bool,
) -> Vec<crate::kits::LaunchIssue> {
    match &kits.data_root {
        Some(root) => cmd.env(crate::kits::data::ENV_VAR, root),
        None => cmd.env_remove(crate::kits::data::ENV_VAR),
    }
    let mut issues = kits.issues;
    let mut ours: Vec<&std::path::Path> = bridge_dir.into_iter().collect();
    for (id, dir) in &kits.plugins {
        if forbidden {
            issues.push(crate::kits::LaunchIssue::new(
                id,
                crate::kits::IssueReason::Forbidden,
                None,
            ));
        } else {
            ours.push(dir);
        }
    }
    crate::bridge::apply_plugin_dirs(cmd, &ours);
    issues
}

/// The loopback API as pane `id` reaches it, when something in the pane will
/// (`wanted`: the bridge mod, a kit's hook) and the API is listening (`port`):
/// its port and the pane's own token (server.rs, `pane_tokens`), never
/// Hangar's, so that the API takes whatever the pane asks as the pane's. The
/// same id gets the same token until the pane is closed for good, so a
/// restart or a hibernation that brings the id back keeps it.
fn pane_api(
    port: Option<u16>,
    id: &str,
    name: Option<&str>,
    wanted: bool,
) -> Option<(u16, String)> {
    let port = port.filter(|_| wanted)?;
    Some((port, crate::server::pane_tokens().issue(id, name)))
}

/// What a hook of a kit, which has no MCP, needs to reach Hangar from its pane
/// (docs/kits.md): the loopback API's URL and the pane's token, beside the `HANGAR_PANE_ID`
/// every pane has. A pane with kits gets them whether or not the bridge mod comes
/// along; any other starts without them, even under a Hangar launched from inside
/// a pane, whose token would be that Hangar's.
fn apply_kit_api(cmd: &mut CommandBuilder, has_kits: bool, endpoint: Option<(u16, &str)>) {
    match endpoint.filter(|_| has_kits) {
        Some((port, token)) => {
            cmd.env("HANGAR_API_URL", format!("http://127.0.0.1:{port}"));
            cmd.env("HANGAR_API_TOKEN", token);
        }
        None => {
            cmd.env_remove("HANGAR_API_URL");
            cmd.env_remove("HANGAR_API_TOKEN");
        }
    }
}

/// Whether a pane's shell is running here, whatever the windows make of it: the
/// one thing a caller who says it is that pane can be held to.
pub fn pane_alive(manager: &PtyManager, id: &str) -> bool {
    manager.lock().sessions.contains_key(id)
}

/// A Claude Code pane on a profile runs with `CLAUDE_CONFIG_DIR` naming the
/// profile's folder; every other pane, and one on the main account, keeps
/// the environment Hangar has (accounts.rs).
fn apply_account(cmd: &mut CommandBuilder, agent: Option<&str>, dir: Option<&std::path::Path>) {
    for (name, value) in crate::accounts::env_for(agent, dir) {
        match value {
            Some(value) => cmd.env(name, value),
            None => cmd.env_remove(name),
        }
    }
}

/// What a spawn tells the pane besides succeeding.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Spawned {
    /// Kits the pane was given that stay out of it, and why.
    pub kit_issues: Vec<crate::kits::LaunchIssue>,
    /// The Claude account the pane's Claude Code runs on: the one asked for,
    /// or main when it is gone. Absent for a pane that is not Claude Code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// What the account's profile could not take from the main account.
    pub account_issues: Vec<crate::accounts::Issue>,
}

/// How many live panes were started on `account`.
pub fn panes_on(manager: &PtyManager, account: &str) -> usize {
    manager
        .lock()
        .sessions
        .values()
        .filter(|session| session.account.as_deref() == Some(account))
        .count()
}

// The arguments after `output` are the command's IPC fields, named as the
// frontend sent them.
#[allow(clippy::too_many_arguments)]
pub fn pty_spawn(
    manager: &PtyManager,
    output: PtySink,
    id: String,
    cwd: String,
    shell: Option<crate::shells::ShellInfo>,
    cols: u16,
    rows: u16,
    name: Option<String>,
    bridge: Option<bool>,
    port_base: Option<u16>,
    mcp_config: Option<String>,
    kits: Option<Vec<String>>,
    agent: Option<String>,
    claude_account: Option<String>,
) -> Result<Spawned, String> {
    pty_spawn_with(
        manager,
        output,
        id,
        cwd,
        shell,
        cols,
        rows,
        name,
        bridge,
        port_base,
        Vec::new(),
        mcp_config,
        kits,
        agent,
        claude_account,
        false,
    )
}

/// `pty_spawn` with the workspace's build cache environment
/// (`build_cache::env_for`), applied to a worker's pane only: one without
/// `port_base` starts without it, whatever is passed.
///
/// `scoped`: the `pane-scopes` experiment is on, and the shell is moved into
/// a systemd user scope of its own once started (Linux; see `pty/scope.rs`).
/// Nothing else about the pane changes, and a scope that cannot be made
/// leaves it where it would have been.
#[allow(clippy::too_many_arguments)]
pub fn pty_spawn_with(
    manager: &PtyManager,
    output: PtySink,
    id: String,
    cwd: String,
    shell: Option<crate::shells::ShellInfo>,
    cols: u16,
    rows: u16,
    name: Option<String>,
    bridge: Option<bool>,
    port_base: Option<u16>,
    build_env: Vec<(String, String)>,
    mcp_config: Option<String>,
    kits: Option<Vec<String>>,
    agent: Option<String>,
    claude_account: Option<String>,
    scoped: bool,
) -> Result<Spawned, String> {
    // An id belongs to one shell at a time, and one being reaped by `pty_kill`
    // still counts: its child is alive and its reader thread still speaks under
    // that id. A live session, or one another spawn is starting, is a caller
    // mistake and fails, but a dying one is routine — the frontend kills and
    // respawns a pane without awaiting the kill — so it is waited out. The
    // lock is released while waiting, while the shell starts and during the
    // reap itself, so no other pane stalls behind it; the id stays booked in
    // `starting` meanwhile.
    let slot = reserve(manager, &id)?;

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
    // The API as this pane reaches it: its port, and a token of the pane's
    // own, never Hangar's, so that what the pane asks is done as the pane.
    let kits = kits.unwrap_or_default();
    let endpoint = pane_api(
        crate::bridge::endpoint().map(|(port, _)| port),
        &id,
        name.as_deref(),
        bridge != Some(false) || !kits.is_empty(),
    );
    let endpoint = endpoint
        .as_ref()
        .map(|(port, token)| (*port, token.as_str()));
    // Lets a Claude Code started in this pane load the hangar-bridge mod, and
    // the plugins of its workspace's kits.
    let bridge_dir = if bridge != Some(false) {
        crate::bridge::apply_env(&mut cmd, endpoint)
    } else {
        None
    };
    apply_kit_api(&mut cmd, !kits.is_empty(), endpoint);
    let kit_issues = apply_plugins(
        &mut cmd,
        bridge_dir,
        crate::kits::launch(&kits),
        crate::bridge::sideloading_forbidden(),
    );
    apply_ports(&mut cmd, port_base);
    apply_app_only_env(
        &mut cmd,
        &APP_ONLY_ENV.lock().unwrap_or_else(PoisonError::into_inner),
    );
    apply_build_cache(&mut cmd, port_base, &build_env);
    apply_mcp_config(&mut cmd, mcp_config.as_deref());
    // The account's profile is brought level with the main account before
    // its Claude Code starts on it.
    let launch = crate::accounts::for_launch(agent.as_deref(), claude_account.as_deref());
    apply_account(
        &mut cmd,
        agent.as_deref(),
        launch.as_ref().and_then(|launch| launch.dir.as_deref()),
    );
    let (account, account_issues) = match launch {
        Some(launch) => (Some(launch.id), launch.issues),
        None => (None, Vec::new()),
    };

    let mut child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    let ends = (
        drain::reader(&*pair.master),
        pair.master.take_writer().map_err(|e| e.to_string()),
    );
    let (mut reader, writer) = match ends {
        (Ok(reader), Ok(writer)) => (reader, writer),
        // The shell runs already: not left behind with no pane to own it.
        (Err(e), _) | (_, Err(e)) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
    };

    // Dropping the slave handle is what lets the reader see EOF on exit.
    drop(pair.slave);

    // Its own scope, before the shell gets far enough to start anything:
    // what it forks from then on is born there.
    scope::place(
        scoped,
        child.process_id(),
        &id,
        name.as_deref(),
        start_scope,
        |message| eprintln!("{message}"),
    );

    let live = Arc::new(AtomicBool::new(true));
    let history = Arc::new(Mutex::new(History::default()));
    slot.install(PtySession {
        master: pair.master,
        writer: Arc::new(Mutex::new(writer)),
        child,
        live: live.clone(),
        history: history.clone(),
        account: account.clone(),
    });

    let mut output = output;
    let reader_id = id.clone();
    let reader_live = live;
    // Named after the pane, so a profile or `top -H` tells the panes apart
    // (the kernel keeps the first 15 bytes): unnamed, it would carry the name
    // of the thread that spawned it, a worker of tokio's blocking pool.
    let reader_thread = std::thread::Builder::new().name(format!("pty-{id}"));
    let spawned = reader_thread.spawn(move || {
        let mut pending: Vec<u8> = Vec::new();
        // Each event carries what the PTY held when it was read, up to
        // `drain::MOST`: a flood in a few big pieces rather than a line
        // each, and a lone echo at once all the same.
        let mut buf = vec![0u8; drain::MOST];
        loop {
            let open = drain::read_some(&mut *reader, &mut buf, &mut pending);
            // The pane may have been killed while this chunk was in
            // flight. It belongs to nobody now, and the id may already
            // be back in use by another pane.
            if !reader_live.load(Ordering::Relaxed) {
                break;
            }
            let text = take_utf8(&mut pending);
            if !text.is_empty() {
                emit(&mut output, &history, &reader_id, text);
            }
            if !open {
                break;
            }
        }
        // Only a shell that ended on its own has an exit to report: a killed
        // pane is already gone as far as the frontend is concerned, and the id
        // may have been handed to a new shell in the meantime.
        if reader_live.load(Ordering::Relaxed) {
            // A codepoint the output ended in the middle of: shown as what it
            // is rather than dropped.
            if !pending.is_empty() {
                let text = String::from_utf8_lossy(&pending).into_owned();
                emit(&mut output, &history, &reader_id, text);
            }
            output(PtyEvent::Exit(PtyExit {
                id: reader_id,
                code: None,
            }));
        }
    });
    // As `std::thread::spawn` would: a process that cannot start a thread
    // any more has bigger problems than this pane.
    spawned.expect("a thread to read the pane's output");

    Ok(Spawned {
        kit_issues,
        account,
        account_issues,
    })
}

/// The user manager, asked for a pane's scope.
#[cfg(target_os = "linux")]
fn start_scope(unit: &str, description: &str, pid: u32) -> Result<(), String> {
    scope::start_transient(unit, description, pid)
}

#[cfg(not(target_os = "linux"))]
fn start_scope(_unit: &str, _description: &str, _pid: u32) -> Result<(), String> {
    Err("systemd scopes are Linux's".into())
}

/// Records a chunk in the pane's history, then hands it to its sink:
/// recorded before it is sent, so a window reading the history in between
/// finds the chunk there and drops the event when it arrives.
fn emit(output: &mut PtySink, history: &Mutex<History>, id: &str, text: String) {
    let seq = history
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(&text);
    output(PtyEvent::Output(PtyOutput {
        id: id.to_string(),
        data: text,
        seq,
    }));
}

/// Writes to a pane's input. Blocks while the pane's program does not read —
/// the pty's buffer is a few kilobytes — but only the writes to that pane:
/// the registry's lock is released before the write starts (`SharedWriter`).
/// Writes to one pane still go one at a time, in the order they come.
pub fn pty_write(manager: &PtyManager, id: String, data: String) -> Result<(), String> {
    let writer = manager
        .lock()
        .sessions
        .get(&id)
        .ok_or("pty not found")?
        .writer
        .clone();
    let mut writer = writer.lock().unwrap_or_else(PoisonError::into_inner);
    writer
        .write_all(data.as_bytes())
        .map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())
}

pub fn pty_resize(manager: &PtyManager, id: String, cols: u16, rows: u16) -> Result<(), String> {
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
///
/// The pane's prompt inbox (bridge.rs) is kept: a restart, a role restart and
/// a hibernation all come through here and bring the same id back, and what
/// was queued for it is still for it. A pane closed for good gives its inbox
/// up with `bridge::forget`.
///
/// The manager comes as an `Arc` because the reap outlives the call, on a
/// thread of its own that holds on to it.
pub fn pty_kill(manager: &Arc<PtyManager>, id: String, tree: Option<bool>) -> Result<(), String> {
    // Who edited what is the session's, which ends here; a respawned one
    // starts its own.
    crate::bridge::release_edits(&id);
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

    // Off the caller's thread, and not as a courtesy: the caller may be the
    // one the window repaints on, and every millisecond spent here would be
    // a millisecond the window does not scroll or answer a key. That bill is
    // never zero on unix — the pane's programs are given `HANGUP_GRACE` to
    // end on the hangup — and a shell that hangs on the way out would freeze
    // it for good.
    //
    // Nothing waits on this thread. `dying` already holds the id, and the
    // spawn that wants it back blocks on the condvar until the reap ends.
    let manager = manager.clone();
    std::thread::spawn(move || {
        let _reaping = Reaping {
            manager: &manager,
            id,
        };
        end(std::slice::from_mut(&mut session), tree == Some(true));
        let _ = session.child.wait();
        // Closing the master is what ends the reader thread, so it happens
        // before the id is handed back rather than at the end of the scope.
        drop(session);
    });

    Ok(())
}

/// How long the programs of the panes being ended get, all at once, to end
/// on the hangup — Claude Code writing the last of its transcript — before
/// what is left is killed outright. Polled: panes that end sooner are not
/// waited for.
#[cfg(unix)]
const HANGUP_GRACE: Duration = Duration::from_millis(300);
#[cfg(unix)]
const HANGUP_POLL: Duration = Duration::from_millis(20);

/// A pane's process group (unix), or nothing.
#[cfg(unix)]
type Group = libc::pid_t;
#[cfg(not(unix))]
type Group = i32;

/// A pane on its way out, with what to end beside its shell, read while the
/// shell still runs.
struct Ending<'a> {
    session: &'a mut PtySession,
    group: Option<Group>,
    /// The processes below the shell, each with its start time where the
    /// system tells it (`procs::started`).
    below: Vec<(u32, Option<u64>)>,
    /// The shell is gone and reaped: its pid is no longer ours to signal.
    exited: bool,
}

/// Ends panes taken out of the registry: each one's group is hung up, as a
/// terminal that closes would, and given `HANGUP_GRACE` (in all, not each) to
/// end; then what is left — what ignored the hangup, an agent started with
/// `nohup` — is killed outright, the group, the processes listed below each
/// shell with `tree`, and the shell. Does not wait for the shells.
fn end(sessions: &mut [PtySession], tree: bool) {
    let table = if tree { crate::procs::table() } else { None };
    let mut ending: Vec<Ending<'_>> = sessions
        .iter_mut()
        .map(|session| {
            // Listed before anything goes, while the tree still hangs off
            // the shell. So is the group: a reaped shell's group can no
            // longer be asked for.
            let below = match (&table, session.child.process_id()) {
                (Some(table), Some(pid)) => {
                    crate::procs::descendants(table, pid, crate::procs::started)
                        .into_iter()
                        .map(|pid| (pid, crate::procs::started(pid)))
                        .collect()
                }
                _ => Vec::new(),
            };
            let group = process_group(session);
            Ending {
                session,
                group,
                below,
                exited: false,
            }
        })
        .collect();

    hang_up(&mut ending);

    for pane in &ending {
        kill_group(pane.group);
        // A process whose start time no longer matches is another one that
        // was given its id since it was listed.
        let still: Vec<u32> = pane
            .below
            .iter()
            .filter(|(pid, born)| crate::procs::started(*pid) == *born)
            .map(|(pid, _)| *pid)
            .collect();
        crate::procs::terminate(&still);
    }
    for pane in &mut ending {
        if !pane.exited {
            let _ = pane.session.child.kill();
        }
    }
}

/// Hangs up every pane's group, then polls until each pane's shell has ended
/// and its group is empty, for `HANGUP_GRACE` at most.
#[cfg(unix)]
fn hang_up(ending: &mut [Ending<'_>]) {
    for pane in ending.iter() {
        match pane.group {
            Some(group) => signal_group(Some(group), libc::SIGHUP),
            // No group of its own: the shell alone, still ours to signal.
            None => {
                if let Some(pid) = pane.session.child.process_id() {
                    unsafe {
                        libc::kill(pid as libc::pid_t, libc::SIGHUP);
                    }
                }
            }
        }
    }
    let deadline = std::time::Instant::now() + HANGUP_GRACE;
    loop {
        let mut over = true;
        for pane in ending.iter_mut() {
            if !pane.exited {
                pane.exited = matches!(pane.session.child.try_wait(), Ok(Some(_)));
            }
            over &= pane.exited && !group_alive(pane.group);
        }
        if over || std::time::Instant::now() >= deadline {
            return;
        }
        std::thread::sleep(HANGUP_POLL);
    }
}

/// Windows has no hangup to send: the shells are terminated straight away.
#[cfg(not(unix))]
fn hang_up(_ending: &mut [Ending<'_>]) {}

/// A pane's process group, to kill whatever the shell left running behind it.
///
/// `Child::kill` signals the shell alone, and an agent it started outlives it:
/// re-parented, still holding the pty open, still burning cpu on a pane that is
/// gone from screen. The shell owns a session of its own — the pty gave it one
/// — so its process group is exactly the pane and nothing else.
///
/// Read while the shell is still there, since a reaped shell's group can no
/// longer be asked for. Guarded against ever naming our own group: a pty that
/// somehow did not get its own session would otherwise make the kill the last
/// thing the app does.
#[cfg(unix)]
fn process_group(session: &PtySession) -> Option<Group> {
    let pid = session.child.process_id()?;
    let group = unsafe { libc::getpgid(pid as libc::pid_t) };
    (group > 0 && group != unsafe { libc::getpgrp() }).then_some(group)
}

#[cfg(not(unix))]
fn process_group(_session: &PtySession) -> Option<Group> {
    None
}

#[cfg(unix)]
fn signal_group(group: Option<Group>, signal: libc::c_int) {
    if let Some(group) = group {
        unsafe {
            libc::killpg(group, signal);
        }
    }
}

/// Whether anything is left in a pane's group.
#[cfg(unix)]
fn group_alive(group: Option<Group>) -> bool {
    group.is_some_and(|group| unsafe { libc::killpg(group, 0) } == 0)
}

/// Kills what is left of a pane's group, outright.
#[cfg(unix)]
fn kill_group(group: Option<Group>) {
    signal_group(group, libc::SIGKILL);
}

#[cfg(not(unix))]
fn kill_group(_group: Option<Group>) {}

/// The tail of a live pane's output, for a window taking the pane over.
pub fn pty_history(manager: &PtyManager, id: String) -> Result<PtyHistory, String> {
    let registry = manager.lock();
    let session = registry.sessions.get(&id).ok_or("pty not found")?;
    let history = session.history.lock().unwrap();
    Ok(PtyHistory {
        data: history.text.clone(),
        end: history.end,
        account: session.account.clone(),
    })
}

pub fn pty_alive(manager: &PtyManager, id: String) -> bool {
    manager.lock().sessions.contains_key(&id)
}

/// The panes among `ids` whose shell still runs a process of its own — a dev
/// server, a watcher, a build — which closing the pane would end. A pane that
/// is not running has nothing to lose and is left out.
///
/// Off the main thread: reading the process table is a few milliseconds on
/// Windows and a `ps` on unix, and a command that is not `async` would spend
/// them where the window repaints.
pub async fn pty_has_children(
    manager: Arc<PtyManager>,
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
    tokio::task::spawn_blocking(move || {
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

/// How long quitting waits for the panes `pty_kill` is still reaping.
const QUIT_REAP_WAIT: Duration = Duration::from_secs(1);

/// Kills every live pane, with everything running in it: the app is ending
/// (`Core::shutdown`), and nothing a pane started is to outlive it.
///
/// As `pty_kill` with `tree`, for all the panes at once (`end`): hung up
/// together and given one short grace, so Claude Code can write the last of
/// its transcript and quitting stays quick, then killed — their groups, the
/// shells, and the processes below each shell read off one process table,
/// which is what reaches, on Windows, the ones a pane's console does not.
/// Then the panes `pty_kill` was still reaping are waited for, a bounded
/// moment, so their own last kill is not cut short by the app's exit.
pub fn kill_all(manager: &PtyManager) {
    // Out of the registry and its lock: the kills wait, and nothing else
    // needs the panes now.
    let mut sessions: Vec<PtySession> = manager
        .lock()
        .sessions
        .drain()
        .map(|(_, session)| session)
        .collect();
    for session in &sessions {
        session.live.store(false, Ordering::Relaxed);
    }
    end(&mut sessions, true);
    drop(sessions);
    let registry = manager.lock();
    let _ = manager
        .reaped
        .wait_timeout_while(registry, QUIT_REAP_WAIT, |r| !r.dying.is_empty());
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
    fn only_a_worker_carries_the_build_cache_variables() {
        let env = vec![(
            "CARGO_TARGET_DIR".to_string(),
            "C:/b/1234abcd/target".to_string(),
        )];
        let mut worker = CommandBuilder::new("sh");
        apply_build_cache(&mut worker, Some(41020), &env);
        assert_eq!(
            worker.get_env("CARGO_TARGET_DIR"),
            Some(std::ffi::OsStr::new("C:/b/1234abcd/target"))
        );

        // Not a worker: not given.
        let mut other = CommandBuilder::new("sh");
        apply_build_cache(&mut other, None, &env);
        assert_ne!(
            other.get_env("CARGO_TARGET_DIR"),
            Some(std::ffi::OsStr::new("C:/b/1234abcd/target"))
        );

        // A worker whose workspace turned the cache off.
        let mut off = CommandBuilder::new("sh");
        apply_build_cache(&mut off, Some(41020), &[]);
        assert_ne!(
            off.get_env("CARGO_TARGET_DIR"),
            Some(std::ffi::OsStr::new("C:/b/1234abcd/target"))
        );
    }

    #[test]
    fn the_mcp_config_file_is_handed_to_the_shell_and_never_inherited() {
        let path = r"C:\Users\Jo Doe\AppData\Roaming\com.simnjs.hangar\mcp\ws.json";
        let mut pane = CommandBuilder::new("pwsh");
        apply_mcp_config(&mut pane, Some(path));
        assert_eq!(
            pane.get_env("HANGAR_MCP_CONFIG"),
            Some(std::ffi::OsStr::new(path))
        );

        for none in [None, Some("")] {
            let mut other = CommandBuilder::new("sh");
            other.env("HANGAR_MCP_CONFIG", "/elsewhere.json");
            apply_mcp_config(&mut other, none);
            assert_eq!(other.get_env("HANGAR_MCP_CONFIG"), None);
        }
    }

    #[test]
    fn only_a_claude_code_pane_on_a_profile_gets_its_config_dir() {
        let profile = std::path::Path::new("/home/me/.hangar/accounts/work");
        let inherited = "/home/me/custom-claude";

        let mut claude = CommandBuilder::new("pwsh");
        claude.env("CLAUDE_CONFIG_DIR", inherited);
        claude.env("CLAUDE_SECURESTORAGE_CONFIG_DIR", inherited);
        apply_account(&mut claude, Some("claude"), Some(profile));
        assert_eq!(
            claude.get_env("CLAUDE_CONFIG_DIR"),
            Some(profile.as_os_str())
        );
        assert_eq!(claude.get_env("CLAUDE_SECURESTORAGE_CONFIG_DIR"), None);

        // The main account, and a shell, keep what Hangar has.
        for (agent, dir) in [
            (Some("claude"), None),
            (Some("shell"), Some(profile)),
            (None, Some(profile)),
        ] {
            let mut other = CommandBuilder::new("pwsh");
            other.env("CLAUDE_CONFIG_DIR", inherited);
            apply_account(&mut other, agent, dir);
            assert_eq!(
                other.get_env("CLAUDE_CONFIG_DIR"),
                Some(std::ffi::OsStr::new(inherited)),
                "{agent:?}"
            );
        }
    }

    fn plugin_dirs(cmd: &CommandBuilder) -> Option<String> {
        cmd.get_env(crate::bridge::PLUGIN_DIRS_VAR)
            .map(|value| value.to_string_lossy().into_owned())
    }

    fn kits(plugins: &[(&str, &str)]) -> crate::kits::Launch {
        crate::kits::Launch {
            plugins: plugins
                .iter()
                .map(|(id, dir)| (id.to_string(), std::path::PathBuf::from(dir)))
                .collect(),
            ..crate::kits::Launch::default()
        }
    }

    fn kits_data(cmd: &CommandBuilder) -> Option<String> {
        cmd.get_env(crate::kits::data::ENV_VAR)
            .map(|value| value.to_string_lossy().into_owned())
    }

    fn env_of(cmd: &CommandBuilder, name: &str) -> Option<String> {
        cmd.get_env(name)
            .map(|value| value.to_string_lossy().into_owned())
    }

    #[test]
    fn a_variable_the_app_set_for_itself_stays_out_of_the_panes() {
        let mut pane = CommandBuilder::new("sh");
        pane.env("WGPU_BACKEND", "vulkan");
        apply_app_only_env(&mut pane, &["WGPU_BACKEND"]);
        assert_eq!(env_of(&pane, "WGPU_BACKEND"), None);
    }

    #[test]
    fn a_variable_the_person_exported_reaches_the_panes() {
        // Hangar set nothing itself: the list is empty, the person's value stays.
        let mut pane = CommandBuilder::new("sh");
        pane.env("WGPU_BACKEND", "gl");
        apply_app_only_env(&mut pane, &[]);
        assert_eq!(env_of(&pane, "WGPU_BACKEND").as_deref(), Some("gl"));
    }

    #[test]
    fn a_name_kept_from_the_panes_is_listed_once() {
        keep_from_panes("HANGAR_TEST_APP_ONLY");
        keep_from_panes("HANGAR_TEST_APP_ONLY");
        let names = APP_ONLY_ENV.lock().unwrap();
        assert_eq!(
            names
                .iter()
                .filter(|name| **name == "HANGAR_TEST_APP_ONLY")
                .count(),
            1
        );
    }

    #[test]
    fn a_pane_with_kits_can_reach_the_api_with_the_bridge_on_or_off() {
        let mut cmd = CommandBuilder::new("sh");
        // The bridge off for this pane: none of its variables. Nothing here reads them.
        cmd.env_remove("HANGAR_BRIDGE_URL");
        cmd.env_remove("HANGAR_BRIDGE_TOKEN");
        apply_kit_api(&mut cmd, true, Some((4242, "secret")));
        assert_eq!(
            env_of(&cmd, "HANGAR_API_URL").as_deref(),
            Some("http://127.0.0.1:4242")
        );
        assert_eq!(env_of(&cmd, "HANGAR_API_TOKEN").as_deref(), Some("secret"));
        assert_eq!(env_of(&cmd, "HANGAR_BRIDGE_URL"), None);
        assert_eq!(env_of(&cmd, "HANGAR_BRIDGE_TOKEN"), None);
    }

    #[test]
    fn a_pane_without_kits_gets_no_token_even_one_it_would_inherit() {
        let mut bare = CommandBuilder::new("sh");
        apply_kit_api(&mut bare, false, Some((4242, "secret")));
        assert_eq!(env_of(&bare, "HANGAR_API_URL"), None);
        assert_eq!(env_of(&bare, "HANGAR_API_TOKEN"), None);

        // A Hangar started inside a pane of another: that one's are not this one's.
        for (has_kits, endpoint) in [(false, Some((4242, "secret"))), (true, None)] {
            let mut inherited = CommandBuilder::new("sh");
            inherited.env("HANGAR_API_URL", "http://127.0.0.1:1");
            inherited.env("HANGAR_API_TOKEN", "the other one");
            apply_kit_api(&mut inherited, has_kits, endpoint);
            assert_eq!(env_of(&inherited, "HANGAR_API_URL"), None);
            assert_eq!(env_of(&inherited, "HANGAR_API_TOKEN"), None);
        }
    }

    #[test]
    fn a_pane_s_environment_carries_its_own_token_and_not_hangar_s() {
        let tokens = crate::server::pane_tokens();
        let api = pane_api(Some(4242), "p-own-token", Some("Ada"), true);
        let (port, token) = api.clone().expect("the API is listening");
        assert_eq!(port, 4242);
        // The pane's own: the one the API knows it by, and no other pane's.
        assert_eq!(token, tokens.issue("p-own-token", Some("Ada")));
        assert_ne!(token, tokens.issue("p-own-token-other", None));
        let mut cmd = CommandBuilder::new("sh");
        apply_kit_api(&mut cmd, true, api.as_ref().map(|(p, t)| (*p, t.as_str())));
        assert_eq!(env_of(&cmd, "HANGAR_API_TOKEN"), Some(token.clone()));

        // A respawn on the same id (a restart, a hibernation) keeps it.
        let again = pane_api(Some(4242), "p-own-token", Some("Ada"), true);
        assert_eq!(again.map(|(_, t)| t), Some(token.clone()));

        // Closed for good, the next pane on that id is somebody else.
        tokens.revoke("p-own-token");
        let after = pane_api(Some(4242), "p-own-token", Some("Ada"), true);
        assert_ne!(after.map(|(_, t)| t), Some(token));

        // Nothing in the pane would use one, or the API is not listening.
        assert_eq!(pane_api(Some(4242), "p-own-token-none", None, false), None);
        assert_eq!(pane_api(None, "p-own-token-none", None, true), None);
        tokens.revoke("p-own-token");
        tokens.revoke("p-own-token-other");
    }

    #[test]
    fn a_pane_nobody_started_is_not_alive() {
        assert!(!pane_alive(&PtyManager::default(), "p-unknown"));
        assert!(!pane_alive(&PtyManager::default(), ""));
    }

    #[test]
    fn a_pane_with_kits_finds_their_data_with_the_bridge_or_without_it() {
        let root = std::path::PathBuf::from("/home/me/.hangar/kit-data");
        let bridge = std::path::Path::new("/hangar/mods/hangar-bridge");
        let with_root = || crate::kits::Launch {
            data_root: Some(root.clone()),
            ..kits(&[("studio", "/kits/studio")])
        };
        let mut with_bridge = CommandBuilder::new("pwsh");
        apply_plugins(&mut with_bridge, Some(bridge), with_root(), false);
        assert_eq!(kits_data(&with_bridge).as_deref(), root.to_str());

        let mut without = CommandBuilder::new("pwsh");
        apply_plugins(&mut without, None, with_root(), false);
        assert_eq!(kits_data(&without).as_deref(), root.to_str());

        // A kit with tabs alone, or one a policy kept out: the pane has kits.
        let mut tabs_only = CommandBuilder::new("pwsh");
        let launch = crate::kits::Launch {
            data_root: Some(root.clone()),
            ..kits(&[])
        };
        apply_plugins(&mut tabs_only, None, launch, false);
        assert_eq!(kits_data(&tabs_only).as_deref(), root.to_str());
        let mut forbidden = CommandBuilder::new("pwsh");
        apply_plugins(&mut forbidden, None, with_root(), true);
        assert_eq!(kits_data(&forbidden).as_deref(), root.to_str());
    }

    #[test]
    fn a_pane_without_kits_has_no_data_root_even_one_it_would_inherit() {
        let mut bare = CommandBuilder::new("pwsh");
        apply_plugins(&mut bare, None, crate::kits::launch(&[]), false);
        assert_eq!(kits_data(&bare), None);

        let mut inherited = CommandBuilder::new("pwsh");
        inherited.env(crate::kits::data::ENV_VAR, "/somewhere/else");
        let bridge = std::path::Path::new("/hangar/mods/hangar-bridge");
        apply_plugins(
            &mut inherited,
            Some(bridge),
            crate::kits::launch(&[]),
            false,
        );
        assert_eq!(kits_data(&inherited), None);
    }

    #[test]
    fn a_kit_reaches_claude_code_with_the_bridge_or_without_it() {
        let sep = if cfg!(windows) { ";" } else { ":" };
        let bridge = std::path::Path::new("/hangar/mods/hangar-bridge");

        let mut with_bridge = CommandBuilder::new("pwsh");
        with_bridge.env_remove(crate::bridge::PLUGIN_DIRS_VAR);
        let issues = apply_plugins(
            &mut with_bridge,
            Some(bridge),
            kits(&[("studio", "/kits/studio")]),
            false,
        );
        assert!(issues.is_empty());
        assert_eq!(
            plugin_dirs(&with_bridge),
            Some(format!("{}{sep}/kits/studio", bridge.display()))
        );

        // The bridge off, or the pane started with bridge=false: the kit still
        // comes, and the folders the user loads this way stay behind it.
        let mut without = CommandBuilder::new("pwsh");
        without.env(crate::bridge::PLUGIN_DIRS_VAR, "/mine");
        apply_plugins(
            &mut without,
            None,
            kits(&[("studio", "/kits/studio")]),
            false,
        );
        assert_eq!(
            plugin_dirs(&without),
            Some(format!("/kits/studio{sep}/mine"))
        );
    }

    #[test]
    fn a_pane_of_a_workspace_without_kits_loads_none() {
        let mut other = CommandBuilder::new("pwsh");
        other.env_remove(crate::bridge::PLUGIN_DIRS_VAR);
        assert!(apply_plugins(&mut other, None, kits(&[]), false).is_empty());
        assert_eq!(plugin_dirs(&other), None);

        let mut inherited = CommandBuilder::new("pwsh");
        inherited.env(crate::bridge::PLUGIN_DIRS_VAR, "/mine");
        apply_plugins(&mut inherited, None, kits(&[]), false);
        assert_eq!(
            plugin_dirs(&inherited),
            Some("/mine".to_string()),
            "left as it was"
        );
    }

    #[test]
    fn a_policy_forbidding_sideloading_keeps_kits_out_and_says_so() {
        let mut pane = CommandBuilder::new("pwsh");
        pane.env_remove(crate::bridge::PLUGIN_DIRS_VAR);
        let mut launch = kits(&[("studio", "/kits/studio"), ("docs", "/kits/docs")]);
        launch.issues.push(crate::kits::LaunchIssue::new(
            "gone",
            crate::kits::IssueReason::Missing,
            None,
        ));
        // The bridge never hands its folder back under that policy.
        let issues = apply_plugins(&mut pane, None, launch, true);
        assert_eq!(
            plugin_dirs(&pane),
            None,
            "Claude Code would refuse to start"
        );
        let reasons: Vec<(&str, crate::kits::IssueReason)> = issues
            .iter()
            .map(|issue| (issue.id.as_str(), issue.reason))
            .collect();
        assert_eq!(
            reasons,
            [
                ("gone", crate::kits::IssueReason::Missing),
                ("studio", crate::kits::IssueReason::Forbidden),
                ("docs", crate::kits::IssueReason::Forbidden),
            ]
        );
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

    /// What the window heard as `pty:output` and `pty:exit` reaches the sink
    /// the pane was spawned with, in order, the history matching every chunk.
    #[cfg(unix)]
    #[test]
    fn a_pane_s_output_then_its_end_reach_its_sink() {
        let manager = Arc::new(PtyManager::default());
        let (sent, received) = std::sync::mpsc::channel();
        let shell = crate::shells::ShellInfo {
            id: "sh".into(),
            label: "sh".into(),
            program: "/bin/sh".into(),
            args: args(&["-c", "printf hangar"]),
        };
        let reader_name = Arc::new(Mutex::new(None));
        let named = reader_name.clone();
        pty_spawn(
            &manager,
            Box::new(move |event| {
                *named.lock().unwrap() = std::thread::current().name().map(str::to_owned);
                let _ = sent.send(event);
            }),
            "p-sink".into(),
            std::env::temp_dir().to_string_lossy().into_owned(),
            Some(shell),
            80,
            24,
            None,
            Some(false),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("spawned");

        let mut printed = String::new();
        loop {
            match received
                .recv_timeout(Duration::from_secs(10))
                .expect("an event")
            {
                PtyEvent::Output(chunk) => {
                    assert_eq!(chunk.id, "p-sink");
                    printed.push_str(&chunk.data);
                    assert_eq!(chunk.seq, printed.len() as u64);
                }
                PtyEvent::Exit(exit) => {
                    assert_eq!(exit.id, "p-sink");
                    break;
                }
            }
        }
        assert!(printed.contains("hangar"), "{printed:?}");
        // On a thread named after the pane, for profiles and `top -H`.
        assert_eq!(reader_name.lock().unwrap().as_deref(), Some("pty-p-sink"));
        assert_eq!(
            pty_history(&manager, "p-sink".into()).unwrap().end,
            printed.len() as u64
        );
        pty_kill(&manager, "p-sink".into(), None).unwrap();
        assert!(!pty_alive(&manager, "p-sink".into()));
    }

    /// A flood of small writes, as `seq` or a build log makes: the PTY holds
    /// them while the sink is busy with the first, and they come out whole,
    /// in order, numbered, in a few events rather than one per write.
    #[cfg(unix)]
    #[test]
    fn a_flood_of_small_writes_reaches_the_sink_whole_in_few_events() {
        let manager = Arc::new(PtyManager::default());
        let (sent, received) = std::sync::mpsc::channel();
        let mut first = true;
        // 2000 writes of 6 bytes, `printf` being a builtin of sh.
        let script =
            "printf 'go\\n'; i=0; while [ $i -lt 2000 ]; do printf 'x%04d\\n' $i; i=$((i+1)); done";
        pty_spawn(
            &manager,
            Box::new(move |event| {
                if first {
                    first = false;
                    std::thread::sleep(Duration::from_millis(500));
                }
                let _ = sent.send(event);
            }),
            "p-flood".into(),
            std::env::temp_dir().to_string_lossy().into_owned(),
            Some(sh(script)),
            80,
            24,
            None,
            Some(false),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("spawned");

        let mut printed = String::new();
        let mut events = 0;
        loop {
            match received
                .recv_timeout(Duration::from_secs(10))
                .expect("an event")
            {
                PtyEvent::Output(chunk) => {
                    events += 1;
                    printed.push_str(&chunk.data);
                    assert_eq!(chunk.seq, printed.len() as u64);
                }
                PtyEvent::Exit(_) => break,
            }
        }
        let expected: String = std::iter::once("go\r\n".to_string())
            .chain((0..2000).map(|i| format!("x{i:04}\r\n")))
            .collect();
        assert_eq!(printed, expected);
        assert!(events < 20, "{events} events for 2001 writes");
        pty_kill(&manager, "p-flood".into(), None).unwrap();
    }

    /// Spawns `script` under /bin/sh in pane `id`, and answers the pid it
    /// prints as `started:<pid>`.
    #[cfg(unix)]
    fn spawn_printing_a_pid(manager: &PtyManager, id: &str, script: &str) -> libc::pid_t {
        let (sent, received) = std::sync::mpsc::channel();
        let shell = crate::shells::ShellInfo {
            id: "sh".into(),
            label: "sh".into(),
            program: "/bin/sh".into(),
            args: args(&["-c", script]),
        };
        pty_spawn(
            manager,
            Box::new(move |event| {
                let _ = sent.send(event);
            }),
            id.into(),
            std::env::temp_dir().to_string_lossy().into_owned(),
            Some(shell),
            80,
            24,
            None,
            Some(false),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("spawned");
        let mut printed = String::new();
        loop {
            match received
                .recv_timeout(Duration::from_secs(10))
                .expect("the pid")
            {
                PtyEvent::Output(chunk) => printed.push_str(&chunk.data),
                PtyEvent::Exit(_) => panic!("the shell ended early: {printed:?}"),
            }
            if let Some(pid) = printed
                .split("started:")
                .nth(1)
                .and_then(|rest| rest.split_whitespace().next())
                .and_then(|pid| pid.parse().ok())
                .filter(|_| printed.trim_end().ends_with(|c: char| c.is_ascii_digit()))
            {
                return pid;
            }
        }
    }

    /// With `pane-scopes` on and a user manager to ask, the pane's shell is
    /// in a `hangar-pane-*.scope` of its own, and closing the pane still
    /// kills it. Skipped where no systemd user manager answers.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_scoped_pane_lives_in_a_scope_of_its_own_and_still_dies_with_its_pane() {
        let reachable = std::env::var_os("XDG_RUNTIME_DIR")
            .map(|dir| std::path::Path::new(&dir).join("systemd/private").exists())
            .unwrap_or(false)
            && zbus::blocking::Connection::session().is_ok();
        if !reachable {
            eprintln!("no systemd user manager here, case skipped");
            return;
        }
        let manager = PtyManager::default();
        let id = format!("pty-scope-{}", std::process::id());
        pty_spawn_with(
            &manager,
            Box::new(|_| {}),
            id.clone(),
            std::env::temp_dir().to_string_lossy().into_owned(),
            Some(sh("exec sleep 30")),
            80,
            24,
            Some("Scope test".into()),
            Some(false),
            None,
            Vec::new(),
            None,
            None,
            None,
            None,
            true,
        )
        .expect("spawned");
        let pid = manager
            .lock()
            .sessions
            .get(&id)
            .and_then(|session| session.child.process_id())
            .expect("a pid") as libc::pid_t;
        let wanted = "hangar-pane-scope-test-ptyscope-";
        let mut cgroup = String::new();
        for _ in 0..100 {
            cgroup = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).unwrap_or_default();
            let leaf = cgroup.trim_end().rsplit('/').next().unwrap_or("");
            if leaf.starts_with(wanted) && leaf.ends_with(".scope") {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let leaf = cgroup
            .trim_end()
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_owned();
        let in_scope = leaf.starts_with(wanted) && leaf.ends_with(".scope");
        pty_kill(&Arc::new(manager), id, None).unwrap();
        assert!(in_scope, "the shell's cgroup is {cgroup:?}");
        assert!(ends(pid), "the scoped shell outlived its pane");
    }

    /// Whether `pid` is gone within a few seconds (a process reparented to
    /// init is reaped there, which takes a moment).
    #[cfg(unix)]
    fn ends(pid: libc::pid_t) -> bool {
        for _ in 0..100 {
            if unsafe { libc::kill(pid, 0) } != 0 {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        unsafe { libc::kill(pid, libc::SIGKILL) };
        false
    }

    /// Quitting takes the whole pane along: a process that ignores the
    /// hangup, which `Child::kill` alone would leave running once its shell
    /// is gone, is killed with its group.
    #[cfg(unix)]
    #[test]
    fn quitting_kills_what_ignores_the_hangup_too() {
        let manager = PtyManager::default();
        let pid = spawn_printing_a_pid(
            &manager,
            "p-quit",
            "trap '' HUP; sleep 60 & echo started:$!; wait",
        );
        kill_all(&manager);
        assert!(!pty_alive(&manager, "p-quit".into()));
        assert!(ends(pid), "sleep {pid} outlived the app");
    }

    /// A shell that ends on the hangup is reaped inside `kill`; its group,
    /// read before, still goes.
    #[cfg(unix)]
    #[test]
    fn closing_a_pane_kills_its_group_even_when_the_shell_ended_first() {
        let manager = Arc::new(PtyManager::default());
        let pid = spawn_printing_a_pid(
            &manager,
            "p-close",
            "(trap '' HUP; exec sleep 60) & echo started:$!; wait",
        );
        pty_kill(&manager, "p-close".into(), None).unwrap();
        assert!(ends(pid), "sleep {pid} outlived its pane");
    }

    fn decode(chunks: &[&[u8]]) -> (Vec<String>, Vec<u8>) {
        let mut pending = Vec::new();
        let texts = chunks
            .iter()
            .map(|chunk| {
                pending.extend_from_slice(chunk);
                take_utf8(&mut pending)
            })
            .collect();
        (texts, pending)
    }

    #[test]
    fn utf8_after_an_invalid_byte_is_shown_with_its_chunk() {
        // Latin-1 "café" then a prompt: nothing held back behind the é.
        let (texts, pending) = decode(&[b"caf\xe9 done\n$ "]);
        assert_eq!(texts, ["caf\u{FFFD} done\n$ "]);
        assert!(pending.is_empty());

        // Several invalid sequences, each replaced as from_utf8_lossy does.
        let bytes = b"a\xffb\xc3(c\xe2\x82d\xf0\x9f\x98\x80e";
        let (texts, pending) = decode(&[bytes]);
        assert_eq!(texts, [String::from_utf8_lossy(bytes).into_owned()]);
        assert!(pending.is_empty());
    }

    #[test]
    fn utf8_cut_by_a_read_waits_for_the_next_one() {
        let smile = "😀".as_bytes();
        let (texts, pending) = decode(&[b"ok \xff ", &smile[..2]]);
        assert_eq!(texts, ["ok \u{FFFD} ", ""]);
        assert_eq!(pending, &smile[..2]);

        let (texts, pending) = decode(&[&[b"x".as_slice(), &smile[..3]].concat(), &smile[3..]]);
        assert_eq!(texts, ["x", "😀"]);
        assert!(pending.is_empty());
    }

    #[test]
    fn a_booked_id_is_nobody_elses_until_handed_back() {
        let manager = PtyManager::default();
        let first = reserve(&manager, "p-book").expect("free");
        let second = reserve(&manager, "p-book").err().expect("booked");
        assert!(second.contains("already starting"), "{second}");
        assert!(!pane_alive(&manager, "p-book"), "booked is not alive");
        // Another id is not held up by it.
        drop(reserve(&manager, "p-other").expect("free"));
        // A spawn that fails hands the id back.
        drop(first);
        assert!(manager.lock().starting.is_empty());
        drop(reserve(&manager, "p-book").expect("free again"));
    }

    #[cfg(unix)]
    fn sh(script: &str) -> crate::shells::ShellInfo {
        crate::shells::ShellInfo {
            id: "sh".into(),
            label: "sh".into(),
            program: "/bin/sh".into(),
            args: args(&["-c", script]),
        }
    }

    #[cfg(unix)]
    fn spawn_sh(manager: &PtyManager, id: &str, script: &str) -> Result<Spawned, String> {
        pty_spawn(
            manager,
            Box::new(|_| {}),
            id.into(),
            std::env::temp_dir().to_string_lossy().into_owned(),
            Some(sh(script)),
            80,
            24,
            None,
            Some(false),
            None,
            None,
            None,
            None,
            None,
        )
    }

    /// Two spawns of one id racing — a restart from the window and one from
    /// the control API — start one shell, never two under one id.
    #[cfg(unix)]
    #[test]
    fn two_spawns_of_one_id_start_one_shell() {
        let manager = Arc::new(PtyManager::default());
        let barrier = Arc::new(std::sync::Barrier::new(4));
        let spawns: Vec<_> = (0..4)
            .map(|_| {
                let (manager, barrier) = (manager.clone(), barrier.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    spawn_sh(&manager, "p-race", "sleep 60")
                })
            })
            .collect();
        let results: Vec<_> = spawns.into_iter().map(|s| s.join().unwrap()).collect();
        assert_eq!(
            results.iter().filter(|r| r.is_ok()).count(),
            1,
            "{:?}",
            results.iter().map(|r| r.as_ref().err()).collect::<Vec<_>>()
        );
        assert!(pty_alive(&manager, "p-race".into()));
        assert!(manager.lock().starting.is_empty());
        kill_all(&manager);
    }

    /// A writer that blocks until told to go on, as a pty does whose program
    /// does not read.
    #[cfg(unix)]
    struct Stuck {
        entered: std::sync::mpsc::Sender<()>,
        release: Arc<(Mutex<bool>, Condvar)>,
    }

    #[cfg(unix)]
    impl Write for Stuck {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let _ = self.entered.send(());
            let (go, moved) = &*self.release;
            let go = go.lock().unwrap();
            let _go = moved.wait_while(go, |go| !*go).unwrap();
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// One pane whose program does not read its input — a big paste into a
    /// busy agent — blocks the writes to that pane, never another pane's
    /// typing, nor a resize, a spawn or a check of who is alive.
    #[cfg(unix)]
    #[test]
    fn a_pane_that_does_not_read_blocks_no_other_pane() {
        let manager = Arc::new(PtyManager::default());
        spawn_sh(&manager, "p-stuck", "sleep 60").expect("spawned");
        spawn_sh(&manager, "p-free", "cat >/dev/null").expect("spawned");

        let (entered, entering) = std::sync::mpsc::channel();
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let stuck: Box<dyn Write + Send> = Box::new(Stuck {
            entered,
            release: release.clone(),
        });
        manager
            .lock()
            .sessions
            .get_mut("p-stuck")
            .expect("live")
            .writer = Arc::new(Mutex::new(stuck));

        let blocked = {
            let manager = manager.clone();
            std::thread::spawn(move || pty_write(&manager, "p-stuck".into(), "x".repeat(65536)))
        };
        entering
            .recv_timeout(Duration::from_secs(5))
            .expect("the write to the stuck pane started");

        let (done, finished) = std::sync::mpsc::channel();
        {
            let manager = manager.clone();
            std::thread::spawn(move || {
                let wrote = pty_write(&manager, "p-free".into(), "hello\n".into());
                let resized = pty_resize(&manager, "p-free".into(), 100, 30);
                let alive = pty_alive(&manager, "p-free".into());
                let _ = done.send((wrote, resized, alive));
            });
        }
        let (wrote, resized, alive) = finished
            .recv_timeout(Duration::from_secs(5))
            .expect("the other pane was not held up");
        assert_eq!((wrote, resized, alive), (Ok(()), Ok(()), true));

        *release.0.lock().unwrap() = true;
        release.1.notify_all();
        assert_eq!(blocked.join().unwrap(), Ok(()));
        kill_all(&manager);
    }

    /// Quitting hangs the panes up and gives them a moment before the kill:
    /// what writes something on the way out — Claude Code its transcript —
    /// has the time to, and quitting stays quick.
    #[cfg(unix)]
    #[test]
    fn quitting_gives_the_panes_a_moment_to_finish_writing() {
        let file = crate::testdir::scratch("pty-flush").join("flushed");
        let manager = PtyManager::default();
        spawn_printing_a_pid(
            &manager,
            "p-flush",
            &format!(
                "trap 'sleep 0.1; echo flushed > {}; exit 0' HUP; echo started:$$; while :; do sleep 1; done",
                file.display()
            ),
        );
        let started = std::time::Instant::now();
        kill_all(&manager);
        let took = started.elapsed();
        let written = std::fs::read_to_string(&file).unwrap_or_default();
        let _ = std::fs::remove_file(&file);
        assert_eq!(written.trim(), "flushed");
        assert!(took < Duration::from_secs(1), "quitting took {took:?}");
    }

    /// Quitting does not leave behind a pane `pty_kill` was still reaping.
    #[cfg(unix)]
    #[test]
    fn quitting_waits_for_the_panes_being_reaped() {
        let manager = Arc::new(PtyManager::default());
        spawn_printing_a_pid(
            &manager,
            "p-reaping",
            "trap '' HUP; sleep 60 & echo started:$!; wait",
        );
        pty_kill(&manager, "p-reaping".into(), None).unwrap();
        kill_all(&manager);
        assert!(manager.lock().dying.is_empty());
    }
}
