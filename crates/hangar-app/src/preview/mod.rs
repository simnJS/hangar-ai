//! P15: the previews and the 2D viewers.
//!
//! Ports `components/preview/*` except `KitTabView`, and
//! `lib/{preview,modelStore,modelHost,modelFiles}.ts`. Read
//! inventory-frontend.md §2.4/1.5–1.6, §2.4/3 and crates/hangar-app/PACKAGES.md.
//! 1.x drew the previews in a panel beside the terminals, with a tab per
//! entry (`ModelPanel.tsx`); here every entry is a pane of its workspace's
//! grid, a view pane beside the terminals (`panes.rs`), and there is no panel.
//!
//! # What is here
//!
//! - **The store** (`store.rs`): what each workspace has open. Plain data
//!   with the rules of `modelStore.ts`; `access.rs` shares it under a lock
//!   between the UI, the agents' control operations (`PreviewAccess`,
//!   `preview_access`) and the kit tabs and 3D view of P16 and P17, and keeps
//!   the kit tabs in `<app data>/kit-tabs.json`.
//! - **The panes** (`panes.rs`, `head.rs`): every entry a pane of the grid,
//!   which the panes package draws with what `head.rs` says of the entry and
//!   the body `view_entry` draws.
//! - **The viewers**: images with zoom and pan (`image.rs`, `image_stage.rs`),
//!   text and code, coloured by `two-face`'s grammars (`text.rs`,
//!   `code_lines.rs`, `highlight.rs`), Markdown on iced's `markdown`
//!   (`markdown.rs`).
//! - **The helper** (`webview.rs`, `windowed.rs`): a PDF, an HTML page, a
//!   video or a sound opens in a window of `hangar-webview`, which loads the
//!   file through hangar-core's `model::serve`.
//! - **File watching** (`Msg::Tick`): the file of each view pane on screen is
//!   looked at every 1.5 s and read again when it was rewritten.
//!
//! # With the shell and the panes (P0b, P5)
//!
//! After every change of the store the panes follow it (`panes::sync`): an
//! entry opened is a view pane of the grid, beside the focused pane, and one
//! closed takes its pane with it. The panes package draws the view pane, its
//! header and its moves; its close is `Intent::CloseView`, handled here.
//! `Intent::ClosePreviews(ws)` closes every entry of a workspace, and so its
//! view panes.
//!
//! # With the control bus (P18)
//!
//! Once the state is read this package installs `Hook::Preview` (`hosts.rs`):
//! a `PreviewHost` per workspace over the same store, which lists the entry
//! last brought forward, whether the grid is drawn, a model's findings and a
//! kit tab's params, and opens, reveals, reloads, waits for and closes
//! entries. What the agents open and close is laid out again by the control
//! bus (`ModelCmd::ViewsChanged`).
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
pub mod animated;
pub mod avif;
pub mod code_lines;
pub mod decode;
pub mod dev;
pub mod glyph;
pub mod head;
pub mod highlight;
pub mod hosts;
pub mod image;
pub mod image_stage;
pub mod markdown;
pub mod model;
pub mod panes;
pub mod parts;
pub mod powershell;
pub mod roots;
pub mod store;
pub mod text;
pub mod travelling;
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
use iced::{Element, Subscription, Task};

use hangar_i18n::{Key, args};

use self::access::Shared;
use self::model::{FileKind, Kind};
use self::parts::Notice;
use self::store::{Body, Entry, Scope, Status};
use self::text::{Head, Load};
use self::viewer::{CodeColours, Coloured, Loaded, ViewerMsg, ViewerState, language_for};
use self::webview::{HelperHandle, StartError, WebMsg};
use crate::control::{self, Hook};
use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, PreviewAccess, Services};
use crate::shell::Region;
use crate::state::{Model, WsId};

