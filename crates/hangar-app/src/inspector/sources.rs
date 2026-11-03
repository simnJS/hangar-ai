//! Where the inspector reads what other packages own.
//!
//! The inspector draws the focused pane from five things it does not keep:
//! what the pane reported (P4's `PaneInsight`, its activity and the plan's
//! limits), the board task on it (P12's boards cache), the shortcut of a
//! command (P7's keymap) and the order the panes are read in (P0b's layout).
//! Those packages are being built beside this one, so each read is a function
//! here, and **this file is the only place that has to change when one of
//! them lands**: the three `Live` stubs are unit structs today, and each
//! function says what it will read.
//!
//! Also here, small and pure, the few helpers of those packages' TS libraries
//! the inspector calls (`formatTokens`, `elapsedLabel`, `shortTaskId`,
//! `queuedBy`, `paneEventText`, `resolveShell`). Each is a stand-in with the
//! tests of its TS; when the owner's module lands, its function replaces the
//! stand-in.

use std::collections::HashSet;
use std::sync::LazyLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use hangar_core::board::Task;
use hangar_core::shells::ShellInfo;
use hangar_i18n::{Key, Translator, args};
use iced::window;
use serde_json::Value;

use crate::commands::CommandId;
use crate::ctx::Ctx;
use crate::shell::WindowKind;
use crate::state::live::{PaneActivity, PaneEvent, PaneInsight, PlanLimits};
use crate::state::{Model, Pane, PaneId, Workspace};

// ---- The workspace and the pane -----------------------------------------------------------

/// The workspace a window shows: its own when it is a workspace window, the
/// active one in the main window.
pub fn window_workspace(m: &Model, window: window::Id) -> Option<&Workspace> {
    match m.live.windows.get(&window) {
        Some(WindowKind::Workspace(id)) => m.workspace(id),
        _ => m.active_workspace(),
    }
}

/// The panes of a workspace in the order they are read on screen (`readingOrder`
/// of `lib/layout.ts`): the tree's leaves that name a pane, once each, then
/// the panes the tree does not mention, in their own order.
///
/// TODO(P0b): `state::layout::normalize_tree` is not ported yet; this is the
/// part of it a reading order needs, and it gives the same order.
pub fn reading_order(ws: &Workspace) -> Vec<&Pane> {
    let mut seen = HashSet::new();
    let mut order: Vec<&Pane> = Vec::with_capacity(ws.panes.len());
    if let Some(tree) = &ws.tree {
        for id in crate::state::layout::leaf_ids(tree) {
            if let Some(pane) = ws.panes.iter().find(|pane| pane.id == id)
                && seen.insert(&pane.id)
            {
                order.push(pane);
            }
        }
    }
    order.extend(ws.panes.iter().filter(|pane| !seen.contains(&pane.id)));
    order
}

/// The pane the inspector speaks about and its 1-based place in reading
/// order. The pane in focus when the workspace has one; otherwise the first,
/// which is where App's effect puts the focus ("keep focus on a pane that
/// still exists") before the inspector could be asked.
pub fn focused_pane<'a>(m: &Model, ws: &'a Workspace) -> Option<(&'a Pane, usize)> {
    let order = reading_order(ws);
    let wanted = m.ui.focus_by_workspace.get(&ws.id);
    let found = wanted.and_then(|id| order.iter().position(|pane| &pane.id == id));
    let at = found.unwrap_or(0);
    order.get(at).map(|pane| (*pane, at + 1))
}

/// A shell's name for the header: the first of the pane's, the workspace's
/// and the settings' choice that is installed, else the first detected
/// (`resolveShell`, P4's `shells`).
pub fn resolve_shell<'a>(
    shells: &'a [ShellInfo],
    preferred: &[Option<&str>],
) -> Option<&'a ShellInfo> {
    preferred
        .iter()
        .flatten()
        .filter(|id| !id.is_empty())
        .find_map(|id| shells.iter().find(|shell| shell.id == *id))
        .or_else(|| shells.first())
}

// ---- What the pane reported (P4) ----------------------------------------------------------

static NO_INSIGHT: LazyLock<PaneInsight> = LazyLock::new(PaneInsight::default);

/// What the pane knows about its agent (`usePaneInsight`): model, version,
/// context, cost, queue, edited files, history. Empty for a pane that has
/// reported nothing yet, as the TS store's default was.
///
/// TODO(P4): `m.live.panes_logic` holds the insight per pane id.
pub fn insight<'a>(m: &'a Model, pane: &PaneId) -> &'a PaneInsight {
    let _ = (m, pane);
    &NO_INSIGHT
}

