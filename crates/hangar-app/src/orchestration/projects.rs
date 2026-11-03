//! Where every project stands, for the manager (`lib/projects.ts`): the one
//! pane with role "manager", in the HQ workspace, which follows every other
//! workspace of Hangar, tells the person what waits for them and passes their
//! instructions on to each project's orchestrator.
//!
//! One summary per workspace, the HQ left out — its orchestrator and workers,
//! the panes waiting for the person, its board, its plan's status, what
//! happened last — and the texts a model reads them in. Everything is read
//! through `ProjectsPort`, so the tools (P18's `project_*`), the briefing and
//! the phone (P20's `remote_state`) all write the same overview. A part that
//! cannot be read (a board, the memory) is left empty, and every other
//! project is summarized all the same.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, LazyLock};

use regex::Regex;
use serde::Serialize;

use hangar_core::board::Task;
use hangar_core::memory::MemoryEntry;

use super::brief::{find_plan, plan_title};
use super::events::{JournalEntry, by_rank, describe_event};
use super::roles::{orchestrator, workers_of};
use super::text::{clip, clock, flat};
use crate::state::roles::{NEEDS_YOU_LABEL, READY_TO_LAND_LABEL};
use crate::state::types::{AgentId, AppState, Workspace, WsId};
use crate::state::{AgentActivity, PaneActivity};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectPane {
    pub name: String,
    /// `None`: no agent the app knows of runs in it.
    pub activity: Option<AgentActivity>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWorker {
    pub name: String,
    pub activity: Option<AgentActivity>,
    pub task_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectTask {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DoingTask {
    pub id: String,
    pub title: String,
    pub assignee: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WaitingPane {
    pub name: String,
    pub agent: AgentId,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct BoardCounts {
    pub todo: usize,
    pub doing: usize,
    pub review: usize,
    pub done: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LastEvent {
    pub at: u64,
    pub text: String,
}

/// One workspace as the manager sees it (`ProjectSummary`); serialised as the
/// TS wrote it, for `project_list` and the phone.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub id: WsId,
    pub name: String,
    pub cwd: String,
    /// Its panes are up in a window.
    pub open: bool,
    pub asleep: bool,
    /// Whether its folder is a git repository; `None` when that cannot be told.
    pub git: Option<bool>,
    pub orchestrator: Option<ProjectPane>,
    pub workers: Vec<ProjectWorker>,
    /// Agent panes held by a permission or a question, workers or not.
    pub waiting: Vec<WaitingPane>,
    pub board: BoardCounts,
    /// Why the board could not be read; its parts are all empty then.
    pub board_error: Option<String>,
    /// Open tasks only: a done task waits for nobody, whatever its labels.
    pub needs_you: Vec<ProjectTask>,
    pub ready_to_land: Vec<ProjectTask>,
    pub doing: Vec<DoingTask>,
    /// The board has tasks, and every one of them is done.
    pub all_done: bool,
    /// The "Status:" part of the orchestrator's plan; `None` without one.
    pub plan_status: Option<String>,
    /// The orchestrator's latest events, oldest first, as lines a model reads.
    pub last_events: Vec<LastEvent>,
}

/// What the summaries are read from (`ProjectsDeps`).
pub trait ProjectsPort: Sync {
    fn state(&self) -> Arc<AppState>;
    fn load_board(&self, cwd: &str) -> impl Future<Output = Result<Vec<Task>, String>> + Send;
    fn load_memory(&self) -> impl Future<Output = Result<Vec<MemoryEntry>, String>> + Send;
    /// Whether a folder is a git repository; `None` when that cannot be told.
    fn is_repo(&self, cwd: &str) -> impl Future<Output = Result<Option<bool>, String>> + Send;
    fn activity(&self, pane: &str) -> Option<PaneActivity>;
    fn asleep(&self, workspace_id: &str) -> bool;
    /// Whether the workspace's panes are up in a window.
    fn is_open(&self, workspace_id: &str) -> bool;
    fn journal(&self, workspace_id: &str) -> Vec<JournalEntry>;
}

/* ── The plan's status ─────────────────────────────────────────── */

const MAX_STATUS: usize = 300;

static STATUS_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*(?:[-*]\s+)?(?:#{1,6}\s+)?\*{0,2}status\*{0,2}\s*:\s*\*{0,2}\s*(.*)$").unwrap()
});
static STATUS_HEADING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^\s*#{1,6}\s+status\s*$").unwrap());
/// The other parts the orchestrator's role has the plan written in.
static PLAN_PART: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*(?:objective|validation criteria|decisions|assumptions|open questions|tasks)\b[^:\n]{0,40}:")
        .unwrap()
});

/// The status in a plan: what follows "Status:" on its line or, the line
/// holding nothing else, the lines under it up to a blank one or the next
/// part (`planStatusOf`).
pub fn plan_status_of(content: &str) -> Option<String> {
    let lines: Vec<&str> = content.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line)).collect();
    for (at, line) in lines.iter().enumerate() {
        let inline = STATUS_LINE.captures(line);
        if inline.is_none() && !STATUS_HEADING.is_match(line) {
            continue;
        }
        let first = inline
            .and_then(|caps| caps.get(1).map(|m| m.as_str().trim().to_string()))
            .unwrap_or_default();
        if !first.is_empty() {
            return Some(flat(&first, MAX_STATUS));
        }
        let mut under = Vec::new();
        for line in &lines[at + 1..] {
            if line.trim().is_empty() || line.trim_start().starts_with('#') || PLAN_PART.is_match(line) {
                break;
            }
            under.push(line.trim());
        }
        return if under.is_empty() { None } else { Some(flat(&under.join("\n"), MAX_STATUS)) };
    }
    None
}

