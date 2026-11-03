//! The charts' table twins: the same values, no hover needed to read them
//! (`StatsColumnsTable`, `StatsHeatTable`).

use hangar_i18n::{Key, args};
use hangar_theme::tokens::{font_size, radius, space};
use hangar_theme::{fonts, style};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{column, container, row, rule, scrollable};
use iced::{Element, Length, Padding, alignment};

use super::charts::{KINDS, format_measure, total_value};
use super::format::{self, format_duration};
use super::model::{Granularity, Measure, Summary};
use super::ui::{Ui, one_line};
use crate::message::Message;

/// The tallest a table grows before it scrolls.
const MAX_HEIGHT: f32 = 260.0;
const FIRST_COLUMN: Length = Length::FillPortion(3);
const NUMBER_COLUMN: Length = Length::FillPortion(2);

fn cell<'a>(
    ui: Ui,
    content: String,
    width: impl Into<Length>,
    left: bool,
    head: bool,
) -> Element<'a, Message> {
    let t = ui.t;
    let words = one_line(content).size(font_size::UI);
    let words = if head {
        words.style(style::text::muted(t)).font(fonts::ui_medium())
    } else if left {
        words
            .style(style::text::secondary(t))
            .font(fonts::ui_medium())
    } else {
        words.style(style::text::primary(t))
    };
    container(words)
        .width(width.into())
        .padding(Padding::from([5.0, space::LG]))
        .align_x(if left {
            alignment::Horizontal::Left
        } else {
            alignment::Horizontal::Right
        })
        .into()
}

/// A table's frame: a hairline around it, scrolling both ways past
/// [`MAX_HEIGHT`] or the card's width.
fn frame<'a>(ui: Ui, rows: Vec<Element<'a, Message>>, sideways: bool) -> Element<'a, Message> {
    let t = ui.t;
    let mut body = column![];
    for (at, line) in rows.into_iter().enumerate() {
        if at > 0 {
            body = body.push(rule::horizontal(1).style(style::rule::hairline(t)));
        }
        body = body.push(line);
    }
    container(
        scrollable(body)
            .direction(if sideways {
                Direction::Both {
                    vertical: Scrollbar::new().width(hangar_theme::tokens::size::SCROLLBAR),
                    horizontal: Scrollbar::new().width(hangar_theme::tokens::size::SCROLLBAR),
                }
            } else {
                Direction::Vertical(Scrollbar::new().width(hangar_theme::tokens::size::SCROLLBAR))
            })
            .style(style::scrollable::default(t))
            .width(Length::Fill),
    )
    .max_height(MAX_HEIGHT)
    .style(move |_| container::Style {
        border: iced::Border {
            color: t.colors.line,
            width: 1.0,
            radius: radius::MD.into(),
        },
        ..container::Style::default()
    })
    .into()
}

/// The table twin of the column chart: every value, latest first.
pub fn columns<'a>(ui: Ui, summary: &Summary, measure: Measure) -> Element<'a, Message> {
    let locale = ui.locale();
    let week = summary.granularity == Granularity::Week;
    let mut head = row![cell(
        ui,
        ui.s(if week {
            Key::StatsColWeek
        } else {
            Key::StatsColDay
        }),
        FIRST_COLUMN,
        true,
        true,
    )];
    for kind in KINDS.iter().rev() {
        head = head.push(cell(ui, ui.s(kind.key()), NUMBER_COLUMN, false, true));
    }
    head = head.push(cell(ui, ui.s(Key::StatsTotal), NUMBER_COLUMN, false, true));

    let mut rows: Vec<Element<'a, Message>> = vec![head.into()];
    for point in summary.series.iter().rev() {
        let mut line = row![cell(
            ui,
            format::full_date(point.at, locale, !week),
            FIRST_COLUMN,
            true,
            false,
        )];
        for kind in KINDS.iter().rev() {
            line = line.push(cell(
                ui,
                format_measure(kind.value(point, measure), measure, locale),
                NUMBER_COLUMN,
                false,
                false,
            ));
        }
        line = line.push(cell(
            ui,
            format_measure(total_value(point, measure), measure, locale),
            NUMBER_COLUMN,
            false,
            false,
        ));
        rows.push(line.into());
    }
    frame(ui, rows, false)
}

/// The table twin of the heatmap: a row per weekday, a column per hour.
pub fn heat<'a>(ui: Ui, summary: &Summary) -> Element<'a, Message> {
    const HOUR_COLUMN: f32 = 76.0;
    let names = format::weekday_names(ui.locale(), true);
    let mut head = row![cell(ui, ui.s(Key::StatsColDay), 110.0, true, true)];
    for hour in 0..24 {
        head = head.push(cell(
            ui,
            ui.tr
                .t_with(Key::StatsHeatHour, &args!("h" => hour))
                .into_owned(),
            HOUR_COLUMN,
            false,
            true,
        ));
    }
    let mut rows: Vec<Element<'a, Message>> = vec![head.into()];
    for (day, hours) in summary.heat.iter().enumerate() {
        let mut line = row![cell(ui, names[day].to_owned(), 110.0, true, false)];
        for value in hours {
            let text = if *value > 0 {
                format_duration(*value as f64, ui.tr)
            } else {
                "–".to_owned()
            };
            line = line.push(cell(ui, text, HOUR_COLUMN, false, false));
        }
        rows.push(line.into());
    }
    frame(ui, rows, true)
}
