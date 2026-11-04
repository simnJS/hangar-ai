//! The delivery_* control operations (deliveryControl.ts): a worker handing
//! in its branch, which lands on its workspace's branch once checked and
//! reviewed (core's delivery.rs), and the deliveries running
//! (deliveryRuns.ts).
//!
//! Core does the git, the check and the review, one delivery at a time per
//! repository, and says what happened. What follows is decided here, where
//! the board, the panes and the orchestrator are: a landed task is done and
//! its worker goes; a failed one goes back to its worker, as a message from
//! Hangar, with what to fix, however many times it fails. Hangar never hands
//! it to the person for that: whether to keep trying, reassign it or ask the
//! person is the orchestrator's call, and it hears of every failure.
//!
//! One that passed but waits for the person stands for the branch as it was
//! tested: new commits on it replace it with a delivery of their own, through
//! the merge, the check and the review again, and it never lands past them.
//!
//! A worker in a workspace that is not a git repository has no branch: what
//! it hands in is done as it is (`done_in_folder`).

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::{Value, json};

use hangar_core::board::{Task, TaskPatch};
use hangar_core::delivery::{Cleanup, Delivery, Kind, Outcome, Plan, TaskBrief, Tested};
use hangar_core::shells::ShellInfo;

use crate::accounts::{MAIN_ACCOUNT, launch_account_with};
use crate::state::roles::{NEEDS_YOU_LABEL, READY_TO_LAND_LABEL, orchestration_of, workers_of};
use crate::state::types::PaneRole;
use crate::state::{Pane, Settings, Workspace, WsId};

use super::delivery_text::{
    AS_TESTED, FAILED, HANGAR, OUTSIDE, REVIEW_STOPPED, SUMMARY, USAGE_LIMIT, check_line,
    dirty_head, failed_comment, ready_head, review_line, short,
};
use super::paths::resolve_path;
use super::port::{DeliveryDeps, ModelCmd, Port, workspace_in};
use super::run::unknown;
use super::types::{
    Caller, DeliveryOutcomeKind, DeliveryState, OrchestratorEvent, outcome_name, outcome_of, text,
};

/// A worker done without git keeps its pane this long: its answer has to
/// reach it first.
const CLOSE_AFTER_MS: u64 = 5_000;

/* ── State ─────────────────────────────────────────────────────── */

/// A delivery under way (`InFlight`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InFlight {
    pub workspace_id: WsId,
    pub worker: String,
    pub since: u64,
    /// The person landing a delivery that waited, rather than a worker
    /// submitting.
    pub landing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Settled {
    outcome: DeliveryOutcomeKind,
    detail: String,
    at: u64,
}

/// deliveryControl's module state, held by the port: one delivery at a time
/// per task, and the last outcome of each while the app runs.
#[derive(Debug, Default)]
pub struct Deliveries {
    book: Mutex<Book>,
}

#[derive(Debug, Default)]
struct Book {
    in_flight: HashMap<String, InFlight>,
    settled: HashMap<String, Settled>,
}

impl Deliveries {
    fn book(&self) -> std::sync::MutexGuard<'_, Book> {
        self.book
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// Takes the task for one delivery, unless one is under way: then
    /// answers that one. Taken before anything is awaited, so the person
    /// landing it, or a second submission, waits for this one.
    pub fn take(&self, task: &str, entry: InFlight) -> Result<(), InFlight> {
        let mut book = self.book();
        if let Some(busy) = book.in_flight.get(task) {
            return Err(busy.clone());
        }
        book.in_flight.insert(task.to_string(), entry);
        Ok(())
    }

    pub fn release(&self, task: &str) {
        self.book().in_flight.remove(task);
    }

    pub fn in_flight(&self, task: &str) -> Option<InFlight> {
        self.book().in_flight.get(task).cloned()
    }

    pub fn settle(&self, task: &str, outcome: DeliveryOutcomeKind, detail: String, at: u64) {
        self.book().settled.insert(
            task.to_string(),
            Settled {
                outcome,
                detail,
                at,
            },
        );
    }

    /// Where a task's delivery stands (`deliveryState`).
    pub fn state(&self, task: &str) -> Option<DeliveryState> {
        let book = self.book();
        if let Some(running) = book.in_flight.get(task) {
            return Some(DeliveryState {
                outcome: None,
                since: running.since,
                detail: None,
            });
        }
        book.settled.get(task).map(|last| DeliveryState {
            outcome: Some(last.outcome),
            since: last.at,
            detail: Some(last.detail.clone()),
        })
    }

    /// Whether a delivery of the workspace is being tested or landed
    /// (`deliveringIn`): minutes during which its worker waits quietly and
    /// its workspace must not sleep.
    pub fn delivering_in(&self, ws: &str) -> bool {
        self.book()
            .in_flight
            .values()
            .any(|entry| entry.workspace_id == ws)
    }
}

