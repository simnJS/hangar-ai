//! The port the tests run the operations against: an app in memory that
//! logs what was done the way control.test.ts's fakes did (`open:w2`,
//! `touch:w1`, `announce:created:Reviewer:Ava`), so the TS cases port one to
//! one.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

use serde_json::{Map, Value, json};
use tokio::task::JoinHandle;

use hangar_core::git::Worktree;
use hangar_core::kits::KitInfo;

use crate::state::{
    AgentId, AppState, Pane, PaneActivity, PaneId, Snapshot, SplitDir, Workspace, WsId,
};

use super::delivery::Deliveries;
use super::paths::basename;
use super::port::{
    AppPort, BoxFuture, CmdOut, DeliveryDeps, EntryKind, EntryStatus, FromGit, KitTabCatalog,
    ModelCmd, ModelFileInfo, OpenEntry, OrchestrationHooks, PaneTerminal, Port, PreviewHost,
    PreviewListing, Showing, SnapshotAngle,
};
use super::types::ControlEvent;

pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn ready<T: Send + 'static>(value: T) -> BoxFuture<T> {
    Box::pin(std::future::ready(value))
}

/// `pane(id, name, agent, cwd)` of control.test.ts: a session to resume.
pub fn pane(id: &str, name: &str, agent: AgentId, cwd: Option<&str>) -> Pane {
    Pane {
        id: id.into(),
        name: name.into(),
        agent,
        session_id: Some(format!("s-{id}")),
        cwd: cwd.map(str::to_string),
        ..Pane::default()
    }
}

pub fn workspace(id: &str, name: &str, cwd: &str, panes: Vec<Pane>) -> Workspace {
    Workspace {
        id: id.into(),
        name: name.into(),
        cwd: cwd.into(),
        panes,
        ..Workspace::default()
    }
}

/// What the viewer found in a model (`INFO`).
pub fn info() -> Value {
    json!({"triangles": 1284, "vertices": 702, "meshes": 3, "materials": 2,
           "animations": ["Idle"], "size": [2, 1, 3], "center": [0, 0.5, 0]})
}

/// A model open in the preview, loaded (`openModel` of the tests).
pub fn open_model(id: &str, path: &str, ws: &str) -> (WsId, OpenEntry) {
    (
        ws.into(),
        OpenEntry {
            id: id.into(),
            path: path.into(),
            title: basename(path),
            opened_by: Some("Ava".into()),
            modified_ms: 100,
            status: EntryStatus::Ready,
            error: None,
            what: EntryKind::Model {
                format: "glb".into(),
                info: Some(info()),
            },
        },
    )
}

/// A file open in the preview (`openFile` of the tests).
pub fn open_file(id: &str, path: &str, kind: &str, ws: &str) -> (WsId, OpenEntry) {
    (
        ws.into(),
        OpenEntry {
            id: id.into(),
            path: path.into(),
            title: basename(path),
            opened_by: Some("Ava".into()),
            modified_ms: 100,
            status: EntryStatus::Ready,
            error: None,
            what: EntryKind::File {
                kind: kind.into(),
                mime: mime_of(kind).into(),
                size: 4096,
            },
        },
    )
}

/// What core's `preview_inspect` says a file is, by its extension.
fn kind_of(extension: &str) -> Option<&'static str> {
    Some(match extension {
        "png" | "gif" => "image",
        "mp4" => "video",
        "ogg" => "audio",
        "txt" | "json" | "log" => "text",
        "md" => "markdown",
        "pdf" => "pdf",
        "html" => "html",
        "glb" | "gltf" => "model",
        // A kind this window has no viewer for.
        "xyz" => "binary",
        _ => return None,
    })
}

fn mime_of(kind: &str) -> &'static str {
    match kind {
        "image" => "image/png",
        "video" => "video/mp4",
        "audio" => "audio/ogg",
        "text" | "markdown" => "text/plain",
        "pdf" => "application/pdf",
        "html" => "text/html",
        "model" => "model/gltf-binary",
        _ => "application/octet-stream",
    }
}

/// How a load the preview waits on ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settle {
    Ready,
    Error,
    Timeout,
}

