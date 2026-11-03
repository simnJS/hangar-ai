//! Hangar Mobile over the running app: the `Sources` the operations read
//! (core's commands, P11's statistics cache, P19's journals and projects),
//! and the two things that run on their own once the state is in, as
//! `useRemoteControl` and `startRemotePush` started them: the activity log,
//! read every 5 s on P19's ticker, and the push, on a task of its own that
//! keeps its timers and asks every 10 s how many phones listen.
//!
//! `remote_state` is asked every 2 s by each phone, and the projects it
//! summarizes mean every board, the memory and git: the summaries are kept a
//! few seconds, dropped as soon as a board or the memory changes or the
//! state does, and whether a folder is a git repository is asked of git once
//! every 5 minutes.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::Notify;

use hangar_core::board::Task;
use hangar_core::chat::ChatPage;
use hangar_core::git::{ChangedFile, Changes, FileDiff};
use hangar_core::memory::MemoryEntry;
use hangar_core::remote::{ActivityEntry, Remote, Share, Text};
use hangar_core::stats::UsageStats;

use crate::control::port::{BoxFuture, is_open};
use crate::orchestration::OrchestrationHandle;
use crate::orchestration::events::JournalEntry;
use crate::orchestration::manager_feed::{ManagerJournalEntry, ManagerNewsEntry};
use crate::orchestration::projects::{ProjectSummary, ProjectsPort, load_projects};
use crate::services::Services;
use crate::state::live::BridgeEvent;
use crate::state::{AgentActivity, AppState, PaneActivity, PaneId, Snapshot, WsId};

use super::activity_log::{ActivityIo, ActivityLog, app_started};
use super::engine::{ChatRead, Mobile, Sources, locked};
use super::push::{Push, PushMessage, PushSink};

/// The summaries stand this long at most.
const PROJECTS_FRESH_MS: u64 = 5_000;
/// Whether a folder is a repository changes seldom.
const REPO_FRESH_MS: u64 = 5 * 60_000;
/// How often the push asks how many phones listen (`useOrchestrator`'s 10 s).
const REFRESH: Duration = Duration::from_secs(10);

fn now_ms() -> u64 {
    crate::control::types::now_ms()
}

/// `f` on tokio's blocking pool: board files, the memory, the history.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|err| format!("the task stopped: {err}"))
}

/// The summaries as last read, with what they were read from.
struct Kept {
    at: u64,
    state: Arc<AppState>,
    generation: u64,
    summaries: Vec<ProjectSummary>,
}

/// What reads the projects for `remote_state` (`projectsDeps`).
struct ProjectsReader {
    services: Services,
    handle: OrchestrationHandle,
    repos: Mutex<HashMap<String, (u64, Option<bool>)>>,
}

impl ProjectsReader {
    fn snapshot(&self) -> Arc<Snapshot> {
        self.services.snapshot()
    }
}

impl ProjectsPort for ProjectsReader {
    fn state(&self) -> Arc<AppState> {
        self.snapshot().state.clone()
    }

    async fn load_board(&self, cwd: &str) -> Result<Vec<Task>, String> {
        let cwd = cwd.to_string();
        blocking(move || hangar_core::board_load(cwd).tasks).await
    }

    async fn load_memory(&self) -> Result<Vec<MemoryEntry>, String> {
        let host = self.services.core().host().clone();
        blocking(move || hangar_core::memory::memory_load(&host).map(|memory| memory.entries))
            .await?
    }

    async fn is_repo(&self, cwd: &str) -> Result<Option<bool>, String> {
        let now = now_ms();
        let known = locked(&self.repos)
            .get(cwd)
            .filter(|(at, _)| now.saturating_sub(*at) < REPO_FRESH_MS)
            .map(|(_, repo)| *repo);
        if let Some(repo) = known {
            return Ok(repo);
        }
        // Without git installed, nobody can tell.
        let info = hangar_core::git::git_repo_info(cwd.to_string()).await;
        let repo = info.installed.then_some(info.is_repo);
        locked(&self.repos).insert(cwd.to_string(), (now, repo));
        Ok(repo)
    }

    fn activity(&self, pane: &str) -> Option<PaneActivity> {
        self.snapshot().activity.get(pane).cloned()
    }

    fn asleep(&self, workspace_id: &str) -> bool {
        self.snapshot().asleep.contains(workspace_id)
    }

