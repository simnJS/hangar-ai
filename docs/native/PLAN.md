# Hangar, native — the plan

This branch (`feat/native-rust`) rewrites Hangar.AI as a native Rust application.
This file is the map: what is being built, why it is built this way, in what
order, and how to pick the work up with no other context. Read it first, then
[STATUS.md](STATUS.md) for where things stand today.

Companion documents, in this folder:

| File | What it holds |
| --- | --- |
| [STATUS.md](STATUS.md) | The log: what has landed, what is running, decisions taken along the way, what is next. Updated after every merge. |
| [inventory-frontend.md](inventory-frontend.md) | Every surface, state slice, logic module and setting of the React app, with the TS files they come from, and the work packages for porting them. |
| [core-api.md](core-api.md) | Every former Tauri command and event, and the Rust function or `Event` variant that replaces it in `hangar-core`. |
| [kits-native.md](kits-native.md) | The contract of native kit tabs: manifest, WebAssembly ABI, widget catalogue, host API, sandbox. |

## 1. The goal, in the owner's words

> Recoder Hangar en Rust natif dans une branche, que du Rust, plus de webview.
> Hangar devra quand même pouvoir lire des webviews si besoin, pour des kits qui
> en dépendent à 100 %, ou pour des PDF et les trucs comme ça. Pour les kits, une
> lib graphique native. Tout doit être natif.

Decisions the owner took before the work started (2026-11-03):

| Question | Answer |
| --- | --- |
| GUI toolkit | **iced** (0.14) |
| How a kit draws a native interface | **Rust compiled to WebAssembly** (the "plugins Rust compilés ou WASM" option; see 4.6 for why it is WASM only) |
| Scope | **Maximum parity, in parallel**: many agents at once rather than a thin slice first |
| Where | `~/dev/hangar-native`, branch `feat/native-rust`, pushed to `hangar-ai/hangar` regularly (no pull request until the owner asks) |

House rules that apply to every commit on this branch:

- No `Co-Authored-By: Claude` trailer and no mention of Claude in commits or pull requests (the owner removed one before and asked for none).
- Comments explain *why*, in full sentences, as the existing code does. Read a few files of `crates/hangar-core/src` before writing any.
- The owner may be away: never block on a question. Decide, write the decision and its reason in STATUS.md, and move on.

## 2. What Hangar is (for someone who has never seen it)

Hangar.AI is a desktop app (Windows, macOS, Linux) that runs CLI coding agents
(Claude Code, Codex, Gemini CLI, OpenCode) and shells side by side, each in a
real terminal pane, up to 16 per workspace. Around the terminals:

- a **task board** per workspace that agents read and write over Hangar's **MCP server** (atomic claims), and a shared **memory**;
- **orchestration**: one agent plans, workers run in git worktrees, deliveries are checked, reviewed by a separate agent and merged locally (`delivery.rs`, `orchestrator.rs`);
- a **project manager** agent that follows every workspace (HQ);
- an IDE-like window: navigator (left), pane grid, inspector (right), status bar, command palette, 33 themes, rebindable shortcuts, English and French;
- the **Claude Code bridge**: a Claude Code plugin Hangar installs (`claude-mod/hangar-bridge`) that reports turns, permission prompts, context fill and plan usage;
- **kits**: packages that bring a domain into Hangar (Claude Code plugin + tabs + setup commands), see `docs/kits.md`;
- **Hangar Mobile**: a phone web app served by Hangar over the owner's Tailscale network (`remote/`, `src/mobile/`);
- usage statistics, voice dictation (whisper.cpp), Discord presence, 3D model and file previews, an updater.

Until this branch, it was Tauri 2 (Rust backend, ~30k lines) and React 19 with
xterm.js (frontend, ~100k lines of TS/TSX including tests, ~18k lines of CSS).

## 3. Architecture

