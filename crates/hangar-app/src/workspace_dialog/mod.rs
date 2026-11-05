//! P8: Workspace creation.
//!
//! Ports WorkspaceDialog.tsx (with WorkspaceKitsDialog and the worktree-hooks
//! section). Read inventory-frontend.md §2.1/11 and
//! crates/hangar-app/PACKAGES.md.
//!
//! One form, no wizard steps. A *Source* control switches between two sets of
//! fields: an existing folder (or a `.code-workspace` file) with the agents to
//! start in it, and a Git repository to clone first (`git_flow`, called as the
//! person: no consent to ask, they just chose).
//!
//! Files: `folder` (the folder half of the form, as data), `git_form` (the
//! status of the Git half), `hooks` (the worktree hooks section), `view` (the
//! dialog), `kits_dialog` (the kits of an existing workspace).

pub mod folder;
pub mod git_form;
pub mod hooks;
mod kits_dialog;
mod sweep;
#[cfg(test)]
pub(crate) mod testing;
#[cfg(test)]
mod tests;
mod view;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use hangar_i18n::{Key, Translator, args};
use iced::widget::{Id, operation};
use iced::{Element, Subscription, Task};

use crate::ctx::Ctx;
use crate::git_flow::AppDeps;
use crate::git_flow::clone::{
    CloneProgress, ParseResult, clean_repo_input, is_progress_of, new_clone_job_id,
    overall_percent, parse_repo_input,
};
use crate::git_flow::flow::{FromGitOutcome, FromGitRequest};
use crate::git_flow::paths::{projects_folder_of, resolve_parent};
use crate::message::{GitOrigin, Intent, Message};
use crate::services::{CoreEvent, Services, dialogs};
use crate::shell::{Notice, Overlay, Region};
use crate::state::types::{AgentId, WorkspaceKit};
use crate::state::{Model, Patch, StoreAction, WsId};

use folder::{FolderForm, Root};
use git_form::{FormKey, FromGitForm, FromGitStatus, from_git_key, from_git_status, one_line};
use hooks::{HookField, HooksDraft};

/// The pause in the typing after which an address is read.
const PARSE_DELAY: Duration = Duration::from_millis(250);
/// How often a cancel is asked again while the clone has not started yet. It
/// is asked until it lands or the flow ends: the job only exists once the
/// ownership checks (`gh api`, with no short timeout) are through, and a Stop
/// given up on before that would let the clone go ahead.
const STOP_RETRY: Duration = Duration::from_millis(200);

/// Tokens of `.code-workspace` reads, unique for the whole run: a `State` reset
/// by the dialog reopening must not hand an old read's token out again.
static READS: AtomicU64 = AtomicU64::new(1);

/// What the dialog creates a workspace from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Source {
    #[default]
    Folder,
    Git,
}

/// The folder half of the form.
#[derive(Debug)]
struct Folder {
    cwd: String,
    /// Empty until a `.code-workspace` is loaded; `cwd` is one of these.
    roots: Vec<Root>,
    /// A workspace file found next to the folder that was just picked.
    suggestion: Option<String>,
    /// The `.code-workspace` read whose answer the form waits for. A read for
    /// a folder since changed, or for a form since reset, is not applied.
    reading: Option<u64>,
    file_error: Option<String>,
    shell_id: Option<String>,
    layout: u8,
    /// One entry per pane: the source of truth the counters and the preview
    /// share.
    assignments: Vec<AgentId>,
    claude_account: Option<String>,
    hooks: HooksDraft,
}

impl Default for Folder {
    fn default() -> Self {
        Folder {
            cwd: String::new(),
            roots: Vec::new(),
            suggestion: None,
            reading: None,
            file_error: None,
            shell_id: None,
            layout: folder::DEFAULT_LAYOUT,
            assignments: folder::initial_assignments(folder::DEFAULT_LAYOUT),
            claude_account: None,
            hooks: HooksDraft::default(),
        }
    }
}