/// Takes the task for one delivery (`Deliveries::take`), and tells the model,
/// whose snapshot then keeps the workspace awake (`deliveringIn`).
async fn hold(port: &Port, task: &str, entry: InFlight) -> Result<(), InFlight> {
    let ws = entry.workspace_id.clone();
    port.deliveries().take(task, entry)?;
    let _ = port
        .apply(ModelCmd::BeginDelivery {
            ws,
            task: task.to_string(),
        })
        .await;
    Ok(())
}

/// The task's delivery is over, or never started.
async fn let_go(port: &Port, task: &str) {
    let Some(entry) = port.deliveries().in_flight(task) else {
        return;
    };
    port.deliveries().release(task);
    let _ = port
        .apply(ModelCmd::EndDelivery {
            ws: entry.workspace_id,
            task: task.to_string(),
        })
        .await;
}

/// Where a task's delivery stands, for P19's `worker_list` and P20's phone.
pub fn delivery_state(port: &Port, task: &str) -> Option<DeliveryState> {
    port.deliveries().state(task)
}

/// Whether a delivery of the workspace is under way, for hibernation.
pub fn delivering_in(port: &Port, ws: &str) -> bool {
    port.deliveries().delivering_in(ws)
}

/// A delivery being run for the person (`beginDelivery`): until `end`, its
/// workspace stays awake and « Fusionner » shows its spinner. Ending twice
/// is harmless.
pub struct DeliveryRun {
    port: Port,
    ws: WsId,
    task: String,
    ended: bool,
}

pub async fn begin_delivery(port: &Port, ws: &str, task: &str) -> DeliveryRun {
    let _ = port
        .apply(ModelCmd::BeginDelivery {
            ws: ws.to_string(),
            task: task.to_string(),
        })
        .await;
    DeliveryRun {
        port: port.clone(),
        ws: ws.to_string(),
        task: task.to_string(),
        ended: false,
    }
}

impl DeliveryRun {
    pub async fn end(&mut self) {
        if std::mem::replace(&mut self.ended, true) {
            return;
        }
        let _ = self
            .port
            .apply(ModelCmd::EndDelivery {
                ws: self.ws.clone(),
                task: self.task.clone(),
            })
            .await;
    }
}

/* ── Helpers ───────────────────────────────────────────────────── */

fn without(labels: &[String], gone: &[&str]) -> Vec<String> {
    labels
        .iter()
        .filter(|label| !gone.contains(&label.as_str()))
        .cloned()
        .collect()
}

fn with_label(labels: &[String], label: &str) -> Vec<String> {
    let mut labels = labels.to_vec();
    if !labels.iter().any(|held| held == label) {
        labels.push(label.to_string());
    }
    labels
}

fn moved(column: &str, labels: Vec<String>) -> TaskPatch {
    TaskPatch {
        column: Some(column.to_string()),
        labels: Some(labels),
        ..TaskPatch::default()
    }
}

/// At most `max` characters, the last three of them `...` when cut.
fn capped(line: String, max: usize) -> String {
    if line.chars().count() > max {
        let kept: String = line.chars().take(max - 3).collect();
        format!("{kept}...")
    } else {
        line
    }
}

/// The first sentence or line of `detail`, short enough for an event
/// (`headline`).
pub fn headline(detail: &str) -> String {
    let line = detail.split('\n').next().unwrap_or("").trim();
    let chars: Vec<char> = line.chars().collect();
    let end = chars.iter().enumerate().position(|(index, c)| {
        matches!(c, '.' | ':') && chars.get(index + 1).is_none_or(|next| next.is_whitespace())
    });
    let sentence = match end {
        Some(end) => chars[..=end].iter().collect::<String>().trim().to_string(),
        None => line.to_string(),
    };
    capped(sentence, 240)
}

