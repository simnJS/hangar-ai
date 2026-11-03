//! The viewport's colours. The crate does not know Hangar's themes: the app
//! hands in the handful of colours the window around the viewport is drawn
//! with, and the floor, the axes and the wireframe are derived from them so
//! they sit in the same palette as the panel.

use iced::Color;

/// Blends two colours in sRGB; `amount` is how much of `b` to mix in. The
/// chrome's own tokens are mixed the same way (`mix` in `src/lib/contrast.ts`),
/// so the grid lines come out the shade the old viewer drew them in.
pub fn mix(a: Color, b: Color, amount: f32) -> Color {
    Color::from_rgb(
        a.r + (b.r - a.r) * amount,
        a.g + (b.g - a.g) * amount,
        a.b + (b.b - a.b) * amount,
    )
}

/// One sRGB channel to linear light.
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// An sRGB colour as linear RGB, which is what the shaders compute in.
pub fn to_linear(color: Color) -> [f32; 3] {
    [
        srgb_to_linear(color.r),
        srgb_to_linear(color.g),
        srgb_to_linear(color.b),
    ]
}

/// The theme as the viewport draws it. All colours are sRGB, as the window's
/// own are; alpha is ignored.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewerTheme {
    pub dark: bool,
    pub background: Color,
    pub grid_minor: Color,
    pub grid_major: Color,
    pub axis_x: Color,
    pub axis_y: Color,
    pub axis_z: Color,
    pub wire: Color,
    pub normals: Color,
}

impl ViewerTheme {
    /// The viewport's colours from the chrome's tokens: the window background
    /// and foreground, the accent, and the danger and success colours.
    pub fn derive(
        background: Color,
        foreground: Color,
        accent: Color,
        danger: Color,
        success: Color,
        dark: bool,
    ) -> ViewerTheme {
        ViewerTheme {
            dark,
            background,
            // Lines a little off the background, the major ones a step
            // further: present, never louder than the model.
            grid_minor: mix(background, foreground, if dark { 0.1 } else { 0.09 }),
            grid_major: mix(background, foreground, if dark { 0.2 } else { 0.18 }),
            // Red, green and blue for X, Y and Z, as in every DCC tool: the
            // theme's own danger, success and accent, which are those hues on
            // every theme.
            axis_x: danger,
            axis_y: success,
            axis_z: accent,
            wire: mix(foreground, background, if dark { 0.1 } else { 0.25 }),
            normals: accent,
        }
    }

    /// A dark theme with the old viewer's fallbacks, for an app that has not
    /// got a palette to give yet.
    pub fn dark() -> ViewerTheme {
        ViewerTheme::derive(
            rgb(0x1b1c1f),
            rgb(0xe6e6e6),
            rgb(0x4f8cff),
            rgb(0xe5484d),
            rgb(0x46a758),
            true,
        )
    }

    /// The light counterpart of [`ViewerTheme::dark`].
    pub fn light() -> ViewerTheme {
        ViewerTheme::derive(
            rgb(0xffffff),
            rgb(0x1f2328),
            rgb(0x4f8cff),
            rgb(0xe5484d),
            rgb(0x46a758),
            false,
        )
    }

    /// How opaque the wireframe is drawn over the shaded model.
    pub fn wire_opacity(&self) -> f32 {
        if self.dark { 0.32 } else { 0.42 }
    }

    pub fn grid_opacity(&self) -> f32 {
        if self.dark { 0.9 } else { 1.0 }
    }
}

impl Default for ViewerTheme {
    fn default() -> Self {
        ViewerTheme::dark()
    }
}

fn rgb(hex: u32) -> Color {
    Color::from_rgb8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(color: Color) -> [u8; 3] {
        let c = color.into_rgba8();
        [c[0], c[1], c[2]]
    }

    #[test]
    fn mixes_in_srgb_toward_the_second_colour() {
        let black = Color::BLACK;
        let white = Color::WHITE;
        assert_eq!(bytes(mix(black, white, 0.0)), [0, 0, 0]);
        assert_eq!(bytes(mix(black, white, 1.0)), [255, 255, 255]);
        assert_eq!(bytes(mix(black, white, 0.5)), [128, 128, 128]);
    }

    #[test]
    fn derives_the_floor_from_the_background_and_foreground() {
        let theme = ViewerTheme::dark();
        // 0x1b + (0xe6 - 0x1b) * 0.1 = 0x2f.
        assert_eq!(bytes(theme.grid_minor)[0], 0x2f);
        assert_eq!(bytes(theme.grid_major)[0], 0x44);
        assert!(theme.grid_major.r > theme.grid_minor.r);
        let light = ViewerTheme::light();
        assert!(light.grid_major.r < light.grid_minor.r);
    }

    #[test]
    fn takes_the_axes_from_danger_success_and_accent() {
        let theme = ViewerTheme::dark();
        assert_eq!(bytes(theme.axis_x), [0xe5, 0x48, 0x4d]);
        assert_eq!(bytes(theme.axis_y), [0x46, 0xa7, 0x58]);
        assert_eq!(bytes(theme.axis_z), bytes(theme.normals));
    }

    #[test]
    fn converts_srgb_to_linear_light() {
        assert_eq!(srgb_to_linear(0.0), 0.0);
        assert!((srgb_to_linear(1.0) - 1.0).abs() < 1e-6);
        assert!((srgb_to_linear(0.5) - 0.214).abs() < 1e-3);
    }
}
