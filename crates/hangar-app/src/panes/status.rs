//! The pure half of a pane header (`components/PaneStatus.ts`, tests in
//! PaneStatus.test.ts): what its glyph, its state word and its gauge say.
//!
//! The module's text helpers (`firstLine`, `fileBaseName`, `shortSessionId`,
//! `sessionIdLabel`, `newlyQueued`) word what a pane reports to its
//! insight, which is P4's: they are in `panes_logic::bridge_events`, with
//! their cases. `gaugeLevel` (amber from 70 %, red from 90 %) is the ring's
//! own, hangar-ui's `glyph::gauge_level`, with the "warms the gauge" case.
//! What the header itself draws with is here.

use hangar_i18n::Key;

use crate::state::live::PaneStatus;
use crate::state::types::ShellInfo;
use crate::state::{AgentActivity, GlyphState};

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
        use hangar_ui::glyph::{GaugeLevel, gauge_level};
        assert_eq!(gauge_level(0), GaugeLevel::Ok);
        assert_eq!(gauge_level(69), GaugeLevel::Ok);
        assert_eq!(gauge_level(70), GaugeLevel::Warn);
        assert_eq!(gauge_level(89), GaugeLevel::Warn);
        assert_eq!(gauge_level(90), GaugeLevel::Hot);
        assert_eq!(gauge_level(100), GaugeLevel::Hot);
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
