//! The preview store, shared: the UI reads and writes it, the engines (the
//! agents' control operations, P18) reach it through `PreviewAccess`, and the
//! kit tabs (P16) and the 3D view (P17) go through the same lock.
//!
//! The control handlers of the TS asked a `ModelHost` that was local, or
//! forwarded to the window that drew the workspace; in one process that is
//! this object. Every change bumps a counter that a subscription turns into a
//! message (`Msg::Changed`), so the views learn of a file an agent opened
//! without being called.
//!
//! There is one per process. `State` and `Live` are both `Default`, built by
//! the shell before anything can hand either of them the other, so both take
//! the process's store (`Shared::global`). Tests build their own.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};
use std::time::Duration;

use serde_json::{Map, Value};
use tokio::sync::watch;

use super::model::{self, FileKind, Kind, PreviewFile};
use super::roots::{self, RootGrants};
use super::store::{Entry, SavedKitTab, SavedTabs, Scope, ShowFile, Status, Store, View, What};
use super::webview::Helper;
use crate::services::PreviewAccess;
use crate::services::preview::{BoxFuture, PreviewEntry, PreviewRequest, SnapshotAngle};
use crate::state::{Snapshot, Workspace, WsId};

/// How long a 3D view that is mounting gets to register its renderer: it
/// loads its pipelines on demand the first time (`MOUNT_WAIT_MS`).
const MOUNT_WAIT: Duration = Duration::from_secs(4);
const MOUNT_POLL: Duration = Duration::from_millis(50);

/// The message of a refusal to look at another workspace's file, which the
/// control handlers pass on to the agent (`NOT_HERE`).
pub const NOT_HERE: &str = "That file is not open in this workspace.";

/// Renders an open model off screen, as PNG bytes: the 3D view registers how
/// (`registerSnapshotter`). P17 implements it over the offscreen renderer of
/// hangar-model3d.
pub trait Snapshotter: Send + Sync {
    fn snapshot(
        &self,
        id: &str,
        angle: SnapshotAngle,
        size: (u32, u32),
    ) -> BoxFuture<Result<Vec<u8>, String>>;
}

/// A workspace's preview as the control handlers answer about it
/// (`ModelList`).
#[derive(Debug, Clone, PartialEq)]
pub struct ListView {
    /// Every tab, of every kind.
    pub entries: Vec<PreviewEntry>,
    pub active_id: Option<String>,
    /// The workspace is the one its window has on screen.
    pub on_screen: bool,
    /// And its grid is drawn there (the terminals view, not the board or the
    /// stats): the panes of what it shows can be seen.
    pub shown: bool,
}

pub struct Shared {
    me: Weak<Shared>,
    store: Mutex<Store>,
    changes: watch::Sender<u64>,
    grants: Mutex<RootGrants>,
    snapshots: Mutex<Option<watch::Receiver<Arc<Snapshot>>>>,
    snapshotter: Mutex<Option<Arc<dyn Snapshotter>>>,
    /// `MOUNT_WAIT`, shorter under test.
    mount_wait: Mutex<Duration>,
    /// The browser-engine helper, one for everything that shows a page.
    helper: Arc<Helper>,
}

impl Shared {
    pub fn new(saved: Option<Box<dyn SavedTabs>>) -> Arc<Shared> {
        let store = match saved {
            Some(saved) => Store::with_saved(saved),
            None => Store::new(),
        };
        Arc::new_cyclic(|me| Shared {
            me: me.clone(),
            store: Mutex::new(store),
            changes: watch::channel(0).0,
            grants: Mutex::new(RootGrants::default()),
            snapshots: Mutex::new(None),
            snapshotter: Mutex::new(None),
            mount_wait: Mutex::new(MOUNT_WAIT),
            helper: Helper::new(),
        })
    }

    /// The helper process that shows what only a browser engine can: a PDF, a
    /// page, a video, and the HTML tabs of kits (P16).
    pub fn helper(&self) -> &Arc<Helper> {
        &self.helper
    }

    #[cfg(test)]
    fn wait_for_mount(&self, wait: Duration) {
        *self.mount_wait.lock().unwrap() = wait;
    }