/// What reading an address said, and of which address.
#[derive(Debug, Clone)]
struct Parsed {
    input: String,
    result: ParseResult,
}

/// The Git half of the form.
#[derive(Debug, Default)]
struct Git {
    repo: String,
    /// The folder typed in the dialog; empty follows the setting, then the
    /// current workspace.
    parent: String,
    parsed: Option<Parsed>,
    /// Bumped by every edit of the address, so that a reading asked for an
    /// older one is not believed.
    generation: u64,
    running: bool,
    progress: Option<CloneProgress>,
    error: Option<String>,
    /// The job the dialog follows and cancels.
    job: Option<String>,
    /// Raised when that job's flow has ended, which is what stops a Stop
    /// still waiting for the job to exist.
    ended: Option<Arc<AtomicBool>>,
    cancelled: bool,
    /// When the clone began and the last tick since: the sweep of the
    /// indeterminate bar is the difference.
    began: Option<Instant>,
    beat: Option<Instant>,
}

impl Git {
    /// Whether the bar has nothing to measure yet: the clone runs, and git
    /// has not given a percentage the bar can use.
    fn indeterminate(&self) -> bool {
        self.running
            && self
                .progress
                .as_ref()
                .and_then(|progress| overall_percent(&progress.phase, progress.percent))
                .is_none()
    }

    /// How far into its animation the indeterminate bar is.
    fn sweep_elapsed(&self) -> Duration {
        match (self.began, self.beat) {
            (Some(began), Some(beat)) => beat.saturating_duration_since(began),
            _ => Duration::ZERO,
        }
    }

    /// What was said about the address as it is now, never about an older
    /// one.
    fn parse(&self) -> Option<&ParseResult> {
        self.parsed
            .as_ref()
            .filter(|parsed| parsed.input == clean_repo_input(&self.repo))
            .map(|parsed| &parsed.result)
    }
}

