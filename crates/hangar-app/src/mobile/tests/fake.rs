//! The window the tests run the phone's calls against: P18's port over an app
//! in memory, logging what was done the way the TS rigs did (`open:w1`,
//! `touch:w1`, `paste:w1:hello`, `write:w1:"\r"`), and `Sources` that answer
//! what each test says.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;

use hangar_core::board::Task;
use hangar_core::chat::ChatPage;
use hangar_core::git::{ChangedFile, Changes, FileDiff, Worktree};
use hangar_core::memory::MemoryEntry;
use hangar_core::remote::ActivityEntry;
use hangar_core::stats::UsageStats;

use crate::control::delivery::Deliveries;
use crate::control::port::{
    AppPort, BoxFuture, CmdOut, DeliveryDeps, ModelCmd, ModelFileInfo, PaneTerminal, Port,
    PreviewHost,
};
use crate::orchestration::events::JournalEntry;
use crate::orchestration::manager_feed::ManagerJournalEntry;
use crate::orchestration::projects::{ProjectSummary, ProjectsPort, load_projects};
use crate::state::types::PaneRole;
use crate::state::{
    AgentActivity, AgentId, AppState, Pane, PaneActivity, PaneId, Snapshot, Workspace, WsId,
};

use super::super::engine::{ChatRead, Mobile, Sources, locked};

pub fn ready<T: Send + 'static>(value: T) -> BoxFuture<T> {
    Box::pin(std::future::ready(value))
}

/// `pane(id, name, role?, agent = "claude")` of the TS tests.
pub fn pane(id: &str, name: &str, role: Option<PaneRole>, agent: AgentId) -> Pane {
    Pane {
        id: id.into(),
        name: name.into(),
        agent,
        role,
        ..Pane::default()
    }
}

pub fn claude(id: &str, name: &str, role: Option<PaneRole>) -> Pane {
    pane(id, name, role, AgentId::Claude)
}

/// A workspace in `C:\dev\<id>`.
pub fn workspace(id: &str, name: &str, panes: Vec<Pane>) -> Workspace {
    Workspace {
        id: id.into(),
        name: name.into(),
        cwd: format!("C:\\dev\\{id}"),
        panes,
        ..Workspace::default()
    }
}

/// What a pane's terminal shows and takes, when a test draws it.
pub trait Screen: Send + Sync {
    fn paste(&self, text: &str);
    fn key(&self, name: &str);
    fn lines(&self) -> Vec<String>;
}

/// The keys `pane_send` writes, by name, for a screen that takes names.
fn key_of(data: &str) -> String {
    match data {
        "\r" => "enter".into(),
        "\x1b[B" => "down".into(),
        "\x1b[A" => "up".into(),
        "\x1b" => "escape".into(),
        other => other.into(),
    }
}

/// An app in memory.
pub struct App {
    pub state: Mutex<AppState>,
    pub opened: Mutex<HashSet<WsId>>,
    pub asleep: Mutex<HashSet<WsId>>,
    pub activity: Mutex<HashMap<PaneId, PaneActivity>>,
    /// Panes with a terminal; the others have none until their workspace
    /// opens.
    pub live: Mutex<HashSet<PaneId>>,
    pub typed: Arc<Mutex<Vec<String>>>,
    /// What went to the hangar-bridge queue, as `pane:text`.
    pub queued: Mutex<Vec<String>>,
    /// Model writes: `open:w1`, `touch:w1`, `close:w1:p1`.
    pub log: Arc<Mutex<Vec<String>>>,
    pub screen: Arc<Mutex<Vec<String>>>,
    /// A dialog drawn in every terminal, instead of `screen`.
    pub drawn: Mutex<Option<Arc<dyn Screen>>>,
    /// Sleeps taken.
    pub naps: Arc<AtomicU32>,
    /// A terminal mounts one nap after its workspace opens, its shell two
    /// naps later still, and opening mounts nothing by itself.
    pub slow: AtomicBool,
    pub delivering: Mutex<HashMap<(WsId, String), u32>>,
    pub delivery: Mutex<Option<Arc<dyn DeliveryDeps>>>,
    deliveries: Deliveries,
}