/// How often the file in front is looked at for a newer version
/// (`WATCH_MS`).
const WATCH: Duration = Duration::from_millis(1500);
/// A changed file is read again this long after, to not load it half-written
/// (`SETTLE_MS`).
const SETTLE: Duration = Duration::from_millis(300);
/// How often a spinner turns a step.
const SPIN: Duration = Duration::from_millis(100);
/// How often the package looks for the state to have been read.
const HYDRATION: Duration = Duration::from_millis(250);

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
    helper_starting: bool,
    /// The workspaces as of the last look, to see one go.
    known: Vec<WsId>,
    /// `HANGAR_DEV_PREVIEW` has been looked at (dev.rs).
    dev_done: bool,
    /// The hook for the control bus has been sent.
    hosted: bool,
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
            helper_starting: false,
            known: Vec::new(),
            dev_done: false,
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
/// `Model.live.preview`: the entries of each workspace, which its view panes
/// draw.
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
    /// The person opened entry `id` of workspace `ws`: its pane takes the
    /// focus, once it is in the grid.
    Opened(WsId, String),
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
        Msg::Opened(ws, id) => {
            m.live.preview.sync();
            panes::sync(m);
            Task::done(Message::Intent(Intent::FocusPane {
                ws,
                pane: id,
                terminal: true,
            }))
        }
    }
}

