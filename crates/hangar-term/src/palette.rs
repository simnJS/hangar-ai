//! The colours a terminal is drawn with.
//!
//! The crate does not know Hangar's themes: the app maps whichever theme is
//! active onto a [`Palette`] and hands it to [`Terminal::set_palette`]. Colours
//! a program picks itself are not in the palette's way: truecolour cells are
//! drawn as they are, and `OSC 4`/`10`/`11`/`12` overrides win over it until
//! the program resets them.
//!
//! [`Terminal::set_palette`]: crate::Terminal::set_palette

use iced::Color;

/// Everything colour in a terminal that is not decided by the program running
/// in it.
#[derive(Debug, Clone, PartialEq)]
pub struct Palette {
    /// Text without a colour of its own.
    pub foreground: Color,
    /// The terminal's background, and the one cells are compared against: a
    /// cell with this background is not drawn at all.
    pub background: Color,
    /// The cursor's fill.
    pub cursor: Color,
    /// The character under a block cursor, which must stay legible on the
    /// fill above.
    pub cursor_text: Color,
    /// The fill behind selected text.
    pub selection_background: Color,
    /// The sixteen ANSI colours: black, red, green, yellow, blue, magenta,
    /// cyan, white, then the same eight bright.
    pub ansi: [Color; 16],
    /// Colours 16 to 255, when the theme wants to replace the xterm cube and
    /// grey ramp. `None` computes them the way xterm does, which is what
    /// programs that pick colours by number are written against.
    pub indexed: Option<Box<[Color; 240]>>,
    /// Every search match, laid over the background.
    pub search_match: Color,
    /// The match the search is on.
    pub search_active: Color,
}

impl Default for Palette {
    /// Hangar's graphite theme, which is also what a terminal looks like
    /// before the app has said anything about the theme.
    fn default() -> Self {
        let hex = |rgb: u32| Color::from_rgb8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
        Self {
            foreground: hex(0xc8c9ce),
            background: hex(0x121316),
            cursor: hex(0xd8d9dd),
            cursor_text: hex(0x121316),
            selection_background: hex(0x2a3954),
            ansi: [
                hex(0x1b1c1f),
                hex(0xff7b72),
                hex(0x5fd27f),
                hex(0xe7c06b),
                hex(0x79b0ff),
                hex(0xc79bff),
                hex(0x6fd3df),
                hex(0xc8c9ce),
                hex(0x6b6c73),
                hex(0xff9a92),
                hex(0x7fe29b),
                hex(0xf2d38b),
                hex(0x9cc4ff),
                hex(0xd8b6ff),
                hex(0x93e3ec),
                hex(0xecedf0),
            ],
            indexed: None,
            search_match: hex(0xe7c06b),
            search_active: hex(0x3e8bff),
        }
    }
}

impl Palette {
    /// The colour of palette entry `index`, 0 to 255.
    pub fn indexed(&self, index: u8) -> Color {
        match index {
            0..=15 => self.ansi[index as usize],
            _ => match &self.indexed {
                Some(table) => table[index as usize - 16],
                None => xterm_indexed(index),
            },
        }
    }
}

/// Colours 16 to 255 as xterm defines them: a 6x6x6 cube, then 24 greys.
///
/// The cube's steps are 0 and then 95 + 40n, not an even ramp, because that is
/// what every other terminal draws and what programs pick their colours
/// against.
pub fn xterm_indexed(index: u8) -> Color {
    match index {
        16..=231 => {
            let n = index - 16;
            let step = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Color::from_rgb8(step(n / 36), step(n / 6 % 6), step(n % 6))
        }
        232..=255 => {
            let level = 8 + (index - 232) * 10;
            Color::from_rgb8(level, level, level)
        }
        // The first sixteen belong to the palette; this is only the fallback
        // for a caller that asks for them here.
        _ => Color::BLACK,
    }
}

/// `a` moved `amount` of the way towards `b`, as the blend used for dimmed
/// text and for highlights laid over a background.
pub(crate) fn mix(a: Color, b: Color, amount: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * amount,
        g: a.g + (b.g - a.g) * amount,
        b: a.b + (b.b - a.b) * amount,
        a: a.a + (b.a - a.a) * amount,
    }
}

/// An 8-bit RGB triple as the emulator reports it, for answering colour
/// queries.
pub(crate) fn to_rgb8(color: Color) -> (u8, u8, u8) {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    (q(color.r), q(color.g), q(color.b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_corners_match_xterm() {
        assert_eq!(to_rgb8(xterm_indexed(16)), (0, 0, 0));
        assert_eq!(to_rgb8(xterm_indexed(21)), (0, 0, 255));
        assert_eq!(to_rgb8(xterm_indexed(196)), (255, 0, 0));
        assert_eq!(to_rgb8(xterm_indexed(231)), (255, 255, 255));
        // Colour 17 is the first step of blue: 95, not 51.
        assert_eq!(to_rgb8(xterm_indexed(17)), (0, 0, 95));
    }

    #[test]
    fn grey_ramp_runs_from_8_to_238() {
        assert_eq!(to_rgb8(xterm_indexed(232)), (8, 8, 8));
        assert_eq!(to_rgb8(xterm_indexed(255)), (238, 238, 238));
    }

    #[test]
    fn a_theme_can_replace_the_cube() {
        let mut palette = Palette::default();
        let mut table = Box::new([Color::BLACK; 240]);
        table[0] = Color::WHITE;
        palette.indexed = Some(table);
        assert_eq!(palette.indexed(16), Color::WHITE);
        assert_eq!(palette.indexed(1), palette.ansi[1]);
    }
}