impl App {
    pub fn new(state: AppState) -> Arc<App> {
        let all: HashSet<PaneId> = state
            .workspaces
            .iter()
            .flat_map(|ws| ws.panes.iter().map(|pane| pane.id.clone()))
            .collect();
        let opened = state.workspaces.iter().map(|ws| ws.id.clone()).collect();
        Arc::new(App {
            state: Mutex::new(state),
            opened: Mutex::new(opened),
            asleep: Mutex::new(HashSet::new()),
            activity: Mutex::new(HashMap::new()),
            live: Mutex::new(all),
            typed: Arc::new(Mutex::new(Vec::new())),
            queued: Mutex::new(Vec::new()),
            log: Arc::new(Mutex::new(Vec::new())),
            screen: Arc::new(Mutex::new(vec!["one".into(), "two".into(), "three".into()])),
            drawn: Mutex::new(None),
            naps: Arc::new(AtomicU32::new(0)),
            slow: AtomicBool::new(false),
            delivering: Mutex::new(HashMap::new()),
            delivery: Mutex::new(None),
            deliveries: Deliveries::default(),
        })
    }

    pub fn port(self: &Arc<Self>) -> Port {
        self.clone()
    }

    /// `act(pane, workspace, activity)`: as the mod reports it.
    pub fn act(&self, pane: &str, ws: &str, activity: AgentActivity) {
        locked(&self.activity).insert(
            pane.into(),
            PaneActivity {
                workspace_id: ws.into(),
                activity,
                bridged: true,
            },
        );
    }

    /// Puts a workspace to sleep: its panes have no terminal until it opens.
    pub fn sleep_ws(&self, ws: &str) {
        locked(&self.asleep).insert(ws.into());
        locked(&self.opened).remove(ws);
        let panes: Vec<PaneId> = locked(&self.state)
            .workspaces
            .iter()
            .filter(|entry| entry.id == ws)
            .flat_map(|entry| entry.panes.iter().map(|pane| pane.id.clone()))
            .collect();
        let mut live = locked(&self.live);
        for pane in panes {
            live.remove(&pane);
        }
    }

    pub fn typed(&self) -> Vec<String> {
        locked(&self.typed).clone()
    }

    pub fn queued(&self) -> Vec<String> {
        locked(&self.queued).clone()
    }

    pub fn logged(&self, prefix: &str) -> Vec<String> {
        locked(&self.log)
            .iter()
            .filter_map(|line| line.strip_prefix(prefix).map(str::to_string))
            .collect()
    }

    pub fn is_delivering(&self, ws: &str) -> bool {
        locked(&self.delivering)
            .iter()
            .any(|((id, _), count)| id == ws && *count > 0)
    }

    pub fn change(&self, edit: impl FnOnce(&mut AppState)) {
        edit(&mut locked(&self.state));
    }

    fn mounted(&self, pane: &str) -> bool {
        if self.slow.load(Ordering::SeqCst) {
            self.naps.load(Ordering::SeqCst) >= 1
        } else {
            locked(&self.live).contains(pane)
        }
    }
}

struct Terminal {
    pane: PaneId,
    typed: Arc<Mutex<Vec<String>>>,
    screen: Arc<Mutex<Vec<String>>>,
    drawn: Option<Arc<dyn Screen>>,
    naps: Arc<AtomicU32>,
    slow: bool,
}

impl PaneTerminal for Terminal {
    fn paste(&self, text: &str) -> BoxFuture<Result<(), String>> {
        if let Some(drawn) = &self.drawn {
            drawn.paste(text);
        } else if self.slow {
            let naps = self.naps.load(Ordering::SeqCst);
            locked(&self.typed).push(format!("paste:{}:{text} after {naps} naps", self.pane));
        } else {
            locked(&self.typed).push(format!("paste:{}:{text}", self.pane));
        }
        ready(Ok(()))
    }

    fn read(&self, n: usize) -> Vec<String> {
        let lines = match &self.drawn {
            Some(drawn) => drawn.lines(),
            None => locked(&self.screen).clone(),
        };
        lines[lines.len().saturating_sub(n)..].to_vec()
    }

    fn application_cursor(&self) -> bool {
        false
    }
}

impl AppPort for App {
    fn snapshot(&self) -> Arc<Snapshot> {
        let delivering = {
            let mut sets: HashMap<WsId, HashSet<String>> = HashMap::new();
            for ((ws, task), count) in locked(&self.delivering).iter() {
                if *count > 0 {
                    sets.entry(ws.clone()).or_default().insert(task.clone());
                }
            }
            sets
        };
        Arc::new(Snapshot {
            state: Arc::new(locked(&self.state).clone()),
            opened: locked(&self.opened).clone(),
            asleep: locked(&self.asleep).clone(),
            activity: locked(&self.activity).clone(),
            delivering,
            hydrated: true,
        })
    }

