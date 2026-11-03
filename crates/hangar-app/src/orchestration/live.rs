//! The ports over the running app: the snapshot for reads, hangar-core for
//! boards, git, the panes' shells and the bridge queue, and the inbox for the
//! model writes, which the package's `update` applies on the UI thread
//! (PACKAGES.md, rule 6).
//!
//! The model writes ride in this package's own message,
//! `Message::Orchestration(Msg::Model(write, answer))`, because P18's
//! `ModelCmd` was still empty when this was written. Once P18 defines
//! `ModelCmd::{AddPane, ClosePane, OpenWorkspace, Touch, Announce}`, these
//! sends become `Message::Control(control::Msg::Model(cmd, answer))` and the
//! package's `Msg::Model` goes away.

use std::sync::{Arc, Mutex};

use hangar_core::board::Task;
use hangar_core::git::{Changes, CopyResult, DeleteBranchResult, HookReport, RemoveResult, RepoInfo, Worktree};
use hangar_core::memory::MemoryEntry;
use hangar_i18n::{Key, Locale, args, t, t_with};

use super::brief::BriefingPort;
use super::events::{EngineIo, JournalEntry, OrchestratorEvent};
use super::manager_feed::ManagerJournalEntry;
use super::port::{AnnounceKind, Announcement, NewPane};
use super::projects::{ProjectSummary, ProjectsPort};
use super::runtime::OrchestrationHandle;
use super::text::now_ms;
use super::worker::{Starting, WorkerPort};
use super::{ModelWrite, Msg};
use crate::message::{Intent, Message};
use crate::services::inbox::Answer;
use crate::shell::Notice;
use crate::state::types::{AppState, PaneId};
use crate::state::{PaneActivity, Snapshot};

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

impl LivePort {
    pub(super) fn new(handle: OrchestrationHandle) -> Self {
        LivePort { handle }
    }

    fn snapshot(&self) -> Arc<Snapshot> {
        self.handle.services().snapshot()
    }

    fn host(&self) -> hangar_core::Host {
        self.handle.services().core().host().clone()
    }

    fn locale(&self) -> Locale {
        self.snapshot().state.settings.locale.unwrap_or_else(Locale::system)
    }

    /// A model write, applied by the package's `update`; `None` when the app
    /// went away before answering.
    async fn write(&self, write: ModelWrite) -> Option<crate::state::StoreOutcome> {
        let (answer, outcome) = Answer::channel();
        if !self.handle.services().send(Message::Orchestration(Msg::Model(write, answer))) {
            return None;
        }
        outcome.await.ok()
    }