    /// The process's store, with its kit tabs in `<app data>/kit-tabs.json`
    /// (none under test, which must never touch a person's files).
    pub fn global() -> Arc<Shared> {
        static GLOBAL: OnceLock<Arc<Shared>> = OnceLock::new();
        GLOBAL
            .get_or_init(|| {
                let saved = (!cfg!(test))
                    .then(|| hangar_core::paths::Paths::from_env().app_data_dir().ok())
                    .flatten()
                    .map(|dir| {
                        Box::new(FileSavedTabs::new(dir.join("kit-tabs.json")))
                            as Box<dyn SavedTabs>
                    });
                Shared::new(saved)
            })
            .clone()
    }

    fn lock(&self) -> MutexGuard<'_, Store> {
        self.store
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// Reads the store.
    pub fn read<T>(&self, read: impl FnOnce(&Store) -> T) -> T {
        read(&self.lock())
    }

    /// Changes the store, and tells whoever follows it if it changed. The
    /// closure runs under the lock: it must not call back into this object.
    pub fn edit<T>(&self, edit: impl FnOnce(&mut Store) -> T) -> T {
        let mut store = self.lock();
        let before = store.revision();
        let out = edit(&mut store);
        let after = store.revision();
        drop(store);
        if after != before {
            self.changes.send_replace(after);
        }
        out
    }

    /// The revision of the store: what a cache of its content follows.
    pub fn revision(&self) -> u64 {
        self.lock().revision()
    }

    /// Wakes on every change of the store.
    pub fn follow(&self) -> watch::Receiver<u64> {
        self.changes.subscribe()
    }

    /// Every workspace's preview, cloned, for the views.
    pub fn scopes(&self) -> BTreeMap<WsId, Scope> {
        self.read(|store| {
            store
                .scopes()
                .iter()
                .map(|(id, scope)| (id.clone(), scope.clone()))
                .collect()
        })
    }

    /// Gives the store a way to read the model, which only the shell's
    /// snapshot offers off the UI thread: where a relative path starts and
    /// which folders a workspace owns.
    pub fn attach(&self, snapshots: watch::Receiver<Arc<Snapshot>>) {
        *self
            .snapshots
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = Some(snapshots);
    }

    fn model_snapshot(&self) -> Option<Arc<Snapshot>> {
        self.snapshots
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .as_ref()
            .map(|receiver| receiver.borrow().clone())
    }

    /// A workspace of the model now.
    pub fn workspace(&self, id: &str) -> Option<Workspace> {
        self.model_snapshot()?
            .state
            .workspaces
            .iter()
            .find(|workspace| workspace.id == id)
            .cloned()
    }

