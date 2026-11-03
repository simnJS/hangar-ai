//! Badges and counters: small marks that say how many, or what kind.
//!
//! ```ignore
//! use hangar_ui::badge::{self, Tone};
//!
//! badge::badge(t, "new", Tone::Accent)
//! badge::badge(t, "2 conflicts", Tone::Danger)
//! badge::count(t, 12)               // the quiet mono number of a folder's size
//! badge::tag(t, "custom")           // a word set on a surface
//! ```

use hangar_theme::tokens::{font_size, space};
use hangar_theme::{Theme, fonts, style};
use iced::widget::{container, text};
use iced::{Alignment, Element, Padding};

pub use hangar_theme::style::Tone;

/// A word or a number in a capsule tinted for what it says (`.chip`).
pub fn badge<'a, M: 'a>(t: &'a Theme, label: impl Into<String>, tone: Tone) -> Element<'a, M> {
    let ink = style::container::tone_text(t, tone);
    container(
        text(label.into())
            .size(font_size::MICRO)
            .font(fonts::mono())
            .style(move |_: &iced::Theme| text::Style { color: Some(ink) }),
    )
    .padding(Padding::from([1.0, space::SM]))
    .align_y(Alignment::Center)
    .style(style::container::badge(t, tone))
    .into()
}

/// A number in the quietest text, in the mono face: how many a folder holds
/// (`.nav-count`).
pub fn count<'a, M: 'a>(t: &'a Theme, n: usize) -> Element<'a, M> {
    text(n.to_string())
        .size(font_size::LABEL)
        .font(fonts::mono())
        .style(style::text::subtle(t))
        .into()
}

/// A small word on a surface of its own (`.key-row__tag`): "custom".
pub fn tag<'a, M: 'a>(t: &'a Theme, label: impl Into<String>) -> Element<'a, M> {
    container(
        text(label.into())
            .size(font_size::LABEL)
            .font(fonts::ui_medium()),
    )
    .padding(Padding::from([0.0, space::SM]))
    .style(style::container::tag(t))
    .into()
}