/// An outcome in one line for the orchestrator: the fact it acts on (the
/// files in conflict, the line of the check that failed, the first blocking
/// item), not the instructions the worker gets (`eventDetail`).
pub fn event_detail(outcome: &Outcome, target: &str) -> String {
    let files = outcome.files.join(", ");
    let line = match outcome.outcome {
        Kind::Landed => format!("landed on {target} ({})", short(outcome.merge.as_deref())),
        Kind::Ready => format!("ready to land on {target}"),
        Kind::Conflict if !files.is_empty() => format!("conflict with {target} in {files}"),
        Kind::Dirty if outcome.in_checkout => {
            let files = if files.is_empty() {
                String::new()
            } else {
                format!(" ({files})")
            };
            format!(
                "passed, but {target}'s checkout has local changes in the way{files}: waits for the person"
            )
        }
        Kind::Dirty => format!("uncommitted changes in the worktree: {files}"),
        Kind::CheckFailed if outcome.check.is_some() => {
            let check = outcome.check.as_ref().expect("checked above");
            let lines: Vec<&str> = check
                .tail
                .split('\n')
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect();
            let telling = lines
                .iter()
                .find(|line| {
                    let lower = line.to_lowercase();
                    ["error", "fail", "panic", "assert"]
                        .iter()
                        .any(|word| lower.contains(word))
                })
                .or(lines.last());
            let how = if check.timed_out {
                format!("timed out after {} s", check.seconds)
            } else {
                let exit = check
                    .exit_code
                    .map_or_else(|| "?".to_string(), |code| code.to_string());
                format!("exited {exit}")
            };
            let telling = telling.map(|line| format!(": {line}")).unwrap_or_default();
            format!("`{}` {how}{telling}", check.command)
        }
        Kind::ChangesRequested => {
            let blocking = outcome
                .review
                .as_ref()
                .map(|review| review.blocking.as_slice())
                .unwrap_or_default();
            match blocking.first() {
                None => {
                    let notes = outcome
                        .review
                        .as_ref()
                        .map(|review| review.notes.trim())
                        .filter(|notes| !notes.is_empty())
                        .unwrap_or("changes requested");
                    format!("review: {notes}")
                }
                Some(first) => {
                    let more = if blocking.len() > 1 {
                        format!(" (+{} more)", blocking.len() - 1)
                    } else {
                        String::new()
                    };
                    let line = first
                        .line
                        .filter(|line| *line != 0)
                        .map(|line| format!(":{line}"))
                        .unwrap_or_default();
                    format!(
                        "review: {}{line}: {}{more}",
                        first.file,
                        first.problem.trim()
                    )
                }
            }
        }
        Kind::Stale => format!(
            "the person asked to land it on {target}; it was tested as {}",
            short(outcome.merge.as_deref())
        ),
        _ => headline(&outcome.detail),
    };
    capped(line, 360)
}

fn caller_pane<'a>(ws: &'a Workspace, caller: &Caller) -> Option<&'a Pane> {
    let id = caller.pane_id.as_deref()?;
    ws.panes.iter().find(|pane| pane.id == id)
}

fn is_worker_on_task(pane: &Pane) -> bool {
    pane.role == Some(PaneRole::Worker) && pane.task_id.as_deref().is_some_and(|id| !id.is_empty())
}

fn require(port: &Port) -> Result<std::sync::Arc<dyn DeliveryDeps>, String> {
    port.delivery()
        .ok_or_else(|| "Deliveries are not available in this Hangar window.".into())
}

/// A pane's worktree: its folder, read from the workspace's.
fn worktree_of(ws: &Workspace, pane: &Pane) -> String {
    let folder = pane
        .cwd
        .as_deref()
        .filter(|cwd| !cwd.is_empty())
        .unwrap_or(&ws.cwd);
    resolve_path(&ws.cwd, folder)
}

/// The Claude account the review spends: the one a pane of this workspace
/// would start on (`launchAccount`), by P8's rule
/// (`accounts::launch_account_with`): pane over workspace over
/// settings over main, an account that no longer exists skipped.
///
/// P8's rule asks for the list synchronously, and only when a level names an
/// account other than main; the deliveries' list is async. So the rule is
/// asked once without the list, and again with it only if it wanted it.
async fn launch_account(
    settings: &Settings,
    ws: &Workspace,
    pane: Option<&Pane>,
    delivery: &dyn DeliveryDeps,
) -> String {
    let (default, workspace, pane) = (
        settings.claude_account.as_deref(),
        ws.claude_account.as_deref(),
        pane.and_then(|pane| pane.claude_account.as_deref()),
    );
    let mut wanted = false;
    let without = launch_account_with(
        || {
            wanted = true;
            Ok(Vec::new())
        },
        default,
        workspace,
        pane,
    );
    if !wanted {
        return without.id;
    }
    let accounts = delivery.accounts().await;
    launch_account_with(move || accounts, default, workspace, pane).id
}

/// The first of `preferred` that is detected, else the first shell detected
/// (`resolveShell`).
fn resolve_shell(shells: &[ShellInfo], preferred: &[Option<&str>]) -> Option<ShellInfo> {
    preferred
        .iter()
        .flatten()
        .find_map(|id| shells.iter().find(|shell| shell.id == *id))
        .or(shells.first())
        .cloned()
}

/* ── Operations ────────────────────────────────────────────────── */

pub async fn control(
    op: &str,
    ws: &Workspace,
    args: &Value,
    caller: &Caller,
    port: &Port,
) -> Result<Value, String> {
    match op {
        "delivery_submit" => submit(ws, args, caller, port, require(port)?).await,
        "delivery_status" => status(ws, caller, port, require(port)?).await,
        _ => Err(unknown(op)),
    }
}