    fn live_workspaces(&self) -> Vec<WsId> {
        self.model_snapshot()
            .map(|snapshot| {
                snapshot
                    .state
                    .workspaces
                    .iter()
                    .map(|workspace| workspace.id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The 3D view registers how it renders a snapshot; `None` when it goes
    /// (`registerSnapshotter`).
    pub fn register_snapshotter(&self, snapshotter: Option<Arc<dyn Snapshotter>>) {
        *self
            .snapshotter
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = snapshotter;
    }

    pub fn has_snapshotter(&self) -> bool {
        self.snapshotter
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .is_some()
    }

    fn current_snapshotter(&self) -> Option<Arc<dyn Snapshotter>> {
        self.snapshotter
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }

    /// A workspace's preview with where it stands on screen (`ModelHost.list`).
    pub fn list_view(&self, workspace: &str) -> ListView {
        self.read(|store| {
            let scope = store.scope(workspace);
            let view: &View = store.view();
            ListView {
                entries: scope.entries.iter().map(preview_entry).collect(),
                active_id: scope.active_id.clone(),
                on_screen: view.on_screen(workspace),
                shown: view.panel_shown(workspace),
            }
        })
    }

    /// Sends hangar-core the folders every live workspace was granted, this
    /// workspace's now `roots` (`grantModelRoots`).
    pub async fn grant_roots(&self, workspace: &str, roots: Vec<String>) -> Result<(), String> {
        let live = self.live_workspaces();
        let union = self
            .grants
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .grant(workspace, roots, &live);
        hangar_core::model::model_allow_roots(union).await
    }

    /// Lets hangar-core serve the workspace's folders (and its worktrees), then
    /// asks it what `path` is: the file, or the refusal worth passing on
    /// (`admit`).
    pub async fn admit(&self, workspace: &Workspace, path: &str) -> Result<PreviewFile, String> {
        let mut roots = roots::workspace_roots(workspace);
        let repos: Vec<String> = std::iter::once(workspace.cwd.clone())
            .chain(workspace.extra_roots.iter().cloned())
            .collect();
        for repo in repos {
            // A folder that is not a repository, or a machine without git,
            // just adds none.
            if let Ok(listed) = hangar_core::git::git_worktree_list(repo).await {
                roots.extend(
                    listed
                        .into_iter()
                        .filter(|tree| !tree.bare && tree.prunable.is_none())
                        .map(|tree| tree.path),
                );
            }
        }
        let mut seen = std::collections::HashSet::new();
        roots.retain(|root| !root.trim().is_empty() && seen.insert(roots::comparable(root)));
        let outside = || {
            format!(
                "{path} is outside your workspace. The preview only opens files from {} \
                 (and their subfolders). Move or copy the file into one of them, then show it from there.",
                roots.join(", ")
            )
        };
        self.grant_roots(&workspace.id, roots.clone()).await?;
        // Rust is the real gate, on the canonical path. The folders it allows
        // are every workspace's that showed a file, though, and this one is
        // checked against the caller's own: the path as given, or where its
        // links lead.
        let given_inside = roots::inside(path, &roots);
        let inspected = hangar_core::preview::preview_inspect(path.to_string()).await;
        let found = match inspected {
            Ok(found) => found,
            Err(_) if !given_inside => return Err(outside()),
            Err(refusal) => return Err(refusal),
        };
        let file = model::from_core(&found)?;
        if !given_inside && !roots::inside(&file.path, &roots) {
            return Err(outside());
        }
        Ok(file)
    }

    /// What `PreviewAccess::show` does for a request, asynchronously: the path
    /// resolved against the workspace's folder, admitted, then opened.
    pub async fn show_request(&self, request: PreviewRequest) -> Result<String, String> {
        let workspace = self
            .workspace(&request.ws)
            .ok_or_else(|| "that workspace is gone".to_string())?;
        let path = roots::resolve_path(&workspace.cwd, &request.path);
        let file = self.admit(&workspace, &path).await?;
        self.open_file(&request, &file)
    }

    /// Opens an inspected file in its workspace's preview.
    pub fn open_file(
        &self,
        request: &PreviewRequest,
        file: &PreviewFile,
    ) -> Result<String, String> {
        let what = match (file.kind, FileKind::of(file.kind)) {
            (Kind::Model, _) => {
                // The cap a kit's `preview.show` had (`showFile`): core's
                // inspection holds models to it already, and this is what is
                // said when a file is handed here that it did not inspect.
                if file.size > model::MAX_MODEL_BYTES {
                    return Err(format!("{} is too large for the preview", file.name));
                }
                let format = model::model_format_of(&file.path).ok_or_else(|| {
                    format!(
                        "{} is not a 3D model the preview can open. Formats: {}.",
                        file.name,
                        model::MODEL_FORMATS.join(", ")
                    )
                })?;
                What::Model {
                    format: format.into(),
                }
            }
            (_, Some(kind)) => What::File {
                kind,
                mime: file.mime.clone(),
                size: file.size,
            },
            (_, None) => return Err("The preview has no viewer for that file.".into()),
        };
        let shown = ShowFile {
            workspace_id: request.ws.clone(),
            path: file.path.clone(),
            title: request.title.clone(),
            opened_by: request.opened_by.clone(),
            modified_ms: file.modified_ms,
            what,
        };
        Ok(self.edit(|store| store.show_file_with(shown, request.reveal)))
    }

    /// Whether the workspace's preview has the entry open.
    pub fn owns(&self, workspace: &str, id: &str) -> bool {
        self.read(|store| store.scope(workspace).entry(id).is_some())
    }

    /// The entry once it has loaded or failed, or `None` past `timeout`, or
    /// when it is not open in that workspace (`whenSettled`).
    pub fn settle(&self, ws: &WsId, id: &str, timeout: Duration) -> BoxFuture<Option<Entry>> {
        let Some(me) = self.me.upgrade() else {
            return Box::pin(std::future::ready(None));
        };
        let (ws, id) = (ws.clone(), id.to_string());
        let mut changes = self.changes.subscribe();
        Box::pin(async move {
            let deadline = tokio::time::Instant::now() + timeout;
            loop {
                let settled = me.read(|store| {
                    let entry = store.scope(&ws).entry(&id)?;
                    Some((entry.status != Status::Loading).then(|| entry.clone()))
                });
                match settled {
                    None => return None,
                    Some(Some(entry)) => return Some(entry),
                    Some(None) => {}
                }
                match tokio::time::timeout_at(deadline, changes.changed()).await {
                    Ok(Ok(())) => {}
                    _ => return None,
                }
            }
        })
    }
}

/// An entry as an engine sees it.
pub fn preview_entry(entry: &Entry) -> PreviewEntry {
    PreviewEntry {
        id: entry.id.clone(),
        kind: entry.kind_name().to_string(),
        path: entry.path.clone(),
        title: entry.title.clone(),
        opened_by: entry.opened_by.clone(),
        loading: entry.status == Status::Loading,
        error: entry.error.clone(),
    }
}

/// Runs `future` to its end from a thread that is not an async one, which is
/// what the engines' synchronous calls into the preview are: a worker of the
/// multi-threaded runtime lends its thread, anything else gets a runtime of
/// its own for the time it takes.
fn block_on<T: Send + 'static>(future: impl Future<Output = T> + Send + 'static) -> T {
    use tokio::runtime::{Builder, Handle, RuntimeFlavor};
    match Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(|| handle.block_on(future))
        }
        _ => std::thread::spawn(move || {
            Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("a runtime starts")
                .block_on(future)
        })
        .join()
        .expect("the thread ends"),
    }
}

impl PreviewAccess for Shared {
    fn list(&self, ws: &WsId) -> Vec<PreviewEntry> {
        self.read(|store| {
            store
                .scope(ws)
                .entries
                .iter()
                .filter(|entry| !entry.is_kit_tab())
                .map(preview_entry)
                .collect()
        })
    }