    fn enqueue_now(&self, pane: &str, text: &str, from: &str) {
        hangar_core::bridge::bridge_enqueue(&self.host(), pane.into(), text.into(), Some(from.into()));
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
    /// problem 4).
    fn is_open(&self, workspace_id: &str) -> bool {
        self.snapshot().opened.contains(workspace_id)
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
    /// `announce` showed it, in the person's language.
    fn announce(&self, announcement: Announcement) {
        let locale = self.locale();
        let by = match &announcement.by {
            Some(by) => by.clone(),
            None => t(locale, Key::ControlSomeone).into_owned(),
        };
        let key = match announcement.kind {
            AnnounceKind::Created => Key::ControlCreated,
            AnnounceKind::Closed => Key::ControlClosed,
        };
        let text = t_with(
            locale,
            key,
            &args!("by" => by, "pane" => &announcement.pane, "workspace" => &announcement.workspace),
        );
        self.handle
            .services()
            .send(Message::Intent(Intent::Notice(Notice::Agent(text.into_owned()))));
    }

    async fn add_pane(&self, workspace_id: &str, pane: NewPane) -> Option<PaneId> {
        self.write(ModelWrite::AddPane { ws: workspace_id.into(), pane }).await?.pane
    }

    async fn close_pane(&self, workspace_id: &str, pane: &str) {
        self.write(ModelWrite::ClosePane { ws: workspace_id.into(), pane: pane.into() }).await;
    }

    /// Hibernation's `touchWorkspace` is P4's, which has no way in from an
    /// engine yet: until it has (P18's planned `ModelCmd::Touch`), a
    /// workspace a worker was just started or stopped in can still fall
    /// asleep on the person's idle delay. Nothing else depends on it.
    fn touch(&self, workspace_id: &str) {
        let _ = workspace_id;
    }

    fn open_workspace(&self, workspace_id: &str) {
        let (answer, _) = Answer::channel();
        let write = ModelWrite::OpenWorkspace(workspace_id.into());
        self.handle.services().send(Message::Orchestration(Msg::Model(write, answer)));
    }

    async fn kill_pane(&self, pane: &str) -> Result<(), String> {
        hangar_core::pty::pty_kill(self.handle.services().core().pty(), pane.into(), Some(true))
    }

    async fn alive(&self, pane: &str) -> bool {
        hangar_core::pty::pty_alive(self.handle.services().core().pty(), pane.into())
    }

    async fn enqueue(&self, pane: &str, text: &str, from: &str) -> Result<(), String> {
        self.enqueue_now(pane, text, from);
        Ok(())
    }

    async fn dir_exists(&self, path: &str) -> bool {
        let path = path.to_string();
        blocking(move || hangar_core::store::dir_exists(path)).await.unwrap_or(false)
    }

    async fn sleep(&self, ms: u64) {
        tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
    }

    async fn board_load(&self, cwd: &str) -> Result<Vec<Task>, String> {
        let cwd = cwd.to_string();
        blocking(move || hangar_core::board_load(cwd).tasks).await
    }

    async fn claim(&self, cwd: &str, id: &str, worker: &str, max_attempts: u32, shared_folder: bool) -> Result<Task, String> {
        let (host, cwd, id, worker) = (self.host(), cwd.to_string(), id.to_string(), worker.to_string());
        blocking(move || hangar_core::board_claim_worker(&host, cwd, id, worker, max_attempts, Some(shared_folder))).await?
    }

    async fn release(&self, cwd: &str, id: &str, worker: &str, undo: bool) -> Result<Task, String> {
        let (host, cwd, id, worker) = (self.host(), cwd.to_string(), id.to_string(), worker.to_string());
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
    async fn worktree_remove(&self, path: &str, worktree_path: &str, force: bool) -> Result<RemoveResult, String> {
        hangar_core::git::git_worktree_remove(path.into(), worktree_path.into(), force, None).await
    }

    async fn worktree_hook(&self, path: &str, worktree_path: &str, command: &str) -> Result<Option<HookReport>, String> {
        hangar_core::git::git_worktree_hook(path.into(), worktree_path.into(), Some(command.into())).await
    }

    async fn branch_delete(&self, path: &str, branch: &str, force: bool) -> Result<DeleteBranchResult, String> {
        hangar_core::git::git_branch_delete(path.into(), branch.into(), force).await
    }

    async fn copy_ignored(&self, from: &str, to: &str, patterns: &[&str]) -> Result<CopyResult, String> {
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

    async fn load_board(&self, cwd: &str) -> Result<Vec<Task>, String> {
        let cwd = cwd.to_string();
        blocking(move || hangar_core::board_load(cwd).tasks).await
    }

    async fn load_memory(&self) -> Result<Vec<MemoryEntry>, String> {
        let host = self.host();
        blocking(move || hangar_core::memory::memory_load(&host).map(|memory| memory.entries)).await?
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
        blocking(move || hangar_core::memory::memory_load(&host).map(|memory| memory.entries)).await?
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
        self.snapshot().opened.contains(workspace_id)
    }

    fn journal(&self, workspace_id: &str) -> Vec<JournalEntry> {
        self.handle.journal(workspace_id)
    }
}
