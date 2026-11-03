//! Text on one line that gives up its beginning, not its end
//! (`.insp-tail`, the `direction: rtl` trick of the web app).
//!
//! ```ignore
//! use hangar_ui::tail_text::tail_text;
//!
//! // "…/clients/storefront/packages/ui" in a cell too narrow for the whole path.
//! tail_text("/home/ada/dev/clients/storefront/packages/ui")
//!     .size(10.5)
//!     .font(fonts::mono())
//!     .color(t.colors.fg_muted)
//! ```
//!
//! A path in a cell too narrow for it loses its first folders: the last ones
//! are the ones that tell two checkouts apart. iced's own `text` can only cut
//! the end, so this widget measures the text with the renderer's shaper at
//! layout time and keeps the longest tail that fits after an ellipsis.

use iced::advanced::Widget;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::text::{self, Paragraph as _, Renderer as _, Text};
use iced::advanced::widget::Tree;
use iced::{Color, Element, Font, Length, Pixels, Point, Rectangle, Size, alignment, mouse};

/// The mark that stands for the part that was cut.
const ELLIPSIS: char = '…';

/// The longest tail of `content` that fits in `max` after an ellipsis, or
/// all of it when it fits whole. `measure` is the width of a string; it is
/// called O(log n) times.
///
/// Cuts between characters, which is right for paths and names. When not
/// even the ellipsis fits, that is what is returned: a cell that narrow shows
/// that something is there.
pub fn fit_tail(content: &str, max: f32, measure: impl Fn(&str) -> f32) -> String {
    fit(content, max, measure, Cut::Start)
}

/// The longest head of `content` that fits in `max` before an ellipsis: the
/// same as [`fit_tail`] for a text whose end is what can go (an id, a
/// sentence).
pub fn fit_head(content: &str, max: f32, measure: impl Fn(&str) -> f32) -> String {
    fit(content, max, measure, Cut::End)
}

/// Which end of the text is given up when the room is short.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cut {
    Start,
    End,
}

fn fit(content: &str, max: f32, measure: impl Fn(&str) -> f32, cut: Cut) -> String {
    // An unknown room (NaN) cuts nothing, as an unbounded one does.
    if max.is_nan() || measure(content) <= max {
        return content.to_string();
    }
    let starts: Vec<usize> = content.char_indices().map(|(at, _)| at).collect();
    let kept = |count: usize| {
        let mut shown = String::with_capacity(content.len() + ELLIPSIS.len_utf8());
        match cut {
            Cut::Start => {
                shown.push(ELLIPSIS);
                shown.push_str(&content[starts[starts.len() - count]..]);
            }
            Cut::End => {
                let upto = starts.get(count).copied().unwrap_or(content.len());
                shown.push_str(&content[..upto]);
                shown.push(ELLIPSIS);
            }
        }
        shown
    };
    // `fits` holds a number of characters that fit and `too_many` one that
    // does not; the width only grows with the count.
    let (mut fits, mut too_many) = (0, starts.len());
    while fits + 1 < too_many {
        let middle = fits + (too_many - fits) / 2;
        if measure(&kept(middle)) <= max {
            fits = middle;
        } else {
            too_many = middle;
        }
    }
    if fits == 0 {
        ELLIPSIS.to_string()
    } else {
        kept(fits)
    }
}

/// The widget [`tail_text`] makes.
#[must_use]
pub struct TailText {
    content: String,
    shown: String,
    size: f32,
    font: Font,
    color: Option<Color>,
    width: Length,
    cut: Cut,
    /// A last piece that is never cut while there is room for it, in a colour
    /// of its own: the file name after its folder.
    suffix: Option<(String, Color)>,
    /// How wide what is shown of `content` came out, where the suffix starts.
    head_width: f32,
}

/// `content` on one line, cut from its start when the room is short.
pub fn tail_text(content: impl Into<String>) -> TailText {
    TailText {
        content: content.into(),
        shown: String::new(),
        size: 12.0,
        font: Font::DEFAULT,
        color: None,
        width: Length::Fill,
        cut: Cut::Start,
        suffix: None,
        head_width: 0.0,
    }
}

impl TailText {
    /// A piece that follows the text, in its own colour, and is the last to
    /// give way: `tail_text(folder).suffix(name, ink)` keeps the file name
    /// whole and shortens the folder before it (`.insp-file__path`).
    pub fn suffix(mut self, suffix: impl Into<String>, color: Color) -> Self {
        self.suffix = Some((suffix.into(), color));
        self
    }

