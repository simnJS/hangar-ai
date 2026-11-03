//! The small wording helpers every text of the engine is written with:
//! `oneLine`, `clip`, `flat`, `taskRef`, `clock` and their kin, which the TS
//! repeated in orchestratorEvents.ts, orchestratorBrief.ts and projects.ts.
//!
//! Lengths are counted in characters where the TS counted UTF-16 units. The
//! two agree on every character of the Basic Multilingual Plane (titles,
//! dashes, accents); only an emoji or a rarer script is counted once here and
//! twice there, which moves a cut by a character and changes nothing a model
//! reads.

use chrono::TimeZone;

/// Longest line of free text an event carries (`MAX_TEXT`).
pub const MAX_TEXT: usize = 280;

/// The first `max - 1` characters and an ellipsis, when `text` is longer
/// than `max`.
fn cut(text: &str, max: usize) -> String {
    if text.chars().count() > max {
        let kept: String = text.chars().take(max.saturating_sub(1)).collect();
        format!("{kept}…")
    } else {
        text.to_string()
    }
}

/// Every line break, with the whitespace around it, as " / " (`\s*\n\s*`).
/// The pattern's first `\s*` is greedy, so a run of whitespace holding one
/// line break or several (a blank line) becomes a single " / ".
fn join_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut run = String::new();
    let flush = |out: &mut String, run: &mut String| {
        if run.contains('\n') {
            out.push_str(" / ");
        } else {
            out.push_str(run);
        }
        run.clear();
    };
    for ch in text.chars() {
        if ch.is_whitespace() {
            run.push(ch);
        } else {
            flush(&mut out, &mut run);
            out.push(ch);
        }
    }
    flush(&mut out, &mut run);
    out
}

/// Every run of whitespace as one space (`\s+` → " ").
fn squeeze(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            space = true;
        } else {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.push(ch);
        }
    }
    if space {
        out.push(' ');
    }
    out
}

/// The text on one line, its whitespace folded, cut at `max` (`oneLine`).
pub fn one_line(text: &str, max: usize) -> String {
    let flat = squeeze(&join_lines(text));
    cut(flat.trim(), max)
}

/// Trimmed and cut at `max` (`clip`).
pub fn clip(text: &str, max: usize) -> String {
    cut(text.trim(), max)
}

/// Its lines joined with " / ", then clipped (`flat`).
pub fn flat(text: &str, max: usize) -> String {
    clip(&join_lines(text), max)
}

/// `"Title" [id]`: the title for the reader, the id for the next tool call
/// (`taskRef`).
pub fn task_ref(title: Option<&str>, id: &str) -> String {
    match title {
        Some(title) if !title.is_empty() => format!("\"{}\" [{id}]", one_line(title, 120)),
        _ => format!("[{id}]"),
    }
}

/// Two names the person would call the same: case and outer spaces aside.
pub fn same_name(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// Whether a comment is signed by Hangar itself (`/^hangar\b/i`): "hangar",
/// "Hangar", "hangar:delivery", but not "hangarbot".
pub fn is_hangar_author(author: &str) -> bool {
    let Some(head) = author.get(..6) else {
        return false;
    };
    head.eq_ignore_ascii_case("hangar")
        && author[6..]
            .chars()
            .next()
            .is_none_or(|next| !(next.is_ascii_alphanumeric() || next == '_'))
}

/// A duration as whole minutes, never under one (`minutes`).
pub fn minutes(ms: u64) -> String {
    format!("{} min", round_div(ms, 60_000).max(1))
}

/// `ms / unit`, rounded half up as `Math.round` does for positive numbers.
pub fn round_div(ms: u64, unit: u64) -> u64 {
    (ms + unit / 2) / unit
}

/// How long ago, in the briefing's words (`ago`).
pub fn ago(ms: i64) -> String {
    let min = (ms as f64 / 60_000.0).round() as i64;
    if min < 1 {
        return "just now".into();
    }
    if min < 90 {
        return format!("{min} min ago");
    }
    let hours = (min as f64 / 60.0).round() as i64;
    if hours < 36 {
        format!("{hours} h ago")
    } else {
        format!("{} days ago", (hours as f64 / 24.0).round() as i64)
    }
}

/// Local wall time, `HH:MM`: a model is told the time the text was written at
/// (`clock`).
pub fn clock(at: u64) -> String {
    match chrono::Local.timestamp_millis_opt(at as i64) {
        chrono::LocalResult::Single(time) | chrono::LocalResult::Ambiguous(time, _) => {
            time.format("%H:%M").to_string()
        }
        chrono::LocalResult::None => "--:--".into(),
    }
}

/// Milliseconds since the epoch, the engine's clock.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_line_joins_lines_and_folds_spaces() {
        assert_eq!(
            one_line("Done.\nRan pnpm test: 12 passed", MAX_TEXT),
            "Done. / Ran pnpm test: 12 passed"
        );
        assert_eq!(one_line("  a  \n\n  b   c ", MAX_TEXT), "a / b c");
        assert_eq!(one_line("abcdef", 4), "abc…");
        assert_eq!(
            flat("Form done,\ntests next", 240),
            "Form done, / tests next"
        );
    }

    #[test]
    fn hangar_signs_its_own_comments() {
        assert!(is_hangar_author("hangar"));
        assert!(is_hangar_author("Hangar: delivery"));
        assert!(!is_hangar_author("hangarbot"));
        assert!(!is_hangar_author("worker-1"));
    }

    #[test]
    fn durations_round_as_the_ts_did() {
        assert_eq!(minutes(185_000), "3 min");
        assert_eq!(minutes(10_000), "1 min");
        assert_eq!(ago(12 * 60_000), "12 min ago");
        assert_eq!(ago(10_000), "just now");
        assert_eq!(ago(3 * 3_600_000), "3 h ago");
        assert_eq!(ago(72 * 3_600_000), "3 days ago");
    }
}
