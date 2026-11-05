//! The typeface a terminal is drawn in, and the size of the cells it implies.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use iced::font::{Family, Style, Weight};

/// How text is set: family, size and the height of a line.
///
/// A terminal is a grid, so the family has to be monospaced and the size of a
/// cell follows from it: the width of a cell is the advance of the font, the
/// height is `size * line_height` rounded to whole pixels. The app does not
/// have to compute either; the widget measures them and reports the resulting
/// number of columns and rows.
#[derive(Debug, Clone, PartialEq)]
pub struct Font {
    /// Families in order of preference. The first one installed on the machine
    /// is used, and the system's monospace font when none is, which is how a
    /// CSS font stack behaves.
    pub families: Vec<String>,
    /// The size of the text in logical pixels.
    pub size: f32,
    /// The height of a line as a multiple of `size`.
    pub line_height: f32,
    /// Extra space after every character, in logical pixels (`letterSpacing`
    /// of the old terminal; negative tightens). It widens the cell, not the
    /// glyph: each character is centred in a cell of the font's advance plus
    /// this, so the columns stay a grid and box drawing, backgrounds and the
    /// cursor follow. A terminal with a spacing sets every character on its
    /// own, since the font cannot add it to a run, and so cannot form
    /// ligatures.
    pub letter_spacing: f32,
    /// Whether the font's ligatures are applied (`->`, `!=`, `==>` in
    /// programming fonts). Off by default: ligatures change how many glyphs a
    /// run of cells shows, which most people do not want in a terminal, and
    /// they cost the cheaper shaping path.
    pub ligatures: bool,
}

impl Default for Font {
    fn default() -> Self {
        Self {
            families: Vec::new(),
            size: 13.0,
            line_height: 1.2,
            letter_spacing: 0.0,
            ligatures: false,
        }
    }
}

impl Font {
    /// The system monospace font at `size` logical pixels.
    pub fn monospace(size: f32) -> Self {
        Self {
            size,
            ..Self::default()
        }
    }

    /// A font from a CSS-style stack: `"Cascadia Code", "JetBrains Mono",
    /// Consolas, monospace`. Generic names (`monospace`, `ui-monospace`) end
    /// the list, since they mean "whatever the system has", which is what an
    /// empty list already does.
    ///
    /// This is the form of Hangar's `fontFamily` setting, so the app can pass
    /// it through without parsing it.
    pub fn from_css_stack(stack: &str, size: f32, line_height: f32) -> Self {
        let mut families = Vec::new();
        for part in stack.split(',') {
            let name = part.trim().trim_matches(|c| c == '"' || c == '\'').trim();
            if name.is_empty() {
                continue;
            }
            let generic = matches!(
                name.to_ascii_lowercase().as_str(),
                "monospace" | "ui-monospace" | "sans-serif" | "serif" | "system-ui"
            );
            if generic {
                break;
            }
            families.push(name.to_owned());
        }
        Self {
            families,
            size,
            line_height,
            ..Self::default()
        }
    }

    /// The same font with `px` of space after every character
    /// ([`letter_spacing`](Self::letter_spacing)). A value that is not a
    /// finite number counts as none, and the spacing is kept within what a
    /// cell can bear: a font cannot be squeezed past a third of its size
    /// without its glyphs overprinting, nor spread past its own size.
    pub fn with_letter_spacing(mut self, px: f32) -> Self {
        self.letter_spacing = if px.is_finite() {
            px.clamp(-self.size / 3.0, self.size)
        } else {
            0.0
        };
        self
    }

    /// Whether characters are set one by one because of a spacing.
    pub(crate) fn is_spaced(&self) -> bool {
        self.letter_spacing != 0.0
    }

    /// The height of a cell in logical pixels, rounded to a whole number of
    /// physical pixels at `scale`.
    ///
    /// Rounding is what keeps rows evenly spaced: with a fractional height,
    /// some rows would land a pixel higher than their neighbours and the text
    /// would look as if it jittered.
    pub(crate) fn cell_height(&self, scale: f32) -> f32 {
        let scale = if scale > 0.0 { scale } else { 1.0 };
        ((self.size * self.line_height * scale).round().max(1.0)) / scale
    }

