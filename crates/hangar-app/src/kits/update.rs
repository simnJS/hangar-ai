//! What the package does with its messages.
//!
//! Three places share them: the section of a workspace's kits, the activation
//! dialog, and Settings › Kits with its library. They all end in the same few
//! writes, which is why the logic sits here and not in the views:
//! `Workspace.kits` (a workspace's own), `Settings.kitsEverywhere`, or the
//! draft of a workspace being created, always through `Model::apply`.

use std::sync::Arc;
use std::time::Duration;

use iced::Task;

use hangar_core::Host;
use hangar_core::kits::KitInfo;
use hangar_core::pty::PtyManager;
use hangar_i18n::{Key, Translator, args};

use super::activation::{
    accepted_grants, disable_kit, enable_kit, kit_name, panes_to_restart, update_outcome,
};
use super::gallery::{gallery_install_url, kit_gallery_list};
use super::icon::load_icon;
use super::model::{everywhere_kits, workspace_kits};
use super::setup::{
    PaneTyping, SetupRunResult, WAKE_PAUSE, show_workspace_selections, type_into_pane,
};
use super::{Activation, Adding, GalleryFailure, Msg, Restart, State, StepRun, Target, UpdateNote};
use crate::message::{Intent, Message};
use crate::services::{Services, clipboard, dialogs};
use crate::shell::Overlay;
use crate::state::types::{AgentId, Settings, Workspace, WorkspaceKit};
use crate::state::{Model, Patch, StoreAction, WsId};

fn kits(msg: Msg) -> Message {
    Message::Kits(msg)
}

fn translator(m: &Model) -> Translator {
    Translator::new(m.locale())
}

fn intent(intent: Intent) -> Task<Message> {
    Task::done(Message::Intent(intent))
}

/// `kits_list`, answered as `Msg::Installed`.
pub fn read_installed() -> Task<Message> {
    Task::perform(hangar_core::kits::kits_list(), |result| {
        kits(Msg::Installed(result))
    })
}

fn target_of(ws: Option<WsId>) -> Target {
    match ws {
        Some(ws) => Target::Workspace(ws),
        None => Target::Draft,
    }
}

/// The kits a target holds of its own.
fn own_kits(st: &State, m: &Model, target: &Target) -> Vec<WorkspaceKit> {
    match target {
        Target::Workspace(ws) => workspace_kits(m.workspace(ws)),
        Target::Draft => st.draft.clone(),
        Target::Everywhere => everywhere_kits(Some(m.settings())),
    }
}

/// The write of a target's kits into the persisted state; `None` for the draft
/// of a workspace being created, which is not persisted yet. The list is
/// written as it is, empty included, as the web app's `updateWorkspace({kits})`
/// did.
fn store_action(target: &Target, next: Vec<WorkspaceKit>) -> Option<StoreAction> {
    match target {
        Target::Workspace(ws) => Some(StoreAction::UpdateWorkspace(
            ws.clone(),
            Patch::new(move |workspace: &mut Workspace| workspace.kits = Some(next.clone())),
        )),
        Target::Draft => None,
        Target::Everywhere => Some(StoreAction::UpdateSettings(Patch::new(
            move |settings: &mut Settings| settings.kits_everywhere = Some(next.clone()),
        ))),
    }
}

/// Writes a target's kits.
fn write_kits(st: &mut State, m: &mut Model, target: &Target, next: Vec<WorkspaceKit>) {
    if *target == Target::Draft {
        st.draft = next;
    } else if let Some(action) = store_action(target, next) {
        m.apply(action);
    }
}

/// What reading a kit for its activation takes: `kit_inspect` and
/// `kit_setup_status`, both again when its hooks or commands change.
fn read_for_activation(kit: &str) -> Task<Message> {
    let (inspect, status) = (kit.to_owned(), kit.to_owned());
    Task::batch([
        Task::perform(hangar_core::kits::kit_inspect(inspect.clone()), move |r| {
            kits(Msg::Inspected(inspect.clone(), r.map(Box::new)))
        }),
        Task::perform(
            hangar_core::kits::kit_setup_status(status.clone()),
            move |r| kits(Msg::StepsRead(status.clone(), r)),
        ),
    ])
}

