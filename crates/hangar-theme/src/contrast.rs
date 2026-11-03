//! Colour arithmetic for the chrome's tokens: mixing, WCAG contrast and the
//! hue of a colour. A port of `src/lib/contrast.ts`.
//!
//! Everything works on [`Rgb`], an opaque colour of 8-bit channels, because
//! the TypeScript works on `#rrggbb` strings and rounds to 8 bits after every
//! step. Keeping that rounding is what makes [`crate::derive_colors`] give the
//! same tokens as `deriveChromeTokens` rather than ones a unit off.

use iced::Color;

/// An opaque colour of 8-bit channels, the `#rrggbb` of the web app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const WHITE: Rgb = Rgb(255, 255, 255);
    pub const BLACK: Rgb = Rgb(0, 0, 0);

    /// Reads `#rrggbb` or `#rgb`, the `#` being optional, as `toRgb` does.
    pub fn parse(hex: &str) -> Option<Rgb> {
        let clean = hex.strip_prefix('#').unwrap_or(hex);
        let channel = |digits: &str| u8::from_str_radix(digits, 16).ok();
        match clean.len() {
            3 => {
                let doubled = |at: usize| channel(&clean[at..=at].repeat(2));
                Some(Rgb(doubled(0)?, doubled(1)?, doubled(2)?))
            }
            6 if clean.is_ascii() => Some(Rgb(
                channel(&clean[0..2])?,
                channel(&clean[2..4])?,
                channel(&clean[4..6])?,
            )),
            _ => None,
        }
    }

    /// [`Rgb::parse`] for a literal that is known to be a colour.
    ///
    /// # Panics
    ///
    /// When `hex` is not a colour, so a typo in a table or a test fails loudly.
    pub fn hex(hex: &str) -> Rgb {
        Rgb::parse(hex).unwrap_or_else(|| panic!("{hex:?} is not a #rrggbb colour"))
    }

    /// `#rrggbb`, in lower case.
    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }

    /// The colour iced draws, fully opaque.
    pub fn to_color(self) -> Color {
        Color::from_rgb8(self.0, self.1, self.2)
    }

    fn channels(self) -> [f64; 3] {
        [self.0 as f64, self.1 as f64, self.2 as f64]
    }

    /// Back to 8 bits the way `toHex` does: clamped, then rounded.
    fn from_channels([r, g, b]: [f64; 3]) -> Rgb {
        let channel = |value: f64| value.clamp(0.0, 255.0).round() as u8;
        Rgb(channel(r), channel(g), channel(b))
    }
}

impl From<Rgb> for Color {
    fn from(rgb: Rgb) -> Color {
        rgb.to_color()
    }
}

impl From<Color> for Rgb {
    /// Drops the alpha: a translucent token is composited first (see [`over`]).
    fn from(color: Color) -> Rgb {
        let [r, g, b, _] = color.into_rgba8();
        Rgb(r, g, b)
    }
}

/// Blends two colours; `amount` is how much of `b` to mix in.
pub fn mix(a: Rgb, b: Rgb, amount: f64) -> Rgb {
    let [r1, g1, b1] = a.channels();
    let [r2, g2, b2] = b.channels();
    Rgb::from_channels([
        r1 + (r2 - r1) * amount,
        g1 + (g2 - g1) * amount,
        b1 + (b2 - b1) * amount,
    ])
}

/// `top` laid over the opaque `base` at its own alpha, as a page composites it.
pub fn over(top: Color, base: Rgb) -> Rgb {
    mix(base, Rgb::from(top), top.a as f64)
}

