# Hangar, native — parity audit

An audit of `feat/native-rust` against the old React app, by reading the code. Use it to decide what to fix before the parity checklist (PLAN.md §8) can be ticked. It was written on 2026-11-03 at commit `4fe0c10`.

How it was checked:
- **Reading.** Every surface, command, setting, control op and event in [inventory-frontend.md](inventory-frontend.md) was compared, with the TypeScript in `src/` as the reference.
- **Tests.** `cargo test -p hangar-app` passes 2408 tests, with 0 failures and 3 ignored.
- **Dead-code pass.** `main.rs` allows dead code crate-wide, which hides providers nobody calls. `cargo rustc -p hangar-app --profile=check -- --force-warn dead_code` lists 144 items; the seams it shows are in §5.2.
- **One real run.** The app ran through `scripts/native-dev.sh` on a temporary `HANGAR_DEV_HOME`, with a copy of `state/fixtures/typical.json` changed to shell panes only, and `HANGAR_DEV_SCREENSHOT`. The main window drew three live shells, the navigator, the toolbar, the board dock and the broadcast bar, in French. After quitting, no process was left and `state.json` was unchanged.

**P15 (preview) merged while this audit ran** (`22ef8a2`). Its items are classified from the merged code but were not run. Still out of scope, marked *in flight*: P16 (kit tabs), P17's 3D view in the app, P20 (Hangar Mobile, window side), packaging.

All paths are under `crates/hangar-app/src/` unless a crate is named.

Statuses:
- **done**: works as the TS did.
- **partial**: simplified, or part of it is missing.
- **missing**: no code.
- **regressed**: the native app does it wrong, or worse than the TS, in a way that matters.

---

## 0. Summary

### Counts

Each row of the tables below counts once.

| Area | done | partial | missing | regressed | in flight |
|---|---|---|---|---|---|
| Shell frame (§1 of the inventory) | 19 | 4 | 0 | 1 | 0 |
| Chrome, navigator, palette, dialogs, accounts (§2.1) | 136 | 12 | 15 | 6 | 1 |
| Inspector, diff, worktrees, memory, MCP, orchestration panel, stats, board (§2.2, §2.3/1) | 77 | 14 | 1 | 4 | 0 |
| Settings, effect of each of the 53 rows (§2.3/2–3) | 42 | 10 | 1 | 0 | 0 |
| Kits management (§2.4/1.1–1.3) | 10 | 0 | 0 | 0 | 0 |
| Terminal pane and pane grid (§4) | 53 | 7 | 3 | 2 | 0 |
| Logic modules ported with their tests (§3) | 56 | 3 | 0 | 0 | 5 |
| Commands (§1.10, 88) | 70 | 0 | 0 | 18 | 0 |
| Control ops (27) | 20 | 1 | 0 | 0 | 6 |
| Engines (13) | 10 | 2 | 0 | 0 | 1 |
| Core events (16) | 14 | 1 | 0 | 0 | 1 |
| Control hooks (5) | 3 | 0 | 0 | 0 | 2 |
| `Intent` variants (59) | 54 | 3 | 0 | 1 | 1 |
| **Total (690 rows)** | **564** | **57** | **20** | **32** | **17** |

The 32 regressed rows come from 11 causes, listed below. Fifteen of the rows are the pane-focus commands, which share one cause (gap 8).

### The top gaps, most harmful first

1. **Kits whose hooks changed since they were accepted still load into Claude Code panes.**
   - The pane launch keeps its own copy of `effectiveKits` (`panes/controller.rs:146-159`).
   - P14's `kits::model::{pane_kits, load_pane_kits, kit_issue_notes}` exist but are dead code.
   - So a GitHub kit's new hooks run without the person's consent, and the `warnHooks` note is never shown.
2. **A detached workspace window acts on the main window's active workspace.**
   - The toolbar's Restart all, Reset agents and presets use `m.active_workspace()` (`toolbar/mod.rs:116,147,260,272`).
   - So does the status bar's broadcast, which types text plus Enter (`status_bar/mod.rs:159,198,263`).
   - So do the board and dock, including their writes (`board/mod.rs:241,674,788`), and the inspector's git polling (`inspector/mod.rs:310`).
   - In the TS each window's App had its own workspace.
3. **A workspace detached when the app last quit reopens with an empty grid.**
   - Only the active workspace joins `ui.opened_ids` (`app/shell.rs:325-330`, `state/store.rs:324-328`).
   - P5 spawns PTYs only for `opened_ids` (`panes/mod.rs:869-876`).
   - So its panes never start until the workspace is picked in the navigator.
4. **Needs-you cannot open.** Nothing writes `live.signals.needs_you` (`state/live.rs:99`; only tests do), and the shell closes the panel after every message while it is 0 (`app/shell.rs:374-376`). For the same reason `view.needsYou` is never live (`commands/run.rs:162`).
5. **Voice dictation cannot start.**
   - `voice.dictate` sends `Intent::Dictate` (`commands/run.rs:295`), and no package handles it (`voice/mod.rs:335-342` returns `None`).
   - Nothing sends `voice::Msg::Press`.
   - The key is taken and does nothing. The seed `HANGAR_DEV_VOICE` hides this in screenshots.
6. **The MCP servers panel restarts every Claude Code pane**, mid-turn ones included, and never says "up to date". `running_mcp` returns `None` and `is_working` returns `false` (`mcp/servers_panel.rs:88-97`). For the same reason the inspector's MCP row never shows (`inspector/sections.rs:236`): `PaneInsight.mcp` has no writer. The launch's MCP is in `panes::Live::report(pane).mcp` (`panes/mod.rs:121-125`).
7. **No shortcut is shown anywhere in the chrome.** The one function every tooltip and hint asks is a stub that returns `""` (`navigator/reads.rs:118-121`), although `KeymapIndex::label/title` exist (`keymap/index.rs:182,189`).
8. **Keyboard regressions.**
   - Mod+, opens the settings through `Action::ToggleSettings` (`commands/run.rs:283`, `app/shell.rs:174-188`), bypassing the settings package's sync. The first category click goes back to General, the cards are not loaded, and closing the sheet with the key leaves its subscriptions running.
   - Ctrl+Tab, Alt+arrows and Mod+1…9 go through `focus_pane`, which clears every overlay of the main window (`app/shell.rs:511-520`). A dialog closes and loses what was typed.
   - Only one text field asks the keymap first (`keymap::guard` at `saved_commands/dialog.rs:289,295`). Elsewhere a shortcut acts in the field and also runs: Mod+Shift+Enter in the broadcast bar sends the line and splits a pane.
9. **Three paths can lose `state.json`, and one write can lose a panel edit.**
   - A load error other than a JSON parse error (I/O, invalid UTF-8) is not quarantined (`hangar-core/src/store.rs:83`). The app then starts empty (`app.rs:332-335`), and the next change saves the empty state over the file.
   - A document whose root is not an object reads as the empty state (`state/lenient.rs:38-55`), and the same overwrite follows.
   - So does a document that fails even the lenient read.
   - `workspace_set_check` writes an `orchestration` block built from a snapshot, which can overwrite a panel edit made in between (`control/workspace.rs:205-216`).
10. **Integration seams left open.**
    - There are 27 `TODO(P…)` markers. 25 name a package that has merged, and a provider now exists for 22 of them (§5.1).
    - The shell's and P4's real providers are dead code while local copies run: `state::roles::orchestration_of`, `state::layout::closed_by_preset`, `state::pane_names::pick_pane_names` and `panes_logic::find_agent_pane` are never called (§5.2).
    - `Intent::AnnounceRole` has no handler (§5.3).

