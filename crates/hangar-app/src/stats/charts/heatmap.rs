//! The week, hour by hour: seven rows of 24 cells, darker where more active
//! time was spent (`StatsHeatmap`).

use std::sync::Arc;

use hangar_i18n::{Key, Locale, Translator, args};
use hangar_theme::{Theme, Colors};
use iced::widget::canvas::{self, Canvas, Path, Stroke};
use iced::widget::{column, container, responsive, row, stack, text, Space};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, alignment, mouse};

use super::readout::{Place, Row, readout};
use super::{Event, caption};
use crate::message::Message;
use crate::stats::format;
use crate::stats::model::{Summary, heat_level};
use hangar_theme::{style, tokens::{font_size, space}};

/// The column of weekday names.
const LABEL_WIDTH: f32 = 40.0;
const GAP: f32 = 2.0;
/// The row of hour labels.
const HEAD: f32 = 14.0;
pub const CELL: f32 = 15.0;
/// The whole canvas: the hour labels and seven rows.
pub const HEIGHT: f32 = HEAD + GAP + 7.0 * CELL + 6.0 * GAP;
const PITCH: f32 = CELL + GAP;
/// Where the first row starts.
const ROWS_TOP: f32 = HEAD + GAP;

/// The colour of a level of the heat ramp (`--viz-heat-0` … `-5`).
pub fn level_color(colors: &Colors, level: usize) -> Color {
    match level {
        0 => colors.viz_heat_0,
        1 => colors.viz_heat_1,
        2 => colors.viz_heat_2,
        3 => colors.viz_heat_3,
        4 => colors.viz_heat_4,
        _ => colors.viz_heat_5,
    }
}

/// Where the cells stand in a chart `width` wide.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub cell_width: f32,
}

impl Layout {
    pub fn new(width: f32) -> Layout {
        Layout {
            cell_width: ((width - LABEL_WIDTH - 24.0 * GAP) / 24.0).max(0.0),
        }
    }

    /// The left edge of an hour's column.
    pub fn x(&self, hour: usize) -> f32 {
        LABEL_WIDTH + GAP + hour as f32 * (self.cell_width + GAP)
    }

    /// The middle of an hour's column.
    pub fn center(&self, hour: usize) -> f32 {
        self.x(hour) + self.cell_width / 2.0
    }

    /// The top of a weekday's row.
    pub fn y(weekday: usize) -> f32 {
        ROWS_TOP + weekday as f32 * PITCH
    }

    /// The cell under a point of the chart: a gap between cells belongs to
    /// the cell before it, so the readout does not flicker as the pointer
    /// crosses one.
    pub fn cell_at(&self, point: Point) -> Option<(usize, usize)> {
        if self.cell_width <= 0.0 || point.x < self.x(0) || point.y < ROWS_TOP {
            return None;
        }
        let hour = ((point.x - self.x(0)) / (self.cell_width + GAP)).floor() as usize;
        let weekday = ((point.y - ROWS_TOP) / PITCH).floor() as usize;
        (hour < 24 && weekday < 7).then_some((weekday, hour))
    }
}

#[derive(Clone)]
pub struct Heatmap {
    pub theme: &'static Theme,
    pub locale: Locale,
    pub summary: Arc<Summary>,
    /// The cell the readout is about: weekday (Monday 0) and hour.
    pub active: Option<(usize, usize)>,
    pub keyboard: bool,
    pub focused: bool,
    /// The hour labels over the grid (0, 6, 12, 18).
    pub hours: [String; 4],
    pub on_event: fn(Event) -> Message,
}

impl Heatmap {
    /// The chart with its readout over it, and its legend under it.
    pub fn view<'a>(self, tr: Translator) -> Element<'a, Message> {
        let theme = self.theme;
        let legend = legend(theme, tr);
        let chart = responsive(move |size: Size| {
            let grid = Canvas::new(self.clone())
                .width(Length::Fill)
                .height(Length::Fixed(HEIGHT));
            let mut layers = stack![grid];
            if let Some((day, hour)) = self.active {
                let layout = Layout::new(size.width);
                let value = self.summary.heat[day][hour];
                let level = heat_level(value, self.summary.heat_max);
                // Beside the hovered row, never over it: under it in the first
                // half of the week, above it in the second.
                let place = if day < 4 {
                    Place::Top(Layout::y(day) + PITCH + 6.0)
                } else {
                    Place::Bottom((7 - day) as f32 * PITCH + 6.0)
                };
                let long = format::weekday_names(self.locale, true)[day];
                layers = layers.push(readout(
                    theme,
                    layout.center(hour),
                    size.width,
                    place,
                    format!(
                        "{long}, {}",
                        tr.t_with(Key::StatsHeatHour, &args!("h" => hour))
                    ),
                    vec![Row {
                        label: tr.t(Key::StatsKpiActive).into_owned(),
                        value: format::format_duration(value as f64, tr),
                        color: Some(level_color(&theme.colors, level.max(1))),
                        strong: false,
                    }],
                ));
            }
            layers.into()
        })
        .width(Length::Fill)
        .height(Length::Fixed(HEIGHT));
        column![chart, legend].spacing(space::LG).into()
    }
}

