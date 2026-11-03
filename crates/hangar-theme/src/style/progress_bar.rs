//! Progress bars: a pill track a step above the page and a bar in one of the
//! status colours. Give the bar its thickness with `.girth(..)` (the stylesheets
//! use 3 to 4 px for thin ones).

use iced::widget::progress_bar::Style;
use iced::{Background, Color};

use super::rounded;
use crate::Theme;
use crate::tokens::radius;

fn bar(t: &Theme, fill: Color) -> Style {
    Style {
        background: Background::Color(t.colors.border),
        bar: Background::Color(fill),
        border: rounded(radius::FULL),
    }
}

/// The accent: progress in general.
pub fn accent(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| bar(t, t.colors.accent)
}

/// Success, a working agent's green.
pub fn success(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| bar(t, t.colors.state_working_fill)
}

/// A limit being approached.
pub fn warning(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| bar(t, t.colors.state_waiting_fill)
}

/// A limit exceeded.
pub fn danger(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| bar(t, t.colors.danger)
}