/* ── Reading ───────────────────────────────────────────────────── */

const EVENTS_KEPT: usize = 5;

/// Two workspaces on one folder share its board and its repository.
fn folder_key(cwd: &str) -> String {
    cwd.trim_end_matches(['\\', '/']).replace('\\', "/").to_lowercase()
}

#[derive(Debug, Clone)]
struct BoardRead {
    tasks: Vec<Task>,
    error: Option<String>,
}

/// Every workspace but the HQ, summarized, in the order the store keeps them
/// (`loadProjects`). Each folder is read once, and every folder at once.
pub async fn load_projects(port: &impl ProjectsPort) -> Vec<ProjectSummary> {
    use iced::futures::future::join_all;

    let state = port.state();
    let spaces: Vec<&Workspace> = state.workspaces.iter().filter(|ws| ws.hq != Some(true)).collect();
    let mut folders: Vec<(String, &str)> = Vec::new();
    for ws in &spaces {
        let key = folder_key(&ws.cwd);
        if !folders.iter().any(|(known, _)| *known == key) {
            folders.push((key, ws.cwd.as_str()));
        }
    }
    let reads = folders.iter().map(|(_, cwd)| async move {
        let board = match port.load_board(cwd).await {
            Ok(tasks) => BoardRead { tasks, error: None },
            Err(error) => {
                let error = error.strip_prefix("Error: ").unwrap_or(&error).to_string();
                BoardRead {
                    tasks: Vec::new(),
                    error: Some(if error.is_empty() { "unknown error".into() } else { error }),
                }
            }
        };
        let git = port.is_repo(cwd).await.ok().flatten();
        (board, git)
    });
    // An unreadable memory is no plan anywhere, not a failed list.
    let (reads, memory) = iced::futures::join!(join_all(reads), port.load_memory());
    let memory = memory.unwrap_or_default();
    let by_folder: HashMap<&str, &(BoardRead, Option<bool>)> = folders
        .iter()
        .map(|(key, _)| key.as_str())
        .zip(reads.iter())
        .collect();
    spaces
        .iter()
        .map(|ws| {
            let (board, git) = by_folder[folder_key(&ws.cwd).as_str()];
            summarize(ws, &state.workspaces, board, *git, &memory, port)
        })
        .collect()
}

