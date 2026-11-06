//! The ports over the running app: the snapshot for reads, hangar-core for
//! boards, git, the panes' shells and the bridge queue, and the inbox for the
//! model writes, which P18's `control::update` applies on the UI thread
//! (`Message::Control(control::Msg::Model(cmd, answer))`, PACKAGES.md,
//! rule 6).

use std::sync::{Arc, Mutex};

use hangar_core::board::Task;
use hangar_core::git::{
    Changes, CopyResult, DeleteBranchResult, HookReport, RemoveResult, RepoInfo, Worktree,
};
use hangar_core::memory::MemoryEntry;
use hangar_core::pty::PtyManager;

use super::brief::{BriefingPort, CheckCost};
use super::events::{EngineIo, JournalEntry, OrchestratorEvent};
use super::manager_feed::ManagerJournalEntry;
use super::port::{Announcement, NewPane};
use super::projects::{ProjectSummary, ProjectsPort};
use super::runtime::OrchestrationHandle;
use super::text::now_ms;
use super::worker::{Starting, WorkerPort};

use crate::control::ModelCmd;
use crate::control::port::{is_open, workspace_in};
use crate::message::Message;
use crate::services::inbox::Answer;
use crate::state::types::{AppState, Pane, PaneId};
use crate::state::{PaneActivity, PanePatch, Snapshot};

/// The ports, over the running app.
#[derive(Clone, Debug)]
pub struct LivePort {
    handle: OrchestrationHandle,
}

/// `f` on Tokio's blocking pool: board files and the memory are read there.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|err| format!("the task stopped: {err}"))
}

/// The last delivery check of each workspace, by id: what it took and said.
fn checks() -> &'static Mutex<std::collections::HashMap<String, CheckCost>> {
    static CHECKS: std::sync::OnceLock<Mutex<std::collections::HashMap<String, CheckCost>>> =
        std::sync::OnceLock::new();
    CHECKS.get_or_init(Default::default)
}

/// Remembers how long a workspace's delivery check ran and what it said, for
/// the orchestrator's next briefing. A check that did not run is not recorded.
pub fn record_check(workspace_id: &str, seconds: u64, passed: bool) {
    checks()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(workspace_id.to_string(), CheckCost { seconds, passed });
}

/// The workspace's last check, if one ran since the app started.
pub fn last_check(workspace_id: &str) -> Option<CheckCost> {
    checks()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(workspace_id)
        .copied()
}

/// Once per run: what old versions' deliveries left in the temp folder goes,
/// and git forgets the worktrees whose folder is gone, in each workspace's
/// repository. On a thread of its own: git is slow on large repositories.
fn sweep_leftovers_once(folders: Vec<String>) {
    static DONE: std::sync::Once = std::sync::Once::new();
    if folders.is_empty() {
        return;
    }
    DONE.call_once(|| {
        let _ = std::thread::Builder::new()
            .name("delivery-sweep".into())
            .spawn(move || hangar_core::delivery::sweep_leftovers(&folders));
    });
}

impl LivePort {
    pub(super) fn new(handle: OrchestrationHandle) -> Self {
        let port = LivePort { handle };
        sweep_leftovers_once(
            port.snapshot()
                .state
                .workspaces
                .iter()
                .map(|ws| ws.cwd.clone())
                .filter(|cwd| !cwd.is_empty())
                .collect(),
        );
        port
    }

    fn snapshot(&self) -> Arc<Snapshot> {
        self.handle.services().snapshot()
    }

    fn host(&self) -> hangar_core::Host {
        self.handle.services().core().host().clone()
    }

    /// A model write, applied on the UI thread by P18's `control::update`;
    /// `None` when the app went away before answering.
    async fn write(&self, cmd: ModelCmd) -> Option<crate::state::StoreOutcome> {
        let (answer, outcome) = Answer::channel();
        if !self
            .handle
            .services()
            .send(Message::Control(crate::control::Msg::Model(cmd, answer)))
        {
            return None;
        }
        outcome.await.ok()?.ok()
    }

    /// The same, for a write nothing waits on.
    fn tell(&self, cmd: ModelCmd) {
        let (answer, _) = Answer::channel();
        self.handle
            .services()
            .send(Message::Control(crate::control::Msg::Model(cmd, answer)));
    }

    fn enqueue_now(&self, pane: &str, text: &str, from: &str) {
        hangar_core::bridge::bridge_enqueue(
            &self.host(),
            pane.into(),
            text.into(),
            Some(from.into()),
        );
    }
}

impl EngineIo for LivePort {
    fn state(&self) -> Arc<AppState> {
        self.snapshot().state.clone()
    }