/// Opens the activation of a kit for a target: the dialog, over whatever is
/// open, reading what the kit brings.
fn start_activation(st: &mut State, m: &Model, target: Target, kit_id: &str) -> Task<Message> {
    let Some(kit) = m.live.kits.find(kit_id) else {
        return Task::none();
    };
    let ws = match &target {
        Target::Workspace(ws) => Some(ws.clone()),
        Target::Draft => None,
        Target::Everywhere => m.state().active_workspace_id.clone(),
    };
    st.activation = Some(Activation {
        kit: kit_id.to_owned(),
        target,
        ws: ws.clone(),
        inspection: None,
        steps: None,
        running: None,
        ran: None,
        error: None,
        seen: (
            kit.hooks_permission.clone(),
            kit.commands_permission.clone(),
        ),
    });
    Task::batch([
        read_for_activation(kit_id),
        intent(Intent::OpenOverlay(Overlay::KitActivation {
            kit: kit_id.to_owned(),
            ws,
        })),
    ])
}

pub fn on_intent(st: &mut State, m: &mut Model, intent: &Intent) -> Option<Task<Message>> {
    match intent {
        // A workspace dialog starts from no kits, as the TS section did: its
        // state lived in the dialog.
        Intent::OpenOverlay(Overlay::WorkspaceDialog) => {
            st.draft.clear();
            st.restart = None;
            st.restarted = None;
        }
        // The overlay is gone (Escape, the veil, a button): so is what it was
        // reading.
        Intent::CloseOverlay => {
            let open =
                m.ui.overlays
                    .iter()
                    .any(|overlay| matches!(overlay, Overlay::KitActivation { .. }));
            if !open {
                st.activation = None;
            }
        }
        _ => {}
    }
    None
}

/// What the messages reach outside the model: core's host (the kits'
/// commands send their event through it) and the panes' terminals.
pub struct Env {
    pub host: Host,
    pub pty: Arc<PtyManager>,
}

impl Env {
    pub fn of(sv: &Services) -> Env {
        Env {
            host: sv.core().host().clone(),
            pty: sv.core().pty().clone(),
        }
    }
}

