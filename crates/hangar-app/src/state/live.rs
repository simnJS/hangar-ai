//! What the window knows and never saves: `Model.ui` (App.tsx's `useState`,
//! inventory §1.5), `Model.live` (one read-model per package, plus the
//! shell's own), and the value types several packages share.
//!
//! The shared value types (`AgentActivity`, `PaneActivity`, `PlanLimits`,
//! `PaneInsight`, `BridgeEvent`) are declared here from the TS so that every
//! package names the same ones; P4 (`panes_logic`) fills them in and owns
//! their meaning. A field P4 needs to add goes here in P0b, or as a type of
//! P4's own that these wrap.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use iced::window;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::types::{PaneId, ShellInfo, WsId};
use crate::shell::{Drawer, Focus, Notice, Overlay, View, WindowKind};

/// `Model.ui`: the window's transient state.
#[derive(Debug, Default)]
pub struct Ui {
    pub view: View,
    /// A stack: the top one has the keyboard, Escape closes it.
    pub overlays: Vec<Overlay>,
    pub drawer: Option<Drawer>,
    pub focus_by_workspace: HashMap<WsId, PaneId>,
    pub zoom_by_workspace: HashMap<WsId, bool>,
    /// Workspaces visited once, whose panes stay alive while hidden.
    pub opened_ids: Vec<WsId>,
    /// `pane.resetAll` and `workspace.stopWorkers` arm a confirmation first.
    pub confirm_reset: bool,
    pub confirm_stop_workers: bool,
    pub needs_you_open: bool,
    /// The status bar's broadcast input.
    pub broadcast: String,
    /// The one-line notice at the bottom, with the id its expiry names.
    pub notice: Option<(u64, Notice)>,
    /// The chords of a shortcut sequence typed so far (the chord hint).
    /// TODO(P7): `keymap::Chord` once it exists; strings until then.
    pub pending_chords: Vec<String>,
    /// From `detect_agents`, once at start.
    pub available_agents: Vec<String>,
    /// From `detect_shells`, once at start.
    pub shells: Vec<ShellInfo>,
    /// The measured width of each window's main body, for `side_panels`.
    pub body_width: HashMap<window::Id, f32>,
    pub focus: Option<Focus>,
    /// The window that last had the system's focus: where a command runs.
    pub focused_window: Option<window::Id>,
}

/// `Model.live`: every package's shared read-model, and the shell's own.
#[derive(Debug, Default)]
pub struct Live {
    /// `load_state` has answered and the state is in. Nothing is saved, and
    /// no engine or control request runs, before.
    pub hydrated: bool,
    pub windows: HashMap<window::Id, WindowKind>,
    /// Bumped whenever a read-model changes, so views can cache on it.
    pub version: u64,

    pub panes_logic: crate::panes_logic::Live,
    pub panes: crate::panes::Live,
    pub navigator: crate::navigator::Live,
    pub toolbar: crate::toolbar::Live,
    pub status_bar: crate::status_bar::Live,
    pub needs_you: crate::needs_you::Live,
    pub update_banner: crate::update_banner::Live,
    pub voice: crate::voice::Live,
    pub keymap: crate::keymap::Live,
    pub palette: crate::palette::Live,
    pub saved_commands: crate::saved_commands::Live,
    pub session_picker: crate::session_picker::Live,
    pub pane_menu: crate::pane_menu::Live,
    pub workspace_dialog: crate::workspace_dialog::Live,
    pub git_flow: crate::git_flow::Live,
    pub consent_card: crate::consent_card::Live,
    pub accounts: crate::accounts::Live,
    pub inspector: crate::inspector::Live,
    pub git_ui: crate::git_ui::Live,
    pub memory: crate::memory::Live,
    pub mcp: crate::mcp::Live,
    pub orchestration_ui: crate::orchestration_ui::Live,
    pub stats: crate::stats::Live,
    pub board: crate::board::Live,
    pub settings: crate::settings::Live,
    pub kits: crate::kits::Live,
    pub preview: crate::preview::Live,
    pub kit_tabs: crate::kit_tabs::Live,
    pub model3d_view: crate::model3d_view::Live,
    pub control: crate::control::Live,
    pub orchestration: crate::orchestration::Live,
    pub mobile: crate::mobile::Live,
}

/// What an agent is doing (`AgentActivity`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentActivity {
    /// Busy on its own.
    Working,
    /// Blocked until the person answers.
    Waiting,
    /// Done; the turn is the person's.
    Yours,
    #[default]
    Idle,
}

impl From<AgentActivity> for hangar_theme::AgentState {
    fn from(activity: AgentActivity) -> Self {
        match activity {
            AgentActivity::Working => hangar_theme::AgentState::Working,
            AgentActivity::Waiting => hangar_theme::AgentState::Waiting,
            AgentActivity::Yours => hangar_theme::AgentState::Yours,
            AgentActivity::Idle => hangar_theme::AgentState::Idle,
        }
    }
}

/// One pane's activity (`lib/agentState.ts`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneActivity {
    pub workspace_id: WsId,
    pub activity: AgentActivity,
    /// The state comes from the hangar-bridge mod rather than from a guess.
    pub bridged: bool,
}

