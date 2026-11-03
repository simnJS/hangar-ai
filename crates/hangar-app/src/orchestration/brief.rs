//! What an orchestrator, its workers and the project manager are told
//! (`lib/orchestratorBrief.ts`): the role each one is given in its system
//! prompt (brief_text.rs), and the state it is handed again at session start,
//! on resume and after every compaction — the board, its plan, the latest
//! events.
//!
//! A pane's hangar-bridge mod asks for it through
//! `GET /api/bridge/{pane}/briefing`, which core relays to the app as the
//! internal control request `orchestrator_briefing` (hangar-core
//! orchestrator.rs): roles live in the app's model. Everything here is read by
//! a model, so it is English, and kept short — the context comes back with
//! every compaction.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use serde::Serialize;

use hangar_core::board::Task;
use hangar_core::memory::MemoryEntry;

use super::brief_text::{
    PLAN_TAG, TRACKER_TAG, TRACKER_TITLE, manager_role, orchestrator_role, release_message,
    role_message, worker_role,
};
use super::events::{JournalEntry, by_rank, describe_event};
use super::manager_feed::{ManagerJournalEntry, ManagerNews, describe_manager_event};
use super::projects::{ProjectSummary, render_overview};
use super::text::{ago, clip, clock, flat, same_name, task_ref};
use crate::state::PaneActivity;
use crate::state::roles::{orchestration_of, orchestrator_of, workers_of};
use crate::state::types::{AgentId, AppState, Pane, PaneRole, Workspace};

/// A pane's part in an orchestration, as its mod builds the session from it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Briefing {
    pub role: PaneRole,
    /// Appended to the session's system prompt.
    pub system: String,
    /// Handed to the session as it starts, resumes and compacts.
    pub context: String,
}

/* ── The plan document ─────────────────────────────────────────── */

fn same_path(a: &str, b: &str) -> bool {
    let norm = |p: &str| {
        p.trim_end_matches(['\\', '/'])
            .replace('\\', "/")
            .to_lowercase()
    };
    norm(a) == norm(b)
}

/// The title of a workspace's plan (`planTitle`). Memory is one store for
/// every project and a title is unique in it, so two workspaces of the same
/// name tell theirs apart by folder.
pub fn plan_title(ws: &Workspace, workspaces: &[Workspace]) -> String {
    let namesake = workspaces
        .iter()
        .any(|other| other.id != ws.id && same_name(&other.name, &ws.name));
    if namesake {
        format!("Plan — {} ({})", ws.name, ws.cwd)
    } else {
        format!("Plan — {}", ws.name)
    }
}

/// The plan by its title, or — renamed workspace — the one written from its
/// folder (`findPlan`).
pub fn find_plan<'a>(
    memory: &'a [MemoryEntry],
    title: &str,
    ws: &Workspace,
) -> Option<&'a MemoryEntry> {
    if let Some(exact) = memory.iter().find(|entry| entry.title == title) {
        return Some(exact);
    }
    let mut mine: Vec<&MemoryEntry> = memory
        .iter()
        .filter(|entry| {
            entry.tags.iter().any(|tag| tag == PLAN_TAG)
                && entry.title.starts_with("Plan — ")
                && same_path(&entry.workspace, &ws.cwd)
        })
        .collect();
    mine.sort_by_key(|entry| std::cmp::Reverse(entry.updated_at));
    mine.first().copied()
}

/// The manager's tracker: the entry of that title, tagged for the manager
/// (`findTracker`).
pub fn find_tracker(memory: &[MemoryEntry]) -> Option<&MemoryEntry> {
    let mut mine: Vec<&MemoryEntry> = memory
        .iter()
        .filter(|entry| {
            entry.title == TRACKER_TITLE && entry.tags.iter().any(|tag| tag == TRACKER_TAG)
        })
        .collect();
    mine.sort_by_key(|entry| std::cmp::Reverse(entry.updated_at));
    mine.first().copied()
}

/* ── Context ───────────────────────────────────────────────────── */

const MAX_PLAN: usize = 4_000;
const MAX_DESCRIPTION: usize = 3_000;
const MAX_COMMENT: usize = 240;
const MAX_OPEN_TASKS: usize = 25;
const MAX_DONE_TITLES: usize = 10;
const EVENTS_IN_BRIEFING: usize = 20;