    fn activity(&self, pane: &str) -> Option<PaneActivity> {
        self.snapshot().activity.get(pane).cloned()
    }

    fn load_board(&self, cwd: &str) -> Result<Vec<Task>, String> {
        Ok(hangar_core::board_load(cwd.into()).tasks)
    }

    fn enqueue(&self, pane: &str, text: &str, from: &str) -> Result<(), String> {
        self.enqueue_now(pane, text, from);
        Ok(())
    }
}

impl WorkerPort for LivePort {
    fn state(&self) -> Arc<AppState> {
        self.snapshot().state.clone()
    }

    /// The model's opened set, not a guess from the activity (inventory A6,
    /// problem 4), or a window of its own.
    fn is_open(&self, workspace_id: &str) -> bool {
        open_in(&self.snapshot(), workspace_id)
    }

    fn activity(&self, pane: &str) -> Option<PaneActivity> {
        self.snapshot().activity.get(pane).cloned()
    }

    fn starting(&self) -> &Mutex<Starting> {
        self.handle.starting()
    }

    fn journal(&self, workspace_id: &str) -> Vec<JournalEntry> {
        self.handle.journal(workspace_id)
    }

    fn post_event(&self, workspace_id: &str, event: OrchestratorEvent) {
        self.handle.post(workspace_id, event);
    }

    /// The notice `control.created` / `control.closed`, as App.tsx's
    /// `announce` showed it, in the person's language (P18 words it).
    fn announce(&self, announcement: Announcement) {
        self.tell(ModelCmd::Announce(announcement));
    }

    async fn add_pane(&self, workspace_id: &str, pane: NewPane) -> Option<PaneId> {
        // In the add itself, so the worker's shell starts with all of it.
        let init = pane.init.map(|init| {
            PanePatch::new(move |target: &mut Pane| {
                target.role = Some(init.role);
                target.task_id = Some(init.task_id.clone());
                target.model = Some(init.model.clone());
                target.port_base = Some(init.port_base);
                if init.shared_folder.is_some() {
                    target.shared_folder = init.shared_folder;
                }
            })
        });
        let cmd = ModelCmd::AddPane {
            ws: workspace_id.into(),
            agent: Some(pane.agent),
            near: pane.near,
            dir: None,
            cwd: Some(pane.cwd),
            name: Some(pane.name),
            init,
        };
        self.write(cmd).await?.pane
    }

    async fn close_pane(&self, workspace_id: &str, pane: &str) {
        self.write(ModelCmd::ClosePane {
            ws: workspace_id.into(),
            pane: pane.into(),
        })
        .await;
    }

    /// Hibernation's `touchWorkspace`, through P18 to P4: a workspace a
    /// worker was just started or stopped in does not fall asleep on the
    /// person's idle delay.
    fn touch(&self, workspace_id: &str) {
        self.tell(ModelCmd::Touch(workspace_id.into()));
    }

    fn open_workspace(&self, workspace_id: &str) {
        self.tell(ModelCmd::OpenWorkspace(workspace_id.into()));
    }

    /// On the blocking pool: the kill takes the PTY manager's lock, which
    /// the panes' readers hold, and an async thread waiting on it stalls
    /// every task scheduled behind it.
    async fn kill_pane(&self, pane: &str) -> Result<(), String> {
        // A pane of a remote workspace is its node's to kill.
        if let Some(killed) = crate::nodes::remote::kill_pane(pane).await {
            return killed;
        }
        let (pty, pane) = (
            self.handle.services().core().pty().clone(),
            pane.to_string(),
        );
        on_pty(pty, move |pty| {
            hangar_core::pty::pty_kill(pty, pane, Some(true))
        })
        .await?
    }

    async fn alive(&self, pane: &str) -> bool {
        if let Some(alive) = crate::nodes::remote::alive(pane) {
            return alive;
        }
        let (pty, pane) = (
            self.handle.services().core().pty().clone(),
            pane.to_string(),
        );
        on_pty(pty, move |pty| hangar_core::pty::pty_alive(pty, pane))
            .await
            .unwrap_or(false)
    }

    async fn enqueue(&self, pane: &str, text: &str, from: &str) -> Result<(), String> {
        self.enqueue_now(pane, text, from);
        Ok(())
    }

    async fn dir_exists(&self, path: &str) -> bool {
        let path = path.to_string();
        blocking(move || hangar_core::store::dir_exists(path))
            .await
            .unwrap_or(false)
    }

    async fn sleep(&self, ms: u64) {
        tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
    }

