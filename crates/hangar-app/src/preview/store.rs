//! What is open in the previews, one preview per workspace: the files each
//! workspace has open (3D models, images, videos, sounds, text, Markdown,
//! PDF, web pages) and the tabs of its kits, and which one was brought
//! forward last (`lib/modelStore.ts`). Each entry is a pane of its
//! workspace's grid (`panes.rs`): 1.x's panel, whether it showed and whether
//! it filled the work area, is gone.
//! Switching workspaces switches previews; a file an agent shows in a
//! workspace that is not on screen waits there.
//!
//! This is the plain data and its rules, as a struct with `&mut self`
//! methods in place of the TS module's globals; `access.rs` wraps it in the
//! lock the UI, the engines and the kit tabs share. Files are not saved. A
//! kit's tabs are, with their params, in `<app data>/kit-tabs.json` where the
//! TS used the window's storage: they come back with their workspace
//! (`restore_kit_tabs`).
//!
//! Gone with the cross-window protocol of `modelHost.ts`: `takeModelScope`
//! and `adoptModelScope` (a workspace in a window of its own shares this
//! process and so this store), the window's list of 3D models and of kit
//! tabs that kept their identity so a viewer was not woken by an image
//! opening (the views read a snapshot and compare ids), and the tests of
//! those.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::model::{FileKind, file_title};
use crate::state::WsId;

/// What a model's loader found (`ModelInfo`): the 3D view reports it with
/// `settle_model`, the info strip draws it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ModelInfo {
    pub triangles: u64,
    pub vertices: u64,
    pub meshes: u64,
    pub materials: u64,
    /// Animation clip names, in file order.
    pub animations: Vec<String>,
    /// Bounding box size and centre, in the file's units.
    pub size: [f64; 3],
    pub center: [f64; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Loading,
    /// A file that is not a model is ready as soon as it is open: its viewer
    /// reads it on screen.
    Ready,
    Error,
}

/// What a caller passes a kit's tab, by the names its manifest declares. Kept
/// sorted, so a set of params is the same whichever order its names came in.
pub type KitTabParams = BTreeMap<String, String>;

/// The pane of the agent or hook that last asked for a kit's tab, which its
/// page can answer (`KitTabCaller`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KitTabCaller {
    pub pane: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KitBadgeState {
    Running,
    Success,
    Failure,
    Neutral,
}

/// A kit's page's own word for its state, in the tab strip (`tab.setBadge`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KitBadge {
    pub state: KitBadgeState,
    pub text: Option<String>,
}

/// A kit's tab: a page of the kit, isolated (P16 draws it). Which page, and
/// what it may ask, is read from the installed kit and the workspace each time
/// it is drawn, never kept here: a kit updated or taken off the workspace
/// changes it at once.
#[derive(Debug, Clone, PartialEq)]
pub struct KitTab {
    pub kit_id: String,
    pub tab_id: String,
    /// What it was opened with, which its page reads (`tab.params`).
    pub params: KitTabParams,
    /// Its state in the tab strip, until the page next loads.
    pub badge: Option<KitBadge>,
    /// The page named it (`tab.setTitle`): asking for it again leaves that
    /// name.
    pub titled: bool,
    /// Brought back at startup and not shown since: its page is not loaded
    /// yet. Never saved; it ends when the tab is first shown, reloaded, or
    /// asked for again by an agent or a hook.
    pub restored: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    Model {
        /// One of `MODEL_FORMATS`.
        format: String,
        info: Option<ModelInfo>,
    },
    File {
        kind: FileKind,
        mime: String,
        /// In bytes, when it was shown.
        size: u64,
    },
    Kit(KitTab),
}

/// A tab of the preview, whatever it holds (`OpenPreview`).
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub id: String,
    /// Canonical absolute path; `kit:<kit id>/<tab id>` (and the params that
    /// tell it from its siblings) for a kit's tab, which no file's path can
    /// be.
    pub path: String,
    pub title: String,
    /// The workspace whose preview it is open in.
    pub workspace_id: WsId,
    /// The pane (or its name) of the agent that opened it; `None` when the
    /// person did.
    pub opened_by: Option<String>,
    pub opened_at: u64,
    /// The file's mtime when last loaded; a newer one means reload.
    pub modified_ms: u64,
    /// Bumped to make the viewer load the file again.
    pub revision: u64,
    pub status: Status,
    pub error: Option<String>,
    pub body: Body,
}

impl Entry {
    pub fn is_model(&self) -> bool {
        matches!(self.body, Body::Model { .. })
    }

    pub fn is_kit_tab(&self) -> bool {
        matches!(self.body, Body::Kit(_))
    }

    /// A file of the preview: neither a 3D model nor a kit's tab.
    pub fn is_file(&self) -> bool {
        matches!(self.body, Body::File { .. })
    }

