//! A path that loses its beginning when it does not fit (`direction: rtl` with
//! an ellipsis in `.status__path`): the end of a path is the part that says
//! where you are, so it is the end that stays.

use hangar_theme::fonts;
use iced::advanced::text::{self, Paragraph as _, Text};
use iced::{Font, Pixels, Size, alignment};

const ELLIPSIS: &str = "…";

/// The longest end of `full` that fits in `width`, with an ellipsis in front
/// when something was cut. `measure` is the width of a string as drawn.
pub fn fit(full: &str, width: f32, measure: impl Fn(&str) -> f32) -> String {
    if measure(full) <= width {
        return full.to_string();
    }
    let starts: Vec<usize> = full
        .char_indices()
        .map(|(at, _)| at)
        .chain(std::iter::once(full.len()))
        .collect();
    // The smallest cut that fits, by bisection: each candidate is the ellipsis
    // and the text from `starts[cut]` on, which only gets shorter as `cut`
    // grows.
    let (mut low, mut high) = (1, starts.len() - 1);
    while low < high {
        let middle = (low + high) / 2;
        if measure(&format!("{ELLIPSIS}{}", &full[starts[middle]..])) <= width {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    format!("{ELLIPSIS}{}", &full[starts[low]..])
}

/// The width of `content` set in this size and face.
pub fn text_width(content: &str, size: f32, font: Font) -> f32 {
    type Paragraph = <iced::Renderer as text::Renderer>::Paragraph;
    Paragraph::with_text(Text {
        content,
        bounds: Size::INFINITE,
        size: Pixels(size),
        line_height: text::LineHeight::default(),
        font,
        align_x: text::Alignment::Default,
        align_y: alignment::Vertical::Top,
        shaping: text::Shaping::Basic,
        wrapping: text::Wrapping::None,
    })
    .min_bounds()
    .width
}

/// A path in the status bar's face (11 px mono).
pub fn path_width(content: &str, size: f32) -> f32 {
    text_width(content, size, fonts::mono())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character is 10 wide, so a width of 100 holds ten.
    fn ten(text: &str) -> f32 {
        text.chars().count() as f32 * 10.0
    }

    #[test]
    fn a_path_that_fits_is_left_alone() {
        assert_eq!(fit("/home/me/app", 200.0, ten), "/home/me/app");
    }

    #[test]
    fn a_path_that_does_not_fit_keeps_its_end() {
        // Ten characters in all: the ellipsis and nine of the path's end.
        let cut = fit("/home/simnjs/dev/hangar", 100.0, ten);
        assert_eq!(cut, "…ev/hangar");
        assert_eq!(cut.chars().count(), 10);
    }

    #[test]
    fn it_cuts_no_more_than_it_has_to() {
        let cut = fit("abcdefghij", 90.0, ten);
        assert_eq!(cut, "…cdefghij");
        assert!(ten(&cut) <= 90.0);
        // One character more would not have fitted.
        assert!(ten(&format!("…{}", &"abcdefghij"[1..])) > 90.0);
    }

    #[test]
    fn a_room_too_small_for_anything_still_answers() {
        assert_eq!(fit("abcdef", 5.0, ten), "…");
    }

    #[test]
    fn it_cuts_between_characters_not_bytes() {
        let cut = fit("dossier/été/projet", 80.0, ten);
        assert!(cut.starts_with('…'));
        assert!(ten(&cut) <= 80.0);
        assert!(cut.ends_with("projet"));
    }
}
