//! The port over the app's `Services`: what the operations reach when they
//! run for real (App.tsx's `controlDeps`).
//!
//! Model writes go to the UI thread as `Msg::Model` and are awaited; the
//! terminals and the preview through the traits P5 and P15 implement; core's
//! commands directly, the blocking ones on tokio's blocking pool. The hooks
//! other packages install (`Msg::Install`) are held in `Shared`, which the
//! package's `State` keeps, so one installed before the first request is
//! there when it comes.

use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Duration;

use serde_json::Value;

use hangar_core::accounts::Account;
use hangar_core::board::{Board, Task, TaskPatch};
use hangar_core::delivery::{Cleanup, Delivery, Outcome, Plan, Running};
use hangar_core::git::Worktree;
use hangar_core::host::{Event, Host};
use hangar_core::kits::KitInfo;
use hangar_core::shells::ShellInfo;

use crate::message::Message;
use crate::services::preview::{PreviewEntry, PreviewRequest};
use crate::services::{Services, inbox::Answer};
use crate::state::{PaneId, Snapshot, WsId};

use super::Msg;
use super::delivery::Deliveries;
use super::port::{
    AppPort, BoxFuture, CmdOut, DeliveryDeps, EntryKind, EntryStatus, FromGit, Hook,
    KitTabCatalog, MobileHooks, ModelCmd, ModelFileInfo, OpenEntry, OrchestrationHooks,
    PaneTerminal, PreviewHost, PreviewHosts, PreviewListing, Showing, SnapshotAngle, workspace_in,
};
use super::preview::{RootGrants, model_format_of};

/// What the port keeps between requests.
#[derive(Default)]
pub struct Shared {
    hooks: RwLock<Hooks>,
    deliveries: Deliveries,
    grants: Mutex<RootGrants>,
}

#[derive(Default)]
struct Hooks {
    orchestration: Option<Arc<dyn OrchestrationHooks>>,
    mobile: Option<Arc<dyn MobileHooks>>,
    from_git: Option<Arc<dyn FromGit>>,
    kit_tabs: Option<Arc<dyn KitTabCatalog>>,
    preview: Option<Arc<dyn PreviewHosts>>,
}

impl Shared {
    /// Another package's part of the bus, from now on.
    pub fn install(&self, hook: Hook) {
        let mut hooks = self
            .hooks
            .write()
            .unwrap_or_else(|poison| poison.into_inner());
        match hook {
            Hook::Orchestration(engine) => hooks.orchestration = Some(engine),
            Hook::Mobile(mobile) => hooks.mobile = Some(mobile),
            Hook::FromGit(flow) => hooks.from_git = Some(flow),
            Hook::KitTabs(catalog) => hooks.kit_tabs = Some(catalog),
            Hook::Preview(hosts) => hooks.preview = Some(hosts),
        }
    }

    fn hooks(&self) -> std::sync::RwLockReadGuard<'_, Hooks> {
        self.hooks.read().unwrap_or_else(|poison| poison.into_inner())
    }
}

pub struct LivePort {
    services: Services,
    shared: Arc<Shared>,
}

impl LivePort {
    pub fn new(services: Services, shared: Arc<Shared>) -> LivePort {
        LivePort { services, shared }
    }

    fn host(&self) -> Host {
        self.services.core().host().clone()
    }
}

/// `f` on tokio's blocking pool: the core commands that touch the disk or a
/// lock the panes' readers hold.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|err| format!("the task stopped: {err}"))
}

impl AppPort for LivePort {
    fn snapshot(&self) -> Arc<Snapshot> {
        self.services.snapshot()
    }

    fn apply(&self, cmd: ModelCmd) -> BoxFuture<Result<CmdOut, String>> {
        let (answer, done) = Answer::channel();
        let sent = self
            .services
            .send(Message::Control(Msg::Model(cmd, answer)));
        Box::pin(async move {
            if !sent {
                return Err("Hangar.AI is closing.".into());
            }
            done.await
                .map_err(|_| "Hangar.AI is closing.".to_string())?
        })
    }

    fn terminal(&self, pane: &PaneId) -> Option<Arc<dyn PaneTerminal>> {
        let access = self.services.terminals().clone();
        access.has(pane).then(|| {
            Arc::new(LiveTerminal {
                access,
                pane: pane.clone(),
            }) as Arc<dyn PaneTerminal>
        })
    }

