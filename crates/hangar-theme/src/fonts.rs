//! The fonts of the window. Inter is the interface's face on every platform,
//! so that the window reads the same everywhere.
//!
//! iced draws text with cosmic-text, which reads TrueType and OpenType and not
//! WOFF2, so the `inter.woff2` the kits are served cannot be loaded; that
//! file is also a subset made for the web. The window uses Inter Variable as
//! the Inter project ships it, a TTF, from `fonts/` (SIL Open Font License
//! 1.1, `fonts/Inter-LICENSE.txt`, which has to travel with the font). Its
//! weight axis is what gives the medium (500) and semibold (600) the
//! stylesheets use from a single file.
//!
//! The app loads it once, and makes it the default:
//!
//! ```ignore
//! iced::application(App::new, App::update, App::view)
//!     .font(hangar_theme::fonts::INTER_VARIABLE)
//!     .default_font(hangar_theme::fonts::ui())
//! ```

use iced::Font;
use iced::font::{Family, Stretch, Style, Weight};

/// Inter Variable (TTF), for `Settings::fonts` or `Application::font`.
pub const INTER_VARIABLE: &[u8] = include_bytes!("../fonts/InterVariable.ttf");

/// The licence of the font above, to show in an about page.
pub const INTER_LICENSE: &str = include_str!("../fonts/Inter-LICENSE.txt");

/// Inter's family name as the font file declares it, what `font-family`
/// said in the web app (`"Inter Variable"`).
pub const UI_FAMILY: &str = "Inter Variable";

const fn inter(weight: Weight) -> Font {
    Font {
        family: Family::Name(UI_FAMILY),
        weight,
        stretch: Stretch::Normal,
        style: Style::Normal,
    }
}

/// The interface font at regular weight: the window's default.
pub const fn ui() -> Font {
    inter(Weight::Normal)
}

/// Buttons, tabs, segments (weight 500).
pub const fn ui_medium() -> Font {
    inter(Weight::Medium)
}

/// Titles, labels, a primary button (weight 600).
pub const fn ui_semibold() -> Font {
    inter(Weight::Semibold)
}

/// The interface font at a weight of CSS, as the stylesheets name them
/// (400, 500, 600, 700). The 550 and 650 the stylesheets use once each go to
/// semibold, since iced has only those four weights.
pub const fn ui_weight(css_weight: u16) -> Font {
    inter(match css_weight {
        0..=449 => Weight::Normal,
        450..=549 => Weight::Medium,
        550..=650 => Weight::Semibold,
        _ => Weight::Bold,
    })
}

/// What the interface sets in mono: versions, key hints, paths. The terminals
/// have a font of their own, from the settings.
pub const fn mono() -> Font {
    Font::MONOSPACE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_font_is_a_truetype_file_with_the_family_the_stylesheets_name() {
        // TrueType, not WOFF2 (`wOF2`), which cosmic-text cannot read.
        assert_eq!(&INTER_VARIABLE[..4], &[0, 1, 0, 0]);
        let needle = UI_FAMILY
            .encode_utf16()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>();
        assert!(
            INTER_VARIABLE
                .windows(needle.len())
                .any(|window| window == needle)
        );
    }

    #[test]
    fn the_licence_travels_with_the_font() {
        assert!(INTER_LICENSE.contains("SIL OPEN FONT LICENSE Version 1.1"));
    }

    #[test]
    fn weights_round_to_the_nearest_step() {
        assert_eq!(ui_weight(400).weight, Weight::Normal);
        assert_eq!(ui_weight(500).weight, Weight::Medium);
        assert_eq!(ui_weight(550).weight, Weight::Semibold);
        assert_eq!(ui_weight(600).weight, Weight::Semibold);
        assert_eq!(ui_weight(650).weight, Weight::Semibold);
        assert_eq!(ui_weight(700).weight, Weight::Bold);
    }
}
