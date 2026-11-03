//! Text fields (`.set-input`, `.text-input`, `.palette__input`). Use as
//! `text_input("..", &value).style(style::text_input::field(t))`.

use iced::widget::text_input::{Status, Style};
use iced::{Background, Border, Color};

use super::{faded, faded_border, hairline, rounded};
use crate::Theme;
use crate::tokens::radius;

/// A field in the settings or a dialog: the page's colour, a hairline, and
/// the accent around it while it has the keyboard.
pub fn field(t: &Theme) -> impl Fn(&iced::Theme, Status) -> Style + '_ {
    move |_, status| {
        let c = &t.colors;
        let border = |color: Color| hairline(color, radius::SM);
        let base = |border: Border| Style {
            background: Background::Color(c.bg),
            border,
            icon: c.fg_muted,
            placeholder: c.fg_subtle,
            value: c.fg,
            selection: c.accent_ring,
        };
        match status {
            Status::Active => base(border(c.line_strong)),
            Status::Hovered => base(border(c.border_strong)),
            // The web app also drew a 3 px halo of `accent_soft` here; the
            // accent border has to do alone.
            Status::Focused { .. } => base(border(c.accent)),
            Status::Disabled => {
                let style = base(border(c.line_strong));
                Style {
                    background: Background::Color(faded(c.bg)),
                    border: faded_border(style.border),
                    icon: faded(style.icon),
                    placeholder: faded(style.placeholder),
                    value: faded(style.value),
                    ..style
                }
            }
        }
    }
}

/// A field whose value is refused: the same field with the danger colour for
/// its edge, kept while it has the keyboard so the error stays in view.
pub fn field_invalid(t: &Theme) -> impl Fn(&iced::Theme, Status) -> Style + '_ {
    move |theme, status| {
        let style = field(t)(theme, status);
        match status {
            Status::Disabled => style,
            _ => Style {
                border: hairline(t.colors.danger, radius::SM),
                ..style
            },
        }
    }
}

/// A field with no box of its own, for one that sits inside another surface:
/// the command palette's search, the status bar's broadcast field.
pub fn bare(t: &Theme) -> impl Fn(&iced::Theme, Status) -> Style + '_ {
    move |_, status| {
        let c = &t.colors;
        let value = match status {
            Status::Disabled => faded(c.fg),
            _ => c.fg,
        };
        Style {
            background: Background::Color(Color::TRANSPARENT),
            border: rounded(0.0),
            icon: c.fg_muted,
            placeholder: c.fg_secondary,
            value,
            selection: c.accent_ring,
        }
    }
}
