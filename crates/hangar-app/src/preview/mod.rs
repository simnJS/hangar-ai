//! P15: Preview panel and 2D viewers.
//!
//! Ports `ModelPanel.tsx` (the panel: tabs, grip, expand, hide),
//! `components/preview/*` except `KitTabView`, and `lib/{preview,modelStore,
//! modelHost,modelFiles}.ts`. Read inventory-frontend.md §2.4/1.5–1.6, §2.4/3
//! and crates/hangar-app/PACKAGES.md.
//!
//! # What is here
//!
//! - **The store** (`store.rs`): what each workspace has open, which tab is in
//!   front, whether its panel shows. Plain data with the rules of
//!   `modelStore.ts`; `access.rs` shares it under a lock between the UI, the
//!   agents' control operations (`PreviewAccess`, `preview_access`) and the
//!   kit tabs and 3D view of P16 and P17, and keeps the kit tabs in
//!   `<app data>/kit-tabs.json`.
//! - **The panel** (`panel.rs`, `tabs.rs`, `grip.rs`): drawn by `view_panel`.
//! - **The viewers**: images with zoom and pan (`image.rs`, `image_stage.rs`),
//!   text and code, coloured by `two-face`'s grammars (`text.rs`,
//!   `code_lines.rs`, `highlight.rs`), Markdown on iced's `markdown`
//!   (`markdown.rs`).
//! - **The helper** (`webview.rs`, `windowed.rs`): a PDF, an HTML page, a
//!   video or a sound opens in a window of `hangar-webview`, which loads the
//!   file through hangar-core's `model::serve`.
//! - **File watching** (`Msg::Tick`): the file in front is looked at every
//!   1.5 s and read again when it was rewritten.
//!
//! # With the shell (P0b)
//!
//! The shell draws the panel (`view_panel`, in the mode `Panels` gives it:
//! docked, a drawer or expanded) and owns whether it is open or expanded: its
//! toggles write `open` and `expanded` in `live.signals.preview[ws]`, and
//! `TogglePreview`, `ClosePreviewPanel` and `TogglePreviewExpand` are not
//! handled here. This package keeps `entries` and `unseen` there from the
//! store, and the store's `panel_open` and `expanded` agree with `open` and
//! `expanded` (`sync_panels`): what the shell toggles reaches the store, what
//! an agent reveals reaches the shell. The panel's own buttons send the
//! shell's intents. `Intent::ClosePreviews(ws)` closes every tab of a
//! workspace.
//!
//! # With the control bus (P18)
//!
//! Once the state is read this package installs `Hook::Preview` (`hosts.rs`):
//! a `PreviewHost` per workspace over the same store, which lists the tab in
//! front, whether the panel is drawn, a model's findings and a kit tab's
//! params, and opens, reveals, reloads, waits for and closes entries.
//!
//! # For P16, P17, P18
//!
//! `Live::shared()` is the store: `edit` to change it (`open_kit_tab`,
//! `set_kit_tab_badge`, `settle_model`…), `read` to look, `follow` to wait on
//! changes, `register_snapshotter` for the 3D view's off-screen renders,
//! `list_view`/`admit`/`show_request` for the control handlers, and
//! `webview::Helper` (`shared().helper()`) for the one helper process: windows
//! named `preview-…` are this package's, any other prefix can be routed with
//! `Helper::route`.

pub mod access;
pub mod code_lines;
pub mod dev;
pub mod glyph;
pub mod grip;
pub mod highlight;
pub mod hosts;
pub mod image;
pub mod image_stage;
pub mod markdown;
pub mod model;
pub mod panel;
pub mod parts;
pub mod roots;
pub mod store;
pub mod tabs;
pub mod text;
pub mod viewer;
pub mod webview;
pub mod windowed;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use iced::futures::stream::{self, StreamExt};
use iced::keyboard::{self, Key as KeyboardKey, key::Named};
use iced::{Element, Event, Subscription, Task};

use hangar_i18n::{Key, args};

use self::access::Shared;
use self::model::{FileKind, Kind};
use self::parts::Notice;
use self::store::{Body, Entry, Scope, Status};
use self::tabs::TabKey;
use self::text::{Head, Load};
use self::viewer::{Coloured, Loaded, ViewerMsg, ViewerState, language_for};
use self::webview::{HelperHandle, StartError, WebMsg};
use crate::control::{self, Hook};
use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, PreviewAccess, Services};
use crate::shell::{Drawer, Focus, PanelMode, Panels, Region};
use crate::state::{Model, PanelDrag, Patch, PreviewPanel, Settings, StoreAction, WsId};

/// How often the file in front is looked at for a newer version
/// (`WATCH_MS`).
const WATCH: Duration = Duration::from_millis(1500);
/// A changed file is read again this long after, to not load it half-written
/// (`SETTLE_MS`).
const SETTLE: Duration = Duration::from_millis(300);
/// How often a spinner turns a step.
const SPIN: Duration = Duration::from_millis(100);
/// How long after a player loads the engine is asked whether it plays.
const PROBE: Duration = Duration::from_millis(1500);
/// How often the package looks for the state to have been read.
const HYDRATION: Duration = Duration::from_millis(250);

/// A drag of the grip: the width it started from and the one the pointer
/// has reached, in points.
#[derive(Debug, Clone, Copy)]
struct Dragging {
    start: f64,
    to: f64,
}

/// The package's UI state; lives in `App.pkg.preview`.
#[derive(Debug)]
pub struct State {
    shared: Arc<Shared>,
    /// The store has been given the model's snapshots.
    linked: bool,
    /// What each viewer holds of the file in front of its tab, by entry id.
    pub(crate) viewers: HashMap<String, ViewerState>,
    /// The helper's windows, by entry id.
    pub(crate) windows: webview::Windows,
    /// One choice for every text tab, kept while the app runs.
    pub(crate) wrap: bool,
    /// A word after an action, by entry id, with the number that ends it.
    pub(crate) notices: HashMap<String, (u64, Notice)>,
    notice_seq: u64,
    /// Why a file on screen cannot be read any more, by entry id.
    pub(crate) missing: HashMap<String, String>,
    /// Entries a look at the file is under way for.
    watching: HashSet<String>,
    /// A step of the spinners.
    pub(crate) spin: usize,
    /// Numbers the texts read, so that a colouring finds its own.
    generation: u64,
    grip: Option<Dragging>,
    helper_starting: bool,
    /// The workspaces as of the last look, to see one go.
    known: Vec<WsId>,
    /// `HANGAR_DEV_PREVIEW` has been looked at (dev.rs).
    dev_done: bool,
    /// The tab in front of each workspace as of the last look.
    fronts: HashMap<WsId, Option<String>>,
    /// What the shell and the store last agreed on for each workspace's panel.
    agreed: HashMap<WsId, Agreed>,
    /// The hook for the control bus has been sent.
    hosted: bool,
}