    fn show(&self, request: PreviewRequest) -> Result<String, String> {
        let Some(me) = self.me.upgrade() else {
            return Err("the preview is closing".into());
        };
        block_on(async move { me.show_request(request).await })
    }

    fn reveal(&self, ws: &WsId, id: &str) -> bool {
        self.edit(|store| {
            if store.scope(ws).entry(id).is_none() {
                return false;
            }
            store.activate_model(id);
            true
        })
    }

    fn reload(&self, ws: &WsId, id: &str) -> bool {
        let Some(path) = self.read(|store| {
            store
                .scope(ws)
                .entry(id)
                .filter(|entry| !entry.is_kit_tab())
                .map(|entry| entry.path.clone())
        }) else {
            return false;
        };
        // What the file's modification time is now, so that the watch does not
        // load the same version again.
        let modified = std::fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |since| since.as_millis() as u64);
        self.edit(|store| store.reload_model(id, modified, None));
        true
    }

    fn close(&self, ws: &WsId, id: Option<&str>) -> usize {
        self.edit(|store| match id {
            Some(id) if store.scope(ws).entry(id).is_some() => store.close_models(&[id.into()]),
            Some(_) => 0,
            None => store.close_workspace_models(ws),
        })
    }

    fn when_settled(
        &self,
        ws: &WsId,
        id: &str,
        timeout: Duration,
    ) -> BoxFuture<Option<PreviewEntry>> {
        let settled = self.settle(ws, id, timeout);
        Box::pin(async move { settled.await.map(|entry| preview_entry(&entry)) })
    }

    fn snapshot(
        &self,
        ws: &WsId,
        id: &str,
        angle: SnapshotAngle,
        size: (u32, u32),
    ) -> BoxFuture<Result<Vec<u8>, String>> {
        let Some(me) = self.me.upgrade() else {
            return Box::pin(std::future::ready(Err("the preview is closing".into())));
        };
        // Never another workspace's model, whatever id it is asked for.
        if !self.owns(ws, id) {
            return Box::pin(std::future::ready(Err(NOT_HERE.into())));
        }
        let id = id.to_string();
        Box::pin(async move {
            // The 3D view may be mounting, for its first model or after the
            // window changed what it shows: give it the time to register its
            // renderer instead of failing a request it can carry out.
            let mount_wait = *me
                .mount_wait
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let mut waited = Duration::ZERO;
            while !me.has_snapshotter() && waited < mount_wait {
                tokio::time::sleep(MOUNT_POLL).await;
                waited += MOUNT_POLL;
            }
            match me.current_snapshotter() {
                Some(snapshotter) => snapshotter.snapshot(&id, angle, size).await,
                None => Err("the 3D preview is not mounted".into()),
            }
        })
    }