    /// Gives up the end instead of the start: the ordinary ellipsis, for a
    /// cell that holds a name or an id and not a path.
    pub fn cut_end(mut self) -> Self {
        self.cut = Cut::End;
        self
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub fn font(mut self, font: Font) -> Self {
        self.font = font;
        self
    }

    /// The colour of the text; the surrounding text colour when not given.
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// `Fill` (the default) takes the room it is given; `Shrink` is as wide
    /// as the whole text, up to that room.
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    fn line_height(&self) -> f32 {
        text::LineHeight::default().to_absolute(Pixels(self.size)).0
    }
}

/// The width of `content` set in this size and face.
fn text_width(content: &str, size: f32, font: Font) -> f32 {
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

impl<Message, Theme> Widget<Message, Theme, iced::Renderer> for TailText {
    fn size(&self) -> Size<Length> {
        Size::new(self.width, Length::Fixed(self.line_height()))
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let height = self.line_height();
        let (text_size, font) = (self.size, self.font);
        let suffix_width = self
            .suffix
            .as_ref()
            .map_or(0.0, |(suffix, _)| text_width(suffix, text_size, font));
        let natural = text_width(&self.content, text_size, font) + suffix_width;
        let size = limits
            .width(self.width)
            .height(Length::Fixed(height))
            .resolve(
                self.width,
                Length::Fixed(height),
                Size::new(natural, height),
            );
        // The suffix keeps what it needs; the text gives way for the rest.
        self.shown = fit(
            &self.content,
            (size.width - suffix_width).max(0.0),
            |candidate| text_width(candidate, text_size, font),
            self.cut,
        );
        self.head_width = text_width(&self.shown, text_size, font);
        layout::Node::new(size)
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let Some(clip) = bounds.intersection(viewport) else {
            return;
        };
        let piece = |content: String| Text {
            content,
            bounds: bounds.size(),
            size: Pixels(self.size),
            line_height: text::LineHeight::default(),
            font: self.font,
            align_x: text::Alignment::Left,
            align_y: alignment::Vertical::Center,
            shaping: text::Shaping::Advanced,
            wrapping: text::Wrapping::None,
        };
        renderer.fill_text(
            piece(self.shown.clone()),
            Point::new(bounds.x, bounds.center_y()),
            self.color.unwrap_or(style.text_color),
            clip,
        );
        if let Some((suffix, color)) = &self.suffix {
            renderer.fill_text(
                piece(suffix.clone()),
                Point::new(bounds.x + self.head_width, bounds.center_y()),
                *color,
                clip,
            );
        }
    }
}

impl<'a, Message: 'a, Theme: 'a> From<TailText> for Element<'a, Message, Theme, iced::Renderer> {
    fn from(text: TailText) -> Self {
        Element::new(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One unit per character, the ellipsis included.
    fn units(s: &str) -> f32 {
        s.chars().count() as f32
    }

    #[test]
    fn text_that_fits_is_left_whole() {
        assert_eq!(fit_tail("/a/b/c", 6.0, units), "/a/b/c");
        assert_eq!(fit_tail("/a/b/c", 100.0, units), "/a/b/c");
        assert_eq!(fit_tail("", 0.0, units), "");
    }

    #[test]
    fn the_start_goes_first_and_the_end_stays() {
        // Seven characters in a room of five: the mark and the last four.
        assert_eq!(fit_tail("/a/b/cd", 5.0, units), "…b/cd");
        assert_eq!(fit_tail("/home/ada/dev/app", 9.0, units), "…/dev/app");
    }

    #[test]
    fn a_room_too_small_for_anything_shows_the_mark_alone() {
        assert_eq!(fit_tail("/home/ada", 1.0, units), "…");
        assert_eq!(fit_tail("/home/ada", 0.0, units), "…");
    }

    #[test]
    fn it_cuts_between_characters_not_bytes() {
        // "/été" is four characters and six bytes.
        assert_eq!(fit_tail("/données/été", 5.0, units), "…/été");
    }

    #[test]
    fn the_end_can_be_the_part_that_goes() {
        assert_eq!(fit_head("abcdefgh", 5.0, units), "abcd…");
        assert_eq!(fit_head("abcde", 5.0, units), "abcde");
        assert_eq!(fit_head("abcdefgh", 0.0, units), "…");
        assert_eq!(fit_head("été-été", 4.0, units), "été…");
    }

    #[test]
    fn an_unbounded_room_never_cuts() {
        assert_eq!(fit_tail("/a/b", f32::INFINITY, units), "/a/b");
    }

    #[test]
    fn the_measure_is_asked_a_logarithmic_number_of_times() {
        let calls = std::cell::Cell::new(0);
        let path = "x".repeat(4096);
        let shown = fit_tail(&path, 100.0, |s| {
            calls.set(calls.get() + 1);
            units(s)
        });
        assert_eq!(shown.chars().count(), 100);
        assert!(calls.get() < 30, "asked {} times", calls.get());
    }
}
