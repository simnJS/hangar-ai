//! Text colours: the levels of the foreground and the status words. Use as
//! `text("..").style(style::text::muted(t))`.
//!
//! Like the tokens they come from, each reads at 4.5:1 on every surface of
//! the chrome; `subtle` and `muted` are the two quietest and are not for
//! text on a selected row or a chip, where `secondary` is the quietest.

use iced::Color;
use iced::widget::text::Style;

use crate::{AgentState, Theme};

fn colored(color: Color) -> Style {
    Style { color: Some(color) }
}

/// Body text.
pub fn primary(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| colored(t.colors.fg)
}

/// Labels and secondary lines.
pub fn secondary(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| colored(t.colors.fg_secondary)
}

/// Metadata.
pub fn muted(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| colored(t.colors.fg_muted)
}

/// The quietest hints: paths, counts, placeholders.
pub fn subtle(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| colored(t.colors.fg_subtle)
}

/// Links and the focus.
pub fn accent(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| colored(t.colors.accent_hi)
}

/// Text on the accent.
pub fn on_accent(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| colored(t.colors.accent_fg)
}

pub fn danger(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| colored(t.colors.danger)
}

pub fn success(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| colored(t.colors.success)
}

pub fn warning(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| colored(t.colors.warning)
}

/// The word for an agent's state (working, waiting, your turn, idle).
pub fn state(t: &Theme, state: AgentState) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| colored(t.colors.state(state).text)
}