fn summarize(
    ws: &Workspace,
    workspaces: &[Workspace],
    board: &BoardRead,
    git: Option<bool>,
    memory: &[MemoryEntry],
    port: &impl ProjectsPort,
) -> ProjectSummary {
    let tasks = &board.tasks;
    let activity_of = |pane: &str| port.activity(pane).map(|a| a.activity);
    let count = |column: &str| tasks.iter().filter(|task| task.column == column).count();
    let labelled = |label: &str| {
        let mut found: Vec<&Task> = tasks
            .iter()
            .filter(|task| task.column != "done" && task.labels.iter().any(|l| l == label))
            .collect();
        found.sort_by(|a, b| by_rank(a, b));
        found
            .into_iter()
            .map(|task| ProjectTask { id: task.id.clone(), title: task.title.clone() })
            .collect::<Vec<_>>()
    };
    let titles = |id: &str| tasks.iter().find(|task| task.id == id).map(|task| task.title.clone());
    let plan = find_plan(memory, &plan_title(ws, workspaces), ws);
    let mut doing: Vec<&Task> = tasks.iter().filter(|task| task.column == "doing").collect();
    doing.sort_by(|a, b| by_rank(a, b));
    let journal = port.journal(&ws.id);
    let from = journal.len().saturating_sub(EVENTS_KEPT);
    ProjectSummary {
        id: ws.id.clone(),
        name: ws.name.clone(),
        cwd: ws.cwd.clone(),
        open: port.is_open(&ws.id),
        asleep: port.asleep(&ws.id),
        git,
        orchestrator: orchestrator(ws).map(|pane| ProjectPane { name: pane.name.clone(), activity: activity_of(&pane.id) }),
        workers: workers_of(ws)
            .into_iter()
            .map(|pane| ProjectWorker {
                name: pane.name.clone(),
                activity: activity_of(&pane.id),
                task_id: pane.task_id.clone(),
            })
            .collect(),
        waiting: ws
            .panes
            .iter()
            .filter(|pane| pane.agent != AgentId::Shell && activity_of(&pane.id) == Some(AgentActivity::Waiting))
            .map(|pane| WaitingPane { name: pane.name.clone(), agent: pane.agent })
            .collect(),
        board: BoardCounts { todo: count("todo"), doing: count("doing"), review: count("review"), done: count("done") },
        board_error: board.error.clone(),
        needs_you: labelled(NEEDS_YOU_LABEL),
        ready_to_land: labelled(READY_TO_LAND_LABEL),
        doing: doing
            .into_iter()
            .map(|task| DoingTask { id: task.id.clone(), title: task.title.clone(), assignee: task.assignee.clone() })
            .collect(),
        all_done: !tasks.is_empty() && tasks.iter().all(|task| task.column == "done"),
        plan_status: plan.and_then(|plan| plan_status_of(&plan.content)),
        last_events: journal[from..]
            .iter()
            .map(|entry| LastEvent { at: entry.at, text: describe_event(&entry.event, &titles) })
            .collect(),
    }
}

/* ── Texts ─────────────────────────────────────────────────────── */

/// About 300 characters a project, for twenty of them.
const MAX_OVERVIEW: usize = 6_000;
/// Kept free for the line that says which projects were left out.
const FOOTER_ROOM: usize = 400;
const MAX_DOING_IN_DETAIL: usize = 25;

/// How much of a project the overview says: the fullest of these forms that
/// every project fits in, or the last one, which counts tasks rather than
/// naming them.
struct Form {
    /// Task titles listed in a line, the others counted; 0: counted only.
    titles: usize,
    title: usize,
    status: Option<usize>,
    /// Said under the summary, so that the reader knows what is left out.
    note: Option<&'static str>,
}