```
                         ┌────────────────────────────────────┐
                         │ hangar-app  (bin "hangar-ai")       │
                         │ iced daemon: windows, state, views  │
                         └──┬──────┬──────┬──────┬──────┬─────┘
                            │      │      │      │      │ spawns, JSON lines
              ┌─────────────┘      │      │      │      └──────────────┐
              ▼                    ▼      ▼      ▼                     ▼
       ┌────────────┐   ┌──────────┐ ┌─────────┐ ┌───────────────┐ ┌────────────────┐
       │ hangar-core│   │hangar-ui │ │hangar-  │ │hangar-kit-host│ │ hangar-webview │
       │ no GUI:    │   │widgets   │ │term     │ │wasmtime +     │ │ separate proc: │
       │ board, git,│   │on iced   │ │alacritty│ │renders kit-ui │ │ tao + wry, for │
       │ MCP, PTYs, │   └────┬─────┘ │_terminal│ └──────┬────────┘ │ HTML kit tabs, │
       │ kits, mobile│       ▼       └─────────┘        ▼          │ PDFs, web pages│
       └────────────┘   ┌────────────┐          ┌──────────────┐ └────────────────┘
                        │hangar-theme│          │hangar-kit-ui │◄── hangar-kit-sdk
                        │33 themes,  │          │widget tree + │    (in the kit, wasm32)
                        │tokens      │          │events (data) │
                        └────────────┘          └──────────────┘
```

One process draws everything with iced (wgpu, falling back to tiny-skia). The
only browser engine is in `hangar-webview`, a different executable, started on
demand. The main executable does not link a webview library at all.

### 3.1 Crates

| Crate | Kind | Responsibility | Depends on |
| --- | --- | --- | --- |
| `hangar-core` | lib | Everything that is not drawing: the former `src-tauri/src`, moved with `git mv` and freed of Tauri. Board, memory, git and worktrees, delivery, orchestrator, MCP server (HTTP + stdio), MCP scopes and installs, agent instructions, sessions, context, stats, pricing, accounts, Claude Code bridge, control, chat transcripts, kits (manifest, store, sources, gallery, setup, hooks, data, files, commands, loopback), kit pages (`kit_scheme`), Hangar Mobile server, notifications, Discord, voice, shells, PTYs, persisted state. | none of ours |
| `hangar-theme` | lib | The 33 themes and design tokens as data; mapping onto `iced::Theme` and per-widget style functions; terminal colours; fonts. | iced |
| `hangar-ui` | lib | Hangar's widgets: icons, buttons, menus, context menus, dialogs, toasts, tooltips, the command palette, split panes, tabs, form rows for the settings. Views never pick colours themselves. | iced, hangar-theme |
| `hangar-term` | lib | The terminal widget: alacritty_terminal emulation, native drawing, keyboard/IME/dead keys, mouse, selection, links, search. PTY-agnostic. | iced, alacritty_terminal |
| `hangar-kit-ui` | lib | The native interface library of kits: the widget tree and events, as serde data. Compiles to wasm32. A public contract. | serde |
| `hangar-kit-sdk` | lib | What a kit author links: `Kit` trait, export macro, typed host calls, view builders. Compiles to wasm32. | hangar-kit-ui |
| `hangar-kit-host` | lib | Runs kit tabs in wasmtime with limits and declared permissions; renders `hangar-kit-ui` trees with iced; `KitApi` trait the app implements. | wasmtime, iced, hangar-kit-ui (then hangar-theme, hangar-ui) |
| `hangar-webview` | lib + bin | Lib: protocol + client the app uses (serde only). Bin (feature `engine`): tao + wry window(s), custom-scheme requests answered by the parent, `window.hangar` bridge for HTML kits. | serde; wry, tao behind `engine` |
| `hangar-i18n` | lib | The interface strings, English and French, generated from the old `src/i18n.ts` and `src/strings/*` with the same keys; plurals, placeholders, French typography. | none |
| `hangar-model3d` | lib | The 3D model preview replacing three.js: glTF/GLB, OBJ, STL, PLY loaded off the UI thread, drawn with wgpu in an iced `shader` widget, orbit camera, framing, grid, PNG capture. | iced (its wgpu) |
| `hangar-update` | lib | The self-updater replacing `tauri-plugin-updater`: same `latest.json`, same minisign key, per-platform install and relaunch. | ureq |
| `hangar-app` | bin `hangar-ai` | The application: iced daemon (multi-window), global state (port of `store.tsx` and `src/lib/*`), every view (port of `src/components/*`), i18n, wiring to core events, PTYs to terminals, kits to kit-host or webview. `hangar-ai --mcp` stays the stdio MCP server. | all of the above |

### 3.2 Runtime model

