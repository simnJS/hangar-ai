//! The statistics page's charts, drawn on iced's `canvas`: a stacked column
//! chart of the period, the week's heatmap and the KPI sparklines
//! (`StatsCharts.tsx`, which drew them by hand in SVG).
//!
//! Every colour is a `--viz-*` token of the theme (`Colors::viz_*`), so the
//! charts follow the theme like the chrome around them. Text on a canvas is
//! always drawn above the shapes, so the readout that follows the pointer is
//! not part of the canvas: it is an ordinary widget stacked over it
//! ([`readout`]), which is also how the web page did it.
//!
//! A canvas cannot take the keyboard focus the way the web page's charts did.
//! A click on a chart focuses it (the page's `State` remembers which), the
//! arrow keys then walk it while it has the focus, and a click anywhere else
//! lets it go; the focus ring is drawn as the web page's was.

pub mod columns;
pub mod heatmap;
pub mod readout;
pub mod sparkline;
pub mod ticks;

use hangar_theme::{Theme, fonts};
use iced::alignment;
use iced::advanced::text::{Alignment, Shaping};
use iced::widget::canvas;
use iced::{Color, Pixels, Point};

use super::format;
use super::model::{Measure, Point as Column};
use hangar_i18n::Locale;

/// Which chart of the page an event is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartId {
    Columns,
    Heat,
}

/// What a chart tells the page about the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// The pointer is over a column (`(index, 0)`) or a heatmap cell
    /// (`(weekday, hour)`), or left the chart.
    Hover(Option<(usize, usize)>),
    /// Pressed: the chart takes the keys.
    Focus,
    /// A press elsewhere: it lets them go.
    Blur,
}

/// The four kinds of token, from the baseline up: cheapest nearest it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    CacheRead,
    CacheWrite,
    Input,
    Output,
}

/// `KINDS`.
pub const KINDS: [Kind; 4] = [Kind::CacheRead, Kind::CacheWrite, Kind::Input, Kind::Output];

impl Kind {
    pub fn key(self) -> hangar_i18n::Key {
        use hangar_i18n::Key;
        match self {
            Kind::CacheRead => Key::StatsKindCacheRead,
            Kind::CacheWrite => Key::StatsKindCacheWrite,
            Kind::Input => Key::StatsKindInput,
            Kind::Output => Key::StatsKindOutput,
        }
    }

    /// The colour of a kind: steps of one ramp (`--viz-1` … `--viz-4`).
    pub fn color(self, theme: &Theme) -> Color {
        let c = &theme.colors;
        match self {
            Kind::CacheRead => c.viz_1,
            Kind::CacheWrite => c.viz_2,
            Kind::Input => c.viz_3,
            Kind::Output => c.viz_4,
        }
    }

    /// The kind's share of a column, in tokens or in dollars.
    pub fn value(self, column: &Column, measure: Measure) -> f64 {
        let t = &column.totals;
        match (measure, self) {
            (Measure::Tokens, Kind::CacheRead) => t.cache_read as f64,
            (Measure::Tokens, Kind::CacheWrite) => t.cache_write as f64,
            (Measure::Tokens, Kind::Input) => t.input as f64,
            (Measure::Tokens, Kind::Output) => t.output as f64,
            (Measure::Cost, Kind::CacheRead) => t.cost_cache_read,
            (Measure::Cost, Kind::CacheWrite) => t.cost_cache_write,
            (Measure::Cost, Kind::Input) => t.cost_input,
            (Measure::Cost, Kind::Output) => t.cost_output,
        }
    }
}

/// A column's whole height, in tokens or in dollars.
pub fn total_value(column: &Column, measure: Measure) -> f64 {
    column.totals.measured(measure)
}

/// A value of the measure, in full: a table cell, a readout.
pub fn format_measure(value: f64, measure: Measure, locale: Locale) -> String {
    match measure {
        Measure::Cost => format::format_usd(value, locale, true),
        Measure::Tokens => format::format_integer(value, locale),
    }
}

/// Text on a canvas, the interface's font, anchored the way `align` says.
pub(crate) fn caption(
    content: impl Into<String>,
    at: Point,
    color: Color,
    size: f32,
    align_x: alignment::Horizontal,
    align_y: alignment::Vertical,
) -> canvas::Text {
    canvas::Text {
        content: content.into(),
        position: at,
        color,
        size: Pixels(size),
        font: fonts::ui(),
        align_x: Alignment::from(align_x),
        align_y,
        // Advanced shaping finds the no-break spaces French sets before a
        // unit in a font that has them.
        shaping: Shaping::Advanced,
        ..canvas::Text::default()
    }
}

/// `color` at a fraction of its opacity.
pub(crate) fn faded(color: Color, factor: f32) -> Color {
    Color {
        a: color.a * factor,
        ..color
    }
}

/// A hairline that lands on whole pixels: a line one pixel wide is crisp at
/// the middle of a pixel and blurred on its edge (`shape-rendering:
/// crispEdges`).
pub(crate) fn crisp(y: f32) -> f32 {
    y.round() + 0.5
}