const FORMS: [Form; 3] = [
    Form { titles: 3, title: 60, status: Some(160), note: None },
    Form {
        titles: 1,
        title: 40,
        status: None,
        note: Some("Short form, for room: one task title per list, no plan status."),
    },
    Form {
        titles: 0,
        title: 0,
        status: None,
        note: Some("Shortest form, for room: tasks counted rather than named, no plan status."),
    },
];

fn len(text: &str) -> usize {
    text.chars().count()
}

fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => format!("{}{}.", first.to_uppercase(), chars.as_str()),
        None => ".".into(),
    }
}

fn doing_words(activity: Option<AgentActivity>) -> &'static str {
    match activity {
        Some(AgentActivity::Working) => "working",
        Some(AgentActivity::Waiting) => "waiting for the person",
        Some(AgentActivity::Yours) => "ended its turn",
        Some(AgentActivity::Idle) => "idle",
        None => "not running",
    }
}

fn task_ref(id: &str, title: &str, max: usize) -> String {
    format!("\"{}\" [{id}]", flat(title, max))
}

fn plural(n: usize, one: &str) -> String {
    format!("{n} {}", if n == 1 { one.to_string() } else { format!("{one}s") })
}

/// Something only the person can move: an answer, a landing, a pane held.
fn for_the_person(project: &ProjectSummary) -> bool {
    !project.needs_you.is_empty() || !project.ready_to_land.is_empty() || !project.waiting.is_empty()
}

fn at_work(project: &ProjectSummary) -> bool {
    project.orchestrator.as_ref().and_then(|o| o.activity) == Some(AgentActivity::Working)
        || !project.workers.is_empty()
        || project.board.doing > 0
        || project.board.review > 0
}

fn rank(project: &ProjectSummary) -> u8 {
    if for_the_person(project) {
        0
    } else if at_work(project) {
        1
    } else {
        2
    }
}

fn head_line(project: &ProjectSummary) -> String {
    let mut parts = vec![if project.asleep {
        "asleep"
    } else if project.open {
        "open"
    } else {
        "not open"
    }];
    match project.git {
        Some(true) => parts.push("git repository"),
        Some(false) => parts.push("no git repository"),
        None => {}
    }
    format!("{} — {} · {}", project.name, project.cwd, parts.join(" · "))
}

fn board_state(project: &ProjectSummary) -> String {
    if let Some(error) = &project.board_error {
        return format!("could not be read ({})", flat(error, 120));
    }
    let BoardCounts { todo, doing, review, done } = project.board;
    let total = todo + doing + review + done;
    if total == 0 {
        return "empty".into();
    }
    if project.all_done {
        return format!("all {} done", plural(total, "task"));
    }
    format!("{todo} todo, {doing} doing, {review} review, {done} done")
}

fn orchestrator_state(project: &ProjectSummary) -> String {
    match &project.orchestrator {
        Some(o) => format!("orchestrator \"{}\" {}", o.name, doing_words(o.activity)),
        None => "no orchestrator".into(),
    }
}

/// Who works on it and how far its board is, in one line.
fn team_state(project: &ProjectSummary) -> String {
    let mut parts = vec![orchestrator_state(project)];
    if !project.workers.is_empty() {
        let mut tally: Vec<(&str, usize)> = Vec::new();
        for worker in &project.workers {
            let what = doing_words(worker.activity);
            match tally.iter_mut().find(|(known, _)| *known == what) {
                Some((_, n)) => *n += 1,
                None => tally.push((what, 1)),
            }
        }
        let counts: Vec<String> = tally.iter().map(|(what, n)| format!("{n} {what}")).collect();
        parts.push(format!("{}: {}", plural(project.workers.len(), "worker"), counts.join(", ")));
    }
    parts.push(format!("board: {}", board_state(project)));
    parts.join(" · ")
}

fn refs(tasks: &[ProjectTask], form: &Form) -> String {
    let listed: Vec<String> = tasks
        .iter()
        .take(form.titles)
        .map(|task| task_ref(&task.id, &task.title, form.title))
        .collect();
    let more = if tasks.len() > form.titles {
        format!(", and {} more", tasks.len() - form.titles)
    } else {
        String::new()
    };
    format!("{}{more}", listed.join(", "))
}