- `main` checks `--mcp` first (stdio MCP server, no window: `hangar_core::run_mcp()`), as the old `main.rs` did.
- Otherwise it starts `hangar_core` (the old Tauri `setup`: PATH warm-up, bridge plugin, kit and account watchers, the board API server and its endpoint file, Hangar Mobile) and runs an `iced::daemon` with the `tokio` feature, so core's async code and iced's tasks share one runtime.
- The window calls core functions directly: synchronous ones inside `Task::perform` on a blocking thread when they touch disk or git, async ones as futures. There is no IPC and no serialization between window and core.
- Core events (`board:changed`, `memory:changed`, `control:request`, `bridge:event`, voice, remote, kits, accounts…) arrive through `Host::subscribe()` (a `tokio::sync::broadcast`), wrapped in one `iced::Subscription`.
- PTY output does not go through that bus: each pane gets its own channel from core's PTY manager into its `hangar_term::Terminal`, and keystrokes go back the same way. That path is the one latency matters on.
- Multi-window: workspace windows (the old `windows.rs`) become iced windows of the same daemon; panes move between windows without restarting their processes.
- Shutdown does what the Tauri `Destroyed` handler did: kill PTYs, mark Hangar Mobile's activity stopped, cancel running deliveries, clear the endpoint file, stop Discord presence.

### 3.3 Compatibility the native app must keep

People already use Hangar; switching to the native build must not lose anything.

- **App identifier `com.simnjs.iabench`** and Tauri's exact folders per OS (`hangar-core/src/paths.rs`): `state.json`, boards, memory, kits, accounts, Hangar Mobile pairing, voice models all stay where they are.
- **`state.json` format**: read the same file, write it back so the old app could still read it (until the old app is gone, people may go back and forth).
- **Executable name `hangar-ai`**: MCP configs written into agents' settings (`install.rs`) point at it; `hangar-ai --mcp` must keep working at the same path after an update.
- **The endpoint file and the bridge plugin**: agents and the Claude Code mod find the running app the same way.
- **Kits**: every existing kit keeps working. HTML tabs open in `hangar-webview` with the same `window.hangar` API and the same permissions; native tabs are an addition (manifest schema rules in kits-native.md).
- **Hangar Mobile** keeps serving the same phone web app (`src/mobile`, built to `dist-mobile/`, embedded by `hangar-core/src/remote/assets.rs`). It is a web app because a phone browser shows it; it is not a webview inside Hangar and is out of scope for "no webview".

## 4. Decisions and their reasons

1. **iced 0.14** — the owner's choice. Elm architecture (state, messages, update, view); multi-window through `iced::daemon`; wgpu with a tiny-skia fallback; `advanced` for custom widgets (the terminal, the pane grid's drag targets), `canvas` for charts, `svg` for icons, `markdown` + `highlighter` for chat and file previews.
2. **Move, then edit** — the backend was moved with pure renames in one commit, then edited. Git's rename detection lets fixes made on `main` to `src-tauri/src/x.rs` still merge into `crates/hangar-core/src/x.rs`.
3. **The old app stays readable until the end** — `src/` (React) and `src-tauri/` (now only config, icons, installer) remain on the branch as the reference for behaviour, and `src-tauri` is excluded from the workspace. They are deleted in the last wave (section 6, wave 5).
4. **The terminal is ours, on alacritty_terminal** — `iced_term` exists but ties us to its iced version and its choices on input; the two Linux bugs that started this (latency, dead keys sticking) live exactly in the input and drawing paths, so we own them.
5. **The webview is a separate process** — keeps WebKitGTK/WebView2/WKWebView out of the main executable, isolates a crashing page, and works on Wayland, where a webview cannot be embedded as a child of a winit window. The cost: an HTML kit tab is a window of its own rather than a tab inside the grid. Embedding on Windows/macOS can come later behind the same protocol.
6. **Native kit tabs are WebAssembly** — a compiled plugin as a native dynamic library has no stable Rust ABI, cannot be sandboxed, and would need one build per OS and architecture. A kit compiled to `wasm32-unknown-unknown` is one file that runs everywhere, inside wasmtime limits, with only the host calls its manifest declares. That is the "Rust compilé" the owner chose, made safe.
7. **Kit UI is a data tree, drawn by Hangar** — a kit never draws pixels; it returns a `hangar-kit-ui` tree, Hangar renders it with its own widgets and theme, so kits look like Hangar and follow the theme for free.
8. **Same data, same identifier, same binary name** — see 3.3.
9. **Edition** — `hangar-core` stays on edition 2021 (moved code); new crates use 2024.
10. **Build cache** — sccache through an untracked `.cargo/config.toml` at the repo root (see section 7); agents' worktrees under `.claude/worktrees/` inherit it.

## 5. Porting the frontend

The React app is ported surface by surface into `crates/hangar-app`. The full
map is [inventory-frontend.md](inventory-frontend.md): it lists every surface and
logic module with its TS sources, and splits the work into packages that own
disjoint files. Principles:

