//! What answers the phone's calls (`runRemote` of lib/remoteControl.ts): the
//! state the TS kept in module variables, held by one `Mobile` the package
//! makes on hydration, and the dispatch of the `remote_*` operations.
//!
//! An operation runs as the person, on P18's port: what it types into a pane
//! goes through `control::run_as_person` (`pane_send`, `pane_read`), never
//! refused for the agents' switch. What the port does not reach — the
//! projects, the journals, the transcripts, the statistics, git, the history
//! on disk — comes from `Sources`, the app's in the app (live.rs) and a fake
//! in the tests, as the TS took its readers as arguments.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

use serde_json::Value;

use hangar_core::chat::ChatPage;
use hangar_core::git::{ChangedFile, Changes, FileDiff};
use hangar_core::remote::ActivityEntry;
use hangar_core::stats::UsageStats;

use crate::control::port::{BoxFuture, MobileHooks, Port};
use crate::orchestration::manager_feed::ManagerJournalEntry;
use crate::orchestration::projects::ProjectSummary;
use crate::state::PaneId;
use crate::state::live::BridgeEvent;

use super::chat_live::{ChatLive, Plan};
use super::delivery::Landing;

/// A page of a transcript to read (`chat_page`'s arguments).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatRead {
    pub session_id: String,
    pub cwd: String,
    pub before: Option<String>,
    pub limit: Option<usize>,
}

/// What the operations read beyond the port.
pub trait Sources: Send + Sync {
    /// Every project but the HQ, summarized (`loadProjects`).
    fn projects(&self) -> BoxFuture<Vec<ProjectSummary>>;
    /// What lately waited for the person (`managerJournal`).
    fn manager_journal(&self) -> Vec<ManagerJournalEntry>;
    /// The history kept on disk, after `since`, the latest lines written
    /// first (`activityHistory.read`).
    fn history(&self, since: u64) -> BoxFuture<Result<Vec<ActivityEntry>, String>>;
    /// A page of a Claude Code transcript; `None` while there is none.
    fn chat(&self, read: ChatRead) -> BoxFuture<Result<Option<ChatPage>, String>>;
    /// The usage statistics, fresh for a minute (P11's cache).
    fn usage(&self) -> BoxFuture<Result<Arc<UsageStats>, String>>;
    /// The worktrees of the repository at `cwd`: their sessions are its
    /// workspace's.
    fn worktrees(&self, cwd: &str) -> BoxFuture<Result<Vec<String>, String>>;
    /// The statistics' clock, ms.
    fn stats_now(&self) -> i64;
    /// A worktree against the branch it lands on (`git_changes`).
    fn changes(&self, path: &str, base: &str) -> BoxFuture<Result<Changes, String>>;
    /// One file of it (`git_file_diff`).
    fn file_diff(
        &self,
        path: &str,
        file: &ChangedFile,
        base: &str,
    ) -> BoxFuture<Result<FileDiff, String>>;
    /// What `remote_land` waits for a landing at most.
    fn wait(&self, ms: u64) -> BoxFuture<()>;
    /// Which of these ports a server listens on, on this PC's loopback.
    fn listening(&self, ports: Vec<u16>) -> BoxFuture<Result<Vec<u16>, String>>;
    /// A board's tasks, by id and title, for the workers' apps.
    fn tasks(&self, cwd: &str) -> BoxFuture<Result<Vec<(String, String)>, String>>;
}

/// The TS's module state: what it remembered between two calls.
#[derive(Debug, Default)]
pub struct Book {
    /// Since when something waits, when the journals cannot say: the first
    /// time it was seen (`firstSeen`).
    pub first_seen: HashMap<String, u64>,
    /// Claude Code panes of a workspace woken from the phone, since when
    /// (`resuming`): their shells start at once, and Claude Code resumes in
    /// them seconds later, so until the mod says hello what is typed would
    /// land in the shell.
    pub resuming: HashMap<PaneId, u64>,
    /// Panes an answer is being typed into: a second one would type over it.
    pub answering: HashSet<PaneId>,
    /// What the panes' mods report of their calls and dialogs, and of the plan.
    pub chat: ChatLive,
}