/// The workspaces the windows have on screen (the main window's active one,
/// and every one in a window of its own), each with whether its grid is drawn
/// there: the terminals view, where the view panes are, rather than the
/// board, the memory or the stats.
fn screens(m: &Model) -> BTreeMap<WsId, bool> {
    let mut screens = BTreeMap::new();
    for window in m.live.windows.keys() {
        let Some(workspace) = m.window_workspace(*window) else {
            continue;
        };
        let shown = m.live.hydrated && m.view_in(*window) == crate::shell::View::Terminals;
        *screens.entry(workspace.id.clone()).or_insert(false) |= shown;
    }
    screens
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

    // Every entry is a pane of its workspace's grid: the panes follow what
    // opened and closed (panes.rs).
    let synced = panes::sync(m);
    if let Some(words) = synced.notice(m) {
        tasks.push(Task::done(Message::Intent(Intent::Notice(
            crate::shell::Notice::Problem(words),
        ))));
    }

    // The store hears which grids are on screen and which panes in them.
    let screens = screens(m);
    let grids: std::collections::BTreeSet<WsId> = screens
        .iter()
        .filter(|(_, shown)| **shown)
        .map(|(ws, _)| ws.clone())
        .collect();
    let shown = panes::shown(m, &grids);
    st.shared.edit(|store| {
        store.note_screens(screens.iter().map(|(ws, shown)| (ws.clone(), *shown)));
        store.note_shown(shown.iter().cloned());
    });
    m.live.preview.sync();

    // The files of the workspaces on screen, each with whether its pane is.
    let mut front_files: Vec<(WsId, Entry, bool)> = Vec::new();
    for ws in screens.keys() {
        let Some(scope) = m.live.preview.scope(ws) else {
            continue;
        };
        for entry in scope.entries.iter().filter(|entry| entry.is_file()) {
            front_files.push((ws.clone(), entry.clone(), shown.contains(&entry.id)));
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
    viewer::release_hidden(&mut st.viewers, &shown, std::time::Instant::now());
    for (_, entry, seen) in &front_files {
        let Body::File { kind, .. } = &entry.body else {
            continue;
        };
        // A viewer reads its file once its pane is on screen, and keeps it
        // until it has been out of sight for a while (`release_hidden`).
        if !seen && !st.viewers.contains_key(&entry.id) {
            continue;
        }
        let current = st
            .viewers
            .get(&entry.id)
            .is_some_and(|viewer| viewer.revision == entry.revision);
        if !current {
            tasks.push(mount(st, entry, *kind));
        }
    }

    // The helper's windows: the ones whose file went are closed, the ones
    // whose file was rewritten load it again, each is on screen only while
    // its entry's pane is on screen (in an awake workspace's grid, drawn),
    // and the file of such a pane opens its own once
    // (webview.rs, `Windows`).
    let in_helper = |entry: &Entry| {
        entry
            .file_kind()
            .is_some_and(|kind| kind.kind().viewer().in_helper())
    };
    let helper_entries: HashMap<&str, &Entry> = every
        .iter()
        .filter(|(_, entry)| in_helper(entry))
        .map(|(id, entry)| (*id, *entry))
        .collect();
    let client = st.shared.helper().client();
    st.windows.reconcile(client.as_deref(), &helper_entries);
    let seen = webview::seen_entries(
        front_files
            .iter()
            .filter(|(_, entry, _)| in_helper(entry))
            .map(|(ws, entry, shown)| (ws.as_str(), entry, *shown)),
        |ws| m.live.panes_logic.is_asleep(ws),
    );
    st.windows.set_seen(client.as_deref(), &seen);
    for (entry, keyboard) in st.windows.to_open(&seen) {
        st.windows.mark_tried(&entry.id);
        tasks.push(open_window(st, m, env, &entry, keyboard));
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

/// Reads the pictures a Markdown page names from the disk, off the UI thread
/// and through the checks of `markdown::load_images`.
fn markdown_images(
    id: &str,
    revision: u64,
    wanted: Vec<(String, std::path::PathBuf)>,
) -> Task<Message> {
    if wanted.is_empty() {
        return Task::none();
    }
    let id = id.to_string();
    Task::perform(markdown::load_images(wanted), move |found| {
        viewer_message(&id, revision, ViewerMsg::MarkdownImages(found))
    })
}

/// Colours the code blocks of a Markdown page off the UI thread, once.
fn markdown_code(
    id: &str,
    revision: u64,
    generation: u64,
    blocks: Vec<markdown::CodeBlock>,
) -> Task<Message> {
    if blocks.is_empty() {
        return Task::none();
    }
    let id = id.to_string();
    Task::perform(
        blocking(move || Ok(markdown::highlight_blocks(blocks))),
        move |found| {
            let found = CodeColours(found.unwrap_or_default());
            viewer_message(&id, revision, ViewerMsg::MarkdownCode(generation, found))
        },
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
            let mut images = Task::none();
            let mut code = Task::none();
            if kind == FileKind::Markdown {
                let rendered = markdown::Rendered::parse(&ready.text, &entry.path);
                images = markdown_images(id, revision, rendered.wanted());
                code = markdown_code(id, revision, generation, rendered.code_blocks());
                viewer.rendered = Some(rendered);
            }
            viewer.generation = generation;
            let (body, path) = (ready.text.clone(), entry.path.clone());
            viewer.load = Some(Load::Ready(Box::new(ready)));
            // A page's source is HTML to colour; its path says `.html`.
            Task::batch([
                colour(id, revision, generation, body, path, language),
                images,
                code,
            ])
        }
        ViewerMsg::MarkdownCode(generation, CodeColours(found)) => {
            if let Some(viewer) = st.viewers.get_mut(id)
                && viewer.generation == generation
                && let Some(rendered) = viewer.rendered.as_mut()
            {
                rendered.colour_code(found);
            }
            Task::none()
        }
        ViewerMsg::MarkdownImages(found) => {
            if let Some(rendered) = st.viewers.get_mut(id).and_then(|v| v.rendered.as_mut()) {
                rendered.resolve(found);
            }
            Task::none()
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
            // The lines are laid out anew and the view stays where it was
            // scrolled to, as a class toggled on the TS's `<pre>` kept its
            // `scrollTop`: the scrollable is the same and keeps its offset
            // (the window of built rows follows it), and shortens it itself
            // when the text is now shorter than that.
            st.wrap = !st.wrap;
            Task::none()
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
            // The lines and the rendered page are not the same text.
            viewer.doc.clear();
            viewer.selection.clear();
            if let Some(rendered) = &viewer.rendered {
                rendered.clear_selection();
            }
            // A source is read the first time it is asked for.
            if viewer.source && viewer.load.is_none() {
                viewer.load = Some(Load::Loading);
                return Task::batch([read_text(id, revision, &entry.path), to_top(id)]);
            }
            to_top(id)
        }
        ViewerMsg::Copy => {
            // What is selected, when something is; else the whole text.
            let Some(text) = st.viewers.get(id).and_then(ViewerState::copy_text) else {
                return Task::none();
            };
            let done = notify(
                st,
                id,
                Notice::info(hangar_i18n::t(locale, Key::PreviewCopied)),
            );
            Task::batch([iced::clipboard::write(text), done])
        }
        // The press is in a view pane, which the grid gives the focus (and
        // so takes the keyboard from a terminal, whose Ctrl+C would interrupt
        // what runs in it where here it copies what is selected).
        ViewerMsg::Pressed => Task::none(),
        ViewerMsg::Selected(text) => {
            if let Some(viewer) = st.viewers.get_mut(id) {
                viewer.selection = text;
            }
            Task::none()
        }
        ViewerMsg::Copied => notify(
            st,
            id,
            Notice::info(hangar_i18n::t(locale, Key::PreviewCopied)),
        ),
        ViewerMsg::Image(image::Msg::Gesture(image_stage::Gesture::Pressed)) => Task::none(),
        ViewerMsg::Image(msg) => {
            if let Some(viewer) = st.viewers.get_mut(id) {
                viewer.picture.update(msg);
            }
            Task::none()
        }
        ViewerMsg::Markdown(msg) => markdown_update(st, m, id, revision, msg, locale),
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
    m: &Model,
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
    match msg {
        // The grid gives the pane the focus (`ViewerMsg::Pressed`).
        markdown::Msg::Pressed => return Task::none(),
        markdown::Msg::Selected(text) => {
            if let Some(viewer) = st.viewers.get_mut(id) {
                viewer.selection = text;
            }
            return Task::none();
        }
        markdown::Msg::Copied => {
            return notify(
                st,
                id,
                Notice::info(hangar_i18n::t(locale, Key::PreviewCopied)),
            );
        }
        _ => {}
    }
    let Some(rendered) = st.viewers.get_mut(id).and_then(|v| v.rendered.as_mut()) else {
        return Task::none();
    };
    match msg {
        markdown::Msg::Scrolled(offset) => {
            rendered.scrolled(offset);
            Task::none()
        }
        markdown::Msg::Found(y) => markdown::scroll_by(scroll, y).map(to_message),
        // Handled above, before the page is looked for.
        markdown::Msg::Pressed | markdown::Msg::Selected(_) | markdown::Msg::Copied => Task::none(),
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
/// running. `keyboard`: the window asks for the keyboard, which only a window
/// the person asked for does (`Windows::to_open`).
fn open_window(
    st: &mut State,
    m: &Model,
    env: &Env,
    entry: &Entry,
    keyboard: bool,
) -> Task<Message> {
    let theme = m.theme();
    let helper = st.shared.helper().clone();
    match helper.client() {
        Some(client) => {
            st.windows
                .open(Some(&client), entry, kind_of(entry), theme, keyboard);
            Task::none()
        }
        None => {
            st.windows
                .open(None, entry, kind_of(entry), theme, keyboard);
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
            // The person's click: the window may take the keyboard.
            open_window(st, m, env, entry, true)
        }
        windowed::Action::Focus => {
            if let Some(client) = client {
                st.windows.focus(&client, &entry.id);
            }
            Task::none()
        }
        windowed::Action::Reload => {
            // The TS's reload mounted the frame again, whatever had become of
            // it, and read the source again when it was the one shown
            // (`head.retry()`): a window that was closed comes back, and a
            // source is read from the file once more.
            let source = st
                .viewers
                .get(&entry.id)
                .is_some_and(|viewer| viewer.source);
            let plan = webview::Reload::of(source, st.windows.state(&entry.id));
            let mut tasks = Vec::new();
            if plan.reread {
                tasks.push(viewer_update(
                    st,
                    m,
                    env,
                    &entry.id,
                    entry.revision,
                    ViewerMsg::Retry,
                ));
            }
            if plan.reopen {
                st.windows.mark_tried(&entry.id);
                tasks.push(open_window(st, m, env, entry, true));
            } else if plan.navigate
                && let Some(client) = client
            {
                st.windows.reload(&client, entry);
            }
            Task::batch(tasks)
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

/// Looks at the file of each view pane on screen, as the 3D view watches
/// its model: a newer modification time, steady for a moment, has the viewer
/// read it again. A file that disappears or is now refused keeps its last
/// version on screen, and the reason is said.
fn watch(st: &mut State, m: &Model) -> Task<Message> {
    let mut tasks = Vec::new();
    let view = st.shared.read(|store| store.view().clone());
    for (ws, shown) in screens(m) {
        let Some(scope) = m.live.preview.scope(&ws).filter(|_| shown) else {
            continue;
        };
        for entry in scope
            .entries
            .iter()
            .filter(|entry| (entry.is_file() || entry.is_model()) && view.shows(&entry.id))
        {
            if !st.watching.insert(entry.id.clone()) {
                continue;
            }
            let (id, revision, path) = (entry.id.clone(), entry.revision, entry.path.clone());
            tasks.push(Task::perform(inspect(path), move |found| {
                Message::Preview(Msg::Watched(id.clone(), revision, found))
            }));
        }
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
    Subscription::batch(subscriptions)
}

/// What takes the focus when F6 reaches a region this package owns. It owns
/// none since the previews are panes of the grid (region `main`), so the
/// shell never calls it.
pub fn focus_entry(st: &State, m: &Model, region: Region) -> Option<Task<Message>> {
    let _ = (st, m, region);
    None
}

/// Every intent, after the shell's own handling of it: `Some` when this
/// package acted on it.
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
    match intent {
        // The person's own open lands on its pane; an agent's does not move
        // the keyboard.
        Intent::ShowPreview(request) => {
            let shared = st.shared.clone();
            let request = request.clone();
            let ws = request.ws.clone();
            let person = request.opened_by.is_none();
            Some(Task::perform(
                async move { shared.show_request(request).await },
                move |shown| match shown {
                    Ok(id) if person => Message::Preview(Msg::Opened(ws.clone(), id)),
                    Ok(_) => Message::Noop,
                    Err(why) => Message::Intent(Intent::Notice(crate::shell::Notice::Problem(why))),
                },
            ))
        }
        // A view pane's cross: its entry closes, the pane with it.
        Intent::CloseView { pane, .. } => {
            st.shared.edit(|store| store.close_model(pane));
            m.live.preview.sync();
            panes::sync(m);
            Some(Task::none())
        }
        Intent::ActivateWorkspace(ws) => {
            if m.live.hydrated {
                st.shared.edit(|store| store.restore_kit_tabs(ws));
            }
            None
        }
        // `view.closeModels`: every entry of the workspace closes, and its
        // view panes with them, at once; the viewers and the helper's windows
        // follow the store's change.
        Intent::ClosePreviews(ws) => {
            st.shared.edit(|store| store.close_workspace_models(ws));
            m.live.preview.sync();
            panes::sync(m);
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

/// The body of an entry's view pane: a file viewer, the 3D view
/// (`model3d_view::view`) or a kit tab (`kit_tabs::view`).
pub fn view_entry<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    ws: &'a WsId,
    entry: &'a str,
) -> Element<'a, Message> {
    match ctx.model.live.preview.entry(entry) {
        Some(found) => match &found.body {
            Body::Model { .. } => {
                crate::model3d_view::view(&ctx.pkg.model3d_view, ctx, ws, &found.id)
            }
            Body::Kit(_) => crate::kit_tabs::view(&ctx.pkg.kit_tabs, ctx, ws, &found.id),
            Body::File { .. } => viewer::view(st, ctx, ws, found),
        },
        None => parts::loading(ctx, entry, None),
    }
}

/// What the engines may do with the previews (`Services::preview`). The shell
/// asks once, at boot, after `State::default()`.
pub fn preview_access(st: &State) -> Arc<dyn PreviewAccess> {
    st.shared.clone()
}