- **Logic first, with its tests.** `src/lib/*.ts` modules are mostly pure functions with vitest tests; each becomes a Rust module under `hangar-app/src/model/` with the same tests as `#[test]`s. Behaviour is defined by those tests, not by guesswork.
- **State is one `App` struct, split by slices** that mirror `store.tsx`; every package adds its slice and its `Message` variants in its own module, behind enum variants the shell pre-declares.
- **Views per surface** under `hangar-app/src/view/`, built only from `hangar-ui` widgets and `hangar-theme` styles.
- **Strings**: `src/strings/*` (en, fr) become Rust tables with the same keys, so translations carry over.
- **Shortcuts**: the defaults of `src/lib/keys.ts`, rebindable, saved in the same settings.
- **Accessibility**: iced 0.14 has no screen-reader support yet (AccessKit integration is pending upstream); keyboard navigation (F6 regions, focus order) must still work. Recorded as a known gap.

## 6. Waves

Each wave's packages run in parallel, one agent per package, each in its own git
worktree and branch, merged into `feat/native-rust` by the coordinator.

**Wave 1 — foundations (started 2026-11-03, evening)**

| Package | Crates / files | Result |
| --- | --- | --- |
| W1.core | `hangar-core` | Compiles without Tauri, tests pass; `Host`, `Event`, `paths.rs`, PTY output channel; `core-api.md`. |
| W1.term | `hangar-term` | Terminal widget + `examples/shell.rs`. |
| W1.theme | `hangar-theme` | 33 themes, tokens, iced styles, fonts. |
| W1.kits | `hangar-kit-ui`, `hangar-kit-sdk`, `hangar-kit-host`, `examples/kits/hello-native` | Native kit tabs end to end; `kits-native.md`. |
| W1.webview | `hangar-webview` | Helper process, protocol, client, kit bridge, PDF finding. |
| W1.inventory | `docs/native/inventory-frontend.md` | The porting map and wave 3's packages. |
| W1.i18n | `hangar-i18n` | Strings generated from the TS dictionaries, translate API, ported tests. |
| W1.update | `hangar-update` | Updater compatible with the releases' `latest.json`. |
| W1.model3d | `hangar-model3d` | Native 3D preview (was three.js). |

**Wave 2 — the shell (after W1.theme and W1.core)**

| Package | Result |
| --- | --- |
| W2.ui | `hangar-ui`: the widget set every view uses. |
| W2.shell | `hangar-app` skeleton: daemon, windows, regions (navigator, toolbar, grid, inspector, status bar), state skeleton with every slice and `Message` variant pre-declared, core start/shutdown, event subscription, PTY → terminal wiring, i18n tables, settings load/save in the old `state.json` format. After this, Hangar opens and runs shells and agents in panes. |

**Wave 3 — surfaces, in parallel (after W2)**: the packages listed in
inventory-frontend.md section 7 (navigator, pane grid and terminal pane, inspector,
board, needs-you, command palette and menus, settings, orchestration, worktrees and
git, memory, stats, accounts, MCP panels, kits UI and kit tabs, preview, voice HUD,
project manager/HQ, Hangar Mobile settings, update banner and updater).

**Wave 4 — integration**: kit-host and the webview helper wired to real kits
(kit-editor, kit-guard, kit-ci from the `hangar-ai` organization), multi-window,
3D preview natively (wgpu), the updater (GitHub releases + signature check, as
`tauri-plugin-updater` did), packaging (`.deb`, AppImage, MSI/NSIS, `.dmg`, the
webview helper bundled next to the main executable), CI (`.github/workflows/ci.yml`
and `release.yml` moved from `src-tauri` to the workspace).

**Wave 5 — retiring the webview app**: once the parity checklist (section 8) is
green, delete `src/` (except `src/mobile`, moved to `mobile/`), `src-tauri/`,
`index.html`, the Vite configs and the desktop parts of `package.json`; the site
and the demo move or stay as their owner decides.

## 7. How to work on this (setup for a new agent or person)

```bash
git clone https://github.com/hangar-ai/hangar.git hangar-native
cd hangar-native && git checkout feat/native-rust
rustup target add wasm32-unknown-unknown        # native kit tabs
cargo install sccache --locked                   # optional, see below
cargo check --workspace                          # first build: several minutes
cargo test --workspace
scripts/native-dev.sh                            # the native Hangar, on its own data (see below)
cargo run -p hangar-term --example shell         # the terminal alone
```

