//! What the page's parts share: the theme and the language they are drawn in,
//! and the few styles that belong to the statistics page alone.

use std::borrow::Cow;

use hangar_i18n::{AsKey, Locale, Translator};
use hangar_theme::tokens::radius;
use hangar_theme::{Theme, style};
use iced::widget::{container, text};
use iced::{Background, Border, Color};

use super::model::Measure;
use crate::ctx::Ctx;

/// The theme and the language of the frame being drawn.
#[derive(Clone, Copy)]
pub struct Ui {
    pub t: &'static Theme,
    pub tr: Translator,
}

impl Ui {
    pub fn new(ctx: &Ctx<'_>) -> Ui {
        Ui {
            t: ctx.theme,
            tr: ctx.i18n,
        }
    }

    pub fn locale(&self) -> Locale {
        self.tr.locale()
    }

    /// A label, owned, for a widget that outlives the call.
    pub fn s(&self, key: impl AsKey) -> String {
        self.tr.t(key).into_owned()
    }

    /// A label as the borrowed text it is.
    pub fn cow(&self, key: impl AsKey) -> Cow<'static, str> {
        self.tr.t(key)
    }
}

/// The key of a measure's name: "Tokens", "Cost".
pub fn measure_key(measure: Measure) -> hangar_i18n::Key {
    match measure {
        Measure::Tokens => hangar_i18n::Key::StatsMeasureTokens,
        Measure::Cost => hangar_i18n::Key::StatsMeasureCost,
    }
}

/// The page under the cards: the work area's own background on a dark theme,
/// where cards lift off it; a grey on a light one, where they are white
/// sheets on it, the way the settings draw their groups (`--stats-page`).
pub fn page(t: &Theme) -> impl Fn(&iced::Theme) -> container::Style + '_ {
    move |_| {
        let colors = &t.colors;
        let background = if t.is_dark() {
            colors.bg
        } else {
            colors.chrome_bar
        };
        container::Style {
            background: Some(Background::Color(background)),
            text_color: Some(colors.fg),
            ..container::Style::default()
        }
    }
}

/// A key figure or a card of the page.
pub fn card(t: &Theme) -> impl Fn(&iced::Theme) -> container::Style + '_ {
    style::container::card(t)
}

/// A solid box of one colour with rounded corners: a legend swatch, the fill
/// of a bar.
pub fn swatch(color: Color, radius: f32) -> impl Fn(&iced::Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(color)),
        border: Border {
            radius: radius.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// The track a bar fills (`.rank__track`).
pub fn track(t: &Theme) -> impl Fn(&iced::Theme) -> container::Style + '_ {
    move |_| container::Style {
        background: Some(Background::Color(t.colors.viz_heat_0)),
        border: Border {
            radius: radius::XS.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// Text that stays on one line and is cut at the edge of its box, as the
/// web page's ellipsis was.
pub fn one_line<'a>(content: impl Into<String>) -> text::Text<'a> {
    text(content.into()).wrapping(text::Wrapping::None)
}
