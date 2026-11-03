//! P10: Orchestration panel and badge.
//!
//! Ports OrchestrationPanel.tsx, OrchestrationBadge.tsx,
//! lib/{orchestrationForm,orchestrationActions}.ts. Read inventory-frontend.md
//! §2.2 Orchestration and crates/hangar-app/PACKAGES.md. Files of this
//! directory: `mod`, `panel`, `badge`, `form` (the draft and its rules) and
//! `actions` (pin, unpin, create, the project manager).
//!
//! The shell's intents reach the actions here: `Intent::PinOrchestrator`,
//! `CreateOrchestrator` and `OpenManager` are handled in `on_intent`, which
//! also sets the panel up when `OpenOrchestration` (or `OpenOverlay`) names a
//! workspace.

pub mod actions;
mod badge;
mod form;
mod panel;

use iced::{Element, Subscription, Task};

use hangar_i18n::{Key, args};

use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::{Notice, Overlay, Region, View};
use crate::state::types::PaneRole;
use crate::state::{Model, PaneId, WsId};

// For the pane header (P5), which knows its own width.
#[allow(unused_imports)]
pub use badge::{has_badge, pane_badge_in};

/// The package's UI state; lives in `App.pkg.orchestration_ui`.
/// `actions` provides pin, unpin, create orchestrator and open manager, reached
/// through `on_intent` (`Intent::PinOrchestrator`, `CreateOrchestrator`, `OpenManager`).
#[derive(Debug, Default)]
pub struct State {
    /// Keeps two requests for the manager from both making the HQ's folder.
    manager: actions::SingleFlight,
    /// The panel while `Overlay::Orchestration` is open.
    panel: Option<panel::Panel>,
}

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.orchestration_ui`. Nothing is shared: what other packages need
/// is a function (`actions`, `pane_badge`).
#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::OrchestrationUi(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    Panel(panel::PanelMsg),
    /// `hq_dir` answered: the folder of the HQ, made, or why not.
    FolderMade(Result<String, String>),
}

pub fn update(st: &mut State, m: &mut Model, _sv: &Services, msg: Msg) -> Task<Message> {
    match msg {
        Msg::Panel(message) => panel::update(st, m, message),
        Msg::FolderMade(folder) => {
            st.manager.end();
            let name = hq_name(m);
            let done = actions::finish_open_manager(m, &name, folder);
            manager_opened(m, done)
        }
    }
}

/// Nothing to follow: a worker's badge reads the title of its task from the
/// boards cache (`board::Live`), which holds the board of every workspace's
/// folder, a detached workspace's and a worktree's shared one included, and
/// reads it again when it changes.
pub fn subscription(_st: &State, _m: &Model) -> Subscription<Message> {
    Subscription::none()
}

/// What takes the focus when F6 reaches a region this package owns. It owns
/// none, so the shell never calls it.
pub fn focus_entry(_st: &State, _m: &Model, _region: Region) -> Option<Task<Message>> {
    None
}

fn hq_name(m: &Model) -> String {
    hangar_i18n::t(m.locale(), Key::ManagerHqName).into_owned()
}

fn say(notice: Notice) -> Task<Message> {
    Task::done(Message::Intent(Intent::Notice(notice)))
}

/// Pins or unpins a pane as its workspace's orchestrator, and says so. The
/// session is told of its new role by a message queued for it (P19).
fn pin_orchestrator(m: &mut Model, ws_id: &str, pane_id: &str, pinned: bool) -> Task<Message> {
    let Some(ws) = m.workspace(ws_id) else {
        return Task::none();
    };
    let Some(pane) = ws.panes.iter().find(|pane| pane.id == pane_id) else {
        return Task::none();
    };
    let (name, workspace) = (pane.name.clone(), ws.name.clone());
    let changed = if pinned {
        actions::pin(m, ws_id, pane_id)
    } else {
        actions::unpin(m, ws_id, pane_id)
    };
    if !changed {
        return Task::none();
    }
    let text = hangar_i18n::t_with(
        m.locale(),
        if pinned {
            Key::OrchPinned
        } else {
            Key::OrchUnpinned
        },
        &args!("name" => name.as_str(), "workspace" => workspace.as_str()),
    );
    Task::batch([
        say(Notice::Agent(text.into_owned())),
        Task::done(Message::Intent(Intent::AnnounceRole {
            ws: ws_id.to_owned(),
            pane: pane_id.to_owned(),
            previous: (!pinned).then_some(PaneRole::Orchestrator),
        })),
    ])
}

/// A new Claude Code pane, pinned from the start, brought forward.
fn create_orchestrator(m: &mut Model, ws_id: &str) -> Task<Message> {
    let Some(ws) = m.workspace(ws_id) else {
        return Task::none();
    };
    if !actions::can_create_orchestrator(ws) {
        return Task::none();
    }
    let workspace = ws.name.clone();
    let near = m.ui.focus_by_workspace.get(ws_id).cloned();
    let Some(id) = actions::create(m, ws_id, near) else {
        let text = hangar_i18n::t_with(
            m.locale(),
            Key::OrchNoRoom,
            &args!("workspace" => workspace.as_str()),
        );
        return say(Notice::Problem(text.into_owned()));
    };
    let name = m
        .workspace(ws_id)
        .and_then(|ws| ws.panes.iter().find(|pane| pane.id == id))
        .map(|pane| pane.name.clone())
        .unwrap_or_default();
    let text = hangar_i18n::t_with(
        m.locale(),
        Key::OrchPinned,
        &args!("name" => name.as_str(), "workspace" => workspace.as_str()),
    );
    Task::batch([
        Task::done(Message::Intent(Intent::FocusPane {
            ws: ws_id.to_owned(),
            pane: id,
            terminal: true,
        })),
        say(Notice::Agent(text.into_owned())),
    ])
}

/// « Ouvrir le chef de projet »: the HQ and its manager, made if need be,
/// then shown.
fn open_manager(st: &mut State, m: &mut Model, sv: &Services) -> Task<Message> {
    if !st.manager.begin() {
        return Task::none();
    }
    let name = hq_name(m);
    match actions::begin_open_manager(m, &name) {
        actions::ManagerStep::Done(done) => {
            st.manager.end();
            manager_opened(m, done)
        }
        actions::ManagerStep::NeedsFolder => {
            let host = sv.core().host().clone();
            Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || hangar_core::hq_dir(&host))
                        .await
                        .map_err(|error| error.to_string())
                        .and_then(|folder| folder)
                },
                |folder| Message::OrchestrationUi(Msg::FolderMade(folder)),
            )
        }
    }
}

/// What follows opening the manager: the terminals shown on its pane, and a
/// notice for what was made, or for why nothing was.
fn manager_opened(m: &Model, done: actions::OpenManager) -> Task<Message> {
    use actions::OpenManager as Opened;
    let (workspace, pane, made) = match done {
        Opened::Failed { error } => {
            let text = match error {
                Some(error) => hangar_i18n::t_with(
                    m.locale(),
                    Key::ManagerFailedWith,
                    &args!("error" => error.as_str()),
                ),
                None => hangar_i18n::t(m.locale(), Key::ManagerFailed),
            };
            return say(Notice::Problem(text.into_owned()));
        }
        Opened::Shown { workspace, pane } => (workspace, pane, None),
        Opened::Created { workspace, pane } => (workspace, pane, Some(Key::ManagerCreated)),
        Opened::Restored { workspace, pane } => (workspace, pane, Some(Key::ManagerRestored)),
    };
    let mut tasks = vec![
        Task::done(Message::Intent(Intent::SetView(View::Terminals))),
        Task::done(Message::Intent(Intent::FocusPane {
            ws: workspace,
            pane,
            terminal: true,
        })),
    ];
    if let Some(key) = made {
        tasks.push(say(Notice::Agent(
            hangar_i18n::t(m.locale(), key).into_owned(),
        )));
    }
    Task::batch(tasks)
}

/// Every intent, after the shell's own handling of it: `Some` when this
/// package acted on it.
pub fn on_intent(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    match intent {
        // The shell pushes the overlay; the panel opens on what the workspace
        // runs with now.
        Intent::OpenOrchestration(ws) | Intent::OpenOverlay(Overlay::Orchestration(ws)) => {
            st.panel = panel::Panel::open(m, ws);
            Some(Task::none())
        }
        Intent::PinOrchestrator { ws, pane, pinned } => {
            Some(pin_orchestrator(m, ws, pane, *pinned))
        }
        Intent::CreateOrchestrator(ws) => Some(create_orchestrator(m, ws)),
        Intent::OpenManager => Some(open_manager(st, m, sv)),
        _ => None,
    }
}

/// Every core event, after the shell's own handling of it. None concerns this
/// package: a board that changed reaches the badges through `board::Live`.
pub fn on_event(
    _st: &mut State,
    _m: &mut Model,
    _sv: &Services,
    _event: &CoreEvent,
) -> Option<Task<Message>> {
    None
}

/// `Overlay::Orchestration`: a workspace's orchestration settings.
pub fn panel_view<'a>(st: &'a State, ctx: Ctx<'a>, ws: &'a WsId) -> Element<'a, Message> {
    panel::view(st, ctx, ws)
}

/// A pane bar's orchestration badge (orchestrator, worker, manager); nothing
/// for any other pane. `has_badge` says whether there is one.
pub fn pane_badge<'a>(ctx: Ctx<'a>, ws: &'a WsId, pane: &'a PaneId) -> Element<'a, Message> {
    badge::pane_badge(ctx, ws, pane)
}
