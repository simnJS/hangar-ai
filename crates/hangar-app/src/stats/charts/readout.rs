//! The readout that follows the pointer or the keys: the value first, then
//! what it is (`.viz-tip`). It is a widget stacked over the chart rather than
//! drawn in it, so it hides the chart's text (the axis labels) the way the web
//! page's did.

use hangar_theme::tokens::{font_size, space};
use hangar_theme::{Theme, fonts, style};
use iced::widget::{Space, column, container, row, text};
use iced::{Color, Element, Length, Padding, alignment};

/// How far the readout stands from the point it is about.
const MARGIN: f32 = 14.0;
/// `min-width` of the box.
const MIN_WIDTH: f32 = 170.0;
/// The width of a box that has a total in it.
const TOTAL_WIDTH: f32 = 220.0;

/// One line of a readout.
#[derive(Debug, Clone)]
pub struct Row {
    pub label: String,
    pub value: String,
    /// The series' colour, drawn as a short stroke; none for a line that is
    /// not a series (the total).
    pub color: Option<Color>,
    /// The total: set off by a hairline above.
    pub strong: bool,
}

/// Where the readout stands vertically.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Place {
    /// This far from the top of the chart.
    Top(f32),
    /// This far from its bottom.
    Bottom(f32),
}

/// A readout about the point `x` of a chart `width` wide: to its right, and
/// past the middle to its left so that it stays inside.
pub fn readout<'a, M: 'a>(
    theme: &'a Theme,
    x: f32,
    width: f32,
    place: Place,
    title: String,
    rows: Vec<Row>,
) -> Element<'a, M> {
    let has_total = rows.iter().any(|line| line.strong);
    let mut lines = column![
        Space::new().width(MIN_WIDTH).height(0),
        text(title)
            .size(font_size::CAPTION_LG)
            .font(fonts::ui_semibold())
            .style(style::text::secondary(theme)),
    ]
    .spacing(space::SM - 1.0);
    for line in rows {
        let key: Element<'a, M> = match line.color {
            Some(color) => container(Space::new())
                .width(10)
                .height(3)
                .style(move |_| container::Style {
                    background: Some(color.into()),
                    border: iced::Border {
                        radius: 2.0.into(),
                        ..iced::Border::default()
                    },
                    ..container::Style::default()
                })
                .into(),
            None => Space::new().width(10).height(3).into(),
        };
        let line_row = row![
            container(key).center_y(Length::Shrink),
            text(line.value)
                .size(font_size::CAPTION_LG)
                .font(fonts::ui_semibold())
                .style(style::text::primary(theme)),
            text(line.label)
                .size(font_size::CAPTION_LG)
                .style(style::text::muted(theme)),
        ]
        .spacing(space::MD)
        .align_y(alignment::Vertical::Center);
        if line.strong {
            // The total sits under a hairline with room around it.
            lines = lines.push(
                column![
                    iced::widget::rule::horizontal(1).style(style::rule::hairline(theme)),
                    line_row
                ]
                .spacing(space::SM - 1.0),
            );
        } else {
            lines = lines.push(line_row);
        }
    }
    // The hairline over the total fills its column, and a column that shrinks
    // to its content cannot give a filling child a width: a box with a total
    // has the width its widest figure and label need instead.
    let card = container(lines)
        .padding(Padding::from([space::MD, space::LG]))
        .style(style::container::popover(theme));
    let card = if has_total {
        card.width(TOTAL_WIDTH)
    } else {
        card
    };

    let flip = x > width / 2.0;
    let mut padding = Padding::ZERO;
    if flip {
        padding.right = (width - x + MARGIN).max(0.0);
    } else {
        padding.left = (x + MARGIN).max(0.0);
    }
    let align_y = match place {
        Place::Top(top) => {
            padding.top = top;
            alignment::Vertical::Top
        }
        Place::Bottom(bottom) => {
            padding.bottom = bottom;
            alignment::Vertical::Bottom
        }
    };
    container(card)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(if flip {
            alignment::Horizontal::Right
        } else {
            alignment::Horizontal::Left
        })
        .align_y(align_y)
        .padding(padding)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_readout_builds_in_both_places() {
        let theme = hangar_theme::default_theme();
        let rows = vec![
            Row {
                label: "Output".into(),
                value: "12".into(),
                color: Some(theme.colors.viz_4),
                strong: false,
            },
            Row {
                label: "Total".into(),
                value: "12".into(),
                color: None,
                strong: true,
            },
        ];
        let _: Element<'_, ()> = readout(
            theme,
            10.0,
            400.0,
            Place::Top(4.0),
            "Day".into(),
            rows.clone(),
        );
        let _: Element<'_, ()> =
            readout(theme, 390.0, 400.0, Place::Bottom(8.0), "Day".into(), rows);
    }
}
