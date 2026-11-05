//! What the window answers Hangar Mobile about a delivery that passed and
//! waits for the person (lib/remoteDelivery.ts): what it brings — the
//! worker's worktree against the branch it lands on, as the diff view reads
//! it — what the check and the review said, read back from the hangar comment
//! the delivery left on its task (P18's `delivery_text.rs` writes them), and
//! the two things the person can do about it from the phone: land it, as the
//! PC's « Fusionner » does, or send it back to its worker with what to fix.
//!
//! The phone passes back the merge it was shown: once another delivery of
//! the task has taken its place, landing or sending back the one seen is
//! refused.

use std::sync::{Arc, LazyLock, Mutex};

use regex::Regex;
use serde_json::Value;
use tokio::sync::watch;

use hangar_core::board::{Comment, Task};
use hangar_core::delivery::Plan;
use hangar_core::git::{ChangedFile, Changes};

use crate::control::delivery::{begin_delivery, delivery_state, headline, land_delivery};
use crate::control::delivery_text::{AS_TESTED, HANGAR, OUTSIDE};
use crate::control::port::{DeliveryDeps, Port};
use crate::state::roles::{READY_TO_LAND_LABEL, orchestration_of, workers_of};
use crate::state::{Pane, Workspace};

use super::contract::{
    DeliveryBlocking, DeliveryCheck, DeliveryEarlier, DeliveryFile, DeliveryReview, RemoteDelivery,
    RemoteDeliveryPatch, RemoteLanded, RemoteSent, STAGES, value,
};
use super::engine::{Mobile, locked};
use super::ops::Tell;

/// A patch past this is cut: the phone draws every line of it.
pub const PATCH_MAX_BYTES: usize = 200 * 1024;
/// `remote_land` waits this long for a landing, within the 10 s the mobile
/// server gives the window; past it, the phone asks again.
pub const LAND_WAIT_MS: u64 = 7_000;
/// A landing's outcome nobody came back for is forgotten after this.
const LAND_KEPT_MS: u64 = 5 * 60_000;

/// The delivery ops of `remote_*` (`DELIVERY_OPS`).
pub const OPS: [&str; 4] = [
    "remote_delivery",
    "remote_delivery_file",
    "remote_land",
    "remote_rework",
];

/* ── The hangar comments ──────────────────────────────────────── */

/// What the hangar comment of a delivery that passed says (`Passed`).
#[derive(Debug, Clone, PartialEq)]
pub struct Passed {
    pub at: u64,
    pub target: String,
    /// Short; `None` when the comment has none.
    pub merge: Option<String>,
    /// Local changes in the target's checkout stood in the way.
    pub in_the_way: Option<Vec<String>>,
    pub summary: Option<String>,
    pub check: Option<DeliveryCheck>,
    pub review: Option<DeliveryReview>,
}

fn regex(pattern: &str) -> Regex {
    Regex::new(pattern).expect("a valid pattern")
}

static READY_HEAD: LazyLock<Regex> = LazyLock::new(|| {
    regex(r"^Passed: waits for the person to land it on (.+) \(tested as ([0-9a-f]+|\?)\)\.$")
});
static DIRTY_HEAD: LazyLock<Regex> = LazyLock::new(|| {
    regex(
        r"^Passed, but (.+?)'s checkout has local changes in the way(?: \((.*)\))?: waits for the person to land it once they are committed or set aside \(tested as ([0-9a-f]+|\?)\)\.$",
    )
});
static CHECK_PASSED: LazyLock<Regex> =
    LazyLock::new(|| regex(r"^Check: `(.*)` passed in (\d+(?:\.\d+)?) s\.$"));
static CHECK_TIMED_OUT: LazyLock<Regex> =
    LazyLock::new(|| regex(r"^Check: `(.*)` timed out after (\d+(?:\.\d+)?) s\.$"));
