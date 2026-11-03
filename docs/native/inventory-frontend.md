# Hangar native: frontend inventory and porting map

This is the map of the React frontend (`src/`, branch `feat/native-rust`) for the iced 0.14 rewrite. It is written so that an agent with no other context can take **one work package (§8)** and port it.

- Each package lists the files it owns, the TypeScript it ports, what it needs from the shell, and the sections of this document to read.
- The backend (`crates/hangar-core`) is out of scope, except where the frontend's contract with it matters.
- `src/mobile` stays a web app. §5/2 lists what the desktop must keep serving for it.

Inventory written 2026-11-03 from commit `44cfd25`, by reading the whole frontend.

---

## 0. Read me first

### 0.1 Size of what is ported

| Part | Files | Lines |
|---|---|---|
| Desktop TS/TSX, not tests (`src/` minus `src/mobile`) | 206 | **62,389** |
| Desktop tests: vitest `*.test.ts`, 84 files | 84 | **23,316** (1,653 `it`/`test` cases) |
| Desktop CSS (`styles.css` 8,361, `styles/*.css` 4,854, component CSS 1,050) | 14 | **14,265** |
| `src/components/**/*.tsx` | 77 | 25,697 |
| `src/lib/**` (logic, hooks, IPC wrappers, `model3d/`), not tests | 99 | 26,258 |
| `src/mobile/**`: the phone web app, **not ported** | 72 | 19,547 |

The frontend calls the backend in two ways:
- **133** distinct `invoke("…")` command names. The backend declares 135 `#[tauri::command]`s.
- **19** backend → window event names (§1.9).

### 0.2 Conventions used everywhere below

- **`invoke x`** is the Tauri command name the frontend calls today. In native code it is a direct call into `hangar-core`, in the module of the same name (`board_load` → `hangar_core::board`, `git_*` → `hangar_core::git`, and so on). Commands that still take an `AppHandle` need a native variant from the backend agent.
- **`event x`** is a Tauri event.
  - Backend → window events (`pty:output`, `board:changed`, …) become a `hangar-core` broadcast channel, which the app turns into an `iced::Subscription`. P0 pre-creates `CoreEvent` (§8.2).
  - Window ↔ window events disappear in native: `store:state`, `store:hello`, `terminal:ask`/`answer`, `model:*`, `agent:activity`, `window:focus-pane`, `kits:grant-roots`. One iced daemon holds one model for all windows (§1.7).
- **`t("k")`** is an i18n key. Plurals are `k_one`/`k_other`, called as `t("k", {n})`. Placeholders are `{name}` (§6).
- **References** like `§2.1/5` mean the heading numbered 5 inside section 2.1, and `§4/1.5` means heading 1.5 inside section 4. The embedded area maps keep their own numbering.
- **LOC** is `wc -l`. A "test case" is a `it(`/`test(` line.
- **Kinds of module:**
  - *pure*: port 1:1 with its tests.
  - *IO*: calls the backend.
  - *hook* or *store*: React state or a module-level store. It becomes app state plus `update` plus `Subscription`, never a global.
- **"Out of scope" inside an area map** (§2.1–2.4, §4, §5) means "covered in another section of this document". Use §2.0 and §3 to find the owner. The phrase "mapped elsewhere" means the same.
- **Behaviour oracle.** The vitest suites are the specification. Port each test file next to its Rust module as `#[cfg(test)]`, keeping the same inputs and expected strings.

### 0.3 Breakages found on this branch (fix before using the TS as an oracle)

1. **Mobile assets are no longer embedded.** `crates/hangar-core/src/remote/assets.rs:19` has `#[folder = "../dist-mobile/"]`. This is relative to the crate, so it now points at `crates/dist-mobile/`. Vite writes `<repo>/dist-mobile/` (`vite.mobile.config.ts:72`). Because of `allow_missing`, the build silently serves the placeholder page.
   - Fix: `"../../dist-mobile/"`.
2. **The kit SDK moved, but its imports did not.** It now lives in `crates/hangar-core/kit-sdk/`, but these still import `../../src-tauri/kit-sdk/…`:
   - `src/lib/kitTabs.ts:3`
   - `src/lib/kitTabs.test.ts`, `kitGallery.test.ts`, `hangarUi.test.ts`
   - `scripts/gen-hangar-ui.mjs:20`

   `pnpm build` and `pnpm test` fail on these until the paths are updated.

---

## 1. App structure

### 1.1 Entry points

| File | LOC | Role |
|---|---|---|
| `index.html` → `src/main.tsx` | 12 | `createRoot(#root).render(<StoreProvider><App/></StoreProvider>)`. **No StrictMode**: its double effects would spawn every PTY twice. |
| `src/stylesheet.ts` | 13 | Imports, in cascade order: `@fontsource-variable/inter/opsz.css`, `styles.css`, `styles/{chrome,inspector,palette,model,mcp-scope,stats,orchestration}.css`. |
| `src/store.tsx` | 722 | `StoreProvider` / `useStore()`: the only persisted state (§1.2–1.4). |
| `src/App.tsx` | 1,912 | The whole window: transient state (§1.5), side-panel placement, regions, shortcut handler table, multi-window, control wiring, render tree (§1.11). |
| `src/types.ts` | 536 | The persisted model types, `AGENTS`, `DEFAULT_SETTINGS`, voice constants. |
| `src/themes.ts` | 1,262 | 33 terminal themes plus the chrome token derivation (§7). |
| `src/i18n.ts` | 3,182 | en/fr dictionaries plus `translate` (§6). |

**Native equivalent** (P0):
- `crates/hangar-app/src/main.rs`:
  - `hangar-ai --mcp` → `hangar_core::run_mcp()`, which is stdio only: no window, as today in `hangar-core/src/lib.rs:42`;
  - otherwise `iced::daemon(...)` with one `App` holding every window.

### 1.2 Persisted model: `AppState` (`src/types.ts`)

```ts
AppState  { workspaces: Workspace[]; folders: WorkspaceFolder[] /* sidebar order */; activeWorkspaceId: string|null; settings: Settings }
Workspace { id; name; cwd; extraRoots: string[]; panes: Pane[]; themeId: string|null; shellId: string|null;
            tree: SplitNode|null; savedCommands: SavedCommand[]; folderId: string|null;
            detached?: boolean; windowBounds?: {x,y,width,height}|null /* logical px */;
            mcp?: {mode:"inherit"|"custom"; enabled: string[]};  kits?: {id; granted: string[]}[];
            orchestration?: {maxWorkers:3; maxAttempts:2; autoMerge:true; checkCommand:null; orchestratorModel:"opus";
                             workerModel:"sonnet"; reviewerModel:"sonnet"; branch:null; permissionMode:null /* auto|acceptEdits|bypassPermissions */};
            hq?: boolean; mobilePorts?: number[]; claudeAccount?: string|null; worktreeHooks?: {postCreate?; preRemove?} }
Pane      { id; name /* unique across ALL workspaces */; agent: "shell"|"claude"|"codex"|"gemini"|"opencode";
            sessionId: string|null; cwd: string|null; shellId: string|null; title: string|null;
            role?: "orchestrator"|"worker"|"manager"; taskId?: string|null; model?: string|null; portBase?: number|null;
            sharedFolder?: boolean; claudeAccount?: string|null; login?: boolean }
SplitNode = {type:"pane"; id} | {type:"split"; dir:"row"|"col"; ratio: number; a: SplitNode; b: SplitNode}
SavedCommand { id; label; command; autoRun: boolean; broadcast: boolean }
WorkspaceFolder { id; name; collapsed: boolean }   // one level deep; a folder owns nothing
```

The orchestration values shown above are the defaults (`ORCHESTRATION_DEFAULTS`, `lib/orchestration.ts:15`).

**`AGENTS`:**

| id | label | resumable |
|---|---|---|
| shell | Shell | no |
| claude | Claude Code | yes |
| codex | Codex | yes |
| gemini | Gemini | no |
| opencode | OpenCode | no |

**`Settings` fields and defaults** (`DEFAULT_SETTINGS`). §2.3/2 has the UI controls.

| Field | Default |
|---|---|
| themeId | `"graphite"` |
| locale | `null` (follow the OS) |
| shellId | `null` |
| fontFamily | `'"Cascadia Code", "JetBrains Mono", Consolas, "Courier New", monospace'` |
| fontSize | 13 |
| lineHeight | 1.3 |
| letterSpacing | 0 |
| cursorStyle | `"bar"` |
| cursorBlink | true |
| scrollback | 10000 |
| padding | 10 |
| agentCommands | `{}` |
| autoResume | true |
| hibernateIdle | true |
| hibernateAfterMin | 10 |
| claudeBridge | true |
| launchDelayMs | 700 |
| notifyOnIdle | true |
| notifyIdleMs | 3000 |
| notifyOnlyWhenAway | true |
| keybindings | `{}` (overrides only; `[]` disables a command) |
| savedCommands | `[]` |
| voiceEnabled | false |
| voiceEngine | `"local"` |
| voiceModel | `"whisper-small-q5_1"` |
| voiceCloudModel | `"whisper-large-v3-turbo"` |
| voiceLanguage | `""` |
| voiceApiKey? | absent (legacy, migrated by the backend) |
| voiceCleanup | false |
| voiceCleanupModel | `""` |
| voiceCleanupHint | `""` |
| voiceSubmit | false |
| discordPresence | false |
| discordShowWorkspace | true |
| discordShowAgents | true |
| discordAppId | `""` |
| boardDockOpen | false |
| boardDockWidth | 420 |
| modelPanelWidth | 440 |
| navigatorOpen | true |
| inspectorOpen | true |
| inspectorWidth | 296 |
| inspectorCollapsed | `[]` |
| uiScale | 100 (range 90–130) |
| agentPaneControl | true |
| claudeAccount | `"main"` |
| experiments | `[]` |
| kitsEverywhere? | absent |
| projectsFolder? | absent |

### 1.3 Store actions (`src/store.tsx`): the only way the persisted model changes

Every write goes through `update(fn)`:
1. It applies `fn` to a ref **synchronously**, so `snapshot()` reads back what the same tick wrote.
2. It calls `setState`.
3. It broadcasts the whole state once per tick to the other windows.

`snapshot()` is used by ~30 modules: control, engines and the preview store all read the latest state. In Rust, `StoreAction` is applied in `update()`, and `Model` is the snapshot. Engines that run off the UI thread read an `Arc<Snapshot>` that is republished after every update (§5).

| Action | Semantics to keep | Called from |
|---|---|---|
| `addWorkspace(draft) → id` | `makeWorkspace`: `layout` panes, names picked against **all** workspaces, `presetTree`, `kits`/`claudeAccount` written **only when set**. It becomes the active workspace. | App (git flow), WorkspaceDialog, WorktreePanel, orchestrationActions (HQ) |
| `removeWorkspace(id)` | If it was active, the first remaining workspace becomes active. | App, Sidebar, workspaceFromGit (rollback) |
| `updateWorkspace(id, patch)` | Shallow merge. | App, control, Sidebar, SettingsPage, McpServersPanel, OrchestrationPanel, MobileSettings, WorkspaceDialog, accounts |
| `setActiveWorkspace(id)` | – | App, Sidebar, KitActivation, WorktreePanel, orchestrationActions |
| `addFolder(name) → id` / `updateFolder` / `removeFolder` / `moveFolder(id, beforeId)` | Removing a folder sends its workspaces back to the top level. | Sidebar, KitsSettings |
| `moveWorkspace(id, folderId, beforeId)` | `placeBefore`. A folder deleted in the meantime means the top level. | Sidebar |
| `applyPreset(ws, count, modelPaneId)` | Keeps panes in **reading order**. A shrinking preset closes the rest. New panes copy the agent of the focused pane (or of the last pane kept). Then `presetTree`. | App (toolbar presets) |
| `tidyLayout(ws)` | `presetTree(leafIds(normalizeTree))`: the same ids, so nothing restarts. | App |
| `setTree(ws, tree)` | Divider drags. | WorkspaceTerminals |
| `addPane(ws, {agent?, near?, dir?, cwd?, name?}) → id\|null` | `null` at `MAX_PANES = 16`. The new pane copies the agent of `near`. An explicit name is used only if free. `insertLeaf(near, zone right/bottom)`. | App, control, workerControl, WorktreePanel, KitActivation, accounts, orchestrationActions |
| `closePane(ws, pane)` | The last pane stays. `removeLeaf`. | App, control, workerControl, deliveryControl, kitSetup, KitActivation, WorkspaceTerminals |
| `movePane(ws, drag, target, zone)` | `moveLeaf`; zone `center` = swap. | App (swap keys), WorkspaceTerminals (drag) |
| `updatePane(ws, pane, patch)` | Shallow merge. | App, workerControl, orchestrationActions, accounts, WorkspaceTerminals (sessionId capture) |
| `respawnPane(ws, pane, patch) → newId\|null` | **New pane id** = remount = kill the PTY and spawn again; `renameLeaf`. This is how restart, agent change and session change work. | App (`replacePane`), control, KitActivation, accounts |
| `updateSettings(patch)` | Shallow merge. `resetSettings` = `updateSettings(DEFAULT_SETTINGS)`, so `projectsFolder`, `kitsEverywhere` and `voiceApiKey` survive. | App, Sidebar, SettingsPage, ShortcutsSettings, KitsSettings, accounts |
| `add/update/remove/moveSavedCommand(scope, ws, …)` | scope `workspace` or `global` (`settings.savedCommands`). A move keeps the id and lands last. | WorkspaceTerminals |

### 1.4 Persistence: `state.json` (must stay byte-compatible in meaning)

- **Location:** Tauri `app_data_dir()` for identifier **`com.simnjs.iabench`** (`src-tauri/tauri.conf.json:6`; the identifier was never renamed):
  - Linux: `$XDG_DATA_HOME/com.simnjs.iabench/state.json` (`~/.local/share/...`)
  - macOS: `~/Library/Application Support/com.simnjs.iabench/state.json`
  - Windows: `%APPDATA%\com.simnjs.iabench\state.json`
- **Native:** `dirs::data_dir().join("com.simnjs.iabench")`.
- **IO, unchanged.** Keep using `hangar-core/src/store.rs`, which only needs its `AppHandle` replaced by a path:
  - `load_state`:
    - copies over a `state.json` from the previous identifiers `com.simnjs.iabench` / `com.simnjs.hangar-ai` if none exists;
    - runs `voice::key::migrate`, which moves a plaintext `settings.voiceApiKey` to the keychain and rewrites the file;
    - on a parse error, quarantines the file as `state.json.corrupt.<epoch>[-n]` and returns `Err`. The app then starts empty.
  - `save_state`: `serde_json::to_string_pretty`, written to `state.json.tmp`, then renamed.
- **Writer rules** (store.tsx:379):
  - only the main window saves;
  - **never before hydration**;
  - **debounced 400 ms** after the last change;
  - the whole `AppState` each time.
  - Native: one writer task, plus a final flush on exit. Today a change made less than 400 ms before quit is lost; flushing is a harmless improvement.
- **Format:** plain JSON of `AppState` with **camelCase** keys, exactly the TS shape above. Rules for the serde structs (P0 `state/types.rs`):
  1. **Preserve unknown fields** on `AppState`, `Workspace`, `Pane`, `Settings`, `WorkspaceFolder` and `SavedCommand`, using `#[serde(flatten)] extra: serde_json::Map<String, Value>`. TS spreads (`{...ws, ...patch}`) keep fields written by newer versions or hand edits, and a downgrade must not lose them.
  2. **`null` vs absent.** Fields typed `T | null` (`themeId`, `shellId`, `tree`, `folderId`, `activeWorkspaceId`, `sessionId`, `cwd`, `title`, `locale`) are written as **`null`**. Optional `?` fields (`detached`, `windowBounds`, `mcp`, `kits`, `orchestration`, `hq`, `mobilePorts`, `claudeAccount`, `worktreeHooks`, `role`, `taskId`, `model`, `portBase`, `sharedFolder`, `login`, `kitsEverywhere`, `projectsFolder`, `voiceApiKey`) are **omitted when absent** (`skip_serializing_if = "Option::is_none"`). `kits` and `claudeAccount` are deliberately absent rather than empty.
  3. **Hydration** (`store.tsx:287`):
     - `panes ?? []`, `extraRoots ?? []`, `savedCommands ?? []`;
     - a `folderId` naming a folder that does not exist becomes `null`;
     - `uniquePaneNames` renames duplicate or missing pane names across **all** workspaces;
     - `activeWorkspaceId ?? null`;
     - **settings = `{...DEFAULT_SETTINGS, ...loaded.settings}`**, i.e. `#[serde(default)]` with `Default` = `DEFAULT_SETTINGS`.
  4. **Readers must be defensive.** `state.json` is hand-edited. `readAccountChoice`, `workspaceKits`/`everywhereKits`, `orchestrationOf`, `workspaceMcp` and `isExperimentEnabled` all treat malformed values as "none". Use lenient deserialisers (`#[serde(deserialize_with = "lenient")]` that falls back to the default) rather than failing the whole file. A strict parse error would quarantine the user's workspaces.
  5. `keybindings` keeps ids this version does not know (`prune` in shortcuts.ts keeps them).
  6. `hibernateAfterMin` is read as `max(1, value || 0)` minutes; `uiScale` is clamped to 90–130.
- **Persisted outside `state.json`** (localStorage today). Proposed native homes:

  | Key | Content | Native |
  |---|---|---|
  | `hangar.kitTabs.<workspaceId>` | the workspace's open kit tabs and their params (`lib/modelStore.ts:531`) | `<data>/kit-tabs.json` (P16) |
  | `hangar.accountUsage` | the last known usage per Claude account (`components/accounts/usageStore.ts:14`) | `<data>/account-usage.json` (P8) |
  | `hangar.webgl` | an xterm renderer override | dropped |

### 1.5 Transient state

None of this is ever saved: App.tsx `useState`, plus module-level stores.

**App.tsx `useState`** (→ `Model.ui` in P0 `state/live.rs`):

| Field | Type / default | Mutated by |
|---|---|---|
| `showSettings`, `settingsCategory` | bool / `"general"` | `view.settings`, `view.shortcuts`, Sidebar gear, notification landing (closes it) |
| `showCreate` | bool | `workspace.new`, Sidebar `+`, placeholder button |
| `showMcp` | bool | `view.mcp`, Board "connect agents" |
| `mcpServersFor`, `orchestrationFor` | `wsId \| null` | Sidebar menu, Inspector, palette commands |
| `needsYouOpen` | bool | toolbar button, `view.needsYou` |
| `view` | `"terminals"\|"board"\|"memory"\|"stats"` | Toolbar views, `view.*` commands |
| `paletteOpen` | bool | `view.palette` (toggles) |
| `drawer` | `"inspector"\|"dock"\|"model"\|null` | toggles when a panel does not fit (§1.11) |
| `focusByWorkspace` | `Record<wsId, paneId>` | `focusPane`, every pane click, `pane.focus*`, notifications |
| `zoomByWorkspace` | `Record<wsId, bool>` | `pane.zoom`, PaneZoomBar |
| `pickerPaneId` | `paneId \| null` | `pane.sessions`, pane header, Inspector |
| `confirmReset`, `confirmStopWorkers` | bool | armed by command or menu; reset on workspace or view change |
| `availableAgents`, `shells` | `string[]`, `ShellInfo[]` | once on mount: `detect_agents`, `detect_shells` |
| `broadcast` | string | StatusBar input; voice transcript when it has focus |
| `openedIds` | `wsId[]` | workspaces mounted once visited (so their PTYs live); removed by hibernation |
| `notice` | `string \| {text, kind: "asleep"\|"agent"} \| null` | auto-clears after **6 s** |
| `kitOpened` | counter | `openKit` |

**Module-level stores** (all become fields of `Model`, updated in `update()`). The shared read-models are defined by P0 and written by the owning package:

| Module | Holds | Owner |
|---|---|---|
| `lib/agentState.ts` | `Map<paneId, {workspaceId, activity: working\|waiting\|yours\|idle, bridged}>`, `planLimits`, relay | P4 |
| `lib/paneInsight.ts` | per pane: model, version, session since, turn start, last answer, cost, context, events (max 40), edited files (max 50) | P4 |
| `lib/hibernateStore.ts` | `touched`, `rest`, `resumeOnce` (from `?resume=`), `asleep` | P4 |
| `lib/terminalRegistry.ts` | `paneId → xterm Terminal`, finders, diff openers | P5 (becomes `TerminalHub`) |
| `lib/ptyBus.ts`, `lib/bridge.ts` | output, exit, bridge-event and queue handlers per pane; queue carry (10 s) | P5 |
| `lib/paneTask.ts` | board cache per cwd (`board_load` + `board:changed`) | P12 (shared `boards`) |
| `lib/paneBranch.ts` | branch cache per folder | P4 |
| `lib/modelStore.ts` | per-workspace preview scopes: models, kit tabs, panel open/expanded/unseen | P15 |
| `lib/kitTabs.ts` | installed kits (`kits_list` + `kits:changed`) | P14 |
| `lib/stats.ts` | usage stats cache (60 s fresh) | P11 |
| `lib/deliveryRuns.ts`, `lib/deliveryControl.ts` | deliveries running, in flight, settled | P18 |
| `lib/accounts.ts` / `usageStore.ts` | launched account per pane; usage per account | P8 |
| `lib/workspaceFromGit.ts` | clone runs in flight; the consent queue (`gitCloneConsent`) | P8 |
| `lib/orchestrationActions.ts` | restart notes; `openManager` single flight | P10 |
| `lib/remoteControl.ts`, `remoteDelivery.ts`, `remotePush.ts`, `activityLog.ts` | phone-side live state | P20 |
| `lib/orchestratorEvents.ts`, `managerEvents.ts`, `orchestratorBrief.ts` | engines | P19 |
| `lib/windows.ts` | handover, launching | P0 (most of it disappears) |
| `lib/useShortcuts.ts` | `suspensions` counter (shortcut recorder) | P7 |
| `lib/agents.ts` | `claimed` session ids | P4 |

### 1.6 Startup sequence

**Today, main window:**
1. `main.tsx` renders `StoreProvider`. The state starts as `EMPTY` (default settings), and App renders `<div class="boot">t("app.loading")</div>` until hydrated.
2. StoreProvider:
   - listens to `store:state` (and `store:hello` in the main window);
   - **main:** `load_state()` → hydrate (§1.4) → `hydrated = true`;
   - **workspace window:** emits `store:hello`, adopts the main window's `store:state`, and falls back to `load_state()` after 2,500 ms.
3. App mount effects:
   - `applyThemeToDocument(theme)` writes the CSS token variables, `--ui-scale` and `--mono` (= `settings.fontFamily`);
   - `detect_agents()` and `detect_shells()`;
   - listens to `notification-activated {workspaceId, paneId}` → `activate`;
   - main: listens to `agent:activity` and `serveModelHandovers`; workspace window: `relayActivity`, `serveTerminals`, `serveModelHost`;
   - main: `serveRootGrants`.
4. After hydration:
   - the active workspace is added to `openedIds`, so its `WorkspaceTerminals` mounts. Each `TerminalPane` spawns its PTY (or adopts a live one) and, after `launchDelayMs`, types the agent command with `--resume <sessionId>` when `autoResume` is on (§4);
   - `controlDeps` becomes non-null: `usePaneControl` answers `control:request`, `useRemoteControl` answers phone calls, `useOrchestrator` starts the engines;
   - main only, once: every `detached` workspace gets `open_workspace_window(id, name, windowBounds)` again;
   - `useHibernation` (main, `hibernateIdle`), `useNeedsYouNotices` (main), `useDiscordPresence` (main);
   - `useInstalledKits` (`kits_list`), then `restoreKitTabs` for each mounted workspace;
   - `UpdateBanner` runs `check()` and `GitCloneConsent` mounts (main only);
   - the debounced save starts.

**Backend setup, in parallel** (`hangar-core/src/lib.rs:69`, for the backend agent; the app must start these):
- threads: `path_env::ensure`, `bridge::plugin_dir()`;
- watchers: `kits::watch` (→ `kits:changed`), `accounts::watch` (→ `accounts:changed`);
- `server::start(store, token)`: the HTTP API for agents and MCP. Then `bridge::set_endpoint` and `endpoint::publish(port, token)`;
- `remote::Remote::open` + `resume()` (Hangar Mobile).

**On main-window close** (`lib.rs:256`): `pty::kill_all`, `remote.activity_stopped()`, `delivery::cancel_all()`, `endpoint::clear()`, Discord `stop()`, `exit(0)`.

**Native order (P0):**
1. Parse args.
2. Start the core services.
3. Open the main window: 1360×860, min 900×560, centred.
4. `Task::perform(load_state)` → `Message::Booted`. Draw the boot screen until it arrives.
5. Hydrate.
6. Mark the active workspace opened.
7. Reopen detached workspace windows.
8. Enable the subscriptions that are gated on `hydrated`: control, engines, hibernation, notices.

### 1.7 Multi-window (workspace windows)

**Today:**
- Main window label `main`. A workspace window has label `workspace-<id>` and URL `index.html?workspace=<id>[&resume=<paneIds>]`. `hangar-core/src/windows.rs`:
  - title `"<name> — Hangar.AI"`, min 640×420;
  - size: the saved `windowBounds` if they are on a screen, else a second screen, else 1200×800 centred.
- **Detach** (main only, `canDetach`):
  1. `markHandover(paneIds)`, held 15 s, so the unmount does not kill those PTYs;
  2. the panes owed a resume are collected;
  3. `updateWorkspace({detached: true})`;
  4. the main window activates the next workspace that is not detached;
  5. `open_workspace_window(id, name, bounds, resume)`. On failure everything is reverted and a notice `window.openFailed` is shown.
- **The workspace window:**
  - adopts the live PTYs with `pty_alive` + `pty_history` replay;
  - its close button emits `window:close-requested`, which runs `reattach`: `updateWorkspace({detached: false})`, activate it in the main window, then `close_workspace_window` after 150 ms;
  - remembers `windowBounds` 700 ms after a move or resize (logical px);
  - if its workspace is deleted: `pty_kill` its panes and close.
- **Main window when the active workspace is detached:** a placeholder (`window.elsewhereTitle/Body`, buttons Show → `focus_workspace_window`, Bring back → reattach).
- **Cross-window protocols:** `store:state`/`store:hello` (whole state, main window authoritative), `agent:activity` relay, `terminal:ask/answer` (paste/read/probe a terminal drawn in another window, 1.5 s timeout), `model:*` (preview host), `window:focus-pane`, `kits:grant-roots`.

**Native (P0 `shell/windows.rs`):**
- One `iced::daemon` and one `Model`, with `windows: HashMap<window::Id, WindowKind{Main | Workspace(WsId)}>`.
- `view(window_id)` draws the main frame, or the frame of one workspace, which has no navigator and no settings (§1.10: the shortcuts `view.settings`, `view.shortcuts` and `workspace.new` are absent there; so are the navigator commands).
- Terminal emulator state (`hangar-term`) and PTYs live in the model, not in a window. Detach and reattach are therefore just "which window draws this workspace": **no handover, no replay, no ask/answer**.
- Keep `detached` and `windowBounds` in `state.json`, and reopen those windows at boot.
- A workspace window's close request = reattach (`window::close_requests()` subscription + `window::close` after the flag is set).

### 1.8 Regions and focus (F6) (`src/lib/regions.ts` 81 LOC, test 39/5)

- `Region = navigator | toolbar | main | model | dock | inspector | status`. **F6 order:** navigator, toolbar, main, model (3D preview), dock (board panel), inspector, status. It wraps.
  - From outside every region: F6 starts at the first, Shift+F6 at the last.
- **Regions shown:** the navigator if shown; toolbar, main and status if there is a workspace; model, dock and inspector if each is shown.
- A region with nothing to focus is skipped; for example the status bar outside the terminals view, which has no input.
- **Focus target per region:**
  - navigator: its tree's current row (roving tab stop);
  - status: the broadcast `input`;
  - model, dock, inspector: the panel itself;
  - main in the terminals view: **the focused pane's terminal**;
  - otherwise: the first focusable.
- A dialog (`aria-modal`) keeps the keyboard: F6 does nothing while one is open.
- `view.focusNavigator` / `view.focusInspector` first show the panel if it is hidden, then focus it on the next render (`pendingRegion`).
- `lib/focusable.ts` (16): the focusable query. `lib/menuNav.ts` (28, test 48/8): `arrowStep` / `stepIndex` for Up/Down/Home/End in lists.
- **Native (P0):**
  - `Model.focus: Focus { Terminal(PaneId) | Region(Region) | Widget(widget::Id) }`;
  - `shell/regions.rs` has `next_region`, plus a `focus_entry(region)` hook that each package exports (P0 pre-declares the stubs);
  - `widget::focus_next`/`text_input::focus` for widgets; `hangar-term`'s own focus flag for terminals.

### 1.9 Backend → window events (all become `CoreEvent` variants, §8.2)

| Event | Payload | Consumers (package) |
|---|---|---|
| `pty:output` | `{id, data, seq}` (seq = total UTF-8 bytes printed) | P5 |
| `pty:exit` | `{id, code: null}` (only a shell that ended by itself) | P5, P19 |
| `bridge:event` | `{paneId, event: BridgeEvent}` (hello, session, turn start/end, permission, usage, edit, dropped, tool, end) | P4/P5, P8 (usage), P19, P20 |
| `bridge:queue` | `{paneId, pending: QueuedPrompt[]}` | P5, P9 |
| `board:changed` | `{cwd}` | P12 (cache), P19, P6 (Needs-you), P9 |
| `memory:changed` | `()` | P10 |
| `kits:changed` | `()` | P14 |
| `accounts:changed` | `()` | P8 |
| `git-clone:progress` | `{jobId, phase, percent}` | P8 |
| `voice:phase`, `voice:level`, `voice:text`, `voice:download` | phase/error, level, text, download progress | P6, P13 |
| `remote:changed`, `remote:shared` | `()`, `Share` | P13, P20 |
| `control:request`, `control:internal` | `{id, op, args}` | P18, P19, P20; replaced by a request channel with a oneshot reply (§5) |
| `notification-activated` | `{workspaceId, paneId}` | P0 (activate) |
| `window:close-requested` | `()` | P0 |
| `tauri://drag-enter/over/drop/leave` | paths + position | P5 (file drop) → iced `window::Event::FileHovered/FileDropped` |

### 1.10 Keyboard shortcut system

**Files:**
- `src/lib/keys.ts` (335 LOC, test 218/28 cases): the chord algebra.
- `src/lib/shortcuts.ts` (786, test 169/20): the commands, defaults, keymap, conflicts.
- `src/lib/useShortcuts.ts` (216, no test): the dispatcher.
- App.tsx `buildHandlers()` (1282–1472): which commands are live right now.

**Chord model** (keys.ts):
- `Chord {key, ctrl, shift, alt, meta}`. A `Binding` string is `"Ctrl+Shift+Enter"`; a sequence is chords separated by spaces (`"Mod+K Mod+S"`).
- **`Mod`** = Meta on macOS, Ctrl elsewhere.
- **Modifier spellings accepted:** ctrl, control, ctl, shift, alt, option, opt, meta, cmd, command, super, win.
- **Key aliases:** esc, return, space, del, ins, pgup/pgdn, arrows (`Up`/`Down`/`Left`/`Right`), plus, minus, comma, period, slash, backslash. `F1`–`F24`. Letters are upper-cased.
- **Keys from an event:** `event.key` leads, so layouts are honoured (AZERTY).
  - Fallbacks to the code: a digit row that needs Shift falls back to `Digit<n>`, a non-Latin layout to `Key<X>`, and numpad codes map to symbols.
  - **AltGr** (Ctrl+Alt with `AltGraph`) is never a chord.
- **Serialisation for matching:** `ctrl+alt+shift+meta+KEY`.
- **Display:**
  - macOS: `⌃⌥⇧⌘` + key, no separator;
  - elsewhere: `Ctrl+Alt+Shift+Win+Key`;
  - arrows are shown as `↑↓←→`.
- `isTextEditingChord`: Mod + A/C/V/X/Z/Y belongs to text fields.

**Dispatcher rules** (useShortcuts.ts). Today it is a `window` keydown listener in the **capture phase**, above every widget, the xterm textarea included.
- The sequence timeout is **1,600 ms**. A pending sequence shows the chord hint (`keymap.pending`).
- A bare chord (no Ctrl, Alt or Meta), unless it is a function key, is ignored while the target is a text field. xterm's textarea is **not** counted as a field.
- A text-editing chord is ignored in a field.
- The **first command in the keymap that has a live handler** wins. A key with no live handler is **not consumed**: it reaches the terminal.
- A prefix of a longer sequence is consumed and held.
- A sequence that leads nowhere swallows the key that ended it.
- `suspendShortcuts()` (counter) is used while the shortcut recorder in Settings is open.
- While the palette is open, the only handler is `view.palette`, which closes it.

> **Native difficulty (risk #1).** iced delivers keys to widgets first. P3 must give `TermView` a `key_filter` (an `Arc<KeymapIndex>` check) so that live shortcuts are **not** consumed by the terminal. The same check must wrap text inputs. The dispatcher itself is a `keyboard::listen()` / `event::listen_with` subscription in P7.

**Every command and its default binding.** `Mod` = Ctrl on Windows/Linux, Cmd on macOS. "∅" = unbound by default. Section = the Settings › Shortcuts group. "Live when" = the condition under which App adds the handler (`buildHandlers`); a command that is not live is not consumed and the palette does not list it.

| # | Command id | Section | Win/Linux default | macOS default | Live when |
|---|---|---|---|---|---|
| 1 | `pane.split` | panes | Mod+Shift+Enter | Cmd+Shift+Enter | terminals view and a tree. Splits along the pane's longer side (`preferredDir`) |
| 2 | `pane.splitRight` | panes | Ctrl+Alt+Right | Meta+D | same |
| 3 | `pane.splitDown` | panes | Ctrl+Alt+Down | Meta+Shift+D | same |
| 4 | `pane.close` | panes | Mod+Shift+X | Cmd+Shift+X | focused pane, and >1 pane |
| 5 | `pane.restart` | panes | Mod+Shift+R | Cmd+Shift+R | focused pane |
| 6 | `pane.restartAll` | panes | Mod+Alt+Shift+R | Cmd+Alt+Shift+R | terminals view |
| 7 | `pane.resetAll` | panes | ∅ | ∅ | some agent pane; **arms** the confirmation only |
| 8 | `pane.tidy` | panes | Mod+Alt+G | Cmd+Alt+G | >1 pane |
| 9 | `pane.sessions` | panes | Mod+Shift+S | Cmd+Shift+S | focused pane runs a resumable agent |
| 10 | `pane.zoom` | panes | Mod+Shift+Z | Meta+Alt+Z | >1 pane |
| 11 | `pane.pinOrchestrator` | panes | ∅ | ∅ | `pinRefusal(ws, focused) === null` |
| 12 | `pane.unpinOrchestrator` | panes | ∅ | ∅ | the focused pane is the orchestrator |
| 13 | `pane.next` | navigation | Ctrl+Tab | Ctrl+Tab | >1 pane (reading order) |
| 14 | `pane.prev` | navigation | Ctrl+Shift+Tab | Ctrl+Shift+Tab | >1 pane |
| 15–18 | `pane.focusLeft/Right/Up/Down` | navigation | Alt+←/→/↑/↓ | Ctrl+Alt+←/→/↑/↓ | focused pane (`neighborOf`) |
| 19–27 | `pane.focus1`…`pane.focus9` | navigation | Mod+1…Mod+9 | Cmd+1…Cmd+9 | panes exist at that index (reading order) |
| 28–31 | `pane.swapLeft/Right/Up/Down` | navigation | Alt+Shift+arrow | Ctrl+Alt+Shift+arrow | focused pane (`movePane(…, "center")`) |
| 32 | `workspace.next` | workspaces | Ctrl+PageDown | Meta+Alt+Right | >1 workspace (sidebar order), main window |
| 33 | `workspace.prev` | workspaces | Ctrl+PageUp | Meta+Alt+Left | same |
| 34 | `workspace.new` | workspaces | Mod+Shift+N | Cmd+Shift+N | main window |
| 35 | `workspace.mcpServers` | workspaces | ∅ | ∅ | a workspace |
| 36 | `workspace.newOrchestrator` | workspaces | ∅ | ∅ | `canCreateOrchestrator(ws)` |
| 37 | `workspace.orchestration` | workspaces | ∅ | ∅ | a workspace |
| 38 | `workspace.stopWorkers` | workspaces | ∅ | ∅ | workers exist; arms the confirmation |
| 39 | `workspace.sleep` | workspaces | ∅ | ∅ | >1 workspace; `forcedSleep.run(active)` |
| 40–48 | `workspace.go1`…`go9` | workspaces | Alt+1…Alt+9 | Meta+Alt+1…9 | main window; sidebar order |
| 49 | `view.settings` | view | Mod+, | Cmd+, | main window (toggles) |
| 50 | `view.shortcuts` | view | Mod+K Mod+S | Cmd+K Cmd+S | main window |
| 51 | `view.palette` | view | Mod+Shift+P | Cmd+Shift+P | always (toggles) |
| 52 | `view.navigator` | view | Mod+Alt+N | Cmd+Alt+N | main window |
| 53 | `view.inspector` | view | Mod+Alt+I | Cmd+Alt+I | always |
| 54 | `view.focusNavigator` | view | Mod+Alt+Shift+N | Cmd+Alt+Shift+N | main window |
| 55 | `view.focusInspector` | view | Mod+Alt+Shift+I | Cmd+Alt+Shift+I | a workspace |
| 56 | `view.nextRegion` | view | F6 | F6 | always |
| 57 | `view.prevRegion` | view | Shift+F6 | Shift+F6 | always |
| 58 | `view.terminals` | view | Mod+Alt+T | Cmd+Alt+T | a workspace |
| 59 | `view.board` | view | Mod+Alt+B | Cmd+Alt+B | a workspace |
| 60 | `view.boardDock` | view | Mod+Alt+Shift+B | Cmd+Alt+Shift+B | a workspace |
| 61 | `view.modelPreview` | view | Mod+Alt+P | Cmd+Alt+P | the workspace has a preview open |
| 62 | `view.closeModels` | view | ∅ | ∅ | same |
| 63 | `view.memory` | view | Mod+Alt+K | Cmd+Alt+K | a workspace |
| 64 | `view.stats` | view | Mod+Alt+U | Cmd+Alt+U | a workspace |
| 65 | `view.mcp` | view | Mod+Alt+M | Cmd+Alt+M | a workspace |
| 66 | `view.needsYou` | view | ∅ | ∅ | Needs-you has items |
| 67 | `view.broadcast` | view | Mod+Shift+B | Cmd+Shift+B | terminals view (focuses the broadcast input) |
| 68 | `terminal.clear` | terminal | Mod+Shift+L | Cmd+Shift+L | focused pane |
| 69 | `terminal.copy` | terminal | Ctrl+Shift+C | Meta+Shift+C | focused pane (selection → clipboard) |
| 70 | `terminal.paste` | terminal | Ctrl+Shift+V | Meta+Shift+V | focused pane (bracketed paste through the terminal) |
| 71 | `terminal.selectAll` | terminal | Ctrl+Shift+A | Meta+Shift+A | focused pane |
| 72 | `terminal.scrollTop` | terminal | Ctrl+Home | Meta+Up | focused pane |
| 73 | `terminal.scrollBottom` | terminal | Ctrl+End | Meta+Down | focused pane |
| 74 | `terminal.find` | terminal | Ctrl+Shift+F | Meta+F | focused pane (opens TerminalSearch) |
| 75–83 | `command.run1`…`run9` | commands | ∅ | ∅ | saved commands exist at that index (`mergeCommands`: workspace commands, then global ones). Broadcast commands go to every pane |
| 84 | `app.zoomIn` | app | Mod+= **and** Mod++ | Cmd+= and Cmd++ | always (terminal `fontSize` +1, clamped 9–24) |
| 85 | `app.zoomOut` | app | Mod+- | Cmd+- | always |
| 86 | `app.zoomReset` | app | Mod+0 | Cmd+0 | always (→ 13) |
| 87 | `app.fullscreen` | app | F11 | Ctrl+Meta+F | always |
| 88 | `voice.dictate` | voice | Mod+Alt+V | Cmd+Alt+V | `voiceEnabled`. Push-to-talk: held, it records; released, it transcribes |

- **Settings sections:** panes, navigation, workspaces, view, terminal, commands, voice, app.
- **Conflicts** (`findConflicts`):
  - `duplicate`: the same keystroke on several commands;
  - `shadow`: a sequence whose first chord is itself bound.
- **Palette:** it hides `pane.focusN`, `workspace.goN`, `voice.dictate`, `view.nextRegion` and `view.prevRegion` (§2.1/5).
- **Tooltips:** `t("keymap.withKeys", {label, keys})` uses the **first** binding (`shortcutLabel`).

### 1.11 The window layout, side panels and drawers

**Render tree (App.tsx:1516):**

```
.app [--window] [--no-nav]
├─ UpdateBanner (main only) · GitCloneConsent card (main only, bottom-right, never takes focus)
├─ Sidebar = navigator (main only, settings.navigatorOpen)          region "navigator"
└─ main
   ├─ (workspace) Toolbar                                            region "toolbar"
   │  └─ main__body (measured width)
   │     ├─ workarea                                                 region "main"
   │     │  ├─ WorkspaceTerminals × each opened, not detached workspace (hidden, never unmounted)
   │     │  ├─ BoardView (view=board) · MemoryView (view=memory) · StatsView (view=stats)
   │     │  ├─ ModelPanel (preview: docked, drawer or expanded)          region "model"
   │     │  └─ BoardDock (terminals view; docked or drawer)            region "dock"
   │     └─ Inspector (docked or drawer)                               region "inspector"
   │  StatusBar                                                       region "status"
   ├─ (active workspace detached) placeholder "shown in its own window"
   └─ (none) placeholder: title, agents list, "+ New workspace"
├─ VoiceHud (phase ≠ idle) · notice pill (6 s) · chord hint (sequence pending)
└─ overlays: CommandPalette · SettingsPage (full sheet) · WorkspaceDialog · McpPanel · McpServersPanel
             · OrchestrationPanel · SessionPicker   (+ each package's own modals: Diff, Worktrees, TaskDialog, Kits…)
```

**`lib/sidePanels.ts`** (145, test 209/21) decides docked vs drawer:
- the terminals keep `TERMINALS_MIN = 640 px`;
- priority: model (min 320) > dock (min 300) > inspector (240–440);
- widths are in pt × `uiScale`;
- a panel that does not fit becomes a **drawer** over the terminals (48 px margin). Its toggle opens the drawer and leaves the setting alone;
- widening the window ends a drawer.

**`useDrawer.ts`** (126): a drawer focuses itself on open. It closes on an outside press or focus (except over modals, menus and `[data-drawer-toggle]`) and on Escape, then restores focus to the opener or the terminal. It is a state machine in P0 `shell/drawer.rs`.

**Notices:** one line at the bottom, auto-cleared after 6 s.
- A plain string = a problem (warning mark).
- `{kind: asleep}` = a workspace went to sleep (moon).
- `{kind: agent}` = an agent or Hangar acted on the layout.
- Texts come from `control.*`, `orch.*`, `hibernate.*`, `manager.*`, `needsYou.*`, `window.*`, `voice.*`.

**Theme application:**
- `getTheme(activeWorkspace.themeId ?? settings.themeId)` → the chrome tokens for the whole window.
- Each `WorkspaceTerminals` uses its own workspace's theme for its terminals.

---

## 2. Every user-facing surface

### 2.0 Surface index

Each row gives the TS sources (LOC), the owning package (§8) and the section that details it. The detailed entries (§2.1–2.4, §4/3) give for each surface:
- what it shows, with its empty, loading and error states;
- every interaction;
- the props and callbacks it takes from App;
- the store actions, backend commands and events it uses;
- its i18n key prefixes and CSS classes;
- iced porting notes.

| Surface | TS sources (LOC) | Package | Detail |
|---|---|---|---|
| App frame, placeholders ("no workspace", "shown in its own window"), boot screen, notice pill, chord hint | `App.tsx` (1,912) | P0 | §1.11 |
| Shared modal, backdrop and focus trap; anchored popover; context menu; logo; icons; state glyph | `Backdrop.tsx` (161), `ToolbarMenu.tsx` (180), `NavigatorMenu.tsx` (222), `Logo.tsx` (24), `Icon.tsx` (369), `StateGlyph.tsx` (33) | P2 | §2.1/0 "Shared popup and modal primitives", §7 |
| Navigator (sidebar): folders, workspaces, filter, drag reorder, rename, menus, plan meters, status counters | `Sidebar.tsx` (1,192), `NavigatorModel.ts` (186), `NavigatorStatus.tsx` (128), `NavigatorPlan.tsx` (80), `lib/folders.ts` (20) | P6 | §2.1/1 |
| Toolbar: views, activity capsule, presets, more menu, confirmations, kits menu, panel toggles | `Toolbar.tsx` (732), `ToolbarActivity.tsx` (215) | P6 | §2.1/2 |
| Status bar: broadcast input, cwd, git branch, pane count | `StatusBar.tsx` (208) | P6 | §2.1/3 |
| Needs-you ("Besoin de toi") button and panel | `NeedsYou.tsx` (315), `lib/needsYou.ts` (244) | P6 | §2.1/4 |
| Update banner | `UpdateBanner.tsx` (125) | P6 | §2.1/8 |
| Voice HUD and push-to-talk | `VoiceHud.tsx` (70), `lib/voice.ts` (206) | P6 | §2.1/10 |
| Command palette | `CommandPalette.tsx` (643), `lib/paletteMatch.ts` (259) | P7 | §2.1/5 |
| Saved commands menu and dialog | `CommandMenu.tsx` (217), `CommandDialog.tsx` (168), `lib/savedCommands.ts` (44) | P7 | §2.1/6 |
| Pane "…" menu | `PaneMenu.tsx` (161) | P7 | §2.1/0 |
| Session picker | `SessionPicker.tsx` (143) | P7 | §2.1/7 |
| Workspace dialog (folder / git modes), workspace kits dialog, worktree hooks | `WorkspaceDialog.tsx` (897), `lib/workspaceFromGit.ts` (746), `lib/gitClone.ts` (152) | P8 | §2.1/11 |
| Git clone consent card | `GitCloneConsent.tsx` (106 + 73 CSS) | P8 | §2.1/9 |
| Claude accounts: switcher, badges, pickers, pane items, Settings › Accounts | `components/accounts/*` (1,622) | P8 | §2.1/13 |
| Pane grid, terminal pane, search, zoom bar, gauge, file drop, workspace terminals host | `PaneGrid.tsx` (459), `TerminalPane.tsx` (1,942), `TerminalSearch.tsx` (241), `PaneZoomBar.tsx` (89), `PaneGauge.tsx` (41), `PaneStatus.ts` (72), `WorkspaceTerminals.tsx` (330) | P5 (+P3 widget, P4 logic) | §4, §2.1/12 |
| Inspector and its panels (session, terminal, context, task, queue, activity, files) | `Inspector.tsx` (432), `InspectorSection.tsx` (53), `InspectorPanels.tsx` (535), `InspectorFiles.tsx` (248) | P9 | §2.2 |
| Diff view ("Changes" modal) | `DiffView.tsx` (376) | P9 | §2.2 |
| Worktree panel ("Git worktrees" modal) | `WorktreePanel.tsx` (1,630) | P9 | §2.2 |
| Memory view | `MemoryView.tsx` (517), `memoryGuards.ts` (45) | P10 | §2.2 |
| MCP install panel; per-workspace MCP servers panel | `McpPanel.tsx` (206), `McpServersPanel.tsx` (341) | P10 | §2.2 |
| Orchestration panel and pane badge | `OrchestrationPanel.tsx` (363), `OrchestrationBadge.tsx` (63) | P10 | §2.2 |
| Stats view and charts | `StatsView.tsx` (783), `StatsCharts.tsx` (612) | P11 | §2.2 |
| Board view, dock, filter, task dialog, priority, assignee | `BoardView.tsx` (743), `BoardDock.tsx` (140), `BoardFilter.tsx` (188), `TaskDialog.tsx` (478), `BoardPriority.tsx` (26), `BoardAssignee.tsx` (67) | P12 | §2.3/1 |
| Settings page: schema-driven categories, search, shortcuts editor, Mobile/Voice/Discord cards, experiments | `settings/SettingsPage.tsx` (438), `schema.tsx` (1,212), `ShortcutsSettings.tsx` (473), `MobileSettings.tsx` (753), `mobileOptions.ts` (19), `VoiceCard.tsx` (313), `DiscordCard.tsx` (128) | P13 | §2.3/2–3 |
| Kit activation, workspace kits section, Settings › Kits, kit gallery | `KitActivation.tsx` (892 + 318 CSS), `settings/KitsSettings.tsx` (674), `settings/KitsGallery.tsx` (198) | P14 | §2.4/1.1–1.3 |
| Preview panel (tabs, docked, drawer and expanded modes) and 2D viewers (image, text/code, markdown, media, PDF, HTML) | `ModelPanel.tsx` panel parts (`ModelPanel`, `PreviewTabs`, `Grip`, `DrawerFocus`), `components/preview/*` except KitTabView (1,458) | P15 | §2.4/1.5–1.6 |
| Kit tab (HTML today) and the kit API | `preview/KitTabView.tsx` (378), `lib/kitApi.ts` (833), `lib/kitTabs.ts` (477) | P16 | §2.4/1.4 |
| 3D model viewer (three.js) | `ModelPanel.tsx` 3D parts (`ModelView`, `ModelToolbar`, `AnimationBar`, `InfoStrip`, `LoadingCard`, `ProgressBar`), `lib/model3d/*` (3,149) | P17 | §2.4/1.7, §2.4/3 |
| (no UI) Control ops for agents, orchestration engines, phone ops | `lib/control.ts` and friends (§5) | P18–P20 | §5 |

### 2.1 Chrome, navigator, palette, dialogs, accounts

This section has its own numbering: §2.1/0 shared primitives, /1 navigator, /2 toolbar, /3 status bar, /4 Needs-you, /5 palette, /6 saved commands, /7 session picker, /8 update banner, /9 consent card, /10 voice, /11 workspace dialog, /12 workspace terminals host, /13 accounts, /14 non-visual libs.

#### Scope: navigator, chrome bars, palette, dialogs, Claude accounts

Repo facts that shape the port:
- `crates/hangar-app/src/main.rs` is an 8-line stub, and `hangar-ui` is 4 lines.
- `hangar-core` already holds the backend for this scope: `accounts.rs` (828 LOC), `git_clone.rs` (2609), `open_editor.rs`, `git.rs`, `sessions.rs`, `workspace_file.rs`.
- So in Rust, every "invoke" below becomes a direct `hangar-core` call, and every Tauri event becomes an iced `Subscription` or channel.

---

##### 0. Shared popup and modal primitives (used by everything below)

| Primitive | File (LOC) | Behaviour to reproduce |
|---|---|---|
| `NavigatorMenu` | `components/NavigatorMenu.tsx` (222) | **Context menu at a point (x, y)**, clamped 8px inside the window. It is measured hidden for one frame, then placed.<br>**Entries:** `item {label, icon?, glyph?(agent state), danger?, checked?(radio + check icon), disabled?, reason?(greyed but still reachable by arrows, reason shown as tooltip), title?, takesFocus?}`, `heading`, `separator`.<br>**Keys:** Up/Down/Home/End wrap. Escape closes and returns focus to the row (`onRestoreFocus`). Tab closes.<br>**Pointer:** moving over an item focuses it. Leaving the menu clears the highlight. A pointerdown outside (capture phase) or window blur closes it.<br>**After a pick:** a `takesFocus` item closes without restoring focus; others close and restore. A refused item click does nothing and the menu stays open.<br>CSS: `ctx-menu`, `ctx-menu__item(--danger)`, `__icon`, `__label`, `__check`, `__sep`, `__head` (styles.css). |
| `ToolbarMenu` | `components/ToolbarMenu.tsx` (180) | **Popover anchored to a button.** It is portalled; placed right-aligned under the anchor (gap 6, margin 8), and flips above when it does not fit below.<br>**Roles:** `menu`, `alertdialog` (confirm), `dialog` (panel).<br>**Close reasons:** `escape`, `tab`, `outside`, `blur` (the owner decides where focus goes). A press on the anchor does not count as outside, because the anchor toggles. A window resize closes it.<br>**`menu` role:** arrows walk the items, Tab closes, the first item takes focus.<br>**`dialog`/`alertdialog` roles:** Tab cycles inside (a focus trap). An `autoFocus` child wins.<br>Hovering focuses an item. CSS: `tmenu`, `tmenu--confirm`, `tmenu__item(--danger)`, `__label`, `__keys`, `__sep`, `__title`, `__body`, `__actions`, `__panes`, `__pane` (chrome.css, `@keyframes tmenu-in`). |
| `PaneMenu` | `components/PaneMenu.tsx` (161) | **The "…" menu of a pane header** (hosted by `TerminalPane`, which builds the items: restart, sessions, zoom, diff…; out of scope).<br>Props: `id`, `label`, `anchorRef`, `items: {id,label,icon,keys?,separated?,onSelect}[]`, `onClose`, `accountFor?:{workspaceId,paneId}` (appends `PaneAccountItems`).<br>Fixed position from the anchor; same flip logic.<br>**Keys:** Up/Down/Home/End wrap. Escape and Tab close. Keys it uses are stopped from propagating (the app's shortcuts listen on the window).<br>Hover focuses. Outside press or blur closes. CSS: `pane-menu`, `__row`, `__item`, `__label`, `__keys`, `__sep`, `__heading` (`@keyframes pane-menu-in`). |
| `Backdrop` / `ModalHead` / `useModalFocus` | `components/Backdrop.tsx` (161) | **Modal scrim.** It closes only when both press and release land on the scrim. Escape (a window listener) closes unless `escape=false`.<br>**`useModalFocus`:** initial focus goes to the first focusable inside `[data-modal-body]`/`.modal__body`, unless something already has focus. Tab and Shift+Tab wrap. On unmount, focus returns to the opener if nothing else claimed it.<br>**`ModalHead`:** title, optional mono subtitle (a path), close × with tooltip `keymap.withKeys{label: modal.close, keys: "Esc"}`.<br>CSS: `modal-backdrop`, `modal`, `modal--wide`, `modal--list`, `modal__head`, `__titles`, `__title`, `__sub`, `__close`, `__body`, `__foot`, `__form`, `__empty`, `__sep`, `__note` (`@keyframes modal-in`). |
| `Logo` | `components/Logo.tsx` (24) | Inline SVG, 32×32 viewBox, three rects. The first one is `.logo__active` (accent); the others use the foreground colour. Port it as a `canvas` or `svg` widget. |

**iced notes (shared).**
- iced 0.14 has no anchored popover or context menu. Build one custom `Overlay` widget, `widgets/popover.rs`, that records the anchor's layout bounds, positions against the window, flips, clamps, and reports `CloseReason`. Every menu in this scope is that widget.
- There is no DOM focus for buttons. Model roving focus as an explicit `cursor`/`focused_item` in state, draw the focus ring yourself, and route keys through a `keyboard::on_key_press` subscription when an overlay is open.
- "Capture-phase outside press": the overlay's `on_event` sees all mouse events first, so it can close and still let the event through.

---

##### 1. Sidebar / navigator

**Files.** `components/Sidebar.tsx` (1192), `NavigatorModel.ts` (186; test 215 LOC, 22 cases), `NavigatorStatus.tsx` (128), `NavigatorPlan.tsx` (80), `NavigatorMenu.tsx` (222), `lib/folders.ts` (20; test 42 LOC, 5 cases). It also mounts `WorktreePanel` (out of scope) and `WorkspaceKitsDialog` (§11).

**Props from App** (`App.tsx:1529`, rendered only when `navigatorShown`, main window only):

| Prop | App wiring |
|---|---|
| `onOpenSettings` | `openSettings()` |
| `onNewWorkspace` | `setShowCreate(true)` |
| `focusByWorkspace: Record<wsId,paneId>` | App state |
| `onFocusPane(wsId,paneId)` | `activateRef.current` (switches workspace and focuses the pane) |
| `onOpenMcpServers(wsId)` | `setMcpServersFor` |
| `onOpenOrchestration(wsId)` | `setOrchestrationFor` |
| `onOpenManager()` | `openManager` → `orchestration.openManager(t("manager.hqName"))`, then activates the HQ |
| `sleep: ForcedSleep` | `useHibernation` → `{verdict(ws)→Verdict\|null, check(ws)→Promise<Verdict>, run(ws)}` |

**Store actions.** `removeWorkspace`, `updateWorkspace({name})`, `setActiveWorkspace`, `addFolder(name)→id`, `updateFolder(id,{name|collapsed})`, `removeFolder`, `moveFolder(id, beforeId|null)`, `moveWorkspace(id, folderId|null, beforeId|null)`, `updateSettings({navigatorOpen:false})`.

**Other hooks.** `useAccounts`, `useAccountSwitching().setWorkspaceAccount(ws,id,false)`, `useAgentsIn` (only when the attention filter is on), `useAsleep` (lib/hibernateStore), `useWorkspaceActivity` (re-render while a menu is open), `useShortcutTitle` (tooltips with bound keys).

**What it shows, top to bottom:**
1. **`nav-top`.** Logo(15), "Hangar.AI", version chip (`getVersion()`, hidden on error), hide-navigator button (`sidebarLeft`, tooltip shows the `view.navigator` shortcut).
2. **HQ button.** "Open project manager" (`manager.open`, title `manager.openHint`). Accent outline with inline styles; `--active` plus a surface-hover background when the HQ is the active workspace. HQ workspaces (`ws.hq`) never appear in the tree.
3. **`nav-section` heading** "Workspaces" plus a new-folder button (`folderPlus`). It becomes a **drop target** (`root`, heading) while a workspace is dragged and folders exist.
4. **Tree** (`role=tree`). Loose workspaces first (aria-level 1), then each folder. Workspaces whose folderId is unknown count as loose.
   - **Workspace row** `nav-ws` (`--active`, `--dragging`, `--drop-before`/`--drop-after`); tooltip = cwd. Left to right:
     - icon and name;
     - namesake hint `nav-ws__hint` (from `namesakeHints`);
     - `nav-end` cluster:
       - `AccountBadge`, only when the workspace's own account ≠ default and there are ≥2 accounts;
       - orchestrator mark (`orch.navMark` / `orch.navMarkWorkers{name,n}`);
       - detached-window mark (`window.inOwnWindow`);
       - asleep moon (`sidebar.asleep{time: proseTime}`);
       - `NavigatorStatus`;
       - "…" more button (`nav-more nav-ctl`).
   - **Folder** `nav-folder`.
     - **Head** `nav-folder__head`, with `--into` (drop into), `--current` (folded and holding the active workspace), `--drop-before`/`--drop-after`. Contents: name, disclosure chevron (`nav-disc--open` rotates it), and — when folded — a `NavigatorStatus` aggregated over every filed workspace, a size count `nav-count` (title `sidebar.folderSize{n}`), and "…".
     - **Body** `role=group`, workspace rows at level 2. An empty folder shows `sidebar.folderEmpty`, which is also a drop target.
5. **Status lines** `nav-empty`:
   - "Open in VS Code" failure note (`editorNotice`, auto-hides after 6s);
   - `sidebar.empty` when there are no projects;
   - filter with no result: `sidebar.noMatch{query}` or `sidebar.noneWaiting`, plus a "Clear filter" link that refocuses the field.
6. **`nav-root` bottom drop zone.** It reads `sidebar.noFolder` while a workspace is dragged; `nav-root--drop` when hovered.
7. **`NavigatorPlan`** (rate-limit meters, see below).
8. **`nav-filter` bar:**
   - "+" new workspace (shortcut `workspace.new`);
   - field `nav-field(--active)`: search icon, input (`sidebar.filter`), and an attention toggle `nav-field__scope` with the waiting diamond glyph (aria-pressed). A press anywhere on the field focuses the input;
   - settings gear (`view.settings`).

**NavigatorStatus.**
- Counters in the order waiting, yours, working, each a `StateGlyph` plus a count (`nav-status__item--{state}`). Nothing is shown when everything is idle.
- Tooltip: `whoIs` → `sidebar.whoWaiting|whoYours|whoWorking{n,names}`, with names joined by `sidebar.namesLast`.
- Click: brings forward the next pane in that state after the current one, wrapping (`bringForward` = `onFocusPane` + focus the terminal on the next frame).
- A screen-reader `role=status` span carries the sentence.

**NavigatorPlan.**
- Data: `usePlanLimits()` from lib/agentState, i.e. the rate limits the hangar-bridge reports. Hidden until there is data.
- Head: `sidebar.plan` plus the next reset time (`resetWhen`).
- One `nav-meter--ok|warn(≥70)|hot(≥90)` per limit: label (`five_hour` → `sidebar.planFiveHour`, `seven_day` → `sidebar.planWeek`, otherwise the kind with spaces), track/fill bar, and a percentage formatted with `Intl` percent.
- Tooltip: `sidebar.planHint` plus one `sidebar.planResets{window,reset}` line per window.

**Interactions:**
- **Click** a row → `setActiveWorkspace`. Click a folder head → toggle `collapsed` (no-op while filtering).
- **Rename in place.** Double-click a workspace row (not on `.nav-ctl`), F2, or the menu "Rename".
  - Input `nav-rename`, autofocus. Enter saves and refocuses the row. Escape cancels (stopPropagation) and refocuses. Blur saves. A trimmed empty value saves nothing.
  - A **new folder** is created with `sidebar.folderDefault` and opens straight into rename with the text selected.
- **Keyboard (single tab stop, roving `cursor` key).**
  - Up/Down/Home/End move.
  - Right: on a closed folder, open it; on an open one, step to its first child.
  - Left: fold an open foldable folder, otherwise go to the parent row.
  - Enter/Space: toggle the folder, or activate the workspace. F2: rename.
  - ContextMenu key or Shift+F10: menu placed under the row. The menu counts as keyboard-opened when the last pointer press was >800ms ago.
- **Filter field.** Down → focus the tree. Escape clears the text, then the toggle, then focuses the tree.
- **Context menu** (right-click, "…", or menu key):
  - *Workspace:*
    1. [`sidebar.goTo` heading + busy agents (waiting, yours, working; glyph, `bringForward`) + separator]
    2. Rename · Open in VS Code (`editor.open`) · Worktrees (`worktrees.action` → WorktreePanel) · MCP servers (`mcpScope.menu` → App) · Kits (`kitsManage.menu` → WorkspaceKitsDialog) · Orchestration settings (`orch.settingsMenu` → App)
    3. [separator, heading `menu.title` (accounts strings), radio items from `accountMenuItems`]
    4. separator, heading `sidebar.moveTo`, each folder (checked and disabled when current), `sidebar.noFolder` (if filed), `sidebar.newFolder` (create, move, rename)
    5. separator, **Sleep** (`sleep.action`, moon). Disabled with a reason from `sleepRefusal` when the verdict is `awake`. The verdict is probed async through `sleep.check` when the menu opens.
    6. **Remove** (danger) → `removeWorkspace` + `mcp_scope_clear`. **No confirmation.**
  - *Folder:* [busy-agent entries if folded], Rename, separator, Remove folder (danger, title `sidebar.removeFolder`). **No confirmation.**
- **Drag reorder** (pointer events, not HTML5 drag-and-drop, because Tauri takes window drops).
  - Starts on press after a 5px threshold. The row takes pointer capture and `body.is-sorting` is set.
  - Hit-testing uses `elementFromPoint` and `data-drop ∈ {workspace, folder, folder-body, root}`. Before/after is decided by the half-height. A gap between rows keeps the previous target.
  - **Workspace:**
    - over a workspace row → beside it: `moveWorkspace(id, anchor.folderId, beforeId)`, where "after" = before the next one;
    - over a folder head or body → into the folder, last;
    - over root → loose, last.
  - **Folder:** over another folder head → `moveFolder(id, beforeId)`; over root → last.
  - The move commits only on pointerup. Cancel or window blur reverts. The click that follows the drop is swallowed.

**NavigatorModel (pure, port 1:1 with tests).**
- `rowKey(kind,id)` = `ws:<id>` / `folder:<id>`.
- `fold` = NFD, strip combining marks, lowercase.
- `readingOrder(ws)` = leaf ids of `normalizeTree`.
- `namesakeHints`: for names shared after `fold`, take the last path segment that no namesake has at the same depth, skipping segments equal to the name; namesakes left without a hint are numbered 1..n.
- `buildNavTree({workspaces,folders,query,attentionOnly,wantsYou})`:
  - A workspace matches by its own name, its folder's name (a folder-name hit shows all its children), any pane name, or any agent label.
  - `attentionOnly` keeps workspaces with a pane `waiting` or `yours`.
  - While filtering, folders are expanded and not foldable; folders with nothing kept are dropped (a folder-name hit alone does not survive `attentionOnly`).
  - Returns `{loose, folders[{folder,workspaces,expanded,size}], rows[{key,kind,id,parent,expanded?,foldable?}], filtering}`.

**`lib/folders.ts`.** `sidebarOrder(ws, folders)` = loose first, then folder order. It is what `workspace.goN`, the palette, Needs-you and hibernation step through.

**Backend.**
- `open_in_vscode {cwd, fromCodeWorkspace}` → `{status: opened|missingFolder|notFound|failed}`.
- `mcp_scope_clear {workspaceId}`.
- Tauri `app.getVersion()`; in Rust, `env!("CARGO_PKG_VERSION")`.

**i18n.**
- `sidebar.*` ×31 across these files (26 in Sidebar, 5 in Plan; NavigatorStatus adds `whoWaiting`, `whoYours`, `whoWorking`, `namesLast`).
- Plus `orch.navMark`, `orch.navMarkWorkers`, `orch.settingsMenu`, `manager.open`, `manager.openHint`, `worktrees.action`, `window.inOwnWindow`, `sleep.action`, `mcpScope.menu`, `kitsManage.menu`, `editor.open`, `cmd.workspace.new`.
- Accounts strings: `menu.title`, `picker.default`, `picker.unknown`.

**CSS** (styles.css). `nav-top*`, `nav-hq`, `nav-section*`, `nav-list`, `nav-tree`, `nav-ws*`, `nav-folder*`, `nav-disc*`, `nav-end`, `nav-mark*`, `nav-status*`, `nav-sr`, `nav-count`, `nav-more`, `nav-ctl`, `nav-rename`, `nav-empty*`, `nav-link`, `nav-root*`, `nav-plan*`, `nav-meter*`, `nav-filter`, `nav-field*`, `nav-tb`, `is-sorting`.

**iced porting notes:**
- **Drag reorder.**
  - Option A (preferred): give rows fixed heights and compute the target from the cursor y against a row-offset table built in `view`. A `mouse_area` per row records the press; a window-level `Subscription` tracks moves; "outside the window" ends the drag on `CursorLeft` or `window::Event::Unfocused`.
  - Option B: a custom widget. Drop markers are a 2px top or bottom border on the target row.
- **Rename-in-place.** Swap the row for a `text_input` with an `Id`, call `text_input::focus(id)` and `select_all`. Blur detection needs a custom wrapper or a focus subscription.
- **Roving keyboard tree.** Store `cursor: Option<RowKey>` in app state and handle keys while the sidebar "has focus". Model a `FocusRegion` enum in the shell; F6/region cycling exists elsewhere.
- **Hover-only "…" button and tooltips.** Use the `tooltip` widget and mouse_area hover state.
- **Async sleep verdict** while the menu is open: a `Task` that updates `probed`.

---

##### 2. Toolbar (+ ToolbarActivity)

**Files.** `components/Toolbar.tsx` (732), `ToolbarActivity.tsx` (215), `ToolbarMenu.tsx` (180).

**Props from App** (`App.tsx:1544`, the active workspace only):

| Prop group | App wiring |
|---|---|
| `workspace`, `view`, `onView` | `setView` (`terminals\|board\|memory\|stats`) |
| `panes`, `focusedPaneId`, `onFocusPane(paneId)` | `focusPane(active, id)` |
| `dockShown`, `onToggleDock` / `modelsOpen`, `modelShown`, `modelsUnseen`, `onToggleModels` | drawers |
| `kitTabs: {key,title,kit,onOpen}[] \| null`, `kitsLoading` | `kitChoices` → `openKit` |
| `canDetach`, `onDetach` / `inOwnWindow`, `onReattach` | window detach |
| `onPreset(size)` | `applyPreset(ws, size, focused)` |
| `onTidy` | `tidyLayout` |
| `onSplit` | `splitPane(ws, null)` |
| `onRestartAll` | `replacePane(…, {})` on every pane |
| `agentPaneCount`, `confirmReset`, `onArmReset`, `onCancelReset`, `onReset` | `resetAgents` |
| `hasNavigator` (main window), `navigatorOpen`, `onToggleNavigator` / `inspectorOpen`, `onToggleInspector` / `onOpenPalette` | — |
| `needsYou: ReactNode` | the `<NeedsYou/>` slot |
| `orchestrated`, `workerCount`, `onOpenOrchestration`, `confirmStopWorkers`, `onArmStopWorkers`, `onCancelStopWorkers`, `onStopWorkers` | — |

**What it shows:**
- **Lead group:**
  - "show navigator" button, only when the navigator is hidden;
  - `h1` workspace name (tooltip: name and cwd);
  - **views radiogroup** with 4 buttons (terminal/board/bookmark/chart icons plus `view.{id}` labels; tooltip `cmd.view.{id}` with the `view.{id}` shortcut).
- **Centre:** the ToolbarActivity capsule.
- **Trail:**
  - the NeedsYou slot;
  - `ConnectedAccountSwitcher`;
  - *terminals view only:* layout presets 1/2/4/8 (`segmented--icons`; aria-pressed when the pane count equals the size), then `toolbar__actions` = Tidy (disabled with <2 panes), separator, New terminal (disabled at `MAX_PANES`), Restart all;
  - "…" more button (`is-open`);
  - separator;
  - Kits button (only when `kitTabs` is set);
  - 3D toggle (only when `modelsOpen`; `tbtn--unseen` dot);
  - Board dock toggle and Inspector toggle (`aria-pressed`, `data-drawer-toggle`).

**Menus and confirmations** (all `ToolbarMenu`):
- **More menu:**
  1. [if the actions are folded *and* in terminals view: New terminal, Tidy, Restart all (with kbd), separator]
  2. [Detach to window / Reattach, separator]
  3. Command palette (kbd)
  4. [if orchestrated: separator, Orchestration settings (kbd), Stop workers (danger; only if workers > 0; arms the confirmation)]
  5. separator, **Reset agents** (danger; disabled with no agent panes; title `topbar.resetAllHint` or `topbar.resetAllNone`; arms the confirmation)
- **Reset confirmation** (alertdialog on "…"): `toolbar.resetTitle{n}`, `toolbar.resetBody`, Cancel (autofocus), danger `topbar.resetAllConfirm{n}`. Also opened by the shortcut or the palette (`confirmReset` from App); cancelling hands focus back to the terminal.
- **Stop-workers confirmation:** `orch.stopTitle{n}`, `orch.stopBody`, Cancel (autofocus), `orch.stopConfirm{n}`.
- **Preset confirmation** (alertdialog on the pressed preset button). Asked when `closedByPreset(panes, tree, size)` contains panes whose agent is not idle.
  - Content: `toolbar.presetTitle{names}` (names joined by `toolbar.presetNames`), `toolbar.presetBody{n}`, and a busy list (glyph, name, `pane.state.{activity}`).
  - Buttons: Cancel (autofocus), danger `toolbar.presetConfirm{n}`.
  - Reset when the workspace, the view or `confirmReset` changes.
- **Kits menu:** one item per kit tab (title, kit name on the right), or a disabled `kitsTab.menuLoading`/`kitsTab.menuNone`. Closes when the workspace changes.

**Keyboard.** The views radiogroup handles Left/Right/Up/Down/Home/End: it selects and focuses, and re-focuses on the next frame (otherwise the terminal grabs focus).

**Responsive** (`@container toolbar`, chrome.css):
- <80em: hide the title;
- <71em: hide the activity context chip and badges;
- <64em: views become icon-only;
- <46em: `toolbar__actions` is hidden. The "…" menu then lists them; the toolbar reads `getComputedStyle(display)==='none'` when the menu opens.

**ToolbarActivity** (capsule `activity`):
- **Focused pane:** glyph, name, coloured state word (`pane.state.*`), then a detail:
  - waiting → `· <tool>` from the latest `permission` event;
  - working with a task → `toolbar.activity.on{title}` (strong);
  - yours with a task → `· toolbar.activity.task{title}`;
  - otherwise → `· toolbar.activity.lastAnswer{answer}`.
- **No focused pane:** workspace name plus `sidebar.activityWorking{n}` or `toolbar.activity.quiet`.
- **Context chip** `toolbar.activity.context{pct}` (from `usePaneInsight().context.pct`) and a thin progress underline (`--activity-progress`).
- **Badges** waiting/yours (glyph + count; title `toolbar.activity.goTo{summary,name}`). A click switches to the terminals view and focuses the next pane in that state, in reading order, wrapping.
- **Data:** `usePaneActivity`, `usePaneInsight`, `useBoardTasks(cwd)` (`board_load`) + `taskForPane`, `useWorkspaceActivity`.

**i18n.**
- Toolbar: `toolbar.*` ×17, `topbar.*` ×8, `orch.*` ×5, `kitsTab.*` ×4, `window.*` ×3 (`detachHint`, `reattach`, `reattachHint`), `model.*` ×2, `board.*` ×2, `create.cancel`, `view.group`.
- Built dynamically: `view.${id}`, `cmd.view.${id}`, `pane.state.${activity}`.
- ToolbarActivity: `toolbar.activity.*` ×8 + `sidebar.activity{Waiting,Yours,Working}`.

**CSS** (chrome.css). `toolbar*`, `tbtn(--toggle|--unseen)`, `segmented(--views|--icons)`, `segmented__btn`, `segmented__icon`, `segmented__label`, `activity*`, `tmenu*`.

**iced:** use the `responsive` widget for the breakpoints; express `em` thresholds as `px × ui_scale`. The confirmations are popover overlays with a focus-trap state. "autoFocus on Cancel" = an initial `focused_button` index in the overlay state.

---

##### 3. StatusBar

**File.** `components/StatusBar.tsx` (208).

**Props from App:**
- `workspace`;
- `showBroadcast` (= `view==="terminals"`);
- `value`, `onChange` (App's `broadcast` string);
- `onSend(whenFree)` → `App.sendBroadcast`: `pty_write(text\r)` to every pane, or `bridge_enqueue` when it should wait until the agent is free;
- `inputRef` (focused by the `view.broadcast` shortcut);
- `paneCount`;
- `bridgedCount` (`useBridgedCount`).

**Shows:**
- **Left, broadcast mode:** broadcast icon; input (`broadcast.placeholder{n}`; Enter sends, **Alt+Enter = when free**; kbd hint for the `view.broadcast` shortcut); "When free" button (clock icon, `broadcast.whenFreeHint{n bridged}`); primary Send. Both buttons are disabled when the field is empty.
- **Left, info mode:** folder icon, cwd (truncated from the start, `dir=ltr`), and a summary — waiting > yours > working counts (`sidebar.activity*`, coloured `status__summary--{kind}`) or `toolbar.activity.quiet`.
- **Right:**
  - git branch item (`status.branch{branch,root}` or `status.detached{head,root}`; a `status.worktree` tag when it is a linked worktree);
  - dictation keys, when `settings.voiceEnabled` (`status.dictate`);
  - pane count (`status.panes{n}`, `status.panesHint`).

**Backend.** `git_repo_info {path}` → `RepoInfo{installed,isRepo,root,mainRoot,isLinkedWorktree,branch,head}`.
- Polled every 20s only while the document is visible.
- Re-read on window focus if more than 1s has passed.
- A same branch, head and root keeps the old object (no re-render).

**i18n.** `status.*` ×8, `broadcast.*` ×4, `sidebar.activity*` ×3, `toolbar.activity.quiet`, `cmd.view.broadcast`.

**CSS.** `status`, `status__broadcast`, `__lead`, `__field`, `__input`, `__kbd`, `__btn`, `__send`, `__info`, `__path`, `__summary--*`, `__items`, `__item(--panes)`, `__text`, `__tag` (chrome.css, `@container status`).

**iced:** `time::every(20s)` gated on `window::Event::Focused`/`Unfocused` and minimised state. Start-truncation of the path needs a custom ellipsis (measure the text and trim the front).

---

##### 4. NeedsYou ("Besoin de toi")

**Files.** `components/NeedsYou.tsx` (315), `lib/needsYou.ts` (244; test 252 LOC, 17 cases).

**Props from App:**
- `items` = `useNeedsYou(sidebarOrder(...)).items`;
- `workspaces`;
- `open`, `onOpenChange` (`needsYouOpen`; also the `view.needsYou` shortcut);
- `onGoToPane` = `goToPane` (activate + focus the terminal);
- `onOpenBoard` = `openBoardOf` (activate + `view=board`);
- `onLand(item)` = `landItem` → `landDelivery(ws, taskId, controlDeps)`, wrapped in `beginDelivery`; it rejects with an error message.

**Shows.**
- **Button** `needs-btn` (only while items > 0): flag icon, `needsYou.title`, count. Title `needsYou.button{n}` with the shortcut.
- **Panel** (`ToolbarMenu role=dialog`, class `needs`, Tab trapped). Header: title and total. Groups in `KINDS` order:
  - **waiting:** diamond glyph, pane name, "`ws` · `needsYou.waitingDetail`", **Go** button.
  - **question:** flag icon, task title, "ws · assignee", the latest comment's first line (`needs__note`, 2 lines max). **Answer** (focuses the orchestrator pane; shown only if there is an orchestrator), plus a board icon button.
  - **land:** merge icon, title, meta, an inline error (`needsYou.landFailed{error}`, role=alert). Primary **Land** (`needsYou.land`/`landing`; disabled and aria-busy while `useDelivering(ws, task)`), plus a board button.
- The first row's first action autofocuses. Row actions close the panel first, then run. Per-row failure map: cleared when an item leaves the list, and on retry.

**lib/needsYou (pure plus 2 hooks):**
- `isOrchestrated` = some pane with role orchestrator or worker.
- `watchedBoards` = unique cwds of orchestrated workspaces.
- `needsYouItems`: empty unless something is orchestrated. Order:
  1. waiting panes, in navigator order;
  2. `needs-you`-labelled open tasks (priority desc, then `updated_at` desc), one board per cwd;
  3. `ready-to-land`-labelled tasks without `needs-you` (oldest first).
- Keys: `pane:<id>`, `task:<cwd>:<taskId>:<label>`.
- `latestNote`: first non-empty line of the last comment.
- `noticeDelta`: only boards already primed raise news; an item that leaves and comes back is news again.
- `useNeedsYou`: `useAgentsIn` + `useBoardsOf` (`board_load` per cwd, plus board events).
- `useNeedsYouNotices`: `notify(title, body, {workspaceId, paneId: orchestrator})` for questions and lands. Main window only; respects `notifyOnIdle` and `notifyOnlyWhenAway`.

**i18n.** `needsYou.*` ×12 (button, title, waiting, questions, lands, board, waitingDetail, goTo, go, answerIn, answer, land, landing, landHint, landFailed) + `notifyTitle`, `notifyQuestion`, `notifyLand`.

**CSS** (orchestration.css). `needs-btn*`, `needs`, `needs__head`, `__title`, `__total`, `__body`, `__group(-title|-count)`, `__list`, `__row`, `__mark--*`, `__text`, `__what`, `__meta`, `__note`, `__error`, `__actions`, `__icon-btn`.

---

##### 5. CommandPalette

**Files.** `components/CommandPalette.tsx` (643), `lib/paletteMatch.ts` (259; test 183 LOC, 21 cases).

**Props from App** (`App.tsx:1824`):
- `onClose`;
- `handlers: ShortcutHandlers` = `paletteHandlers` (what each `CommandId` does *right now*; a command absent here is not listed);
- `keymap`;
- `workspaces` (`sidebarOrder`, or the window's open workspaces);
- `activeWorkspaceId`, `focusedPaneId`;
- `onFocusPane` (activate);
- `onActivateWorkspace` (`setActiveWorkspace` + `view=terminals`);
- `onOpenManager`;
- `extraEntries` (kit tabs: group `kitsTab.paletteGroup`, label `kitsTab.paletteOpen{title}`, detail = kit name, keywords).

**Single mode** (no prefix modes). Item sources, in build order:
1. **Suggestions** (`palette.suggestions`): panes `waiting`, then `yours`, across workspaces (active first, then sidebar order), each in reading order.
2. **Panes of the active workspace** not already suggested (group `keymap.section.panes`). Label `palette.goTo{name}`. Detail: "[ws ·] agent · state". Keys: `pane.focusN` for N ≤ 9.
3. **For each `SECTION_ORDER`** = panes, navigation, view, terminal, commands, app, workspaces:
   - *workspaces section only:* the other workspaces first (`palette.open{name}`; detail = busiest-state count, or `palette.inWindow`, or `status.panes{n}`; keys `workspace.goN`), then "Open project manager" (keywords include "manager", "HQ", `manager.hqName`);
   - every `COMMANDS` entry (lib/shortcuts) in that section that has a handler and is not hidden. Hidden: `pane.focusN`, `workspace.goN`, `voice.dictate`, `view.nextRegion`, `view.prevRegion`. `command.runN` is labelled `palette.runSaved{label}` from `mergeCommands`;
   - *after view:* `extraEntries`.
- **Keywords per command:** the label in the other locale(s) (`LOCALES` + `translate`), toolbar ALIASES (12 ids), `COMMAND_SYNONYMS` (fr+en, 31 ids), the group name in both locales.
- **Icons:** `ICONS` map (≈37 ids), else `SECTION_ICONS`.
- **`OWNS_FOCUS`** (14 ids) skip the terminal refocus after running.

**Matching** (`paletteMatch`):
- `fold` = NFD, strip marks, NBSP/NNBSP → space, lowercase. `foldQuery` also trims and collapses spaces.
- Scores: prefix **4** > word-start **3** > substring **2** > multi-token, all at word starts **1.5** > all tokens anywhere **1** > keyword-only **0.5** (no highlight).
- `foldWithOrigins` maps folded indices back to original UTF-16 ranges; `highlight` turns the ranges into `<mark>` runs.
- `filterPalette`: groups keep their first-seen order. With no query, input order. With a query, a stable sort inside each group by score, then groups by their best score.

**UI and keys:**
- Scrim `palette` (closes on press and release on the scrim). Panel `role=dialog`. Search row: icon, combobox input (autofocus, `aria-activedescendant`), "Esc" button.
- Grouped listbox: heading, then option (icon or glyph, highlighted label, detail, kbd); `is-selected`. Empty: `palette.empty{query}`.
- Footer hints: ↑↓ `palette.navigate`, ↵ `palette.run`, Esc `palette.close`, `palette.anywhere{keys}`.
- Up/Down wrap. Enter runs. Escape dismisses and restores focus (opener, else the terminal). Tab is swallowed. IME composition is ignored.
- Mouse-move selects; click runs. Selection resets to 0 on typing, and the selected row is scrolled into view.
- **Run:** close, then run on the next frame; if focus fell to `<body>`, refocus the terminal.
- The list is rebuilt on every render; `useWorkspaceActivity` keeps it live.

**i18n.** `palette.*` ×14, `keymap.section.*` (dynamic), `sidebar.activity*`, `status.panes`, `manager.open`, `manager.hqName`, `pane.shell`, `pane.state.*`, each command's `labelKey`, plus alias keys.

**CSS.** palette.css: `palette`, `palette__panel`, `__search`, `__input`, `__esc`, `__list`, `__group`, `__heading`, `__option`, `__icon`, `__label`, `__detail`, `__state--*`, `__keys`, `__empty`, `__foot(-end)` (`@keyframes palette-fade`, `palette-rise`).

**iced:** `rich_text` spans for the highlight; `scrollable::scroll_to` to keep the selection visible (needs the row offsets); an explicit overlay layer for the scrim. Keep `paletteMatch` as a pure Rust module with all 21 tests.

---

##### 6. Saved commands: CommandMenu, CommandDialog (+ `lib/savedCommands`)

**CommandMenu** (`CommandMenu.tsx`, 217; hosted by `TerminalPane`'s ⚡ or "…" button):
- Props: `anchorRef`, `workspaceCommands`, `globalCommands`, `onRun`, `onEdit(cmd,scope)`, `onRemove(cmd,scope)`, `onAdd`, `onClose`.
- Groups `commands.scopeWorkspace` / `commands.scopeGlobal`. Row: label plus a mono command (or the mono command alone), a `⇉` broadcast tag, a shortcut (`command.runN`, only when bound) that is swapped on hover or focus for edit and remove buttons.
- Empty state: `commands.empty`. Footer item: "+ `commands.add`".
- Window keydown: Escape closes; Up/Down wrap (also from the anchor). Outside press closes.
- Fixed position, right-aligned, flips above.
- CSS: `cmd-menu*`, `cmd-item*` (styles.css).

**CommandDialog** (`CommandDialog.tsx`, 168; hosted by `WorkspaceTerminals`):
- Modal fields:
  - Command (mono, autofocus, required);
  - Label (placeholder = the command; hint);
  - Auto-run switch (default on);
  - Broadcast switch (default off);
  - Scope segmented: workspace | global.
- Enter in any field submits; Save is disabled while the command is empty.
- `onSubmit({label, command, autoRun, broadcast}, scope)`.
- i18n `commands.*` ×14. CSS `form-grid*`, `switch*`, `seg*`.

**`lib/savedCommands.ts`** (44, no test):
- `mergeCommands(ws, settings)` = workspace commands, then global ones. This is the `command.runN` order.
- `runSavedCommand(cmd, paneIds)`: `pty_write {id, data: cmd + "\r" if autoRun}`, or `bridge_enqueue {paneId, text}` when autoRun and the pane is bridged and `working|waiting`.

**iced:** a `toggler` for the switches; segmented control = a row of styled buttons.

---

##### 7. SessionPicker

**File.** `components/SessionPicker.tsx` (143).

**From App:** `agent`, `cwd` (pane.cwd ‖ ws.cwd), `currentId` (pane.sessionId), `onPick(id|null)` → `replacePane(ws, pane, {sessionId})`, `onClose`.

**Backend.** `list_sessions {agent, cwd}` → `AgentSession{id,label,modified_ms}[]` (an error becomes an empty list).

**Shows** (modal `modal--list`, title `sessions.title{agent}`, cwd as subtitle):
- first row "`sessions.new`" with `sessions.newHint`;
- separator;
- `sessions.loading` or `sessions.empty`;
- session rows: check if current, label or italic `sessions.untitled`, meta = relative time (`sessions.justNow`, `minutesAgo`, `hoursAgo`, `daysAgo`) · short id (`sessionIdLabel` from PaneStatus; full id as tooltip);
- footer: `sessions.note` + Cancel.

Focus jumps to the current session once the list loads. Up/Down step without wrapping. i18n `sessions.*` ×11.

---

##### 8. UpdateBanner

**File.** `UpdateBanner.tsx` (125); main window only.

**Tauri.** `@tauri-apps/plugin-updater` `check()` on mount (silent failure); `update.downloadAndInstall(cb)` with events `Started{contentLength}`, `Progress{chunkLength}`, `Finished`; `@tauri-apps/plugin-process` `relaunch()`.

**Stages:**
- `available`: `update.available{version}`, Later (hide), Install;
- `downloading`: `update.downloading{progress}` plus a progress bar;
- `ready`: `update.ready`, Restart;
- `failed`: `update.failed` (error in the tooltip), Retry → available.

Floating capsule `update update--{stage}` (`@keyframes update-in`). i18n `update.*` ×8.

**iced:** needs a self-update solution (decision: the `self_update` or `cargo-packager-updater` crate behind a `hangar-core::updater` trait), and relaunch = spawn `current_exe` then exit.

---

##### 9. GitCloneConsent (consent card)

**Files.** `GitCloneConsent.tsx` (106), `GitCloneConsent.css` (73); main window only.

**Data.** `useSyncExternalStore(gitCloneConsent.subscribe, gitCloneConsent.pending)`, the queue from `lib/workspaceFromGit`. It is fed by the MCP tool `workspace_create` (lib/control) when the repository is not the person's own.

**Shows only `pending[0]`.** A fixed card at the bottom right (warning left border). It is **not modal and never takes focus**.
- Title: `gitConsent.title{by}` or `titleUnknown`.
- `dl` facts: Repository (url, mono), Owner, Cloned into (destination, mono).
- Reason: `gitConsent.notYours{reason}` (trailing dots stripped).
- Warning paragraph.
- Actions: "`{n} more waiting`", Refuse, Clone → `gitCloneConsent.answer(id, allow)`.

**Side effects.**
- `notify(title, url)` once per request id, when `notifyOnIdle` and (not `notifyOnlyWhenAway` or the window is unfocused).
- Queue timeout 3 min → `timeout`.
- Every answer is logged via `invoke("activity_append", {entries:[consentTrace]})`, in the system locale, with the folder name only.

**Strings** come from `strings/workspaceFromGit.ts` → `workspaceFromGitText(locale, key)` (not `t()`): `gitConsent.*` ×13.

**CSS.** `git-consent`, `__title`, `__facts`, `__mono`, `__reason`, `__warning`, `__actions`, `__more`.

---

##### 10. VoiceHud (+ `lib/voice.ts`)

**VoiceHud** (70):
- Props `phase`, `level` from App's `useVoice`; rendered when phase ≠ idle.
- Pill `voice-pill`: Logo(15), separator, **9 bars**. They scroll every 70ms with the latest level: height = 24% + √level × 76%.
- In `transcribing`, the bars get `is-working` (`@keyframes voice-wave`).
- `role=status`, label `voice.recording` / `voice.transcribing`.

**`lib/voice.ts`** (206, no test): `useVoice({settings, onText, onError})` → `{phase, level, press}`.
- **Push-to-talk.** `press` only starts (`voice_start {config}`), so key auto-repeat is harmless.
- While recording, capture-phase handlers: any keyup → `voice_stop {config, cancel:false}`; Escape → stop with cancel; window blur → stop.
- On mount, `voice_recording` is checked; if a recording is live, it is stopped with cancel.
- **Events:** `voice:phase {phase,error}`, `voice:level {level}`, `voice:text {text}`.
- `voiceConfig(settings)` maps engine, model (local vs cloud), language, cleanup, cleanupModel, cleanupHint.

**iced:** keyup comes from `keyboard::listen` / `on_key_release`; the level ticker is `time::every(70ms)` while recording.

---

##### 11. WorkspaceDialog (+ WorkspaceKitsDialog, WorkspaceHooksSection, git flow)

**Files.** `components/WorkspaceDialog.tsx` (897), `lib/gitClone.ts` (152; test 113 LOC, 8 cases), `lib/workspaceFromGit.ts` (746; test 923 LOC, 65 cases), `strings/workspaceFromGit.ts` (112).

**Props from App** (`App.tsx:1857`):
- `availableAgents: string[]` (installed agent ids);
- `shells: ShellInfo[]`;
- `askerCwd` (active workspace cwd);
- `fromGit(args{repo,name,parent,jobId}) → Promise<FromGitOutcome>` = `createFromGit` → `workspaceFromGit({repo, name, origin:{kind:"person"}, askerCwd}, {...fromGitDeps, projectsFolder: ()=>args.parent, newJobId: ()=>args.jobId})`;
- `onNotice` (`setNotice`);
- `onClose`.

**Store.** `addWorkspace(WorkspaceDraft{name,cwd,extraRoots,layout,agents,shellId,kits,claudeAccount}) → id`; `updateWorkspace(id, {worktreeHooks})`; `state.workspaces` (for pane names); `state.settings` (projects folder, accounts).

**One form, no wizard steps.** A **Source** segmented control (disabled while running) switches between two field sets.

*Folder mode:*
| Field | Details |
|---|---|
| Folder | Path `text_input` + **Browse** (`dialog.open({directory:true})`) + **Open file** (`dialog.open({filters: code-workspace})`). After Browse: the name is filled with the basename if empty, then `find_code_workspace {dir}` → a suggestion notice "`create.fileFound{name}`" [Use] [×]. Loading a file: `read_code_workspace {path}` → `WorkspaceRoot{name,path,exists}[]`, shown as **root buttons** (tag main/extra/missing; a missing root is disabled; a click picks the main cwd). Main = the first existing root; the name comes from the file stem. A failed read shows `create.fileFailed`. |
| Name | Placeholder = basename(cwd). |
| Shell | Select: "`create.shellDefault{name of shells[0]}`" plus every shell; hint `create.shellDetected{n}`. |
| Terminals | 1/2/4/8 segmented, default 4. It resizes `assignments` (padded with shell). |
| Agents | Per non-shell agent: label (+ `create.notInstalled` when missing from `availableAgents`) and a stepper −/count/+. − is disabled at 0; + is disabled with no free slot. Hint `create.freeSlots{n}` / `create.allAssigned`. |
| Layout preview | Grid columns {1:1, 2:2, 4:2, 8:4}. Each cell shows the next pane name (`pickPaneNames(layout, allPaneNames(ws))`) and the agent label. A **click cycles** shell → claude → codex → gemini → …. |
| Claude account | `WorkspaceAccountPicker`, if any accounts. |
| Kits | `WorkspaceKitsSection` (KitActivation.tsx, out of scope). |
| Worktree hooks | `postCreate`, `preRemove` text inputs; `cleanHooks` drops blanks, and an empty result is no setting at all. |

*Git mode:*
| Field | Details |
|---|---|
| Repository | Autofocus. Debounced 250ms `git_repo_parse {input}` → `{status:"ok",url,host,owner,name}` or `{status:"refused",reason}`. Hints: the refusal reason, or `create.gitChecking`. |
| Clone into | Input + Browse (directory). Placeholder = `resolveParent(typed, settings.projectsFolder, parentFolder(askerCwd))`. Hint text, plus `create.gitNoFolder` or `create.gitDestination{path=join(parent, repo.name)}`. |
| Name | Placeholder = the parsed repo name. |
| Progress (while running) | `<progress>` using `overallPercent` (receiving 0–80, resolving 80–100, "updating files" = 100), and `progressLabel` "phase N %" or `create.gitStarting`. After a failure: an error notice (`create.gitFailed{oneLine(reason)}` / `create.gitCancelled`). |

**Footer.**
- Cancel, or while running "`create.gitStop`" → `git_clone_cancel {jobId}`, retried up to 15 × 200ms.
- Submit: folder mode → `create.submit`, disabled without a cwd. Git mode → `create.gitSubmit` / `create.gitRunning`, enabled only when the status is `ready`.

**Validation and states.**
- Folder mode only requires a cwd; there is no client-side existence check.
- Git mode: `fromGitStatus` = empty | checking | refused | noFolder | ready | running. Fields are locked while running; Escape and backdrop close are blocked while running; Enter submits.
- On success: `existing` → notice `create.gitExisting`, `reusedFolder` → notice `create.gitReused`, then close.

**Git flow** (`workspaceFromGit`, pure with injected `FromGitDeps`), step by step:
1. clean the input;
2. parse (`git_repo_parse`);
3. check the name (≤80 chars, no control characters);
4. `parentProblem` (absolute, no `..`, not an OS folder: Windows, Program Files, `/usr`, `/etc`, …);
5. resolve the parent and the destination;
6. in-flight dedupe keyed by the lowercased destination: the same url shares the first run's promise, another url gets `folderBusy`;
7. a workspace already on the destination → `existing` (a person request shows it);
8. `git_repo_ownership {owner, host}` → `{mine, reason}`;
9. *agent origin and not mine* → the consent queue (`allow | refuse | timeout | busy`), then the existing-workspace check again;
10. `git_clone {url, parent, name, jobId}`, with **event `git-clone:progress {jobId, phase, percent}`** → `{status:"cloned", path, reused, branch}` or `refused{kind ∈ invalidUrl|invalidDestination|exists|busy|gitMissing|cancelled|timeout|failed}`;
11. if mine: `claude_trust_folder {folder, account}` (the account comes from `launchAccount(settings)`);
12. `createWorkspace`: layout 1 with one claude pane, updated to `role:"orchestrator"` with the model from `orchestrationOf(ws).orchestratorModel`; activate when the person asked;
13. `bridge_enqueue {paneId, text: firstMessage(...), from:"hangar"}`;
14. `openWorkspace`.
- **Rollback on failure:** `removeWorkspace`, plus `git_clone_discard {path}` when this call made the clone. Refusals are values, never throws.
- `firstMessage` is a fixed English brief (7 steps: read, install, set the check with `workspace_set_check`, run tests, change nothing, propose kits, sum up).

**WorkspaceKitsDialog** (same file). It is opened from the Sidebar menu and from Settings › Kits.
- Modal "`kitsManage.editTitle{name}`": `WorkspaceKitsSection(value=workspaceKits(ws), workspace, onChange → updateWorkspace({kits}))`, then a hooks section (live `updateWorkspace({worktreeHooks: cleanHooks})`), then Done.
- Capture-phase Escape closes only this dialog, unless `#kit-act-title` (the activation view) is open.

**Tauri.** `plugin-dialog` `open` (×3). Rust equivalent: `rfd` (already a dependency).

**i18n.** `create.*` ×50 (20 of them from `strings/workspaceFromGit.ts`), `worktrees.hooks*` ×7, `kitsManage.*` ×3, `pane.shell`, `modal.close`, accounts `picker.label`. The button label is chosen dynamically (`gitButton.label`).

**CSS** (styles.css). `form-grid*`, `seg*`, `seg--numbers`, `picker`, `picker__path`, `notice(--error)`, `notice__text`, `notice__close`, `roots`, `root(.is-main|.is-missing)`, `root__*`, `set-select*`, `agent-counters`, `counter*`, `stepper*`, `preview`, `preview__cell(.is-shell)`, `preview__head`, `preview__name`, `preview__agent`, `form-hint`.

**iced:** the debounced parse = a `Task` with a generation counter; clone progress = a `Subscription`/channel keyed by job id; the stepper = 3 buttons; the preview = a `grid` built from rows and columns; `pick_list` for the shell.

---

##### 12. WorkspaceTerminals (host, mostly glue)

**File.** `WorkspaceTerminals.tsx` (330). One per opened workspace, **never unmounted** (unmounting would kill the PTYs); hidden when not in front.

**Props from App:** `workspace`, `hidden`, `settings`, `availableAgents`, `shells`, `focusedPaneId`, `zoomed`, `onZoomChange`, `onFocusPane`, `onSplitPane(paneId, dir?)`, `onReplacePane(paneId, patch)`, `onOpenSessions(paneId)`, `onPinOrchestrator(paneId, pinned)`, `onOpenOrchestration`.

**Store.** `setTree`, `movePane(ws, dragId, targetId, zone)`, `closePane`, `updatePane({sessionId})`, `addSavedCommand`, `updateSavedCommand` + `moveSavedCommand` (when the scope changed), `removeSavedCommand`.

**Behaviour:**
- Renders `PaneGrid` + `TerminalPane` per pane (out of scope).
- `DiffView` opens from a pane or via `registerDiffOpener(wsId)` (the inspector).
- Hosts the `CommandDialog`. The editor and diff are dropped when the workspace is hidden.
- Zoom ends when the pane count changes; `zoomedId` applies only with >1 pane.
- **File drops:** `watchFileDrops` (Tauri window drag-drop) → hit-test `[data-slot]` → `fileTarget{paneId, count}` highlight → `term.paste(dropText(paths, agent, shell))`.
- Theme = `getTheme(ws.themeId ?? settings.themeId)`.
- Run command: `touchWorkspace` + `runSavedCommand` (broadcast → every pane).

**iced:** this becomes the per-workspace `PaneGridState` owner in the shell. Iced has `window::Event::FileHovered`/`FileDropped`; map the cursor position to a pane via pane_grid layout.

---

##### 13. Claude accounts: model and components

**Model** (`lib/accounts.ts` 174, test 150 LOC/15 cases; `lib/accountSwitch.ts` 160, test 134 LOC/12 cases):
- `ClaudeAccount {id, label, isMain, loggedIn, email?, plan?}`. **`"main"`** is Claude Code's own config: always present, never removable. Each extra account is a separate `CLAUDE_CONFIG_DIR` profile (backend `hangar-core/accounts.rs`).
- **The choice is stored at 3 levels** as `claudeAccount?: string|null` on Settings (default), Workspace and Pane. `readAccountChoice` treats a non-string or blank value as "none" (`state.json` is hand-edited).
- **Resolution** `resolveAccount(accounts, settings, ws, pane)`: pane > workspace > default > main. Unknown ids are skipped; the first one met is reported as `missing`. Returns `{id, source: pane|workspace|default|main, missing?}`.
- `launchAccount`: called by TerminalPane at spawn. It lists accounts only if some level names a non-main id; a failed list falls back to main.
- **Launched registry:** `noteLaunchedAccount(paneId, id|null)`, `launchedAccount`, `subscribeLaunchedAccounts`. "Running" = launched ≠ null. A pane's usage counts against the account it was **launched** on.
- **Switch logic** (pure): `panesToRestartOnWorkspaceChange` = claude panes without their own choice; `panesToRestartOnDefaultChange` = the same, over workspaces without their own choice. Also `runningCount`, `workspacesUsing`, `namesAccount`.
- **Usage:**
  - `usageByAccount(measures, accountOf)`: windows `five_hour` → `fiveHour`, `seven_day` → `weekly`; the latest `at` per account and window.
  - `mergeUsage`: the newer measure wins.
  - `usagePercent` = 0..100 or null.
  - `accountName`, `defaultLabel(template "Default ({name})")`.
- **Backend:**
  - `accounts_list`;
  - `account_add {label}` → `{account, login:{command:"claude auth login"}}`;
  - `account_rename {id, label}`, `account_remove {id}`;
  - `accounts_support` → `{isolatedLogins, note?}` (false on a macOS Keychain that cannot separate logins);
  - **event `accounts:changed`**.

**Hooks:**
- `useAccounts` (42) → `{accounts|null, support|null, refresh}`; refreshes on `accounts:changed`.
- `useAccountSwitching` (89): every switch respawns the running panes it concerns, conversation kept (a respawn with no patch keeps the session id). The Rust port needs no external-store subscription: the launched map lives in app state.
  - `setWorkspaceAccount(ws, id|null, restartNow)` → `updateWorkspace` + respawn of the affected running panes;
  - `setDefaultAccount(id, restartNow)` → `updateSettings({claudeAccount})` + respawn in workspaces without their own choice;
  - `setPaneAccount(ws, pane, id)` → `updatePane` + respawn if running.
- `usageStore.ts` (63) `useAccountUsage()`: listens to **`bridge:event`** `{paneId, event:{kind:"usage", rateLimits}}`, attributes it to `launchedAccount(paneId)`, merges, and persists to **localStorage `hangar.accountUsage`** (port it to a small JSON file in the app data dir).

**Components:**

| Component (LOC) | Host | Shows / interactions |
|---|---|---|
| `AccountSwitcher` (185) + `ConnectedAccountSwitcher` (44) | Toolbar trail (`workspaceId`) | **Button** `acc-switch`: bolt icon, current account name, chevron (title `switcher.button{name}`).<br>**Menu** (`ToolbarMenu`, `acc-menu`): heading `switcher.heading`, then per account a row with:<br>• a main item: check if current, label, `· switcher.notLoggedIn`, a usage line `switcher.usage{five, fiveValue, week, weekValue}` (`switcher.percent{n}` or `switcher.unknown`), email as tooltip only → scope *workspace*;<br>• a secondary item `switcher.useEverywhere` (aria-current when it is the default) → scope *default*.<br>**Running agents in that scope > 0** → an alertdialog: `switcher.askHere` / `switcher.askEverywhere{name}`, `switcher.running{n}` (plural), then **Restart now (conversations kept)** (autofocus), **On next launch**, **Back**. With nothing running it switches immediately. |
| `AccountBadge` (36) | Sidebar row; `PaneAccountBadge` | Chip `acc-badge` with the label (never the email); only when there are ≥2 accounts. Title `badge.title`. |
| `PaneAccountBadge` (22) | TerminalPane header | Only on a claude pane whose resolved source is `pane` and whose account ≠ the workspace's. |
| `PaneAccountItems` (64) | end of `PaneMenu` | Separator, heading `pane.menuHeading`, `menuitemradio` items from `accountMenuItems`. "Default" names the workspace's account. Hidden for non-claude and login panes. A pick → `setPaneAccount`, then the menu closes. |
| `WorkspaceAccountPicker` (65) | WorkspaceDialog | A select: `picker.default{name}` plus every account. Shows `picker.missing{id}` when the value names a gone account. |
| `accountMenuItems` (54; test 36 LOC, 3 cases) | Sidebar menu, PaneAccountItems | Pure builder: `[{key:"default", label:"Default (X)", checked: no known own}, …accounts{checked}]`; `onSelect(null\|id)`. Labels only. |
| `AccountsSettingsPage` (282) + `ConnectedAccountsSettings` (85) | Settings › Accounts (`settings/schema.tsx:847`) | **Header:** `settings.title` and intro. **Warning** when `!support.isolatedLogins` (+ note). **Error** line (`settings.failed{detail}`).<br>**Rows:** name, `Default` tag, logged-in/out tag, "email · plan", `settings.usedBy{names}` / `usedByNone` (resolution includes the default).<br>**Row actions:**<br>• *Log in* (if logged out) → `openLoginPane`: `addPane(active,{agent:"claude"})` + `updatePane({claudeAccount:id, login:true})`; error `settings.noPane` when there is no active workspace or no room;<br>• *Make default* → `updateSettings`;<br>• *Rename* (inline form: autofocus; Save disabled on `labelProblem` = `nameEmpty` / `nameTaken`, case-insensitive, excluding itself; Cancel; Escape);<br>• *Remove* (non-main; inline alertdialog `settings.removeAsk` / `removeBody`, danger confirm autofocus) → `account_remove`, then clear every workspace and pane `claudeAccount` naming it, and reset a default that named it to main;<br>• main shows `settings.mainFixed` instead.<br>**Add form:** name + "Add and log in" (`account_add` → login pane); hint, or the problem after a first submit. `busy` disables everything; the host refreshes after each command. |

**Strings** (`accounts/strings.ts` 194; test 77 LOC, 11 cases):
- Separate en/fr dictionaries: `settings.*` ×30, `switcher.*` ×20, `picker.*` ×4, `pane.*` ×2, `badge.*` ×1, `menu.*` ×1.
- French is passed through `typeset` (narrow no-break spaces before `?!;:»%`, after `«`), done at load.
- Plurals via `_one`/`_other` with `Intl.PluralRules`; numbers via `Intl.NumberFormat`. Placeholders are filled after typesetting, untouched.
- The test enforces matching keys and placeholders and the French typography rules → port it as a Rust unit test.

**CSS.** `accounts.css` (284): `acc`, `acc-switch*`, `acc-menu*`, `acc-ask__choices`, `acc-badge`, `acc-picker`, `acc-select`, `acc-note(--bad)`, `acc-warning`, `acc-error`, `acc-list`, `acc-row*`, `acc-tag(--ok|--off)`, `acc-form`, `acc-input`, `acc-actions`, `acc-confirm*`, `acc-add__title`, `pane-menu__heading`. One transition only.

---

##### 14. Non-visual libs in scope (consumers outside this scope)
- **`sidePanels.ts`** → App layout. `placeSidePanels({room, scale, inspector, dock, model})` → `{fits, width px, max pt}` per panel. Rules:
  - the terminals keep `TERMINALS_MIN` = 640px;
  - priority: model (min 320) > dock (min 300) > inspector (240–440);
  - a panel that does not fit becomes a drawer, leaving a 48px margin;
  - clamp helpers.
- **`useDrawer.ts`** → BoardDock, Inspector, ModelPanel. A transient drawer:
  - focuses the panel on open;
  - closes on an outside pointerdown or focusin (ignoring modals, menus and `[data-drawer-toggle]`) and on Escape (unless something is above it);
  - on close, restores the opener or the pane's terminal.
- **`activity.ts`** → TerminalPane. `createActivityWatcher({idleMs, onSettle, onBusy})`:
  - a burst counts as work after ≥2500ms of output; once that is seen, `onBusy` fires;
  - silence ≥ `idleMs` → settle;
  - a bell settles immediately;
  - settles within 1500ms of each other are debounced;
  - checked every 250ms during a burst.
- **`projects.ts`** → managerControl / remoteControl / orchestratorBrief, for the HQ manager and Hangar Mobile.
  - `loadProjects(deps)`: every non-HQ workspace summarized (team, waiting panes, board counts, needs-you / ready-to-land / doing tasks, plan status, the last 5 journal events). Board and repo reads are deduped per folder.
  - `planStatusOf` (markdown "Status:" extraction).
  - `renderOverview` (≤6000 chars, 3 progressively shorter forms, a footer for omitted projects).
  - `renderProject`.
  - Backend: `board_load`, `memory_load`, `git_repo_info`.
- **`openEditor.ts`**: `openInVSCode` → `open_in_vscode`; `editorNotice` maps the outcome to `editor.missingFolder` / `editor.notFound` / `editor.failed`.

---

#### Logic libs table

Test counts are `it()`/`test()` cases counted with grep; no `.each`.

| module | LOC | test file (LOC, cases) | computes / exports | kind | port target |
|---|---|---|---|---|---|
| `components/NavigatorModel.ts` | 186 | `NavigatorModel.test.ts` (215, **22**) | `rowKey`, `fold`, `readingOrder`, `namesakeHints`, `buildNavTree` → NavTree/NavRow/NavFolder | pure | `hangar-app/src/views/sidebar/model.rs` |
| `lib/folders.ts` | 20 | `folders.test.ts` (42, **5**) | `sidebarOrder(ws, folders)` | pure | `hangar-core::store` helper or `app/src/logic/order.rs` |
| `lib/needsYou.ts` | 244 | `needsYou.test.ts` (252, **17**) | `KINDS`, `isOrchestrated`, `watchedBoards`, `needsYouItems`, `latestNote`, `noticeDelta`, `EMPTY_MEMORY`; hooks `useNeedsYou`, `useNeedsYouNotices` | pure + 2 React hooks (board subscription, `notify`) | `app/src/logic/needs_you.rs` (pure) + shell subscription for boards and notices |
| `lib/paletteMatch.ts` | 259 | `paletteMatch.test.ts` (183, **21**) | `COMMAND_SYNONYMS`, `commandSynonyms`, `SCORE_*`, `fold`, `foldQuery`, `matchText`, `matchEntry`, `highlight`, `filterPalette` | pure | `app/src/logic/palette_match.rs` (unicode-normalization crate for NFD) |
| `lib/savedCommands.ts` | 44 | — | `mergeCommands`, `runSavedCommand` (`pty_write` / `bridge_enqueue`) | pure + IO | `app/src/logic/saved_commands.rs` (calls `hangar-core::pty` / `bridge`) |
| `lib/sidePanels.ts` | 145 | `sidePanels.test.ts` (209, **21**) | constants, `clamp*`, `placeSidePanels` | pure | `app/src/shell/side_panels.rs` |
| `lib/useDrawer.ts` | 126 | — | `useDrawer`, `DRAWER_TOGGLE`, `DrawerCloseReason` | React hook (DOM focus/events) | shell overlay policy in `app/src/shell/drawer.rs` (state machine, not a hook) |
| `lib/projects.ts` | 467 | `projects.test.ts` (526, **23**) | `projectsDeps`, `loadProjects`, `planStatusOf`, `renderOverview`, `renderProject`, types | pure via injected deps (async IO) | `hangar-core::projects` (used by manager/MCP/mobile; no UI) |
| `lib/workspaceFromGit.ts` | 746 | `workspaceFromGit.test.ts` (923, **65**) | path helpers (`parentFolder`, `joinFolder`, `sameRepoUrl`, `sameFolder`, `parentProblem`, `projectsFolderOf`, `resolveParent`, `cloneDestination`), form logic (`fromGitStatus`, `fromGitButton`, `fromGitFieldsDisabled`, `fromGitKey`, `oneLine`), `workspaceFromGit` flow, `createConsentQueue`/`gitCloneConsent`, `consentTrace`/`recordConsent` (`activity_append`), `firstMessage` | pure flow (async deps) + 1 invoke | flow + queue → `hangar-core::git_flow` (async trait deps); form logic → `app/src/dialogs/workspace/git_form.rs` |
| `lib/gitClone.ts` | 152 | `gitClone.test.ts` (113, **8**) | types, `CLONE_PROGRESS_EVENT`, `cleanRepoInput`, `defaultCloneFolder`, `newCloneJobId`, `progressLabel`, `overallPercent`; invokes `git_repo_parse`, `git_repo_ownership`, `git_clone`, `git_clone_cancel`, `git_clone_discard`; `onCloneProgress` listener | pure + IO | already in `hangar-core::git_clone`; UI helpers → `git_form.rs` |
| `lib/accounts.ts` | 174 | `accounts.test.ts` (150, **15**) | types, `MAIN_ACCOUNT`, `LOGIN_COMMAND`, `ACCOUNTS_CHANGED`, 5 invokes, `readAccountChoice`, `resolveAccount`, `launchAccount`, launched registry | pure + IO + module state | resolution → `hangar-core::accounts` (exists, 828 LOC: check overlap); launched map → app state |
| `lib/accountSwitch.ts` | 160 | `accountSwitch.test.ts` (134, **12**) | restart sets, `runningCount`, `usageByAccount`, `usagePercent`, `accountName`, `defaultLabel`, `workspacesUsing`, `mergeUsage`, `namesAccount` | pure | `app/src/logic/account_switch.rs` |
| `components/accounts/accountMenuItems.ts` | 54 | `accountMenuItems.test.ts` (36, **3**) | `accountMenuItems` | pure | `app/src/views/accounts/menu_items.rs` |
| `components/accounts/strings.ts` | 194 | `strings.test.ts` (77, **11**) | en/fr dictionaries, `translateAccounts`, `useAccountsT` | pure + hook | merged into the app i18n (`hangar-app/src/i18n/accounts.rs`), keeping the plural/typeset tests |
| `components/accounts/usageStore.ts` | 63 | — | `useAccountUsage` (`bridge:event` + localStorage) | IO + module store | shell subscription on bridge events; persist in the app data JSON |
| `components/accounts/useAccounts.ts` / `useAccountSwitching.ts` | 42 / 89 | — | account list + refresh; switch actions + respawn | React hooks | `Message::Accounts(..)` handlers in `app/src/state/accounts.rs` |
| `lib/activity.ts` | 96 | `activity.test.ts` (156, **12**) | `createActivityWatcher` | pure timer state machine | `hangar-term` or `app/src/pane/activity.rs` (tick-driven; no window timers) |
| `lib/openEditor.ts` | 44 | `openEditor.test.ts` (60, **6**) | `openInVSCode` (`open_in_vscode`), `editorNotice` | IO + pure | `hangar-core::open_editor` exists; notice → view helper |
| `lib/voice.ts` | 206 | — | `VoicePhase`, `VoiceConfig`, `voiceConfig`, `useVoice` (events `voice:phase`/`level`/`text`; invokes `voice_start`/`voice_stop`/`voice_recording`) | hook + IO | `app/src/shell/voice.rs` (push-to-talk state machine + subscription) |
| `strings/workspaceFromGit.ts` | 112 | (covered in workspaceFromGit.test) | `en`, `fr`, `workspaceFromGitText` | pure | app i18n table |
| `lib/activityLog.ts` (adjacent, not in the brief) | 347 | `activityLog.test.ts` (274, **14**) | the activity log the consent trace appends to | — | owned by another package |

---

---

### 2.2 Inspector, diff view, worktrees, memory, MCP, orchestration panel, stats

#### Scope: Inspector, Diff/Worktrees, Memory, MCP, Orchestration, Stats

**Cross-cutting facts, read these first**
- Every backend command used here already has a Rust implementation in `crates/hangar-core/src/` (`git.rs` 4068 LOC, `stats.rs`, `memory.rs`, `mcp_scope.rs`, `install.rs`, `instructions.rs`, `bridge.rs`, `store.rs`). Most `git_*` functions are `async fn(path: String, …)` with no `AppHandle`. These still need an `AppHandle` and will need native variants: `memory_*`, `mcp_scope_clear`, `bridge_cancel`, `usage_stats`. `usage_stats_at(&Roots, cache, …)` is already free of `AppHandle`.
- Events in scope: `memory:changed` (empty payload, emitted by `memory.rs:653`) and `board:changed` (heard by `lib/paneTask` for the inspector Task section and OrchestrationBadge). Native: core broadcast channel → iced `Subscription`.
- All destructive confirmations are **inline two-click "armed" buttons**, never native dialogs. The source says a native dialog would block the terminals' event loop. Keep them inline in iced.
- Plural keys use `_one`/`_other` with `{n}` (for example `worktrees.cleanupRemove_one`). Interpolation uses `{name}`. The Rust i18n needs both.
- The en/fr number and date formats are pinned by tests: `Intl.NumberFormat` compact, currency `narrowSymbol` ("1,84 $" vs "$1.84"), percent, `h23` clock, French "20 h 01", and NNBSP `\u202f` before "k". This needs a shared `fmt` module (see WP0).
- Modals use `Backdrop` (Escape and click outside close them) plus `ModalHead`. Those are outside this scope but needed by the shell.
- Stylesheets: in-scope CSS covers only the inspector, stats, MCP servers and orchestration. The classes for DiffView `.diff__*` (55 rules @4932), Worktree `.wt*` (31 @4933), Memory `.memory__*`/`.mem__*` (58 @7097) and McpPanel `.target*`/`.reports`/`.manual` live in `src/styles.css` (8361 LOC). I did not read those in full.

---

##### Inspector (right column)

**Files:** `Inspector.tsx` 432, `InspectorSection.tsx` 53, `InspectorPanels.tsx` 535, `InspectorFiles.tsx` 248. Libs: `inspectorFiles.ts` 179, `inspectorFormat.ts` 154. CSS: `styles/inspector.css` 1050.

**Opening, sizing and collapsing (state lives in App.tsx and settings)**
- Shown when `inspectorShown = inspectorDrawer ? drawer === "inspector" : settings.inspectorOpen` (App.tsx:684). Persisted settings: `inspectorOpen` (default true), `inspectorWidth` (default 296), `inspectorCollapsed: string[]` (default []).
- Toggled by command `view.inspector` (`toggleInspector`, App:679). When docked it flips `inspectorOpen`; in drawer mode it flips App's transient `drawer` state. `focusRegion("inspector")` (App:740) opens it or moves focus to it.
- **Dock vs drawer** comes from `lib/sidePanels.computeSides` (out of scope). The inspector docks if `room − width×scale − reserved ≥ TERMINALS_MIN`; otherwise it is drawn over the terminals as a drawer, with `width = min(px, room − DRAWER_MARGIN)`. When both panels are open and only one fits, the inspector is the one that becomes a drawer, not the board dock.
- The drawer comes from `lib/useDrawer`. It takes focus on open and closes on Escape from anywhere, on a pointerdown outside it (capture phase), or when focus leaves. Escape and the close button give focus back to whatever had it. It slides in with the `insp-drawer-in` animation.
- **Width** is in interface points, clamped to `INSPECTOR_MIN 240` … `INSPECTOR_MAX 440`, also capped by `maxWidth` from App, and drawn at width × `--ui-scale`. The grip on the left edge (`role=separator`, focusable) works three ways:
  - Pointer drag: pointer capture, live `--inspector-w` CSS variable, body class `is-resizing`; it commits `onResize` once on release.
  - Double-click resets to `DEFAULT_SETTINGS.inspectorWidth` (296).
  - ArrowLeft grows and ArrowRight shrinks by `KEY_STEP 16`.
- **Section folding:** `onToggleSection(id)`. App reads `snapshot().settings.inspectorCollapsed` and toggles the id. A folded section renders no body and its git polling stops.
- **Close:** header X (title shows the `view.inspector` shortcut). Docked: `updateSettings({inspectorOpen:false})` and focus goes back to the focused pane's terminal (`getTerminal(paneId).focus()`). Drawer: `closeDrawer("button")`.
- Container query `insp (max-width:280px)` hides `.insp-head__version` and the "View diff" label.

**Props from App** (App.tsx:1697): `workspace=activeWorkspace`, `workspaces`, `activeWorkspaceId`, `pane` (the focused pane or null), `paneIndex` (1-based reading order), `shells`, `width`, `maxWidth`, `drawer`, `onResize → updateSettings({inspectorWidth})`, `collapsed`, `onToggleSection`, `onClose=closeInspector`, `onOpenBoard → setView("board")`, `onOpenSessions → setPickerPaneId` (opens SessionPicker), `onOpenMcpServers → setMcpServersFor(activeWorkspace.id)`.

**Data hooks (external stores, all out of scope)**
- `usePaneInsight(paneId)` (lib/paneInsight, fed by bridge events) supplies: `model`, `agentVersion`, `sessionSince`, `context: ContextGauge{pct, usedTokens, window}`, `costUsd`, `queue: QueuedPrompt[]`, `editedFiles`, `events: PaneEvent[]`, `turnStartedAt`, `mcp: PaneMcp`.
- `usePaneActivity` gives `activity` and `bridged`. `usePlanLimits` gives `RateLimit[]` (`kind`, `percentUsed`, `resetsAt`).
- `useBoardTasks(workspace.cwd)` calls `board_load` and listens to `board:changed`; `taskForPane()` picks the task.
- `useStore().state.settings.shellId`.

**No pane state** (`.insp-none`): close X, a mark, `inspector.noPane` and `inspector.noPaneBody`.

**Header** (`InspectorHeader`, `.insp-head`)
- Avatar showing a monogram (first character of the pane name, uppercased) with a `StateGlyph` badge.
- Name, plus a state word `pane.state.<activity>` with class `insp-state--{idle|working|waiting|yours…}`.
- A `<kbd>` with the `pane.focusN` shortcut when the index is 1–9.
- Close X.
- Sub-line: for an agent, `agentLabel [version] · prettyModel(model)`; for a shell, `agentLabel · shellLabel`.

**Panels offered** (the order is fixed; each is memoised):

| id | Title (en) | Component | Visible when | Shows | Interactions / IO |
|---|---|---|---|---|---|
| `session` | Session | `SessionSection` | agent pane (not shell) | key-value grid: **Resume** (only if `AGENTS[].resumable`) = sessionId in mono, or faint "new"; **Since** = `formatSince` (tooltip: full date); **Model** = raw id; **MCP** (claude only) = custom server list or "inherits"; **Folder** = TailPath (leading ellipsis) | "Change" link → `onOpenSessions(paneId)`; MCP "Change" → `onOpenMcpServers`; 30 s minute clock |
| `terminal` | Terminal | `TerminalSection` | shell pane | Shell label, Folder | none |
| `context` | Context and cost | `ContextSection` | agent pane | **Ring** gauge (SVG r=27): pct, tone normal / warn ≥70 / danger ≥90; "X % used", "used / window tokens" (`formatTokens` + `spacedUnit`), hint (`contextHintSoon` at ≥90); otherwise `contextEmpty`. Stat tiles, only when cost or plan is known: **Cost** `formatCost`, "over {elapsed}", or "—" with an unknown tooltip; **Plan** (claude only): two meters `five_hour` / `seven_day` (bar = pct%, tone, tooltip with `resetWhen`), or "—" | 30 s clock |
| `task` | Board task | `TaskSection` | always | card: `#shortTaskId`, `BoardPriority`, title, footer (`review` → "yours" glyph + `taskReview`; `doing` → "Doing for {elapsed}" or `taskDoingNow`), comment count; else `taskNone` | whole card click or "Open ↗" action → `onOpenBoard`; 30 s clock |
| `queue` | Prompt queue | `QueueSection` | `agent==="claude"` or the queue is non-empty | meta = count; ordered list: number, text, "from" chip (`queuedBy`), × button; else `queueEmpty` | × → `invoke("bridge_cancel",{paneId,id})`, fire and forget; the list updates through insight |
| `files` | Changed files | `FilesSection` | always | **edited mode** (agent reported edits): `editedRows`, first 10 + "+N more"; meta "+A −D" totals or a count. **folder mode** (no edits, git known): subhead `filesFolder`, `folderRows` first 8 + more. Row: status letter M/A/D/R/C/U (or "·" when settled, dimmed), dir (tail) + name, +a −d counts and a **5-segment diffbar** (`diffSegments`), or "binary". Empty: `filesClean` / `filesEmpty` | Row click or "View diff" action → `openDiff(workspaceId, paneId)` (terminalRegistry → WorkspaceTerminals mounts DiffView). **Git poll:** `invoke("git_changes",{path:folder, base:null})`, debounced 400 ms after `[paneId, editedFiles, turnStartedAt]` changes, then every 15 s while `document.visibilityState==="visible"`; the next poll is booked only after an answer; **a failure stops polling** until the next trigger; regaining visibility re-schedules after 400 ms; disabled while folded; result discarded if the folder changed |
| `activity` | Recent activity | `ActivitySection` | agent pane, or a shell with events | the last 8 `PaneEvent`s: `clockTime` HH:MM (h23), dot `insp-event__dot--{session\|turn-start\|turn-end\|permission\|edit\|queued\|dropped\|exit}`, `paneEventText(event,t)`. Empty: claude bridged → `activityQuiet`, claude not bridged → `activityEmpty`, other agents → `activityOther{agent}` | none |

`InspectorSection` is a toggle button (chevron, title, meta) with an action slot to its right, shown only when the section is unfolded.

**i18n:** `inspector.*` has 78 en keys (including `inspector.section.*` 7 and `inspector.status.*` 6). Also used: `pane.state.*` (11), `cmd.pane.focusN`, `board.doingSince`, and `sidebar.planReset{Today,Tomorrow,Day,Date}` via `resetWhen`.

**CSS blocks** (inspector.css): `.inspector` (`--inspector-w`, `--insp-pad`, container `insp`), `--drawer`, `__panel`, `__grip` (::before line, `is-active`), `__scroll`. Plus `.insp-head__{avatar,badge,title,name,state,tools,kbd,sub,version}`, `.insp-section` (`--folded`, `__head/__toggle/__title/__meta/__action/__body`), `.insp-kv` (`__split/__clip/__faint`), `.insp-tail`, `.insp-ring` (`--warn/--danger`, `__track/__value/__label`), `.insp-ctx`, `.insp-stats`/`.insp-stat`, `.insp-meter` (`__bar/__value`), `.insp-task`, `.insp-queue`, `.insp-files`/`.insp-file` (`--settled`, `__status--X`, `__path/__dir/__name/__counts`), `.insp-diffbar__seg--{add,del,none}`, `.insp-add`/`.insp-del` (`is-zero`), `.insp-activity`/`.insp-event`, `.insp-empty`, `.insp-more`, `.insp-subhead`, `.insp-link`, `.insp-icon-btn`, `.insp-mono`, `.insp-num`. Grids: key-value 54px|1fr, files 30px|1fr|auto.

**iced notes**
- Resizable side panel: `mouse_area` on the grip, then a cursor-move/release subscription while dragging. Store a dragging width in transient state and write settings on release.
- Drawer: an overlay layer in the shell (`Stack`), with click-outside and Escape handling.
- Ring gauge: a canvas arc.
- Meters and diffbar: plain containers.
- TailPath needs a **leading-ellipsis text** widget (iced has none), shared in hangar-ui.
- Timers: `time::every(30s)` only while a visible section counts time. Git poll: `Task` + sleep with a sequence id. Window focus or minimise events stand in for `visibilitychange`.
- The memoisation per section maps naturally to `lazy`.

---

##### DiffView ("Changes" modal)

**Files:** `DiffView.tsx` 376, `lib/diff.ts` 74 (+ test 57/5), `lib/git.ts` types.

**Opened from:**
1. The pane header "show diff" (`TerminalPane.onShowDiff`).
2. The inspector Files section via `openDiff(ws, pane)`. Both of these are mounted by `WorkspaceTerminals.tsx:298` with `cwd=pane.cwd||ws.cwd` and `label=pane.name`; it is not rendered while the workspace is hidden.
3. The "Changes" button on a WorktreePanel row. That **replaces** the panel instead of stacking on it, with `cwd=worktree.path` and `label=branch|lastSegment`; closing returns to the panel with its state intact.

**Setup**
- `invoke("git_repo_info",{path})`; a rejection means "unavailable".
- If the folder is a linked worktree: `invoke("git_worktree_list",{path})` gives `mainBranch` (the `isMain` worktree's branch). The base toggle is offered only when `mainBranch` differs from the current branch, and **the default base is then mainBranch**, so commits are included. Otherwise the base is null (HEAD).

**Layout**
- Head: title "Changes" + label chip, close X.
- Bar: repo root (truncated), branch badge, segmented "Uncommitted" / "Since {branch}" (sets `base`), spacer, summary "N files +A −D against {base}", Refresh button (disabled while loading).
- Body is split in two:
  - Left: `role=listbox` file list. Each button shows a status mark (`?`→"U", git `U`→"!", others as is) with class `diff__status--{new|M|A|D|R|C|T|U}` and a tooltip `diff.status.X`, then dir + name, then +a −d (not for binary files). Rename tooltip `diff.renamedFrom`. A `truncated` note (`diff.truncated`).
  - Right: the diff pane.

**States:** loading / unavailable / noGit / notRepo / error notice / clean (`diff.clean{base}` or `cleanUnborn`) / pick / per-file error / binary / noText / `diffTruncated` note.

**Loading behaviour**
- Listing: `invoke("git_changes",{path:cwd, base})` on ready and whenever `base` changes. A `loadSeq` counter drops stale answers. Selection is kept if the file is still present, otherwise the first file is selected.
- Per file: `invoke("git_file_diff",{path, file:file.path, oldPath, untracked: status==="?", base})`. Results are cached per `base␁oldPath␀path`; each reload swaps in a fresh cache map.

**Diff rendering**
- Format: **unified only, no side-by-side, no syntax highlighting.** `parseUnifiedDiff(text)` returns rows `{kind: meta|hunk|add|del|ctx|note, text, oldLine?, newLine?}`.
  - The hunk regex `^@@ -(\d+)(,\d+)? \+(\d+)(,\d+)? @@` starts body mode; `diff --git` ends it, so `---`/`+++` header lines stay meta.
  - A trailing empty line is dropped and `\r` is stripped.
- Table with 4 columns: old number | new number | sign (+ / − / blank) | code. Rows `diff__row--{hunk,add,del,ctx,note}`. **Meta rows are hidden.**

**Keyboard:** ArrowUp/Down in the list moves the selection and focus. Escape and click outside go through Backdrop.

**i18n:** `diff.*` has 33 keys (`diff.status.*` 8; `diff.worktree` and `diff.worktreeHint` are used by WorktreePanel).

**iced notes:** a virtualised monospace row list (diffs can be long; the backend truncates). Line numbers are a fixed-width column. Optional `iced::highlighter` later; none exists today. The file list needs keyboard navigation, which means a focusable list widget. A shared `hangar-ui::diff_table` is suggested.

---

##### WorktreePanel ("Git worktrees" modal)

**Files:** `WorktreePanel.tsx` 1630, `lib/worktrees.ts` 46 (no test), `lib/git.ts` 291, `strings/worktrees.ts` 143 (53 en keys merged into i18n). CSS: `.wt*` in styles.css.

**Opened from:** the Sidebar workspace row menu (`Sidebar.tsx:848 setWorktreesFor(ws.id)` → `<WorktreePanel workspaceId cwd onClose>` at :1139; on close, focus returns to the row).

**Store usage:** `state.workspaces`, `addPane`, `addWorkspace`, `setActiveWorkspace`. Reads `workspace.worktreeHooks.{postCreate, preRemove}`; these are edited in `WorkspaceDialog.tsx` (the `worktrees.hooks*` strings), which is out of scope.

**Load**
- `git_repo_info`; if git is installed and the folder is a repo, `Promise.allSettled([git_worktree_list, git_branches])`.
- A branch-listing failure degrades the panel: rows lose commit metadata and the create form has no branch picker, and the error is shown.

**Panel states:** loading; noGit (error notice); notRepo; a "linked" hint (`worktrees.linked{root:mainRoot}`); `none`.

**Worktree row (`.wt`)**
- **Label:** branch, else head, else "—".
- **Badges:** detached (muted), bare (muted), main, current, track gone (alert), ahead↑ behind↓ (muted; parsed from `track` "[ahead 1, behind 2]"), locked (alert; tooltip = reason or `lockedNoReason`), prunable (alert).
- **Path:** shortened to 54 characters from the head, cut at a separator.
- **Stats line:** sha, author, relative age (`ageSec/Min/Hour/Day/Week`).
- **Actions:**

| Button | Shown / disabled | Effect |
|---|---|---|
| Changes (`diff.worktree`) | disabled if busy, bare or prunable | `setDiffFor(wt)` → DiffView replaces the panel |
| Open as workspace | — | `activateWorkspace`: reuse a workspace with the same normalised path, else `addWorkspace({name: branch\|lastSegment, cwd, layout:1, agents:[origin first pane's agent or "shell"], shellId, folderId})`; then close |
| Open in pane | disabled for the current worktree | `addPane(workspaceId,{cwd})`; null → error `worktrees.paneFull`; else close |
| Bring changes back (migrate) | not main, not bare, not prunable | inline confirm notice (`migrateWarn` + Confirm/Cancel) → `invoke("git_worktree_migrate",{path,worktreePath})`. `moved` with `stash===null` → flash `migrateDone`. Anything else becomes a row notice dismissed with ×: `nothing`; `moved` with a stash = `migrateDropFailed`; `conflict` = file list + "and N more"; `applyFailed` = message with `git stash apply {stash}` |
| Lock / Unlock | not main | `git_worktree_lock` / `git_worktree_unlock` `{path,worktreePath}` → reload |
| Remove | not main, not current; **disabled when held** by a workspace or pane cwd (`inUse{name}`) | two-click arm ("Confirm…") → `invoke("git_worktree_remove",{path,worktreePath,force:false,preRemove})`. Removed → reload, then if the branch is now a local branch with no worktree, an **orphan notice** "Delete branch X?" with Delete / Keep → `git_branch_delete{path,branch,force:false}`; `needsForce` → error notice with git's text and "Force delete (-D)" (`force:true`). Not removed: `hookBlocked` → row notice + "Remove anyway" (re-run skipping the hook); `needsForce` (dirty/locked) → row notice + "Force" (`force:true`, `preRemove:null`) |

**Cleanup flow** (footer "Clean up merged…", disabled while open)
1. `invoke("git_worktrees_merged",{path, heldFolders})`, where `heldFolders` = every `ws.cwd` and every `pane.cwd`.
2. A checkbox list; `preselected` items are ticked and locked items are disabled. Badges: locked, `noOwnCommit`, "open in {name}" (held), dirty unknown, "{n} files not committed".
3. Two-click "Remove N" → "Confirm: remove N?". The confirm step adds a `cleanupConfirmDirty{n}` warning when some picked items are dirty.
4. Removal runs **sequentially** with `force:false` and `preRemove`, collecting `forced[]` and `failed[]`, then reloads.
5. Each forced row gets a "Force" button (hidden if the message mentions "locked") → `git_worktree_remove(force:true)`.
6. Orphaned branches are offered: "Delete the branches" / "Keep them" → `git_branch_delete` each with `force:false`. Branches git calls not fully merged trigger a second question, "force (-D)?", with `force:true`.
7. Counters (`cleanupRemoved`, `cleanupBranchesDone`), a failed list, and "Back to the list".

**Create form** (`.form-grid`)
- **Mode** segmented control: New branch / Existing.
- **New:**
  - The name input is run through `sanitizeBranchName` (a VS Code port); the hint `sanitized{name}` shows when the result differs from what was typed. A name that is already a local branch gives `nameTaken`.
  - The **base select** defaults to "current branch/HEAD", with optgroups Local and Remote. A remote base sets `baseRef` to the remote name.
- **Existing:** a select of locals (held ones greyed and disabled, labelled "— taken") and remotes (disabled if the local tracking that remote is held). Resolution rules:
  - A local held elsewhere → `takenBy`.
  - A remote with a tracking local → reuse that local (`trackingTaken` if it is held).
  - Otherwise derive the local name by stripping the first segment (`localExists` if taken) and create it from the remote → hint `willTrack`.
  - `noFreeBranch` when nothing is pickable.
- **Path:** auto `worktreeFolder(mainRoot, branch)` = `<parent>/<repo>-worktrees/<folderName(branch)>`. It follows the branch name until the user edits it (`folderTouched`). Hint `pathHint`.
- **Checkboxes:** Copy ignored files (default on; `COPY_PATTERNS = [".env", ".env.*", "*.local", ".vscode/**"]`) and Open after (default on).
- **Create** is enabled when there is a branch, a path, and nothing busy; it reads "Creating…" while running. Steps:
  1. `resolveFolder` makes a relative path absolute against cwd.
  2. `freeFolder`: `invoke("dir_exists",{path})` on base, base-1 … base-20, else `folderTaken`.
  3. `invoke("git_worktree_add",{path:cwd, worktreePath, branch, createBranch, baseRef})`.
  4. Notes: `folderSuffixed` when a suffix was added; copy → `invoke("git_copy_untracked",{from:info.root??cwd, to, patterns, ignoredOnly:false})` → `copied{n}` + `copyFailed{n}` (warn) or `copyError`.
  5. If `postCreate` is set: busy="hook", `invoke("git_worktree_hook",{path,worktreePath,command})` → `hookProblem(report)` → error `hookPostFailed{problem}`, else note `hookPostDone`.
  6. Flash the notes (info or warn, auto-clears after **7 s**) and reset the form.
  7. If Open after: `activateWorkspace(target, branch)`, and close only if there was nothing to report; else reload.

**Footer:** Fetch (`git_fetch`, label "Fetching…"), Prune (`git_worktree_prune`), Clean up merged…, Refresh, Close. A single `busy` string disables every action. The error notice stays until the next action.

**i18n:** `worktrees.*` = 76 unique bases in i18n.ts (78 lines, some plural) + 53 in `strings/worktrees.ts` (`worktrees.cleanup*`, `migrate*`, `hooks*`, `hookPost*`, `hookRemoveAnyway`). Also `diff.worktree*`.

**iced notes**
- Needs a **grouped select with disabled options** (iced `pick_list` has neither); build it in hangar-ui.
- Path normalisation is Windows-aware (case-insensitive, either slash).
- Keep `busy` as an enum of in-flight operations.
- Each step of create is an async `Task` chain; it can live in hangar-core as `create_worktree_flow` returning a report.
- Panel state must survive while the DiffView sub-view is shown.

---

##### MemoryView (main view, `view==="memory"`)

**Files:** `MemoryView.tsx` 517, `memoryGuards.ts` 45 (+test 58/7), `lib/memory.ts` 58, `lib/memoryFormat.ts` 57 (+test 63/11).

**Props:** `cwd = activeWorkspace.cwd` (App:1651). No store use.

**IO**
- `invoke("memory_load")` → `{entries}`.
- `invoke("memory_create",{input:{title,content,tags,workspace:cwd,agent:"user"}})`. This is an **upsert on title**.
- `invoke("memory_update",{id,patch:{title,content,tags}})`.
- `invoke("memory_delete",{id})`.
- `listen("memory:changed") → refresh`.
- A load failure gives `entries=[]` plus a persistent error banner. **The empty state never shows under an error**, so an unreadable memory never invites rewriting facts that are still on disk. A successful load clears the error.

**Layout**
- **Bar:** a heading ("Memory" while loading, "N results" when searching, "N entries"); search input (whitespace-split terms; every term must appear in title + content + tags, lowercased; **Escape clears** a non-empty field and stops propagation); hint chip (plug icon, agents write via MCP); "New" toggle (`aria-expanded`).
- **Near-cap note** at ≥ `MEMORY_WARN_AT 900` of `MEMORY_CAP 1000`, with a sort toggle recent ↔ oldest (by `updated_at`).
- **Composer:** title (autofocus), content textarea (5 rows), tags (comma-separated, deduplicated case-insensitively), `upsertHint`, Cancel, Create (disabled when title or content is blank).
  - Guard: `findTitleClash` (trim + lowercase, the same key as `memory.rs title_key`). The first click shows `replaceAsk{title}` and the button becomes "Replace"; the second click upserts. Editing the title resets the confirmation.
- **Loading:** a skeleton of 4 ghost rows.
- **Empty:** bookmark icon, `empty`, `emptyBody`, `emptyAction` (opens the composer).
- **No results:** `noResults{q}` + "Clear search" (refocuses the search field).
- **Entry card** (`.mem`):
  - Toggle button: chevron + title; expand/collapse; content is clamped when collapsed.
  - Tag chips.
  - When open: an action row with "created at {full date}", Edit, Delete. Delete is two-step: Cancel / `deleteConfirm`.
  - Edit mode: title, content (8 rows), tags, `tagsHint`, Cancel, Save (disabled when blank).
- **Right column:** signer (`byYou`, or "Written by **{AGENTS label | raw}**"); date (`createdToday`/`updatedToday`/`createdAt`/`updatedAt` with `shortDay`; counts as updated only if more than 60 s after creation); `compactPath(workspace, 30)` with a tooltip.

**i18n:** `memory.*` 39 keys + `create.cancel`.

**iced notes:** `text_editor` for content, chips row, skeleton shimmer (optional), and a subscription on the core memory channel.

---

##### McpPanel ("Connect agents to the board")

**Files:** `McpPanel.tsx` 206. Wrappers are in `lib/board.ts`.

**Opened from:** App `showMcp` (command `view.mcp`, BoardView / BoardDock `onOpenMcp`). Props `cwd`, `onClose`.

**On mount**
- `invoke("mcp_targets",{workspaceCwd})` → `McpTarget{id,label,path,detected,configured,stale,command,project_scoped}`. Pre-ticks targets that are `detected && (!configured || stale)`.
- `invoke("mcp_manual_commands",{workspaceCwd})` → `{executable,claude,codex,workspace}`.
- `invoke("agent_instructions_status",{workspaceCwd})` → bool.
- Implementations: `install.rs:365/386/535`, `instructions.rs`.

**Layout**
- Intro.
- **Install targets:** checkbox rows (class `target--done` when configured and not stale). Badges: stale (alert, with `staleHint{command}` / `staleHintNoCommand`), configured (ok), not detected (muted). Path line.
- **Playbook** checkbox (default on): writes the agent instructions files. Badge "already there".
- **Reports** after install: ok/fail icon, id, message, then `restartHint`.
- **Manual section:** two `<pre>` snippets (claude, codex).
- **Footer:** Close; "Install N" (busy label "Installing…", disabled when 0 selected).

**Install:** `invoke("mcp_install",{ids,workspaceCwd})`; then, if the playbook box is ticked, `invoke("write_agent_instructions",{workspaceCwd})` and its reports are appended (id = file basename); then `mcp_targets` is re-fetched.

**i18n:** `mcp.*` 16 keys.

---

##### McpServersPanel ("MCP servers" for a workspace)

**Files:** `McpServersPanel.tsx` 341, `lib/mcpScope.ts` 292 (+test 229/20). CSS `mcp-scope.css` 315.

**Opened from:** `setMcpServersFor(wsId)` via command `workspace.mcpServers`, the navigator row menu, and the inspector Session section "MCP › Change".

**Props:** `workspace`; `onRestart(paneIds)` (App: `replacePane(ws.id, id, {})` for each, a respawn that resumes the session); `onClose`.

**Store:** `updateWorkspace(ws.id,{mcp})`, saved **on every toggle**.

**Data**
- `invoke("mcp_servers",{cwd, account:null})` → `{servers: McpServer{name, scope user|local|project, transport stdio|http|sse|ws, summary, status active|unapproved|rejected|disabled, hangar, source}[], unreadable: string[]}`.
- Switching back to inherit also calls `invoke("mcp_scope_clear",{workspaceId})`.
- Reads `getPaneInsight(pane).mcp` and `getPaneActivity` (both outside this scope).

**Layout**
- Intro.
- **Mode** segmented: Inherit / Custom. Custom is disabled until discovery returns; choosing it from inherit = `presetAll` (what Claude Code would load). A hint line.
- **List header** with presets **All** (`presetAll`, pressed when equal) and **None** (`presetHangarOnly`: enabled = []).
- **Rows** (`.mcps__row`, `is-off` when not loaded):
  - The Hangar row comes first, with a "Hangar" badge and a **disabled switch** (always on); if Hangar's server is missing, the `hangarMissing` prose shows instead.
  - Other rows: name, status badge with hint (non-active statuses), scope label (tooltip with the source path), transport, summary, and a switch → `toggleServer` → save.
- States: loading / load-failed alert / empty. `strictNote` when custom. One alert per unreadable file.
- **Stats tiles:** `stdioCount` per session; sessions = claude panes; total processes, plus "was N" compared with inherit (class `--less` when lower).
- **Footer:** note `upToDate` or `applies`, plus `busy{n}` (stale panes currently working); Close; **"Restart N agents"** → `onRestart(stale)` then close. Stale panes are those where `needsRestart(running, mcp, servers)` holds; a pane never seen launching counts as stale. Disabled when there are none.

**i18n:** `mcpScope.*` 58 keys (scope, scopeHint and status groups of 3 each, plus statusHint 3).

**CSS:** `.mcps__{mode,mode-hint,group,group-head,group-title,presets,preset,list,row(--hangar),main,name,name-text,badge(--app),meta,scope,transport,summary(--prose),empty,note,alert,stats,stat,stat-n,stat-label,stat-figures,stat-was(--less),busy}` and `.modal--mcps`; switches use the shared `.switch`.

**Note:** `shellFamily`, `quoteArg`, `mcpConfigArg` and `MCP_CONFIG_ENV` in mcpScope.ts belong to pane launch (`lib/agents.launchCommand`), not to this panel. Port them with the launch work package; they are also in mcpScope.test.

---

##### OrchestrationPanel and OrchestrationBadge

**What orchestration means in the UI**
- `Pane.role ∈ {orchestrator, worker, manager}` and `Pane.taskId` (worker).
- `Workspace.orchestration?: OrchestrationSettings` and `Workspace.hq?: true`.
- One Claude Code **orchestrator** per workspace plans on the board and spawns **worker** panes, each in its own worktree; their deliveries are merged after a check and a review. In a non-git folder the workers share the folder and nothing is merged.
- There is one global **manager** ("Project manager") pane, in the HQ workspace (folder from `invoke("hq_dir")`).
- Pin, unpin and create go through `useOrchestrationActions()` (pane menu in TerminalPane, palette, App). `pinRefusal` reasons: `missing | notClaude | worker | pinned | taken | manager | hq`. `roleRestart` decides whether a pane given a role mid-session is restarted with `--resume` (`skip|wait|now|unresumable`), with en/fr notes in `RESTART_TEXT`. `announceRole` (orchestratorBrief, out of scope) tells the session about its new role.

**OrchestrationPanel** (`OrchestrationPanel.tsx` 363)
- Opened via `setOrchestrationFor(wsId)`: command `workspace.orchestration`, navigator, pane menu `orchestration.onSettings`, toolbar. Keyed by workspace id.
- **Props:** `workspace`; `onCreateOrchestrator` (App: close, then `createOrchestrator(ws.id)` = `actions.create`, a new claude pane with role orchestrator on `orchestratorModel`); `onSaved` (App notice `orch.saved{workspace}`); `onClose`.
- **Store:** `updateWorkspace(ws.id,{orchestration: storedOrchestration(next)})`, which stores `undefined` when everything is at its default.
- It is a draft form, with no live saving.
- **Status line** (`.orch-panel__status`, `is-on`): "Pinned: {name}" + "N workers" / "no worker"; or "No orchestrator" + hint + a **"New orchestrator"** button (`cmd.workspace.newOrchestrator`).
- **Section Workers:**
  - Stepper `maxWorkers` 1–8 (default 3) and stepper `maxAttempts` 1–5 (default 2). Each stepper is − / value / + with min and max disabling, and shows "default X" beside it.
  - Select `permissionMode`: "" (theirs) / auto / acceptEdits / bypassPermissions.
- **Section Deliveries:** switch `autoMerge` (default true); text `checkCommand` (mono); text `branch`, validated by `BAD_BRANCH` → error `branchInvalid`.
- **Section Models:** `orchestratorModel` (opus), `workerModel` (sonnet), `reviewerModel` (sonnet). Datalist of opus/sonnet/haiku; validated by `MODEL_NAME /^[\w.:/@[\]-]+$/` → `modelInvalid`; placeholder = default; `modelsHint`.
- **Enter** in any text field saves.
- **Footer:** `applies` note; **Reset** (draft back to defaults, disabled when already at defaults); Cancel; **Save** (disabled when there are problems or nothing changed).

**OrchestrationBadge** (63) is rendered in the TerminalPane header after the pane name (`TerminalPane.tsx:1598`, props `pane`, `cwd`):
- orchestrator → `.orch-badge--lead` with icon + "Orchestrator"
- manager → lead badge + "Project manager"
- worker → `.orch-badge--worker` "Worker · {task title}", where the task comes from `useBoardTasks(cwd)` by `pane.taskId`; tooltip `badgeWorkerHint{title}` / `badgeWorkerNoTask`
- otherwise nothing.

**i18n:** `orch.*` 69 keys (permission 3; badge* 6; section* 3), plus `cmd.workspace.newOrchestrator`, `create.cancel`.

**CSS** (orchestration.css 489):
- `.orch-badge` (`--lead`, `--worker`, `__role/__sep/__task`; the title shrinks first and a narrow header drops the word).
- `.nav-mark--orch` (navigator).
- `.needs-btn*` and `.needs__*` are the **"Besoin de toi" tray (out of scope)**.
- `.orch-panel__{section,heading,grid,status(.is-on),status-icon,status-text,status-sub,model,error}`, `.modal--orch`; a narrow window stacks each label over its field.

**iced notes:** stepper widget, toggler, `combo_box` for models (it replaces the datalist), a grouped form-grid layout with right-aligned labels, and an inline error row.

---

##### StatsView and StatsCharts (main view, `view==="stats"`)

**Files:** `StatsView.tsx` 783, `StatsCharts.tsx` 612, `lib/stats.ts` 156, `lib/statsModel.ts` 576 (+test 264/16). CSS `stats.css` 1092.

**Props:** `workspaces = state.workspaces` (App:1655).

**Data**
- `useUsageStats()` is a module-level store (`useSyncExternalStore`) that lives for the window's lifetime.
- `refreshStats()`: if there is no data yet, first `invoke("usage_stats",{cachedOnly:true})` (used only if it has buckets); then `invoke("usage_stats",{})` for a full scan. Calls arriving during a refresh join the one in flight.
- On mount it refreshes only if the last scan is older than 60 s.
- The UI never passes `from`/`to`.
- Also `invoke("git_worktree_list",{path:ws.cwd})` for every workspace, so worktree folders are counted under their workspace's project. It is re-asked only when workspace folders change.

**`usage_stats` payload**

| Field | Contents |
|---|---|
| `generatedAt` | — |
| `scan` | `{files, parsed, reused, failed, bytes, ms, cachedOnly}` |
| `sources[]` | `{agent: claude\|codex, root, found, files}` |
| `activeGapCapMs` | — |
| `sessions[]` | `{agent, id, cwd, title, firstAt, lastAt}` |
| `models[]` | `{id, priced}` |
| `buckets[]` | `{session idx, at (local hour start), model idx or -1, input, output, cacheWrite, cacheRead, cost, costInput, costOutput, costCacheWrite, costCacheRead, calls, turns, activeMs}` |

**Filters** (persisted in a module variable for the window's lifetime, not in settings; defaults 30d / tokens / all projects)
- **Period:** segmented radiogroup 7d / 30d / 90d / all. Arrow keys, Home and End walk it.
- **Project:** a `ToolbarMenu` dropdown with "All projects", a separator, then each project with a check mark and its share. Also a clear-× button, clicking a project ranking row (toggles), and the project link in the session list.
- **Measure:** tokens / cost.
- **Refresh** button.
- Status: spinner "Refreshing…" / `upToDate{time}` / `fromCache`.

**Model** (`summarize`)
- Range: whole local days up to today, plus a previous range of the same length for trends; "all" starts at the first data day and has no previous range.
- Granularity is **week (Monday start) when the range exceeds 120 days**. Every column of the period is filled, quiet ones included.
- Project of a session = the workspace root deepest above the session cwd. Roots are `ws.cwd`, `extraRoots`, worktrees, and pane cwds that are not above the workspace folder. Otherwise the session's own folder (`dir:` key).
- The project ranking ignores the project filter (it is how a project is picked).
- Top 8 sessions by the measure; each session's model is the one with the most tokens.
- Unpriced model tokens are set aside. Heat = 7×24 grid of activeMs.

**Page states:** loading (busy `PageState` with a chart icon); error + Retry; empty (no buckets) + a list of sources (root, `sourceFiles{n}` / `sourceMissing`). When data exists and a refresh failed, an inline error banner. `.is-stale` dims the page while refreshing over cache-only data.

**Charts** (all hand-drawn SVG today; **redraw with iced canvas**):

| Chart | Type | Axes / geometry | Series and data fields | Interaction |
|---|---|---|---|---|
| KPI sparklines ×4 | polyline + last-point circle | 88×26, y scaled to the max | `series[].tokens`, `.cost`, `.activeMs`, `.sessions` | none |
| KPI tiles ×4 | text | — | **Tokens** `formatCompact(totals.tokens)` + detail cache-read share; **Cost** `formatUsd` + note "API-equivalent"; **Active time** `formatDuration` + detail per active day; **Sessions** `formatInteger` + detail `prompts{n}=turns`. Trend: ▲/▼ `formatChange` vs previous N days, flat when <0.5 %, "new" when previous=0; for "all": `since{date}` | none |
| Per day / per week (`StatsColumns`) | **stacked columns** | Y: `niceTicks(max,4)` (1/2/2.5/5×10ⁿ), labels `formatTick` (compact tokens or compact $), gridlines + zero axis; plot height 184, axis 22, gutter 52. X: one slot per Point, bar = 64 % of slot (≤24 px), rounded top radius ≤4 via clip, 2 px gaps between stacked parts; labels every ≥68 px counted back from the last column; ≤7 days → short weekday + day, else "d MMM"; last label right-anchored | stacks from the baseline up: `cacheRead`, `cacheWrite`, `input`, `output` (colours `--viz-1..4`); tokens mode uses `point[kind]`, cost mode uses `costCacheRead/costCacheWrite/costInput/costOutput` | hover maps x to a column → band highlight, other columns dimmed, readout tooltip (4 kinds + total, full values; flips left past the middle); keys ←/→/Home/End/Escape; "no data" overlay when max ≤ 0; legend; **table toggle** → `StatsColumnsTable` (latest first; columns day/week, output, input, cacheWrite, cacheRead, total) |
| By project | ranked horizontal bars | bar = value / top (min 0.6 %) | `Ranked{label, detail=path, sessions, activeMs, share}` | row click filters or unfilters; 7 shown + "N more" / "fewer" |
| By agent / By model | ranked bars | same | agents (sub: sessions · duration); models `modelLabel` ("Opus 5.5", "GPT-5.6 Sol"; detail = raw id); "unpriced" badge in cost mode; `unpricedNote{tokens}` | none |
| Week rhythm (`StatsHeatmap`) | 7×24 heat grid | rows Mon…Sun (locale short names), columns hours, hour labels at 0/6/12/18; cell 15 px × ui-scale; 6 levels `heatLevel` = ceil(√(v/max)·5) → `--viz-heat-0..5` | `summary.heat[weekday][hour]` = activeMs | hover/keys (arrows, Home/End, Esc; focus starts at now) → readout "day, hour: duration"; legend less→more; **table toggle** → `StatsHeatTable` (24 columns of durations) |
| Heaviest sessions | ordered list | — | rank, title / untitled, project link, agent · model, date, value, `sessionActive` | the project link sets the filter |
| Method footer | text | — | sources, `methodActive{cap: activeGapCapMs}`, cost, `methodScan{files, parsed, ms}` when not cache-only | none |

**i18n:** `stats.*` 99 keys (period 5, periodHint 4, measure 3, kind 4, …).

**CSS:**
- `.stats` (`__bar`, `__bar-end`, `__title`, `__choice`, `__project`, `__picker(.is-filtered,.is-open)`, `__clear`, `__check`, `__status`, `__spinner`, `__scroll(.is-stale)`, `__page`, `__kpis` (4 columns → 2 → 1), `__grid`, `__col`, `__error`, `__state*`, `__sources`, `__method*`).
- `.scard` (`--wide`, `--split`, `__head/__titles/__title(--sub)/__hint/__actions/__toggle/__more/__empty/__note`), `.kpi__*`, `.rank` (`--picking`, `__row(--button)`, `__text/__name/__detail/__numbers/__value/__share/__badge/__track/__fill`), `.slist__*`.
- `.viz-columns(__svg/__band/__col.is-dim)`, `.viz-tip(--left, __title/__row--total/__key/__value/__label)`, `.viz-heat(__grid/__hours/__row/__day/__cell.is-active/__legend/__swatch)`, `.viz-legend`, `.viz-table(--wide)`, `.viz-spark`, `.viz-axis/.viz-grid/.viz-tick/.viz-empty`, `.segmented`, `.stats-sr`.
- Container queries `statsbar` (<52em, <40em) and `statspage` (<760px, <900px). Animations `stats-spin` and `stats-breathe`. Colour tokens `--viz-1..4` and `--viz-heat-0..5` come from `themes.ts:1241` and must go into hangar-theme.

**iced notes**
- Canvas `Program`s for columns, sparkline, heatmap, plus a readout overlay. iced canvas cannot take focus, so keyboard walking needs a "focused chart" state and keys routed while the stats view is active.
- Live regions are dropped. Keep the table twins (they are cheap) and the `nice_ticks` port.
- Weekday names and number formats need the en/fr formatter.

---

#### 2. Logic libraries in scope

| Module | LOC | Test file (LOC, cases) | What it computes / exports | Kind | Port target |
|---|---|---|---|---|---|
| `lib/inspectorFiles.ts` | 179 | .test 144, **14** | `FileRow`; `normalizePath`, `relativeTo` (case-insensitive on Windows drives), `splitPath`, `statusLetter` (`?`→A, `T`→M), `editedRows` (edits merged with git counts, `settled` flag), `folderRows(limit 8)`, `diffTotals`, `diffSegments` (5 GitHub squares) | pure | `hangar-app/src/inspector/files_model.rs` |
| `lib/inspectorFormat.ts` | 154 | .test 110, **10** | `usageTone` (70/90), `prettyModel`, `clockTime`, `formatCost`, `formatPercent`, `spacedUnit` (French NNBSP), `proseTime`, `formatSince`, `resetWhen`, `planUsage` (clamp 0–100) | pure (takes locale and t) | `hangar-app/src/fmt/{time,money}.rs` (shared) + `inspector/format.rs` |
| `lib/diff.ts` | 74 | .test 57, **5** | `parseUnifiedDiff` → `DiffRow{kind, text, oldLine, newLine}`; `splitPath` | pure | `hangar-app/src/git/diff_parse.rs` (or `hangar-core::git::diff_rows`) |
| `lib/git.ts` | 291 | none (gitClone.test is out of scope) | types `RepoInfo, Worktree, MergedWorktree, Branch, HookReport, RemoveResult, DeleteBranchResult, CopyResult, ChangedFile, Changes, FileDiff, MigrateResult`; 17 invoke wrappers (`git_repo_info`, `git_worktree_list`, `git_worktrees_merged`, `git_branches`, `git_worktree_add`, `git_worktree_remove`, `git_worktree_hook`, `git_worktree_migrate`, `git_worktree_prune`, `git_fetch`, `git_branch_delete`, `git_worktree_lock`, `git_worktree_unlock`, `git_copy_untracked`, `git_commits_ahead`, `git_changes`, `git_file_diff`); pure `hookProblem` | IO + 1 pure | call `hangar-core::git::*` directly; `hook_problem` in `git/worktrees/report.rs` |
| `lib/worktrees.ts` | 46 | none | `folderName`, `sanitizeBranchName` (VS Code regex), `COPY_PATTERNS`, `worktreeFolder` | pure | `hangar-core::worktree_naming` (also used by workerControl) |
| `lib/stats.ts` | 156 | none | `UsageStats` types; `usageStats` invoke; module store `refreshStats` (cache-first then full scan, joins in-flight calls), `useUsageStats` (fresh for 60 s) | IO + React hook (external store) | `hangar-app/src/stats/mod.rs` (state + `Task`); types from `hangar-core::stats` |
| `lib/statsModel.ts` | 576 | .test 264, **16** | `Period`, `PERIODS`, `startOfDay/addDays/startOfWeek/weekdayOf`, `periodRange`, `WEEKLY_AFTER_DAYS=120`, `normalizePath`, `workspaceRoots`, `projectFor`, `Totals/Point/Ranked/SessionRow/Summary`, `summarize`, `modelLabel`, `firstActivity`, `heatLevel`, `change`, `formatCompact/Integer/Usd/Tick/Change/Share/Duration` | pure (local-time dependent: needs `chrono::Local`) | `hangar-app/src/stats/model.rs` + `fmt/number.rs` |
| `lib/memory.ts` | 58 | none | `MEMORY_CHANGED_EVENT="memory:changed"`; `MemoryEntry` (snake_case `created_at`/`updated_at`); `memory_load/create/update/delete` | IO | `hangar-core::memory` (needs non-`AppHandle` variants) |
| `components/memoryGuards.ts` | 45 | .test 58, **7** | `MEMORY_CAP 1000`, `MEMORY_WARN_AT 900`, `nearCap`, `titleKey`, `findTitleClash`, `sortEntries(recent\|oldest)` | pure | `hangar-app/src/memory/guards.rs` |
| `lib/memoryFormat.ts` | 57 | .test 63, **11** | `compactPath(max 30)`, `signerName` (AGENTS label, `user`→null), `shortDay` ("today" or localised short date), `wasUpdated` (>60 s) | pure | `hangar-app/src/memory/format.rs` |
| `lib/mcpScope.ts` | 292 | .test 229, **20** (4 of them test `agents.launchCommand`) | types; invokes `mcp_servers`, `mcp_scope_write`, `claude_trust_folder`, `mcp_scope_clear`; pure `INHERIT`, `workspaceMcp`, `inheritedNames`, `isLoaded`, `presetAll`, `presetHangarOnly`, `toggleServer`, `stdioCount`, `needsRestart`; launch helpers `shellFamily`, `quoteArg`, `MCP_CONFIG_ENV`, `mcpConfigArg` | IO + pure | panel logic → `hangar-app/src/mcp/scope.rs`; launch helpers → `hangar-core::launch` (pane work package) |
| `lib/orchestration.ts` | 92 | none (covered indirectly) | `ORCHESTRATION_DEFAULTS`, `ORCHESTRATION_PERMISSION_MODES`, `ORCHESTRATION_ALLOWED_TOOLS`, `orchestrationOf` (defensive read), `NEEDS_YOU_LABEL`, `READY_TO_LAND_LABEL`, `orchestratorOf`, `workersOf`, `managerOf`, `hqOf` | pure | `hangar-core::orchestration` (shared with the backend orchestrator) |
| `lib/orchestrationForm.ts` | 133 | .test 144, **13** | `ORCHESTRATION_LIMITS`, `MODEL_SUGGESTIONS`, `OrchestrationDraft`, `draftOf`, `draftProblems` (MODEL_NAME, BAD_BRANCH regexes), `normalizeDraft`, `sameSettings`, `storedOrchestration` | pure | `hangar-app/src/orchestration/form.rs` |
| `lib/orchestrationActions.ts` | 289 | .test 254, **21** | `pinRefusal`, `isMidTurn`, `roleRestart` + `RESTART_TEXT`/`roleRestartText`, `queueRestartNote`/`takeRestartNote` (module map), `canCreateOrchestrator`, `openManager` (single flight, HQ creation), `hqDir` (invoke `hq_dir`), `useOrchestrationActions` (pin/unpin/create/openManager over store actions + `announceRole`) | pure + IO + React hook | pure parts → `hangar-core::orchestration`; actions → `hangar-app/src/orchestration/actions.rs` (store reducers) |

All tests are vitest. Total: **117 cases** across 9 test files; port them as `#[cfg(test)]` modules with the same expected strings.

---

---

### 2.3 Board and settings

This section has its own numbering: §2.3/1 board, /2 settings (schema, **complete settings table**, hidden fields, shortcuts editor, experiments), /3 Mobile, Voice and Discord cards, /4 logic libs.

#### 1. Board

##### BoardView — `src/components/BoardView.tsx` (743 LOC)
- **Role**: kanban for the active workspace's `cwd`. Two variants. `full` is the "board" view: 4 columns side by side, and they stack below a 640px container width. `dock` is the narrow panel beside the terminals: columns stack and each one folds.
- **Props from App** (all transient App state, not store):
  - `cwd = activeWorkspace.cwd`
  - `onOpenMcp = () => setShowMcp(true)`
  - `findAgent(assignee) → (() => void) | null`. It is App's callback: `findAgentPane(assignee, workspaces, activeWorkspaceId)` then `activateRef(wsId, paneId)`.
  - Dock only: `variant`, `onClose`, `onExpand` (these come through BoardDock).
- **Store reads**: `state.workspaces` and `state.activeWorkspaceId`. Used for `paneFor(assignee) → {id, name, agent label}`, cached per render through `lib/agentPane.findAgentPane` (mapped elsewhere).
- **Local state**:
  - `tasks: Task[] | null` (null = not loaded yet, shows the skeleton)
  - `openTaskId`, `draft{column → text}`, `dragOver`, `dragging`
  - `labelFilter`, `assigneeFilter`, `stateFilter`
  - `toast` (cleared after 6 s)
  - `folded[]` (dock starts with `["todo","done"]` folded), `showFilters` (dock), `composing`, `focusAdd`
- **Loading / refresh**:
  - `board_load {cwd}` returns `{tasks}`; on error the board is shown empty.
  - Re-reads on mount and on `cwd` change.
  - Re-reads on **event `board:changed`** (payload = caller's cwd, deliberately NOT compared because worktrees alias one board).
  - Re-reads every 60 s while any task has `blocked.recheck` (the backend's `sweep_rechecks` unblocks on read).
  - Re-reads after every mutation it makes itself.
- **Clock**: `now` ticks every 30 s while any `doing` task has `doing_since`.
- **Columns**:
  - Order `todo, doing, review, done`. Dock order is `DOCK_ORDER = doing, review, todo, done`.
  - Sort inside a column: `priority desc`, then `order asc`. There is no manual reordering, and a drag never changes `order`.
- **Column header**: mark dot (todo = `--fg-subtle`, doing = `--state-working-fill`, review = `--state-yours-fill`, done = hollow ring), name `board.{col}`, count, and a `+` button (`board.addTo {column}`). In dock mode the header is a fold toggle with a caret.
- **Add field at the bottom of each column** (`column__add`, placeholder `board.add`):
  - Enter or blur creates the task if the trimmed text is non-empty: `board_create {cwd, task:{title, column}}`.
  - Escape clears a non-empty draft (stopPropagation).
  - `startAdding(col)`: composing = true, unfold the column, focus and scroll to its add field.
- **Card** (`article.card`, tabIndex 0, Enter/Space or click opens TaskDialog):
  - Top: `#shortTaskId` (tooltip = full id) and `BoardPriority`. Then title and label chips.
  - On-hold line when `blocked`: pause icon, `board.blocked` · reason · `board.blockedRecheck {date}`. Tooltip adds `board.blockedPerson` when `humanOnly`.
  - Dependency line when not done and `blockersOf` is non-empty: lock icon, `board.blockedBy {tasks}`, each dep written as `#id · title`, or `board.missingTask` for a deleted dep.
  - Footer: `BoardAssignee` or `board.nobody`.
  - Meta: clock with `elapsedLabel(now - doing_since)` (doing only, tooltip `board.doingSince {date}`); link icon + `depends_on.length` only when all deps are done; bubble + comment count.
  - Classes: `card--done`, `card--blocked` (unmet deps), `card--onhold` (blocked), `card--dragging`.
- **Filters** (three `BoardFilter`s, AND-combined):
  - Label: `""` = any, otherwise the labels sorted with localeCompare. A filter value that has left the board stays listed.
  - Assignee: any / `"\0"` = unassigned (`board.unassigned`) / names. Each name's hint is the pane name when it differs.
  - State: any / `blocked` / `free` (based on `task.blocked`, not on dependencies).
  - Clear button (`board.clearFiltersShort`) appears when any filter is active.
  - Count text: `board.shown {shown, n}` while filtering, else `board.tasks {n}`.
  - Full mode hides the filter group when the board is empty and nothing is filtered. Dock mode shows the filter bar via the filter tool button (`is-active` while filtering).
- **Bars**:
  - Full: count, filters, MCP button (`board.hint`, text hidden below 900px container), "Add task" (`board.addTask`, adds to todo).
  - Dock: `board.title`, then the sub-line (`board.dockActive {n = shown doing+review}`, or the count while filtering), then filter, expand (`board.dockExpand`) and hide (`board.dockHide`) buttons.
- **Empty state** (loaded, 0 tasks, not composing): `board.emptyTitle`, `board.emptyBody`, buttons `board.emptyAdd` (add to todo) and `board.emptyMcp` (onOpenMcp).
- **Skeleton**: shapes `[3,2,1,2]`, or `[2,1]` in dock; fades in after a delay.
- **Move**: `moveTask(id, col)`. If `blockedMoveRefusal(task, col)` returns a reason, show toast `board.blockedStays {title, reason}` and make no backend call. Otherwise `board_update {cwd, id, patch:{column}}` (errors swallowed), then refresh.
- **Drag rules**:
  - Pointer events only, no HTML5 DnD: `boardDrag.ts` explains that Tauri's `dragDropEnabled` swallows dragover/drop on Windows.
  - A press starts only if `button === 0` and the target is not inside `button, a, input, select, textarea`.
  - A new press cancels any drag already in progress.
  - The press becomes a drag once the pointer has moved `hypot ≥ 5px` (`DRAG_THRESHOLD`). Then `dragging = id` and `body.userSelect = none`.
  - While dragging, the target column comes from the `[data-column]` section rects: `x ∈ [left, right)`, `y ∈ [top, bottom]`, last match wins (a shared edge belongs to the second column). That column gets `column--over`.
  - Folded dock columns still have their `section`, so their header is a valid drop target.
  - `pointerup`: if the press moved, the pointer is over a column, and that column ≠ `task.column`, call `moveTask`.
  - `pointercancel`, Escape (capture phase + stopPropagation), unmount, or a second press cancel the drag.
  - The click that ends a drag is ignored (`justDragged` flag, reset on the next tick).
  - No dependency check on drop.
- **Focus after the dialog closes**: focus goes back to the card by `data-task-id`, else the first focusable in `.column--{col}` (the card is re-created when an agent moves the task).
- **Toast**: `board__toast`, role=status, 6 s.
- **Claims**: no claim UI at all. `board_claim` is never invoked from the frontend. `board_claim_worker` and `board_release_worker` are used only by `lib/workerControl.ts` (orchestration, mapped elsewhere).

##### BoardDock — `src/components/BoardDock.tsx` (140 LOC)
- An `<aside class="board-dock [--drawer]">` with width `drawnWidth`, a resize grip, and `<BoardView variant="dock">` inside.
- **Props from App** (transient App state or settings):
  - `width = settings.boardDockWidth`
  - `drawnWidth = sides.dock.width`, `maxWidth = sides.dock.max` (from `lib/sidePanels.placeSidePanels`)
  - `scale = uiScale`
  - `drawer = dockDrawer` (there is no room beside the terminals)
  - `paneId = focusedPaneId`
  - `onResize(w) → updateSettings({boardDockWidth: w})`
  - `onClose = closeDock` (`setDrawer(null)` in drawer mode, else `boardDockOpen = false`)
  - `onExpand = setView("board")`
- **Shown when**: `view === "terminals"` and (drawer ? `drawer === "dock"` : `settings.boardDockOpen`).
- **Grip** (`board-dock__grip`, role=separator):
  - Pointer drag paints the width directly during the move and commits once on release. Width = `clamp((startW + startX − x) / scale)` with `clamp = min(maxWidth, max(DOCK_MIN = 300, round))`. Release, pointercancel and window blur all end the drag.
  - Double-click resets to `DEFAULT_SETTINGS.boardDockWidth` (420).
  - ArrowLeft = +24pt, ArrowRight = −24pt.
- **Drawer behaviour**: `useDrawer` (mapped elsewhere) handles Escape, outside click and focus return. The close button calls `closeDrawer("button")`.

##### BoardFilter — `src/components/BoardFilter.tsx` (188 LOC)
- A custom listbox dropdown: button `[name] [value or short] ⇅`, drawn with the accent `is-active` when the filter hides cards.
- **Option shape**: `{value, label, short?, hint?}`.
- **Keyboard on the button**: ArrowUp/ArrowDown open the list.
- **Keyboard in the list** (focused on open): ↑/↓/Home/End move the cursor, Enter/Space pick, Escape closes and refocuses the button (stopPropagation), Tab closes without refocus.
- **Mouse**: hover sets the cursor; a pointerdown outside closes. The menu flips to right alignment (`--end`) when it would overflow `innerWidth − 8`. The cursor row is kept scrolled into view; the check icon marks the selected option.

##### TaskDialog — `src/components/TaskDialog.tsx` (478 LOC)
Portal modal (`modal modal--task` in `Backdrop`: Escape and backdrop click close it, Tab is trapped).

- **Props**: `cwd`, `task`, `allTasks`, `onClose`, `onChanged = refresh`, `assigneePane`, `onFocusAgent`.
- **Header**: column mark, `task.title` + `#id`, column name, close button.
- **Fields** (each edit calls `board_update {cwd, id, patch}`, errors swallowed, then `onChanged`):

  | Field | Control | Saves |
  |---|---|---|
  | Title | input | `{title}` on blur, if changed |
  | Description | textarea, 5 rows, hint `task.descriptionHint` | `{description}` on blur, if changed |
  | Column | select of 4 | `{column}` on change; non-todo options disabled while blocked |
  | Priority | number input 0–9 + BoardPriority | `{priority: Number(v)}` on change |
  | Assignee | BoardAssignee + Release button | `{release: true}` (button `task.release`); `task.free` when nobody |

- **Block section** (shown when blocked, or column is todo or doing):
  - **Blocked**: `board.blockedFor {reason}`, then `board.blockedSince {by, date}` · recheck · person. A note input (Enter submits) and an Unblock button send `unblockPatch(note) = {unblock:{by:"user", person:true, note?}}`.
  - **Block form** (opened by the `board.block` button):
    - Reason: input, maxLength 500, autofocus, Enter submits.
    - Recheck: `type=date` input.
    - `humanOnly` checkbox (`board.blockHumanOnly`).
    - Buttons `create.cancel` / `board.blockConfirm`.
  - **Validation**: empty reason → `board.blockReasonRequired`. `recheckProblem` returns `past` → `board.blockRecheckPast`, `format` → `board.blockRecheckFormat`. A backend error string is shown as-is (`task__error`, role=alert).
  - **Patch**: `blockPatch = {block:{reason trimmed, recheck?, human_only, by:"user", person:true}}`. The backend moves the task to todo, releases the assignee, and refunds an attempt if it was in doing.
  - Errors from the backend are shown here only. Other mutations swallow errors.
- **Read-only blocks**:
  - Label chips (there is NO label editing).
  - Dependency list (NO editing; missing deps are silently dropped here). Each row: check or lock icon, `#id`, title, column.
- **Thread**: comments (author, short date+time). Scrolls to the bottom when the comment count changes. Input + Send, disabled when empty; Enter sends `board_comment {cwd, id, author:"user", text}`.
- **Footer**:
  - `task.createdAt {date}`.
  - Delete is two-step: `task.delete` turns into `create.cancel` / `task.deleteConfirm`. Focus goes to Cancel, then back to Delete. Confirm calls `board_delete {cwd, id}`, then `onChanged`, then `onClose`.
  - Close (primary).
- **External edits**: title and description re-sync from props whenever they change, overwriting local edits in progress.

##### BoardPriority (26 LOC) / BoardAssignee (67 LOC)
- **Priority**:
  - `priorityLevel(p)`: `≤ 0` or non-finite → 1 (Low); `== 1` → 2 (Medium, the backend default); `> 1` → 3 (High).
  - Drawn as 3 bars with `prio--{level}` plus a word (`board.priority.low|medium|high`). Tooltip `board.priorityTitle {n, level}`.
- **Assignee**:
  - `StateGlyph` from `usePaneActivity(pane.id)` (mapped elsewhere; `"idle"` when no pane resolved). No glyph when the task is done; the label is `pane.state.{state}`.
  - Then the name (pane name, else the assignee string), then `· agent label`.
  - When `onFocus` is set it is a button (`card__who--link`, tooltip `board.focusAgent {name}`) that stops propagation for both click and keydown. Otherwise a span with tooltip `board.assigneeTitle`.

##### Task data model (TS `lib/board.ts` = `hangar-core/src/board.rs`)
- **Task**:
  - `id` (UUID), `title`, `description`
  - `column`: `todo | doing | review | done`
  - `priority`: Rust `u8`, default 1
  - `assignee: string | null` (held by an agent or worker)
  - `labels[]` (`needs-you` = waiting for the person)
  - `comments[]`, `depends_on[]` (ids)
  - `created_at`, `updated_at` (ms)
  - `order` (f64, set by `next_order` on create)
  - `doing_since?` (set on entering doing, cleared on leaving)
  - `owns?[]` (globs for orchestration; two doing tasks never overlap)
  - `attempts?` (workers started)
  - `blocked?`
- **Blocked** (camelCase JSON): `{reason (≤ 500 chars), by (agent name | "user"), since, recheck?: "YYYY-MM-DD", humanOnly, reminded?}`. "Blocked" is a state, not a column: a blocked task always sits in todo and is held by nobody.
- **TaskComment**: `{id, author, text, created_at}`. Hangar writes its own comments with author `hangar` (unblock, recheck passed).
- **TaskPatch** (snake_case): `title, description, column, priority, assignee, release, labels, depends_on, order, owns`, plus `block {reason, recheck?, human_only?, by?, person?}` and `unblock {note?, by?, person?}`. The backend refuses `column != todo` while blocked. A `human_only` block can only be lifted with `person: true`, i.e. from the window.
- **NewTask**: `{title, description?, column?, priority?, labels?, depends_on?, owns?}`.
- **Claims**: `claim_task` (agent: refused if held by someone else, blocked, or an owns clash; todo → doing). `claim_for_worker` (todo only, deps done, `attempts < max`, no clash; attempts += 1). `release_worker` (back to todo when it was in todo/doing; `undo` takes the attempt back).
- **Dependency blockers**: `blockersOf(task, byId)` returns each dep id that is not a done task, missing ids included.
- **Other wrappers in board.ts** (MCP panel, mapped elsewhere): `mcp_targets`, `mcp_install`, `write_agent_instructions`, `agent_instructions_status`, `mcp_manual_commands`.

##### Board backend calls and events

| Wrapper | Command | Args |
|---|---|---|
| boardLoad | `board_load` | `{cwd}` |
| boardCreate | `board_create` | `{cwd, task}` |
| boardUpdate | `board_update` | `{cwd, id, patch}` |
| boardDelete | `board_delete` | `{cwd, id}` |
| boardComment | `board_comment` | `{cwd, id, author, text}` |
| boardClaim *(unused in the UI)* | `board_claim` | `{cwd, id, agent}` |
| boardClaimWorker | `board_claim_worker` | `{cwd, id, worker, maxAttempts, sharedFolder}` |
| boardReleaseWorker | `board_release_worker` | `{cwd, id, worker, undo}` |

Event: `board:changed` (payload cwd), emitted by `hangar-core/src/server.rs:66` and `lib.rs:348,361`.

##### i18n and CSS (board)
- **Keys**:
  - `board.*`: 77 entries (73 callable; `_one`/`_other` pairs: `tasks`, `comments`, `shown`, `dockActive`). Sub-prefixes: `board.priority.*` (3), `board.filterName.*` (3).
  - `task.*`: 20.
  - Also used: `create.cancel`, `pane.state.*` (11).
- **CSS** in `src/styles.css`:
  - Board L3789–4602: `board*` 32 classes, `column*` 16, `card*` 17, `board-filter*` 10, `prio*` 5, `skeleton*`. Container queries `board` at 900px and 640px.
  - Dock L4603–4905: `board-dock*` 3, plus the dense dock variant.
  - Task dialog L5279–5673: `task__*` 45, `modal--task`.

##### iced porting notes (board)
- Replace the DOM rects with `mouse_area` per column (`on_enter` / `on_exit`, giving `ColumnHover(col)`) plus a card `mouse_area` (`on_press` with cursor position, `on_move`, `on_release`).
  - Assumption: no drag ghost is needed (React draws none; it only dims the card and highlights the column).
  - Inner buttons capture the press before the mouse_area, which reproduces `startsDrag`.
  - Escape cancels via a keyboard subscription that is only active while dragging.
- `board:changed` becomes a `Subscription` over a hangar-core broadcast channel. Calls go straight to `hangar_core::board::{BoardStore, patch_task, …}` inside `Task::perform` (no IPC).
- Timers: `iced::time::every(30s)` only while a doing task has a clock; `every(60s)` only while rechecks exist; the toast expiry is an `Instant` checked on tick.
- The `type=date` input becomes a text field `YYYY-MM-DD` validated with `recheck_problem` (iced has no date picker; that is an assumption to confirm).
- The priority input becomes a numeric stepper 0–9.
- TaskDialog is a modal overlay (`stack` + scrim). It needs a focus trap or focus manager in hangar-ui; card keyboard-open (Enter/Space) depends on it.

#### 2. Settings

##### How the schema works (`schema.tsx` 1212 LOC, `SettingsPage.tsx` 438 LOC)
- **Builder**: `buildCategories(ctx: SettingsContext) → SettingCategory[]`, rebuilt on every render.
  - **ctx**: `{t, settings, shells, workspace (active), version, experiments?, set(key, value), patchWorkspace(patch), resetSettings}`.
  - `set` = `updateSettings({[k]: v})` (shallow merge into the store).
  - `patchWorkspace` = `updateWorkspace(active.id, patch)`.
  - `resetSettings` = `updateSettings(DEFAULT_SETTINGS)`. Because it is a merge, `projectsFolder`, `kitsEverywhere` and `voiceApiKey` survive a reset, and workspaces are untouched.
- **Shape**:
  - Category `{id, label, icon, sections[]}`.
  - Section `{id, title, hint?, items[]}`.
  - Item base `{id, label, hint?, keywords?}`.
- **Item kinds**:

  | Kind | Payload | Rendered as |
  |---|---|---|
  | `select` | value, choices | native select in a chevron wrapper `set-select` |
  | `segmented` | value, choices | `seg` / `seg__btn is-active` |
  | `toggle` | bool | `switch` |
  | `input` | value, placeholder | `set-input` |
  | `slider` | value, min, max, step, display | `set-range` with a `--fill` % track, plus a `set-value` label |
  | `text` | value | read-only `set-text` |
  | `action` | actionLabel, confirmLabel?, danger? | button; with `confirmLabel` it arms into Cancel + confirm (`btn--danger` or `--primary`) |
  | `custom` | label?, bare?, `render()` | full-width block |

- **Conditions**: built into the code, not declarative.
  - Rows that only exist with an active workspace: `themeScope`, `shellWorkspace`.
  - Voice rows depend on `settings.voiceEngine`: `local` shows VoiceCard + Unload; `groq` shows the cloud-model select.
  - The Experimental category only exists when `availableExperiments()` is non-empty.
  - The version row is omitted while the version is `""`.
  - Nothing is ever disabled (for example, the hibernate delay stays enabled when hibernation is off).
- **Layout** (`SettingsPage`):
  - A Backdrop sheet (`modal-backdrop--settings`, `escape={false}`) over the workspace, so the terminals stay mounted.
  - Left side: search input (autofocus, clear button `settings.searchClear`) and the category nav (icon + label, plus a per-category match count while searching).
  - Right side: title (the active category label) and close button (tooltip `keymap.withKeys {label, keys:"Esc"}`), then sections.
  - A section header is shown only if `section.title !== category.label` or a hint exists.
  - A section is `set-section__bare` when all its items are custom+bare, else `set-group`.
  - A non-custom row is a `<label>` (select, toggle, slider and input kinds), so clicking the text focuses the control.
  - A custom row shows label and hint only if it has a label (so Mobile's hint is used for search only).
- **Search**:
  - `fold` = NFD normalisation, strip U+0300–036F, lowercase.
  - Each item is matched against `label + hint + keywords + section.title + category.label`. Empty sections and categories are dropped.
  - The active category falls back to the first visible one. With nothing visible: `settings.noResults {q}`.
- **Keys**:
  - Escape (window, capture phase) first clears the query, then closes. It is ignored while a shortcut recorder holds `shortcutsSuspended()`.
  - ↑/↓ in the nav move between categories (`menuNav.arrowStep`/`stepIndex`, no wrap) and move focus.
  - Content scrolls to the top on category change.
- **Version**: `getVersion()` from `@tauri-apps/api/app` (in Rust: `env!("CARGO_PKG_VERSION")`).
- **Props from App** (transient App state):
  - `shells` (App state filled by `detect_shells`)
  - `category`: App's `settingsCategory`, default `"general"`; the `view.shortcuts` command opens `"shortcuts"`. Changing it while the page is open switches category.
  - `onClose = setShowSettings(false)`. The `view.settings` shortcut toggles the page.
- **Store reads**: `state.settings`, `activeWorkspace`, `updateSettings`, `updateWorkspace`.

##### Complete settings table
Category order: general, appearance, terminal, shell, notifications, mobile, kits, accounts, voice, discord, shortcuts, [experimental], about.

**General** (icon `sliders`, `settings.catGeneral`)

| key | control | default | options / range | label key | notes |
|---|---|---|---|---|---|
| `locale` | segmented | `null` | `""` = Auto (`settings.localeAuto`), `en` "English", `fr` "Français" | `settings.languageLabel` (section `settings.language`) | `""` ↔ null |
| `projectsFolder?` | custom: path input + Browse | absent | free path; placeholder `settings.projectsFolderPlaceholder` | `settings.projectsFolder` / `…Hint` | keys live in `strings/workspaceFromGit.ts`. Browse = folder dialog titled `settings.projectsFolderDialog`, button `create.browse`. Stored raw on each keystroke |
| `autoResume` | toggle | true | — | `settings.autoResume` / `…Hint` | section `settings.sessions`; TerminalPane |
| `claudeBridge` | toggle | true | — | `settings.claudeBridge` / `…Hint` | hangar-bridge plugin injected into Claude Code panes |
| `launchDelayMs` | slider | 700 | 0–4000, step 100, "{n} ms" | `settings.launchDelay` / `…Hint` | item id `launchDelay` |
| `hibernateIdle` | toggle | true | — | `settings.hibernate` / `…Hint` | section `settings.memory` |
| `hibernateAfterMin` | select | 10 | {5,10,20,30,60} ∪ {current}, sorted, "{num} min" | `settings.hibernateDelay` / `…Hint` | value stored as a string, `Number()` on save |
| `agentPaneControl` | toggle | true | — | `settings.agentPaneControl` / `…Hint` (section `settings.agentControlSection`) | gates App's control dispatcher `enabled()`; no backend call |
| `agentCommands.{claude,codex,gemini,opencode}` | 4 inputs | `{}` | placeholder = the default command (`claude`, `codex`, `gemini`, `opencode`) | literal agent labels "Claude Code", "Codex", "Gemini", "OpenCode"; section `settings.agentCommands` + hint `settings.agentCommandsHint` | item id `agentCommand.{id}`; blank removes the key; stored untrimmed |

**Appearance** (icon `appearance`, `settings.catAppearance`)

| key | control | default | options / range | label key | notes |
|---|---|---|---|---|---|
| `uiScale` | slider | 100 | 90–130 (`UI_SCALE_MIN/MAX`), step 5, "{n} %" | `settings.uiScale` / `…Hint` | section `settings.interface`; global UI scale |
| `navigatorOpen` | toggle | true | — | `settings.showNavigator` / `…Hint` + " · {keys}" | keys from `shortcutLabel(keymap, "view.navigator")` |
| `inspectorOpen` | toggle | true | — | `settings.showInspector` / `…Hint` + keys | from `view.inspector` |
| Workspace.`themeId` (item `themeScope`) | toggle | null | on = pin the current effective theme, off = null | `settings.themePerWorkspace` / `…Hint {name}` | only with an active workspace; `patchWorkspace` |
| `themeId` (item `themePick`) | custom ThemeGrid | `"graphite"` | 33 THEMES (25 dark, 8 light) under `settings.themeDark` / `settings.themeLight`; preview cards | no label; section `settings.theme` hint `settings.themeHint` | writes Workspace.themeId when pinned; keywords = all theme names |

**Terminal** (icon `terminal`, `settings.catTerminal`)

| key | control | default | options / range | label key | notes |
|---|---|---|---|---|---|
| — (item `termPreview`) | custom bare TerminalPreview | — | — | section `settings.preview` | reads font, size, lineHeight, letterSpacing, padding, cursor |
| `fontFamily` (item `font`) | custom FontGrid | `"Cascadia Code", "JetBrains Mono", Consolas, "Courier New", monospace` | 7 stacks: Cascadia Code (= default), JetBrains Mono, Fira Code, Consolas, Hack, IBM Plex Mono, System; "Custom" card (`settings.fontCustom`) when the value is unknown | `settings.font` / `…Hint` | section `settings.typography`; sample "Aa Bb 0123 => != ->" |
| `fontSize` | slider | 13 | 9–24, step 1, "{n} px" | `settings.size` | |
| `lineHeight` | slider | 1.3 | 1–2, step 0.05, num with 2 decimals | `settings.lineHeight` | |
| `letterSpacing` | slider | 0 | −1–3, step 0.5, "{n} px" | `settings.letterSpacing` | |
| `padding` | slider | 10 | 0–28, step 1, "{n} px" | `settings.padding` | |
| `cursorStyle` | segmented | `"bar"` | bar / block / underline (`settings.cursorBar` / `Block` / `Underline`) | `settings.cursor` (section `settings.cursorSection`) | |
| `cursorBlink` | toggle | true | — | `settings.blink` | |
| `scrollback` | slider | 10000 | 1000–50000, step 1000, display num(n) | `settings.scrollback`, hint `settings.scrollbackLines {n}` | section `settings.history` |

**Shell** (icon `prompt`, `settings.catShell`)

| key | control | default | options / range | label key | notes |
|---|---|---|---|---|---|
| `shellId` (item `shellDefault`) | select | null | `""` = `settings.shellAuto {name: shells[0].label ?? create.shellNone}`, then detected shells | `settings.shellDefault` | section `settings.shell` hint `settings.shellDetected {n}` (plural) |
| Workspace.`shellId` (item `shellWorkspace`) | select | null | `""` = `settings.shellFollow`, then shells | `settings.shellWorkspace` / `…Hint` | only with an active workspace |

**Notifications** (icon `bell`, `settings.catNotifications`; section `settings.notifications`)

| key | control | default | options / range | label key | notes |
|---|---|---|---|---|---|
| `notifyOnIdle` | toggle | true | — | `settings.notifyOnIdle` / `…Hint` | lib/needsYou, TerminalPane |
| `notifyIdleMs` | slider | 3000 | 1000–15000, step 500, "{num(ms/1000, 1)} s" | `settings.notifyIdle` | |
| `notifyOnlyWhenAway` (item `notifyAway`) | toggle | true | — | `settings.notifyAway` / `…Hint` | |

**Entry-point and custom categories**

| category | key | control | notes |
|---|---|---|---|
| mobile (icon `broadcast`, `settings.catMobile`) | — | custom bare `MobileSettings` (§3) | hint `mobile.switchHint` (search only), keywords `mobile.keywords`; edits backend state and Workspace.`mobilePorts` |
| kits (icon `plug`, `kitsManage.title`) | `kitsEverywhere`, Workspace.`kits` | custom bare `KitsSettings` | **entry point, mapped elsewhere**; hint `kitsManage.intro`, keywords `kitsManage.keywords` |
| accounts (icon `bolt`, accounts strings `settings.title` via `translateAccounts`) | `claudeAccount` | custom bare `ConnectedAccountsSettings` | **entry point, mapped elsewhere**; hint from accounts strings `settings.intro` |

**Voice** (icon `mic`, `settings.catVoice`; section `settings.voice`, hint `settings.voiceHint`)

| key | control | default | options / range | label key | notes |
|---|---|---|---|---|---|
| `voiceEnabled` | toggle | false | — | `settings.voiceEnable` / `…Hint` | |
| — (item `voiceShortcut`) | text | — | `shortcutLabel(keymap, "voice.dictate")` | `settings.voiceShortcut` | empty when unbound |
| `voiceEngine` | segmented | `"local"` | local (`settings.voiceEngineLocal`) / groq (`settings.voiceEngineCloud`) | `settings.voiceEngine` / `…Hint` | switches the next rows |
| `voiceModel` (item `voiceModel`) | custom VoiceCard | `"whisper-small-q5_1"` | catalogue from `voice_models` | `settings.voiceModel` | engine=local only; display/download, no picker |
| `voiceCloudModel` | select | `"whisper-large-v3-turbo"` | "Whisper large-v3 turbo — $0.04/h", "Whisper large-v3 — $0.111/h" | `settings.voiceCloudModel` / `…Hint` | engine=groq only |
| `voiceLanguage` | select | `""` | `""` = `settings.voiceLanguageAuto`; en fr de es it pt nl pl ru uk sv (code shown as label) | `settings.voiceLanguage` / `…Hint` | `""` becomes null in VoiceConfig |
| `voiceSubmit` | toggle | false | — | `settings.voiceSubmit` / `…Hint` | |
| — (item `voiceUnload`) | action, no confirm | — | button `settings.voiceUnloadAction` | `settings.voiceUnload` | engine=local only; calls `voice_unload` |
| keychain key + legacy `voiceApiKey?` (item `voiceApiKey`, section titled `settings.voiceEngineCloud`) | custom VoiceKeyCard | absent | — | `settings.voiceKey` | always shown; `voice_key_*`; the drop action sets `voiceApiKey` to undefined |
| `voiceCleanup` | toggle | false | — | `settings.voiceCleanup`, hint `settings.voiceCleanupHintRow` | section `settings.voiceCleanupSection` |
| `voiceCleanupModel` | input | `""` | placeholder `"llama-3.1-8b-instant"` | `settings.voiceCleanupModel`, hint `…Hint {model}` | trimmed on every change |
| `voiceCleanupHint` | input | `""` | placeholder `settings.voiceVocabularyPlaceholder` | `settings.voiceVocabulary` / `…Hint` | |

**Discord** (icon `gamepad`, `settings.catDiscord`)

| key | control | default | options / range | label key | notes |
|---|---|---|---|---|---|
| — (item `discordCard`) | custom bare DiscordCard | — | — | section `settings.preview` | built with `discordPresence` forced true; greyed when off |
| `discordPresence` | toggle | false | — | `settings.discordEnable` / `…Hint` | section `settings.discord` hint `settings.discordHint`; App's `useDiscordPresence` calls `discord_presence_set` |
| `discordShowWorkspace` | toggle | true | — | `settings.discordWorkspace` / `…Hint` | |
| `discordShowAgents` | toggle | true | — | `settings.discordAgents` / `…Hint` | |
| `discordAppId` | input | `""` | placeholder `settings.discordAppIdPlaceholder` | `settings.discordAppId` / `…Hint` | section `settings.discordApp`; trimmed; empty → `DEFAULT_APP_ID 1534592584045957282` |

**Shortcuts** (icon `keyboard`, `settings.catShortcuts`)

| key | control | default | label key | notes |
|---|---|---|---|---|
| `keybindings` (item `keymap`) | custom bare ShortcutsSettings | `{}` | section `settings.shortcutsAll` hint `settings.shortcutsHint {mod: Cmd\|Ctrl}` | overrides only; `[]` = command disabled |

**Experimental** (icon `bolt`, `settings.catExperimental`; section `experiments.title`, hint `experiments.warning`)

| key | control | default | label key | notes |
|---|---|---|---|---|
| `experiments` (item `experiment.{id}` per entry) | toggle | `[]` | `t(exp.title)`; hint = `experiments.stage.{stage}` · `experiments.onlyOn {platforms}` · `t(exp.hint)` | `withExperiment` keeps the order turned on. `EXPERIMENTS = []` today, so the category is absent (it sits just before About when present) |

**About** (icon `info`, `settings.catAbout`)

| key | control | value / buttons | label | notes |
|---|---|---|---|---|
| — (item `appName`) | text | `"hangar-ai"` | literal "Hangar.AI" | section title hidden (same as the category) |
| — (item `version`) | text | `getVersion()` | `settings.version` | omitted while empty |
| — (item `reset`) | action, danger, two-step | `settings.reset`, then `settings.resetConfirm` | `settings.reset` / `settings.resetHint` | section `settings.maintenance`; `updateSettings(DEFAULT_SETTINGS)` |

##### Settings fields with no schema row (hidden / internal)

| field | type | default | edited by |
|---|---|---|---|
| `savedCommands` | `SavedCommand[] {id, label, command, autoRun, broadcast}` | `[]` | command menu / dialog (elsewhere) |
| `voiceModel` | string | `"whisper-small-q5_1"` | shown by VoiceCard, never set by the UI (an unknown id resolves to `models[0]`) |
| `voiceApiKey?` | string | absent | legacy only; VoiceKeyCard can move it to the keychain or drop it |
| `boardDockOpen` | bool | false | `view.boardDock` toggle, dock close |
| `boardDockWidth` | number (pt) | 420 | BoardDock grip |
| `modelPanelWidth` | number | 440 | ModelPanel grip |
| `inspectorWidth` | number | 296 | Inspector grip |
| `inspectorCollapsed` | string[] | `[]` | Inspector sections |
| `claudeAccount?` | string \| null | `"main"` | Accounts page only |
| `kitsEverywhere?` | `WorkspaceKit[] {id, granted[]}` | absent | Kits page only |
| `experiments` | string[] | `[]` | effectively hidden while the registry is empty |

- Workspace fields edited from Settings: `themeId`, `shellId`, `mobilePorts`.
- Fields with non-row custom controls: `themeId`, `fontFamily`, `projectsFolder`, `keybindings`.

##### ShortcutsSettings — `ShortcutsSettings.tsx` (473 LOC, UI only)
- **What it needs from the shortcut model**:
  - `lib/shortcuts`: `COMMANDS`, `COMMANDS_BY_ID`, `SECTIONS` (8 sections: panes, navigation, workspaces, view, terminal, commands, voice, app → `keymap.section.*`), `resolveKeymap(overrides)`, `bindingsOf`, `commandLabel(t, cmd)`, `commandsFor(keymap, chords, exceptId)`, `findConflicts`, `isCustomized`, `withBinding`, `withoutBinding`, `resetCommand`, `shortcutLabel`; types `CommandId`, `Keymap`, `ShortcutCommand`, `ShortcutSection`.
  - `lib/keys`: `chordFromEvent`, `chordSegments`, `parseBinding`, `writeBinding`, `formatBinding`, `isBareChord`, `isMac`; types `Binding`, `Chord`.
  - `lib/useShortcuts`: `suspendShortcuts() → release` and `shortcutsSuspended()`.
- **Top bar**:
  - Search (`fold` over label + formatted bindings + section name). Focusing the search cancels any recording.
  - Conflict badge: `keymap.conflicts {n}` or `keymap.noConflict`; tooltip lists the bindings.
  - "Reset all": two-step (`keymap.resetAll` → `create.cancel` / `keymap.resetAllConfirm`), disabled when nothing is customised. Sets `keybindings = {}`.
- **Command row**:
  - Label, `keymap.custom` tag when customised.
  - Each binding as kbd chords with an × (`keymap.remove {keys}`), or `keymap.unbound` when there is none.
  - `+` (`keymap.add`) starts recording; reset (`keymap.reset`) is disabled unless customised.
  - Conflict warning per binding: `keymap.alsoBound {keys, commands}` with a `keymap.free` link that removes the binding from the rival commands.
- **Recorder** (one row at a time):
  - Suspends global shortcuts and captures keydown and keyup in the capture phase.
  - Modifiers alone show live segments: ⌃⌥⇧⌘ on Mac, Ctrl/Alt/Shift/Win elsewhere.
  - Bare Escape (no Shift) cancels. Bare Enter confirms if at least one chord was recorded.
  - Every other chord is appended, up to 2; a third chord restarts the sequence.
  - ✓ and ✗ buttons (`keymap.confirm`, `keymap.cancel`). Prompt text: `keymap.recording`, `keymap.recordingHint`.
- **Keys**: `keymap.*` has 30 entries (`conflicts_one`/`_other` is a pair).

##### Experiments — `lib/experiments.ts` (77) and `strings/experiments.ts` (32)
- **Experiment**: `{id (kebab-case), stage: experimental|beta, title, hint, platforms?: linux|windows|macos[]}`.
- `currentPlatform(ua)` returns null on phones.
- `availableExperiments(registry, platform)`, `isExperimentEnabled(settings, id)` (unknown or unavailable ids are off; a non-array setting is off), `withExperiment(list, id, on)`.
- **Strings**: `settings.catExperimental`, `experiments.title`, `experiments.warning`, `experiments.stage.experimental`, `experiments.stage.beta`, `experiments.onlyOn` (6 keys, en + fr).

##### i18n and CSS (settings)
- **Keys**:
  - `settings.*`: 146 in i18n.ts (plural pair `shellDetected`). Plus `settings.catExperimental` (strings/experiments) and `settings.projectsFolder*` (strings/workspaceFromGit).
  - Also used: `kitsManage.*` (3), `create.cancel|browse|shellNone`, `keymap.withKeys`.
- **CSS** in `styles.css`:
  - Settings page L2398–2786: `settings__*` 16.
  - Rows and controls L2787–3213: `set-*` 22, `seg*`, `switch*`, `theme-*` 8, `font-card*`, `term-preview*`, `picker*`.
  - Discord L3214–3382: `dc*` 18.
  - Shortcuts L6298–6817: `keymap*`, `key-*` 15, `kbd*`.
  - Voice L6818+: `vm*` 10.

#### 3. Mobile, Voice and Discord cards

##### MobileSettings — `MobileSettings.tsx` (753) + `.css` (206) + `mobileOptions.ts` (19) + `lib/remote.ts` (196)
- **Structure**: `MobileView` is pure (everything from props, tested). `MobileSettings` is the controller.
- **Controller state**: `remote`, `tailscale`, `devices`, `pairing`, `justPaired` (6 s flash), `revoking`, `options`, `busy {toggle, serve, pair, options}`, `error`, `now` (1 s tick while pairing), `slots`, `portDrafts`, `portProblems`, plus a `known` device-id set.
- **Store**: `state.workspaces` (non-HQ ones become `appProjects {id, name, ports = appPortsOf(ws)}`) and `updateWorkspace(id, {mobilePorts})` (set to `undefined` when the list is empty).
- **Blocks**, in order; everything after the switch only renders while `remote.enabled`:
  1. **Switch** `mobile.switch` / `mobile.switchHint`. Disabled until the status answers or while busy. Error line `mobile.error {error}` (local error, else `remote.error`).
  2. **Tailscale**, by `tailscaleStep` (`!installed` → missing; `!running || !dnsName` → stopped; `serving` ? ready : unserved):
     - missing: `mobile.tailscaleMissing` + Download button (opens `https://tailscale.com/download`) + `mobile.recheck`
     - stopped: `mobile.tailscaleStopped` + recheck
     - unserved: `mobile.serve` / `mobile.serving` button + `mobile.serveHint`. Only that click ever runs `tailscale serve`.
     - ready: `mobile.tailscaleReady` + `https://{dnsName}`
  3. **Pair** (`mobile.pair` / `pairHint`):
     - Show-code button (`mobile.pairShow` / `pairBusy`), disabled unless `canPair` (enabled, running and Tailscale ready); otherwise `mobile.pairBlocked`.
     - While pairing: the QR from the backend's SVG string (`pairing.qrSvg`, injected raw), the code, and a countdown `mobile.pairExpires {m:ss}`. At 0 it shows `mobile.pairExpired` + `mobile.pairAgain`. `mobile.pairClose`.
     - When a refresh finds a device that was not in `known`, pairing closes and `mobile.paired {name}` flashes.
  4. **Devices**, sorted newest `created` first: name + `mobile.devicePaired {date}` · `mobile.deviceSeen {date}` (or `mobile.deviceNeverSeen`). Revoke is inline two-step (`mobile.revokeAsk {name}` / `mobile.revokeHint` / Cancel / `mobile.revoke`). Empty: `mobile.devicesEmpty`.
  5. **Apps** (per workspace): port chips with × (`mobile.appPortRemove`), a numeric input (placeholder 5173) and Add (disabled when blank).
     - `checkAppPort` rules: must match `^\d{1,6}$` (else invalid); range 1024–65535 (else range); not in `hangarPorts` = mobile server port ∪ slot local ports (default 47822–47827) (else hangar); not already listed (else duplicate).
     - Errors: `mobile.appPort{Invalid|Range|Hangar|Duplicate} {port}`. Typing clears the problem.
  6. **Slots** (only once loaded): the slots that have an app (`mobile.appSlot {workspace, port}`, `mobile.appSlotVia {https, date}`); for each `mapping === "taken"`, `mobile.appSlotTaken {https}`; the notes `mobile.appsTailscale` and `appsTailscaleRemove` (with `<code>` for `tailscale serve --https=8443 off` and `tailscale serve reset`); recheck.
  7. **Away** (only once options are loaded): `keepAwake` (disabled when unsupported → `mobile.keepAwakeUnsupported`) and `autostart`.
  8. **iPhone** info: `mobile.iphoneOpen`, `mobile.iphoneNotify`.
- **Commands**:

  | Command | Args → result | When |
  |---|---|---|
  | `remote_status` | → `{enabled, running, port, error}` | on mount |
  | `remote_options` | → `{keepAwake, keepAwakeSupported, autostart}` | on mount; re-read after each set |
  | `remote_set_enabled` | `{enabled}` → RemoteStatus | switch; off also clears pairing and revoking |
  | `remote_tailscale` | → `{installed, running, dnsName, serving}` | when enabled; recheck |
  | `remote_tailscale_serve` | → TailscaleStatus | Serve button |
  | `remote_devices` | → `[{id, name, created, lastSeen}]` | when enabled; on event; after revoke |
  | `remote_pair_start` | → `{code, url, qrSvg, expiresAt}` | pair button |
  | `remote_revoke` | `{id}` | revoke confirm |
  | `remote_app_slots` | → `[{slot, httpsPort 8443–8448, localPort 47822–47827, mapping: mapped\|free\|taken\|null, app: {workspaceId, workspace, port, lastUsed}\|null}]` | when enabled; recheck |
  | `remote_set_keep_awake` | `{enabled}` | Away switch |
  | `remote_set_autostart` | `{enabled}` | Away switch |

  - Event **`remote:changed`**: re-reads devices (listener only while enabled).
  - `plugin-opener.openUrl` opens the Tailscale download page.
- **Keys**: `mobile.*`, 63.
- **CSS**: `mob*`, 23 classes.
- **Tests**: `MobileSettings.test.ts` has 24 cases × `describe.each(en, fr)` = 48 runs. It renders `MobileView` to static markup.

##### VoiceCard and VoiceKeyCard — `VoiceCard.tsx` (313)
- **VoiceCard** (`selected = settings.voiceModel`):
  - `voice_models` on mount returns `[{id, label, approxBytes, languages, license, installed}]`. The card shows `selected`, else `models[0]`, and renders nothing when the list is empty.
  - Header: name + `settings.voiceInstalled`, or the size formatted per locale ("629 MB", "629 Mo", "1,5 Go").
  - Meta: `settings.voiceModelHint {languages, license}`.
  - Download: `voice_model_download {id}` resolves when installed. Progress comes on **event `voice:download` `{id, received, total, done}`**: a bar plus `settings.voiceDownloading {percent}`; `done` re-reads the catalogue.
  - Buttons `settings.voiceDownload {size}` / `settings.voiceDownloadAgain`, note `settings.voiceMissing`. Errors are shown raw.
- **VoiceKeyCard** (Groq key, kept in the OS keychain):
  - `voice_key_status` returns a bool. An error means the keychain is unavailable: `settings.voiceKeyUnavailable {error}`, short form `…UnavailableShort`.
  - State line: `voiceKeySaved` / `voiceKeyNone`.
  - When there is no key, or while replacing: a password input (`voiceKeyPlaceholder`), Enter or `voiceKeySave` calls `voice_key_set {key trimmed}`, and Cancel appears when replacing.
  - When saved: `voiceKeyReplace`, and `voiceKeyClear` (two-step, `voiceKeyClearConfirm`) calling `voice_key_clear`.
  - Legacy plaintext `voiceApiKey`: `voiceKeyLegacy` with `voiceKeyLegacyMove` (calls `voice_key_set`, then drops the field) and `voiceKeyLegacyDrop` (sets it to undefined).
- **Other voice settings-side pieces**:
  - `voice_unload` (the action row).
  - `lib/voice.ts voiceConfig(settings) → {engine, model (local ? voiceModel : voiceCloudModel), language|null, cleanup, cleanupModel, cleanupHint}`.
  - `useVoice` (events `voice:phase` / `voice:level` / `voice:text`; commands `voice_start` / `voice_stop` / `voice_recording`) is the HUD's, mapped elsewhere.

##### DiscordCard — `DiscordCard.tsx` (128) + `lib/discord.ts` (161)
- **Card**: the bundled `assets/logo.png`; a badge with the first letter of `smallText`; head `discord.cardHead`; "Hangar.AI"; `details`; `state`; elapsed `discord.elapsed {hh:mm:ss}` ticking every 1 s; a fake button with `buttonLabel`. Class `is-off` when disabled.
- **Status line**: while enabled, polls `discord_presence_status` every 2 s → `{enabled, connected, error}`. States off / on / waiting (`discord.statusOff|Connected|Waiting`, plus `statusWaitingHint` with the error in a tooltip).
- **`buildPresence({settings, workspace, focusedPaneId, t})`**: returns null when the setting is off.
  - details = workspace name, or `discord.working` when the name is hidden, or `discord.noWorkspace`.
  - state = `discord.idle`, or `discord.panes {n}` · (`discord.shellOnly` | agent labels | `discord.agents {n}`); shells don't count as agents.
  - badge = the focused pane's agent, else the first agent kind, only when `discordShowAgents`.
  - appId = `discordAppId` or `DEFAULT_APP_ID`; largeImage = the raw GitHub logo URL; startedAt = app launch; button `discord.button` linking to `REPO_URL`.
- **Publishing**: `useDiscordPresence` (App-level, main window only) calls `discord_presence_set {wanted | null}` when the serialized payload changes, and null on unmount.
- **Keys**: `discord.*`, 16 (plural pairs `panes`, `agents`).

#### 4. Logic libs in scope

| module | LOC | tests (LOC, cases) | computes / exports | kind | port target |
|---|---|---|---|---|---|
| `lib/board.ts` | 227 | board.test.ts (80, 8) | columns, `columnKey`, types, `isBlocked`, `isoDay`, `recheckProblem`, `blockedMoveRefusal`, `blockPatch`, `unblockPatch`, `blockersOf`, `MAX_BLOCK_REASON`; invoke wrappers (board_*, mcp_*, agent instructions) | pure + IO wrappers | `hangar-app/src/board/model.rs` (reuse the hangar-core board types; calls go direct to hangar-core) |
| `lib/boardDrag.ts` | 54 | boardDrag.test.ts (46, 7) | `DRAG_THRESHOLD = 5`, `movedBeyond`, `columnAtPoint`, `startsDrag` | pure | `board/drag.rs` |
| `lib/boardFormat.ts` | 59 | boardFormat.test.ts (56, 8) | `priorityLevel`, `priorityKey`, `shortTaskId` (first 4 chars with dashes removed), `elapsedLabel` (now / m / h:mm / d h) | pure | `board/format.rs` |
| `lib/experiments.ts` | 77 | experiments.test.ts (95, 14) | registry (empty), `currentPlatform`, `availableExperiments`, `isExperimentEnabled`, `withExperiment` | pure | `hangar-app/src/experiments.rs` (platform via `cfg!(target_os)`) |
| `lib/discord.ts` | 161 | — | `DEFAULT_APP_ID`, `REPO_URL`, Presence/Status types, `buildPresence`, `useDiscordPresence` | pure + hook (IO) | `hangar-app/src/discord.rs` (pure builder; republish-on-diff in App update) |
| `lib/voice.ts` (settings side) | 206 | — | VoiceConfig / VoiceModel / VoiceDownload types, `voiceConfig()`; `useVoice` hook (HUD's) | pure + hook | `hangar-app/src/voice.rs` (`voice_config` + types) |
| `lib/remote.ts` | 196 | remote.test.ts (189, 20) | Remote / Pair / Device / Tailscale / AppSlot types, `REMOTE_CHANGED`, the Tailscale URL, remote_* wrappers, `APP_PORT_MIN/MAX`, `DEFAULT_SLOT_PORTS`, `appPortsOf`, `hangarPorts`, `checkAppPort`, `with/withoutAppPort`, `tailscaleStep`, `remoteAddress`, `canPair`, `secondsLeft`, `formatCountdown`, `newDevices`, `sortDevices`, `formatStamp` (Intl, year only when it differs) | pure + IO wrappers | `settings/mobile_model.rs` |
| `settings/mobileOptions.ts` | 19 | — | MobileOptions + 3 remote_* wrappers | IO | merged into `mobile_model.rs` |
| `settings/schema.tsx` | 1212 | schema.experiments.test.ts (80, 4) | item / section / category types, `FONT_STACKS`, ThemePreview / ThemeGrid / FontGrid / TerminalPreview, `buildCategories` | pure builder + React | `settings/schema.rs` + `settings/previews.rs` |
| `strings/experiments.ts` | 32 | — | 6 keys en + fr | data | i18n tables |

Shared helpers to port with these: `fold()` (needs the `unicode-normalization` crate), locale number formatting `num(v, digits)`, `formatStamp`, and `formatBytes` (Intl unit).

---

### 2.4 Kits, kit tabs, preview panel, 2D viewers, 3D viewer

This section has its own numbering: §2.4/1.1–1.3 kits management, /1.4 kit tab and **kit API contract**, /1.5 preview panel shell, /1.6 file viewers, /1.7 3D viewer, /1.8 cross-window protocols, /2 logic libs, /3 recommendations (3D in Rust, webview helper, markdown, highlighting).

#### Kits UI, kit tabs, preview and 3D viewer: porting map

**Things to know first**
- **Broken import paths on this branch.** `src/lib/kitTabs.ts` and `src/lib/hangarUi.test.ts` import `../../src-tauri/kit-sdk/tokens.json`. `hangarUi.test.ts` also reads `src-tauri/kit-sdk/{kit.js,kit-ui.js,gallery.html}` through `scripts/gen-hangar-ui.mjs`. Those files now live in `crates/hangar-core/kit-sdk/`, so the TS build and tests of these modules no longer resolve. Fix the paths before using vitest as the behaviour oracle.
- **Native crates are still stubs.** `crates/hangar-kit-host/src/lib.rs`, `hangar-kit-ui`, `hangar-webview/src/main.rs` and `hangar-app/src/main.rs` are 4 to 8 lines each, so nothing exists yet to conflict with.
- **Cross-window code mostly disappears.** Everything in `modelHost.ts` (the `model:*` events) and `kits:grant-roots` exists only because Tauri windows are separate JS realms. In one iced daemon process all of it becomes direct state access.
- **Assumption about kit-tab persistence.** `localStorage["hangar.kitTabs.<wsId>"]` has no native equivalent. I propose a separate `kit-tabs.json` in the app data dir, not `state.json`, so the old app's file stays compatible.

---

#### 1. Surfaces

##### 1.1 Kit activation: `WorkspaceKitsSection`, `KitActivation` dialog, `KitActivationView`, `KitIcon`, `KitEverywhereRow`

- **Files:** `src/components/KitActivation.tsx` (892), `KitActivation.css` (318), `KitActivation.test.ts` (77 LOC, 6 cases: KitIcon SVG-mask/PNG/fallback, CSS width rules).
- **Where it is mounted:** `WorkspaceDialog.tsx` (the "Kits" section and `WorkspaceKitsDialog`) and `settings/KitsSettings.tsx` (everywhere mode).
- **Props.**
  - `WorkspaceKitsSection{value: WorkspaceKit[], onChange, workspace?: Workspace|null}`. These come from WorkspaceDialog's local state, not from App.
  - `KitActivation{kit, entry?, workspace|null, everywhere?, onAccept(granted: KitGrant[]), onClose}`.
  - `KitActivationView` is a pure props view (`t, kit, inspection, steps, entry, workspaceName, everywhere, canRun, os, running, ran, error` + 5 callbacks), so every state can be drawn in tests.
- **Global store use:** `addPane`, `closePane`, `snapshot`, `setActiveWorkspace`, `respawnPane`, `state.activeWorkspaceId`, `state.settings.kitsEverywhere`. `useAsleep()` comes from `lib/hibernateStore`.

**Workspace section (list).**
- One row per kit from `kits_list` (refreshed on the `kits:changed` event).
- Each row: a switch (`role=switch`), a 16 px `KitIcon`, the name, version or error, a state tag, and a tiny button "Details"/"Review" when the state is on/review.
  - State tag values: on, off, "to review", "review hooks", broken.
- Switch on opens the activation dialog. Switch off applies `disableKit` at once.
- "Gone" rows are kits enabled but no longer installed; their switch removes them.
- `KitEverywhereRow`: a kit enabled on all workspaces and not listed by this one shows a locked switch (checked, disabled) and an "everywhere" tag.
- After an accept, if the workspace has Claude panes, a restart notice appears:
  - "Restart N panes" calls `respawnPane(ws, pane, {})` for each pane in `panesToRestart`. The session is kept.
  - "Later" dismisses; then a `restartDone` hint shows.
- `pendingNote` shows while a workspace is being created (`workspace == null`).

**Activation dialog flow.**
1. On mount it calls `kit_inspect(id)` and `kit_setup_status(id)`. Both are fetched again when `kit.hooksPermission` or `kit.commandsPermission` change.
2. **Escape** is a capture-phase `keydown` with `stopImmediatePropagation`, because the dialog sits on top of the workspace dialog. `Backdrop escape={false}`.
3. Title is `actTitle` or `actReviewTitle` (when an entry already exists).
4. Sections, in order:
   - **Header:** workspace name, or the "everywhere" note; then icon (18 px), name, `v<version>`, description.
   - **Brings:** chip lists with counts for skills, commands, agents, MCP servers and tabs (tab titles); then the "Claude Code only" note.
   - **Permissions, per tab:** each permission in plain words (`permissionLabel`: 9 named labels, `loopback:<port>`, `command:<name>`). When an entry exists each one is tagged New or Granted. A tab holding `files.read` and `files.write` gets the `filesSecrets` notice.
   - **Setup steps:** per step a title, a Done/To-do tag, the description, and the exact command in a focusable `<pre>` labelled with the OS.
     - **Run** is disabled when there is no workspace, a step is running, or another step awaits confirmation. While its pane opens the button reads "Running…".
     - After a run: "opened in pane X", then **It worked** (calls `kit_setup_mark(id, stepId)`) or **Not yet**.
   - **Runs by itself:**
     - Plain hooks: `event · matcher` plus the command in a `<pre>`.
     - "Running" groups in a fixed order: module, mcp, monitor, lsp, skillHook, settings, grammar, bin, binary, dependency, unknown. Each group has a title and a "when" sentence. Each item shows `name · when`, its description, the argv line (quoted, with invisible characters escaped as `\u{..}`), env names, header names, and an "unreadable" note.
     - A New/Granted consent tag when the kit is a GitHub kit with a hooks grant. Notes for "hooks changed" and "folder is read live".
   - **Commands its tabs may run** (declared commands): name, tag, description, argv line, hint (fixed or with placeholders), a `{kitDir}` note, and a `<dl>` of rules (`{name}`: pattern + description). Notes for changed and live.
   - **Error notice.**
   - **Footer:** Refuse (ghost) and Accept (primary). The Accept label is `actAccept`, `actAcceptChanged` (hooks or commands changed) or `actAcceptPerms`. It is disabled until the inspection has loaded and the kit is usable. When nothing needs accepting, the footer shows only **Close**.
5. Accept emits `acceptedGrants(kit, inspection)`. The hashes come from what was shown, not from the kit as it is now.

**Setup run** (`kitSetup.runSetupCommand`).
1. `showWorkspace` selects or wakes the workspace. If it is asleep and already active it goes `setActive(null)`, waits 50 ms, then `setActive(id)`.
2. `addPane(ws, {agent: "shell", cwd: kit.dir})`.
3. Polls `pty_alive` up to 80 times, 250 ms apart.
4. Writes with `pty_write(command + "\r")`.
5. On failure it closes the pane. Errors: `stepNoPane`, `stepPaneLost` or `stepPaneLostDetached`, `stepFailed`.
6. While a step awaits confirmation the dialog "peeks" (`.kit-act-peek`): transparent, click-through backdrop, dialog bottom-right, at most 460 px wide and 45 vh tall.

**KitIcon.** The URL is `kitIconUrl` → `hangar-kit://localhost/<id>/<icon>`. An SVG is drawn as a CSS mask so it takes `currentColor`; a hidden `<img>` detects load errors. A PNG is an `<img>`. If there is no icon or it fails, the "plug" icon is used.

- **Backend calls:** `kits_list`, `kit_inspect`, `kit_setup_status`, `kit_setup_mark`, `pty_alive`, `pty_write`. Event: `kits:changed`.
- **i18n:** 74 distinct `kitsManage.*` keys and 10 `kitsTab.*` keys (labels in `kitActivation.ts`: 31 + 2).
- **CSS:** `.kit-act` with `__ws __desc __icon __ver __block __heading __group __label __count __chips __chip __perms __perm __tag(--new|--ok) __note __row __foot`; `.kit-act-peek`; `.kit-step` with `__head __title __cmd __ask __rules __rule __unreadable`; `.kit-sec` with `__list __row __main __icon __text __name __meta __restart`; `.kit-icon(--glyph)`.
- **iced notes:**
  - Use a modal from hangar-ui. It has to stack over another modal, with its own Escape handling.
  - The "peek" needs a non-blocking dock mode where the grid stays interactive.
  - `<pre>` blocks become selectable monospace text.
  - Kit icons: read the bytes through core; draw SVG with `svg::Handle` and a style colour override (`currentColor` equivalent), PNG with `image::Handle`.

##### 1.2 Settings › Kits: `KitsSettings` (fed from Rust) and `KitsView` (pure)

- **Files:** `settings/KitsSettings.tsx` (674), `.css` (117), `.test.ts` (272 LOC, 18 cases covering list, source, update, remove wording, add errors, per-workspace list, everywhere switch and review, restart notice, French).
- **Mounted from:** `settings/schema.tsx:825`. It takes no props. Store use: `state.workspaces` (HQ excluded), `state.activeWorkspaceId`, `state.settings.kitsEverywhere`, `updateSettings`.

**Interactions.**
- Restart notice for "everywhere" changes, with a dismiss button.
- The Library (§1.3) sits above the list.
- **Installed list (`KitRow`):**
  - 20 px icon; name or "Unreadable kit (id)"; `v<version>`; description.
  - Source line: "Folder: path" or "GitHub: url (ref · subdir)", plus "· linked, read live" or "· pinned to <tag>".
  - Error notice when the kit is broken.
  - Update note, one of: checking, up to date, available (current → latest), updating, done (from → to, plus "new permissions" when the update added some), folder (no update for linked folders), reloaded, error.
  - **"On all workspaces" switch:** on opens `KitActivation{everywhere, workspace: active ws}`. Off writes `updateSettings({kitsEverywhere: disableKit(...)})`, shows a flash note and sets `restartFor`.
  - Review tag and button when the everywhere grant needs review.
  - Hint: `everywhereHint`, `everywhereReview` or `kitsTab.everywhereReviewCommands`, plus `everywhereNeedsWs` when no workspace is active.
  - **Actions:** Reload (folder kit) → `kit_update`; Update (when available) → `kit_update`; otherwise Check for update → `kit_check_update`. **Remove** opens an inline confirm (Cancel / Remove, with different wording for folder and git) → `kit_remove`.
- **"Kits per workspace" block:** each workspace's name and "n enabled", with **Manage** opening `WorkspaceKitsDialog`.
- **"Add a kit" block:**
  - "Add from a folder": `@tauri-apps/plugin-dialog open({directory: true})` → `kit_add_folder(path)`.
  - GitHub form: text input (placeholder `https://github.com/owner/repo`), Enter or the button submits → `kit_add_git(url)`. Inline errors; flash "added".

- **Backend calls:** `kits_list`, `kit_add_folder`, `kit_add_git`, `kit_check_update`, `kit_update`, `kit_remove`, `kit_gallery_list`. Event: `kits:changed`.
- **i18n:** 58 `kitsManage.*` keys and 1 `kitsTab.*` key.
- **CSS:** `.kits __intro`; `.kit-row` with `__top __icon __text __ver __source __note __error __everywhere(-main|-label) __confirm __actions`; `.kit-add __line __url`. It reuses `.set-section/.set-row/.set-group/.set-input`.
- **iced notes:** the folder picker is `rfd::AsyncFileDialog::pick_folder`. Two-click removal is local view state.

##### 1.3 Kit library: `KitsGalleryView`

- **Files:** `settings/KitsGallery.tsx` (198), `.css` (52). Pure props: `{gallery, error, refreshing, installed, busy, failure, query, onQuery, onRefresh, onInstall(id), onUpdate(kit)}`.
- **Source:** a single catalog, the `hangar-ai` organization's static file, read by Rust through `kit_gallery_list` → `{origin: live|cached|unavailable, kits, fetchedAt}`.
- **Interactions:**
  - Search field. A kit matches when every word of the query appears in its id, name, description or tags.
  - Refresh button (shows "refreshing").
  - Notices: cached (with the date), unavailable, install failed.
  - States: loading, empty, no match.
  - **Card:** name, version, description, `repo · tag`.
    - **Install** → `kit_add_git("https://github.com/<repo>/tree/<tag>")`.
    - **Update to X** → `kit_update(id)`.
    - Verified tag, or Community tag plus a note.
    - Tag chips; "asks: <permissionsSummary>".
    - Status line: installed (+ pinned tag), linked (vX), or update available.
- **Status logic:** `galleryStatus` with semver `isNewer`. An update is offered only for a kit pinned to a tag of the same repo.
- **i18n:** 26 `kitsManage.gallery*` keys.
- **CSS:** `.kit-gallery __bar __filter __top __action __line __tags __asks`.

##### 1.4 Kit tab: `KitTabView` + `KitFrame`. The contract `hangar-webview` and `hangar-kit-host` must provide

- **Files:** `preview/KitTabView.tsx` (378), `lib/kitApi.ts` (833), `lib/kitTabs.ts` (477), `crates/hangar-core/kit-sdk/kit.js` (154, served at `/_hangar/kit.js`).
- **Mounted by:** `ModelPanel`. Every kit tab of the window stays mounted; hidden ones get `display:none`; tabs of sleeping workspaces are unmounted. Props: `{entry: OpenKitTab, shown}`.

**How a tab is hosted today.**
- An `<iframe sandbox="allow-scripts" allow="" referrerPolicy="no-referrer">`.
- Its `src` is `kitFileUrl` → `hangar-kit://localhost/<kitId>/<entry, each segment encoded>` (Windows: `http://hangar-kit.localhost/...`).
- `hangar-core/src/kit_scheme.rs` serves it with this CSP:
  - `sandbox allow-scripts`; `default-src 'none'`
  - `script-src 'unsafe-inline' 'wasm-unsafe-eval'`, the kit's own files, `/_hangar/kit.js`, `/_hangar/kit-ui.js`
  - `style-src` inline, own files, `/_hangar/kit.css`
  - `img-src` / `media-src`: own, `data:`, `blob:`; `font-src` own + `inter.woff2`; `connect-src` own only
  - no frames, workers, objects or forms.
- Library files: `kit.js`, `kit-ui.js` (`window.hangar.ui`), `kit.css`, `inter.woff2`.
- The page reaches Hangar only through `window.parent.postMessage`. The SDK exposes a frozen `window.hangar = {version: 1, call(method, params), on(event, fn) → off, ui}`. SDK-side timeouts are 30 s, or 130 s for `command.run`, failing with code `timeout`.

**Host checks** (`createKitHost.receive`):
- `event.source` must be the frame's `contentWindow` and `event.origin === "null"`.
- `data.hangar === 1`; `id` must be a 1–128 character string or a finite number. Anything failing these is dropped silently.
- `method` must be a string, else the answer is `invalid`.
- At most 16 calls pending, else `rate_limited`.
- Answers go to the frame with `postMessage({hangar: 1, id, result|error: {code, message}}, "*")`.
- Permissions are read again at each call: `effectivePermissions` = what the tab declares ∩ what the workspace granted. `command:*` permissions are dropped when a GitHub kit's `commands:<hash>` grant is out of date.

**Methods.** Path rules used below:
- **ws-rel**: relative to the workspace folder; non-empty, ≤1024 characters, no NUL, no leading `/` or `\`, no `..`, no `:` in any part, no part ending in a dot or a space, hidden parts refused.
- **proj-rel**: same, hidden parts allowed, `.git` refused at any depth.
- **data-rel**: same, hidden allowed.

| Method | Needs | Params (checked in JS) | Result | Backend (Tauri command → port) | Limit |
|---|---|---|---|---|---|
| `workspace.info` | `workspace.read` | — | `{id,name,folder}` | store | — |
| `panes.list` | `panes.read` | — | `[{name,agent,role,state,canSend}]` | store + `agentState` activity | — |
| `panes.send` | `panes.send` | `{pane≤200, text 1..20000}`; target must be a `claude` pane with `activity.bridged` (`not_found`/`denied`/`unavailable`) | `{queued:true,pane}` | `bridge_enqueue(paneId,text,from:"kit:<id>")` | 10/60 s |
| `files.read` | `files.read` | `{path}`: ws-rel, or proj-rel when the tab also holds `files.write` | string | `kit_file_read(root,path)`, or `kit_files_read(root,path)` in the hidden mode | — |
| `files.readBinary` | `files.read` | `{path}`, same two modes | `{base64,mime}` | `kit_files_read_binary(root,path,hidden)` | — |
| `files.list` | `files.read` | `{path?: proj-rel or "."; pattern? ≤256}` | `{files:[{path,size,modified}],truncated}` | `kit_files_list(root,path\|null,pattern\|null)` | — |
| `files.write` | `files.write` | `{path proj-rel, text ≤1 MB UTF-8, expected? ≤1 MB}` | `{}` | `kit_files_write(root,path,text,expected)` | 10/60 s, shared with `files.writeBinary` |
| `files.writeBinary` | `files.write` | `{path, base64 ≤ ceil(10 MB/3)·4 chars}` | `{}` | `kit_files_write_binary(root,path,base64)` | shared |
| `data.read` | `data.read` | `{path data-rel}` | `{text}` | `kit_data_read(kit,path)` | — |
| `data.write` | `data.write` | `{path, text ≤1 MB}` | `{}` | `kit_data_write(kit,path,text)` | 10/60 s |
| `loopback.fetch` | `loopback:<port>` (that exact port) | `{port 1024..65535, path "/…" (not "//"), ≤2048 printable ASCII, no `\` or `#`; method GET\|POST; body? string, POST only, ≤2 MB; timeoutMs 1..15000, default 5000}` | `{status,text}` | `kit_loopback_fetch({port,path,method,body,timeoutMs})` | — |
| `command.run` | `command:<name>` | `{name, args?: Record<string,string>}` (a non-string value → `bad_args`) | `{exitCode\|null,stdout,stderr,timedOut,truncated}` | `kit_command_run({kit,name,args,cwd:ws.cwd,accepted:"commands:<hash>"\|null})` | Rust: 4 at once per kit (`busy`) |
| `preview.show` | `preview.show` | `{path ws-rel}` | `{opened:true,path}` | `kit_workspace_path(root,path)`, then grant roots (`model_allow_roots(union)`; workspace windows go through `kits:grant-roots`), then `preview_inspect(real)`, then `showPreview` (models: `modelFormatOf`, ≤256 MB), `openedBy = kitName` | — |
| `notify` | `notify` | `{title 1..120, body? ≤1000}` | `null` | plugin-notification `sendNotification("<kitName> · <title>", body)` | 5/60 s |
| `tab.params` | none | — | `{params, caller:{pane}\|null}` | preview store | — |
| `tab.setTitle` | none | `{title}`: trimmed, ≤40 code points, no Cc/Cf/Zl/Zp characters | `{}` | `setKitTabTitle` (sets `titled`) | 120/60 s |
| `tab.setBadge` | none | `{state: running\|success\|failure\|neutral\|null, text? ≤12}` | `{}` | `setKitTabBadge` | 120/60 s |

- **Error codes:** `denied`, `invalid`, `unknown`, `not_found`, `rate_limited`, `unavailable`, `unreachable`, `bad_args`, `busy`, `conflict`, `unsupported`, `failed`.
  - A Rust rejection shaped `{code, message}` keeps its code. Any other error becomes `failed`. `files.read` without `files.write`, and `preview.show`, always map to `failed`.
  - Refusals for loopback and commands: if the port or name is valid but not granted, `denied` naming it. If it is invalid, the answer is `invalid` when the tab holds at least one port or command, otherwise `denied`.
- **Events** are sent as `{hangar:1, event, data}`:
  - `panes.changed` (needs `panes.read`; `data: {}`): sent when `JSON(kitPanes)` changes.
  - `tab.params` (no permission; `{params, caller}`): sent when a singleton gets new params, or an agent or hook asks for the tab again.
  - `theme` (no permission): sent on the first `load` and on every change. Payload `{scheme, locale, tokens}`: `--kit-scale`, `--kit-mono`, plus the 34 colour tokens of `tokens.json` mapped through `deriveChromeTokens` of the tab's own workspace theme.

**Lifecycle.**
- `kitTabState` is one of loading, disabled, missing, broken(error) or ready. The non-ready states are StageMessages.
- `kitTabLoads`: a tab restored at startup creates no frame until it is first shown. Then `markKitTabShown`.
- Reload: the toolbar button calls `reloadKitTab` → revision+1 → the frame's key changes → new frame and new host.
- "Left": a second `onload` closes the host and shows "left" with a Reload action.
- Focus guard: if focus jumps into the frame from an `.xterm` while the pointer is not over it, focus goes back (`frameTookKeyboard`).
- Bar: Reload tool; a lock chip "isolated" whose tooltip lists the permissions in words (`kitsTab.permission.*`).

- **i18n:** 16 `kitsTab.*` keys here; 51 in total.
- **CSS:** `.preview-file`, `.model-panel__bar`, `.preview-isolated`, `.preview-frame--page`.
- **iced and native notes:**
  - **HTML tabs** run in `hangar-webview`. Keep `kit.js` unchanged by loading the kit page inside a helper-owned shell page with the same `<iframe sandbox="allow-scripts">`. The shell relays `postMessage` to wry's IPC, then as JSON lines over stdio to a Rust `KitHost` in hangar-app.
    - This keeps the `source`/`"null"` origin check and the "left" detection (counting onload in the shell).
    - The helper must answer `hangar-kit://` itself from the kit folder with the CSP above, or ask the parent.
    - It opens without stealing focus when an agent or hook opens it (tao `with_focused(false)`). That replaces `frameTookKeyboard`.
  - **Native wasm tabs** (`hangar-kit-host`) expose the same 18 methods and 3 events as host imports through a `KitApi` trait. One Rust `KitHost` implementation serves both backends.

##### 1.5 Preview panel shell: `ModelPanel`, `PreviewTabs`, `Grip`, `DrawerFocus`

- **Files:** `ModelPanel.tsx` (987), `src/styles/model.css` (857).
- **Props, all from App** (`App.tsx:1662`): `workspaceId`, `shown`, `drawer`, `expanded`, `width`, `drawnWidth`, `maxWidth`, `scale`, `paneId` (the focused pane), `onResize(width)` → `updateSettings({modelPanelWidth})`, `onClose`, `onToggleExpand`.
  - `shown`, `drawer` and `expanded` are App transient state computed by `lib/sidePanels`.
  - A second, hidden instance (`App.tsx:1755`) keeps the 3D viewer alive when no workspace is on screen.
- **What it shows:** a header tab strip; Expand/Collapse; Hide. Then the body: the 3D view, mounted while any model is open in the window and hidden when another tab is in front; all kit tabs; `FileView` for other files; an empty state.

**Interactions.**
- **Tabs:** click activates. Keyboard: ArrowLeft/ArrowRight cycle, Home/End, Delete closes the active tab (roving tabindex). Each tab has a close ×.
- **Tab icon:** spinner while loading, alert on error, cube for a model, window for a kit, a kind glyph for a file, or the kit badge glyph (spinner, alert, check or dot).
- **Tooltip:** path plus "opened by"; for kits: kit, badge text, params, opener.
- **"By" label:** only on the active tab, or when there is a single tab. For kit tabs the badge text takes its place.
- The active tab is scrolled into view.
- **Grip:**
  - Pointer drag with capture; width is painted live and committed on release.
  - Double-click resets to `DEFAULT_SETTINGS.modelPanelWidth`.
  - ArrowLeft/ArrowRight change the width by ±24 pt; clamped by `clampModelPanel` and `maxWidth`.
  - `role=separator` with aria values.
- **Escape** while expanded collapses the panel.
- **Drawer mode:** `useDrawer` traps focus and gives it back. When the panel hides with focus inside, focus goes to `getTerminal(paneId)`.

- **i18n:** 50 `model.*` keys (48 in `i18n.ts`) and 2 `kitsTab.*` keys (`badge.*`, `tabHint`/`tabParams`).
- **CSS (`model.css`, summary):**
  - Panel modes: docked, `--drawer`, `--expanded`, with keyframes `model-panel-in` and `-grow`.
  - Grip.
  - Xcode-style tabs: `.model-tab __main __title __by __close __icon __spinner`.
  - `.model-panel__tool`; pressed state uses the accent.
  - Viewport `.model-view` with a studio radial backdrop from theme tokens and `data-lost`.
  - `.model-loading`, `.model-progress(--slim, is-indeterminate)` with keyframes `model-travel`.
  - `.model-stage__reload/__chip`, `.model-banner(--danger|--warning)`, `.model-message`, `.model-anim(__clip __scrub __time)` with `--progress` filling the track.
  - `.model-info` with container queries that drop items below 460 and 360 px.
  - `.tbtn--unseen` dot; a prefers-reduced-motion block.
  - The `.preview-*` and 10 `.hljs-*` rules live in `src/styles.css`, lines 7819–8270.

##### 1.6 File viewers: `FileView` → viewer by kind

- **Files:** `preview/FileView.tsx` (69), `parts.tsx` (204), `TextParts.tsx` (102), `TextView` (48), `CodeLines` (68), `highlight.ts` (94), `MarkdownView` (103), `markdown.ts` (68), `ImageView` (263), `MediaView` (116), `PdfView` (40), `HtmlView` (80), `Glyph` (83, 10 glyphs), `useTextHead` (62), `useFileWatch` (58).
- **Props:** `{entry: OpenFile, shown}`. A viewer is remounted on `id:revision`.

**Extension → kind.** Rust `preview_inspect` decides, by extension and magic bytes (`crates/hangar-core/src/preview.rs`). The JS `viewerOf` maps kind → viewer.

| Kind (viewer) | Extensions | Size cap |
|---|---|---|
| model (3D) | glb gltf obj stl ply fbx usdz | 256 MB |
| image (`ImageView`) | png jpg jpeg gif webp svg bmp avif | 64 MB |
| video (`MediaView`) | mp4 m4v mov webm | none; streamed in 8 MB Range chunks |
| audio (`MediaView`) | mp3 wav ogg m4a flac | none; streamed |
| pdf (`PdfView`) | pdf | 256 MB |
| markdown (`MarkdownView`) | md markdown | 10 MB |
| html (`HtmlView`) | html htm | 64 MB |
| text (`TextView`) | 40 named extensions (txt log json jsonc yaml yml toml csv xml ts tsx js jsx mjs cjs rs py go java kt c h cpp hpp cs rb php sh ps1 bat css scss sql lua verse gd swift dart vue svelte), plus any unknown extension whose head is UTF-8 without NUL | 10 MB |

- Model side-files (bin, mtl, textures) are served but not shown alone.
- Text kinds are served as `text/plain`. HTML and SVG get CSP `sandbox allow-scripts allow-forms allow-popups`.

**Text and code** (`TextView`/`CodeLines`/`useTextHead`):
- Reads at most 256 KiB through `fetch` with `Range: bytes=0-262143` (a 416 response means an empty file). Draws at most 5000 lines; `textHead` drops a partial last line and strips the BOM and CRLF.
- Note line "first X of Y" or "first 5000 lines".
- Line-number gutter sized by digit count; a wrap toggle (one preference for the session); Copy (clipboard text).
- One-line JSON is laid out by `prettyJson` (whitespace only) with a "pretty" chip; a language chip.
- Highlighting: highlight.js 11 core, loaded lazily, **31 languages**: bash c cpp csharp css diff dockerfile dos go ini java javascript json kotlin less lua makefile markdown php plaintext powershell python ruby rust scss shell sql swift typescript xml yaml. Options `ignoreIllegals`; output split per line by `splitHighlighted`.
- `languageOf` maps 10 whole names (Dockerfile, Makefile, .bashrc, .env…) and 67 extensions.

**Markdown:**
- markdown-it options `{html: false, linkify: true, typographer: false, highlight}` (hljs `<pre class="hljs">`).
- Image rule: `markdownImageSrc` resolves relative paths through `hangar-model://`; allows http(s), `//` and `data:image/(png|gif|jpe?g|webp|avif)`; otherwise renders the alt text. Images get `loading=lazy`.
- `heading_open` adds GitHub-style ids (`headingId`, with -1/-2 suffixes).
- Bar: source toggle (markdown-highlighted CodeLines + wrap), Copy.
- Links: `#anchor` scrolls to the heading; http(s)/mailto open through `openUrl`; anything else shows a `linkInert` notice.

**Image:**
- Fit zoom (never above 1, 16 px margin) that follows panel resizes while fitted.
- Zoom range 0.02–32. Wheel factor `exp(-dy·0.002)`, or ×0.01 with Ctrl (trackpad pinch), around the pointer.
- Drag to pan with pointer capture; two-finger pinch zoom and pan.
- Double-click toggles fit and 1:1 at the pointer.
- Keys: arrows nudge 40 px; `+`/`=` and `-` zoom by ×1.25; `0` fits; `1` is actual size.
- Bar: zoom out, %, zoom in, Fit (pressed when fitted), 1:1, W×H chip. Checkerboard backdrop; `image-rendering: pixelated` from zoom 3.
- An SVG without its own size is treated as 512². SVG gets a source toggle (XML highlighting via `?as=text`). Failure shows a message.

**Video and audio:**
- The webview's own player (`controls`, `playsInline`, `preload=metadata`). The audio view adds a glyph and the title.
- Added on top: a Loop toggle and a speed select (0.5, 0.75, 1, 1.25, 1.5, 2; sets `defaultPlaybackRate` too).
- Pauses when the panel is hidden. On error: a message with **Show in folder**.

**PDF:**
- Inline `<iframe>` only when `navigator.pdfViewerEnabled === true` (WebView2, macOS WebKit). Otherwise a message.
- An "Open in default app" button everywhere: `openPath`, falling back to `revealItemInDir`, then a notice.

**HTML:**
- `<iframe sandbox="allow-scripts allow-forms allow-popups" referrerPolicy=no-referrer>`.
- Bar: Reload (remounts the frame), Source toggle (XML CodeLines + wrap + copy), "isolated" chip.
- Note `inlineOnly`: the page's own sibling `.js`/`.css` are refused (served as text with nosniff).

**FileView and shared parts:**
- `useFileWatch` polls `preview_inspect` every 1.5 s while shown and the document is visible. A newer mtime that stays the same for 300 ms bumps the revision through `reloadModel`. A disappearing file shows a `model.missing` note and keeps the last version.
- Info strip: extension, kind, size (bytes/KB/MB), MIME type.
- `useNotice`: 2.2 s, or 6 s for errors.

- **Backend calls:** `preview_inspect`, and `hangar-model://` fetches (Range; `?as=text`; `?rev=`). Plugins: opener (`openPath`, `revealItemInDir`, `openUrl`), clipboard.
- **i18n:** about 50 `preview.*` keys plus `model.missing` and `model.megabytes`.

##### 1.7 3D viewer: `ModelView` + `lib/model3d/*` (three.js 0.186)

**Toolbar:**
- Recenter.
- Toggles: Wireframe, Grid (default on), Axes, Normals. Normals are disabled above 600k vertices, with a "too many" tooltip.
- Capture menu (`ToolbarMenu`):
  - Copy image: `viewer.capture(2, background)`, then the clipboard as `image/png`.
  - Save image: `plugin-dialog save` proposing `<stem>-YYYYMMDD-HHMMSS.png`, then `model_save_png(path, base64)`.
- Notice.

**Stage:**
- Focusable `role=application` canvas host.
- Overlays:
  - Reload in progress: slim progress bar and a "reloading" chip.
  - Banner "reload failed" with Retry; banner "missing file".
  - StageMessage for: unavailable (WebGL failed); context lost with Restart (generation+1); empty; error with Retry.
  - Loading card with phases starting, download (MB loaded / total, determinate), parse and prepare.
- **Animation bar:** play/pause; a clip `<select>` when there is more than one clip (otherwise the name); a scrub slider from 0 to duration in 0.01 steps; time label "x.x / y.y".
- **Info strip:** format; triangles; size W×H×D, with "m" for glTF and a "unitless" tooltip otherwise; materials; vertices. Counts become compact (e.g. 1.2 M) at 100k and above.

**Mouse and keyboard:**
- OrbitControls: left-drag orbits, right-drag (or shift/ctrl drag) pans in screen space, the wheel dollies toward the cursor. Damping 0.12; distance from r·0.02 to r·40.
- Keys: arrows pan; Shift, Ctrl or Cmd with arrows orbit (24° steps); `+`/`=` and `-`/`_` dolly by ×0.8; Home recenters.

**Engine features:**
- Renderer: `WebGLRenderer{antialias, alpha, high-performance}`, sRGB output, `NeutralToneMapping` with exposure 1, no shadows.
- Lighting: PMREM of `RoomEnvironment` (σ 0.04), `environmentIntensity` 0.8, plus one `DirectionalLight` (1.6) placed at centre + r·(1, 1.6, 1.3).
- Camera: `PerspectiveCamera` FOV 35. Near/far come from `clipPlanes`: `near = max(r·1e-3, d·2e-3, (d−r)·0.05)`, `far = d + 60r`.
- Framing: `frameView` uses a hull of at most 6000 sampled vertices (skinned meshes as posed), balanced in two rounds, margin 1.12. Default view: azimuth 45°, elevation 30°.
- Floor grid: a `ShaderMaterial` plane. Lines anti-aliased with `fwidth`; spacing by powers of ten (`gridSpacing`); fades out from the model; polygonOffset; colours from the theme.
- Axes: `LineSegments` coloured X `--danger`, Y `--success`, Z `--accent`; `depthTest` off.
- Wireframe overlay: cloned meshes sharing geometry, skeleton and morph targets, with a translucent `MeshBasicMaterial` (opacity 0.32 dark, 0.42 light). Solids get polygonOffset meanwhile.
- `VertexNormalsHelper`: size r·0.02, accent colour; follows a playing animation live up to 150k vertices.
- Animation: `AnimationMixer` with `LoopRepeat`. The first clip autoplays. A reload keeps the clip and time.
- Memory: an LRU keeps 3 models loaded beside the active one, across every workspace of the window.
- A reload keeps the user's camera unless the model's radius changed by more than ×4 or less than ×0.25.
- While the camera is untouched (auto-framed), resizes re-frame the model.
- Recenter glides for 420 ms with a cubic ease-out, or jumps when reduced motion is requested.
- Render on demand only (dirty flag); no frames at all while the size is zero.
- `compileAsync`, then textures uploaded within an 8 ms-per-frame budget.
- WebGL context loss and restore are handled, rebuilding the PMREM.

**Loaders:**
- `GLTFLoader` with Draco (bundled wasm), KTX2 (bundled basis transcoder) and Meshopt for glb/gltf.
- `FBXLoader`; `USDLoader` for usdz.
- OBJ (`OBJLoader` + `MTLLoader` for `mtllib`), STL and PLY are parsed on an **inline Web Worker**. Typed arrays are transferred with no copy (`transfer.ts`); bounds and normals are computed on the worker.
- STL is treated as Z-up and rotated −90° about X. A PLY without faces becomes a point cloud (`PointsMaterial`, 2 px).
- Default "clay" material: `#b9bec6`, roughness 0.55.
- `standardizeMaterials` turns Phong/Lambert into `MeshStandardMaterial` with `roughness = clamp((2/(s+2))^0.25, 0.04, 1)` and corrects colour maps to sRGB.
- `TrackedManager` waits up to 15 s for textures before reporting the model loaded.
- `fetchBytes` streams the download with progress. `httpError` maps 404/403 bodies to readable messages.

**Snapshots** (for agents, through `registerSnapshotter`):
- Any loaded model, at the requested width and height, from a canonical angle: front, back, left, right, top, bottom or iso.
- The grid is hidden from below; axes, helpers and wireframe are hidden.
- Background filled with the theme's `--bg`.
- `readPixels`, flipped, encoded to PNG base64.
- The user's camera, tab and toggles are left untouched. `capture` uses the current camera at 2× the panel size, at most 4096.

**Plumbing** (`useModelViewer.ts`):
- Lazy `import("./engine")`.
- `useModelLoads`: one load per model; a newer revision aborts the old one; progress repaints at most every 100 ms; every outcome goes to `settleModel`.
- `useModelWatch`: `model_stat` every 1.5 s with a 300 ms settle.
- `useViewerTheme`: a `MutationObserver` on `<html style>` feeds `readViewerTheme`.
- **Backend calls:** `model_stat`, `model_save_png`, `hangar-model://` fetches.

##### 1.8 Cross-window protocols (`modelHost.ts`, `kitTabs.ts`)

| Event | Payload | Direction | Timeout | Native |
|---|---|---|---|---|
| `model:ask` | `{reqId:"model:<t36>:<n>", workspaceId, method: probe\|list\|show\|reveal\|reload\|whenSettled\|snapshot\|close, args[]}` | main → workspace window | probe 1500 ms, call 5000 ms, whenSettled ms+5000, snapshot 30 000 ms | gone: direct call into `PreviewStore` |
| `model:answer` | `{reqId, ok, value?, error?}` | workspace window → main | — | gone |
| `model:claim` | `{workspaceId}` | workspace window, once listening → main | — | gone: scopes are keyed by workspace, the window owning it reads them |
| `model:handover` | `{to:"main"\|wsId, workspaceId, scope: ModelScope}` | both directions (`takeModelScope`/`adoptModelScope`) | — | gone |
| `kits:grant-roots` / `-done` | `{reqId, workspaceId}` / `{reqId, ok, error?}` | workspace window ↔ main | 5000 ms | gone: call `grant_model_roots` directly |
| `kits:changed` | none | core → all windows | — | core event in the `Host::subscribe` bus |

- `ModelHost` API: `list`, `show`, `reveal`, `reload`, `whenSettled`, `snapshot` (waits up to 4 s for a snapshotter to register), `close`. It is used by the MCP control handlers (`lib/control.ts`, out of scope): `preview_show`, model snapshot, `kit_tab_open`/`kit_tab_list`.
- `createRootGrants` keeps the union of roots per live workspace and sends it with `model_allow_roots`. That logic stays: hangar-core still holds one list.

---

#### 2. Logic libraries in scope

| Module | LOC | Test file (LOC, cases) | What it computes / exports | Kind | Port target |
|---|---|---|---|---|---|
| `lib/kits.ts` | 619 | kits.test (605, 45) | Types (KitInfo, Manifest, Inspection, Running, Setup…); permission set and parsers (`kitLoopbackPort`, `kitCommandName`, `kitPermissionsIn`, `kitReadsHiddenFiles`); `workspaceKits`/`everywhereKits`/`effectiveKits` (defensive readers); `kitHooksGrant`/`kitCommandsGrant`; `paneKits`/`loadPaneKits`; `kitIssueNotes`; `KIT_METHODS`/`KIT_EVENTS`, limits, `kitMayCall`; 9 invoke wrappers + `onKitsChanged` | pure, plus thin IO wrappers | `hangar-app/src/model/kits.rs`; types from `hangar-core::kits` directly |
| `lib/kitActivation.ts` | 323 | kitActivation.test (992, ~82, one `it.each`) | `PERMISSION_LABEL`, `permissionLabel`, `requestedPermissions`, `permissionDiff`, `hooksToAccept`/`commandsToAccept`/`onlyCommandsToAccept`, `kitArgvLine`, `kitRuleText`, `RUNNING_LABEL`, `kitRunningGroups`, `kitPlainHooks`, `kitRunningLine`, `kitState`, `kitRequested`, `acceptedGrants`, `enableKit`/`disableKit`, `kitPanes`/`panesToRestart`, `sameKits`, `sourceLine`, `kitName`, `updateOutcome` | pure | `model/kit_activation.rs` |
| `lib/kitSetup.ts` | 129 | kitSetup.test (122, 11) | `pendingSteps`, `setupSummary`, `setupKeystrokes`, `currentOs`, `runSetupCommand` (injected deps), `showWorkspace` | pure + async with injected deps | `model/kit_setup.rs` (async fn over a deps trait) |
| `lib/kitGallery.ts` | 99 | kitGallery.test (373, 35; also covers KitsGalleryView markup and kit-ui `confirm`) | `GalleryKit`/`Gallery`, `kitGalleryList`, `galleryInstallUrl`, `isNewer` (semver + pre-release), `galleryStatus`, `filterGallery` | pure + 1 invoke | `model/kit_gallery.rs` (or the `semver` crate) |
| `lib/kitIcon.ts` | 19 | kitIcon.test (37, 4) | `kitIconUrl` | pure | `model/kit_icon.rs`: becomes a path into the kit dir (no URL) |
| `lib/kitTabs.ts` | 477 | kitTabs.test (440, 37) | `kitFileUrl`; `effectivePermissions`; `workspaceKitTabs`; `kitTabState`; `kitTabLoads`; `readKitTabParams` (regex-anchored params); `kitTabToOpen`/`kitTabListing` (for MCP); `frameTookKeyboard`; `kitThemeEvent`/`kitThemeSender`/`kitUiScale`; `useInstalledKits` (singleton store + `kits:changed`); `askRootGrant`/`serveRootGrants` | pure + React hook + events | `model/kit_tabs.rs`; the installed-kits store becomes an App slice; grants become direct calls |
| `lib/kitApi.ts` | 833 | kitApi.test (1543, 104) | `createKitHost` (dispatch, path rules, params checks, 6 rate limiters, max 16 pending, error mapping, events), `kitPanes`, `rateLimiter`, `workspaceRelativePath`/`dataRelativePath`/`projectRelativePath`, `kitMayCallEvent`, constants | pure; IO through a `KitHostDeps` trait | `model/kit_api.rs`: a `KitHost` shared by the webview bridge and `hangar-kit-host`'s `KitApi` |
| `lib/hangarUi.test.ts` | 642 | itself (44, one `it.each`) | Tests the HTML kit library, not app code: the generated part of `kit.css` matches themes, fonts and measures (`scripts/gen-hangar-ui.mjs`); tokens exist in both themes and meet `TEXT_CONTRAST`/`FILL_CONTRAST`; `docs/kit-ui.md` covers every class and component; `gallery.html`; `kit.js` theme application; `kit-ui.js` behaviours (i18n and plurals, duration/time/size formats, `confirm` with AbortSignal, list heights, log error detection) | test only | keep in node until wave 5 (paths need fixing); add a Rust test in `hangar-theme` that the theme event covers every `tokens.json` colour |
| `lib/preview.ts` | 474 | preview.test (272, 26) | `PREVIEW_KINDS`, `checkPreviewFile`, `previewUrl`, `viewerOf`, `TEXT_HEAD_*`, `languageOf`, `textHead`, `textLines`, `rangeTotal`, `prettyJson`, `escapeHtml`, `splitHighlighted`, `markdownImageSrc`, `markdownLink`, `headingId`, `fitZoom`/`zoomAround`, `pdfInline`, `openExternally`, `showInFolder`, `mimeType`, `hasSource` | pure + opener IO | `model/preview.rs`; core's `preview::inspect` is called directly |
| `lib/modelFiles.ts` | 57 | (tested in modelStore.test, "modelFiles") | `MODEL_FORMATS`, `MAX_MODEL_BYTES`, `modelFormatOf`, `modelAllowRoots`, `modelStat`, `modelUrl` | pure + IO | `model/preview.rs` (no URLs natively: paths) |
| `lib/modelCapture.ts` | 38 | none | `copyPng`, `captureName`, `savePng` (save dialog + `model_save_png`) | IO | `view/preview/model3d.rs`: `rfd` + `std::fs::write` + `arboard` image |
| `lib/modelHost.ts` | 330 | modelHost.test (219, 10) | `localModelHost`, `remoteModelHost`, `serveModelHost`, `serveModelHandovers`, `handBackModels`, `createRootGrants`, `grantModelRoots` | IO / events | mostly dropped; `createRootGrants` and the `ModelHost` API become `model/preview_store.rs` methods; keep about 3 tests (workspace isolation, root union) |
| `lib/modelStore.ts` | 859 | modelStore.test (916, 64) | Per-workspace `ModelScope{models, activeId, panelOpen, expanded, unseen}`; `showPreview`, `openKitTab`/`findKitTab`/`kitTabPath`/`kitParamsKey`, `reloadKitTab`, `markKitTabShown`, `kitTabParams`/`kitTabCaller`/`watchKitTabParams`, `setKitTabTitle`/`Badge`, saved kit tabs (`restoreKitTabs`, `forgetSavedKitTabs`, localStorage), `closeModels`, `closeWorkspaceModels`, `closeAllModels`, `pruneModels`, `activateModel`, `settleModel`, `reloadModel`, `setModelPanel`, `markModelsSeen`, `take`/`adoptModelScope`, `whenSettled`, `noteModelView`/`modelView`, snapshotter registry, hooks | pure state + subscriptions | `model/preview_store.rs` (an App slice); kit-tab persistence in `kit-tabs.json` (assumption) |
| `model3d/engine.ts` | 1151 | none | the `ModelViewer` class (§1.7) | GPU | `hangar-model3d::{renderer, scene, orbit, capture}` |
| `model3d/loaders.ts` | 413 | none | `ModelLoaders`, `fetchBytes`, `TrackedManager`, `disposeObject`, `LoadAborted` | IO + parse | `hangar-model3d::load::{gltf,obj,stl,ply}` on a blocking task |
| `model3d/framing.ts` | 253 | framing.test (217, 16) | `directionFrom`, `viewDirection`, `fitDistance`, `frameView`, `clipPlanes`, `gridSpacing`, `centerOf`/`radiusOf`/`normalize` | pure math | `hangar-model3d::framing` (glam), tests ported as they are |
| `model3d/stats.ts` | 88 | stats.test (86, 9) | `countGeometry` (instancing, drawRange, shared materials), `clipNames`, `modelInfo` (bounds rounded to 6 significant digits; precise below 2M vertices) | pure on the scene | `hangar-model3d::stats` |
| `model3d/formats.ts` | 64 | formats.test (50, 7) | `FORMAT_PLANS{parser, unit, zUp}`, `baseUrlOf`, `httpError` | pure | `hangar-model3d::formats` |
| `model3d/materials.ts` | 129 | none | `roughnessFromShininess`, `toStandard`, `standardizeMaterials`, `texturesOf` | pure (three) | `hangar-model3d::material` (MTL Phong to PBR) |
| `model3d/grid.ts` | 134 | none | `FloorGrid` shader (GLSL) | GPU | `hangar-model3d/src/shaders/grid.wgsl` |
| `model3d/transfer.ts` | 98 | none | pack/unpack geometry for the worker | pure | dropped (same process); keep precomputed bounds |
| `model3d/parse.worker.ts` | 105 | none | OBJ/STL/PLY parsing off-thread | worker | `iced::Task::perform` + `spawn_blocking` |
| `model3d/viewerTheme.ts` | 40 | none | `readViewerTheme` (bg, grid minor/major mixes, axes, wire, normals) | pure on CSS | `hangar-model3d::ViewerTheme::from(hangar_theme::Tokens)` |
| `model3d/useModelViewer.ts` | 321 | none | `useViewer`, `useViewerTheme`, `useModelLoads`, `useSnapshotter`, `useModelWatch` | React hooks | `view/preview/model3d.rs` state, update and subscriptions |
| `strings/kitsManage.ts` | 475 | — | 185 keys (en + fr) | data | `hangar-i18n` (W1.i18n) |
| `strings/kitsTab.ts` | 146 | — | 51 keys (en + fr) | data | `hangar-i18n` |

---

#### 3. Recommendations

**3D viewer: a new crate `crates/hangar-model3d`**, so it stays disjoint, builds faster, and keeps three.js's role.

- **Widget:** iced 0.14's `iced::widget::shader` (a `Program` + `Primitive`) with its own wgpu pipelines.
  - Pipelines: PBR metal-rough, IBL from a small pre-baked prefiltered cubemap of RoomEnvironment (generated once, then `include_bytes!`), Khronos Neutral tonemap in WGSL, 4× MSAA, the grid ported GLSL → WGSL, lines for axes and normals, points, and a wireframe overlay through a barycentric shader (avoids `POLYGON_MODE_LINE`, which some backends lack).
  - On the tiny-skia fallback, show the "unavailable" StageMessage.
- **Loaders:**

  | Format | Crate | Notes |
  |---|---|---|
  | glTF / GLB | `gltf` 1.x (`KHR_materials_*`, `KHR_texture_transform`, `extras`) | |
  | Meshopt | `meshopt` (decoder) | |
  | KTX2 / Basis | `ktx2` + `basis-universal` | optional, phase 2 |
  | Draco | none mature in Rust | `draco` sys bindings, or phase 2 |
  | OBJ + MTL | `tobj` 4 | |
  | STL | `stl_io` | Z-up rotation |
  | PLY | `ply-rs` | point clouds |
  | Textures | `image` (png/jpeg/webp/tga/bmp/hdr) | |
  | FBX | `fbxcel-dom` | phase 2 |
  | USDZ | no viable crate | phase 2 or never: show "open externally" |

  - Animation: sample glTF channels in Rust; skinning (joint matrices in a storage buffer) and morph targets in the vertex shader.
  - Math: `glam`.
- **Port:** orbit controls as an `orbit.rs` state machine (damping, zoom-to-cursor, keyboard), plus `framing.rs` and `stats.rs` with their 25 tests.
- **Snapshots** need to work while the panel is hidden, for MCP. Use a **headless offscreen renderer** in `hangar-model3d` (its own `wgpu::Device`, uploads on demand from a CPU scene in `Arc`). Read back with `map_async`, flip, encode with `image` to PNG. **Capture** reuses the same path with the live camera.
- **Clipboard image:** `arboard` (iced's clipboard is text-only).

**PDF, HTML, video: `hangar-webview`.**
- **PDF:** a helper window opening the file through its own scheme, using the built-in PDF viewer of WebView2 or WKWebView.
  - On Linux (WebKitGTK has no PDF viewer) show "Open in default app" through `open::that`, exactly today's fallback.
  - Inline native rendering later: `pdfium-render` (needs the pdfium shared library) or `hayro`.
- **HTML preview:**
  - Serve the page from the helper with the same CSP (`sandbox allow-scripts allow-forms allow-popups`) and nosniff text for sibling files.
  - The source view, reload button and "isolated" chip stay native in the panel. The panel tab becomes "opened in window" plus Focus/Reload.
  - To serve files without base64 over stdio, the helper should read disk directly under roots granted by the parent (an `allow_roots` message) and reuse `hangar-core::preview::answer`/`model::answer` (Range, 8 MB chunks). That means a dependency on hangar-core behind the `engine` feature, or extracting the serving code into a small crate.
- **HTML kit tabs:** the shell-page-plus-sandboxed-iframe relay described in §1.4. The protocol messages are `open_kit_tab{url, policy, theme}`, `kit_message{id, json}` in both directions, `kit_event`, `reload`, `left`, `close`.
- **Video:** iced has no video widget. Use a helper window with `<video controls loop playbackRate>`, served by the same Range server. Audio can also be native with `rodio` + `symphonia` (play/pause/seek/loop/speed); that is optional.
- **Markdown:** iced `markdown` (pulldown-cmark).
  - Keep raw HTML as text, matching `html: false`.
  - linkify needs a small autolink pass, since pulldown-cmark has none.
  - Heading ids and anchor scrolling need a custom `Viewer`.
  - Images resolve beside the file (`image::Handle::from_path`) with the same allow rules as `markdownImageSrc`.
- **Highlighting:** iced's `highlighter` uses syntect's default set, which lacks TypeScript, TOML, Dockerfile, PowerShell, Kotlin and others. Implement `iced::advanced::text::Highlighter` over **`two-face`** (bat's syntaxes, about 150 LOC) and map `languageOf` names to two-face syntax names.
- **Opener:** `open` for open-path and open-url; the `opener` crate (`reveal` feature) for "show in folder".

---

---

## 3. Logic modules (port to Rust with their tests)

**Rules for the port:**
- Every row with a test file ports its tests to a `#[cfg(test)] mod tests` next to the Rust module, keeping the inputs and expected values.
- Timer-driven modules take an injected clock (`now_ms`) and are written as sync state machines, so the vitest fake-timer tests stay deterministic.

**Per-area tables** give each module's exports in detail: §2.1 "Logic libs table", §2.2 "Logic libraries in scope", §2.3 "Logic libs in scope", §2.4 "Logic libraries in scope", §4/4, §5/3. This master table shows ownership.

**Kind codes:** P = pure, IO = backend calls, H = React hook, S = module store, T = timers/engine.

| Module | LOC | Test (LOC / cases) | Computes | Kind | Rust target | Pkg |
|---|---|---|---|---|---|---|
| `store.tsx` | 722 | store.test (16 / 2) | the persisted model and its 22 actions (§1.3) | S | `app/state/store.rs` | P0 |
| `types.ts` | 536 | – | model types, AGENTS, DEFAULT_SETTINGS | P | `app/state/types.rs` | P0 |
| `lib/layout.ts` | 415 | layout.test (294 / 36) | SplitNode algebra, presets, `computeLayout`, drop zones, `neighborOf`, `normalizeTree`, `readingOrder`, MAX_PANES 16, MIN_PANE_PX 150 | P | `app/state/layout.rs` | P0 |
| `lib/paneNames.ts` | 151 | paneNames.test (60 / 5) | 48 pane names, pick and unique names, `sessionName` | P | `app/state/pane_names.rs` | P0 |
| `lib/folders.ts` | 20 | folders.test (42 / 5) | `sidebarOrder` | P | `app/state/folders.rs` | P0 |
| `lib/orchestration.ts` | 92 | (indirect) | defaults, `orchestrationOf`, `orchestratorOf`/`workersOf`/`managerOf`/`hqOf`, labels `needs-you`/`ready-to-land` | P | `app/state/roles.rs` | P0 |
| `lib/sidePanels.ts` | 145 | sidePanels.test (209 / 21) | docked vs drawer placement and widths | P | `app/shell/side_panels.rs` | P0 |
| `lib/useDrawer.ts` | 126 | – | drawer focus and close policy | H | `app/shell/drawer.rs` | P0 |
| `lib/regions.ts` / `focusable.ts` | 81 / 16 | regions.test (39 / 5) | F6 order and focus entry | P | `app/shell/regions.rs` | P0 |
| `lib/windows.ts` | 242 | windows.test (50 / 6) | labels, handover, launch ownership, terminal ask/answer, `readLines` | IO | `app/shell/windows.rs` (most of it disappears); `read_lines` → hangar-term | P0 / P3 |
| `lib/ipc.ts` | 175 | – | the IPC wrappers (pty, bridge, sessions, state, voice, discord…) | IO | direct `hangar-core` calls via `app/services/core.rs` | P0 |
| `lib/notify.ts` | 66 | – | desktop notification (`notify_send`) | IO | `app/services/notify.rs` | P0 |
| `themes.ts` | 1,262 | themes.test (196 / 14) | 33 themes; `deriveChromeTokens` (contrast-held tokens) | P | `hangar-theme/{themes,tokens}.rs` | P1 |
| `lib/contrast.ts` | 152 | contrast.test (119 / 16) | sRGB/OKLab mix, luminance, contrast, `ensureContrast` | P | `hangar-theme/contrast.rs` | P1 |
| `i18n.ts` + `strings/*` + `accounts/strings.ts` | 3,182 + 947 + 194 | i18n.test (57 / 8), accounts strings.test (77 / 11) | dictionaries, plural, placeholders, number format | P | `hangar-i18n` | P1 |
| `lib/typography.ts` | 57 | typography.test (61 / 8) | French narrow no-break spaces, unit binding | P | `hangar-i18n/typography.rs` | P1 |
| `lib/inspectorFormat.ts` (fmt parts) | 154 | inspectorFormat.test (110 / 10) | time, cost, percent and prose time formats | P | `hangar-i18n/fmt` (generic) + `app/inspector/format.rs` | P1 / P9 |
| `components/PaneStatus.ts` | 72 | PaneStatus.test (69 / 7) | glyph state, gauge level 70/90, short ids | P | `app/panes/status.rs` | P5 |
| `lib/ptyBus.ts` | 47 | – | fan-out of `pty:output`/`exit` | IO | `app/panes/pty_link.rs` | P5 |
| `lib/terminalRegistry.ts` | 58 | – | paneId → terminal, finders, diff openers | S | `app/panes/mod.rs` `TerminalHub` | P5 |
| `lib/agentState.ts` | 239 | agentState.test (14 / 2) | per-pane activity, plan limits, counts, rosters | S | `app/panes_logic/agent_state.rs` | P4 |
| `lib/activity.ts` | 96 | activity.test (156 / 12) | output-burst watcher (2,500 / 250 / 1,500 ms) | T | `app/panes_logic/activity.rs` | P4 |
| `lib/paneInsight.ts` | 235 | paneInsight.test (134 / 11) | per-pane insight and events (40), edited files (50) | S | `app/panes_logic/pane_insight.rs` | P4 |
| `lib/bridge.ts` | 129 | bridge.test (33 / 4) | `BridgeEvent` serde union, queue carry | IO/P | `app/panes_logic/bridge_events.rs` | P4 |
| `lib/hibernate.ts` | 322 | hibernate.test (494 / 43) | sleep verdicts, blockers, refusals | P | `app/panes_logic/hibernate.rs` | P4 |
| `lib/hibernateStore.ts` | 155 | – | touched, rest, resume-once, asleep | S | `app/panes_logic/hibernation.rs` | P4 |
| `lib/useHibernation.ts` | 343 | useHibernation.test (261 / 10) | 30 s sweep, forced sleep | H/T | `app/panes_logic/hibernation.rs` (sweep fn + Subscription) | P4 |
| `lib/agents.ts` | 258 | agents.test (106 / 9) | agent commands and flags, resume, `watchForSession`, trust prompt | P/IO | `app/panes_logic/agents.rs`, `session_watch.rs` | P4 |
| `lib/agentPane.ts` | 91 | agentPane.test (78 / 6) | `findAgentPane(assignee)` | P | `app/panes_logic/agent_pane.rs` | P4 |
| `lib/context.ts` | 95 | – | context gauge config and tokens format | P | `app/panes_logic/context_gauge.rs` | P4 |
| `lib/shells.ts` | 21 | – | `resolveShell`, `shellLabel` | P | `app/panes_logic/shells.rs` | P4 |
| `lib/paneBranch.ts` + `strings/paneBranch.ts` | 147 + 39 | paneBranch.test (119 / 9) | branch cache per folder (TTL 30 s) | IO/S | `app/panes_logic/pane_branch.rs` | P4 |
| `lib/fileDrop.ts` | 176 | fileDrop.test (98 / 12) | path translate and quote per shell, `dropText` | P/IO | `app/panes_logic/file_drop.rs` | P4 |
| `lib/webgl.ts` | 62 | webgl.test (57 / 6) | xterm WebGL policy | IO | **dropped** | – |
| `components/NavigatorModel.ts` | 186 | NavigatorModel.test (215 / 22) | nav tree, filter, namesake hints | P | `app/navigator/model.rs` | P6 |
| `lib/needsYou.ts` | 244 | needsYou.test (252 / 17) | Needs-you items, notice delta | P/H | `app/needs_you/model.rs` | P6 |
| `lib/voice.ts` | 206 | – | `voiceConfig`, push-to-talk | H/IO | `app/voice/mod.rs` | P6 |
| `lib/keys.ts` | 335 | keys.test (218 / 28) | the chord algebra (§1.10) | P | `app/keymap/chord.rs` | P7 |
| `lib/shortcuts.ts` | 786 | shortcuts.test (169 / 20) | 88 commands, defaults, keymap, conflicts | P | `app/keymap/keymap.rs` (+ `commands/id.rs` from P0) | P7 |
| `lib/useShortcuts.ts` | 216 | – | dispatcher, sequences, suspension | H | `app/keymap/dispatch.rs` | P7 |
| `lib/menuNav.ts` | 28 | menuNav.test (48 / 8) | arrow / Home / End list stepping | P | `hangar-ui/list_nav.rs` | P2 |
| `lib/paletteMatch.ts` | 259 | paletteMatch.test (183 / 21) | fold, score, highlight, filter | P | `app/palette/matching.rs` | P7 |
| `lib/savedCommands.ts` | 44 | – | merge and run saved commands | P/IO | `app/saved_commands/mod.rs` | P7 |
| `lib/workspaceFromGit.ts` + `strings/workspaceFromGit.ts` | 746 + 112 | workspaceFromGit.test (923 / 65) | clone-to-workspace flow, consent queue, form status | P/IO | `app/git_flow/{mod,consent}.rs`, `app/workspace_dialog/git_form.rs` | P8 |
| `lib/gitClone.ts` | 152 | gitClone.test (113 / 8) | clone wrappers, progress maths | P/IO | `app/git_flow/mod.rs` | P8 |
| `lib/projects.ts` | 467 | projects.test (526 / 23) | project summaries for the manager and the phone | P/IO | `app/orchestration/projects.rs` | P19 |
| `lib/accounts.ts` | 174 | accounts.test (150 / 15) | account resolution, launched registry | P/IO/S | `app/accounts/model.rs` | P8 |
| `lib/accountSwitch.ts` | 160 | accountSwitch.test (134 / 12) | restart sets, usage merge | P | `app/accounts/switch.rs` | P8 |
| `accounts/accountMenuItems.ts` | 54 | accountMenuItems.test (36 / 3) | radio items | P | `app/accounts/menu_items.rs` | P8 |
| `accounts/usageStore.ts`, `useAccounts`, `useAccountSwitching` | 63 / 42 / 89 | – | usage cache, refresh, respawn on switch | H/S | `app/accounts/{usage,mod}.rs` | P8 |
| `lib/openEditor.ts` | 44 | openEditor.test (60 / 6) | VS Code open plus notice | IO | `app/navigator/menu.rs` (calls `hangar_core::open_editor`) | P6 |
| `lib/inspectorFiles.ts` | 179 | inspectorFiles.test (144 / 14) | file rows, diff totals and segments | P | `app/inspector/files_model.rs` | P9 |
| `lib/diff.ts` | 74 | diff.test (57 / 5) | unified diff → rows | P | `app/git_ui/diff_parse.rs` | P9 |
| `lib/git.ts` | 291 | – | 17 git wrappers and types | IO | `hangar-core::git` direct | P9 |
| `lib/worktrees.ts` + `strings/worktrees.ts` | 46 + 143 | – | folder and branch naming, copy patterns | P | `hangar-core::worktree_naming` (P9 writes it; P19 uses it) | P9 |
| `lib/memory.ts` | 58 | – | memory CRUD wrappers | IO | `hangar-core::memory` | P10 |
| `components/memoryGuards.ts` | 45 | memoryGuards.test (58 / 7) | cap 1,000, warn at 900, title clash, sort | P | `app/memory/guards.rs` | P10 |
| `lib/memoryFormat.ts` | 57 | memoryFormat.test (63 / 11) | compact path, signer, short day | P | `app/memory/format.rs` | P10 |
| `lib/mcpScope.ts` | 292 | mcpScope.test (229 / 20) | MCP scope presets, toggles, restart needs | P/IO | `app/mcp/scope.rs` | P10 |
| `lib/orchestrationForm.ts` | 133 | orchestrationForm.test (144 / 13) | orchestration draft validation | P | `app/orchestration_ui/form.rs` | P10 |
| `lib/orchestrationActions.ts` | 289 | orchestrationActions.test (254 / 21) | pin/unpin, role restart, open manager (HQ) | P/IO | `app/orchestration_ui/actions.rs` | P10 |
| `lib/stats.ts` | 156 | – | usage stats cache (60 s) | IO/S | `app/stats/mod.rs` | P11 |
| `lib/statsModel.ts` | 576 | statsModel.test (264 / 16) | periods, summaries, rankings, heat levels | P | `app/stats/model.rs` (P20 reuses it) | P11 |
| `lib/board.ts` | 227 | board.test (80 / 8) | board types, block rules, wrappers | P/IO | `app/board/model.rs` | P12 |
| `lib/boardDrag.ts` | 54 | boardDrag.test (46 / 7) | drag threshold, column hit | P | `app/board/drag.rs` | P12 |
| `lib/boardFormat.ts` | 59 | boardFormat.test (56 / 8) | priority, short id, elapsed | P | `app/board/format.rs` | P12 |
| `lib/paneTask.ts` | 131 | paneTask.test (52 / 3) | shared board cache per cwd, `taskForPane` | IO/S | `app/board/cache.rs` | P12 |
| `settings/schema.tsx` | 1,212 | schema.experiments.test (80 / 4) | categories and items builder | P | `app/settings/schema.rs` | P13 |
| `lib/experiments.ts` | 77 | experiments.test (95 / 14) | experiment registry (empty today) | P | `app/experiments.rs` | P13 |
| `lib/discord.ts` | 161 | – | presence builder, publish on diff | P/IO | `app/discord.rs` | P13 |
| `lib/remote.ts` + `settings/mobileOptions.ts` | 196 + 19 | remote.test (189 / 20) | pairing, devices, Tailscale, app ports | P/IO | `app/settings/mobile_model.rs` | P13 |
| `lib/kits.ts` | 619 | kits.test (605 / 45) | kit types, permissions, effective kits, launch issues | P/IO | `app/kits/model.rs` | P14 |
| `lib/kitActivation.ts` | 323 | kitActivation.test (992 / 82) | permission diff, hooks and commands to accept | P | `app/kits/activation.rs` | P14 |
| `lib/kitSetup.ts` | 129 | kitSetup.test (122 / 11) | setup steps and keystrokes | P/IO | `app/kits/setup.rs` | P14 |
| `lib/kitGallery.ts` | 99 | kitGallery.test (373 / 35) | gallery list, semver, filter | P/IO | `app/kits/gallery.rs` | P14 |
| `lib/kitIcon.ts` | 19 | kitIcon.test (37 / 4) | kit icon path | P | `app/kits/icon.rs` | P14 |
| `components/KitActivation.tsx` view logic, `settings/KitsSettings.tsx` | – | KitActivation.test (77 / 6), KitsSettings.test (272 / 18) | view-state tests | – | `app/kits/views/*` | P14 |
| `lib/preview.ts` | 474 | preview.test (272 / 26) | preview kinds, viewer per extension, text head, markdown helpers, zoom | P | `app/preview/model.rs` | P15 |
| `lib/modelStore.ts` | 859 | modelStore.test (916 / 64) | per-workspace preview scopes, kit tabs, snapshot hooks | S | `app/preview/store.rs` | P15 |
| `lib/modelHost.ts` | 330 | modelHost.test (219 / 10) | cross-window preview host, root grants | IO | mostly dropped; root grants → `app/preview/store.rs` | P15 |
| `lib/modelFiles.ts` / `modelCapture.ts` | 57 / 38 | (in modelStore.test) | formats, size cap, stat; save PNG | P/IO | `app/preview/model.rs`, `app/model3d_view/capture.rs` | P15 / P17 |
| `lib/kitTabs.ts` | 477 | kitTabs.test (440 / 37) | kit tab URLs, permissions, params, theme event | P/S | `app/kit_tabs/tabs.rs` | P16 |
| `lib/kitApi.ts` | 833 | kitApi.test (1,543 / 104) | the kit host: dispatch, path rules, rate limits, events | P/IO | `app/kit_tabs/api.rs` | P16 |
| `lib/hangarUi.test.ts` | – | (642 / 44) | the HTML kit UI library (`kit.css`, tokens) | – | keep as a JS test of `crates/hangar-core/kit-sdk`, or port to `hangar-theme` token tests | P16 |
| `lib/model3d/*` | 3,149 | formats (50 / 7), framing (217 / 16), stats (86 / 9) | three.js viewer, loaders, framing, grid, materials | GPU/P | `crates/hangar-model3d` | P17 |
| `lib/control.ts` | 1,386 | control.test (1,920 / 120) | the 27 agent control ops | IO/H | `app/control/*` | P18 |
| `lib/managerControl.ts` | 116 | managerControl.test (172 / 9) | `project_*` ops | IO | `app/control/manager.rs` | P18 |
| `lib/deliveryControl.ts` / `deliveryRuns.ts` | 822 / 48 | deliveryControl.test (761 / 25) | delivery preflight, run, land | IO/S | `app/control/delivery.rs` | P18 |
| `lib/workerControl.ts` | 922 | workerControl.test (882 / 35) | spawn, stop and list workers, ports, briefs | IO/S | `app/orchestration/worker.rs` | P19 |
| `lib/orchestratorEvents.ts` | 888 | orchestratorEvents.test (578 / 29) | orchestrator event engine | T | `app/orchestration/events.rs` | P19 |
| `lib/managerEvents.ts` | 421 | managerEvents.test (486 / 25) | manager feed | T | `app/orchestration/manager_feed.rs` | P19 |
| `lib/orchestratorBrief.ts` | 793 | orchestratorBrief.test (438 / 18) | briefings and role prompts | P/IO | `app/orchestration/{brief,brief_text}.rs` | P19 |
| `lib/remoteControl.ts` | 919 | remoteControl.test (1,119 / 63) | phone ops (`remote_*`) | IO/S | `app/mobile/{mod,ops,chat_live,stats}.rs` | P20 |
| `lib/remoteAnswer.ts` | 299 | remoteAnswer.test (479 / 22) | answering agent questions from the phone | P | `app/mobile/answer.rs` | P20 |
| `lib/remoteDelivery.ts` | 477 | remoteDelivery.test (501 / 17) | delivery view and landing for the phone | P/IO | `app/mobile/delivery.rs` | P20 |
| `lib/remotePush.ts` | 341 | remotePush.test (489 / 20) | what to push, when | T/IO | `app/mobile/push.rs` | P20 |
| `lib/remoteApps.ts` | 143 | remoteApps.test (193 / 14) | app ports the phone may open | P/IO | `app/mobile/apps.rs` | P20 |
| `lib/activityLog.ts` | 347 | activityLog.test (274 / 14) | the activity log for phone history | T/IO | `app/mobile/activity_log.rs` | P20 |
| `remote/types.ts`, `calls.ts`, `stats.ts` | 571 / 136 / 94 | calls.test (82 / 3) | JSON contract with the phone; call titles; stats view | P | `hangar-core/src/remote/contract.rs`, `hangar-core/src/chat/calls.rs`, `app/mobile/stats.rs` | P20 |

**Totals:** about 1,600 of the 1,653 test cases are in this table. `hangarUi.test` (44) tests kit-SDK assets, not app code.

---

## 4. Terminal specifics and the pane grid

This section has its own numbering: §4/1 terminal specifics (1.1 xterm options, 1.2 palette, 1.3 addons, 1.4 keyboard, 1.5 PTY data flow, 1.6 agent state detection, 1.7 session capture and resume, 1.8 queued prompts, 1.9 broadcast, 1.10 file drop, 1.11 search, 1.12 selection/copy/scroll/focus/bell, 1.13 hibernation, 1.14 multi-window), /2 pane grid model, /3 surfaces, /4 logic libs. Owners: P3 (widget), P4 (logic), P5 (views and controller).

### Terminal pane and pane grid: porting map

Scope: `src/components/{PaneGrid,TerminalPane,TerminalSearch,PaneZoomBar,PaneGauge,StateGlyph}.tsx`, `PaneStatus.ts`, and 25 libs. The parent `WorkspaceTerminals.tsx` (330 LOC) and `lib/activity.ts` (96 LOC) are not in scope, but were read because they own file drop, prop wiring and output-based detection. The frontend is xterm.js 6.0 (`@xterm/xterm ^6.0.0`, fit 0.11, search 0.16, unicode11 0.9, web-links 0.12, webgl 0.19).

---

#### 1. Terminal specifics

##### 1.1 `new Terminal({...})` (TerminalPane.tsx:396)

| Option | Value | Default (`types.ts` DEFAULT_SETTINGS) |
|---|---|---|
| fontFamily | `settings.fontFamily` | `'"Cascadia Code", "JetBrains Mono", Consolas, "Courier New", monospace'` |
| fontSize | `settings.fontSize` | `13` |
| lineHeight | `settings.lineHeight` | `1.3` |
| letterSpacing | `settings.letterSpacing` | `0` |
| cursorStyle | `settings.cursorStyle` (`block`, `bar` or `underline`) | `"bar"` |
| cursorBlink | `settings.cursorBlink` | `true` |
| scrollback | `settings.scrollback` | `10000` |
| theme | `theme.xterm` (ITheme) | theme `graphite` |
| allowProposedApi | `true` | – |
| macOptionIsMeta | `true` | – |
| convertEol | `false` | – |

- **Defaults left alone (the native widget must match them):** `altClickMovesCursor` true, `scrollOnUserInput` true, `drawBoldTextInBrightColors` true, `minimumContrastRatio` 1, no `rightClickSelectsWord` override.
- **Live restyle:** an effect on `[theme, fontFamily, fontSize, lineHeight, letterSpacing, cursorStyle, cursorBlink, scrollback]` mutates `term.options.*`, then `fit()` in the next animation frame. A restyle never restarts the pane.
- **Host div `.pane__term`:** `padding: settings.padding` (default 10px), `background: theme.xterm.background`. `.xterm-viewport` has a transparent background. A pane that is not focused is drawn at `opacity: 0.94`.
- **Theme per workspace:** `getTheme(workspace.themeId ?? settings.themeId)`.

##### 1.2 Theme → palette (`themes.ts`)
- `TerminalTheme { id, name, appearance: "dark"|"light", accent, xterm: ITheme }`. There are **33 themes** (25 dark, 8 light), all with the same **21 xterm keys**: `background, foreground, cursor, cursorAccent, selectionBackground, black, red, green, yellow, blue, magenta, cyan, white, brightBlack … brightWhite`.
- No theme sets `selectionForeground`.
- `accent` drives the chrome and the active search match.
- `mix()` comes from `lib/contrast` and is re-exported.
- Port: one `hangar_theme::Palette` per theme, mapped onto alacritty `Colors` (16 ANSI, foreground, background, cursor, cursor text = `cursorAccent`, selection background).

##### 1.3 Addons

| Addon | When | Notes |
|---|---|---|
| FitAddon | always | `fit()` on mount, from a `ResizeObserver(host)` on every callback, and in the next frame after a restyle |
| WebLinksAddon | always, `new WebLinksAddon()` | default handler (`window.open`, no modifier). Assumption: in Tauri 2 with `tauri_plugin_opener` this lands in the system browser. Native: open with `opener` on click. No file-path links, no custom link provider |
| Unicode11Addon | always, `term.unicode.activeVersion = "11"` | emoji and agent-UI symbols are 2 cells wide. alacritty uses `unicode-width`, which is close enough; check emoji presentation |
| SearchAddon | only while the search bar is open (owned by `TerminalSearch`) | `highlightLimit: 1000`; disposed on close |
| WebglAddon (`lib/webgl.ts`) | only while `visible` | attached on show; on hide, `dispose()` after `WEBGL_PARK_MS = 10_000`. On context loss: dispose and fall back to DOM, retry on the next show. If WebGL2 is missing, a module flag `unavailable` stops all further tries |

- **When WebGL is used:** `localStorage["hangar.webgl"]` = `"1"` forces it on, `"0"` forces it off. Otherwise it is **off on Linux** (WebKitGTK, NVIDIA lag) when the UA matches `/linux/i` and not `/android/i`, and on for Windows and macOS.
- **Why parking exists:** Chromium caps live contexts at 16.
- **Native:** not applicable, since iced draws with wgpu. Equivalent: hidden panes are not drawn; optionally drop their glyph and atlas caches after 10 s hidden.

##### 1.4 Keyboard

**`attachCustomKeyEventHandler` (TerminalPane.tsx:452)**
- Any event that is not `keydown`, or has `altKey`: returned to xterm unchanged (Option = Meta on macOS).
- **Shift+Enter** (no Ctrl or Meta) **in agent panes only** (`pane.agent !== "shell"`): `preventDefault()`, `term.input("\x1b\r")` (ESC+CR, the CLIs' newline), return false.
  - In shell panes Shift+Enter submits as usual: ESC would revert the line in PSReadLine or cmd.
- **Ctrl+V or Cmd+V** (`v` or `V`): return false, so xterm does not send `^V`. The webview pastes into the helper textarea and xterm handles it as a paste (bracketed if DECSET 2004; xterm turns `\r?\n` into `\r`). Readline's quoted-insert (`^V`) is lost on purpose, the same bargain Windows Terminal makes.
- Everything else goes to xterm. Ctrl+C sends `^C`. Copy on macOS (Cmd+C) goes through the textarea `copy` event.

**`term.onData`** → `pty_write(id, data)`. It is dropped while `replaying` (see 1.5). It calls `touchWorkspace(workspaceId)` only when `visible`.

**`term.onKey`** (real keys, not query answers):
- activity `yours` → `idle`
- activity `waiting` → `working`

**App-level shortcuts acting on the terminal** (App.tsx:1451, through `terminalRegistry.getTerminal(focused)`):

| Command id | Win/Linux | macOS | Action |
|---|---|---|---|
| terminal.clear | Mod+Shift+L | Mod+Shift+L | `term.clear()` |
| terminal.copy | Ctrl+Shift+C | Meta+Shift+C | selection → `clipboard.writeText` |
| terminal.paste | Ctrl+Shift+V | Meta+Shift+V | `clipboard.readText` → `term.paste` (bracketed) |
| terminal.selectAll | Ctrl+Shift+A | Meta+Shift+A | `selectAll()` |
| terminal.scrollTop | Ctrl+Home | Meta+Up | `scrollToTop()` |
| terminal.scrollBottom | Ctrl+End | Meta+Down | `scrollToBottom()` |
| terminal.find | Ctrl+Shift+F | Meta+F | `openFinder(focused)` |

Pane shortcuts: `pane.zoom` Mod+Shift+Z (Meta+Alt+Z on macOS), `pane.restart` Mod+Shift+R, `pane.close` Mod+Shift+X, `pane.sessions` Mod+Shift+S, `pane.split` Mod+Shift+Enter, `pane.splitRight` Ctrl+Alt+Right (Meta+D), `pane.splitDown` Ctrl+Alt+Down (Meta+Shift+D), `pane.next`/`prev` Ctrl+(Shift+)Tab, `pane.focus{Left,Right,Up,Down}` Alt+arrow (Ctrl+Alt+arrow on macOS) via `neighborOf`, `pane.swap*` Alt+Shift+arrow (`movePane(..., "center")`), `pane.focus1..9` (shown in the header tooltip for the first 9 panes in reading order).

##### 1.5 PTY data flow end to end

**Spawn** (`ptySpawn` → `invoke("pty_spawn", {...})`):
- Arguments: `{ id: pane.id, cwd: pane.cwd || workspace.cwd, shell: ShellInfo|null, cols: term.cols, rows: term.rows, name: pane.name, bridge: settings.claudeBridge, portBase: pane.portBase ?? null, mcpConfig: <file path>|null, kits: string[], agent: pane.agent, claudeAccount: account.id|null }`.
- Returns `PtySpawned { kitIssues: KitLaunchIssue[], account?, accountIssues? }`.
- **The backend environment** (`pty.rs`):
  - `TERM=xterm-256color`, `HANGAR_PANE_ID`, `HANGAR_PANE_NAME`.
  - If `bridge` is not false: `HANGAR_BRIDGE_URL=http://127.0.0.1:<port>`, `HANGAR_BRIDGE_TOKEN`, `CLAUDE_CODE_PLUGIN_DIRS` (mod and kit plugins; left out when the managed policy forbids sideloading).
  - `HANGAR_PORT_BASE` / `HANGAR_PORT_COUNT=20` (workers only), `HANGAR_MCP_CONFIG`, `CLAUDE_CONFIG_DIR` (account profile).
  - Strips `CLAUDE_CODE_CHILD_SESSION, CLAUDE_CODE_SESSION_ID, CLAUDE_CODE_ENTRYPOINT, CLAUDECODE, CLAUDE_PID`.
  - PowerShell gets `-NoLogo` (after a leading `-Login`).
  - Falls back to `default_shell()` when the given shell is missing.
  - Waits up to `REAP_TIMEOUT` 5 s if the same id is still being reaped.

**Bus** (`lib/ptyBus.ts`):
- One global `listen("pty:output")` and one `listen("pty:exit")` fan out to `Map<paneId, handler>`.
- `subscribePty(id, onOutput(data, seq), onExit)` **must be awaited before spawn**.
- A rejected init clears the cached promise so the next pane retries.

**Backend events and commands:**

| Name | Payload | Semantics |
|---|---|---|
| `pty:output` (event) | `{ id, data: String, seq: u64 }` | reader thread with an 8192-byte buffer. `take_utf8` keeps a partial trailing codepoint for the next read and replaces invalid bytes with U+FFFD. `seq` = **total UTF-8 bytes printed, this chunk included**. Pushed into history **before** the event is emitted |
| `pty:exit` (event) | `{ id, code: null }` | only when the shell ended by itself (`live` still true). Never after `pty_kill` |
| `pty_history(id)` | `{ data, end: u64, account? }` | ring of `HISTORY_BYTES = 256 KiB`. When the text passes 512 KiB it is trimmed to the last 256 KiB, cut after a `\n` (so a replay never starts mid-escape sequence) |
| `pty_alive(id)` | `bool` | session present |
| `pty_write(id, data)` | – | `write_all` and flush |
| `pty_resize(id, cols, rows)` | – | `PtySize` with pixel_width and pixel_height 0 |
| `pty_kill(id, tree?)` | – | `bridge::forget(id)` (queue and edits); the session moves to `dying`; reaped on a thread: `child.kill()`; with `tree`, `procs::descendants` are terminated; unix `killpg(SIGKILL)` of the pane's group; `wait()` |
| `pty_has_children(ids)` | `string[]` | ids whose shell has descendant processes. **Errors** if the process table cannot be read |

**Mount sequence** (effect keyed on `pane.id` only; refs carry the latest props):
1. Create the Terminal and addons, `registerTerminal(id, term)`, `notePaneMounted(id)`, `fit()`.
2. `held = []`; `subscribePty`. Output arriving now goes into `held`.
3. `subscribeBridge(id, onBridgeEvent, setQueue)`, then re-enqueue `takeStashedQueue(carryKey(wsId, pane.name))` with `bridge_enqueue`.
4. `adopted = await pty_alive(id)`. If it is alive (the workspace moved to another window, or this window reloaded):
   - `history = pty_history(id)`.
   - Set `replaying = true` (suppresses onData, because xterm would otherwise answer stale DA/DSR queries and garble input), call `term.write(history.data, () => replaying = false)`, set `replayedTo = history.end`.
   - `pty_resize` to this window's size, `noteLaunchedAccount(history.account)`, status `running`.
5. Flush `held`, skipping chunks with `seq <= replayedTo`. Later live chunks are filtered the same way.
6. If `isResumable(agent)`: `known = knownSessionIds(agent, cwd)` (`list_sessions`) **before** the launch.
7. If not adopted (`launchesPane(adopted) = !adopted`):
   - load kits (`loadPaneKits`) and the account (`launchAccount`, Claude only);
   - `mcpLaunchConfig` (`mcp_scope_write`; HANGAR_MCP_CONFIG referenced per shell as `$env:`, `"%…%"`, `"$…"` or `$env.`);
   - `beginLaunch(id)`, `pty_spawn`, status `running`, note bar ← launch notes;
   - shell panes stop here;
   - login panes: sleep `launchDelayMs`, then write `claude auth login\r` unless the account fell back;
   - otherwise: resume logic (1.7), then write `<launchCommand>\r`.
8. `watchForSession(...)` polls for the pane's whole life (resumable agents only).
9. Any throw writes `\r\n\x1b[31m<pane.spawnFailed>: <err>\x1b[0m` and sets status `exited`. `finally endLaunch(id)`.

**Output handler:**
- `notePaneOutput` (hibernation)
- `term.write(data)`
- `watcher.push()` (activity)
- `scheduleTrustLook()` (600 ms)
- `printedSinceLastLook = true`

**Exit handler:** status `exited`, `bridged = false`, activity `idle`, insight `turnStartedAt: null`, event `{kind: "exit", what: "shell"}`. The header shows a **Restart** button (`pane__revive`).

**Resize:** there is **no debounce**. Every ResizeObserver callback runs `fit()`, and `term.onResize({cols, rows})` sends `pty_resize`. Panes hidden behind a zoom keep their slot size, so they are not resized. Native assumption: coalesce resizes to one per frame and send only when cols or rows change.

**Unmount:**
- `finishing = finishesLaunch(id)` (a handover before the command was typed: this window finishes typing the launch command anyway);
- abort the controller unless finishing;
- if a successor exists (same name, other id, i.e. a restart), `stashQueue(carryKey, queue)`;
- dispose the observer, watcher and trust timer;
- unregister, `forgetPane`, `noteLaunchedAccount(id, null)`, unsubscribe pty and bridge;
- **`pty_kill(id)` unless `isHandingOver(id)`**;
- `term.dispose()`;
- separate effects clear paneActivity and paneInsight unless handing over.

**Restart, agent change, shell change** all mean `respawnPane`: a **new pane id**, so a full remount (kill the old PTY, spawn a new one). `renameLeaf` keeps the tree, `carryResume` keeps the hibernation resume promise.

##### 1.6 Agent state detection

States (`agentState.ts`): `working | waiting | yours | idle`. The shell's own lifecycle is `starting | running | exited`.

Without the mod (output heuristics; all of them stand down once `bridged`):

| Source | Rule | Effect |
|---|---|---|
| `createActivityWatcher` (`lib/activity.ts`) | `MIN_BUSY_MS=2500`, `TICK_MS=250`, `DEBOUNCE_MS=1500`, `idleMs = settings.notifyIdleMs` (default **3000**). The first `push()` starts a burst and a 250 ms ticker | burst ≥2.5 s → `onBusy` once per burst → `working`. Silence ≥ idleMs after a burst ≥2.5 s → `onSettle` → `handBack("notify.settled")`. A short burst stops silently |
| Bell (`term.onBell`) | `watcher.ring()` | settles now (1.5 s debounce) |
| OSC 9 | ignored (returns false) if `/^\d+(;|$)/` (ConEmu progress `9;4;…`, cwd `9;9;…`) | `handBack(data.trim() \|\| notify.settled)` |
| OSC 777 | `notify;title;body`, other kinds ignored | `handBack(body \|\| title \|\| notify.settled)` |
| Trust prompt | 600 ms after the last output, read the viewport rows (`translateToString(true)`). `isTrustPrompt`: whitespace collapsed, lowercased, contains both `"quick safety check"` and `"trust this folder"` | appears → `waiting` and `announce(notify.trustFolder)`; disappears → `working`. While it is up, settle is suppressed |

Other transitions:
- **Keys:** `yours`→`idle`, `waiting`→`working`.
- **Focus and visible** (and window `focus` event): attention cleared, `yours`→`idle`.
- **handBack:** `yours`, or `idle` if `document.hasFocus() && focused && visible`, then `announce`.
- **announce(body):**
  - no-op if window focused, pane focused and pane visible;
  - else `attention = true` (outline `pane--attn-{waiting|yours}`, bell icon on shell panes);
  - desktop `notify(t("notify.title", {name, agent}), body, {workspaceId, paneId})` when `notifyOnIdle` (default true) and not (`notifyOnlyWhenAway` (default true) and window focused).
- **Published:** `setPaneActivity(id, {workspaceId, activity, bridged})` when `reportsActivity = agent !== "shell" || bridged`, otherwise cleared. Relayed to other windows on the `agent:activity` event. Also `notePaneActivity` for hibernation.

**Bridge-mod events** (`bridge:event {paneId, event}`). The first non-`end` event sets `bridged = true` at once.

| kind | Payload | Handling |
|---|---|---|
| hello | sessionId, model, version | `noteAgentExited(false)`, `activityAtHello` (working→idle), insight {model, agentVersion, sessionSince, turnStartedAt: null}, event `session` (resumed = id equals stored id), `captureSession` |
| session | sessionId, source | if it differs from the reported session: new sessionSince, event (resumed = `source === "resume"`), capture |
| turn start | – | `working`, `turnStartedAt`, event `turn-start` |
| turn end | reason (answer, aborted, refusal, error), durationMs, answer | insight lastAnswer, event `turn-end` (outcome answer, aborted or failed), `handBack(answer \|\| notify.aborted or notify.settled)` |
| permission | waiting, tool?, input? | waiting → `waiting`, event, `announce(notify.permission{tool})`; else waiting→working |
| usage | context {tokens, window, percent}, rateLimits[], costUsd | bridge gauge (pct clamped 0–100; usedTokens derived if missing), `setPlanLimits`, insight costUsd |
| edit | file, overlap?, overlapPane? | `recordEditedFile`, event (basename and overlap), `flagOverlap(overlapPane, myName)`: the warning icon shows for `OVERLAP_SHOWN_MS = 2 min` |
| dropped | text, reason | event, `announce(notify.dropped)` |
| tool | start or end | **ignored** by TerminalPane |
| end | reason | `noteAgentExited(true)`, bridged false, gauge null, `idle`, event exit:agent |

`captureSession` refuses ids that do not match `/^[\w-]+$/` and ids equal to the current one. It calls `claim(id)` and `onSessionCaptured(id)` (store `updatePane`).

**PaneStatus.ts → UI:**
- `paneGlyphState(status, activity)`: the status until running, then the activity or `idle`.
- `gaugeLevel`: `ok`; `warn` from **70**; `hot` from **90**.
- `shellShortName`: basename without `.exe|.cmd|.bat|.com`, lowercased.
- `shortSessionId`: 6 characters. `sessionIdLabel` adds `…`.
- `firstLine(text, 120)`, `fileBaseName`, `newlyQueued`.
- **State word key:** `pane.state.${status≠running ? status : reportsActivity ? activity : "running"}`, with ` · pane.state.reported|guessed`. Short forms `waitingShort` ("Answer?") and `yoursShort` ("Your turn").

##### 1.7 Session capture and resume; agent commands (`agents.ts`, `types.AGENTS`)

| id | Label | resumable | Command (default; `settings.agentCommands[id].trim()` overrides) | Resume |
|---|---|---|---|---|
| shell | Shell | no | none (plain prompt) | – |
| claude | Claude Code | yes | `claude` + flags in this order: ` --resume <id>` ` --name <n>` (quoted `"…"` unless `/^[\w-]+$/`; dropped if it holds `"`, backtick, `$`, `\r` or `\n`) ` --model <m>` (quoted unless `/^[\w.-]+$/`) ` --permission-mode <M>` (`/^[A-Za-z]+$/`) ` --allowedTools r1 r2` (`/^[\w*.-]+$/`) ` --mcp-config <arg> [--strict-mcp-config]` ` --add-dir "r1" "r2"` (always last; extra roots, unsafe ones dropped) | `--resume <id>` |
| codex | Codex | yes | `codex` | `codex resume <id>` |
| gemini | Gemini | no | `gemini` | – |
| opencode | OpenCode | no | `opencode` | – |

- **`--name`** = `sessionName(pane.name, workspaceName)` → `"Ava (Workspace)"`. The workspace part has `"'$\\%!^&|<>` and backticks stripped.
- **Role panes** add `permissionMode` from the orchestration settings and `ORCHESTRATION_ALLOWED_TOOLS`. Login panes run `claude auth login`.
- **Resume at launch:**
  - `owed = mustResume(id)` (hibernation);
  - `saved = (settings.autoResume (default true) || owed) ? pane.sessionId : null`;
  - `session_exists(agent, saved)` runs **in parallel** with `sleep(launchDelayMs = 700)`;
  - if it returns `false` (not null), drop the resume and call `onSessionCaptured(null)`;
  - `claim(resumeId)`, write `${command}\r`;
  - non-bridged agents record the session event themselves.
- **`watchForSession`:**
  - polls `list_sessions(agent, cwd)` → `[{id, label, modified_ms}]`, sorted newest first;
  - every `HUNT_MS = 1500` until the first find, then `FOLLOW_MS = 5000`;
  - skips a round when nothing was printed since the last look;
  - adopts the first session that is not in `known`, not in the global `claimed` set and has `now - modified_ms < FRESH_MS (15 000)`;
  - `onFound` → `onSessionCaptured` unless bridged;
  - runs until the pane's AbortSignal fires, so it follows `/new` and `/clear`.
- **Header session button** (resumable agents): `pane.resuming {id}` or `pane.noSession`; the label is the short id + `…` or `pane.newSession` ("new"). Click → `onOpenSessions` (sessions picker, out of scope).
- **Role restart** (orchestration, `roleRestart`): a running Claude pane pinned without Hangar's MCP server prints a yellow line, calls `queueRestartNote`, checks `session_exists`, then `onRestart`.

##### 1.8 Queued prompts (bridge queue)
- **Event:** `bridge:queue {paneId, pending: QueuedPrompt[]}`. `QueuedPrompt = {id: uuid, text, from?}` (`from` absent = the person; a pane name for an agent; `"hangar"` for Hangar's own).
- **Commands:**
  - `bridge_enqueue {paneId, text, from|null}` → QueuedPrompt
  - `bridge_cancel {paneId, id|null}` (null clears all)
  - `bridge_queue {paneId}`
- **Backend:** FIFO per pane. The mod pulls through `GET next?labelled=` (mid-turn it takes only `from != null`). `pty_kill` forgets the queue.
- **UI:** a header button `pane__queue` (tray icon and count; tooltip `pane.queue_one|_other`); **click cancels the whole queue**. Each new entry is recorded as a `queued` insight event.
- **Carry over a restart:** `CARRY_MS = 10 000`, keyed `${workspaceId}/${paneName}`, taken once.

##### 1.9 Broadcast and saved commands (App.tsx, `lib/savedCommands.ts`)
- **Broadcast box (toolbar):** for each pane of the active workspace, if `whenFree` and the pane is bridged → `bridge_enqueue(text)`; else `pty_write(text + "\r")`. Then `touchWorkspace`.
- **`runSavedCommand(cmd, ids)`:** payload `cmd.command` (+ `\r` if `autoRun`). If `autoRun` and the pane is bridged and busy (working or waiting) → `bridge_enqueue`; else `pty_write`. `cmd.broadcast` → every pane id of the workspace. `command.run1..9` shortcuts follow the workspace commands, then the global ones.
- **Voice transcript** → `pty_write(focused, text[+"\r" if voiceSubmit])`.

##### 1.10 File drop (`lib/fileDrop.ts`, used in WorkspaceTerminals)
- **Events:** `tauri://drag-enter {paths, position}` (only enter carries the paths, so the count is remembered), `tauri://drag-over {position}`, `tauri://drag-drop {paths, position}`, `tauri://drag-leave`.
- **Position units:** CSS px = position / `devicePixelRatio` **on Windows only**. macOS and Linux positions are already logical.
- **Target:** `elementFromPoint` → `[data-slot]`; only the workspace on screen listens. The overlay is `.grid__filedrop` with `drop.paste_one|_other`.
- **Drop:** focus the pane, `term.paste(dropText(paths, agent, shell))` (as a paste, so Claude Code sees bracketed paste, which is how it tells image paths apart), then focus the terminal.
- **`pathStyleOf(shell)`:**

  | Shell | Style | C:\x\y | `\\wsl$\D\p` or `\\wsl.localhost\D\p` |
  |---|---|---|---|
  | id `wsl` or program `wsl.exe` | `wsl` | `/mnt/c/x/y` | `/p` |
  | `git-bash`, `msys2` | `msys` | `/c/x/y` | unchanged |
  | `cygwin` | `cygwin` | `/cygdrive/c/x/y` | unchanged |
  | any other shell | `native` | unchanged | unchanged |

- **`quoteStyleOf(agent, shell)`** (only quotes when the bare-word regex fails):

  | When | Style | Bare-word regex | Quoting |
  |---|---|---|---|
  | any agent pane | `agent` | `/^\S+$/` | `"…"` with `\"` escapes |
  | shell is cmd | `cmd` | `/^[\w\-./:\\@+=,~#]+$/` | `"…"` |
  | shell is pwsh, powershell or nu | `single` | `/^[\w\-./:\\%+=~]+$/` | `'…'` with `''` |
  | any other shell | `posix` | `/^[\w\-./:@%+=,]+$/` | `'…'` with `'\''` |

- Paths are joined with spaces, with a **trailing space and no Enter**.
- **iced note:** `window::Event::FileHovered(PathBuf)` and `FileDropped(PathBuf)` come **one per file and carry no position**. Track the cursor from `CursorMoved` and count the hovered files until `FilesHoveredLeft`.

##### 1.11 Search (`TerminalSearch`)
- **Opening:** `terminal.find` → `openFinder(id)` → pane state `{request, seed}`.
  - The first open seeds from a **single-line** selection, else from the last query.
  - Pressing again while open bumps `request`: it refocuses and selects the input, without reseeding.
- **Typing:** incremental `findPrevious` (searches **upward**; the newest match is at the bottom).
- **Keys in the field:**
  - Enter = previous (up), Shift+Enter = next (down)
  - Escape closes; the query is kept for the next open
  - Alt+C toggles case, Alt+R toggles regex
- **Buttons:** `Aa`, `.*`, up, down, close. Mousedown is prevented so the input keeps the focus.
- **Status:**
  - `search.invalid` for a regex that fails to compile;
  - `search.none` when there is no hit;
  - `"i/N"`, or `"N"` when no match is current; N shows as `1000+` once it reaches the limit.
- **Decorations:**
  - matchBackground `mix(bg, yellow, .3)`, matchBorder `mix(bg, yellow, .65)`, overview ruler `yellow`;
  - active match: `mix(bg, accent, .45)`, border `accent`, ruler `accent`;
  - fallbacks: bg `#000000`, yellow `#e0af68`.
- Closing refocuses the terminal.

##### 1.12 Selection, copy, scroll, focus, titles, bell, notifications
- **Selection:** xterm defaults (drag; double-click word; triple-click line; Alt+click moves the cursor; Shift overrides mouse reporting; colour `selectionBackground`). No copy on select. Copy only by shortcut, or by the OS copy event (Cmd+C on macOS). Double-click on a **header** zooms; in the terminal it selects a word.
- **Scroll:** no custom code. xterm defaults: the wheel scrolls the scrollback; in the alt screen without mouse mode it sends arrow keys; input scrolls to the bottom (`scrollOnUserInput`). Shortcuts scroll to top or bottom.
- **Focus:**
  - `term.focus()` when `focused && visible`; this also re-runs when visibility changes, because workspace switches drop DOM focus;
  - pane `onMouseDown` → `touchWorkspace` and `onFocus`;
  - menus and search hand the focus back to the terminal on close;
  - zoom gestures focus the target terminal in the next animation frame;
  - xterm's focus reporting (DECSET 1004 `\e[I`/`\e[O`) is implicit and must be implemented natively.
- **Titles:** OSC 0/2 window titles are **not used** (`Pane.title` exists but is never read here). The header shows `pane.name` (from `paneNames`).
- **Bell:** no audio or visual bell; it only settles the activity watcher.
- **Notifications** (`lib/notify.ts`):
  - permission is checked and requested once (plugin);
  - with a target → `invoke("notify_send", {title, body, workspaceId, paneId})`; clickable on **Windows only** (emits `notification-activated`);
  - otherwise, or on failure → plugin `sendNotification({title, body})`; errors are swallowed.

##### 1.13 Hibernation (`hibernate.ts`, `hibernateStore.ts`, `useHibernation.ts`)
- **Settings:** `hibernateIdle` (default true), `hibernateAfterMin` (default **10**; choices `HIBERNATE_DELAYS = [5,10,20,30,60]`).
- **Sweep:** `HIBERNATE_TICK_MS = 30 000`, one window-wide timer, re-entrancy guarded. Off in workspace windows. The active workspace is touched on every activeId change and when it leaves the screen.
- **Rules** (`hibernationVerdict`, the first failure is the blocker):
  1. Must be mounted (`notMounted`), not on screen (`onScreen`), not detached (`detached`).
  2. Must not host a `manager` pane or a `worker` pane, and nothing may be `delivering` (`isDelivering || deliveringIn`).
  3. Untouched for the delay (`recentInteraction`).
  4. At least one agent pane (`noAgents`).
  5. Each pane:
     - it must have a rest entry (`unknownPane`);
     - agent panes must be resumable (`notResumable`), have a sessionId (`noSession`), not be busy (`busy`), have rested for the delay (`recentlyBusy`), and have nothing queued (`queued`);
     - an agent that exited counts as a shell;
     - shells must have printed nothing for the delay (`shellOutput`) and have no children (`shellChildren`); `hasChildren` undefined → `probe`.
  6. The probe asks `pty_has_children`, then **re-reads** everything with the answer. A second probe, or an error, gives `unknownProcesses`.
- **Forced sleep** (navigator or palette): same rules without the clock. It refuses `asleep`, `detached`, `notMounted`, and `alone` (on screen with nowhere for the main window to step aside to). The on-screen workspace first `show(stepAside(...))` (most recent awake, else the navigator order, never detached). A workspace of shells alone may sleep.
- **What sleep does** (`putToSleep`):
  - `resumeOnWake(agent pane ids)`;
  - `pty_kill(id, tree = true)` for **every** pane;
  - `markAsleep(wsId, {at, agents})`;
  - `unmount(wsId)`.
- **Wake:** any remount (navigator click, agent request, sending it to a window) calls `markAwake` and `touchWorkspace`. Each agent pane then resumes because `mustResume(id)` overrides `autoResume`. Workspace windows get `?resume=a,b` in their URL.
- **Pane rest data** (in memory, per window): `lastOutputAt`, `busy` (working or waiting), `restingSince`, `agentExited`. `queued` is read from paneInsight.
- **`sleepRefusal`** maps each blocker to a `sleep.*` string.

##### 1.14 Multi-window (`lib/windows.ts`)
- **Commands:** `open_workspace_window {workspaceId, title, bounds|null, resume|null}`, `focus_workspace_window {workspaceId}` → bool, `close_workspace_window {workspaceId}`.
- **Window identity:** label `main` (or `demo`) owns the store. `?workspace=<id>` marks a workspace window.
- **Handover:**
  - `markHandover(ids)` lasts `HANDOVER_MS = 15 000`; an unmount does not kill those PTYs;
  - the receiving window adopts them (1.5 step 4);
  - **exactly one launcher**: `finishesLaunch` = launching and handing over;
  - `beginLaunch` / `endLaunch` wrap the launch.
- **`terminal:ask {reqId: "<label>:<n>", paneId, op}`**, where `op` is `{kind: "probe"}`, `{kind: "paste", text}` or `{kind: "read", lines}`.
  - Answered by `terminal:answer {reqId, value}`; timeout `ASK_TIMEOUT_MS = 1500` (resolves undefined).
  - A probe returns `{applicationCursor: modes.applicationCursorKeysMode}`.
  - `readLines` joins wrapped rows, drops trailing blank lines and returns the last N.
  - Served by `serveTerminals(getTerminal)` in every window; used by MCP control (`terminal(paneId)`).
- **Native:** windows are iced windows of one daemon, so ask/answer becomes a direct call. Keep the `PaneTerminal {paste, read(lines), application_cursor}` trait.

---

#### 2. PaneGrid model (`lib/layout.ts`, store.tsx)

- **Tree:** `SplitNode = {type: "pane", id} | {type: "split", dir: "row"|"col", ratio, a, b}`. `row` = side by side (vertical bar), `col` = stacked. `Path = ("a"|"b")[]`; `pathKey` = joined string or `"root"`.
- **Limits:** `MIN_PANE_PX = 150`, `MAX_PANES = 16`. `presetSize(n) = clamp(round(n), 1, 16)`. UI presets `LayoutSize = 1|2|4|8`.
- **`presetTree(ids)`:**
  - `rows = max(1, ceil(sqrt(n/2)))`, `columns = ceil(n/rows)`;
  - bands of `columns` folded as `row`, the bands folded as `col`;
  - `fold` = balanced binary split with `ratio = ceil(len/2)/len`;
  - shapes: 1, 1×2, 2×2, 4×2, and an odd last pane takes the whole last row.
- **Operations:** `withRatio`/`ratioAt` (a leaf answers 0.5), `removeLeaf` (the sibling takes the space; null when empty), `renameLeaf` (respawn), `insertLeaf(target, new, zone)` (ratio 0.5; left or top puts the new pane in `a`), `swapLeaves`, `moveLeaf` (center = swap, edge = remove then insert; a drop on itself does nothing).
- **`normalizeTree(panes, tree)`:**
  - migrates legacy `{type: "pane", index}` nodes;
  - a split with a missing child collapses; a ratio outside (0,1) becomes 0.5; `dir` defaults to `row`;
  - returns the same tree if it covers exactly the current panes;
  - otherwise `presetTree(kept-in-order + new ids)`; no panes → leaf `""`.
- **Reading order:** `readingOrder` = leaves of the normalized tree. `closedByPreset` = reading order sliced from the preset size (a smaller preset closes the panes read last).
- **`computeLayout(tree)`:**
  - fractional rects; ratio clamped to [0.05, 0.95];
  - `handles[]` with `{key, path, dir, rect (zero-thickness line), bounds (the split's area), before[], after[]}`;
  - `order` = reading order.
- **Drop zones:** `zoneAt(rect, x, y)` returns `center` if 0.33 < dx, dy < 0.67, else the nearest edge (ties: left, right, top, bottom). `zoneRect` = the half rect for the preview.
- **`neighborOf(tree, id, dir)`:** works on rects, not the tree. A candidate must lie wholly past our edge (ε = 1e-4) and overlap on the other axis. The smallest gap wins, then the largest overlap.
- **`preferredDir(id)`:** DOM `width >= height*1.15` → `row`, else `col`. Used for splits that give no direction.
- **Store operations:**
  - `addPane` (refused at MAX_PANES; inserted right of or below `near`, defaulting to the last leaf; inherits the agent of `near`; `pickPaneName` unique across all workspaces);
  - `closePane` (never the last pane);
  - `movePane`, `setTree`;
  - `applyPreset(count, modelPaneId)` (keeps panes in reading order; new panes use the agent of the model pane, else of the last kept pane);
  - `tidyLayout` (`presetTree` of the current order);
  - `respawnPane` (new id + `renameLeaf`).
- **Rendering** (`PaneGrid.tsx`):
  - flat absolutely positioned `.grid__slot[data-slot=id]` in **stable pane order**: no reparenting, so terminals never remount;
  - `is-inner-x/y` gives a 1px separator when left or top > 1e-6;
  - `grid` is `display: none` when the workspace is hidden.
- **Resizing dividers** (`.splitter.splitter--col|row`, `role=separator`, tabIndex 0):
  - pointer: capture on the grip, `is-active` class, `body.is-resizing`;
  - live repaint by **writing styles straight to the DOM** (`paintLayout`, bypassing React);
  - minimum ratio `min(0.45, 150/span_px)`;
  - commits on up, cancel **or window blur** (a cancel still commits the latest ratio);
  - double-click sets 0.5;
  - keys: arrows along the axis move `24px` (Shift ×5 = 120 px); Home = min, End = 1 − min; Enter = 0.5;
  - `aria-valuenow` = ratio clamped to [0.05, 0.95] as a percentage; label `grid.resizeColumns|Rows {before, after}` (pane names).
- **Drag to move:**
  - starts from a header `pointerdown` (button 0; ignored on button, select, input or a; never while zoomed);
  - pointer capture; arms after **6 px**;
  - `body.is-dragging-pane`, `.grid__slot.is-dragging`;
  - a `.drag-ghost` with the pane name follows the pointer (`translate`);
  - hit test with `elementFromPoint` → slot → `zoneAt`; preview `.drop-hint.drop-hint--{zone}` = `zoneRect`;
  - release → `onMove(drag, target, zone)`; cancel or blur → nothing.
- **Zoom** (maximize):
  - the zoomed slot takes `top: var(--zoombar-h)` (28px × ui-scale), full size;
  - the others get `.is-behind`: hidden but still mounted and **not resized**;
  - splitters are not rendered; `PaneZoomBar` is shown;
  - double-click on the pressed header toggles zoom;
  - App's `zoomByWorkspace[wsId]: bool`, so **the zoom follows focus** (`zoomedId = zoomed && panes > 1 ? focusedPaneId : null`);
  - any change in pane count ends the zoom;
  - visibility per pane = `!hidden && (!zoomedId || zoomedId === id)`.

---

#### 3. Surfaces

##### PaneGrid
- **Files:** `src/components/PaneGrid.tsx` (459 LOC); `src/lib/layout.ts` (415 LOC).
- **Shows:** the slots, splitters, drop hint, drag ghost, file-drop overlay and zoom bar.
- **Interactions:** divider drag, keys and double-click; header drag to move; header double-click to zoom; file-drop highlight.
- **Props** (from WorkspaceTerminals): `panes`, `tree` (normalized), `renderPane(pane)`, `onTreeChange` → `setTree`, `onMove` → `movePane`, `zoomedId`, `onZoom` → `zoomTo` (focus, set zoom, focus the terminal next frame), `fileTarget {paneId, count}`, `hidden`.
- **Exports:** `usePaneDrag()`, a context `{begin(paneId, PointerEvent), draggingId}` used by the TerminalPane header.
- **Backend:** none.
- **i18n:** `grid.*` 2, `drop.*` 1 key (plural, 2 forms).
- **CSS:** `grid`, `grid--zoomed`, `grid__slot` (`is-inner-x`, `is-inner-y`, `is-dragging`, `is-behind`), `grid__filedrop`, `grid__filedrop-label`, `splitter`, `splitter--col`, `splitter--row`, `is-active`, `drop-hint`, `drop-hint--{left|right|top|bottom|center}`, `drag-ghost`, `body.is-resizing`, `body.is-dragging-pane`, var `--zoombar-h`.
- **iced notes:**
  - a custom `Widget` that owns the per-pane `Element`s and lays the children out at rect × bounds (pane widgets must not be rebuilt when the tree changes);
  - keep the `pane_id → widget::Id` identity stable so `hangar_term` state survives moves;
  - splitter hit areas of about 6 px on handle rects;
  - mouse capture lives in widget state (iced delivers events after the cursor leaves the window when a button is pressed); treat `window::Event::Unfocused` as the end of a gesture;
  - drag ghost and hint go in an overlay layer;
  - keyboard-focusable splitters via `widget::focus` operations.

##### TerminalPane (+ `PaneBranchLabel`)
- **Files:** `src/components/TerminalPane.tsx` (1942 LOC; the PaneBranchLabel at 1904–1942); `PaneStatus.ts` (72).
- **Shows:**
  - **Header** `.pane__bar`, 30px × scale, font 12px × scale, a grab cursor, and the drag handle (`title = pane.move`). In order:
    1. StateGlyph
    2. name (title `pane.jump {keys}` for the first 9 panes, else `pane.label`)
    3. git branch (`git_repo_info`; max 18ch; shrinks first; detached shows the short sha + italic `paneBranch.detachedMark`)
    4. `OrchestrationBadge` and `PaneAccountBadge` (out of scope)
    5. agent picker: a native `<select>` laid transparently over its label (long label, first-word short label, updown icon); `(missing)` suffix for agents not on PATH
    6. `.pane__meta`: shell picker (`"<label> (inherited)"` plus the detected shells), session button, PaneGauge
    7. spacer
    8. state word (long and short, `role=status`; hidden for running shells)
    9. queue button
    10. conflict warning (`pane.conflict {name}`)
    11. not-on-PATH warning
    12. bell (attention, shell panes only)
    13. **Restart** button when exited
    14. action group (always shown on the focused pane; on hover or focus-within for the others): ⚡ commands (`CommandMenu`), diff (if `onShowDiff`), zoom (if more than 1 pane; `aria-pressed`), `…` more (`PaneMenu`), close (disabled if it is the last pane: `pane.closeLast`)
  - **Launch notes bar:** amber rows (`#e5c07b`, inline styled), deduplicated, each dismissible; the dismiss label is **hard-coded** "Dismiss"/"Masquer" (not in i18n). Notes are kit issues, a missing account, mcpScope warnings and the restart note.
  - **Body:** `.pane__body` > `.pane__term` (the xterm host) + the floating `TerminalSearch`.
- **"…" menu items** (in order):
  1. restart
  2. sessions (resumable only)
  3. split right *(separated)*, split down
  4. zoom (only if the header folded it; label `zoom.restore` or `pane.menuZoom`)
  5. commands (if folded)
  6. diff (if folded)
  7. orchestration group *(separated)*: role orchestrator → `orch.unpin` + `orch.settingsMenu`; else if `canPin` → `orch.pin`

  Folded buttons are detected with `getClientRects().length === 0`. Native: compute them from the width breakpoints below.
- **Header breakpoints** (CSS container `pane-bar`; em = 12px × scale, measured on the content box):

  | Max width | Hides |
  |---|---|
  | 47em | shell picker; waiting word goes short |
  | 42em | session id; all state words short |
  | 37em | gauge % |
  | 34em | diff button |
  | 29em (focused) / 28.5em (hover) | state word |
  | 27.5em | ⚡ and the session button |
  | 25em | meta and the zoom button |
  | 21em | short agent name; session button |
  | 15.5em | meta, icons; actions 20px wide |
  | 12em | agent picker, queue, state |

- **Outline:** focus ring `inset 1px accent 66%`. Attention: `pane--attn-waiting` (waiting fill) or `pane--attn-yours` (yours fill).
- **Props from WorkspaceTerminals:** `pane`, `workspaceId`, `workspaceName`, `cwd`, `extraRoots`, `mcp?`, `orchestrationSettings?`, `settings`, `theme`, `focused`, `visible`, `index` (reading order), `availableAgents`, `shells`, `shell` (resolved pane → workspace → global → first), `canClose`, `workspaceCommands`, `globalCommands`, `zoomed`, and these callbacks:
  - `onFocus`
  - `onAgentChange(agent)` → respawn with `{agent, sessionId: null}`
  - `onShellChange(id|null)` → respawn
  - `onSessionCaptured(id|null)` → `updatePane`
  - `onRestart` → respawn `{}`
  - `onSplit(dir?)`, `onClose`, `onOpenSessions`
  - `onShowDiff?` (DiffView)
  - `onToggleZoom?`
  - `onRunCommand` → `runSavedCommand`
  - `onEditCommand`, `onRemoveCommand`, `onAddCommand` (CommandDialog)
  - `orchestration? {canPin, onPin, onUnpin, onSettings}`
- **Backend invokes:** `pty_spawn`, `pty_write`, `pty_resize`, `pty_kill`, `pty_alive`, `pty_history`, `bridge_enqueue`, `bridge_cancel`, `list_sessions`, `session_exists`, `context_usage` (every **5000 ms** while visible, running, not bridged, resumable and with a sessionId; null for gemini and opencode), `mcp_scope_write`, `git_repo_info` (paneBranch), `notify_send`; plus the accounts and kits calls inside `launchAccount` / `loadPaneKits`.
- **Events:** `pty:output`, `pty:exit`, `bridge:event`, `bridge:queue`; window `focus`.
- **i18n** (distinct keys):
  - `pane.*` 26 + `pane.state.*` 11, `notify.*` 6, `mcpScope.warn*` 4, `cmd.pane.*` 3, `orch.*` 3, `commands.menu`, `diff.open`, `zoom.restore`;
  - from other dictionaries: `picker.missing` (accounts strings), `paneBranch.*` 4 (`src/strings/paneBranch.ts`, its own EN/FR table), `roleRestartText` (orchestrationActions).
- **CSS:** `pane`, `pane--focused`, `pane--attn`, `pane--attn-{waiting|yours}`, `pane__bar`, `pane__glyph`, `pane__name`, `pane__branch`, `pane__pick`, `pane__pick--agent|--shell`, `pane__pick-text`, `--long|--short`, `pane__select`, `pane__meta`, `pane__session` (`is-set`), `pane__session-id`, `pane__spacer`, `pane__state`, `pane__state--{glyph}`, `pane__state-long`, `pane__state-short`, `pane__queue`, `pane__warn`, `pane__warn--conflict`, `pane__attn`, `pane__revive`, `pane__actions`, `pane__action`, `--cmd|--diff|--zoom|--more|--close` (`is-open`), `pane__body`, `pane__term`; about 40 classes in all.
- **iced notes:**
  - split the pane into a `PaneController` (non-view state machine: lifecycle, launch, resume, watcher, bridge reducer) and a `view`;
  - native `<select>` → `pick_list`;
  - the effect keyed on `pane.id` means a controller is created and destroyed per pane id;
  - every `disposed` / `halted` check maps to an aborted task (`Task::abortable`);
  - announce needs the window's focused state (`window::Event::Focused` / `Unfocused`).

##### TerminalSearch
- **Files:** `src/components/TerminalSearch.tsx` (241 LOC).
- **Shows:** a floating bar at the top right (top 8px, right 14px, z 5, max width 100% − 28px) with the input, count, `Aa`, `.*`, separator, up, down and close.
- **Interactions:** as in 1.11.
- **Props:** `term`, `theme`, `request` (counter), `seed`, `onClose(query)`.
- **Backend:** none.
- **i18n:** `search.*` 9 (all of them). Tooltips use `formatBinding("Alt+C"|"Alt+R"|"Enter"|"Shift+Enter"|"Escape")`.
- **CSS:** `term-search`, `term-search__input` (`aria-invalid`), `term-search__count` (`is-missed`), `term-search__opt` (`is-on`), `term-search__sep`, `term-search__btn`.
- **iced notes:** a `text_input` overlay stacked over the terminal. The search engine belongs in hangar-term (`RegexSearch` from alacritty_terminal, plus a plain-text mode). Return `{index, count}` capped at 1000, and draw match and active-match fills.

##### PaneZoomBar
- **Files:** `src/components/PaneZoomBar.tsx` (89 LOC).
- **Shows:** the tag `zoom.tag`, the zoomed pane's name, `· zoom.others {n}`, chips for the other panes in reading order (StateGlyph size 7 from `usePaneActivity`, name, title `zoom.show · pane.state.*`), spacer, restore button.
- **Interactions:** a chip zooms onto that pane; restore or Escape (when the bar is reached with Tab) clears the zoom; mousedown is prevented so the terminal keeps the keyboard.
- **Props:** `zoomedId`, `order`, `names: Map`, `onZoom`.
- **Backend:** none.
- **i18n:** `zoom.*` 5 (plural `others`), `pane.state.*` 4.
- **CSS:** `zoombar` (container `zoombar`, collapses at 55em), `zoombar__tag`, `__name`, `__others`, `__dot`, `__panes`, `__pane`, `__spacer`, `__restore`.
- **iced notes:** a row of buttons. Activity comes from a global `AgentState` map in app state.

##### PaneGauge
- **Files:** `src/components/PaneGauge.tsx` (41 LOC); `lib/context.ts`.
- **Shows:** an SVG ring (viewBox 12, r = 4.6, dash = `TURN*pct/100`) and the `pane.ctxPct` text. Classes `pane__ctx--ok|warn|hot`, `pane__ring`, `pane__ring-track`, `pane__ring-value`, `pane__ctxpct`.
- **Tooltip:** `pane.context {used, window, pct, model}` with `formatTokens` (`166k`, `1M`, `1.5M`).
- **Props:** `gauge {pct, usedTokens, window, model}`, `fallbackModel` (agent id). The gauge comes from the bridge `usage` event or the transcript (`computeContextGauge`):
  - window = `contextWindow ?? claudeWindows`, first fragment match: fable-5, opus-5, sonnet-5, opus-4-8, opus-4-7, opus-4-6 and sonnet-4-6 = 1M; haiku-4-5, sonnet-4-5 and opus-4-5 = 200k;
  - `compactFraction` 1 for claude and codex;
  - null when the window is unknown or tokens ≤ 0.
- **i18n:** `pane.context`, `pane.ctxPct`.
- **iced notes:** a small `canvas` arc. Levels come from `gauge_level`.

##### StateGlyph
- **Files:** `src/components/StateGlyph.tsx` (33 LOC); CSS `styles.css:1077–1150`.
- **Shows:** an 8px glyph (size × ui-scale):

  | State | Glyph |
  |---|---|
  | working | filled dot + expanding ring pulse (2.4 s) |
  | waiting | diamond (rotate 45°, scale .86, radius 1.5px) |
  | yours | solid dot |
  | idle, exited | hollow ring 1.5px |
  | starting | hollow ring, blinking (1.2 s steps(2)) |

  Colour tokens `--state-{working|waiting|yours|idle}-fill`. Animations are off under reduced motion.
- **Props:** `state`, `size = 8`, `label?` (sets role img).
- **Used by:** navigator, header, inspector, toolbar, palette and zoom bar.
- **iced notes:** a `canvas::Program` with an animation tick subscription only while some glyph is `working` or `starting`. Put it in hangar-ui.

---

#### 4. Logic libraries

| Module | LOC | Test file (LOC, cases) | Computes / exports | Kind | Port target |
|---|---|---|---|---|---|
| components/PaneStatus.ts | 72 | PaneStatus.test.ts (69, 7) | paneGlyphState, gaugeLevel (70/90), shellShortName, shortSessionId, sessionIdLabel, firstLine, fileBaseName, newlyQueued | pure | `hangar-app/src/pane/status.rs` |
| lib/ptyBus.ts | 47 | – | one listener pair fanned out per id; subscribePty | IO (events) | replaced by per-pane channels from core (`hangar-app/src/pane/pty_link.rs`) |
| lib/agentState.ts | 239 | agentState.test.ts (14, 2) | AgentActivity; per-pane activity map; activityAtHello; overlap marks (2 min); plan limits; workspace counts and rosters (cached); bridged count; cross-window relay | store + hooks | `hangar-app/src/state/agent_state.rs` |
| lib/activity.ts *(related)* | 96 | activity.test.ts (156, 12) | output-burst watcher (2500 / 250 / 1500 ms, idleMs) | timers | `hangar-app/src/pane/activity.rs` (pure, given a clock) |
| lib/terminalRegistry.ts | 58 | – | id → Terminal; finders; diff openers | registry | app state `HashMap<PaneId, …>`; finders become Messages |
| lib/hibernate.ts | 322 | hibernate.test.ts (494, 43) | hibernationVerdict, forcedSleepVerdict, stepAside, sleepRefusal, Blocker, HIBERNATE_DELAYS, TICK 30 s | pure | `hangar-app/src/logic/hibernate.rs` (port all 43 tests) |
| lib/hibernateStore.ts | 155 | – | touched map, pane rest, resumeOnce (URL `?resume=`), asleep map | store | `hangar-app/src/state/hibernation.rs` |
| lib/useHibernation.ts | 343 | useHibernation.test.ts (261, 10) | sweep, decide (probe then re-read), readWorkspace, forceSleep, putToSleep | hook + IO | `hangar-app/src/logic/hibernation_sweep.rs` + a 30 s `Subscription` |
| lib/fileDrop.ts | 176 | fileDrop.test.ts (98, 12) | watchFileDrops; pathStyleOf, quoteStyleOf, translatePath, quotePath, dropText | IO + pure | `hangar-app/src/logic/file_drop.rs` (pure); events from iced window |
| lib/webgl.ts | 62 | webgl.test.ts (57, 6) | webglWanted, attachWebgl, PARK 10 s | IO | drop (wgpu); optional cache park in hangar-term |
| lib/layout.ts | 415 | layout.test.ts (294, 36) | SplitNode algebra, presets, computeLayout, zones, neighborOf, normalizeTree, readingOrder, MAX_PANES 16, MIN_PANE_PX 150 | pure (except `preferredDir`: DOM) | `hangar-app/src/logic/layout.rs` (all 36 tests); `preferred_dir` from rect sizes |
| lib/bridge.ts | 129 | bridge.test.ts (33, 4) | BridgeEvent union, RateLimit, subscribeBridge, queue carry (10 s) | IO + pure | `hangar-app/src/pane/bridge_events.rs` (serde enum shared with core) |
| lib/paneNames.ts | 151 | paneNames.test.ts (60, 5) | PANE_NAMES (48), pickPaneName(s), uniquePaneNames, sessionName | pure | `hangar-app/src/logic/pane_names.rs` |
| lib/paneInsight.ts | 235 | paneInsight.test.ts (134, 11) | per-pane insight store (MAX_EVENTS 40, MAX_EDITED_FILES 50), PaneEventDetail, paneEventText, queuedBy | store + pure | `hangar-app/src/state/pane_insight.rs` |
| lib/paneBranch.ts | 147 | paneBranch.test.ts (119, 9) | branch cache per folder (TTL 30 s, refresh 45 s, focus 2 s); folderKey; paneBranchOf | IO + hook | `hangar-app/src/logic/pane_branch.rs` + Subscription |
| lib/paneTask.ts | 131 | paneTask.test.ts (52, 3) | shared board cache (`board_load`, `board:changed`), taskForPane | IO + hook | `hangar-app/src/state/boards.rs` (inspector package) |
| lib/agentPane.ts | 91 | agentPane.test.ts (78, 6) | findAgentPane(assignee) heuristics | pure | `hangar-app/src/logic/agent_pane.rs` |
| lib/agents.ts | 258 | agents.test.ts (106, 9) | DEFAULT_AGENT_COMMANDS, agentCommand, launchCommand, isResumable, claim set, watchForSession (1500 / 5000 / 15 000 ms), knownSessionIds, sleep, isTrustPrompt | pure + IO | `hangar-app/src/logic/agents.rs` (pure) + `pane/session_watch.rs` |
| lib/context.ts | 95 | – | CONTEXT_GAUGE_CONFIG, computeContextGauge, formatTokens | pure | `hangar-app/src/logic/context_gauge.rs` (or hangar-core context.rs) |
| lib/shells.ts | 21 | – | resolveShell (pane → workspace → global → first), shellLabel ("Auto") | pure | `hangar-app/src/logic/shells.rs` |
| lib/notify.ts | 66 | – | permission once; `notify_send` with a plugin fallback | IO | `hangar-core::notify` call from the app |
| lib/windows.ts | 242 | windows.test.ts (50, 6) | window label, handover 15 s, launch ownership, visibleLaunchNotes, readLines, terminal ask/serve (1500 ms) | IO + pure | `hangar-app/src/windows.rs`; `read_lines` in hangar-term |
| lib/typography.ts | 57 | typography.test.ts (61, 8) | bindUnits, frenchSpacing (U+202F, U+00A0), typeset | pure | `hangar-i18n` |
| lib/contrast.ts | 152 | contrast.test.ts (119, 16) | toRgb, toHex, mix, relativeLuminance, perceivedLightness (OKLab), contrastRatio, ensureContrast (200 steps), hue helpers | pure | `hangar-theme/src/contrast.rs` |
| strings/paneBranch.ts | 39 | – | 4 EN/FR keys + its own formatter | data | `hangar-i18n` (key prefix `paneBranch.`) |

---

---

## 5. Control, orchestration and Hangar Mobile: logic that moves out of the UI

This section has its own numbering: §5/1 control and orchestration engine (1.0 today's transport and timeouts, 1.1 `control.ts` ops, … 1.10 events and commands), /2 the Hangar Mobile contract (what the desktop must keep serving), /3 logic libs. Owners: P18, P19, P20. The async contract they share is in Appendix A6.

### 1. Control & orchestration engine

#### 1.0 Today's transport (it disappears in native)

| Path | Backend side | Frontend side | Ops | Timeout (asker side) |
|---|---|---|---|---|
| Agent ops | `POST /api/control/{op}` (server.rs:448; adds `callerCwd` from `?cwd=`), `POST /api/kit/tab/open` (kit hook route, caller pane checked running) → `control::ask` → `emit("control:request", {id, op, args})` | `usePaneControl` (control.ts:1354), main window only → `runControl` → `invoke("control_reply", {id, ok, value, error})` | the 27 `OPERATIONS` (control.rs:24) | 20 s by default; `model_*`/`preview_*` 60 s; `worker_*` 120 s; `project_*` 60 s; `workspace_create` 35 min |
| Internal: briefing | `GET /api/bridge/{pane}/briefing` → `orchestrator::briefing` → `ask_internal("orchestrator_briefing", {paneId})` → `emit("control:internal")` | `useOrchestrator` → `answerInternal` (orchestratorBrief.ts:712) | `orchestrator_briefing` | 8 s; Rust then checks the reply and clips system to 24k chars, context to 32k |
| Internal: phone | `remote::Window.send` closure → `ask_internal_via` → `control:internal` | `useRemoteControl` → `answerRemote` (remoteControl.ts:869) | `remote_*`: 15 `CALLS` + `remote_workspace` (upload) + `remote_apps` (asked by apps.rs:366 before `/m/api/app/open`) | 10 s (`CALL_TIMEOUT`) |

- **Reply contract:** one reply per id. A late reply is dropped (the `pending` map). A refusal is `Err(String)`, which becomes HTTP 400 `{error:"refused", message}`. No answer means `Unavailable`, HTTP 503 (`pc-busy` for the phone). If the store is not loaded yet, the reply is `Err("Hangar.AI is still starting. Try again in a moment.")`.
- **Gating:** agent ops throw `DISABLED` unless `settings.agentPaneControl !== false`. Remote ops run as the person: `runRemote` rebuilds the deps with `enabled: () => true`. The briefing is never gated.
- **Two listeners share `control:internal`:** each claims its own ops by name. An unknown internal op is answered by nobody and times out.
- **Caller:** comes from `args.caller {paneId, paneName, cwd}`, set by mcp.rs:1112 from `HANGAR_PANE_ID`/`HANGAR_PANE_NAME`, with `args.callerCwd` as fallback.
  - `callerWorkspace`: the workspace owning the pane id. Otherwise an exact cwd match (case- and separator-insensitive) on `ws.cwd` or any `pane.cwd`; on a tie, by pane name, then the active workspace. Otherwise the deepest ancestor `ws.cwd`.
  - `workspaceFor`: `args.workspace` (by id, then name, then cwd). Otherwise a pane name held by exactly one workspace. Otherwise the caller's workspace, or throw with the list of workspace names.
- **Error strings are part of the contract:** every thrown message is English text read by a model, and the tests assert on it. Port the strings verbatim.

#### 1.1 control.ts: `runControl(op, args, deps: ControlDeps)`

`ControlDeps` (control.ts:69) is the port the Rust `AppPort` should mirror:
- model reads: `snapshot`, `enabled`, `isOpen`, `asleep`, `activity`
- model writes: `addPane`, `closePane`, `respawnPane`, `openWorkspace`, `touch`, `activate`, `updateWorkspace`
- terminals and processes: `terminal`, `write`, `alive`, `enqueue`, `dirExists`
- windows: `canDetach`, `detach`, `reattach`
- UI and timing: `announce`, `sleep`
- preview: `modelAllowRoots`, `modelStat`, `worktrees`, `modelHost`, `previewInspect`, `kits`
- sub-deps: `fromGit`, `workers: WorkerDeps`, `delivery: DeliveryDeps`, `projects`

App.tsx:856–915 wires it:
- `announce` shows a notice with i18n `control.<kind>`.
- `terminal` uses the local xterm or the detached window's terminal (`remoteTerminal`).
- `modelHost` is `localModelHost`, or `remoteModelHost` for a detached workspace.
- `isOpen` is true when the workspace is in `openedIds` or detached.
- `respawnPane` also moves the focus and calls `carryResume`.
- `pane_send`, `pane_create` and `pane_restart` call `touch(ws)` first.

| Op | Args | Effect on state, panes, board | Reply | Backend commands |
|---|---|---|---|---|
| `workspace_list` | none | read only | `{workspaces:[{name,cwd,panes:n,active,open,asleep,yours,id}]}` | none |
| `workspace_create` | `repo` (required), `name?`, `parent?` (strings) | `workspaceFromGit`, origin agent: consent dialog (3 min), clone, `createWorkspace` with a pane `role:"orchestrator"` and `model: orchestratorModel` (same tick), trust folder, first message queued. Announces `workspaceCreated`. If the folder is not trusted and the caller is a pane, `tellWhenWaiting` runs in the background: polls activity every 2 s, 300 times; stops if `bridged`; on `waiting` it enqueues a warning to the asker from `"hangar"` | existing: `{created:false, workspace:{name,folder,id,orchestrator}, note}`. New: `{created:true, workspace, repository, yours, cloned, folder, firstMessage, note}`. Refused: throws `reason` | `git_repo_parse`, `git_repo_ownership`, `git_clone`, `git_clone_discard`, `claude_trust_folder`, `bridge_enqueue`, `activity_append` (consent trace) |
| `workspace_set_check` | `command`: ≤500 chars, no control chars | caller must be its own workspace's orchestrator; `updateWorkspace(orchestration.checkCommand)`; announces `checkSet` | `{workspace, checkCommand, previous, note}` | none |
| `pane_list` | `workspace?`, `pane?` | read; panes in layout order (`leafIds(normalizeTree)`) | `{workspace:{name,cwd,open,asleep?}, note?, panes:[{name,agent,cwd,running,activity,you,id}]}` | `pty_alive` (only when the workspace is open) |
| `pane_create` | `agent?` (an AGENTS id), `name?` (unique across all workspaces), `cwd?` (resolved against `ws.cwd`, must exist), `near?`, `direction` (`"down"` gives `col`, else `row`), `prompt?` (Claude only), `focus?` | refuses past `MAX_PANES`; `addPane`; `openWorkspace`; prompt queued with `from = caller name`; `activate` if `focus`; announces `created`. The agent defaults to `near`'s or the last pane's, else `shell` | `{workspace, pane:{name,agent,cwd,id}, prompt?:"queued: …"}` | `dir_exists`, `bridge_enqueue` |
| `pane_close` | `pane` | refuses the last pane; `closePane`; announces `closed` | `{closed, workspace}` | none |
| `pane_restart` | `pane`, `agent?`, `fresh?` | `respawnPane` with patch `{agent, sessionId:null}`, `{sessionId:null}` or `{}`, which gives a new id; `openWorkspace`; announces `reset` or `restarted` | `{pane:{name,agent,id}, fresh}` | none (the PTY respawns through the model) |
| `pane_send` | `pane`, `text?`, `keys[]`, `submit` (default true), `when_idle` | **when_idle**: Claude panes only, no keys; enqueue with `from = caller`, open the workspace if closed → `{queued:true, pane, starting?}`. **Otherwise**: needs the pane's terminal. Without one, a closed or asleep workspace is opened and the call throws "send again". Keys are checked before anything is typed. `term.paste(text)` (bracketed), sleep 120 ms, write `\r` unless `submit:false`, then each key followed by 40 ms. Keys: `enter`→`\r`, `escape`→`\x1b`, `tab`→`\t`, `shift+tab`→`\x1b[Z`, `backspace`→`\x7f`, `ctrl+c`→`\x03`, `ctrl+d`→`\x04`, arrows → `ESC O x` in application-cursor mode, else `ESC [ x` | `{sent:true, pane}` | `pty_write`, `bridge_enqueue` |
| `pane_read` | `pane`, `lines` (default 60, clamped 1..1000) | `term.read(n)`: last n lines, wrapped rows joined, trailing blank rows dropped (`readLines`, windows.ts:137) | `{pane, lines, text}` | none (terminal buffer) |
| `workspace_window` | `detach` (default true) | moves the workspace to its own window or back | `{workspace, detached, changed}` | `open_workspace_window`, `close_workspace_window` |
| `pane_focus` | `pane` | `activate(ws, pane)` | `{focused, workspace}` | none |
| `model_show` | `path`, `title?` | path resolved from the caller pane's folder; format in `MODEL_FORMATS`. `admit`: roots = ws.cwd + extraRoots + pane cwds + every non-bare worktree, then allow-roots, then stat, then an inside check on the given and canonical path. Size ≤ `MAX_MODEL_BYTES`. `preview.show({kind:"model"})`, wake if asleep, announce `modelShown`/`modelShownElsewhere`, `whenSettled` 45 s, then `sight()` | `{opened, status?:"loading", path, format, title, info, workspace, visible, woke?, note?}` | `git_worktree_list`, `model_allow_roots`, `model_stat` |
| `model_snapshot` | `path?`, `angle` (front/back/left/right/top/bottom/iso, default iso), `width`/`height` (64..2048, default 1024×768) | reveal; reload if the mtime is newer; `whenSettled` 40 s; `preview.snapshot` | `{png (base64), mimeType:"image/png", path, angle, width, height, info}` | `model_stat` |
| `model_close` | `path?` (none closes all models) | `preview.close`; announces `modelClosed(Elsewhere)` | `{closed:[path], open:n}` | `model_stat` (canonical path) |
| `preview_show` | `path`, `title?` | `preview_inspect` decides the kind: a model goes through `openModel`, anything else to `preview.show({kind,mime,size,…})`; wake; announce | `{opened, kind, name, size, path, mime, title, workspace, visible, woke?, note?}` | `preview_inspect`, `model_allow_roots` |
| `preview_close` | `path?` | closes one tab or all | `{closed, open}` | `preview_inspect` |
| `kit_tab_list` | caller only; `workspace` and `pane` are ignored | none | `{workspace, tabs: kitTabListing(withEffectiveKits(ws), kits)}` | `kits_list` |
| `kit_tab_open` | `kit`, `tab`, `params?`, `reveal` (bool, default true) | `kitTabToOpen` + `readKitTabParams`; `preview.show({kind:"kit", path: kitTabPath, singleton, reveal})`; announce only when revealed | `{outcome: opened\|replaced\|focused, kit, tab, params, workspace, ...visible/note or sight}` | `kits_list` |
| `kit_tab_close` | `kit`, `tab`, `params?` | closes the matching tabs; the kit is not consulted | `{closed:[params], open:[params]}` | none |
| `project_*` | | handled by managerControl | | |
| `worker_*` | | handled by workerControl, after `workspaceFor` | | |
| `delivery_*` | | handled by deliveryControl | | |

- **ControlEvent** (sent to `announce`): `created|closed|restarted|reset|workspaceCreated {by,pane,workspace}`, `checkSet {by,workspace,command}`, `modelShown|modelClosed|modelShownElsewhere|modelClosedElsewhere {by,file,workspace}`.
- **Pure helpers to port first:** `resolvePath` (folds `.`/`..`, keeps UNC and drive letters), `comparable` (strips `\\?\`), `callerWorkspace`, `modelRoots`, `keySequence`.

#### 1.2 managerControl.ts (`project_*`, the manager pane only: `pane.role === "manager"`)

| Op | Args | Effect | Reply |
|---|---|---|---|
| `project_list` | none | `loadProjects` (every non-HQ board, memory, git, activity, asleep, journal) | `{projects: ProjectSummary[], text: renderOverview(projects, now)}` |
| `project_status` | `workspace` (id, name or cwd; the HQ is refused) | as above | `{project, text: renderProject}` |
| `project_send` | `workspace`, `text` | needs an orchestrator; `touch`; `enqueue(orchestrator, "Relayed by the project manager from the person: " + text, from = manager name)`; opens the workspace if not open | `{queued:true, workspace, orchestrator, woke?\|starting?, note}` |

#### 1.3 workerControl.ts (`worker_*`, the workspace's orchestrator only, checked by `orchestratorCalling`)

- **`WorkerDeps`** and the commands behind them:
  - `updatePane`: model write
  - `killPane`: `pty_kill {id, tree:true}`
  - `boardLoad`: `board_load`
  - `claim`: `board_claim_worker {cwd,id,worker,maxAttempts,sharedFolder}`
  - `release`: `board_release_worker {…,undo}`
  - `repoInfo`: `git_repo_info`
  - `worktreeAdd`: `git_worktree_add`
  - `worktreeRemove`: `git_worktree_remove`
  - `worktreeHook`: `git_worktree_hook`
  - `branchDelete`: `git_branch_delete`
  - `copyIgnored`: `git_copy_untracked {ignoredOnly:true}`
  - `changes`: `git_changes {base:null}`
  - `commitsAhead`: `git_commits_ahead`
- **`worker_spawn {task, name?, model?}` → `spawnWorker`**, all or nothing:
  1. Checks: the bridge is on (`claudeBridge`); running + starting < `maxWorkers`; under `MAX_PANES`; model name matches `/^[\w.:[\]-]{1,80}$/`; name is unique across workspaces and in-flight starts; with no name, `pickPaneName` avoiding names still in the orchestrator journal.
  2. Ports: `allocatePortBase`, a block of 20 from 21000..26999 in steps of 20.
  3. Reserves the name, ports and slot in the module-level `starting` sets.
  4. `repoInfo`; the target branch is `settings.branch ?? repo.branch` (refused on a detached HEAD).
  5. `claim`.
  6. Worktree: branch `hangar/<taskSlug≤32>-<id6>`, 9 branch names × 20 folders (`worktreeFolder`); on a subfolder workspace, the same subfolder inside the worktree.
  7. `copyIgnored(COPY_PATTERNS)` and the `postCreate` hook. Neither undoes the start on failure; a hook failure goes into the brief.
  8. `addPane(claude, cwd, near: last worker or orchestrator)` plus, in the same tick, `updatePane{role:"worker", taskId, model, portBase, sharedFolder?}`.
  9. `touch`, `openWorkspace`, enqueue `workerBrief(...)` from the orchestrator's name (or `"hangar"`), announce `created`, `postOrchestratorEvent(workerStarted)`.
  10. On failure, undo in reverse: `release(undo=true)`; remove the worktree (6 tries, 500 ms apart) and the branch; `killPane` + `closePane` + 400 ms.

  Reply `SpawnedWorker {worker, task{id,title,attempt,maxAttempts}, model, folder, branch, base, worktree, ports{base,count:20}, brief:"queued…", note}`.
- **`worker_list`** → `{workspace, maxWorkers, maxAttempts, workers:[{name, task{id,title,column,attempts}|{id,missing:true}|null, activity, running, branch, worktree, folder?, model, ports, delivery: landed|done|ready|needs-you|review|null}]}`. Uses `board_load`, `git_worktree_list` and `pty_alive`.
- **`worker_stop {worker, keepWorktree?}` → `stopWorker`:** reads the worktrees first (they name the branch); `killPane`; `closePane`; `touch`; `release(undo=false)`. Unless the worker shared the folder, sits in the main checkout, or `keepWorktree` is set: sleep 400 ms, `workKept` (uncommitted changes, `commitsAhead` against the target), `removeWorktree`, `branchDelete`. Then announce `closed` and post `workerStopped`. Reply `{stopped, task{id,title,column}|null, worktree{path,branch,removed,branchDeleted,kept?}}`.
- **Also exported:** `stopAllWorkers` (UI « Arrêter les workers », App.tsx:979), `spawnWorker(by=null)` for the person, `workerBrief`, `taskSlug`, `workerBranch`, `PORT_COUNT=20`.

#### 1.4 deliveryControl.ts (`delivery_*`) and deliveryRuns.ts

- **`DeliveryDeps`** (built by `deliveryDeps(shells)`):
  - `preflight`: `delivery_preflight {workspace,worktree,target,landing,task?}` → `DeliveryPlan {repo,queue,branch,target,ahead,tested?{base,merge,current}}`
  - `run`: `delivery_run {delivery: DeliveryRequest}` (takes minutes) → `DeliveryOutcome`
  - `cleanup`: `delivery_cleanup {worktree,branch,target,taskId}`
  - `running`: `delivery_running` → `[{taskId,queue,stage,since}]`
  - `accounts`: `accounts_list`
  - `board.load`/`update`/`comment`: `board_load`/`board_update`/`board_comment`. Update and comment also `emit("board:changed", cwd)` from the frontend, because these writes skip the HTTP API.
  - `tell(pane, text)`: `bridge_enqueue(from:"hangar")`
- **Module state:** `inFlight: Map<taskId, {workspaceId, worker, since, landing}>`; `settled: Map<taskId, {outcome, detail, at}>`.
- **`delivery_submit {summary?}`** (worker pane with a task):
  - Refused while in flight; checks the column (`doing`, or `review` + ready-to-land).
  - A shared folder takes `doneInFolder`: board → done, a comment, a `delivery: done` event, the pane closed after 5 s → `{submitted, done, task, next}`.
  - Runs `preflight(task)`. Ready-to-land with an unchanged branch → `waitsAlready` → `{submitted:false, waiting:true, task, branch, target, testedAs, next}`. Ready-to-land with new commits → back to doing, label removed, comment.
  - Starts `deliver()` without awaiting it → `{submitted:true, task, branch, target, commits, queuedBehind?, replaces?, next}`.
- **`deliver`**: `launchAccount` + `resolveShell` build the `DeliveryRequest {workspace, worktree, branch, target, task{id,title,description,owns}, summary, checkCommand, shell, reviewerModel, claudeAccount, land: landing||autoMerge, reuse: landing, portBase}`; then `run`; then `settle`.
- **`settle` by outcome:**
  - `landed`: board → done, worker pane closed, `cleanup`, comment "Landed on …", event.
  - `ready`, or `dirty` in the checkout: column review + `ready-to-land`, comment "Passed: waits for the person…", the worker is told, event.
  - `stale`: column doing, comment, the worker is told, event.
  - `error` containing "has reached its usage limit": comment "Review not run (usage limit)", the worker is told; it does not count as a failure.
  - Any other failure: comment `Delivery failed (<outcome>, n of 3): …`; the third failure adds `needs-you` and a `taskNeedsYou` event; the worker gets the detail.
- **`delivery_status`** → `{deliveries:[{worker, task, state (stage|running|outcome|none), since?, detail?}]}`.
- **Also exported:** `landDelivery(ws, taskId, deps)` for « Fusionner » (App.tsx:997) and `remote_land`; `deliveryState`; `deliveringIn(ws)`, which hibernation reads.
- **Coupling to keep:** remoteDelivery parses these comment strings with regexes (READY_HEAD, DIRTY_HEAD, CHECK_*, REVIEW, FAILED), and boards on disk already carry them. Keep them as shared constants, byte-identical.
- **deliveryRuns.ts:** `Map<wsId, Set<taskId>>` plus `beginDelivery() → end()`, `isDelivering`, and the `useDelivering` hook (useSyncExternalStore) behind the « Fusionner » spinner. In native this is UI state in the model.

#### 1.5 orchestratorEvents.ts: per-workspace event engine (main window only, needs an orchestrator)

- **`OrchestratorEvent` kinds:**
  - `workerStarted|workerStopped|workerExited {worker, taskId}`
  - `workerWaiting|workerIdle {worker, taskId, forMs}`
  - `taskMoved {taskId,title,from,to,by}`
  - `taskCommented {…,by,text}`
  - `taskNeedsYou {…,reason}`
  - `taskBlocked {…,by,reason,humanOnly}`
  - `taskUnblocked {…,how}`
  - `taskRecheckDue {…,reason}`
  - `delivery {taskId,title,worker,outcome: landed|done|ready|conflict|checkFailed|changesRequested|dirty|stale|error, detail}`
- **Inputs:**
  - `post(ws, event, quiet)`, from workerControl and deliveryControl. A worker start or stop during the orchestrator's own turn is quiet.
  - `boardChanged()`, on `board:changed`; reads after a 250 ms debounce.
  - `bridgeEvent(pane, ev)` on `bridge:event`: `hello`, `turn` start/end and `permission` set the turn state; `end` means the worker exited.
  - `ptyExited(pane)` on `pty:exit {id}`.
  - `tick()` every 10 s (`useOrchestrator`): read the boards, then `watchWorkers`. Waiting ≥ 2 min gives `workerWaiting`; ended its turn and idle ≥ 10 min while the task is still in doing and undelivered gives `workerIdle`; a pane gone with no worker of that name gives `workerExited`.
- **Pure functions:** `diffBoard(snap, tasks, {name,busy})` (moves, comments not by the orchestrator or "hangar", block, unblock, recheck, needs-you), `settle(batch, journal, live)` (drops stale items, folds, ranks), `renderDelta(events, tasks, ws)` (≤12 lines + "Ready to start: …"), `readyTasks`, `describeEvent`, `oneLine`, `taskRef`, `snapBoard`.
- **Timers (`TIMING`):** debounce 5 s; max wait 30 s; min gap 15 s between messages; hold 3 s while the orchestrator is busy (in a turn or waiting); `nearMs` 60 s.
- **State per workspace:** journal (last 50), pending (≤200), board snapshot, tasks, timer, holding, reading/readAgain. Also `workers: Map<paneId, WorkerWatch>` and `turns`.
- **Output:** `enqueue(orchestrator, renderDelta, "hangar")`.
- **Singleton API:** `startOrchestratorEvents`, `postOrchestratorEvent`, `orchestratorJournal(ws)`. The journal feeds the briefing, `worker_spawn` names, project summaries and the activity log.

#### 1.6 managerEvents.ts: manager feed (non-HQ workspaces; runs only while a manager runs or a phone is subscribed)

- **`ManagerEvent`:** `paneWaiting {ws, pane, forMs}`, `taskNeedsYou {…,reason}`, `readyToLand`, `boardDone {tasks}`.
- **`BlockEvent`** (manager only, never pushed): `taskBlocked`, `taskUnblocked`, `taskRecheckDue`, for human-only blocks.
- **Detection:**
  - Board diffs: needs-you label added, ready-to-land label added, the board turning all done, human-only block changes. The first read is the baseline.
  - Any non-manager pane `waiting` ≥ 2 min.
- **Batching:** same `TIMING` as 1.5. Held while the manager is `working` or `waiting`; waits that ended are dropped; output `enqueue(manager, renderManagerDelta, "hangar")`.
- **Push sink:** each `ManagerEvent` goes at once to `push.sink.post` (block events excepted).
- **Journal:** last 50; `managerJournal()` (without block events, used by the briefing and remote_state) and `managerNews()` (used by the activity log).

#### 1.7 orchestratorBrief.ts

- **Answers `orchestrator_briefing {paneId}`:** `briefingFor` → `Briefing {role: orchestrator|worker|manager, system, context}`, or `null` when the pane has no role or is not Claude.
  - Reads `board_load` (not for the manager), `memory_load` (orchestrator or manager), `git_repo_info` (orchestrator: decides the git or no-git role text), and `loadProjects` plus the manager journal (manager).
- **Texts (port verbatim, roughly 250 lines of prompts):** `orchestratorRole(ws, planTitle, git)`, `managerRole()`, `workerRole(ws, taskRef, orchestrator, sharedFolder)`.
  - Context sections: settings line, workers, board (≤25 open tasks, ≤10 done titles), plan (memory titled `Plan — <ws>` or `Plan — <ws> (<cwd>)`, tagged `orchestration`, ≤4000 chars), tracker (`Projects — tracker`, tag `manager`), events (last 20).
  - Helpers: `planTitle`, `findPlan`, `findTracker`.
- **`announceRole(ws, pane, previous)`:** called by the UI on pin and unpin. If the pane is bridged, it enqueues `roleMessage` or `releaseMessage` from `"hangar"`.
- **The `useOrchestrator(snapshot)` hook** (App.tsx:1041) is the runtime root:
  - starts the event engine, `startRemotePush`, and `startManagerEvents(push: push.sink)`;
  - listens to `board:changed` (both engines), `bridge:event`, `pty:exit` and `control:internal`;
  - every 10 s runs `engine.tick()`, then `push.refresh().then(manager.tick)`.

#### 1.8 remoteControl.ts (`remote_*`, answered as the person)

- **Module state:**
  - `firstSeen` (when a waiting item was first seen);
  - `resuming: Map<pane, since>` (Claude panes woken from the phone, cleared by any bridge event or after `RESUME_MS` 120 s);
  - `live: Map<pane, {calls, permission}>` and `planLimits`, both fed by `noteBridgeEvent` on `bridge:event`;
  - `answering: Set<pane>`;
  - `worktreesSeen` (5-minute cache);
  - `usageReader` (fresh 60 s; waits 6 s for a scan, else falls back to `cachedOnly`).
- **Shared helpers:**
  - `type()` uses `runControl("pane_send")`. A pane that is not up but resuming has its text held: `bridge_enqueue(pane, text)` with no `from`, so it reads as the person's words, then a wake. A pane with no terminal: wake, then poll `terminal && alive` every 250 ms for up to 8 s.
  - `wake()` = `openWorkspace` + `touch` + mark the Claude panes as resuming. The bridge queue is used only when `settings.claudeBridge !== false`.

| Op | Args | Does | Reply | Backend commands |
|---|---|---|---|---|
| `remote_state` | none | `loadProjects`; the waiting list (panes `waiting`, plus needs-you and ready-to-land from summaries, `since` from the manager journal or first seen); manager | `RemoteState` | `board_load`, `memory_load`, `git_repo_info` |
| `remote_screen` | `pane` (id or name), `lines=80` | `pane_read`; empty text when asleep or closed | `RemoteScreen` | none |
| `remote_send` | `pane`, `text?`, `keys?` (REMOTE_KEYS), `submit?` | `type()` | `{sent:true, queued?}` | `pty_write`, `bridge_enqueue` |
| `remote_permission` | `pane`, `allow` (bool) | refused unless the pane is `waiting` (an asleep workspace is woken, then refused); sends the key `enter` or `escape` | `{answered:true}` | `pty_write` |
| `remote_manager` | `text` | `type()` into the manager pane; "Open the project manager on the PC" when there is none | `{sent:true}` | as above |
| `remote_history` | `since?` | `activityHistory.read`: a log tick, then `activity_read`; `textOf` in the PC's locale; newest first | `{entries}` | `activity_read` |
| `remote_chat` | `pane`, `before?`, `limit?` | non-Claude pane or no session gives `reason`; `chat_page {sessionId, cwd, before?, limit?}`; `pendingOf` (live question, else page question, else live permission via `describeCall`, else `openCall`); running live calls are appended on the newest page | `RemoteChat` | `chat_page` |
| `remote_answer` | `pane`, `toolUseId`, `answers[]` | `pendingQuestions` + `choicesOf`. A resuming pane gets `answerText` held in the queue. Otherwise `typeAnswers` through `pane_read(120)` and `pane_send(submit:false)` | `{answered:true, queued?}` | `chat_page`, `pty_write` |
| `remote_wake` | `workspace` | `wake()` if asleep | `{waking}` | none |
| `remote_workspace` | `workspace?` \| `folder?` | lookup; internal, used by the upload route | `{id,name,cwd}` | none |
| `remote_stats` | `period` (7d/30d/90d/all, default 30d), `measure` (tokens/cost) | `usage_stats` + worktrees per workspace, then `statsModel.summarize`, then `remoteStatsOf` (+ plan windows) | `RemoteStats` | `usage_stats`, `git_worktree_list` |
| `remote_apps` | `workspace?` | remoteApps | `RemoteApps` | |
| `remote_delivery`, `remote_delivery_file`, `remote_land`, `remote_rework` | | remoteDelivery | | |

#### 1.9 remoteAnswer, remoteDelivery, remoteApps, remotePush, activityLog

- **remoteAnswer.ts:** pure, apart from the injected I/O `{read, type, sleep}`.
  - `pendingQuestions`; `choicesOf` (validates labels, single versus multi select, cleans "other" text); `stepsFor` (down and enter key sequences); screen parsers `dialogOf`, `untouched`, `chosen`, `reviewed` (Claude Code 2.1.x dialog: `☐/☒ header` tabs, `❯ 1.` rows, "Type something", "Review your answers", "Submit answers").
  - `typeAnswers` checks the screen before each step (150 ms × 20 tries, 3 tries for the first) and never submits unverified answers.
- **remoteDelivery.ts:** `deliveryHistory(comments)` parses the hangar comments into the passed delivery and the earlier failures. `waiting()` finds the worker that holds the task and checks the ready-to-land label.

  | Op | Args | Does | Reply | Backend commands |
  |---|---|---|---|---|
  | `remote_delivery` | `task` | preflight(landing) + `git_changes(worktree, target)` (untracked left out) + stage | `RemoteDelivery` | `delivery_preflight`, `git_changes`, `delivery_running` |
  | `remote_delivery_file` | `task`, `file` | `git_file_diff`, `capPatch` 200 KB | `RemoteDeliveryPatch` | `git_file_diff` |
  | `remote_land` | `task`, `merge?` | `merge` must equal the merge shown (`sameAs`). `startLanding` = `beginDelivery` + `landDelivery`; waits up to 7 s; else `{landed:false, stage}`, and a repeat call waits on the same landing. Outcomes are kept 5 min | `RemoteLanded` | via deliveryControl |
  | `remote_rework` | `task`, `text`, `merge?` | refused while a stage runs; `toWorker` (queue + wake); `board_comment(author:"user")` | `{sent:true}` | `bridge_enqueue`, `board_comment` |

- **remoteApps.ts:** `candidatesOf`: each worker's 20-port block, then the `mobilePorts` setting (`appPortsOf`), HQ excluded. Ports are probed with `remote_ports_listening {ports}` in batches of 256; task titles come from `board_load`. Reply `{apps:[{workspaceId, workspace, port, source: worker|custom, pane?, task?}]}`.
- **remotePush.ts:**
  - Items: `ManagerEvent`, or `{kind:"shared", share}` from `remote:shared`.
  - Slot per `(kind, ws)`: gather 3 s, gap 30 s, ≤50 pending. `settlePush` drops ended waits; `pushMessage` builds en and fr text through `typeset`. URLs: `./#/`, `./#/shares`, `./#/pane/<id>`, `./#/projects/<id>`.
  - Active only while subscribers > 0; `refresh()` calls `remote_push_subscribers` on `remote:changed` and on every 10 s tick.
  - Output: `invoke("remote_push", {title:{en,fr}, body:{en,fr}, url})`.
  - Pure: `pushMessage`, `settlePush`, `workspaceOfShare`.
- **activityLog.ts:**
  - Tick every 5 s. Reads `managerNews` and each workspace's `orchestratorJournal`; adds asleep and awake transitions (`isAsleep`, after the first baseline), "paneWaited" when a wait ends (with its duration), and `remote:shared`.
  - Same-task asks within 10 min are deduplicated; the HQ is skipped; text uses `activity.*` i18n keys in the PC's locale.
  - Writes `invoke("activity_append", {entries:[{at,workspace,kind,text}]})` serially, keeping up to 200 unwritten lines to retry.
  - Writes `appStarted` at start. `activityHistory.read(since)` ticks, then `activity_read {since}`.
  - Started by `useRemoteControl` once the store has loaded.

#### 1.10 Frontend events and commands used by these modules

- **listen:**
  - `control:request {id,op,args}` and `control:internal {id,op,args}`
  - `board:changed` (cwd string)
  - `bridge:event {paneId, event: BridgeEvent}`: hello, session, turn start/end, permission{waiting,tool,input}, tool start/end{toolUseId,tool,input}, usage{rateLimits}, end
  - `pty:exit {id}`
  - `remote:shared` (`SharedFile {id,at,workspace(folder),pane,name,mime,size,caption}`)
  - `remote:changed` (no payload)
- **emit (frontend):** `board:changed` (deliveryControl).
- **invoke:**
  - control: `control_reply`
  - board: `board_load`, `board_update`, `board_comment`, `board_claim_worker`, `board_release_worker`
  - git: `git_repo_info`, `git_worktree_list`, `git_worktree_add`, `git_worktree_remove`, `git_worktree_hook`, `git_branch_delete`, `git_copy_untracked`, `git_changes`, `git_commits_ahead`, `git_file_diff`
  - delivery: `delivery_preflight`, `delivery_run`, `delivery_cleanup`, `delivery_running`
  - processes and bridge: `pty_kill`, `pty_write`, `pty_alive`, `bridge_enqueue`, `dir_exists`
  - preview and kits: `model_allow_roots`, `model_stat`, `preview_inspect`, `kits_list`
  - other core: `accounts_list`, `memory_load`, `usage_stats`, `chat_page`
  - remote: `remote_ports_listening`, `remote_push`, `remote_push_subscribers`, `activity_append`, `activity_read`
  - windows: `open_workspace_window`, `close_workspace_window`
  - workspace creation: `git_repo_parse`, `git_repo_ownership`, `git_clone`, `git_clone_discard`, `claude_trust_folder`
- **Window-to-window events that vanish in a single process:** `model:ask`, `model:answer`, `model:claim`, `model:handover` (modelHost), and the `remoteTerminal` relay (lib/windows).

---

### 2. Hangar Mobile contract (what the desktop must keep serving)

The phone app stays a web app (React PWA, `src/mobile/*`, not ported). The desktop must keep building it, embedding it, serving it, and answering the `remote_*` ops from the native model.

| Route | Auth | Who answers | Payload, reply |
|---|---|---|---|
| `GET /`, `/*` (not `/m/`) | open | backend `assets::respond`: embedded `dist-mobile/`; SPA fallback to `index.html`; a placeholder page when the build is missing | static files |
| `GET /m/api/health` | open | backend | `{ok, pc, version, paired}` |
| `POST /m/api/pair` | open, `X-Hangar` | backend (`pairing.rs`; the code comes from desktop Settings `remote_pair_start`) | `{code, device}` → Set-Cookie `hangar_device` (HttpOnly, Secure, SameSite=Strict, 1 year) + `{device:{id,name}}`; 401 `wrong-code {left}`, 410 `expired` |
| `POST /m/api/call/{op}` (body ≤64 KB) | cookie + `X-Hangar` + Sec-Fetch-Site same-origin or none | **window**: `remote_<op>`, 10 s | ops `state`, `screen`, `send`, `permission`, `manager`, `history`, `chat`, `answer`, `wake`, `stats`, `apps`, `delivery`, `delivery_file`, `land`, `rework`. 200 = answer; 400 `{error:"refused", message}`; 503 `pc-busy`; 404 `unknown-op` |
| `POST /m/api/app/open` `{workspaceId, port}` | cookie + header | backend `apps::open`, which first asks the **window** `remote_apps` to see whether the port is allowed, then `tailscale serve` on a slot (8443–8448) and `proxy.rs` | `{url, slot}`; 403 not-allowed, 409 not-listening, 400 refused, 503 tailscale / no-free-slot / pc-busy |
| `GET /m/api/shares?since=` | cookie | backend (`files::Shares`) | `{shares:[Share]}` |
| `GET /m/files/{id}` (Range, HEAD) | cookie | backend | bytes, with CSP and disposition per kind |
| `POST /m/api/upload?workspace=&name=` (≤25 MB) | cookie + header | backend, after asking the **window** `remote_workspace` for the folder | `{path}` |
| `GET /m/api/push/key` | cookie | backend (VAPID key) | `{key}` |
| `POST` / `DELETE /m/api/push/subscribe` `{subscription, lang}` | cookie + header | backend | `{subscribed}` |
| Slot proxies `127.0.0.1:<slot>` | device cookie | backend `proxy.rs` | relayed to the dev server |

- **Phone client** (`src/mobile/api.ts`):
  - `createClient` uses timeout 12 s (40 s for openApp) and retries a `pc-busy` once on `send`, `permission`, `manager`, `answer`, `stats`, `delivery_file`, `land`, `rework` and `wake`.
  - Every 401 means pair again.
- **Polling cadence (each paired phone):**

  | Call | Interval |
  |---|---|
  | `state` | 2 s |
  | `chat` | 1 s (5 s when empty) |
  | `screen` | 1 s |
  | land poll | 1.5 s |
  | `delivery` | 5 s |
  | app viewer `apps` | 5 s |
  | `apps` (while shown) | 10 s |
  | `shares` | 15 s |
  | `history` (ForYou) | 30 s |
  | `stats` | 60 s |

  Rust consequence: `remote_state` runs `loadProjects`, which reads every board, memory and git each time. Cache it, or drive it from board-change events.
- **Push, agent to phone:** `POST /api/phone/share` (agents' API, server.rs:625) → `remote.share`, which fires the `shared` closure (`remote:shared` today). The window decides what to push (`remotePush`) and calls `remote_push {title{en,fr}, body{en,fr}, url}`. push.rs then sends Web Push (VAPID, urgency high, payload `{title, body, url}` in the phone's language) to each subscription and drops dead ones (404/410). `remote_push_subscribers` returns the count.
  - In native, `remote::Window` is already three closures (`send`, `changed`, `shared`). Plug them into the app's channels; no event names are needed.
- **Service worker** (`src/mobile/sw.js`):
  - Caches the app shell (SHELL list and VERSION injected at build); never caches `/m/`.
  - `push` shows a notification with icon-192 and `data.url`.
  - `notificationclick` focuses or opens the app and posts `{type:"hangar:open", url}`.
  - `pushsubscriptionchange` subscribes again through `/m/api/push/key` and `/m/api/push/subscribe`.
- **Assets:**
  - `vite.mobile.config.ts` builds `mobile.html` + `src/mobile/main.tsx`, with public files from `public/mobile/` (icons, `manifest.webmanifest`), into `<repo>/dist-mobile/`. `mobile.html` is renamed `index.html`, and `sw.js` is written with the shell list and a sha256 version. Script: `pnpm mobile:build`; dev server on port 4310 against the mock.
  - Embedding: `crates/hangar-core/src/remote/assets.rs` uses `#[derive(RustEmbed)] #[folder = "../dist-mobile/"] #[allow_missing = true]`.
- **Desktop-only commands (Settings › Mobile, outside this inventory):** `remote_status`, `remote_set_enabled`, `remote_pair_start`, `remote_devices`, `remote_revoke`, `remote_tailscale`, `remote_tailscale_serve`, `remote_app_slots`, `remote_options`, `remote_set_keep_awake`, `remote_set_autostart`.

**`src/remote/types.ts`: the shared JSON contract** (server, phone app and window; the TS file comment says to change it only with a note on the task that owns the contract). Port it as serde structs with camelCase fields, optional fields skipped when absent. Proposed home: `hangar-core/src/remote/contract.rs`.

| Group | Types |
|---|---|
| Basics | `RemoteActivity` = working, waiting, yours, idle or null. `RemoteKey` / `REMOTE_KEYS` = enter, escape, tab, up, down, ctrl+c |
| `remote_state` | `RemoteState {at, manager{paneId,name,activity}\|null, waiting: RemoteWaiting[], projects: RemoteProject[]}`. `RemoteWaiting {kind: pane\|needsYou\|readyToLand, workspaceId, workspace, paneId?, pane?, taskId?, title?, since}`. `RemoteProject {id, name, open, asleep, git, orchestrator{name,activity}\|null, panes: RemotePane{id,name,agent,role,activity}[], board{todo,doing,review,done}, planStatus, needsYou: RemoteTask[], readyToLand}` |
| Screen and input | `RemoteScreen {paneId, pane, workspace, agent, activity, text}`, `RemoteSent {sent, queued?}`, `RemoteAnswered {answered, queued?}`, `RemoteWoken {waking}` |
| History | `RemoteHistoryEntry {at, workspace, kind, text}` |
| Chat | `RemoteChat {paneId, pane, workspace, agent, activity, sessionId, items: ChatItem[], older, cursor, pending: ChatPending\|null, reason?: notClaude\|noSession\|noTranscript}`. `ChatItem = {id, at}` & one of: user{text, from: person\|hangar, images}, assistant{text}, action{tool, title, detail?, result?, state: running\|done\|error}, question{toolUseId, questions, answer?}, notice{text}. `ChatQuestion {question, header, multiSelect, options[{label, description}]}`, `ChatAnswer {selected[], other?}`, `ChatPending` = question{toolUseId, questions} or permission{tool, title, detail?} |
| Delivery | `RemoteDelivery {taskId, title, workspace, workspaceId, worker, branch, target, summary, check{command,passed,seconds}, review{approved,notes,blocking[{file,line?,problem}]}, files[{path,oldPath?,status: added\|modified\|deleted\|renamed, additions, deletions, binary?}], moreFiles?, waiting, stage: queued\|merging\|checking\|reviewing\|landing\|null, merge, passedAt, moved, inTheWay, earlier[{at,outcome,text,blocking?}]}`. `RemoteDeliveryPatch {path, patch, binary, truncated}`. `RemoteLanded` = {landed:true, target} or {landed:false, stage}. `RemoteReworked {sent}` |
| Apps | `RemoteApp {workspaceId, workspace, port, source: worker\|custom, pane?, task?}`, `RemoteApps {apps}`, `RemoteAppOpened {url, slot}` |
| Stats | `RemoteStats {period, measure, readAt, cachedOnly, range{from,to,days}, granularity: day\|week, firstAt, totals, previous, activeDays, series[{at,value}], projects/agents/models: Ranking{rows≤6, other}, unpricedTokens, plan{at, windows[{kind, percentUsed, resetsAt}]}\|null}` |
| Upload | `RemoteWorkspace {id, name, cwd}` |

---

### 3. Logic libraries in scope

Tests are vitest; the case count is `grep -cE "^\s*(it|test)\("`. In scope: 8,723 source LOC and 8,374 test LOC, 414 cases.

| Module | LOC | Test file (LOC, cases) | Computes / exports | Kind | Port target |
|---|---|---|---|---|---|
| lib/control.ts | 1386 | control.test.ts (1920, 120) | `runControl`, `usePaneControl`, `callerWorkspace`, `resolvePath`, `modelRoots`, `ControlDeps`, `ControlEvent`, `DISABLED` | IO via deps + React hook; pure helpers | app `control/{mod,panes,preview,kit_tabs,workspace}.rs` |
| lib/managerControl.ts | 116 | (172, 9) | `managerControl`, `RELAY_PREFIX` | IO via deps | app `control/manager.rs` |
| lib/workerControl.ts | 922 | (882, 35) | `workerControl`, `spawnWorker`, `stopWorker`, `stopAllWorkers`, `listWorkers`, `allocatePortBase`, `taskSlug`, `workerBranch`, `workerBrief`, `liveWorkerDeps`, `PORT_COUNT` | IO + module state; pure helpers | app `orchestration/worker.rs`; brief and slug text could live in core |
| lib/deliveryControl.ts | 822 | (761, 25) | `deliveryControl`, `landDelivery`, `deliveryState`, `deliveringIn`, `deliveryDeps`, outcome and plan types, `HANGAR`, `MAX_FAILED_DELIVERIES` | IO + module state | app `control/delivery.rs`; comment strings as consts shared with remote_delivery |
| lib/deliveryRuns.ts | 48 | none | `beginDelivery`, `isDelivering`, `useDelivering` | state + React hook | app model field + `control/delivery.rs` |
| lib/orchestratorBrief.ts | 793 | (438, 18) | `briefingFor`, `buildBriefing`, `orchestratorRole`, `managerRole`, `workerRole`, `roleMessage`, `releaseMessage`, `announceRole`, `planTitle`, `findPlan`, `findTracker`, `answerInternal`, `useOrchestrator` | pure text + IO + React hook (runtime root) | app `orchestration/brief.rs` + `brief_text.rs`; the runtime root goes to `orchestration/runtime.rs` |
| lib/orchestratorEvents.ts | 888 | (578, 29) | `createOrchestratorEngine`, `settle`, `diffBoard`, `snapBoard`, `renderDelta`, `readyTasks`, `describeEvent`, `TIMING`, `postOrchestratorEvent`, `orchestratorJournal` | pure + timer engine (injectable time) | app `orchestration/events.rs` |
| lib/managerEvents.ts | 421 | (486, 25) | `createManagerFeed`, `settleManager`, `renderManagerDelta`, `describeManagerEvent`, `managerJournal`, `managerNews` | pure + timer engine | app `orchestration/manager_feed.rs` |
| lib/remoteControl.ts | 919 | (1119, 63) | `runRemote`, `answerRemote`, `useRemoteControl`, `noteBridgeEvent`, `usageReader`, `statsSources` | IO + module state + React hook | app `mobile/{mod,ops,chat_live,stats}.rs` |
| lib/remoteAnswer.ts | 299 | (479, 22) | `pendingQuestions`, `choicesOf`, `stepsFor`, `dialogOf`, `untouched`, `chosen`, `reviewed`, `typeAnswers` | pure + injected IO | app `mobile/answer.rs` (no model dependency, could move to core) |
| lib/remoteDelivery.ts | 477 | (501, 17) | `runDelivery`, `deliveryHistory`, `capPatch`, `DELIVERY_OPS` | pure parsing + IO + state | app `mobile/delivery.rs` |
| lib/remotePush.ts | 341 | (489, 20) | `createRemotePush`, `pushMessage`, `settlePush`, `workspaceOfShare`, `startRemotePush`, `PUSH_TIMING` | pure + timer engine + IO | app `mobile/push.rs`; message text could join core `remote/push.rs` |
| lib/remoteApps.ts | 143 | (193, 14) | `remoteApps`, `candidatesOf`, `runApps` | pure + IO | app `mobile/apps.rs` |
| lib/activityLog.ts | 347 | (274, 14) | `createActivityLog`, `startActivityLog`, `activityHistory`, `textOf`, `lasted`, `ACTIVITY_KINDS` | timer engine + IO | app `mobile/activity_log.rs` |
| remote/types.ts | 571 | none | the JSON contract (section 2) | types only | **core** `remote/contract.rs`, used by server.rs and apps.rs |
| remote/calls.ts | 136 | calls.test.ts (82, 3) | `describeCall`, `askedQuestions` | pure | **core** `chat/calls.rs`, beside `chat/tools.rs`, whose titling it mirrors |
| remote/stats.ts | 94 | none | `remoteStatsOf`, `ranking`, `planWindows` | pure | core `stats/remote_view.rs` or app `mobile/stats.rs` (needs the statsModel port) |
| mobile/{api,remote,poll,push,apps,delivery,chat}.ts, sw.js | 445/248/62/108/105/175/424/150 | 35/–/6/9/25/12/35/5 cases | phone-side client and UI logic | web | **not ported**; keep it building into `dist-mobile` |

**Out-of-scope dependencies these modules need** (from other packages; stub them behind traits):
- `src/types.ts` model types: AppState, Workspace, Pane, Settings
- `lib/layout` (`leafIds`, `normalizeTree`, `MAX_PANES`)
- `lib/orchestration.ts`: `orchestrationOf`, `orchestratorOf`, `workersOf`, `managerOf`, labels `needs-you` and `ready-to-land`
- `lib/projects.ts` (467 LOC): `loadProjects`, `renderOverview`, `renderProject`; needed by WP1, WP2 and WP3, assigned to WP2
- `lib/workspaceFromGit.ts` (746)
- `lib/kitTabs.ts` (477) and `kits.ts`
- modelHost / modelStore / preview (the `PreviewHost` trait)
- `lib/statsModel.ts` (576): `summarize`, `projectFor`, `workspaceRoots`
- `agentState` (`PaneActivity {workspaceId, activity, bridged}`)
- hibernateStore (`isAsleep`, `touchWorkspace`, `carryResume`)
- `paneNames`, `worktrees.ts` (`sanitizeBranchName`, `worktreeFolder`, `COPY_PATTERNS`), `shells.resolveShell`, `accounts.launchAccount`
- i18n (`control.*`, `activity.*`), `typography.typeset`

---

---

## 6. i18n (`src/i18n.ts`, `src/strings/*`, accounts strings)

### 6.1 Structure today

- **`src/i18n.ts`** (3,182 LOC):
  - `const en = {...} as const` holds **1,377** keys (lines 32–1541), then spreads five strings modules into it;
  - `const fr: Record<Key, string>` (line 1584) must cover every `en` key, or the build fails;
  - `DICTIONARIES = {en: typesetAll(en), fr: typesetAll(fr)}` is typeset **once at load**.
- **Merged modules** (keys use the same flat namespace):

  | Module | en keys |
  |---|---|
  | `strings/experiments.ts` | 6 |
  | `strings/kitsManage.ts` | 185 |
  | `strings/kitsTab.ts` | 51 |
  | `strings/worktrees.ts` | 53 |
  | `strings/workspaceFromGit.ts` | 37 |

  **Total: 1,709 en keys** (= 1,709 fr).
- **Separate dictionaries** (not merged):
  - `strings/paneBranch.ts`: 4 keys, with its own formatter `paneBranchText`.
  - `components/accounts/strings.ts`: **58** keys with its own `translateAccounts`/`useAccountsT`. Its key names collide with the main namespace: `settings.title`, `pane.menuHeading`, and others.
  - `workspaceFromGitText(locale, key)`: used by the consent card **in the system locale**, not the setting.
- **Key prefixes in `i18n.ts`** (count):

  | Prefixes |
  |---|
  | settings 146 · stats 99 · worktrees 78 · inspector 78 · board 77 · orch 69 · cmd 64 · mobile 63 · mcpScope 58 · pane 56 · model 54 · preview 52 · sidebar 50 · memory 39 · diff 33 · create 33 · keymap 30 · toolbar 28 · commands 23 · needsYou 21 · task 20 · sleep 19 |
  | mcp 16 · discord 16 · activity 16 · topbar 14 · palette 14 · sessions 11 · control 11 · window 10 · status 9 · search 9 · update 8 · manager 8 · zoom 6 · notify 6 · view 5 · broadcast 5 · voice 4 · hibernate 4 · editor 4 · kits 3 · placeholder 2 · grid 2 · drop 2 · modal 1 · app 1 |

  The strings modules add `kitsManage.*`, `kitsTab.*`, `worktrees.*` and `create.*`/`gitConsent.*`/`settings.projectsFolder*`.
- **Plurals:** **122** `_one`/`_other` pairs, and no other forms.
  - `translate(locale, key, params)` picks `${key}_${Intl.PluralRules(locale).select(params.n)}` when `params.n` is a number, falling back to `${key}_other`.
  - French `one` covers 0 and 1. CLDR French also has `many` (1,000,000…), which falls back to `_other`.
- **Placeholders:** `{name}`, replaced only when present in `params`.
  - Numbers are formatted with `Intl.NumberFormat(locale)`: `10,000` in en, `10 000` (narrow no-break space) in fr.
  - Strings pass through untouched; placeholder **values are never typeset**.
- **Fallback chain:** `dict[locale][key]` → `en[key]` → the key itself.
- **Locale:** `settings.locale ?? systemLocale()`, where `navigator.language` starting with `fr` → fr, else en. Native: the `sys-locale` crate.
- **Typography** (`lib/typography.ts`):
  - `bindUnits`: NBSP between a digit or `}` and a unit `min|ms|s|h|j|d|sem` that is not followed by a letter; also after a unit before a digit, and after `< > ≤ ≥ ~`;
  - **fr only** `frenchSpacing`: U+202F before `? ! ; : » %` and after `«`.
  - `i18n.test.ts` (8 cases) holds fr to it, checks key parity and checks placeholder parity.
- **Dynamic keys:** 58 template-literal `t(\`…${x}\`)` calls. Their families:
  - `pane.state.*`, `view.*`, `cmd.view.*`, `board.<column>`, `keymap.section.*`, `control.<event.kind>`, `manager.<result.kind>`, `orch.permission.*`;
  - `stats.{chart,kind,measure,measureHint,period,periodHint}.*`, `delivery.{stage,status}.*`, `diff.status.*`, `inspector.status.*`;
  - `experiments.stage.*`, `kitsTab.{badge,permission}.*`, `mcpScope.{scope,scopeHint}.*`, `preview.kind.*`, `action.*`, `activity.*`, `${base}_${form}`.
- **Glossary and voice rules for French** are in the comment at `i18n.ts:1545–1583`. They are binding for any new string.
- **`src/mobile/i18n.ts`** (928) is the phone's own dictionary. It stays in the web app.

### 6.2 Rust shape (P1, new crate `crates/hangar-i18n`)

Decision: a crate of its own rather than a module of `hangar-app`, so P1 owns it outright and `hangar-ui` can use it later.

1. **Export once.** `scripts/i18n-export.mjs` (run with `vite-node`) writes:
   - `crates/hangar-i18n/dict/{en,fr}.json`, flat `{key: template}`, raw (not typeset);
   - `dict/accounts.{en,fr}.json`;
   - `dict/pane_branch.{en,fr}.json`.

   The **accounts keys get the prefix `accounts.`** in Rust (`accounts.settings.title`) to remove the collision. From then on the JSON is the source of truth for the native app.
2. **`build.rs`:**
   - reads `en.json` and checks that `fr.json` has exactly the same keys and the same `{placeholders}` per key (the `i18n.test.ts` rules, moved to build time);
   - generates `pub mod k { pub const SIDEBAR_NEW: Key = Key("sidebar.new"); … }`. Names are SCREAMING_SNAKE of the dotted key, the build fails on a collision, and plural bases get a const without the suffix;
   - **a typo is then a compile error** in every package.
3. **Runtime:**

   ```rust
   pub enum Locale { En, Fr }
   pub struct I18n { locale: Locale, /* typeset at construction, like DICTIONARIES */ }
   impl I18n {
       pub fn new(setting: Option<Locale>) -> Self;                 // None → sys_locale
       pub fn t(&self, k: Key) -> &str;
       pub fn tr(&self, k: Key, args: &[(&str, Arg)]) -> String;  // {name} substitution; Arg::Num formatted per locale
       pub fn tn(&self, k: Key, n: i64, args: &[(&str, Arg)]) -> String; // _one/_other (en: n==1; fr: n==0||n==1)
       pub fn locale(&self) -> Locale;
   }
   pub enum Arg<'a> { Str(&'a str), Num(f64), Int(i64) }
   ```

   Keep it in `Ctx` and rebuild it when `settings.locale` changes.
4. **Dynamic families become exhaustive functions** in the owning package. For example `fn state_key(a: AgentActivity) -> Key { match a { Working => k::PANE_STATE_WORKING, … } }`. Never format a key at runtime.
5. **`hangar_i18n::fmt`** replaces `Intl` for two locales, hand-written (no ICU):
   - numbers with grouping;
   - percent, signed percent;
   - compact `1.2k` / `1,2 k`;
   - currency with `narrowSymbol` (`$0.42`, `0,42 $`);
   - bytes;
   - 24 h clock;
   - short date, weekday names;
   - relative time;
   - `formatSince`, `resetWhen`, `proseTime` (from `inspectorFormat.ts`).

   Port its tests and the `statsModel`/`memoryFormat` expectations.

---

## 7. Styling

### 7.1 Files

| File | LOC | Holds |
|---|---|---|
| `src/styles.css` | 8,361 | `:root` base, buttons and forms, navigator, panes and grid, glyphs, modal and backdrop, pane menu, board (L~3,900–4,700), settings (L2,398–3,213: `settings__*`, `set-*`, `seg*`, `switch*`, `theme-*`, `font-card*`, `term-preview*`), Discord `dc*`, shortcuts `keymap*`, `kbd*` (L6,298–6,817), voice `vm*`, voice pill, ctx menu, … |
| `src/styles/chrome.css` | 787 | toolbar, activity capsule, status bar, `tmenu` popovers, container breakpoints |
| `src/styles/inspector.css` | 1,050 | inspector, drawer |
| `src/styles/palette.css` | 264 | command palette |
| `src/styles/model.css` | 857 | preview panel, 3D toolbar, viewers |
| `src/styles/mcp-scope.css` | 315 | MCP servers panel |
| `src/styles/stats.css` | 1,092 | stats page and charts |
| `src/styles/orchestration.css` | 489 | Needs-you, orchestration panel, badges |
| component CSS | 1,050 | `GitCloneConsent.css` 73, `KitActivation.css` 318, `accounts/accounts.css` 284, `settings/KitsGallery.css` 52, `KitsSettings.css` 117, `MobileSettings.css` 206 |

**Porting rule:** each package translates the CSS of the surfaces it owns into iced `Style` functions over the shared tokens and metrics (§7.2). Do not try to port CSS mechanically; match the screenshots and the token use.

### 7.2 Design tokens

**Static `:root`** (styles.css:4):

| Variable | Value |
|---|---|
| `--ui-scale` | `settings.uiScale/100`. **Multiplies every font size: 1,092 uses** of `calc(Npx * var(--ui-scale))` |
| `--sidebar-w` | `252px × scale` |
| radii | `--radius-xs` 4, `--radius-sm` 5, `--radius` 6, `--radius-lg` 8, `--radius-xl` 10 (controls 4, cards 6–8, floating panels 10; panes are square with 1 px separators) |
| `--ui-font` | `"Inter Variable"`, then system fallbacks |
| `--mono` | `settings.fontFamily` (the terminal font is also the chrome's monospace) |
| `--dur` | 140 ms |
| `--ease` | `cubic-bezier(0.2, 0.7, 0.2, 1)` |

**Font-size ramp** (px at 100 %, number of rules):

| px | 11 | 12 | 11.5 | 10.5 | 13 | 12.5 | 10 | 15 | 13.5, 16, 17, 26 |
|---|---|---|---|---|---|---|---|---|---|
| rules | 84 | 72 | 55 | 53 | 34 | 18 | 10 | 8 | 1 each |

P1 exposes this as `Metrics::font(Size::…)` × `ui_scale`.

**Theme tokens.** `deriveChromeTokens(theme)` (themes.ts:1050) produces **53 CSS variables** from the terminal palette, so a theme change restyles the whole window. Port it as `hangar_theme::Tokens` with one field per variable, computed by the same contrast rules.

| Group | Tokens |
|---|---|
| surfaces | `--chrome`, `--chrome-bar`, `--pane-head`, `--pane-head-focus`, `--raised`, `--card`, `--sep`, `--line`, `--line-strong`, `--bg`, `--bg-sunken`, `--surface`, `--surface-hover`, `--surface-active`, `--border`, `--border-strong` |
| text | `--fg`, `--fg-secondary`, `--fg-muted`, `--fg-subtle` (each ≥ 4.5:1 on every surface) |
| accent | `--accent`, `--accent-hi`, `--accent-soft` (14 %), `--accent-ring` (45 %), `--accent-fg` |
| agent states | `--state-{working,waiting,yours,idle}` (text, 4.5:1) and `--state-*-fill` (glyphs, 3:1); "yours" gets a hue clear of the accent and of green (`yoursBase`) |
| status | `--danger`, `--danger-soft`, `--success`, `--warning` |
| depth | `--shadow`, `--float-ring` |
| segmented control | `--seg-track`, `--seg-thumb` |
| charts | `--viz-1..4` (lightness ramp of the accent), `--viz-heat-0..5`, `--viz-mute`, `--viz-line` |

**Themes:** **33** themes (`themes.ts`), each `{id, name, appearance, accent, xterm: 21 colours}`:
- **25 dark:** graphite (default), tokyo-night, catppuccin-mocha, dracula, one-dark, nord, gruvbox-dark, github-dark, ayu-dark, rose-pine, solarized-dark, vscode-dark, night-owl, monokai-pro, material-palenight, synthwave-84, cobalt2, shades-of-purple, andromeda, ayu-mirage, horizon, panda, kanagawa, everforest-dark, poimandres.
- **8 light:** catppuccin-latte, github-light, one-light, solarized-light, quiet-light, ayu-light, rose-pine-dawn, night-owl-light.
- `themes.test.ts` (14 cases) holds every theme to the contrast rules. Port it to `hangar-theme`.
- **Kits** get the 34 colour tokens of `crates/hangar-core/kit-sdk/tokens.json`, mapped from these (`kitThemeEvent`, §2.4).

### 7.3 Fonts

- **UI font:** Inter Variable (`@fontsource-variable/inter/opsz.css`, bundled). `body` sets 13 px × scale, `font-optical-sizing: auto` and `font-feature-settings: "cv11", "ss01"` (styles.css:14–17).
  - Native: embed Inter as **TTF** (static 400/500/600/700, or the variable TTF if cosmic-text renders its weights well enough) in `hangar-theme/fonts/`, then `iced::font::load` at boot.
  - The kit SDK's `inter.woff2` cannot be used by iced.
- **Terminal and mono font:** `settings.fontFamily` is a **CSS font stack string**. Native: parse the stack, pick the first family installed (fontdb), fall back to the bundled default.
  - Settings › Terminal offers 7 stacks: Cascadia Code, JetBrains Mono, Fira Code, Consolas, Hack, IBM Plex Mono, System; plus "Custom".
  - Decision: bundle **one** monospace TTF (JetBrains Mono, OFL) as the guaranteed fallback.

### 7.4 Icons (`src/components/Icon.tsx`, 369 LOC)

- **75 inline SVG glyphs** on a 16-unit grid: `viewBox 0 0 16 16`, `stroke=currentColor`, `stroke-width 1.35`, round caps and joins, no fill. Window glyphs share the frame rect 12.5 × 10.5, r 2.2. Some are after Lucide (ISC).
- Default size 14 px × ui scale. 67 distinct names are referenced through `<Icon name=…>`.
- **Names:** layout1 layout2 layout4 layout8 splitRight splitDown tidy sidebarLeft sidebarRight board terminal workspace window windowReturn folder folderPlus bookmark chart table trash pencil branch merge plug lock link bubble tray bell mic clock moon orchestrator flag stop gear sliders appearance prompt gamepad keyboard info broadcast send download plus minus close check more search filter chevronRight chevronDown chevronUp updown arrowRight arrowUpRight expand collapse restart reset alert bolt diff cube frame wireframe grid axes normals camera play pause copy.
- `components/preview/Glyph.tsx` (83) has a few more preview-only glyphs.
- **Native (P2):**
  - generate `crates/hangar-ui/icons/<name>.svg` from `PATHS` once (a script);
  - expose `icon(Icon::Name, size) -> Svg` styled with the current text colour (`svg::Style { color }`);
  - `include_bytes!` all of them.
- **Logo** (`Logo.tsx`): 32×32, three rects. The first takes the accent; the others take `--fg`.
- **StateGlyph** (`StateGlyph.tsx` + styles.css:1060–1150), one shape per state so colour is not needed to read it:

  | State | Shape |
  |---|---|
  | working | filled dot **plus a ring that grows and fades** (`glyph-pulse` 2.4 s) |
  | waiting | **diamond** (rotated square, r 1.5) |
  | yours | filled dot |
  | idle / exited | hollow ring (1.5 px) |
  | starting | hollow ring **blinking** (`glyph-blink` 1.2 s, steps) |

  Draw it with `canvas` in P2.

### 7.5 Animations worth keeping

Everything is disabled under `prefers-reduced-motion`. Native: a setting, or follow the OS when it can be read.

| Keep (meaningful) | Where | Native |
|---|---|---|
| `glyph-pulse`, `glyph-blink` | agent state | `canvas` + `iced::time::every(16 ms)` only while some glyph is working or starting |
| `cursor-blink` | terminal cursor (`cursorBlink`) | hangar-term, 530 ms period |
| `board-shimmer` / `board-reveal` | board skeleton while loading | P2 skeleton widget |
| `voice-wave`, `rec-pulse` | voice HUD | P6 |
| `model-spin`, `stats-spin` | loading spinners | P2 spinner |
| `stats-breathe` | stats refreshing | P11 (optional) |

**Nice to have** (enter transitions, about 140 ms): `menu-in`, `tmenu-in`, `pane-menu-in`, `palette-fade`/`palette-rise`, `modal-in`, `fade-in`, `slide-in`, `insp-drawer-in`, `board-dock-in`, `model-panel-in`/`grow`, `update-in`, `pill-in`, `board-pop`, `dc-pulse`, `pulse`, `model-fade`, `model-travel`. Use iced 0.14 `Animation` where cheap, and skip them in v1 otherwise.

**Responsive rules** (container queries) that change content, not just spacing:
- toolbar: < 80 em hides the title, < 71 em the activity chip, < 64 em turns views icon-only, < 46 em folds actions into "…";
- activity capsule: < 30 em, < 10 em;
- status bar: < 58 em, < 40 em;
- pane bar (orchestration badge): 42, 40, 34, 15.5 em;
- stats bar and page: 52 / 40 em, 760 / 900 px;
- inspector: ≤ 280 px;
- preview: 460 / 360 px.

Native: iced `responsive` with thresholds in px × ui scale. 1 em = the container's font size: 12 px × scale for the toolbar (`chrome.css:30`), 13 px × scale for `body` elsewhere.

---

## 8. Work packages

There are 21 packages: P0, the shell, plus 20 feature packages.

**File ownership is disjoint.**
- A package edits only the files it owns.
- The only shared file is `crates/hangar-app/src/message.rs`'s `Intent` enum, and only to **append** a variant (§8.2 rule 5).
- Rust LOC estimates include the ported tests.

### 8.1 Package list and waves

| # | Package | Owns (paths under `crates/hangar-app/src/` unless a crate is named) | Ports (TS) | Est. Rust LOC | Depends on | Wave |
|---|---|---|---|---|---|---|
| **P0** | **App shell**: windows, model, store, persistence, regions, side panels, message routing, command table | `main.rs`, `app.rs`, `message.rs`, `ctx.rs`, `state/{mod,types,store,persist,layout,pane_names,folders,roles,live,snapshot}.rs`, `shell/{mod,frame,regions,side_panels,drawer,notice,windows,placeholder,overlay}.rs`, `services/{mod,core,events,inbox,notify,clipboard,dialogs,terminals}.rs`, `commands/{mod,id,run}.rs`; **plus the stub `mod.rs` of every package directory below** (handed over at once) | `main.tsx`, `store.tsx`, `types.ts`, App.tsx frame + `buildHandlers`, `lib/{layout,paneNames,folders,orchestration,sidePanels,useDrawer,regions,focusable,windows,ipc,notify}.ts` | 4,500 | backend agent's core API (stub what is missing) | 0 |
| P1 | **Theme, i18n, formatting** | crate `hangar-theme` (`themes.rs`, `tokens.rs`, `contrast.rs`, `metrics.rs`, `fonts/`); **new crate `hangar-i18n`** (`lib.rs`, `plural.rs`, `typography.rs`, `fmt/{number,time,bytes}.rs`, `dict/*.json`, `build.rs`); `scripts/i18n-export.mjs` | `themes.ts`, `lib/contrast.ts`, `i18n.ts`, `strings/*`, `accounts/strings.ts`, `lib/typography.ts`, the generic parts of `inspectorFormat.ts` | 3,500 + JSON | – | 0 |
| P2 | **hangar-ui widgets and icons** | crate `hangar-ui` (all): `icon.rs` + `icons/*.svg` (75), `logo.rs`, `state_glyph.rs`, `button.rs`, `toggle.rs`, `segmented.rs`, `slider.rs`, `select.rs`, `input.rs`, `stepper.rs`, `armed.rs` (two-step confirm), `kbd.rs`, `tooltip.rs`, `popover.rs` (anchored, flip/clamp, `CloseReason`), `context_menu.rs`, `modal.rs` (backdrop, head, focus trap, Escape, stack), `list_nav.rs`, `resize_grip.rs`, `foldable.rs`, `tabs.rs`, `progress.rs`, `spinner.rs`, `skeleton.rs`, `notice.rs` (banner/tone), `pill.rs`, `chip.rs`, `listbox.rs` (dropdown with keyboard), `tail_text.rs` (leading ellipsis), `ring_gauge.rs`, `meter.rs` | `Icon.tsx`, `Logo.tsx`, `StateGlyph.tsx`, `Backdrop.tsx`, `ToolbarMenu.tsx`, `NavigatorMenu.tsx`, `lib/menuNav.ts`, generic CSS (buttons, forms, modal, menus) | 3,800 | P1 (tokens) | 0 |
| P3 | **hangar-term widget** | crate `hangar-term` (all): `lib.rs`, `term.rs`, `render.rs`, `input.rs`, `mouse.rs`, `search.rs`, `links.rs`, `palette.rs`, `fit.rs`, `examples/shell.rs` | xterm.js behaviour (§4/1), `windows.ts` `readLines` | 3,800 | P1 (palette) | 0 |
| P4 | **Pane logic and live stores** | `panes_logic/{mod,agent_state,pane_insight,bridge_events,activity,hibernate,hibernation,agents,session_watch,agent_pane,context_gauge,shells,pane_branch,file_drop}.rs` | `lib/{agentState,paneInsight,bridge,activity,hibernate,hibernateStore,useHibernation,agents,agentPane,context,shells,paneBranch,fileDrop}.ts`, `strings/paneBranch.ts` | 4,600 | P0 | 1 |
| P5 | **Pane grid and terminal pane** | `panes/{mod,workspace_terminals,grid,terminal_pane,header,search,zoom_bar,gauge,status,controller,pty_link,agent_watch,hub}.rs` | `WorkspaceTerminals.tsx`, `PaneGrid.tsx`, `TerminalPane.tsx`, `TerminalSearch.tsx`, `PaneZoomBar.tsx`, `PaneGauge.tsx`, `PaneStatus.ts`, `lib/{ptyBus,terminalRegistry}.ts` | 3,800 | P0, P2, P3, P4 | 1 |
| P6 | **Navigator and chrome bars** | `navigator/{mod,model,drag,menu,status,plan}.rs`, `toolbar/{mod,activity,menus}.rs`, `status_bar.rs`, `needs_you/{mod,model}.rs`, `update_banner.rs`, `voice/{mod,hud}.rs` | `Sidebar.tsx`, `Navigator*.ts(x)`, `Toolbar.tsx`, `ToolbarActivity.tsx`, `StatusBar.tsx`, `NeedsYou.tsx`, `UpdateBanner.tsx`, `VoiceHud.tsx`, `lib/{needsYou,voice,openEditor}.ts` | 4,600 | P0, P2, P4 (activity reads) | 1 |
| P7 | **Keymap, palette, saved commands, session picker, pane menu** | `keymap/{mod,chord,keymap,dispatch}.rs`, `palette/{mod,matching,sources}.rs`, `saved_commands/{mod,menu,dialog}.rs`, `session_picker.rs`, `pane_menu.rs` | `lib/{keys,shortcuts,useShortcuts,paletteMatch,savedCommands}.ts`, `CommandPalette.tsx`, `CommandMenu.tsx`, `CommandDialog.tsx`, `SessionPicker.tsx`, `PaneMenu.tsx` | 3,100 | P0, P2 | 1 |
| P8 | **Workspace creation, git flow, consent, Claude accounts** | `workspace_dialog/{mod,folder,git_form,kits_dialog,hooks}.rs`, `git_flow/{mod,consent}.rs`, `consent_card.rs`, `accounts/{mod,model,switch,usage,switcher,badge,picker,pane_items,settings_page,menu_items}.rs` | `WorkspaceDialog.tsx`, `GitCloneConsent.tsx`, `components/accounts/*`, `lib/{workspaceFromGit,gitClone,accounts,accountSwitch}.ts`, `strings/workspaceFromGit.ts` | 3,300 | P0, P2; embeds P14 `kits::workspace_section` | 1 |
| P9 | **Inspector, diff view, worktrees** | `inspector/{mod,view,header,sections,files,files_model,format}.rs`, `git_ui/{mod,diff_view,diff_parse,worktrees/{mod,view,create,cleanup,rows}}.rs`; `hangar-core/src/worktree_naming.rs` (coordinate with the backend agent) | `Inspector*.tsx`, `DiffView.tsx`, `WorktreePanel.tsx`, `lib/{inspectorFiles,inspectorFormat,diff,git,worktrees}.ts`, `strings/worktrees.ts` | 4,700 | P0, P2, P4 (insight), P12 (board cache) | 1 |
| P10 | **Memory, MCP and orchestration panels** | `memory/{mod,view,guards,format}.rs`, `mcp/{mod,install_panel,servers_panel,scope}.rs`, `orchestration_ui/{mod,panel,badge,form,actions}.rs` | `MemoryView.tsx`, `memoryGuards.ts`, `McpPanel.tsx`, `McpServersPanel.tsx`, `OrchestrationPanel.tsx`, `OrchestrationBadge.tsx`, `lib/{memory,memoryFormat,mcpScope,orchestrationForm,orchestrationActions}.ts` | 3,100 | P0, P2 | 1 |
| P11 | **Stats** | `stats/{mod,model,view,ranked,sessions,tables,charts/{columns,heatmap,sparkline,readout,ticks}}.rs` | `StatsView.tsx`, `StatsCharts.tsx`, `lib/{stats,statsModel}.ts` | 2,700 | P0, P1 (viz tokens, fmt), P2 | 1 |
| P12 | **Board** | `board/{mod,model,cache,view,card,filter,task_dialog,dock,format,drag}.rs` | `Board*.tsx`, `TaskDialog.tsx`, `lib/{board,boardDrag,boardFormat,paneTask}.ts` | 2,600 | P0, P2 | 1 |
| P13 | **Settings** | `settings/{mod,schema,rows,previews,search,shortcuts,mobile,mobile_model,voice_card,discord_card}.rs`, `discord.rs`, `experiments.rs` | `settings/*` (minus Kits), `lib/{experiments,discord,remote}.ts`, `strings/experiments.ts` | 4,600 | P0, P2, P7 (keymap API for the editor); embeds P8 `accounts::settings_page`, P14 `kits::settings_view` | 1 |
| P14 | **Kits management** | `kits/{mod,model,activation,setup,gallery,icon,views/{settings,gallery,activation,workspace_section}}.rs` | `KitActivation.tsx`, `settings/KitsSettings.tsx`, `settings/KitsGallery.tsx`, `lib/{kits,kitActivation,kitSetup,kitGallery,kitIcon}.ts`, `strings/kitsManage.ts` | 4,500 | P0, P2 | 1 |
| P15 | **Preview panel and 2D viewers, webview helper** | `preview/{mod,store,model,panel,tabs,file_view,image,text,code_lines,highlight,markdown,media,pdf,html,parts,file_watch,webview}.rs`; crate **`hangar-webview`** (all, with `src/kit_relay.rs` left as a stub for P16) | `ModelPanel.tsx` panel parts, `components/preview/*` except KitTabView, `lib/{preview,modelStore,modelHost,modelFiles}.ts` | 5,000 | P0, P2 | 1 |
| P16 | **Kit tabs host and kit API** | `kit_tabs/{mod,api,tabs,view,bridge}.rs`; `hangar-webview/src/kit_relay.rs`; `crates/hangar-kit-host/src/host_api.rs` (only the `KitHost` trait mirroring `kitApi`; the wasmtime runtime is outside this inventory) | `preview/KitTabView.tsx`, `lib/{kitApi,kitTabs}.ts`, `strings/kitsTab.ts` | 3,900 | P14 (kit model), P15 (preview store, helper protocol) | 2 |
| P17 | **3D viewer** | **new crate `hangar-model3d`** (`formats`, `framing`, `stats`, `material`, `load/{gltf,obj,stl,ply}`, `scene`, `orbit`, `renderer`, `offscreen`, `widget`, `shaders/*.wgsl`); `model3d_view/{mod,toolbar,animation_bar,info_strip,capture}.rs` | `ModelPanel.tsx` 3D parts, `lib/model3d/*`, `lib/modelCapture.ts` | 6,100 | crate: P1 only (wave 0 OK); app view: P15 | 0 (crate) / 2 (view) |
| P18 | **Control bus and agent ops** | `control/{mod,port,types,panes,preview,kit_tabs,workspace,manager,delivery,delivery_text}.rs` | `lib/{control,managerControl,deliveryControl,deliveryRuns}.ts` | 6,000 | P0 contracts (§8.2), P4 types; calls P8 `git_flow`, P15 store API, P16 tab listing (via traits) | 1 |
| P19 | **Orchestration engine** | `orchestration/{mod,runtime,worker,events,brief,brief_text,manager_feed,projects}.rs` | `lib/{workerControl,orchestratorEvents,orchestratorBrief,managerEvents,projects}.ts` | 6,200 | P0 contracts, P18 port types | 1 |
| P20 | **Hangar Mobile, window side** | `mobile/{mod,ops,chat_live,answer,delivery,apps,stats,push,activity_log}.rs`; `hangar-core/src/remote/contract.rs`, `hangar-core/src/chat/calls.rs` (coordinate with the backend agent) | `lib/{remoteControl,remoteAnswer,remoteDelivery,remoteApps,remotePush,activityLog}.ts`, `src/remote/*` | 7,100 | P0 contracts, P18 (`pane_read`/`pane_send`, `land_delivery`), P11 `stats::model` | 1 |

**Total: about 91k Rust LOC.**

**Waves:**
- **Wave 0** (start together):
  - **P0 is the gate.**
  - P1, P2, P3 and the P17 crate are separate crates and start at the same time. Their **first commit must be their public API skeleton**, compiling with placeholder bodies, so wave 1 can code against it:
    - P1: `Tokens`, `Theme`, `Metrics`, `I18n`, `k::*`;
    - P2: every widget constructor's signature;
    - P3: `Terminal`, `TermView`, `Event`, the `key_filter` hook.
- **Wave 1:** everything else except P16 and P17's app view, once P0 has merged its skeleton (it compiles, routes messages and draws the frame with placeholders). That is 15 packages in parallel.
- **Wave 2:**
  - P16 (needs P14's kit model and P15's preview store and helper protocol);
  - P17's `model3d_view` (needs P15's panel body slot);
  - then an integration and parity pass: run the old app beside the new one on a copy of a real `state.json`.

### 8.2 P0: what the shell must pre-create so packages do not conflict

**Rules for every package:**
1. A package directory `src/<pkg>/` is created by P0 with a `mod.rs` stub and is then owned by its package. Only that package edits it.
2. Every stub `mod.rs` exports the same surface:

   ```rust
   #[derive(Default)] pub struct State;            // UI state of the package; lives in App.pkg.<name>
   #[derive(Default)] pub struct Live;             // shared read-model other packages read; lives in Model.live.<name>
   #[derive(Debug, Clone)] pub enum Msg { }        // the package's messages; routed by P0 as Message::<Name>(Msg)
   pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message>;
   pub fn subscription(st: &State, m: &Model) -> Subscription<Message>;   // Subscription::none() in the stub
   pub fn focus_entry(st: &State, m: &Model, region: Region) -> Option<Task<Message>>; // only region owners
   // + the view and embed functions listed in the table below, with these exact signatures
   ```

3. Persisted state changes **only** through `Model::apply(StoreAction) -> StoreOutcome`.
   - It is synchronous and returns ids (for example the new pane id).
   - It schedules the debounced save and republishes the `Snapshot`.
   - A package never writes `model.state` directly.
4. Cross-package requests are `Message::Intent(Intent::…)`. P0 handles them centrally (open an overlay, activate a pane, and so on). A package never calls another package's `update`.
5. A missing intent is added as **one appended variant** to `Intent` in a commit of its own. The handler goes in the package's own `on_intent` hook, which P0 pre-declares in every stub as `pub fn on_intent(st, m, sv, &Intent) -> Option<Task<Message>>`. P0's router offers each intent to every package's `on_intent` after its own match.
6. Engines (P18–P20) never touch `Model`:
   - they read `services.snapshot()` (an `Arc<Snapshot>` republished after every update);
   - they act through `services.inbox().send(Message)`, a channel drained by a P0 subscription;
   - model writes from engines are `Message::Control(control::Msg::Model(ModelCmd, Reply))`.

**`message.rs`** (P0):

```rust
pub enum Message {
    // infra (P0)
    Booted(Result<Option<serde_json::Value>, String>), SaveDue(u64), Core(CoreEvent),
    Window(window::Id, window::Event), Key(window::Id, keyboard::Event), Command(CommandId),
    Intent(Intent), Store(StoreAction), Tick(Instant), NoticeExpired(u64), Noop,
    // one route per package (Msg types live in the packages)
    PanesLogic(panes_logic::Msg), Panes(panes::Msg),
    Navigator(navigator::Msg), Toolbar(toolbar::Msg), StatusBar(status_bar::Msg), NeedsYou(needs_you::Msg),
    UpdateBanner(update_banner::Msg), Voice(voice::Msg),
    Keymap(keymap::Msg), Palette(palette::Msg), SavedCommands(saved_commands::Msg),
    SessionPicker(session_picker::Msg), PaneMenu(pane_menu::Msg),
    WorkspaceDialog(workspace_dialog::Msg), GitFlow(git_flow::Msg), Consent(consent_card::Msg), Accounts(accounts::Msg),
    Inspector(inspector::Msg), GitUi(git_ui::Msg),
    Memory(memory::Msg), Mcp(mcp::Msg), OrchestrationUi(orchestration_ui::Msg),
    Stats(stats::Msg), Board(board::Msg), Settings(settings::Msg), Kits(kits::Msg),
    Preview(preview::Msg), KitTabs(kit_tabs::Msg), Model3d(model3d_view::Msg),
    Control(control::Msg), Orchestration(orchestration::Msg), Mobile(mobile::Msg),
}

pub enum Intent {                         // every cross-package request found in this inventory
    ActivateWorkspace(WsId), FocusPane { ws: WsId, pane: PaneId, terminal: bool }, // = App's activateRef
    SetView(View), OpenOverlay(Overlay), CloseOverlay, ClosePopovers(CloseReason),
    OpenSettings(Option<SettingsCategory>), TogglePalette,
    ToggleNavigator, FocusNavigator, ToggleInspector, FocusInspector, ToggleDock, CloseDock,
    TogglePreview, ClosePreviewPanel, TogglePreviewExpand, CycleRegion(i8), FocusRegion(Region),
    SplitPane { ws: WsId, near: Option<PaneId>, dir: Option<SplitDir> },
    ReplacePane { ws: WsId, pane: PaneId, patch: PanePatch },     // respawn + keep focus + carryResume
    RestartPanes { ws: WsId, panes: Vec<PaneId> }, ArmResetAgents(bool), ResetAgents(WsId),
    ApplyPreset { ws: WsId, size: u8 }, Tidy(WsId), SetZoom { ws: WsId, on: bool },
    Detach(WsId), Reattach(WsId), Sleep(WsId),
    PinOrchestrator { ws: WsId, pane: PaneId, pinned: bool }, CreateOrchestrator(WsId), OpenManager,
    ArmStopWorkers(bool), StopWorkers(WsId), Land { ws: WsId, task: String },
    OpenDiff { ws: WsId, pane: Option<PaneId> }, OpenWorktrees(WsId), OpenKitsDialog(WsId),
    OpenMcpInstall, OpenMcpServers(WsId), OpenOrchestration(WsId), OpenSessions { ws: WsId, pane: PaneId },
    OpenBoard(WsId), OpenTask { cwd: String, task: Option<String> },
    ShowPreview(PreviewRequest), OpenKitTab { ws: WsId, kit: String, tab: String },
    OpenFinder(PaneId), RunSavedCommand { ws: WsId, from: PaneId, index: usize },
    Broadcast { ws: WsId, text: String, when_free: bool }, InsertTranscript(String),
    Notice(Notice), Notify(Notification), CreateWorkspaceFromGit(GitRequest),
}
```

**Other types P0 defines** (in `state/` and `shell/`):

| Type | Definition |
|---|---|
| `View` | `Terminals \| Board \| Memory \| Stats` |
| `Region` | §1.8 |
| `Focus` | `Terminal(PaneId) \| Region(Region) \| Widget(widget::Id)` |
| `Drawer` | `Inspector \| Dock \| Model` |
| `Notice` | `Problem(String) \| Asleep(String) \| Agent(String)` (6 s) |
| `Notification` | `{title, body, target: Option<(WsId, PaneId)>}` → `hangar_core::notify`; a click → `Intent::FocusPane` |
| `SettingsCategory` | `General, Appearance, Terminal, Shell, Notifications, Mobile, Kits, Accounts, Voice, Discord, Shortcuts, Experimental, About` |
| `Overlay` (a **stack** `Vec<Overlay>`; the top one has the keyboard; Escape closes the top one) | `Settings(SettingsCategory)`, `Palette`, `WorkspaceDialog`, `SessionPicker{ws,pane}`, `CommandDialog{ws,pane,edit}`, `McpInstall`, `McpServers(WsId)`, `Orchestration(WsId)`, `Worktrees(WsId)`, `KitsDialog(WsId)`, `KitActivation{kit, ws: Option<WsId>}`, `Diff{ws,pane}`, `Task{cwd,task}`. Each overlay's own state lives in its package; the variant only carries identity. |

**`Model.ui`** (App.tsx transient state, §1.5):
- `view`, `overlays`, `drawer`, `focus_by_workspace`, `zoom_by_workspace`, `opened_ids`;
- `confirm_reset`, `confirm_stop_workers`, `needs_you_open`;
- `broadcast: String`, `notice`, `pending_chords: Vec<Chord>`;
- `available_agents`, `shells`, `body_width` (per window), `focus`.

**`Model.live`**: one `Live` per package (above), plus `hydrated`, `windows: HashMap<window::Id, WindowKind>` and `version`.

**`StoreAction`**: one variant per store action of §1.3, returning `StoreOutcome { pane: Option<PaneId>, workspace: Option<WsId>, folder: Option<String> }`. Port `store.test.ts` and the store semantics exactly.

**`CommandId`** (`commands/id.rs`):
- the **88 variants** of §1.10, with `as_str()`/`from_str()` (the strings are the `keybindings` keys in `state.json`), `section()` and `label_key()`;
- `commands/run.rs`:
  - `fn available(m: &Model, w: window::Id) -> Vec<CommandId>`: the "Live when" column;
  - `fn run(m, w, id) -> Task<Message>`: each command becomes an `Intent`, a `StoreAction` or a package message. The palette (P7) lists `available()`.

**`CoreEvent`** (`services/events.rs`): one variant per row of §1.9, with typed payloads.
- If the backend agent's bus is not ready, P0 wraps whatever `hangar-core` exposes, or stubs it.
- **`ControlRequest { id, op, args: Value, channel: Agent | Internal, reply: Reply }`**, where `Reply` = `Arc<Mutex<Option<oneshot::Sender<Result<Value, String>>>>>`. It arrives through `services/events.rs` as `Message::Control(control::Msg::Request(req))` (§5, "Async shape"). Before hydration, P0 answers `Err("Hangar.AI is still starting…")` itself.

**Service traits** (`services/`). Default no-op implementations; the owner swaps in the real one at startup:

```rust
pub trait TerminalAccess: Send + Sync {            // P5 implements (TerminalHub)
    fn has(&self, pane: &PaneId) -> bool;
    fn read_lines(&self, pane: &PaneId, n: usize) -> Option<Vec<String>>; // windows.ts readLines semantics
    fn paste(&self, pane: &PaneId, text: &str) -> bool;                   // bracketed when DECSET 2004
    fn application_cursor(&self, pane: &PaneId) -> bool;
}
pub trait PreviewAccess: Send + Sync { /* list, show, reveal, reload, close, when_settled, snapshot, tabs */ } // P15 (+P16, P17)
pub struct Services { core: CoreHandles, terminals: Arc<dyn TerminalAccess>, preview: Arc<dyn PreviewAccess>,
                      inbox: mpsc::UnboundedSender<Message>, snapshot: watch::Receiver<Arc<Snapshot>>, clipboard, dialogs /* rfd */ }
```

**`Snapshot`** (`state/snapshot.rs`): `Arc<AppState>`, `opened: HashSet<WsId>`, `asleep: HashSet<WsId>`, `activity: HashMap<PaneId, PaneActivity>`, `delivering: HashMap<WsId, HashSet<String>>`. The shared value types (`PaneActivity`, `AgentActivity`, `PlanLimits`, `PaneInsight`, `BridgeEvent`) are declared by P0 in `state/live.rs` from the TS types and **filled in by P4**.

**View slots P0's frame calls** (each stub returns a placeholder):

| Slot | Function |
|---|---|
| region `navigator` | `navigator::view(st, ctx)` |
| region `toolbar` | `toolbar::view(st, ctx)`, which embeds `needs_you::button`, `accounts::switcher` |
| region `main` | `panes::view_workspace(st, ctx, ws, hidden)` for each opened workspace; or `board::view(st, ctx, BoardVariant::Full)`, `memory::view`, `stats::view` |
| region `model` | `preview::view_panel(st, ctx, mode)`, whose body slots are `preview::view_entry` → `model3d_view::view` / `kit_tabs::view` |
| region `dock` | `board::view(st, ctx, BoardVariant::Dock{..})` |
| region `inspector` | `inspector::view(st, ctx)` |
| region `status` | `status_bar::view(st, ctx)` |
| floating | `update_banner::view`, `consent_card::view`, `voice::hud`; the notice pill and chord hint are P0's own |
| overlays | `settings::view`, `palette::view`, `workspace_dialog::view`, `workspace_dialog::kits_dialog_view`, `session_picker::view`, `saved_commands::dialog_view`, `mcp::install_view`, `mcp::servers_view`, `orchestration_ui::panel_view`, `git_ui::diff_view`, `git_ui::worktrees_view`, `kits::activation_view`, `board::task_view` |

**Embed functions** (one package draws a piece of another package's surface):

| Function | Embedded in |
|---|---|
| `accounts::pane_badge(ctx, ws, pane)` | pane header |
| `accounts::badge(ctx, ws)` | navigator row |
| `accounts::menu_items(ctx, ws, pane: Option) -> Vec<MenuEntry>` | navigator menu, pane menu |
| `accounts::workspace_picker(ctx, value) -> Element` | workspace dialog |
| `accounts::settings_page(st, ctx)` | Settings › Accounts |
| `orchestration_ui::pane_badge(ctx, ws, pane)` | pane bar |
| `kits::workspace_section(st, ctx, ws: Option)` | workspace dialog |
| `kits::settings_view(st, ctx)` | Settings › Kits |
| `pane_menu::view(st, ctx, pane)` | pane header |
| `saved_commands::menu_view(st, ctx, pane)` | pane header |
| `panes_logic::state_glyph_state(m, pane) -> GlyphState` | used by everything that draws a glyph |

**`ctx.rs`:** `Ctx<'a> { model: &'a Model, i18n: &'a I18n, tokens: &'a Tokens, metrics: &'a Metrics, keymap: &'a KeymapIndex, window: window::Id, now: Instant }`.
- `KeymapIndex` is defined in P7. P0 holds a placeholder until it lands, which makes P0 → P7 the only compile dependency in that direction.
- P7 publishes `Arc<KeymapIndex>` into `Model.live.keymap` for `hangar-term`'s key filter.

**P0's own behaviour** (port exactly): boot and hydration (§1.6), persistence (§1.4), multi-window (§1.7), regions (§1.8), side panels and drawers (§1.11), notices, theme application (`Tokens` for the active workspace's theme), `--ui-scale` → `Metrics`, the `available()`/`run()` table (§1.10), activate-pane semantics (`activateRef`, App.tsx:438), the placeholders, and the reopening of detached windows.

### 8.3 Per-package briefs: what to read and what "done" means

For every package, "done" means:
- its tests are ported and passing;
- its surfaces match today's app on a copy of a real `state.json`, in both locales and in a dark and a light theme;
- its code uses `hangar-i18n` keys only (no literal UI text);
- the `cargo clippy` lints are clean.

| Pkg | Read | Notes and decisions taken |
|---|---|---|
| P0 | §1, §8.2, Appendix A6 (control contract) | Keep `hangar-ai --mcp`. Main window 1360×860, min 900×560. Workspace windows min 640×420. Flush the debounced save on exit. |
| P1 | §6, §7.2–7.3 | JSON dictionaries are the native source of truth after the export. Accounts keys prefixed `accounts.`. Bundle Inter TTF and JetBrains Mono as fallback. |
| P2 | §2.1/0 (popup and modal primitives), §7.4–7.5 | One anchored popover with flip, clamp and `CloseReason`; a modal stack with a focus trap; icons generated from `Icon.tsx`. |
| P3 | §4/1 (all), §4/1.4 keyboard, §4/1.11 search | Must provide `key_filter` so shortcuts are not swallowed (§1.10). Replay mutes PTY answers. Resizes coalesced per frame. Web links open in the system browser. OSC 0/2 titles ignored. |
| P4 | §4/1.5–1.7, 1.10, 1.13, §4/4 | Clock-injected state machines. A `CoreEvent::Bridge` reducer feeds `Live` activity and insight. |
| P5 | §4 (all), §2.1/12 | Pane identity = pane id. A respawn means a new id, which kills and respawns the PTY. Hidden workspaces stay "mounted": their terminals keep consuming output. File drop uses `window::Event::FileHovered/FileDropped`. |
| P6 | §2.1/1–4, /8, /10 | Drag reorder with fixed row heights. The updater sits behind a `hangar-core` trait (decision: `cargo-packager-updater` with signature verification). Push-to-talk uses key release. |
| P7 | §1.10, §2.1/0, /5–7 | The dispatcher semantics of §1.10 exactly, including sequences, the 1,600 ms timeout and the field rules. The palette lists `commands::available()`. |
| P8 | §2.1/9, /11, /13 | `git_flow` is called by P18's `workspace_create` too: expose `git_flow::run(req, deps)` with the TS `FromGitDeps` as a trait. The usage cache moves to `<data>/account-usage.json`. |
| P9 | §2.2 Inspector, DiffView, WorktreePanel | Confirmations are inline two-click "armed" buttons, never native dialogs. Git polling is visible-only. |
| P10 | §2.2 Memory, MCP, Orchestration | `orchestration_ui::actions` provides pin/unpin/create/open-manager. P0's intents call it through `on_intent`. |
| P11 | §2.2 Stats | Charts on `canvas`; colours from `--viz-*`. A 60 s freshness cache shared with P20's `remote_stats`. |
| P12 | §2.3/1 | Owns the shared `boards` cache (`board_load` + `CoreEvent::BoardChanged`) that P6, P9 and P19 read. |
| P13 | §2.3/2–3 | Every row of the settings table. The shortcut recorder suspends the P7 dispatcher. Mobile, Voice and Discord cards load when their category is shown. |
| P14 | §2.4/1.1–1.3 | The `rfd` folder picker. `kits:changed` → refresh `Live.installed`. |
| P15 | §2.4/1.5–1.6, §3 Recommendations | Highlighting with `two-face`; markdown with iced `markdown`. PDF, HTML and video go to the `hangar-webview` helper (on Linux, PDF opens externally). Kit-tab persistence goes to `<data>/kit-tabs.json`. |
| P16 | §2.4/1.4 (every kit API method), §2.4/1.8 | The HTML tab = helper window plus relay. The `KitHost` trait is shared with native wasm tabs. |
| P17 | §2.4/1.7, §2.4/3 | `iced::widget::shader` + wgpu; `gltf`, `tobj`, `stl_io`, `ply-rs`; a headless offscreen renderer for MCP snapshots. Draco, KTX2, FBX and USDZ are phase 2. |
| P18 | §5/1 (control.ts, managerControl, deliveryControl), Appendix A6 | 27 ops with the per-op timeouts of §5/1.0. `pane_send` keeps the 120 ms gap between paste and Enter. |
| P19 | §5/1 (workerControl, orchestratorEvents, managerEvents, orchestratorBrief), Appendix A6 | One `OrchestrationRuntime` tokio task. The brief texts are copied verbatim. Ticks every 10 s and 5 s. |
| P20 | §5/1.8–1.9, §5/2, Appendix A6 | Keep the timeouts consistent: phone 12 s > server 10 s > window 8/7/6 s. Cache `remote_state` (it is polled every 2 s per phone). Fix the `dist-mobile` embed path (§0.3). |

---

## 9. Risks, decisions and assumptions

### 9.1 Biggest risks (ranked)

1. **Keyboard and focus semantics.** Today one capture-phase `keydown` listener sees every key before the terminal and the text fields. It consumes only keys with a live handler, holds sequences, and respects AZERTY, AltGr and field rules (§1.10).
   - iced gives keys to the focused widget first. `hangar-term`'s `TermView` and every text input must ask the keymap before consuming a key.
   - iced has no DOM focus model: F6 regions, roving tab stops, focus traps, focus restore after menus and drawers, and arrow navigation in menus all have to be built (P0, P2, P7).
   - Get this wrong and either shortcuts die inside terminals, or terminals lose keys such as Ctrl+Tab and Alt+arrows.
2. **Terminal fidelity and performance** (P3, P5).
   - The agents' TUIs rely on xterm behaviours: bracketed paste, Shift+Enter → `ESC CR` in agent panes, Ctrl+V as paste, OSC 9 and 777 notifications, bell, Unicode 11 widths, mouse modes, selection, search with a 1,000-match cap, links, 256 KiB history replay that mutes query answers.
   - Up to 16 panes × every opened workspace stay alive and keep parsing output while hidden.
   - A cosmic-text-per-cell renderer will not keep up. Plan a glyph-atlas wgpu renderer early, and measure with 8 busy Claude Code panes.
3. **Control and orchestration logic moves out of the UI** (P18–P20).
   - About 8,700 TS LOC with 8,400 test LOC and 414 cases (agent ops, workers, deliveries, engines, phone ops) answer the backend over `control:request` today.
   - In native they run on tokio against a snapshot, with a new request/oneshot contract that **depends on the backend agent changing `hangar-core::control`** (the `control::install(tx)` channel, `CoreEvent` bus).
   - Exact prompts, timings (10 s and 5 s ticks; 12/10/8/7/6 s timeouts) and ordering matter to agents and the phone. Regressions are silent. Port the tests first.
4. **Data compatibility and safety.**
   - `state.json` must round-trip: unknown fields kept, `null` vs absent kept, lenient readers (§1.4). A strict serde parse error would quarantine the file, start empty and then overwrite it with an empty state.
   - The data dir stays `com.simnjs.iabench`.
   - State held in WebView localStorage (open kit tabs with their params, the account usage cache) cannot practically be read by the native app. It is lost once: an accepted, documented regression.
   - Run both apps against copies of real state files before shipping.
5. **Web-only surfaces with no iced equivalent** (P15–P17, P6).
   - The 3D viewer (three.js, 1,151-line engine; glTF with Draco, KTX2 and Meshopt; OBJ/MTL, STL, PLY, FBX, USDZ) becomes a custom wgpu renderer.
   - PDF, HTML, video and **HTML kit tabs** move to a separate `hangar-webview` process: windows of their own instead of inline panels, IPC, focus handover. WebKitGTK has no PDF viewer on Linux.
   - Markdown, highlighting (two-face) and charts need parity work.
   - The Tauri updater (signed artifacts, NSIS/MSI/DMG/AppImage), `notification-activated` click-to-pane, and autostart all need native replacements.

**Other risks:**
- **P0 is the bottleneck** for 15 packages. Keep its first deliverable to a compiling skeleton with stubs, and fill in behaviour afterwards.
- **Cross-package view embedding** (badges, pickers, sections) creates compile coupling. The stubs mitigate it.
- **Accessibility:** iced 0.14 has no screen-reader support. Today's ARIA roles, live regions and labels are lost. Accepted for v1.
- **Intl:** relative times, plurals and number formats are re-implemented by hand for en and fr.
- **The branch breakages in §0.3:** the `dist-mobile` embed and the kit-sdk paths.

### 9.2 Decisions taken without asking (change them here if wrong)

- A **new crate `hangar-i18n`** owns the dictionaries, plurals, typography and formatting.
  - Keys are compile-checked constants generated from `en.json`.
  - The accounts dictionary is merged under the prefix `accounts.`.
  - The consent card keeps using the **system** locale, as today.
- A **new crate `hangar-model3d`** holds the 3D viewer.
- Cross-window protocols are dropped. One daemon and one model; detach and reattach change only which window draws a workspace.
- Persisted extras move to JSON files in the data dir: `kit-tabs.json`, `account-usage.json`. `hangar.webgl` is dropped.
- WebGL parking, the xterm renderer choice and the `?resume=` URL hand-off disappear.
- The 400 ms debounced save is kept, **plus a flush on exit**.
- The updater: `cargo-packager-updater` (or `self_update`) behind a `hangar-core` trait, and relaunch = spawn `current_exe` and exit.
- Clipboard images (3D capture): `arboard`. File dialogs: `rfd` (already a dependency). Opening URLs and paths: `open` (already a dependency).
- Resizes are coalesced to one PTY resize per frame (today there is no debounce).
- Web links open in the system browser. OSC 0/2 titles stay ignored.
- `src/mobile` is not touched. The desktop keeps building it (`pnpm mobile:build` → `dist-mobile/`), embedding it and answering its 15 `remote_*` ops (§5/2).

### 9.3 Out of scope here

- `crates/hangar-core` internals: the backend agent is de-Tauri-ing them.
- The wasmtime runtime of `hangar-kit-host`, and `hangar-kit-ui`/`hangar-kit-sdk`. P16 only defines the `KitHost` API, which mirrors `lib/kitApi.ts`.
- The browser demo (`demo/`, `vite.demo.config.ts`) and the website (`site/`).

---

## Appendix A: package notes from the mapping passes

These are the raw "suggested work packages" written while mapping each area. They carry useful file lists, sizes and shell needs, but use **their own** package names. §8 supersedes them:

| Appendix | Area | Their names → §8 |
|---|---|---|
| A1 | Chrome, navigator, palette, dialogs, accounts | WP1 Navigator → P6; WP2 Chrome bars → P6 + P2 (popover); WP3 Palette → P7 (+ P2 modal); WP4 Workspace/git/accounts → P8 |
| A2 | Terminal | WP-T → P3; WP-P → P5; WP-L → P4 (+ P0 for `layout`/`pane_names`; P1 for `contrast`/`typography`) |
| A3 | Inspector and friends | WP0 fmt → P1; WP1 Inspector → P9; WP2 Git surfaces → P9; WP3 Memory/MCP/Orchestration → P10; WP4 Stats → P11 |
| A4 | Kits, preview, 3D | K1 → P14; K2 → P16; P1 (preview) → P15; P2 (3D) → P17 |
| A5 | Board and settings | WP-1 Board → P12; WP-2 Settings → P13; WP-3 cards → P13 |
| A6 | Control engine | WP0 contracts → P0 (types) + P18 (implementation); WP1 → P18; WP2 → P19; WP3 → P20 |

### A1. Chrome, navigator, palette, dialogs, accounts

#### Suggested parallel work packages (4)

Files go under `crates/hangar-app/src/`. Rust LOC figures are rough and include tests.

| WP | Files | Contents | Est. Rust LOC |
|---|---|---|---|
| **WP1 Navigator** | `views/sidebar/mod.rs`, `views/sidebar/model.rs`, `views/sidebar/drag.rs`, `views/sidebar/menu.rs` (entry builder), `views/navigator_status.rs`, `views/navigator_plan.rs`, `logic/order.rs` (sidebarOrder) | Tree, rows and badges; rename in place; filter and attention toggle; roving keyboard; drag reorder; context-menu entries (incl. move-to, sleep verdict, account radio items); plan meters; the 27 model/folder tests | 2000–2400 |
| **WP2 Chrome bars and popovers** | `widgets/popover.rs` (anchored overlay + `CloseReason` + focus trap), `widgets/context_menu.rs` (NavigatorMenu entry model + renderer), `views/toolbar.rs`, `views/toolbar_activity.rs`, `views/status_bar.rs`, `views/needs_you.rs` + `logic/needs_you.rs`, `views/update_banner.rs`, `views/voice_hud.rs`, `shell/voice.rs` | Every anchored menu and confirmation (reset, stop workers, preset, kits, more); responsive folding; capsule; branch polling; Needs-you panel and notices; updater stages; voice pill | 2200–2600 |
| **WP3 Palette, commands, modals** | `widgets/modal.rs` (Backdrop/ModalHead/focus trap), `views/palette.rs`, `logic/palette_match.rs`, `views/command_menu.rs`, `dialogs/command_dialog.rs`, `logic/saved_commands.rs`, `dialogs/session_picker.rs`, `widgets/pane_menu.rs` | Palette sources and ranking with highlight (21 tests); saved commands; session list; the generic pane "…" menu | 1500–1800 |
| **WP4 Workspace creation, git, accounts** | `dialogs/workspace/mod.rs` (folder mode), `dialogs/workspace/git_form.rs`, `dialogs/workspace_kits.rs` + hooks section, `views/git_consent.rs`, `hangar-core/src/git_flow.rs` (flow + consent queue, the 65 tests), `views/accounts/{switcher,badge,picker,pane_items,settings_page,menu_items}.rs`, `logic/account_switch.rs`, `state/accounts.rs`, `i18n/{accounts,workspace_from_git}.rs` | Both dialog modes, file dialogs (rfd), clone progress and cancel, rollback; consent card; the whole account model and UI | 2600–3200 |

**What every package needs from the shared app shell** (`app.rs` / `state.rs` / `message.rs`):
- **State fields:**
  - `store: AppState` (workspaces, folders, `activeWorkspaceId`, settings) plus a `StoreAction` enum mirroring the store actions: `AddWorkspace`, `UpdateWorkspace`, `RemoveWorkspace`, `AddFolder`, `UpdateFolder`, `RemoveFolder`, `MoveFolder{id,before}`, `MoveWorkspace{id,folder,before}`, `UpdatePane`, `RespawnPane`, `AddPane`, `ApplyPreset`, `Tidy`, `Split`, `ReplacePane`, `ClosePane`, `SetTree`, `MovePane`, `Add/Update/Remove/MoveSavedCommand`, `UpdateSettings`;
  - `view: View`, `focus_by_workspace`, `zoom_by_workspace`, `opened_ids`;
  - **`overlay: Option<Overlay>`** — one exclusive modal: Palette, WorkspaceDialog, SessionPicker{pane}, CommandDialog{..}, KitsDialog{ws}, Worktrees{ws}, McpServers{ws}, Orchestration{ws}, Settings;
  - **`popover: Option<Popover>`** — anchored: NavMenu{kind,id,x,y,from_kb}, ToolbarMore, ResetConfirm, StopWorkersConfirm, PresetAsk, KitsMenu, NeedsYou, AccountSwitcher{pending}, PaneMenu, CommandMenu;
  - `confirm_reset`, `confirm_stop_workers`, `broadcast: String`, `notice`;
  - `agent_activity: HashMap<PaneId, PaneActivity>` (waiting/yours/working/idle + bridged + insight);
  - `plan_limits`, `accounts: Option<Vec<ClaudeAccount>>`, `accounts_support`, `launched_accounts`, `account_usage`;
  - `consent_queue`, `voice: {phase, level}`, `asleep`, `boards: HashMap<cwd, Option<Vec<Task>>>`, `keymap`, `focus_region`, `ui_scale`, `locale`.
- **Message variants:**
  - routing: `Sidebar(sidebar::Msg)`, `Toolbar(..)`, `Status(..)`, `NeedsYou(..)`, `Palette(..)`, `Dialog(..)`, `Accounts(..)`;
  - store and navigation: `Store(StoreAction)`, `ActivateWorkspace(id)`, `FocusPane{ws,pane}` (≈ `activateRef`), `SetView(View)`;
  - overlays: `OpenOverlay(Overlay)`, `CloseOverlay`, `OpenPopover(Popover, anchor: Rectangle)`, `ClosePopover(CloseReason)`;
  - shortcuts: `RunCommand(CommandId)` (the palette reuses the shortcut handler table — it needs a `fn available_commands(&State) -> Vec<CommandId>`);
  - events: `Bridge(BridgeEvent)` (usage, activity, plan limits), `AccountsChanged`, `CloneProgress{job,phase,percent}`, `Voice(VoiceEvent)`, `FileDrop(..)`;
  - notifications: `Notice(String)`, `Notify{title,body,target}`;
  - other: `Tick`, `WindowFocus(bool)`.
- **Services:**
  - an async `Backend` handle over `hangar-core` for: git_repo_info, git_repo_parse/ownership/clone/cancel/discard, claude_trust_folder, list_sessions, read/find_code_workspace, open_in_vscode, mcp_scope_clear, accounts_*, pty_write, bridge_enqueue, voice_*, activity_append, board_load;
  - `rfd` file dialogs;
  - a notification backend;
  - an i18n `t(key, params)` with plurals, plus the two extra dictionaries;
  - an icon set (≈45 icon names used here) and a `StateGlyph` widget (waiting diamond, yours, working, idle; pulse/blink animations).

**Assumptions:**
- `hangar-core/accounts.rs` and `git_clone.rs` already implement the backend half, so WP4 ports only the frontend logic (`resolveAccount` may already exist in core; dedupe it).
- The updater has no Tauri equivalent; an updater crate behind a trait is assumed.
- The localStorage usage cache moves to a JSON file.
- Animations (`tmenu-in`, `palette-rise`, `modal-in`, `update-in`, `voice-wave`, glyph pulse) are optional in v1 and can be done with iced 0.14 `Animation`/time subscriptions later.

### A2. Terminal

#### 5. Work packages

**WP-T: `hangar-term` widget (PTY-agnostic), about 3.8k Rust LOC**

| File | Content | LOC |
|---|---|---|
| `crates/hangar-term/src/lib.rs` | public API: `Terminal` (state), `TermView` widget, `Event` (`Input(Vec<u8>)`, `Resize{cols, rows}`, `Bell`, `Notify{body}` (OSC 9/777 rules), `Title` (unused), `Key` (real key pressed), `ClipboardStore/Load`, `LinkClicked(url)`) | 250 |
| `term.rs` | wraps `alacritty_terminal::Term` + `EventListener`; `feed(&str, seq)` with `replayed_to` filtering; `replay(history, end)` that **mutes PtyWrite answers** until done; `read_lines(n)` (joins wrapped rows); `viewport_text()` (trust prompt); `application_cursor()`; `clear`, `select_all`, `scroll_top/bottom`, `paste(text)` (bracketed, `\n`→`\r`) | 650 |
| `render.rs` | wgpu/canvas grid: cell metrics from fontFamily, fontSize, lineHeight, letterSpacing; padding; cursor block, bar or underline with blink, hollow when unfocused; selection; search matches; underline on link hover; dimming | 900 |
| `input.rs` | key → bytes (xterm-compatible, app cursor mode, Option-as-Meta on macOS, IME/compose); hooks: `shift_enter: Option<Vec<u8>>` (app sets `\x1b\r` for agents), `ctrl_v_pastes: bool`; focus reporting | 600 |
| `mouse.rs` | selection (drag, word, line, Alt-block), Alt+click moves the cursor, wheel scrollback, alt-screen wheel → arrows, mouse reporting modes, Shift override | 450 |
| `search.rs` | plain or regex, case toggle, upward incremental, next/prev, `{index, count}` capped at 1000 | 300 |
| `links.rs` | URL regex detection (web-links parity) | 180 |
| `palette.rs` | `hangar_theme::TerminalTheme` → alacritty colours; search decoration colours via `contrast::mix` | 120 |
| `fit.rs` | size → cols/rows (FitAddon parity), resize coalescing | 120 |
| `examples/shell.rs` | portable-pty demo | 150 |

Needs from outside: `hangar-theme` (palette, `mix`), fonts. Nothing from the app.

**WP-P: `hangar-app` pane views and controller, about 3.6k LOC**

| File | Content | LOC |
|---|---|---|
| `crates/hangar-app/src/views/pane_grid.rs` | custom layout widget, splitters (pointer and keys), drag-move with ghost and hint overlay, file-drop overlay, zoom slot | 750 |
| `views/terminal_pane.rs` | header (breakpoints by width), pick_lists, session button, queue, warnings, revive, actions, the "…" menu items, notes bar, search overlay | 1000 |
| `views/terminal_search.rs` | search bar | 220 |
| `views/pane_zoom_bar.rs` | zoom bar | 110 |
| `views/pane_gauge.rs` | context ring | 70 |
| `hangar-ui/src/state_glyph.rs` | glyph | 100 |
| `pane/controller.rs` | per-pane-id lifecycle: subscribe, adopt or replay vs spawn, launch (kits, account, mcp config, notes), resume (`mustResume`, `session_exists` in parallel with `launchDelayMs`), login, unmount (handover, stash queue, kill), restart = new id | 750 |
| `pane/agent_watch.rs` | activity watcher, trust look (600 ms), bridge event reducer, announce and notify rules | 450 |
| `pane/session_watch.rs` | `watchForSession` task | 120 |
| `pane/pty_link.rs` | core PTY channel ↔ `hangar_term` | 120 |

Needs from the shell (W2):
- core PTY manager functions: spawn, write, resize, kill(tree), alive, history{data, end, account}, has_children, plus a per-pane output channel `(String, u64 seq)` and an exit signal;
- a `Host::subscribe` stream for `bridge:event` and `bridge:queue`;
- `Message` variants per pane id;
- the store actions (`updatePane`, `respawnPane`, `setTree`, `movePane`, `closePane`, `addPane`);
- per-workspace `focusedPaneId` and `zoom` maps;
- window focus state;
- the shortcut dispatcher (`terminal.*`, `pane.*`, `command.runN`);
- clipboard, i18n `t()` with plurals, settings and theme lookup;
- `PaneMenu` / `CommandMenu` / `CommandDialog` / `DiffView` / `OrchestrationBadge` / `PaneAccountBadge` from other packages;
- desktop notifications;
- window `FileHovered` / `FileDropped` and cursor events.

**WP-L: pure logic and stores (with ported tests), about 2.6k LOC plus about 2k LOC of tests**
- `logic/layout.rs` (380, 36 tests), `logic/hibernate.rs` (300, 43 tests), `logic/hibernation_sweep.rs` (260, 10 tests), `logic/file_drop.rs` (140, 12 tests), `logic/agents.rs` (200, 9 tests), `logic/pane_names.rs` (110, 5 tests), `logic/agent_pane.rs` (90, 6 tests), `logic/context_gauge.rs` (70), `logic/shells.rs` (20), `pane/status.rs` (60, 7 tests), `pane/activity.rs` (110, 12 tests).
- `state/agent_state.rs` (200, 2 tests), `state/pane_insight.rs` (230, 11 tests), `state/hibernation.rs` (130), `logic/pane_branch.rs` (110, 9 tests), `windows.rs` handover and launch ownership (120, 6 tests), `pane/bridge_events.rs` (serde `BridgeEvent` + queue carry; 120, 4 tests).
- `contrast` (150, 16 tests) goes to `hangar-theme`; `typography` (60, 8 tests) goes to `hangar-i18n`.
- Needs: a clock abstraction (`now_ms`) for timers, so the tests stay deterministic; the `types` port (`Pane`, `SplitNode`, `Settings`, `ShellInfo`, `AgentId`) from the state skeleton.

**Ordering:** WP-L and WP-T can start in parallel right away. WP-P needs W2.shell and the WP-T API (it can stub against `lib.rs`). PaneGrid can land before TerminalPane using a placeholder pane view.

**Decisions I made without asking:**
- Resize is coalesced natively (the JS side does none).
- Web links open in the system browser.
- OSC 0/2 titles stay ignored.
- WebGL parking is dropped.
- The launch-note dismiss strings, hard-coded in EN/FR today, move into `hangar-i18n`.

### A3. Inspector, git surfaces, memory, MCP, orchestration, stats

#### 3. Suggested work packages

**WP0 (prerequisite, roughly 400 LOC; owned by the shell or whoever goes first):** `hangar-app/src/fmt/{mod.rs, number.rs, time.rs}`, an en/fr formatter replacing `Intl`:
- numbers: compact, currency `narrowSymbol`, percent with sign, integer grouping (NNBSP in fr)
- dates and times: `h23` clock, prose time, short date, weekday names, `formatSince`, `resetWhen`.

Plus i18n plurals (`_one`/`_other`) and interpolation. Plus theme tokens `viz_series[4]` and `viz_heat[6]` in hangar-theme.

| WP | Files (crates/hangar-app/src/…) | Sources | Rust LOC est. | Needs from shell / shared widgets |
|---|---|---|---|---|
| **WP1: Inspector** | `inspector/mod.rs` (state, update), `inspector/view.rs` (panel, grip, drawer wrapper), `inspector/header.rs`, `inspector/sections.rs` (session, terminal, context, task, queue, activity), `inspector/files.rs` (+ git poll), `inspector/files_model.rs`, `inspector/format.rs` | 1268 TSX + 333 lib | **~1,700** + ~300 tests | **Message:** `Inspector(InspectorMsg{ToggleSection(SectionId), GripPressed, GripDragged(f32), GripReleased, ResetWidth, NudgeWidth(i16), Close, OpenBoard, OpenSessions(PaneId), OpenMcpServers(WsId), CancelQueued(PaneId, String), OpenDiff(WsId, PaneId), GitChanges(Folder, u64 seq, Result<Changes>), Tick})`. **State:** `settings.inspector_{open,width,collapsed}`, App `drawer: Option<Drawer>`, focused pane, pane-insight / activity / plan-limits stores, board task cache by cwd. **hangar-ui:** `foldable_section`, `kv_grid`, `tail_text` (leading ellipsis), `ring_gauge` (canvas), `meter`, `diff_bar`, `kbd_chip`, `state_glyph`, `icon_button`, `resize_grip` |
| **WP2: Git surfaces** | `git/diff_view.rs`, `git/diff_parse.rs`, `git/worktrees/mod.rs` (state + busy enum), `git/worktrees/view.rs`, `git/worktrees/create.rs` (plan resolution + create flow), `git/worktrees/cleanup.rs`, `git/worktrees/rows.rs`; naming in `hangar-core::worktree_naming` | 2006 TSX + 411 lib + 143 strings | **~2,500** + ~150 tests | **Message:** `OpenDiff{cwd,label}`, `CloseDiff`, `Worktrees(WtMsg…)`, `OpenWorktrees(WsId)`. **Store actions:** `add_pane`, `add_workspace`, `set_active_workspace`; reads `worktree_hooks`. A modal stack with Esc and click-outside. **hangar-ui:** `diff_table` (virtualised, monospace, two number gutters), `grouped_select` (optgroups + disabled options), `segmented`, `badge(variant)`, `notice(tone)` with an action row, `checkbox`, `keyboard_list` |
| **WP3: Memory, MCP, Orchestration** | `memory/{mod.rs, view.rs, guards.rs, format.rs}`, `mcp/{install_panel.rs, servers_panel.rs, scope.rs}`, `orchestration/{panel.rs, badge.rs, form.rs, actions.rs}`; `hangar-core::orchestration` (pure) | 1490 TSX + ~1,060 lib | **~2,600** + ~450 tests | **Message:** `MemoryChanged` (subscription), `Memory(MemMsg)`, `OpenMcpInstall`, `McpInstall(...)`, `OpenMcpServers(WsId)`, `McpServers(...)`, `RestartPanes(WsId, Vec<PaneId>)` (→ `replace_pane`), `OpenOrchestration(WsId)`, `OrchSave`, `CreateOrchestrator(WsId)`, `Notice`. **State:** `update_workspace(mcp\|orchestration)`, pane insight `mcp`, pane activity. **hangar-ui:** `text_editor` field, chips, `toggler` switch, `stepper`, `combo_box`, skeleton rows, `form_grid` layout, stat tiles |
| **WP4: Stats** | `stats/{mod.rs (store, refresh Task, 60 s freshness), model.rs, view.rs (bar, KPIs, cards, method), ranked.rs, sessions.rs, tables.rs}`; **`hangar-ui/src/charts/{columns.rs, heatmap.rs, sparkline.rs, readout.rs, ticks.rs}`** | 1395 TSX + 732 lib | **~2,300** (≈900 of it charts) + ~350 tests | **Message:** `Stats(StatsMsg{SetPeriod, SetMeasure, SetProject(Option<String>), ToggleDailyTable, ToggleHeatTable, ToggleAllProjects, OpenProjectMenu, Refresh, Loaded(Result<UsageStats>), WorktreesLoaded(HashMap<WsId, Vec<String>>), ChartHover(ChartId, Option<idx>), ChartKey(Key)})`. **State:** a window-lifetime stats cache (data, refreshing, error, checked_at) and window-lifetime filter choices. **hangar-ui:** canvas `StackedColumns`, `Heatmap`, `Sparkline`, a `Readout` overlay, `nice_ticks`, `segmented` radiogroup, `toolbar_menu` dropdown, `ranked_bar_row` |

**Dependencies between packages**
- WP2 owns DiffView; WP1 only emits `OpenDiff(ws, pane)`, which the shell routes to WP2's view.
- WP3 owns `needs_restart` and MCP scope logic; WP1 only emits `OpenMcpServers`.
- WP1 and WP3 both need the `board_tasks(cwd)` cache plus the `board:changed` subscription. Put it in a shared store module owned by the board work package.

**Assumptions**
- The native app calls hangar-core functions directly instead of `invoke`. Commands that take an `AppHandle` get thin native adapters.
- Inspector focus restoration and drawer focus handling are simplified, because iced has no DOM focus model.
- Accessibility live regions are not ported.

### A4. Kits, preview, 3D

#### 4. Work packages

| WP | Owns (new files) | Rust LOC (code + tests) | Needs from the shell (W2) |
|---|---|---|---|
| **K1: Kits management UI** | `hangar-app/src/model/{kits.rs, kit_activation.rs, kit_setup.rs, kit_gallery.rs, kit_icon.rs}`; `view/settings/{kits.rs, kits_gallery.rs}`; `view/kits/{activation.rs, workspace_section.rs, icon.rs}` | about 900 logic + 2,000 views; about 1,600 tests (45+82+11+35+4, plus view tests from KitsSettings 18 and KitActivation 6) | `KitsState` slice (installed `Vec<KitInfo>` kept current by core `kits:changed`); store actions `add_pane`, `close_pane`, `respawn_pane`, `set_active_workspace`, `update_settings(kits_everywhere)`, workspace `kits` edit; hibernation set; hangar-ui modal (stackable, its own Escape, "peek" dock mode), switch, buttons (primary/ghost/danger/tiny), tag/chip, notice, text input/search; `rfd` folder picker; i18n with placeholders and plurals; core kit API (`kits_list`, `kit_add_*`, `kit_update`, `kit_check_update`, `kit_remove`, `kit_inspect`, `kit_setup_*`, `kit_gallery_list`); PTY `alive`/`write`. The WorkspaceDialog package mounts `workspace_section.rs`. |
| **K2: Kit tab host and contract** | `model/{kit_api.rs, kit_tabs.rs}`; `view/preview/kit_tab.rs`; `hangar-app/src/kits/webview_bridge.rs`; the `KitApi` impl adapter in `hangar-kit-host` (coordinate with W1.kits); kit-relay messages in the `hangar-webview` protocol (coordinate with W1.webview) | about 1,300 logic + 450 view/bridge; about 2,100 tests (104 + 37) | Core: `kit_files_*`, `kit_file_read`, `kit_workspace_path`, `kit_data_*`, `kit_loopback_fetch`, `kit_command_run`, `bridge_enqueue`, notifications, `preview::inspect`, `model_allow_roots`; pane activity (`bridged`, state); theme tokens (`deriveChromeTokens` port in hangar-theme) for the `theme` event; the WP P1 preview store (`open_kit_tab`, params watchers, title/badge); a webview helper client handle. It must expose `kit_tab_to_open`, `read_kit_tab_params` and `kit_tab_listing` to the MCP control package. |
| **P1: Preview panel and 2D viewers** | `model/{preview.rs, preview_store.rs}`; `view/preview/{panel.rs, tabs.rs, file_view.rs, image.rs, text.rs, code_lines.rs, highlight.rs, markdown.rs, media.rs, pdf.rs, html.rs, parts.rs, file_watch.rs}` | about 1,200 logic + 2,300 views; about 1,400 tests (26 + 64 + about 3 kept from modelHost) | A side-panel region with docked, drawer and expanded modes, width in settings (`modelPanelWidth`), focus return to the focused terminal, F6 region; the active workspace per window; hangar-ui tab strip, toolbar tool buttons (pressed state), select, tooltip, toolbar menu, StageMessage, Banner, progress bar; clipboard; `open`/`opener`; `two-face`; helper launch for pdf, html and video; kit-tab persistence file; MCP control hooks (`show`, `reveal`, `close`, `when_settled`, `list`). |
| **P2: Native 3D viewer** | new crate `crates/hangar-model3d/` (`formats.rs`, `framing.rs`, `stats.rs`, `material.rs`, `load/{gltf,obj,stl,ply}.rs`, `scene.rs`, `orbit.rs`, `renderer.rs`, `offscreen.rs`, `widget.rs`, `shaders/{pbr,grid,lines,wire}.wgsl`, `env/room.ktx2`); `hangar-app/src/view/preview/model3d.rs` (toolbar, animation bar, info strip, loading and error states) | about 4,800 crate + 700 app view; about 600 tests (16 + 9 + 7 + loader fixtures) | The preview store from WP P1 (`settle_model`, `reload_model`, the snapshotter registration API); `ViewerTheme` from hangar-theme tokens; the `rfd` save dialog and `arboard`; the panel region from P1 (the widget goes in P1's body slot); the iced `wgpu` backend; an MCP snapshot entry point. Can start right after W2, in parallel with P1 once `preview_store.rs` types are frozen. |

- **Order:** P1 freezes the `preview_store` types first, so K2 and P2 can build on them. K1 is independent.
- **Shared contracts to pre-declare in the shell:** the `PreviewMessage`, `KitsMessage`, `KitTabMessage` and `Model3dMessage` enum variants; the `KitHost` trait signature used by both webview and wasm tabs; the `WebviewClient` handle.

### A5. Board and settings

#### 5. Suggested parallel work packages

**WP-1 Board (~2,500 Rust LOC)**
- Files under `crates/hangar-app/src/board/`:
  - `mod.rs`: BoardState, Message, update, subscriptions (~550)
  - `view.rs`: bars, columns, empty state, skeleton, full and dock variants (~450)
  - `card.rs`: card, priority, assignee (~280)
  - `filter.rs`: listbox dropdown (~200)
  - `task_dialog.rs` (~550)
  - `dock.rs`: grip and resize (~180)
  - `model.rs` (~180), `format.rs` (~90), `drag.rs` (~80), with the 23 test cases ported
- From the shell:
  - Messages: `Board(board::Message)`, `FocusAgentPane{ws, pane}`, `OpenMcp`, `SetView(Board)`, `CloseDock`, `SetBoardDockWidth(f32)`, `BoardChanged(cwd)`.
  - State: `active_workspace.cwd`, `workspaces`, `active_workspace_id`, `side_panels.dock{width, max, fits}`, `drawer`, `ui_scale`, `focused_pane_id`, `pane_activity` map.
  - A hangar-core event bus that replaces `tauri::Emitter` for `board:changed`.

**WP-2 Settings shell, schema and Shortcuts (~2,600 LOC)**
- Files under `crates/hangar-app/src/settings/`:
  - `mod.rs`: SettingsState `{category, query, armed_actions, custom-card sub-states}`, Escape and nav keys (~450)
  - `schema.rs`: the declarative item enum with `ItemId` → `Changed(ItemId, Value)` applied in update, about 70 rows (~850)
  - `rows.rs`: control renderers (~250)
  - `previews.rs`: theme, font, terminal (~300)
  - `shortcuts.rs`: editor and recorder (~550)
  - `search.rs`: fold and filter (~80)
  - `../experiments.rs` (~120)
- From the shell:
  - Messages: `OpenSettings(Option<CategoryId>)`, `CloseSettings`, `UpdateSettings(SettingsPatch)`, `PatchWorkspace(id, WorkspacePatch{theme_id, shell_id, mobile_ports})`, `ResetSettings` (merge semantics).
  - State: `settings`, `shells`, `app_version`, the resolved `keymap`, and a `shortcuts_suspended` counter checked by the global key dispatcher.
  - A raw keyboard subscription for the recorder.
  - The folder dialog through `rfd`.

**WP-3 Mobile, Voice and Discord cards (~1,950 LOC)**
- Files under `crates/hangar-app/src/settings/`:
  - `mobile.rs`: view blocks (~600)
  - `mobile_ctl.rs`: state, Tasks, subscriptions (~300)
  - `mobile_model.rs` + tests: the 20 remote cases plus view-state tests (~350)
  - `voice_card.rs` (~380)
  - `discord_card.rs` (~180)
  - `../discord.rs`: `build_presence` (~150)
- From the shell:
  - Messages: `Settings(Mobile(..) | Voice(..) | Discord(..))`, `RemoteChanged`, `VoiceDownload(Progress)`.
  - Tick subscriptions, active only when the card is visible: 1 s countdown, 6 s flash, 2 s Discord poll, 1 s Discord elapsed.
  - Handles to the hangar-core remote, voice (models, keychain) and discord services.
  - `open::that` for URLs.
  - Note: React only mounts the active category's cards, so their loads must fire when the category is shown.

**Shared widgets needed in `crates/hangar-ui`** (currently a 4-LOC stub):
- Inputs: toggle switch; styled select / pick_list with chevron; segmented control; slider with filled track and value label; text input (plain, password, numeric, multiline); number stepper 0–9.
- Buttons and confirmation: two-step "armed" confirm button (danger and primary); key-capture recorder and kbd keycaps.
- Overlays: listbox dropdown with keyboard navigation (BoardFilter-style; can also back the select); modal and backdrop with focus trap and Escape; toast.
- Board-specific: draggable card via mouse_area with the press threshold; column drop zones; vertical resize grip (drag, arrow keys, double-click reset); skeleton shimmer; StateGlyph.
- Display: chips; status dot; progress bar; SVG from memory (QR); image (logo).
- Icons: pause, lock, clock, link, bubble, plus, chevronRight, filter, expand, close, plug, board, check, trash, updown, search, reset, sliders, appearance, terminal, prompt, bell, broadcast, bolt, mic, gamepad, keyboard, info.

**Assumptions**:
- The hangar-core services are exposed in-process with broadcast channels replacing Tauri events.
- Task dialog dates use a validated text field.
- Keyboard focus on cards and focus restore depend on a hangar-ui focus manager; they can ship later.

### A6. Control engine: shared contracts (WP0) and packages

#### 4. Proposed split into 3 parallel work packages

##### WP0: shared contracts (written first by WP1, about half a day; all three code against them)

```rust
// hangar-core::control (backend agent de-Tauris it): the window hop becomes an in-process channel
pub enum Channel { Agent, Internal }
#[derive(Clone)] pub struct Reply(Arc<Mutex<Option<oneshot::Sender<Result<Value, String>>>>>); // the first send wins
pub struct ControlRequest { pub id: String, pub op: String, pub args: Value, pub channel: Channel, pub reply: Reply }
pub fn install(tx: mpsc::UnboundedSender<ControlRequest>);   // replaces emit("control:request" | "control:internal")
// ask()/ask_internal()/ask_internal_via() keep OPERATIONS, the per-op timeouts and ControlError:
// a timeout drops the receiver; a dropped Reply that never sent → Unavailable("the window dropped the request").
// control_reply and the `pending` map go away (or stay as a thin shim for tests).
// The core event bus (backend agent): broadcast::Sender<CoreEvent> {BoardChanged(cwd), Bridge{pane,event},
// PtyExit(pane), Shared(Share), RemoteChanged, MemoryChanged}, replacing the Tauri emits at server.rs:66/370,
// pty.rs:525, remote/mod.rs:102/105, lib.rs board commands and memory.rs:653.
```

```rust
// hangar-app/src/control/port.rs: ControlDeps 1:1, iced-free, Send + Sync, generic (no dyn async)
pub struct Snapshot { state: Arc<AppState>, opened: HashSet<WsId>, asleep: HashSet<WsId>,
                      activity: HashMap<PaneId, PaneActivity>, delivering: HashMap<WsId, HashSet<TaskId>> }
pub enum ModelCmd { AddPane{ws, agent, near, dir, cwd, name, init: PanePatch /* role, taskId, model, portBase, sharedFolder: atomic, before the PTY spawns */},
  ClosePane{ws,pane}, RespawnPane{ws,pane,patch}, UpdatePane{ws,pane,patch}, UpdateWorkspace{ws,patch},
  OpenWorkspace(ws), Touch(ws), Activate{ws,pane}, Detach(ws), Reattach(ws), Announce(ControlEvent),
  CreateWorkspace{..}, RemoveWorkspace(ws), BeginDelivery{ws,task}, EndDelivery{ws,task} }
pub trait AppPort: Clone + Send + Sync + 'static {
  fn snapshot(&self) -> Arc<Snapshot>;                                   // watch::Receiver / ArcSwap, republished after every update()
  fn apply(&self, cmd: ModelCmd) -> impl Future<Output = Result<CmdOut, String>> + Send; // Message::Model(cmd, Reply) → update() → reply
  fn terminals(&self) -> &TerminalHub;  // hangar-term: has(pane), read_lines(pane, n) (readLines semantics), paste(pane, s) (bracketed if DECSET 2004), app_cursor(pane)
  fn preview(&self, ws: &WsId) -> impl PreviewHost;                      // list/show/reveal/reload/when_settled/snapshot/close
  fn sleep(&self, ms: u64) -> impl Future<Output = ()> + Send;          // tokio::time::sleep; a fake in tests
}
// Shared enums: Caller, ControlEvent, OrchestratorEvent, JournalEntry, ManagerEvent/BlockEvent (serde, kind-tagged)
```

Core services are called directly (no invoke): `pty::{write,alive,kill}`, `bridge::enqueue(pane,text,from)`, `board::*`, `git::*`, `delivery::{preflight,run,cleanup,running}`, `memory::load`, `stats::usage`, `chat::page`, `model::{allow_roots,stat}`, `preview::inspect`, `kits::list`, `accounts::list`, `remote::{push, push_subscribers, ports_listening, activity_append, activity_read}`. In tests, a `FakePort` writes log lines like the TS fakes (`"open:w2"`, `"touch:w1"`), so the TS tests port one to one.

**Async shape in the app shell:**
1. `main`: `let (tx, rx) = mpsc::unbounded_channel(); hangar_core::control::install(tx)`. `rx` is parked in a `OnceLock<Mutex<Option<_>>>` and drained by `Subscription::run(control_stream)`, which yields `Message::Control(ControlRequest)`. `Reply` is `Arc`-wrapped so `Message` stays `Clone + Debug`.
2. `update(Message::Control(req))`: before hydration, `req.reply.send(Err("Hangar.AI is still starting…"))`. Otherwise `tokio::spawn(engine.handle(req, port.clone()))`. update never blocks; the handler sends the reply itself.
3. `handle` dispatch:
   - `Channel::Agent` → `control::run` (gated by `agentPaneControl`).
   - `Channel::Internal` + `orchestrator_briefing` → `orchestration::brief`.
   - `remote_*` → `mobile::run`, as the person.
   - Anything else → `Err("Unknown …")`.
4. Model writes from the handler: `port.apply(cmd)` sends `Message::Model(cmd, Reply)` through a second mpsc and Subscription. update applies it synchronously, republishes the `Snapshot`, and replies with the new pane id and so on. Engines never touch the model directly.
5. One `OrchestrationRuntime` tokio task, started on hydration, owns every piece of TS module state as plain structs (no statics):
   - event engine, manager feed, push, activity log;
   - `starting`, `inFlight`/`settled`, `landings`, `resuming`/`live`/`firstSeen`, the usage cache.

   Its inputs are `select!` over the `CoreEvent` broadcast, `EngineMsg` (Post, BoardChanged from in-app writes, queries carrying oneshot replies for journals) and `sleep_until(min next_deadline)`.

   Engines are written as sync state machines (`on_*(&mut self, now) -> Vec<Effect>` plus `next_wake()`), and the runtime carries out the effects: `Enqueue{pane,text,from}`, `Push(Message)`, `Append(entries)`. This keeps the TS fake-timer tests deterministic.

   Ticks: 10 s (engine tick, then push refresh, then manager tick) and 5 s (activity log).

   UI actions send `EngineMsg` too: pin/unpin (`announceRole`), « Fusionner » (`landDelivery`), « Arrêter les workers », person-started `spawnWorker`.

##### Packages

| WP | Scope (TS source → Rust) | Files under `crates/hangar-app/src/` | Rust LOC (source + tests) | Needs from the shell |
|---|---|---|---|---|
| **WP1: control bus + agent ops** | control.ts, managerControl.ts, deliveryControl.ts, deliveryRuns.ts (2,372 TS LOC; tests 2,853, 154 cases) + WP0 | `control/mod.rs` (bus stream, dispatch, gating, caller and workspace resolution, `resolvePath`), `control/port.rs`, `control/types.rs`, `control/panes.rs` (pane_*, workspace_list/window/set_check, keys), `control/preview.rs` (model_*/preview_*, `admit`, `sight`), `control/kit_tabs.rs`, `control/workspace.rs` (workspace_create glue, `tell_when_waiting`), `control/manager.rs`, `control/delivery.rs` (+ shared comment consts in `control/delivery_text.rs`) | about 2,900 + 3,100 = **about 6,000** | `Message::Control`, `Message::Model(ModelCmd, Reply)` with the update handlers, Snapshot publishing, `TerminalHub` (read, bracketed paste, app-cursor flag), `PreviewHost` per workspace, notice toast for `Announce`, `model.delivering` |
| **WP2: orchestration engine** | workerControl.ts, orchestratorEvents.ts, orchestratorBrief.ts, managerEvents.ts (3,024; tests 2,384, 107 cases) + projects.ts (467, out of scope but required) | `orchestration/mod.rs`, `orchestration/runtime.rs` (the tokio task, `EngineMsg`, CoreEvent fan-in, ticks), `orchestration/worker.rs`, `orchestration/events.rs`, `orchestration/brief.rs`, `orchestration/brief_text.rs` (verbatim prompts), `orchestration/manager_feed.rs`, `orchestration/projects.rs` | about 3,600 + 2,600 = **about 6,200** | runtime spawned on hydration; the `CoreEvent` broadcast from core; `EngineMsg` sender in app state (pin/unpin → `announce_role`, « Arrêter les workers »); `AddPane{init}` atomic so the worker's PTY starts with `HANGAR_PORT_BASE` |
| **WP3: Hangar Mobile window side** | remoteControl.ts, remoteAnswer.ts, remoteDelivery.ts, remoteApps.ts, remotePush.ts, activityLog.ts, remote/{types,calls,stats}.ts (3,327; tests 3,137, 153 cases) | `mobile/mod.rs` (`run`, 16 ops), `mobile/ops.rs` (state/screen/send/permission/manager/wake/workspace, `type`/`hold`/`wake`), `mobile/chat_live.rs` (`note_bridge_event`, pending, running), `mobile/answer.rs`, `mobile/delivery.rs`, `mobile/apps.rs`, `mobile/stats.rs` (`usage_reader`), `mobile/push.rs`, `mobile/activity_log.rs`; in core: `hangar-core/src/remote/contract.rs`, `hangar-core/src/chat/calls.rs` | about 3,850 + 3,250 = **about 7,100** | the same bus (`Channel::Internal`); runtime hooks for push and the activity log (owned by the WP2 runtime, WP3 supplies the state machines); `remote::Window` closures (`send` → control bus, `changed`/`shared` → CoreEvent); i18n `activity.*` and `typeset` |

**Order and coupling between packages:**
- WP3 calls `control::panes::{pane_read, pane_send}` (WP1) as the person, and `control::delivery::{land_delivery, delivery_state}` (WP1).
- WP2's `worker`/`delivery` modules post `OrchestratorEvent`s to the WP2 runtime.
- WP1 `project_*` needs `orchestration::projects` (WP2).

Freeze the WP0 signatures plus the `projects::load(port) -> Vec<ProjectSummary>` and `land_delivery(ws, task, port)` signatures before starting.

**Problems found while reading:**
1. **The mobile app is probably not embedded.** `assets.rs` has `#[folder = "../dist-mobile/"]`, which is relative to `crates/hangar-core`, so it points at `crates/dist-mobile/`. Vite writes `<repo>/dist-mobile/`. Because of `allow_missing`, the build quietly serves the placeholder page. Fix: use `"../../dist-mobile/"`, or change the Vite `outDir`.
2. deliveryControl writes `board:changed` itself; in native, call `engine.board_changed()` directly, or have core `board` writes always publish `CoreEvent::BoardChanged`.
3. `pane_send` waits 120 ms between paste and Enter because xterm's paste and `pty_write` took different paths. In native, write both through the same PTY writer but keep a short delay: Claude Code's TUI needs the paste to end before Enter.
4. The briefing's `isOpen` guess in `defaultDeps` ("any pane has activity") differs from App's `openedIds`. In native, use the model's opened set everywhere.
5. Several timeouts are tied together: the phone waits 12 s, the server 10 s, and the window answers within 8 s (`OPEN_WAIT_MS`), 7 s (`LAND_WAIT_MS`) and 6 s (`STATS_WAIT_MS`). Keep them consistent.
6. Detached workspaces: with iced multi-window in one process, the `model:*` relay and `remoteTerminal` go away. The `TerminalHub` and `PreviewHost` must reach panes in any window.
