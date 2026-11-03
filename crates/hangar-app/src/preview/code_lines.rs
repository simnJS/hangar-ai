//! Text as numbered lines, coloured once the highlighter has run (plain until
//! then, never empty) (`components/preview/CodeLines.tsx`). Each line is a row
//! beside its number, so a long line wraps under itself and the numbers stay
//! with their lines. The numbers are not part of the text: what is copied is
//! the text.
//!
//! A page of the web app drew all of its 5,000 rows and let the engine decide
//! which were seen. iced lays out every widget it is given, on every change
//! of the interface, so the unwrapped view builds only the rows in sight and a
//! screenful either side, and stands in for the others with a space of their
//! height: every row is exactly one line tall, which is what makes that
//! possible. A wrapped line has a height of its own, which nothing knows
//! before it is laid out, so the wrapped view builds them all.

use iced::font::Style as FontStyle;
use iced::widget::text::{self, LineHeight, Wrapping};
use iced::widget::{Space, column, container, rich_text, row, scrollable, span};
use iced::{Alignment, Element, Font, Length, Pixels};

use hangar_theme::{Theme, fonts, style};

use super::highlight::{self, Lines};

/// The size of code, in points (`.preview-code`: 12 px).
const SIZE: f32 = 12.0;
/// And its line, 1.55 of it, which is what a row is tall (`line-height: 1.55`).
pub const ROW: f32 = SIZE * 1.55;
/// What a mono digit takes of its size, near enough for the common faces: the
/// gutter is sized by digit count, as `1ch` was.
const DIGIT: f32 = 0.6;
/// The room above the first row and under the last.
const PAD: f32 = 6.0;
/// Rows built beyond the ones in sight, each side: what a fast wheel passes
/// before the next message brings the window along.
const OVERSCAN: usize = 30;
/// How tall the view is taken to be before it has said: more than a screen.
const GUESS: f32 = 1600.0;

/// The width of the gutter for a text of `lines` lines (`--digits`): the
/// digits, then the room the web app left around them.
pub fn gutter_width(lines: usize) -> f32 {
    let digits = lines.max(1).to_string().len() as f32;
    digits * DIGIT * SIZE + 22.0
}

/// Where the view is scrolled to, as it last said: how far down, and how tall
/// it is. Nothing yet is `Scroll::default()`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Scroll {
    pub offset: f32,
    pub height: f32,
}

/// The rows to build for a view scrolled to `scroll`, of `total` rows: the
/// ones in sight and `OVERSCAN` either side.
pub fn window(total: usize, scroll: Scroll) -> std::ops::Range<usize> {
    let height = if scroll.height > 0.0 {
        scroll.height
    } else {
        GUESS
    };
    let first = ((scroll.offset - PAD).max(0.0) / ROW).floor() as usize;
    let last = ((scroll.offset + height) / ROW).ceil() as usize;
    let start = first.saturating_sub(OVERSCAN).min(total);
    let end = (last + OVERSCAN).min(total);
    start..end.max(start)
}

/// The rows of `lines`, scrolling both ways, or only down when `wrap` is on.
/// `on_scroll` hears how far down the view went and how tall it is, to say
/// which rows to build next.
pub fn view<'a, M: Clone + 'a>(
    t: &'a Theme,
    lines: &'a Lines,
    wrap: bool,
    scroll: Scroll,
    on_scroll: fn(Scroll) -> M,
    id: iced::widget::Id,
) -> Element<'a, M> {
    let gutter = gutter_width(lines.len());
    let mono = fonts::mono();
    let italic = Font {
        style: FontStyle::Italic,
        ..mono
    };
    let line_height = LineHeight::Absolute(Pixels(ROW));
    let built = if wrap {
        0..lines.len()
    } else {
        window(lines.len(), scroll)
    };
    let before = built.start;
    let after = lines.len() - built.end;
    let rows = lines[built.clone()].iter().enumerate().map(|(at, runs)| {
        let at = at + before;
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
            .line_height(line_height)
            .wrapping(if wrap {
                Wrapping::WordOrGlyph
            } else {
                Wrapping::None
            })
            .color(t.colors.fg);
        let number = iced::widget::text(format!("{}", at + 1))
            .size(SIZE)
            .font(mono)
            .line_height(line_height)
            .color(t.colors.fg_subtle)
            .align_x(iced::alignment::Horizontal::Right)
            .width(gutter - 12.0);
        let row = row![number, Space::new().width(12.0), code].align_y(Alignment::Start);
        if wrap { row.width(Length::Fill) } else { row }.into()
    });
    let mut stack = column![].width(if wrap { Length::Fill } else { Length::Shrink });
    if before > 0 {
        stack = stack.push(Space::new().height(before as f32 * ROW));
    }
    stack = stack.push(column(rows));
    if after > 0 {
        stack = stack.push(Space::new().height(after as f32 * ROW));
    }
    if !wrap {
        // As wide as the widest line, in the rows built or not, so that the
        // horizontal bar does not change as the window moves.
        let widest = lines
            .iter()
            .map(|runs| {
                runs.iter()
                    .map(|run| run.text.chars().count())
                    .sum::<usize>()
            })
            .max()
            .unwrap_or(0);
        stack = stack.push(Space::new().width(gutter + 16.0 + widest as f32 * DIGIT * SIZE));
    }
    let body =
        container(stack)
            .padding([PAD, 0.0])
            .style(move |_| iced::widget::container::Style {
                background: Some(iced::Background::Color(t.colors.bg)),
                text_color: Some(t.colors.fg),
                ..Default::default()
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
            .id(id)
            .direction(direction)
            .on_scroll(move |viewport| {
                on_scroll(Scroll {
                    offset: viewport.absolute_offset().y,
                    height: viewport.bounds().height,
                })
            })
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
        assert!((ROW - 18.6).abs() < 1e-4);
    }

    #[test]
    fn builds_the_rows_in_sight_and_a_few_screens_either_side() {
        // Never scrolled: from the top, a screen and some.
        let top = window(5000, Scroll::default());
        assert_eq!(top.start, 0);
        assert!(top.end > 80 && top.end < 200, "{top:?}");
        // Scrolled a good way down: a window around where it is.
        let middle = window(
            5000,
            Scroll {
                offset: 1000.0 * ROW,
                height: 600.0,
            },
        );
        assert!(middle.start < 1000 && middle.start > 900, "{middle:?}");
        assert!(middle.end > 1032 && middle.end < 1100, "{middle:?}");
    }

    #[test]
    fn the_window_stays_inside_the_text_whatever_the_scroll() {
        assert_eq!(window(0, Scroll::default()), 0..0);
        assert_eq!(window(10, Scroll::default()), 0..10);
        let past = window(
            100,
            Scroll {
                offset: 1.0e9,
                height: 600.0,
            },
        );
        assert_eq!(past, 100..100);
    }

    #[test]
    fn what_is_not_built_is_stood_in_for_by_its_height() {
        // The rows built plus the spaces on either side are the whole text.
        let total = 5000;
        let scroll = Scroll {
            offset: 2500.0 * ROW,
            height: 700.0,
        };
        let built = window(total, scroll);
        assert_eq!(built.start + built.len() + (total - built.end), total);
    }
}