    /// The `iced` font for a style in a family resolved against what is
    /// installed with [`family`](Self::family), which scans the font system
    /// and is best asked once for the four styles.
    pub(crate) fn resolve_with(family: Family, bold: bool, italic: bool) -> iced::Font {
        iced::Font {
            family,
            weight: if bold { Weight::Bold } else { Weight::Normal },
            style: if italic { Style::Italic } else { Style::Normal },
            ..iced::Font::DEFAULT
        }
    }

    /// The first family of the list the font system actually has.
    ///
    /// Asked of the font system rather than assumed: a named family that is
    /// missing would not fall back to a monospace font but to the default
    /// sans-serif one, and every column after the first would be misplaced.
    pub(crate) fn family(&self) -> Family {
        for name in &self.families {
            if installed(name) {
                return Family::Name(intern(name));
            }
        }
        Family::Monospace
    }
}

/// Whether the font system has a family called `name`.
fn installed(name: &str) -> bool {
    let mut system = iced::advanced::graphics::text::font_system()
        .write()
        .expect("font system lock");
    system.raw().db().faces().any(|face| {
        face.families
            .iter()
            .any(|(family, _)| family.eq_ignore_ascii_case(name))
    })
}

/// A `'static` copy of a family name.
///
/// `iced::Font` holds family names as `&'static str`, but a family chosen in
/// the settings is only known at run time. Each distinct name is leaked once;
/// the set of fonts a person tries out is small, so this stays bounded.
fn intern(name: &str) -> &'static str {
    static NAMES: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let mut names = NAMES
        .get_or_init(Default::default)
        .lock()
        .expect("names lock");
    if let Some(known) = names.get(name) {
        return known;
    }
    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
    names.insert(leaked);
    leaked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_css_stack_keeps_the_names_before_the_generic_one() {
        let font = Font::from_css_stack(
            "\"Cascadia Code\", 'JetBrains Mono', Consolas, \"Courier New\", monospace, Other",
            13.0,
            1.3,
        );
        assert_eq!(
            font.families,
            ["Cascadia Code", "JetBrains Mono", "Consolas", "Courier New"]
        );
    }

    #[test]
    fn rows_are_a_whole_number_of_physical_pixels() {
        let font = Font {
            size: 13.0,
            line_height: 1.3,
            ..Font::default()
        };
        // 16.9 logical pixels at scale 1 is 17; at scale 1.5 it is 25.35
        // physical pixels, so 25 of them, which is 16.666... logical.
        assert_eq!(font.cell_height(1.0), 17.0);
        assert!((font.cell_height(1.5) - 25.0 / 1.5).abs() < 1e-4);
    }

    #[test]
    fn a_letter_spacing_stays_within_what_a_cell_can_bear() {
        let font = Font::monospace(12.0);
        assert_eq!(font.letter_spacing, 0.0);
        assert!(!font.is_spaced());
        assert_eq!(font.clone().with_letter_spacing(1.5).letter_spacing, 1.5);
        assert!(font.clone().with_letter_spacing(-0.5).is_spaced());
        // Not a number, or a hand-edited extreme.
        assert_eq!(
            font.clone().with_letter_spacing(f32::NAN).letter_spacing,
            0.0
        );
        assert_eq!(
            font.clone()
                .with_letter_spacing(f32::INFINITY)
                .letter_spacing,
            0.0
        );
        assert_eq!(font.clone().with_letter_spacing(-50.0).letter_spacing, -4.0);
        assert_eq!(font.with_letter_spacing(500.0).letter_spacing, 12.0);
    }

    #[test]
    fn interning_gives_the_same_string_back() {
        assert!(std::ptr::eq(intern("Fira Code"), intern("Fira Code")));
    }
}
