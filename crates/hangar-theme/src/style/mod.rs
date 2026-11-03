//! Style functions for the widgets the app is made of, so that a view never
//! picks a colour: it names a role (`button::primary`, `container::card`) and
//! hands over the theme it is drawn in.
//!
//! ```ignore
//! use hangar_theme::style;
//!
//! let t = self.theme; // a `&'static hangar_theme::Theme`
//! button(text("Save")).style(style::button::primary(t))
//! container(content).style(style::container::card(t))
//! ```
//!
//! Each function takes the Hangar [`Theme`](crate::Theme) and returns the
//! closure iced's `.style(..)` wants (`Fn(&iced::Theme, Status) -> Style`, or
//! without the status where the widget has none). The closure ignores the
//! `iced::Theme` it is called with: that one is [`crate::to_iced`]'s, kept for
//! the widgets that are not styled here, and carries too little (no
//! hairlines, no separators, no state colours) to draw the chrome from.
//! Taking the theme by reference makes the closure borrow it, which costs
//! nothing for the `&'static` ones [`crate::themes`] and [`crate::by_id`]
//! give.
//!
//! The CSS each role comes from is named in its module. Three things the web
//! app did have no counterpart in iced and are approximated:
//!
//! - A border is the same on all four sides, so the single hairlines of the
//!   chrome (the navigator's right edge, the toolbar's bottom) are a
//!   [`rule`] between the two areas rather than a border of one of them.
//! - There is no outline, so the focus ring (2 px, `accent_ring`) is the
//!   control's own border while it has the keyboard.
//! - A shadow has no spread, so the layered shadows of the stylesheets are
//!   their largest layer.

pub mod button;
pub mod checkbox;
pub mod container;
pub mod pick_list;
pub mod progress_bar;
pub mod rule;
pub mod scrollable;
pub mod slider;
pub mod text;
pub mod text_editor;
pub mod text_input;
pub mod toggler;

#[cfg(test)]
mod tests;

use iced::{Border, Color, Shadow, Vector};

use crate::tokens::{BORDER_WIDTH, DISABLED_OPACITY};

/// What a badge or a toast is about, so that a view names a meaning and the
/// theme picks the colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Tone {
    /// A count, a plain fact.
    #[default]
    Neutral,
    /// What is selected or current.
    Accent,
    Success,
    Warning,
    Danger,
}

/// A hairline of this colour with these corners.
pub(crate) fn hairline(color: Color, radius: f32) -> Border {
    Border {
        color,
        width: BORDER_WIDTH,
        radius: radius.into(),
    }
}

/// No border, but the corners of one.
pub(crate) fn rounded(radius: f32) -> Border {
    Border {
        color: Color::TRANSPARENT,
        width: 0.0,
        radius: radius.into(),
    }
}

/// A disabled control: every colour at 45 % of its opacity.
pub(crate) fn faded(color: Color) -> Color {
    color.scale_alpha(DISABLED_OPACITY)
}

pub(crate) fn faded_border(border: Border) -> Border {
    Border {
        color: faded(border.color),
        ..border
    }
}

/// `color` with its channels multiplied by `factor`, the way CSS
/// `filter: brightness()` does: above 1 lighter, below 1 darker.
pub(crate) fn brightness(color: Color, factor: f32) -> Color {
    Color {
        r: (color.r * factor).min(1.0),
        g: (color.g * factor).min(1.0),
        b: (color.b * factor).min(1.0),
        a: color.a,
    }
}

/// `color` with this alpha, whatever it had.
pub(crate) fn with_alpha(color: Color, alpha: f32) -> Color {
    Color { a: alpha, ..color }
}

/// A shadow cast `y` pixels down and blurred over `blur`.
pub(crate) fn drop_shadow(color: Color, y: f32, blur: f32) -> Shadow {
    Shadow {
        color,
        offset: Vector::new(0.0, y),
        blur_radius: blur,
    }
}