    async fn board_load(&self, cwd: &str) -> Result<Vec<Task>, String> {
        let cwd = cwd.to_string();
        blocking(move || hangar_core::board_load(cwd).tasks).await
    }

    async fn claim(
        &self,
        cwd: &str,
        id: &str,
        worker: &str,
        shared_folder: bool,
    ) -> Result<Task, String> {
        let (host, cwd, id, worker) = (
            self.host(),
            cwd.to_string(),
            id.to_string(),
            worker.to_string(),
        );
        blocking(move || {
            hangar_core::board_claim_worker(&host, cwd, id, worker, Some(shared_folder))
        })
        .await?
    }

    async fn release(&self, cwd: &str, id: &str, worker: &str, undo: bool) -> Result<Task, String> {
        let (host, cwd, id, worker) = (
            self.host(),
            cwd.to_string(),
            id.to_string(),
            worker.to_string(),
        );
        blocking(move || hangar_core::board_release_worker(&host, cwd, id, worker, undo)).await?
    }

    async fn repo_info(&self, path: &str) -> RepoInfo {
        hangar_core::git::git_repo_info(path.into()).await
    }

    async fn worktree_list(&self, path: &str) -> Result<Vec<Worktree>, String> {
        hangar_core::git::git_worktree_list(path.into()).await
    }

    async fn worktree_add(
        &self,
        path: &str,
        worktree_path: &str,
        branch: &str,
        create_branch: bool,
        base_ref: Option<&str>,
    ) -> Result<(), String> {
        hangar_core::git::git_worktree_add(
            path.into(),
            worktree_path.into(),
            branch.into(),
            create_branch,
            base_ref.map(str::to_string),
        )
        .await
    }

    /// Without the workspace's `preRemove` hook, as lib/workerControl.ts
    /// called it: the worktree is only removed once nothing in it is kept.
    async fn worktree_remove(
        &self,
        path: &str,
        worktree_path: &str,
        force: bool,
    ) -> Result<RemoveResult, String> {
        hangar_core::git::git_worktree_remove(path.into(), worktree_path.into(), force, None).await
    }

    async fn worktree_hook(
        &self,
        path: &str,
        worktree_path: &str,
        command: &str,
    ) -> Result<Option<HookReport>, String> {
        hangar_core::git::git_worktree_hook(path.into(), worktree_path.into(), Some(command.into()))
            .await
    }

    async fn branch_delete(
        &self,
        path: &str,
        branch: &str,
        force: bool,
    ) -> Result<DeleteBranchResult, String> {
        hangar_core::git::git_branch_delete(path.into(), branch.into(), force).await
    }

    async fn copy_ignored(
        &self,
        from: &str,
        to: &str,
        patterns: &[&str],
    ) -> Result<CopyResult, String> {
        let patterns = patterns.iter().map(|pattern| pattern.to_string()).collect();
        hangar_core::git::git_copy_untracked(from.into(), to.into(), patterns, Some(true)).await
    }

    async fn changes(&self, path: &str) -> Result<Changes, String> {
        hangar_core::git::git_changes(path.into(), None).await
    }

    async fn commits_ahead(&self, path: &str, base: &str, head: &str) -> Result<u32, String> {
        hangar_core::git::git_commits_ahead(path.into(), base.into(), head.into()).await
    }
}

impl BriefingPort for LivePort {
    fn state(&self) -> Arc<AppState> {
        self.snapshot().state.clone()
    }

    fn last_check(&self, workspace_id: &str) -> Option<CheckCost> {
        last_check(workspace_id)
    }

    async fn load_board(&self, cwd: &str) -> Result<Vec<Task>, String> {
        let cwd = cwd.to_string();
        blocking(move || hangar_core::board_load(cwd).tasks).await
    }

    async fn load_memory(&self) -> Result<Vec<MemoryEntry>, String> {
        let host = self.host();
        blocking(move || hangar_core::memory::memory_load(&host).map(|memory| memory.entries))
            .await?
    }

    fn journal(&self, workspace_id: &str) -> Vec<JournalEntry> {
        self.handle.journal(workspace_id)
    }

    fn activity(&self, pane: &str) -> Option<PaneActivity> {
        self.snapshot().activity.get(pane).cloned()
    }

    fn now(&self) -> u64 {
        now_ms()
    }

    /// A repository when git says so; git missing reads as not one, as the
    /// TS's `repo.installed && repo.isRepo` did.
    async fn is_repo(&self, cwd: &str) -> Result<Option<bool>, String> {
        let repo = hangar_core::git::git_repo_info(cwd.into()).await;
        Ok(Some(repo.installed && repo.is_repo))
    }