/// A workspace's panel as the shell's signals and the store both held it at
/// the last look (`sync_panels`): which of them moved since is what is told
/// to the other.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Agreed {
    open: bool,
    expanded: bool,
    unseen: bool,
}

impl Agreed {
    fn of(scope: &Scope) -> Agreed {
        Agreed {
            open: scope.panel_open,
            expanded: scope.expanded,
            unseen: scope.unseen,
        }
    }
}

impl Default for State {
    fn default() -> Self {
        State {
            shared: Shared::global(),
            linked: false,
            viewers: HashMap::new(),
            windows: webview::Windows::default(),
            wrap: false,
            notices: HashMap::new(),
            notice_seq: 0,
            missing: HashMap::new(),
            watching: HashSet::new(),
            spin: 0,
            generation: 0,
            grip: None,
            helper_starting: false,
            known: Vec::new(),
            dev_done: false,
            fronts: HashMap::new(),
            agreed: HashMap::new(),
            hosted: false,
        }
    }
}

impl std::fmt::Debug for Shared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Shared(..)")
    }
}

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.preview`.
/// The preview of each workspace: entries, the active one, panel open,
/// expanded, unseen.
#[derive(Debug)]
pub struct Live {
    shared: Arc<Shared>,
    scopes: Arc<BTreeMap<WsId, Scope>>,
    /// The store's revision `scopes` is of.
    revision: u64,
}

impl Default for Live {
    fn default() -> Self {
        Live {
            shared: Shared::global(),
            scopes: Arc::default(),
            revision: u64::MAX,
        }
    }
}

impl Live {
    /// Takes the store's content when it changed since the last look: what a
    /// view reads.
    pub fn sync(&mut self) {
        let revision = self.shared.revision();
        if revision != self.revision {
            self.scopes = Arc::new(self.shared.scopes());
            self.revision = revision;
        }
    }

    /// The store, shared with the engines.
    pub fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }

    /// A workspace's preview, if it has anything open.
    pub fn scope(&self, workspace: &str) -> Option<&Scope> {
        self.scopes.get(workspace)
    }

    pub fn has_preview(&self, workspace: &str) -> bool {
        self.scope(workspace).is_some()
    }

    /// Whether any workspace of the window has anything open.
    pub fn any_open(&self) -> bool {
        !self.scopes.is_empty()
    }

    pub fn panel_open(&self, workspace: &str) -> bool {
        self.scope(workspace).is_some_and(|scope| scope.panel_open)
    }

    pub fn expanded(&self, workspace: &str) -> bool {
        self.scope(workspace).is_some_and(|scope| scope.expanded)
    }

    /// An agent opened or changed something here that has not been on screen
    /// since: what the toolbar's button says.
    pub fn unseen(&self, workspace: &str) -> bool {
        self.scope(workspace).is_some_and(|scope| scope.unseen)
    }

    fn entry(&self, id: &str) -> Option<&Entry> {
        self.scopes
            .values()
            .flat_map(|scope| scope.entries.iter())
            .find(|entry| entry.id == id)
    }
}

/// The package's messages, routed by the shell as `Message::Preview(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The store changed.
    Changed,
    /// The file in front is looked at.
    Tick,
    /// A step of the spinners.
    Spin,
    /// A tab was pressed.
    Activate(WsId, String),
    Close(WsId, String),
    /// A key of the tab strip.
    Tab(TabKey),
    Grip(grip::GripEvent),
    Viewer(String, u64, ViewerMsg),
    Web(WebMsg),
    /// What the look at the file in front found.
    Watched(String, u64, Result<model::PreviewFile, String>),
    /// And the second look a moment after a change.
    Settled(
        String,
        u64,
        Box<model::PreviewFile>,
        Result<model::PreviewFile, String>,
    ),
    NoticeGone(String, u64),
}

/// A shorthand for the message a viewer's task ends with.
fn viewer_message(id: &str, revision: u64, msg: ViewerMsg) -> Message {
    Message::Preview(Msg::Viewer(id.to_string(), revision, msg))
}

/// `f` on Tokio's blocking pool.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|err| format!("the task stopped: {err}"))?
}

/// What this package takes of the services: the model's snapshots, which the
/// store reads off the UI thread, and the way back into `update`. Nothing else
/// of them is needed, which is what lets the tests drive the package without
/// a running core.
pub(crate) struct Env {
    snapshots: tokio::sync::watch::Receiver<Arc<crate::state::Snapshot>>,
    inbox: crate::services::Inbox,
}

impl Env {
    fn of(sv: &Services) -> Env {
        Env {
            snapshots: sv.snapshots(),
            inbox: sv.inbox().clone(),
        }
    }
}

/// Gives the store the model's snapshots, which only the shell offers off the
/// UI thread.
fn link(st: &mut State, env: &Env) {
    if !st.linked {
        st.shared.attach(env.snapshots.clone());
        st.linked = true;
    }
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    step(st, m, &Env::of(sv), msg)
}

/// `update`, for whoever has an `Env`.
fn step(st: &mut State, m: &mut Model, env: &Env, msg: Msg) -> Task<Message> {
    link(st, env);
    m.live.preview.sync();
    let first = handle(st, m, env, msg);
    let then = reconcile(st, m, env);
    Task::batch([first, then])
}