static CHECK_FAILED: LazyLock<Regex> =
    LazyLock::new(|| regex(r"^Check: `(.*)` failed \(exit .*\)\.$"));
static CHECK_NONE: LazyLock<Regex> =
    LazyLock::new(|| regex(r"^Check: (?:`.*` not run|none set for this workspace)\.$"));
static REVIEW: LazyLock<Regex> = LazyLock::new(|| {
    regex(r"^Review \(.*?\): (approved|changes requested)\.(?: Notes: ([\s\S]*))?$")
});
/// A failed delivery's comment, counted `<n> of <max>` on boards written while
/// a task could fail a set number of times, `<n>` since.
static FAILED: LazyLock<Regex> =
    LazyLock::new(|| regex(r"^Delivery failed \((\w+), \d+(?: of \d+)?\): ([\s\S]*)$"));
static BLOCKING: LazyLock<Regex> = LazyLock::new(|| regex(r"^- (.+?)(?::(\d+))?: (.+)$"));

fn is_check_line(line: &str) -> bool {
    CHECK_PASSED.is_match(line)
        || CHECK_TIMED_OUT.is_match(line)
        || CHECK_FAILED.is_match(line)
        || CHECK_NONE.is_match(line)
}

fn check_of(line: &str) -> Option<DeliveryCheck> {
    let seconds = |text: &str| text.parse::<f64>().unwrap_or(0.0);
    if let Some(caps) = CHECK_PASSED.captures(line) {
        return Some(DeliveryCheck {
            command: caps[1].into(),
            passed: true,
            seconds: seconds(&caps[2]),
        });
    }
    if let Some(caps) = CHECK_TIMED_OUT.captures(line) {
        return Some(DeliveryCheck {
            command: caps[1].into(),
            passed: false,
            seconds: seconds(&caps[2]),
        });
    }
    CHECK_FAILED.captures(line).map(|caps| DeliveryCheck {
        command: caps[1].into(),
        passed: false,
        seconds: 0.0,
    })
}

fn short(sha: &str) -> Option<String> {
    (sha != "?").then(|| sha.chars().take(7).collect())
}