fn activity_words(activity: Option<&PaneActivity>) -> &'static str {
    use crate::state::AgentActivity as A;
    match activity.map(|a| a.activity) {
        Some(A::Working) => "working",
        Some(A::Waiting) => "waiting for the person",
        Some(A::Yours) => "ended its turn",
        Some(A::Idle) => "idle",
        None => "not running",
    }
}

fn settings_line(ws: &Workspace, git: bool) -> String {
    let settings = orchestration_of(ws);
    if !git {
        let check = match &settings.check_command {
            Some(command) => format!("check `{command}`, run by the workers"),
            None => "no check command".into(),
        };
        return format!(
            "Settings: at most {} workers at once, {} attempts per task, {check}; workers run {}. Not a git repository: workers share the workspace's folder, and a task is done when its worker delivers. Nothing is merged or pushed.",
            settings.max_workers, settings.max_attempts, settings.worker_model
        );
    }
    let branch = match &settings.branch {
        Some(branch) => format!("branch {branch}"),
        None => "the workspace's current branch".into(),
    };
    let check = match &settings.check_command {
        Some(command) => format!("check `{command}`"),
        None => "no check command".into(),
    };
    let landing = if settings.auto_merge {
        format!("deliveries land on {branch} by themselves once check and review pass")
    } else {
        format!(
            "deliveries that pass check and review wait for the person to land them on {branch}"
        )
    };
    format!(
        "Settings: at most {} workers at once, {} attempts per task, {check}, {landing}; workers run {}. Nothing is ever pushed.",
        settings.max_workers, settings.max_attempts, settings.worker_model
    )
}

fn task_line(task: &Task, by_id: &HashMap<&str, &Task>, max_attempts: u32) -> String {
    let mut parts = vec![task_ref(Some(&task.title), &task.id)];
    if let Some(assignee) = task.assignee.as_deref().filter(|a| !a.is_empty()) {
        parts.push(assignee.to_string());
    }
    if task.attempts > 0 {
        parts.push(format!("attempt {}/{max_attempts}", task.attempts));
    }
    if task.priority > 1 {
        parts.push(format!("priority {}", task.priority));
    }
    if !task.labels.is_empty() {
        parts.push(format!("labels {}", task.labels.join(", ")));
    }
    parts.push(if task.owns.is_empty() {
        "owns nothing yet".into()
    } else {
        format!("owns {}", task.owns.join(", "))
    });
    if task.column == "todo" {
        let waiting: Vec<&String> = task
            .depends_on
            .iter()
            .filter(|id| by_id.get(id.as_str()).map(|t| t.column.as_str()) != Some("done"))
            .collect();
        parts.push(if waiting.is_empty() {
            "ready".into()
        } else {
            let refs: Vec<String> = waiting
                .iter()
                .map(|id| task_ref(by_id.get(id.as_str()).map(|t| t.title.as_str()), id))
                .collect();
            format!("after {}", refs.join(", "))
        });
    }
    format!("- {}", parts.join(" · "))
}