The other regressions:
- The doubled separator and "Claude account" heading in the navigator and pane menus (`navigator/menu.rs:209-214`, `pane_menu/mod.rs:333`).
- In the detached window, the board-target and inspector-polling rows (counted under gap 2).

---

## 1. PLAN.md §8 parity checklist

| Checklist item | Status | What holds it back |
|---|---|---|
| Opens on the same data: workspaces, panes, boards, memory, settings, theme, language, shortcuts | partial | Round trip done and checked (a run left the file unchanged). Detached workspaces reopen empty (gap 3); the load-error overwrite (gap 9); the localStorage usage cache is not migrated (accepted in the inventory, §9.1). |
| Panes run every agent and shell; resize, zoom, search, copy/paste, links, file drop, scrollback, broadcast, queued prompts, hibernation, session resume | partial | Code covers all of them; only Claude Code and shells have run in the real app. Links need Ctrl+click and open any scheme (§6). The file-drop highlight is untested. Non-bridged agents never record a `session` event or `sessionSince` (`panes/controller.rs:671-676`). |
| Typing without latency; US-International dead keys | partial | 3–4.5 ms echo measured; dead keys pass widget tests; the 16-busy-pane benchmark has not been run. Every terminal action re-hashes and re-publishes every pane (`panes/mod.rs:195-207, 789-853`). |
| Exact pane states from the bridge; context and plan gauges | done | 217 tests in `panes_logic`. |
| Navigator, inspector (all panels), status bar, palette, menus, F6, every shortcut rebindable | partial | Gaps 2, 6, 7 and 8. Rebinding itself is done. |
| Board, needs-you, memory view | regressed | Needs-you cannot open (gap 4); the board writes to the wrong workspace from a detached window. Memory is done. |
| Orchestrator mode end to end | partial | Engines and ops done. Land gives no row feedback; P19 ignores detached workspaces (`orchestration/live.rs:113,403`); `AnnounceRole` has no handler; `set_check` race (§4). |
| Project manager (HQ) | done | `project_*` ops, manager feed, open manager. |
| Settings: every row of the old schema, Experimental included | partial | All 53 rows, with the same keys and defaults. `letterSpacing` has no effect; the voice rows reach nothing (gap 5); Mod+, bypasses the sheet's sync (gap 8). |
| Accounts, MCP servers panel and installs, agent instructions | partial | Accounts and installs done; MCP servers restart regressed (gap 6). |
| Stats with charts; diff view; worktree panel; git clone with consent | done | Small partials: no stale dimming, no text selection in the diff, tabs drawn as 4 spaces. |
| Kits: list, add, update, setup, activation; HTML tabs; native tabs | partial | Management is done. The launch ignores held hooks (gap 1). Tabs are P16, in flight. |
| Previews: images, text and code, video, PDF, 3D | partial | P15 merged (not run here); the toolbar's preview toggle reads a stub (`navigator/reads.rs:158-161`); 3D is P17, in flight. |
| Voice dictation (local and Groq) and its HUD | regressed | It cannot start (gap 5). |
| Hangar Mobile: pairing, devices, Tailscale, push, keep-awake, autostart | in flight | The settings side is done (`settings/mobile*.rs`); the window side (`remote_*` ops, push, `Hook::Mobile`) is P20. |
| Notifications with click back, Discord, updater, multi-window | partial | Notifications, Discord and the updater are done; multi-window has gaps 2 and 3. |
| 33 themes, English and French | done | |
| Packages, CI | in flight | Out of scope. |

---

## 2. Surfaces

### 2.1 Shell frame (inventory §1.1–1.11)

| Item | Status | Evidence | What the TS does that native does not |
|---|---|---|---|
| `hangar-ai --mcp` as a stdio server with no window | done | `main.rs:59-63` | – |
| A debug build refuses the installed app's data | done | `main.rs:64-67`, `dev_guard` | – |
| Core start: PATH, bridge mod, kit and account watchers, board API and endpoint, Mobile | done | `app.rs:204`, `hangar-core/src/lib.rs` | – |
| Boot screen until the state is in | done | `shell/placeholder.rs:18`, `shell/frame.rs:59` | – |
| Load and hydrate: unique names, dangling folder ids, legacy trees, defaults | done | `app.rs:328-360`, `state/store.rs:322-336`, `state/lenient.rs` | – |
| Save: 400 ms debounce, never before hydration, an older write never lands after a newer one, flush on quit | done | `app.rs:364-406, 433-447`, `state/persist.rs` | – |
| Unknown fields, explicit nulls, malformed values written back | done | `state/lenient.rs`, `state/unread.rs`, `state/roundtrip_tests.rs`; checked by a real run | – |
| Load failure other than a parse error | partial | `hangar-core/src/store.rs:83`, `state/lenient.rs:50-55` | Same hole as the TS: starts empty and later overwrites (gap 9) |
| Agents and shells detected once | done | `app.rs:244` | – |
| A notification's click raises the window and lands on the pane | done | `app.rs:412-423` | – |
| Detached windows reopened at boot | regressed | `app.rs:341-352`, `app/shell.rs:325-330`, `panes/mod.rs:869-876` | The window opens, but its panes never start (gap 3) |
| Detach: flag set, main window moves to the next workspace, window opened | done | `app/shell.rs:895-916` | – |
| Detach failure: revert, then the `window.openFailed` notice | partial | no code (grepped `openFailed`) | – |
| A workspace window's close reattaches; the main window's close quits | done | `app/shell.rs:217-223, 920-941` | – |
| Bounds remembered after a pause; reopened where they were | partial | `app/shell.rs:993-1025` | iced cannot list screens, so a window from a second screen reopens centred (STATUS) |
| Deleting a workspace closes its window | done | `app/shell.rs:381-394` | – |
| Placeholders: "no workspace" and "shown in its own window" (Show, Bring back) | done | `shell/placeholder.rs:28, 61` | – |
| F6 regions: order, empty ones skipped, Shift+F6 | partial | `shell/regions.rs:78-120` | No focus inside regions: iced buttons take no focus |
| Side panels docked or as drawers; drawers close on Escape, a press outside, or their toggle | done | `shell/side_panels.rs`, `shell/drawer.rs`, `app.rs:555-572` | – |
| Escape closes one overlay, only when nothing else took it | done | `app.rs:545-553, 579-594` | – |
| Notices (6 s, three kinds); chord hint | done | `shell/notice.rs:52-76`, `shell/frame.rs:283` | – |
| A theme per window; `uiScale` as the scale factor | done | `app.rs:508-532` | – |
| Shutdown: PTYs killed, Mobile marked stopped, deliveries cancelled, endpoint cleared, Discord stopped | done | `app.rs:433-447`, `hangar-core/src/lib.rs:184-197` | No signal handling (§6) |
| Each window keeps its own view, overlays, drawer and focus | done | `app/shell.rs:270-307` | – |

### 2.2 Chrome, navigator, palette, dialogs, accounts (§2.1)

Only rows that are not done are listed. Every other item of §2.1 is done, 136 rows in all, with evidence on file in the audit notes:
- the navigator tree, rows, folders, rename, keyboard and drag;
- the toolbar's lead, presets and confirmations;
- the broadcast send, the update banner and the HUD drawing;
- the palette;
- the saved commands;
- the session picker;
- the workspace dialog in both modes, and the git flow with its 14 steps and consent queue;
- the consent card;
- accounts.