    #[cfg(test)]
    fn tabs(&self, ws: &WsId) -> Vec<PreviewEntry> {
        self.read(|store| {
            store
                .scope(ws)
                .entries
                .iter()
                .filter(|entry| entry.is_kit_tab())
                .map(preview_entry)
                .collect()
        })
    }
}

/* ── kit-tabs.json ─────────────────────────────────────────────── */

/// A workspace's kit tabs in the file `<app data>/kit-tabs.json`, an object
/// by workspace id. It is read on the first ask and written whole, through a
/// temporary file, whenever a workspace's tabs change: by a thread of its
/// own, never the caller's (the store is edited on the UI thread), the
/// changes of [`WRITE_AFTER`] taken together. What is waiting is written when
/// it is dropped.
pub struct FileSavedTabs {
    path: PathBuf,
    held: Mutex<Option<Map<String, Value>>>,
    /// What was last handed to the disk, to write only what changed.
    written: Mutex<String>,
    disk: Arc<Disk>,
    writer: Option<std::thread::JoinHandle<()>>,
}

/// How long a change waits for the next before the file is written.
pub const WRITE_AFTER: Duration = Duration::from_millis(300);

/// What the writer thread is to write.
struct Disk {
    path: PathBuf,
    pending: Mutex<Pending>,
    wake: std::sync::Condvar,
}

#[derive(Default)]
struct Pending {
    text: Option<String>,
    closing: bool,
}

impl Disk {
    fn lock(&self) -> std::sync::MutexGuard<'_, Pending> {
        self.pending
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn write(&self, text: &str) {
        // The tabs stay open if the disk refuses; only a restart loses them.
        if let Err(err) = write_atomically(&self.path, text) {
            eprintln!(
                "hangar: the kit tabs could not be saved to {}: {err}",
                self.path.display()
            );
        }
    }

    /// The writer: waits for a change, then for the ones that follow it
    /// within [`WRITE_AFTER`], and writes the last.
    fn run(&self) {
        let mut pending = self.lock();
        loop {
            while pending.text.is_none() && !pending.closing {
                pending = self
                    .wake
                    .wait(pending)
                    .unwrap_or_else(|poison| poison.into_inner());
            }
            if !pending.closing {
                let deadline = std::time::Instant::now() + WRITE_AFTER;
                while !pending.closing {
                    let left = deadline.saturating_duration_since(std::time::Instant::now());
                    if left.is_zero() {
                        break;
                    }
                    pending = self
                        .wake
                        .wait_timeout(pending, left)
                        .unwrap_or_else(|poison| poison.into_inner())
                        .0;
                }
            }
            let text = pending.text.take();
            let closing = pending.closing;
            drop(pending);
            if let Some(text) = text {
                self.write(&text);
            }
            if closing {
                return;
            }
            pending = self.lock();
        }
    }
}

impl FileSavedTabs {
    pub fn new(path: PathBuf) -> Self {
        let disk = Arc::new(Disk {
            path: path.clone(),
            pending: Mutex::new(Pending::default()),
            wake: std::sync::Condvar::new(),
        });
        let writer = std::thread::Builder::new()
            .name("hangar-kit-tabs".into())
            .spawn({
                let disk = disk.clone();
                move || disk.run()
            })
            .ok();
        FileSavedTabs {
            path,
            held: Mutex::new(None),
            written: Mutex::new(String::new()),
            disk,
            writer,
        }
    }

    /// Hands the text to the writer, or writes it here without one.
    fn hand_over(&self, text: String) {
        if self.writer.is_none() {
            self.disk.write(&text);
            return;
        }
        self.disk.lock().text = Some(text);
        self.disk.wake.notify_one();
    }

