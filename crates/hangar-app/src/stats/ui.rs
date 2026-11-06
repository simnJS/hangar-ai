//! What the page's parts share: the theme and the language they are drawn in,
//! and the few styles that belong to the statistics page alone.

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
    move |_| container::Style {
        background: Some(Background::Color(page_color(t))),
        text_color: Some(t.colors.fg),
        ..container::Style::default()
    }
}

/// The colour of the page itself.
fn page_color(t: &Theme) -> Color {
    if t.is_dark() {
        t.colors.bg
    } else {
        t.colors.chrome_bar
    }
}

/// How much of the page shows through the content while it is stale
/// (`.stats__scroll.is-stale { opacity: 0.62 }`).
const STALE_OPACITY: f32 = 0.62;
/// The pause before the content is held back, so that a quick look at the
/// disk never makes the page flicker (`transition-delay: 300ms`).
pub const STALE_AFTER: std::time::Duration = std::time::Duration::from_millis(300);

/// Whether the content is what the cache said while the disk is still being
/// read, and has been for long enough to show it.
pub fn is_stale(
    refreshing: bool,
    cached_only: bool,
    started: Option<std::time::Instant>,
    now: std::time::Instant,
) -> bool {
    refreshing
        && cached_only
        && started.is_some_and(|started| now.saturating_duration_since(started) >= STALE_AFTER)
}

/// A veil over the content that lets the page behind it show through by what
/// is missing of [`STALE_OPACITY`]. iced has no opacity for a subtree, and
/// the content lies on the page's own colour, so a layer of that colour over
/// it is the same picture. It takes no events: they pass to the content.
pub fn stale_veil<'a, M: 'a>(t: &Theme) -> iced::Element<'a, M> {
    let color = Color {
        a: 1.0 - STALE_OPACITY,
        ..page_color(t)
    };
    container(iced::widget::Space::new())
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(Background::Color(color)),
            ..container::Style::default()
        })
        .into()
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