| Surface | Item | Status | Evidence | What the TS does that native does not |
|---|---|---|---|---|
| Navigator | Asleep moon | missing | `navigator/reads.rs:168-171` returns `None` | The moon on a sleeping workspace |
| Navigator | Bound keys in tooltips | missing | `navigator/reads.rs:118-121` | Every shortcut hint |
| Navigator | Filter bar: a press on the field focuses the input | partial | `navigator/view.rs:820-932` | – |
| Navigator | Menu › Sleep: disabled with a reason, probe on open | partial | `navigator/menu.rs:256-265` (TODO) | P4 refuses on click with a notice instead |
| Navigator | Menu › Claude account section | regressed | `navigator/menu.rs:209-214`, plus `accounts/menu_items.rs:91-94` | Separator and heading shown twice |
| Navigator | Plan meters | partial | `navigator/mod.rs:105-110, 903-917` | Keeps its own copy of the plan instead of P4's |
| Toolbar | Arrow keys on the views group | missing | `toolbar/mod.rs:14-16` | – |
| Toolbar | Capsule: tool asked, last answer | partial | `toolbar/activity.rs:29-82` (its own `Glance`) | – |
| Toolbar | Capsule: the pane's board task | missing | `toolbar/activity.rs:223-228` | `taskForPane` |
| Toolbar | Capsule: context chip | partial | `toolbar/activity.rs:282-299` | – |
| Toolbar | Kits button and menu | missing (listing), in flight (opening, P16) | `navigator/reads.rs:141-144` | – |
| Toolbar | Preview / 3D toggle | missing | `navigator/reads.rs:158-161`, `toolbar/mod.rs:367` | P15 has merged; `preview::Live::{has_preview, panel_open, unseen}` (`preview/mod.rs:257-276`) |
| Toolbar | Shortcut hints | missing | `navigator/reads.rs:118` | – |
| Toolbar | Which workspace it acts on | regressed | `toolbar/mod.rs:116,147,260,272` | Gap 2 |
| Status bar | Branch polling | partial | `status_bar/mod.rs:192-218` | Gated on focus rather than visibility |
| Status bar | Dictation and broadcast key hints | missing | `status_bar/mod.rs:319-340` | – |
| Status bar | Which workspace it acts on; one broadcast text for all windows | regressed | `status_bar/mod.rs:159,198,263,270` | Gap 2 |
| Needs-you | The panel opens; `view.needsYou` | regressed | `app/shell.rs:374-376`, `commands/run.rs:162` | Gap 4 |
| Needs-you | Landing busy state | partial | `needs_you/mod.rs:44-47` (a 45 s timer) | `useDelivering` |
| Needs-you | Inline land error on the row | missing | `Msg::LandFailed` is never sent | – |
| Needs-you | First action autofocused | missing | iced has no focus | – |
| Needs-you | Boards | partial | `needs_you/mod.rs:156-173` (its own `board_load`) | – |
| Voice | `voice.dictate` starts a recording | regressed | `voice/mod.rs:335-342` | Gap 5 |
| Voice | `voice_recording` check at start | missing | only in `hangar-core/src/voice/mod.rs:198` | – |
| Palette | Active workspace when it is detached | partial | `palette/sources.rs:268` | The TS read a detached active workspace as none |
| Palette | Kit-tab entries | in flight (P16) | `palette/sources.rs:492-494` | – |
| Saved commands | A run from the menu touches the workspace | missing | `saved_commands/mod.rs:98-114` | Hibernation does not see runs from the menu |
| Session picker | Full-id tooltip; untitled rows in italics | partial | `session_picker/mod.rs:326-412` | – |
| Session picker | Keyboard row scrolled into view | missing | no `scroll_to` | – |
| Pane menu | Account items | regressed | `pane_menu/mod.rs:333` | Two separators in a row |
| Workspace dialog | Enter submits from any field | partial | `workspace_dialog/view.rs:204, 266` | – |
| Workspace dialog | Initial focus | missing | no code | – |
| Workspace dialog | Clone progress | partial | `workspace_dialog/view.rs:333-349` | An unknown percentage shows 0 instead of an indeterminate bar |
| Accounts | Switcher's ask dialog; inline rename (Escape, autofocus) | partial | `accounts/switcher.rs:261-306`, `accounts/settings_page.rs:543-565` | – |
| Accounts | Switcher's workspace | partial | `accounts/switcher.rs:57-62` | Reads the active workspace |

### 2.3 Inspector, diff, worktrees, memory, MCP, orchestration panel, stats (§2.2) and board (§2.3/1)

Only rows that are not done are listed; the other 77 are done. All done:
- memory;
- the MCP install panel;
- the worktree panel (including every destructive step, each confirmed inline);
- the diff view, except text selection;
- the board's columns, cards, filters, drag and task dialog;
- the charts.

| Surface | Item | Status | Evidence | Gap |
|---|---|---|---|---|
| Inspector | Width grip | partial | `inspector/mod.rs:139-169, 248-265` | Writes the setting on every pointer move; clamped to 240–440 rather than the panel's real maximum |
| Inspector | `shown`, which gates the git polling and the clock | partial | `inspector/mod.rs:134-136` | Wrong in drawer mode: polling and the clock keep running |
| Inspector | Session › MCP row and its "Change" link | regressed | `inspector/sections.rs:236-237` | Gap 6 |
| Inspector | Changed files in a detached window | partial | `inspector/mod.rs:310` | Uses the main window's active workspace |
| Diff view | Text selection; tabs | partial | `git_ui/diff_view.rs:47-57` | Tabs drawn as 4 spaces |
| MCP servers | "Up to date" and "Restart N agents" | regressed | `mcp/servers_panel.rs:88-90` | Gap 6 |
| MCP servers | Busy warning before a restart | regressed | `mcp/servers_panel.rs:95-97` | Gap 6 |
| Orchestration | Worker badge's task title | partial | `orchestration_ui/mod.rs:84-133, 320` | Refreshed only when the workspace's own board changes, so a title edited from a worktree goes stale |
| Stats | Keyboard walk of the segmented controls; stale dimming; sticky header | partial | `stats/view.rs:281-453, 542` | iced limits |
| Board | Which workspace's board | regressed | `board/mod.rs:240-242, 673-690, 787-798` | Gap 2: a detached window writes to another workspace's `board.json` |
| Board | 30 s clock | partial | `board/mod.rs:246-252` | Ignores the dock when it is a drawer |
| Board | Skeleton fade-in; focus after the dialog | partial | `board/view.rs:553-645`, `board/task_dialog.rs:332-335` | – |
| Board | Tab between cards | missing | STATUS P12 | Needs a shell `focus_next` |
| Board | Dock grip | partial | `board/dock.rs:28-110` | Writes the setting on every move; keeps its own copy of the maximum-width rule |
| Libraries | `worktrees.ts` | partial | `git_ui/worktrees/naming.rs:16-60` and `orchestration/worker.rs:62-300` | Two copies |
| Libraries | `orchestration.ts` | partial | `orchestration_ui/actions.rs:74-102`, `orchestration_ui/form.rs:28-75` | Copies `state::roles` |

### 2.4 Settings (§2.3/2–3)

Every one of the 53 rows exists, with the TS key, default, range and category (`settings/schema.rs`; defaults at `state/types.rs:432-487`). Reset keeps `voiceApiKey`, `kitsEverywhere`, `projectsFolder` and unknown fields (`state/store.rs:734-748`). Keybinding overrides are kept verbatim, including ids this version does not know (`keymap/bindings.rs:164-199`). Integer fields stay integers.

