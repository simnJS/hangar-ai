//! The period, column by column: a stacked column chart of the four kinds of
//! token (or of what they cost), a column per day, or per week past 120 days
//! (`StatsColumns`).

use std::sync::Arc;

use hangar_i18n::{Key, Locale, Translator, args};
use hangar_theme::Theme;
use iced::widget::canvas::{self, Canvas, Path, Stroke};
use iced::widget::{responsive, stack};
use iced::{Element, Length, Point, Rectangle, Renderer, Size, alignment, border, mouse};

use super::readout::{Place, Row, readout};
use super::ticks::nice_ticks;
use super::{Event, KINDS, caption, crisp, faded, format_measure, total_value};
use crate::message::Message;
use crate::stats::format;
use crate::stats::model::{Granularity, Measure, Summary};

pub const PLOT_HEIGHT: f32 = 184.0;
pub const AXIS_HEIGHT: f32 = 22.0;
/// The room on the left for the axis labels.
pub const GUTTER: f32 = 52.0;
/// The air between two stacked kinds.
const GAP: f32 = 2.0;
/// The chart's top margin, so the topmost tick's label is not cut.
const TOP: f32 = 6.0;
/// The whole canvas: the plot, the axis and a margin.
pub const HEIGHT: f32 = PLOT_HEIGHT + AXIS_HEIGHT + 8.0;
const TICK_SIZE: f32 = 10.5;

/// Where the columns stand in a chart `width` wide.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub plot_width: f32,
    /// One column's share of the width.
    pub slot: f32,
    pub bar_width: f32,
    count: usize,
}

impl Layout {
    pub fn new(width: f32, count: usize) -> Layout {
        let plot_width = (width - GUTTER - 4.0).max(0.0);
        let slot = if count > 0 {
            plot_width / count as f32
        } else {
            0.0
        };
        // Thin marks: a third of the slot is air, never wider than 24 px.
        let bar_width = if slot > 6.0 { slot * 0.64 } else { slot - 1.0 }.clamp(1.0, 24.0);
        Layout {
            plot_width,
            slot,
            bar_width,
            count,
        }
    }

    /// The column under a pointer `x` pixels from the chart's left edge.
    pub fn index_at(&self, x: f32) -> Option<usize> {
        if self.slot <= 0.0 {
            return None;
        }
        let index = ((x - GUTTER) / self.slot).floor();
        (index >= 0.0 && (index as usize) < self.count).then_some(index as usize)
    }

    /// The middle of a column.
    pub fn center(&self, index: usize) -> f32 {
        GUTTER + index as f32 * self.slot + self.slot / 2.0
    }

    /// Every how many columns a label stands: far enough apart to never touch,
    /// counted back from today so the last column always has one.
    pub fn label_every(&self) -> usize {
        let fit = ((self.plot_width / 68.0).floor() as usize).max(1);
        self.count.div_ceil(fit).max(1)
    }
}

#[derive(Clone)]
pub struct Columns {
    pub theme: &'static Theme,
    pub locale: Locale,
    pub measure: Measure,
    pub summary: Arc<Summary>,
    /// The column the readout is about.
    pub active: Option<usize>,
    /// The readout follows the keys, not the pointer.
    pub keyboard: bool,
    /// The chart has the keys.
    pub focused: bool,
    /// Said over an empty chart: what is missing, and what to try.
    pub empty: (String, String),
    pub on_event: fn(Event) -> Message,
}

impl Columns {
    fn max(&self) -> f64 {
        self.summary
            .series
            .iter()
            .map(|point| total_value(point, self.measure))
            .fold(0.0, f64::max)
    }

    /// The title of a column's readout.
    fn title(&self, tr: Translator, at: i64) -> String {
        match self.summary.granularity {
            Granularity::Week => tr
                .t_with(
                    Key::StatsWeekOf,
                    &args!("date" => format::day_month(at, self.locale)),
                )
                .into_owned(),
            Granularity::Day => format::weekday_day_month_long(at, self.locale),
        }
    }

    /// The readout of the active column: its four kinds, the largest first,
    /// and the total.
    fn rows(&self, tr: Translator, index: usize) -> Vec<Row> {
        let point = &self.summary.series[index];
        let mut rows: Vec<Row> = KINDS
            .iter()
            .rev()
            .map(|kind| Row {
                label: tr.t(kind.key()).into_owned(),
                value: format_measure(kind.value(point, self.measure), self.measure, self.locale),
                color: Some(kind.color(self.theme)),
                strong: false,
            })
            .collect();
        rows.push(Row {
            label: tr.t(Key::StatsTotal).into_owned(),
            value: format_measure(total_value(point, self.measure), self.measure, self.locale),
            color: None,
            strong: true,
        });
        rows
    }