    fn with_map<T>(&self, use_map: impl FnOnce(&mut Map<String, Value>) -> T) -> T {
        let mut held = self
            .held
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let map = held.get_or_insert_with(|| {
            let read = std::fs::read_to_string(&self.path).ok();
            // What the file held when it was read: the next write that
            // changes nothing leaves the file alone.
            let map = read
                .as_deref()
                .and_then(|text| serde_json::from_str::<Value>(text).ok())
                .and_then(|value| match value {
                    Value::Object(map) => Some(map),
                    _ => None,
                })
                .unwrap_or_default();
            *self
                .written
                .lock()
                .unwrap_or_else(|poison| poison.into_inner()) =
                serde_json::to_string_pretty(&map).unwrap_or_default();
            map
        });
        use_map(map)
    }
}

impl SavedTabs for FileSavedTabs {
    fn read(&self, workspace: &str) -> Option<Value> {
        self.with_map(|map| map.get(workspace).cloned())
    }

    fn write(&mut self, workspace: &str, tabs: Option<&[SavedKitTab]>) {
        let text = self.with_map(|map| {
            match tabs {
                Some(tabs) => {
                    map.insert(
                        workspace.to_string(),
                        serde_json::to_value(tabs).unwrap_or(Value::Null),
                    );
                }
                None => {
                    map.remove(workspace);
                }
            }
            serde_json::to_string_pretty(map).unwrap_or_default()
        });
        let mut written = self
            .written
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if *written == text {
            return;
        }
        *written = text.clone();
        drop(written);
        self.hand_over(text);
    }

    fn workspaces(&self) -> Vec<String> {
        self.with_map(|map| map.keys().cloned().collect())
    }
}

impl Drop for FileSavedTabs {
    fn drop(&mut self) {
        self.disk.lock().closing = true;
        self.disk.wake.notify_one();
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

fn write_atomically(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, text)?;
    std::fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::preview::store::{KitTabToOpen, ShowFile, What};

    fn shared() -> Arc<Shared> {
        Shared::new(None)
    }

    fn model_file(path: &str) -> ShowFile {
        ShowFile {
            workspace_id: "w1".into(),
            path: path.into(),
            title: None,
            opened_by: Some("Eve".into()),
            modified_ms: 1,
            what: What::Model {
                format: "glb".into(),
            },
        }
    }

    fn inspected(kind: Kind, name: &str, size: u64) -> PreviewFile {
        PreviewFile {
            path: format!("/w/{name}"),
            kind,
            mime: "application/octet-stream".into(),
            size,
            name: name.into(),
            modified_ms: 1,
        }
    }

    fn request() -> PreviewRequest {
        PreviewRequest {
            ws: "w1".into(),
            path: "x".into(),
            title: None,
            opened_by: None,
            reveal: true,
        }
    }

    #[test]
    fn a_model_over_the_cap_is_too_large_for_the_preview_and_one_at_it_opens() {
        let shared = shared();
        let big = inspected(Kind::Model, "city.glb", model::MAX_MODEL_BYTES + 1);
        let why = shared.open_file(&request(), &big).unwrap_err();
        assert_eq!(why, "city.glb is too large for the preview");
        assert!(shared.read(|store| store.scope("w1").entries.is_empty()));
        let fits = inspected(Kind::Model, "city.glb", model::MAX_MODEL_BYTES);
        assert!(shared.open_file(&request(), &fits).is_ok());
        // What is not a model has its own caps, which core checked.
        let page = inspected(Kind::Text, "log.txt", model::MAX_MODEL_BYTES + 1);
        assert!(shared.open_file(&request(), &page).is_ok());
    }

    #[test]
    fn a_change_wakes_those_who_follow_the_store() {
        let shared = shared();
        let mut changes = shared.follow();
        assert!(!changes.has_changed().unwrap());
        shared.edit(|store| store.show_file(model_file("C:/m/chair.glb")));
        assert!(changes.has_changed().unwrap());
        changes.mark_unchanged();
        // Reading, or asking for nothing, changes nothing.
        shared.read(|store| store.revision());
        shared.edit(|store| store.close_model("no-such-entry"));
        assert!(!changes.has_changed().unwrap());
    }

    #[test]
    fn lists_and_closes_only_the_workspaces_own_files() {
        let shared = shared();
        let mine = shared.edit(|store| store.show_file(model_file("C:/dev/hangar/robot.glb")));
        let mut other = model_file("C:/dev/shop/chair.glb");
        other.workspace_id = "w2".into();
        let theirs = shared.edit(|store| store.show_file(other));
        let w1 = "w1".to_string();
        let ids: Vec<String> = shared.list(&w1).into_iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![mine.clone()]);
        // Never another workspace's file, whatever id it is asked for.
        assert!(!shared.reveal(&w1, &theirs));
        assert_eq!(shared.close(&w1, Some(&theirs)), 0);
        assert_eq!(shared.close(&w1, None), 1);
        assert!(shared.list(&w1).is_empty());
        assert_eq!(shared.list(&"w2".to_string()).len(), 1);
    }