Only the effects that are not done are listed. The other 42 are wired: locale, projectsFolder, autoResume, claudeBridge, launchDelayMs, hibernation, agentPaneControl, agentCommands, uiScale, panels, theme, the terminal font, padding, cursor, scrollback, shells, notifications, Mobile, Discord, Shortcuts, experiments, About.

| Key | Effect | Evidence | Gap |
|---|---|---|---|
| letterSpacing | missing | `panes/look.rs:74-78`; `hangar-term/src/lib.rs:203` | hangar-term has no letter spacing |
| termPreview | partial | `settings/previews.rs:306` | Ignores letterSpacing |
| voiceEnabled | partial | `commands/run.rs:113` | The key becomes live but dictation never starts (gap 5) |
| voiceEngine, voiceModel, voiceCloudModel, voiceLanguage, voiceSubmit, voiceCleanup, voiceCleanupModel, voiceCleanupHint | partial | `voice/mod.rs:68-82` | Configured, but unreachable (gap 5); the model download works |

The Shortcuts editor is done: search, conflicts, two-step reset all, custom tag, add, remove, reset, "also bound" with free, and a recorder that suspends the dispatcher. `voiceUnload` runs on the UI thread.

### 2.5 Kits management (§2.4/1.1–1.3)

All 10 rows are done:
- the workspace kits section;
- the activation dialog;
- accept, which stores the inspected hashes;
- setup commands, run only on a click of Run, in a visible pane closed on failure;
- the kit icon;
- the installed list;
- "on all workspaces" with review;
- remove, on a second click;
- add from a folder or from GitHub;
- the library.

Evidence is in `kits/update.rs`, `kits/views/*`, `kits/setup.rs` and `kits/gallery.rs`. The launch-time gap is gap 1, counted under §2.6.

### 2.6 Terminal pane and pane grid (§4)

Only rows that are not done are listed; the other 53 are done. All done:
- the terminal options and palette;
- live restyle;
- Shift+Enter in agent panes and Ctrl+V;
- the key filter;
- dead keys, in tests;
- spawn, mount and unmount, respawn and exit;
- accounts noted on launch and exit (`panes/controller.rs:320, 553, 723, 740`);
- the MCP scope launch;
- login panes;
- the bridge reducer and notices;
- `agentCommands`, `launchDelayMs`, `autoResume` and `--resume`;
- role restart;
- the queue;
- broadcast;
- file drop;
- search;
- selection and scroll;
- focus reporting;
- hibernation;
- `TerminalAccess`;
- tree algebra, divider drag, header drag, zoom;
- gauge and glyph.

All nine `panes_logic::Msg` messages are sent by P5.

| Item | Status | Evidence | What the TS does that native does not |
|---|---|---|---|
| letterSpacing | missing | `panes/look.rs:77-78` | – |
| Unfocused pane dimmed to 0.94 | partial | no opacity code | – |
| Web links | partial | `hangar-term/src/pointer.rs:141-147`, `hangar-term/src/links.rs:172`, `panes/mod.rs:423-426` | Plain click and http(s) only; native needs Ctrl+click and opens file, ssh and git links too (§6) |
| Kits held for changed hooks | regressed | `panes/controller.rs:146-159` | Gap 1 |
| Trust prompt detection | partial | `panes_logic/mod.rs:1064-1068` | Reads 60 lines including history, where the TS read the viewport only |
| Role panes: permission mode and allowed tools | partial | `panes/controller.rs:646, 663-668` | Reads the raw `ws.orchestration`; `orchestrationOf` dropped modes that are not offered |
| Non-bridged agents: `session` event and `sessionSince` at launch | missing | `panes/controller.rs:671-676` (TS `TerminalPane.tsx:1098-1102`) | – |
| File-drop highlight | partial | `panes/grid.rs:560-573` | Untested |
| Detached workspace mounted at boot | regressed | gap 3 | – |
| Divider keyboard and aria labels | missing | STATUS P5 | – |
| Header name tooltip `pane.jump {keys}` | partial | `panes/header.rs:184-185` | – |
| Zoom bar: Escape after Tab | partial | `panes/zoom_bar.rs` | iced buttons take no focus |

Logic modules (§3): 56 of the TS modules are ported with at least as many tests. Three are partial:
- `windows`: most of it dropped by design;
- `typography`: applied when the dictionaries are generated, with no runtime module;
- `kitActivation`: 52 Rust cases for 82 TS ones.

Five rows are in flight: `kitTabs`, `kitApi` and `hangarUi` (P16), and `remote*` and `activityLog` (P20). `webgl` is dropped (n/a).

---

## 3. Commands (§1.10)

All 88 default bindings match `shortcuts.ts` on Windows/Linux and on macOS (`keymap/bindings.rs:23-156`).

The dispatcher follows every rule of §1.10 (`keymap/dispatch.rs`):
- the 1600 ms sequence timeout, with the chord hint;
- the field rules;
- the first live command wins; unbound keys pass through;
- a prefix is held; a sequence that leads nowhere swallows its last key;
- only `view.palette` is live while the palette is open;
- the recorder suspends it;
- duplicate and shadow conflicts are found.

Its gaps:
- AltGr is guessed as "Ctrl+Alt with printable text" (`keymap/chord.rs:370-374`).
- It has been checked by unit tests only.
- Field guards cover one dialog (gap 8).

| # | Commands | Status | Evidence | Gap |
|---|---|---|---|---|
| 1–3 | `pane.split`, `pane.splitRight`, `pane.splitDown` | done | `app/shell.rs:550-577` | Live in a workspace with no tree |
| 4–12 | close, restart, restartAll, resetAll (arms only), tidy, sessions, zoom, pin and unpin orchestrator | done | `app/shell.rs:89-131`; `orchestration_ui/mod.rs:302` | – |
| 13–14 | `pane.next`, `pane.prev` | regressed | `commands/run.rs:376-385` → `app/shell.rs:499-524` | Closes the main window's overlays (gap 8) |
| 15–18 | `pane.focusLeft/Right/Up/Down` | regressed | `commands/run.rs:428-431` | Same |
| 19–27 | `pane.focus1`…`9` | regressed | `commands/run.rs:375` | Same |
| 28–31 | `pane.swap*` | done | `commands/run.rs:433-447` | – |
| 32–48 | `workspace.*`: next, prev, new, mcpServers, newOrchestrator, orchestration, stopWorkers, sleep, go1–9 | done | `commands/run.rs:300-317`; packages' `on_intent` | – |
| 49 | `view.settings` | regressed | `commands/run.rs:283`, `app/shell.rs:174-188` | Bypasses the settings package (gap 8) |
| 50–60, 63–65, 67 | shortcuts, palette, navigator, inspector, focus, regions, the four views, board dock, mcp, broadcast | done | `app/shell.rs:30-71, 656-823`; `status_bar/mod.rs:221` | – |
| 61–62 | `view.modelPreview`, `view.closeModels` | done (since P15, not run) | `app/shell.rs:705`; `preview/mod.rs:1418` | – |
| 66 | `view.needsYou` | regressed | `commands/run.rs:162` | Never live (gap 4) |
| 68–74 | `terminal.*` | done | `panes/mod.rs:535-570` | – |
| 75–83 | `command.run1`…`9` | done | `saved_commands/mod.rs:282-291` | – |
| 84–87 | zoom in, out, reset; fullscreen | done | `commands/run.rs:273-293`; `app/shell.rs:189-200` | – |
| 88 | `voice.dictate` | regressed | `commands/run.rs:295` | No handler (gap 5) |