fn waiting_list(project: &ProjectSummary, shown: usize) -> String {
    let names: Vec<String> = project
        .waiting
        .iter()
        .take(shown)
        .map(|pane| format!("\"{}\" ({})", pane.name, pane.agent.as_str()))
        .collect();
    let more = if project.waiting.len() > shown {
        format!(", and {} more", project.waiting.len() - shown)
    } else {
        String::new()
    };
    format!("{}{more}", names.join(", "))
}

/// What waits for the person in a project, counted.
fn tally(project: &ProjectSummary) -> Option<String> {
    let counts: Vec<String> = [
        (!project.needs_you.is_empty()).then(|| format!("{} needing you", plural(project.needs_you.len(), "task"))),
        (!project.ready_to_land.is_empty())
            .then(|| format!("{} ready to land", plural(project.ready_to_land.len(), "task"))),
        (!project.waiting.is_empty()).then(|| format!("{} waiting", plural(project.waiting.len(), "pane"))),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!counts.is_empty()).then(|| format!("For the person: {}.", counts.join(", ")))
}

/// One project in a few lines; one with nothing to add, in one.
fn overview_block(project: &ProjectSummary, form: &Form) -> String {
    let mut more: Vec<String> = Vec::new();
    if form.titles == 0 {
        more.extend(tally(project));
    } else {
        if !project.needs_you.is_empty() {
            more.push(format!("Needs you: {}.", refs(&project.needs_you, form)));
        }
        if !project.ready_to_land.is_empty() {
            more.push(format!("Ready to land: {}.", refs(&project.ready_to_land, form)));
        }
        if !project.waiting.is_empty() {
            more.push(format!("Waiting for the person: {}.", waiting_list(project, form.titles + 2)));
        }
    }
    if let (Some(max), Some(status)) = (form.status, &project.plan_status) {
        more.push(format!("Plan status: {}", flat(status, max)));
    }
    if more.is_empty() {
        return format!("{} · {}.", head_line(project), team_state(project));
    }
    let mut lines = vec![head_line(project), sentence(&team_state(project))];
    lines.extend(more);
    lines.join("\n  ")
}

/// Every project in a few lines, those with something for the person first,
/// then those at work, then the quiet ones — the order in which they are
/// left out when even their shortest form does not fit in about 6,000
/// characters (`renderOverview`).
pub fn render_overview(projects: &[ProjectSummary], now: u64) -> String {
    let title = format!("# Hangar projects at {}", clock(now));
    if projects.is_empty() {
        return format!("{title}\nNo project: Hangar has no workspace besides the HQ.");
    }
    let mut ordered: Vec<&ProjectSummary> = projects.iter().collect();
    // Stable: the store's order within a rank.
    ordered.sort_by_key(|project| rank(project));
    let person: Vec<&ProjectSummary> = ordered.iter().copied().filter(|p| for_the_person(p)).collect();
    let working = ordered.iter().filter(|p| !for_the_person(p) && at_work(p)).count();
    let names = if person.is_empty() {
        String::new()
    } else {
        let names: Vec<&str> = person.iter().map(|p| p.name.as_str()).collect();
        format!(" ({})", clip(&names.join(", "), 300))
    };
    let summary = format!(
        "{}: {} with something for the person{names}, {working} at work, {} quiet.",
        plural(projects.len(), "project"),
        person.len(),
        projects.len() - person.len() - working
    );
    let mut head = String::new();
    let mut blocks: Vec<String> = Vec::new();
    for form in &FORMS {
        head = [Some(title.as_str()), Some(summary.as_str()), form.note]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n");
        blocks = ordered.iter().map(|project| overview_block(project, form)).collect();
        let total = blocks.iter().fold(len(&head), |total, block| total + 2 + len(block));
        if total <= MAX_OVERVIEW {
            break;
        }
    }
    let mut text = head;
    let mut text_len = len(&text);
    let mut shown = 0;
    for (index, block) in blocks.iter().enumerate() {
        let room = MAX_OVERVIEW - if index < blocks.len() - 1 { FOOTER_ROOM } else { 0 };
        if text_len + 2 + len(block) > room {
            break;
        }
        text.push_str("\n\n");
        text.push_str(block);
        text_len += 2 + len(block);
        shown += 1;
    }
    let left = &ordered[shown..];
    if !left.is_empty() {
        let held = left.iter().filter(|p| for_the_person(p)).count();
        let lead = format!(
            "…and {} not shown{}: ",
            plural(left.len(), "more project"),
            if held > 0 { format!(", {held} of them with something for the person") } else { String::new() }
        );
        let rest: Vec<&str> = left.iter().map(|p| p.name.as_str()).collect();
        let room = FOOTER_ROOM.saturating_sub(3 + len(&lead));
        text.push_str(&format!("\n\n{lead}{}.", clip(&rest.join(", "), room)));
    }
    text
}

