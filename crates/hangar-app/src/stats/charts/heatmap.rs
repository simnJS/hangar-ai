//! The week, hour by hour: seven rows of 24 cells, darker where more active
//! time was spent (`StatsHeatmap`).

use std::cell::RefCell;
use std::sync::Arc;

use hangar_i18n::{Key, Locale, Translator, args};
use hangar_theme::{Colors, Theme};
use iced::widget::canvas::{self, Canvas, Path, Stroke};
use iced::widget::{Space, column, container, responsive, row, stack, text};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, alignment, mouse};

use super::readout::{Place, Row, readout};
use super::{Event, caption};
use crate::message::Message;
use crate::stats::format;
use crate::stats::model::{Summary, heat_level};
use hangar_theme::{
    style,
    tokens::{font_size, space},
};

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

impl Heatmap {
    /// The hour labels, the weekday names and the cells: what the grid's
    /// cache keeps.
    fn draw_cells(&self, frame: &mut canvas::Frame, layout: Layout) {
        let colors = &self.theme.colors;
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

/// What the cached part of the grid (the labels and the cells) was drawn
/// from: the hover and focus rings are drawn over it, every frame.
#[derive(Debug, Clone, PartialEq)]
struct Drawn {
    heat: [[i64; 24]; 7],
    heat_max: i64,
    theme: *const Theme,
    locale: Locale,
    hours: [String; 4],
}

impl Drawn {
    fn of(heatmap: &Heatmap) -> Drawn {
        Drawn {
            heat: heatmap.summary.heat,
            heat_max: heatmap.summary.heat_max,
            theme: std::ptr::from_ref(heatmap.theme),
            locale: heatmap.locale,
            hours: heatmap.hours.clone(),
        }
    }

    fn is_of(&self, heatmap: &Heatmap) -> bool {
        self.heat == heatmap.summary.heat
            && self.heat_max == heatmap.summary.heat_max
            && std::ptr::eq(self.theme, heatmap.theme)
            && self.locale == heatmap.locale
            && self.hours == heatmap.hours
    }
}

/// The grid's tessellated cells, kept between frames.
///
/// The page's view is rebuilt after every message, and the 168 rounded cells
/// were tessellated again each time: about a fifth of the statistics page's
/// frame (task bddfd5c6). They are drawn again only when what they show
/// changes, or the canvas' size (the cache's own rule).
#[derive(Default)]
pub struct Grid {
    cells: canvas::Cache,
    drawn: RefCell<Option<Drawn>>,
}

impl Grid {
    /// Forgets the cells unless they were drawn from what `heatmap` shows.
    fn keep_for(&self, heatmap: &Heatmap) {
        let mut drawn = self.drawn.borrow_mut();
        if !drawn.as_ref().is_some_and(|drawn| drawn.is_of(heatmap)) {
            self.cells.clear();
            *drawn = Some(Drawn::of(heatmap));
        }
    }
}

impl canvas::Program<Message> for Heatmap {
    type State = Grid;

    fn update(
        &self,
        _state: &mut Grid,
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
                (self.keyboard || self.active != Some(over))
                    .then(|| canvas::Action::publish((self.on_event)(Event::Hover(Some(over)))))
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
        grid: &Grid,
        renderer: &Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let colors = &self.theme.colors;
        let layout = Layout::new(bounds.width);
        grid.keep_for(self);
        let cells = grid.cells.draw(renderer, bounds.size(), |frame| {
            self.draw_cells(frame, layout)
        });

        let mut frame = canvas::Frame::new(renderer, bounds.size());
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
                Stroke::default()
                    .with_color(colors.accent_ring)
                    .with_width(2.0),
            );
        }
        vec![cells, frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Grid,
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
        assert_eq!(
            layout.cell_at(Point::new(
                42.0 + 12.0 * 12.0 + 3.0,
                16.0 + 17.0 * 3.0 + 4.0
            )),
            Some((3, 12))
        );
        // The gap after a cell is still that cell.
        assert_eq!(
            layout.cell_at(Point::new(42.0 + 11.0, 16.0 + 16.0)),
            Some((0, 0))
        );
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

    fn heatmap() -> Heatmap {
        use crate::stats::model::{Measure, Period, SummaryOptions, TOP_SESSIONS, summarize};
        let data = hangar_core::stats::UsageStats {
            generated_at: 0,
            scan: Default::default(),
            sources: Vec::new(),
            active_gap_cap_ms: 0,
            sessions: Vec::new(),
            models: Vec::new(),
            buckets: Vec::new(),
        };
        let summary = summarize(
            &data,
            &SummaryOptions {
                period: Period::D7,
                project: None,
                measure: Measure::Tokens,
                now: 0,
                projects: &[],
                top_sessions: TOP_SESSIONS,
            },
        );
        Heatmap {
            theme: hangar_theme::default_theme(),
            locale: Locale::En,
            summary: Arc::new(summary),
            active: None,
            keyboard: false,
            focused: false,
            hours: ["0".into(), "6".into(), "12".into(), "18".into()],
            on_event: |_| Message::Noop,
        }
    }

    #[test]
    fn the_cells_are_kept_while_what_they_show_stays_the_same() {
        let grid = Grid::default();
        let mut chart = heatmap();
        grid.keep_for(&chart);
        let first = grid.drawn.borrow().clone();
        assert!(first.as_ref().is_some_and(|drawn| drawn.is_of(&chart)));
        // The hover and the focus are drawn over the cells, not in them.
        chart.active = Some((2, 13));
        chart.focused = true;
        chart.keyboard = true;
        grid.keep_for(&chart);
        assert_eq!(*grid.drawn.borrow(), first);
    }

    #[test]
    fn the_cells_are_drawn_again_when_the_data_the_theme_or_the_language_change() {
        let base = heatmap();
        let mut more = heatmap();
        let mut summary = (*more.summary).clone();
        summary.heat[1][5] = 42;
        summary.heat_max = 42;
        more.summary = Arc::new(summary);
        let mut other_theme = heatmap();
        other_theme.theme = hangar_theme::themes()
            .iter()
            .find(|theme| !std::ptr::eq(*theme, base.theme))
            .expect("two themes");
        let mut french = heatmap();
        french.locale = Locale::Fr;
        let mut hours = heatmap();
        hours.hours[1] = "6h".into();
        for changed in [&more, &other_theme, &french, &hours] {
            let grid = Grid::default();
            grid.keep_for(&base);
            assert!(!grid.drawn.borrow().as_ref().unwrap().is_of(changed));
            grid.keep_for(changed);
            assert!(grid.drawn.borrow().as_ref().unwrap().is_of(changed));
        }
    }
}
