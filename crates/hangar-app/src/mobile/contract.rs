//! The JSON of the `remote_*` answers (`mobile/remote/types.ts`): what the
//! window tells Hangar Mobile through core's mobile server, which relays it as
//! it is to the phone app. The phone stays a web app (`mobile/`), so these
//! shapes are its contract: camelCase fields, an optional field left out when
//! it has nothing to say, `null` where the TS wrote `null`.
//!
//! The TS file proposed to live in hangar-core (`remote/contract.rs`); it
//! stays here because nothing in core reads these answers but the two fields
//! of `remote_apps` and `remote_workspace` its routes need, which core reads
//! with deserializers of its own (`remote/apps.rs`). The chat's items are
//! core's own (`hangar_core::chat`), written by `chat_page`.

use serde::Serialize;
use serde_json::Value;

use hangar_core::chat::{ChatItem, Question};

use crate::orchestration::projects::{BoardCounts, ProjectTask};
use crate::state::AgentActivity;
use crate::state::types::PaneRole;

/// `RemoteActivity`: `None` when no agent the window knows of runs in the
/// pane, written `null`.
pub type RemoteActivity = Option<AgentActivity>;

/// The keys a phone may send (`REMOTE_KEYS`), in the order the refusal
/// lists them.
pub const REMOTE_KEYS: [&str; 6] = ["enter", "escape", "tab", "up", "down", "ctrl+c"];