/// A window's previews in memory, reached through the workspace the host
/// was asked for: it lists that workspace's entries only.
pub struct PreviewState {
    pub entries: Vec<(WsId, OpenEntry)>,
    pub active: Option<String>,
    /// The workspace the last host was asked for.
    pub workspace: WsId,
    /// Its workspace on screen, and its grid with it.
    pub on_screen: bool,
    pub shown: bool,
    pub settle: Settle,
    pub snapshot_error: Option<String>,
    next: u32,
}

pub struct FakePreview {
    pub state: Mutex<PreviewState>,
    log: Arc<Mutex<Vec<String>>>,
}

/// The PNG signature: what a snapshot renders, `iVBORw0KGgo=` in base64.
pub const PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

fn angle_name(angle: SnapshotAngle) -> &'static str {
    match angle {
        SnapshotAngle::Front => "front",
        SnapshotAngle::Back => "back",
        SnapshotAngle::Left => "left",
        SnapshotAngle::Right => "right",
        SnapshotAngle::Top => "top",
        SnapshotAngle::Bottom => "bottom",
        SnapshotAngle::Iso => "iso",
    }
}

impl FakePreview {
    pub fn set(&self, entries: Vec<(WsId, OpenEntry)>) {
        lock(&self.state).entries = entries;
    }

    pub fn ids(&self) -> Vec<String> {
        lock(&self.state)
            .entries
            .iter()
            .map(|(_, entry)| entry.id.clone())
            .collect()
    }

    /// The kit tabs open in a workspace.
    pub fn tabs(&self, ws: &str) -> Vec<OpenEntry> {
        lock(&self.state)
            .entries
            .iter()
            .filter(|(id, entry)| id == ws && entry.is_kit_tab())
            .map(|(_, entry)| entry.clone())
            .collect()
    }

    fn log(&self, line: String) {
        lock(&self.log).push(line);
    }
}

impl PreviewHost for FakePreview {
    fn list(&self) -> BoxFuture<Result<PreviewListing, String>> {
        let state = lock(&self.state);
        ready(Ok(PreviewListing {
            entries: state
                .entries
                .iter()
                .filter(|(ws, _)| *ws == state.workspace)
                .map(|(_, entry)| entry.clone())
                .collect(),
            active_id: state.active.clone(),
            on_screen: state.on_screen,
            shown: state.shown,
        }))
    }

    fn show(&self, entry: Showing) -> BoxFuture<Result<String, String>> {
        let mut state = lock(&self.state);
        let workspace = state.workspace.clone();
        let (title, opened_by) = match &entry {
            Showing::Model {
                title, opened_by, ..
            }
            | Showing::File {
                title, opened_by, ..
            }
            | Showing::Kit {
                title, opened_by, ..
            } => (title.clone(), opened_by.clone()),
        };
        self.log(format!(
            "show:{}:{title}:{}:{workspace}",
            entry.path(),
            opened_by.as_deref().unwrap_or("null")
        ));
        if let Showing::Kit {
            path,
            kit_id,
            tab_id,
            title,
            params,
            singleton,
            opened_by,
            reveal,
        } = entry
        {
            // As the preview store keeps kit tabs: a singleton's one tab
            // takes the new params, any other is one tab per set of them.
            let existing = state.entries.iter_mut().find(|(ws, open)| {
                *ws == workspace
                    && match &open.what {
                        EntryKind::Kit {
                            kit_id: kit,
                            tab_id: tab,
                            ..
                        } if singleton => *kit == kit_id && *tab == tab_id,
                        EntryKind::Kit { .. } => open.path == path,
                        _ => false,
                    }
            });
            let id = match existing {
                Some((_, open)) => {
                    open.opened_by = opened_by;
                    if singleton && !params.is_empty() {
                        open.what = EntryKind::Kit {
                            kit_id,
                            tab_id,
                            params,
                        };
                    }
                    open.id.clone()
                }
                None => {
                    state.next += 1;
                    let id = format!("k{}", state.next);
                    state.entries.push((
                        workspace,
                        OpenEntry {
                            id: id.clone(),
                            path,
                            title,
                            opened_by,
                            modified_ms: 0,
                            status: EntryStatus::Ready,
                            error: None,
                            what: EntryKind::Kit {
                                kit_id,
                                tab_id,
                                params,
                            },
                        },
                    ));
                    id
                }
            };
            if reveal {
                state.active = Some(id.clone());
            }
            return ready(Ok(id));
        }
        let path = entry.path().to_string();
        let existing = state
            .entries
            .iter()
            .find(|(ws, open)| open.path == path && *ws == workspace)
            .map(|(_, open)| open.id.clone());
        let id = match existing {
            Some(id) => id,
            None => {
                state.next += 1;
                let model = matches!(entry, Showing::Model { .. });
                let id = format!("{}{}", if model { "m" } else { "f" }, state.next);
                let open = match entry {
                    Showing::Model { opened_by, .. } => OpenEntry {
                        id: id.clone(),
                        title: basename(&path),
                        path,
                        opened_by,
                        modified_ms: 100,
                        status: EntryStatus::Loading,
                        error: None,
                        what: EntryKind::Model {
                            format: "glb".into(),
                            info: None,
                        },
                    },
                    Showing::File {
                        kind,
                        title,
                        mime,
                        size,
                        opened_by,
                        modified_ms,
                        ..
                    } => OpenEntry {
                        id: id.clone(),
                        path,
                        title,
                        opened_by,
                        modified_ms,
                        status: EntryStatus::Ready,
                        error: None,
                        what: EntryKind::File { kind, mime, size },
                    },
                    Showing::Kit { .. } => unreachable!("kit tabs are shown above"),
                };
                state.entries.push((workspace, open));
                id
            }
        };
        state.active = Some(id.clone());
        ready(Ok(id))
    }