    fn write(&self, pane: &PaneId, data: &str) -> BoxFuture<Result<(), String>> {
        let pty = self.services.core().pty().clone();
        let (pane, data) = (pane.clone(), data.to_string());
        Box::pin(async move {
            blocking(move || hangar_core::pty::pty_write(&pty, pane, data)).await?
        })
    }

    fn alive(&self, pane: &PaneId) -> BoxFuture<bool> {
        let pty = self.services.core().pty().clone();
        let pane = pane.clone();
        Box::pin(async move {
            blocking(move || hangar_core::pty::pty_alive(&pty, pane))
                .await
                .unwrap_or(false)
        })
    }

    fn kill(&self, pane: &PaneId) -> BoxFuture<Result<(), String>> {
        let pty = self.services.core().pty().clone();
        let pane = pane.clone();
        Box::pin(async move {
            blocking(move || hangar_core::pty::pty_kill(&pty, pane, Some(true))).await?
        })
    }

    fn enqueue(
        &self,
        pane: &PaneId,
        text: &str,
        from: Option<&str>,
    ) -> BoxFuture<Result<(), String>> {
        let host = self.host();
        let (pane, text, from) = (pane.clone(), text.to_string(), from.map(str::to_string));
        Box::pin(async move {
            blocking(move || {
                hangar_core::bridge::bridge_enqueue(&host, pane, text, from);
            })
            .await
        })
    }

    fn dir_exists(&self, path: &str) -> BoxFuture<bool> {
        let path = path.to_string();
        Box::pin(async move {
            blocking(move || hangar_core::store::dir_exists(path))
                .await
                .unwrap_or(false)
        })
    }

    fn sleep(&self, ms: u64) -> BoxFuture<()> {
        Box::pin(tokio::time::sleep(Duration::from_millis(ms)))
    }

    fn model_allow_roots(&self, ws: &WsId, roots: Vec<String>) -> BoxFuture<Result<(), String>> {
        let live: Vec<String> = self
            .snapshot()
            .state
            .workspaces
            .iter()
            .map(|ws| ws.id.clone())
            .collect();
        let union = self
            .shared
            .grants
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .grant(ws, roots, &live);
        Box::pin(hangar_core::model::model_allow_roots(union))
    }

    fn model_stat(&self, path: &str) -> BoxFuture<Result<ModelFileInfo, String>> {
        let stat = hangar_core::model::model_stat(path.to_string());
        Box::pin(async move {
            let file = stat.await?;
            Ok(ModelFileInfo {
                path: file.path,
                size: file.size,
                modified_ms: file.modified_ms,
            })
        })
    }

    fn worktrees(&self, path: &str) -> BoxFuture<Result<Vec<Worktree>, String>> {
        Box::pin(hangar_core::git::git_worktree_list(path.to_string()))
    }

    fn preview(&self, ws: &WsId, detached: bool) -> BoxFuture<Option<Arc<dyn PreviewHost>>> {
        let installed = self.shared.hooks().preview.clone();
        let host = match installed {
            Some(hosts) => hosts.host(ws, detached),
            None => Some(Arc::new(AccessHost {
                services: self.services.clone(),
                ws: ws.clone(),
            }) as Arc<dyn PreviewHost>),
        };
        Box::pin(std::future::ready(host))
    }

    fn preview_inspect(&self, path: &str) -> BoxFuture<Result<Value, String>> {
        let inspect = hangar_core::preview::preview_inspect(path.to_string());
        Box::pin(async move {
            let file = inspect.await?;
            serde_json::to_value(&file).map_err(|err| err.to_string())
        })
    }

    fn kits(&self) -> BoxFuture<Result<Vec<KitInfo>, String>> {
        Box::pin(hangar_core::kits::kits_list())
    }

    fn kit_tabs(&self) -> Option<Arc<dyn KitTabCatalog>> {
        self.shared.hooks().kit_tabs.clone()
    }

    fn from_git(&self) -> Option<Arc<dyn FromGit>> {
        self.shared.hooks().from_git.clone()
    }

    fn delivery(&self) -> Option<Arc<dyn DeliveryDeps>> {
        Some(Arc::new(LiveDelivery { host: self.host() }))
    }

    fn orchestration(&self) -> Option<Arc<dyn OrchestrationHooks>> {
        self.shared.hooks().orchestration.clone()
    }