/// Relative luminance, as WCAG 2 defines it: 0 for black, 1 for white.
pub fn relative_luminance(color: Rgb) -> f64 {
    let [r, g, b] = color.channels().map(|value| {
        let c = value / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    });
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Lightness as the eye reads it, from 0 for black to 1 for white: OKLab's L.
///
/// WCAG's luminance answers "can it be read"; this answers "do two steps of a
/// ramp look apart", which luminance gets wrong in the darks.
pub fn perceived_lightness(color: Rgb) -> f64 {
    let [r, g, b] = color.channels().map(|value| {
        let c = value / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    });
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s
}

/// WCAG contrast ratio between two colours, from 1 to 21.
pub fn contrast_ratio(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// The contrast `color` keeps on the least favourable of `surfaces`.
pub fn worst_contrast(color: Rgb, surfaces: &[Rgb]) -> f64 {
    surfaces
        .iter()
        .map(|&surface| contrast_ratio(color, surface))
        .fold(f64::INFINITY, f64::min)
}

/// White or black, whichever stands further from every one of `surfaces`.
pub fn farthest_pole(surfaces: &[Rgb]) -> Rgb {
    if worst_contrast(Rgb::WHITE, surfaces) >= worst_contrast(Rgb::BLACK, surfaces) {
        Rgb::WHITE
    } else {
        Rgb::BLACK
    }
}

/// Steps from `color` to `toward`: fine enough that no token moves further
/// than it must.
const STEPS: u32 = 200;

/// `color` itself when it reads at `min`:1 on every one of `against`, or else
/// the least mix of it toward `toward` that does.
///
/// `toward` is white on dark surfaces and black on light ones unless given; a
/// caller passes the theme's foreground, say, to keep a grey's tint. When
/// nothing short of `toward` reads, `toward` is the best there is and comes
/// back as it is.
pub fn ensure_contrast(color: Rgb, against: &[Rgb], min: f64, toward: Option<Rgb>) -> Rgb {
    let target = toward.unwrap_or_else(|| farthest_pole(against));
    if worst_contrast(color, against) >= min {
        return color;
    }
    for step in 1..STEPS {
        let candidate = mix(color, target, step as f64 / STEPS as f64);
        if worst_contrast(candidate, against) >= min {
            return candidate;
        }
    }
    target
}

/// Hue (0 to 360, degrees) and HSL saturation (0 to 1) of a colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HueAndSaturation {
    pub hue: f64,
    pub saturation: f64,
}

pub fn hue_and_saturation(color: Rgb) -> HueAndSaturation {
    let [r, g, b] = color.channels().map(|value| value / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let chroma = max - min;
    if chroma == 0.0 {
        return HueAndSaturation {
            hue: 0.0,
            saturation: 0.0,
        };
    }
    let sector = if max == r {
        ((g - b) / chroma + 6.0) % 6.0
    } else if max == g {
        (b - r) / chroma + 2.0
    } else {
        (r - g) / chroma + 4.0
    };
    let lightness = (max + min) / 2.0;
    HueAndSaturation {
        hue: sector * 60.0,
        saturation: chroma / (1.0 - (2.0 * lightness - 1.0).abs()),
    }
}

/// How far apart two hues sit on the colour wheel, from 0 to 180 degrees.
pub fn hue_distance(a: f64, b: f64) -> f64 {
    let d = (a - b).abs() % 360.0;
    if d > 180.0 { 360.0 - d } else { d }
}

/// `color` turned to `hue`, its saturation and lightness kept.
pub fn with_hue(color: Rgb, hue: f64) -> Rgb {
    let [r, g, b] = color.channels().map(|value| value / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let lightness = (max + min) / 2.0;
    let chroma = max - min;
    let h = (hue % 360.0 + 360.0) % 360.0 / 60.0;
    let x = chroma * (1.0 - (h % 2.0 - 1.0).abs());
    let (r1, g1, b1) = if h < 1.0 {
        (chroma, x, 0.0)
    } else if h < 2.0 {
        (x, chroma, 0.0)
    } else if h < 3.0 {
        (0.0, chroma, x)
    } else if h < 4.0 {
        (0.0, x, chroma)
    } else if h < 5.0 {
        (x, 0.0, chroma)
    } else {
        (chroma, 0.0, x)
    };
    let m = lightness - chroma / 2.0;
    Rgb::from_channels([(r1 + m) * 255.0, (g1 + m) * 255.0, (b1 + m) * 255.0])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(value: &str) -> Rgb {
        Rgb::hex(value)
    }

    fn close(a: f64, b: f64, digits: i32) -> bool {
        (a - b).abs() < 10f64.powi(-digits) / 2.0
    }

    #[test]
    fn parses_and_prints_hex() {
        assert_eq!(Rgb::parse("#c8c9ce"), Some(Rgb(0xc8, 0xc9, 0xce)));
        assert_eq!(Rgb::parse("fff"), Some(Rgb(255, 255, 255)));
        assert_eq!(Rgb::parse("#12"), None);
        assert_eq!(Rgb::parse("#gggggg"), None);
        assert_eq!(Rgb(0x12, 0xab, 0x00).to_hex(), "#12ab00");
    }

    mod relative_luminance {
        use super::*;

        #[test]
        fn runs_from_0_for_black_to_1_for_white() {
            assert_eq!(relative_luminance(hex("#000000")), 0.0);
            assert!(close(relative_luminance(hex("#ffffff")), 1.0, 10));
        }

        #[test]
        fn weighs_green_far_above_blue_as_the_eye_does() {
            assert!(relative_luminance(hex("#00ff00")) > relative_luminance(hex("#0000ff")) * 9.0);
        }
    }

    mod contrast_ratio {
        use super::*;

        #[test]
        fn runs_from_1_to_21_whichever_colour_comes_first() {
            assert!(close(
                contrast_ratio(hex("#000000"), hex("#ffffff")),
                21.0,
                5
            ));
            assert!(close(
                contrast_ratio(hex("#ffffff"), hex("#000000")),
                21.0,
                5
            ));
            assert_eq!(contrast_ratio(hex("#777777"), hex("#777777")), 1.0);
        }

        #[test]
        fn matches_the_published_wcag_figures() {
            // #767676 is the classic lightest grey that passes AA on white.
            assert!(close(
                contrast_ratio(hex("#767676"), hex("#ffffff")),
                4.54,
                2
            ));
            assert!(contrast_ratio(hex("#777777"), hex("#ffffff")) < 4.5);
        }
    }

    mod worst_contrast_and_farthest_pole {
        use super::*;

        #[test]
        fn measures_a_colour_on_the_least_favourable_surface() {
            let ratio = worst_contrast(hex("#ffffff"), &[hex("#000000"), hex("#555555")]);
            assert!(close(
                ratio,
                contrast_ratio(hex("#ffffff"), hex("#555555")),
                10
            ));
        }

        #[test]
        fn goes_to_white_from_dark_surfaces_and_to_black_from_light_ones() {
            assert_eq!(
                farthest_pole(&[hex("#121316"), hex("#232428")]),
                hex("#ffffff")
            );
            assert_eq!(
                farthest_pole(&[hex("#ffffff"), hex("#ededed")]),
                hex("#000000")
            );
        }
    }

    mod ensure_contrast {
        use super::*;

        #[test]
        fn leaves_a_colour_that_already_reads_as_it_is() {
            assert_eq!(
                ensure_contrast(hex("#c8c9ce"), &[hex("#121316")], 4.5, None),
                hex("#c8c9ce")
            );
        }

        #[test]
        fn lightens_on_a_dark_surface_just_enough() {
            let surface = hex("#222222");
            let color = ensure_contrast(hex("#555555"), &[surface], 4.5, None);
            assert!(contrast_ratio(color, surface) >= 4.5);
            assert!(relative_luminance(color) > relative_luminance(hex("#555555")));
            // The least mix that reads: half a step less does not.
            let lighter = relative_luminance(color);
            assert!(lighter < relative_luminance(mix(hex("#555555"), hex("#ffffff"), 0.6)));
        }

        #[test]
        fn darkens_on_a_light_surface() {
            let surface = hex("#ececec");
            let color = ensure_contrast(hex("#f2ae49"), &[surface], 4.5, None);
            assert!(contrast_ratio(color, surface) >= 4.5);
            assert!(relative_luminance(color) < relative_luminance(hex("#f2ae49")));
        }

        #[test]
        fn keeps_the_hue_while_it_moves() {
            let color = ensure_contrast(hex("#f2ae49"), &[hex("#ececec")], 4.5, None);
            let moved = hue_and_saturation(color).hue;
            assert!(hue_distance(moved, hue_and_saturation(hex("#f2ae49")).hue) < 2.0);
        }

        #[test]
        fn reads_on_every_surface_it_is_given() {
            let surfaces = [hex("#1b1c1f"), hex("#202124"), hex("#232428")];
            let color = ensure_contrast(hex("#6d6e72"), &surfaces, 4.5, None);
            for surface in surfaces {
                assert!(contrast_ratio(color, surface) >= 4.5);
            }
        }

        #[test]
        fn leans_toward_the_colour_it_is_given_instead_of_white_or_black() {
            let surface = hex("#1a1b26");
            let color = ensure_contrast(hex("#3a4060"), &[surface], 4.5, Some(hex("#c0caf5")));
            assert!(contrast_ratio(color, surface) >= 4.5);
            // On the way to the lavender foreground, blue stays the strongest
            // channel.
            assert!(color.2 > color.0);
        }

        #[test]
        fn falls_back_on_the_target_when_nothing_short_of_it_reads() {
            assert_eq!(
                ensure_contrast(hex("#808080"), &[hex("#808080")], 4.5, Some(hex("#999999"))),
                hex("#999999")
            );
        }
    }

    mod hues {
        use super::*;

        #[test]
        fn reads_the_hue_and_saturation_of_a_colour() {
            assert_eq!(
                hue_and_saturation(hex("#ff0000")),
                HueAndSaturation {
                    hue: 0.0,
                    saturation: 1.0
                }
            );
            assert_eq!(hue_and_saturation(hex("#00ff00")).hue, 120.0);
            assert_eq!(hue_and_saturation(hex("#0000ff")).hue, 240.0);
            assert_eq!(hue_and_saturation(hex("#777777")).saturation, 0.0);
        }

        #[test]
        fn measures_the_short_way_round_the_wheel() {
            assert_eq!(hue_distance(350.0, 10.0), 20.0);
            assert_eq!(hue_distance(10.0, 350.0), 20.0);
            assert_eq!(hue_distance(0.0, 180.0), 180.0);
        }

        #[test]
        fn turns_a_colour_to_a_hue_and_keeps_the_rest() {
            assert_eq!(with_hue(hex("#ff0000"), 120.0), hex("#00ff00"));
            let turned = with_hue(hex("#6fd3df"), 200.0);
            assert!(close(hue_and_saturation(turned).hue, 200.0, 0));
            assert!(close(
                hue_and_saturation(turned).saturation,
                hue_and_saturation(hex("#6fd3df")).saturation,
                1
            ));
        }
    }
}
