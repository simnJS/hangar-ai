//! The words a delivery leaves on its task's board, shared with whoever reads
//! them back: P20's `remote_delivery` parses these comments into the phone's
//! delivery view, and boards on disk already carry them. Keep them byte for
//! byte as they are (inventory §5/1.4, "Coupling to keep").

use hangar_core::delivery::Outcome;

/// What the comments and the messages are signed with (bridge.rs
/// `Queued.from`).
pub const HANGAR: &str = "hangar";
/// How the board's comments count the failed deliveries: they outlive the app.
pub const FAILED: &str = "Delivery failed";
/// What core says when the account behind the review is out of quota.
pub const USAGE_LIMIT: &str = "has reached its usage limit";
/// A review the account's quota stopped: no failed delivery.
pub const REVIEW_STOPPED: &str = "Review not run (usage limit)";
/// The line that starts with what the worker said of its delivery.
pub const SUMMARY: &str = "Summary: ";
/// The line naming what the review accepted outside the task's files.
pub const OUTSIDE: &str = "Changed outside the files it owns, accepted by the review: ";
/// The line of a delivery the person landed as it waited.
pub const AS_TESTED: &str = "Landed as it was checked and reviewed when it was submitted.";

/// A commit, as the comments name it: seven characters, `?` for none.
pub fn short(sha: Option<&str>) -> String {
    match sha {
        Some(sha) if !sha.is_empty() => sha.chars().take(7).collect(),
        _ => "?".into(),
    }
}

/// `Passed: waits for the person to land it on <target> (tested as <merge>).`
pub fn ready_head(target: &str, merge: Option<&str>) -> String {
    format!(
        "Passed: waits for the person to land it on {target} (tested as {}).",
        short(merge)
    )
}

/// The same, when the person's own checkout is in the way.
pub fn dirty_head(target: &str, files: &[String], merge: Option<&str>) -> String {
    let files = if files.is_empty() {
        String::new()
    } else {
        format!(" ({})", files.join(", "))
    };
    format!(
        "Passed, but {target}'s checkout has local changes in the way{files}: waits for the person to \
         land it once they are committed or set aside (tested as {}).",
        short(merge)
    )
}

/// What the check said (`checkLine`).
pub fn check_line(outcome: &Outcome, check_command: Option<&str>) -> String {
    let Some(check) = &outcome.check else {
        return match check_command {
            Some(command) => format!("Check: `{command}` not run."),
            None => "Check: none set for this workspace.".into(),
        };
    };
    if check.passed {
        format!("Check: `{}` passed in {} s.", check.command, check.seconds)
    } else if check.timed_out {
        format!(
            "Check: `{}` timed out after {} s.",
            check.command, check.seconds
        )
    } else {
        let exit = check
            .exit_code
            .map_or_else(|| "?".to_string(), |code| code.to_string());
        format!("Check: `{}` failed (exit {exit}).", check.command)
    }
}

/// What the review said (`reviewLine`), when one ran.
pub fn review_line(outcome: &Outcome, model: &str) -> Option<String> {
    let review = outcome.review.as_ref()?;
    let notes = review.notes.trim();
    let verdict = if review.approve && review.blocking.is_empty() {
        "approved"
    } else {
        "changes requested"
    };
    let notes = if notes.is_empty() {
        String::new()
    } else {
        format!(" Notes: {notes}")
    };
    Some(format!("Review ({model}): {verdict}.{notes}"))
}

/// `Delivery failed (<outcome>, <n>): <detail>`, the task's n-th failed
/// delivery. Comments written while a task could fail a set number of times
/// say `<n> of <max>`; P20's `FAILED` reads both.
pub fn failed_comment(outcome: &str, failures: usize, detail: &str) -> String {
    format!("{FAILED} ({outcome}, {failures}): {detail}")
}