    fn is_open(&self, workspace_id: &str) -> bool {
        let snapshot = self.snapshot();
        snapshot
            .state
            .workspaces
            .iter()
            .find(|ws| ws.id == workspace_id)
            .is_some_and(|ws| is_open(&snapshot, ws))
    }

    fn journal(&self, workspace_id: &str) -> Vec<JournalEntry> {
        self.handle.journal(workspace_id)
    }
}

/// What the log reads, taken once before it runs on the blocking pool.
struct Gathered {
    state: Arc<AppState>,
    news: Vec<ManagerNewsEntry>,
    journals: HashMap<WsId, Vec<JournalEntry>>,
    asleep: HashSet<WsId>,
    waiting: HashSet<PaneId>,
}

impl Gathered {
    fn of(services: &Services, handle: &OrchestrationHandle) -> Gathered {
        let snapshot = services.snapshot();
        let journals = snapshot
            .state
            .workspaces
            .iter()
            .filter(|ws| ws.hq != Some(true))
            .map(|ws| (ws.id.clone(), handle.journal(&ws.id)))
            .collect();
        Gathered {
            state: snapshot.state.clone(),
            news: handle.manager_news(),
            journals,
            asleep: snapshot.asleep.clone(),
            waiting: waiting_panes(&snapshot),
        }
    }
}

fn waiting_panes(snapshot: &Snapshot) -> HashSet<PaneId> {
    snapshot
        .activity
        .iter()
        .filter(|(_, entry)| entry.activity == AgentActivity::Waiting)
        .map(|(pane, _)| pane.clone())
        .collect()
}

impl ActivityIo for Gathered {
    fn state(&self) -> Arc<AppState> {
        self.state.clone()
    }

    fn manager_news(&self) -> Vec<ManagerNewsEntry> {
        self.news.clone()
    }

    fn orchestrator_journal(&self, workspace_id: &str) -> Vec<JournalEntry> {
        self.journals.get(workspace_id).cloned().unwrap_or_default()
    }

    fn is_asleep(&self, workspace_id: &str) -> bool {
        self.asleep.contains(workspace_id)
    }

    fn is_waiting(&self, pane: &str) -> bool {
        self.waiting.contains(pane)
    }
}

/// Hangar Mobile's half of the window, running.
pub struct Runtime {
    pub mobile: Arc<Mobile>,
    live: Arc<Inner>,
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("mobile::Runtime(..)")
    }
}

/// Everything the live sources keep, shared with the futures they hand out.
struct Inner {
    services: Services,
    handle: OrchestrationHandle,
    remote: Option<Arc<Remote>>,
    projects: ProjectsReader,
    kept: tokio::sync::Mutex<Option<Kept>>,
    /// Bumped when a board or the memory changed: the summaries are read again.
    generation: Mutex<u64>,
    log: Arc<Mutex<ActivityLog>>,
    push: Arc<Mutex<Push>>,
    push_wake: Arc<Notify>,
}

/// The sources the operations read, over the running app.
struct LiveSources(Arc<Inner>);

impl Inner {
    /// The summaries, read again unless the last ones still stand. Phones
    /// asking together wait for one read.
    async fn projects(&self) -> Vec<ProjectSummary> {
        let mut kept = self.kept.lock().await;
        let state = self.services.snapshot().state.clone();
        let generation = *locked(&self.generation);
        let now = now_ms();
        if let Some(last) = kept.as_ref()
            && last.generation == generation
            && Arc::ptr_eq(&last.state, &state)
            && now.saturating_sub(last.at) < PROJECTS_FRESH_MS
        {
            return last.summaries.clone();
        }
        let summaries = load_projects(&self.projects).await;
        *kept = Some(Kept {
            at: now_ms(),
            state,
            generation,
            summaries: summaries.clone(),
        });
        summaries
    }
}

fn write_log(log: &Mutex<ActivityLog>, remote: Option<&Remote>) {
    locked(log).write(&|entries: &[ActivityEntry]| match remote {
        Some(remote) => remote.activity_append(entries),
        None => Err("Hangar Mobile is unavailable.".into()),
    });
}

/// Asks how many phones take notifications (`refresh`).
async fn refresh(push: &Mutex<Push>, remote: Option<Arc<Remote>>) {
    let answer = match remote {
        Some(remote) => hangar_core::remote::remote_push_subscribers(remote).await,
        None => Err("Hangar Mobile is unavailable.".into()),
    };
    locked(push).refreshed(answer);
}