/// The pane's activity and whether it comes from the bridge (`usePaneActivity`).
///
/// TODO(P4): `m.live.panes_logic` holds the activity per pane id.
pub fn activity<'a>(m: &'a Model, pane: &PaneId) -> Option<&'a PaneActivity> {
    let _ = (m, pane);
    None
}

/// The plan's rate limits, from whichever session reported last
/// (`usePlanLimits`).
///
/// TODO(P4): `m.live.panes_logic` holds the plan limits.
pub fn plan_limits(m: &Model) -> Option<&PlanLimits> {
    let _ = m;
    None
}

/// Epoch milliseconds now.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis() as i64)
        .unwrap_or(0)
}

/// The wall-clock time of a monotonic instant the insight carries
/// (`session_since`, `turn_started_at`, an event's `at`), which the inspector
/// words as a date.
///
/// TODO(P4): if the insight moves to epoch times, this goes.
pub fn wall_ms(at: Instant) -> i64 {
    now_ms() - Instant::now().saturating_duration_since(at).as_millis() as i64
}

/// Who a queued prompt is from, as shown: `None` for the person's own, the
/// sending agent's pane name, or Hangar for its own events (`queuedBy`).
pub fn queued_by(from: Option<&str>) -> Option<String> {
    match from {
        None | Some("") => None,
        Some("hangar") => Some("Hangar".into()),
        Some(name) => Some(name.to_string()),
    }
}

/// The line the inspector shows for an event of a pane's history
/// (`paneEventText`). `kind` and `params` are the TS's: the history keeps the
/// facts, never the sentence, so it is worded in the language of the moment.
///
/// TODO(P4): the typed `PaneEventDetail` replaces the JSON reading.
pub fn event_text(event: &PaneEvent, t: Translator) -> String {
    let text = |name: &str| event.params.get(name).and_then(Value::as_str);
    match event.kind.as_str() {
        "session" => {
            if event.params.get("resumed").and_then(Value::as_bool) == Some(true) {
                let id = text("id").unwrap_or_default();
                t.t_with(Key::PaneEventResumed, &args!("id" => id))
                    .into_owned()
            } else {
                t.t(Key::PaneEventNewSession).into_owned()
            }
        }
        "turn-start" => t.t(Key::PaneEventTurnStart).into_owned(),
        "turn-end" => {
            let outcome = text("outcome").unwrap_or("answer");
            if outcome == "aborted" {
                return t.t(Key::PaneEventAborted).into_owned();
            }
            // The answer's first line says more than "Finished" ever could.
            match text("answer").filter(|answer| !answer.is_empty()) {
                Some(answer) => answer.to_string(),
                None if outcome == "answer" => t.t(Key::PaneEventTurnEnd).into_owned(),
                None => t.t(Key::PaneEventFailed).into_owned(),
            }
        }
        "permission" => match text("tool").filter(|tool| !tool.is_empty()) {
            Some(tool) => t
                .t_with(Key::PaneEventPermission, &args!("tool" => tool))
                .into_owned(),
            None => t.t(Key::PaneEventPermissionAny).into_owned(),
        },
        "edit" => {
            let file = text("file").unwrap_or_default();
            match text("overlap").filter(|name| !name.is_empty()) {
                Some(name) => t
                    .t_with(
                        Key::PaneEventEditShared,
                        &args!("file" => file, "name" => name),
                    )
                    .into_owned(),
                None => t
                    .t_with(Key::PaneEventEdit, &args!("file" => file))
                    .into_owned(),
            }
        }
        "queued" => {
            let body = text("text").unwrap_or_default();
            match queued_by(text("from")) {
                Some(name) => t
                    .t_with(
                        Key::PaneEventQueuedBy,
                        &args!("name" => name.as_str(), "text" => body),
                    )
                    .into_owned(),
                None => t
                    .t_with(Key::PaneEventQueued, &args!("text" => body))
                    .into_owned(),
            }
        }
        "dropped" => t
            .t_with(
                Key::PaneEventDropped,
                &args!("text" => text("text").unwrap_or_default()),
            )
            .into_owned(),
        "exit" => {
            if text("what") == Some("agent") {
                t.t(Key::PaneEventEnd).into_owned()
            } else {
                t.t(Key::PaneEventExit).into_owned()
            }
        }
        _ => String::new(),
    }
}