fn handle(st: &mut State, m: &mut Model, env: &Env, msg: Msg) -> Task<Message> {
    match msg {
        Msg::Changed => Task::none(),
        Msg::Spin => {
            st.spin = (st.spin + 1) % 8;
            Task::none()
        }
        Msg::Tick => watch(st, m),
        Msg::Activate(ws, id) => {
            st.shared.edit(|store| store.activate_model(&id));
            strip_to(st, &ws)
        }
        Msg::Close(_, id) => {
            st.shared.edit(|store| store.close_model(&id));
            Task::none()
        }
        Msg::Tab(key) => tab_key(st, m, key),
        Msg::Grip(event) => grip_event(st, m, event),
        Msg::Viewer(id, revision, msg) => viewer_update(st, m, env, &id, revision, msg),
        Msg::Web(msg) => web_update(st, m, env, msg),
        Msg::Watched(id, revision, found) => watched(st, m, &id, revision, found),
        Msg::Settled(id, revision, first, again) => {
            st.watching.remove(&id);
            let current = m
                .live
                .preview
                .entry(&id)
                .is_some_and(|entry| entry.revision == revision);
            if let (true, Ok(again)) = (current, again)
                && again.modified_ms == first.modified_ms
                && again.size == first.size
            {
                st.shared
                    .edit(|store| store.reload_model(&id, again.modified_ms, Some(again.size)));
            }
            Task::none()
        }
        Msg::NoticeGone(id, seq) => {
            if st.notices.get(&id).is_some_and(|(held, _)| *held == seq) {
                st.notices.remove(&id);
            }
            Task::none()
        }
    }
}

/// The workspaces the windows have on screen (the main window's active one,
/// and every one in a window of its own), each with whether its preview panel
/// is drawn there: docked, as a drawer that is open, or expanded. The shell
/// decides (`Panels`), from the room, the view and the signals.
fn screens(m: &Model) -> BTreeMap<WsId, bool> {
    let mut screens = BTreeMap::new();
    for window in m.live.windows.keys() {
        let Some(workspace) = m.window_workspace(*window) else {
            continue;
        };
        let shown = Panels::of(m, *window).model.shown;
        *screens.entry(workspace.id.clone()).or_insert(false) |= shown;
    }
    screens
}

/// The window the person is in, the workspace it draws and where its panels
/// stand.
fn focused(m: &Model) -> Option<(WsId, Panels)> {
    let window = m.focused_window()?;
    let workspace = m.window_workspace(window)?;
    Some((workspace.id.clone(), Panels::of(m, window)))
}

/// The shell's signals and the store both hold a workspace's panel: how many
/// entries it has and whether something is news (this package's to write),
/// whether it is open and whether it is expanded (the shell's toggles write
/// them, and an agent revealing an entry does through the store). Each side
/// is told what the other changed since the last look; when both moved, the
/// store's word stands. The shell only ever clears `unseen`.
///
/// The store keeps `expanded` to a panel that is open, as `setModelPanel`
/// did, and no panel exists for a workspace with nothing open.
fn sync_panels(st: &mut State, m: &mut Model) {
    let held: HashMap<WsId, (usize, Agreed)> = st.shared.read(|store| {
        store
            .scopes()
            .iter()
            .map(|(ws, scope)| (ws.clone(), (scope.entries.len(), Agreed::of(scope))))
            .collect()
    });
    let mut workspaces: HashSet<WsId> = held.keys().cloned().collect();
    workspaces.extend(m.live.signals.preview.keys().cloned());
    for ws in workspaces {
        let (mut entries, mut store) = held.get(&ws).copied().unwrap_or_default();
        let shell = m.live.signals.preview.get(&ws).copied().unwrap_or_default();
        let last = st.agreed.get(&ws).copied().unwrap_or_default();
        let pick = |store: bool, shell: bool, last: bool| if store != last { store } else { shell };
        let wanted = Agreed {
            open: pick(store.open, shell.open, last.open),
            expanded: pick(store.expanded, shell.expanded, last.expanded),
            unseen: store.unseen && !(last.unseen && !shell.unseen),
        };
        if wanted != store {
            st.shared.edit(|edit| {
                if wanted.open != store.open || wanted.expanded != store.expanded {
                    edit.set_model_panel(&ws, Some(wanted.open), Some(wanted.expanded));
                }
                if !wanted.unseen {
                    edit.mark_models_seen(&ws);
                }
            });
            (entries, store) = st.shared.read(|read| {
                let scope = read.scope(&ws);
                (scope.entries.len(), Agreed::of(scope))
            });
        }
        if entries == 0 {
            m.live.signals.preview.remove(&ws);
            st.agreed.remove(&ws);
            continue;
        }
        let panel = PreviewPanel {
            entries,
            open: store.open,
            expanded: store.expanded,
            unseen: store.unseen,
        };
        if m.live.signals.preview.get(&ws) != Some(&panel) {
            m.live.signals.preview.insert(ws.clone(), panel);
        }
        st.agreed.insert(ws, store);
    }
}

/// Brings the strip to the tab in front: an agent may bring forward one that
/// sits past the panel's edge.
fn strip_to(st: &State, ws: &str) -> Task<Message> {
    let Some(scope) = st.shared.read(|store| Some(store.scope(ws).clone())) else {
        return Task::none();
    };
    let ids: Vec<String> = scope.entries.iter().map(|entry| entry.id.clone()).collect();
    let share = tabs::scroll_share(&ids, scope.active_id.as_deref());
    iced::widget::operation::snap_to(
        tabs::strip_id(),
        iced::widget::scrollable::RelativeOffset {
            x: Some(share),
            y: None,
        },
    )
}

fn tab_key(st: &mut State, m: &Model, key: TabKey) -> Task<Message> {
    let Some((ws, _)) = focused(m) else {
        return Task::none();
    };
    let scope = st.shared.read(|store| store.scope(&ws).clone());
    let ids: Vec<String> = scope.entries.iter().map(|entry| entry.id.clone()).collect();
    if key == TabKey::CloseActive {
        if let Some(active) = scope.active_id {
            st.shared.edit(|store| store.close_model(&active));
        }
        return Task::none();
    }
    if let Some(at) = tabs::target_of(key, &ids, scope.active_id.as_deref()) {
        let id = ids[at].clone();
        st.shared.edit(|store| store.activate_model(&id));
        return strip_to(st, &ws);
    }
    Task::none()
}