    pub fn file_kind(&self) -> Option<FileKind> {
        match &self.body {
            Body::File { kind, .. } => Some(*kind),
            _ => None,
        }
    }

    pub fn kit(&self) -> Option<&KitTab> {
        match &self.body {
            Body::Kit(tab) => Some(tab),
            _ => None,
        }
    }

    pub fn model_info(&self) -> Option<&ModelInfo> {
        match &self.body {
            Body::Model { info, .. } => info.as_ref(),
            _ => None,
        }
    }

    /// The word for what it holds: `model`, `kit`, or the file's kind.
    pub fn kind_name(&self) -> &'static str {
        match &self.body {
            Body::Model { .. } => "model",
            Body::File { kind, .. } => kind.as_str(),
            Body::Kit(_) => "kit",
        }
    }
}

/// One workspace's preview (`ModelScope`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Scope {
    /// Every tab, in order: models and the other kinds together.
    pub entries: Vec<Entry>,
    /// The entry brought forward last (shown, revealed, opened): the model an
    /// agent's `model_snapshot` takes without a path.
    pub active_id: Option<String>,
}

impl Scope {
    pub fn active(&self) -> Option<&Entry> {
        let id = self.active_id.as_deref()?;
        self.entries.iter().find(|entry| entry.id == id)
    }

    pub fn entry(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.id == id)
    }
}

/// A set of params, the same whichever order its names came in
/// (`kitParamsKey`): the JSON of its sorted `[name, value]` pairs.
pub fn kit_params_key(params: &KitTabParams) -> String {
    let pairs: Vec<(&String, &String)> = params.iter().collect();
    serde_json::to_string(&pairs).unwrap_or_default()
}

/// What stands for a kit's tab where a file has its path: one per tab, or,
/// given params, one per set of them. A singleton's stays the same whatever it
/// holds (`kitTabPath`).
pub fn kit_tab_path(kit_id: &str, tab_id: &str, params: &KitTabParams) -> String {
    if params.is_empty() {
        format!("kit:{kit_id}/{tab_id}")
    } else {
        format!("kit:{kit_id}/{tab_id}?{}", kit_params_key(params))
    }
}

/// What opening a kit's tab takes (`KitTabToOpen`).
#[derive(Debug, Clone, PartialEq)]
pub struct KitTabToOpen {
    pub workspace_id: WsId,
    pub kit_id: String,
    pub tab_id: String,
    pub title: String,
    pub params: KitTabParams,
    /// One tab whatever its params: new ones replace the old.
    pub singleton: bool,
    /// The pane of the agent or hook that asks; `None` when the person does.
    pub opened_by: Option<String>,
    /// Makes it the entry brought forward last. False adds it without.
    pub reveal: bool,
}

impl KitTabToOpen {
    pub fn new(
        workspace_id: impl Into<WsId>,
        kit_id: impl Into<String>,
        tab_id: impl Into<String>,
        title: impl Into<String>,
    ) -> Self {
        KitTabToOpen {
            workspace_id: workspace_id.into(),
            kit_id: kit_id.into(),
            tab_id: tab_id.into(),
            title: title.into(),
            params: KitTabParams::new(),
            singleton: false,
            opened_by: None,
            reveal: true,
        }
    }

    pub fn params(mut self, params: &[(&str, &str)]) -> Self {
        self.params = params
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        self
    }

    pub fn singleton(mut self) -> Self {
        self.singleton = true;
        self
    }

    pub fn opened_by(mut self, pane: impl Into<String>) -> Self {
        self.opened_by = Some(pane.into());
        self
    }

    pub fn reveal(mut self, reveal: bool) -> Self {
        self.reveal = reveal;
        self
    }
}

/// What opening a file takes, its id aside (`FileToShow`).
#[derive(Debug, Clone, PartialEq)]
pub struct ShowFile {
    pub workspace_id: WsId,
    pub path: String,
    pub title: Option<String>,
    pub opened_by: Option<String>,
    pub modified_ms: u64,
    pub what: What,
}

#[derive(Debug, Clone, PartialEq)]
pub enum What {
    /// A model waits for the 3D viewer to report on it.
    Model { format: String },
    File {
        kind: FileKind,
        mime: String,
        size: u64,
    },
}

/// A kit's tab as the storage keeps it: only what a page cannot say again.
/// Its badge starts over with the page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedKitTab {
    /// The entry's id, which is its view pane's in the workspace's grid
    /// (`state.json`'s `viewPanes` and `tree`): the tab comes back as that
    /// pane, where the person left it. A file written before the tabs were
    /// panes has none, and the tab comes back as a new pane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub kit_id: String,
    pub tab_id: String,
    pub path: String,
    pub title: String,
    pub titled: bool,
    pub params: KitTabParams,
    pub opened_by: Option<String>,
}