    fn reveal(&self, id: &str) -> BoxFuture<()> {
        self.log(format!("reveal:{id}"));
        lock(&self.state).active = Some(id.into());
        ready(())
    }

    fn reload(&self, id: &str, modified_ms: u64) -> BoxFuture<()> {
        self.log(format!("reload:{id}:{modified_ms}"));
        ready(())
    }

    fn when_settled(&self, id: &str, ms: u64) -> BoxFuture<Option<OpenEntry>> {
        self.log(format!("settle:{id}:{ms}"));
        let state = lock(&self.state);
        let Some((_, entry)) = state.entries.iter().find(|(_, entry)| entry.id == id) else {
            return ready(None);
        };
        // A file has nothing to load: it is ready as soon as it is open.
        if !entry.is_model() {
            return ready(Some(entry.clone()));
        }
        let mut entry = entry.clone();
        match state.settle {
            Settle::Timeout => return ready(None),
            Settle::Error => {
                entry.status = EntryStatus::Error;
                entry.error = Some("not a glTF file".into());
            }
            Settle::Ready => {
                entry.status = EntryStatus::Ready;
                if let EntryKind::Model { info: held, .. } = &mut entry.what {
                    *held = Some(info());
                }
            }
        }
        ready(Some(entry))
    }

    fn snapshot(
        &self,
        id: &str,
        angle: SnapshotAngle,
        width: u32,
        height: u32,
    ) -> BoxFuture<Result<Vec<u8>, String>> {
        self.log(format!(
            "snapshot:{id}:{}:{width}x{height}",
            angle_name(angle)
        ));
        let state = lock(&self.state);
        ready(match &state.snapshot_error {
            Some(error) => Err(error.clone()),
            None => Ok(PNG.to_vec()),
        })
    }

    fn close(&self, ids: Vec<String>) -> BoxFuture<()> {
        self.log(format!("closeModels:{}", ids.join(",")));
        lock(&self.state)
            .entries
            .retain(|(_, entry)| !ids.contains(&entry.id));
        ready(())
    }
}

/// A terminal that shows `$ ls` then `a.txt`, and records what is pasted.
struct FakeTerminal {
    typed: Arc<Mutex<Vec<String>>>,
}

impl PaneTerminal for FakeTerminal {
    fn paste(&self, text: &str) -> BoxFuture<Result<(), String>> {
        lock(&self.typed).push(format!("paste:{text}"));
        ready(Ok(()))
    }

    fn read(&self, n: usize) -> Vec<String> {
        let rows = ["$ ls", "a.txt"];
        rows[rows.len().saturating_sub(n)..]
            .iter()
            .map(|row| row.to_string())
            .collect()
    }

    fn application_cursor(&self) -> bool {
        false
    }
}

type StatFn = Box<dyn Fn(&str) -> Option<Result<ModelFileInfo, String>> + Send + Sync>;
type TreesFn = Box<dyn Fn(&str) -> Result<Vec<Worktree>, String> + Send + Sync>;