    fn apply(&self, cmd: ModelCmd) -> BoxFuture<Result<CmdOut, String>> {
        let mut log = locked(&self.log);
        match cmd {
            ModelCmd::OpenWorkspace(id) => {
                log.push(format!("open:{id}"));
                if !self.slow.load(Ordering::SeqCst) {
                    locked(&self.opened).insert(id.clone());
                    locked(&self.asleep).remove(&id);
                    let panes: Vec<PaneId> = locked(&self.state)
                        .workspaces
                        .iter()
                        .filter(|ws| ws.id == id)
                        .flat_map(|ws| ws.panes.iter().map(|pane| pane.id.clone()))
                        .collect();
                    locked(&self.live).extend(panes);
                }
            }
            ModelCmd::Touch(id) => log.push(format!("touch:{id}")),
            ModelCmd::ClosePane { ws, pane } => {
                log.push(format!("close:{ws}:{pane}"));
                let mut state = locked(&self.state);
                if let Some(found) = state.workspaces.iter_mut().find(|entry| entry.id == ws) {
                    found.panes.retain(|entry| entry.id != pane);
                }
            }
            ModelCmd::BeginDelivery { ws, task } => {
                *locked(&self.delivering).entry((ws, task)).or_default() += 1;
            }
            ModelCmd::EndDelivery { ws, task } => {
                if let Some(count) = locked(&self.delivering).get_mut(&(ws, task)) {
                    *count = count.saturating_sub(1);
                }
            }
            _ => {}
        }
        ready(Ok(CmdOut::default()))
    }

    fn terminal(&self, pane: &PaneId) -> Option<Arc<dyn PaneTerminal>> {
        self.mounted(pane).then(|| {
            Arc::new(Terminal {
                pane: pane.clone(),
                typed: self.typed.clone(),
                screen: self.screen.clone(),
                drawn: locked(&self.drawn).clone(),
                naps: self.naps.clone(),
                slow: self.slow.load(Ordering::SeqCst),
            }) as Arc<dyn PaneTerminal>
        })
    }

    fn write(&self, pane: &PaneId, data: &str) -> BoxFuture<Result<(), String>> {
        match locked(&self.drawn).clone() {
            Some(drawn) => drawn.key(&key_of(data)),
            None => locked(&self.typed).push(format!(
                "write:{pane}:{}",
                serde_json::to_string(data).unwrap()
            )),
        }
        ready(Ok(()))
    }

    fn alive(&self, pane: &PaneId) -> BoxFuture<bool> {
        ready(if self.slow.load(Ordering::SeqCst) {
            self.naps.load(Ordering::SeqCst) >= 3
        } else {
            locked(&self.live).contains(pane)
        })
    }

    fn kill(&self, _pane: &PaneId) -> BoxFuture<Result<(), String>> {
        ready(Ok(()))
    }

    fn enqueue(
        &self,
        pane: &PaneId,
        text: &str,
        from: Option<&str>,
    ) -> BoxFuture<Result<(), String>> {
        locked(&self.queued).push(match from {
            Some(from) => format!("{pane}:{text} (from {from})"),
            None => format!("{pane}:{text}"),
        });
        ready(Ok(()))
    }

    fn dir_exists(&self, _path: &str) -> BoxFuture<bool> {
        ready(true)
    }

    fn sleep(&self, _ms: u64) -> BoxFuture<()> {
        self.naps.fetch_add(1, Ordering::SeqCst);
        ready(())
    }

    fn model_allow_roots(&self, _ws: &WsId, _roots: Vec<String>) -> BoxFuture<Result<(), String>> {
        ready(Ok(()))
    }

    fn model_stat(&self, path: &str) -> BoxFuture<Result<ModelFileInfo, String>> {
        ready(Err(format!("no {path}")))
    }

    fn worktrees(&self, _path: &str) -> BoxFuture<Result<Vec<Worktree>, String>> {
        ready(Ok(Vec::new()))
    }

    fn preview(&self, _ws: &WsId, _detached: bool) -> BoxFuture<Option<Arc<dyn PreviewHost>>> {
        ready(None)
    }

