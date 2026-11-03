//! A ranking: one bar per row, its length against the first (`RankedList`).

use hangar_i18n::Key;
use hangar_theme::tokens::{font_size, space};
use hangar_theme::{fonts, style};
use iced::widget::{Space, button, column, container, row};
use iced::{Element, Length, Padding, alignment};

use super::format::{format_compact, format_share, format_usd};
use super::model::{Measure, Ranked, Totals};
use super::ui::{self, Ui, one_line};
use crate::message::Message;

/// A value of the measure at a glance: dollars or a compact token count.
pub fn value_of_totals(totals: &Totals, measure: Measure, locale: hangar_i18n::Locale) -> String {
    match measure {
        Measure::Cost => format_usd(totals.cost, locale, false),
        Measure::Tokens => format_compact(totals.tokens as f64, locale),
    }
}

/// How wide a bar is, in hundredths of a percent of its track: against the
/// largest row, and never less than 0.6 % for a row that has anything, so it
/// still shows.
pub fn bar_portion(value: f64, max: f64) -> u16 {
    if max <= 0.0 {
        return 0;
    }
    let percent = ((value / max) * 100.0).max(if value > 0.0 { 0.6 } else { 0.0 });
    (percent * 100.0).round().clamp(0.0, 10_000.0) as u16
}

/// What a click on a row does: the message for the row's key.
pub type Pick = fn(String) -> Message;

pub struct List<'a> {
    pub rows: &'a [Ranked],
    pub measure: Measure,
    /// The picked row, when the ranking is a way to filter the page.
    pub selected: Option<&'a str>,
    /// Makes the rows buttons.
    pub pick: Option<Pick>,
    /// The tip of a row's button.
    pub pick_tip: Option<&'a dyn Fn(&Ranked) -> String>,
    /// The line under a row's name, when it is not its detail.
    pub sub: Option<&'a dyn Fn(&Ranked) -> String>,
}

pub fn list<'a>(ui: Ui, list: List<'_>) -> Element<'a, Message> {
    let t = ui.t;
    let locale = ui.locale();
    let max = list
        .rows
        .iter()
        .map(|row| row.totals.measured(list.measure))
        .fold(0.0, f64::max);
    let picking = list.selected.is_some();
    let mut rows = column![].spacing(2);
    for ranked in list.rows {
        let value = ranked.totals.measured(list.measure);
        let on = list.selected == Some(ranked.key.as_str());
        let second = match list.sub {
            Some(sub) => sub(ranked),
            None => ranked.detail.clone(),
        };

        let mut text_column = column![
            one_line(ranked.label.clone())
                .size(font_size::UI_LG)
                .font(fonts::ui_medium())
                .style(style::text::primary(t))
        ];
        if !second.is_empty() {
            text_column = text_column.push(
                one_line(second)
                    .size(font_size::CAPTION)
                    .style(style::text::subtle(t)),
            );
        }
        let figure: Element<'a, Message> = if ranked.unpriced && list.measure == Measure::Cost {
            container(
                one_line(ui.s(Key::StatsUnpriced))
                    .size(font_size::CAPTION)
                    .style(style::text::muted(t)),
            )
            .padding(Padding::from([0.0, space::SM]))
            .style(style::container::chip(t))
            .into()
        } else {
            one_line(value_of_totals(&ranked.totals, list.measure, locale))
                .size(font_size::UI_LG)
                .font(fonts::ui_semibold())
                .style(style::text::primary(t))
                .into()
        };
        let numbers = row![
            figure,
            container(
                one_line(format_share(ranked.share, locale))
                    .size(font_size::CAPTION)
                    .style(style::text::subtle(t))
            )
            .width(34)
            .align_x(alignment::Horizontal::Right),
        ]
        .spacing(space::LG)
        .align_y(alignment::Vertical::Bottom);

        // One row picked: it keeps the accent, the others step back.
        let fill = if picking && !on {
            t.colors.viz_mute
        } else {
            t.colors.viz_4
        };
        let portion = bar_portion(value, max);
        let bar: Element<'a, Message> = if portion == 0 {
            Space::new().height(4).into()
        } else if portion >= 10_000 {
            container(Space::new().height(4))
                .width(Length::Fill)
                .style(ui::swatch(fill, 2.0))
                .into()
        } else {
            row![
                container(Space::new().height(4))
                    .width(Length::FillPortion(portion))
                    .style(ui::swatch(fill, 2.0)),
                Space::new().width(Length::FillPortion(10_000 - portion)),
            ]
            .into()
        };
        let track = container(bar)
            .width(Length::Fill)
            .height(4)
            .style(ui::track(t));

        let body = column![
            row![
                container(text_column).width(Length::Fill).clip(true),
                numbers
            ]
            .spacing(space::XL)
            .align_y(alignment::Vertical::Bottom),
            track
        ]
        .spacing(5);

        let line: Element<'a, Message> = match list.pick {
            Some(pick) => {
                let pressed = button(body)
                    .padding(Padding::from([6.0, space::MD]))
                    .width(Length::Fill)
                    .style(style::button::row(t, on))
                    .on_press(pick(ranked.key.clone()));
                match list.pick_tip {
                    Some(words) => hangar_ui::tip::bottom(t, pressed, words(ranked)),
                    None => pressed.into(),
                }
            }
            None => container(body)
                .padding(Padding::from([6.0, 0.0]))
                .width(Length::Fill)
                .into(),
        };
        rows = rows.push(line);
    }
    rows.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bar_is_its_share_of_the_largest_row() {
        assert_eq!(bar_portion(100.0, 100.0), 10_000);
        assert_eq!(bar_portion(50.0, 100.0), 5_000);
        assert_eq!(bar_portion(0.0, 100.0), 0);
        assert_eq!(bar_portion(5.0, 0.0), 0);
    }

    #[test]
    fn a_row_with_anything_still_shows() {
        // 0.6 % of the track, however small the value.
        assert_eq!(bar_portion(1.0, 1_000_000.0), 60);
        assert_eq!(bar_portion(0.0, 1_000_000.0), 0);
    }
}
