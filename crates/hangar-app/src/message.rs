//! The messages of the app: the shell's own, one route per package, and the
//! intents packages send one another (inventory §8.2).
//!
//! `Message` is closed: a package adds variants to its own `Msg`, never here.
//! `Intent` is the one shared enum a package may extend, by appending one
//! variant per commit (PACKAGES.md, rule 5), and handling it in its own
//! `on_intent`.

use std::time::Instant;

use iced::{keyboard, window};
use serde_json::Value;

use crate::commands::CommandId;
use crate::services::{CoreEvent, PreviewRequest};
use crate::shell::{CloseReason, Notice, Notification, Overlay, Region, SettingsCategory, View};
use crate::state::{PaneId, PanePatch, SplitDir, StoreAction, WsId};
use crate::{
    accounts, board, consent_card, control, git_flow, git_ui, inspector, keymap, kit_tabs, kits,
    mcp, memory, mobile, model3d_view, navigator, needs_you, orchestration, orchestration_ui,
    palette, pane_menu, panes, panes_logic, preview, saved_commands, session_picker, settings,
    stats, status_bar, toolbar, update_banner, voice, workspace_dialog,
};

#[derive(Debug, Clone)]
pub enum Message {
    // The shell's own.
    /// `load_state` answered: the raw JSON, `None` when there is no file yet.
    Booted(Result<Option<Value>, String>),
    /// The model has been still for 400 ms since this revision (persist.rs).
    SaveDue(u64),
    /// A save finished.
    Saved(u64, Result<(), String>),
    /// `detect_agents` and `detect_shells` answered.
    Detected(Vec<String>, Vec<crate::state::types::ShellInfo>),
    Core(CoreEvent),
    Window(window::Id, window::Event),
    Key(window::Id, keyboard::Event),
    Command(CommandId),
    Intent(Intent),
    Store(StoreAction),
    Tick(Instant),
    NoticeExpired(u64),
    /// The main window as `HANGAR_DEV_SCREENSHOT` asked for it (debug builds).
    DevScreenshot(window::Screenshot),
    /// What only the frame and the command table ask of the shell
    /// (`shell::Action`); packages send intents instead.
    Shell(crate::shell::Action),
    Noop,

    // One route per package; each `Msg` lives in its package.
    PanesLogic(panes_logic::Msg),
    Panes(panes::Msg),
    Navigator(navigator::Msg),
    Toolbar(toolbar::Msg),
    StatusBar(status_bar::Msg),
    NeedsYou(needs_you::Msg),
    UpdateBanner(update_banner::Msg),
    Voice(voice::Msg),
    Keymap(keymap::Msg),
    Palette(palette::Msg),
    SavedCommands(saved_commands::Msg),
    SessionPicker(session_picker::Msg),
    PaneMenu(pane_menu::Msg),
    WorkspaceDialog(workspace_dialog::Msg),
    GitFlow(git_flow::Msg),
    Consent(consent_card::Msg),
    Accounts(accounts::Msg),
    Inspector(inspector::Msg),
    GitUi(git_ui::Msg),
    Memory(memory::Msg),
    Mcp(mcp::Msg),
    OrchestrationUi(orchestration_ui::Msg),
    Stats(stats::Msg),
    Board(board::Msg),
    Settings(settings::Msg),
    Kits(kits::Msg),
    Preview(preview::Msg),
    KitTabs(kit_tabs::Msg),
    Model3d(model3d_view::Msg),
    Control(control::Msg),
    Orchestration(orchestration::Msg),
    Mobile(mobile::Msg),
}