pub fn update(st: &mut State, m: &mut Model, env: &Env, msg: Msg) -> Task<Message> {
    let t = translator(m);
    let host = || env.host.clone();
    match msg {
        Msg::Installed(result) => installed(st, m, result),
        Msg::IconLoaded { id, key, result } => {
            m.live.kits.icons.loaded(&id, key, result);
            m.live.version += 1;
            Task::none()
        }
        Msg::Copy(text) => clipboard::write(text),

        // -- the section of a workspace's kits ---------------------------------------
        Msg::Toggle { ws, kit, on } => {
            st.restarted = None;
            let target = target_of(ws);
            if on {
                start_activation(st, m, target, &kit)
            } else {
                let next = disable_kit(&own_kits(st, m, &target), &kit);
                write_kits(st, m, &target, next);
                Task::none()
            }
        }
        Msg::Open { ws, kit } => start_activation(st, m, target_of(ws), &kit),
        Msg::RemoveGone { ws, kit } => {
            let target = target_of(ws);
            let next = disable_kit(&own_kits(st, m, &target), &kit);
            write_kits(st, m, &target, next);
            Task::none()
        }
        Msg::RestartPanes(ws) => {
            // A respawn with no patch keeps each pane's session: the
            // conversation resumes.
            let panes = m.workspace(&ws).map(panes_to_restart).unwrap_or_default();
            for pane in panes {
                m.apply(StoreAction::RespawnPane {
                    ws: ws.clone(),
                    pane,
                    patch: Patch::none(),
                });
            }
            st.restart = None;
            st.restarted = Some(ws);
            Task::none()
        }
        Msg::DismissRestart => {
            st.restart = None;
            Task::none()
        }

        // -- the activation dialog ------------------------------------------------------
        Msg::Inspected(kit, result) => {
            if let Some(act) = st.activation.as_mut().filter(|a| a.kit == kit) {
                match result {
                    Ok(found) => act.inspection = Some(*found),
                    Err(detail) => {
                        act.error = Some(
                            t.t_with(
                                Key::KitsManageErrorPrefix,
                                &args!("detail" => detail.as_str()),
                            )
                            .into_owned(),
                        )
                    }
                }
            }
            Task::none()
        }
        Msg::StepsRead(kit, result) => {
            if let Some(act) = st.activation.as_mut().filter(|a| a.kit == kit) {
                act.steps = Some(result.unwrap_or_default());
            }
            Task::none()
        }
        Msg::Accept => accept(st, m),
        Msg::Run(step) => run(st, m, step),
        Msg::SetupPane { kit, step } => setup_pane(st, m, env, kit, step),
        Msg::SetupDone {
            kit,
            step,
            ws,
            pane,
            pane_name,
            result,
        } => setup_done(st, m, kit, step, ws, pane, pane_name, result),
        Msg::Confirm(step) => {
            let Some(act) = st.activation.as_ref() else {
                return Task::none();
            };
            let kit = act.kit.clone();
            Task::perform(
                hangar_core::kits::kit_setup_mark(kit.clone(), step),
                move |r| kits(Msg::Marked(kit.clone(), r)),
            )
        }
        Msg::Marked(kit, result) => {
            if let Some(act) = st.activation.as_mut().filter(|a| a.kit == kit) {
                match result {
                    Ok(next) => {
                        act.steps = Some(next);
                        act.ran = None;
                    }
                    Err(detail) => {
                        act.error = Some(
                            t.t_with(
                                Key::KitsManageErrorPrefix,
                                &args!("detail" => detail.as_str()),
                            )
                            .into_owned(),
                        )
                    }
                }
            }
            Task::none()
        }
        Msg::NotYet => {
            if let Some(act) = st.activation.as_mut() {
                act.ran = None;
            }
            Task::none()
        }

        // -- Settings › Kits -----------------------------------------------------------------
        Msg::AddFolder => {
            st.page.folder_error = None;
            st.page.flash = None;
            let title = t.t(Key::KitsManageAddFolderTitle).into_owned();
            Task::perform(dialogs::pick_folder(title, None), |picked| {
                kits(Msg::FolderPicked(picked))
            })
        }
        Msg::FolderPicked(None) => Task::none(),
        Msg::FolderPicked(Some(path)) => {
            st.page.adding = Some(Adding::Folder);
            Task::perform(
                hangar_core::kits::kit_add_folder(host(), path.to_string_lossy().into_owned()),
                |r| kits(Msg::FolderAdded(r.map(Box::new))),
            )
        }
        Msg::FolderAdded(result) => {
            st.page.adding = None;
            match result {
                Ok(added) => {
                    st.page.flash = Some(added_words(t, &added));
                    read_installed()
                }
                Err(detail) => {
                    st.page.folder_error = Some(detail);
                    Task::none()
                }
            }
        }
        Msg::GitUrl(url) => {
            st.page.git_url = url;
            st.page.git_error = None;
            Task::none()
        }
        Msg::AddGit => {
            let url = st.page.git_url.trim().to_owned();
            if url.is_empty() || st.page.adding.is_some() {
                return Task::none();
            }
            st.page.git_error = None;
            st.page.flash = None;
            st.page.adding = Some(Adding::Git);
            Task::perform(hangar_core::kits::kit_add_git(host(), url), |r| {
                kits(Msg::GitAdded(r.map(Box::new)))
            })
        }
        Msg::GitAdded(result) => {
            st.page.adding = None;
            match result {
                Ok(added) => {
                    st.page.git_url.clear();
                    st.page.flash = Some(added_words(t, &added));
                    read_installed()
                }
                Err(detail) => {
                    st.page.git_error = Some(detail);
                    Task::none()
                }
            }
        }
        Msg::Check(id) => {
            st.page.notes.insert(id.clone(), UpdateNote::Checking);
            Task::perform(
                hangar_core::kits::kit_check_update(host(), id.clone()),
                move |r| kits(Msg::Checked(id.clone(), r)),
            )
        }
        Msg::Checked(id, result) => {
            let note = match result {
                Ok(None) => UpdateNote::Folder,
                Ok(Some(found)) if found.available => UpdateNote::Available {
                    current: found.current,
                    latest: found.latest,
                },
                Ok(Some(found)) => UpdateNote::None {
                    version: found.current,
                },
                Err(detail) => UpdateNote::Error(detail),
            };
            st.page.notes.insert(id, note);
            Task::none()
        }
        Msg::Update(id) => start_update(st, m, env, id),
        Msg::Updated { id, before, result } => {
            if st.page.gallery_busy.as_deref() == Some(id.as_str()) {
                st.page.gallery_busy = None;
            }
            match result {
                Ok(after) => {
                    let outcome = update_outcome(&before, &after);
                    let note = if matches!(after.source, hangar_core::kits::Source::Folder { .. }) {
                        UpdateNote::Reloaded {
                            version: after.installed_version.clone(),
                        }
                    } else {
                        UpdateNote::Done {
                            from: outcome.from,
                            to: outcome.to,
                            new_permissions: outcome.new_permissions.len(),
                        }
                    };
                    st.page.notes.insert(id.clone(), note);
                    // The files of a reloaded kit may have changed under the
                    // same version: its icon is read again.
                    m.live.kits.icons.forget(&id);
                    read_installed()
                }
                Err(detail) => {
                    st.page.notes.insert(id, UpdateNote::Error(detail));
                    Task::none()
                }
            }
        }
        Msg::AskRemove(id) => {
            st.page.removing = id;
            Task::none()
        }
        Msg::Remove(id) => {
            st.page.removing = None;
            Task::perform(
                hangar_core::kits::kit_remove(host(), id.clone()),
                move |r| kits(Msg::Removed(id.clone(), r)),
            )
        }
        Msg::Removed(id, result) => match result {
            Ok(()) => {
                st.page.notes.remove(&id);
                read_installed()
            }
            Err(detail) => {
                st.page.notes.insert(id, UpdateNote::Error(detail));
                Task::none()
            }
        },
        Msg::ToggleEverywhere { kit, on } => {
            st.page.flash = None;
            if on {
                // The switch is disabled for a kit that cannot be used; keep
                // it so here too.
                let usable = m
                    .live
                    .kits
                    .find(&kit)
                    .is_some_and(|found| found.error.is_none() && found.manifest.is_some());
                if !usable {
                    return Task::none();
                }
                return start_activation(st, m, Target::Everywhere, &kit);
            }
            // Asks nothing more than the removal wording; workspaces' own
            // entries stay.
            let next = disable_kit(&everywhere_kits(Some(m.settings())), &kit);
            write_kits(st, m, &Target::Everywhere, next);
            st.page.flash = Some(t.t(Key::KitsManageEverywhereOffNote).into_owned());
            st.page.restart_for = Some(display_name(m, &kit));
            Task::none()
        }
        Msg::ReviewEverywhere(kit) => start_activation(st, m, Target::Everywhere, &kit),
        Msg::DismissEverywhereRestart => {
            st.page.restart_for = None;
            Task::none()
        }
        Msg::ManageWorkspace(ws) => intent(Intent::OpenKitsDialog(ws)),

        // -- the library ---------------------------------------------------------------------
        Msg::Gallery(result) => {
            st.page.gallery_refreshing = false;
            match result {
                Ok(list) => {
                    st.page.gallery = Some(list);
                    st.page.gallery_error = None;
                }
                Err(detail) => st.page.gallery_error = Some(detail),
            }
            Task::none()
        }
        Msg::GalleryRefresh => {
            st.page.gallery_refreshing = true;
            Task::perform(kit_gallery_list(), |r| kits(Msg::Gallery(r)))
        }
        Msg::GalleryQuery(query) => {
            st.page.gallery_query = query;
            Task::none()
        }
        Msg::GalleryInstall(id) => {
            let Some(entry) = st
                .page
                .gallery
                .as_ref()
                .and_then(|g| g.kits.iter().find(|item| item.id == id))
            else {
                return Task::none();
            };
            let (url, name) = (gallery_install_url(entry), entry.name.clone());
            st.page.gallery_failure = None;
            st.page.flash = None;
            st.page.gallery_busy = Some(id.clone());
            Task::perform(hangar_core::kits::kit_add_git(host(), url), move |result| {
                kits(Msg::GalleryInstalled {
                    id: id.clone(),
                    name: name.clone(),
                    result: result.map(Box::new),
                })
            })
        }
        Msg::GalleryInstalled { id, name, result } => {
            st.page.gallery_busy = None;
            match result {
                Ok(added) => {
                    st.page.flash = Some(
                        t.t_with(
                            Key::KitsManageGalleryInstalledFlash,
                            &args!("name" => kit_name(&added)),
                        )
                        .into_owned(),
                    );
                    read_installed()
                }
                Err(detail) => {
                    st.page.gallery_failure = Some(GalleryFailure { id, name, detail });
                    Task::none()
                }
            }
        }
        Msg::GalleryUpdate(id) => {
            st.page.gallery_failure = None;
            st.page.gallery_busy = Some(id.clone());
            start_update(st, m, env, id)
        }
    }
}