/// The package's UI state; lives in `App.pkg.workspace_dialog`.
#[derive(Debug, Default)]
pub struct State {
    source: Source,
    name: String,
    folder: Folder,
    git: Git,
    /// The workspace whose kits are open (`Overlay::KitsDialog`).
    kits: Option<kits_dialog::KitsDialog>,
}

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.workspace_dialog`.
///
/// Nothing yet: the kits chosen for a workspace being made are P14's
/// (`kits::draft_kits`), read when the form is sent.
#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::WorkspaceDialog(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    // The whole form.
    Source(Source),
    Name(String),
    /// Return, or the footer's button. Carries the kits picked in the section
    /// (`kits::draft_kits`), which only the view can read: they are P14's
    /// state, and `update` does not see another package's. Empty for a clone.
    Submit(Vec<WorkspaceKit>),
    /// The footer's Cancel.
    Cancel,
    /// The close cross, the veil and Escape: refused while a clone runs.
    Dismiss,

    // The folder half.
    Cwd(String),
    Browse,
    Picked(Option<PathBuf>),
    BrowseFile,
    FilePicked(Option<PathBuf>),
    /// `find_code_workspace` answered for the folder `for_cwd`.
    Suggestion {
        for_cwd: String,
        found: Option<String>,
    },
    UseSuggestion,
    DismissSuggestion,
    /// A `.code-workspace` was read, for the read numbered `read`.
    FileRead {
        read: u64,
        path: String,
        result: Result<Vec<Root>, String>,
    },
    PickRoot(String),
    Shell(Option<String>),
    Layout(u8),
    More(AgentId),
    Less(AgentId),
    Cycle(usize),
    Hook(HookField, String),
    /// The workspace account picker (`accounts::workspace_picker`).
    Account(Option<String>),

    // The Git half.
    Repo(String),
    Parent(String),
    BrowseParent,
    ParentPicked(Option<PathBuf>),
    ParseDue(u64),
    Parsed {
        generation: u64,
        input: String,
        result: ParseResult,
    },
    /// The flow ended (the job it ran under, to tell it from an older one).
    GitDone {
        job: String,
        outcome: FromGitOutcome,
    },
    Stop,
    /// The tick that moves the indeterminate bar.
    Tick(Instant),

    // The kits dialog.
    KitsHook(HookField, String),
}

/// The repository field (`autoFocus` in the web app, when Git is the source).
pub(crate) fn repo_id() -> Id {
    Id::new("workspace-dialog-repo")
}

/// The dialog is the overlay in front.
fn on_top(m: &Model) -> bool {
    m.ui.overlays.last() == Some(&Overlay::WorkspaceDialog)
}

/// Closes the dialog, if it is the overlay in front.
fn close(m: &Model) -> Task<Message> {
    if on_top(m) {
        Task::done(Message::Intent(Intent::CloseOverlay))
    } else {
        Task::none()
    }
}

fn notice(text: String) -> Task<Message> {
    Task::done(Message::Intent(Intent::Notice(Notice::Agent(text))))
}

impl State {
    /// The destination of a clone: the folder typed, else the setting, else
    /// the parent of the folder of the workspace in front.
    fn parent_for(&self, m: &Model) -> Option<String> {
        resolve_parent(
            Some(&self.git.parent),
            projects_folder_of(m.settings()).as_deref(),
            m.active_workspace().map(|ws| ws.cwd.as_str()),
        )
    }

    fn git_status(&self, m: &Model) -> FromGitStatus {
        from_git_status(&FromGitForm {
            input: &self.git.repo,
            parse: self.git.parse(),
            parent: self.parent_for(m).as_deref(),
            running: self.git.running,
        })
    }

    /// A clean form, as when the dialog opens.
    fn reset(&mut self) {
        *self = State::default();
    }
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    match msg {
        // The two that start or stop a clone are the only ones that reach
        // the services; the rest of the form is `form`.
        Msg::Submit(_) if st.source == Source::Git => start_git(st, m, sv),
        Msg::Stop => stop_clone(st),
        msg => form(st, m, msg),
    }
}

/// Everything the form does that needs no service: it only reads and writes
/// the dialog's state and the model.
fn form(st: &mut State, m: &mut Model, msg: Msg) -> Task<Message> {
    let t = Translator::new(m.locale());
    match msg {
        Msg::Source(source) => {
            if st.git.running {
                return Task::none();
            }
            st.source = source;
            // Reading the address follows the source, as the web app's effect
            // did.
            if source == Source::Git {
                // `autoFocus` of the repository field, which mounts with this
                // source. The dialog's layer already puts the keyboard in
                // the first field when it opens (the folder, or the
                // repository when a clone is still showing); only a switch
                // made inside the open dialog needs asking.
                return Task::batch([schedule_parse(st), operation::focus(repo_id())]);
            }
            Task::none()
        }
        Msg::Name(name) => {
            if !st.git.running {
                st.name = name;
            }
            Task::none()
        }
        Msg::Submit(kits) => match st.source {
            // Started by `update`, which has the services.
            Source::Git => Task::none(),
            Source::Folder => submit_folder(st, m, &kits),
        },
        Msg::Cancel => close(m),
        Msg::Dismiss => {
            if st.git.running {
                Task::none()
            } else {
                close(m)
            }
        }

        // ── the folder half ──
        Msg::Cwd(cwd) => {
            st.folder.cwd = cwd;
            // A folder typed by hand is not the loaded file's any more: its
            // other roots would go out with a workspace they are not part of,
            // and its pending read or suggestion is for another folder.
            st.folder.roots.clear();
            st.folder.reading = None;
            st.folder.suggestion = None;
            Task::none()
        }
        Msg::Browse => Task::perform(
            dialogs::pick_folder(t.t(Key::CreateFolderDialog).into_owned(), None),
            |picked| Message::WorkspaceDialog(Msg::Picked(picked)),
        ),
        Msg::Picked(None) => Task::none(),
        Msg::Picked(Some(path)) => {
            let selected = path.to_string_lossy().into_owned();
            st.folder.cwd = selected.clone();
            st.folder.roots.clear();
            st.folder.file_error = None;
            st.folder.suggestion = None;
            st.folder.reading = None;
            // Only auto-fill the name while the user has not typed their own.
            if st.name.trim().is_empty() {
                st.name = folder::basename(&selected);
            }
            // A folder holding a workspace file is usually meant to be opened
            // through it — usually, not always, so this is offered rather
            // than applied.
            let for_cwd = selected.clone();
            Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || {
                        hangar_core::workspace_file::find_code_workspace(selected)
                    })
                    .await
                    .ok()
                    .flatten()
                },
                move |found| {
                    Message::WorkspaceDialog(Msg::Suggestion {
                        for_cwd: for_cwd.clone(),
                        found,
                    })
                },
            )
        }
        Msg::BrowseFile => Task::perform(
            dialogs::pick_file(
                t.t(Key::CreateFileDialog).into_owned(),
                Some((
                    t.t(Key::CreateFileFilter).into_owned(),
                    vec!["code-workspace".to_string()],
                )),
            ),
            |picked| Message::WorkspaceDialog(Msg::FilePicked(picked)),
        ),
        Msg::FilePicked(None) => Task::none(),
        Msg::FilePicked(Some(path)) => read_workspace_file(st, path.to_string_lossy().into_owned()),
        Msg::Suggestion { for_cwd, found } => {
            // Late, for a folder the form has moved on from (or a form reset
            // since): it would offer another folder's file for this one.
            if for_cwd == st.folder.cwd {
                st.folder.suggestion = found;
            }
            Task::none()
        }
        Msg::UseSuggestion => match st.folder.suggestion.clone() {
            Some(path) => read_workspace_file(st, path),
            None => Task::none(),
        },
        Msg::DismissSuggestion => {
            st.folder.suggestion = None;
            Task::none()
        }
        Msg::FileRead { read, path, result } => {
            if st.folder.reading != Some(read) {
                return Task::none();
            }
            st.folder.reading = None;
            match result {
                Err(_) => st.folder.file_error = Some(path),
                Ok(found) => {
                    st.folder.suggestion = None;
                    st.folder.file_error = None;
                    // Terminals need a folder that is really there, so a file
                    // whose first entry is missing on this machine falls
                    // through to the first one that is not.
                    if let Some(main) = folder::main_root(&found) {
                        st.folder.cwd = main.path.clone();
                    }
                    st.folder.roots = found;
                    if st.name.trim().is_empty() {
                        st.name = folder::workspace_file_name(&path);
                    }
                }
            }
            Task::none()
        }
        Msg::PickRoot(path) => {
            if st
                .folder
                .roots
                .iter()
                .any(|root| root.path == path && root.exists)
            {
                st.folder.cwd = path;
            }
            Task::none()
        }
        Msg::Shell(id) => {
            st.folder.shell_id = id;
            Task::none()
        }
        Msg::Layout(layout) => {
            if folder::LAYOUTS.contains(&layout) {
                st.folder.layout = layout;
                folder::resize(&mut st.folder.assignments, layout);
            }
            Task::none()
        }
        Msg::More(agent) => {
            folder::add_agent(&mut st.folder.assignments, agent);
            Task::none()
        }
        Msg::Less(agent) => {
            folder::remove_agent(&mut st.folder.assignments, agent);
            Task::none()
        }
        Msg::Cycle(index) => {
            folder::cycle_cell(&mut st.folder.assignments, index);
            Task::none()
        }
        Msg::Hook(field, text) => {
            st.folder.hooks.set(field, text);
            Task::none()
        }
        Msg::Account(id) => {
            st.folder.claude_account = id;
            Task::none()
        }

        // ── the Git half ──
        Msg::Repo(repo) => {
            if st.git.running {
                return Task::none();
            }
            st.git.repo = repo;
            schedule_parse(st)
        }
        Msg::Parent(parent) => {
            if !st.git.running {
                st.git.parent = parent;
            }
            Task::none()
        }
        Msg::BrowseParent => Task::perform(
            dialogs::pick_folder(t.t(Key::CreateGitParentDialog).into_owned(), None),
            |picked| Message::WorkspaceDialog(Msg::ParentPicked(picked)),
        ),
        Msg::ParentPicked(picked) => {
            if let Some(path) = picked {
                st.git.parent = path.to_string_lossy().into_owned();
            }
            Task::none()
        }
        Msg::ParseDue(generation) => {
            if generation != st.git.generation || st.source != Source::Git {
                return Task::none();
            }
            let input = clean_repo_input(&st.git.repo).to_string();
            if input.is_empty() {
                return Task::none();
            }
            // Rust reads the address; a pause in the typing is enough to ask.
            Task::perform(
                async move {
                    let result = parse_repo_input(&input);
                    (input, result)
                },
                move |(input, result)| {
                    Message::WorkspaceDialog(Msg::Parsed {
                        generation,
                        input,
                        result,
                    })
                },
            )
        }
        Msg::Parsed {
            generation,
            input,
            result,
        } => {
            if generation == st.git.generation {
                st.git.parsed = Some(Parsed { input, result });
            }
            Task::none()
        }
        Msg::GitDone { job, outcome } => git_done(st, m, t, &job, outcome),
        // Started by `update`.
        Msg::Stop => Task::none(),
        Msg::Tick(at) => {
            st.git.beat = Some(at);
            Task::none()
        }

        // ── the kits dialog ──
        Msg::KitsHook(field, text) => {
            let Some(kits) = &mut st.kits else {
                return Task::none();
            };
            kits.hooks.set(field, text);
            let ws = kits.ws.clone();
            let hooks = kits.hooks.clean();
            // Live: each change is the workspace's, as the web app saved it.
            m.apply(StoreAction::UpdateWorkspace(
                ws,
                Patch::new(move |workspace: &mut crate::state::Workspace| {
                    workspace.worktree_hooks = hooks.clone();
                }),
            ));
            Task::none()
        }
    }
}

/// Reads the address again after a pause in the typing.
fn schedule_parse(st: &mut State) -> Task<Message> {
    st.git.generation += 1;
    if st.source != Source::Git || clean_repo_input(&st.git.repo).is_empty() {
        return Task::none();
    }
    let generation = st.git.generation;
    Task::perform(async { tokio::time::sleep(PARSE_DELAY).await }, move |()| {
        Message::WorkspaceDialog(Msg::ParseDue(generation))
    })
}

/// Splits a `.code-workspace` into the folder to open in and the rest. Only
/// the latest read is applied (`Folder::reading`).
fn read_workspace_file(st: &mut State, path: String) -> Task<Message> {
    let read = READS.fetch_add(1, Ordering::Relaxed);
    st.folder.reading = Some(read);
    let file = path.clone();
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                hangar_core::workspace_file::read_code_workspace(file).map(|roots| {
                    roots
                        .into_iter()
                        .map(|root| Root {
                            name: root.name,
                            path: root.path,
                            exists: root.exists,
                        })
                        .collect::<Vec<_>>()
                })
            })
            .await
            .map_err(|err| err.to_string())
            .and_then(|read| read)
        },
        move |result| Message::WorkspaceDialog(Msg::FileRead { read, path, result }),
    )
}

fn submit_folder(st: &mut State, m: &mut Model, kits: &[WorkspaceKit]) -> Task<Message> {
    let Some(draft) = folder::draft_of(&FolderForm {
        name: &st.name,
        cwd: &st.folder.cwd,
        roots: &st.folder.roots,
        layout: st.folder.layout,
        assignments: &st.folder.assignments,
        shell_id: st.folder.shell_id.as_deref(),
        kits,
        claude_account: st.folder.claude_account.as_deref(),
    }) else {
        return Task::none();
    };
    let made = m.apply(StoreAction::AddWorkspace(draft));
    let mut tasks = vec![close(m)];
    if let Some(id) = made.workspace {
        if let Some(hooks) = folder::hooks_of(&st.folder.hooks) {
            m.apply(StoreAction::UpdateWorkspace(
                id.clone(),
                Patch::new(move |workspace: &mut crate::state::Workspace| {
                    workspace.worktree_hooks = Some(hooks.clone());
                }),
            ));
        }
        // The new workspace is the one in front, and its terminals start.
        tasks.push(Task::done(Message::Intent(Intent::ActivateWorkspace(id))));
    }
    Task::batch(tasks)
}

/// « Clone and create workspace »: the flow of `git_flow`, called as the
/// person. The dialog picks the folder and the job id it follows and cancels.
fn start_git(st: &mut State, m: &mut Model, sv: &Services) -> Task<Message> {
    let status = st.git_status(m);
    let Some(parent) = st.parent_for(m) else {
        return Task::none();
    };
    if from_git_key("Enter", status) != Some(FormKey::Start) {
        return Task::none();
    }
    let job = new_clone_job_id();
    st.git.job = Some(job.clone());
    st.git.ended = Some(Arc::new(AtomicBool::new(false)));
    st.git.cancelled = false;
    st.git.error = None;
    st.git.progress = None;
    st.git.running = true;
    st.git.began = Some(Instant::now());
    st.git.beat = st.git.began;
    let request = FromGitRequest {
        repo: st.git.repo.clone(),
        name: Some(st.name.trim().to_string()).filter(|name| !name.is_empty()),
        parent: None,
        origin: GitOrigin::Person,
        asker_cwd: m.active_workspace().map(|ws| ws.cwd.clone()),
    };
    let deps = AppDeps::new(sv).clone_into(parent).job(job.clone());
    Task::perform(
        async move { crate::git_flow::run(&request, &deps).await },
        move |outcome| {
            Message::WorkspaceDialog(Msg::GitDone {
                job: job.clone(),
                outcome,
            })
        },
    )
}

fn git_done(
    st: &mut State,
    m: &mut Model,
    t: Translator,
    job: &str,
    outcome: FromGitOutcome,
) -> Task<Message> {
    if st.git.job.as_deref() != Some(job) {
        return Task::none();
    }
    st.git.job = None;
    if let Some(ended) = st.git.ended.take() {
        ended.store(true, Ordering::SeqCst);
    }
    st.git.running = false;
    let visible = on_top(m);
    match outcome {
        FromGitOutcome::Refused { reason, .. } => {
            let text = if st.git.cancelled {
                t.t(Key::CreateGitCancelled).into_owned()
            } else {
                t.t_with(
                    Key::CreateGitFailed,
                    &args!("reason" => one_line(&reason, 160).as_str()),
                )
                .into_owned()
            };
            if visible {
                st.git.error = Some(text);
                Task::none()
            } else {
                // The dialog was closed meanwhile: the person is told where
                // they are.
                Task::done(Message::Intent(Intent::Notice(Notice::Problem(text))))
            }
        }
        FromGitOutcome::Existing { workspace, .. } => Task::batch([
            notice(
                t.t_with(
                    Key::CreateGitExisting,
                    &args!("path" => workspace.cwd.as_str()),
                )
                .into_owned(),
            ),
            close(m),
        ]),
        FromGitOutcome::Created {
            workspace,
            reused_folder,
            ..
        } => {
            let told = if reused_folder {
                notice(
                    t.t_with(
                        Key::CreateGitReused,
                        &args!("path" => workspace.cwd.as_str()),
                    )
                    .into_owned(),
                )
            } else {
                Task::none()
            };
            Task::batch([told, close(m)])
        }
    }
}

/// Stops the clone. It may not have started yet (the address is being
/// checked): asked again until it lands or the flow ends.
fn stop_clone(st: &mut State) -> Task<Message> {
    st.git.cancelled = true;
    let Some(job) = st.git.job.clone() else {
        return Task::none();
    };
    let ended = st
        .git
        .ended
        .clone()
        .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    Task::perform(
        cancel_until(job, ended, STOP_RETRY, |job| {
            hangar_core::git_clone::git_clone_cancel(job.to_string())
        }),
        |_| Message::Noop,
    )
}

/// Asks `cancel` for `job` every `retry` until it says it cancelled it
/// (`true`) or the flow has `ended` (`false`).
async fn cancel_until(
    job: String,
    ended: Arc<AtomicBool>,
    retry: Duration,
    mut cancel: impl FnMut(&str) -> bool,
) -> bool {
    loop {
        if cancel(&job) {
            return true;
        }
        if ended.load(Ordering::SeqCst) {
            return false;
        }
        tokio::time::sleep(retry).await;
    }
}

pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    // The moving bar needs a frame now and then; nothing else in the dialog
    // does, so the tick runs only while it is on show.
    if st.source == Source::Git && st.git.indeterminate() && on_top(m) {
        iced::time::every(sweep::TICK).map(|at| Message::WorkspaceDialog(Msg::Tick(at)))
    } else {
        Subscription::none()
    }
}

/// What takes the focus when F6 reaches a region this package owns. It owns
/// none, so the shell never calls it.
pub fn focus_entry(_st: &State, _m: &Model, _region: Region) -> Option<Task<Message>> {
    None
}

/// Every intent, after the shell's own handling of it: `Some` when this
/// package acted on it.
pub fn on_intent(
    st: &mut State,
    m: &mut Model,
    _sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    match intent {
        Intent::OpenOverlay(Overlay::WorkspaceDialog) => {
            opened(st);
            Some(Task::none())
        }
        Intent::OpenOverlay(Overlay::KitsDialog(ws)) | Intent::OpenKitsDialog(ws) => {
            st.kits = kits_dialog::KitsDialog::open(m, ws);
            Some(Task::none())
        }
        _ => None,
    }
}

/// Every core event, after the shell's own handling of it.
pub fn on_event(
    st: &mut State,
    _m: &mut Model,
    _sv: &Services,
    event: &CoreEvent,
) -> Option<Task<Message>> {
    match event {
        CoreEvent::GitCloneProgress(progress) => follow(st, progress).then(Task::none),
        _ => None,
    }
}

/// The dialog opens: the form starts clean, unless a clone is still running
/// from when it was closed, which shows its progress again. (The kits section
/// empties its own draft when the same intent reaches it.)
fn opened(st: &mut State) {
    if !st.git.running {
        st.reset();
    }
}

/// Keeps the progress of the clone this dialog follows (the bus carries every
/// clone's). Returns whether it was that one's.
fn follow(st: &mut State, progress: &CloneProgress) -> bool {
    match st.git.job.as_deref() {
        Some(job) if is_progress_of(progress, job) => {
            st.git.progress = Some(progress.clone());
            true
        }
        _ => false,
    }
}

/// `Overlay::WorkspaceDialog`: a new workspace. Embeds
/// `accounts::workspace_picker` and `kits::workspace_section`.
pub fn view<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    view::dialog(st, ctx)
}

/// `Overlay::KitsDialog`: a workspace's kits, around `kits::workspace_section`.
pub fn kits_dialog_view<'a>(st: &'a State, ctx: Ctx<'a>, ws: &'a WsId) -> Element<'a, Message> {
    kits_dialog::view(st.kits.as_ref().filter(|kits| kits.ws == *ws), ctx, ws)
}