/// An app with two workspaces, the first one open, and a log of what was
/// done (`setup()` of control.test.ts).
pub struct Fake {
    pub state: Mutex<AppState>,
    pub opened: Mutex<HashSet<WsId>>,
    /// Put to sleep for being left alone.
    pub asleep: Mutex<HashSet<WsId>>,
    pub activity: Mutex<HashMap<PaneId, PaneActivity>>,
    pub log: Arc<Mutex<Vec<String>>>,
    pub typed: Arc<Mutex<Vec<String>>>,
    /// Who each queued prompt is from, as the session will read it.
    pub senders: Mutex<Vec<String>>,
    next: Mutex<u32>,
    allowed: Mutex<Vec<String>>,
    pub mtimes: Mutex<HashMap<String, u64>>,
    pub preview: Arc<FakePreview>,
    /// The window drawing the preview answers.
    pub host_answers: Mutex<bool>,
    /// The panes that have a terminal.
    pub terminals: Mutex<Vec<PaneId>>,
    pub stat: Mutex<Option<StatFn>>,
    /// Another `git worktree list`, and the folders it was asked about.
    pub trees: Mutex<Option<TreesFn>>,
    pub listed: Mutex<Vec<String>>,
    pub from_git: Mutex<Option<Arc<dyn FromGit>>>,
    pub catalog: Mutex<Option<Arc<dyn KitTabCatalog>>>,
    pub installed: Mutex<Vec<KitInfo>>,
    pub delivery: Mutex<Option<Arc<dyn DeliveryDeps>>>,
    pub engine: Mutex<Option<Arc<dyn OrchestrationHooks>>>,
    deliveries: Deliveries,
    spawned: Mutex<Vec<JoinHandle<()>>>,
}

impl Fake {
    /// Two workspaces: "Hangar" with Ava (Claude Code) and Max (a shell),
    /// open and on screen; "Shop" with another Ava (a shell), not open.
    pub fn setup() -> Arc<Fake> {
        Fake::with_state(AppState {
            workspaces: vec![
                workspace(
                    "w1",
                    "Hangar",
                    "C:\\dev\\hangar",
                    vec![
                        pane("p1", "Ava", AgentId::Claude, None),
                        pane("p2", "Max", AgentId::Shell, None),
                    ],
                ),
                workspace(
                    "w2",
                    "Shop",
                    "C:\\dev\\shop",
                    vec![pane("p3", "Ava", AgentId::Shell, None)],
                ),
            ],
            active_workspace_id: Some("w1".into()),
            ..AppState::default()
        })
    }

    pub fn with_state(state: AppState) -> Arc<Fake> {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut activity = HashMap::new();
        activity.insert(
            "p1".to_string(),
            PaneActivity {
                workspace_id: "w1".into(),
                activity: crate::state::AgentActivity::Working,
                bridged: true,
            },
        );
        Arc::new(Fake {
            state: Mutex::new(state),
            opened: Mutex::new(HashSet::from(["w1".to_string()])),
            asleep: Mutex::new(HashSet::new()),
            activity: Mutex::new(activity),
            log: log.clone(),
            typed: Arc::new(Mutex::new(Vec::new())),
            senders: Mutex::new(Vec::new()),
            next: Mutex::new(10),
            allowed: Mutex::new(Vec::new()),
            mtimes: Mutex::new(HashMap::new()),
            preview: Arc::new(FakePreview {
                state: Mutex::new(PreviewState {
                    entries: Vec::new(),
                    active: None,
                    workspace: "w1".into(),
                    on_screen: true,
                    shown: true,
                    settle: Settle::Ready,
                    snapshot_error: None,
                    next: 0,
                }),
                log,
            }),
            host_answers: Mutex::new(true),
            terminals: Mutex::new(vec!["p1".into(), "p2".into()]),
            stat: Mutex::new(None),
            trees: Mutex::new(None),
            listed: Mutex::new(Vec::new()),
            from_git: Mutex::new(None),
            catalog: Mutex::new(None),
            installed: Mutex::new(Vec::new()),
            delivery: Mutex::new(None),
            engine: Mutex::new(None),
            deliveries: Deliveries::default(),
            spawned: Mutex::new(Vec::new()),
        })
    }

    pub fn port(self: &Arc<Self>) -> Port {
        self.clone()
    }

    pub fn log(&self) -> Vec<String> {
        lock(&self.log).clone()
    }

