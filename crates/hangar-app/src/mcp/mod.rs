//! P10: MCP panels.
//!
//! Ports McpPanel.tsx, McpServersPanel.tsx, lib/mcpScope.ts. Read
//! inventory-frontend.md §2.2 MCP and crates/hangar-app/PACKAGES.md. Files of
//! this directory: `mod`, `install_panel` (connect the agents to the board),
//! `servers_panel` (a workspace's MCP servers) and `scope` (the pure rules, and
//! the shell-quoting helpers of the pane launch).
//!
//! Both are overlays: the shell pushes the `Overlay` and wraps the view, and
//! `on_intent` sets the panel's state up and starts what it reads.

pub mod install_panel;
pub mod scope;
pub mod servers_panel;

use iced::{Element, Subscription, Task};

use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::Overlay;
use crate::state::{Model, WsId};

/// The package's UI state; lives in `App.pkg.mcp`.
#[derive(Debug, Default)]
pub struct State {
    /// « Connecter les agents au tableau », while it is open.
    install: Option<install_panel::Install>,
    /// A workspace's MCP servers, while they are open.
    servers: Option<servers_panel::Servers>,
}

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.mcp`. Nothing is shared: the pure rules other packages need
/// (the pane launch's quoting) are plain functions of `scope`.
#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::Mcp(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    Install(install_panel::InstallMsg),
    Servers(servers_panel::ServersMsg),
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    match msg {
        Msg::Install(message) => install_panel::update(st, m, sv, message),
        Msg::Servers(message) => servers_panel::update(st, m, sv, message),
    }
}

pub fn subscription(_st: &State, _m: &Model) -> Subscription<Message> {
    Subscription::none()
}

/// Every intent, after the shell's own handling of it: `Some` when this
/// package acted on it. The shell pushes the overlay; the panel opens here,
/// on what is current now.
pub fn on_intent(
    st: &mut State,
    m: &mut Model,
    _sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    match intent {
        Intent::OpenMcpInstall | Intent::OpenOverlay(Overlay::McpInstall) => {
            Some(install_panel::open(st, m))
        }
        Intent::OpenMcpServers(ws) | Intent::OpenOverlay(Overlay::McpServers(ws)) => {
            Some(servers_panel::open(st, m, ws))
        }
        _ => None,
    }
}

/// Every core event, after the shell's own handling of it.
pub fn on_event(
    _st: &mut State,
    _m: &mut Model,
    _sv: &Services,
    _event: &CoreEvent,
) -> Option<Task<Message>> {
    None
}

/// `Overlay::McpInstall`: connect agents to the board.
pub fn install_view<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    install_panel::view(st, ctx)
}

/// `Overlay::McpServers`: a workspace's MCP servers.
pub fn servers_view<'a>(st: &'a State, ctx: Ctx<'a>, ws: &'a WsId) -> Element<'a, Message> {
    servers_panel::view(st, ctx, ws)
}
