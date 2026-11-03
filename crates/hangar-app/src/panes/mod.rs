//! P5: Pane grid and terminal pane.
//!
//! Ports WorkspaceTerminals.tsx, PaneGrid.tsx, TerminalPane.tsx,
//! TerminalSearch.tsx, PaneZoomBar.tsx, PaneGauge.tsx, PaneStatus.ts,
//! lib/{ptyBus,terminalRegistry}.ts. Read inventory-frontend.md §4 (all),
//! §2.1/12 and crates/hangar-app/PACKAGES.md. Planned files of this directory
//! (§8.1): mod, workspace_terminals, grid, terminal_pane, header, search,
//! zoom_bar, gauge, status, controller, pty_link, agent_watch, hub.
//!
//! This is the stub P0 handed over: the surface the shell calls, with
//! placeholder bodies. The directory is this package's own; replace the
//! bodies, add files, keep the signatures the shell calls.

use std::sync::Arc;

use iced::{Element, Subscription, Task};

use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services, TerminalAccess, terminals::NoTerminals};
use crate::shell::{Region, placeholder};
use crate::state::{Model, WsId};

/// The package's UI state; lives in `App.pkg.panes`.
/// The terminals (hangar-term) and their PTY links live here, not in a
/// window: a detached workspace is only drawn elsewhere.
#[derive(Debug, Default)]
pub struct State;

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.panes`. Nothing yet.
#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::Panes(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    match msg {}
}

pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    Subscription::none()
}

/// What takes the focus when F6 reaches region `Main`, which this package
/// owns.
pub fn focus_entry(st: &State, m: &Model, region: Region) -> Option<Task<Message>> {
    None
}

/// Every intent, after the shell's own handling of it: `Some` when this
/// package acted on it.
pub fn on_intent(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    None
}

/// Every core event, after the shell's own handling of it.
pub fn on_event(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    event: &CoreEvent,
) -> Option<Task<Message>> {
    None
}

/// Region `main` in the terminals view: the grid of workspace `ws`. Called for
/// every opened workspace; `hidden` ones are not on screen but stay alive
/// (their terminals keep consuming output) and should draw nothing.
pub fn view_workspace<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    ws: &'a WsId,
    hidden: bool,
) -> Element<'a, Message> {
    placeholder::stub(ctx, "panes::view_workspace")
}

/// What the engines may do with the terminals (`Services::terminals`). The
/// shell asks once, at boot, after `State::default()`: return a handle on the
/// hub this state holds.
pub fn terminal_access(st: &State) -> Arc<dyn TerminalAccess> {
    Arc::new(NoTerminals)
}
