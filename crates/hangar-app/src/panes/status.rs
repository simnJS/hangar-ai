//! The pure half of a pane header (`components/PaneStatus.ts`, tests in
//! PaneStatus.test.ts): what its glyph, its state word and its gauge say, and
//! how the pane words what it reports. Kept apart from the views so it can be
//! tested without a terminal.

use hangar_i18n::Key;

use crate::state::live::QueuedPrompt;
use crate::state::types::ShellInfo;
use crate::state::{AgentActivity, GlyphState};

/// The shell's own life: what a pane is before an agent has anything to say
/// (`PaneStatus`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum PaneStatus {
    #[default]
    Starting,
    Running,
    Exited,
}

/// The shell's own life until it runs; then, for a pane that reports one, the
/// agent's state. A plain shell has nothing to report and rests as idle
/// (`paneGlyphState`).
pub fn pane_glyph_state(status: PaneStatus, activity: Option<AgentActivity>) -> GlyphState {
    match status {
        PaneStatus::Starting => GlyphState::Starting,
        PaneStatus::Exited => GlyphState::Exited,
        PaneStatus::Running => match activity.unwrap_or(AgentActivity::Idle) {
            AgentActivity::Working => GlyphState::Working,
            AgentActivity::Waiting => GlyphState::Waiting,
            AgentActivity::Yours => GlyphState::Yours,
            AgentActivity::Idle => GlyphState::Idle,
        },
    }
}

/// Where a context window starts to deserve a look, then a decision.
pub const GAUGE_WARN: f64 = 70.0;
pub const GAUGE_HOT: f64 = 90.0;

/// How worrying a gauge's reading is (`gaugeLevel`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GaugeLevel {
    Ok,
    Warn,
    Hot,
}

pub fn gauge_level(pct: f64) -> GaugeLevel {
    if pct >= GAUGE_HOT {
        GaugeLevel::Hot
    } else if pct >= GAUGE_WARN {
        GaugeLevel::Warn
    } else {
        GaugeLevel::Ok
    }
}

/// A shell as the header names it: the program, without its folder or its
/// `.exe` (`pwsh`, `bash`, `wsl`), which is what a terminal user calls it and
/// fits the mono metadata of a narrow pane (`shellShortName`).
pub fn shell_short_name(shell: Option<&ShellInfo>) -> Option<String> {
    let shell = shell?;
    let file = shell.program.rsplit(['\\', '/']).next().unwrap_or_default();
    let lower = file.to_lowercase();
    let name = [".exe", ".cmd", ".bat", ".com"]
        .iter()
        .find_map(|ext| lower.strip_suffix(ext))
        .unwrap_or(&lower)
        .to_owned();
    Some(if name.is_empty() {
        shell.label.clone()
    } else {
        name
    })
}

/// Session ids are long; six characters tell two apart on screen.
pub fn short_session_id(id: &str) -> &str {
    match id.char_indices().nth(6) {
        Some((end, _)) => &id[..end],
        None => id,
    }
}

/// The short id, with an ellipsis saying there is more of it.
pub fn session_id_label(id: &str) -> String {
    let short = short_session_id(id);
    if short.len() < id.len() {
        format!("{short}…")
    } else {
        id.to_owned()
    }
}

/// The first line with something on it, trimmed: an event is one line.
pub fn first_line(text: &str, max: usize) -> String {
    let line = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .find(|line| !line.trim().is_empty())
        .map(str::trim)
        .unwrap_or_default();
    if line.chars().count() > max {
        let kept: String = line.chars().take(max.saturating_sub(1)).collect();
        format!("{kept}…")
    } else {
        line.to_owned()
    }
}

/// `src/lib/layout.ts` → `layout.ts`, whichever separator the agent used.
pub fn file_base_name(path: &str) -> &str {
    let trimmed = path.trim_end_matches(['\\', '/']);
    match trimmed.rsplit(['\\', '/']).next() {
        Some(name) if !name.is_empty() => name,
        _ => trimmed,
    }
}

/// The prompts that joined the queue since it last changed, oldest first.
pub fn newly_queued<'a>(
    before: &[QueuedPrompt],
    after: &'a [QueuedPrompt],
) -> Vec<&'a QueuedPrompt> {
    after
        .iter()
        .filter(|entry| !before.iter().any(|known| known.id == entry.id))
        .collect()
}

/// The key of the state word: `pane.state.<status>` until the shell runs,
/// then the agent's activity for a pane that reports one, else `running`.
pub fn state_key(status: PaneStatus, activity: Option<AgentActivity>) -> Key {
    match (status, activity) {
        (PaneStatus::Starting, _) => Key::PaneStateStarting,
        (PaneStatus::Exited, _) => Key::PaneStateExited,
        (PaneStatus::Running, None) => Key::PaneStateRunning,
        (PaneStatus::Running, Some(AgentActivity::Working)) => Key::PaneStateWorking,
        (PaneStatus::Running, Some(AgentActivity::Waiting)) => Key::PaneStateWaiting,
        (PaneStatus::Running, Some(AgentActivity::Yours)) => Key::PaneStateYours,
        (PaneStatus::Running, Some(AgentActivity::Idle)) => Key::PaneStateIdle,
    }
}