/// One comment, if it is the one a delivery that passed left; `as_tested`
/// when it is a landing that waits again, which says nothing of the check,
/// the review or the summary of the delivery it lands.
fn passed_one(comment: &Comment) -> Option<(Passed, bool)> {
    if comment.author != HANGAR {
        return None;
    }
    let lines: Vec<&str> = comment.text.split('\n').collect();
    let first = lines.first().copied().unwrap_or("");
    let (target, merge, in_the_way) = if let Some(ready) = READY_HEAD.captures(first) {
        (ready[1].to_string(), short(&ready[2]), None)
    } else {
        let dirty = DIRTY_HEAD.captures(first)?;
        let files = dirty
            .get(2)
            .map(|files| files.as_str())
            .filter(|files| !files.is_empty())
            .map(|files| {
                files
                    .split(", ")
                    .filter(|file| !file.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        (dirty[1].to_string(), short(&dirty[3]), Some(files))
    };

    // The summary, then the check's line, the review's, and what it changed
    // outside its files: a summary and the notes can take several lines.
    let check_at = lines
        .iter()
        .enumerate()
        .position(|(index, line)| index > 0 && is_check_line(line));
    let end = check_at.unwrap_or(lines.len());
    let said = lines[1.min(lines.len())..end].join("\n");
    let summary = said
        .strip_prefix("Summary: ")
        .map(str::trim)
        .filter(|summary| !summary.is_empty())
        .map(str::to_string);
    let mut rest: Vec<&str> = lines
        .get(end + 1..)
        .map(<[&str]>::to_vec)
        .unwrap_or_default();
    if rest.last().is_some_and(|line| line.starts_with(OUTSIDE)) {
        rest.pop();
    }
    let review = REVIEW
        .captures(&rest.join("\n"))
        .map(|verdict| DeliveryReview {
            approved: &verdict[1] == "approved",
            notes: verdict
                .get(2)
                .map_or("", |notes| notes.as_str())
                .trim()
                .to_string(),
            blocking: Vec::new(),
        });
    let as_tested = check_at.is_none() && lines.contains(&AS_TESTED);
    Some((
        Passed {
            at: comment.created_at,
            target,
            merge,
            in_the_way,
            summary,
            check: check_at.and_then(|at| check_of(lines[at])),
            review,
        },
        as_tested,
    ))
}

/// The delivery that waits, as its hangar comment said it passed, and the
/// deliveries of the task that failed before it: the latest pass wins, a
/// landing tried again writing one of its own (`deliveryHistory`).
pub fn delivery_history(comments: &[Comment]) -> (Option<Passed>, Vec<DeliveryEarlier>) {
    let mut passed: Option<Passed> = None;
    let mut passed_at: Option<usize> = None;
    for (index, comment) in comments.iter().enumerate() {
        let Some((said, as_tested)) = passed_one(comment) else {
            continue;
        };
        passed = Some(match (as_tested, passed.take()) {
            (true, Some(before)) => Passed {
                summary: before.summary,
                check: before.check,
                review: before.review,
                ..said
            },
            _ => said,
        });
        passed_at = Some(index);
    }
    let mut earlier = Vec::new();
    for (index, comment) in comments.iter().enumerate() {
        if comment.author != HANGAR || passed_at.is_some_and(|at| index > at) {
            continue;
        }
        let Some(failed) = FAILED.captures(&comment.text) else {
            continue;
        };
        let outcome = failed[1].to_string();
        let mut blocking = Vec::new();
        if outcome == "changesRequested" {
            for line in failed[2].split('\n') {
                if let Some(item) = BLOCKING.captures(line.trim()) {
                    blocking.push(DeliveryBlocking {
                        file: item[1].to_string(),
                        line: item.get(2).and_then(|line| line.as_str().parse().ok()),
                        problem: item[3].to_string(),
                    });
                }
            }
        }
        earlier.push(DeliveryEarlier {
            at: comment.created_at,
            outcome,
            text: headline(&failed[2]),
            blocking: (!blocking.is_empty()).then_some(blocking),
        });
    }
    (passed, earlier)
}

/* ── Finding the delivery ─────────────────────────────────────── */

struct Found {
    ws: Workspace,
    pane: Pane,
    task: Task,
    delivery: Arc<dyn DeliveryDeps>,
    worktree: String,
}

fn require(port: &Port) -> Result<Arc<dyn DeliveryDeps>, String> {
    port.delivery()
        .ok_or_else(|| "Deliveries are not available in this Hangar window.".into())
}

fn task_ref(args: &Value) -> Result<String, String> {
    let task = args
        .get("task")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if task.is_empty() {
        return Err("Name the task: its id, from the board.".into());
    }
    Ok(task.to_string())
}

fn is_separator(c: char) -> bool {
    c == '\\' || c == '/'
}

/// A worker's folder read from its workspace's, `.` and `..` folded
/// (`resolvePath`, as P18's `worktree_of` reads it: kept to that one use
/// here, P18's module being its own).
fn resolve_path(base: &str, path: &str) -> String {
    let bytes = path.as_bytes();
    let absolute = (bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/'))
        || matches!(bytes.first(), Some(b'\\' | b'/'));
    let full = if absolute {
        path.to_string()
    } else {
        format!("{base}/{path}")
    };
    let mut chars = full.chars();
    let unc = matches!(
        (chars.next(), chars.next(), chars.next()),
        (Some(a), Some(b), Some(c)) if is_separator(a) && is_separator(b) && !is_separator(c)
    );
    let mut pieces: Vec<&str> = full.split(is_separator).collect();
    let head = pieces.remove(0);
    let drive =
        head.len() == 2 && head.as_bytes()[0].is_ascii_alphabetic() && head.as_bytes()[1] == b':';
    let separator = if unc || drive { "\\" } else { "/" };
    let mut parts: Vec<&str> = Vec::new();
    for part in pieces {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    let joined = std::iter::once(head)
        .chain(parts)
        .collect::<Vec<_>>()
        .join(separator);
    if unc {
        format!("{separator}{joined}")
    } else {
        joined
    }
}

/// A task's delivery that waits for the person: its worker pane still holds
/// it, with the worktree and the branch, and the board labels it ready to
/// land (`waiting`).
async fn waiting(port: &Port, task_id: &str) -> Result<Found, String> {
    let delivery = require(port)?;
    let snapshot = port.snapshot();
    let held = snapshot.state.workspaces.iter().find_map(|ws| {
        workers_of(ws)
            .into_iter()
            .find(|pane| pane.task_id.as_deref() == Some(task_id))
            .map(|pane| (ws.clone(), pane.clone()))
    });
    let Some((ws, pane)) = held else {
        return Err("No worker holds this task any more, so nothing of it waits to land.".into());
    };
    let board = delivery.board_load(&ws.cwd).await?;
    let Some(task) = board.tasks.into_iter().find(|entry| entry.id == task_id) else {
        return Err(format!(
            "No task {task_id} on the board of \"{}\".",
            ws.name
        ));
    };
    if !task.labels.iter().any(|label| label == READY_TO_LAND_LABEL) {
        return Err(format!(
            "Nothing of \"{}\" waits to land any more: the board shows where it is now.",
            task.title
        ));
    }
    let folder = pane
        .cwd
        .clone()
        .filter(|cwd| !cwd.is_empty())
        .unwrap_or_else(|| ws.cwd.clone());
    let worktree = resolve_path(&ws.cwd, &folder);
    Ok(Found {
        ws,
        pane,
        task,
        delivery,
        worktree,
    })
}

/// The branch and its target, and what of the task was tested, as
/// delivery.rs reads them.
async fn plan_of(found: &Found) -> Result<Plan, String> {
    found
        .delivery
        .preflight(
            found.ws.cwd.clone(),
            found.worktree.clone(),
            orchestration_of(&found.ws).branch,
            true,
            Some(found.task.id.clone()),
        )
        .await
}

/// The merge it was tested as, short: what the phone passes back.
fn merge_of(plan: &Plan, passed: Option<&Passed>) -> Option<String> {
    match &plan.tested {
        Some(tested) => Some(tested.merge.chars().take(7).collect()),
        None => passed.and_then(|passed| passed.merge.clone()),
    }
}

/// Refused once another delivery of the task has taken the place of the one
/// the phone showed (`sameAs`).
fn same_as(args: &Value, found: &Found, plan: &Plan) -> Result<(), String> {
    let Some(seen) = args
        .get("merge")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|merge| !merge.is_empty())
    else {
        return Ok(());
    };
    let seen: String = seen.chars().take(7).collect();
    let (passed, _) = delivery_history(&found.task.comments);
    match merge_of(plan, passed.as_ref()) {
        Some(now) if now != seen => Err(format!(
            "\"{}\" was delivered again since you opened it (now {now}, you saw {seen}): look at what waits now first. Nothing was done.",
            found.task.title
        )),
        _ => Ok(()),
    }
}

/// What runs for the task now, if anything: delivery.rs's stage, or queued
/// before it starts (`stageOf`).
async fn stage_of(
    port: &Port,
    mobile: &Mobile,
    delivery: &dyn DeliveryDeps,
    task_id: &str,
) -> Option<&'static str> {
    let running = delivery.running().await.unwrap_or_default();
    if let Some(entry) = running.iter().find(|entry| entry.task_id == task_id) {
        return Some(
            STAGES
                .iter()
                .find(|stage| **stage == entry.stage)
                .copied()
                .unwrap_or("queued"),
        );
    }
    let in_flight = delivery_state(port, task_id).is_some_and(|state| state.outcome.is_none());
    let landing = locked(&mobile.landings)
        .get(task_id)
        .is_some_and(|landing| landing.outcome().is_none());
    (in_flight || landing).then_some("queued")
}

/* ── The ops ──────────────────────────────────────────────────── */

fn file_of(file: &ChangedFile) -> DeliveryFile {
    DeliveryFile {
        path: file.path.clone(),
        old_path: file.old_path.clone().filter(|path| !path.is_empty()),
        status: match file.status.as_str() {
            "A" | "C" => "added",
            "D" => "deleted",
            "R" => "renamed",
            _ => "modified",
        },
        additions: file.additions.unwrap_or(0),
        deletions: file.deletions.unwrap_or(0),
        binary: file.binary.then_some(true),
    }
}

/// What a delivery brings: its commits; an untracked file was never part of
/// one.
fn delivered(changes: &Changes) -> impl Iterator<Item = &ChangedFile> {
    changes.files.iter().filter(|file| file.status != "?")
}

async fn remote_delivery(
    port: &Port,
    mobile: &Mobile,
    args: &Value,
) -> Result<RemoteDelivery, String> {
    let found = waiting(port, &task_ref(args)?).await?;
    let plan = plan_of(&found).await?;
    let (changes, stage) = tokio::join!(
        mobile.sources().changes(&found.worktree, &plan.target),
        stage_of(port, mobile, found.delivery.as_ref(), &found.task.id),
    );
    let changes = changes?;
    let (passed, earlier) = delivery_history(&found.task.comments);
    Ok(RemoteDelivery {
        task_id: found.task.id.clone(),
        title: found.task.title.clone(),
        workspace: found.ws.name.clone(),
        workspace_id: found.ws.id.clone(),
        worker: found.pane.name.clone(),
        branch: plan.branch.clone(),
        target: plan.target.clone(),
        summary: passed.as_ref().and_then(|passed| passed.summary.clone()),
        check: passed.as_ref().and_then(|passed| passed.check.clone()),
        review: passed.as_ref().and_then(|passed| passed.review.clone()),
        files: delivered(&changes).map(file_of).collect(),
        more_files: changes.truncated.then_some(true),
        waiting: stage.is_none(),
        stage,
        merge: merge_of(&plan, passed.as_ref()),
        passed_at: passed.as_ref().map(|passed| passed.at),
        moved: plan.tested.as_ref().is_some_and(|tested| !tested.current),
        in_the_way: passed.as_ref().and_then(|passed| passed.in_the_way.clone()),
        earlier,
    })
}

/// A patch cut at a line boundary past `max` bytes of UTF-8, or within the
/// line when the first one is longer than that, never inside a character
/// (`capPatch`).
pub fn cap_patch(text: &str, max: usize) -> (String, bool) {
    let bytes = text.as_bytes();
    if bytes.len() <= max {
        return (text.to_string(), false);
    }
    let mut end = bytes[..max]
        .iter()
        .rposition(|&byte| byte == b'\n')
        .map_or(0, |at| at + 1);
    if end == 0 {
        end = max;
        while end > 0 && (bytes[end] & 0xc0) == 0x80 {
            end -= 1;
        }
    }
    (String::from_utf8_lossy(&bytes[..end]).into_owned(), true)
}

async fn remote_delivery_file(
    port: &Port,
    mobile: &Mobile,
    args: &Value,
) -> Result<RemoteDeliveryPatch, String> {
    let wanted = args
        .get("file")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if wanted.is_empty() {
        return Err("Name the file: its path, from the delivery's files.".into());
    }
    let found = waiting(port, &task_ref(args)?).await?;
    let plan = plan_of(&found).await?;
    let changes = mobile
        .sources()
        .changes(&found.worktree, &plan.target)
        .await?;
    let Some(file) = delivered(&changes)
        .find(|file| file.path == wanted)
        .cloned()
    else {
        return Err(format!(
            "\"{wanted}\" is not one of the files \"{}\" changes.",
            found.task.title
        ));
    };
    let diff = mobile
        .sources()
        .file_diff(&found.worktree, &file, &plan.target)
        .await?;
    let (patch, cut) = cap_patch(if diff.binary { "" } else { &diff.text }, PATCH_MAX_BYTES);
    Ok(RemoteDeliveryPatch {
        path: file.path,
        patch,
        binary: diff.binary || file.binary,
        truncated: diff.truncated || cut,
    })
}

/// How a landing the phone started ended: landed, or the reason it did not,
/// in words for the person.
type Outcome = Result<(), String>;

/// A landing the phone started: it asks again until it has the outcome.
#[derive(Clone)]
pub struct Landing {
    /// The branch it lands on, once its plan is read.
    target: Arc<Mutex<String>>,
    outcome: Arc<Mutex<Option<Outcome>>>,
    ended_at: Arc<Mutex<u64>>,
    done: watch::Receiver<bool>,
}

impl Landing {
    pub fn outcome(&self) -> Option<Outcome> {
        locked(&self.outcome).clone()
    }

    /// One not started yet, and what ends it.
    fn new() -> (Landing, watch::Sender<bool>) {
        let (finished, done) = watch::channel(false);
        let landing = Landing {
            target: Arc::new(Mutex::new(String::new())),
            outcome: Arc::new(Mutex::new(None)),
            ended_at: Arc::new(Mutex::new(0)),
            done,
        };
        (landing, finished)
    }

    fn end(&self, outcome: Outcome, at: u64, finished: &watch::Sender<bool>) {
        *locked(&self.outcome) = Some(outcome);
        *locked(&self.ended_at) = at;
        let _ = finished.send(true);
    }

    /// Ended longer ago than any phone still asks for its outcome.
    fn stale(&self, now: u64) -> bool {
        self.outcome().is_some() && now.saturating_sub(*locked(&self.ended_at)) > LAND_KEPT_MS
    }
}

/// Lands the delivery in the background, as « Fusionner » does: the PC shows
/// it landing, and its workspace stays awake for it (`startLanding`).
async fn start_landing(
    port: &Port,
    found: &Found,
    plan: &Plan,
    landing: &Landing,
    finished: watch::Sender<bool>,
) {
    *locked(&landing.target) = plan.target.clone();
    let mut run = begin_delivery(port, &found.ws.id, &found.task.id).await;
    let (ws, task, port_held, kept) = (
        found.ws.clone(),
        found.task.id.clone(),
        port.clone(),
        landing.clone(),
    );
    port.spawn(Box::pin(async move {
        let outcome = land_delivery(&ws, &task, &port_held).await;
        run.end().await;
        kept.end(outcome, port_held.now_ms(), &finished);
    }));
}

/// Whether this call is the one to start the task's landing, or one to wait
/// on the landing another call started.
enum Claim {
    Start(Landing, watch::Sender<bool>),
    Known(Landing),
}

/// The task's landing, or a new one booked under the same lock: two taps at
/// once — two phones, or a retry — never both start one, the second waiting
/// on the first's outcome (MOB-07). Landings no phone came back for are
/// forgotten here (MOB-16).
fn claim(mobile: &Mobile, task_id: &str, now: u64) -> Claim {
    let mut landings = locked(&mobile.landings);
    landings.retain(|_, landing| !landing.stale(now));
    if let Some(landing) = landings.get(task_id) {
        return Claim::Known(landing.clone());
    }
    let (landing, finished) = Landing::new();
    landings.insert(task_id.to_string(), landing.clone());
    Claim::Start(landing, finished)
}

async fn remote_land(port: &Port, mobile: &Mobile, args: &Value) -> Result<RemoteLanded, String> {
    let task_id = task_ref(args)?;
    let landing = match claim(mobile, &task_id, port.now_ms()) {
        Claim::Known(landing) => landing,
        Claim::Start(landing, finished) => {
            let ready = async {
                let found = waiting(port, &task_id).await?;
                let plan = plan_of(&found).await?;
                same_as(args, &found, &plan)?;
                Ok::<_, String>((found, plan))
            }
            .await;
            match ready {
                Ok((found, plan)) => {
                    start_landing(port, &found, &plan, &landing, finished).await;
                    landing
                }
                Err(error) => {
                    // Refused before it started: a call waiting on it is
                    // told why, and the next tap is a landing of its own.
                    locked(&mobile.landings).remove(&task_id);
                    landing.end(Err(error.clone()), port.now_ms(), &finished);
                    return Err(error);
                }
            }
        }
    };
    let mut done = landing.done.clone();
    tokio::select! {
        biased;
        _ = done.wait_for(|done| *done) => {}
        () = mobile.sources().wait(LAND_WAIT_MS) => {}
    }
    let Some(outcome) = landing.outcome() else {
        let delivery = require(port)?;
        let stage = stage_of(port, mobile, delivery.as_ref(), &task_id).await;
        return Ok(RemoteLanded::Running {
            landed: false,
            stage: stage.unwrap_or("queued"),
        });
    };
    // Said once: the next « Fusionner » is a landing of its own.
    {
        let mut landings = locked(&mobile.landings);
        if landings
            .get(&task_id)
            .is_some_and(|kept| Arc::ptr_eq(&kept.outcome, &landing.outcome))
        {
            landings.remove(&task_id);
        }
    }
    outcome?;
    let target = locked(&landing.target).clone();
    Ok(RemoteLanded::Landed {
        landed: true,
        target,
    })
}

async fn remote_rework(
    port: &Port,
    mobile: &Mobile,
    args: &Value,
    tell: &dyn Tell,
) -> Result<RemoteSent, String> {
    let text = args
        .get("text")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if text.is_empty() {
        return Err("Say what to fix: give text.".into());
    }
    let found = waiting(port, &task_ref(args)?).await?;
    if args.get("merge").is_some_and(Value::is_string) {
        let plan = plan_of(&found).await?;
        same_as(args, &found, &plan)?;
    }
    if stage_of(port, mobile, found.delivery.as_ref(), &found.task.id)
        .await
        .is_some()
    {
        return Err(format!(
            "\"{}\" is being landed or tested right now: wait for its outcome. Nothing was sent.",
            found.task.title
        ));
    }
    tell.tell(port, mobile, &found.ws, &found.pane, text)
        .await?;
    let note = format!(
        "Sent back to {} from the phone, to fix before landing: {text}",
        found.pane.name
    );
    if let Err(error) = found
        .delivery
        .board_comment(&found.ws.cwd, &found.task.id, "user", &note)
        .await
    {
        return Err(format!(
            "Sent to {}, but the comment on the task could not be written: {error}",
            found.pane.name
        ));
    }
    Ok(RemoteSent::SENT)
}

/// A delivery op, run as the person (`runDelivery`).
pub async fn run(
    op: &str,
    args: &Value,
    port: &Port,
    mobile: &Mobile,
    tell: &dyn Tell,
) -> Result<Value, String> {
    match op {
        "remote_delivery" => remote_delivery(port, mobile, args)
            .await
            .and_then(|a| value(&a)),
        "remote_delivery_file" => remote_delivery_file(port, mobile, args)
            .await
            .and_then(|a| value(&a)),
        "remote_land" => remote_land(port, mobile, args)
            .await
            .and_then(|a| value(&a)),
        "remote_rework" => remote_rework(port, mobile, args, tell)
            .await
            .and_then(|a| value(&a)),
        _ => Err(format!("Unknown remote request \"{op}\".")),
    }
}