/// 166234 → "166k", 1000000 → "1M": the pane header has no room for digits
/// (`formatTokens` of `lib/context.ts`, P4).
pub fn format_tokens(count: u64) -> String {
    if count >= 10_000_000 {
        format!("{}M", (count + 500_000) / 1_000_000)
    } else if count >= 1_000_000 {
        // One decimal, rounded half up on the integers (the TS's `toFixed`
        // rounded on a binary fraction, which made 1.15 M read "1.1M"), and
        // none when it would be ".0".
        let tenths = (count + 50_000) / 100_000;
        if tenths.is_multiple_of(10) {
            format!("{}M", tenths / 10)
        } else {
            format!("{}.{}M", tenths / 10, tenths % 10)
        }
    } else if count >= 1000 {
        format!("{}k", (count as f64 / 1000.0).round() as u64)
    } else {
        count.to_string()
    }
}

// ---- The board task (P12) ----------------------------------------------------------------

/// What the task card shows of a board task.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskCard {
    pub id: String,
    pub title: String,
    pub priority: u8,
    /// `doing` or `review`: the only columns a pane's task can be in.
    pub column: String,
    /// When it entered `doing`, epoch milliseconds.
    pub doing_since: Option<i64>,
    pub comments: usize,
}

impl TaskCard {
    pub fn of(task: &Task) -> TaskCard {
        TaskCard {
            id: task.id.clone(),
            title: task.title.clone(),
            priority: task.priority,
            column: task.column.clone(),
            doing_since: task
                .doing_since
                .map(|since| i64::try_from(since).unwrap_or(i64::MAX)),
            comments: task.comments.len(),
        }
    }

    pub fn in_review(&self) -> bool {
        self.column == "review"
    }
}

/// The task an agent in this pane is on, as far as the board can tell
/// (`taskForPane` over `useBoardTasks(workspace.cwd)`): an open task whose
/// assignee resolves to this pane.
///
/// TODO(P12): the boards cache is `m.live.board` and `taskForPane` is P12's
/// (`board::pane_task`); it needs P4's `find_agent_pane` for the assignee.
/// Then this is `board_tasks(m, &ws.cwd).and_then(|tasks| task_for_pane(tasks,
/// pane, workspaces, active)).map(TaskCard::of)`.
pub fn task_for_pane(m: &Model, ws: &Workspace, pane: &PaneId) -> Option<TaskCard> {
    let _ = (m, ws, pane);
    None
}

/// The short handle a card shows for its task: the first four characters of
/// the id without its dashes (`shortTaskId`, P12).
pub fn short_task_id(id: &str) -> String {
    let compact: String = id.chars().filter(|c| *c != '-').collect();
    let source = if compact.is_empty() { id } else { &compact };
    source.chars().take(4).collect()
}

/// How long something has been going on, at a glance rather than to the
/// second (`elapsedLabel`, P12).
pub fn elapsed_label(ms: i64, t: Translator) -> String {
    let minutes = ms.max(0) / 60_000;
    if minutes < 1 {
        return t.t(Key::BoardElapsedNow).into_owned();
    }
    if minutes < 60 {
        return t
            .t_with(Key::BoardElapsedMinutes, &args!("m" => minutes))
            .into_owned();
    }
    let hours = minutes / 60;
    if hours < 24 {
        let padded = format!("{:02}", minutes % 60);
        return t
            .t_with(
                Key::BoardElapsedHours,
                &args!("h" => hours, "m" => padded.as_str()),
            )
            .into_owned();
    }
    t.t_with(
        Key::BoardElapsedDays,
        &args!("d" => hours / 24, "h" => hours % 24),
    )
    .into_owned()
}

/// A task's priority as the board words it: 1 low, 2 medium, 3 high
/// (`priorityLevel`, P12).
pub fn priority_level(priority: u8) -> u8 {
    match priority {
        0 => 1,
        1 => 2,
        _ => 3,
    }
}

// ---- Shortcuts (P7) ----------------------------------------------------------------------