fn added_words(t: Translator, kit: &KitInfo) -> String {
    t.t_with(Key::KitsManageAdded, &args!("name" => kit_name(kit)))
        .into_owned()
}

/// A kit's name for a sentence: its manifest's, or its id.
fn display_name(m: &Model, id: &str) -> String {
    m.live
        .kits
        .find(id)
        .map_or_else(|| id.to_owned(), |kit| kit_name(kit).to_owned())
}

fn start_update(st: &mut State, m: &Model, env: &Env, id: String) -> Task<Message> {
    let Some(before) = m.live.kits.find(&id).cloned() else {
        st.page.gallery_busy = None;
        return Task::none();
    };
    st.page.notes.insert(id.clone(), UpdateNote::Updating);
    Task::perform(
        hangar_core::kits::kit_update(env.host.clone(), id.clone()),
        move |result| {
            kits(Msg::Updated {
                id: id.clone(),
                before: Box::new(before.clone()),
                result: result.map(Box::new),
            })
        },
    )
}

/// `kits_list` answered: take it in, read the icons it names, and keep an open
/// activation honest about the kit it shows.
fn installed(st: &mut State, m: &mut Model, result: Result<Vec<KitInfo>, String>) -> Task<Message> {
    m.live.version += 1;
    let list = match result {
        Ok(list) => list,
        // A read that fails keeps the list there was.
        Err(error) => {
            m.live.kits.error = Some(error);
            return Task::none();
        }
    };
    m.live.kits.error = None;
    m.live.kits.icons.retain(&list);
    let reads = m.live.kits.icons.missing(&list);
    let mut tasks: Vec<Task<Message>> = reads
        .into_iter()
        .map(|read| {
            Task::perform(load_icon(read.dir, read.file), move |result| {
                kits(Msg::IconLoaded {
                    id: read.id.clone(),
                    key: read.key.clone(),
                    result,
                })
            })
        })
        .collect();

    if let Some(act) = st.activation.as_mut() {
        match list.iter().find(|kit| kit.id == act.kit) {
            // The kit was removed under its dialog: the dialog goes.
            None => {
                st.activation = None;
                tasks.push(intent(Intent::CloseOverlay));
            }
            Some(kit) => {
                let now = (
                    kit.hooks_permission.clone(),
                    kit.commands_permission.clone(),
                );
                if now != act.seen {
                    // Read again when its hooks or commands change, so what is
                    // shown is what is accepted.
                    act.seen = now;
                    tasks.push(read_for_activation(&act.kit));
                }
            }
        }
    }
    m.live.kits.installed = Some(list);
    Task::batch(tasks)
}

