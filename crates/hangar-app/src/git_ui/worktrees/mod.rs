//! The "Git worktrees" modal (`WorktreePanel.tsx`): a workspace's worktrees,
//! one row each with what can be done to it, the cleanup of merged ones, and
//! the form that makes a new one.
//!
//! The panel's state ([`Panel`]) lives while the overlay is open and is made
//! again when it opens. Everything git does is asked of hangar-core as a task
//! and comes back as a message tagged with the panel's generation, so an
//! answer for an earlier opening is dropped. `busy` is one value saying which
//! write is in flight, and through it what is disabled; the destructive
//! actions confirm with a second click inline, never in a native dialog (that
//! would block the event loop the terminals run on).
//!
//! The "Changes" button of a row shows the diff view *instead of* the panel
//! and gives it back as it was: two modals stacked would both answer Escape,
//! and the panel's state (the form, the cleanup) must survive the visit, so
//! the diff is a field of the panel and not an overlay of its own.

pub mod cleanup;
pub mod create;
pub mod naming;
pub mod rows;
mod view;

use hangar_core::git::{
    self, Branch, DeleteBranchResult, MergedWorktree, MigrateResult, RemoveResult, RepoInfo,
    Worktree,
};
use hangar_i18n::{Key, Translator};
use iced::Task;

use self::cleanup::{BranchesRun, Cleanup, Run};
use self::create::{CreateRequest, Created, Mode, Resolved, resolve};
use self::naming::{sanitize_branch_name, worktree_folder};
use self::rows::{last_segment, same_path};
use super::diff_view::DiffState;
use super::{DiffMsg, Msg};
use crate::message::{Intent, Message};
use crate::shell::Overlay;
use crate::state::types::AgentId;
use crate::state::{Model, StoreAction, WorkspaceDraft, WsId};

#[cfg(test)]
pub(crate) use view::modal;
pub use view::view;

/// The key a branch is picked by.
pub(super) use self::rows::ref_key as key_of;

/// How long a "copied, suffixed, …" line stays before it stops being news.
const NOTICE_SECONDS: u64 = 7;

/// Which write is in flight. Anything but `None` disables every action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Busy {
    Create,
    Fetch,
    Prune,
    Cleanup,
    CleanupWorktree(String),
    CleanupBranches,
    Remove(String),
    Migrate(String),
    Branch(String),
    Lock(String),
}

/// The worktree whose Remove button is armed; only ever one at a time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Armed {
    pub path: String,
    pub branch: Option<String>,
}

/// A removal git turned down, kept per worktree so the row can offer force.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blocked {
    pub path: String,
    pub message: Option<String>,
    /// The preRemove command stopped it: the way out is « remove anyway ».
    pub hook: bool,
}

/// A branch left with no worktree by the removal that just went through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Orphan {
    pub name: String,
    pub needs_force: bool,
    pub message: Option<String>,
}

/// Says what a step did without stopping the flow; clears itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub warn: bool,
    pub text: String,
    /// Which notice this is, so that the timer of an earlier one does not
    /// clear a later one.
    seq: u64,
}

/// Which of the two branch pickers is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Picker {
    Existing,
    Base,
}

/// The form that makes a worktree.
#[derive(Debug, Clone)]
pub struct Form {
    pub mode: Mode,
    pub new_branch: String,
    /// The `ref_key` of the base picked, or `None` for the current branch.
    pub new_base: Option<String>,
    pub existing: Option<String>,
    pub folder: String,
    /// Until this flips, the folder field follows the branch name.
    pub folder_touched: bool,
    pub open_after: bool,
    pub copy_untracked: bool,
}

impl Default for Form {
    fn default() -> Form {
        Form {
            mode: Mode::New,
            new_branch: String::new(),
            new_base: None,
            existing: None,
            folder: String::new(),
            folder_touched: false,
            open_after: true,
            copy_untracked: true,
        }
    }
}

/// What reading the repository found.
#[derive(Debug, Clone)]
pub enum Loaded {
    /// No git, or the folder is not a repository: a state, not a failure.
    Unavailable(RepoInfo),
    Ready {
        info: RepoInfo,
        /// Settled, not all-or-nothing: `for-each-ref %(worktreepath)` needs a
        /// git newer (2.23) than the worktree commands themselves, and a panel
        /// that can list and remove worktrees is worth having even where the
        /// branch listing dies — it degrades to rows without commit metadata
        /// and a create form with no branch picker, beside the error saying
        /// why.
        list: Result<Vec<Worktree>, String>,
        refs: Result<Vec<Branch>, String>,
    },
}