---

## 4. Control ops, engines, events, hooks, intents

General points:
- Core keeps the per-op timeouts (`hangar-core/src/control.rs:89-101`).
- `agentPaneControl` is honoured for agents only (`control/run.rs:31`).
- Requests that arrive before the state loads get the TS text word for word (`app.rs:153-155, 265-270`).
- Refusal texts match `control.ts`, `managerControl.ts` and `deliveryControl.ts`.
- Engines never touch `Model`: their writes go through `Msg::Model` and are answered after the snapshot is republished (`control/writes.rs:133-136`).

| Op(s) | Status | Evidence | Gap |
|---|---|---|---|
| `workspace_list`, `pane_list`, `pane_create`, `pane_close`, `pane_restart`, `pane_read`, `pane_focus`, `workspace_window` | done | `control/panes.rs` | `pane_close` may close any pane, as in the TS |
| `pane_send` | done | `control/panes.rs:24, 411-419` (120 ms between paste and Enter) | – |
| `workspace_create` | done | `control/workspace.rs:42`; P8's flow hook (`app/hooks.rs:19-22`) | – |
| `workspace_set_check` | done | `control/workspace.rs:178-216` | Its write can lose a panel edit (gap 9) |
| `preview_show`, `preview_close` | done (since P15, not run) | `preview/mod.rs:714-723` installs `Hook::Preview` | – |
| `model_show`, `model_snapshot`, `model_close` | in flight (P17) | `control/preview.rs`; `control/live.rs:421-545` fallback | – |
| `kit_tab_list`, `kit_tab_open`, `kit_tab_close` | in flight (P16) | `control/kit_tabs.rs:54, 68, 144, 200` | – |
| `worker_spawn`, `worker_stop` | done | `orchestration/worker.rs:716, 1303` | A narrow race on a respawned worker (`worker.rs:1327, 1371`) |
| `worker_list` | partial | `orchestration/worker.rs:1156` | Ignores detached workspaces (`orchestration/live.rs:113, 403` read `snapshot.opened` only) |
| `delivery_submit`, `delivery_status` | done | `control/delivery.rs:470, 772` | – |
| `project_list`, `project_status`, `project_send` | done | `control/manager.rs:24, 93` | – |

| Engine | Status | Evidence / gap |
|---|---|---|
| orchestratorEvents: 10 s tick, board debounce, quiet during the orchestrator's own turn | done | `orchestration/runtime.rs:42, 119-145`; `orchestration/events.rs:218-232, 971-981` |
| managerEvents feed | done | `orchestration/manager_feed.rs:404-418` (the phone push is P20) |
| 5 s ticker (activity log) | in flight (P20) | `orchestration/runtime.rs:44, 233-241` |
| Briefing | done | `control/mod.rs:207` → `control/engines.rs:28` |
| announceRole | partial | Inferred from `PinOrchestrator` (`orchestration/mod.rs:179-201`); `Intent::AnnounceRole` has no handler |
| deliveryRuns read-model | partial | Read-model done (`control/mod.rs:115-161`); Needs-you's « Fusionner » ignores it |
| Check, separate reviewer, per-repository queue, land, three failures → needs-you | done | `control/delivery.rs:813-1163`; `hangar-core/src/delivery.rs:1126, 1556` |
| Worker spawn in worktrees, all-or-nothing undo | done | `orchestration/worker.rs:855-1051` |
| Restart notes | done | `control/writes.rs:57-70` |
| `touch` keep-awake | done | `control/run.rs:57-59` → `panes_logic/mod.rs:942` |
| Delivery cancel on shutdown | done | `app.rs:446` → `hangar-core/src/lib.rs:184-197` |
| Stop all workers | done | `orchestration/mod.rs:157`, `orchestration/worker.rs:1496` |

Events: `services/events.rs:137-166` matches every core `Event` with no wildcard. Under load, events are logged and dropped (`events.rs:208-210`); a control request lost that way only times out on core's side.

| Event | Consumers | Status |
|---|---|---|
| `pty:output` | each pane's sink, by design | done |
| `pty:exit` | `panes_logic`, `panes`, `orchestration` | done |
| `bridge:event` | `panes_logic`, `panes`, `toolbar`, `status_bar`, `navigator`, `accounts`, `orchestration` | done |
| `bridge:queue` | `panes_logic`, `panes` | done |
| `board:changed` | `board`, `orchestration`, `needs_you`, `orchestration_ui` | done |
| `memory:changed` | `memory` | done |
| `kits:changed` | `kits` | done |
| `accounts:changed` | `accounts` | done |
| `git-clone:progress` | `workspace_dialog` | done |
| `voice:phase`, `voice:level`, `voice:text`, `voice:download` | `voice`, `settings` | done |
| `control:request`, `control:internal` | `Message::Control(Msg::Request)` | done |
| `notification-activated` | `app.rs:412` | done |
| `window:close-requested` | `app/shell.rs:217` | done |
| file drag and drop | `panes`, `panes_logic` | done |
| `remote:changed` | `settings` only | partial (P20) |
| `remote:shared` | none | in flight (P20) |

| Hook | Installed by | Status |
|---|---|---|
| `Hook::Orchestration` | `control/mod.rs:88-97` | done |
| `Hook::FromGit` | `app/hooks.rs:19-22` | done |
| `Hook::Preview` | `preview/mod.rs:714-723` (P15) | done |
| `Hook::KitTabs` | none | in flight (P16) |
| `Hook::Mobile` | none; `mobile/mod.rs` is still the stub | in flight (P20) |

Snapshot: `opened`, `asleep`, `activity` and `delivering` are filled from the real read-models (`state/snapshot.rs:33-43`). `orchestration::port` now re-exports P18's `Caller` and `Announcement`.

---

## 5. Integration seams

### 5.1 Every `TODO(P…)` marker

