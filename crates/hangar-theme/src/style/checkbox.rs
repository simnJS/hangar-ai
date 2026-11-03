//! Checkboxes. The web app had none of its own (the settings use switches),
//! so this is a field's look with the accent when ticked.

use iced::widget::checkbox::{Status, Style};
use iced::{Background, Color};

use super::{faded, faded_border, hairline};
use crate::Theme;
use crate::tokens::radius;

pub fn default(t: &Theme) -> impl Fn(&iced::Theme, Status) -> Style + '_ {
    move |_, status| {
        let c = &t.colors;
        let (checked, hovered, disabled) = match status {
            Status::Active { is_checked } => (is_checked, false, false),
            Status::Hovered { is_checked } => (is_checked, true, false),
            Status::Disabled { is_checked } => (is_checked, false, true),
        };
        let (background, edge): (Color, Color) = if checked {
            (c.accent, c.accent)
        } else if hovered {
            (c.bg, c.fg_subtle)
        } else {
            (c.bg, c.border_strong)
        };
        let style = Style {
            background: Background::Color(background),
            icon_color: c.accent_fg,
            border: hairline(edge, radius::XS),
            text_color: Some(c.fg),
        };
        if disabled {
            Style {
                background: Background::Color(faded(background)),
                icon_color: faded(style.icon_color),
                border: faded_border(style.border),
                text_color: Some(faded(c.fg)),
            }
        } else {
            style
        }
    }
}