/// What to do with what a reading found, once it is in.
#[derive(Debug, Clone)]
pub enum After {
    Nothing,
    /// A removal went through: offer to delete the branch it held, if
    /// nothing holds it now.
    Orphan(String),
    CleanupRan(Run),
    CleanupForced {
        item: MergedWorktree,
        failure: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub enum WtMsg {
    Loaded(u64, Box<Loaded>, After),
    Close,

    // The rows.
    ShowChanges(String),
    Diff(DiffMsg),
    CloseDiff,
    OpenWorkspace(String, Option<String>),
    OpenPane(String),
    MigrateAsk(String),
    MigrateCancel,
    MigrateRun(String),
    Migrated(u64, String, Box<Result<MigrateResult, String>>),
    DismissMigrated,
    ToggleLock(String),
    LockDone(u64, Result<(), String>),
    /// The Remove button: the first press arms it, the second removes.
    RemovePressed(String),
    Remove {
        path: String,
        force: bool,
        skip_hook: bool,
    },
    Removed(
        u64,
        String,
        Option<String>,
        Box<Result<RemoveResult, String>>,
    ),
    OrphanDelete,
    OrphanKeep,
    OrphanDeleted(u64, Box<Result<DeleteBranchResult, String>>),

    // The footer.
    Fetch,
    Prune,
    Refresh,
    /// Fetch or prune finished.
    Done(u64, Result<(), String>),

    // The cleanup.
    OpenCleanup,
    CleanupListed(u64, Result<Vec<MergedWorktree>, String>),
    CleanupToggle(String),
    CleanupRemovePressed,
    CleanupRan(u64, Box<Run>),
    CleanupForce(String),
    CleanupForced(u64, String, Option<String>),
    CleanupDeleteBranches,
    CleanupBranchesDone(u64, Box<BranchesRun>),
    CleanupKeepBranches,
    CleanupBack,

    // The form.
    SetMode(Mode),
    NewBranch(String),
    PickBase(Option<String>),
    PickExisting(String),
    Folder(String),
    OpenAfter(bool),
    CopyUntracked(bool),
    Create,
    Created(u64, Box<Result<Created, String>>),
    TogglePicker(Picker),
    ClosePicker,

    NoticeExpired(u64),
}

pub struct Panel {
    pub ws: WsId,
    /// The workspace's folder: where `git -C` runs.
    pub cwd: String,
    generation: u64,
    info: Option<RepoInfo>,
    worktrees: Vec<Worktree>,
    branches: Vec<Branch>,
    loading: bool,
    error: Option<String>,
    busy: Option<Busy>,
    armed: Option<Armed>,
    blocked: Option<Blocked>,
    /// The worktree whose « bring changes back » waits for its confirmation.
    migrating: Option<String>,
    /// What the last migration said, kept on the row it concerns until
    /// dismissed.
    migrated: Option<(String, MigrateResult)>,
    pub(super) orphan: Option<Orphan>,
    notice: Option<Notice>,
    notice_seq: u64,
    form: Form,
    /// The folder the form suggests: siblings of the checkout, named after the
    /// branch. Kept here because the field borrows it for as long as it shows.
    suggested: String,
    picker: Option<Picker>,
    /// The worktree whose changes are on screen, in place of the panel.
    diff_for: Option<DiffState>,
    cleanup: Option<Cleanup>,
}

impl std::fmt::Debug for Panel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Panel")
            .field("ws", &self.ws)
            .field("cwd", &self.cwd)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

/// A message of the panel, as the router carries it.
pub fn wrap(msg: WtMsg) -> Message {
    Message::GitUi(Msg::Wt(msg))
}

fn wrap_diff(msg: DiffMsg) -> Message {
    wrap(WtMsg::Diff(msg))
}

fn intent(intent: Intent) -> Task<Message> {
    Task::done(Message::Intent(intent))
}

impl Panel {
    /// The panel of `ws`'s worktrees, and its first reading of the repository.
    pub fn open(ws: WsId, cwd: String, generation: u64) -> (Panel, Task<Message>) {
        let mut panel = Panel {
            ws,
            cwd,
            generation,
            info: None,
            worktrees: Vec::new(),
            branches: Vec::new(),
            loading: true,
            error: None,
            busy: None,
            armed: None,
            blocked: None,
            migrating: None,
            migrated: None,
            orphan: None,
            notice: None,
            notice_seq: 0,
            form: Form::default(),
            suggested: String::new(),
            picker: None,
            diff_for: None,
            cleanup: None,
        };
        let first = panel.read(After::Nothing);
        (panel, first)
    }

    fn stale(&self, generation: u64) -> bool {
        generation != self.generation
    }

    /// Reads the repository: its info, then, if it is one, its worktrees and
    /// branches together.
    fn read(&mut self, after: After) -> Task<Message> {
        self.loading = true;
        self.error = None;
        let (generation, cwd) = (self.generation, self.cwd.clone());
        Task::perform(load(cwd), move |loaded| {
            wrap(WtMsg::Loaded(generation, Box::new(loaded), after.clone()))
        })
    }

    /// The branch pickers, by `ref_key`.
    fn locals(&self) -> impl Iterator<Item = &Branch> {
        self.branches.iter().filter(|branch| branch.kind == "local")
    }

    fn sanitized(&self) -> String {
        sanitize_branch_name(&self.form.new_branch)
    }

    /// What the form's choices come to.
    pub fn resolved(&self) -> Resolved {
        resolve(
            self.form.mode,
            &self.sanitized(),
            self.form.new_base.as_deref(),
            self.form.existing.as_deref(),
            &self.branches,
        )
    }

    /// Siblings of the checkout rather than children of it.
    pub fn suggested_folder(&self) -> String {
        let root = self
            .info
            .as_ref()
            .and_then(|info| info.main_root.as_deref());
        let branch = self.resolved().plan.map(|plan| plan.branch);
        match (root, branch) {
            (Some(root), Some(branch)) if !branch.is_empty() => worktree_folder(root, &branch),
            _ => String::new(),
        }
    }

    /// The folder in the field: the person's, or the suggestion.
    pub fn folder_text(&self) -> &str {
        if self.form.folder_touched {
            &self.form.folder
        } else {
            &self.suggested
        }
    }

    pub fn can_create(&self) -> bool {
        self.resolved().plan.is_some()
            && !self.folder_text().trim().is_empty()
            && self.busy.is_none()
    }

    /// Says what a step did, and clears it after a while.
    fn flash(&mut self, warn: bool, text: String) -> Task<Message> {
        self.notice_seq += 1;
        let seq = self.notice_seq;
        self.notice = Some(Notice { warn, text, seq });
        // The timer is made when the task runs, inside the runtime, not here.
        Task::perform(
            async {
                tokio::time::sleep(std::time::Duration::from_secs(NOTICE_SECONDS)).await;
            },
            move |()| wrap(WtMsg::NoticeExpired(seq)),
        )
    }
}

/// The repository and its lists, as `load` of the panel read them.
async fn load(cwd: String) -> Loaded {
    let info = git::git_repo_info(cwd.clone()).await;
    if !info.installed || !info.is_repo {
        return Loaded::Unavailable(info);
    }
    let (list, refs) = tokio::join!(git::git_worktree_list(cwd.clone()), git::git_branches(cwd));
    Loaded::Ready { info, list, refs }
}

/// Every folder a workspace or a pane of Hangar is open in: an agent may be
/// working there.
fn held_folders(m: &Model) -> Vec<String> {
    let mut held = Vec::new();
    for ws in &m.state().workspaces {
        held.push(ws.cwd.clone());
        held.extend(ws.panes.iter().filter_map(|pane| pane.cwd.clone()));
    }
    held
}

/// The person's own commands, set in the workspace's dialog. Blank runs
/// nothing.
fn hooks(m: &Model, ws: &str) -> (Option<String>, Option<String>) {
    let hooks = m.workspace(ws).and_then(|ws| ws.worktree_hooks.as_ref());
    let clean = |hook: Option<&String>| {
        hook.map(|command| command.trim().to_string())
            .filter(|command| !command.is_empty())
    };
    (
        clean(hooks.and_then(|hooks| hooks.post_create.as_ref())),
        clean(hooks.and_then(|hooks| hooks.pre_remove.as_ref())),
    )
}

/// Closes the panel: its overlay goes, if it is the one in front.
fn close(m: &mut Model) -> Task<Message> {
    if matches!(m.ui.overlays.last(), Some(Overlay::Worktrees(_))) {
        m.ui.overlays.pop();
    }
    Task::none()
}

/// Reuses the workspace already sitting on that folder — two workspaces on
/// one worktree would fight over the same files for no gain.
fn activate_workspace(
    m: &mut Model,
    origin: &str,
    target: &str,
    label: Option<&str>,
) -> Task<Message> {
    if let Some(existing) = m
        .state()
        .workspaces
        .iter()
        .find(|ws| same_path(&ws.cwd, target))
        .map(|ws| ws.id.clone())
    {
        return intent(Intent::ActivateWorkspace(existing));
    }
    // A worktree is the same project on another branch: it opens with the
    // agent and the shell of the workspace it was listed from, and is filed
    // next to it, so a project's branches stay together.
    let origin = m.workspace(origin);
    let draft = WorkspaceDraft {
        name: label
            .map(str::to_string)
            .unwrap_or_else(|| last_segment(target)),
        cwd: target.to_string(),
        layout: 1,
        agents: vec![
            origin
                .and_then(|ws| ws.panes.first())
                .map(|pane| pane.agent)
                .unwrap_or(AgentId::Shell),
        ],
        shell_id: origin.and_then(|ws| ws.shell_id.clone()),
        folder_id: origin.and_then(|ws| ws.folder_id.clone()),
        ..WorkspaceDraft::default()
    };
    let made = m.apply(StoreAction::AddWorkspace(draft));
    match made.workspace {
        // The new workspace is active; the shell still has to keep it alive.
        Some(id) => intent(Intent::ActivateWorkspace(id)),
        None => Task::none(),
    }
}

/// Does one message of the panel, and keeps the suggested folder in step
/// with whatever the message changed.
pub fn update(panel: &mut Panel, m: &mut Model, msg: WtMsg) -> Task<Message> {
    let task = step(panel, m, msg);
    panel.suggested = panel.suggested_folder();
    task
}

fn step(panel: &mut Panel, m: &mut Model, msg: WtMsg) -> Task<Message> {
    let tr = Translator::new(m.locale());
    match msg {
        WtMsg::Loaded(generation, loaded, after) if !panel.stale(generation) => {
            loaded_in(panel, *loaded, after)
        }
        WtMsg::Close => close(m),

        // ---- The rows ----
        WtMsg::ShowChanges(path) => {
            let label = panel
                .worktrees
                .iter()
                .find(|worktree| worktree.path == path)
                .and_then(|worktree| worktree.branch.clone())
                .unwrap_or_else(|| last_segment(&path));
            panel.generation += 1;
            let (state, first) = DiffState::open(path, Some(label), panel.generation);
            panel.diff_for = Some(state);
            first.map(wrap_diff)
        }
        WtMsg::Diff(msg) => match &mut panel.diff_for {
            Some(diff) => diff.update(msg).map(wrap_diff),
            None => Task::none(),
        },
        WtMsg::CloseDiff => {
            panel.diff_for = None;
            Task::none()
        }
        WtMsg::OpenWorkspace(path, branch) => {
            let opened = activate_workspace(m, &panel.ws, &path, branch.as_deref());
            Task::batch([opened, close(m)])
        }
        WtMsg::OpenPane(path) => {
            let made = m.apply(StoreAction::AddPane {
                ws: panel.ws.clone(),
                agent: None,
                near: None,
                dir: None,
                cwd: Some(path),
                name: None,
                init: None,
            });
            if made.pane.is_none() {
                panel.error = Some(tr.t(Key::WorktreesPaneFull).into_owned());
                return Task::none();
            }
            close(m)
        }
        WtMsg::MigrateAsk(path) => {
            panel.migrated = None;
            panel.migrating = if panel.migrating.as_ref() == Some(&path) {
                None
            } else {
                Some(path)
            };
            Task::none()
        }
        WtMsg::MigrateCancel => {
            panel.migrating = None;
            Task::none()
        }
        WtMsg::MigrateRun(path) => {
            panel.migrating = None;
            panel.migrated = None;
            panel.busy = Some(Busy::Migrate(path.clone()));
            panel.error = None;
            let (generation, cwd) = (panel.generation, panel.cwd.clone());
            let target = path.clone();
            Task::perform(git::git_worktree_migrate(cwd, path), move |result| {
                wrap(WtMsg::Migrated(
                    generation,
                    target.clone(),
                    Box::new(result),
                ))
            })
        }
        WtMsg::Migrated(generation, path, result) if !panel.stale(generation) => {
            panel.busy = None;
            match *result {
                Ok(result) if result.outcome == "moved" && result.stash.is_none() => {
                    return panel.flash(false, tr.t(Key::WorktreesMigrateDone).into_owned());
                }
                // Everything else is worth reading, so it stays on the row.
                Ok(result) => panel.migrated = Some((path, result)),
                Err(error) => panel.error = Some(error),
            }
            Task::none()
        }
        WtMsg::DismissMigrated => {
            panel.migrated = None;
            Task::none()
        }
        WtMsg::ToggleLock(path) => {
            let locked = panel
                .worktrees
                .iter()
                .any(|worktree| worktree.path == path && worktree.locked.is_some());
            panel.busy = Some(Busy::Lock(path.clone()));
            panel.error = None;
            let (generation, cwd) = (panel.generation, panel.cwd.clone());
            let done = move |result| wrap(WtMsg::LockDone(generation, result));
            if locked {
                Task::perform(git::git_worktree_unlock(cwd, path), done)
            } else {
                Task::perform(git::git_worktree_lock(cwd, path), done)
            }
        }
        WtMsg::LockDone(generation, result) | WtMsg::Done(generation, result)
            if !panel.stale(generation) =>
        {
            panel.busy = None;
            // The reading clears the last action's error as it starts, so a
            // failure is put back after it: the web app lost it that way (a
            // failed fetch showed nothing), and it is the one thing the
            // person needs to read.
            let reading = panel.read(After::Nothing);
            if let Err(error) = result {
                panel.error = Some(error);
            }
            reading
        }
        WtMsg::RemovePressed(path) => {
            // Inline, because a native dialog would block the event loop the
            // terminals run on.
            if panel.armed.as_ref().is_some_and(|armed| armed.path == path) {
                return step(
                    panel,
                    m,
                    WtMsg::Remove {
                        path,
                        force: false,
                        skip_hook: false,
                    },
                );
            }
            let branch = panel
                .worktrees
                .iter()
                .find(|worktree| worktree.path == path)
                .and_then(|worktree| worktree.branch.clone());
            panel.armed = Some(Armed { path, branch });
            Task::none()
        }
        WtMsg::Remove {
            path,
            force,
            skip_hook,
        } => {
            // Read before the deletion: afterwards git no longer ties the
            // branch to a worktree, and the offer to delete it would name the
            // wrong one.
            let removed_branch = match &panel.armed {
                Some(armed) if armed.path == path => armed.branch.clone(),
                _ => panel
                    .worktrees
                    .iter()
                    .find(|worktree| worktree.path == path)
                    .and_then(|worktree| worktree.branch.clone()),
            };
            panel.busy = Some(Busy::Remove(path.clone()));
            panel.error = None;
            let (_, pre_remove) = hooks(m, &panel.ws);
            // Forcing follows a first try that already ran the command.
            let pre_remove = if force || skip_hook { None } else { pre_remove };
            let (generation, cwd) = (panel.generation, panel.cwd.clone());
            let target = path.clone();
            Task::perform(
                git::git_worktree_remove(cwd, path, force, pre_remove),
                move |result| {
                    wrap(WtMsg::Removed(
                        generation,
                        target.clone(),
                        removed_branch.clone(),
                        Box::new(result),
                    ))
                },
            )
        }
        WtMsg::Removed(generation, path, branch, result) if !panel.stale(generation) => {
            panel.armed = None;
            match *result {
                Ok(result) if result.removed => {
                    panel.blocked = None;
                    panel.busy = None;
                    return panel.read(match branch {
                        Some(branch) => After::Orphan(branch),
                        None => After::Nothing,
                    });
                }
                // Dirty or locked. Throwing the work away is the user's call,
                // so the row keeps git's reason and offers the forced run
                // instead of deciding.
                Ok(result) if result.hook_blocked => {
                    panel.blocked = Some(Blocked {
                        path,
                        message: result.message,
                        hook: true,
                    });
                }
                Ok(result) if result.needs_force => {
                    panel.blocked = Some(Blocked {
                        path,
                        message: result.message,
                        hook: false,
                    });
                }
                Ok(result) => panel.error = result.message,
                Err(error) => panel.error = Some(error),
            }
            panel.busy = None;
            Task::none()
        }
        WtMsg::OrphanDelete => {
            let Some(orphan) = panel.orphan.clone() else {
                return Task::none();
            };
            panel.busy = Some(Busy::Branch(orphan.name.clone()));
            panel.error = None;
            let (generation, cwd) = (panel.generation, panel.cwd.clone());
            Task::perform(
                git::git_branch_delete(cwd, orphan.name, orphan.needs_force),
                move |result| wrap(WtMsg::OrphanDeleted(generation, Box::new(result))),
            )
        }
        WtMsg::OrphanKeep => {
            panel.orphan = None;
            Task::none()
        }
        WtMsg::OrphanDeleted(generation, result) if !panel.stale(generation) => {
            panel.busy = None;
            match *result {
                Ok(result) if result.deleted => {
                    panel.orphan = None;
                    return panel.read(After::Nothing);
                }
                // "not fully merged" is a question, not a failure: git's own
                // sentence stays on screen, next to the button that answers it.
                Ok(result) if result.needs_force => {
                    if let Some(orphan) = &mut panel.orphan {
                        orphan.needs_force = true;
                        orphan.message = result.message;
                    }
                }
                Ok(result) => {
                    panel.error = result.message;
                    panel.orphan = None;
                }
                Err(error) => {
                    panel.error = Some(error);
                    panel.orphan = None;
                }
            }
            Task::none()
        }

        // ---- The footer ----
        WtMsg::Fetch => {
            panel.busy = Some(Busy::Fetch);
            panel.error = None;
            let (generation, cwd) = (panel.generation, panel.cwd.clone());
            Task::perform(git::git_fetch(cwd), move |result| {
                wrap(WtMsg::Done(generation, result))
            })
        }
        WtMsg::Prune => {
            panel.busy = Some(Busy::Prune);
            panel.error = None;
            let (generation, cwd) = (panel.generation, panel.cwd.clone());
            Task::perform(git::git_worktree_prune(cwd), move |result| {
                wrap(WtMsg::Done(generation, result))
            })
        }
        WtMsg::Refresh => panel.read(After::Nothing),

        // ---- The cleanup ----
        WtMsg::OpenCleanup => {
            panel.error = None;
            panel.cleanup = Some(Cleanup::loading());
            let held = held_folders(m);
            let (generation, cwd) = (panel.generation, panel.cwd.clone());
            Task::perform(git::git_worktrees_merged(cwd, held), move |result| {
                wrap(WtMsg::CleanupListed(generation, result))
            })
        }
        WtMsg::CleanupListed(generation, result) if !panel.stale(generation) => {
            match result {
                Ok(items) => {
                    if let Some(cleanup) = &mut panel.cleanup {
                        cleanup.listed(items);
                    }
                }
                Err(error) => {
                    panel.error = Some(error);
                    panel.cleanup = None;
                }
            }
            Task::none()
        }
        WtMsg::CleanupToggle(path) => {
            if let Some(cleanup) = &mut panel.cleanup {
                cleanup.toggle(&path);
            }
            Task::none()
        }
        WtMsg::CleanupRemovePressed => {
            let Some(cleanup) = &mut panel.cleanup else {
                return Task::none();
            };
            // Inline, as everywhere in this panel.
            if !cleanup.confirming {
                cleanup.confirming = true;
                return Task::none();
            }
            let todo = cleanup.todo();
            if todo.is_empty() {
                return Task::none();
            }
            panel.busy = Some(Busy::Cleanup);
            panel.error = None;
            let (_, pre_remove) = hooks(m, &panel.ws);
            let (generation, cwd) = (panel.generation, panel.cwd.clone());
            Task::perform(cleanup::run(cwd, todo, pre_remove), move |run| {
                wrap(WtMsg::CleanupRan(generation, Box::new(run)))
            })
        }
        WtMsg::CleanupRan(generation, run) if !panel.stale(generation) => {
            panel.busy = None;
            panel.read(After::CleanupRan(*run))
        }
        WtMsg::CleanupForce(path) => {
            panel.busy = Some(Busy::CleanupWorktree(path.clone()));
            panel.error = None;
            let (generation, cwd) = (panel.generation, panel.cwd.clone());
            let target = path.clone();
            Task::perform(cleanup::force_one(cwd, path), move |failure| {
                wrap(WtMsg::CleanupForced(generation, target.clone(), failure))
            })
        }
        WtMsg::CleanupForced(generation, path, failure) if !panel.stale(generation) => {
            panel.busy = None;
            let item = panel
                .cleanup
                .iter()
                .flat_map(|cleanup| cleanup.forced.iter())
                .map(|(item, _)| item)
                .find(|item| item.path == path)
                .cloned();
            match item {
                Some(item) => panel.read(After::CleanupForced { item, failure }),
                None => panel.read(After::Nothing),
            }
        }
        WtMsg::CleanupDeleteBranches => {
            let Some(cleanup) = &panel.cleanup else {
                return Task::none();
            };
            let (names, force) = (cleanup.branches.clone(), cleanup.branches_force);
            panel.busy = Some(Busy::CleanupBranches);
            panel.error = None;
            let (generation, cwd) = (panel.generation, panel.cwd.clone());
            Task::perform(cleanup::delete_branches(cwd, names, force), move |run| {
                wrap(WtMsg::CleanupBranchesDone(generation, Box::new(run)))
            })
        }
        WtMsg::CleanupBranchesDone(generation, run) if !panel.stale(generation) => {
            panel.busy = None;
            let error = run.error.clone();
            if let Some(cleanup) = &mut panel.cleanup {
                cleanup.after_branches(*run);
            }
            let reading = panel.read(After::Nothing);
            // As after a fetch: the reading must not swallow the failure.
            if error.is_some() {
                panel.error = error;
            }
            reading
        }
        WtMsg::CleanupKeepBranches => {
            if let Some(cleanup) = &mut panel.cleanup {
                cleanup.keep_branches();
            }
            Task::none()
        }
        WtMsg::CleanupBack => {
            panel.cleanup = None;
            Task::none()
        }

        // ---- The form ----
        WtMsg::SetMode(mode) => {
            panel.form.mode = mode;
            panel.picker = None;
            Task::none()
        }
        WtMsg::NewBranch(name) => {
            panel.form.new_branch = name;
            Task::none()
        }
        WtMsg::PickBase(key) => {
            panel.form.new_base = key;
            panel.picker = None;
            Task::none()
        }
        WtMsg::PickExisting(key) => {
            panel.form.existing = Some(key);
            panel.picker = None;
            Task::none()
        }
        WtMsg::Folder(path) => {
            panel.form.folder_touched = true;
            panel.form.folder = path;
            Task::none()
        }
        WtMsg::OpenAfter(on) => {
            panel.form.open_after = on;
            Task::none()
        }
        WtMsg::CopyUntracked(on) => {
            panel.form.copy_untracked = on;
            Task::none()
        }
        WtMsg::TogglePicker(which) => {
            panel.picker = if panel.picker == Some(which) {
                None
            } else {
                Some(which)
            };
            Task::none()
        }
        WtMsg::ClosePicker => {
            panel.picker = None;
            Task::none()
        }
        WtMsg::Create => {
            let Some(plan) = panel.resolved().plan else {
                return Task::none();
            };
            let wanted = panel.folder_text().trim().to_string();
            if wanted.is_empty() {
                return Task::none();
            }
            let (post_create, _) = hooks(m, &panel.ws);
            let request = CreateRequest {
                cwd: panel.cwd.clone(),
                root: panel.info.as_ref().and_then(|info| info.root.clone()),
                plan,
                wanted,
                copy_untracked: panel.form.copy_untracked,
                post_create,
            };
            panel.busy = Some(Busy::Create);
            panel.error = None;
            let generation = panel.generation;
            Task::perform(create::run(request, tr), move |result| {
                wrap(WtMsg::Created(generation, Box::new(result)))
            })
        }
        WtMsg::Created(generation, result) if !panel.stale(generation) => {
            created(panel, m, *result)
        }
        WtMsg::NoticeExpired(seq) => {
            if panel
                .notice
                .as_ref()
                .is_some_and(|notice| notice.seq == seq)
            {
                panel.notice = None;
            }
            Task::none()
        }

        // An answer for an earlier opening of the panel.
        WtMsg::Loaded(..)
        | WtMsg::Migrated(..)
        | WtMsg::LockDone(..)
        | WtMsg::Done(..)
        | WtMsg::Removed(..)
        | WtMsg::OrphanDeleted(..)
        | WtMsg::CleanupListed(..)
        | WtMsg::CleanupRan(..)
        | WtMsg::CleanupForced(..)
        | WtMsg::CleanupBranchesDone(..)
        | WtMsg::Created(..) => Task::none(),
    }
}

/// A reading came in.
fn loaded_in(panel: &mut Panel, loaded: Loaded, after: After) -> Task<Message> {
    panel.loading = false;
    // What the callers act on: the fresh branches, when the reading gave them.
    let fresh = match loaded {
        Loaded::Unavailable(info) => {
            panel.info = Some(info);
            panel.worktrees.clear();
            panel.branches.clear();
            None
        }
        Loaded::Ready { info, list, refs } => {
            panel.info = Some(info);
            match list {
                Err(error) => {
                    panel.error = Some(error);
                    None
                }
                Ok(list) => {
                    panel.worktrees = list;
                    panel.branches = refs.clone().unwrap_or_default();
                    if let Err(error) = refs {
                        panel.error = Some(error);
                    }
                    Some(panel.branches.clone())
                }
            }
        }
    };
    match after {
        After::Nothing => {}
        After::Orphan(name) => {
            let held = fresh.as_ref().is_some_and(|branches| {
                branches.iter().any(|branch| {
                    branch.kind == "local" && branch.name == name && branch.worktree_path.is_none()
                })
            });
            if held {
                panel.orphan = Some(Orphan {
                    name,
                    needs_force: false,
                    message: None,
                });
            }
        }
        After::CleanupRan(run) => {
            if let Some(cleanup) = &mut panel.cleanup {
                cleanup.after_run(run, fresh.as_deref());
            }
        }
        After::CleanupForced { item, failure } => {
            if let Some(cleanup) = &mut panel.cleanup {
                cleanup.after_forced(&item, failure, fresh.as_deref());
            }
        }
    }
    Task::none()
}

/// A worktree was made, or was not.
fn created(panel: &mut Panel, m: &mut Model, result: Result<Created, String>) -> Task<Message> {
    panel.busy = None;
    let made = match result {
        Ok(made) => made,
        Err(error) => {
            panel.error = Some(error);
            return Task::none();
        }
    };
    let mut tasks = Vec::new();
    if !made.notes.is_empty() {
        tasks.push(panel.flash(made.warn, made.notes.join(" ")));
    }
    let open_after = panel.form.open_after;
    let quiet = made.notes.is_empty() && made.hook_failure.is_none();
    // The form starts over; the person's choices about what comes with a
    // worktree are kept.
    panel.form = Form {
        open_after,
        copy_untracked: panel.form.copy_untracked,
        ..Form::default()
    };
    if open_after {
        tasks.push(activate_workspace(
            m,
            &panel.ws,
            &made.target,
            Some(&made.branch),
        ));
        // Closing would unmount the report with the panel: a failed .env copy
        // or a suffixed folder is exactly what the person must not miss. The
        // workspace is already switched behind the dialog; when there is
        // nothing to say, the panel goes with it.
        if quiet {
            tasks.push(close(m));
            return Task::batch(tasks);
        }
    }
    tasks.push(panel.read(After::Nothing));
    // After the reading, which clears the last action's error as it starts:
    // the command's failure stays on screen until the next action, as the web
    // app meant it to (it set it first and lost it to the reading).
    if let Some(failure) = made.hook_failure {
        panel.error = Some(failure);
    }
    Task::batch(tasks)
}

#[cfg(test)]
mod tests;