async fn submit(
    ws: &Workspace,
    args: &Value,
    caller: &Caller,
    port: &Port,
    delivery: std::sync::Arc<dyn DeliveryDeps>,
) -> Result<Value, String> {
    let Some(pane) = caller_pane(ws, caller).filter(|pane| is_worker_on_task(pane)) else {
        return Err(
            "delivery_submit is for worker panes, the ones an orchestrator starts on a board task with \
             worker_spawn, and this is not one. Commit your work and tell whoever asked for it instead."
                .into(),
        );
    };
    let task_id = pane.task_id.clone().unwrap_or_default();
    let entry = InFlight {
        workspace_id: ws.id.clone(),
        worker: pane.name.clone(),
        since: port.now_ms(),
        landing: false,
    };
    if let Err(busy) = hold(port, &task_id, entry).await {
        return Err(if busy.landing {
            "Your delivery is being landed right now. Wait for Hangar's message; do not submit again."
                .into()
        } else {
            "Your delivery is already being merged, checked and reviewed. Wait for Hangar's message with \
             the outcome; do not submit again meanwhile."
                .into()
        });
    }
    let mut started = false;
    let answer = submitted(ws, pane, &task_id, args, port, &delivery, &mut started).await;
    if !started {
        let_go(port, &task_id).await;
    }
    answer
}

/// The rest of `submit`, with the task taken; `started` once the delivery
/// runs in the background and holds the task itself.
async fn submitted(
    ws: &Workspace,
    pane: &Pane,
    task_id: &str,
    args: &Value,
    port: &Port,
    delivery: &std::sync::Arc<dyn DeliveryDeps>,
    started: &mut bool,
) -> Result<Value, String> {
    let board = delivery.board_load(&ws.cwd).await?;
    let Some(task) = board.tasks.into_iter().find(|entry| entry.id == task_id) else {
        return Err(format!(
            "Your task ({task_id}) is no longer on the board, so there is nothing to deliver it for. Stop \
             here, and say so in your last message."
        ));
    };
    if task.column == "done" {
        return Err(format!(
            "\"{}\" is already done: there is nothing more to deliver. Stop here.",
            task.title
        ));
    }
    // Passed, and waiting for the person to land it.
    let waiting = task.labels.iter().any(|label| label == READY_TO_LAND_LABEL);
    if task.column != "doing" && !(waiting && task.column == "review") {
        return Err(format!(
            "\"{}\" is in {}, not doing: nobody is meant to be working on it now. Stop here.",
            task.title, task.column
        ));
    }
    let summary = text(args.get("summary"));
    if pane.shared_folder == Some(true) {
        return done_in_folder(ws, pane, &task, summary, port, delivery.as_ref()).await;
    }

    let settings = orchestration_of(ws);
    let plan = delivery
        .preflight(
            ws.cwd.clone(),
            worktree_of(ws, pane),
            settings.branch.clone(),
            false,
            Some(task_id.to_string()),
        )
        .await?;

    let tested = plan.tested.clone();
    if waiting && tested.as_ref().is_some_and(|tested| tested.current) {
        let tested = tested.expect("checked above");
        return Ok(waits_already(ws, pane, &task, &plan, &tested, port));
    }
    if waiting {
        // New commits since it passed: what waited no longer stands for the
        // branch. It goes, and this one takes the whole way again.
        delivery
            .board_update(
                &ws.cwd,
                task_id,
                moved("doing", without(&task.labels, &[READY_TO_LAND_LABEL])),
            )
            .await?;
        let as_tested = tested
            .as_ref()
            .map(|tested| format!(" (tested as {})", short(Some(&tested.merge))))
            .unwrap_or_default();
        delivery
            .board_comment(
                &ws.cwd,
                task_id,
                HANGAR,
                &format!(
                    "Submitted again with new commits on {}: the delivery that waited for the person{as_tested} \
                     is withdrawn, and this one is merged, checked and reviewed before it can land.",
                    plan.branch
                ),
            )
            .await?;
    }
    let ahead = delivery
        .running()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|entry| entry.queue == plan.queue)
        .count();

    *started = true;
    let run = {
        let (ws, pane, task, plan, port) = (
            ws.clone(),
            pane.clone(),
            task.clone(),
            plan.clone(),
            port.clone(),
        );
        let delivery = delivery.clone();
        let summary = summary.clone();
        async move {
            deliver(
                &ws,
                &pane,
                &task,
                &plan,
                summary,
                &port,
                delivery.as_ref(),
                false,
            )
            .await;
        }
    };
    port.spawn(Box::pin(run));

    let check = settings
        .check_command
        .as_deref()
        .map(|command| format!("runs `{command}` there, "))
        .unwrap_or_default();
    let mut answer = json!({
        "submitted": true,
        "task": task.title,
        "branch": plan.branch,
        "target": plan.target,
        "commits": plan.ahead,
        "next": format!(
            "Hangar now merges {} onto {} in a separate worktree, {check}and has a reviewer read the change. \
             The outcome comes to you as a message from Hangar, usually within minutes. End your turn now \
             and wait; do not change {} meanwhile.",
            plan.branch, plan.target, plan.branch
        ),
    });
    if ahead > 0 {
        answer["queuedBehind"] = json!(ahead);
    }
    if waiting {
        answer["replaces"] = json!(match &tested {
            Some(tested) => short(Some(&tested.merge)),
            None => "the delivery that waited for the person".into(),
        });
    }
    Ok(answer)
}