/// One notification to every subscribed phone; one that fails is not the
/// next one's business.
async fn send(remote: Option<Arc<Remote>>, message: PushMessage) {
    let Some(remote) = remote else {
        return;
    };
    let text = |words: super::push::Localized| Text::ByLang {
        en: words.en,
        fr: Some(words.fr),
    };
    let _ = hangar_core::remote::remote_push(
        remote,
        text(message.title),
        text(message.body),
        message.url,
    )
    .await;
}

/// The push's own task: its timers, and every 10 s how many phones listen.
async fn run_push(inner: Arc<Inner>) {
    let mut every = tokio::time::interval(REFRESH);
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        let wake = locked(&inner.push).next_wake();
        let deadline = async {
            match wake {
                Some(at) => {
                    tokio::time::sleep(Duration::from_millis(at.saturating_sub(now_ms()))).await
                }
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            () = deadline => {
                let waiting = waiting_panes(&inner.services.snapshot());
                let is_waiting = |pane: &str| waiting.contains(pane);
                let now = now_ms();
                let mut messages = Vec::new();
                {
                    let mut push = locked(&inner.push);
                    while push.due(now) {
                        if let Some(message) = push.fire_due(now, &is_waiting) {
                            messages.push(message);
                        }
                    }
                }
                for message in messages {
                    send(inner.remote.clone(), message).await;
                }
            }
            _ = every.tick() => refresh(&inner.push, inner.remote.clone()).await,
            () = inner.push_wake.notified() => {}
        }
    }
}

impl Runtime {
    /// Starts Hangar Mobile's side once the state is in and P19's runtime
    /// runs: the push beside the manager, the activity log on its ticker.
    pub fn start(services: Services, handle: OrchestrationHandle) -> Runtime {
        let remote = services.core().core().remote().cloned();
        let inner = Arc::new(Inner {
            projects: ProjectsReader {
                services: services.clone(),
                handle: handle.clone(),
                repos: Mutex::new(HashMap::new()),
            },
            services,
            handle,
            remote,
            kept: tokio::sync::Mutex::new(None),
            generation: Mutex::new(0),
            log: Arc::new(Mutex::new(ActivityLog::default())),
            push: Arc::new(Mutex::new(Push::default())),
            push_wake: Arc::new(Notify::new()),
        });
        let mobile = Mobile::new(Arc::new(LiveSources(inner.clone())));

        // The push: told by the manager feed, on its own pace.
        let sink = {
            let (services, wake) = (inner.services.clone(), inner.push_wake.clone());
            PushSink {
                push: inner.push.clone(),
                state: Box::new(move || services.snapshot().state.clone()),
                now: Box::new(now_ms),
                wake: Box::new(move || wake.notify_one()),
            }
        };
        inner.handle.set_push(Box::new(sink));
        tokio::spawn(run_push(inner.clone()));

        // The history, written from the moment the state is in.
        {
            let (log, remote, state) = (
                inner.log.clone(),
                inner.remote.clone(),
                inner.services.snapshot().state.clone(),
            );
            let gathered = Gathered::of(&inner.services, &inner.handle);
            tokio::task::spawn_blocking(move || {
                if let Some(remote) = &remote {
                    let _ = remote.activity_append(&[app_started(&state, now_ms())]);
                }
                locked(&log).tick(&gathered, now_ms());
                write_log(&log, remote.as_deref());
            });
        }
        // What the ticker keeps, without the handle it is handed: the
        // runtime holds the ticker, which holding the runtime would keep
        // alive for good.
        let (services, log, remote) = (
            inner.services.clone(),
            inner.log.clone(),
            inner.remote.clone(),
        );
        inner.handle.add_ticker(Box::new(move |handle, now| {
            let gathered = Gathered::of(&services, handle);
            let (log, remote) = (log.clone(), remote.clone());
            tokio::task::spawn_blocking(move || {
                locked(&log).tick(&gathered, now);
                write_log(&log, remote.as_deref());
            });
        }));
        Runtime {
            mobile,
            live: inner,
        }
    }

    /// A report of a pane's hangar-bridge mod.
    pub fn bridge(&self, pane: &str, event: &BridgeEvent) {
        self.mobile.note_bridge_event(pane, event, now_ms());
    }