    pub fn logged(&self, prefix: &str) -> Vec<String> {
        self.log()
            .into_iter()
            .filter(|line| line.starts_with(prefix))
            .collect()
    }

    pub fn typed(&self) -> Vec<String> {
        lock(&self.typed).clone()
    }

    /// Changes a workspace, as the tests' `patch(id, change)`.
    pub fn patch(&self, ws: &str, change: impl FnOnce(&mut Workspace)) {
        let mut state = lock(&self.state);
        if let Some(found) = state.workspaces.iter_mut().find(|entry| entry.id == ws) {
            change(found);
        }
    }

    pub fn grant(&self, roots: &[&str]) {
        *lock(&self.allowed) = roots.iter().map(|root| root.to_string()).collect();
    }

    /// Until everything run in the background has ended, what it started
    /// included.
    pub async fn settle(&self) {
        loop {
            let handles: Vec<JoinHandle<()>> = std::mem::take(&mut *lock(&self.spawned));
            if handles.is_empty() {
                return;
            }
            for handle in handles {
                handle.await.expect("a background task panicked");
            }
        }
    }

    fn push(&self, line: String) {
        lock(&self.log).push(line);
    }

    fn outside(&self, path: &str) -> bool {
        let lower = path.to_lowercase();
        !lock(&self.allowed)
            .iter()
            .any(|root| lower.starts_with(&root.to_lowercase()))
    }

    fn carry_out(&self, cmd: ModelCmd) -> CmdOut {
        let mut out = CmdOut::default();
        match cmd {
            ModelCmd::AddPane {
                ws,
                agent,
                near,
                dir,
                cwd,
                name,
                init,
            } => {
                let id = {
                    let mut next = lock(&self.next);
                    let id = format!("p{next}");
                    *next += 1;
                    id
                };
                // As JSON.stringify wrote the options: absent ones left out.
                let mut opts = Map::new();
                if let Some(agent) = agent {
                    opts.insert("agent".into(), json!(agent));
                }
                opts.insert("near".into(), json!(near));
                if let Some(dir) = dir {
                    opts.insert(
                        "dir".into(),
                        json!(if dir == SplitDir::Col { "col" } else { "row" }),
                    );
                }
                opts.insert("cwd".into(), json!(cwd));
                if let Some(name) = &name {
                    opts.insert("name".into(), json!(name));
                }
                self.push(format!("add:{ws}:{}", Value::Object(opts)));
                let mut added = pane(
                    &id,
                    name.as_deref().unwrap_or("Leo"),
                    agent.unwrap_or(AgentId::Shell),
                    cwd.as_deref(),
                );
                if let Some(init) = init {
                    init.apply(&mut added);
                }
                self.patch(&ws, |entry| entry.panes.push(added));
                out.pane = Some(id);
            }
            ModelCmd::ClosePane { ws, pane } => {
                self.push(format!("close:{ws}:{pane}"));
                self.patch(&ws, |entry| entry.panes.retain(|held| held.id != pane));
            }
            ModelCmd::RespawnPane {
                ws, pane, patch, ..
            } => {
                let before = lock(&self.state)
                    .workspaces
                    .iter()
                    .flat_map(|entry| &entry.panes)
                    .find(|held| held.id == pane)
                    .cloned()
                    .unwrap_or_default();
                let mut after = before.clone();
                patch.apply(&mut after);
                let mut changed = Map::new();
                if after.agent != before.agent {
                    changed.insert("agent".into(), json!(after.agent));
                }
                if after.session_id != before.session_id {
                    changed.insert("sessionId".into(), json!(after.session_id));
                }
                // serde_json sorts the keys; the TS wrote agent first too.
                self.push(format!("respawn:{ws}:{pane}:{}", Value::Object(changed)));
                out.pane = Some("fresh-id".into());
            }
            ModelCmd::UpdatePane { ws, pane, patch } => {
                self.push(format!("updatePane:{ws}:{pane}"));
                self.patch(&ws, |entry| {
                    if let Some(held) = entry.panes.iter_mut().find(|held| held.id == pane) {
                        patch.apply(held);
                    }
                });
            }
            ModelCmd::UpdateWorkspace { ws, patch } => {
                self.patch(&ws, |entry| patch.apply(entry));
                let orchestration = lock(&self.state)
                    .workspaces
                    .iter()
                    .find(|entry| entry.id == ws)
                    .map(|entry| json!(entry.orchestration))
                    .unwrap_or(Value::Null);
                self.push(format!("patch:{ws}:{orchestration}"));
            }
            ModelCmd::OpenWorkspace(ws) => {
                lock(&self.opened).insert(ws.clone());
                self.push(format!("open:{ws}"));
            }
            ModelCmd::Touch(ws) => self.push(format!("touch:{ws}")),
            ModelCmd::DevCommand { command, query, .. } => {
                self.push(format!("dev:{command:?}:{query:?}"))
            }
            ModelCmd::Activate { ws, pane } => self.push(format!("activate:{ws}:{pane}")),
            ModelCmd::Detach(ws) => self.push(format!("detach:{ws}")),
            ModelCmd::Reattach(ws) => self.push(format!("reattach:{ws}")),
            ModelCmd::Announce(event) => self.push(announced(&event)),
            ModelCmd::CreateWorkspace { draft, .. } => self.push(format!("create:{}", draft.name)),
            ModelCmd::RemoveWorkspace(ws) => self.push(format!("remove:{ws}")),
            ModelCmd::BeginDelivery { ws, task } => self.push(format!("begin:{ws}:{task}")),
            ModelCmd::EndDelivery { ws, task } => self.push(format!("end:{ws}:{task}")),
            ModelCmd::ViewsChanged { ws, arrange } => {
                self.push(format!(
                    "views:{ws}:{}",
                    if arrange { "arrange" } else { "keep" }
                ));
            }
        }
        out
    }
}