/// One project in detail: its team, its open tasks, its plan's status, its
/// latest events (`renderProject`).
pub fn render_project(project: &ProjectSummary) -> String {
    let mut lines = vec![head_line(project), sentence(&orchestrator_state(project))];
    if project.workers.is_empty() {
        lines.push("Workers: none running.".into());
    } else {
        lines.push(format!("Workers ({}):", project.workers.len()));
        for worker in &project.workers {
            let on = match worker.task_id.as_deref().filter(|id| !id.is_empty()) {
                Some(id) => {
                    let known = project
                        .doing
                        .iter()
                        .map(|t| (t.id.as_str(), t.title.as_str()))
                        .chain(project.ready_to_land.iter().map(|t| (t.id.as_str(), t.title.as_str())))
                        .chain(project.needs_you.iter().map(|t| (t.id.as_str(), t.title.as_str())))
                        .find(|(known, _)| *known == id);
                    match known {
                        Some((id, title)) => format!(" on {}", task_ref(id, title, 120)),
                        None => format!(" on [{id}]"),
                    }
                }
                None => String::new(),
            };
            lines.push(format!("- \"{}\"{on}: {}", worker.name, doing_words(worker.activity)));
        }
    }
    lines.push(format!("Board: {}.", board_state(project)));
    if !project.doing.is_empty() {
        lines.push("Doing:".into());
        for task in project.doing.iter().take(MAX_DOING_IN_DETAIL) {
            let assignee = task
                .assignee
                .as_deref()
                .filter(|a| !a.is_empty())
                .map(|a| format!(" · {a}"))
                .unwrap_or_default();
            lines.push(format!("- {}{assignee}", task_ref(&task.id, &task.title, 120)));
        }
        if project.doing.len() > MAX_DOING_IN_DETAIL {
            lines.push(format!("…and {} more in doing.", project.doing.len() - MAX_DOING_IN_DETAIL));
        }
    }
    if !project.needs_you.is_empty() {
        lines.push("Needs you:".into());
        lines.extend(project.needs_you.iter().map(|t| format!("- {}", task_ref(&t.id, &t.title, 120))));
    }
    if !project.ready_to_land.is_empty() {
        lines.push("Ready to land:".into());
        lines.extend(project.ready_to_land.iter().map(|t| format!("- {}", task_ref(&t.id, &t.title, 120))));
    }
    lines.push(if project.waiting.is_empty() {
        "No pane waits for the person.".into()
    } else {
        format!("Waiting for the person: {}.", waiting_list(project, project.waiting.len()))
    });
    lines.push(match &project.plan_status {
        Some(status) => format!("Plan status: {status}"),
        None => "Plan status: none found.".into(),
    });
    if !project.last_events.is_empty() {
        lines.push("Latest events, oldest first:".into());
        lines.extend(project.last_events.iter().map(|e| format!("- {} {}", clock(e.at), e.text)));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests;