/// "Less ▢▢▢▢▢▢ More", at the right.
fn legend<'a>(theme: &'static Theme, tr: Translator) -> Element<'a, Message> {
    let mut line = row![
        text(tr.t(Key::StatsHeatLess).into_owned())
            .size(font_size::CAPTION)
            .style(style::text::subtle(theme)),
        Space::new().width(space::XS - 1.0),
    ]
    .spacing(3)
    .align_y(alignment::Vertical::Center);
    for level in 0..6 {
        let color = level_color(&theme.colors, level);
        line = line.push(
            container(Space::new())
                .width(11)
                .height(11)
                .style(move |_| container::Style {
                    background: Some(color.into()),
                    border: iced::Border {
                        radius: 3.0.into(),
                        ..iced::Border::default()
                    },
                    ..container::Style::default()
                }),
        );
    }
    line = line.push(Space::new().width(space::XS - 1.0)).push(
        text(tr.t(Key::StatsHeatMore).into_owned())
            .size(font_size::CAPTION)
            .style(style::text::subtle(theme)),
    );
    container(line)
        .width(Length::Fill)
        .align_x(alignment::Horizontal::Right)
        .into()
}

/// "lun." → "Lun.": the weekday column's `text-transform: capitalize`.
fn capitalized(name: &str) -> String {
    let mut chars = name.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

impl canvas::Program<Message> for Heatmap {
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
            mouse::Event::CursorMoved { .. } => {
                let layout = Layout::new(bounds.width);
                let over = cursor
                    .position_in(bounds)
                    .and_then(|point| layout.cell_at(point));
                // The pointer leaving the grid is a CursorLeft; moving over a
                // gap or a label changes nothing.
                let over = over?;
                (self.keyboard || self.active != Some(over)).then(|| {
                    canvas::Action::publish((self.on_event)(Event::Hover(Some(over))))
                })
            }
            mouse::Event::CursorLeft => (!self.keyboard && self.active.is_some())
                .then(|| canvas::Action::publish((self.on_event)(Event::Hover(None)))),
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
        let layout = Layout::new(bounds.width);
        let summary = &self.summary;

        for (at, label) in self.hours.iter().enumerate() {
            frame.fill_text(caption(
                label.clone(),
                Point::new(layout.x(at * 6), HEAD / 2.0),
                colors.fg_subtle,
                font_size::LABEL,
                alignment::Horizontal::Left,
                alignment::Vertical::Center,
            ));
        }

        let days = format::weekday_names(self.locale, false);
        for (day, hours) in summary.heat.iter().enumerate() {
            let top = Layout::y(day);
            frame.fill_text(caption(
                capitalized(days[day]),
                Point::new(0.0, top + CELL / 2.0),
                colors.fg_muted,
                font_size::CAPTION,
                alignment::Horizontal::Left,
                alignment::Vertical::Center,
            ));
            for (hour, value) in hours.iter().enumerate() {
                let level = heat_level(*value, summary.heat_max);
                frame.fill(
                    &Path::rounded_rectangle(
                        Point::new(layout.x(hour), top),
                        Size::new(layout.cell_width, CELL),
                        3.0.into(),
                    ),
                    level_color(colors, level),
                );
            }
        }

        if let Some((day, hour)) = self.active.filter(|(day, hour)| *day < 7 && *hour < 24) {
            // A ring around the cell, outside it, as the web page's shadow was.
            frame.stroke(
                &Path::rounded_rectangle(
                    Point::new(layout.x(hour) - 0.75, Layout::y(day) - 0.75),
                    Size::new(layout.cell_width + 1.5, CELL + 1.5),
                    3.75.into(),
                ),
                Stroke::default().with_color(colors.fg).with_width(1.5),
            );
        }

        if self.focused && self.keyboard {
            frame.stroke(
                &Path::rounded_rectangle(
                    Point::new(1.0, 1.0),
                    Size::new(bounds.width - 2.0, bounds.height - 2.0),
                    6.0.into(),
                ),
                Stroke::default().with_color(colors.accent_ring).with_width(2.0),
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
    fn the_grid_is_133_pixels_tall() {
        assert_eq!(HEIGHT, 133.0);
        assert_eq!(Layout::y(0), 16.0);
        assert_eq!(Layout::y(6) + CELL, HEIGHT);
    }

    #[test]
    fn a_cell_is_found_by_its_row_and_column() {
        let layout = Layout::new(40.0 + 24.0 * 2.0 + 24.0 * 10.0);
        assert_eq!(layout.cell_width, 10.0);
        assert_eq!(layout.cell_at(Point::new(42.0 + 1.0, 17.0)), Some((0, 0)));
        assert_eq!(layout.cell_at(Point::new(42.0 + 12.0 * 12.0 + 3.0, 16.0 + 17.0 * 3.0 + 4.0)), Some((3, 12)));
        // The gap after a cell is still that cell.
        assert_eq!(layout.cell_at(Point::new(42.0 + 11.0, 16.0 + 16.0)), Some((0, 0)));
        // The weekday names, the hour labels, past the grid.
        assert_eq!(layout.cell_at(Point::new(10.0, 20.0)), None);
        assert_eq!(layout.cell_at(Point::new(60.0, 5.0)), None);
        assert_eq!(layout.cell_at(Point::new(60.0, 16.0 + 7.0 * 17.0)), None);
        assert_eq!(layout.cell_at(Point::new(42.0 + 24.0 * 12.0, 20.0)), None);
    }

    #[test]
    fn weekday_names_are_capitalised() {
        assert_eq!(capitalized("lun."), "Lun.");
        assert_eq!(capitalized("Mon"), "Mon");
        assert_eq!(capitalized(""), "");
    }

    #[test]
    fn every_level_has_its_own_colour() {
        let colors = hangar_theme::default_theme().colors;
        let ramp: Vec<Color> = (0..6).map(|level| level_color(&colors, level)).collect();
        for pair in ramp.windows(2) {
            assert_ne!(pair[0], pair[1]);
        }
    }
}