/// `announce:<kind>:<pane, file or command>:<by>`.
fn announced(event: &ControlEvent) -> String {
    format!(
        "announce:{}:{}:{}",
        event.kind(),
        event.subject(),
        event.by().unwrap_or("null")
    )
}

/// What `git worktree list` says of the fake repository.
pub fn worktree(path: &str, is_main: bool, prunable: Option<&str>) -> Worktree {
    Worktree {
        path: path.into(),
        head: Some("abc1234".into()),
        branch: Some("main".into()),
        is_main,
        prunable: prunable.map(str::to_string),
        ..Worktree::default()
    }
}

impl AppPort for Fake {
    fn snapshot(&self) -> Arc<Snapshot> {
        Arc::new(Snapshot {
            state: Arc::new(lock(&self.state).clone()),
            opened: lock(&self.opened).clone(),
            asleep: lock(&self.asleep).clone(),
            activity: lock(&self.activity).clone(),
            delivering: Default::default(),
            hydrated: true,
        })
    }

    fn apply(&self, cmd: ModelCmd) -> BoxFuture<Result<CmdOut, String>> {
        ready(Ok(self.carry_out(cmd)))
    }

    fn terminal(&self, pane: &PaneId) -> Option<Arc<dyn PaneTerminal>> {
        lock(&self.terminals).contains(pane).then(|| {
            Arc::new(FakeTerminal {
                typed: self.typed.clone(),
            }) as Arc<dyn PaneTerminal>
        })
    }

    fn write(&self, pane: &PaneId, data: &str) -> BoxFuture<Result<(), String>> {
        lock(&self.typed).push(format!("write:{pane}:{}", json!(data)));
        ready(Ok(()))
    }

    fn alive(&self, _pane: &PaneId) -> BoxFuture<bool> {
        ready(true)
    }

    fn kill(&self, pane: &PaneId) -> BoxFuture<Result<(), String>> {
        self.push(format!("kill:{pane}"));
        ready(Ok(()))
    }

    fn enqueue(
        &self,
        pane: &PaneId,
        text: &str,
        from: Option<&str>,
    ) -> BoxFuture<Result<(), String>> {
        lock(&self.senders).push(format!("{text} <- {}", from.unwrap_or("the person")));
        self.push(format!("enqueue:{pane}:{text}"));
        ready(Ok(()))
    }

    fn dir_exists(&self, path: &str) -> BoxFuture<bool> {
        ready(!path.contains("missing"))
    }

    fn sleep(&self, _ms: u64) -> BoxFuture<()> {
        ready(())
    }

    fn spawn(&self, task: BoxFuture<()>) {
        lock(&self.spawned).push(tokio::spawn(task));
    }

    fn now_ms(&self) -> u64 {
        1_000
    }