/// While the grip is held, the panel is drawn at the width the pointer has
/// reached (`live.signals.panel_drag`, which `Panels::of` reads) and nothing
/// is written; the setting is saved once, on release, as the TS did, instead
/// of a save and a snapshot for every move. Escape drops the drag and the
/// panel keeps its width. The widest it may be is what the shell leaves it
/// beside the terminals and the other panels (`Slot::max`).
fn grip_event(st: &mut State, m: &mut Model, event: grip::GripEvent) -> Task<Message> {
    let width = m.settings().model_panel_width;
    let Some((_, panels)) = focused(m) else {
        end_grip(st, m);
        return Task::none();
    };
    let max = f64::from(panels.model.max);
    let wanted = match event {
        grip::GripEvent::Begin => {
            let start = grip::clamp_width(width, max);
            st.grip = Some(Dragging { start, to: start });
            return Task::none();
        }
        grip::GripEvent::Drag(moved) => {
            if let Some(held) = st.grip.as_mut() {
                held.to = grip::dragged_width(held.start, f64::from(moved), max);
                let to = held.to;
                m.live.signals.panel_drag = Some(PanelDrag {
                    panel: Drawer::Model,
                    width: to as f32,
                });
            }
            return Task::none();
        }
        grip::GripEvent::End => {
            let held = st.grip;
            end_grip(st, m);
            match held {
                Some(held) => held.to,
                None => return Task::none(),
            }
        }
        grip::GripEvent::Cancel => {
            end_grip(st, m);
            return Task::none();
        }
        grip::GripEvent::Reset => {
            end_grip(st, m);
            grip::default_width()
        }
        grip::GripEvent::Nudge(step) => grip::clamp_width(width + f64::from(step), max),
    };
    if (wanted - width).abs() >= 0.5 {
        m.apply(StoreAction::UpdateSettings(Patch::new(
            move |settings: &mut Settings| settings.model_panel_width = wanted,
        )));
    }
    Task::none()
}

/// The grip is let go of, or dropped: the panel goes back to being drawn at
/// its setting. Another panel's held grip is not the preview's to let go.
fn end_grip(st: &mut State, m: &mut Model) {
    st.grip = None;
    if m.live
        .signals
        .panel_drag
        .is_some_and(|drag| drag.panel == Drawer::Model)
    {
        m.live.signals.panel_drag = None;
    }
}

/* ── what to do after each change ──────────────────────────────── */

/// Brings everything that depends on the store in line with it: the view's
/// copy, the viewers of the files in front, the helper's windows.
fn reconcile(st: &mut State, m: &mut Model, env: &Env) -> Task<Message> {
    m.live.preview.sync();
    let mut tasks = Vec::new();

    // A debug build asked to show files, once the workspace is known.
    if !st.dev_done && m.live.hydrated {
        st.dev_done = true;
        let paths = dev::requested();
        if !paths.is_empty()
            && let Some(ws) = m.state().active_workspace_id.clone()
        {
            tasks.push(dev::show(st.shared.clone(), ws, paths));
        }
    }

    // Kit tabs come back the first time their workspace is opened, and what a
    // deleted workspace left goes with it.
    if m.live.hydrated {
        let opened: Vec<WsId> = m.ui.opened_ids.clone();
        let live: Vec<WsId> = m
            .state()
            .workspaces
            .iter()
            .map(|ws| ws.id.clone())
            .collect();
        st.shared.edit(|store| {
            for id in &opened {
                store.restore_kit_tabs(id);
            }
        });
        if st.known != live {
            st.shared.edit(|store| {
                store.prune_models(&live);
                store.forget_saved_kit_tabs(&live);
            });
            st.known = live;
        }
    }
    m.live.preview.sync();

    // The shell's panel and the store's agree, then the shell says which
    // panels are on screen: what they show has been seen.
    sync_panels(st, m);
    let screens = screens(m);
    st.shared.edit(|store| {
        store.note_screens(screens.iter().map(|(ws, shown)| (ws.clone(), *shown)));
        for (ws, shown) in &screens {
            if *shown {
                store.mark_models_seen(ws);
            }
        }
    });
    sync_panels(st, m);
    m.live.preview.sync();

    let mut front_files: Vec<(WsId, Entry, bool)> = Vec::new();
    for (ws, shown) in &screens {
        let Some(scope) = m.live.preview.scope(ws) else {
            continue;
        };
        if let Some(front) = scope.active()
            && front.is_file()
        {
            front_files.push((ws.clone(), front.clone(), *shown));
        }
        // The strip follows what an agent brought forward.
        if st.fronts.get(ws) != Some(&scope.active_id) {
            st.fronts.insert(ws.clone(), scope.active_id.clone());
            tasks.push(strip_to(st, ws));
        }
    }

    // The agents' operations reach the previews through the control bus.
    if let Some(hook) = hook_to_install(st, m) {
        tasks.push(Task::done(Message::Control(control::Msg::Install(hook))));
    }

    // Viewers: one for the file in front of each workspace on screen, built
    // again when the file was rewritten; gone with their entries.
    let every: HashMap<&str, &Entry> = m
        .live
        .preview
        .scopes
        .values()
        .flat_map(|scope| scope.entries.iter())
        .map(|entry| (entry.id.as_str(), entry))
        .collect();
    st.viewers.retain(|id, _| every.contains_key(id.as_str()));
    st.notices.retain(|id, _| every.contains_key(id.as_str()));
    st.missing.retain(|id, _| every.contains_key(id.as_str()));
    st.watching.retain(|id| every.contains_key(id.as_str()));
    for (_, entry, _) in &front_files {
        let Body::File { kind, .. } = &entry.body else {
            continue;
        };
        let current = st
            .viewers
            .get(&entry.id)
            .is_some_and(|viewer| viewer.revision == entry.revision);
        if !current {
            tasks.push(mount(st, entry, *kind));
        }
    }

    // The helper's windows: the ones whose file went are closed, the ones
    // whose file was rewritten load it again, and the file in front of a
    // panel on screen opens its own once.
    let helper_entries: HashMap<&str, &Entry> = every
        .iter()
        .filter(|(_, entry)| {
            entry
                .file_kind()
                .is_some_and(|kind| kind.kind().viewer().in_helper())
        })
        .map(|(id, entry)| (*id, *entry))
        .collect();
    let client = st.shared.helper().client();
    st.windows.reconcile(client.as_deref(), &helper_entries);
    let opening: Vec<Entry> = front_files
        .iter()
        .filter(|(_, entry, shown)| {
            *shown
                && entry
                    .file_kind()
                    .is_some_and(|kind| kind.kind().viewer().in_helper())
                && !st.windows.was_tried(&entry.id)
                && st.windows.get(&entry.id).is_none()
        })
        .map(|(_, entry, _)| entry.clone())
        .collect();
    for entry in opening {
        st.windows.mark_tried(&entry.id);
        tasks.push(open_window(st, m, env, &entry));
    }
    Task::batch(tasks)
}

