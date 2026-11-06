//! The persisted model: `AppState` and what it holds, as `src/types.ts`
//! declared it and as `state.json` stores it (inventory §1.2 and §1.4).
//!
//! The file is shared with the old app while people may go back and forth,
//! so these types follow its shape exactly rather than what would be nicest
//! in Rust:
//!
//! - keys are camelCase, as the TS wrote them;
//! - `T | null` fields are `Option<T>` written as `null`, and `?` fields are
//!   `Option<T>` left out when absent (`kits` and `claudeAccount` are absent
//!   on purpose rather than empty);
//! - every struct the TS spread (`{...ws, ...patch}`) keeps the keys it does
//!   not know in `extra`, so a file written by a newer version, or edited by
//!   hand, loses nothing when this one saves it;
//! - a missing field takes its default (`DEFAULT_SETTINGS` for the
//!   settings), and a malformed one is read leniently (`lenient.rs`), since a
//!   strict parse error would quarantine the person's workspaces.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use hangar_i18n::Locale;

/// A workspace's id, as the TS made it (`crypto.randomUUID()` or older
/// formats): any string.
pub type WsId = String;
/// A pane's id. A respawn gives the pane a new one (inventory §1.3).
pub type PaneId = String;

/// Unknown keys of a struct, kept as they were read.
pub type Extra = Map<String, Value>;

/// The most panes a workspace holds (`MAX_PANES`).
pub const MAX_PANES: usize = 16;

/// Bounds of the interface size, in percent (`UI_SCALE_MIN`/`UI_SCALE_MAX`).
pub const UI_SCALE_MIN: f64 = 90.0;
pub const UI_SCALE_MAX: f64 = 130.0;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppState {
    pub workspaces: Vec<Workspace>,
    /// In sidebar order.
    pub folders: Vec<WorkspaceFolder>,
    pub active_workspace_id: Option<WsId>,
    pub settings: Settings,
    #[serde(flatten)]
    pub extra: Extra,
}

/// What a pane runs (`AgentId`): Claude Code, or a plain shell.
///
/// Claude Code is the only agent Hangar runs (owner's decision, 2026-11-03).
/// The old app also offered `codex`, `gemini` and `opencode`; a pane saved
/// with one of them opens as a shell (`RETIRED_AGENTS`, lenient.rs).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentId {
    #[default]
    Shell,
    Claude,
}

/// The agents the old app could run and this one no longer does, by the id
/// `state.json` holds, with the name a pane's note gives them.
pub const RETIRED_AGENTS: [(&str, &str); 3] = [
    ("codex", "Codex"),
    ("gemini", "Gemini CLI"),
    ("opencode", "OpenCode"),
];

/// One row of `AGENTS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentInfo {
    pub id: AgentId,
    pub label: &'static str,
    /// Whether it can be relaunched on a previous session (`--resume`).
    pub resumable: bool,
}

/// `AGENTS`, in the order the pickers list them.
pub const AGENTS: [AgentInfo; 2] = [
    AgentInfo {
        id: AgentId::Shell,
        label: "Shell",
        resumable: false,
    },
    AgentInfo {
        id: AgentId::Claude,
        label: "Claude Code",
        resumable: true,
    },
];

impl AgentId {
    pub fn info(self) -> &'static AgentInfo {
        AGENTS
            .iter()
            .find(|agent| agent.id == self)
            .unwrap_or(&AGENTS[0])
    }

    /// The id as `state.json` writes it.
    pub fn as_str(self) -> &'static str {
        match self {
            AgentId::Shell => "shell",
            AgentId::Claude => "claude",
        }
    }
}