fn board_section(tasks: &[Task], max_attempts: u32) -> Vec<String> {
    if tasks.is_empty() {
        return vec!["Board: empty.".into()];
    }
    let by_id: HashMap<&str, &Task> = tasks.iter().map(|task| (task.id.as_str(), task)).collect();
    let count = |column: &str| tasks.iter().filter(|task| task.column == column).count();
    let ordered = |column: &str| {
        let mut open: Vec<&Task> = tasks.iter().filter(|task| task.column == column).collect();
        open.sort_by(|a, b| by_rank(a, b));
        open
    };
    let mut lines = vec![format!(
        "Board: {} todo, {} doing, {} review, {} done.",
        count("todo"),
        count("doing"),
        count("review"),
        count("done")
    )];
    let mut listed = 0;
    for (column, heading) in [("doing", "Doing"), ("review", "Review"), ("todo", "Todo")] {
        let open = ordered(column);
        if open.is_empty() {
            continue;
        }
        lines.push(format!("{heading}:"));
        for task in open {
            if listed >= MAX_OPEN_TASKS {
                break;
            }
            listed += 1;
            lines.push(task_line(task, &by_id, max_attempts));
            if let Some(last) = task.comments.last()
                && column != "todo"
            {
                lines.push(format!(
                    "  last comment, {}: \"{}\"",
                    last.author,
                    flat(&last.text, MAX_COMMENT)
                ));
            }
        }
    }
    let open = tasks.len() - count("done");
    if open > listed {
        lines.push(format!(
            "…and {} more open tasks: board_list_tasks.",
            open - listed
        ));
    }
    let mut done = ordered("done");
    done.sort_by_key(|task| std::cmp::Reverse(task.updated_at));
    if !done.is_empty() {
        let titles: Vec<String> = done
            .iter()
            .take(MAX_DONE_TITLES)
            .map(|task| format!("\"{}\"", flat(&task.title, 80)))
            .collect();
        let more = if done.len() > MAX_DONE_TITLES {
            format!(", and {} more", done.len() - MAX_DONE_TITLES)
        } else {
            String::new()
        };
        lines.push(format!("Done: {}{more}.", titles.join(", ")));
    }
    lines
}

/// A memory entry's text, cut where the briefing would grow too long.
fn document_body(entry: &MemoryEntry) -> String {
    let body = if entry.content.chars().count() > MAX_PLAN {
        let kept: String = entry.content.chars().take(MAX_PLAN).collect();
        format!(
            "{kept}\n…(cut here: memory_read {} for the rest, and shorten it)",
            entry.id
        )
    } else {
        entry.content.clone()
    };
    body.trim().to_string()
}

fn since(now: u64, then: u64) -> String {
    ago(now as i64 - then as i64)
}

fn plan_section(plan: Option<&MemoryEntry>, title: &str, now: u64) -> Vec<String> {
    match plan {
        None => vec![format!(
            "Plan: none yet. Write it with memory_write, title \"{title}\", tag \"{PLAN_TAG}\", before creating tasks."
        )],
        Some(plan) => vec![
            format!(
                "Plan (memory \"{}\" [{}], updated {}):",
                plan.title,
                plan.id,
                since(now, plan.updated_at)
            ),
            document_body(plan),
        ],
    }
}

fn tracker_section(tracker: Option<&MemoryEntry>, now: u64) -> Vec<String> {
    match tracker {
        None => vec![format!(
            "Tracker: none yet. Write it with memory_write, title \"{TRACKER_TITLE}\", tag \"{TRACKER_TAG}\": per project, what the person wants and where it stands."
        )],
        Some(tracker) => vec![
            format!(
                "Tracker (memory \"{}\" [{}], updated {}):",
                tracker.title,
                tracker.id,
                since(now, tracker.updated_at)
            ),
            document_body(tracker),
        ],
    }
}

fn events_section(journal: &[&JournalEntry], tasks: &[Task], max: usize) -> Vec<String> {
    if journal.is_empty() {
        return Vec::new();
    }
    let titles = |id: &str| {
        tasks
            .iter()
            .find(|task| task.id == id)
            .map(|task| task.title.clone())
    };
    let mut lines = vec!["Latest events, oldest first:".to_string()];
    let from = journal.len().saturating_sub(max);
    lines.extend(journal[from..].iter().map(|entry| {
        format!(
            "- {} {}",
            clock(entry.at),
            describe_event(&entry.event, &titles)
        )
    }));
    lines
}

/// What lately waited for the person, as the manager was told.
fn manager_events_section(journal: &[ManagerJournalEntry], max: usize) -> Vec<String> {
    if journal.is_empty() {
        return Vec::new();
    }
    let mut lines = vec!["Latest things that waited for the person, oldest first:".to_string()];
    let from = journal.len().saturating_sub(max);
    lines.extend(journal[from..].iter().map(|entry| {
        format!(
            "- {} {}",
            clock(entry.at),
            describe_manager_event(&ManagerNews::Event(entry.event.clone()))
        )
    }));
    lines.push(String::new());
    lines
}

