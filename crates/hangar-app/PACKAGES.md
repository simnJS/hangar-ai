# hangar-app: what a package agent reads first

`crates/hangar-app` is split so that the frontend packages of
[inventory-frontend.md §8](../../docs/native/inventory-frontend.md) can be
ported in parallel without touching each other's files. P0 (the shell) made
one directory per package, each with a stub `mod.rs` the shell already calls.
Your package's job is to replace the stub bodies with the real thing.

Read, in this order: this page, your package's row of inventory §8.1 and
§8.3, the inventory sections your stub's header names, then the TypeScript it
ports and its tests.

## Running it

```bash
scripts/native-dev.sh                       # the app, on data under target/dev-home/
HANGAR_DEV_SCREENSHOT=/tmp/x.png scripts/native-dev.sh   # a PNG of the main window, then quit
cargo test -p hangar-app && cargo clippy -p hangar-app --all-targets -- -D warnings
```

- **Never run the app plainly.** The released Hangar is installed and in use
  on the development machines, and both use the same `state.json`, endpoint
  file and Claude Code mods. A debug build refuses to start unless
  `HANGAR_DATA_ROOT` and `HANGAR_ENDPOINT_FILE` are set, which
  `scripts/native-dev.sh` does. `HANGAR_DEV_HOME=<dir>` points it at another
  folder; put a `data/state.json` there to start on a given state.
