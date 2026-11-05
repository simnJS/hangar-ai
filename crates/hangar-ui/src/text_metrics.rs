//! Measures shared by the widgets that set text on one line.

use iced::advanced::text::{self, Paragraph as _, Text};
use iced::{Font, Pixels, Size, alignment};

/// The width of `content` set in this size and face.
pub(crate) fn text_width(content: &str, size: f32, font: Font) -> f32 {
    type Paragraph = <iced::Renderer as text::Renderer>::Paragraph;
    Paragraph::with_text(Text {
        content,
        bounds: Size::INFINITE,
        size: Pixels(size),
        line_height: text::LineHeight::default(),
        font,
        align_x: text::Alignment::Default,
        align_y: alignment::Vertical::Top,
        shaping: text::Shaping::Advanced,
        wrapping: text::Wrapping::None,
    })
    .min_bounds()
    .width
}

/// The nearest byte at or before `at` that starts a character.
pub(crate) fn floor_char_boundary(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}