/// What a briefing is written from (`BriefingInputs`).
pub struct BriefingInputs<'a> {
    pub state: &'a AppState,
    pub pane_id: &'a str,
    pub tasks: &'a [Task],
    pub memory: &'a [MemoryEntry],
    pub journal: &'a [JournalEntry],
    pub activity: &'a dyn Fn(&str) -> Option<PaneActivity>,
    /// Milliseconds since the epoch.
    pub now: u64,
    /// Whether the workspace is a git repository; the orchestrator's role
    /// depends on it.
    pub git: bool,
    /// Every project but the HQ, for the manager's briefing.
    pub projects: &'a [ProjectSummary],
    /// What lately waited for the person, for the manager's briefing.
    pub manager_events: &'a [ManagerJournalEntry],
}

fn pane_of<'a>(state: &'a AppState, pane_id: &str) -> Option<(&'a Workspace, &'a Pane)> {
    state.workspaces.iter().find_map(|ws| {
        ws.panes
            .iter()
            .find(|pane| pane.id == pane_id)
            .map(|pane| (ws, pane))
    })
}

/// The pane's briefing, or `None` for a pane with no part in an
/// orchestration (`buildBriefing`).
pub fn build_briefing(inputs: &BriefingInputs<'_>) -> Option<Briefing> {
    let (ws, pane) = pane_of(inputs.state, inputs.pane_id)?;
    let role = pane.role?;
    if pane.agent != AgentId::Claude {
        return None;
    }
    Some(match role {
        PaneRole::Manager => manager_briefing(inputs),
        PaneRole::Orchestrator => orchestrator_briefing(ws, inputs),
        PaneRole::Worker => worker_briefing(ws, pane, inputs),
    })
}

fn manager_briefing(inputs: &BriefingInputs<'_>) -> Briefing {
    let mut context = vec![render_overview(inputs.projects, inputs.now), String::new()];
    context.extend(manager_events_section(
        inputs.manager_events,
        EVENTS_IN_BRIEFING,
    ));
    context.extend(tracker_section(find_tracker(inputs.memory), inputs.now));
    Briefing {
        role: PaneRole::Manager,
        system: manager_role(),
        context: context.join("\n"),
    }
}

fn orchestrator_briefing(ws: &Workspace, inputs: &BriefingInputs<'_>) -> Briefing {
    let title = plan_title(ws, &inputs.state.workspaces);
    let settings = orchestration_of(ws);
    let workers = workers_of(ws);
    let mut lines = vec![
        format!(
            "# Hangar state at {}, workspace \"{}\" ({})",
            clock(inputs.now),
            ws.name,
            ws.cwd
        ),
        "This is current: trust it over what you remember.".to_string(),
        String::new(),
        settings_line(ws, inputs.git),
        if workers.is_empty() {
            format!("Workers: none running (up to {}).", settings.max_workers)
        } else {
            format!("Workers ({} of {}):", workers.len(), settings.max_workers)
        },
    ];
    for worker in &workers {
        let on = match worker.task_id.as_deref().filter(|id| !id.is_empty()) {
            Some(id) => {
                let title = inputs
                    .tasks
                    .iter()
                    .find(|task| task.id == id)
                    .map(|task| task.title.as_str());
                format!(" on {}", task_ref(title, id))
            }
            None => String::new(),
        };
        let doing = activity_words((inputs.activity)(&worker.id).as_ref());
        lines.push(format!("- {}{on}: {doing}", worker.name));
    }
    lines.push(String::new());
    lines.extend(board_section(inputs.tasks, settings.max_attempts));
    lines.push(String::new());
    lines.extend(plan_section(
        find_plan(inputs.memory, &title, ws),
        &title,
        inputs.now,
    ));
    let journal: Vec<&JournalEntry> = inputs.journal.iter().collect();
    let events = events_section(&journal, inputs.tasks, EVENTS_IN_BRIEFING);
    if !events.is_empty() {
        lines.push(String::new());
        lines.extend(events);
    }
    Briefing {
        role: PaneRole::Orchestrator,
        system: orchestrator_role(&ws.name, &title, inputs.git),
        context: lines.join("\n"),
    }
}