/// Where a workspace's kit tabs are kept across a restart. The app's is the
/// file `kit-tabs.json` (`FileSavedTabs`); the tests', memory.
pub trait SavedTabs: Send {
    /// What a workspace saved: anything, read with care.
    fn read(&self, workspace: &str) -> Option<Value>;
    /// Replaces what a workspace saved; `None` forgets it.
    fn write(&mut self, workspace: &str, tabs: Option<&[SavedKitTab]>);
    /// Every workspace with something saved.
    fn workspaces(&self) -> Vec<String>;
}

/// A tab saved in the storage, read with care: it can hold anything
/// (`readSavedTab`).
fn read_saved_tab(raw: &Value) -> Option<SavedKitTab> {
    let object = raw.as_object()?;
    let text = |key: &str| object.get(key).and_then(Value::as_str);
    let kit_id = text("kitId")?;
    let tab_id = text("tabId")?;
    let path = text("path")?;
    if !path.starts_with(&kit_tab_path(kit_id, tab_id, &KitTabParams::new())) {
        return None;
    }
    let title = text("title")?;
    let mut params = KitTabParams::new();
    for (name, value) in object.get("params")?.as_object()? {
        params.insert(name.clone(), value.as_str()?.to_string());
    }
    Some(SavedKitTab {
        id: text("id").filter(|id| !id.is_empty()).map(str::to_string),
        kit_id: kit_id.into(),
        tab_id: tab_id.into(),
        path: path.into(),
        title: title.into(),
        titled: object.get("titled") == Some(&Value::Bool(true)),
        params,
        opened_by: text("openedBy").map(str::to_string),
    })
}

fn read_saved_tabs(storage: Option<&dyn SavedTabs>, workspace: &str) -> Vec<SavedKitTab> {
    let Some(Value::Array(raw)) = storage.and_then(|storage| storage.read(workspace)) else {
        return Vec::new();
    };
    raw.iter().filter_map(read_saved_tab).collect()
}

/// Hears a tab taking new params, or an agent or a hook asking for it again.
/// It runs under the store's lock: it must not call back into the store.
pub type KitTabWatcher = Box<dyn FnMut(&KitTabParams, Option<&KitTabCaller>) + Send>;

/// The id `watch_kit_tab_params` hands back, to stop listening.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WatchId(u64);

/// The workspaces the windows have on screen (the main window's active one and
/// every workspace in a window of its own), whether each one's grid is drawn
/// there (the terminals view, not the board or the stats), and which entries
/// have their pane on screen (`noteModelView`): kept for the agents' answers,
/// since a file shown elsewhere is not one the user can see, and for what
/// runs only while it is seen (a kit tab that waits for its first look, a
/// helper window, a playing clip).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct View {
    screens: BTreeMap<WsId, bool>,
    shown: BTreeSet<String>,
}

impl View {
    /// A window has the workspace on screen.
    pub fn on_screen(&self, workspace: &str) -> bool {
        self.screens.contains_key(workspace)
    }

    /// And its grid is drawn there: its view panes can be seen, but one
    /// zoomed over the others.
    pub fn panel_shown(&self, workspace: &str) -> bool {
        self.screens.get(workspace).copied().unwrap_or(false)
    }

    /// The entry's pane is on screen: its workspace's grid is drawn, and no
    /// other pane is zoomed over it.
    pub fn shows(&self, id: &str) -> bool {
        self.shown.contains(id)
    }
}

pub struct Store {
    scopes: HashMap<WsId, Scope>,
    revision: u64,
    view: View,
    saved: Option<Box<dyn SavedTabs>>,
    /// The workspaces whose saved tabs have been read: until then, saving
    /// would lose them.
    restored: HashSet<WsId>,
    watchers: HashMap<String, Vec<(WatchId, KitTabWatcher)>>,
    next_watch: u64,
    next_id: u64,
    clock: fn() -> u64,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// A workspace with nothing open.
pub static EMPTY: Scope = Scope {
    entries: Vec::new(),
    active_id: None,
};

impl Default for Store {
    fn default() -> Self {
        Store::new()
    }
}

impl Store {
    pub fn new() -> Store {
        Store::with_clock(now_ms)
    }

    pub fn with_clock(clock: fn() -> u64) -> Store {
        Store {
            scopes: HashMap::new(),
            revision: 0,
            view: View::default(),
            saved: None,
            restored: HashSet::new(),
            watchers: HashMap::new(),
            next_watch: 0,
            next_id: 0,
            clock,
        }
    }

    /// A store that keeps its kit tabs in `saved`.
    pub fn with_saved(saved: Box<dyn SavedTabs>) -> Store {
        let mut store = Store::new();
        store.saved = Some(saved);
        store
    }