/// A worker in a workspace that is not a git repository hands its task in:
/// there is no branch to merge, check or review, and its changes are in the
/// folder already. The task is done and the orchestrator told; the pane goes,
/// as it does once a delivery lands, a moment after this answer reaches it.
async fn done_in_folder(
    ws: &Workspace,
    pane: &Pane,
    task: &Task,
    summary: Option<String>,
    port: &Port,
    delivery: &dyn DeliveryDeps,
) -> Result<Value, String> {
    delivery
        .board_update(
            &ws.cwd,
            &task.id,
            moved(
                "done",
                without(&task.labels, &[READY_TO_LAND_LABEL, NEEDS_YOU_LABEL]),
            ),
        )
        .await?;
    let mut comment = format!(
        "Done by {} in the workspace's folder. It is not a git repository, so Hangar merged, checked and \
         reviewed nothing: the changes are in place as the worker left them.",
        pane.name
    );
    if let Some(summary) = &summary {
        comment.push_str(&format!("\n{SUMMARY}{summary}"));
    }
    delivery
        .board_comment(&ws.cwd, &task.id, HANGAR, &comment)
        .await?;
    let detail = summary
        .map(|summary| format!("summary: {summary}"))
        .unwrap_or_default();
    port.deliveries().settle(
        &task.id,
        DeliveryOutcomeKind::Done,
        detail.clone(),
        port.now_ms(),
    );
    port.post_event(
        &ws.id,
        OrchestratorEvent::Delivery {
            task_id: task.id.clone(),
            title: task.title.clone(),
            worker: pane.name.clone(),
            outcome: DeliveryOutcomeKind::Done,
            detail,
        },
        false,
    );
    let closing = {
        let (port, ws, pane) = (port.clone(), ws.id.clone(), pane.id.clone());
        async move {
            port.sleep(CLOSE_AFTER_MS).await;
            let snapshot = port.snapshot();
            if workspace_in(&snapshot, &ws)
                .is_some_and(|entry| entry.panes.iter().any(|held| held.id == pane))
            {
                let _ = port.apply(ModelCmd::ClosePane { ws, pane }).await;
            }
        }
    };
    port.spawn(Box::pin(closing));
    Ok(json!({
        "submitted": true,
        "done": true,
        "task": task.title,
        "next": "Your task is done: the workspace is not a git repository, so there is nothing to merge, and the \
                 orchestrator has been told. End your turn now; Hangar closes your pane in a moment.",
    }))
}

/// Submitted again without a new commit: what passed still waits for the
/// person, and the worker has nothing to do. Said to the orchestrator too,
/// which may be waiting on this submission.
fn waits_already(
    ws: &Workspace,
    pane: &Pane,
    task: &Task,
    plan: &Plan,
    tested: &Tested,
    port: &Port,
) -> Value {
    let tested_as = short(Some(&tested.merge));
    port.post_event(
        &ws.id,
        OrchestratorEvent::Delivery {
            task_id: task.id.clone(),
            title: task.title.clone(),
            worker: pane.name.clone(),
            outcome: DeliveryOutcomeKind::Ready,
            detail: format!(
                "submitted again without a new commit: still waits to land on {} as tested ({tested_as})",
                plan.target
            ),
        },
        false,
    );
    json!({
        "submitted": false,
        "waiting": true,
        "task": task.title,
        "branch": plan.branch,
        "target": plan.target,
        "testedAs": tested_as,
        "next": format!(
            "This delivery already passed its check and its review, as {tested_as}, and waits for the person to \
             land it on {}: {} has no commit since. There is nothing more for you to do: stop here. If you were \
             asked to change something, commit the change on {}, then call delivery_submit again: the new \
             delivery replaces the one waiting.",
            plan.target, plan.branch, plan.branch
        ),
    })
}