fn worker_briefing(ws: &Workspace, pane: &Pane, inputs: &BriefingInputs<'_>) -> Briefing {
    let settings = orchestration_of(ws);
    let orchestrator = orchestrator_of(ws);
    let task_id = pane.task_id.as_deref().filter(|id| !id.is_empty());
    let task = task_id.and_then(|id| inputs.tasks.iter().find(|task| task.id == id));
    let reference = match task_id {
        Some(id) => task_ref(task.map(|t| t.title.as_str()), id),
        None => "(no task given)".into(),
    };
    let mut lines = vec![format!(
        "# Hangar state at {}, worker \"{}\", workspace \"{}\"",
        clock(inputs.now),
        pane.name,
        ws.name
    )];
    match task {
        Some(task) => {
            let attempt = if task.attempts > 0 {
                format!(", attempt {} of {}", task.attempts, settings.max_attempts)
            } else {
                String::new()
            };
            lines.push(format!("Your task {reference}: in {}{attempt}.", task.column));
            lines.push(format!(
                "Owns: {}.",
                if task.owns.is_empty() {
                    "nothing listed: ask the orchestrator in a comment before changing files".to_string()
                } else {
                    task.owns.join(", ")
                }
            ));
        }
        None => lines.push(format!(
            "Your task {reference} is not on the board any more: comment nothing, end your turn and wait."
        )),
    }
    let facts: Vec<String> = [
        orchestrator.map(|o| format!("Orchestrator: pane \"{}\".", o.name)),
        settings
            .check_command
            .as_ref()
            .map(|command| format!("Check: `{command}`.")),
        pane.port_base
            .filter(|base| *base != 0)
            .map(|base| format!("Ports: {base} to {} (HANGAR_PORT_BASE).", base + 19)),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !facts.is_empty() {
        lines.push(facts.join(" "));
    }
    if let Some(task) = task
        && !task.description.trim().is_empty()
    {
        lines.extend([
            String::new(),
            "Description:".into(),
            clip(&task.description, MAX_DESCRIPTION),
        ]);
    }
    let comments = task.map(|task| {
        let from = task.comments.len().saturating_sub(4);
        &task.comments[from..]
    });
    if let Some(comments) = comments.filter(|c| !c.is_empty()) {
        lines.extend([String::new(), "Latest comments on it:".into()]);
        lines.extend(comments.iter().map(|c| {
            format!(
                "- {} {}: \"{}\"",
                clock(c.created_at),
                c.author,
                flat(&c.text, MAX_COMMENT * 2)
            )
        }));
    }
    let mine: Vec<&JournalEntry> = inputs
        .journal
        .iter()
        .filter(|entry| {
            matches!(&entry.event, super::events::OrchestratorEvent::Delivery { task_id: id, .. }
                if Some(id.as_str()) == pane.task_id.as_deref())
        })
        .collect();
    let events = events_section(&mine, inputs.tasks, 5);
    if !events.is_empty() {
        lines.push(String::new());
        lines.extend(events);
    }
    Briefing {
        role: PaneRole::Worker,
        system: worker_role(
            &ws.name,
            &reference,
            orchestrator.map(|o| o.name.as_str()),
            pane.shared_folder == Some(true),
        ),
        context: lines.join("\n"),
    }
}

/* ── Asking for one ────────────────────────────────────────────── */

/// What a briefing is read from (`BriefingDeps`).
pub trait BriefingPort: Sync {
    fn state(&self) -> Arc<AppState>;
    fn load_board(&self, cwd: &str) -> impl Future<Output = Result<Vec<Task>, String>> + Send;
    fn load_memory(&self) -> impl Future<Output = Result<Vec<MemoryEntry>, String>> + Send;
    fn journal(&self, workspace_id: &str) -> Vec<JournalEntry>;
    fn activity(&self, pane: &str) -> Option<PaneActivity>;
    fn now(&self) -> u64;
    /// Whether a folder is a git repository; `None` when that cannot be told.
    fn is_repo(&self, cwd: &str) -> impl Future<Output = Result<Option<bool>, String>> + Send;
    /// Every project's summary, read for a manager pane only.
    fn load_projects(&self) -> impl Future<Output = Result<Vec<ProjectSummary>, String>> + Send;
    /// What lately waited for the person, for a manager pane only.
    fn manager_journal(&self) -> Vec<ManagerJournalEntry>;
    /// `bridge_enqueue`, for `announce_role`.
    fn enqueue(&self, pane: &str, text: &str, from: &str) -> Result<(), String>;
}

/// Reads what a briefing needs, then writes it (`briefingFor`).
pub async fn briefing_for(
    pane_id: &str,
    port: &impl BriefingPort,
) -> Result<Option<Briefing>, String> {
    let state = port.state();
    let Some((ws, pane)) = pane_of(&state, pane_id) else {
        return Ok(None);
    };
    let Some(role) = pane.role else {
        return Ok(None);
    };
    let orchestrating = role == PaneRole::Orchestrator;
    let manager = role == PaneRole::Manager;
    // The manager follows every project's board, not the HQ's own.
    let tasks = if manager {
        Vec::new()
    } else {
        port.load_board(&ws.cwd).await?
    };
    // A worker's briefing has no plan in it; an unreadable memory is no plan.
    let memory = if orchestrating || manager {
        port.load_memory().await.unwrap_or_default()
    } else {
        Vec::new()
    };
    // Only the orchestrator's role depends on it; a worker's pane says it. A
    // folder that cannot be told is taken for a repository, as it always was.
    let git = if orchestrating {
        port.is_repo(&ws.cwd).await.ok().flatten()
    } else {
        None
    };
    // An unreadable overview is an empty one, not a failed briefing.
    let projects = if manager {
        port.load_projects().await.unwrap_or_default()
    } else {
        Vec::new()
    };
    let manager_events = if manager {
        port.manager_journal()
    } else {
        Vec::new()
    };
    let journal = port.journal(&ws.id);
    let activity = |pane: &str| port.activity(pane);
    Ok(build_briefing(&BriefingInputs {
        state: &state,
        pane_id,
        tasks: &tasks,
        memory: &memory,
        journal: &journal,
        activity: &activity,
        now: port.now(),
        git: git.unwrap_or(true),
        projects: &projects,
        manager_events: &manager_events,
    }))
}

/// The answer to the internal request `orchestrator_briefing {paneId}`:
/// `{role, system, context}`, or `null` for a pane with no role
/// (`answerInternal`). Core checks and clips it (orchestrator.rs).
pub async fn answer_briefing(
    args: &serde_json::Value,
    port: &impl BriefingPort,
) -> Result<serde_json::Value, String> {
    let pane = match args.get("paneId") {
        Some(serde_json::Value::String(pane)) => pane.clone(),
        Some(serde_json::Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    };
    let briefing = briefing_for(&pane, port).await?;
    serde_json::to_value(briefing).map_err(|err| err.to_string())
}

/// Tells a running session the role its pane was just given — or, with
/// `previous` and no role left on the pane, that it lost it (`announceRole`).
/// Called right after pinning or unpinning an existing pane; a pane created
/// with its role needs nothing, its session starts with the role in its
/// system prompt.
///
/// Answers whether a message was queued. Nothing is queued for a pane whose
/// Claude Code is not running with Hangar's mod: the next session it starts
/// reads its role from the briefing.
pub async fn announce_role(
    workspace_id: &str,
    pane_id: &str,
    previous: Option<PaneRole>,
    port: &impl BriefingPort,
) -> Result<bool, String> {
    let state = port.state();
    let Some(ws) = state.workspaces.iter().find(|ws| ws.id == workspace_id) else {
        return Ok(false);
    };
    let Some(pane) = ws.panes.iter().find(|pane| pane.id == pane_id) else {
        return Ok(false);
    };
    if pane.agent != AgentId::Claude || !port.activity(&pane.id).is_some_and(|a| a.bridged) {
        return Ok(false);
    }
    let text = match (pane.role, previous) {
        (Some(_), _) => briefing_for(&pane.id, port).await?.map(|briefing| {
            role_message(briefing.role, &briefing.system, &briefing.context, &ws.name)
        }),
        (None, Some(previous)) => Some(release_message(previous, &ws.name)),
        (None, None) => None,
    };
    let Some(text) = text else {
        return Ok(false);
    };
    port.enqueue(&pane.id, &text, "hangar")?;
    Ok(true)
}

#[cfg(test)]
mod tests;