    /// The chart with its readout over it.
    pub fn view<'a>(self, tr: Translator) -> Element<'a, Message> {
        responsive(move |size: Size| {
            let chart = Canvas::new(self.clone())
                .width(Length::Fill)
                .height(Length::Fixed(HEIGHT));
            let mut layers = stack![chart];
            if let Some(index) = self
                .active
                .filter(|index| *index < self.summary.series.len())
            {
                let layout = Layout::new(size.width, self.summary.series.len());
                layers = layers.push(readout(
                    self.theme,
                    layout.center(index),
                    size.width,
                    Place::Top(4.0),
                    self.title(tr, self.summary.series[index].at),
                    self.rows(tr, index),
                ));
            }
            layers.into()
        })
        .width(Length::Fill)
        .height(Length::Fixed(HEIGHT))
        .into()
    }
}

impl canvas::Program<Message> for Columns {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let canvas::Event::Mouse(event) = event else {
            return None;
        };
        match event {
            mouse::Event::CursorMoved { .. } | mouse::Event::CursorLeft => {
                let layout = Layout::new(bounds.width, self.summary.series.len());
                let over = cursor
                    .position_in(bounds)
                    .and_then(|point| layout.index_at(point.x));
                // Only a change is news; in keyboard mode the keys own the
                // readout until the pointer is over a column.
                let changed = match over {
                    Some(index) => self.keyboard || self.active != Some(index),
                    None => !self.keyboard && self.active.is_some(),
                };
                changed.then(|| {
                    canvas::Action::publish((self.on_event)(Event::Hover(over.map(|i| (i, 0)))))
                })
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                if cursor.is_over(bounds) {
                    Some(canvas::Action::publish((self.on_event)(Event::Focus)).and_capture())
                } else if self.focused {
                    Some(canvas::Action::publish((self.on_event)(Event::Blur)))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let colors = &self.theme.colors;
        let series = &self.summary.series;
        let layout = Layout::new(bounds.width, series.len());
        let max = self.max();
        let ticks = nice_ticks(max, 4);
        let top = ticks
            .last()
            .copied()
            .filter(|tick| *tick > 0.0)
            .unwrap_or(1.0);
        let height_of = |value: f64| (value / top) as f32 * PLOT_HEIGHT;
        let floor = TOP + PLOT_HEIGHT;

        for tick in &ticks {
            let y = crisp(floor - height_of(*tick));
            let color = if *tick == 0.0 {
                colors.line_strong
            } else {
                colors.line
            };
            frame.stroke(
                &Path::line(
                    Point::new(GUTTER, y),
                    Point::new(GUTTER + layout.plot_width, y),
                ),
                Stroke::default().with_color(color).with_width(1.0),
            );
            frame.fill_text(caption(
                format::format_tick(*tick, self.measure, self.locale),
                Point::new(GUTTER - 8.0, y),
                colors.fg_subtle,
                TICK_SIZE,
                alignment::Horizontal::Right,
                alignment::Vertical::Center,
            ));
        }

        if let Some(active) = self.active.filter(|active| *active < series.len()) {
            frame.fill(
                &Path::rounded_rectangle(
                    Point::new(GUTTER + active as f32 * layout.slot, TOP),
                    Size::new(layout.slot, PLOT_HEIGHT),
                    3.0.into(),
                ),
                colors.line,
            );
        }

        let radius = 4f32.min(layout.bar_width / 2.0);
        for (index, point) in series.iter().enumerate() {
            let total = total_value(point, self.measure);
            if total <= 0.0 {
                continue;
            }
            // The others step back while one is read.
            let dim = if self.active.is_some_and(|active| active != index) {
                0.5
            } else {
                1.0
            };
            let x = GUTTER + index as f32 * layout.slot + (layout.slot - layout.bar_width) / 2.0;
            let parts: Vec<_> = KINDS
                .iter()
                .map(|kind| (*kind, height_of(kind.value(point, self.measure))))
                .filter(|(_, height)| *height > 0.0)
                .collect();
            let mut base = floor;
            for (at, (kind, height)) in parts.iter().enumerate() {
                // A 2 px gap of the card between two kinds, taken from the
                // lower one when it is tall enough to give it.
                let last = at == parts.len() - 1;
                let gap = if !last && *height > GAP + 1.0 {
                    GAP
                } else {
                    0.0
                };
                let part = (*height - gap).max(0.0);
                // One rounded top per column, square at the baseline.
                let corners = if last {
                    border::top(radius.min(part))
                } else {
                    border::Radius::default()
                };
                frame.fill(
                    &Path::rounded_rectangle(
                        Point::new(x, base - height + gap),
                        Size::new(layout.bar_width, part),
                        corners,
                    ),
                    faded(kind.color(self.theme), dim),
                );
                base -= height;
            }
        }

        let every = layout.label_every();
        for (index, point) in series.iter().enumerate() {
            if !(series.len() - 1 - index).is_multiple_of(every) {
                continue;
            }
            let text = if self.summary.granularity == Granularity::Day && series.len() <= 7 {
                format::weekday_day(point.at, self.locale)
            } else {
                format::day_month(point.at, self.locale)
            };
            // The last label is right-anchored so that it never leaves the chart.
            let align = if index == series.len() - 1 && series.len() > 7 {
                alignment::Horizontal::Right
            } else {
                alignment::Horizontal::Center
            };
            frame.fill_text(caption(
                text,
                Point::new(layout.center(index), floor + 16.0),
                colors.fg_subtle,
                TICK_SIZE,
                align,
                alignment::Vertical::Center,
            ));
        }

        if max <= 0.0 {
            let middle_x = GUTTER + (bounds.width - GUTTER) / 2.0;
            let middle_y = (bounds.height - 28.0) / 2.0;
            frame.fill_text(caption(
                self.empty.0.clone(),
                Point::new(middle_x, middle_y - 9.0),
                colors.fg_secondary,
                13.0,
                alignment::Horizontal::Center,
                alignment::Vertical::Center,
            ));
            frame.fill_text(caption(
                self.empty.1.clone(),
                Point::new(middle_x, middle_y + 10.0),
                colors.fg_subtle,
                12.0,
                alignment::Horizontal::Center,
                alignment::Vertical::Center,
            ));
        }

        if self.focused && self.keyboard {
            frame.stroke(
                &Path::rounded_rectangle(
                    Point::new(1.0, 1.0),
                    Size::new(bounds.width - 2.0, bounds.height - 2.0),
                    6.0.into(),
                ),
                Stroke::default()
                    .with_color(colors.accent_ring)
                    .with_width(2.0),
            );
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &(),
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.is_over(bounds) {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_column_is_a_share_of_the_width_after_the_gutter() {
        let layout = Layout::new(52.0 + 4.0 + 300.0, 30);
        assert_eq!(layout.plot_width, 300.0);
        assert_eq!(layout.slot, 10.0);
        assert!((layout.bar_width - 6.4).abs() < 1e-5);
        assert_eq!(layout.index_at(GUTTER + 5.0), Some(0));
        assert_eq!(layout.index_at(GUTTER + 299.0), Some(29));
        assert_eq!(layout.index_at(GUTTER + 300.0), None);
        assert_eq!(layout.index_at(10.0), None);
        assert_eq!(layout.center(1), GUTTER + 15.0);
    }

    #[test]
    fn bars_are_thin_but_never_wider_than_24_pixels() {
        assert_eq!(Layout::new(2000.0, 7).bar_width, 24.0);
        // A hairline at the least, however many columns.
        assert_eq!(Layout::new(300.0, 400).bar_width, 1.0);
        // Slots of six pixels or fewer leave one pixel of air.
        assert_eq!(Layout::new(GUTTER + 4.0 + 300.0, 60).bar_width, 4.0);
    }

    #[test]
    fn no_width_or_no_columns_hits_nothing() {
        assert_eq!(Layout::new(10.0, 5).index_at(30.0), None);
        assert_eq!(Layout::new(400.0, 0).index_at(100.0), None);
        assert_eq!(Layout::new(400.0, 0).label_every(), 1);
    }

    #[test]
    fn labels_stand_at_least_68_pixels_apart() {
        // 30 columns in 300 px: four labels fit, so every eighth column.
        assert_eq!(Layout::new(GUTTER + 4.0 + 300.0, 30).label_every(), 8);
        // Seven days in a wide chart: all of them.
        assert_eq!(Layout::new(900.0, 7).label_every(), 1);
        // Too narrow for even one: one label at the end.
        assert_eq!(Layout::new(60.0, 7).label_every(), 7);
    }
}
