//! Text as the window sets it: a size, a weight and a colour for each role, so
//! that a view says "a hint" and not "11.5 pixels in the secondary grey".
//!
//! ```ignore
//! use hangar_ui::text;
//!
//! column![
//!     text::title(t, "Shortcuts"),
//!     text::hint(t, "Click a key to record another."),
//! ]
//! ```
//!
//! Every function returns iced's own `Text`, so what a view needs on top
//! (`.width`, `.wrapping`, `.align_x`) is still there.

use hangar_theme::tokens::font_size;
use hangar_theme::{Theme, fonts, style};
use iced::widget::text::{self, IntoFragment, Text};

/// The page's text: 13 px in the foreground colour.
pub fn body<'a>(t: &'a Theme, content: impl IntoFragment<'a>) -> Text<'a> {
    text::Text::new(content)
        .size(font_size::BODY)
        .style(style::text::primary(t))
}

/// A label: 13 px in the secondary grey.
pub fn label<'a>(t: &'a Theme, content: impl IntoFragment<'a>) -> Text<'a> {
    text::Text::new(content)
        .size(font_size::BODY)
        .style(style::text::secondary(t))
}

/// What fields and the chrome are set in: 12 px.
pub fn ui<'a>(t: &'a Theme, content: impl IntoFragment<'a>) -> Text<'a> {
    text::Text::new(content)
        .size(font_size::UI)
        .style(style::text::primary(t))
}

/// Metadata: 12 px in the muted grey.
pub fn muted<'a>(t: &'a Theme, content: impl IntoFragment<'a>) -> Text<'a> {
    text::Text::new(content)
        .size(font_size::UI)
        .style(style::text::muted(t))
}

/// A line under a control saying what it does (`.form-hint`, `.set-row__hint`):
/// 11.5 px in the secondary grey.
pub fn hint<'a>(t: &'a Theme, content: impl IntoFragment<'a>) -> Text<'a> {
    text::Text::new(content)
        .size(font_size::CAPTION_LG)
        .style(style::text::secondary(t))
}

/// The quietest text: 11 px in the subtle grey, for paths and counts.
pub fn subtle<'a>(t: &'a Theme, content: impl IntoFragment<'a>) -> Text<'a> {
    text::Text::new(content)
        .size(font_size::CAPTION)
        .style(style::text::subtle(t))
}

/// A dialog's or a page's title: 15 px, semibold.
pub fn title<'a>(t: &'a Theme, content: impl IntoFragment<'a>) -> Text<'a> {
    text::Text::new(content)
        .size(font_size::TITLE)
        .font(fonts::ui_semibold())
        .style(style::text::primary(t))
}

/// The heading of a group of things: 11 px, semibold, muted
/// (`.ctx-menu__head`, `.insp-section__title`).
pub fn heading<'a>(t: &'a Theme, content: impl IntoFragment<'a>) -> Text<'a> {
    text::Text::new(content)
        .size(font_size::CAPTION)
        .font(fonts::ui_semibold())
        .style(style::text::muted(t))
}

/// What the interface sets in the mono face: versions, paths, counts.
pub fn mono<'a>(t: &'a Theme, content: impl IntoFragment<'a>) -> Text<'a> {
    text::Text::new(content)
        .size(font_size::CAPTION_LG)
        .font(fonts::mono())
        .style(style::text::muted(t))
}

/// The accent as a word: a link that is not a button.
pub fn accent<'a>(t: &'a Theme, content: impl IntoFragment<'a>) -> Text<'a> {
    text::Text::new(content)
        .size(font_size::UI)
        .style(style::text::accent(t))
}

/// A warning in words, in the danger colour.
pub fn danger<'a>(t: &'a Theme, content: impl IntoFragment<'a>) -> Text<'a> {
    text::Text::new(content)
        .size(font_size::CAPTION_LG)
        .style(style::text::danger(t))
}