async fn status(
    ws: &Workspace,
    caller: &Caller,
    port: &Port,
    delivery: std::sync::Arc<dyn DeliveryDeps>,
) -> Result<Value, String> {
    let running = delivery.running().await.unwrap_or_default();
    let panes: Vec<&Pane> = match caller_pane(ws, caller).filter(|pane| is_worker_on_task(pane)) {
        Some(mine) => vec![mine],
        None => workers_of(ws)
            .into_iter()
            .filter(|pane| pane.task_id.as_deref().is_some_and(|id| !id.is_empty()))
            .collect(),
    };
    let deliveries = panes
        .iter()
        .map(|pane| {
            let task_id = pane.task_id.clone().unwrap_or_default();
            let stage = running.iter().find(|entry| entry.task_id == task_id);
            let state = port.deliveries().state(&task_id);
            let mut entry = json!({
                "worker": pane.name,
                "task": task_id,
                "state": stage
                    .map(|stage| stage.stage.clone())
                    .or(state.as_ref().map(|state| state.state().to_string()))
                    .unwrap_or_else(|| "none".into()),
            });
            if let Some(state) = &state {
                entry["since"] = json!(iso_time(stage.map_or(state.since, |stage| stage.since)));
                if let Some(detail) = state.detail.as_deref().filter(|detail| !detail.is_empty()) {
                    entry["detail"] = json!(detail);
                }
            }
            entry
        })
        .collect::<Vec<_>>();
    Ok(json!({ "deliveries": deliveries }))
}

/// Runs one delivery to its end, and does what its outcome calls for.
#[allow(clippy::too_many_arguments)]
async fn deliver(
    ws: &Workspace,
    pane: &Pane,
    task: &Task,
    plan: &Plan,
    summary: Option<String>,
    port: &Port,
    delivery: &dyn DeliveryDeps,
    landing: bool,
) -> Outcome {
    let settings = orchestration_of(ws);
    let snapshot = port.snapshot();
    // The review spends the account a pane of this workspace would start on.
    let account = launch_account(&snapshot.state.settings, ws, None, delivery).await;
    let shells = delivery.shells();
    let request = Delivery {
        workspace: ws.cwd.clone(),
        worktree: worktree_of(ws, pane),
        branch: plan.branch.clone(),
        target: plan.target.clone(),
        task: TaskBrief {
            id: task.id.clone(),
            title: task.title.clone(),
            description: task.description.clone(),
            owns: task.owns.clone(),
        },
        summary,
        check_command: settings.check_command.clone(),
        shell: resolve_shell(
            &shells,
            &[
                pane.shell_id.as_deref(),
                ws.shell_id.as_deref(),
                snapshot.state.settings.shell_id.as_deref(),
            ],
        ),
        reviewer_model: settings.reviewer_model.clone(),
        claude_account: (account != MAIN_ACCOUNT).then_some(account),
        land: landing || settings.auto_merge,
        reuse: landing,
        port_base: pane.port_base,
    };
    let outcome = match delivery.run(request.clone()).await {
        Ok(outcome) => outcome,
        Err(error) => Outcome {
            outcome: Kind::Error,
            detail: format!("The delivery could not run: {error}. Nothing was merged."),
            files: vec![],
            in_checkout: false,
            changed: vec![],
            outside_owns: vec![],
            check: None,
            review: None,
            merge: None,
            base: None,
            landed_in: None,
        },
    };
    // The outcome stands whatever its follow-ups managed; the next delivery or
    // the person sees the board as it is.
    let _ = settle(ws, pane, task, &request, &outcome, port, delivery).await;
    port.deliveries().settle(
        &task.id,
        outcome_of(outcome.outcome),
        headline(&outcome.detail),
        port.now_ms(),
    );
    let_go(port, &task.id).await;
    outcome
}