    async fn load_projects(&self) -> Result<Vec<ProjectSummary>, String> {
        Ok(super::projects::load_projects(self).await)
    }

    fn manager_journal(&self) -> Vec<ManagerJournalEntry> {
        self.handle.manager_journal()
    }

    fn enqueue(&self, pane: &str, text: &str, from: &str) -> Result<(), String> {
        self.enqueue_now(pane, text, from);
        Ok(())
    }
}

impl ProjectsPort for LivePort {
    fn state(&self) -> Arc<AppState> {
        self.snapshot().state.clone()
    }

    async fn load_board(&self, cwd: &str) -> Result<Vec<Task>, String> {
        let cwd = cwd.to_string();
        blocking(move || hangar_core::board_load(cwd).tasks).await
    }

    async fn load_memory(&self) -> Result<Vec<MemoryEntry>, String> {
        let host = self.host();
        blocking(move || hangar_core::memory::memory_load(&host).map(|memory| memory.entries))
            .await?
    }

    /// Without git installed, nobody can tell.
    async fn is_repo(&self, cwd: &str) -> Result<Option<bool>, String> {
        let repo = hangar_core::git::git_repo_info(cwd.into()).await;
        Ok(repo.installed.then_some(repo.is_repo))
    }

    fn activity(&self, pane: &str) -> Option<PaneActivity> {
        self.snapshot().activity.get(pane).cloned()
    }

    fn asleep(&self, workspace_id: &str) -> bool {
        self.snapshot().asleep.contains(workspace_id)
    }

    fn is_open(&self, workspace_id: &str) -> bool {
        open_in(&self.snapshot(), workspace_id)
    }

    fn journal(&self, workspace_id: &str) -> Vec<JournalEntry> {
        self.handle.journal(workspace_id)
    }
}

/// Whether a workspace's panes are up: opened in the main window during this
/// run, or shown in a window of its own (P18's `is_open`, App.tsx's
/// `isOpen`). A workspace detached when Hangar last quit draws its panes
/// without ever being opened in the main window; read off the opened set
/// alone, its workers would be listed as stopped while they run.
pub fn open_in(snapshot: &Snapshot, workspace_id: &str) -> bool {
    workspace_in(snapshot, workspace_id).is_some_and(|ws| is_open(snapshot, ws))
}

/// `f` on the blocking pool for the panes' shells: the PTY manager's lock is
/// the one every pane's reader and writer takes, and a kill reaps the shell.
async fn on_pty<T: Send + 'static>(
    pty: Arc<PtyManager>,
    f: impl FnOnce(&Arc<PtyManager>) -> T + Send + 'static,
) -> Result<T, String> {
    blocking(move || f(&pty)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::types::Workspace;

    #[test]
    fn a_workspace_in_a_window_of_its_own_is_open_without_being_opened() {
        let workspace = |id: &str, detached| Workspace {
            id: id.into(),
            detached,
            ..Workspace::default()
        };
        let snapshot = Snapshot {
            state: Arc::new(AppState {
                workspaces: vec![
                    workspace("main", None),
                    workspace("site", Some(true)),
                    workspace("shut", Some(false)),
                ],
                ..AppState::default()
            }),
            opened: ["main".to_string()].into(),
            ..Snapshot::default()
        };
        assert!(open_in(&snapshot, "main"));
        assert!(open_in(&snapshot, "site"));
        assert!(!open_in(&snapshot, "shut"));
        assert!(!open_in(&snapshot, "gone"));
    }

    #[tokio::test]
    async fn answers_the_pty_questions_through_the_blocking_pool() {
        let pty = Arc::new(PtyManager::default());
        let alive = on_pty(pty.clone(), |pty| {
            hangar_core::pty::pty_alive(pty, "nobody".into())
        })
        .await;
        assert_eq!(alive, Ok(false));
        // A pane with no shell: nothing to kill, and no error.
        let killed = on_pty(pty, |pty| {
            hangar_core::pty::pty_kill(pty, "nobody".into(), Some(true))
        })
        .await;
        assert_eq!(killed, Ok(Ok(())));
    }
}

#[cfg(test)]
mod check_cost_tests {
    use super::*;

    #[test]
    fn the_last_check_of_a_workspace_is_the_one_kept() {
        assert_eq!(last_check("ws-never-checked"), None);
        record_check("ws-checked", 700, true);
        record_check("ws-checked", 95, false);
        assert_eq!(
            last_check("ws-checked"),
            Some(CheckCost {
                seconds: 95,
                passed: false
            })
        );
        assert_eq!(last_check("ws-other"), None);
    }
}