/* ── remote_state ─────────────────────────────────────────────── */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WaitingKind {
    Pane,
    NeedsYou,
    ReadyToLand,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteWaiting {
    pub kind: WaitingKind,
    pub workspace_id: String,
    pub workspace: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Since when it waits, ms: from the journals, else when first seen.
    pub since: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RemotePane {
    pub id: String,
    pub name: String,
    pub agent: String,
    pub role: Option<PaneRole>,
    pub activity: RemoteActivity,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NamedActivity {
    pub name: String,
    pub activity: RemoteActivity,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteProject {
    pub id: String,
    pub name: String,
    pub open: bool,
    pub asleep: bool,
    pub git: Option<bool>,
    pub orchestrator: Option<NamedActivity>,
    pub panes: Vec<RemotePane>,
    pub board: BoardCounts,
    pub plan_status: Option<String>,
    pub needs_you: Vec<ProjectTask>,
    pub ready_to_land: Vec<ProjectTask>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteManager {
    pub pane_id: String,
    pub name: String,
    pub activity: RemoteActivity,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RemoteState {
    pub at: u64,
    pub manager: Option<RemoteManager>,
    pub waiting: Vec<RemoteWaiting>,
    /// The HQ is not a project.
    pub projects: Vec<RemoteProject>,
}

/* ── Screen and input ─────────────────────────────────────────── */

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteScreen {
    pub pane_id: String,
    pub pane: String,
    pub workspace: String,
    pub agent: String,
    pub activity: RemoteActivity,
    pub text: String,
    /// The permission the pane asks for, when the PC can tell which
    /// (`ChatPending::Permission`'s `prompt`), for the phone to send back
    /// with its answer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

/// `RemoteSent`: `queued` when held for a Claude Code session not up yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RemoteSent {
    pub sent: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queued: Option<bool>,
}

impl RemoteSent {
    pub const SENT: RemoteSent = RemoteSent {
        sent: true,
        queued: None,
    };
    pub const QUEUED: RemoteSent = RemoteSent {
        sent: true,
        queued: Some(true),
    };
}

/// `RemoteAnswered`: `queued` when the answers wait in the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RemoteAnswered {
    pub answered: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queued: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RemoteWoken {
    pub waking: bool,
}

/* ── History ──────────────────────────────────────────────────── */

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RemoteHistory {
    pub entries: Vec<hangar_core::remote::ActivityEntry>,
}

/* ── Chat ─────────────────────────────────────────────────────── */

/// What the pane waits on the person for (`ChatPending`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChatPending {
    #[serde(rename_all = "camelCase")]
    Question {
        tool_use_id: String,
        questions: Vec<Question>,
    },
    Permission {
        tool: String,
        title: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
        /// Which prompt this is (`chat_live::prompt_of`): the phone sends it
        /// back with its answer, which is refused unless the pane still asks
        /// this one (MOB-02).
        prompt: String,
    },
}

/// Why a chat is empty when it is not the conversation's doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatReason {
    NotClaude,
    NoSession,
    NoTranscript,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteChat {
    pub pane_id: String,
    pub pane: String,
    pub workspace: String,
    pub agent: String,
    pub activity: RemoteActivity,
    pub session_id: Option<String>,
    /// Oldest first.
    pub items: Vec<ChatItem>,
    pub older: bool,
    pub cursor: Option<String>,
    pub pending: Option<ChatPending>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<ChatReason>,
}

/* ── Deliveries ───────────────────────────────────────────────── */

/// What runs for a delivery on the PC, as delivery.rs names its stages.
pub const STAGES: [&str; 5] = ["queued", "merging", "checking", "reviewing", "landing"];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeliveryCheck {
    pub command: String,
    pub passed: bool,
    pub seconds: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeliveryBlocking {
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    pub problem: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeliveryReview {
    pub approved: bool,
    pub notes: String,
    pub blocking: Vec<DeliveryBlocking>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryFile {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    /// added, modified, deleted or renamed.
    pub status: &'static str,
    pub additions: u32,
    pub deletions: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binary: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeliveryEarlier {
    pub at: u64,
    pub outcome: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocking: Option<Vec<DeliveryBlocking>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDelivery {
    pub task_id: String,
    pub title: String,
    pub workspace: String,
    pub workspace_id: String,
    pub worker: String,
    pub branch: String,
    pub target: String,
    pub summary: Option<String>,
    pub check: Option<DeliveryCheck>,
    pub review: Option<DeliveryReview>,
    pub files: Vec<DeliveryFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub more_files: Option<bool>,
    pub waiting: bool,
    pub stage: Option<&'static str>,
    pub merge: Option<String>,
    pub passed_at: Option<u64>,
    pub moved: bool,
    pub in_the_way: Option<Vec<String>>,
    pub earlier: Vec<DeliveryEarlier>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RemoteDeliveryPatch {
    pub path: String,
    pub patch: String,
    pub binary: bool,
    pub truncated: bool,
}

/// `RemoteLanded`: landed on its target, or not yet, at this stage.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum RemoteLanded {
    Landed { landed: bool, target: String },
    Running { landed: bool, stage: &'static str },
}

/* ── Apps ─────────────────────────────────────────────────────── */

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteApp {
    pub workspace_id: String,
    pub workspace: String,
    pub port: u16,
    /// worker or custom.
    pub source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RemoteApps {
    pub apps: Vec<RemoteApp>,
}

/* ── remote_stats ─────────────────────────────────────────────── */

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsTotals {
    pub tokens: u64,
    pub cache_read: u64,
    pub cost: f64,
    pub active_ms: i64,
    pub sessions: usize,
    pub turns: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsRow {
    pub key: String,
    pub label: String,
    pub tokens: u64,
    pub cost: f64,
    pub active_ms: i64,
    pub sessions: usize,
    pub share: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unpriced: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsOther {
    pub count: usize,
    pub tokens: u64,
    pub cost: f64,
    pub active_ms: i64,
    pub share: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatsRanking {
    pub rows: Vec<StatsRow>,
    pub other: Option<StatsOther>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsWindow {
    pub kind: String,
    pub percent_used: u32,
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatsPlan {
    pub at: u64,
    pub windows: Vec<StatsWindow>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatsRange {
    pub from: i64,
    pub to: i64,
    pub days: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatsPoint {
    pub at: i64,
    /// A whole number of tokens, or dollars.
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteStats {
    pub period: &'static str,
    pub measure: &'static str,
    pub read_at: i64,
    pub cached_only: bool,
    pub range: StatsRange,
    /// day or week.
    pub granularity: &'static str,
    pub first_at: Option<i64>,
    pub totals: StatsTotals,
    pub previous: Option<StatsTotals>,
    pub active_days: usize,
    pub series: Vec<StatsPoint>,
    pub projects: StatsRanking,
    pub agents: StatsRanking,
    pub models: StatsRanking,
    pub unpriced_tokens: u64,
    pub plan: Option<StatsPlan>,
}

/* ── Upload ───────────────────────────────────────────────────── */

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RemoteWorkspace {
    pub id: String,
    pub name: String,
    pub cwd: String,
}

/// An answer as the bus carries it; one that cannot be put in JSON fails
/// with why, rather than reaching the phone as a `null` it would take for an
/// answer (MOB-21).
pub fn value<T: Serialize>(answer: &T) -> Result<Value, String> {
    serde_json::to_value(answer).map_err(|err| format!("The answer could not be written: {err}"))
}