    fn mobile(&self) -> Option<Arc<dyn MobileHooks>> {
        self.shared.hooks().mobile.clone()
    }

    fn deliveries(&self) -> &Deliveries {
        &self.shared.deliveries
    }
}

/// A pane's terminal through P5's hub.
struct LiveTerminal {
    access: Arc<dyn crate::services::TerminalAccess>,
    pane: PaneId,
}

impl PaneTerminal for LiveTerminal {
    fn paste(&self, text: &str) -> BoxFuture<Result<(), String>> {
        let pasted = self.access.paste(&self.pane, text);
        Box::pin(std::future::ready(if pasted {
            Ok(())
        } else {
            Err("The pane's terminal went away before the text reached it. Send again.".into())
        }))
    }

    fn read(&self, n: usize) -> Vec<String> {
        self.access.read_lines(&self.pane, n).unwrap_or_default()
    }

    fn application_cursor(&self) -> bool {
        self.access.application_cursor(&self.pane)
    }
}

/// core's delivery, board and accounts, and the bridge queue.
struct LiveDelivery {
    host: Host,
}

/// The shells detected on this machine, once: what the shell detected at
/// start is the window's, not the snapshot's.
fn shells() -> &'static Vec<ShellInfo> {
    static SHELLS: OnceLock<Vec<ShellInfo>> = OnceLock::new();
    SHELLS.get_or_init(hangar_core::shells::detect_shells)
}

impl DeliveryDeps for LiveDelivery {
    fn preflight(
        &self,
        workspace: String,
        worktree: String,
        target: Option<String>,
        landing: bool,
        task: Option<String>,
    ) -> BoxFuture<Result<Plan, String>> {
        Box::pin(hangar_core::delivery::delivery_preflight(
            workspace,
            worktree,
            target,
            Some(landing),
            task,
        ))
    }

    fn run(&self, delivery: Delivery) -> BoxFuture<Result<Outcome, String>> {
        Box::pin(async move { Ok(hangar_core::delivery::delivery_run(delivery).await) })
    }

    fn cleanup(
        &self,
        worktree: String,
        branch: String,
        target: String,
        task_id: String,
    ) -> BoxFuture<Result<Cleanup, String>> {
        Box::pin(async move {
            Ok(hangar_core::delivery::delivery_cleanup(worktree, branch, target, task_id).await)
        })
    }

    fn running(&self) -> BoxFuture<Result<Vec<Running>, String>> {
        Box::pin(blocking(hangar_core::delivery::delivery_running))
    }

    fn accounts(&self) -> BoxFuture<Result<Vec<Account>, String>> {
        Box::pin(blocking(hangar_core::accounts::accounts_list))
    }

    fn shells(&self) -> Vec<ShellInfo> {
        shells().clone()
    }

    fn board_load(&self, cwd: &str) -> BoxFuture<Result<Board, String>> {
        let cwd = cwd.to_string();
        Box::pin(blocking(move || hangar_core::board_load(cwd)))
    }

    fn board_update(&self, cwd: &str, id: &str, patch: TaskPatch) -> BoxFuture<Result<Task, String>> {
        let (host, cwd, id) = (self.host.clone(), cwd.to_string(), id.to_string());
        Box::pin(async move {
            let task = blocking({
                let cwd = cwd.clone();
                move || hangar_core::board_update(cwd, id, patch)
            })
            .await??;
            changed(&host, cwd);
            Ok(task)
        })
    }

    fn board_comment(
        &self,
        cwd: &str,
        id: &str,
        author: &str,
        text: &str,
    ) -> BoxFuture<Result<Task, String>> {
        let (host, cwd, id, author, text) = (
            self.host.clone(),
            cwd.to_string(),
            id.to_string(),
            author.to_string(),
            text.to_string(),
        );
        Box::pin(async move {
            let task = blocking({
                let cwd = cwd.clone();
                move || hangar_core::board_comment(cwd, id, author, text)
            })
            .await??;
            changed(&host, cwd);
            Ok(task)
        })
    }

    fn tell(&self, pane: &PaneId, text: &str) -> BoxFuture<Result<(), String>> {
        let (host, pane, text) = (self.host.clone(), pane.clone(), text.to_string());
        Box::pin(async move {
            blocking(move || {
                hangar_core::bridge::bridge_enqueue(&host, pane, text, Some(super::delivery_text::HANGAR.into()));
            })
            .await
        })
    }
}

