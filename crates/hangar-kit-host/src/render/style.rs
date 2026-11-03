//! The colours and faces kits are drawn in. Hangar UI's tokens, by their
//! names (docs/kit-ui.md, "Tokens"), with the values of its two default
//! themes, graphite and github-light (crates/hangar-core/kit-sdk/kit.css).
//!
//! The app gives a [`KitStyle`] made from the theme in use. Until
//! hangar-theme exists it can take [`KitStyle::dark`] or
//! [`KitStyle::light`]; once it does, it fills the same struct from the
//! theme's chrome tokens, the map tokens.json already writes, and this crate
//! does not change.

use iced::{Color, Font, font};

use hangar_kit_ui::{Status, Surface, Tone};

/// Hangar UI's colour tokens, `--kit-*` without the prefix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub bg: Color,
    pub surface: Color,
    pub raised: Color,
    pub float: Color,
    pub field: Color,
    pub well: Color,
    pub hover: Color,
    pub selected: Color,
    pub seg_track: Color,
    pub seg_thumb: Color,
    pub text: Color,
    pub text_dim: Color,
    pub text_muted: Color,
    pub text_faint: Color,
    pub line: Color,
    pub border: Color,
    pub border_hover: Color,
    pub sep: Color,
    pub accent: Color,
    pub accent_text: Color,
    pub accent_soft: Color,
    pub accent_ring: Color,
    pub on_accent: Color,
    pub focus: Color,
    pub ok: Color,
    pub ok_fill: Color,
    pub warn: Color,
    pub warn_fill: Color,
    pub danger: Color,
    pub danger_soft: Color,
    pub neutral: Color,
    pub neutral_fill: Color,
    pub shadow: Color,
}

const fn rgb(hex: u32) -> Color {
    Color::from_rgb8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::from_rgba8(r, g, b, a)
}

impl Palette {
    /// graphite, Hangar's default dark theme.
    pub const DARK: Palette = Palette {
        bg: rgb(0x1b1c1f),
        surface: rgb(0x232426),
        raised: rgb(0x232426),
        float: rgb(0x202124),
        field: rgb(0x121316),
        well: rgb(0x0c0c0e),
        hover: rgb(0x252629),
        selected: rgb(0x313234),
        seg_track: rgb(0x121316),
        seg_thumb: rgb(0x3d3e41),
        text: rgb(0xc8c9ce),
        text_dim: rgb(0x9c9da2),
        text_muted: rgb(0x8c8d91),
        text_faint: rgb(0x898a8e),
        line: rgba(255, 255, 255, 0.07),
        border: rgba(255, 255, 255, 0.12),
        border_hover: rgb(0x414245),
        sep: rgb(0x090a0b),
        accent: rgb(0x3e8bff),
        accent_text: rgb(0x68a5ff),
        accent_soft: rgba(62, 139, 255, 0.14),
        accent_ring: rgba(62, 139, 255, 0.45),
        on_accent: rgb(0x0b192e),
        focus: rgb(0x68a5ff),
        ok: rgb(0x5fd27f),
        ok_fill: rgb(0x5fd27f),
        warn: rgb(0xe7c06b),
        warn_fill: rgb(0xe7c06b),
        danger: rgb(0xff7b72),
        danger_soft: rgba(255, 123, 114, 0.14),
        neutral: rgb(0x999a9f),
        neutral_fill: rgb(0x7b7c80),
        shadow: rgba(0, 0, 0, 0.45),
    };