/// Hangar Mobile's side of the window, one per app.
pub struct Mobile {
    sources: Arc<dyn Sources>,
    book: Mutex<Book>,
    /// The landings the phone started, by task (`landings`).
    pub(super) landings: Mutex<HashMap<String, Landing>>,
}

impl std::fmt::Debug for Mobile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mobile").finish_non_exhaustive()
    }
}

pub fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

impl Mobile {
    pub fn new(sources: Arc<dyn Sources>) -> Arc<Mobile> {
        Arc::new(Mobile {
            sources,
            book: Mutex::new(Book::default()),
            landings: Mutex::new(HashMap::new()),
        })
    }

    pub fn sources(&self) -> &dyn Sources {
        self.sources.as_ref()
    }

    pub fn book(&self) -> MutexGuard<'_, Book> {
        locked(&self.book)
    }

    /// Keeps what a pane's mod reports (`noteBridgeEvent`): its Claude Code
    /// is up, or has quit, so it no longer resumes either way.
    pub fn note_bridge_event(&self, pane: &str, event: &BridgeEvent, at: u64) {
        let mut book = self.book();
        book.resuming.remove(pane);
        book.chat.note(pane, event, at);
    }

    /// The plan's windows as the last session to report them saw them.
    pub fn plan(&self) -> Option<Plan> {
        self.book().chat.plan.clone()
    }
}

/// The handler P18's bus calls for every `remote_*` request.
pub struct Hook(pub Arc<Mobile>);

impl MobileHooks for Hook {
    fn remote(&self, port: Port, op: String, args: Value) -> BoxFuture<Result<Value, String>> {
        let mobile = self.0.clone();
        Box::pin(async move { run(&op, &args, &port, &mobile).await })
    }
}

/// One phone's call, answered (`runRemote`).
pub async fn run(op: &str, args: &Value, port: &Port, mobile: &Mobile) -> Result<Value, String> {
    use super::contract::value;
    if super::delivery::OPS.contains(&op) {
        return super::delivery::run(op, args, port, mobile, &super::ops::ToWorker).await;
    }
    match op {
        "remote_apps" => super::apps::remote_apps(port, args, mobile.sources())
            .await
            .and_then(|answer| value(&answer)),
        "remote_state" => value(&super::ops::remote_state(port, mobile).await),
        "remote_screen" => super::ops::remote_screen(port, mobile, args)
            .await
            .and_then(|answer| value(&answer)),
        "remote_send" => super::ops::remote_send(port, mobile, args)
            .await
            .and_then(|answer| value(&answer)),
        "remote_permission" => super::ops::remote_permission(port, mobile, args)
            .await
            .and_then(|answer| value(&answer)),
        "remote_manager" => super::ops::remote_manager(port, mobile, args)
            .await
            .and_then(|answer| value(&answer)),
        "remote_history" => super::ops::remote_history(port, mobile, args)
            .await
            .and_then(|answer| value(&answer)),
        "remote_chat" => super::chat_live::remote_chat(port, mobile, args)
            .await
            .and_then(|answer| value(&answer)),
        "remote_answer" => super::answer::remote_answer(port, mobile, args)
            .await
            .and_then(|answer| value(&answer)),
        "remote_wake" => super::ops::remote_wake(port, mobile, args)
            .await
            .and_then(|answer| value(&answer)),
        "remote_workspace" => {
            super::ops::remote_workspace(port, args).and_then(|answer| value(&answer))
        }
        "remote_stats" => super::stats::remote_stats(port, mobile, args)
            .await
            .and_then(|answer| value(&answer)),
        _ => Err(format!("Unknown remote request \"{op}\".")),
    }
}