/// The previews' part of the control bus (`Hook::Preview`), once the state is
/// read and only once: until then the bus answers the agents itself.
fn hook_to_install(st: &mut State, m: &Model) -> Option<Hook> {
    if !m.live.hydrated || st.hosted {
        return None;
    }
    st.hosted = true;
    let hosts: Arc<dyn control::port::PreviewHosts> =
        Arc::new(hosts::Hosts::new(st.shared.clone()));
    Some(Hook::Preview(hosts))
}

/// A viewer for the file in front, reading what it shows.
fn mount(st: &mut State, entry: &Entry, kind: FileKind) -> Task<Message> {
    let mut viewer = ViewerState::new(entry, kind);
    let task = match kind {
        FileKind::Text | FileKind::Markdown => {
            viewer.load = Some(Load::Loading);
            read_text(&entry.id, entry.revision, &entry.path)
        }
        FileKind::Image => read_image(&entry.id, entry.revision, &entry.path),
        FileKind::Html | FileKind::Pdf | FileKind::Video | FileKind::Audio => Task::none(),
    };
    st.viewers.insert(entry.id.clone(), viewer);
    task
}

fn read_text(id: &str, revision: u64, path: &str) -> Task<Message> {
    let (id, path) = (id.to_string(), path.to_string());
    Task::perform(
        blocking(move || text::read_head(Path::new(&path))),
        move |read| {
            viewer_message(
                &id,
                revision,
                ViewerMsg::Text(read.map(|head: Head| Loaded(Arc::new(head)))),
            )
        },
    )
}

fn read_image(id: &str, revision: u64, path: &str) -> Task<Message> {
    let (id, path) = (id.to_string(), path.to_string());
    Task::perform(
        blocking(move || image::read(Path::new(&path))),
        move |read| viewer_message(&id, revision, ViewerMsg::Bytes(read)),
    )
}

/// Colours a text off the UI thread: the grammars take a moment to load and
/// a long file longer.
fn colour(
    id: &str,
    revision: u64,
    generation: u64,
    text: String,
    path: String,
    language: Option<&'static str>,
) -> Task<Message> {
    let id = id.to_string();
    Task::perform(
        blocking(move || Ok(highlight::highlight_file(&text, &path, language))),
        move |lines| {
            let lines = lines.ok().flatten().map(|lines| Coloured(Arc::new(lines)));
            viewer_message(&id, revision, ViewerMsg::Coloured(generation, lines))
        },
    )
}

/// A word after an action, gone after a moment.
fn notify(st: &mut State, id: &str, notice: Notice) -> Task<Message> {
    st.notice_seq += 1;
    let seq = st.notice_seq;
    let lasts = notice.lasts();
    st.notices.insert(id.to_string(), (seq, notice));
    let id = id.to_string();
    Task::perform(tokio::time::sleep(lasts), move |()| {
        Message::Preview(Msg::NoticeGone(id.clone(), seq))
    })
}

fn translate(m: &Model, key: Key) -> String {
    hangar_i18n::t(m.locale(), key).into_owned()
}

fn viewer_update(
    st: &mut State,
    m: &Model,
    env: &Env,
    id: &str,
    revision: u64,
    msg: ViewerMsg,
) -> Task<Message> {
    let Some(entry) = m.live.preview.entry(id).cloned() else {
        return Task::none();
    };
    if !st
        .viewers
        .get(id)
        .is_some_and(|viewer| viewer.revision == revision)
    {
        return Task::none();
    }
    let locale = m.locale();
    match msg {
        ViewerMsg::Text(Err(why)) => {
            if let Some(viewer) = st.viewers.get_mut(id) {
                viewer.load = Some(Load::Failed(why));
            }
            Task::none()
        }
        ViewerMsg::Text(Ok(Loaded(head))) => {
            st.generation += 1;
            let generation = st.generation;
            let Some(viewer) = st.viewers.get_mut(id) else {
                return Task::none();
            };
            let kind = viewer.kind;
            let language = language_for(&entry, kind);
            let ready = text::Ready::of(
                &head,
                kind == FileKind::Text && language == Some("json"),
                generation,
            );
            if kind == FileKind::Markdown {
                viewer.rendered = Some(markdown::Rendered::parse(&ready.text, &entry.path));
            }
            viewer.generation = generation;
            let (body, path) = (ready.text.clone(), entry.path.clone());
            viewer.load = Some(Load::Ready(Box::new(ready)));
            // A page's source is HTML to colour; its path says `.html`.
            colour(id, revision, generation, body, path, language)
        }
        ViewerMsg::Coloured(generation, lines) => {
            if let Some(viewer) = st.viewers.get_mut(id)
                && let Some(Load::Ready(ready)) = &mut viewer.load
                && ready.generation == generation
                && let Some(Coloured(lines)) = lines
            {
                ready.lines = lines;
            }
            Task::none()
        }
        ViewerMsg::Retry => {
            if let Some(viewer) = st.viewers.get_mut(id) {
                viewer.load = Some(Load::Loading);
            }
            read_text(id, revision, &entry.path)
        }
        ViewerMsg::Bytes(Ok(bytes)) => {
            if let Some(viewer) = st.viewers.get_mut(id) {
                viewer.picture.pic = Some(image::pic_of(&entry.path, &bytes));
                viewer.picture.failed = None;
            }
            Task::none()
        }
        ViewerMsg::Bytes(Err(why)) => {
            if let Some(viewer) = st.viewers.get_mut(id) {
                viewer.picture.failed = Some(why);
            }
            Task::none()
        }
        ViewerMsg::ToggleWrap => {
            st.wrap = !st.wrap;
            // The lines are laid out anew: from the top, as the TS's were.
            if let Some(viewer) = st.viewers.get_mut(id) {
                viewer.scroll = code_lines::Scroll::default();
            }
            to_top(id)
        }
        ViewerMsg::CodeScrolled(scroll) => {
            if let Some(viewer) = st.viewers.get_mut(id) {
                viewer.scroll = scroll;
            }
            Task::none()
        }
        ViewerMsg::ToggleSource | ViewerMsg::Image(image::Msg::ToggleSource) => {
            let Some(viewer) = st.viewers.get_mut(id) else {
                return Task::none();
            };
            viewer.source = !viewer.source;
            viewer.scroll = code_lines::Scroll::default();
            // A source is read the first time it is asked for.
            if viewer.source && viewer.load.is_none() {
                viewer.load = Some(Load::Loading);
                return Task::batch([read_text(id, revision, &entry.path), to_top(id)]);
            }
            to_top(id)
        }
        ViewerMsg::Copy => {
            let Some(text) = st
                .viewers
                .get(id)
                .and_then(ViewerState::text)
                .map(|ready| ready.text.clone())
            else {
                return Task::none();
            };
            let done = notify(
                st,
                id,
                Notice::info(hangar_i18n::t(locale, Key::PreviewCopied)),
            );
            Task::batch([iced::clipboard::write(text), done])
        }
        ViewerMsg::Image(msg) => {
            if let Some(viewer) = st.viewers.get_mut(id) {
                viewer.picture.update(msg);
            }
            Task::none()
        }
        ViewerMsg::Markdown(msg) => markdown_update(st, id, revision, msg, locale),
        ViewerMsg::Window(action) => window_action(st, m, env, &entry, action),
    }
}