/// What an outcome calls for: the board, the worker, the orchestrator.
async fn settle(
    submitted: &Workspace,
    pane: &Pane,
    task: &Task,
    request: &Delivery,
    outcome: &Outcome,
    port: &Port,
    delivery: &dyn DeliveryDeps,
) -> Result<(), String> {
    // As things are now, minutes later: the pane may have been closed, the
    // task moved or renamed.
    let snapshot = port.snapshot();
    let ws = workspace_in(&snapshot, &submitted.id).unwrap_or(submitted);
    let worker = ws.panes.iter().find(|entry| entry.id == pane.id);
    let cwd = ws.cwd.as_str();
    let current = delivery
        .board_load(cwd)
        .await?
        .tasks
        .into_iter()
        .find(|entry| entry.id == task.id)
        .unwrap_or_else(|| task.clone());
    let (target, branch) = (request.target.as_str(), request.branch.as_str());
    let kind = outcome_of(outcome.outcome);
    let post = |detail: String| {
        port.post_event(
            &ws.id,
            OrchestratorEvent::Delivery {
                task_id: task.id.clone(),
                title: current.title.clone(),
                worker: pane.name.clone(),
                outcome: kind,
                detail,
            },
            false,
        )
    };
    // Landing what waited for the person lands the merge tested back then.
    let as_tested = request.reuse && outcome.check.is_none() && outcome.review.is_none();
    let details: Vec<String> = [
        request
            .summary
            .as_ref()
            .map(|summary| format!("{SUMMARY}{summary}")),
        Some(if as_tested {
            AS_TESTED.to_string()
        } else {
            check_line(outcome, request.check_command.as_deref())
        }),
        if as_tested {
            None
        } else {
            review_line(outcome, &request.reviewer_model)
        },
        (!outcome.outside_owns.is_empty())
            .then(|| format!("{OUTSIDE}{}.", outcome.outside_owns.join(", "))),
    ]
    .into_iter()
    .flatten()
    .collect();
    let merge = outcome.merge.as_deref();

    if outcome.outcome == Kind::Landed {
        delivery
            .board_update(
                cwd,
                &task.id,
                moved(
                    "done",
                    without(&current.labels, &[READY_TO_LAND_LABEL, NEEDS_YOU_LABEL]),
                ),
            )
            .await?;
        // Its work is on the branch now: the worker, its worktree and its
        // branch go.
        if let Some(worker) = worker {
            let _ = port
                .apply(ModelCmd::ClosePane {
                    ws: ws.id.clone(),
                    pane: worker.id.clone(),
                })
                .await;
        }
        let cleanup = delivery
            .cleanup(
                request.worktree.clone(),
                branch.to_string(),
                target.to_string(),
                task.id.clone(),
            )
            .await
            .unwrap_or_else(|error| Cleanup {
                worktree_removed: false,
                branch_deleted: false,
                kept: vec![error],
            });
        let place = match &outcome.landed_in {
            Some(checkout) => format!(", fast-forwarded in {checkout}"),
            None => " (only its ref moved)".into(),
        };
        let mut lines = vec![format!("Landed on {target} ({}){place}.", short(merge))];
        lines.extend(details);
        if !cleanup.kept.is_empty() {
            lines.push(format!("Kept: {}.", cleanup.kept.join("; ")));
        }
        delivery
            .board_comment(cwd, &task.id, HANGAR, &lines.join("\n"))
            .await?;
        post(event_detail(outcome, target));
        return Ok(());
    }

    if outcome.outcome == Kind::Ready || (outcome.outcome == Kind::Dirty && outcome.in_checkout) {
        delivery
            .board_update(
                cwd,
                &task.id,
                moved("review", with_label(&current.labels, READY_TO_LAND_LABEL)),
            )
            .await?;
        let waits = if outcome.outcome == Kind::Ready {
            ready_head(target, merge)
        } else {
            dirty_head(target, &outcome.files, merge)
        };
        let mut lines = vec![waits];
        lines.extend(details);
        delivery
            .board_comment(cwd, &task.id, HANGAR, &lines.join("\n"))
            .await?;
        if let Some(worker) = worker {
            delivery.tell(&worker.id, &outcome.detail).await?;
        }
        post(event_detail(outcome, target));
        return Ok(());
    }

    if outcome.outcome == Kind::Stale {
        // Nothing landed and nothing failed: the worker moved on past what
        // waited, which goes; its next submission is the delivery.
        delivery
            .board_update(
                cwd,
                &task.id,
                moved("doing", without(&current.labels, &[READY_TO_LAND_LABEL])),
            )
            .await?;
        delivery
            .board_comment(
                cwd,
                &task.id,
                HANGAR,
                &format!(
                    "Not landed: {branch} has commits that were not in the delivery that waited (tested as {}). \
                     It is withdrawn; {} was asked to submit again.",
                    short(merge),
                    pane.name
                ),
            )
            .await?;
        if let Some(worker) = worker {
            delivery
                .tell(
                    &worker.id,
                    &format!(
                        "\"{}\" was not landed.\n\n{}",
                        current.title, outcome.detail
                    ),
                )
                .await?;
        }
        post(event_detail(outcome, target));
        return Ok(());
    }

    if outcome.outcome == Kind::Error && outcome.detail.contains(USAGE_LIMIT) {
        // The account behind the review is out of quota: nothing is wrong
        // with the work, so this is no failed delivery. It goes back to the
        // worker, which submits again once the limit resets.
        delivery
            .board_comment(
                cwd,
                &task.id,
                HANGAR,
                &format!("{REVIEW_STOPPED}: {}", outcome.detail),
            )
            .await?;
        if let Some(worker) = worker {
            delivery
                .tell(
                    &worker.id,
                    &format!(
                        "The review of \"{}\" could not run.\n\n{}\n\nThis is not a problem with your work and \
                         it does not count as a failed delivery: stop here, and submit again when you are asked to.",
                        current.title, outcome.detail
                    ),
                )
                .await?;
        }
        post(event_detail(outcome, target));
        return Ok(());
    }

    // A failure: back to the worker, with what to fix, however many came
    // before. The orchestrator hears of each one and decides when to stop.
    let failures = current
        .comments
        .iter()
        .filter(|comment| comment.author == HANGAR && comment.text.starts_with(FAILED))
        .count()
        + 1;
    delivery
        .board_comment(
            cwd,
            &task.id,
            HANGAR,
            &failed_comment(outcome_name(kind), failures, &outcome.detail),
        )
        .await?;
    // One that was waiting to land and no longer can is worked on again.
    let labels = without(&current.labels, &[READY_TO_LAND_LABEL]);
    if current.column != "doing" || labels.len() != current.labels.len() {
        delivery
            .board_update(cwd, &task.id, moved("doing", labels))
            .await?;
    }
    if let Some(worker) = worker {
        let next = if outcome.outcome == Kind::Error {
            "If it fails again the same way, stop and say so in a comment on the task.".into()
        } else {
            format!("Failed deliveries so far: {failures}.")
        };
        delivery
            .tell(
                &worker.id,
                &format!(
                    "Your delivery of \"{}\" did not land.\n\n{}\n\n{next}",
                    current.title, outcome.detail
                ),
            )
            .await?;
    }
    post(format!(
        "{} (failed delivery {failures})",
        event_detail(outcome, target)
    ));
    Ok(())
}

