//! The colours, faces and words kits are drawn in: Hangar UI's tokens, by
//! their names (docs/kit-ui.md, "Tokens"), taken from the theme in use, the
//! window's two faces, and the person's language.
//!
//! Each `--kit-*` token is the chrome token `crates/hangar-core/kit-sdk/
//! tokens.json` maps it to, the same that an HTML tab's theme event carries
//! (hangar-theme's `kit_tokens`); a test holds the two to the same values on
//! every theme, so a native tab and a page of the same kit look alike.

use std::borrow::Cow;

use iced::{Color, Font, font};

use hangar_i18n::{Key, Locale};
use hangar_kit_ui::{Status, Surface, Tone};
use hangar_theme::Theme;

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

impl Palette {
    /// The tokens of `theme`, each from the chrome token `tokens.json` names
    /// for it.
    pub fn of(theme: &Theme) -> Palette {
        let c = &theme.colors;
        Palette {
            bg: c.chrome,
            surface: c.card,
            raised: c.raised,
            float: c.chrome_bar,
            field: c.bg,
            well: c.bg_sunken,
            hover: c.surface_hover,
            selected: c.surface_active,
            seg_track: c.seg_track,
            seg_thumb: c.seg_thumb,
            text: c.fg,
            text_dim: c.fg_secondary,
            text_muted: c.fg_muted,
            text_faint: c.fg_subtle,
            line: c.line,
            border: c.line_strong,
            border_hover: c.border_strong,
            sep: c.sep,
            accent: c.accent,
            accent_text: c.accent_hi,
            accent_soft: c.accent_soft,
            accent_ring: c.accent_ring,
            on_accent: c.accent_fg,
            focus: c.accent_hi,
            ok: c.success,
            ok_fill: c.state_working_fill,
            warn: c.warning,
            warn_fill: c.state_waiting_fill,
            danger: c.danger,
            danger_soft: c.danger_soft,
            neutral: c.state_idle,
            neutral_fill: c.state_idle_fill,
            shadow: c.shadow,
        }
    }

    /// The tokens by their `--kit-*` names, to hold against `kit_tokens`.
    #[cfg(test)]
    fn named(&self) -> Vec<(&'static str, Color)> {
        vec![
            ("--kit-bg", self.bg),
            ("--kit-surface", self.surface),
            ("--kit-raised", self.raised),
            ("--kit-float", self.float),
            ("--kit-field", self.field),
            ("--kit-well", self.well),
            ("--kit-hover", self.hover),
            ("--kit-selected", self.selected),
            ("--kit-seg-track", self.seg_track),
            ("--kit-seg-thumb", self.seg_thumb),
            ("--kit-text", self.text),
            ("--kit-text-dim", self.text_dim),
            ("--kit-text-muted", self.text_muted),
            ("--kit-text-faint", self.text_faint),
            ("--kit-line", self.line),
            ("--kit-border", self.border),
            ("--kit-border-hover", self.border_hover),
            ("--kit-sep", self.sep),
            ("--kit-accent", self.accent),
            ("--kit-accent-text", self.accent_text),
            ("--kit-accent-soft", self.accent_soft),
            ("--kit-accent-ring", self.accent_ring),
            ("--kit-on-accent", self.on_accent),
            ("--kit-focus", self.focus),
            ("--kit-ok", self.ok),
            ("--kit-ok-fill", self.ok_fill),
            ("--kit-warn", self.warn),
            ("--kit-warn-fill", self.warn_fill),
            ("--kit-danger", self.danger),
            ("--kit-danger-soft", self.danger_soft),
            ("--kit-neutral", self.neutral),
            ("--kit-neutral-fill", self.neutral_fill),
            ("--kit-shadow", self.shadow),
        ]
    }
}

/// What a kit's widgets are drawn with: the theme in use (hangar-ui's
/// widgets take it as it is), its tokens, Hangar's two faces, and the
/// language of the words Hangar says itself (a tab that could not start, a
/// widget this Hangar does not know). Copied into every style closure, so it
/// is small and `Copy`.
#[derive(Debug, Clone, Copy)]
pub struct KitStyle {
    pub theme: &'static Theme,
    pub palette: Palette,
    /// The interface's face, Inter.
    pub font: Font,
    /// What the interface sets in mono.
    pub mono: Font,
    pub locale: Locale,
}

impl PartialEq for KitStyle {
    fn eq(&self, other: &Self) -> bool {
        self.theme.id == other.theme.id
            && self.palette == other.palette
            && self.font == other.font
            && self.mono == other.mono
            && self.locale == other.locale
    }
}

impl KitStyle {
    /// The style of `theme`, in `locale`.
    pub fn new(theme: &'static Theme, locale: Locale) -> Self {
        KitStyle {
            theme,
            palette: Palette::of(theme),
            font: hangar_theme::fonts::ui(),
            mono: hangar_theme::fonts::mono(),
            locale,
        }
    }

    /// Graphite, the theme kits are shown in outside Hangar when dark.
    pub fn dark() -> Self {
        KitStyle::new(default_theme(true), Locale::En)
    }

    /// GitHub Light, the same when light.
    pub fn light() -> Self {
        KitStyle::new(default_theme(false), Locale::En)
    }

    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    /// A word of Hangar's own, in the tab's language.
    pub(crate) fn t(&self, key: Key) -> Cow<'static, str> {
        hangar_i18n::t(self.locale, key)
    }

    pub(crate) fn tone(&self, tone: Tone) -> Color {
        let p = &self.palette;
        match tone {
            Tone::Default => p.text,
            Tone::Dim => p.text_dim,
            // A fifth of the way back to the text's own colour: the theme's
            // muted and faint tones were set for a browser's subpixel
            // smoothing, and in grey they fade on a kit's small sizes.
            Tone::Muted => toward(p.text_muted, p.text, 0.2),
            Tone::Faint => toward(p.text_faint, p.text, 0.2),
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

/// The theme `tokens.json` names for a page opened outside Hangar, in each
/// scheme.
fn default_theme(dark: bool) -> &'static Theme {
    let (dark_id, light_id) = hangar_theme::kit_default_themes();
    hangar_theme::by_id_or_default(if dark { dark_id } else { light_id })
}

impl Default for KitStyle {
    fn default() -> Self {
        KitStyle::dark()
    }
}

/// `from` moved `amount` of the way to `to`, alpha included.
fn toward(from: Color, to: Color, amount: f32) -> Color {
    Color {
        r: from.r + (to.r - from.r) * amount,
        g: from.g + (to.g - from.g) * amount,
        b: from.b + (to.b - from.b) * amount,
        a: from.a + (to.a - from.a) * amount,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_native_tab_has_the_colours_an_html_tab_of_the_same_theme_is_told() {
        for theme in hangar_theme::themes() {
            let told = hangar_theme::kit_tokens(theme);
            for (name, colour) in Palette::of(theme).named() {
                let sent = told
                    .iter()
                    .find(|(token, _)| token == name)
                    .map(|(_, value)| value.as_str());
                assert_eq!(
                    sent,
                    Some(hangar_theme::css(colour).as_str()),
                    "{name} on {}",
                    theme.id
                );
            }
        }
    }

    #[test]
    fn the_defaults_are_the_themes_the_kits_stylesheet_holds() {
        assert_eq!(KitStyle::dark().theme.id, "graphite");
        assert_eq!(KitStyle::light().theme.id, "github-light");
        assert_eq!(
            KitStyle::dark().palette.bg,
            Color::from_rgb8(0x1b, 0x1c, 0x1f)
        );
    }
}