    #[test]
    fn opening_in_its_own_workspace_whatever_is_on_screen_and_saying_what_is() {
        let shared = shared();
        shared.edit(|store| store.note_screens([("w1".into(), true)]));
        let id = shared.edit(|store| {
            let mut file = model_file("C:/dev/shop/chair.glb");
            file.workspace_id = "w2".into();
            store.show_file(file)
        });
        let listed = shared.list_view("w2");
        assert_eq!(listed.active_id.as_deref(), Some(id.as_str()));
        assert!(!listed.on_screen);
        assert!(!listed.shown);
        shared.edit(|store| store.note_screens([("w2".into(), false)]));
        let listed = shared.list_view("w2");
        assert!(listed.on_screen);
        assert!(!listed.shown);
        // A workspace in a window of its own is on screen with the main
        // window's.
        shared.edit(|store| store.note_screens([("w1".into(), false), ("w2".into(), true)]));
        assert!(shared.list_view("w2").shown);
        assert!(shared.list_view("w1").on_screen);
        assert!(!shared.list_view("w1").shown);
        assert!(shared.list_view("w1").entries.is_empty());
    }

    #[test]
    fn keeps_the_kit_tabs_out_of_the_files_list() {
        let shared = shared();
        shared.edit(|store| store.show_file(model_file("C:/dev/hangar/robot.glb")));
        shared.edit(|store| store.open_kit_tab(KitTabToOpen::new("w1", "ci", "run", "Run")));
        let w1 = "w1".to_string();
        assert_eq!(shared.list(&w1).len(), 1);
        let tabs = shared.tabs(&w1);
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].kind, "kit");
    }

    #[tokio::test]
    async fn tells_a_waiting_caller_how_the_load_ended() {
        let shared = shared();
        let id = shared.edit(|store| store.show_file(model_file("C:/m/a.glb")));
        let waiting = shared.when_settled(&"w1".to_string(), &id, Duration::from_secs(5));
        let settle = {
            let shared = shared.clone();
            let id = id.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(20)).await;
                shared.edit(|store| store.settle_model(&id, Err("not a glTF file".into())));
            })
        };
        let settled = waiting.await.expect("it settled");
        settle.await.unwrap();
        assert_eq!(settled.error.as_deref(), Some("not a glTF file"));
        assert!(!settled.loading);
    }

    #[tokio::test]
    async fn gives_up_on_a_load_that_does_not_end_and_on_a_file_that_is_not_open() {
        let shared = shared();
        let id = shared.edit(|store| store.show_file(model_file("C:/m/a.glb")));
        let w1 = "w1".to_string();
        assert!(
            shared
                .when_settled(&w1, &id, Duration::from_millis(30))
                .await
                .is_none()
        );
        assert!(
            shared
                .when_settled(&"w2".to_string(), &id, Duration::from_secs(1))
                .await
                .is_none(),
            "another workspace's file is not this one's to wait for"
        );
    }

    struct Fake;

    impl Snapshotter for Fake {
        fn snapshot(
            &self,
            id: &str,
            angle: SnapshotAngle,
            size: (u32, u32),
        ) -> BoxFuture<Result<Vec<u8>, String>> {
            let answer = format!("{id}:{angle:?}:{}x{}", size.0, size.1).into_bytes();
            Box::pin(std::future::ready(Ok(answer)))
        }
    }

    #[tokio::test]
    async fn gives_a_view_that_is_mounting_the_time_to_register_its_renderer() {
        let shared = shared();
        let id = shared.edit(|store| store.show_file(model_file("C:/m/a.glb")));
        let w1 = "w1".to_string();
        let pending = shared.snapshot(&w1, &id, SnapshotAngle::Front, (64, 64));
        let register = {
            let shared = shared.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(120)).await;
                shared.register_snapshotter(Some(Arc::new(Fake)));
            })
        };
        let png = pending.await.expect("rendered");
        register.await.unwrap();
        assert_eq!(png, format!("{id}:Front:64x64").into_bytes());
    }

    #[tokio::test]
    async fn says_the_view_is_not_mounted_when_it_never_is_and_refuses_another_workspaces_model() {
        let shared = shared();
        shared.wait_for_mount(Duration::from_millis(100));
        let id = shared.edit(|store| store.show_file(model_file("C:/m/a.glb")));
        let failure = shared
            .snapshot(&"w1".to_string(), &id, SnapshotAngle::Iso, (64, 64))
            .await
            .unwrap_err();
        assert!(failure.contains("not mounted"), "{failure}");
        let refused = shared
            .snapshot(&"w2".to_string(), &id, SnapshotAngle::Iso, (64, 64))
            .await
            .unwrap_err();
        assert!(refused.contains("not open in this workspace"), "{refused}");
    }

    #[test]
    fn writes_a_workspaces_kit_tabs_to_a_file_and_reads_them_back() {
        let dir = crate::kit_tabs::testdir::scratch("kit-tabs");
        let path = dir.join("kit-tabs.json");
        {
            let shared = Shared::new(Some(Box::new(FileSavedTabs::new(path.clone()))));
            shared.edit(|store| {
                store.open_kit_tab(
                    KitTabToOpen::new("w1", "ci", "run", "Run")
                        .params(&[("run", "4567")])
                        .opened_by("Ava"),
                )
            });
        }
        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written["w1"][0]["kitId"], "ci");
        assert_eq!(written["w1"][0]["params"]["run"], "4567");

        let shared = Shared::new(Some(Box::new(FileSavedTabs::new(path.clone()))));
        shared.edit(|store| store.restore_kit_tabs("w1"));
        let restored = shared.read(|store| store.scope("w1").entries.clone());
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].opened_by.as_deref(), Some("Ava"));

        // Closing the last tab leaves nothing for the workspace.
        shared.edit(|store| store.close_workspace_models("w1"));
        drop(shared);
        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(written.get("w1").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_that_is_not_json_is_no_saved_tab() {
        let dir = crate::kit_tabs::testdir::scratch("kit-tabs-bad");
        let path = dir.join("kit-tabs.json");
        std::fs::write(&path, "{ not json").unwrap();
        let saved = FileSavedTabs::new(path);
        assert_eq!(saved.read("w1"), None);
        assert!(saved.workspaces().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn kit_tabs_are_written_off_the_callers_thread_the_last_of_a_burst_once_dropped_at_the_latest()
    {
        let dir = crate::kit_tabs::testdir::scratch("kit-tabs-burst");
        let path = dir.join("kit-tabs.json");
        let tab = |title: &str| crate::preview::store::SavedKitTab {
            id: Some("p1".into()),
            kit_id: "ci".into(),
            tab_id: "run".into(),
            path: "hangar-kit://ci/run".into(),
            title: title.into(),
            titled: true,
            params: Default::default(),
            opened_by: None,
        };
        let mut saved = FileSavedTabs::new(path.clone());
        for n in 0..20 {
            saved.write("w1", Some(&[tab(&format!("Run #{n}"))]));
        }
        // The caller did not wait on the disk: the burst is still waiting.
        assert!(!path.exists(), "written on the caller's thread");
        drop(saved);
        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written["w1"][0]["title"], "Run #19");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn kit_tabs_are_written_soon_after_a_change_without_a_drop() {
        let dir = crate::kit_tabs::testdir::scratch("kit-tabs-soon");
        let path = dir.join("kit-tabs.json");
        let mut saved = FileSavedTabs::new(path.clone());
        saved.write("w1", None);
        saved.write("w2", Some(&[]));
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !path.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(written.get("w2").is_some());
        drop(saved);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