    /// Bumped by every change: what the views and a waiting caller follow.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Whether the workspace's saved kit tabs have been read back
    /// (`restore_kit_tabs`): until then its view panes wait for their
    /// entries, rather than being dropped for having none.
    pub fn is_restored(&self, workspace: &str) -> bool {
        self.restored.contains(workspace)
    }

    /// An id no entry has: a tab brought back from `kit-tabs.json` keeps the
    /// one an earlier run gave it, which is its pane's.
    fn new_id(&mut self) -> String {
        loop {
            self.next_id += 1;
            let id = format!("p{:x}-{}", (self.clock)(), self.next_id);
            if self.entry(&id).is_none() {
                return id;
            }
        }
    }

    /// Replaces the scopes named in `patch`; `None` drops one. A scope with
    /// nothing open is dropped too (`set`).
    fn set(&mut self, patch: Vec<(WsId, Option<Scope>)>) {
        let named: Vec<WsId> = patch.iter().map(|(id, _)| id.clone()).collect();
        for (workspace, scope) in patch {
            match scope {
                Some(scope) if !scope.entries.is_empty() => {
                    self.scopes.insert(workspace, scope);
                }
                _ => {
                    self.scopes.remove(&workspace);
                }
            }
        }
        self.revision += 1;
        for workspace in named {
            self.save_kit_tabs(&workspace);
        }
    }

    /// The workspace's preview; an empty one when it has nothing open
    /// (`modelScope`).
    pub fn scope(&self, workspace: &str) -> &Scope {
        self.scopes.get(workspace).unwrap_or(&EMPTY)
    }

    /// Every workspace's, for the views.
    pub fn scopes(&self) -> &HashMap<WsId, Scope> {
        &self.scopes
    }

    /// Everything open in the window, every workspace's, every kind.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.scopes.values().flat_map(|scope| scope.entries.iter())
    }

    /// Every workspace's 3D models in one list: what the window's 3D viewer
    /// may hold.
    pub fn models(&self) -> Vec<&Entry> {
        self.entries().filter(|entry| entry.is_model()).collect()
    }

    /// Every workspace's kit tabs: the pages the window keeps running.
    pub fn kit_tabs(&self) -> Vec<&Entry> {
        self.entries().filter(|entry| entry.is_kit_tab()).collect()
    }

    pub fn entry(&self, id: &str) -> Option<&Entry> {
        self.entries().find(|entry| entry.id == id)
    }

    /// One entry of a scope rewritten.
    fn with_entry(scope: &Scope, id: &str, change: impl FnOnce(&mut Entry)) -> Scope {
        let mut next = scope.clone();
        if let Some(entry) = next.entries.iter_mut().find(|entry| entry.id == id) {
            change(entry);
        }
        next
    }

    fn kind_fields(what: &What) -> (Body, Status) {
        match what {
            What::Model { format } => (
                Body::Model {
                    format: format.clone(),
                    info: None,
                },
                Status::Loading,
            ),
            What::File { kind, mime, size } => (
                Body::File {
                    kind: *kind,
                    mime: mime.clone(),
                    size: *size,
                },
                Status::Ready,
            ),
        }
    }

    /// Opens a file in its workspace's preview, or brings it forward and
    /// reloads it when that workspace has it open already: an agent showing
    /// the same file again has usually just rewritten it. Returns the entry's
    /// id (`showPreview`).
    pub fn show_file(&mut self, file: ShowFile) -> String {
        self.show_file_with(file, true)
    }

    /// `show_file`; without `reveal` the file is only added, the entry
    /// brought forward last as it was, as a kit's tab is.
    pub fn show_file_with(&mut self, file: ShowFile, reveal: bool) -> String {
        let scope = self.scope(&file.workspace_id).clone();
        let title = file
            .title
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .map_or_else(|| file_title(&file.path), str::to_string);
        if let Some(existing) = scope.entries.iter().find(|entry| entry.path == file.path) {
            let id = existing.id.clone();
            let mut next = Store::with_entry(&scope, &id, |entry| {
                let (mut body, status) = Store::kind_fields(&file.what);
                // A model keeps what it last showed until the new version
                // lands.
                if let (Body::Model { info, .. }, Body::Model { info: previous, .. }) =
                    (&mut body, &entry.body)
                {
                    *info = previous.clone();
                }
                entry.title = title;
                entry.opened_by = file.opened_by.clone().or_else(|| entry.opened_by.take());
                entry.revision += 1;
                entry.error = None;
                entry.modified_ms = file.modified_ms;
                entry.body = body;
                entry.status = status;
            });
            if reveal {
                next.active_id = Some(id.clone());
            }
            self.set(vec![(file.workspace_id, Some(next))]);
            return id;
        }
        let id = self.new_id();
        let (body, status) = Store::kind_fields(&file.what);
        let entry = Entry {
            id: id.clone(),
            path: file.path,
            title,
            workspace_id: file.workspace_id.clone(),
            opened_by: file.opened_by,
            opened_at: (self.clock)(),
            modified_ms: file.modified_ms,
            revision: 0,
            status,
            error: None,
            body,
        };
        let mut next = scope;
        if reveal || next.active_id.is_none() {
            next.active_id = Some(id.clone());
        }
        next.entries.push(entry);
        self.set(vec![(file.workspace_id, Some(next))]);
        id
    }