/// The binding of a command as the settings store it (`"Ctrl+Alt+I"`), or
/// `None` when the keymap has nothing for it.
///
/// TODO(P7): `ctx.keymap` (`KeymapIndex`) answers; it is a unit struct until
/// P7 lands, so no shortcut is shown rather than a default that a rebound
/// key would make wrong.
pub fn shortcut(ctx: Ctx<'_>, command: CommandId) -> Option<String> {
    let _ = (ctx, command);
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use hangar_i18n::Locale;
    use serde_json::json;

    use crate::state::types::{Pane, SplitNode};

    fn pane(id: &str) -> Pane {
        Pane {
            id: id.into(),
            name: id.to_uppercase(),
            ..Pane::default()
        }
    }

    fn workspace(ids: &[&str], tree: Option<SplitNode>) -> Workspace {
        Workspace {
            id: "w".into(),
            panes: ids.iter().map(|id| pane(id)).collect(),
            tree,
            ..Workspace::default()
        }
    }

    fn leaf(id: &str) -> SplitNode {
        SplitNode::Pane { id: id.into() }
    }

    fn row(a: SplitNode, b: SplitNode) -> SplitNode {
        SplitNode::Split {
            dir: crate::state::SplitDir::Row,
            ratio: 0.5,
            a: Box::new(a),
            b: Box::new(b),
        }
    }

    fn names(ws: &Workspace) -> Vec<&str> {
        reading_order(ws)
            .into_iter()
            .map(|pane| pane.id.as_str())
            .collect()
    }

    #[test]
    fn the_reading_order_follows_the_tree_not_the_spawning() {
        let ws = workspace(
            &["a", "b", "c"],
            Some(row(row(leaf("c"), leaf("a")), leaf("b"))),
        );
        assert_eq!(names(&ws), ["c", "a", "b"]);
    }

    #[test]
    fn a_stale_tree_is_repaired_keeping_what_it_knew() {
        // "x" no longer exists, "d" is not in the tree yet, "a" shows twice.
        let ws = workspace(
            &["a", "b", "d"],
            Some(row(row(leaf("b"), leaf("x")), row(leaf("a"), leaf("a")))),
        );
        assert_eq!(names(&ws), ["b", "a", "d"]);
        assert_eq!(names(&workspace(&["a", "b"], None)), ["a", "b"]);
    }

    #[test]
    fn the_focused_pane_is_found_with_its_place_and_falls_back_to_the_first() {
        let ws = workspace(
            &["a", "b", "c"],
            Some(row(row(leaf("c"), leaf("a")), leaf("b"))),
        );
        let mut m = Model::default();
        m.ui.focus_by_workspace.insert("w".into(), "a".into());
        let (found, place) = focused_pane(&m, &ws).unwrap();
        assert_eq!((found.id.as_str(), place), ("a", 2));

        m.ui.focus_by_workspace.insert("w".into(), "gone".into());
        let (found, place) = focused_pane(&m, &ws).unwrap();
        assert_eq!((found.id.as_str(), place), ("c", 1));

        assert!(focused_pane(&m, &workspace(&[], None)).is_none());
    }

    #[test]
    fn a_shell_is_resolved_from_the_most_specific_choice_that_exists() {
        let shell = |id: &str| ShellInfo {
            id: id.into(),
            label: id.to_uppercase(),
            program: String::new(),
            args: Vec::new(),
        };
        let shells = [shell("bash"), shell("zsh"), shell("fish")];
        let pick = |preferred: &[Option<&str>]| {
            resolve_shell(&shells, preferred).map(|found| found.id.as_str())
        };
        assert_eq!(pick(&[Some("zsh"), Some("fish"), None]), Some("zsh"));
        // An uninstalled shell is skipped.
        assert_eq!(pick(&[Some("nu"), None, Some("fish")]), Some("fish"));
        assert_eq!(pick(&[None, Some(""), None]), Some("bash"));
        assert_eq!(resolve_shell(&[], &[Some("zsh")]).map(|s| &s.id), None);
    }

    #[test]
    fn tokens_are_written_short() {
        assert_eq!(format_tokens(512), "512");
        assert_eq!(format_tokens(1000), "1k");
        assert_eq!(format_tokens(166_234), "166k");
        assert_eq!(format_tokens(200_000), "200k");
        assert_eq!(format_tokens(1_000_000), "1M");
        assert_eq!(format_tokens(1_250_000), "1.3M");
        assert_eq!(format_tokens(12_400_000), "12M");
    }

    #[test]
    fn a_task_handle_is_four_characters_of_the_id() {
        assert_eq!(
            short_task_id("3f2a9c1e-0b7d-4c55-9a43-2f1e8d0c6b11"),
            "3f2a"
        );
        assert_eq!(short_task_id("ab-cdef"), "abcd");
        assert_eq!(short_task_id("09"), "09");
    }

    #[test]
    fn elapsed_time_is_counted_in_minutes_then_hours_then_days() {
        let t = Translator::new(Locale::En);
        let nbsp = '\u{a0}';
        assert_eq!(elapsed_label(59_000, t), format!("<{nbsp}1{nbsp}min"));
        assert_eq!(elapsed_label(-5_000, t), format!("<{nbsp}1{nbsp}min"));
        assert_eq!(elapsed_label(24 * 60_000, t), format!("24{nbsp}min"));
        assert_eq!(elapsed_label(72 * 60_000, t), format!("1{nbsp}h{nbsp}12"));
        assert_eq!(elapsed_label(65 * 60_000, t), format!("1{nbsp}h{nbsp}05"));
        assert_eq!(
            elapsed_label(27 * 3_600_000, t),
            format!("1{nbsp}d{nbsp}3{nbsp}h")
        );
    }

    #[test]
    fn priorities_read_as_low_medium_high() {
        assert_eq!(priority_level(0), 1);
        assert_eq!(priority_level(1), 2);
        assert_eq!(priority_level(2), 3);
        assert_eq!(priority_level(9), 3);
    }

    #[test]
    fn who_queued_a_prompt_is_named_unless_it_was_the_person() {
        assert_eq!(queued_by(None), None);
        assert_eq!(queued_by(Some("")), None);
        assert_eq!(queued_by(Some("hangar")).as_deref(), Some("Hangar"));
        assert_eq!(queued_by(Some("Ava")).as_deref(), Some("Ava"));
    }

    fn event(kind: &str, params: Value) -> PaneEvent {
        PaneEvent {
            at: Instant::now(),
            kind: kind.into(),
            params,
        }
    }

    #[test]
    fn an_event_is_worded_from_its_facts_in_the_language_of_the_moment() {
        let en = Translator::new(Locale::En);
        let fr = Translator::new(Locale::Fr);
        let text = |kind, params| event_text(&event(kind, params), en);

        assert_eq!(
            text("session", json!({"id": "ab12", "resumed": true})),
            "Session ab12 resumed"
        );
        assert_eq!(
            text("session", json!({"id": "ab12", "resumed": false})),
            "New session"
        );
        assert_eq!(text("turn-start", json!({})), "Prompt received");
        assert_eq!(
            text("turn-end", json!({"outcome": "aborted", "answer": null})),
            "Interrupted"
        );
        assert_eq!(
            text("turn-end", json!({"outcome": "answer", "answer": "Done."})),
            "Done."
        );
        assert_eq!(
            text("turn-end", json!({"outcome": "answer", "answer": null})),
            "Finished"
        );
        assert_eq!(
            text("turn-end", json!({"outcome": "failed", "answer": ""})),
            "Stopped on an error"
        );
        assert_eq!(
            text("permission", json!({"tool": "Bash"})),
            "Waiting for your permission: Bash"
        );
        assert_eq!(
            text("permission", json!({"tool": null})),
            "Waiting for your permission"
        );
        assert_eq!(text("edit", json!({"file": "a.rs"})), "Edited a.rs");
        assert_eq!(
            text("edit", json!({"file": "a.rs", "overlap": "Ava"})),
            "Edited a.rs, which Ava had just edited"
        );
        assert_eq!(
            text("queued", json!({"text": "run tests"})),
            "Queued: run tests"
        );
        assert_eq!(
            text("queued", json!({"text": "run tests", "from": "hangar"})),
            "Queued by Hangar: run tests"
        );
        assert_eq!(
            text("dropped", json!({"text": "x"})),
            "Dropped from the queue: x"
        );
        assert_eq!(text("exit", json!({"what": "agent"})), "Claude Code exited");
        assert_eq!(text("exit", json!({"what": "shell"})), "The shell exited");
        // The same history, read in French.
        assert_eq!(
            event_text(&event("turn-start", json!({})), fr),
            "Prompt reçu"
        );
    }

    #[test]
    fn a_board_task_becomes_a_card() {
        let task = Task {
            id: "3f2a9c1e".into(),
            title: "Fix the login".into(),
            description: String::new(),
            column: "doing".into(),
            priority: 1,
            assignee: Some("ava".into()),
            labels: Vec::new(),
            comments: Vec::new(),
            depends_on: Vec::new(),
            created_at: 1,
            updated_at: 2,
            order: 0.0,
            doing_since: Some(1_700_000_000_000),
            owns: Vec::new(),
            attempts: 0,
            blocked: None,
        };
        let card = TaskCard::of(&task);
        assert_eq!(card.doing_since, Some(1_700_000_000_000));
        assert!(!card.in_review());
        assert_eq!(card.comments, 0);
    }
}