/// Lands a delivery that passed its check and its review but waits for the
/// person, as it does with automatic landing off (`landDelivery`): the merge
/// that was tested when the target has not moved since, tested again first
/// when it has; never once the worker's branch has moved past it. Refused,
/// with what stood in the way, in words for the person who asked.
pub async fn land_delivery(ws: &Workspace, task_id: &str, port: &Port) -> Result<(), String> {
    let delivery = require(port)?;
    let pane = workers_of(ws)
        .into_iter()
        .find(|pane| pane.task_id.as_deref() == Some(task_id))
        .cloned();
    // Taken before anything is awaited, like a submission.
    let entry = InFlight {
        workspace_id: ws.id.clone(),
        worker: pane
            .as_ref()
            .map(|pane| pane.name.clone())
            .unwrap_or_default(),
        since: port.now_ms(),
        landing: true,
    };
    if hold(port, task_id, entry).await.is_err() {
        return Err("This delivery is being tested or landed already.".into());
    }
    let mut started = false;
    let landed = landing(
        ws,
        task_id,
        pane.as_ref(),
        port,
        delivery.as_ref(),
        &mut started,
    )
    .await;
    if !started {
        let_go(port, task_id).await;
    }
    let (pane, plan, outcome) = landed?;
    let files = if outcome.files.is_empty() {
        String::new()
    } else {
        format!(" ({})", outcome.files.join(", "))
    };
    match outcome.outcome {
        Kind::Landed => Ok(()),
        Kind::Stale => Err(format!(
            "{name} committed on {} after it was checked and reviewed, so it would not land what is there now. \
             Nothing was landed: {name} was asked to submit it again, and it comes back here once checked and \
             reviewed.",
            plan.branch,
            name = pane.name
        )),
        Kind::Dirty if outcome.in_checkout => Err(format!(
            "Local changes in {}'s checkout are in the way{files}: commit them or set them aside, then land it \
             again. Nothing was touched.",
            plan.target
        )),
        Kind::Error => Err(format!("It could not land: {}", headline(&outcome.detail))),
        _ => Err(format!(
            "It no longer lands as it was tested ({}), so {} was told what to fix.",
            event_detail(&outcome, &plan.target),
            pane.name
        )),
    }
}

/// The rest of `land_delivery`, with the task taken.
async fn landing(
    ws: &Workspace,
    task_id: &str,
    pane: Option<&Pane>,
    port: &Port,
    delivery: &dyn DeliveryDeps,
    started: &mut bool,
) -> Result<(Pane, Plan, Outcome), String> {
    let board = delivery.board_load(&ws.cwd).await?;
    let Some(task) = board.tasks.into_iter().find(|entry| entry.id == task_id) else {
        return Err(format!(
            "No task {task_id} on the board of \"{}\".",
            ws.name
        ));
    };
    let Some(pane) = pane else {
        return Err(format!(
            "No worker holds \"{}\" any more, so there is no branch to land.",
            task.title
        ));
    };
    // Submitted again since, or sent back: what is there now was not tested.
    if !task.labels.iter().any(|label| label == READY_TO_LAND_LABEL) {
        return Err(format!(
            "Nothing of \"{}\" waits to land any more: the board shows where it is now.",
            task.title
        ));
    }
    let plan = delivery
        .preflight(
            ws.cwd.clone(),
            worktree_of(ws, pane),
            orchestration_of(ws).branch,
            true,
            None,
        )
        .await?;
    *started = true;
    let outcome = deliver(ws, pane, &task, &plan, None, port, delivery, true).await;
    Ok((pane.clone(), plan, outcome))
}

/// `new Date(ms).toISOString()`: `2023-11-14T22:13:20.000Z`.
pub fn iso_time(ms: u64) -> String {
    let days = (ms / 86_400_000) as i64;
    let rest = ms % 86_400_000;
    // Days to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rest / 3_600_000,
        rest / 60_000 % 60,
        rest / 1000 % 60,
        rest % 1000
    )
}