| # | Marker | Named package merged? | Provider now | Action |
|---|---|---|---|---|
| 1 | `board/agent_pane.rs:18` TODO(P4) | yes | `panes_logic::find_agent_pane` (`panes_logic/agent_pane.rs:44`, dead code) | Delete the file; call P4's |
| 2 | `board/dock.rs:45` TODO(P0b) | yes | `shell::side_panels::Panels::of(m, w).dock.max`, or `ctx.panels()` | Replace `max_width`, `room` and `dock_room` (`board/mod.rs:353-360`) |
| 3 | `control/delivery.rs:411` TODO(P8) | yes | `accounts::model::launch_account_with` (`accounts/model.rs:114`) | Replace the copy |
| 4 | `control/live.rs:424` TODO(P15) | yes (`22ef8a2`) | `Hook::Preview` from `preview/hosts.rs` | Keep `AccessHost` only as the before-hook fallback, or delete it |
| 5 | `control/run.rs:110` TODO(P0b) | yes | `state::layout::reading_order` / `normalize_tree` (`state/layout.rs:309, 336`) | Replace `ordered` |
| 6 | `control/run.rs:356` TODO(P0b) | yes | `state::roles::orchestration_of` (`state/roles.rs:37`, dead code) | Replace the third copy |
| 7 | `inspector/sources.rs:41` TODO(P0b) | yes | `state::layout::reading_order` | Replace |
| 8 | `mcp/servers_panel.rs:84` TODO(P4) | yes | `m.live.panes.report(&pane.id).and_then(\|r\| r.mcp.as_ref())` (`panes/mod.rs:121-125`); `PaneInsight.mcp` has no writer | Read it and map `PaneMcpMode` to `McpMode` (gap 6) |
| 9 | `mcp/servers_panel.rs:94` TODO(P4) | yes | `m.live.panes_logic.activity_or_idle(id) == Working` (`panes_logic/mod.rs:298`) | Replace |
| 10 | `navigator/clock.rs:7` TODO(P9) | yes | `inspector::format::{prose_time, reset_when}` (`inspector/format.rs:210, 333`) | Delete `clock.rs` |
| 11 | `navigator/menu.rs:257` TODO(P4) | yes | `panes_logic::hibernate::{forced_sleep_verdict, sleep_refusal}` exist, but no synchronous `Live` accessor | P4 to expose a verdict, then the navigator reads it |
| 12 | `navigator/model.rs:105` TODO(P0b) | yes | `state::layout::{reading_order, closed_by_preset}` (`closed_by_preset` is dead code) | Delete the copies (also used by `toolbar/mod.rs:36,150`, `toolbar/activity.rs:21,139`) |
| 13 | `navigator/model.rs:324` TODO(P0b) | yes | `state::folders::sidebar_order` | Delete the copy (used by `needs_you/mod.rs:34,138,153`) |
| 14 | `navigator/mod.rs:107` TODO(P4) | yes | `panes_logic::Live::plan_limits()` (`panes_logic/mod.rs:330`) | Drop `State::plan` |
| 15 | `navigator/reads.rs:115` TODO(P7) | yes | `KeymapIndex::label` / `title` (`keymap/index.rs:182, 189`) | Gap 7 |
| 16 | `navigator/reads.rs:138` TODO(P14, P16) | P14 yes, P16 no | `kits::Live` plus `kits::model::effective_kits` (`kits/model.rs:248`, dead code) for the list | List now; opening a tab waits for P16 |
| 17 | `navigator/reads.rs:156` TODO(P15) | yes | `preview::Live::{has_preview, panel_open, unseen}` (`preview/mod.rs:257-276`) | Draw the toggle |
| 18 | `navigator/reads.rs:165` TODO(P4) | yes | `panes_logic::Live::asleep()` (`panes_logic/mod.rs:364`) | The moon |
| 19 | `navigator/reads.rs:8` TODO(P4) | yes | `activity_or_idle`, `workspace_activity`, `bridged_count` (`panes_logic/mod.rs:298-325`) | Replace the glyph-based reads |
| 20 | `needs_you/mod.rs:15` TODO(P12) | yes | `board::Live::{tasks, boards_of}` (`board/cache.rs:60, 71`) | Drop `State::boards` |
| 21 | `services/clipboard.rs:4` TODO(P17) | no (in flight) | – | – |
| 22 | `services/preview.rs:33` TODO(P15) | yes | filled by `preview/access.rs:394` | Delete the marker |
| 23 | `shell/ui.rs:4` TODO(hangar-ui) | yes | hangar-ui has no `CloseReason` or menu-entry type | Keep the types; drop the TODO, or move them to hangar-ui |
| 24 | `toolbar/activity.rs:223` TODO(P4, P12) | yes | `board::Live::task_for_pane` (`board/cache.rs:84`) | The capsule's task |
| 25 | `toolbar/activity.rs:29` TODO(P4) | yes | `panes_logic::Live::insight(id)` (`panes_logic/mod.rs:341`) | Drop `Glance` |
| 26 | `toolbar/mod.rs:367` TODO(P15) | yes | as #17 | – |
| 27 | `workspace_dialog/folder.rs:115` TODO(P0b) | yes | `state::pane_names::pick_pane_names` (`state/pane_names.rs:49`, dead code) | Delete the 48-name copy |

The untagged "TODO of P0b" at `settings/mod.rs:25` (Escape while the search is full) is likely resolved: the shell only takes an Escape nothing else captured (`app.rs:545-553`).

### 5.2 Local copies that should call the real one

The dead-code pass shows the providers below are never called, while copies run:

| Real provider (dead code) | Copies in use |
|---|---|
| `state::roles::{orchestration_of, orchestration_defaults, ORCHESTRATION_PERMISSION_MODES, ORCHESTRATION_ALLOWED_TOOLS, manager_of, hq_of}` | `control/run.rs:356-396`, `orchestration/roles.rs`, `orchestration_ui/actions.rs:74-102`, `orchestration_ui/form.rs:28-75`, `panes/controller.rs:55, 646`, `git_flow/deps.rs:228-234` |
| `state::layout::closed_by_preset` | `navigator/model.rs:138-141` |
| `state::pane_names::pick_pane_names` | `workspace_dialog/folder.rs:109-156` |
| `panes_logic::find_agent_pane`, `PaneTarget` | `board/agent_pane.rs` |
| `kits::model::{pane_kits, load_pane_kits, kit_issue_notes, effective_kits}` | `panes/controller.rs:146-159, 455` (gap 1) |
| `control::Live::task_delivering` (live), `control::delivery::{delivery_state, delivering_in}` (dead code, for P20) | `needs_you/mod.rs:44-47` (a 45 s timer) |
| `state::layout::reading_order` | `navigator/model.rs:89`, `inspector/sources.rs:43`, `palette/sources.rs:212`, `control/run.rs:110` |
| `state::folders::sidebar_order` | `navigator/model.rs:326`, `palette/sources.rs:234` |
| `panes_logic::state_glyph_state`, `context_gauge::format_tokens`, `folder_of` | `panes/status.rs:20`, `panes/gauge.rs:15`, `panes/controller.rs:139` |
| `Model::window_workspace` / `ctx.workspace()` | `inspector/sources.rs:30-35`, `palette/mod.rs:201-209`, plus the `active_workspace()` reads of gap 2 |
| `shell::side_panels::Panels::of` | `inspector/mod.rs:134`, `board/mod.rs:246, 353-360`, `board/dock.rs:45-67` |
| `board::Live::tasks` | `orchestration_ui/mod.rs:84-133` (its own title cache) |
| `orchestration_ui::actions::pin_refusal` | `pane_menu/mod.rs:142-150` (`can_pin`) |
| hangar-term's OSC 9/777 parsers (the ones that run) | `panes_logic/pane_agent.rs:131, 147` (dead code) |
| none yet (worktree naming twice; the inventory wanted it in hangar-core) | `git_ui/worktrees/naming.rs`, `orchestration/worker.rs:62-300` |

### 5.3 Intents nobody handles

The audit checked all 59 variants of `Intent`. 54 have a real handler in the shell (`app/shell.rs:25-161`) or in a package's `on_intent`.

| Intent | Sent by | Handled by | Status |
|---|---|---|---|
| `Dictate` | `commands/run.rs:295` | nobody | regressed (gap 5) |
| `AnnounceRole` | `orchestration_ui/mod.rs:178` | nobody. P19 infers it from `PinOrchestrator` before P10's refusal is known (`orchestration/mod.rs:179-201`), so a refused pin can queue a role message | partial |
| `PaneRespawned` | `app/shell.rs:314-321` | no explicit handler. P5 respawns through `sync`. The doc (`message.rs:241-242`) says P4 handles it; it does not, and need not: the TS carried the resume only from the control path | partial (fix the doc) |
| `Land` | `needs_you/mod.rs:207` | `control/mod.rs:241-288` | partial: an error becomes a notice; `needs_you::Msg::LandFailed` is never sent |
| `OpenKitTab` | `toolbar/menus.rs:365` | shell (view and drawer) | in flight (P16) |