    /// github-light, Hangar's default light theme.
    pub const LIGHT: Palette = Palette {
        bg: rgb(0xf5f5f5),
        surface: rgb(0xffffff),
        raised: rgb(0xededed),
        float: rgb(0xf0f0f0),
        field: rgb(0xffffff),
        well: rgb(0xffffff),
        hover: rgb(0xebebeb),
        selected: rgb(0xdedede),
        seg_track: rgb(0xdedede),
        seg_thumb: rgb(0xffffff),
        text: rgb(0x24292f),
        text_dim: rgb(0x595c61),
        text_muted: rgb(0x686c71),
        text_faint: rgb(0x696c71),
        line: rgba(0, 0, 0, 0.08),
        border: rgba(0, 0, 0, 0.14),
        border_hover: rgb(0xcccccc),
        sep: rgb(0xdbdbdb),
        accent: rgb(0x0969da),
        accent_text: rgb(0x085cc0),
        accent_soft: rgba(9, 105, 218, 0.14),
        accent_ring: rgba(9, 105, 218, 0.45),
        on_accent: rgb(0xffffff),
        focus: rgb(0x085cc0),
        ok: rgb(0x116329),
        ok_fill: rgb(0x116329),
        warn: rgb(0x9b4708),
        warn_fill: rgb(0xb45309),
        danger: rgb(0xba1f29),
        danger_soft: rgba(186, 31, 41, 0.14),
        neutral: rgb(0x5c6065),
        neutral_fill: rgb(0x797b7d),
        shadow: rgba(0, 0, 0, 0.12),
    };
}

/// What a kit's widgets are drawn with: the theme's tokens, and Hangar's two
/// faces. Copied into every style closure, so it is small and `Copy`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KitStyle {
    pub palette: Palette,
    /// The interface's face, Inter.
    pub font: Font,
    /// The terminals' face.
    pub mono: Font,
}

impl KitStyle {
    pub fn dark() -> Self {
        KitStyle {
            palette: Palette::DARK,
            font: Font::DEFAULT,
            mono: Font::MONOSPACE,
        }
    }

    pub fn light() -> Self {
        KitStyle {
            palette: Palette::LIGHT,
            font: Font::DEFAULT,
            mono: Font::MONOSPACE,
        }
    }

    pub(crate) fn tone(&self, tone: Tone) -> Color {
        let p = &self.palette;
        match tone {
            Tone::Default => p.text,
            Tone::Dim => p.text_dim,
            Tone::Muted => p.text_muted,
            Tone::Faint => p.text_faint,
            Tone::Accent => p.accent_text,
            Tone::Ok => p.ok,
            Tone::Warn => p.warn,
            Tone::Danger => p.danger,
            Tone::Neutral => p.neutral,
        }
    }

    /// A state as a word, at 4.5:1.
    pub(crate) fn status_text(&self, status: Status) -> Color {
        let p = &self.palette;
        match status {
            Status::Running => p.accent_text,
            Status::Success => p.ok,
            Status::Failure => p.danger,
            Status::Waiting => p.warn,
            Status::Neutral => p.neutral,
        }
    }

    /// A state as a shape or a bar, at 3:1.
    pub(crate) fn status_fill(&self, status: Status) -> Color {
        let p = &self.palette;
        match status {
            Status::Running => p.accent,
            Status::Success => p.ok_fill,
            Status::Failure => p.danger,
            Status::Waiting => p.warn_fill,
            Status::Neutral => p.neutral_fill,
        }
    }

    pub(crate) fn surface(&self, surface: Surface) -> Option<Color> {
        let p = &self.palette;
        match surface {
            Surface::None => None,
            Surface::Card => Some(p.surface),
            Surface::Raised => Some(p.raised),
            Surface::Float => Some(p.float),
            Surface::Field => Some(p.field),
            Surface::Well => Some(p.well),
            Surface::Selected => Some(p.selected),
            Surface::Accent => Some(p.accent_soft),
            Surface::Danger => Some(p.danger_soft),
        }
    }

    pub(crate) fn weighted(&self, weight: hangar_kit_ui::Weight, mono: bool) -> Font {
        let base = if mono { self.mono } else { self.font };
        Font {
            weight: match weight {
                hangar_kit_ui::Weight::Normal => font::Weight::Normal,
                hangar_kit_ui::Weight::Medium => font::Weight::Medium,
                hangar_kit_ui::Weight::Semibold => font::Weight::Semibold,
                hangar_kit_ui::Weight::Bold => font::Weight::Bold,
            },
            ..base
        }
    }
}

impl Default for KitStyle {
    fn default() -> Self {
        KitStyle::dark()
    }
}