    /// A file an agent shared with the phones: pushed, and written down.
    pub fn shared(&self, share: &Share) {
        let state = self.live.services.snapshot().state.clone();
        let now = now_ms();
        locked(&self.live.push).shared(share, &state, now);
        self.live.push_wake.notify_one();
        // The log may be writing to disk: the UI thread never waits for it.
        let (log, remote, share) = (
            self.live.log.clone(),
            self.live.remote.clone(),
            share.clone(),
        );
        tokio::task::spawn_blocking(move || {
            locked(&log).shared(&share, &state, now);
            write_log(&log, remote.as_deref());
        });
    }

    /// A phone paired, revoked, subscribed or not: how many listen now.
    pub fn remote_changed(&self) {
        let (push, remote) = (self.live.push.clone(), self.live.remote.clone());
        tokio::spawn(async move { refresh(&push, remote).await });
    }

    /// A board or the memory changed: the next `remote_state` reads them.
    pub fn projects_changed(&self) {
        *locked(&self.live.generation) += 1;
    }
}

impl Sources for LiveSources {
    fn projects(&self) -> BoxFuture<Vec<ProjectSummary>> {
        let inner = self.0.clone();
        Box::pin(async move { inner.projects().await })
    }

    fn manager_journal(&self) -> Vec<ManagerJournalEntry> {
        self.0.handle.manager_journal()
    }

    fn history(&self, since: u64) -> BoxFuture<Result<Vec<ActivityEntry>, String>> {
        let gathered = Gathered::of(&self.0.services, &self.0.handle);
        let (log, remote) = (self.0.log.clone(), self.0.remote.clone());
        Box::pin(async move {
            blocking(move || {
                // The latest lines included: the log reads the journals first.
                locked(&log).tick(&gathered, now_ms());
                write_log(&log, remote.as_deref());
                match remote {
                    Some(remote) => Ok(remote.activity_read(Some(since), None)),
                    None => Err("Hangar Mobile is unavailable.".to_string()),
                }
            })
            .await?
        })
    }

    fn chat(&self, read: ChatRead) -> BoxFuture<Result<Option<ChatPage>, String>> {
        Box::pin(hangar_core::chat::chat_page(
            read.session_id,
            read.cwd,
            read.before,
            read.limit,
        ))
    }

    fn usage(&self) -> BoxFuture<Result<Arc<UsageStats>, String>> {
        let host = self.0.services.core().host().clone();
        Box::pin(async move { crate::stats::usage(&host).await })
    }

    fn worktrees(&self, cwd: &str) -> BoxFuture<Result<Vec<String>, String>> {
        let cwd = cwd.to_string();
        Box::pin(async move { Ok(crate::stats::worktrees_of(&cwd).await) })
    }

    fn stats_now(&self) -> i64 {
        crate::stats::time::now_ms()
    }

    fn changes(&self, path: &str, base: &str) -> BoxFuture<Result<Changes, String>> {
        Box::pin(hangar_core::git::git_changes(
            path.to_string(),
            Some(base.to_string()),
        ))
    }

    fn file_diff(
        &self,
        path: &str,
        file: &ChangedFile,
        base: &str,
    ) -> BoxFuture<Result<FileDiff, String>> {
        Box::pin(hangar_core::git::git_file_diff(
            path.to_string(),
            file.path.clone(),
            file.old_path.clone(),
            file.status == "?",
            Some(base.to_string()),
        ))
    }

    fn wait(&self, ms: u64) -> BoxFuture<()> {
        Box::pin(tokio::time::sleep(Duration::from_millis(ms)))
    }

    fn listening(&self, ports: Vec<u16>) -> BoxFuture<Result<Vec<u16>, String>> {
        Box::pin(async move {
            Ok(super::apps::listening(&ports, |batch| {
                Box::pin(hangar_core::remote::remote_ports_listening(batch))
            })
            .await)
        })
    }

    fn tasks(&self, cwd: &str) -> BoxFuture<Result<Vec<(String, String)>, String>> {
        let cwd = cwd.to_string();
        Box::pin(async move {
            blocking(move || {
                hangar_core::board_load(cwd)
                    .tasks
                    .into_iter()
                    .map(|task| (task.id, task.title))
                    .collect()
            })
            .await
        })
    }
}