    fn preview_inspect(&self, path: &str) -> BoxFuture<Result<Value, String>> {
        ready(Err(format!("no {path}")))
    }

    fn delivery(&self) -> Option<Arc<dyn DeliveryDeps>> {
        locked(&self.delivery).clone()
    }

    fn deliveries(&self) -> &Deliveries {
        &self.deliveries
    }
}

type Answer<A, T> = Box<dyn Fn(A) -> T + Send + Sync>;
type Ports = Result<Vec<u16>, String>;
type Titles = Result<Vec<(String, String)>, String>;

/// How `wait` answers: at once, or never (a landing gets as long as it
/// takes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wait {
    Now,
    Never,
}

/// What the operations read beyond the port, as each test says.
pub struct Fakes {
    pub app: Arc<App>,
    /// The board every workspace reads.
    pub board: Mutex<Vec<Task>>,
    pub manager: Mutex<Vec<ManagerJournalEntry>>,
    pub history: Mutex<Answer<u64, Result<Vec<ActivityEntry>, String>>>,
    pub reads: Mutex<Vec<u64>>,
    pub chat: Mutex<Answer<ChatRead, Result<Option<ChatPage>, String>>>,
    pub chats: Mutex<Vec<ChatRead>>,
    pub usage: Mutex<Option<UsageStats>>,
    pub worktrees: Mutex<Answer<String, Result<Vec<String>, String>>>,
    pub now: Mutex<i64>,
    pub changes: Mutex<Changes>,
    pub diffs: Mutex<HashMap<String, FileDiff>>,
    pub wait: Mutex<Wait>,
    /// What git and the boards were asked, as `changes:<path>:<base>`.
    pub log: Arc<Mutex<Vec<String>>>,
    pub listening: Mutex<Answer<Vec<u16>, Ports>>,
    pub probed: Mutex<Vec<Vec<u16>>>,
    pub tasks: Mutex<HashMap<String, Titles>>,
    pub boards: Mutex<Vec<String>>,
}

impl Fakes {
    pub fn new(app: Arc<App>) -> Arc<Fakes> {
        Arc::new(Fakes {
            app,
            board: Mutex::new(Vec::new()),
            manager: Mutex::new(Vec::new()),
            history: Mutex::new(Box::new(|_| Ok(Vec::new()))),
            reads: Mutex::new(Vec::new()),
            chat: Mutex::new(Box::new(|_| Ok(None))),
            chats: Mutex::new(Vec::new()),
            usage: Mutex::new(None),
            worktrees: Mutex::new(Box::new(|_| Ok(Vec::new()))),
            now: Mutex::new(0),
            changes: Mutex::new(Changes::default()),
            diffs: Mutex::new(HashMap::new()),
            wait: Mutex::new(Wait::Never),
            log: Arc::new(Mutex::new(Vec::new())),
            listening: Mutex::new(Box::new(Ok)),
            probed: Mutex::new(Vec::new()),
            tasks: Mutex::new(HashMap::new()),
            boards: Mutex::new(Vec::new()),
        })
    }

    pub fn mobile(self: &Arc<Self>) -> Arc<Mobile> {
        Mobile::new(self.clone())
    }
}

/// `loadProjects` over the app: every board reads `board`, every folder is
/// a repository, the journals are empty.
struct Projects<'a> {
    app: &'a App,
    board: Vec<Task>,
}

impl ProjectsPort for Projects<'_> {
    fn state(&self) -> Arc<AppState> {
        Arc::new(locked(&self.app.state).clone())
    }

    async fn load_board(&self, _cwd: &str) -> Result<Vec<Task>, String> {
        Ok(self.board.clone())
    }

    async fn load_memory(&self) -> Result<Vec<MemoryEntry>, String> {
        Ok(Vec::new())
    }

    async fn is_repo(&self, _cwd: &str) -> Result<Option<bool>, String> {
        Ok(Some(true))
    }

    fn activity(&self, pane: &str) -> Option<PaneActivity> {
        locked(&self.app.activity).get(pane).cloned()
    }

    fn asleep(&self, workspace_id: &str) -> bool {
        locked(&self.app.asleep).contains(workspace_id)
    }

    fn is_open(&self, workspace_id: &str) -> bool {
        locked(&self.app.opened).contains(workspace_id)
    }

    fn journal(&self, _workspace_id: &str) -> Vec<JournalEntry> {
        Vec::new()
    }
}