    fn model_allow_roots(&self, ws: &WsId, roots: Vec<String>) -> BoxFuture<Result<(), String>> {
        self.push(format!("allow:{ws}:{}", roots.join("|")));
        *lock(&self.allowed) = roots;
        ready(Ok(()))
    }

    // Core's rules, roughly: inside an allowed folder, present, any size.
    fn model_stat(&self, path: &str) -> BoxFuture<Result<ModelFileInfo, String>> {
        if let Some(answer) = lock(&self.stat).as_ref().and_then(|stat| stat(path)) {
            return ready(answer);
        }
        self.push(format!("stat:{path}"));
        let lower = path.to_lowercase();
        if self.outside(path) {
            return ready(Err(format!(
                "{path} is outside every folder the preview may read"
            )));
        }
        if lower.contains("missing") {
            return ready(Err(format!("No such file: {path}")));
        }
        ready(Ok(ModelFileInfo {
            path: path.into(),
            size: if lower.contains("huge") {
                300 * 1024 * 1024
            } else {
                48_000
            },
            modified_ms: lock(&self.mtimes).get(path).copied().unwrap_or(100),
        }))
    }

    fn worktrees(&self, path: &str) -> BoxFuture<Result<Vec<Worktree>, String>> {
        if let Some(trees) = lock(&self.trees).as_ref() {
            lock(&self.listed).push(path.to_string());
            return ready(trees(path));
        }
        if path != "C:\\dev\\hangar" {
            return ready(Err("not a git repository".into()));
        }
        ready(Ok(vec![
            worktree("C:\\dev\\hangar", true, None),
            worktree("C:\\dev\\hangar-wt\\feature", false, None),
            worktree(
                "C:\\dev\\hangar-wt\\old",
                false,
                Some("gitdir file points to non-existent location"),
            ),
        ]))
    }

    fn preview(&self, ws: &WsId, detached: bool) -> BoxFuture<Option<Arc<dyn PreviewHost>>> {
        if !*lock(&self.host_answers) {
            return ready(None);
        }
        self.push(format!(
            "host:{ws}:{}",
            if detached { "own window" } else { "here" }
        ));
        lock(&self.preview.state).workspace = ws.clone();
        ready(Some(self.preview.clone() as Arc<dyn PreviewHost>))
    }

    // Core's rules, roughly, as for model_stat; the kind by extension.
    fn preview_inspect(&self, path: &str) -> BoxFuture<Result<Value, String>> {
        self.push(format!("inspect:{path}"));
        let lower = path.to_lowercase();
        if self.outside(path) {
            return ready(Err(format!(
                "{path} is outside every folder the preview may read"
            )));
        }
        if lower.contains("missing") {
            return ready(Err(format!("No such file: {path}")));
        }
        let extension = lower.rsplit('.').next().unwrap_or("").to_string();
        let Some(kind) = kind_of(&extension) else {
            return ready(Err(format!(
                "The preview does not show .{extension} files."
            )));
        };
        ready(Ok(json!({
            "path": path,
            "kind": kind,
            "mime": mime_of(kind),
            "size": 4096,
            "name": basename(path),
            "modifiedMs": lock(&self.mtimes).get(path).copied().unwrap_or(100),
        })))
    }

    fn kits(&self) -> BoxFuture<Result<Vec<KitInfo>, String>> {
        ready(Ok(lock(&self.installed).clone()))
    }

    fn kit_tabs(&self) -> Option<Arc<dyn KitTabCatalog>> {
        lock(&self.catalog).clone()
    }

    fn git_flow(&self) -> Option<Arc<dyn FromGit>> {
        lock(&self.from_git).clone()
    }

    fn delivery(&self) -> Option<Arc<dyn DeliveryDeps>> {
        lock(&self.delivery).clone()
    }

    fn orchestration(&self) -> Option<Arc<dyn OrchestrationHooks>> {
        lock(&self.engine).clone()
    }

    fn deliveries(&self) -> &Deliveries {
        &self.deliveries
    }

    fn board_request(
        &self,
        pane: &PaneId,
        name: &str,
        cwd: &str,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> BoxFuture<Result<Value, String>> {
        self.push(format!(
            "board:{pane}:{name}:{cwd}:{method} {path}:{}",
            body.map(|body| body.to_string()).unwrap_or_default()
        ));
        ready(Ok(json!({ "task": { "id": "t1" } })))
    }
}
