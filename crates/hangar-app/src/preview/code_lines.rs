//! Text as numbered lines, coloured once the highlighter has run (plain until
//! then, never empty) (`components/preview/CodeLines.tsx`). Each line is a row
//! beside its number, so a long line wraps under itself and the numbers stay
//! with their lines. The numbers are not part of the text: what is copied is
//! the text.

use iced::font::Style as FontStyle;
use iced::widget::text::{self, LineHeight, Wrapping};
use iced::widget::{Space, column, container, rich_text, row, scrollable, span};
use iced::{Alignment, Element, Font, Length};

use hangar_theme::{Theme, fonts, style};

use super::highlight::{self, Lines};

/// The size of code, in points (`.preview-code`: 12 px).
const SIZE: f32 = 12.0;
/// And its line, 1.55 of it.
const LINE: f32 = 1.55;
/// What a mono digit takes of its size, near enough for the common faces: the
/// gutter is sized by digit count, as `1ch` was.
const DIGIT: f32 = 0.6;

/// The width of the gutter for a text of `lines` lines (`--digits`): the
/// digits, then the room the web app left around them.
pub fn gutter_width(lines: usize) -> f32 {
    let digits = lines.max(1).to_string().len() as f32;
    digits * DIGIT * SIZE + 22.0
}

/// The rows of `lines`, scrolling both ways, or only down when `wrap` is on.
pub fn view<'a, M: Clone + 'a>(t: &'a Theme, lines: &'a Lines, wrap: bool) -> Element<'a, M> {
    let gutter = gutter_width(lines.len());
    let mono = fonts::mono();
    let italic = Font {
        style: FontStyle::Italic,
        ..mono
    };
    let rows = lines.iter().enumerate().map(|(at, runs)| {
        let spans: Vec<text::Span<'a, (), Font>> = runs
            .iter()
            .map(|run| {
                let piece = span(run.text.as_str());
                let piece = match highlight::colour(t, run.class) {
                    Some(colour) => piece.color(colour),
                    None => piece,
                };
                if run.class.italic() {
                    piece.font(italic)
                } else {
                    piece.font(mono)
                }
            })
            .collect();
        let code = rich_text::<(), M, iced::Theme, iced::Renderer>(spans)
            .size(SIZE)
            .line_height(LineHeight::Relative(LINE))
            .wrapping(if wrap {
                Wrapping::WordOrGlyph
            } else {
                Wrapping::None
            })
            .color(t.colors.fg);
        let number = iced::widget::text(format!("{}", at + 1))
            .size(SIZE)
            .font(mono)
            .line_height(LineHeight::Relative(LINE))
            .color(t.colors.fg_subtle)
            .align_x(iced::alignment::Horizontal::Right)
            .width(gutter - 12.0);
        let row = row![number, Space::new().width(12.0), code].align_y(Alignment::Start);
        if wrap { row.width(Length::Fill) } else { row }.into()
    });
    let body = container(column(rows)).padding([6.0, 0.0]).style(move |_| {
        iced::widget::container::Style {
            background: Some(iced::Background::Color(t.colors.bg)),
            text_color: Some(t.colors.fg),
            ..Default::default()
        }
    });
    let direction = if wrap {
        scrollable::Direction::Vertical(scrollable::Scrollbar::default())
    } else {
        scrollable::Direction::Both {
            vertical: scrollable::Scrollbar::default(),
            horizontal: scrollable::Scrollbar::default(),
        }
    };
    // The page's colour reaches the edges whatever the lines' width.
    container(
        scrollable(body.width(if wrap { Length::Fill } else { Length::Shrink }))
            .direction(direction)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(style::scrollable::default(t)),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(move |_| iced::widget::container::Style {
        background: Some(iced::Background::Color(t.colors.bg)),
        ..Default::default()
    })
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gutter_grows_with_the_digits_of_the_last_line() {
        assert!(gutter_width(9) < gutter_width(10));
        assert!(gutter_width(99) < gutter_width(100));
        assert_eq!(gutter_width(10), gutter_width(99));
        assert_eq!(gutter_width(0), gutter_width(1));
    }

    #[test]
    fn code_is_set_at_the_size_of_the_web_app() {
        assert_eq!(SIZE, 12.0);
        assert!((hangar_theme::tokens::font_size::UI - SIZE).abs() < f32::EPSILON);
    }
}
