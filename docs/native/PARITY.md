# Hangar, native — parity audit

An audit of `feat/native-rust` against the old React app, by reading the code. Use it to decide what to fix before the parity checklist (PLAN.md §8) can be ticked. It was first written on 2026-11-03 at commit `4fe0c10`. **It was re-graded the same day at `c4949b1`**, after the fix wave: P15, P16 (kit tabs), P17's 3D view, P20 (Hangar Mobile, window side), packaging, WP1–WP8 (the first version's §7) and the final integration pass.

**Since wave 5 (2026-11-03) the old app's sources are no longer on the branch.** Every `src/…`, `src-tauri/…` and `demo/…` path below names a file of tag `v1.12.1`: read it with `git show v1.12.1:src/lib/x.ts`, list them with `git ls-tree -r --name-only v1.12.1 src/`, search them with `git grep <word> v1.12.1 -- src/`. Hangar Mobile is the exception: it moved, as TypeScript, to `mobile/`.

How it was checked:
- **Reading.** Every surface, command, setting, control op and event in [inventory-frontend.md](inventory-frontend.md) was compared, with the TypeScript in `src/` (now at tag `v1.12.1`) as the reference. In this pass every row that was not `done` was read again in the current code.
- **Tests.** `cargo test -p hangar-app` passes 2951 tests, with 0 failures and 3 ignored (it was 2408). The workspace's other crates were not re-run here; STATUS records them green.
- **Dead-code pass.** `main.rs:12` still allows dead code crate-wide. `cargo rustc -p hangar-app --profile=check -- --force-warn dead_code` now lists 122 items (it was 144). They are mostly constants and pure ports that tests use, see §5.2.
- **Marker pass.** `grep -rn "TODO(P" crates/` finds nothing (there were 27).
- **One real run.** The app ran through `scripts/native-dev.sh` on a temporary `HANGAR_DEV_HOME`, with a copy of `state/fixtures/typical.json` changed to shell panes only (it has `site` detached). The main window drew three live shells, the navigator with `site` marked as detached, the toolbar, the board dock and the broadcast bar (with its `Ctrl+Shift+B` hint), in French. `workspace_list` over the control endpoint reported `site` as `open` and `pane_list` showed its pane running, with its shell process alive. This is gap 3 of the first audit, closed. After the run `state.json` was byte for byte unchanged and no pane process was left.
- **Two reviewers for §2.7 and §2.8.** The preview, viewer, kit tab and 3D surfaces were not in the first audit's rows. Two readers graded them from the code and ran the tests of those packages (`preview::` 245 pass, `kit_tabs::` 176, `model3d_view::` 59, `hangar-model3d` 145, `hangar-kit-host` 25, `hangar-core` `kits::` 203, all green). I re-checked four of their findings in the code: a GPU failure is only logged (`hangar-model3d/src/viewer.rs:750, 793`); `KHR_mesh_quantization` is refused (`hangar-model3d/src/load/gltf.rs:33`); the helper window is never opened unfocused (`hangar-webview/src/engine.rs` has no `with_focused`); an absolute Markdown image path is read as written (`preview/model.rs:610`).

Evidence is a file and line, or a test name. A row that says *code only* has no test of its own. All paths are under `crates/hangar-app/src/` unless a crate is named.

Statuses:
- **done**: works as the TS did, or as a recorded decision (§0.3) says it should.
- **partial**: simplified, or part of it is missing.
- **missing**: no code.
- **regressed**: the native app does it wrong, or worse than the TS, in a way that matters.
- **in flight**: no row has this status any more. The column stays in the tables so they can be compared with the first audit.

---

## 0. Summary

### 0.1 Counts

Each row of the tables below counts once. The first 13 areas are the 690 rows of the first audit, graded again. The last two areas (§2.7, §2.8) are surfaces the first audit left out because they were still being written; they add 126 rows.

| Area | done | partial | missing | regressed | in flight |
|---|---|---|---|---|---|
| Shell frame (§1 of the inventory) | 22 | 2 | 0 | 0 | 0 |
| Chrome, navigator, palette, dialogs, accounts (§2.1) | 165 | 3 | 2 | 0 | 0 |
| Inspector, diff, worktrees, memory, MCP, orchestration panel, stats, board (§2.2, §2.3/1) | 91 | 4 | 1 | 0 | 0 |
| Settings, effect of each of the 53 rows (§2.3/2–3) | 51 | 1 | 1 | 0 | 0 |
| Kits management (§2.4/1.1–1.3) | 10 | 0 | 0 | 0 | 0 |
| Terminal pane and pane grid (§4) | 58 | 5 | 2 | 0 | 0 |
| Logic modules ported with their tests (§3) | 61 | 3 | 0 | 0 | 0 |
| Commands (§1.10, 88) | 88 | 0 | 0 | 0 | 0 |
| Control ops (27) | 27 | 0 | 0 | 0 | 0 |
| Engines (13) | 13 | 0 | 0 | 0 | 0 |
| Core events (16) | 16 | 0 | 0 | 0 | 0 |
| Control hooks (5) | 5 | 0 | 0 | 0 | 0 |
| `Intent` variants (59) | 59 | 0 | 0 | 0 | 0 |
| **The 690 rows of the first audit, now** | **666** | **18** | **6** | **0** | **0** |
| *The same 690 rows, first audit* | *564* | *57* | *20* | *32* | *17* |
| Preview panel and file viewers (§2.7, 51 rows, new) | 26 | 21 | 2 | 2 | 0 |
| Kit tabs and 3D viewer (§2.8, 75 rows, new) | 48 | 17 | 7 | 3 | 0 |
| **Total (816 rows)** | **740** | **56** | **15** | **5** | **0** |

All 32 regressed rows of the first audit are closed (11 causes, §0.2). The 5 regressed rows now are all in §2.7 and §2.8: two helper-window behaviours, an unchecked Markdown image path, and two silent failures of the 3D viewer.

Most of the 6 missing and about half of the 18 partial rows among the first 690 are iced 0.14 limits (no keyboard focus on buttons, no text selection). The rest are `letterSpacing` (two rows), pane opacity, the link modifier, the trust prompt's 60 lines, and three small duplicates or untested paths.

### 0.2 The first audit's top gaps, and where each stands

| # | Gap | Now | Evidence |
|---|---|---|---|
| 1 | Kits whose hooks changed since they were accepted loaded into Claude Code panes | closed (WP1) | `panes/controller.rs:169-190` reads `load_pane_kits_with`; only accepted ids reach the spawn and the MCP scope; tests `controller.rs:980, 1033, 1043, 1054` |
| 2 | A detached window acted on the main window's workspace | closed (WP4, WP5, WP6) | `toolbar/per_window.rs:70, 122, 154, 189, 206`; `status_bar/tests.rs:99, 124`; `board/mod.rs:1229, 1277`; `inspector/mod.rs:410` |
| 3 | A workspace detached at the last quit reopened empty | closed (WP2) | `app/shell.rs:964`, test `app/shell/tests.rs:54`; live run today |
| 4 | Needs-you could not open | closed (WP3) | `needs_you/mod.rs:200`; tests `needs_you/tests.rs:125, 140, 160, 175`; `commands/run.rs:169` |
| 5 | Voice dictation could not start | closed (WP3) | `voice/mod.rs:387-399`; tests `voice/tests.rs:174, 182, 192`. Not driven by real keys |
| 6 | The MCP panel restarted mid-turn panes and never said "up to date" | closed (WP5; decision §0.3) | `mcp/servers_panel.rs:86`; tests `mcp/servers_panel/tests.rs:117, 129, 142, 158, 170` |
| 7 | No shortcut shown anywhere in the chrome | closed (WP4) | `navigator/reads.rs` `binding`; tests `navigator/reads/tests.rs:45, 54, 67`, `toolbar/per_window.rs:238, 283` |
| 8 | Keyboard: Mod+, bypassed the sheet, pane-focus keys closed dialogs, shortcuts acted in fields | closed (WP2, WP8, integration pass) | `commands/run.rs:304, 394`; `app/shell.rs:38, 1087`; tests `app/shell/tests.rs:133, 156, 174` |
| 9 | Three paths could lose `state.json`; `set_check` could lose a panel edit | closed (WP2, WP7) | `hangar-core/src/store.rs` (`Rescue`, `HELD`), tests `store.rs:309, 329, 340, 362, 392`; `state/lenient.rs:57`, test `:352`; `control/tests/workspace.rs:411` |
| 10 | Integration seams: 27 `TODO(P…)`, dead providers, `AnnounceRole` unhandled | closed (WP4–WP8, integration pass) | §5 |

### 0.3 Decisions recorded in STATUS that change what parity means

