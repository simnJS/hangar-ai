//! How a pane's terminal looks and behaves, from the settings and the
//! workspace's theme (`new Terminal({...})` of TerminalPane.tsx, inventory
//! §4/1.1–1.2). A change of either restyles the terminals in place, as the
//! TS effect on `[theme, fontFamily, …]` did: nothing restarts.

use hangar_term::{Config, CursorShape, Font, Osc52, Palette};
use hangar_theme::Theme;

use crate::state::types::{AgentId, CursorStyle, Settings, Workspace};

/// Everything a terminal is set up with, compared to know when to restyle.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    pub palette: Palette,
    pub font: Font,
    pub config: Config,
    /// Around the grid (`settings.padding`, 10 px by default).
    pub padding: f32,
}

/// The theme a workspace's terminals are drawn in: its own, else the
/// settings' (`getTheme(workspace.themeId ?? settings.themeId)`). Hidden
/// workspaces keep theirs: their terminals stay styled the way their owner
/// set them.
pub fn theme_of(settings: &Settings, ws: &Workspace) -> &'static Theme {
    hangar_theme::by_id_or_default(ws.theme_id.as_deref().unwrap_or(&settings.theme_id))
}

/// The terminal colours of a theme, with its search marks: matches in the
/// theme's own yellow, the current one in its accent, both laid over the
/// background (`decorationsFor` of TerminalSearch.tsx).
pub fn palette(theme: &Theme) -> Palette {
    let terminal = &theme.terminal;
    let search = theme.search_colors();
    Palette {
        foreground: terminal.foreground,
        background: terminal.background,
        cursor: terminal.cursor,
        cursor_text: terminal.cursor_text,
        selection_background: terminal.selection,
        ansi: terminal.ansi,
        indexed: None,
        search_match: search.match_background,
        search_active: search.active_background,
    }
}

/// The behaviour of a pane's terminal.
///
/// - Shift+Enter opens a line instead of sending one in an agent pane: ESC+CR
///   is what the CLIs read as a newline. A shell keeps it as Enter, since ESC
///   would revert the line in PSReadLine or cmd.
/// - Ctrl+V pastes, as it does everywhere else on the machine; readline's
///   quoted-insert is lost on purpose, the bargain Windows Terminal makes.
/// - Programs may neither write nor read the clipboard (`OSC 52`). xterm.js
///   without its clipboard addon ignored the sequence, and an agent, or
///   anything it runs, putting text on the clipboard unseen, for the person
///   to paste into another shell, is not worth the convenience. The person
///   copies with the selection, as before. Offering it back would take a
///   setting, which the TS never had.
pub fn config(settings: &Settings, agent: AgentId) -> Config {
    Config {
        osc52: Osc52::Disabled,
        scrollback: usize::try_from(settings.scrollback).unwrap_or(usize::MAX),
        cursor_shape: match settings.cursor_style {
            CursorStyle::Block => CursorShape::Block,
            CursorStyle::Bar => CursorShape::Beam,
            CursorStyle::Underline => CursorShape::Underline,
        },
        cursor_blink: settings.cursor_blink,
        shift_enter: (agent != AgentId::Shell).then(|| b"\x1b\r".to_vec()),
        ctrl_v_pastes: true,
        // xterm.js copied nothing on select and flashed nothing on a bell;
        // the bell only settles the activity watcher.
        copy_on_select: false,
        visual_bell: false,
        ..Config::default()
    }
}

/// Font, size and line height as the settings write them. Letter spacing has
/// no counterpart in hangar-term yet (its "Not done" list).
pub fn font(settings: &Settings) -> Font {
    let size = if settings.font_size.is_finite() && settings.font_size > 0.0 {
        settings.font_size as f32
    } else {
        13.0
    };
    let line_height = if settings.line_height.is_finite() && settings.line_height > 0.0 {
        settings.line_height as f32
    } else {
        1.3
    };
    Font::from_css_stack(&settings.font_family, size, line_height)
}

pub fn look(settings: &Settings, ws: &Workspace, agent: AgentId) -> Look {
    Look {
        palette: palette(theme_of(settings, ws)),
        font: font(settings),
        config: config(settings, agent),
        padding: if settings.padding.is_finite() {
            settings.padding.clamp(0.0, 64.0) as f32
        } else {
            10.0
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_agent_panes_turn_shift_enter_into_a_newline() {
        let settings = Settings::default();
        assert_eq!(config(&settings, AgentId::Shell).shift_enter, None);
        assert_eq!(
            config(&settings, AgentId::Claude).shift_enter.as_deref(),
            Some(&b"\x1b\r"[..])
        );
        assert!(config(&settings, AgentId::Shell).ctrl_v_pastes);
    }

    #[test]
    fn a_program_cannot_reach_the_clipboard() {
        use hangar_term::{Event, Terminal};
        for agent in [AgentId::Shell, AgentId::Claude] {
            let config = config(&Settings::default(), agent);
            assert_eq!(config.osc52, Osc52::Disabled);
            let mut terminal = Terminal::new(config, 80, 24);
            // Copy "hello", then ask for what the clipboard holds.
            let events = terminal.feed(b"\x1b]52;c;aGVsbG8=\x07\x1b]52;c;?\x07");
            assert!(
                !events.iter().any(|event| matches!(
                    event,
                    Event::SetClipboard { .. } | Event::ClipboardLoad { .. }
                )),
                "{events:?}"
            );
        }
        // What the same bytes do in a terminal that allows it, so the test
        // above is not passing for want of a well-formed sequence.
        let mut open = Terminal::new(Config::default(), 80, 24);
        let events = open.feed(b"\x1b]52;c;aGVsbG8=\x07");
        assert!(
            events
                .iter()
                .any(|event| matches!(event, Event::SetClipboard { text, .. } if text == "hello")),
            "{events:?}"
        );
    }

    #[test]
    fn the_default_cursor_is_a_bar() {
        let settings = Settings::default();
        assert_eq!(
            config(&settings, AgentId::Shell).cursor_shape,
            CursorShape::Beam
        );
        assert!(config(&settings, AgentId::Shell).cursor_blink);
    }

    #[test]
    fn a_workspace_theme_wins_over_the_settings() {
        let settings = Settings::default();
        let ws = Workspace {
            theme_id: Some("tokyo-night".into()),
            ..Workspace::default()
        };
        assert_eq!(
            theme_of(&settings, &ws).id,
            hangar_theme::by_id_or_default("tokyo-night").id
        );
        let plain = Workspace::default();
        assert_eq!(
            theme_of(&settings, &plain).id,
            hangar_theme::by_id_or_default(&settings.theme_id).id
        );
    }

    #[test]
    fn the_font_follows_the_settings_stack() {
        let font = font(&Settings::default());
        assert_eq!(
            font.families.first().map(String::as_str),
            Some("Cascadia Code")
        );
        assert_eq!(font.size, 13.0);
    }
}