### 5.4 Hooks not installed

`Hook::KitTabs` (P16) and `Hook::Mobile` (P20) are in flight. The other three are installed (§4).

### 5.5 `Live` read-models nothing fills

- `live.signals.needs_you` (`state/live.rs:99`) has no writer outside tests (gap 4).
- `PaneInsight.mcp` (`state/live.rs:406`) has no writer. The launch's MCP lives in `panes::Live` (gap 6).
- The 22 unit `Live` structs (`pub struct Live;`) are not read by anyone, which is fine.

### 5.6 Debug-only seeds

Every seed is behind `cfg!(debug_assertions)`, so no release build reads one:
`HANGAR_DEV_ACTIVITY`, `_FOCUSED_PANE`, `_PLAN`, `_NAV_MENU`, `_NAV_RENAME`, `_NAV_DRAG`, `_NEEDS_YOU_OPEN`, `_VOICE`, `_UPDATE_BANNER`, `_SETTINGS`, `_PREVIEW`, `_SCREENSHOT`.

Three of them cover over a gap in screenshots:
- `HANGAR_DEV_NEEDS_YOU_OPEN` hides gap 4.
- `HANGAR_DEV_VOICE` hides gap 5.
- `HANGAR_DEV_ACTIVITY` overrides P4's real activity in debug builds.

Remove them once the gaps are fixed.

---

## 6. Risks to data and to the person

### Data

| Risk | Where | Severity | Fix |
|---|---|---|---|
| A read error other than a parse error (I/O, invalid UTF-8, permissions), a non-object root, or a lenient-read failure starts the app empty, and the next change overwrites `state.json`. The TS had the same hole. | `hangar-core/src/store.rs:83`; `state/lenient.rs:38-55`; `app.rs:332-335` | medium (rare, total loss) | Quarantine on every load error, or refuse to save until the person acts, with a notice |
| `workspace_set_check` writes a whole `orchestration` block built from a snapshot over a panel edit made in between | `control/workspace.rs:205-216` | low | Set only `check_command` inside the patch |
| The inspector and dock grips write the settings on every pointer move: a save is scheduled and the snapshot republished each time | `inspector/mod.rs:248-253`, `board/mod.rs:569` | cost only | Commit on release, as the TS did |
| Boards written from a detached window go to another workspace's `board.json` | `board/mod.rs:335-343, 457-478` | medium | Gap 2 |
| A dev build writes the person's real files outside `HANGAR_DATA_ROOT` | see the list below | medium for dev machines | In debug builds with `HANGAR_DATA_ROOT` set, refuse or ask before the MCP install and autostart |

What a dev build writes outside `HANGAR_DATA_ROOT`:
- **The MCP install panel** writes the dev binary (`current_exe`, `hangar-core/src/install.rs:153`) into `~/.claude.json`, `~/.codex`, `~/.gemini`, Cursor and VS Code. This replaces the installed app's entry.
- **The Mobile autostart switch** rewrites `~/.config/autostart/Hangar.AI.desktop` to point at the dev binary (`hangar-core/src/autostart.rs`).
- **The git-clone trust step** edits `~/.claude.json` (`git_flow/deps.rs:174-190`). The TS did this too.

Checked and safe:
- The state, the endpoint, the mods and the accounts go under the dev home.
- `account_remove` and `discard_clone` are guarded.
- Destructive git operations are confirmed inline.
- A run on the fixture changed nothing in `state.json`.

### Processes

| Risk | Where | Severity |
|---|---|---|
| In a detached window, Restart all, Reset agents and the presets that close panes kill another workspace's agents. Broadcast types text plus Enter into them. | `toolbar/mod.rs:147, 260, 558-560`; `status_bar/mod.rs:159-170` | high (gap 2) |
| "Restart N agents" in the MCP servers panel respawns every Claude Code pane, mid-turn ones included, with no warning | `mcp/servers_panel.rs:155-164` | medium (gap 6) |
| Quit and close kill only the shell (`child.kill()`, no process group), and nothing handles SIGTERM or SIGINT. Processes that ignore SIGHUP survive. The old app was the same. | `hangar-core/src/pty.rs:746-752` | low |
| The engine's mutex is held across a tick that reads every board; `pty_kill` and `pty_alive` run on async threads | `orchestration/runtime.rs:180-191, 247-259`; `orchestration/live.rs:183-189` | low (stalls, not kills) |
| `stop_worker` can miss a worker respawned in that instant, then force-remove its worktree | `orchestration/worker.rs:1327, 1371` | very low |

### Commands without consent

| Risk | Where | Severity |
|---|---|---|
| A GitHub kit's changed hooks run without being accepted again | `panes/controller.rs:146-159` | **high** (gap 1) |
| Ctrl+click on a link opens any URI: `file:`, `ssh:`, `git:` and any OSC 8 target, through `open::that_detached`. The TS opened web links only. | `panes/mod.rs:423-426`; `hangar-term/src/links.rs:172` | medium |
| Programs may write the clipboard (OSC 52). The TS ignored OSC 52. | `panes/pty_link.rs:158-161`; `panes/mod.rs:527` | low (decide, then record the decision in STATUS) |
| Kitty keyboard protocol on by default: agents that negotiate it get other key encodings than under xterm.js | `hangar-term/src/config.rs` | behaviour change |

Checked and safe:
- Setup commands run only on a click of Run.
- Saved commands run only on a click or a shortcut.
- `tailscale serve` runs only from its button.
- The updater installs only after Restart.
- Removing a kit takes two clicks.

Note about this audit's own run: the scratchpad is shared, and its `devhome/data/state.json` (a scratch copy, not the installed app's) was overwritten once before the run moved to its own folder.

---

## 7. Fix list: work packages for parallel agents

The packages own disjoint files and are listed most valuable first. Each one reads this file, then PACKAGES.md. Every package ends with `cargo test -p hangar-app`, `cargo clippy -p hangar-app --all-targets -- -D warnings`, and a run through `scripts/native-dev.sh` with a temporary `HANGAR_DEV_HOME`.

### WP1 — Pane launch safety (`panes/**`)

1. Launch kits through `kits::model::load_pane_kits`. Spawn and write the MCP scope with `ids` only. Show `held` with `kit_issue_notes` (the `KitsManageWarnHooks` note). Delete `kits_of` and `kit_notes` (`controller.rs:146-159, 455`).
2. Take the permission mode and allowed tools from `state::roles::{orchestration_of, ORCHESTRATION_ALLOWED_TOOLS}` (`controller.rs:55, 646`).
3. Open http and https links only (`mod.rs:423`).
4. Drop OSC 52 clipboard writes, or put them behind a setting, and record the decision (`pty_link.rs:158`).
5. Replace the copies of `state_glyph_state`, `format_tokens` and `folder_of` with P4's.
6. Skip `sync`/`publish` for terminal actions other than Resize and Pressed (`mod.rs:195-207`).
7. Put the terminal search field behind `keymap::guard`.

Verify:
- A new controller test: a git kit with unaccepted hooks is not in `ids` and its note is shown.
- In a dev home with such a kit, the pane prints the warning.
- Ctrl+click on a `file://` link does nothing.

### WP2 — Shell: windows, keys, persistence (`app.rs`, `app/**`, `commands/**`, `shell/**`, `state/**`, `message.rs` docs, `main.rs`; `hangar-core/src/{store.rs, pty.rs, install.rs, autostart.rs}`)

