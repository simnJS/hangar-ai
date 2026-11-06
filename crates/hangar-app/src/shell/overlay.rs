//! Overlays: the dialogs and sheets over the window, as a stack
//! (`Model.ui.overlays`, each window its own). The top one has the keyboard.
//! Escape closes it: the overlay's own hangar-ui layer, which captures the
//! key, or else the shell, which only sees an Escape nothing captured, so one
//! press never closes two. The focus trap is the layer's; closing gives the
//! keyboard back to what had it when the overlay opened
//! (`Model.ui.overlay_openers`, app/shell.rs).
//!
//! A variant only says which overlay it is and for what (a workspace, a
//! pane); the overlay's own state lives in its package, which sets it up when
//! `Intent::OpenOverlay` reaches its `on_intent`. The package draws the whole
//! overlay, backdrop included (hangar-ui's modal); the shell puts it over the
//! window and keeps the clicks from reaching what is under it.

use iced::Element;
use iced::widget::opaque;

use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::{PaneId, WsId};

/// The categories of the settings sheet, in its order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum SettingsCategory {
    #[default]
    General,
    Appearance,
    Terminal,
    Shell,
    Notifications,
    Mobile,
    Kits,
    Accounts,
    Voice,
    Discord,
    Shortcuts,
    /// Only while the `nodes` experiment is on.
    Nodes,
    Experimental,
    About,
}

impl SettingsCategory {
    /// The id the TS used (`settingsCategory`), for anything that names one
    /// by string (a control op, a search result).
    pub fn id(self) -> &'static str {
        match self {
            SettingsCategory::General => "general",
            SettingsCategory::Appearance => "appearance",
            SettingsCategory::Terminal => "terminal",
            SettingsCategory::Shell => "shell",
            SettingsCategory::Notifications => "notifications",
            SettingsCategory::Mobile => "mobile",
            SettingsCategory::Kits => "kits",
            SettingsCategory::Accounts => "accounts",
            SettingsCategory::Voice => "voice",
            SettingsCategory::Discord => "discord",
            SettingsCategory::Shortcuts => "shortcuts",
            SettingsCategory::Nodes => "nodes",
            SettingsCategory::Experimental => "experimental",
            SettingsCategory::About => "about",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Overlay {
    /// The full settings sheet (P13).
    Settings(SettingsCategory),
    /// The command palette (P7).
    Palette,
    /// New workspace (P8).
    WorkspaceDialog,
    /// A pane's sessions to resume (P7).
    SessionPicker { ws: WsId, pane: PaneId },
    /// A saved command, new or edited (P7).
    CommandDialog {
        ws: WsId,
        pane: PaneId,
        /// The command being edited; `None` makes one.
        edit: Option<String>,
    },
    /// Connect agents to the board (P10).
    McpInstall,
    /// A workspace's MCP servers (P10).
    McpServers(WsId),
    /// A workspace's orchestration settings (P10).
    Orchestration(WsId),
    /// Git worktrees (P9).
    Worktrees(WsId),
    /// A workspace's kits (P8, embedding P14's section).
    KitsDialog(WsId),
    /// Enabling a kit (P14).
    KitActivation { kit: String, ws: Option<WsId> },
    /// The changes of a workspace or a pane (P9).
    Diff { ws: WsId, pane: Option<PaneId> },
    /// A board task, new or opened (P12).
    Task { cwd: String, task: Option<String> },
}

/// The overlay drawn by the package that owns it, over everything below.
pub fn view<'a>(ctx: Ctx<'a>, overlay: &'a Overlay) -> Element<'a, Message> {
    let p = ctx.pkg;
    let content = match overlay {
        Overlay::Settings(category) => crate::settings::view(&p.settings, ctx, *category),
        Overlay::Palette => crate::palette::view(&p.palette, ctx),
        Overlay::WorkspaceDialog => crate::workspace_dialog::view(&p.workspace_dialog, ctx),
        Overlay::SessionPicker { ws, pane } => {
            crate::session_picker::view(&p.session_picker, ctx, ws, pane)
        }
        Overlay::CommandDialog { ws, pane, edit } => {
            crate::saved_commands::dialog_view(&p.saved_commands, ctx, ws, pane, edit.as_deref())
        }
        Overlay::McpInstall => crate::mcp::install_view(&p.mcp, ctx),
        Overlay::McpServers(ws) => crate::mcp::servers_view(&p.mcp, ctx, ws),
        Overlay::Orchestration(ws) => {
            crate::orchestration_ui::panel_view(&p.orchestration_ui, ctx, ws)
        }
        Overlay::Worktrees(ws) => crate::git_ui::worktrees_view(&p.git_ui, ctx, ws),
        Overlay::KitsDialog(ws) => {
            crate::workspace_dialog::kits_dialog_view(&p.workspace_dialog, ctx, ws)
        }
        Overlay::KitActivation { kit, ws } => {
            crate::kits::activation_view(&p.kits, ctx, kit, ws.as_ref())
        }
        Overlay::Diff { ws, pane } => crate::git_ui::diff_view(&p.git_ui, ctx, ws, pane.as_ref()),
        Overlay::Task { cwd, task } => crate::board::task_view(&p.board, ctx, cwd, task.as_deref()),
    };
    opaque(content)
}