/// Every cross-package request found in the inventory. The shell handles its
/// part of each (an overlay pushed, a pane focused), then offers it to every
/// package's `on_intent`.
///
/// Append new variants at the end, one per commit, with a comment naming the
/// package that handles it.
#[derive(Debug, Clone)]
pub enum Intent {
    ActivateWorkspace(WsId),
    /// App.tsx's `activateRef`: the workspace, the pane, and with `terminal`
    /// the keyboard in its terminal.
    FocusPane {
        ws: WsId,
        pane: PaneId,
        terminal: bool,
    },
    SetView(View),
    OpenOverlay(Overlay),
    /// Closes the top overlay.
    CloseOverlay,
    ClosePopovers(CloseReason),
    /// `None`: the last category, else General.
    OpenSettings(Option<SettingsCategory>),
    TogglePalette,
    ToggleNavigator,
    FocusNavigator,
    ToggleInspector,
    FocusInspector,
    ToggleDock,
    CloseDock,
    TogglePreview,
    ClosePreviewPanel,
    TogglePreviewExpand,
    /// F6 (`1`) and Shift+F6 (`-1`).
    CycleRegion(i8),
    FocusRegion(Region),
    SplitPane {
        ws: WsId,
        near: Option<PaneId>,
        dir: Option<SplitDir>,
    },
    /// Respawn with a patch, keep the focus on it, carry its resume.
    ReplacePane {
        ws: WsId,
        pane: PaneId,
        patch: PanePatch,
    },
    RestartPanes {
        ws: WsId,
        panes: Vec<PaneId>,
    },
    ArmResetAgents(bool),
    ResetAgents(WsId),
    ApplyPreset {
        ws: WsId,
        size: u8,
    },
    Tidy(WsId),
    SetZoom {
        ws: WsId,
        on: bool,
    },
    Detach(WsId),
    Reattach(WsId),
    Sleep(WsId),
    PinOrchestrator {
        ws: WsId,
        pane: PaneId,
        pinned: bool,
    },
    CreateOrchestrator(WsId),
    OpenManager,
    ArmStopWorkers(bool),
    StopWorkers(WsId),
    /// « Fusionner »: land a delivery.
    Land {
        ws: WsId,
        task: String,
    },
    OpenDiff {
        ws: WsId,
        pane: Option<PaneId>,
    },
    OpenWorktrees(WsId),
    OpenKitsDialog(WsId),
    OpenMcpInstall,
    OpenMcpServers(WsId),
    OpenOrchestration(WsId),
    OpenSessions {
        ws: WsId,
        pane: PaneId,
    },
    OpenBoard(WsId),
    OpenTask {
        cwd: String,
        task: Option<String>,
    },
    ShowPreview(PreviewRequest),
    OpenKitTab {
        ws: WsId,
        kit: String,
        tab: String,
    },
    /// The terminal search of a pane.
    OpenFinder(PaneId),
    RunSavedCommand {
        ws: WsId,
        from: PaneId,
        index: usize,
    },
    /// Types `text` into every pane of the workspace; with `when_free`, into
    /// each agent once it is idle (the bridge queue).
    Broadcast {
        ws: WsId,
        text: String,
        when_free: bool,
    },
    /// A dictation's text, for whatever has the focus.
    InsertTranscript(String),
    Notice(Notice),
    Notify(Notification),
    CreateWorkspaceFromGit(GitRequest),
    /// The `terminal.*` commands on a pane's terminal (App.tsx's handlers on
    /// `getTerminal(focused)`): clear, copy the selection, paste through the
    /// terminal (bracketed), select all, scroll to either end. Sent by the
    /// command table; handled by P5 (`panes`), which holds the terminals.
    TerminalAction {
        pane: PaneId,
        action: TerminalAction,
    },
    /// `voice.dictate` was pressed: push to talk starts (`voice.press` of
    /// lib/voice.ts). Sent by the command table, only while dictation is
    /// enabled; handled by P6 (`voice`), which also watches for the key's
    /// release that ends the recording.
    Dictate,
}

/// What `Intent::TerminalAction` asks of a pane's terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TerminalAction {
    Clear,
    /// The selection to the clipboard, when there is one.
    Copy,
    /// The clipboard's text through the terminal, which brackets it, so a
    /// multi-line paste lands as text instead of running line by line.
    Paste,
    SelectAll,
    ScrollTop,
    ScrollBottom,
}

/// « From a Git repository » (`FromGitRequest` of lib/workspaceFromGit.ts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRequest {
    /// As typed or handed over: `owner/repo`, an https or ssh address.
    pub repo: String,
    /// The workspace's name; the repository's by default.
    pub name: Option<String>,
    /// The folder the clone lands in, when the person said where.
    pub parent: Option<String>,
    pub origin: GitOrigin,
    /// The folder of the workspace asking: where the clone goes when no
    /// projects folder is set.
    pub asker_cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitOrigin {
    /// An agent, through the MCP tool: a repository not the person's needs
    /// their word (the consent card).
    Agent { pane: Option<PaneId> },
    /// The person clicked: asking again would be asking what they just said.
    Person,
}