    /// The open tab a request lands on: a singleton's one tab whatever it
    /// holds, or the tab holding this very set of params (`findKitTab`).
    pub fn find_kit_tab<'a>(
        entries: &'a [Entry],
        kit_id: &str,
        tab_id: &str,
        params: &KitTabParams,
        singleton: bool,
    ) -> Option<&'a Entry> {
        let key = kit_params_key(params);
        entries.iter().find(|entry| {
            entry.kit().is_some_and(|tab| {
                tab.kit_id == kit_id
                    && tab.tab_id == tab_id
                    && (singleton || kit_params_key(&tab.params) == key)
            })
        })
    }

    /// Opens a kit's tab in its workspace's preview, or brings it forward when
    /// it is open already, as it is: a page someone is working in is not
    /// reloaded for being asked for again. A singleton given other params
    /// takes them; none given leaves what it holds.
    ///
    /// An agent or a hook asking for an open tab becomes who its page answers
    /// (`kit_tab_caller`), and the page hears of it (`watch_kit_tab_params`),
    /// as it does of new params. The person bringing it forward changes
    /// neither: the agent that asked is still the one waiting.
    ///
    /// With `reveal`, the default, it is the entry brought forward last;
    /// without, that stays as it was. Neither ever moves the keyboard.
    /// Returns the entry's id (`openKitTab`).
    pub fn open_kit_tab(&mut self, tab: KitTabToOpen) -> String {
        self.restore_kit_tabs(&tab.workspace_id);
        let scope = self.scope(&tab.workspace_id).clone();
        let opened_by = tab.opened_by.clone();
        let title = Some(tab.title.trim())
            .filter(|title| !title.is_empty())
            .unwrap_or(&tab.tab_id)
            .to_string();
        let existing = Store::find_kit_tab(
            &scope.entries,
            &tab.kit_id,
            &tab.tab_id,
            &tab.params,
            tab.singleton,
        )
        .cloned();
        if let Some(existing) = existing {
            let existing_tab = existing.kit().cloned().unwrap_or_else(|| KitTab {
                kit_id: tab.kit_id.clone(),
                tab_id: tab.tab_id.clone(),
                params: KitTabParams::new(),
                badge: None,
                titled: false,
                restored: false,
            });
            let replaced = !tab.params.is_empty()
                && kit_params_key(&tab.params) != kit_params_key(&existing_tab.params);
            let mut next = Store::with_entry(&scope, &existing.id, |entry| {
                if !existing_tab.titled {
                    entry.title = title;
                }
                if opened_by.is_some() {
                    entry.opened_by = opened_by.clone();
                }
                if let Body::Kit(kit) = &mut entry.body {
                    // Asked for by an agent or a hook: it may be followed, so
                    // it loads.
                    if opened_by.is_some() {
                        kit.restored = false;
                    }
                    if replaced {
                        kit.params = tab.params.clone();
                    }
                }
            });
            if tab.reveal {
                next.active_id = Some(existing.id.clone());
            }
            self.set(vec![(tab.workspace_id.clone(), Some(next))]);
            if replaced || opened_by.is_some() {
                self.tell_watchers(&existing.id);
            }
            return existing.id;
        }
        let id = self.new_id();
        // A singleton's path stays the same whatever it holds.
        let no_params = KitTabParams::new();
        let path = kit_tab_path(
            &tab.kit_id,
            &tab.tab_id,
            if tab.singleton {
                &no_params
            } else {
                &tab.params
            },
        );
        let entry = self.kit_tab_entry(
            id.clone(),
            tab.workspace_id.clone(),
            KitTab {
                kit_id: tab.kit_id,
                tab_id: tab.tab_id,
                params: tab.params,
                badge: None,
                titled: false,
                restored: false,
            },
            path,
            title,
            opened_by.clone(),
        );
        let mut next = scope.clone();
        next.entries.push(entry);
        if tab.reveal || scope.active_id.is_none() {
            next.active_id = Some(id.clone());
        }
        self.set(vec![(tab.workspace_id, Some(next))]);
        id
    }

    fn kit_tab_entry(
        &self,
        id: String,
        workspace_id: WsId,
        kit: KitTab,
        path: String,
        title: String,
        opened_by: Option<String>,
    ) -> Entry {
        Entry {
            id,
            path,
            title,
            workspace_id,
            opened_by,
            opened_at: (self.clock)(),
            modified_ms: 0,
            revision: 0,
            status: Status::Ready,
            error: None,
            body: Body::Kit(kit),
        }
    }

    /// Loads a kit tab's page afresh: its reload button. What the last page
    /// said of its state goes with it (`reloadKitTab`).
    pub fn reload_kit_tab(&mut self, id: &str) {
        let Some(entry) = self.entry(id).filter(|entry| entry.is_kit_tab()) else {
            return;
        };
        let workspace = entry.workspace_id.clone();
        let next = Store::with_entry(self.scope(&workspace), id, |entry| {
            entry.revision += 1;
            if let Body::Kit(kit) = &mut entry.body {
                kit.badge = None;
                kit.restored = false;
            }
        });
        self.set(vec![(workspace, Some(next))]);
    }

    /// A restored kit tab has been shown: from now on its page is loaded like
    /// any other (`markKitTabShown`).
    pub fn mark_kit_tab_shown(&mut self, id: &str) {
        let Some(entry) = self
            .entry(id)
            .filter(|entry| entry.kit().is_some_and(|kit| kit.restored))
        else {
            return;
        };
        let workspace = entry.workspace_id.clone();
        let next = Store::with_entry(self.scope(&workspace), id, |entry| {
            if let Body::Kit(kit) = &mut entry.body {
                kit.restored = false;
            }
        });
        self.set(vec![(workspace, Some(next))]);
    }

    /// What a kit's tab holds now; none once it is closed (`kitTabParams`).
    pub fn kit_tab_params(&self, id: &str) -> KitTabParams {
        self.entry(id)
            .and_then(Entry::kit)
            .map(|kit| kit.params.clone())
            .unwrap_or_default()
    }

    /// Who a kit's tab answers: the pane of the agent or hook that last asked
    /// for it, from Hangar's own record of the request. `None` when only the
    /// person did, and once it is closed (`kitTabCaller`).
    pub fn kit_tab_caller(&self, id: &str) -> Option<KitTabCaller> {
        let entry = self.entry(id).filter(|entry| entry.is_kit_tab())?;
        entry
            .opened_by
            .as_ref()
            .map(|pane| KitTabCaller { pane: pane.clone() })
    }

    /// Hears a tab taking new params, or an agent or a hook asking for it
    /// again. Stop with `unwatch_kit_tab_params`.
    pub fn watch_kit_tab_params(&mut self, id: &str, watcher: KitTabWatcher) -> WatchId {
        self.next_watch += 1;
        let watch = WatchId(self.next_watch);
        self.watchers
            .entry(id.to_string())
            .or_default()
            .push((watch, watcher));
        watch
    }

    pub fn unwatch_kit_tab_params(&mut self, id: &str, watch: WatchId) {
        if let Some(list) = self.watchers.get_mut(id) {
            list.retain(|(held, _)| *held != watch);
            if list.is_empty() {
                self.watchers.remove(id);
            }
        }
    }

    fn tell_watchers(&mut self, id: &str) {
        let params = self.kit_tab_params(id);
        let caller = self.kit_tab_caller(id);
        if let Some(list) = self.watchers.get_mut(id) {
            for (_, watcher) in list.iter_mut() {
                watcher(&params, caller.as_ref());
            }
        }
    }

    /// A kit tab's own name for itself (`tab.setTitle`), checked by P16
    /// (`setKitTabTitle`).
    pub fn set_kit_tab_title(&mut self, id: &str, title: &str) {
        let Some(entry) = self.entry(id) else { return };
        let Some(kit) = entry.kit() else { return };
        if kit.titled && entry.title == title {
            return;
        }
        let workspace = entry.workspace_id.clone();
        let next = Store::with_entry(self.scope(&workspace), id, |entry| {
            entry.title = title.to_string();
            if let Body::Kit(kit) = &mut entry.body {
                kit.titled = true;
            }
        });
        self.set(vec![(workspace, Some(next))]);
    }

    /// A kit tab's state in the tab strip (`tab.setBadge`), checked by P16;
    /// `None` takes it away (`setKitTabBadge`).
    pub fn set_kit_tab_badge(&mut self, id: &str, badge: Option<KitBadge>) {
        let Some(entry) = self.entry(id) else { return };
        let Some(kit) = entry.kit() else { return };
        if kit.badge == badge {
            return;
        }
        let workspace = entry.workspace_id.clone();
        let next = Store::with_entry(self.scope(&workspace), id, |entry| {
            if let Body::Kit(kit) = &mut entry.body {
                kit.badge = badge;
            }
        });
        self.set(vec![(workspace, Some(next))]);
    }

    /* ── a kit's tabs, across a restart ──────────────────────────── */

    fn save_kit_tabs(&mut self, workspace: &str) {
        if !self.restored.contains(workspace) {
            return;
        }
        let tabs: Vec<SavedKitTab> = self
            .scope(workspace)
            .entries
            .iter()
            .filter_map(|entry| {
                let kit = entry.kit()?;
                Some(SavedKitTab {
                    id: Some(entry.id.clone()),
                    kit_id: kit.kit_id.clone(),
                    tab_id: kit.tab_id.clone(),
                    path: entry.path.clone(),
                    title: entry.title.clone(),
                    titled: kit.titled,
                    params: kit.params.clone(),
                    opened_by: entry.opened_by.clone(),
                })
            })
            .collect();
        if let Some(saved) = &mut self.saved {
            saved.write(workspace, (!tabs.is_empty()).then_some(tabs.as_slice()));
        }
    }

    /// Brings back the kit tabs a workspace had open when the app last closed,
    /// with their params, once. Called when the workspace is first opened, so
    /// no page runs for a workspace nobody opened, each under the id it was
    /// saved with, which is its view pane's (`panes.rs`), when no entry has it.
    /// A restored tab's page waits for its pane to be on screen. What is open
    /// already stays, and is not opened twice (`restoreKitTabs`).
    pub fn restore_kit_tabs(&mut self, workspace: &str) {
        if !self.restored.insert(workspace.to_string()) {
            return;
        }
        let scope = self.scope(workspace).clone();
        let paths: HashSet<&str> = scope.entries.iter().map(|e| e.path.as_str()).collect();
        let saved = read_saved_tabs(self.saved.as_deref(), workspace);
        let mut taken: HashSet<String> = self.entries().map(|entry| entry.id.clone()).collect();
        let arriving: Vec<Entry> = saved
            .into_iter()
            .filter(|tab| !paths.contains(tab.path.as_str()))
            .map(|tab| {
                // Its own id when it has one no entry uses: the id of its pane.
                let id = match tab.id.filter(|id| !taken.contains(id)) {
                    Some(id) => id,
                    None => self.new_id(),
                };
                taken.insert(id.clone());
                self.kit_tab_entry(
                    id,
                    workspace.to_string(),
                    KitTab {
                        kit_id: tab.kit_id,
                        tab_id: tab.tab_id,
                        params: tab.params,
                        badge: None,
                        titled: tab.titled,
                        restored: true,
                    },
                    tab.path,
                    tab.title,
                    tab.opened_by,
                )
            })
            .collect();
        if arriving.is_empty() {
            return self.save_kit_tabs(workspace);
        }
        let mut next = scope.clone();
        next.active_id = scope
            .active_id
            .clone()
            .or_else(|| arriving.last().map(|entry| entry.id.clone()));
        next.entries.extend(arriving);
        self.set(vec![(workspace.to_string(), Some(next))]);
    }

    /// Forgets what the workspaces that are not in `live` saved: deleted since
    /// (`forgetSavedKitTabs`).
    pub fn forget_saved_kit_tabs(&mut self, live: &[WsId]) {
        let keep: HashSet<&WsId> = live.iter().collect();
        let Some(saved) = &mut self.saved else { return };
        for workspace in saved.workspaces() {
            if !keep.contains(&workspace) {
                saved.write(&workspace, None);
            }
        }
    }

    /* ── closing ─────────────────────────────────────────────────── */

    /// Closes entries, wherever they are open. A workspace that still has some
    /// has the last of them brought forward if the one brought forward last
    /// went (`closeModels`).
    pub fn close_models(&mut self, ids: &[String]) -> usize {
        let closing: HashSet<&str> = ids.iter().map(String::as_str).collect();
        let mut patch = Vec::new();
        let mut closed = 0;
        for (workspace, scope) in &self.scopes {
            let count = scope
                .entries
                .iter()
                .filter(|entry| closing.contains(entry.id.as_str()))
                .count();
            if count == 0 {
                continue;
            }
            closed += count;
            let mut next = scope.clone();
            next.entries
                .retain(|entry| !closing.contains(entry.id.as_str()));
            if scope
                .active_id
                .as_deref()
                .is_some_and(|active| closing.contains(active))
            {
                next.active_id = next.entries.last().map(|entry| entry.id.clone());
            }
            patch.push((
                workspace.clone(),
                (!next.entries.is_empty()).then_some(next),
            ));
        }
        if !patch.is_empty() {
            self.set(patch);
            // A closed tab's page has nobody left to hear.
            for id in ids {
                self.watchers.remove(id);
            }
        }
        closed
    }

    pub fn close_model(&mut self, id: &str) {
        self.close_models(&[id.to_string()]);
    }

    /// Everything one workspace has open: its preview goes away
    /// (`closeWorkspaceModels`).
    pub fn close_workspace_models(&mut self, workspace: &str) -> usize {
        let Some(scope) = self.scopes.get(workspace) else {
            return 0;
        };
        let count = scope.entries.len();
        for entry in &scope.entries {
            self.watchers.remove(&entry.id);
        }
        self.set(vec![(workspace.to_string(), None)]);
        count
    }

    /// Everything every workspace has open (`closeAllModels`).
    pub fn close_all_models(&mut self) {
        if self.scopes.is_empty() {
            return;
        }
        let patch = self.scopes.keys().map(|id| (id.clone(), None)).collect();
        self.watchers.clear();
        self.set(patch);
    }

    /// Forgets the previews of the workspaces that are not in `live`: deleted
    /// since (`pruneModels`).
    pub fn prune_models(&mut self, live: &[WsId]) {
        let keep: HashSet<&WsId> = live.iter().collect();
        let gone: Vec<WsId> = self
            .scopes
            .keys()
            .filter(|id| !keep.contains(id))
            .cloned()
            .collect();
        if gone.is_empty() {
            return;
        }
        for id in &gone {
            if let Some(scope) = self.scopes.get(id) {
                for entry in &scope.entries {
                    self.watchers.remove(&entry.id);
                }
            }
        }
        self.set(gone.into_iter().map(|id| (id, None)).collect());
    }

    /// Puts an entry in front in its workspace's preview (`activateModel`).
    pub fn activate_model(&mut self, id: &str) {
        let Some(entry) = self.entry(id) else { return };
        let workspace = entry.workspace_id.clone();
        let mut next = self.scope(&workspace).clone();
        if next.active_id.as_deref() == Some(id) {
            return;
        }
        next.active_id = Some(id.to_string());
        self.set(vec![(workspace, Some(next))]);
    }

    /// The 3D viewer's report on a load: what it found, or why it failed
    /// (`settleModel`).
    pub fn settle_model(&mut self, id: &str, result: Result<ModelInfo, String>) {
        let Some(model) = self.entry(id).filter(|entry| entry.is_model()) else {
            return;
        };
        let workspace = model.workspace_id.clone();
        let next = Store::with_entry(self.scope(&workspace), id, |entry| match result {
            Ok(found) => {
                if let Body::Model { info, .. } = &mut entry.body {
                    *info = Some(found);
                }
                entry.status = Status::Ready;
                entry.error = None;
            }
            Err(error) => {
                entry.status = Status::Error;
                entry.error = Some(error);
            }
        });
        self.set(vec![(workspace, Some(next))]);
    }

    /// A rewritten file: load it again, a model keeping the camera where the
    /// user left it, and waiting for the 3D viewer; any other kind read afresh
    /// by its viewer, its new size noted when it is known (`reloadModel`). A
    /// kit's tab has no file to watch: its own button reloads it.
    pub fn reload_model(&mut self, id: &str, modified_ms: u64, size: Option<u64>) {
        let Some(entry) = self.entry(id).filter(|entry| !entry.is_kit_tab()) else {
            return;
        };
        let workspace = entry.workspace_id.clone();
        let next = Store::with_entry(self.scope(&workspace), id, |entry| match &mut entry.body {
            Body::Model { .. } => {
                entry.modified_ms = modified_ms;
                entry.revision += 1;
                entry.status = Status::Loading;
            }
            Body::File { size: held, .. } => {
                entry.modified_ms = modified_ms;
                entry.revision += 1;
                *held = size.unwrap_or(*held);
            }
            Body::Kit(_) => {}
        });
        self.set(vec![(workspace, Some(next))]);
    }

    /// Whether an entry has loaded or failed, for a caller waiting on it
    /// (`whenSettled`'s test): `None` for an entry that is not open.
    pub fn settled(&self, id: &str) -> Option<Option<&Entry>> {
        let entry = self.entry(id)?;
        Some((entry.status != Status::Loading).then_some(entry))
    }

    /* ── what the window shows ───────────────────────────────────── */

    /// Which workspaces the windows have on screen, and whether the grid is
    /// drawn with each (`noteModelView`). It changes nothing a view draws:
    /// nobody is woken.
    pub fn note_screens(&mut self, screens: impl IntoIterator<Item = (WsId, bool)>) {
        self.view.screens = screens.into_iter().collect();
    }

    /// Which entries have their pane on screen (`View::shows`). It changes
    /// nothing a view draws either.
    pub fn note_shown(&mut self, shown: impl IntoIterator<Item = String>) {
        self.view.shown = shown.into_iter().collect();
    }

    /// One entry's pane put on screen or taken off it, the others as they
    /// were: the tests share the process's store, and each says what is on
    /// screen of its own entries only.
    #[cfg(test)]
    pub fn set_shown(&mut self, id: &str, shown: bool) {
        if shown {
            self.view.shown.insert(id.to_string());
        } else {
            self.view.shown.remove(id);
        }
    }

    pub fn view(&self) -> &View {
        &self.view
    }
}

#[cfg(test)]
mod tests;
