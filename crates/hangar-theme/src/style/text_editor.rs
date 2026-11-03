//! The multi-line editor (`textarea` in the web app's forms): a text field's
//! look over several lines.

use iced::widget::text_editor::{Status, Style};
use iced::{Background, Color};

use super::{faded, faded_border, hairline};
use crate::Theme;
use crate::tokens::radius;

pub fn field(t: &Theme) -> impl Fn(&iced::Theme, Status) -> Style + '_ {
    move |_, status| {
        let c = &t.colors;
        let edge = |color: Color| hairline(color, radius::SM);
        let base = |border| Style {
            background: Background::Color(c.bg),
            border,
            placeholder: c.fg_subtle,
            value: c.fg,
            selection: c.accent_ring,
        };
        match status {
            Status::Active => base(edge(c.line_strong)),
            Status::Hovered => base(edge(c.border_strong)),
            Status::Focused { .. } => base(edge(c.accent)),
            Status::Disabled => {
                let style = base(edge(c.line_strong));
                Style {
                    background: Background::Color(faded(c.bg)),
                    border: faded_border(style.border),
                    placeholder: faded(style.placeholder),
                    value: faded(style.value),
                    ..style
                }
            }
        }
    }
}

/// A field that refuses its value: the danger colour for its edge.
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