- `HANGAR_DEV_SCREENSHOT` is how you see your surface: screen capture is
  refused on the GNOME Wayland session. `HANGAR_DEV_SCREENSHOT_AFTER_MS`
  (default 3000) is the wait before the capture. The picture is the main
  window's view drawn off screen at that moment, at the window's size and
  scale (shell/dev_screenshot.rs says why not iced's `window::screenshot`):
  what the widgets keep themselves, a scroll offset or the caret, starts
  afresh in it. `HANGAR_DEV_KIT_TABS=<kit>/<tab>?<param>=<value>&…` opens
  kit tabs first (kit_tabs/dev.rs).
- `-j 16` at most; other agents build on the same machine. Never `cargo clean`.

## The rules (inventory §8.2)

1. **Your directory is yours.** `src/<pkg>/` belongs to your package only:
   add files, split `mod.rs`, add tests. Edit nothing else in the crate,
   except rule 5.
2. **Keep the surface the shell calls.** Every stub exports the same items,
   and the view and embed functions of the table below. Keep their names and
   signatures; everything else in the stub is yours to change.

   ```rust
   #[derive(Debug, Default)] pub struct State;   // App.pkg.<pkg>: your UI state
   #[derive(Debug, Default)] pub struct Live;    // Model.live.<pkg>: what other packages read
   #[derive(Debug, Clone)] pub enum Msg {}       // routed as Message::<Route>(Msg)
   pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message>;
   pub fn subscription(st: &State, m: &Model) -> Subscription<Message>;
   pub fn focus_entry(st: &State, m: &Model, region: Region) -> Option<Task<Message>>;
   pub fn on_intent(st: &mut State, m: &mut Model, sv: &Services, intent: &Intent) -> Option<Task<Message>>;
   pub fn on_event(st: &mut State, m: &mut Model, sv: &Services, event: &CoreEvent) -> Option<Task<Message>>;
   ```

   `State` and `Live` must stay `Debug + Default`; `Msg` must stay
   `Debug + Clone`. Views take `(st: &'a State, ctx: Ctx<'a>, …)` and return
   `Element<'a, Message>`; wrap your own messages as `Message::<Route>(…)`.
3. **The persisted model changes only through `m.apply(StoreAction)`**, which
   answers the new ids (`StoreOutcome`), schedules the save and republishes
   the snapshot. `m.state()` reads it; there is no other way to write it.
   It runs against the state as the last action left it, so two in a row
   each see the other's change, as the TS `update` did.
4. **Ask other packages through intents**, never by calling their `update`:
   return `Task::done(Message::Intent(Intent::…))`. The shell handles its part
   (pushing an overlay, focusing a pane), then offers the intent to every
   package's `on_intent`.
5. **A missing intent is one appended variant** at the end of `Intent` in
   `src/message.rs`, in a commit of its own, with a comment naming the
   package that handles it; handle it in your own `on_intent`. That is the
   only shared edit. If a payload type the shell defined lacks something
   (`PreviewRequest`, `GitRequest`…), append a variant carrying a type of
   your own package instead of editing the shell's; say so in your report.
6. **Engines (P18–P20) never touch `Model`.** On tokio they read
   `sv.snapshot()` (an `Arc<Snapshot>` republished after every change, or
   `sv.snapshots()` to wait on the next) and act with `sv.send(Message::…)`,
   which the shell's inbox subscription feeds to `update`. Model writes from
   an engine are `Message::Control(control::Msg::Model(cmd, answer))`, with
   `services::inbox::Answer::channel()` to await the outcome.

And the house rules: no literal UI text (every string is a `hangar_i18n::Key`,
`ctx.t(Key::…)`); no colours picked in a view (`hangar_theme::style::*` with
`ctx.theme`, sizes from `hangar_theme::tokens`); port the vitest suites next
to the code as `#[cfg(test)]`; comments are full sentences that explain why.

## What the shell gives you

| Item | Where | What |
| --- | --- | --- |
| `Model` | `state/mod.rs` | `state()` (the `AppState` of `state/types.rs`), `apply(StoreAction)`, `settings()`, `workspace(id)`, `active_workspace()`, `theme()`, `locale()`, `ui` (`state/live.rs`: view, overlays, drawer, focus per workspace, zoom, opened ids, notice, available agents, shells…), `live` (every package's `Live`, `hydrated`, `windows`, `signals`). Per window: `window_workspace(w)`, `view_in(w)`, `overlays_in(w)`, `drawer_in(w)`, `theme_in(w)`, `focused_window()`, `window_of(ws)`; `focused_pane(ws)`. `ui.view`, `ui.overlays`, `ui.drawer` and `ui.focus` are the focused window's; the others' wait in `ui.parked`. |
| Store helpers | `state/` | `store::{make_pane, make_workspace, new_id}`, `layout::*` (layout.ts: `normalize_tree`, `reading_order`, `neighbor_of`, `compute_layout`…), `pane_names::*`, `folders::{sidebar_order, place_before}`, `roles::{orchestration_of, orchestrator_of, workers_of, manager_of, hq_of}`. |
| `live.signals` | `state/live.rs` | What a package tells the shell, which the shell reads: `preview[ws]` (P15 writes `entries` and `unseen`; the shell's toggles write `open`/`expanded`, which P15 reads, and P15 sets `open` when it reveals an entry), `needs_you` (P6: the item count, `view.needsYou` live while > 0). The snapshot takes P4's activity and sleepers and P18's deliveries from their own read-models. |
| Shared value types | `state/live.rs` | `AgentActivity`, `PaneActivity`, `GlyphState`, `RateLimit`, `PlanLimits`, `PaneInsight`, `BridgeEvent` (with `BridgeEvent::parse`), `QueuedPrompt`. P4 owns their meaning; ask in your report if one needs a field. |
| `Services` | `services/mod.rs` | `core()` (`host()`, `pty()`, `core()` for the voice, Discord and Mobile managers), `terminals()` (`TerminalAccess`, P5), `preview()` (`PreviewAccess`, P15), `send(Message)`, `snapshot()`, `snapshots()`. Clone it onto a tokio task. |
| hangar-core | the crate | Call it directly: [core-api.md](../../docs/native/core-api.md) maps every former `invoke` to its function. Sync ones that touch disk or git go on `tokio::task::spawn_blocking` inside `Task::perform`. |
| `CoreEvent` | `services/events.rs` | Every core event but control requests, offered to every package's `on_event`. Control requests go to P18 as `control::Msg::Request(ControlRequest)` with a `Reply` (first `send` wins). A pane's output is not an event: P5 gets it from its `PtySink`, and sends `CoreEvent::PtyExit` through the inbox when a shell ends. |
| `Ctx<'a>` | `ctx.rs` | `model`, `pkg` (every package's `State`, read-only, for embeds), `i18n` (`Translator`), `theme` (`&'static hangar_theme::Theme`, the window's workspace's), `metrics` (`scale`, `body_width`), `keymap`, `window`, `now`; and the window's own `workspace()`, `view()` (read it rather than `ui.view`) and `panels()` (where the side panels go). `Copy`. |
| Shell types | `shell/` | `View`, `Region`, `Focus`, `Drawer`, `Overlay`, `SettingsCategory`, `Notice`, `Notification`, `PanelMode`, `BoardVariant`, and the stand-ins `CloseReason`, `MenuEntry` until hangar-ui has them. |
| Commands | `commands/` | `CommandId` (the 88, `as_str`/`FromStr`, `section`, `label_key`, `label_n`, `in_palette`), `available(m, w)` (the live ones, in keymap order), `is_available(m, w, id)`, `run(m, w, id)` (nothing when not live). |
| Shell intents | `message.rs` | The shell sends `Intent::PaneRespawned` after every `RespawnPane`, whoever applied it (P4 carries the resume, P5 respawns the PTY). The command table sends `TerminalAction` (P5), `Dictate` (P6), `ClosePreviews` (P15). |
| Placeholders | `shell/placeholder.rs` | `stub`, `stub_inline`, `stub_modal`: what the stubs draw. Delete their calls as you replace them. |

Dependencies: `hangar-core`, `hangar-theme`, `hangar-i18n`, `hangar-term`,
`iced`, `tokio`, `serde`, `serde_json`, `rfd`, `open`, `png` are in. The
coordinator adds `hangar-ui` to `crates/hangar-app/Cargo.toml` when it merges
(until then, plain iced widgets with `hangar_theme::style`, marked
`TODO(hangar-ui)`); P16 adds `hangar-kit-host`, P17 `hangar-model3d`. Any other
crate: one line in `[dependencies]` (and `[workspace.dependencies]` if new),
in a commit of its own; never a bare `cargo update`.

Things iced makes you know: `Subscription::map` takes only a non-capturing
closure or a fn (`Message::Board` works); a subscription that needs data uses
`Subscription::run_with(data, fn)`; views are rebuilt after every message, so
keep them cheap and cache what is costly in `State`.

## The packages

"Shell calls" lists what the router and the frame call beyond the common
surface. Regions: your `focus_entry` is called when F6 or a focus command
reaches it. Overlays: the shell pushes the `Overlay` and wraps your view so
clicks do not reach the window; you draw the whole overlay, backdrop
included, and set up its state in `on_intent(Intent::OpenOverlay(…))`.

| Pkg | Directory | Route | Shell calls | Region / overlay | Notes |
| --- | --- | --- | --- | --- | --- |
| P4 | `panes_logic` | `PanesLogic` | `state_glyph_state(m, pane) -> GlyphState`; `Live::activity_map()`, `asleep_ids()` for the snapshot | – | Fill `Live` with activity, insight, plan limits, hibernation, branch cache from `CoreEvent::Bridge`/`BridgeQueue`; bump `live.version` when they change so the snapshot is republished. |
| P5 | `panes` | `Panes` | `view_workspace(st, ctx, ws, hidden)` for every opened workspace; `terminal_access(st) -> Arc<dyn TerminalAccess>` once at boot | region `main` (terminals view) | Terminals and PTY links live in `State`, never in a window. `pty_spawn` takes a `PtySink`: push output into a channel your subscription drains. Hidden workspaces draw nothing but keep consuming output. |
| P6 | `navigator` | `Navigator` | `view(st, ctx)` | region `navigator` | Main window only. Embeds `accounts::badge`. |
| P6 | `toolbar` | `Toolbar` | `view(st, ctx)` | region `toolbar` | Embeds `needs_you::button(&ctx.pkg.needs_you, ctx)`, `accounts::switcher(&ctx.pkg.accounts, ctx)`. |
| P6 | `status_bar` | `StatusBar` | `view(st, ctx)` | region `status` | The broadcast text is the package's own, one per window (`status_bar::State::broadcast_text`); a send is `Intent::Broadcast` with the text. |
| P6 | `needs_you` | `NeedsYou` | `button(st, ctx)` (embedded) | – | Panel open flag is `m.ui.needs_you_open`. |
| P6 | `update_banner` | `UpdateBanner` | `view(st, ctx)`, floating top centre | – | Draw nothing when idle. Updater: `hangar-update`. |
| P6 | `voice` | `Voice` | `hud(st, ctx)`, floating bottom centre | – | Draw nothing when idle. `CoreEvent::Voice*`. |
| P7 | `keymap` | `Keymap` | `Live.index: Arc<KeymapIndex>` read for `Ctx::keymap` | – | Define `KeymapIndex` here. The dispatcher is your own `subscription`; run commands with `Message::Command(id)`. Default bindings live here, not in `CommandId`. |
| P7 | `palette` | `Palette` | `view(st, ctx)` | `Overlay::Palette` | Lists `commands::available()`. |
| P7 | `saved_commands` | `SavedCommands` | `menu_view(st, ctx, pane)` (embedded in the pane header), `dialog_view(st, ctx, ws, pane, edit)` | `Overlay::CommandDialog` | |
| P7 | `session_picker` | `SessionPicker` | `view(st, ctx, ws, pane)` | `Overlay::SessionPicker` | |
| P7 | `pane_menu` | `PaneMenu` | `view(st, ctx, pane)` (embedded in the pane header) | – | Appends `accounts::menu_items`. |
| P8 | `workspace_dialog` | `WorkspaceDialog` | `view(st, ctx)`, `kits_dialog_view(st, ctx, ws)` | `Overlay::WorkspaceDialog`, `Overlay::KitsDialog` | Embeds `accounts::workspace_picker`, `kits::workspace_section`. |
| P8 | `git_flow` | `GitFlow` | – | – | Handles `Intent::CreateWorkspaceFromGit`; expose `git_flow::run(req, deps)` for P18. |
| P8 | `consent_card` | `Consent` | `view(st, ctx)`, floating bottom right | – | Main window only; never takes the focus. |
| P8 | `accounts` | `Accounts` | `switcher`, `pane_badge(ctx, ws, pane)`, `badge(ctx, ws)`, `menu_items(ctx, ws, pane) -> Vec<MenuEntry>`, `workspace_picker(ctx, value)`, `settings_page(st, ctx)` | – | Usage cache moves to `<data>/account-usage.json`. |
| P9 | `inspector` | `Inspector` | `view(st, ctx)` | region `inspector` | Width and fold state are settings. |
| P9 | `git_ui` | `GitUi` | `diff_view(st, ctx, ws, pane)`, `worktrees_view(st, ctx, ws)` | `Overlay::Diff`, `Overlay::Worktrees` | |
| P10 | `memory` | `Memory` | `view(st, ctx)` | region `main` (memory view) | |
| P10 | `mcp` | `Mcp` | `install_view(st, ctx)`, `servers_view(st, ctx, ws)` | `Overlay::McpInstall`, `Overlay::McpServers` | |
| P10 | `orchestration_ui` | `OrchestrationUi` | `panel_view(st, ctx, ws)`, `pane_badge(ctx, ws, pane)` | `Overlay::Orchestration` | Pin/unpin/create/open manager through `on_intent`. |
| P11 | `stats` | `Stats` | `view(st, ctx)` | region `main` (stats view) | Charts on `canvas`. |
| P12 | `board` | `Board` | `view(st, ctx, BoardVariant)`, `task_view(st, ctx, cwd, task)` | regions `main` (board view) and `dock`; `Overlay::Task` | Owns the boards cache in `Live`. |
| P13 | `settings` | `Settings` | `view(st, ctx, SettingsCategory)` | `Overlay::Settings` | Embeds `accounts::settings_page`, `kits::settings_view`. `discord` and `experiments` go in this directory. |
| P14 | `kits` | `Kits` | `activation_view(st, ctx, kit, ws)`, `workspace_section(st, ctx, ws)`, `settings_view(st, ctx)` | `Overlay::KitActivation` | Installed kits in `Live`. |
| P15 | `preview` | `Preview` | `view_panel(st, ctx, PanelMode)`, `view_entry(st, ctx, ws, entry)`; `preview_access(st) -> Arc<dyn PreviewAccess>` once at boot | region `model` | The frame shows the panel (docked, drawer or expanded) from `live.signals.preview[ws]`: keep `entries` and `unseen` there. The shell handles `TogglePreview`, `ClosePreviewPanel` and `TogglePreviewExpand` itself, on that entry's `open`/`expanded`. |
| P16 | `kit_tabs` | `KitTabs` | `view(st, ctx, ws, tab)` (called by `preview::view_entry`) | – | Wave 2. |
| P17 | `model3d_view` | `Model3d` | `view(st, ctx, ws, entry)` (called by `preview::view_entry`) | – | Wave 2. |
| P18 | `control` | `Control` | `Msg::Request(ControlRequest)` (the shell routes core's requests here) | – | `Msg::Model(ModelCmd, Answer<…>)` and `ModelCmd` are yours to shape with P19 and P20; the stub answers every request with an error until you land. Requests before hydration never reach you. |
| P19 | `orchestration` | `Orchestration` | – | – | Start the runtime on hydration (`m.live.hydrated` in `on_event`/`update`), not before. |
| P20 | `mobile` | `Mobile` | – | – | `remote_*` requests come through P18's dispatch. |

## What the shell does (P0b)

The store's semantics (every action of store.tsx, with its tests), hydration
(unique pane names, dangling folder ids, legacy trees) and persistence (the
debounced save, a flush on exit, unknown fields, nulls and unreadable values
written back as they were); boot and reopening detached windows; one view,
overlay stack, drawer and focus per window; detach, reattach, a workspace
window's close and bounds; side panels docked or as drawers (sidePanels.ts),
drawers closing on Escape, a press outside or their toggle; F6 over the
regions shown; Escape (only when nothing captured it) and the focus given back
when an overlay or a drawer closes; the 88 commands; notices; the theme of
each window; activate-pane (`Intent::FocusPane`). What packages fill in, the
frame draws: the drawers' content, the preview panel, the regions' entries.