/// Accept: stores exactly what the person was shown.
fn accept(st: &mut State, m: &mut Model) -> Task<Message> {
    let Some(act) = st.activation.take() else {
        return Task::none();
    };
    let Some(kit) = m.live.kits.find(&act.kit).cloned() else {
        return Task::none();
    };
    // The hashes are the inspection's, not the kit's as it is now.
    let granted = accepted_grants(&kit.source, act.inspection.as_ref());
    let next = enable_kit(&own_kits(st, m, &act.target), &kit.id, &granted);
    write_kits(st, m, &act.target, next);
    let name = kit_name(&kit).to_owned();
    match &act.target {
        Target::Workspace(ws) => {
            // Agents already running keep the kits they started with.
            if m.workspace(ws)
                .is_some_and(|w| !panes_to_restart(w).is_empty())
            {
                st.restart = Some(Restart {
                    ws: ws.clone(),
                    kit_name: name,
                });
                st.restarted = None;
            }
        }
        Target::Draft => {}
        Target::Everywhere => st.page.restart_for = Some(name),
    }
    intent(Intent::CloseOverlay)
}

/// Run: makes the workspace the one on screen, then opens the pane.
fn run(st: &mut State, m: &mut Model, step: String) -> Task<Message> {
    let t = translator(m);
    let Some(act) = st.activation.as_mut() else {
        return Task::none();
    };
    let Some(ws) = act.ws.clone().filter(|ws| m.workspace(ws).is_some()) else {
        return Task::none();
    };
    act.error = None;
    act.running = Some(step.clone());
    let kit = act.kit.clone();
    let _ = t;

    let active = m.state().active_workspace_id.clone();
    // Asleep (its panes unmounted, its agents stopped), as the pane logic's
    // hibernation tells: one already selected is selected again to wake.
    let asleep = m.live.panes_logic.is_asleep(&ws);
    let selections = show_workspace_selections(&ws, active.as_deref(), asleep);
    let wake = selections.len() > 1;
    let first = if wake {
        m.apply(StoreAction::SetActiveWorkspace(None));
        Task::perform(tokio::time::sleep(WAKE_PAUSE), |()| Message::Noop)
    } else {
        Task::none()
    };
    first
        .chain(intent(Intent::ActivateWorkspace(ws)))
        .chain(Task::done(kits(Msg::SetupPane { kit, step })))
}