| Decision | Old app | Native | Effect on the grading |
|---|---|---|---|
| The MCP panel leaves a mid-turn Claude pane alone, names it, and restarts it once its turn ends | warned, then restarted it anyway | `mcp/servers_panel.rs`, key `mcpScope.held` | Rows are `done`; behaviour differs on purpose |
| Programs in a pane can neither write nor read the clipboard (OSC 52) | ignored OSC 52 too | `panes/look.rs:55-63` (`Osc52::Disabled`; hangar-term's own default is `CopyOnly`), test `look.rs:130` | Equal to the old app. An agent that relies on OSC 52 to copy over SSH or tmux gets nothing; copy stays an explicit action |
| A pane opened to log in to an account starts with no kits | code passed every kit on, against its own comment | `panes/controller.rs`, test `:1043` | `done`, as the comment intended |
| HTML kit tabs and PDFs open in their own window (the helper is a separate process) | inline iframe | `hangar-webview`; in practice HTML pages, video and audio too | §2.7 and §2.8 rows are `partial`; the helper's window behaviour is gap 4 below |
| Native kits are WebAssembly only | none | `hangar-kit-host` | A new capability, not graded against the old app |
| The MCP install and the login item refuse whenever `HANGAR_DATA_ROOT` is set; an isolated run leaves Claude Code's trust list alone | wrote the real files | `hangar-core/src/install.rs:170`, `autostart.rs:67`, `Paths::isolated()` | Closes the dev-machine risks of §6. Release builds never set it |
| Links open only for `http` and `https`, on Ctrl or Cmd plus click (integration pass, in the log only) | xterm's web-links addon opened any matching URL on a plain click | `panes/mod.rs:471`, `hangar-term/src/links.rs:188`, `pointer.rs:140-148` | The consent hole is closed; the modifier is a difference to decide (row "Web links", §7 WP-H) |
| The old app stays on the branch until wave 5 (done); the native app ships as **Hangar 2.0.0** (owner's decision): the workspace is 2.0.0, `examples/kits/hello-native` asks for `>=2.0.0` | – | – | No row |

### 0.4 The top gaps now, most harmful first

1. **Windows and macOS have never run the app, and CI is red on all three platforms.**
   - The first complete CI run had 3 failures on macOS (keymap guard tests assume Ctrl; Mod is Cmd there), 3 on Windows (Unix paths in preview-root tests, a webview serving test) and 1 on Ubuntu (a `hangar-model3d` capture compared with a render the runner's software GPU draws differently). A fix package is running in another worktree.
   - Packaging is verified on Linux only. Installing a package, an update from a real old install and the real signing key were not tried (PACKAGING.md).
2. **Nothing has been compared with the old app on real data.** The parity pass of the inventory (§8.1) is not done. Codex, Gemini CLI and OpenCode panes: dropped (owner's decision, 2026-11-03); Claude Code is the only agent, and a saved pane that ran one of them opens as a shell with a note. Dictation was never driven by its key and a microphone. Mobile was tried with a fake phone, never with a push to a real one or a real landing. The key dispatcher is checked by unit tests only (AltGr is a guess, `keymap/chord.rs:366-373`).
3. **The 3D viewer shows still models only.** No clip plays (`model3d_view/animation_bar.rs` is drawn disabled). Draco, meshopt, KTX2, USDZ and `KHR_mesh_quantization` are refused where three.js read them. Skinned meshes stay in rest pose. A GPU failure, a lost device or iced's software renderer leaves a blank viewport with no message and no Restart.
4. **PDF, HTML pages, video, audio and HTML kit tabs live in their own OS window.** That window opens by itself when an agent shows one, is never opened unfocused (so it may take the keyboard from a terminal), is not tied to the panel, the tab or the workspace being on screen, and media keeps playing when hidden.
5. **The 2D viewers lose features.** AVIF does not open; animated GIF and WebP show their first frame; code, Markdown and the diff cannot be selected (Copy takes everything); web images in Markdown are links; a Markdown file can show any local image path.
6. **No keyboard focus on buttons and no accessibility tree** (iced 0.14). The toolbar's views group, the stats segments, F6 inside regions, the first action of Needs-you, the account rename and the preview tabs have no keyboard walk. Screen-reader roles and labels exist nowhere; the old app had aria.
7. **Terminal differences.** `letterSpacing` does nothing, an unfocused pane is not dimmed, links need Ctrl/Cmd+click where the old app took a plain click, the kitty keyboard protocol is on by default (programs that negotiate it get other key bytes than under xterm.js), dividers cannot be resized by key, and the trust prompt is looked for in 60 lines of history, not the viewport.
8. **Kit tab leftovers.** Native tabs' text inputs, selects and tables are iced's own; compiled modules are not cached on disk; the HTML host has never been tested with the real `kit.js` under its CSP (`hangar-webview/tests/engine.rs` is ignored); `preview.show` lacks the 256 MB cap; `panes.send` hides a failed enqueue; `panes.changed` can be a second late.

---

## 1. PLAN.md §8 parity checklist

| Checklist item | Status | What holds it back |
|---|---|---|
| Opens on the same data: workspaces, panes, boards, memory, settings, theme, language, shortcuts | done | Round trip checked by tests and by a real run (the file stayed byte for byte). Detached workspaces mount at boot, and every load error moves the file aside. The localStorage usage cache is not migrated (accepted, inventory §9.1). Not yet compared with the old app on a real file (§7 WP-A) |
| Panes run Claude Code and every shell; resize, zoom, search, copy/paste, links, file drop, scrollback, broadcast, queued prompts, hibernation, session resume | partial | Codex, Gemini CLI and OpenCode: dropped (owner's decision, 2026-11-03). Links need Ctrl/Cmd+click. File-drop highlight untested |
| Typing without latency; US-International dead keys | done | Release build, 16 panes looping coloured `ls`: a typed byte reaches the terminal in about 3 ms with 15 busy panes, UI thread 26% of a core, 60 fps (STATUS, integration pass). Dead keys pass widget tests, not tried on a real layout |
| Exact pane states from the bridge; context and plan gauges | done | 217 tests in `panes_logic` |
| Navigator, inspector (all panels), status bar, palette, menus, F6, every shortcut rebindable | partial | All wired and per window. No keyboard focus on iced buttons: F6 stops at regions, the toolbar's views group has no arrow walk |
| Board, needs-you, memory view | done | Board Tab-between-cards missing (iced) |
| Orchestrator mode end to end | partial | Engines, ops and UI done and each checked in a dev app; a real landing (it launches a Claude review) and a real orchestrator with workers have not run together |
| Project manager (HQ) | done | |
| Settings: every row of the old schema, Experimental included | partial | All 53 rows work except `letterSpacing` (no effect) |
| Accounts, MCP servers panel and installs, agent instructions | done | The install panel writes Claude Code's `.mcp.json` only and the playbook goes to `CLAUDE.md` only; the Codex, Gemini CLI, Cursor and VS Code targets and `AGENTS.md`: dropped (owner's decision, 2026-11-03) |
| Stats with charts; diff view; worktree panel; git clone with consent | done | Codex rollouts, the by-agent card and non-Anthropic prices: dropped (owner's decision, 2026-11-03). Small partials: no text selection in the diff, no keyboard walk of the stats segments, no sticky table header; two copies of worktree naming |
| Kits: list, add, update, setup, activation; HTML tabs; native tabs | partial | Management and the launch consent are done. Tabs: §2.8 |
| Previews: images, text and code, video, PDF, 3D | partial | §2.7, §2.8: AVIF, animated images, no inline video/PDF/HTML, 3D animation and compressed formats |
| Voice dictation (local and Groq) and its HUD | done | Not driven by its key and a microphone |
| Hangar Mobile: pairing, devices, Tailscale, push, keep-awake, autostart | done | Window side checked against a fake paired phone and curl; no real phone or push |
| Notifications with click back, Discord, updater, multi-window | done | A window last on a second screen reopens centred (iced cannot list screens) |
| 33 themes, English and French | done | |
| Packages, CI | partial | Linux packages verified; CI red on 7 tests; Windows and macOS never run (gap 1) |

---

## 2. Surfaces

### 2.1 Shell frame (inventory §1.1–1.11)

All 24 rows. Rows that were `done` in the first audit are unchanged.

| Item | Status | Evidence | What the TS does that native does not |
|---|---|---|---|
| `hangar-ai --mcp` as a stdio server with no window | done | `main.rs:57-62` | – |
| A debug build refuses the installed app's data | done | `main.rs:64`, `dev_guard` (`main.rs:80`) | – |
| Core start: PATH, bridge mod, kit and account watchers, board API and endpoint, Mobile | done | `app.rs:212 boot`, `hangar-core/src/lib.rs` | – |
| Boot screen until the state is in | done | `shell/placeholder.rs:18`, `shell/frame.rs:59` | – |
| Load and hydrate: unique names, dangling folder ids, legacy trees, defaults | done | `app.rs:344 booted`, `state/store.rs:316-336`, `state/lenient.rs` | – |
| Save: 400 ms debounce, never before hydration, an older write never lands after a newer one, flush on quit | done | `app.rs:378-421`, `:447 quit`, `state/persist.rs` | – |
| Unknown fields, explicit nulls, malformed values written back | done | `state/lenient.rs`, `state/unread.rs`, `state/roundtrip_tests.rs`; real run | – |
| Load failure of any kind | **done** (was partial) | `hangar-core/src/store.rs` (`Rescue`, `LoadError`, `HELD`, `set_aside`); tests `store.rs:309` (not UTF-8), `:329` (not an object), `:340` (a BOM is no error), `:362` (cannot be moved: never written over), `:392`; `state/lenient.rs:57 read_checked`, test `:352`; `app.rs:196` | The TS started empty and overwrote the file |
| Agents and shells detected once | done | `app.rs:256`; only Claude Code is looked for now, the other agents dropped (owner's decision, 2026-11-03) | – |
| A notification's click raises the window and lands on the pane | done | `app.rs:422 core_event` | – |
| Detached windows reopened at boot | **done** (was regressed) | `app/shell.rs:964 open_workspace_window`, `state/mod.rs:226-239` (a window opened mounts its workspace); test `app/shell/tests.rs:54`; real run: `site` open, its pane running | – |
| Detach: flag set, main window moves to the next workspace, window opened | done | `app/shell.rs:884 detach` | – |
| Detach failure: revert, then the `window.openFailed` notice | **done** (was partial) | `app/shell.rs:995 open_timed_out`, `:1120 undo_open`, `shell/windows.rs:58` (10 s); tests `app/shell/tests.rs:87` (with notice), `:114` (at boot, quiet) | – |
| A workspace window's close reattaches; the main window's close quits | done | `app/shell.rs:211-245`, `:910 reattach`; `app.rs:447 quit` | – |
| Bounds remembered after a pause; reopened where they were | partial | `app/shell.rs:1008-1040`, `shell/windows.rs:123` | iced cannot list screens, so a window from a second screen reopens centred |
| Deleting a workspace closes its window | done | `app/shell.rs:366` (orphaned windows closed) | – |
| Placeholders: "no workspace" and "shown in its own window" | done | `shell/placeholder.rs:28, 61` | – |
| F6 regions: order, empty ones skipped, Shift+F6 | partial | `shell/regions.rs:78-120`, `app/shell.rs:794-869` | No focus inside regions: iced buttons take no focus |
| Side panels docked or as drawers | done | `shell/side_panels.rs`, `shell/drawer.rs` | – |
| Escape closes one overlay | done | `app.rs:594 key`, `app/shell.rs:459 close_overlay` | – |
| Notices (6 s, three kinds); chord hint | done | `shell/notice.rs:52-76`, `shell/frame.rs:283` | – |
| A theme per window; `uiScale` as the scale factor | done | `app.rs:522-546` | – |
| Shutdown: PTYs killed, Mobile stopped, deliveries cancelled, endpoint cleared, Discord stopped | done | `app.rs:447 quit`; SIGTERM, SIGINT and SIGHUP quit like closing the window, a second one exits at once (`app.rs:621-680`, Unix; Ctrl+C elsewhere); `pty::kill_all` kills each pane's process group (`hangar-core/src/pty.rs:773`, test `:1291`) | – |
| Each window keeps its own view, overlays, drawer and focus | done | `app/shell.rs:275-307, 1050-1085` | – |

### 2.2 Chrome, navigator, palette, dialogs, accounts (§2.1)

Of 170 rows, 136 were `done` in the first audit and still are: the navigator tree, rows, folders, rename, keyboard and drag; the toolbar's lead, presets and confirmations; the broadcast send, the update banner and the HUD drawing; the palette; the saved commands; the workspace dialog in both modes and the git flow with its 14 steps; the consent card; accounts. The 34 rows below were not `done`.

| Surface | Item | Was → now | Evidence | What the TS does that native does not |
|---|---|---|---|---|
| Navigator | Asleep moon | missing → done | `navigator/view.rs:619`, `navigator/reads.rs:169` (`panes_logic.asleep()`); test `navigator/reads/tests.rs:39` covers the absent case only | – |
| Navigator | Bound keys in tooltips | missing → done | `navigator/reads.rs` `binding`; `navigator/view.rs:244, 905, 913`; tests `navigator/reads/tests.rs:45, 54, 67` | – |
| Navigator | Filter bar: a press on the field focuses the input | partial → partial | `navigator/view.rs:816 filter_bar` | iced limit |
| Navigator | Menu › Sleep: disabled with a reason, probe on open | partial → done | `navigator/menu.rs:264-268`, `navigator/sleep.rs`; tests `navigator/sleep/tests.rs:35, 51` | – |
| Navigator | Menu › Claude account section | regressed → done | `navigator/menu.rs:84, 218-221`; tests `navigator/tests.rs:504`, `pane_menu/tests.rs:63, 87, 101` | – |
| Navigator | Plan meters | partial → done | `navigator/reads.rs` `plan` → `panes_logic.plan_limits()`; `HANGAR_DEV_PLAN` only while no pane has reported | – |
| Toolbar | Arrow keys on the views group | missing → missing | `toolbar/mod.rs:14-16` | iced buttons take no focus |
| Toolbar | Capsule: tool asked, last answer, context chip | partial → done | `toolbar/activity.rs:44-70, 114-116, 230` read `panes_logic.insight`; tests `toolbar/activity/tests.rs` | – |
| Toolbar | Capsule: the pane's board task | missing → done | `toolbar/activity.rs:121, 173` (`board::Live::task_for_pane`) | – |
| Toolbar | Kits button and menu | in flight → done | `navigator/reads.rs` `kit_tabs`; tests `navigator/reads/tests.rs:150, 157, 166, 191, 205`; opening is P16 (§2.8) | – |
| Toolbar | Preview / 3D toggle | missing → done | `navigator/reads.rs` `models` (reads `Panels::of`); tests `navigator/reads/tests.rs:232, 271, 285` | – |
| Toolbar | Shortcut hints | missing → done | `toolbar/mod.rs:331`; tests `toolbar/per_window.rs:238, 283` | – |
| Toolbar | Which workspace it acts on | regressed → done | `toolbar/per_window.rs`; tests `:70, 99, 122, 154, 189, 206, 220` | – |
| Status bar | Branch polling | partial → partial | `status_bar/mod.rs:215-250` | Beats for windows that have the focus, where the TS read visibility; test `status_bar/tests.rs:161` |
| Status bar | Dictation and broadcast key hints | missing → done | `status_bar/mod.rs:365-380, 465` | – |
| Status bar | Which workspace it acts on; one broadcast text per window | regressed → done | `status_bar/tests.rs:99, 112, 124` | – |
| Needs-you | The panel opens; `view.needsYou` | regressed → done | `needs_you/mod.rs:200` writes `live.signals.needs_you`; tests `needs_you/tests.rs:125, 140, 160, 175`; `HANGAR_DEV_NEEDS_YOU_OPEN` removed | – |
| Needs-you | Landing busy state | partial → done | `needs_you/mod.rs:51, 103, 208` (`control::Live::task_delivering`); tests `needs_you/tests.rs:50, 61` | – |
| Needs-you | Inline land error on the row | missing → done | `control/mod.rs:265-300` sends `Msg::LandFailed`; test `control/tests/delivery.rs:1014` | – |
| Needs-you | First action autofocused | missing → missing | no code | iced has no focus |
| Needs-you | Boards | partial → done | `needs_you/mod.rs` reads `board::Live`; test `needs_you/tests.rs:217` | – |
| Voice | `voice.dictate` starts a recording | regressed → done | `commands/run.rs:318`, `voice/mod.rs:387-399`; tests `voice/tests.rs:174, 182, 192` | Not driven by real keys |
| Voice | `voice_recording` check at start | missing → done | `voice/mod.rs:292-304` (a microphone left open is released at start), code only | – |
| Palette | Active workspace when it is detached | partial → done | `palette/mod.rs:107` (`window_workspace`) | – |
| Palette | Kit-tab entries | in flight → done | `palette/sources.rs:466-490` | – |
| Saved commands | A run from the menu touches the workspace | missing → done | `panes_logic/mod.rs:1537` (`RunSavedCommand` touches) | – |
| Session picker | Full-id tooltip; untitled rows in italics | partial → done | `session_picker/mod.rs:305-411` | – |
| Session picker | Keyboard row scrolled into view | missing → done | `session_picker/mod.rs:72` (`scroll_to`) | – |
| Pane menu | Account items | regressed → done | `pane_menu/mod.rs:338-352`; tests `pane_menu/tests.rs:63, 87, 101` | – |
| Workspace dialog | Enter submits from any field | partial → done | `workspace_dialog/view.rs:210, 273, 309, 386`; `hooks.rs:93` | – |
| Workspace dialog | Initial focus | missing → done | the dialog's layer puts the keyboard in the first field; `workspace_dialog/mod.rs:319` for a source switch | – |
| Workspace dialog | Clone progress | partial → done | `workspace_dialog/mod.rs:125-142, 741`; test `workspace_dialog/tests.rs:820` (indeterminate bar) | – |
| Accounts | Switcher's ask dialog; inline rename (Escape, autofocus) | partial → partial | `accounts/switcher.rs:263`, `accounts/settings_page.rs:543-565` | The rename field has no autofocus and no Escape |
| Accounts | Switcher's workspace | partial → done | `accounts/switcher.rs:57-62` (`ctx.workspace()`) | – |

### 2.3 Inspector, diff, worktrees, memory, MCP, orchestration panel, stats (§2.2) and board (§2.3/1)

Of 96 rows, 77 were `done` and still are: memory, the MCP install panel, the worktree panel with every confirmed step, the diff view (except selection), the board's columns, cards, filters, drag and task dialog, the charts. The 16 rows below were not.

| Surface | Item | Was → now | Evidence | Gap |
|---|---|---|---|---|
| Inspector | Width grip | partial → done | `inspector/mod.rs:159-235` (`panel_drag`, saved on release); tests `inspector/tests.rs:130, 152, 213, 238, 253` | – |
| Inspector | `shown`, which gates the git polling and the clock | partial → done | `inspector/mod.rs:162 shown_in` → `Panels::of`; test `inspector/tests.rs:990` | – |
| Inspector | Session › MCP row and "Change" | regressed → done | `inspector/sections.rs:236`; `panes_logic/mod.rs:458` (writer); test `inspector/tests.rs:853` | – |
| Inspector | Changed files in a detached window | partial → done | `inspector/mod.rs:410` (`window_workspace`); `inspector/tests.rs:59 with_detached` | – |
| Diff view | Text selection; tabs | partial → partial | `git_ui/diff_view.rs:56` (`TAB_STOP = 8`) | No text selection (iced) |
| MCP servers | "Up to date" and "Restart N agents" | regressed → done | `mcp/servers_panel.rs:86 running_mcp`, `mcp/scope.rs:162 needs_restart`; tests `servers_panel/tests.rs:117, 129` | – |
| MCP servers | Busy warning before a restart | regressed → done (decision §0.3) | `mcp/servers_panel.rs:111-140`; tests `servers_panel/tests.rs:142, 158, 170` | Mid-turn panes are held, not warned about |
| Orchestration | Worker badge's task title | partial → done | `orchestration_ui/badge.rs:130` reads `board::Live` | – |
| Stats | Keyboard walk of the segmented controls; stale dimming; sticky header | partial → partial | `stats/view.rs:90-97` (stale veil, done) | Keyboard walk and sticky header: iced limits |
| Board | Which workspace's board | regressed → done | `board/mod.rs:1229, 1277`; `board/interaction.rs:224` | – |
| Board | 30 s clock | partial → done | `board/mod.rs:280` (`Panels::of(...).dock.shown`) | – |
| Board | Skeleton fade-in; focus after the dialog | partial → partial | `board/view.rs:567`, `board/task_dialog.rs` | – |
| Board | Tab between cards | missing → missing | – | Needs a shell `focus_next` |
| Board | Dock grip | partial → done | `board/dock.rs`; tests `board/interaction.rs:188, 260`; commit `0720f5c` | – |
| Libraries | `worktrees.ts` | partial → partial | `git_ui/worktrees/naming.rs:45`, `orchestration/worker.rs:299` | Two copies of the naming |
| Libraries | `orchestration.ts` | partial → done | `orchestration_ui/actions.rs`, `form.rs`, `panel.rs` use `state::roles` | – |

### 2.4 Settings (§2.3/2–3)

All 53 rows exist, with the TS key, default, range and category (`settings/schema.rs`; defaults at `state/types.rs:432-487`). 51 are wired.

| Key | Was → now | Evidence | Gap |
|---|---|---|---|
| letterSpacing | missing → missing | `panes/look.rs:81` ("no counterpart in hangar-term yet"); `hangar-term/src/lib.rs:204` | hangar-term has no letter spacing |
| termPreview | partial → partial | `settings/previews.rs:306` | Ignores letterSpacing |
| voiceEnabled | partial → done | `commands/run.rs:121`; `voice/tests.rs:182, 192` | – |
| voiceEngine, voiceModel, voiceCloudModel, voiceLanguage, voiceSubmit, voiceCleanup, voiceCleanupModel, voiceCleanupHint | partial → done | `voice/mod.rs` `voice_config`; `voice/tests.rs:182` | Never run with a microphone here |

The Shortcuts editor is done. Reset keeps `voiceApiKey`, `kitsEverywhere`, `projectsFolder` and unknown fields (`state/store.rs:735-748`).

### 2.5 Kits management (§2.4/1.1–1.3)

All 10 rows are done (`kits/update.rs`, `kits/views/*`, `kits/setup.rs`, `kits/gallery.rs`). The launch-time hook consent is closed (§0.2 gap 1).

### 2.6 Terminal pane and pane grid (§4)

Of 65 rows, 53 were `done` and still are. The 12 rows below were not.

| Item | Was → now | Evidence | What the TS does that native does not |
|---|---|---|---|
| letterSpacing | missing → missing | `panes/look.rs:81` | – |
| Unfocused pane dimmed to 0.94 | partial → partial | no opacity code in `panes/grid.rs` or hangar-term | – |
| Web links | partial → partial | `panes/mod.rs:471` (`is_web_link`), `hangar-term/src/links.rs:188`, tests `links.rs:366, 383`, `pointer.rs:615` | Only `http` and `https` open now (the consent risk is closed). Opens on Ctrl/Cmd+click; the old app's addon opened on a plain click, and inventory §1.3 asked for a click |
| Kits held for changed hooks | regressed → done | `panes/controller.rs:169-190`; tests `:980, 1033, 1043, 1054` | – |
| Trust prompt detection | partial → partial | `panes_logic/mod.rs:1284` (`TRUST_LOOK_LINES = 60`) | Reads 60 lines including history; the TS read the viewport only |
| Role panes: permission mode and allowed tools | partial → done | `panes/controller.rs:210-214` (`state::roles::orchestration_of`); tests `:1094, 1111, 1121` | – |
| Non-bridged agents: `session` event and `sessionSince` | missing → done | `panes/controller.rs:699` sends `Logic::Launched`; `panes_logic/mod.rs:1140, 1219` | – |
| File-drop highlight | partial → partial | `panes/grid.rs:798` | Untested with a real drag |
| Detached workspace mounted at boot | regressed → done | §2.1 | – |
| Divider keyboard and aria labels | missing → missing | STATUS P5 | iced has no focusable separator |
| Header name tooltip `pane.jump {keys}` | partial → done | `panes/header.rs:144-154`, test `:714` | – |
| Zoom bar: Escape after Tab | partial → partial | `panes/zoom_bar.rs` | iced buttons take no focus |

Logic modules (§3): 61 of 64 are ported with at least as many tests. The five that were in flight are done: `kitTabs` (37 TS cases) and `kitApi` (104) are in `kit_tabs/` (176 tests); `remote*` and `activityLog` are in `mobile/` (149 tests, STATUS counts the same); `hangarUi` stays a script-generated sheet served by `hangar-core/src/kit_scheme.rs`; its vitest went with `src/` in wave 5, and `hangar-theme`'s `kit` tests took over its colour checks (the sheet's two themes, and every pair of the components at 4.5:1 and 3:1 on all 33 themes), not its checks of `kit.js`, `kit-ui.js`, the gallery and `kit-ui.md`. Three are partial: `windows` (most of it dropped by design), `typography` (applied when the dictionaries are generated, no runtime module), `kitActivation` (52 Rust cases for 82 TS ones). `webgl` is dropped (n/a).

### 2.7 Preview panel and file viewers (§2.4/1.5–1.6) — new rows

Graded by reading. 51 rows: 26 done, 21 partial, 2 missing, 2 regressed. P15's own gaps in STATUS are included. `cargo test -p hangar-app preview::` passes 245, with 2 ignored (the real-helper ones, which need an engine and a display).

Done (26): tab click and close; tab icons (`preview/tabs.rs:143-200`); the grip's drag, release and double-click reset (`grip.rs`, tests `the_grip_draws_the_panel_where_the_pointer_is_and_saves_on_release`, `escape_drops_the_grip…`); drawer focus take and return (`shell/drawer.rs`); expand, collapse, hide and the three modes (`panel.rs:36-110`); empty state; kind detection and size caps (`hangar-core/src/preview.rs`); a viewer rebuilt per `id:revision`; text head of 256 KiB with BOM, CRLF and partial line; 5000 lines with notes and gutter; Copy; pretty-JSON and language chips; `languageOf` (10 names, 67 extensions); Markdown safe render, links, heading ids, source toggle and inert-link notice; image fit, zoom range, pan, double-click, keys, bar; loop and speed of the player; media error with Show in folder; PDF Open in default app; `inlineOnly` sibling refusal; the 300 ms settle and the missing-file note; notices of 2.2 s and 6 s.

| Feature | Status | Evidence | What the old app does that native does not |
|---|---|---|---|
| Tab keys (←, →, Home, End, Delete) | partial | `preview/mod.rs:1320, 1388` (only while focus is `Region::Model`); test `the_keys_of_the_strip_move_between_tabs` sends messages, not keys | Reached by F6 or a drawer, not by clicking a tab; with an image in front F6 lands on the image stage, so the tabs are out of reach |
| Tooltips and the "by" label | partial | `preview/tabs.rs:84-140, 228-262`, no test | Rules match the TS |
| Active tab scrolled into view | partial | `preview/mod.rs:506 strip_to`; test `the_strip_scrolls_to_the_tab_in_front` | Snaps by index; tabs have varied widths, so one can end half hidden |
| Grip ±24 keys, role, tooltip | partial | `preview/grip.rs:19, 101, 146` | Arrows work only after a press on the grip; no tooltip or separator role |
| Escape collapses an expanded panel | partial | `preview/mod.rs:1347, 1391`, no test | Window-wide, not only inside the panel |
| Hide with focus inside → pane | partial | `app/shell.rs:771 after_docked_close`, no test | – |
| Wrap toggle | partial | `preview/mod.rs:913` | The scroll snaps to the top |
| Highlighting languages | partial | `preview/highlight.rs` (`syntect` via `two-face`); test `knows_a_grammar_for_every_language_a_file_is_named_after` | No PowerShell grammar; 9 colour classes |
| Markdown `#anchor` scroll | partial | `preview/markdown.rs`, `mod.rs:1001-1005`; tests parse only | Never run |
| Markdown images: relative, absolute, `data:` | partial | `preview/markdown.rs:100`; tests `loads_a_relative_image_from_beside_the_file…`, `an_inline_image_is_read_from_its_bytes` | A local `.svg` goes to iced's raster widget, which cannot decode it; GIF is a first frame; AVIF fails |
| Markdown http(s) and `//` images | partial | `preview/markdown.rs` `Page::image`; test `a_web_image_is_a_link_not_a_download` | Shown as an alt-text link, never fetched (badges, screenshots) |
| Markdown image path safety | **regressed** | `preview/model.rs:599-626`, `preview/markdown.rs:100` (no root, no size cap); a test accepts `../art/logo.svg` | The old scheme refused anything outside the allowed folders and capped images at 64 MB. A hostile README can draw any local image path (`/dev/zero`, a UNC share). It only displays the image; nothing leaves the machine |
| Image wheel zoom, Ctrl and pinch | partial | `preview/image_stage.rs:86`; test `the_wheel_zooms_by_its_distance_and_a_pinch_in_finer_steps` | Trackpad pinch never arrives (iced 0.14) |
| Checkerboard backdrop, pixelated from zoom 3 | partial | `preview/image_stage.rs:161` | No test |
| SVG without a size, failure message | partial | `preview/image_stage.rs:211 measure` | A broken SVG becomes a blank 512² stage, never "Image failed" |
| Animated images | partial | `preview/image.rs` header; STATUS P15 | GIF and animated WebP show their first frame |
| AVIF | missing | `hangar-core/src/preview.rs` lists `.avif`; no decoder in `Cargo.lock` | The tab opens to the "Image failed" card |
| Video and audio player | partial | `preview/windowed.rs`, `webview.rs:379` | Opens in a separate window (§0.3); the panel shows a card with Focus, Reopen, Show in folder |
| Pause when hidden | missing | no code in `preview/` or `hangar-webview/src` | Playback goes on after a tab switch, a hidden panel or another workspace |
| Helper window: lifecycle and focus | **regressed** | `preview/mod.rs:725-750` (opens once when in front and shown), `preview/webview.rs reconcile` (closes only when the entry is gone), `hangar-webview/src/engine.rs:199, 299` (focus is only ever set, never withheld) | An agent that shows a PDF, page or video opens a window that may take the keyboard from a terminal, and it is not tied to the panel, the tab or the workspace being on screen. Not observed live |
| PDF inline or message | partial | `preview/webview.rs:407, 620`; tests `a_pdf_the_engine_draws_stays_open`, `a_pdf_the_engine_cannot_draw_is_closed_and_said_so` | The window opens first and closes after the engine says it has no viewer (a flash on WebKitGTK); no real engine run |
| HTML page | partial | `preview/webview.rs:379`, core CSP | Its own window, not an iframe; a link click opens the browser |
| HTML reload, source toggle, "isolated" chip | partial | `preview/windowed.rs:97` | Reload only while the window is open; in source mode it does not re-read the source |
| File watch every 1.5 s while shown | partial | `preview/mod.rs:109, 1227, 1374`; no test drives `Tick` | No check that the window is hidden or minimised |
| Info strip | partial | `preview/viewer.rs info`, `parts.rs byte_size`, no assertion | No dropping of items below 460 and 360 px |

Not in the inventory, also missing: keyboard scrolling of code and Markdown (iced's scrollable has none), text selection, accessibility roles (tablist, separator, application), a travelling loading bar (it is static), Markdown front matter (iced's parser hides a `---` block where markdown-it printed it).

### 2.8 Kit tabs and the 3D viewer (§2.4/1.4, 1.7, 1.8) — new rows

Graded by reading. 75 rows: 48 done, 17 partial, 7 missing, 3 regressed. Tests pass: `kit_tabs::` 176, `model3d_view::` 59, `hangar-model3d` 134 + 11 capture, `hangar-kit-host` 25, `hangar-webview` kit 7, `hangar-core` `kits::` 203 and `kit_scheme` 17. The helper's end-to-end test (`hangar-webview/tests/engine.rs`) is ignored.

Done (48): native WebAssembly tab (`hangar-kit-host/src/tab.rs`, tests `a_real_module_starts_calls_and_draws`, `the_hello_native_example_runs`); the CSP and scheme (`hangar-core/src/kit_scheme.rs`, test `the_policy_isolates_the_page_and_allows_its_own_files_only`); id and method validation; the 16-pending `rate_limited`; the 18 methods except `preview.show` (`kit_tabs/api.rs:185-419`; each has permission, limit and error-code tests, e.g. `refuses_a_port_not_granted_even_with_another_loopback_granted`); events `tab.params` and `theme`; permissions re-read per call; stale GitHub-kit command grants; the five tab states as logic; reload; the isolated chip's words; Rust-side limits; 3D recenter with its 420 ms glide; the four toggles and the 600k normals limit; capture save and the capture at 2× up to 4096; banners; the loading card; the info strip; mouse and keyboard controls; renderer; camera and framing; grid and axes; wireframe; the LRU of 3 models; camera kept on reload and re-framed on resize; render on demand; glb/gltf, OBJ+MTL, STL, PLY; clay and material standardisation; texture wait and readable errors; snapshots (angles, size, theme background, the user's camera untouched; `a_snapshot_of_a_model_in_memory_is_taken_from_every_angle_off_screen`); one load per model, a newer revision aborting the old; the cross-window protocols (replaced by direct calls through `PreviewAccess`, `services/preview.rs:67`) and the grant of model roots for a kit's `preview.show` (`preview/access.rs:253`).

| Feature | Status | Evidence | What the old app does that native does not |
|---|---|---|---|
| HTML tab hosting | partial | `kit_tabs/bridge.rs:93, 123`, `kit_tabs/mod.rs:558-590`, `hangar-webview/src/kit_bridge.js`; the end-to-end test is ignored and uses a stand-in SDK | A separate 1000×720 window opened only while the tab is in front of a shown panel. The real `kit.js` under the sandbox CSP and the relay have never run together under a test |
| Fallback "Shown as a page" | partial | `kit_tabs/mod.rs:484-557`, `view.rs:140-153`; test `modules_that_break_the_abi_do_not_load` | The fallback branch itself is untested |
| Host check: source and origin | partial | `kit_tabs/api.rs:518-524`, `kit_relay.rs is_kit_document` | There is no iframe: trust is the helper window id and the page URL prefix |
| `preview.show` | partial | `kit_tabs/deps.rs:319` → `preview/access.rs:266`; test `opens_the_file_rust_resolved_under_the_kits_name` (fakes) | No 256 MB cap and no "too large for the preview" message; the real `show_file` is untested |
| Event `panes.changed` | partial | `kit_tabs/mod.rs:642-692`; only the gate is tested | Driven by a 1 s tick, where the TS followed activity live |
| States drawn as StageMessages | partial | `kit_tabs/view.rs:166-199`, no render test | – |
| Lazy creation of restored tabs | partial | `kit_tabs/tabs.rs:196`; test `keeps_a_restored_tab_unloaded_while_it_is_hidden_and_loads_it_once_shown` | An HTML page also waits until it is in front of a shown panel, even when an agent opened it (the TS loaded those at once to follow a CI run) |
| "Left" detection | partial | `kit_tabs/bridge.rs:135-146`, `view.rs:269`, no test | A navigation outside the kit is blocked rather than shown as "left" |
| Focus guard (`frameTookKeyboard`) | **regressed** | no `with_focused(false)` anywhere; `hangar-webview/src/engine.rs:299` | A window for an agent-opened tab can take the keyboard from a terminal when the panel is on screen |
| Capture: copy image | partial | `model3d_view/capture.rs`, `services/clipboard.rs` (arboard); no test | Not live-checked |
| Canvas focusable (`role=application`) | partial | `hangar-model3d/src/viewer.rs:592` | Focus on click only; no F6, no role |
| Reload overlay (slim bar and chip) | partial | `model3d_view/stage.rs:121`, no assertion | – |
| StageMessage "unavailable" | **regressed** | `hangar-model3d/src/viewer.rs:750, 793` (`eprintln!` only) | The old app said WebGL was unavailable; the viewport is now blank without a word |
| StageMessage "context lost" + Restart | missing | `ModelLost` and `ModelRestart` are never used | No device-loss handling |
| Software GPU (tiny-skia) fallback | **regressed** | `hangar-model3d/src/lib.rs:50` | On iced's tiny-skia the viewport is blank with no message; captures and snapshots still work through the offscreen adapter (`offscreen.rs:58`) |
| Animation play and pause | missing | `model3d_view/animation_bar.rs`: the button has no `on_press` | – |
| Clip select | partial | `animation_bar.rs`: names listed, items disabled with a reason | Cannot switch clip |
| Scrub | missing | static `progress_bar(.., 0.0)` | – |
| Time label | partial | `format::seconds(0.0)` | Always "0.0 / duration" |
| Animation (mixer, loop, autoplay, kept on reload) | missing | `hangar-model3d/src/load/gltf.rs:591` keeps a clip's name and length, not its tracks | No playback; nothing kept on reload |
| Lighting | partial | `hangar-model3d/src/render/shaders/mesh.wgsl:134-177` (analytic sky and ground, 0.8 and 1.6) | No PMREM `RoomEnvironment`; not compared with the old look |
| Reduced-motion recenter | partial | `hangar-model3d` `set_reduced_motion` is never called (`model3d_view/mod.rs:40`) | The camera always glides |
| `compileAsync` and the upload budget | missing | `hangar-model3d/src/render/frame.rs:230` uploads at the first frame | A big model may stall one frame |
| Draco, meshopt, KTX2 | missing | `hangar-model3d/src/load/gltf.rs:30-40` refuses with an export hint; KTX2 drops the texture with a warning | three.js read all three. `KHR_mesh_quantization` is now refused too, and `hangar-model3d/Cargo.toml` enables only 4 glTF extensions (clearcoat, transmission, ior, specular, volume and sheen are not read) |
| FBX | partial | `hangar-model3d/src/load/fbx.rs`, 5 tests | Binary 7.x only; no animation or skins |
| USDZ | missing | `hangar-model3d/src/load/mod.rs` returns Unsupported | – |
| Skinned meshes | partial | `hangar-model3d/src/load/gltf.rs:322 skin_vertices` (rest pose) | No animation, no morph targets |

Not in the inventory, also missing: `KitTabView.showFile`'s 256 MB refusal (see `preview.show`), `Deps::enqueue` swallows the outcome of `bridge_enqueue` (`kit_tabs/deps.rs:345`; the TS awaited it), limits count code points where the TS counted UTF-16 units (more lenient for emoji), the iframe `title` hint naming the kit, and the snapshot's three retries when a file is rewritten while loading (native waits up to 40 s for the latest revision, `model3d_view/snapshots.rs`).

---

## 3. Commands (§1.10)

All 88 default bindings match `shortcuts.ts` on Windows/Linux and on macOS (`keymap/bindings.rs:23-156`). The dispatcher follows every rule of §1.10 (`keymap/dispatch.rs`). Its caveats: AltGr is guessed as "Ctrl+Alt with printable text" (`keymap/chord.rs:366-373`), and it has been checked by unit tests only, never with real key presses.

All 88 commands are `done` now. The 18 that were `regressed`:

| # | Commands | Was → now | Evidence |
|---|---|---|---|
| 13–27 | `pane.next`, `pane.prev`, `pane.focusLeft/Right/Up/Down`, `pane.focus1`…`9` | regressed → done | `commands/run.rs:394` sends `Intent::SelectPane`, which leaves overlays open; `app/shell.rs:1087 select_pane`; tests `app/shell/tests.rs:133, 156` |
| 49 | `view.settings` | regressed → done | `commands/run.rs:304` → `Intent::OpenSettings(None)` / `CloseOverlay`; test `app/shell/tests.rs:174` |
| 66 | `view.needsYou` | regressed → done | `commands/run.rs:169`; test `needs_you/tests.rs:140` |
| 88 | `voice.dictate` | regressed → done | `commands/run.rs:318`; test `voice/tests.rs:174` |

The pane commands now need a tree (`commands/run.rs:176`). The other 70 are unchanged: pane split, close, restart, tidy, pane zoom and swap; workspace; views; terminal; saved commands; interface zoom; fullscreen (`app/shell.rs`, `commands/run.rs`, packages' `on_intent`).

---

## 4. Control ops, engines, events, hooks, intents

General points:
- Core keeps the per-op timeouts (`hangar-core/src/control.rs:89-101`).
- `agentPaneControl` is honoured for agents only (`control/run.rs:32`).
- Requests that arrive before the state loads get the TS text word for word.
- Refusal texts match `control.ts`, `managerControl.ts` and `deliveryControl.ts`.
- Engines never touch `Model`: their writes go through `Msg::Model`, answered after the snapshot is republished.

| Op(s) | Was → now | Evidence |
|---|---|---|
| `workspace_list`, `pane_list`, `pane_create`, `pane_close`, `pane_restart`, `pane_read`, `pane_focus`, `workspace_window`, `pane_send`, `workspace_create` | done | `control/panes.rs`, `control/workspace.rs`. `workspace_list` and `pane_list` checked over curl in today's run |
| `workspace_set_check` | done | `control/workspace.rs:218` writes `check_command` only; test `control/tests/workspace.rs:411` |
| `preview_show`, `preview_close` | done | `preview/mod.rs:751-760` installs `Hook::Preview` |
| `model_show`, `model_snapshot`, `model_close` | in flight → done | `control/run.rs:86-88`, `control/preview.rs:443`; four angles, a rewritten file and a corrupt file checked over the control endpoint (STATUS P17) |
| `kit_tab_list`, `kit_tab_open`, `kit_tab_close` | in flight → done | `control/kit_tabs.rs`; `Hook::KitTabs` at `kit_tabs/mod.rs:404`; checked with curl (STATUS P16) |
| `worker_spawn`, `worker_stop` | done | `orchestration/worker.rs:716, 1305`. The respawn race at `worker.rs:1327, 1371` is unchanged, very low |
| `worker_list` | partial → done | `orchestration/live.rs:431-437 is_open` uses `control::port::is_open`; test `live.rs:455` |
| `delivery_submit`, `delivery_status` | done | `control/delivery.rs` |
| `project_list`, `project_status`, `project_send` | done | `control/manager.rs` |

| Engine | Was → now | Evidence |
|---|---|---|
| orchestratorEvents, managerEvents, briefing, check/reviewer/queue/land, worker spawn and undo, restart notes, `touch`, delivery cancel, stop all workers | done | unchanged |
| 5 s ticker (activity log) | in flight → done | `orchestration/runtime.rs:364`, `mobile/activity_log.rs` |
| announceRole | partial → done | `orchestration/mod.rs:184-195` handles `Intent::AnnounceRole`; the inference from `PinOrchestrator` is gone; a refused pin tells no session |
| deliveryRuns read-model | partial → done | `needs_you/mod.rs:51, 103, 208` read `control::Live::task_delivering` |

| Event | Was → now | Consumers |
|---|---|---|
| `remote:changed` | partial → done | `settings/mod.rs:544`, `mobile/mod.rs:148` |
| `remote:shared` | in flight → done | `mobile/mod.rs:149`, `mobile/push.rs` |
| the other 14 | done | unchanged; `services/events.rs:137-166` has no wildcard |

| Hook | Now |
|---|---|
| `Hook::Orchestration`, `Hook::FromGit`, `Hook::Preview` | done |
| `Hook::KitTabs` | done (`kit_tabs/mod.rs:404`) |
| `Hook::Mobile` | done (`mobile/mod.rs:93`) |

Intents: all 59 have a handler. `Dictate` (`voice/mod.rs:399`), `AnnounceRole` (`orchestration/mod.rs:191`), `Land` (`control/mod.rs:265-300` also sends `LandFailed`) and `OpenKitTab` (`kit_tabs/mod.rs:871`) are done. `PaneRespawned` needs no handler and its doc now says so (`message.rs:236-246`).

Snapshot: `opened`, `asleep`, `activity` and `delivering` are filled from the real read-models (`state/snapshot.rs:33-43`).

---

## 5. Integration seams

### 5.1 `TODO(P…)` markers

None are left in `crates/` (the first audit counted 27; 25 named merged packages). All were closed: the board and orchestration, control, inspector, MCP, navigator, needs-you, toolbar and workspace-dialog markers by calling the real provider (`panes_logic::find_agent_pane`, `Panels::of`, `state::layout`, `state::roles`, `state::folders`, `state::pane_names`, `accounts::model::launch_account_with`, `board::Live`, the keymap's `first_binding`, `kit_tabs::choices`, `panes_logic::asleep()` and `plan_limits()`); the clipboard and preview services by `services::clipboard` (commit `2c772a2`) and the preview access (`787e068`); `shell/ui.rs` by keeping its two types, which hangar-ui has no counterpart for (`c53f5cc`). `board/agent_pane.rs` and `navigator/clock.rs` are deleted.

### 5.2 Local copies and dead code

Most copies of the first audit are gone: every provider listed there is now called, except for the few below. `cargo rustc ... --force-warn dead_code` lists 122 items.

| Left | Where |
|---|---|
| Worktree naming twice (the inventory wanted it in hangar-core) | `git_ui/worktrees/naming.rs:45`, `orchestration/worker.rs:299` |
| A raw read of `ws.orchestration` | `git_flow/deps.rs:228-234` (`state::roles::orchestration_of` would do) |
| `hangar_core::model::allowed_roots` is `pub(crate)`; the 3D view recomputes the roots | `model3d_view`, `hangar-core/src/model.rs` |
| The `AccessHost` fallback, kept on purpose until the preview hook is installed | `control/live.rs:422` |
| Dead: old synchronous hibernation (`decide`, `sweep`, `force_sleep`), `DropTracker`, `osc9_notification`/`osc777_notification` (hangar-term's parsers run), `sleep_verdict`, `focus_entry` | `panes_logic/hibernation.rs:480-622`, `panes_logic/file_drop.rs:213`, `panes_logic/pane_agent.rs:131, 147`, `panes_logic/mod.rs:580, 1517` |
| Dead: 18 constants and functions that hangar-core owns | `kits/model.rs:217-487` |
| Dead: the shell's `stub*` helpers, `state::lenient::{app_state, read}`, and about 60 more small items | `shell/placeholder.rs:95-126`, `state/lenient.rs:39, 46` |

### 5.3 Intents, hooks, `Live` read-models

All 59 intents are handled (§4). All 5 hooks are installed. `live.signals.needs_you` is written (`needs_you/mod.rs:200`). `PaneInsight.mcp` is written (`panes_logic/mod.rs:458`, from `panes/controller.rs:546`).

### 5.4 Debug-only seeds

Every seed is behind `cfg!(debug_assertions)`, so no release build reads one. Left: `HANGAR_DEV_SCREENSHOT`, `_SCREENSHOT_AFTER_MS`, `_PREVIEW`, `_MODEL3D_CAPTURE`, `_USE_REAL_DATA`, `_SETTINGS`, `_KIT_TABS`, `_VOICE` (HUD only; the real press works), `_UPDATE_BANNER`, `_PLAN` (only while no pane has reported), `_NAV_RENAME`, `_NAV_MENU`, `_NAV_DRAG`, `_FOCUSED_PANE`. `HANGAR_DEV_NEEDS_YOU_OPEN` and `HANGAR_DEV_ACTIVITY`, which hid gaps, are gone.

---

## 6. Risks to data and to the person

### Data

| Risk | Was → now | Where |
|---|---|---|
| A read error, a non-object root or a lenient-read failure started the app empty and the next change overwrote `state.json` | medium → closed | `hangar-core/src/store.rs` (set aside byte for byte; if it cannot be moved every save is refused with a notice), `state/lenient.rs:57`, `app.rs:196`; tests `store.rs:309, 329, 362, 392` |
| `workspace_set_check` could overwrite a panel edit | low → closed | `control/workspace.rs:218`; test `control/tests/workspace.rs:411` |
| Grips wrote the settings on every pointer move | cost → closed | inspector, board dock and preview grips draw from `panel_drag` and save once on release; Escape cancels |
| Boards written from a detached window went to another workspace | medium → closed | `board/mod.rs:1229, 1277` |
| A dev build wrote the person's real files | medium → closed | MCP install and login item refuse whenever `HANGAR_DATA_ROOT` is set (`hangar-core/src/install.rs:170`, `autostart.rs:67`); a clone in an isolated run leaves `~/.claude.json` alone (`Paths::isolated()`) |
| A Markdown file can show any local image path | new, low | `preview/model.rs:599-626`: no root check, no size cap (gap 5, §7 WP-F) |

### Processes

| Risk | Was → now | Where |
|---|---|---|
| Restart all, Reset agents and presets from a detached window hit another workspace; broadcast typed into it | high → closed | `toolbar/per_window.rs`, `status_bar/tests.rs:124` |
| "Restart N agents" respawned mid-turn panes | medium → closed (decision §0.3) | `mcp/servers_panel.rs` |
| Quit killed only the shell; no signal handling | low → closed | `hangar-core/src/pty.rs:773`, `app.rs:621`; SIGTERM leaves no pane process (STATUS) |
| The engine's mutex held across a board read; `pty_kill` on async threads | low → closed | `orchestration/live.rs:40` (`spawn_blocking`); STATUS WP7 |
| `stop_worker` can miss a worker respawned in that instant, then force-remove its worktree | very low → open | `orchestration/worker.rs:1327, 1371` |
| A helper window can take the keyboard from a terminal | new, medium (not observed) | `hangar-webview/src/engine.rs`; gap 4 |
| A test crash (SIGSEGV) about 1 run in 6 when several `iced_test` simulators draw at once | new, low | Mobile page fixed by taking turns; WP8 saw it twice at 32 threads. If it returns, serialize the simulators crate-wide |

### Commands without consent

| Risk | Was → now | Where |
|---|---|---|
| A GitHub kit's changed hooks ran without being accepted again | **high** → closed | `panes/controller.rs:169-190`; tests `:980, 1033` |
| Ctrl+click opened any URI (`file:`, `ssh:`, OSC 8 targets) | medium → closed | `panes/mod.rs:471`, `hangar-term/src/links.rs:188` (`http` and `https` only) |
| Programs may write the clipboard (OSC 52) | low → decided | Disabled (`panes/look.rs:55-63`), as in the old app |
| Kitty keyboard protocol on by default | behaviour change → open | `hangar-term/src/config.rs:81`; `panes/look.rs` does not turn it off |

Checked and safe: setup commands run only on a click of Run; saved commands only on a click or a shortcut; `tailscale serve` only from its button; the updater installs only after Restart; removing a kit takes two clicks; a wasm kit tab has no WASI and only `hangar.log` and `hangar.now` as imports, and the same permission codes and messages as `kitApi.ts`.

---

## 7. Remaining work: packages for parallel agents

The packages own disjoint files and are listed most valuable first. Where two need one file, the package says who owns which part or who goes first. WP-A is not code and runs first, because most of the numbers above come from reading; WP-J runs last, because it deletes in files the others own. Each package reads this file, then `crates/hangar-app/PACKAGES.md` and the rows it names, and ends with `cargo test -p hangar-app` (plus the crates it touches), `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`, and where the app is involved a run through `scripts/native-dev.sh` with `HANGAR_DEV_HOME` set to a temporary folder (a debug build refuses to start without it; put a copy of `state/fixtures/typical.json` at `$HANGAR_DEV_HOME/data/state.json`). A package that changes a row's status updates this file. Agents' worktrees are cut from `main`: check `git merge-base --is-ancestor 44cfd25 HEAD` and reset onto `feat/native-rust` first.

### WP-A — Run it for real (owns no source files; findings become rows here)

1. Copy a real `state.json` into a dev home and run the native app beside the old one: navigator, toolbar, panes, boards, memory, settings, themes, shortcuts, detached windows. Compare what each shows.
2. ~~Open Codex, Gemini CLI and OpenCode panes: launch, resume, state glyph, trust prompt, Shift+Enter.~~ Dropped (owner's decision, 2026-11-03): instead, open a `state.json` holding a Codex pane and check it comes up as a shell in its folder, with the note in its bar.
3. Real keys: Mod+Alt+V with voice on (and a microphone), AltGr and AZERTY digits, US-International dead keys, Mod+, then a category click, F6 and Shift+F6.
4. An orchestrator with two workers in a scratch repo, to a real landing (it launches a Claude review).
5. The 3D capture's clipboard copy and save dialog; a Markdown file with a relative `.svg`; a file dropped on a pane.
6. Hangar Mobile with a real paired phone: a push and the `remote_*` calls.

Verify: every defect found is a new row in §2.

### WP-B — Windows, macOS and CI (`.github/workflows/**`, `scripts/*.ps1`, `scripts/preview.nsi`, `crates/hangar-app/packaging/**`, `docs/native/PACKAGING.md`, `crates/hangar-webview/build.rs`)

1. Get CI green: the 7 failing tests of the first run (macOS keymap guard tests, Windows path and webview-serving tests, the Ubuntu `hangar-model3d` capture). Another agent is on it: read `git log` before starting.
2. Add a Linux packaging smoke job: install the `.deb` in a container, run `hangar-ai --version`, check `hangar-webview` sits beside it.
3. ~~Point `scripts/preview*.ps1|nsi` at the native layout~~ (wave 5 deleted the Tauri-only preview and pointed `scripts/installer-art.ps1` at `packaging/`); give `hangar-webview` its Windows icon resource.
4. On real machines: install, start, detach a workspace, open a PDF and an HTML kit tab (WebView2, WKWebView), update from an old install, sign with the real key.

Verify: a CI run with every job green; the checklist of PACKAGING.md ticked on Windows and macOS.

### WP-C — Helper windows (`crates/hangar-webview/src/{engine,protocol,client}.rs`, `crates/hangar-app/src/preview/{webview,windowed}.rs`, `crates/hangar-app/src/kit_tabs/{bridge,view}.rs`)

0. First, move the helper-window code out of `preview/mod.rs` (the reconcile and opening at `:725-750`, the media probes at `:1104`) into `preview/windowed.rs`, so that WP-F can own the rest of `preview/mod.rs`. `kit_tabs/mod.rs:558-590` (where an HTML tab's window opens) is WP-C's; the rest of that file is WP-I's.
1. Add a `focus` flag to the `open` command and open unfocused (tao `with_focused(false)`) whenever an agent or hook opened the window.
2. Tie each window to being seen: when the panel hides, another tab is in front, or the workspace leaves the screen or sleeps, hide or close the window and bring it back on show (`preview/mod.rs:725-750`).
3. Pause audio and video when hidden (`eval` of `pause()`). Read a media error from the page's event, not two polls (`preview/mod.rs:1104`).
4. Ask the engine for `pdfViewerEnabled` before showing a PDF window (no flash).
5. HTML reload in source mode re-reads the source.
6. Run the two ignored real-helper tests in CI under a virtual display, plus one test that loads a page with the real `kit.js` under the CSP through the relay.

Verify: a test that a hidden panel closes its helper windows; a dev run where an agent shows a PDF while a terminal has the focus (over `preview_show`) and the terminal keeps it.

### WP-D — The 3D viewer's behaviour (`crates/hangar-model3d/src/{viewer,camera,render,load/gltf}*`, `crates/hangar-app/src/model3d_view/**`)

1. Keep a clip's tracks (`load/gltf.rs:591` keeps only the name and length), sample them in the viewer, apply skins and morph targets, autoplay the first clip, keep clip and time on reload; give `animation_bar.rs` its messages (play, clip select, scrub, time).
2. Show a StageMessage for "GPU unavailable" (`viewer.rs:750, 793` only log), for a lost device with Restart (`ModelLost`, `ModelRestart` are unused), and for iced's software renderer.
3. Call `set_reduced_motion` (`model3d_view/mod.rs:40`); spread texture uploads over frames (`hangar-model3d/src/render/frame.rs:230`).
4. Decide whether to match the old look (PMREM environment) and compare side by side.

Verify: `cargo test -p hangar-model3d -p hangar-app model3d_view::`; an animated glb plays in a dev run; `model_snapshot` of it over the control endpoint.

### WP-E — 3D formats (`crates/hangar-model3d/src/load/{draco,meshopt,ktx2,usd,fbx}.rs` new or edited, and the table at `load/gltf.rs:30-40`; start after WP-D merges its change to `gltf.rs`)

1. Draco, meshopt, KTX2/basis and `KHR_mesh_quantization` in glTF; the extensions clearcoat, transmission, ior, specular, volume and sheen.
2. USDZ (an `openusd` feature, off by default if it is heavy); ASCII FBX.

Verify: a fixture of each, drawn by the capture tests; the refusal sentences (`gltf.rs:30`) go away only when the format reads.

### WP-F — 2D viewers (`crates/hangar-app/src/preview/{image,image_stage,markdown,markdown/**,text,code_lines,highlight,viewer,model,tabs,grip,panel,parts,mod}.rs` — `mod.rs` after WP-C's step 0 —, `crates/hangar-core/src/model.rs` for `allowed_roots`, `crates/hangar-ui/src/selectable.rs` new)

1. Open AVIF (the extension is already accepted by `hangar-core/src/preview.rs`); play animated GIF and WebP.
2. Markdown: read local images only under the allowed roots and under a size cap (make `hangar_core::model::allowed_roots` `pub`); decode a local `.svg`; decide on `http(s)` images (fetch with a cap, or keep the link) and write the decision in STATUS; stop hiding front matter.
3. A selectable text widget in `hangar-ui` (new file) for code, Markdown and the diff (`git_ui/diff_view.rs`), so Copy can copy a selection.
4. Tab strip: arrow keys after a click, scroll to the tab by its real position (`mod.rs:506`), Escape only inside the panel. Smaller: a PowerShell grammar, the wrap toggle keeps the scroll, a travelling loading bar, a "failed" card for a broken SVG, the info strip's container steps, `preview.show`-style size caps in `open_file`.

Verify: the tests of `preview::`; a README with a `../` and an absolute image path draws neither.

### WP-G — Keyboard focus and accessibility (`crates/hangar-ui/**` except WP-F's `selectable.rs`, `shell/regions.rs`, `toolbar/mod.rs`, `stats/view.rs`, `needs_you/view.rs`, `board/view.rs`, `accounts/settings_page.rs`, `navigator/view.rs`, `panes/zoom_bar.rs`)

1. Decide first, in STATUS: a focusable button and segmented control in hangar-ui built on iced's `Focusable` operation, or waiting for an accessibility tree in iced. iced 0.14 has neither.
2. Then give a ring, Enter and Space to buttons; the arrow walk to the toolbar's views group and the stats segments; F6 focus inside regions; the first action of Needs-you; autofocus and Escape for the account rename; a press on the navigator filter; Tab between board cards (a shell `focus_next`); the zoom bar's Escape.
3. Screen-reader names and roles where the toolkit allows.

Verify: `iced_test` walks of each control.

### WP-H — Terminal (`crates/hangar-term/**`, `crates/hangar-app/src/panes/{look,grid,mod}.rs`, `crates/hangar-app/src/panes_logic/mod.rs`, `services/terminals.rs`, `settings/previews.rs`)

1. `letterSpacing` in hangar-term and in the settings preview (`settings/previews.rs`).
2. The unfocused pane at 0.94 opacity (pane opacity in hangar-term).
3. Decide, with a real agent, plain click versus Ctrl/Cmd+click for links, and the kitty keyboard default (`hangar-term/src/config.rs:81`): compare the bytes Claude Code receives with xterm.js's (Codex: dropped (owner's decision, 2026-11-03)).
4. Keyboard resize of dividers (a key handler under F6 on the grid).
5. The trust prompt looked for in the viewport only: `TerminalAccess::screen_text` and `panes_logic/mod.rs:1284`.
6. Check the file-drop highlight with a real drag; selection foreground colour; fractional scaling.

Verify: hangar-term tests; the widget tests of `panes::`.

### WP-I — Kit tab leftovers (`crates/hangar-app/src/kit_tabs/{api,deps,mod,native,tabs,catalog}.rs`, `crates/hangar-kit-host/**`, `crates/hangar-kit-sdk/**`, `crates/hangar-core/src/kits/**`)

1. `preview.show`: the 256 MB cap and its message (`deps.rs:319`); `panes.send` reports a failed `bridge_enqueue` (`deps.rs:345`); `panes.changed` driven by activity events, not the 1 s tick (`mod.rs:642`).
2. Cache compiled wasm modules on disk; draw a native tab's text inputs, selects, tables and trees with hangar-ui widgets (the kit-host renderer).
3. Tests for the `unloadable` fallback, the StageMessages, "left" detection, `reconcile`.
4. ~~Decide the release number for kits with `schema: 2`.~~ Decided by the owner: the native Hangar is 2.0.0, and the workspace carries it.

Verify: `cargo test -p hangar-app kit_tabs:: -p hangar-kit-host`; the `hello-native` example kit still opens.

### WP-J — Cleanup, last and alone (`crates/hangar-app/src/{main.rs,kits/model.rs,kits/setup.rs,panes_logic/{hibernation,file_drop,pane_agent,mod}.rs,git_ui/worktrees/naming.rs,orchestration/worker.rs,git_flow/deps.rs,status_bar/mod.rs}` and whichever file holds a dead item; `crates/hangar-core/src/worktrees.rs` new)

1. Delete the 122 dead items or put them under `#[cfg(test)]`, then remove `#![allow(dead_code, unused_variables)]` from `main.rs:12` so the compiler guards them.
2. One worktree naming, in hangar-core (`git_ui/worktrees/naming.rs:45`, `orchestration/worker.rs:299`).
3. `stop_worker` race (`orchestration/worker.rs:1305-1375`); `git_flow/deps.rs:228-234` through `state::roles`.
4. The status bar's git beat on window visibility, not focus (`status_bar/mod.rs:244`).
5. If the SIGSEGV flake returns, serialize `iced_test` simulators crate-wide.

Verify: the whole workspace builds with no `allow(dead_code)`.

### Decisions the owner should make

- ~~Native release number.~~ 2.0.0 (owner's decision): it ships once the owner has used it for a day or two, and `feat/native-rust` then replaces `main`.
- Plain click or Ctrl/Cmd+click for terminal links; kitty keyboard protocol on or off by default.
- Whether PDF, HTML, video and audio should stay in their own window or be drawn in the panel (a native PDF renderer, a bundled pdf.js); the helper-window behaviour of WP-C holds either way.
- Whether web images in Markdown should be fetched.
- Whether an unfinished 3D format set (no Draco, no USDZ) blocks parity, or is documented as a change.
- Whether iced stays on 0.14 without keyboard focus and accessibility, or the project moves to something that has them.