/// Brings the text of an entry back to its top: the lines and the rendered
/// page are scrollables of their own, which keep their place across a change
/// of what they hold.
fn to_top(id: &str) -> Task<Message> {
    let top = iced::widget::scrollable::RelativeOffset {
        x: Some(0.0),
        y: Some(0.0),
    };
    Task::batch([
        iced::widget::operation::snap_to(viewer::code_id(id), top),
        iced::widget::operation::snap_to(viewer::scroll_id(id), top),
    ])
}

fn markdown_update(
    st: &mut State,
    id: &str,
    revision: u64,
    msg: markdown::Msg,
    locale: hangar_i18n::Locale,
) -> Task<Message> {
    let scroll = viewer::scroll_id(id);
    let to_message = {
        let id = id.to_string();
        move |msg: markdown::Msg| viewer_message(&id, revision, ViewerMsg::Markdown(msg))
    };
    let Some(rendered) = st.viewers.get_mut(id).and_then(|v| v.rendered.as_mut()) else {
        return Task::none();
    };
    match msg {
        markdown::Msg::Scrolled(offset) => {
            rendered.scrolled(offset);
            Task::none()
        }
        markdown::Msg::Found(y) => markdown::scroll_by(scroll, y).map(to_message),
        markdown::Msg::Link(href) => {
            if let Some(anchor) = markdown::Rendered::anchor(&href) {
                return markdown::scroll_to_heading(scroll, &anchor).map(to_message);
            }
            match rendered.follow(&href) {
                markdown::Follow::Nothing => Task::none(),
                markdown::Follow::Open(url) => match hangar_core::opener::open_url(&url) {
                    Ok(()) => Task::none(),
                    Err(error) => {
                        let words = hangar_i18n::t_with(
                            locale,
                            Key::PreviewOpenFailed,
                            &args!("error" => error),
                        )
                        .into_owned();
                        notify(st, id, Notice::error(words))
                    }
                },
                markdown::Follow::Inert(href) => {
                    let words =
                        hangar_i18n::t_with(locale, Key::PreviewLinkInert, &args!("href" => href))
                            .into_owned();
                    notify(st, id, Notice::error(words))
                }
            }
        }
    }
}

/* ── the helper's windows ──────────────────────────────────────── */

fn kind_of(entry: &Entry) -> Kind {
    entry.file_kind().map_or(Kind::Html, FileKind::kind)
}

/// Opens (or opens again) an entry's window, starting the helper if it is not
/// running.
fn open_window(st: &mut State, m: &Model, env: &Env, entry: &Entry) -> Task<Message> {
    let theme = m.theme();
    let helper = st.shared.helper().clone();
    match helper.client() {
        Some(client) => {
            st.windows.open(Some(&client), entry, kind_of(entry), theme);
            Task::none()
        }
        None => {
            st.windows.open(None, entry, kind_of(entry), theme);
            if st.helper_starting {
                return Task::none();
            }
            st.helper_starting = true;
            let inbox = env.inbox.clone();
            Task::perform(
                async move {
                    match tokio::task::spawn_blocking(move || helper.start(inbox)).await {
                        Ok(started) => started,
                        Err(why) => Err(StartError::Failed(why.to_string())),
                    }
                },
                |started| Message::Preview(Msg::Web(WebMsg::Started(started.map(HelperHandle)))),
            )
        }
    }
}

fn web_update(st: &mut State, m: &mut Model, env: &Env, msg: WebMsg) -> Task<Message> {
    let theme = m.theme();
    match msg {
        WebMsg::Started(Ok(HelperHandle(client))) => {
            st.helper_starting = false;
            let entries: HashMap<&str, &Entry> = m
                .live
                .preview
                .scopes
                .values()
                .flat_map(|scope| scope.entries.iter())
                .map(|entry| (entry.id.as_str(), entry))
                .collect();
            st.windows.flush(&client, &entries, theme);
            Task::none()
        }
        WebMsg::Started(Err(error)) => {
            st.helper_starting = false;
            st.windows.fail_waiting(&error);
            Task::none()
        }
        WebMsg::Event(event) => {
            let client = st.shared.helper().client();
            let mut tasks = Vec::new();
            let handled = st.windows.on_event(client.as_deref(), &event);
            if let Some(url) = handled.link {
                // A link the window may not follow opens in the browser; a
                // page cannot make a window of its own.
                if hangar_core::opener::open_url(&url).is_err() {
                    tasks.push(Task::done(Message::Intent(Intent::Notice(
                        crate::shell::Notice::Problem(url),
                    ))));
                }
            }
            if let Some(entry) = handled.probe {
                // Asked twice: a codec the engine lacks says so a moment after
                // the load, a slow file a little later.
                for after in [PROBE, PROBE * 3] {
                    let entry = entry.clone();
                    tasks.push(Task::perform(tokio::time::sleep(after), move |()| {
                        Message::Preview(Msg::Web(WebMsg::Probe(entry.clone())))
                    }));
                }
            }
            if let hangar_webview::Event::Error {
                window: None,
                message,
                ..
            } = &event
            {
                tasks.push(Task::done(Message::Intent(Intent::Notice(
                    crate::shell::Notice::Problem(message.clone()),
                ))));
            }
            Task::batch(tasks)
        }
        WebMsg::Probe(entry) => {
            if let Some(client) = st.shared.helper().client() {
                st.windows.probe_media(&client, &entry);
            }
            Task::none()
        }
        WebMsg::Gone => {
            st.windows.lost();
            let _ = env;
            Task::none()
        }
    }
}