/// The terminal of a pane, as the setup types into it.
struct PtyTyping {
    pty: Arc<PtyManager>,
}

impl PaneTyping for PtyTyping {
    async fn alive(&self, pane: &str) -> bool {
        crate::nodes::remote::alive(pane)
            .unwrap_or_else(|| hangar_core::pty::pty_alive(&self.pty, pane.to_owned()))
    }

    /// A pane of a remote workspace is typed into on its node (the kit's
    /// files stay this machine's: kits of remote files come later).
    async fn write(&self, pane: &str, data: &str) -> Result<(), String> {
        crate::nodes::remote::write(pane, data.to_owned()).unwrap_or_else(|| {
            hangar_core::pty::pty_write(&self.pty, pane.to_owned(), data.to_owned())
        })
    }

    async fn wait(&self, delay: Duration) {
        tokio::time::sleep(delay).await;
    }

    /// The model is not reachable from a task: the pane is closed by
    /// `SetupDone` when the run did not come to typing.
    fn close_pane(&self, _pane: &str) {}
}

fn setup_pane(
    st: &mut State,
    m: &mut Model,
    env: &Env,
    kit: String,
    step: String,
) -> Task<Message> {
    let t = translator(m);
    let Some(act) = st
        .activation
        .as_mut()
        .filter(|a| a.kit == kit && a.running.as_deref() == Some(step.as_str()))
    else {
        return Task::none();
    };
    let Some(ws) = act.ws.clone() else {
        act.running = None;
        return Task::none();
    };
    let Some(command) = act
        .steps
        .as_ref()
        .and_then(|steps| steps.iter().find(|s| s.id == step))
        .map(|s| s.command.clone())
    else {
        act.running = None;
        return Task::none();
    };
    let cwd = m.live.kits.find(&kit).map(|found| found.dir.clone());
    let opened = m.apply(StoreAction::AddPane {
        ws: ws.clone(),
        agent: Some(AgentId::Shell),
        near: None,
        dir: None,
        cwd,
        name: None,
        init: None,
    });
    let Some(pane) = opened.pane else {
        act.running = None;
        act.error = Some(t.t(Key::KitsManageStepNoPane).into_owned());
        return Task::none();
    };
    let pane_name = m
        .workspace(&ws)
        .and_then(|w| w.panes.iter().find(|p| p.id == pane))
        .map(|p| p.name.clone())
        .unwrap_or_default();
    let deps = PtyTyping {
        pty: env.pty.clone(),
    };
    let typed = pane.clone();
    Task::perform(
        async move { type_into_pane(&typed, &command, &deps).await },
        move |result| {
            kits(Msg::SetupDone {
                kit: kit.clone(),
                step: step.clone(),
                ws: ws.clone(),
                pane: pane.clone(),
                pane_name: pane_name.clone(),
                result,
            })
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn setup_done(
    st: &mut State,
    m: &mut Model,
    kit: String,
    step: String,
    ws: WsId,
    pane: String,
    pane_name: String,
    result: SetupRunResult,
) -> Task<Message> {
    let t = translator(m);
    let detached = m.workspace(&ws).is_some_and(|w| w.detached == Some(true));
    // A pane that never took the command is not left behind.
    let failed = !matches!(result, SetupRunResult::Ok { .. });
    if failed {
        m.apply(StoreAction::ClosePane {
            ws,
            pane: pane.clone(),
        });
    }
    let Some(act) = st.activation.as_mut().filter(|a| a.kit == kit) else {
        return Task::none();
    };
    act.running = None;
    match result {
        SetupRunResult::Ok { .. } => {
            act.ran = Some(StepRun {
                step,
                pane_name: if pane_name.is_empty() {
                    "…".into()
                } else {
                    pane_name
                },
            });
        }
        SetupRunResult::NoPane => {
            act.error = Some(t.t(Key::KitsManageStepNoPane).into_owned());
        }
        SetupRunResult::NotStarted => {
            act.error = Some(
                t.t(if detached {
                    Key::KitsManageStepPaneLostDetached
                } else {
                    Key::KitsManageStepPaneLost
                })
                .into_owned(),
            );
        }
        SetupRunResult::WriteFailed { detail } => {
            act.error = Some(
                t.t_with(
                    Key::KitsManageStepFailed,
                    &args!("detail" => detail.as_str()),
                )
                .into_owned(),
            );
        }
    }
    Task::none()
}

#[cfg(test)]
mod tests;