/// The word of an activity alone, for the zoom bar's chips.
pub fn activity_key(activity: AgentActivity) -> Key {
    state_key(PaneStatus::Running, Some(activity))
}

/// hangar-ui's glyph for the app's.
pub fn ui_glyph(state: GlyphState) -> hangar_ui::glyph::GlyphState {
    use hangar_ui::glyph::GlyphState as Ui;
    match state {
        GlyphState::Starting => Ui::Starting,
        GlyphState::Exited => Ui::Exited,
        GlyphState::Working => Ui::Working,
        GlyphState::Waiting => Ui::Waiting,
        GlyphState::Yours => Ui::Yours,
        GlyphState::Idle => Ui::Idle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queued(id: &str, text: &str) -> QueuedPrompt {
        QueuedPrompt {
            id: id.into(),
            text: text.into(),
            from: None,
        }
    }

    #[test]
    fn shows_the_shells_life_before_the_agents_state() {
        assert_eq!(
            pane_glyph_state(PaneStatus::Starting, Some(AgentActivity::Working)),
            GlyphState::Starting
        );
        assert_eq!(
            pane_glyph_state(PaneStatus::Exited, Some(AgentActivity::Waiting)),
            GlyphState::Exited
        );
        assert_eq!(
            pane_glyph_state(PaneStatus::Running, Some(AgentActivity::Waiting)),
            GlyphState::Waiting
        );
        // A plain shell reports nothing and rests as idle.
        assert_eq!(
            pane_glyph_state(PaneStatus::Running, None),
            GlyphState::Idle
        );
    }

    #[test]
    fn warms_the_gauge_from_70_and_heats_it_from_90() {
        assert_eq!(gauge_level(0.0), GaugeLevel::Ok);
        assert_eq!(gauge_level(69.0), GaugeLevel::Ok);
        assert_eq!(gauge_level(70.0), GaugeLevel::Warn);
        assert_eq!(gauge_level(89.0), GaugeLevel::Warn);
        assert_eq!(gauge_level(90.0), GaugeLevel::Hot);
        assert_eq!(gauge_level(100.0), GaugeLevel::Hot);
    }

    #[test]
    fn names_a_shell_after_its_program() {
        let shell = |program: &str| ShellInfo {
            id: "x".into(),
            label: "Label".into(),
            program: program.into(),
            args: vec![],
        };
        assert_eq!(
            shell_short_name(Some(&shell("pwsh.exe"))).as_deref(),
            Some("pwsh")
        );
        assert_eq!(
            shell_short_name(Some(&shell("C:\\Program Files\\Git\\bin\\bash.exe"))).as_deref(),
            Some("bash")
        );
        assert_eq!(
            shell_short_name(Some(&shell("/bin/zsh"))).as_deref(),
            Some("zsh")
        );
        assert_eq!(
            shell_short_name(Some(&shell("CMD.EXE"))).as_deref(),
            Some("cmd")
        );
        assert_eq!(shell_short_name(Some(&shell(""))).as_deref(), Some("Label"));
        assert_eq!(shell_short_name(None), None);
    }

    #[test]
    fn shortens_session_ids() {
        assert_eq!(short_session_id("01J8Z4-pty-bridge"), "01J8Z4");
        assert_eq!(short_session_id("abc"), "abc");
        assert_eq!(session_id_label("01J8Z4-pty-bridge"), "01J8Z4…");
        assert_eq!(session_id_label("abc"), "abc");
    }

    #[test]
    fn keeps_the_first_line_that_says_something() {
        assert_eq!(
            first_line("\n  \n  Done: tests pass\nmore", 120),
            "Done: tests pass"
        );
        assert_eq!(first_line("", 120), "");
        assert_eq!(first_line(&"x".repeat(10), 5), "xxxx…");
    }

    #[test]
    fn takes_a_files_name_off_either_kind_of_path() {
        assert_eq!(file_base_name("src/lib/layout.ts"), "layout.ts");
        assert_eq!(file_base_name("C:\\dev\\app\\main.rs"), "main.rs");
        assert_eq!(file_base_name("README.md"), "README.md");
        assert_eq!(file_base_name("src/board/"), "board");
    }

    #[test]
    fn finds_the_prompts_that_joined_the_queue() {
        let a = queued("1", "a");
        let b = queued("2", "b");
        let c = queued("3", "c");
        let ids =
            |list: Vec<&QueuedPrompt>| list.into_iter().map(|q| q.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(newly_queued(&[], &[a.clone(), b.clone()])), ["1", "2"]);
        assert_eq!(
            ids(newly_queued(
                &[a.clone(), b.clone()],
                &[b.clone(), c.clone()]
            )),
            ["3"]
        );
        assert!(newly_queued(&[a, b.clone()], &[b]).is_empty());
    }

    #[test]
    fn words_the_state_as_the_header_did() {
        assert_eq!(
            state_key(PaneStatus::Starting, Some(AgentActivity::Working)),
            Key::PaneStateStarting
        );
        assert_eq!(state_key(PaneStatus::Running, None), Key::PaneStateRunning);
        assert_eq!(
            state_key(PaneStatus::Running, Some(AgentActivity::Yours)),
            Key::PaneStateYours
        );
    }
}
