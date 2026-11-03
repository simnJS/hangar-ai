//! Drop-downs (`.set-select`). A pick list has two styles: the field that
//! shows the choice, and the menu that opens under it.
//!
//! ```ignore
//! pick_list(options, selected, Message::Picked)
//!     .style(style::pick_list::field(t))
//!     .menu_style(style::pick_list::menu(t))
//! ```

use iced::overlay::menu;
use iced::widget::pick_list::{Status, Style};
use iced::{Background, Border};

use super::{drop_shadow, hairline};
use crate::Theme;
use crate::tokens::radius;

/// The closed field: dressed like a text field, with its chevron in
/// `fg_muted`.
pub fn field(t: &Theme) -> impl Fn(&iced::Theme, Status) -> Style + '_ {
    move |_, status| {
        let c = &t.colors;
        let border = match status {
            Status::Active => c.line_strong,
            Status::Hovered => c.border_strong,
            Status::Opened { .. } => c.accent,
        };
        Style {
            text_color: c.fg,
            placeholder_color: c.fg_subtle,
            handle_color: c.fg_muted,
            background: Background::Color(c.bg),
            border: hairline(border, radius::SM),
        }
    }
}

/// The open list: a popover, whose highlighted line is the accent as in a
/// context menu.
pub fn menu(t: &Theme) -> impl Fn(&iced::Theme) -> menu::Style + '_ {
    move |_| {
        let c = &t.colors;
        menu::Style {
            background: Background::Color(c.chrome_bar),
            border: Border {
                radius: radius::LG.into(),
                ..hairline(c.line_strong, radius::LG)
            },
            text_color: c.fg,
            selected_text_color: c.accent_fg,
            selected_background: Background::Color(c.accent),
            shadow: drop_shadow(c.shadow, 12.0, 32.0),
        }
    }
}