/// A glyph's state: the shell's own life until it runs, then the agent's
/// activity (`GlyphState` of StateGlyph.tsx, `paneGlyphState`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum GlyphState {
    Starting,
    Exited,
    Working,
    Waiting,
    Yours,
    #[default]
    Idle,
}

/// A rate-limit window of the plan, as the mod reports it (`RateLimit`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimit {
    /// `five_hour`, `seven_day`, or a gateway's own window.
    pub kind: String,
    pub percent_used: f64,
    pub resets_at: Option<String>,
}

/// The plan's limits, from whichever session reported last, and when.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanLimits {
    pub limits: Vec<RateLimit>,
    pub at: Instant,
}

/// A prompt held until the pane's Claude Code is free (`QueuedPrompt`).
pub type QueuedPrompt = hangar_core::bridge::Queued;

/// The context gauge of a pane header (`ContextGauge`).
#[derive(Debug, Clone, PartialEq)]
pub struct ContextGauge {
    /// 0–100, toward the auto-compaction boundary.
    pub pct: f64,
    pub used_tokens: u64,
    pub window: u64,
    pub model: Option<String>,
}

/// One line of a pane's history in the inspector (`PaneEvent`).
/// TODO(P4): the typed `PaneEventDetail` of paneInsight.ts.
#[derive(Debug, Clone, PartialEq)]
pub struct PaneEvent {
    pub at: Instant,
    pub kind: String,
    pub params: Value,
}

/// What the inspector and the pane header know of a pane
/// (`lib/paneInsight.ts`); `MAX_EVENTS` 40, `MAX_EDITED_FILES` 50.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PaneInsight {
    pub model: Option<String>,
    pub agent_version: Option<String>,
    pub session_since: Option<Instant>,
    pub context: Option<ContextGauge>,
    pub cost_usd: Option<f64>,
    pub queue: Vec<QueuedPrompt>,
    /// Most recent first, each once.
    pub edited_files: Vec<String>,
    /// Most recent first.
    pub events: Vec<PaneEvent>,
    pub last_answer: Option<String>,
    pub turn_started_at: Option<Instant>,
    /// The MCP servers its Claude Code started with; `None` for other agents.
    /// TODO(P4): the typed `PaneMcp`.
    pub mcp: Option<Value>,
}

/// What a pane's hangar-bridge mod reports (`BridgeEvent` of
/// `lib/bridge.ts`). Core hands it over as raw JSON
/// (`CoreEvent::Bridge`); `BridgeEvent::parse` reads it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum BridgeEvent {
    #[serde(rename_all = "camelCase")]
    Hello {
        session_id: String,
        model: String,
        version: String,
    },
    #[serde(rename_all = "camelCase")]
    Session {
        session_id: String,
        source: String,
    },
    Turn(TurnPhase),
    Permission {
        waiting: bool,
        #[serde(default)]
        tool: Option<String>,
        #[serde(default)]
        input: Option<Value>,
    },
    Tool(ToolPhase),
    #[serde(rename_all = "camelCase")]
    Usage {
        context: UsageContext,
        rate_limits: Vec<RateLimit>,
        cost_usd: Option<f64>,
    },
    #[serde(rename_all = "camelCase")]
    Edit {
        file: String,
        #[serde(default)]
        overlap: Option<String>,
        #[serde(default)]
        overlap_pane: Option<String>,
    },
    Dropped {
        text: String,
        reason: String,
    },
    End {
        reason: String,
    },
}

impl BridgeEvent {
    /// The report as the mod sent it, or `None` for a kind this version does
    /// not know (a newer mod), which the TS ignored as well.
    pub fn parse(raw: &Value) -> Option<BridgeEvent> {
        serde_json::from_value(raw.clone()).ok()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "lowercase")]
pub enum TurnPhase {
    Start,
    #[serde(rename_all = "camelCase")]
    End {
        /// `answer`, `aborted`, `refusal` or `error`.
        reason: String,
        duration_ms: u64,
        /// First line of the final answer, possibly empty.
        answer: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "lowercase")]
pub enum ToolPhase {
    #[serde(rename_all = "camelCase")]
    Start {
        tool_use_id: String,
        tool: String,
        input: Value,
    },
    #[serde(rename_all = "camelCase")]
    End { tool_use_id: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageContext {
    pub tokens: Option<u64>,
    pub window: u64,
    pub percent: Option<f64>,
}

/// Deliveries running, per workspace, by task id (`lib/deliveryRuns.ts`):
/// what the snapshot carries for the engines.
pub type Delivering = HashMap<WsId, HashSet<String>>;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn bridge_reports_read_with_their_nested_phase() {
        let end = json!({"kind": "turn", "phase": "end", "reason": "answer",
                         "durationMs": 1200, "answer": "Done."});
        assert_eq!(
            BridgeEvent::parse(&end),
            Some(BridgeEvent::Turn(TurnPhase::End {
                reason: "answer".into(),
                duration_ms: 1200,
                answer: "Done.".into(),
            }))
        );
        let start = json!({"kind": "turn", "phase": "start"});
        assert_eq!(
            BridgeEvent::parse(&start),
            Some(BridgeEvent::Turn(TurnPhase::Start))
        );
        assert_eq!(BridgeEvent::parse(&json!({"kind": "future"})), None);
    }
}