1. A workspace whose window opens (at boot or on detach) joins `ui.opened_ids` (`app/shell.rs:966-991`, `state/store.rs:322-328`).
2. Revert a detach whose window fails to open, with the `window.openFailed` notice.
3. Make `view.settings` go through `Intent::OpenSettings(None)` / `Intent::CloseOverlay`, so the sheet syncs (`commands/run.rs:283`).
4. Pane-focus commands record the focus without clearing overlays. Keep the full landing (`focus_pane`) for notifications, the board and Needs-you; append an intent if one is needed (`commands/run.rs:367-431`).
5. Keep a broadcast text per window in `ui` (with its parked copy), and give `status_bar` an accessor to read.
6. On load errors that were not quarantined, and on the lenient read's fallback to empty, do not save until the person acts. Show a notice. In core, quarantine on read errors too.
7. Route SIGTERM and SIGINT to `quit`; make `pty::kill_all` kill the process group.
8. In debug builds with `HANGAR_DATA_ROOT` set, `install::mcp_install` and `autostart::set_enabled` refuse with a clear error.
9. Fix the `PaneRespawned` doc. Decide `shell/ui.rs`'s TODO.
10. Make pane commands need a tree.

Verify:
- A headless test boots `typical.json`, where `site` is detached, and finds `site` in `opened_ids` and its pane spawned.
- `pane.focus2` with Settings open keeps the overlay.
- Mod+, then a category click stays on that category.
- A truncated, non-UTF-8 `state.json` in a dev home is not overwritten.

### WP3 — Needs-you and voice (`needs_you/**`, `voice/**`)

1. Write `m.live.signals.needs_you` with the item count whenever the items are computed.
2. Read boards from `board::Live`, and the landing state from `m.live.control.task_delivering`.
3. Handle `Intent::Dictate` as `Msg::Press`. Check `voice_recording` at start.
4. Remove `HANGAR_DEV_NEEDS_YOU_OPEN` once the panel opens for real.

Verify:
- A test: a model with a waiting pane and a needs-you task makes `view.needsYou` live, and the panel stays open after `OpenNeedsYou`.
- A test: `Intent::Dictate` starts the press.
- A dev run with voice enabled shows the HUD on Mod+Alt+V.

### WP4 — Chrome reads and per-window chrome (`navigator/**`, `toolbar/**`, `status_bar/**`, `palette/**`, `pane_menu/**`, `saved_commands/**`, `accounts/switcher.rs`)

1. `navigator/reads.rs`:
   - `binding` from `ctx.keymap.label` and `title`;
   - activity and counts from `panes_logic`;
   - `asleep_at` from `panes_logic.asleep()`;
   - the kits list from `kits::Live` and `effective_kits`;
   - `models` from `preview::Live`.
2. Delete the copies in `navigator/model.rs` and `navigator/clock.rs`. Take the plan from `panes_logic.plan_limits()`, and worker counts from `state::roles::workers_of`.
3. Toolbar capsule: P4's insight, and `board::Live::task_for_pane`.
4. Act on the window's own workspace: `ctx.workspace()` in views, `m.window_workspace(focused)` in `update` (the rows of gap 2 in these directories).
5. Remove the doubled account separators. `can_pin` → `pin_refusal`. A saved-command run from the menu goes through `Intent::RunSavedCommand`.
6. Put `keymap::guard` on the broadcast input, the palette field and the navigator filter.

Verify:
- iced_test: a tooltip carries the binding.
- A two-window test: Restart all from a detached window respawns only that workspace's panes.
- A screenshot with a sleeping workspace shows the moon.

### WP5 — Pane read-models, MCP panel and inspector (`panes_logic/**`, `mcp/**`, `inspector/**`)

1. `running_mcp` from `panes::Live::report(id).mcp`, mapped onto `scope::PaneMcp`. `is_working` from `activity_or_idle`. The inspector's MCP row reads the same provider.
2. Inspector:
   - `reading_order` from `state::layout`;
   - the window's own workspace (`mod.rs:310`, `sections.rs:823`);
   - `shown` and the width cap from `Panels::of`;
   - commit the grip on release.
3. `panes_logic`:
   - move `bridge_enqueue` in `Mounted` to `spawn_blocking`;
   - expose a synchronous sleep verdict for the navigator's menu;
   - add `Msg::Launched{pane, resumed}`, which records the session event and `sessionSince` for agents without the bridge. Afterwards, WP1 adds the one call in `panes/controller.rs`.

Verify:
- A test of `needs_restart` with a known launch: "up to date" shows, and only stale panes are in Restart.
- An inspector test shows the MCP row.

### WP6 — Board and orchestration UI (`board/**`, `orchestration_ui/**`)

1. The board and dock use the window's workspace; messages carry the cwd (`board/mod.rs:240-242, 673-690, 787-798`).
2. `board_shown` and the dock's maximum from `Panels::of`; commit the grip on release.
3. Delete `board/agent_pane.rs` and call `panes_logic::find_agent_pane`.
4. `orchestration_ui` takes `state::roles`; the badge's titles come from `board::Live`.
5. Put `keymap::guard` on the add-card field and the task dialog's fields.

Verify:
- A two-window test: a card added in a detached window lands on that workspace's board.
- A worker badge follows a title edited from its worktree.

### WP7 — Control and orchestration seams (`control/**`, `orchestration/**`)

1. `workspace_set_check` patches `check_command` only.
2. `control/run.rs`'s `ordered` and `orchestration_of` → `state::layout` / `state::roles`. `control/delivery.rs`'s `launch_account` → `accounts::model::launch_account_with`.
3. A Land that fails also sends `needs_you::Msg::LandFailed`.
4. Orchestration:
   - handle `Intent::AnnounceRole`, and drop the inference from `PinOrchestrator`;
   - use `control::port::is_open` for detached workspaces (`orchestration/live.rs:113, 403`);
   - run `pty_kill` and `pty_alive` on `spawn_blocking`;
   - do not hold the engine mutex across the board reads;
   - `orchestration/roles.rs` → `state::roles`.
5. Decide whether to keep the `AccessHost` fallback now that P15 installs `Hook::Preview`.

Verify:
- A refused pin queues no role message.
- A panel edit made during `set_check` survives.
- `worker_list` reports a detached workspace's workers as running.

### WP8 — Field guards and small partials (`settings/**`, `workspace_dialog/**`, `memory/**`, `kits/**`, `git_ui/**`, `stats/**`)

1. Put `keymap::guard` around the settings search, every workspace dialog field, the memory search and composer, the kits library filter, and the worktree create form.
2. Workspace dialog:
   - pane names from `state::pane_names::pick_pane_names`;
   - Enter submits from every field;
   - focus the first field on open;
   - an unknown clone percentage shows an indeterminate bar.
3. Session picker and diff partials, where iced allows.

Verify: `iced_test`: Mod+Shift+Enter in a guarded field runs the shortcut and types nothing.

### Later, needing work outside these files

- `letterSpacing` and unfocused-pane opacity in hangar-term.
- Keyboard resizing of dividers (iced has no focusable separator).
- A synchronous trust-prompt read of the viewport only: `TerminalAccess::screen_text` (`services/terminals.rs`, after WP2) and `panes/hub.rs` (after WP1).
- The 16-busy-pane benchmark.
- Running Codex, Gemini CLI and OpenCode panes in the real app.
- The parity pass the inventory plans (§8.1, wave 2): the old app beside the new one on a copy of a real `state.json`.