fn window_action(
    st: &mut State,
    m: &Model,
    env: &Env,
    entry: &Entry,
    action: windowed::Action,
) -> Task<Message> {
    let client = st.shared.helper().client();
    match action {
        windowed::Action::Open => {
            st.windows.mark_tried(&entry.id);
            open_window(st, m, env, entry)
        }
        windowed::Action::Focus => {
            if let Some(client) = client {
                st.windows.focus(&client, &entry.id);
            }
            Task::none()
        }
        windowed::Action::Reload => {
            if let Some(client) = client {
                st.windows.reload(&client, entry);
            }
            Task::none()
        }
        windowed::Action::Loop(on) => {
            st.windows
                .set_player(client.as_deref(), &entry.id, Some(on), None);
            Task::none()
        }
        windowed::Action::Speed(speed) => {
            st.windows
                .set_player(client.as_deref(), &entry.id, None, Some(speed));
            Task::none()
        }
        windowed::Action::ToggleSource => viewer_update(
            st,
            m,
            env,
            &entry.id,
            entry.revision,
            ViewerMsg::ToggleSource,
        ),
        windowed::Action::OpenExternally => {
            let words = match hangar_core::opener::open_path(&entry.path) {
                Ok(()) => Notice::info(translate(m, Key::PreviewOpened)),
                // Refused anyway, it shows the file in its folder instead,
                // and says which it did.
                Err(_) => match hangar_core::opener::reveal_item_in_dir(&entry.path) {
                    Ok(()) => Notice::info(translate(m, Key::PreviewRevealed)),
                    Err(error) => Notice::error(
                        hangar_i18n::t_with(
                            m.locale(),
                            Key::PreviewOpenFailed,
                            &args!("error" => error),
                        )
                        .into_owned(),
                    ),
                },
            };
            notify(st, &entry.id, words)
        }
        windowed::Action::ShowInFolder => {
            let words = match hangar_core::opener::reveal_item_in_dir(&entry.path) {
                Ok(()) => return Task::none(),
                Err(error) => Notice::error(
                    hangar_i18n::t_with(
                        m.locale(),
                        Key::PreviewOpenFailed,
                        &args!("error" => error),
                    )
                    .into_owned(),
                ),
            };
            notify(st, &entry.id, words)
        }
    }
}

/* ── the file watch ────────────────────────────────────────────── */

async fn inspect(path: String) -> Result<model::PreviewFile, String> {
    let found = hangar_core::preview::preview_inspect(path).await?;
    model::from_core(&found)
}

/// Looks at the file in front of each panel on screen, as the 3D view
/// watches its model: a newer modification time, steady for a moment, has the
/// viewer read it again. A file that disappears or is now refused keeps its
/// last version on screen, and the reason is said.
fn watch(st: &mut State, m: &Model) -> Task<Message> {
    let mut tasks = Vec::new();
    for (ws, shown) in screens(m) {
        let Some(scope) = m.live.preview.scope(&ws).filter(|_| shown) else {
            continue;
        };
        let Some(front) = scope
            .active()
            .filter(|entry| entry.is_file() || entry.is_model())
        else {
            continue;
        };
        if !st.watching.insert(front.id.clone()) {
            continue;
        }
        let (id, revision, path) = (front.id.clone(), front.revision, front.path.clone());
        tasks.push(Task::perform(inspect(path), move |found| {
            Message::Preview(Msg::Watched(id.clone(), revision, found))
        }));
    }
    Task::batch(tasks)
}

fn watched(
    st: &mut State,
    m: &Model,
    id: &str,
    revision: u64,
    found: Result<model::PreviewFile, String>,
) -> Task<Message> {
    let Some(entry) = m
        .live
        .preview
        .entry(id)
        .filter(|entry| entry.revision == revision)
    else {
        st.watching.remove(id);
        return Task::none();
    };
    match found {
        Err(why) => {
            st.watching.remove(id);
            st.missing.insert(id.to_string(), why);
            Task::none()
        }
        Ok(now) => {
            st.missing.remove(id);
            if now.modified_ms <= entry.modified_ms {
                st.watching.remove(id);
                return Task::none();
            }
            // Once more a moment later: an agent writing the file in several
            // goes would otherwise have its first half read.
            let (id, path) = (id.to_string(), entry.path.clone());
            Task::perform(
                async move {
                    tokio::time::sleep(SETTLE).await;
                    let again = inspect(path).await;
                    (id, now, again)
                },
                move |(id, now, again)| {
                    Message::Preview(Msg::Settled(id, revision, Box::new(now), again))
                },
            )
        }
    }
}

/* ── subscriptions ─────────────────────────────────────────────── */

/// Follows the store's changes, once at start for the first look.
#[derive(Clone)]
struct Follow(Arc<Shared>);

impl std::hash::Hash for Follow {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (Arc::as_ptr(&self.0) as usize).hash(state);
    }
}

fn changes(shared: &Arc<Shared>) -> Subscription<Message> {
    Subscription::run_with(Follow(shared.clone()), |follow| {
        let receiver = follow.0.follow();
        let first = stream::once(std::future::ready(Message::Preview(Msg::Changed)));
        let rest = stream::unfold(receiver, |mut receiver| async move {
            receiver.changed().await.ok()?;
            Some((Message::Preview(Msg::Changed), receiver))
        });
        first.chain(rest)
    })
}