impl Sources for Fakes {
    fn projects(&self) -> BoxFuture<Vec<ProjectSummary>> {
        let (app, board) = (self.app.clone(), locked(&self.board).clone());
        Box::pin(async move { load_projects(&Projects { app: &app, board }).await })
    }

    fn manager_journal(&self) -> Vec<ManagerJournalEntry> {
        locked(&self.manager).clone()
    }

    fn history(&self, since: u64) -> BoxFuture<Result<Vec<ActivityEntry>, String>> {
        locked(&self.reads).push(since);
        ready((locked(&self.history))(since))
    }

    fn chat(&self, read: ChatRead) -> BoxFuture<Result<Option<ChatPage>, String>> {
        locked(&self.chats).push(read.clone());
        let page = (locked(&self.chat))(read);
        // Read off the UI as the TS's `await` was: a second call gets its
        // turn meanwhile.
        Box::pin(async move {
            tokio::task::yield_now().await;
            page
        })
    }

    fn usage(&self) -> BoxFuture<Result<Arc<UsageStats>, String>> {
        ready(
            locked(&self.usage)
                .clone()
                .map(Arc::new)
                .ok_or_else(|| "no statistics".to_string()),
        )
    }

    fn worktrees(&self, cwd: &str) -> BoxFuture<Result<Vec<String>, String>> {
        ready((locked(&self.worktrees))(cwd.to_string()))
    }

    fn stats_now(&self) -> i64 {
        *locked(&self.now)
    }

    fn changes(&self, path: &str, base: &str) -> BoxFuture<Result<Changes, String>> {
        locked(&self.log).push(format!("changes:{path}:{base}"));
        ready(Ok(locked(&self.changes).clone()))
    }

    fn file_diff(
        &self,
        path: &str,
        file: &ChangedFile,
        base: &str,
    ) -> BoxFuture<Result<FileDiff, String>> {
        locked(&self.log).push(format!("diff:{path}:{}:{base}", file.path));
        ready(
            locked(&self.diffs)
                .get(&file.path)
                .cloned()
                .ok_or_else(|| format!("no diff for {}", file.path)),
        )
    }

    fn wait(&self, _ms: u64) -> BoxFuture<()> {
        match *locked(&self.wait) {
            Wait::Now => ready(()),
            Wait::Never => Box::pin(std::future::pending()),
        }
    }

    fn listening(&self, ports: Vec<u16>) -> BoxFuture<Result<Vec<u16>, String>> {
        locked(&self.probed).push(ports.clone());
        ready((locked(&self.listening))(ports))
    }

    fn tasks(&self, cwd: &str) -> BoxFuture<Result<Vec<(String, String)>, String>> {
        locked(&self.boards).push(cwd.to_string());
        ready(
            locked(&self.tasks)
                .get(cwd)
                .cloned()
                .unwrap_or_else(|| Ok(Vec::new())),
        )
    }
}

/* ── Comparing answers ─────────────────────────────────────────── */

/// Two numbers are one whatever JSON kind holds them: `84` is `84.0`.
fn same_number(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(a), Some(b)) => (a - b).abs() < 1e-9,
        _ => false,
    }
}

/// `toEqual`: the same keys, the same values, numbers compared as numbers.
pub fn equal(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Number(_), Value::Number(_)) => same_number(actual, expected),
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| equal(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && b.iter()
                    .all(|(key, value)| a.get(key).is_some_and(|got| equal(got, value)))
        }
        _ => actual == expected,
    }
}

/// `toMatchObject`: what is expected is there; the rest may be anything.
pub fn matches(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Number(_), Value::Number(_)) => same_number(actual, expected),
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| matches(a, b))
        }
        (Value::Object(a), Value::Object(b)) => b
            .iter()
            .all(|(key, value)| a.get(key).is_some_and(|got| matches(got, value))),
        _ => actual == expected,
    }
}

#[track_caller]
pub fn assert_equal(actual: &Value, expected: Value) {
    assert!(
        equal(actual, &expected),
        "\n  got: {actual:#}\n want: {expected:#}"
    );
}

#[track_caller]
pub fn assert_matches(actual: &Value, expected: Value) {
    assert!(
        matches(actual, &expected),
        "\n  got: {actual:#}\n want at least: {expected:#}"
    );
}