Linux build dependencies: those of the old Tauri app cover it (`libwebkit2gtk-4.1-dev`
only for `hangar-webview --features engine`; `libasound2-dev` for voice; a C/C++
toolchain and `cmake` for whisper.cpp).

**Never run the native app on the installed Hangar's data** while porting: both
read the same `state.json`. `scripts/native-dev.sh` sets `HANGAR_DATA_ROOT` and
`HANGAR_ENDPOINT_FILE` so everything goes under `target/dev-home/` (or
`HANGAR_DEV_HOME`); run examples that start `hangar_core::Core` the same way.

Optional local cache, not committed (listed in `.git/info/exclude`):

```toml
# .cargo/config.toml at the repo root
[build]
rustc-wrapper = "/home/<you>/.cargo/bin/sccache"
jobs = 16
```

Working rules:

- One package, one worktree, one branch off `feat/native-rust`. A worktree made by the agent tooling starts from `main`: check `git merge-base --is-ancestor 44cfd25 HEAD` and, if it fails, `git reset --hard feat/native-rust` before anything else. Touch only the files your package owns, plus additions to the root `Cargo.toml` and `Cargo.lock`. Never a bare `cargo update`; keep `windows`/`windows-core` at 0.61 (see the cpal note in `crates/hangar-core/Cargo.toml`).
- Before handing in: `cargo fmt`, `cargo clippy -p <crate> --all-targets -- -D warnings`, `cargo test -p <crate>`.
- The coordinator merges with `git merge --no-ff`, runs `cargo check --workspace && cargo test --workspace`, updates STATUS.md, pushes.
- Reference for any behaviour: the TS in `src/` and its tests, the Rust in `crates/hangar-core`, `docs/kits.md`, `docs/kit-ui.md`, README.md.

## 8. Parity checklist (the definition of done)

The native app replaces the old one when all of these hold, on Linux first, then
Windows and macOS:

- [ ] Opens on the same data: workspaces, panes, boards, memory, settings, theme, language, shortcuts.
- [ ] Panes run Claude Code, Codex, Gemini CLI, OpenCode and every detected shell; resize, zoom, search, copy/paste, links, file drop, scrollback, broadcast, queued prompts, hibernation, session resume.
- [ ] Typing has no perceptible latency; US-International dead keys compose (`'` + `e` → `é`) and nothing sticks.
- [ ] Pane states from the bridge are exact (working, waiting, done, permission); context and plan gauges.
- [ ] Navigator, inspector (all panels), status bar, command palette, menus, F6 regions, every shortcut rebindable.
- [ ] Board (view, dock, filters, drag, task dialog, comments), needs-you, memory view.
- [ ] Orchestrator mode end to end: plan, worker spawn in worktrees, delivery checks and review, merge queue, events.
- [ ] Project manager (HQ).
- [ ] Settings: every setting of the old schema, including the Experimental page.
- [ ] Accounts (several Claude accounts), MCP servers panel and installs, agent instructions.
- [ ] Stats view with charts; diff view; worktree panel; git clone with consent.
- [ ] Kits: list, add from folder/git/library, update, setup commands, activation; HTML tabs in the webview helper with the same API and permissions; native WebAssembly tabs.
- [ ] Previews: images, text and code, video, PDF (webview helper), 3D models.
- [ ] Voice dictation (local and Groq), its HUD.
- [ ] Hangar Mobile: pairing, devices, Tailscale, push, keep-awake, autostart.
- [ ] Notifications (with click back to the pane), Discord presence, updater, multi-window.
- [ ] 33 themes, English and French.
- [ ] Packages for Linux, Windows, macOS; CI green.

## 9. Risks

| Risk | Mitigation |
| --- | --- |
| iced has no accessibility tree yet | Keep full keyboard navigation; track upstream AccessKit work; note it in release notes. |
| Text input and IME edge cases on Linux (the reason for the rewrite) | `hangar-term` owns its input path with table tests; test on GNOME Wayland with US-International first. |
| HTML kits in a separate window feel different | Same API and permissions; window placed and titled by Hangar; native tabs are the long-term path, and public kits can be ported to WASM. |
| Size of the frontend (~100k lines) | Many agents on disjoint packages, logic ported with its tests, shell pre-declares extension points so merges do not conflict. |
| Data written by the native app breaking the old one | Same `state.json` shape, round-trip tests against files written by the old app. |
| GPU/driver trouble (NVIDIA + Wayland) | iced falls back to tiny-skia; `WGPU_BACKEND`/software switch exposed as a setting. |
