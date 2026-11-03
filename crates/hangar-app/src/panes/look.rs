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
/// - The kitty keyboard protocol is off. xterm.js 6.0 has none, so no program
///   in the old terminal ever got to ask for it: Claude Code sends the query
///   (`CSI ? u`, then `CSI c` to learn whether anyone answered) and, with no
///   answer, reads the classic encoding, which the tests in hangar-term's
///   `xterm_keys.rs` pin to xterm.js's own. Answering it would hand every
///   agent that supports the protocol a different set of bytes for Escape,
///   Enter with a modifier, Ctrl+letter and the rest than the ones they were
///   used with here, for a gain (telling Shift+Enter from Enter) that
///   `shift_enter` already gives agent panes. Turning it on would be a
///   setting of its own.
pub fn config(settings: &Settings, agent: AgentId) -> Config {
    Config {
        osc52: Osc52::Disabled,
        kitty_keyboard: false,
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

/// Font, size, line height and letter spacing as the settings write them.
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
    // `letterSpacing` went to xterm.js as it stood, a number of pixels (the
    // slider offers -1 to 3 in halves); hangar-term takes care of a value
    // that is not one.
    Font::from_css_stack(&settings.font_family, size, line_height)
        .with_letter_spacing(settings.letter_spacing as f32)
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
    fn a_program_that_asks_for_the_kitty_protocol_gets_the_classic_keys_as_under_xterm_js() {
        use hangar_term::{
            Event, KeyEventKind, KeyOptions, KeyPress, TermMode, Terminal, encode_key,
        };
        use iced::keyboard::key::{Named, NativeCode, Physical};
        use iced::keyboard::{Key, Location, Modifiers};

        let press = |key: Key, modifiers: Modifiers| KeyPress {
            modified_key: key.clone(),
            key,
            physical_key: Physical::Unidentified(NativeCode::Unidentified),
            location: Location::Standard,
            modifiers,
            text: None,
            kind: KeyEventKind::Press,
        };
        for agent in [AgentId::Shell, AgentId::Claude] {
            let config = config(&Settings::default(), agent);
            let options = KeyOptions {
                alt_sends_escape: config.alt_sends_escape,
                shift_enter: config.shift_enter.clone(),
            };
            let mut terminal = Terminal::new(config, 80, 24);
            // What Claude Code does at start, then what a program does once it
            // has an answer: ask for the flags, and push the first ones.
            let events = terminal.feed(b"\x1b[?u\x1b[>1u\x1b[>31u\x1b[c");
            let answers: Vec<&[u8]> = events
                .iter()
                .filter_map(|event| match event {
                    Event::Write(bytes) => Some(bytes.as_slice()),
                    _ => None,
                })
                .collect();
            // Only the device attributes are answered; xterm.js answered no
            // more, so the program concludes there is no protocol.
            assert_eq!(answers.len(), 1, "{agent:?}: {answers:?}");
            assert!(answers[0].ends_with(b"c"), "{agent:?}: {answers:?}");
            assert!(
                !terminal
                    .mode()
                    .intersects(TermMode::KITTY_KEYBOARD_PROTOCOL),
                "{agent:?}"
            );

            // So the keys are the classic ones: Escape is one byte, Ctrl+C
            // the interrupt, and Shift+Enter what the agent panes bind it to.
            let bytes = |key: Key, modifiers: Modifiers| {
                encode_key(&press(key, modifiers), terminal.mode(), &options)
            };
            assert_eq!(
                bytes(Key::Named(Named::Escape), Modifiers::empty()),
                Some(b"\x1b".to_vec())
            );
            assert_eq!(
                bytes(Key::Named(Named::Enter), Modifiers::empty()),
                Some(b"\r".to_vec())
            );
            let mut ctrl_c = press(Key::Character("c".into()), Modifiers::CTRL);
            ctrl_c.text = Some("\u{3}".into());
            assert_eq!(
                encode_key(&ctrl_c, terminal.mode(), &options),
                Some(b"\x03".to_vec())
            );
            let shift_enter = bytes(Key::Named(Named::Enter), Modifiers::SHIFT);
            let expected = (agent != AgentId::Shell).then(|| b"\x1b\r".to_vec());
            assert_eq!(shift_enter, expected.or(Some(b"\r".to_vec())), "{agent:?}");
        }
        // The same bytes, in a terminal that does offer the protocol: the
        // program is answered and the keys become `CSI u`. This is the
        // difference the setting above avoids.
        let mut open = Terminal::new(Config::default(), 80, 24);
        open.feed(b"\x1b[>1u");
        assert!(open.mode().intersects(TermMode::KITTY_KEYBOARD_PROTOCOL));
    }

    #[test]
    fn selected_text_keeps_its_colour_and_stays_readable_in_every_theme() {
        // No theme sets `selectionForeground`, as in the old terminal, so the
        // text under a selection is drawn in the colour it had: the palette
        // carries no colour for it, and the selection is the theme's
        // background colour for selected text.
        let rgb = |color: iced::Color| {
            let [r, g, b, _] = color.into_rgba8();
            hangar_theme::Rgb(r, g, b)
        };
        let mut worst = (f64::MAX, "");
        for theme in hangar_theme::themes() {
            let palette = palette(theme);
            assert_eq!(palette.selection_background, theme.terminal.selection);
            let ratio = hangar_theme::contrast_ratio(
                rgb(palette.foreground),
                rgb(palette.selection_background),
            );
            if ratio < worst.0 {
                worst = (ratio, theme.id);
            }
        }
        // WCAG's floor for large text; xterm.js made no promise, and the
        // dimmest pairs of the 33 themes are above it.
        assert!(worst.0 >= 3.0, "{worst:?}");
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
    fn the_font_carries_the_letter_spacing_of_the_settings() {
        let mut settings = Settings::default();
        assert_eq!(font(&settings).letter_spacing, 0.0);
        settings.letter_spacing = 1.5;
        assert_eq!(font(&settings).letter_spacing, 1.5);
        settings.letter_spacing = -1.0;
        assert_eq!(font(&settings).letter_spacing, -1.0);
        // A hand-edited file may hold anything; the terminal stays usable.
        settings.letter_spacing = f64::NAN;
        assert_eq!(font(&settings).letter_spacing, 0.0);
        settings.letter_spacing = 1e9;
        assert_eq!(font(&settings).letter_spacing, 13.0);
        // And the look, which is compared to know when to restyle, changes
        // with it: no pane would be restyled otherwise.
        let ws = Workspace::default();
        let before = look(&Settings::default(), &ws, AgentId::Shell);
        settings.letter_spacing = 2.0;
        assert_ne!(look(&settings, &ws, AgentId::Shell), before);
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
