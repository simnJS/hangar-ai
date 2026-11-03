# hangar-core: the API the native app calls

`crates/hangar-core` is what `src-tauri/src` was, without Tauri: everything
Hangar does that is not drawing. The Tauri window reached it through
`invoke("name", { … })` and `listen("event", …)`; the native app calls plain
Rust functions and reads a typed event bus. This file is the whole contract:
every former command, every former event, and what the app now does itself.

The React frontend in `src/` stays the reference for *when* each call is
made; this file says *what* to call instead.

## Contents

1. [Starting and stopping](#starting-and-stopping)
2. [Conventions](#conventions)
3. [Where files live](#where-files-live)
4. [Events](#events)
5. [Pane output](#pane-output)
6. [Commands](#commands)
7. [The two schemes](#the-two-schemes)
8. [What replaced Tauri's plugins](#what-replaced-tauris-plugins)
9. [What the UI provides](#what-the-ui-provides)
10. [Decisions taken in the port](#decisions-taken-in-the-port)

## Starting and stopping

```rust
// `hangar-ai --mcp` is the stdio MCP server agent CLIs start: no window, no
// Core. Checked first, as src-tauri/main.rs.legacy did.
if std::env::args().any(|arg| arg == "--mcp") {
    hangar_core::run_mcp();
    return;
}

let host = hangar_core::Host::new();          // the folders Tauri used, a new event bus
let mut events = host.subscribe();            // before start: the bus keeps nothing for later
let core = hangar_core::Core::start(host.clone(), runtime.handle());
// … the app runs, reading `events` and calling the functions below …
core.shutdown();                              // when the main window goes; then exit
```

`Core::start(host: Host, runtime: &tokio::runtime::Handle) -> Core` does what
the Tauri `setup` hook did, in this order:

1. warms the login shell's `PATH` (`path_env::ensure`) and writes the
   hangar-bridge mod to `~/.hangar/mods/` (`bridge::plugin_dir`), each on a
   thread of its own;
2. starts the watchers: `kits::watch` (every 2 s: the kits' index and
   manifests, `Event::KitsChanged`) and `accounts::watch` (every 3 s: the
   Claude accounts' logins, `Event::AccountsChanged`; it also finishes
   deleting what an account removal could not);
3. opens Hangar Mobile (`remote::Remote::open`, `<app data>/remote.json`) and
   spawns `resume()` on `runtime`, which listens again if the person left it
   on. A failure is printed (`Hangar Mobile is unavailable: …`) and
   `core.remote()` is then `None`;
4. spawns the agents' HTTP API on `runtime` (`server::start`, 127.0.0.1 on an
   ephemeral port, a bearer token made at startup), then hands the port and
   token to the panes (`bridge::set_endpoint`) and publishes them for the
   agents (`endpoint::publish`: `~/.hangar/endpoint.json` and the legacy
   `~/.iabench/endpoint.json`, or `$HANGAR_ENDPOINT_FILE` alone). A failure
   is printed (`board API failed to start: …`).

`runtime` must outlive the `Core`; the app's own Tokio runtime (iced's `tokio`
executor) is the natural one.

`Core::shutdown(&self)` does what the Tauri app did when the main window was
destroyed: kills every pane's shell (`pty::kill_all`), records in Hangar
Mobile's history that Hangar stopped and lets the PC sleep
(`remote.activity_stopped()`), cancels a delivery's check or review still
running (`delivery::cancel_all`), deletes the published endpoint
(`endpoint::clear`) and takes the Discord presence down. The app then exits
(Tauri called `app.exit(0)`). Only the main window's end does this: a
workspace window closing leaves its panes running.

What Tauri managed (`.manage(…)`) is held by the `Core`:

| Tauri state | Now | Handed to the functions as |
|---|---|---|
| `State<PtyManager>` | `core.pty() -> &Arc<pty::PtyManager>` | `&PtyManager` (sync), `&Arc<PtyManager>` (`pty_kill`), `Arc<PtyManager>` (async) |
| `State<VoiceManager>` | `core.voice() -> &Arc<voice::VoiceManager>` | `&VoiceManager`, `&Arc<VoiceManager>` (`voice_stop`), `Arc<VoiceManager>` (`voice_start`) |
| `State<DiscordPresence>` | `core.discord() -> &discord::DiscordPresence` | `&DiscordPresence` |
| `State<Arc<Remote>>` | `core.remote() -> Option<&Arc<remote::Remote>>` | `&Remote` (sync), `Arc<Remote>` (async) |
| `AppHandle` | `core.host() -> &Host` (or any clone) | `&Host` (sync), `Host` (async) |

## Conventions

- **Same names.** Every command keeps its name; its arguments keep theirs in
  snake_case (the window sent `workspaceCwd`, Rust takes `workspace_cwd`) and
  their types (`String` where the window sent a string, `Option<…>` where it
  could leave one out). Argument and answer types are the structs the window
  (de)serialised: their fields are public and named in snake_case, where the
  window saw camelCase (`#[serde(rename_all = "camelCase")]` is still on them,
  for anything that serialises them again). Enums the window saw as
  `{ status: "…", … }` are plain Rust enums.
- **Async functions** take what they need by value (`Host`, `Arc<…>`, owned
  `String`s), so their futures are `'static` and `Send`: hand them straight to
  `iced::Task::perform`. They run their slow part on Tokio's blocking pool, as
  they did on Tauri's runtime, and must be awaited on a Tokio runtime.
- **Sync functions** do their work where they are called. Tauri ran a sync
  command on the main thread too, so those that touch the disk or a process
  (`load_state`, `save_state`, `memory_*`, `board_*`, `mcp_*`, `accounts_*`,
  `detect_*`, `list_sessions`, `read_code_workspace`, `write_agent_instructions`…)
  cost the UI thread what they cost the window; run them on
  `tokio::task::spawn_blocking` where that matters. The pty ones (`pty_write`,
  `pty_resize`, `pty_alive`, `pty_history`, `pty_kill`) are made to be called
  from the UI thread.
- **Errors.** `Result<T, String>`: the `String` is the sentence the window
  showed, unchanged. A kit tab's calls answer `Result<T, kits::TabError>`, the
  window's `{ code, message }`. Several functions never fail and answer an
  outcome instead (`git_repo_info`, `git_clone`, `delivery_run`,
  `open_in_vscode`…): that is said at each.
- **Types.** Everything is reachable from the crate root: `pub mod` for every
  module, and the types of private submodules re-exported where the commands
  name them — `kits::{Inspection, StepStatus, UpdateInfo, Source, GitSource,
  Manifest, Tab, …}`, `remote::{DeviceView, SlotView, TailscaleStatus, Text,
  Lang, Message, ShareKind, …}`, `stats::Agent`.

## Where files live

`hangar_core::paths::Paths` resolves the folders Tauri 2 resolved for the
bundle identifier `com.simnjs.iabench` (`paths::IDENTIFIER`), byte for byte,
so an existing install finds its data. `Host::new()` uses
`Paths::system()`; `Host::with_paths(Paths::rooted(dir))` keeps a test or a
side build away from the real ones.

| `Paths` method | Linux | macOS | Windows |
|---|---|---|---|
| `app_data_dir()` | `~/.local/share/com.simnjs.iabench` | `~/Library/Application Support/com.simnjs.iabench` | `%APPDATA%\com.simnjs.iabench` |
| `app_config_dir()` | `~/.config/com.simnjs.iabench` | `~/Library/Application Support/com.simnjs.iabench` | `%APPDATA%\com.simnjs.iabench` |
| `app_local_data_dir()` | `~/.local/share/com.simnjs.iabench` | `~/Library/Application Support/com.simnjs.iabench` | `%LOCALAPPDATA%\com.simnjs.iabench` |
| `app_cache_dir()` | `~/.cache/com.simnjs.iabench` | `~/Library/Caches/com.simnjs.iabench` | `%LOCALAPPDATA%\com.simnjs.iabench` |
| `app_log_dir()` | `~/.local/share/com.simnjs.iabench/logs` | `~/Library/Logs/com.simnjs.iabench` | `%LOCALAPPDATA%\com.simnjs.iabench\logs` |

(`$XDG_DATA_HOME`, `$XDG_CONFIG_HOME` and `$XDG_CACHE_HOME` move the Linux
ones, as they did.) Each returns `Err(paths::UnknownPath)`, displayed
`unknown path` as Tauri's error was, when the system names no such folder.

Every file the backend keeps went under `app_data_dir()`: `state.json` (and
`state.json.corrupt.*` rescues), `memory.json`, `remote.json`, `remote/`
(Hangar Mobile's shares and push key), `activity/`, `models/` (voice), `mcp/`
(the per-pane MCP files), `usage-stats-cache.json`, `hq/`. The other folders
are the app's: whatever the Tauri window or its plugins kept there (the
webview's own storage, the updater's downloads) stays where it was. Outside
the app folders, as before: `~/.hangar/` (mods, kits, endpoint), `~/.claude*`
and the agents' configs.

## Events

`Host::subscribe() -> tokio::sync::broadcast::Receiver<host::Event>`
(`Event` is `Clone + Debug`; `hangar_core::Event` at the root). Every
subscriber gets every event sent after it subscribed; the bus holds 1024 for
a subscriber that has not read them, and one that falls further behind is
told `RecvError::Lagged(n)` and loses the oldest. A lost change notification
is a reload to do anyway; a lost control request times out on the side that
asked. `Event::name()` gives the event's old name.

`Host::emit(Event) -> Result<(), host::NoListener>` sends one; it fails only
when nothing is subscribed. The app may send events of its own on it (the
window emitted `board:changed` itself after a delivery, in
`src/lib/deliveryControl.ts`).

| `Event` variant | Window's event | Payload | Sent when |
|---|---|---|---|
| `BoardChanged { cwd: String }` | `board:changed` (payload: the cwd) | the workspace's folder, as the board is keyed | the agents' API wrote a board (task created, patched, deleted, claimed, commented); `board_claim_worker`, `board_release_worker` |
| `MemoryChanged` | `memory:changed` (payload `""`) | — | the API or `memory_create`/`memory_update`/`memory_delete` wrote the memory |
| `ControlRequest(control::ControlRequest)` | `control:request` | `ControlRequest { id: String, op: String, args: serde_json::Value }` | an agent asked to act on the panes (`/api/control/{op}`, or a kit hook's `/api/kit/tab/open`). `op` is one of `control::OPERATIONS`. Answer with `control::control_reply(id, ok, value, error)` |
| `ControlInternal(control::ControlRequest)` | `control:internal` | same | Hangar's own questions: `orchestrator_briefing` (`{ paneId }`, given 8 s, answered by src/lib/orchestratorBrief.ts) and the phone's calls `remote_<op>` (given 10 s, answered by src/lib/remoteControl.ts; `<op>` is `state`, `screen`, `send`, `permission`, `manager`, `history`, `chat`, `answer`, `wake`, `stats`, `apps`, `delivery`, `delivery_file`, `land`, `rework`, and `workspace`). Answered whether or not pane control is on |
| `BridgeEvent { pane_id: String, event: serde_json::Value }` | `bridge:event` (`{ paneId, event }`) | the mod's report as it sent it; the window owns the meaning of `event.kind` | a pane's hangar-bridge mod reported (turns, permission dialogs, context, edits) |
| `BridgeQueue(bridge::QueueChanged)` | `bridge:queue` | `QueueChanged { pane_id: String, pending: Vec<bridge::Queued> }`, `Queued { id, text, from: Option<String> }` | `bridge_enqueue`, `bridge_cancel`, and the mod taking a queued prompt |
| `VoicePhase(voice::Phase)` | `voice:phase` | `Phase { phase: &'static str /* "idle", "recording", "transcribing" */, error: Option<String> }` | `voice_start`, `voice_stop`, a transcription ending |
| `VoiceText(voice::Transcript)` | `voice:text` | `Transcript { text: String }` | a dictation transcribed to something |
| `VoiceLevel(voice::Level)` | `voice:level` | `Level { level: f32 /* 0..1 */ }` | every 60 ms while recording |
| `VoiceDownload(voice::models::Progress)` | `voice:download` | `Progress { id: String, received: u64, total: u64, done: bool }` | `voice_model_download`, at most every 200 ms, and once `done` |
| `RemoteChanged` | `remote:changed` | — | a phone paired or revoked, a push subscription added or dropped |
| `RemoteShared(remote::Share)` | `remote:shared` | `Share { id, at, workspace, pane, name, mime, kind: remote::ShareKind, size, caption }` | an agent shared a file with the phones (`/api/phone/share`); the window's history and push listened |
| `KitsChanged` | `kits:changed` | — | `kit_add_folder`, `kit_add_git`, `kit_remove`, `kit_update`, `kit_check_update` finding one, or the watcher seeing a manifest or the index change |
| `AccountsChanged` | `accounts:changed` | — | `account_add`, `account_rename`, `account_remove`, or the watcher seeing a login appear, change or go |
| `GitCloneProgress(git_clone::CloneProgress)` | `git-clone:progress` | `CloneProgress { job_id: String, phase: String, percent: Option<u8> }` | while `git_clone` runs |
| `NotificationActivated(notify::Activated)` | `notification-activated` | `Activated { workspace_id: String, pane_id: String }` | the person clicked a toast raised by `notify_send` (Windows) |

Control request timeouts, counted from the request (control.rs): 20 s for a
pane operation, 60 s for `model_*`, `preview_*` and `project_*`, 120 s for
`worker_*`, 35 min for `workspace_create`. With nothing subscribed, a request
fails at once (`could not reach the window: nothing is listening to
Hangar's events`) instead of waiting its timeout out.

Gone from the bus:

- `pty:output` and `pty:exit`: each pane's own sink, below.
- `window:close-requested`: windows.rs sent it to a workspace window whose
  close button was pressed; windows are the app's now (see "What the UI
  provides").

Events the windows sent each other, never the backend, are the app's own
business now: `store:state`, `store:hello` (src/store.tsx),
`model:ask`/`model:answer`/`model:claim`/`model:handover`
(src/lib/modelHost.ts), `terminal:ask`/`terminal:answer` (src/lib/windows.ts),
`window:focus-pane`, `agent:activity` (src/App.tsx),
`kits:grant-roots`/`kits:grant-roots-done` (src/lib/kitTabs.ts), and Tauri's
`tauri://drag-*` file drops (src/lib/fileDrop.ts).

## Pane output

A pane's output does not go through the bus: `pty_spawn` takes a sink,

```rust
pub type PtySink = Box<dyn FnMut(pty::PtyEvent) + Send + 'static>;

pub enum PtyEvent {
    Output(PtyOutput), // PtyOutput { id: String, data: String, seq: u64 }
    Exit(PtyExit),     // PtyExit { id: String, code: Option<u32> }, always None
}
```

called on the pane's reader thread, in order, for as long as the shell
prints: `Output` for each chunk (whole UTF-8 sequences only; `seq` is the
pane's byte total, this chunk included), then `Exit` when the shell ends by
itself. A pane killed with `pty_kill` gets no `Exit`, and its reader stops
at the next chunk it reads: the id may already belong to a new shell. The
sink must not block for long: the shell's output waits in the pty while it
runs. Feed the pane's terminal from it, or push into a channel the UI drains.

Why not the bus: a pane's output is most of what the backend says (a build
log, an agent redrawing sixty times a second) and only ever for one pane; on
the bus every subscriber would copy every pane's every chunk, and a burst
would push the rarer events, control requests among them, out of a bounded
bus. The window's `orchestratorBrief.ts` listened to `pty:exit` for every
pane: the app routes each sink's `Exit` there itself.

A window taking a pane over (the Tauri app's workspace windows, a reload)
read `pty_history` and dropped the live chunks whose `seq` it already had;
the sink is set once at spawn, so in the native app the pane's terminal
lives in app state and any window draws it.

## Commands

All 134 commands the window could invoke are here, under their old name: 131
as functions of hangar-core, and the three window commands under "What the
UI provides" (`open_workspace_window`, `focus_workspace_window`,
`close_workspace_window`). "Invoked from" names the frontend files that
called each one.

### Panes' terminals

#### `pty_spawn`

Invoked as `pty_spawn` from `src/lib/ipc.ts`. Now `hangar_core::pty::pty_spawn`, sync.

```rust
pub fn pty_spawn(
    manager: &PtyManager,
    output: PtySink,
    id: String,
    cwd: String,
    shell: Option<hangar_core::shells::ShellInfo>,
    cols: u16,
    rows: u16,
    name: Option<String>,
    bridge: Option<bool>,
    port_base: Option<u16>,
    mcp_config: Option<String>,
    kits: Option<Vec<String>>,
    agent: Option<String>,
    claude_account: Option<String>,
) -> Result<Spawned, String>
```

- Note: blocks up to 5 s while a pane killed under the same id is still being reaped.
- Note: `output` is called on the pane's reader thread with `PtyEvent::Output(PtyOutput { id, data, seq })` for every chunk, then `PtyEvent::Exit(PtyExit { id, code: None })` if the shell ends by itself (never after `pty_kill`). It replaces the global `pty:output` / `pty:exit` events: see "Pane output".
- Errors: `pty {id} already running`, `pty {id} is still shutting down`, `no usable shell found on this system`, or portable-pty's message when the pty cannot be opened or the shell started.
- Note: `Spawned { kit_issues, account, account_issues }`: kits kept out of the pane and why, the Claude account it runs on (absent when not Claude Code), what the account's profile could not share.

#### `pty_write`

Invoked as `pty_write` from `src/lib/ipc.ts`. Now `hangar_core::pty::pty_write`, sync.

```rust
pub fn pty_write(manager: &PtyManager, id: String, data: String) -> Result<(), String>
```

- Errors: `pty not found`, or the write's I/O error.

#### `pty_resize`

Invoked as `pty_resize` from `src/lib/ipc.ts`. Now `hangar_core::pty::pty_resize`, sync.

```rust
pub fn pty_resize(manager: &PtyManager, id: String, cols: u16, rows: u16) -> Result<(), String>
```

- Errors: `pty not found`, or the resize's error.

#### `pty_kill`

Invoked as `pty_kill` from `src/lib/ipc.ts`. Now `hangar_core::pty::pty_kill`, sync.

```rust
pub fn pty_kill(manager: &Arc<PtyManager>, id: String, tree: Option<bool>) -> Result<(), String>
```

`tree` also ends every process below the shell, which is what a pane put to sleep wants: the memory is the point. On Windows the shell's own end does not reach them — a process there outlives its parent — and an agent's helpers (its MCP servers, a build it started) are not always tied to the pane's console either. A pane closed by hand keeps the gentler kill, which leaves alone what the user deliberately started out of it, an editor or a browser. The manager comes as an `Arc` because the reap outlives the call, on a thread of its own that holds on to it.

- Note: returns at once; the shell is reaped on a thread of its own, and a `pty_spawn` on the same id waits for it. An unknown id is `Ok`. `tree: Some(true)` also ends every process below the shell (a pane put to sleep).
- Note: takes `&Arc<PtyManager>` (`core.pty()`): the reaping thread keeps a handle.

#### `pty_alive`

Invoked as `pty_alive` from `src/lib/ipc.ts`. Now `hangar_core::pty::pty_alive`, sync.

```rust
pub fn pty_alive(manager: &PtyManager, id: String) -> bool
```

#### `pty_has_children`

Invoked as `pty_has_children` from `src/lib/ipc.ts`. Now `hangar_core::pty::pty_has_children`, async.

```rust
pub async fn pty_has_children(
    manager: Arc<PtyManager>,
    ids: Vec<String>,
) -> Result<Vec<String>, String>
```

The panes among `ids` whose shell still runs a process of its own — a dev server, a watcher, a build — which closing the pane would end. A pane that is not running has nothing to lose and is left out. Off the main thread: reading the process table is a few milliseconds on Windows and a `ps` on unix, and a command that is not `async` would spend them where the window repaints.

- Errors: `the process table could not be read`, or the blocking task's join error.

#### `pty_history`

Invoked as `pty_history` from `src/lib/ipc.ts`. Now `hangar_core::pty::pty_history`, sync.

```rust
pub fn pty_history(manager: &PtyManager, id: String) -> Result<PtyHistory, String>
```

The tail of a live pane's output, for a window taking the pane over.

- Errors: `pty not found`.
- Note: `PtyHistory { data, end, account }`: `end` is the `seq` the last chunk in `data` carried, to drop live chunks the history already holds.

### Claude accounts

#### `accounts_list`

Invoked as `accounts_list` from `src/lib/accounts.ts`. Now `hangar_core::accounts::accounts_list`, sync.

```rust
pub fn accounts_list() -> Vec<Account>
```

#### `account_add`

Invoked as `account_add` from `src/lib/accounts.ts`. Now `hangar_core::accounts::account_add`, sync.

```rust
pub fn account_add(host: &Host, label: String) -> Result<Added, String>
```

A new profile named `label`, ready for its login. An account that could not share main's conversations is not made: a conversation could not move to it or from it.

- Sends: `Event::AccountsChanged`.
- Errors: `The account could not share the conversations of the main one: {reason}`, or the account store's refusal (label, file).

#### `account_rename`

Invoked as `account_rename` from `src/lib/accounts.ts`. Now `hangar_core::accounts::account_rename`, sync.

```rust
pub fn account_rename(host: &Host, id: String, label: String) -> Result<Account, String>
```

- Sends: `Event::AccountsChanged`.
- Errors: the account store's refusal (`No such account.`, an empty label…).

#### `account_remove`

Invoked as `account_remove` from `src/lib/accounts.ts`. Now `hangar_core::accounts::account_remove`, sync.

```rust
pub fn account_remove(
    host: &Host,
    manager: &hangar_core::pty::PtyManager,
    id: String,
) -> Result<(), String>
```

Deletes a profile Hangar made — its folder, its login — once the window has asked the person. Never main, nothing the links lead to, and not while a pane still runs on it.

- Sends: `Event::AccountsChanged`.
- Errors: `{n} pane(s) still run on this account: switch or close them first.`, `The main account cannot be removed.`, `No such account.`, `This account's folder is the main one's: it is not removed.`, `The account's folder is in use ({err}): close what uses it and try again.`

#### `accounts_support`

Invoked as `accounts_support` from `src/lib/accounts.ts`. Now `hangar_core::accounts::accounts_support`, sync.

```rust
pub fn accounts_support() -> Support
```

Whether logging in to one account here leaves the others logged in.

### hangar-bridge queue

#### `bridge_enqueue`

Invoked as `bridge_enqueue` from `src/lib/ipc.ts`. Now `hangar_core::bridge::bridge_enqueue`, sync.

```rust
pub fn bridge_enqueue(
    host: &Host,
    pane_id: String,
    text: String,
    from: Option<String>,
) -> Queued
```

- Sends: `Event::BridgeQueue`.

#### `bridge_cancel`

Invoked as `bridge_cancel` from `src/lib/ipc.ts`. Now `hangar_core::bridge::bridge_cancel`, sync.

```rust
pub fn bridge_cancel(host: &Host, pane_id: String, id: Option<String>)
```

- Sends: `Event::BridgeQueue`.
- Note: `id: None` empties the pane's queue.

#### `bridge_queue`

Invoked as `bridge_queue` from `src/lib/ipc.ts`. Now `hangar_core::bridge::bridge_queue`, sync.

```rust
pub fn bridge_queue(pane_id: String) -> Vec<Queued>
```

### Pane control

#### `control_reply`

Invoked as `control_reply` from `src/lib/control.ts`, `src/lib/orchestratorBrief.ts`, `src/lib/remoteControl.ts`. Now `hangar_core::control::control_reply`, sync.

```rust
pub fn control_reply(id: String, ok: bool, value: Option<Value>, error: Option<String>)
```

The window's answer to a control request, internal or not. An id nobody waits for any more — the request timed out — is dropped.

- Note: answers an `Event::ControlRequest` or `Event::ControlInternal` by its `id`, once. `ok: false` gives the asker `error` (or `the window refused without saying why`); `ok: true` gives `value` (or `null`). An id nobody waits for any more is dropped.

### Sessions, context, conversation, usage

#### `list_sessions`

Invoked as `list_sessions` from `src/lib/ipc.ts`. Now `hangar_core::sessions::list_sessions`, sync.

```rust
pub fn list_sessions(cwd: String) -> Vec<AgentSession>
```

The Claude Code conversations kept for `cwd`, newest first. The TS passed an agent id as well; Claude Code is the only agent since the owner's decision of 2026-11-03, so the argument is gone (Codex's rollouts are no longer listed).

#### `session_exists`

Invoked as `session_exists` from `src/lib/ipc.ts`. Now `hangar_core::sessions::session_exists`, sync.

```rust
pub fn session_exists(id: String) -> Option<bool>
```

Whether the conversation a pane is about to resume has a transcript left: `Some(false)` only when it is nowhere on disk, `None` when that cannot be told. Claude Code takes its session id at startup but writes nothing until the first message. A pane reset and then left alone keeps an id with no conversation behind it, and `--resume` on that id only prints "No conversation found". Every project directory is searched, not just the pane's: a transcript filed under another spelling of the path is still worth handing to the agent. Only Claude Code's transcripts are looked for; the TS's agent argument is gone.

#### `detect_agents`

Invoked as `detect_agents` from `src/lib/ipc.ts`. Now `hangar_core::sessions::claude_installed`, sync.

```rust
pub fn claude_installed() -> bool
```

Whether Claude Code is on PATH, so the UI can say so where it offers it. Codex, Gemini CLI and OpenCode are no longer looked for (owner's decision, 2026-11-03).

#### `context_usage`

Invoked as `context_usage` from `src/lib/ipc.ts`. Now `hangar_core::context::context_usage`, async.

```rust
pub async fn context_usage(
    session_id: String,
    cwd: String,
) -> Option<ContextUsage>
```

Where the pane's Claude Code stands in its context window, according to its own transcript. `None` covers every "show nothing" case: a session not written yet, a file with no usage line so far. The TS's agent argument is gone: Codex rollouts are no longer read.

#### `chat_page`

Invoked as `chat_page` from `src/lib/remoteControl.ts`. Now `hangar_core::chat::chat_page`, async.

```rust
pub async fn chat_page(
    session_id: String,
    cwd: String,
    before: Option<String>,
    limit: Option<usize>,
) -> Result<Option<ChatPage>, String>
```

One page of a Claude Code session's conversation: the newest without `before`, else the one before that cursor. `None` while the session has no transcript yet.

- Errors: the transcript could not be read; `Ok(None)` while the session has none yet.

#### `usage_stats`

Invoked as `usage_stats` from `src/lib/stats.ts`. Now `hangar_core::stats::usage_stats`, async.

```rust
pub async fn usage_stats(
    host: Host,
    from: Option<i64>,
    to: Option<i64>,
    cached_only: Option<bool>,
) -> Result<UsageStats, String>
```

- Errors: only the blocking task's join error. `cached_only: Some(true)` answers from the cache without scanning the transcripts.
- Note: reads Claude Code's transcripts only (`~/.claude/projects/` of every account); Codex rollouts are no longer read, so no zstd decoding, and the price table covers Anthropic's models only.

### Shells, state file, folders

#### `detect_shells`

Invoked as `detect_shells` from `src/lib/ipc.ts`. Now `hangar_core::shells::detect_shells`, sync.

```rust
pub fn detect_shells() -> Vec<ShellInfo>
```

#### `load_state`

Invoked as `load_state` from `src/lib/ipc.ts`. Now `hangar_core::store::load_state`, sync.

```rust
pub fn load_state(host: &Host) -> Result<Option<serde_json::Value>, String>
```

- Errors: `no app data dir: unknown path`, I/O errors, `state.json is unreadable ({err}); kept as {path}`, `state.json is unreadable ({err}); it could not be moved aside`. `Ok(None)` when there is no state yet (after trying the previous identifiers' folders).
- Note: moves a Groq key left in the state into the keychain, and rewrites the file without it.

#### `save_state`

Invoked as `save_state` from `src/lib/ipc.ts`. Now `hangar_core::store::save_state`, sync.

```rust
pub fn save_state(host: &Host, state: serde_json::Value) -> Result<(), String>
```

- Errors: `no app data dir: unknown path`, serialisation or I/O errors. Written to `state.json.tmp`, then renamed.

#### `dir_exists`

Invoked as `dir_exists` from `src/lib/ipc.ts`. Now `hangar_core::store::dir_exists`, sync.

```rust
pub fn dir_exists(path: String) -> bool
```

#### `home_dir`

Invoked as `home_dir` from `src/lib/ipc.ts`. Now `hangar_core::store::home_dir`, sync.

```rust
pub fn home_dir() -> String
```

- Note: `""` when the system names no home.

#### `hq_dir`

Invoked as `hq_dir` from `src/lib/orchestrationActions.ts`. Now `hangar_core::hq_dir`, sync.

```rust
pub fn hq_dir(host: &Host) -> Result<String, String>
```

The project manager's folder: `<app data dir>/hq`, created if missing. Its Claude Code works from here rather than from any project.

- Errors: `unknown path`, `could not create {dir}: {err}`.

#### `read_code_workspace`

Invoked as `read_code_workspace` from `src/lib/ipc.ts`. Now `hangar_core::workspace_file::read_code_workspace`, sync.

```rust
pub fn read_code_workspace(path: String) -> Result<Vec<WorkspaceRoot>, String>
```

- Errors: `{path}: {io or JSON error}`, `{path}: no "folders" list`.

#### `find_code_workspace`

Invoked as `find_code_workspace` from `src/lib/ipc.ts`. Now `hangar_core::workspace_file::find_code_workspace`, sync.

```rust
pub fn find_code_workspace(dir: String) -> Option<String>
```

The `.code-workspace` sitting directly in `dir`, if there is exactly one obvious candidate. Used to offer the workspace file to someone who picked the folder holding it.

### Board

#### `board_load`

Invoked as `board_load` from `src/lib/board.ts`. Now `hangar_core::board_load`, sync.

```rust
pub fn board_load(cwd: String) -> board::Board
```

#### `board_create`

Invoked as `board_create` from `src/lib/board.ts`. Now `hangar_core::board_create`, sync.

```rust
pub fn board_create(cwd: String, task: board::NewTask) -> Result<board::Task, String>
```

- Errors: the board's own sentence (`unknown column '{column}'`, a title or a field it refuses…), or the workspace's `.hangar/board.json` could not be written.

#### `board_update`

Invoked as `board_update` from `src/lib/board.ts`. Now `hangar_core::board_update`, sync.

```rust
pub fn board_update(
    cwd: String,
    id: String,
    patch: board::TaskPatch,
) -> Result<board::Task, String>
```

- Errors: as `board_create`, and `task not found`.

#### `board_delete`

Invoked as `board_delete` from `src/lib/board.ts`. Now `hangar_core::board_delete`, sync.

```rust
pub fn board_delete(cwd: String, id: String) -> Result<(), String>
```

- Errors: `task not found`, or a write failure.

#### `board_claim`

Invoked as `board_claim` from `src/lib/board.ts`. Now `hangar_core::board_claim`, sync.

```rust
pub fn board_claim(cwd: String, id: String, agent: String) -> Result<board::Task, String>
```

- Errors: `task not found`, `already claimed by {owner}`, a task that cannot start (blocked, done: `"{title}" is done already.`), or a write failure.

#### `board_claim_worker`

Invoked as `board_claim_worker` from `src/lib/board.ts`. Now `hangar_core::board_claim_worker`, sync.

```rust
pub fn board_claim_worker(
    host: &Host,
    cwd: String,
    id: String,
    worker: String,
    max_attempts: u32,
    shared_folder: Option<bool>,
) -> Result<board::Task, String>
```

A worker's start and end (lib/workerControl.ts). Done by the window rather than by an agent over the API, so the board views are told here.

- Sends: `Event::BoardChanged { cwd }`.
- Errors: as `board_claim`.

#### `board_release_worker`

Invoked as `board_release_worker` from `src/lib/board.ts`. Now `hangar_core::board_release_worker`, sync.

```rust
pub fn board_release_worker(
    host: &Host,
    cwd: String,
    id: String,
    worker: String,
    undo: bool,
) -> Result<board::Task, String>
```

- Sends: `Event::BoardChanged { cwd }`.
- Errors: `task not found`, a worker that does not hold the task, or a write failure.

#### `board_comment`

Invoked as `board_comment` from `src/lib/board.ts`. Now `hangar_core::board_comment`, sync.

```rust
pub fn board_comment(
    cwd: String,
    id: String,
    author: String,
    text: String,
) -> Result<board::Task, String>
```

- Errors: `task not found`, a comment the board refuses, or a write failure.

### Git

All async, each run on the blocking pool (`git::off_thread`). Unless said otherwise, an `Err` is git's own message (the one line of it worth showing), or a refusal sentence of Hangar's.

#### `git_repo_info`

Invoked as `git_repo_info` from `src/lib/git.ts`. Now `hangar_core::git::git_repo_info`, async.

```rust
pub async fn git_repo_info(path: String) -> RepoInfo
```

Never fails: a missing git and a directory outside any repository are answers, and the UI needs to be able to draw both.

#### `git_worktree_list`

Invoked as `git_worktree_list` from `src/lib/git.ts`. Now `hangar_core::git::git_worktree_list`, async.

```rust
pub async fn git_worktree_list(path: String) -> Result<Vec<Worktree>, String>
```

#### `git_worktrees_merged`

Invoked as `git_worktrees_merged` from `src/lib/git.ts`. Now `hangar_core::git::git_worktrees_merged`, async.

```rust
pub async fn git_worktrees_merged(
    path: String,
    held_folders: Vec<String>,
) -> Result<Vec<MergedWorktree>, String>
```

#### `git_branches`

Invoked as `git_branches` from `src/lib/git.ts`. Now `hangar_core::git::git_branches`, async.

```rust
pub async fn git_branches(path: String) -> Result<Vec<Branch>, String>
```

Every branch of the repository, most recently committed to first.

#### `git_worktree_add`

Invoked as `git_worktree_add` from `src/lib/git.ts`. Now `hangar_core::git::git_worktree_add`, async.

```rust
pub async fn git_worktree_add(
    path: String,
    worktree_path: String,
    branch: String,
    create_branch: bool,
    base_ref: Option<String>,
) -> Result<(), String>
```

#### `git_worktree_remove`

Invoked as `git_worktree_remove` from `src/lib/git.ts`. Now `hangar_core::git::git_worktree_remove`, async.

```rust
pub async fn git_worktree_remove(
    path: String,
    worktree_path: String,
    force: bool,
    pre_remove: Option<String>,
) -> Result<RemoveResult, String>
```

#### `git_worktree_hook`

Invoked as `git_worktree_hook` from `src/lib/git.ts`. Now `hangar_core::git::git_worktree_hook`, async.

```rust
pub async fn git_worktree_hook(
    path: String,
    worktree_path: String,
    command: Option<String>,
) -> Result<Option<HookReport>, String>
```

The `postCreate` command, run in a worktree that exists by now. Resolves with what it did; a failing hook is a report, never an error, and never undoes the worktree. `None` when no command is set.

- Note: a failing hook is a `HookReport`, never an `Err`.

#### `git_worktree_migrate`

Invoked as `git_worktree_migrate` from `src/lib/git.ts`. Now `hangar_core::git::git_worktree_migrate`, async.

```rust
pub async fn git_worktree_migrate(
    path: String,
    worktree_path: String,
) -> Result<MigrateResult, String>
```

« Bring changes back »: `worktree_path`'s uncommitted work into the main checkout of the repository `path` belongs to.

- Errors: also `the main checkout is the destination, not a source`.

#### `git_worktree_prune`

Invoked as `git_worktree_prune` from `src/lib/git.ts`. Now `hangar_core::git::git_worktree_prune`, async.

```rust
pub async fn git_worktree_prune(path: String) -> Result<(), String>
```

#### `git_worktree_lock`

Invoked as `git_worktree_lock` from `src/lib/git.ts`. Now `hangar_core::git::git_worktree_lock`, async.

```rust
pub async fn git_worktree_lock(path: String, worktree_path: String) -> Result<(), String>
```

Marks a worktree as one `prune` must not collect and `remove` must not take without being told twice. No `--reason`: this is a toggle in a panel with nowhere to type one, and Git Worktree Manager (MIT) locks the same way.

#### `git_worktree_unlock`

Invoked as `git_worktree_unlock` from `src/lib/git.ts`. Now `hangar_core::git::git_worktree_unlock`, async.

```rust
pub async fn git_worktree_unlock(path: String, worktree_path: String) -> Result<(), String>
```

#### `git_fetch`

Invoked as `git_fetch` from `src/lib/git.ts`. Now `hangar_core::git::git_fetch`, async.

```rust
pub async fn git_fetch(path: String) -> Result<(), String>
```

`--all` because the panel offers one button and a repository is allowed more than one remote, and `--prune` so a branch someone deleted upstream stops being offered here as something to check out.

#### `git_branch_delete`

Invoked as `git_branch_delete` from `src/lib/git.ts`. Now `hangar_core::git::git_branch_delete`, async.

```rust
pub async fn git_branch_delete(
    path: String,
    branch: String,
    force: bool,
) -> Result<DeleteBranchResult, String>
```

#### `git_copy_untracked`

Invoked as `git_copy_untracked` from `src/lib/git.ts`. Now `hangar_core::git::git_copy_untracked`, async.

```rust
pub async fn git_copy_untracked(
    from: String,
    to: String,
    patterns: Vec<String>,
    ignored_only: Option<bool>,
) -> Result<CopyResult, String>
```

Carries into a new worktree the files a checkout does not: `.env`, `*.local`, whatever else the user listed. A linked worktree is a clean checkout of a branch, which is precisely what leaves it unable to run until the untracked configuration is sitting next to it. Modelled on `git.worktreeIncludeFiles` in microsoft/vscode extensions/git (MIT) — same purpose, same "copy what is missing, replace nothing" rule. `ignored_only` leaves out the files git does not ignore: a worker's worktree (lib/workerControl.ts) gets its `.env` without an untracked file that a `git add -A` would put on its branch.

#### `git_commits_ahead`

Invoked as `git_commits_ahead` from `src/lib/git.ts`. Now `hangar_core::git::git_commits_ahead`, async.

```rust
pub async fn git_commits_ahead(path: String, base: String, head: String) -> Result<u32, String>
```

`commits_ahead` for the window (lib/workerControl.ts): whether a worker's branch still holds work its target branch does not.

#### `git_changes`

Invoked as `git_changes` from `src/lib/git.ts`. Now `hangar_core::git::git_changes`, async.

```rust
pub async fn git_changes(path: String, base: Option<String>) -> Result<Changes, String>
```

What changed in the working tree holding `path`: tracked changes against HEAD — or against where `base` forked, to take in the branch's commits too — plus the untracked files.

#### `git_file_diff`

Invoked as `git_file_diff` from `src/lib/git.ts`. Now `hangar_core::git::git_file_diff`, async.

```rust
pub async fn git_file_diff(
    path: String,
    file: String,
    old_path: Option<String>,
    untracked: bool,
    base: Option<String>,
) -> Result<FileDiff, String>
```

The unified diff of one file of `git_changes`, read-only.

### Cloning a repository

#### `git_repo_parse`

Invoked as `git_repo_parse` from `src/lib/gitClone.ts`. Now `hangar_core::git_clone::git_repo_parse`, sync.

```rust
pub fn git_repo_parse(input: String) -> ParseResult
```

#### `git_repo_ownership`

Invoked as `git_repo_ownership` from `src/lib/gitClone.ts`. Now `hangar_core::git_clone::git_repo_ownership`, async.

```rust
pub async fn git_repo_ownership(owner: String, host: String) -> Ownership
```

- Note: never fails: asks `gh` (15 s at most); `Ownership { mine, reason }`.

#### `git_clone`

Invoked as `git_clone` from `src/lib/gitClone.ts`. Now `hangar_core::git_clone::git_clone`, async.

```rust
pub async fn git_clone(
    host: Host,
    url: String,
    parent: String,
    name: String,
    job_id: String,
) -> CloneOutcome
```

- Sends: `Event::GitCloneProgress(CloneProgress { job_id, phase, percent })` while it runs.
- Note: never errs: `CloneOutcome::Cloned { path, reused, branch }` or `Refused { kind, reason }`, `kind` one of `invalidUrl`, `invalidDestination`, `exists`, `busy`, `gitMissing`, `cancelled`, `timeout`, `failed`. Given up on after 30 minutes.

#### `git_clone_cancel`

Invoked as `git_clone_cancel` from `src/lib/gitClone.ts`. Now `hangar_core::git_clone::git_clone_cancel`, sync.

```rust
pub fn git_clone_cancel(job_id: String) -> bool
```

- Note: `true` when a clone with that `job_id` was running.

#### `git_clone_discard`

Invoked as `git_clone_discard` from `src/lib/gitClone.ts`. Now `hangar_core::git_clone::git_clone_discard`, async.

```rust
pub async fn git_clone_discard(path: String) -> DiscardOutcome
```

- Note: never errs: `DiscardOutcome::Removed`, or `Refused { kind, reason }`, `kind` one of `invalidPath`, `notMade`, `expired`, `moved`, `link`, `failed`. Only a clone this process made is removed.

### Editor

#### `open_in_vscode`

Invoked as `open_in_vscode` from `src/lib/openEditor.ts`. Now `hangar_core::open_editor::open_in_vscode`, async.

```rust
pub async fn open_in_vscode(cwd: String, from_code_workspace: bool) -> EditorOutcome
```

- Note: never errs: `EditorOutcome::Opened`, `MissingFolder`, `NotFound` (no VS Code) or `Failed { error }`.

### Delivery

#### `delivery_preflight`

Invoked as `delivery_preflight` from `src/lib/deliveryControl.ts`. Now `hangar_core::delivery::delivery_preflight`, async.

```rust
pub async fn delivery_preflight(
    workspace: String,
    worktree: String,
    target: Option<String>,
    landing: Option<bool>,
    task: Option<String>,
) -> Result<Plan, String>
```

- Errors: a sentence for the agent that asked: `{worktree} is not a git worktree: {err}.`, `The workspace folder {workspace} is not a git repository ({err}), so there is nothing to land on.`, `{worktree} is on a detached HEAD: …`, `The branch deliveries land on, {target}, does not exist.`, `Your worktree is on {target} itself, …`, and the like.

#### `delivery_run`

Invoked as `delivery_run` from `src/lib/deliveryControl.ts`. Now `hangar_core::delivery::delivery_run`, async.

```rust
pub async fn delivery_run(delivery: Delivery) -> Outcome
```

Takes as long as the delivery does — the check and the review are minutes — on a thread of its own, never the window's.

- Note: minutes long (the check, then the review). Never errs: `Outcome { outcome, detail, … }`; a crashed run is `Outcome::error("the delivery stopped unexpectedly: …")`.

#### `delivery_cleanup`

Invoked as `delivery_cleanup` from `src/lib/deliveryControl.ts`. Now `hangar_core::delivery::delivery_cleanup`, async.

```rust
pub async fn delivery_cleanup(
    worktree: String,
    branch: String,
    target: String,
    task_id: String,
) -> Cleanup
```

- Note: never errs: `Cleanup` says what was removed and what was kept, and why.

#### `delivery_running`

Invoked as `delivery_running` from `src/lib/deliveryControl.ts`. Now `hangar_core::delivery::delivery_running`, sync.

```rust
pub fn delivery_running() -> Vec<Running>
```

### Memory

#### `memory_load`

Invoked as `memory_load` from `src/lib/memory.ts`. Now `hangar_core::memory::memory_load`, sync.

```rust
pub fn memory_load(host: &Host) -> Result<Memory, String>
```

- Errors: `no app data dir: unknown path`, or `memory.json` unreadable (reported, never an empty memory).

#### `memory_create`

Invoked as `memory_create` from `src/lib/memory.ts`. Now `hangar_core::memory::memory_create`, sync.

```rust
pub fn memory_create(host: &Host, input: NewMemory) -> Result<MemoryEntry, String>
```

- Sends: `Event::MemoryChanged`.
- Note: an upsert by title; `workspace` is resolved to the main repository of a worktree.
- Errors: `title must not be empty`, `content must not be empty`, a limit passed (`{n} tags, the limit is …`), or `memory.json` unreadable or unwritable.

#### `memory_update`

Invoked as `memory_update` from `src/lib/memory.ts`. Now `hangar_core::memory::memory_update`, sync.

```rust
pub fn memory_update(host: &Host, id: String, patch: MemoryPatch) -> Result<MemoryEntry, String>
```

- Sends: `Event::MemoryChanged`.
- Errors: `memory entry not found`, `another entry is already titled '{title}'`, a limit passed, or a storage failure.

#### `memory_delete`

Invoked as `memory_delete` from `src/lib/memory.ts`. Now `hangar_core::memory::memory_delete`, sync.

```rust
pub fn memory_delete(host: &Host, id: String) -> Result<(), String>
```

- Sends: `Event::MemoryChanged`.
- Errors: `memory entry not found`, or a storage failure.

### 3D model and file preview

#### `model_allow_roots`

Invoked as `model_allow_roots` from `src/lib/modelFiles.ts`. Now `hangar_core::model::model_allow_roots`, async.

```rust
pub async fn model_allow_roots(roots: Vec<String>) -> Result<(), String>
```

Replaces the folders the preview may read from. The window calls it each time an agent shows a model, with the folders of every workspace that has one open, so a model shown earlier for another workspace keeps loading its textures and reloading when its file changes.

- Note: replaces, for the whole process, the folders `model::serve` and `preview_inspect` may read; relative and missing folders are dropped.

#### `model_stat`

Invoked as `model_stat` from `src/lib/modelFiles.ts`. Now `hangar_core::model::model_stat`, async.

```rust
pub async fn model_stat(path: String) -> Result<ModelFile, String>
```

Size and modification time of a model the window is about to show or is watching, or the reason it may not read it.

- Errors: the refusal explained: outside the allowed folders, not a model, over 256 MB, missing.

#### `model_save_png`

Invoked as `model_save_png` from `src/lib/modelCapture.ts`. Now `hangar_core::model::model_save_png`, async.

```rust
pub async fn model_save_png(path: String, data: String) -> Result<(), String>
```

Writes a capture of the 3D preview to the file the user chose in the save dialog.

- Errors: `{path} is not a .png file`, `the capture is not valid base64: {err}`, `the capture is not a PNG image`, `could not write {path}: {err}`.
- Note: `path` comes from the save dialog the UI shows (see "What the UI provides").

#### `preview_inspect`

Invoked as `preview_inspect` from `src/lib/preview.ts`. Now `hangar_core::preview::preview_inspect`, async.

```rust
pub async fn preview_inspect(path: String) -> Result<PreviewFile, String>
```

What a file the window is about to show is, or the reason it may not show it. The folders are those `model_allow_roots` last allowed, as for a model.

- Errors: the refusal explained, as for `model_stat`.

### MCP install and per-pane MCP scope

#### `mcp_targets`

Invoked as `mcp_targets` from `src/lib/board.ts`. Now `hangar_core::install::mcp_targets`, sync.

```rust
pub fn mcp_targets(workspace_cwd: String) -> Vec<TargetStatus>
```

- Note: one target, `claude-code` (the project's `.mcp.json`). The Codex, Gemini CLI, Cursor and VS Code targets the TS listed are gone (owner's decision, 2026-11-03).

#### `mcp_install`

Invoked as `mcp_install` from `src/lib/board.ts`. Now `hangar_core::install::mcp_install`, sync.

```rust
pub fn mcp_install(ids: Vec<String>, workspace_cwd: String) -> Vec<InstallReport>
```

- Note: never errs: one `InstallReport { id, ok, message, path }` per target. `claude-code` is the only target; installing it also approves the server in `~/.claude/settings.json`.

#### `mcp_manual_commands`

Invoked as `mcp_manual_commands` from `src/lib/board.ts`. Now `hangar_core::install::mcp_manual_commands`, sync.

```rust
pub fn mcp_manual_commands(workspace_cwd: String) -> Result<serde_json::Value, String>
```

Shown in the UI so the user can register the server by hand if they prefer.

- Errors: `std::env::current_exe`'s error.
- Note: `{ executable, claude, workspace }`: the `claude mcp add` line to run by hand. The TS's `codex mcp add` line is gone.
- Note: the window never invoked it by itself: src/lib/board.ts names it in a list of commands.

#### `mcp_servers`

Invoked as `mcp_servers` from `src/lib/mcpScope.ts`. Now `hangar_core::mcp_scope::mcp_servers`, sync.

```rust
pub fn mcp_servers(cwd: String, account: Option<String>) -> Discovery
```

The servers a Claude Code session started in `cwd` on `account` (main when absent) would load by itself.

#### `mcp_scope_write`

Invoked as `mcp_scope_write` from `src/lib/mcpScope.ts`. Now `hangar_core::mcp_scope::mcp_scope_write`, sync.

```rust
pub fn mcp_scope_write(
    host: &Host,
    workspace_id: String,
    workspace_cwd: String,
    cwd: String,
    enabled: Vec<String>,
    kits: Option<Vec<String>>,
    account: Option<String>,
) -> Result<ScopeFile, String>
```

Writes the file a pane's Claude Code is pointed at with `--mcp-config`, from the definitions as they stand right now. `workspace_cwd` is what Hangar's own server is told its workspace is — the board it reads — and `cwd` is where the pane runs, which decides the servers it would otherwise have. `kits` are the workspace's kits, given only for a file loaded with `--strict-mcp-config`: without it, the kits' plugins start their servers themselves, and a copy here would run each one twice. A machine that forbids sideloading keeps the kits out entirely, servers included. `account` is the Claude account the pane runs on: its servers are the ones its Claude Code would have, read from its profile (accounts.rs).

- Errors: `no home directory`, `no app data dir: unknown path`, I/O errors.

#### `mcp_scope_clear`

Invoked as `mcp_scope_clear` from `src/lib/mcpScope.ts`. Now `hangar_core::mcp_scope::mcp_scope_clear`, sync.

```rust
pub fn mcp_scope_clear(host: &Host, workspace_id: String) -> Result<(), String>
```

Deletes a workspace's files once it goes back to Claude Code's own configuration: they hold copies of the servers' entries, and nothing reads them any more.

- Errors: `no app data dir: unknown path`, the folder could not be listed.

#### `claude_trust_folder`

Invoked as `claude_trust_folder` from `src/lib/mcpScope.ts`. Now `hangar_core::mcp_scope::claude_trust_folder`, sync.

```rust
pub fn claude_trust_folder(folder: String, account: Option<String>) -> Result<Trusted, String>
```

Starts a workspace's Claude Code on `folder` without the trust question, for a repository that is the person's own. `account` is the workspace's Claude account (main when absent).

- Errors: `no home directory`, or the account's `.claude.json` could not be read or written.

### Kits

#### `kits_list`

Invoked as `kits_list` from `src/lib/kits.ts`. Now `hangar_core::kits::kits_list`, async.

```rust
pub async fn kits_list() -> Result<Vec<KitInfo>, String>
```

- Errors: the kits' index (`~/.hangar/kits/`) could not be read.

#### `kit_add_folder`

Invoked as `kit_add_folder` from `src/lib/kits.ts`. Now `hangar_core::kits::kit_add_folder`, async.

```rust
pub async fn kit_add_folder(host: Host, path: String) -> Result<KitInfo, String>
```

- Sends: `Event::KitsChanged`.
- Errors: `a kit folder is given by its full path`, no or a broken `hangar-kit.json`, `a kit {id:?} is already installed`, or the index could not be written.

#### `kit_add_git`

Invoked as `kit_add_git` from `src/lib/kits.ts`. Now `hangar_core::kits::kit_add_git`, async.

```rust
pub async fn kit_add_git(host: Host, url: String) -> Result<KitInfo, String>
```

- Sends: `Event::KitsChanged`.
- Errors: as `kit_add_folder`, and the clone's failure.

#### `kit_gallery_list`

Invoked as `kit_gallery_list` from `src/lib/kitGallery.ts`. Now `hangar_core::kits::gallery::kit_gallery_list`, async.

```rust
pub async fn kit_gallery_list() -> Result<Gallery, String>
```

- Note: never errs but for the blocking task's join error: a catalogue that could not be fetched is answered from the last good list, or as unavailable (`Gallery { origin, kits, fetched_at }`).

#### `kit_remove`

Invoked as `kit_remove` from `src/lib/kits.ts`. Now `hangar_core::kits::kit_remove`, async.

```rust
pub async fn kit_remove(host: Host, id: String) -> Result<(), String>
```

- Sends: `Event::KitsChanged` (also when its clone could not all be deleted).

#### `kit_check_update`

Invoked as `kit_check_update` from `src/lib/kits.ts`. Now `hangar_core::kits::kit_check_update`, async.

```rust
pub async fn kit_check_update(host: Host, id: String) -> Result<Option<UpdateInfo>, String>
```

- Sends: `Event::KitsChanged` when an update is found.

#### `kit_update`

Invoked as `kit_update` from `src/lib/kits.ts`. Now `hangar_core::kits::kit_update`, async.

```rust
pub async fn kit_update(host: Host, id: String) -> Result<KitInfo, String>
```

- Sends: `Event::KitsChanged`.

#### `kit_inspect`

Invoked as `kit_inspect` from `src/lib/kits.ts`. Now `hangar_core::kits::kit_inspect`, async.

```rust
pub async fn kit_inspect(id: String) -> Result<Inspection, String>
```

#### `kit_setup_status`

Invoked as `kit_setup_status` from `src/lib/kits.ts`. Now `hangar_core::kits::kit_setup_status`, async.

```rust
pub async fn kit_setup_status(id: String) -> Result<Vec<StepStatus>, String>
```

#### `kit_setup_mark`

Invoked as `kit_setup_mark` from `src/lib/kits.ts`. Now `hangar_core::kits::kit_setup_mark`, async.

```rust
pub async fn kit_setup_mark(id: String, step_id: String) -> Result<Vec<StepStatus>, String>
```

#### The manifest's tabs: HTML and native

New in the native port (docs/native/kits-native.md, "The manifest"); `KitInfo.manifest` is a `kits::Manifest` and nothing about the commands above changes. `hangar-kit.json` reads schema **1 and 2** (`kits::manifest::SCHEMA` is now 2, the newest read; a higher number is still the error `schema: 3 needs a newer Hangar (this one reads 2)`, and any other value `schema: must be 1 or 2`).

```rust
pub struct Tab {
    pub id: String, pub title: String,
    pub entry: String,           // an .html page, or under schema 2 a .wasm module; normalized, inside the kit
    pub native: Option<String>,  // a .wasm module beside an HTML entry (serialized only when set)
    pub icon: Option<String>, pub permissions: Vec<String>,
    pub params: BTreeMap<String, TabParam>, pub singleton: bool,
}
pub enum TabKind { Html, Native }                 // re-exported as kits::TabKind
impl Tab {
    pub fn kind(&self) -> TabKind;                // Native when the tab has a module, as `entry` or as `native`
    pub fn module(&self) -> Option<&str>;         // the .wasm to give to hangar_kit_host::KitRuntime::compile
    pub fn page(&self) -> Option<&str>;           // the .html the kit scheme serves; None for a .wasm entry
}
```

- `permissions`, `params` and `singleton` are the same for both kinds, checked by the same rules.
- A `.wasm` `entry` under schema 1 is the manifest error `tabs[0].entry: a native tab needs "schema": 2`. `native` is accepted under both schemas, beside an HTML `entry` only (`tabs[0].native: is for a tab with an HTML entry, …` otherwise).
- `entry` and `native` are normalized like every manifest path and refused when absolute, when they climb out with `..`, or when a link leads out of the kit folder; they must exist, and a module is at most 64 MB (`tabs[0].native: is larger than 64 MB`). The module is a path to give the host, never bytes: the manifest reads no module.
- A native Hangar opens `module()`; when it cannot be loaded (an ABI it does not read, a module it refuses) it falls back to `page()` in the webview helper, when the tab has one. That fallback is the shell's, not hangar-core's.
- New helpers in `kits::manifest` (module private, as before): `NATIVE_SCHEMA`, `MAX_MODULE_BYTES`, `is_module(path)`.

### Kit tabs' calls

What a kit's tab asks through the SDK (docs/kits.md), checked by the window against what the person granted the kit before the call (src/lib/kitApi.ts). The `kits::*` ones answer `Err(kits::TabError { code, message })`, `code` one of the `TabError::*` constants — `invalid`, `not_found`, `unreachable`, `failed`, `denied`, `unavailable`, `bad_args`, `busy`, `conflict`, `unsupported` — which the window passed on to the page as is.

#### `kit_data_read`

Invoked as `kit_data_read` from `src/components/preview/KitTabView.tsx`. Now `hangar_core::kits::data::kit_data_read`, async.

```rust
pub async fn kit_data_read(kit: String, path: String) -> Result<String, TabError>
```

`data.read`. `kit` is the window's own record of the tab, never the page's.

- Errors: `kits::TabError`, as said at the start of this section.

#### `kit_data_write`

Invoked as `kit_data_write` from `src/components/preview/KitTabView.tsx`. Now `hangar_core::kits::data::kit_data_write`, async.

```rust
pub async fn kit_data_write(kit: String, path: String, text: String) -> Result<(), TabError>
```

`data.write`. `kit` is the window's own record of the tab, never the page's.

- Errors: `kits::TabError`, as said at the start of this section.

#### `kit_files_read`

Invoked as `kit_files_read` from `src/components/preview/KitTabView.tsx`. Now `hangar_core::kits::files::kit_files_read`, async.

```rust
pub async fn kit_files_read(root: String, path: String) -> Result<String, TabError>
```

`files.read` for a tab holding `files.write` as well. `root` is the window's own record of the tab, never the page's, as for all of these.

- Errors: `kits::TabError`, as said at the start of this section.

#### `kit_files_write`

Invoked as `kit_files_write` from `src/components/preview/KitTabView.tsx`. Now `hangar_core::kits::files::kit_files_write`, async.

```rust
pub async fn kit_files_write(
    root: String,
    path: String,
    text: String,
    expected: Option<String>,
) -> Result<(), TabError>
```

`files.write`.

- Errors: `kits::TabError`, as said at the start of this section.

#### `kit_files_write_binary`

Invoked as `kit_files_write_binary` from `src/components/preview/KitTabView.tsx`. Now `hangar_core::kits::files::kit_files_write_binary`, async.

```rust
pub async fn kit_files_write_binary(
    root: String,
    path: String,
    base64: String,
) -> Result<(), TabError>
```

`files.writeBinary`.

- Errors: `kits::TabError`, as said at the start of this section.

#### `kit_files_read_binary`

Invoked as `kit_files_read_binary` from `src/components/preview/KitTabView.tsx`. Now `hangar_core::kits::files::kit_files_read_binary`, async.

```rust
pub async fn kit_files_read_binary(
    root: String,
    path: String,
    hidden: bool,
) -> Result<BinaryFile, TabError>
```

`files.readBinary`. `hidden`: the tab holds `files.write` as well.

- Errors: `kits::TabError`, as said at the start of this section.

#### `kit_files_list`

Invoked as `kit_files_list` from `src/components/preview/KitTabView.tsx`. Now `hangar_core::kits::files::kit_files_list`, async.

```rust
pub async fn kit_files_list(
    root: String,
    path: Option<String>,
    pattern: Option<String>,
) -> Result<Listing, TabError>
```

`files.list`.

- Errors: `kits::TabError`, as said at the start of this section.

#### `kit_loopback_fetch`

Invoked as `kit_loopback_fetch` from `src/components/preview/KitTabView.tsx`. Now `hangar_core::kits::loopback::kit_loopback_fetch`, async.

```rust
pub async fn kit_loopback_fetch(
    port: u16,
    path: String,
    method: Option<String>,
    body: Option<String>,
    timeout_ms: Option<u64>,
) -> Result<Answer, TabError>
```

`loopback.fetch`. The window checked the tab holds `loopback:<port>`.

- Errors: `kits::TabError`, as said at the start of this section.

#### `kit_command_run`

Invoked as `kit_command_run` from `src/components/preview/KitTabView.tsx`. Now `hangar_core::kits::commands::kit_command_run`, async.

```rust
pub async fn kit_command_run(
    kit: String,
    name: String,
    args: HashMap<String, String>,
    cwd: String,
    accepted: Option<String>,
) -> Result<Output, TabError>
```

`command.run`. The window checked the tab holds `command:<name>`.

- Errors: `kits::TabError`, as said at the start of this section.

#### `kit_file_read`

Invoked as `kit_file_read` from `src/components/preview/KitTabView.tsx`. Now `hangar_core::kit_scheme::files::kit_file_read`, async.

```rust
pub async fn kit_file_read(root: String, path: String) -> Result<String, String>
```

`files.read`: the text of `path`, relative to the workspace folder `root`.

- Errors: a sentence: outside the folder, a link out of it, not text, too large, missing.

#### `kit_workspace_path`

Invoked as `kit_workspace_path` from `src/components/preview/KitTabView.tsx`. Now `hangar_core::kit_scheme::files::kit_workspace_path`, async.

```rust
pub async fn kit_workspace_path(root: String, path: String) -> Result<String, String>
```

`preview.show`: the canonical path of the file `path` names inside the workspace folder `root`, for the preview to open.

- Errors: as `kit_file_read`.

### Agent instructions

#### `write_agent_instructions`

Invoked as `write_agent_instructions` from `src/lib/board.ts`. Now `hangar_core::instructions::write_agent_instructions`, sync.

```rust
pub fn write_agent_instructions(workspace_cwd: String) -> Vec<InstructionReport>
```

- Note: never errs: one `InstructionReport { path, ok, message }` per file: `CLAUDE.md` and `.gitignore`. `AGENTS.md`, which the TS wrote too for the other agents, is no longer written.

#### `agent_instructions_status`

Invoked as `agent_instructions_status` from `src/lib/board.ts`. Now `hangar_core::instructions::agent_instructions_status`, sync.

```rust
pub fn agent_instructions_status(workspace_cwd: String) -> bool
```

Lets the UI show whether the playbook is already in place, in `CLAUDE.md`.

### Notifications

#### `notify_send`

Invoked as `notify_send` from `src/lib/notify.ts`. Now `hangar_core::notify::notify_send`, sync.

```rust
pub fn notify_send(
    host: &Host,
    title: String,
    body: String,
    workspace_id: String,
    pane_id: String,
) -> Result<(), String>
```

- Note: Windows: a toast that, clicked, sends `Event::NotificationActivated(Activated { workspace_id, pane_id })`. Elsewhere it always fails, `clickable notifications are only implemented on Windows`, and the window fell back to the plain notification: `notify::show(title, body)` now.

### Discord

#### `discord_presence_set`

Invoked as `discord_presence_set` from `src/lib/ipc.ts`. Now `hangar_core::discord::discord_presence_set`, sync.

```rust
pub fn discord_presence_set(presence: &DiscordPresence, wanted: Option<Presence>)
```

- Note: `presence` is `core.discord()`. Returns at once; a worker thread talks to Discord.

#### `discord_presence_status`

Invoked as `discord_presence_status` from `src/lib/ipc.ts`. Now `hangar_core::discord::discord_presence_status`, sync.

```rust
pub fn discord_presence_status(presence: &DiscordPresence) -> Status
```

### Dictation

#### `voice_start`

Invoked as `voice_start` from `src/lib/ipc.ts`. Now `hangar_core::voice::voice_start`, async.

```rust
pub async fn voice_start(
    host: Host,
    manager: Arc<VoiceManager>,
    config: VoiceConfig,
) -> Result<(), String>
```

Starts recording. Fails if there is no microphone, if the chosen engine cannot run, or if a recording is already open — which the frontend prevents, but a stuck key would not.

- Sends: `Event::VoicePhase` (`recording`), then `Event::VoiceLevel` every 60 ms until the recording ends.
- Errors: `already recording`, `the local model is not downloaded yet — Settings → Voice`, the Groq key missing or unreadable, `capture thread failed: {err}`, no microphone.
- Note: `manager` is `core.voice().clone()`.

#### `voice_stop`

Invoked as `voice_stop` from `src/lib/ipc.ts`. Now `hangar_core::voice::voice_stop`, sync.

```rust
pub fn voice_stop(host: &Host, manager: &Arc<VoiceManager>, config: VoiceConfig, cancel: bool)
```

Ends the recording. With `cancel`, the audio is dropped and nothing is transcribed — the escape hatch for a dictation you thought better of.

- Note: returns at once. `cancel`: `Event::VoicePhase` `idle`. Otherwise `transcribing`, then on a thread `idle` and `Event::VoiceText` with what was said, or `idle` alone when nothing was, or `idle` with `error` set when it failed.

#### `voice_recording`

Invoked as `voice_recording` from `src/lib/ipc.ts`. Now `hangar_core::voice::voice_recording`, sync.

```rust
pub fn voice_recording(manager: &VoiceManager) -> bool
```

True while a recording is open, so the interface can recover its own state after a reload rather than showing a microphone that is not running.

#### `voice_unload`

Invoked as `voice_unload` from `src/lib/ipc.ts`. Now `hangar_core::voice::voice_unload`, sync.

```rust
pub fn voice_unload(manager: &VoiceManager)
```

Drops the resident model, freeing its memory until the next dictation.

#### `voice_models`

Invoked as `voice_models` from `src/lib/ipc.ts`. Now `hangar_core::voice::models::voice_models`, sync.

```rust
pub fn voice_models(host: &Host) -> Vec<ModelInfo>
```

#### `voice_model_download`

Invoked as `voice_model_download` from `src/lib/ipc.ts`. Now `hangar_core::voice::models::voice_model_download`, async.

```rust
pub async fn voice_model_download(host: Host, id: String) -> Result<(), String>
```

Downloads a model, reporting progress as `Event::VoiceDownload`.

- Sends: `Event::VoiceDownload(Progress { id, received, total, done })` at most every 200 ms, and once with `done: true`.
- Errors: `unknown model {id}`, `could not download {file}: {err}`, `cannot write {file}: {err}`, `download of {file} failed: {err}`, `cannot install the model: {err}`, `download thread failed: {err}`.

#### `voice_key_status`

Invoked as `voice_key_status` from `src/lib/ipc.ts`. Now `hangar_core::voice::key::voice_key_status`, async.

```rust
pub async fn voice_key_status() -> Result<bool, String>
```

`true` when a key is saved. An error means the keychain itself could not be asked, which the settings show as such rather than as "no key".

- Errors: the keychain could not be asked.

#### `voice_key_set`

Invoked as `voice_key_set` from `src/lib/ipc.ts`. Now `hangar_core::voice::key::voice_key_set`, async.

```rust
pub async fn voice_key_set(key: String) -> Result<(), String>
```

- Errors: the keychain refused.

#### `voice_key_clear`

Invoked as `voice_key_clear` from `src/lib/ipc.ts`. Now `hangar_core::voice::key::voice_key_clear`, async.

```rust
pub async fn voice_key_clear() -> Result<(), String>
```

- Errors: the keychain refused.

### Hangar Mobile

`remote` is `core.remote()`: `None` when Hangar Mobile could not open its store, in which case the Tauri window got an error for every call that needed it, Tauri refusing a command whose state was never managed; show Hangar Mobile as unavailable. `remote_set_autostart` and `remote_ports_listening` need no `Remote`.

#### `remote_status`

Invoked as `remote_status` from `src/lib/remote.ts`. Now `hangar_core::remote::remote_status`, async.

```rust
pub async fn remote_status(remote: Arc<Remote>) -> Result<RemoteStatus, String>
```

#### `remote_set_enabled`

Invoked as `remote_set_enabled` from `src/lib/remote.ts`. Now `hangar_core::remote::remote_set_enabled`, async.

```rust
pub async fn remote_set_enabled(
    remote: Arc<Remote>,
    enabled: bool,
) -> Result<RemoteStatus, String>
```

- Errors: the listener could not start (the port taken: it stays off and says so), or `remote.json` could not be written.

#### `remote_pair_start`

Invoked as `remote_pair_start` from `src/lib/remote.ts`. Now `hangar_core::remote::remote_pair_start`, async.

```rust
pub async fn remote_pair_start(remote: Arc<Remote>) -> Result<PairStart, String>
```

- Errors: `Hangar Mobile is off: turn it on first.`, Tailscale not installed or not connected (`remote::tailscale` messages).

#### `remote_devices`

Invoked as `remote_devices` from `src/lib/remote.ts`. Now `hangar_core::remote::remote_devices`, sync.

```rust
pub fn remote_devices(remote: &Remote) -> Vec<DeviceView>
```

#### `remote_revoke`

Invoked as `remote_revoke` from `src/lib/remote.ts`. Now `hangar_core::remote::remote_revoke`, sync.

```rust
pub fn remote_revoke(remote: &Remote, id: String) -> Result<(), String>
```

- Sends: `Event::RemoteChanged`.
- Errors: `remote.json` could not be written; a device already gone is no error.

#### `remote_tailscale`

Invoked as `remote_tailscale` from `src/lib/remote.ts`. Now `hangar_core::remote::remote_tailscale`, async.

```rust
pub async fn remote_tailscale(remote: Arc<Remote>) -> Result<TailscaleStatus, String>
```

- Note: never fails: `TailscaleStatus` says what is missing.

#### `remote_tailscale_serve`

Invoked as `remote_tailscale_serve` from `src/lib/remote.ts`. Now `hangar_core::remote::remote_tailscale_serve`, async.

```rust
pub async fn remote_tailscale_serve(remote: Arc<Remote>) -> Result<TailscaleStatus, String>
```

Runs `tailscale serve`: only ever from the person's click in the settings.

- Errors: the Tailscale CLI's refusal or its time limit.

#### `remote_push`

Invoked as `remote_push` from `src/lib/remotePush.ts`. Now `hangar_core::remote::remote_push`, async.

```rust
pub async fn remote_push(
    remote: Arc<Remote>,
    title: Text,
    body: Text,
    url: String,
) -> Result<PushOutcome, String>
```

A notification to every subscribed phone. `title` and `body` are one text or `{ en, fr }`, each phone getting its language; `url` is the screen it opens, `./#/pane/<id>`, `./#/shares`, `./#/`.

- Errors: `the push failed: {err}` (the key, the encryption); per-phone failures are counted in `PushOutcome`.

#### `remote_push_subscribers`

Invoked as `remote_push_subscribers` from `src/lib/remotePush.ts`. Now `hangar_core::remote::remote_push_subscribers`, async.

```rust
pub async fn remote_push_subscribers(remote: Arc<Remote>) -> Result<usize, String>
```

How many phones take notifications, for the window to know whether to watch for what it would push.

#### `remote_options`

Invoked as `remote_options` from `src/components/settings/mobileOptions.ts`. Now `hangar_core::remote::remote_options`, sync.

```rust
pub fn remote_options(remote: &Remote) -> Options
```

The two options that keep Hangar reachable: the PC kept awake, and Hangar started at login.

- Note: `autostart` is read from the login item (autostart.rs), `false` when it cannot be.

#### `remote_set_keep_awake`

Invoked as `remote_set_keep_awake` from `src/components/settings/mobileOptions.ts`. Now `hangar_core::remote::remote_set_keep_awake`, sync.

```rust
pub fn remote_set_keep_awake(remote: &Remote, enabled: bool) -> Result<(), String>
```

- Errors: `remote.json` could not be written.

#### `remote_set_autostart`

Invoked as `remote_set_autostart` from `src/components/settings/mobileOptions.ts`. Now `hangar_core::remote::remote_set_autostart`, sync.

```rust
pub fn remote_set_autostart(enabled: bool) -> Result<(), String>
```

- Note: no `Remote` needed. The login item is `Hangar.AI`, the running executable, no arguments (autostart.rs).
- Errors: auto-launch's message (the item could not be written or removed).

#### `remote_ports_listening`

Invoked as `remote_ports_listening` from `src/lib/remoteApps.ts`. Now `hangar_core::remote::remote_ports_listening`, async.

```rust
pub async fn remote_ports_listening(ports: Vec<u16>) -> Vec<u16>
```

Which of these ports a server listens on, on this PC's loopback: the dev servers the window can offer the phone. Asked off the main thread, each port given half a second at most, all at once.

- Note: no `Remote` needed; half a second per port at most, all at once.

#### `remote_app_slots`

Invoked as `remote_app_slots` from `src/lib/remote.ts`. Now `hangar_core::remote::remote_app_slots`, async.

```rust
pub async fn remote_app_slots(remote: Arc<Remote>) -> Result<Vec<SlotView>, String>
```

For the settings: each slot, the app it shows and when it was last used, and what Tailscale Serve does with its port.

#### `activity_append`

Invoked as `activity_append` from `src/lib/activityLog.ts`, `src/lib/workspaceFromGit.ts`. Now `hangar_core::remote::activity_append`, sync.

```rust
pub fn activity_append(remote: &Remote, entries: Vec<ActivityEntry>) -> Result<(), String>
```

- Errors: the day's history file could not be written.

#### `activity_read`

Invoked as `activity_read` from `src/lib/activityLog.ts`. Now `hangar_core::remote::activity_read`, sync.

```rust
pub fn activity_read(
    remote: &Remote,
    since: Option<u64>,
    limit: Option<usize>,
) -> Vec<ActivityEntry>
```

Newest first, after `since` (ms), at most `limit`.

## The two schemes

The Tauri app registered two URI schemes on its webview; both are pure
functions now, for whichever webview registers them (the `hangar-webview`
helper) and for the native preview, which can ask them directly:

```rust
pub fn hangar_core::model::serve(request: &http::Request<Vec<u8>>, origins: &[String]) -> http::Response<Vec<u8>>
pub fn hangar_core::kit_scheme::serve(request: &http::Request<Vec<u8>>, origins: &[String]) -> http::Response<Vec<u8>>
```

- `model::SCHEME` is `"hangar-model"`: the preview's files (3D models and
  what they pull in, images, video, audio, text, Markdown, PDF, pages), read
  only from the folders `model_allow_roots` last allowed, only for the types
  model.rs and preview.rs know, with `Range` (one byte range), `HEAD` and
  `OPTIONS`. URLs are `hangar-model://localhost/<absolute path, one encoded
  segment per component>` (`http://hangar-model.localhost/…` on Windows),
  as `modelUrl` in src/lib/modelFiles.ts builds them; `?as=text` asks for a
  page's or an SVG's source.
- `kit_scheme::SCHEME` is `"hangar-kit"`: `<base>/<kit id>/<path in the kit>`
  for an installed, usable kit, and `/_hangar/kit.js`, `/_hangar/kit-ui.js`,
  `/_hangar/kit.css`, `/_hangar/inter.woff2` for every tab (docs/kits.md,
  docs/kit-ui.md). Every answer carries a `sandbox` Content-Security-Policy.
  A `.wasm` file that is the `entry` of a native tab (`Tab::page()` is `None`)
  is refused with `403` and a sentence saying Hangar runs it itself: it is no
  page. Any other `.wasm` of the kit is served (`application/wasm`), including
  the `native` module beside an HTML `entry`, since a page may load its own.
- `origins` are the origins of the pages the caller serves Hangar's own
  interface from: the only pages allowed to `fetch` these files with an
  `Origin` header. The Tauri window's were `tauri://localhost`,
  `http://tauri.localhost` and `https://tauri.localhost` (and its dev
  server's during `tauri dev`). A request without an `Origin` (a navigation,
  an `<img>`) is answered whatever they are, and a kit's own pages, whose
  origin the sandbox makes `null`, may read kit files.
- Both read the disk where they are called: run them off the UI thread (Tauri
  ran them on its blocking pool). A body is whole in memory, so files are
  capped: 256 MB for a model and what it pulls in, a size per kind for the
  preview's other files, 8 MB of video or audio per response (a player asks
  for the rest by range), 64 MB for a kit file.
- What preview.rs and kit_scheme.rs say about Tauri's IPC is what a webview
  serving these schemes owes their pages: no IPC of its own reachable from a
  document of either scheme.

## What replaced Tauri's plugins

| Plugin | Who used it | Now |
|---|---|---|
| `tauri-plugin-notification` | src/lib/notify.ts: `sendNotification({ title, body })`, and the permission check | `hangar_core::notify::show(title: String, body: String)`: notify-rust on macOS and Linux, the `tauri-winrt-notification` toast on Windows, as the plugin drew them: the app's icon; on Windows filed under `com.simnjs.iabench` when installed, under PowerShell's AUMID from a `target/debug` or `target/release` binary; on macOS under the bundle, or under Terminal for a debug build. Shown from a thread, no answer. There is no permission to ask: the plugin answered "granted" on every desktop |
| (Rust side) clickable toast | `notify_send` | unchanged, Windows only; the click is `Event::NotificationActivated` |
| `tauri-plugin-opener` | `openUrl` (src/components/preview/MarkdownView.tsx, src/components/settings/MobileSettings.tsx), `openPath` and `revealItemInDir` (src/lib/preview.ts) | `hangar_core::opener::open_url(url: &str) -> Result<(), String>`: `http`, `https`, `mailto`, `tel` only, as the plugin's default scope; `opener::open_path(path: &str) -> Result<(), String>`: PDF files only, as the capability scoped it (`**/*.pdf`), the file must exist; `opener::reveal_item_in_dir(path: &str) -> Result<(), String>`: the file manager with the file selected (Explorer `/select,`, `open -R`, the file manager's `org.freedesktop.FileManager1.ShowItems`, else its folder opened). preview.ts's `openExternally` tried `open_path`, then `reveal_item_in_dir` when refused |
| `tauri-plugin-autostart` | the Mobile settings' own commands | `hangar_core::autostart::{is_enabled, set_enabled}`, behind `remote_options` and `remote_set_autostart`: the `auto-launch` crate the plugin wrapped, at the version it locked (0.5.0), with what it gave it — the name `Hangar.AI`, the running executable (the AppImage on Linux when there is one, canonical on macOS), no arguments, a launch agent on macOS — so the login item an earlier version made is the one found |
| `tauri-plugin-dialog` | see below | the UI |
| `tauri-plugin-updater`, `tauri-plugin-process` | src/components/UpdateBanner.tsx | the UI |

## What the UI provides

What Tauri, its plugins or the window's own JavaScript did, and hangar-core
does not:

- **Windows.** windows.rs is gone. It opened a workspace in a window of its
  own (`open_workspace_window`, src/lib/windows.ts): the ids checked
  (non-empty, at most 64 characters, ASCII letters, digits, `-`, `_`; the
  same for the pane ids in `resume`), the window labelled
  `workspace-<id>`, titled `<title> — Hangar.AI`, at least 640×420, showing
  that workspace (and starting the `resume` panes on their sessions whatever
  the auto-resume setting). It opened at the saved `bounds { x, y, width,
  height }` (logical pixels) when their title bar — the point half way
  across, 20 px down — is on a connected screen; otherwise centred on a screen
  other than the main window's, at 85 % of it and at most 1360×860, placed in
  physical pixels after creation; otherwise 1200×800, centred. An open one
  was shown, unminimised and focused instead. `focus_workspace_window(id) ->
  bool` did that to an open one; `close_workspace_window(id)` destroyed it and
  brought the main window forward. Its close button did not close it: the
  window was sent `window:close-requested`, handed its panes back to the main
  window (their shells keep running) and then called
  `close_workspace_window`; a window that had not done so 4 s later was
  destroyed anyway. The main window is 1360×860, at least 900×560, centred,
  titled `Hangar.AI`; its end is the app's (`Core::shutdown`, then exit). The
  windows also saved their bounds as they moved and resized, set their title
  and full screen (src/App.tsx), and took files dropped on them
  (src/lib/fileDrop.ts).
- **A notification's click.** On `Event::NotificationActivated { workspace_id,
  pane_id }`: the workspace's own window when it has one, else the main one,
  shown, unminimised and focused — and when Windows refuses the focus, which
  it does silently, the taskbar button flashed (`UserAttentionType::Critical`)
  — then the pane revealed. Notifications that name no pane, and every one
  outside Windows, are `notify::show`.
- **Dialogs** (`tauri-plugin-dialog`; `rfd` is in the workspace): a folder
  for a new workspace, for the parent of a clone (src/components/WorkspaceDialog.tsx),
  for a kit (src/components/settings/KitsSettings.tsx) and for the projects
  folder setting (src/components/settings/schema.tsx); a `.code-workspace`
  file (WorkspaceDialog.tsx); where to save a capture of the 3D preview, a
  `.png` (src/lib/modelCapture.ts), whose path goes to `model_save_png`.
- **Updates** (`tauri-plugin-updater`): the banner checked
  `https://github.com/hangar-ai/hangar/releases/latest/download/latest.json`
  against the minisign public key in src-tauri/tauri.conf.json
  (`plugins.updater.pubkey`), downloaded and installed the update with its
  progress, and offered to restart.
- **Restarting and exiting** (`tauri-plugin-process`): `relaunch()` after an
  update; the exit after `Core::shutdown`.
- **The version** (`getVersion()` from `@tauri-apps/api/app`, in the sidebar
  and the settings): the app's `CARGO_PKG_VERSION`, the workspace's;
  `hangar_core::kits::HANGAR_VERSION` is the same number.
- **The webview's schemes**: registering `hangar-model` and `hangar-kit`
  with `model::serve` and `kit_scheme::serve`, off the UI thread, and the
  origins of its own pages.
- **What the windows told each other**: the store mirrored to the workspace
  windows, the 3D viewer handed between windows, a terminal's questions asked
  across windows, the kit grant prompts (see "Events"); `board:changed` after
  a delivery, which the window sent itself (`Host::emit` takes it).
- **The console window** on Windows: `#![cfg_attr(not(debug_assertions),
  windows_subsystem = "windows")]` on the binary, and `--mcp` handled before
  anything else (src-tauri/main.rs.legacy).

## Decisions taken in the port

- `Host` is the folders and the event bus only. The managers Tauri held as
  state are the `Core`'s and are passed explicitly, so a function's arguments
  say everything it touches.
- `Host::emit` fails when nothing is subscribed. Every caller ignores that
  but two: a control request, which then fails at once instead of waiting
  20 s for a window that is not there, and the dictation's level meter, which
  stops.
- The bus keeps 1024 events per subscriber; a pane's output never goes
  through it (a per-pane `FnMut` sink, set once at `pty_spawn`).
- Async functions take their arguments by value so their futures are
  `'static`; their blocking parts moved from `tauri::async_runtime` to
  `tokio::task::spawn_blocking`.
- The schemes take the origins of the caller's own pages rather than
  assuming Tauri's. `kit_scheme::serve` reads the kits' index synchronously
  (`kits::list_now`) where Tauri's handler awaited `kits_list`.
- `notify_send` stays Windows only, and fails elsewhere as before; the plain
  notification the window fell back to is `notify::show`. Tauri's `is_dev`
  (which filed a macOS notification under Terminal) is a debug build here.
- The opener keeps the capability's limits (link schemes, PDF only); the PDF
  extension is compared without case. `reveal_item_in_dir` asks the platform
  itself, through zbus on Linux as the plugin did, since the `open` crate has
  no "reveal".
- The login item is the auto-launch crate at the plugin's version and with
  its settings, so it stays the same item.
- Types: every struct field the window read or sent is public, and the
  types of private submodules are re-exported (`kits`, `remote`, `stats`); the
  constants that named Tauri events (`kits::CHANGED_EVENT`,
  `accounts::CHANGED_EVENT`) went, `Event::name()` keeps the names.
- Cargo.lock: adding notify-rust, auto-launch and zbus re-resolved cpal and
  iana-time-zone onto `windows`/`windows-core` 0.62; both were put back on
  0.61 by hand (see the cpal comment in crates/hangar-core/Cargo.toml).