/// The board views and the orchestrator's events reload on this, which the
/// writes through the agents' API send by themselves and these do not.
fn changed(host: &Host, cwd: String) {
    // Nobody listening is a window closing: nothing to tell.
    let _ = host.emit(Event::BoardChanged { cwd });
}

/// A workspace's preview through the shell's `PreviewAccess`, until P15
/// installs its own hosts (`Hook::Preview`).
///
/// TODO(P15): `PreviewAccess` lists entries without the active one, the
/// panel's visibility, a model's format and findings or a kit tab's params;
/// they read as absent here. P15 installs `PreviewHosts` with all of it.
struct AccessHost {
    services: Services,
    ws: WsId,
}

impl AccessHost {
    fn entry(entry: PreviewEntry) -> OpenEntry {
        let what = match entry.kind.as_str() {
            "model" => EntryKind::Model {
                format: model_format_of(&entry.path).unwrap_or("").to_string(),
                info: None,
            },
            "kit" => {
                let named = entry.path.strip_prefix("kit:").unwrap_or("");
                let named = named.split('?').next().unwrap_or("");
                let (kit_id, tab_id) = named.split_once('/').unwrap_or((named, ""));
                EntryKind::Kit {
                    kit_id: kit_id.to_string(),
                    tab_id: tab_id.to_string(),
                    params: Default::default(),
                }
            }
            kind => EntryKind::File {
                kind: kind.to_string(),
                mime: String::new(),
                size: 0,
            },
        };
        OpenEntry {
            id: entry.id,
            path: entry.path,
            title: entry.title,
            opened_by: entry.opened_by,
            modified_ms: 0,
            status: if entry.error.is_some() {
                EntryStatus::Error
            } else if entry.loading {
                EntryStatus::Loading
            } else {
                EntryStatus::Ready
            },
            error: entry.error,
            what,
        }
    }
}

impl PreviewHost for AccessHost {
    fn list(&self) -> BoxFuture<Result<PreviewListing, String>> {
        let snapshot = self.services.snapshot();
        let on_screen = workspace_in(&snapshot, &self.ws).is_some_and(|ws| {
            super::run::on_screen(&snapshot.state, ws)
        });
        let entries = self
            .services
            .preview()
            .list(&self.ws)
            .into_iter()
            .map(AccessHost::entry)
            .collect();
        Box::pin(std::future::ready(Ok(PreviewListing {
            entries,
            active_id: None,
            on_screen,
            shown: false,
        })))
    }

    fn show(&self, entry: Showing) -> BoxFuture<Result<String, String>> {
        let (title, reveal) = match &entry {
            Showing::Model { title, .. } | Showing::File { title, .. } => (title.clone(), true),
            Showing::Kit { title, reveal, .. } => (title.clone(), *reveal),
        };
        let shown = self.services.preview().show(PreviewRequest {
            ws: self.ws.clone(),
            path: entry.path().to_string(),
            title: Some(title),
            opened_by: None,
            reveal,
        });
        Box::pin(std::future::ready(shown))
    }

    fn reveal(&self, id: &str) -> BoxFuture<()> {
        self.services.preview().reveal(&self.ws, id);
        Box::pin(std::future::ready(()))
    }

    fn reload(&self, id: &str, _modified_ms: u64) -> BoxFuture<()> {
        self.services.preview().reload(&self.ws, id);
        Box::pin(std::future::ready(()))
    }

    fn when_settled(&self, id: &str, ms: u64) -> BoxFuture<Option<OpenEntry>> {
        let settled = self
            .services
            .preview()
            .when_settled(&self.ws, id, Duration::from_millis(ms));
        Box::pin(async move { settled.await.map(AccessHost::entry) })
    }

    fn snapshot(
        &self,
        id: &str,
        angle: SnapshotAngle,
        width: u32,
        height: u32,
    ) -> BoxFuture<Result<Vec<u8>, String>> {
        self.services
            .preview()
            .snapshot(&self.ws, id, angle, (width, height))
    }

    fn close(&self, ids: Vec<String>) -> BoxFuture<()> {
        for id in ids {
            self.services.preview().close(&self.ws, Some(&id));
        }
        Box::pin(std::future::ready(()))
    }
}
