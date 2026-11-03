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
//! # For the shell (P0b)
//!
//! Whether to draw the panel: `Live::has_preview(ws)`; whether it is open or
//! expanded: `Live::scope(ws)`. When it is shown on screen, `Live::shared()`
//! `.edit(|s| s.mark_models_seen(ws))` clears the toolbar's dot (this package
//! does it for the panel it believes shown: open, in the terminals view).
//! `Intent::TogglePreview`, `ClosePreviewPanel` and `TogglePreviewExpand` are
//! handled here for a docked panel; drawers (`Model.ui.drawer`) are the
//! shell's, which gets `Msg::Hide` only through the panel's own button.
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
pub mod glyph;
pub mod grip;
pub mod highlight;
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
use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, PreviewAccess, Services};
use crate::shell::{Focus, PanelMode, Region, View};
use crate::state::{Model, Patch, Settings, StoreAction, WsId};

/// How often the file in front is looked at for a newer version
/// (`WATCH_MS`).
const WATCH: Duration = Duration::from_millis(1500);
/// A changed file is read again this long after, to not load it half-written
/// (`SETTLE_MS`).
const SETTLE: Duration = Duration::from_millis(300);
/// How often a spinner turns a step.
const SPIN: Duration = Duration::from_millis(100);

/// A drag of the grip: the width it started from, in points.
#[derive(Debug, Clone, Copy)]
struct Dragging {
    start: f64,
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
    /// The tab in front of each workspace as of the last look.
    fronts: HashMap<WsId, Option<String>>,
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
            fronts: HashMap::new(),
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
    /// The panel's open and expanded state: `None` leaves one as it is.
    SetPanel(WsId, Option<bool>, Option<bool>),
    /// The panel's own button: closes it, or ends the drawer it is.
    Hide(WsId),
    /// A key of the tab strip.
    Tab(TabKey),
    Grip(grip::Event2),
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

/// Gives the store the model's snapshots, which only the shell offers off the
/// UI thread.
fn link(st: &mut State, sv: &Services) {
    if !st.linked {
        st.shared.attach(sv.snapshots());
        st.linked = true;
    }
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    link(st, sv);
    m.live.preview.sync();
    let first = handle(st, m, sv, msg);
    let then = reconcile(st, m, sv);
    Task::batch([first, then])
}

fn handle(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
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
        Msg::SetPanel(ws, open, expanded) => {
            st.shared
                .edit(|store| store.set_model_panel(&ws, open, expanded));
            Task::none()
        }
        Msg::Hide(ws) => {
            // As a drawer, hiding ends the drawer; docked, it closes the
            // panel.
            if m.ui.drawer == Some(crate::shell::Drawer::Model) {
                m.ui.drawer = None;
            } else {
                st.shared
                    .edit(|store| store.set_model_panel(&ws, Some(false), None));
            }
            Task::none()
        }
        Msg::Tab(key) => tab_key(st, m, key),
        Msg::Grip(event) => grip_event(st, m, event),
        Msg::Viewer(id, revision, msg) => viewer_update(st, m, sv, &id, revision, msg),
        Msg::Web(msg) => web_update(st, m, sv, msg),
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

/// The workspaces on screen: the main window's active one, and every one
/// shown in a window of its own.
fn on_screen(m: &Model) -> Vec<WsId> {
    let mut on_screen: Vec<WsId> = m.state().active_workspace_id.iter().cloned().collect();
    on_screen.extend(
        m.state()
            .workspaces
            .iter()
            .filter(|ws| ws.detached == Some(true))
            .map(|ws| ws.id.clone()),
    );
    on_screen.dedup();
    on_screen
}

/// Whether a workspace's panel is on screen with it. The shell decides docked
/// or drawer (`side_panels`, P0b); until it does, an open panel in the
/// terminals view counts as shown.
fn panel_shown(m: &Model, scope: &Scope) -> bool {
    scope.panel_open && !scope.entries.is_empty() && m.ui.view == View::Terminals
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
    let Some(ws) = m.state().active_workspace_id.clone() else {
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

/// The grip moves the width of the panel as it goes: the shell sizes the
/// region from the setting, and the save waits for the model to be still.
fn grip_event(st: &mut State, m: &mut Model, event: grip::Event2) -> Task<Message> {
    let width = m.settings().model_panel_width;
    let body =
        m.ui.focused_window
            .and_then(|window| m.ui.body_width.get(&window))
            .copied()
            .unwrap_or(crate::shell::windows::MAIN_SIZE.width);
    let max = grip::max_width(f64::from(body), f64::from(m.settings().scale_factor()));
    let wanted = match event {
        grip::Event2::Begin => {
            st.grip = Some(Dragging { start: width });
            return Task::none();
        }
        grip::Event2::End => {
            st.grip = None;
            return Task::none();
        }
        grip::Event2::Drag(moved) => {
            let start = st.grip.map_or(width, |held| held.start);
            grip::dragged_width(start, f64::from(moved), max)
        }
        grip::Event2::Reset => grip::default_width(),
        grip::Event2::Nudge(step) => grip::clamp_width(width + f64::from(step), max),
    };
    if (wanted - width).abs() >= 0.5 {
        m.apply(StoreAction::UpdateSettings(Patch::new(
            move |settings: &mut Settings| settings.model_panel_width = wanted,
        )));
    }
    Task::none()
}

/* ── what to do after each change ──────────────────────────────── */

/// Brings everything that depends on the store in line with it: the view's
/// copy, the viewers of the files in front, the helper's windows.
fn reconcile(st: &mut State, m: &mut Model, sv: &Services) -> Task<Message> {
    m.live.preview.sync();
    let mut tasks = Vec::new();

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

    let screens = on_screen(m);
    let active = m.state().active_workspace_id.clone();
    let mut front_files: Vec<(WsId, Entry, bool)> = Vec::new();
    for ws in &screens {
        let Some(scope) = m.live.preview.scope(ws) else {
            continue;
        };
        let shown = panel_shown(m, scope);
        if Some(ws) == active.as_ref() {
            let on = ws.clone();
            st.shared.edit(|store| store.note_view(Some(on), shown));
        }
        if shown && scope.unseen {
            st.shared.edit(|store| store.mark_models_seen(ws));
        }
        if let Some(front) = scope.active()
            && front.is_file()
        {
            front_files.push((ws.clone(), front.clone(), shown));
        }
        // The strip follows what an agent brought forward.
        if st.fronts.get(ws) != Some(&scope.active_id) {
            st.fronts.insert(ws.clone(), scope.active_id.clone());
            tasks.push(strip_to(st, ws));
        }
    }
    if active.is_none() {
        st.shared.edit(|store| store.note_view(None, false));
    }
    m.live.preview.sync();

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
        tasks.push(open_window(st, m, sv, &entry));
    }
    Task::batch(tasks)
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
    m: &mut Model,
    sv: &Services,
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
            Task::none()
        }
        ViewerMsg::ToggleSource | ViewerMsg::Image(image::Msg::ToggleSource) => {
            let Some(viewer) = st.viewers.get_mut(id) else {
                return Task::none();
            };
            viewer.source = !viewer.source;
            // A source is read the first time it is asked for.
            if viewer.source && viewer.load.is_none() {
                viewer.load = Some(Load::Loading);
                return read_text(id, revision, &entry.path);
            }
            Task::none()
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
        ViewerMsg::Window(action) => window_action(st, m, sv, &entry, action),
    }
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
fn open_window(st: &mut State, m: &Model, sv: &Services, entry: &Entry) -> Task<Message> {
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
            let inbox = sv.inbox().clone();
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

fn web_update(st: &mut State, m: &mut Model, sv: &Services, msg: WebMsg) -> Task<Message> {
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
            if let Some(url) = st.windows.on_event(client.as_deref(), &event) {
                // A link the window may not follow opens in the browser; a
                // page cannot make a window of its own.
                if hangar_core::opener::open_url(&url).is_err() {
                    tasks.push(Task::done(Message::Intent(Intent::Notice(
                        crate::shell::Notice::Problem(url),
                    ))));
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
        WebMsg::Gone => {
            st.windows.lost();
            let _ = sv;
            Task::none()
        }
    }
}

fn window_action(
    st: &mut State,
    m: &Model,
    sv: &Services,
    entry: &Entry,
    action: windowed::Action,
) -> Task<Message> {
    let client = st.shared.helper().client();
    match action {
        windowed::Action::Open => {
            st.windows.mark_tried(&entry.id);
            open_window(st, m, sv, entry)
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
        windowed::Action::ToggleSource => {
            let revision = entry.revision;
            let Some(viewer) = st.viewers.get_mut(&entry.id) else {
                return Task::none();
            };
            viewer.source = !viewer.source;
            if viewer.source && viewer.load.is_none() {
                viewer.load = Some(Load::Loading);
                return read_text(&entry.id, revision, &entry.path);
            }
            Task::none()
        }
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
    for ws in on_screen(m) {
        let Some(scope) = m.live.preview.scope(&ws) else {
            continue;
        };
        let Some(front) = scope
            .active()
            .filter(|entry| entry.is_file() || entry.is_model())
        else {
            continue;
        };
        if !panel_shown(m, scope) || !st.watching.insert(front.id.clone()) {
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
        .state()
        .active_workspace_id
        .as_deref()
        .is_some_and(|ws| live.expanded(ws));
    if expanded {
        subscriptions.push(iced::event::listen_with(escape_collapses));
    }
    Subscription::batch(subscriptions)
}

/// What takes the focus when F6 reaches region `Model`, which this package
/// owns: the image in front, or the panel as a whole. Nothing when no panel
/// is shown.
pub fn focus_entry(st: &State, m: &Model, region: Region) -> Option<Task<Message>> {
    let ws = m.state().active_workspace_id.as_deref()?;
    let scope = m
        .live
        .preview
        .scope(ws)
        .filter(|scope| panel_shown(m, scope))?;
    let front = scope.active()?;
    if front.file_kind() == Some(FileKind::Image) {
        return Some(iced::widget::operation::focus(viewer::stage_id(&front.id)));
    }
    Some(Task::none())
}

/// Every intent, after the shell's own handling of it: `Some` when this
/// package acted on it.
pub fn on_intent(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    link(st, sv);
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
        Intent::TogglePreview => {
            let ws = m.state().active_workspace_id.clone()?;
            let scope = st.shared.read(|store| store.scope(&ws).clone());
            let mut tasks = Vec::new();
            if m.ui.view != View::Terminals {
                tasks.push(Task::done(Message::Intent(Intent::SetView(
                    View::Terminals,
                ))));
                st.shared
                    .edit(|store| store.set_model_panel(&ws, Some(true), None));
            } else if scope.expanded {
                st.shared
                    .edit(|store| store.set_model_panel(&ws, Some(false), None));
            } else {
                st.shared
                    .edit(|store| store.set_model_panel(&ws, Some(!scope.panel_open), None));
            }
            m.live.preview.sync();
            Some(Task::batch(tasks))
        }
        Intent::ClosePreviewPanel => {
            let ws = m.state().active_workspace_id.clone()?;
            st.shared
                .edit(|store| store.set_model_panel(&ws, Some(false), None));
            m.live.preview.sync();
            Some(Task::none())
        }
        Intent::TogglePreviewExpand => {
            let ws = m.state().active_workspace_id.clone()?;
            let scope = st.shared.read(|store| store.scope(&ws).clone());
            st.shared
                .edit(|store| store.set_model_panel(&ws, Some(true), Some(!scope.expanded)));
            m.live.preview.sync();
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
