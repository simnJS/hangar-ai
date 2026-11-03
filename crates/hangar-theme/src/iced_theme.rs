//! A Hangar theme as an `iced::Theme`.
//!
//! The [`crate::style`] functions draw Hangar's own widgets from the tokens
//! directly. This theme is for everything iced styles by itself (a slider, a
//! text editor, markdown, a combo box, the window's background and default
//! text colour), so that those follow the theme and look like they belong to
//! the window rather than like iced's stock dark blue.

use iced::Color;
use iced::theme::Palette;
use iced::theme::palette::{
    Background, Danger, Extended, Pair, Primary, Secondary, Success, Warning,
};

use crate::contrast::{Rgb, contrast_ratio, mix};
use crate::{Colors, Theme};

/// The `iced::Theme` for a Hangar theme: a custom palette from its page,
/// text, accent and status colours, and an extended palette laid out on the
/// chrome's own surfaces instead of the ones iced would compute from the
/// palette.
///
/// Build it once when the theme changes and return it from the application's
/// `theme` function; it allocates.
pub fn to_iced(theme: &Theme) -> iced::Theme {
    let c = &theme.colors;
    let palette = Palette {
        background: c.bg,
        text: c.fg,
        primary: c.accent,
        success: c.success,
        warning: c.warning,
        danger: c.danger,
    };
    let extended = extended(theme);
    iced::Theme::custom_with_fn(theme.name, palette, move |_| extended)
}

/// Whichever of the foreground, the page and the text-on-accent colour
/// reads best on `color`, so every pair of the palette is legible.
fn on(c: &Colors, color: Color) -> Color {
    let candidates = [c.fg, c.bg, c.accent_fg];
    let background = Rgb::from(color);
    candidates
        .into_iter()
        .max_by(|a, b| {
            contrast_ratio(Rgb::from(*a), background)
                .total_cmp(&contrast_ratio(Rgb::from(*b), background))
        })
        .unwrap_or(c.fg)
}

fn pair(c: &Colors, color: Color) -> Pair {
    Pair::new(color, on(c, color))
}

/// A status colour in iced's three strengths: the colour itself, a tint of
/// it on the page for hovers and backgrounds, and the brighter text variant
/// for emphasis.
fn status(c: &Colors, base: Color, text_variant: Color) -> (Pair, Pair, Pair) {
    let tint = Color::from(mix(Rgb::from(c.bg), Rgb::from(base), 0.25));
    (pair(c, base), pair(c, tint), pair(c, text_variant))
}

fn extended(theme: &Theme) -> Extended {
    let c = &theme.colors;
    let (success_base, success_weak, success_strong) = status(c, c.success, c.state_working_fill);
    let (warning_base, warning_weak, warning_strong) = status(c, c.warning, c.state_waiting_fill);
    let (danger_base, danger_weak, danger_strong) = status(c, c.danger, c.danger);
    let (_, primary_weak, primary_strong) = status(c, c.accent, c.accent_hi);

    Extended {
        // The surfaces in the order they step away from the page, which is the
        // order iced's names run in: the toolbar's and the selection's
        // colours are the middle steps, and the last is a quiet text grey
        // that carries the page's colour as its text.
        background: Background {
            base: Pair::new(c.bg, c.fg),
            weakest: pair(c, c.pane_head),
            weaker: pair(c, c.chrome),
            weak: pair(c, c.raised),
            neutral: pair(c, c.surface_hover),
            strong: pair(c, c.surface_active),
            stronger: pair(c, c.border_strong),
            strongest: Pair::new(c.fg_muted, c.bg),
        },
        primary: Primary {
            base: Pair::new(c.accent, c.accent_fg),
            weak: primary_weak,
            strong: primary_strong,
        },
        // A neutral button: Hangar's secondary is the raised surface.
        secondary: Secondary {
            base: pair(c, c.raised),
            weak: pair(c, c.chrome_bar),
            strong: pair(c, c.surface_hover),
        },
        success: Success {
            base: success_base,
            weak: success_weak,
            strong: success_strong,
        },
        warning: Warning {
            base: warning_base,
            weak: warning_weak,
            strong: warning_strong,
        },
        danger: Danger {
            base: danger_base,
            weak: danger_weak,
            strong: danger_strong,
        },
        is_dark: theme.is_dark(),
    }
}

impl From<&Theme> for iced::Theme {
    fn from(theme: &Theme) -> iced::Theme {
        to_iced(theme)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{by_id, themes};

    #[test]
    fn the_palette_is_the_themes_page_text_accent_and_statuses() {
        let theme = by_id("graphite").unwrap();
        let iced = to_iced(theme);
        let palette = iced.palette();
        assert_eq!(palette.background, theme.colors.bg);
        assert_eq!(palette.text, theme.colors.fg);
        assert_eq!(palette.primary, theme.colors.accent);
        assert_eq!(palette.danger, theme.colors.danger);
        assert_eq!(iced.to_string(), "Graphite");
    }

    #[test]
    fn the_extended_palette_knows_if_the_theme_is_dark() {
        for theme in themes() {
            assert_eq!(
                to_iced(theme).extended_palette().is_dark,
                theme.is_dark(),
                "{}",
                theme.id
            );
        }
    }

    #[test]
    fn every_pair_of_the_extended_palette_is_legible() {
        for theme in themes() {
            let iced = to_iced(theme);
            let e = iced.extended_palette();
            let background = [
                e.background.base,
                e.background.weakest,
                e.background.weaker,
                e.background.weak,
                e.background.neutral,
                e.background.strong,
                e.background.stronger,
                e.background.strongest,
            ];
            let others = [
                e.primary.base,
                e.primary.weak,
                e.primary.strong,
                e.secondary.base,
                e.secondary.weak,
                e.secondary.strong,
                e.success.base,
                e.success.weak,
                e.success.strong,
                e.warning.base,
                e.warning.weak,
                e.warning.strong,
                e.danger.base,
                e.danger.weak,
                e.danger.strong,
            ];
            for pair in background.into_iter().chain(others) {
                let ratio = contrast_ratio(Rgb::from(pair.text), Rgb::from(pair.color));
                // The status colours are held to 3:1 as fills, and a pair of
                // them is a fill and its label: the floor for large text.
                assert!(ratio >= 3.0, "{}: {ratio:.2} on {:?}", theme.id, pair.color);
            }
        }
    }
}
