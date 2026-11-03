//! Tooltips: a short line that appears over a control after the pointer has
//! rested on it.
//!
//! ```ignore
//! use hangar_ui::tip;
//!
//! tip::bottom(t, icon_button, "Settings")
//! tip::keys(t, icon_button, "Settings", "Ctrl+,")   // with its shortcut, as the web app's titles had
//! ```
//!
//! The web app's tooltips were the browser's `title`, which waits about a
//! second; the delay here is shorter, since an icon with nothing else to say
//! what it does is unusable without one.

use std::time::Duration;

use hangar_theme::tokens::{font_size, space};
use hangar_theme::{Theme, style};
use iced::widget::tooltip::Position;
use iced::widget::{container, row, text, tooltip};
use iced::{Element, Padding};

use crate::shortcut::format_binding;
use crate::text as typography;

/// How long the pointer rests before the tip shows.
pub const DELAY: Duration = Duration::from_millis(450);

/// The gap between the control and the tip.
const GAP: f32 = 6.0;

fn build<'a, M: 'a>(
    t: &'a Theme,
    content: impl Into<Element<'a, M>>,
    tip: Element<'a, M>,
    position: Position,
) -> Element<'a, M> {
    tooltip(content, tip, position)
        .gap(GAP)
        .padding(0)
        .delay(DELAY)
        .style(style::container::tooltip(t))
        .into()
}

fn line<'a, M: 'a>(t: &'a Theme, words: String) -> Element<'a, M> {
    container(
        text(words)
            .size(font_size::CAPTION_LG)
            .style(style::text::primary(t)),
    )
    .padding(Padding::from([space::XS - 1.0, space::MD - 1.0]))
    .into()
}

/// `content` with `words` over it.
pub fn over<'a, M: 'a>(
    t: &'a Theme,
    content: impl Into<Element<'a, M>>,
    words: impl Into<String>,
) -> Element<'a, M> {
    build(t, content, line(t, words.into()), Position::Top)
}

/// `content` with `words` under it: for the toolbar and the top of a pane,
/// where there is nothing over it.
pub fn bottom<'a, M: 'a>(
    t: &'a Theme,
    content: impl Into<Element<'a, M>>,
    words: impl Into<String>,
) -> Element<'a, M> {
    build(t, content, line(t, words.into()), Position::Bottom)
}

/// `content` with `words` on its right: for the navigator's rail.
pub fn right<'a, M: 'a>(
    t: &'a Theme,
    content: impl Into<Element<'a, M>>,
    words: impl Into<String>,
) -> Element<'a, M> {
    build(t, content, line(t, words.into()), Position::Right)
}

/// `content` under a tip that ends with the shortcut of what it does, in the
/// mono face (`Settings  Ctrl+,`). A binding that is empty is left out, so
/// the call can pass whatever the keymap says.
pub fn keys<'a, M: 'a>(
    t: &'a Theme,
    content: impl Into<Element<'a, M>>,
    words: impl Into<String>,
    binding: &str,
) -> Element<'a, M> {
    if binding.trim().is_empty() {
        return bottom(t, content, words);
    }
    let shown = format_binding(binding);
    let tip = container(
        row![
            text(words.into())
                .size(font_size::CAPTION_LG)
                .style(style::text::primary(t)),
            typography::mono(t, shown).size(font_size::LABEL),
        ]
        .spacing(space::LG)
        .align_y(iced::Alignment::Center),
    )
    .padding(Padding::from([space::XS - 1.0, space::MD - 1.0]))
    .into();
    build(t, content, tip, Position::Bottom)
}