/// A pane's part in its workspace's orchestration (`PaneRole`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaneRole {
    Orchestrator,
    Worker,
    Manager,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Pane {
    pub id: PaneId,
    /// Unique across all workspaces, stable across splits, moves, restarts.
    pub name: String,
    pub agent: AgentId,
    /// The agent session to resume on the next launch.
    pub session_id: Option<String>,
    /// Overrides the workspace root.
    pub cwd: Option<String>,
    /// Overrides the workspace shell.
    pub shell_id: Option<String>,
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<PaneRole>,
    /// A worker's board task.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// `--model` of its agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// First port a worker may bind (`HANGAR_PORT_BASE`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_base: Option<u32>,
    /// A worker in the workspace's own folder (no worktree).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_folder: Option<bool>,
    /// The Claude account over its workspace's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claude_account: Option<String>,
    /// A Claude Code pane opened to log an account in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub login: Option<bool>,
    /// The agent the file said this pane ran, when it is one this version
    /// no longer runs (`RETIRED_AGENTS`): the reader made the pane a shell
    /// and set this, so its bar says why once, as it first starts
    /// (panes/controller.rs). Never written.
    #[serde(skip)]
    pub retired_agent: Option<&'static str>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SavedCommand {
    pub id: String,
    /// Empty falls back to the command itself.
    pub label: String,
    pub command: String,
    pub auto_run: bool,
    pub broadcast: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Workspace {
    pub id: WsId,
    pub name: String,
    pub cwd: String,
    /// The other folders of a `.code-workspace`.
    pub extra_roots: Vec<String>,
    pub panes: Vec<Pane>,
    /// Overrides the global theme.
    pub theme_id: Option<String>,
    /// Overrides the global default shell.
    pub shell_id: Option<String>,
    pub tree: Option<SplitNode>,
    /// The panes of the grid that are not terminals: what the previews show
    /// (a file, a 3D model, a kit's tab), each a leaf of `tree` beside the
    /// terminals. Written only while there is one.
    ///
    /// Hangar 1.x knows only `panes`, and everything that treats them as
    /// terminals (their shells, the MCP pane tools, broadcast, hibernation)
    /// keeps reading `panes` alone. 1.x carries this field over as it was
    /// (it spreads the workspace), and its `normalizeTree` drops the leaves
    /// it does not know, laying its terminals out again as an even grid.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub view_panes: Vec<ViewPane>,
    pub saved_commands: Vec<SavedCommand>,
    /// The navigator folder it is filed under.
    pub folder_id: Option<String>,
    /// Shown in a window of its own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detached: Option<bool>,
    /// Where that window last stood, in logical pixels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_bounds: Option<WindowBounds>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp: Option<WorkspaceMcp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kits: Option<Vec<WorkspaceKit>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orchestration: Option<OrchestrationSettings>,
    /// The project manager's workspace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hq: Option<bool>,
    /// Ports Hangar Mobile may open for it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mobile_ports: Option<Vec<u16>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claude_account: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_hooks: Option<WorktreeHooks>,
    /// Shown as the chat view rather than its grid, while the `chat-view`
    /// experiment is on (`chat_view::shown`). Written only while it is: off
    /// is the absence, so a file that never used it stays as it was, and an
    /// experiment turned off leaves the grid without touching the file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_view: Option<bool>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A pane of the grid that shows a preview rather than a terminal. Its id is
/// the id of the preview's entry it draws (`preview::store::Entry`): what it
/// shows, its title and its state are the preview store's, which keeps the
/// kit tabs across a restart in `kit-tabs.json`. A view whose entry does not
/// come back (a file, which the previews never kept) is dropped once its
/// workspace is opened (`preview::panes`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ViewPane {
    pub id: PaneId,
    #[serde(flatten)]
    pub extra: Extra,
}

impl ViewPane {
    pub fn new(id: impl Into<PaneId>) -> ViewPane {
        ViewPane {
            id: id.into(),
            extra: Extra::new(),
        }
    }
}

impl Workspace {
    /// Every pane of the grid, terminals first, then the views, in the order
    /// they were made: what the tree is normalized against
    /// (`layout::grid_tree`).
    pub fn leaf_ids(&self) -> Vec<PaneId> {
        self.panes
            .iter()
            .map(|pane| pane.id.clone())
            .chain(self.view_panes.iter().map(|view| view.id.clone()))
            .collect()
    }

    /// How many panes the grid holds, terminals and views: what `MAX_PANES`
    /// bounds.
    pub fn pane_count(&self) -> usize {
        self.panes.len() + self.view_panes.len()
    }

    pub fn has_view(&self, id: &str) -> bool {
        self.view_panes.iter().any(|view| view.id == id)
    }

    /// A terminal or a view of this workspace's grid.
    pub fn has_leaf(&self, id: &str) -> bool {
        self.panes.iter().any(|pane| pane.id == id) || self.has_view(id)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WorktreeHooks {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_create: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_remove: Option<String>,
}

/// How a workspace's orchestrator works. Missing fields read as
/// `ORCHESTRATION_DEFAULTS` (`lib/orchestration.ts:15`).
///
/// The old app also kept `maxWorkers` and `maxAttempts` here, the most
/// workers at once and the most one task gets. This version sets no such
/// limit, the orchestrator deciding both (owner's decision, 2026-11-04): a
/// file that holds them reads as if it did not, and they are left out of the
/// file at the next save. 1.x then reads its own defaults for them, 3 and 2.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OrchestrationSettings {
    pub auto_merge: bool,
    pub check_command: Option<String>,
    pub orchestrator_model: String,
    pub worker_model: String,
    pub reviewer_model: String,
    pub branch: Option<String>,
    /// `auto`, `acceptEdits` or `bypassPermissions`; null: the person's own.
    pub permission_mode: Option<String>,
    /// The workspace's workers build into one cache folder of the
    /// workspace's own (`hangar_core::build_cache`); the delivery checks have
    /// a folder of theirs whatever this says. Off by default: worktrees
    /// building different branches in one folder can mix their builds.
    /// Missing reads false, and false is not written: a file that never turned
    /// it on stays as it was (what 1.x and the round-trip tests expect). Hangar
    /// 1.x does not know the key and, like any other it does not read, leaves
    /// it alone.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub shared_build_cache: bool,
}

impl Default for OrchestrationSettings {
    fn default() -> Self {
        OrchestrationSettings {
            auto_merge: true,
            check_command: None,
            orchestrator_model: "opus".into(),
            worker_model: "sonnet".into(),
            reviewer_model: "sonnet".into(),
            branch: None,
            permission_mode: None,
            shared_build_cache: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpMode {
    #[default]
    Inherit,
    Custom,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WorkspaceMcp {
    pub mode: McpMode,
    pub enabled: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WorkspaceKit {
    pub id: String,
    pub granted: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A navigator group of workspaces, one level deep; it owns nothing.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WorkspaceFolder {
    pub id: String,
    pub name: String,
    pub collapsed: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `row` places its children side by side, `col` stacks them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SplitDir {
    #[default]
    Row,
    Col,
}

/// The pane arrangement: each split owns its own ratio. Leaves name a pane
/// id, so closing or moving a pane never renumbers (`lib/layout.ts`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SplitNode {
    Pane {
        id: PaneId,
    },
    Split {
        dir: SplitDir,
        ratio: f64,
        a: Box<SplitNode>,
        b: Box<SplitNode>,
    },
}

/// Where a pane is dropped on another (`Zone`): `Center` swaps them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DropZone {
    Left,
    Right,
    Top,
    Bottom,
    Center,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorStyle {
    Block,
    #[default]
    Bar,
    Underline,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VoiceEngine {
    #[default]
    Local,
    Groq,
}

/// What a pane types to start Claude Code where it differs from the plain
/// `claude` (`agentCommands`, by agent id). The old app kept one for each of
/// its agents; this version runs Claude Code only, so it reads Claude Code's
/// and the others' are left out of the file at the next save. A value that is
/// not text reads as not customised.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentCommands {
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "text_or_none"
    )]
    pub claude: Option<String>,
}

fn text_or_none<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Ok(Value::deserialize(deserializer)?
        .as_str()
        .map(str::to_owned))
}

/// The settings. `Default` is `DEFAULT_SETTINGS`, and the file's settings
/// are read over it (`{...DEFAULT_SETTINGS, ...loaded.settings}`). The UI
/// controls of each are in inventory §2.3/2.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub theme_id: String,
    /// `null` follows the system's language.
    pub locale: Option<Locale>,
    /// `null`: the first shell detected.
    pub shell_id: Option<String>,
    pub font_family: String,
    pub font_size: f64,
    pub line_height: f64,
    pub letter_spacing: f64,
    pub cursor_style: CursorStyle,
    pub cursor_blink: bool,
    pub scrollback: u64,
    pub padding: f64,
    /// What a pane types to start Claude Code, where it differs.
    pub agent_commands: AgentCommands,
    pub auto_resume: bool,
    pub hibernate_idle: bool,
    /// Read as `max(1, value)` minutes.
    pub hibernate_after_min: f64,
    pub claude_bridge: bool,
    pub launch_delay_ms: u64,
    pub notify_on_idle: bool,
    pub notify_idle_ms: u64,
    pub notify_only_when_away: bool,
    /// Re-bound shortcuts by command id, overrides only; `[]` disables one.
    /// Ids this version does not know are kept (`prune` in shortcuts.ts).
    pub keybindings: Map<String, Value>,
    pub saved_commands: Vec<SavedCommand>,
    pub voice_enabled: bool,
    pub voice_engine: VoiceEngine,
    pub voice_model: String,
    pub voice_cloud_model: String,
    pub voice_language: String,
    /// Legacy plaintext Groq key; `load_state` moves it to the keychain.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice_api_key: Option<String>,
    pub voice_cleanup: bool,
    pub voice_cleanup_model: String,
    pub voice_cleanup_hint: String,
    pub voice_submit: bool,
    pub discord_presence: bool,
    pub discord_show_workspace: bool,
    pub discord_show_agents: bool,
    pub discord_app_id: String,
    pub board_dock_open: bool,
    pub board_dock_width: f64,
    /// The width of 1.x's preview panel, read and written back as it was for
    /// 1.x: this version has no such panel, every preview being a pane of the
    /// grid.
    pub model_panel_width: f64,
    pub navigator_open: bool,
    pub inspector_open: bool,
    pub inspector_width: f64,
    pub inspector_collapsed: Vec<String>,
    /// Percent, clamped to 90–130 where it is read.
    pub ui_scale: f64,
    /// The system draws the windows' title bars, not Hangar (Linux only:
    /// shell/title_bar.rs). Written only when on, so a file the app wrote
    /// before it had the setting is written back the same.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub system_title_bar: bool,
    pub agent_pane_control: bool,
    /// `"main"` by default; `null` is kept as the person wrote it.
    pub claude_account: Option<String>,
    pub experiments: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kits_everywhere: Option<Vec<WorkspaceKit>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projects_folder: Option<String>,
    /// The machines the `nodes` experiment connects to (`nodes`). Written
    /// only when there is one, so a file from before stays as it was.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<NodeConfig>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A machine reached over SSH that runs `hangar-node` (docs/native/nodes.md
/// §4). Only how to reach it is stored: its state is the `nodes` package's
/// and lives as long as the app.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NodeConfig {
    pub id: String,
    /// What the page calls it; the SSH target when empty.
    pub label: String,
    /// What `ssh` is given: `user@host`, or a `Host` of the SSH config.
    pub ssh_target: String,
    /// An SSH config file passed as `-F`; the person's own when `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_config: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme_id: hangar_theme::DEFAULT_ID.into(),
            locale: None,
            shell_id: None,
            font_family: r#""Cascadia Code", "JetBrains Mono", Consolas, "Courier New", monospace"#
                .into(),
            font_size: 13.0,
            line_height: 1.3,
            letter_spacing: 0.0,
            cursor_style: CursorStyle::Bar,
            cursor_blink: true,
            scrollback: 10_000,
            padding: 10.0,
            agent_commands: AgentCommands::default(),
            auto_resume: true,
            hibernate_idle: true,
            hibernate_after_min: 10.0,
            claude_bridge: true,
            launch_delay_ms: 700,
            notify_on_idle: true,
            notify_idle_ms: 3000,
            notify_only_when_away: true,
            keybindings: Map::new(),
            saved_commands: Vec::new(),
            voice_enabled: false,
            voice_engine: VoiceEngine::Local,
            voice_model: "whisper-small-q5_1".into(),
            voice_cloud_model: "whisper-large-v3-turbo".into(),
            voice_language: String::new(),
            voice_api_key: None,
            voice_cleanup: false,
            voice_cleanup_model: String::new(),
            voice_cleanup_hint: String::new(),
            voice_submit: false,
            discord_presence: false,
            discord_show_workspace: true,
            discord_show_agents: true,
            discord_app_id: String::new(),
            board_dock_open: false,
            board_dock_width: 420.0,
            model_panel_width: 440.0,
            navigator_open: true,
            inspector_open: true,
            inspector_width: 296.0,
            inspector_collapsed: Vec::new(),
            ui_scale: 100.0,
            system_title_bar: false,
            agent_pane_control: true,
            claude_account: Some("main".into()),
            experiments: Vec::new(),
            kits_everywhere: None,
            projects_folder: None,
            nodes: Vec::new(),
            extra: Map::new(),
        }
    }
}

impl Settings {
    /// `uiScale` as the factor iced scales the window by, clamped as the TS
    /// clamped it.
    pub fn scale_factor(&self) -> f32 {
        let percent = if self.ui_scale.is_finite() {
            self.ui_scale.clamp(UI_SCALE_MIN, UI_SCALE_MAX)
        } else {
            100.0
        };
        (percent / 100.0) as f32
    }
}

/// A shell discovered on this machine.
pub type ShellInfo = hangar_core::shells::ShellInfo;