/// The keys the tab strip answers while the panel has the keyboard.
fn tab_keys(
    event: Event,
    status: iced::event::Status,
    _window: iced::window::Id,
) -> Option<Message> {
    if status == iced::event::Status::Captured {
        return None;
    }
    let Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event else {
        return None;
    };
    if modifiers.control() || modifiers.alt() || modifiers.command() {
        return None;
    }
    let key = match key {
        KeyboardKey::Named(Named::ArrowRight) => TabKey::Next,
        KeyboardKey::Named(Named::ArrowLeft) => TabKey::Previous,
        KeyboardKey::Named(Named::Home) => TabKey::First,
        KeyboardKey::Named(Named::End) => TabKey::Last,
        KeyboardKey::Named(Named::Delete) => TabKey::CloseActive,
        _ => return None,
    };
    Some(Message::Preview(Msg::Tab(key)))
}

/// Expanded, Escape puts the panel back beside the terminals, like leaving a
/// full-screen view; anything inside that handles Escape itself keeps it.
fn escape_collapses(
    event: Event,
    status: iced::event::Status,
    _window: iced::window::Id,
) -> Option<Message> {
    if status == iced::event::Status::Captured {
        return None;
    }
    match event {
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: KeyboardKey::Named(Named::Escape),
            ..
        }) => Some(Message::Intent(Intent::TogglePreviewExpand)),
        _ => None,
    }
}

pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    let mut subscriptions = vec![changes(&st.shared)];
    let live = &m.live.preview;
    if !m.live.hydrated || !st.dev_done {
        // Nothing tells this package that the state has been read, and the
        // kit tabs of the workspace on screen come back with it: it looks
        // until it has.
        subscriptions.push(iced::time::every(HYDRATION).map(|_| Message::Preview(Msg::Changed)));
    }
    if live.any_open() {
        subscriptions.push(iced::time::every(WATCH).map(|_| Message::Preview(Msg::Tick)));
    }
    let loading = live.scopes.values().any(|scope| {
        scope.entries.iter().any(|entry| {
            entry.status == Status::Loading
                || entry
                    .kit()
                    .and_then(|kit| kit.badge.as_ref())
                    .is_some_and(|badge| badge.state == store::KitBadgeState::Running)
        })
    });
    if loading {
        subscriptions.push(iced::time::every(SPIN).map(|_| Message::Preview(Msg::Spin)));
    }
    if m.ui.focus == Some(Focus::Region(Region::Model)) {
        subscriptions.push(iced::event::listen_with(tab_keys));
    }
    let expanded = m
        .focused_window()
        .is_some_and(|window| Panels::of(m, window).model_expanded);
    if expanded {
        subscriptions.push(iced::event::listen_with(escape_collapses));
    }
    Subscription::batch(subscriptions)
}

/// What takes the focus when F6 reaches region `Model`, which this package
/// owns: the image in front, or the panel as a whole. Nothing when no panel
/// is shown.
pub fn focus_entry(st: &State, m: &Model, region: Region) -> Option<Task<Message>> {
    let window = m.focused_window()?;
    if !Panels::of(m, window).model.shown {
        return None;
    }
    let ws = m.window_workspace(window)?;
    let front = m.live.preview.scope(&ws.id)?.active()?;
    if front.file_kind() == Some(FileKind::Image) {
        return Some(iced::widget::operation::focus(viewer::stage_id(&front.id)));
    }
    Some(Task::none())
}

/// Every intent, after the shell's own handling of it: `Some` when this
/// package acted on it. The shell's toggles (`TogglePreview`,
/// `ClosePreviewPanel`, `TogglePreviewExpand`) are not handled here: they
/// have just written `open` and `expanded` in the signals, which the store
/// is told of before anything else looks at it.
pub fn on_intent(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    heard(st, m, &Env::of(sv), intent)
}

/// `on_intent`, for whoever has an `Env`.
fn heard(st: &mut State, m: &mut Model, env: &Env, intent: &Intent) -> Option<Task<Message>> {
    link(st, env);
    sync_panels(st, m);
    match intent {
        Intent::ShowPreview(request) => {
            let shared = st.shared.clone();
            let request = request.clone();
            Some(Task::perform(
                async move { shared.show_request(request).await },
                |shown| match shown {
                    Ok(_) => Message::Noop,
                    Err(why) => Message::Intent(Intent::Notice(crate::shell::Notice::Problem(why))),
                },
            ))
        }
        Intent::ActivateWorkspace(ws) => {
            if m.live.hydrated {
                st.shared.edit(|store| store.restore_kit_tabs(ws));
            }
            None
        }
        // `view.closeModels`: the workspace's preview goes away. The shell
        // is told at once, so that it stops drawing a panel with nothing in
        // it; the viewers and the helper's windows follow the store's change.
        Intent::ClosePreviews(ws) => {
            st.shared.edit(|store| store.close_workspace_models(ws));
            sync_panels(st, m);
            Some(Task::none())
        }
        _ => None,
    }
}

/// Every core event, after the shell's own handling of it.
pub fn on_event(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    event: &CoreEvent,
) -> Option<Task<Message>> {
    None
}

/// Region `model`: the preview panel, docked, as a drawer, or expanded.
pub fn view_panel<'a>(st: &'a State, ctx: Ctx<'a>, mode: PanelMode) -> Element<'a, Message> {
    panel::view(st, ctx, mode)
}

/// The body of the panel for one entry: a file viewer, or a slot for
/// `model3d_view::view` and `kit_tabs::view`.
pub fn view_entry<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    ws: &'a WsId,
    entry: &'a str,
) -> Element<'a, Message> {
    match ctx.model.live.preview.entry(entry) {
        Some(found) => panel::entry_body(st, ctx, ws, found),
        None => parts::loading(ctx, entry, None),
    }
}

/// What the engines may do with the previews (`Services::preview`). The shell
/// asks once, at boot, after `State::default()`.
pub fn preview_access(st: &State) -> Arc<dyn PreviewAccess> {
    st.shared.clone()
}
