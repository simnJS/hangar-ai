//! The heaviest sessions of the period, in order (`SessionList`).

use hangar_core::stats::Agent;
use hangar_i18n::{Key, args};
use hangar_theme::tokens::{font_size, radius, space};
use hangar_theme::{Theme, fonts, style};
use iced::widget::button::{Status, Style};
use iced::widget::{button, column, container, row, rule, text};
use iced::{Background, Border, Element, Length, Padding, alignment};

use super::format::{self, format_duration};
use super::model::{Measure, SessionRow};
use super::ranked::value_of_totals;
use super::ui::{Ui, one_line};
use crate::message::Message;

/// The project's chip: a quiet capsule that lights up under the pointer
/// (`.slist__project`).
fn chip(t: &Theme) -> impl Fn(&iced::Theme, Status) -> Style + '_ {
    move |_, status| {
        let c = &t.colors;
        let (background, text_color) = match status {
            Status::Hovered | Status::Pressed => (c.accent_soft, c.fg),
            _ => (c.line, c.fg_secondary),
        };
        Style {
            background: Some(Background::Color(background)),
            text_color,
            border: Border {
                radius: radius::XS.into(),
                ..Border::default()
            },
            ..Style::default()
        }
    }
}

/// What a session's project chip does: filter the page on it.
pub type Pick = fn(String) -> Message;

pub fn list<'a>(
    ui: Ui,
    rows: &[SessionRow],
    measure: Measure,
    agent_label: &dyn Fn(Agent) -> String,
    pick: Pick,
) -> Element<'a, Message> {
    let t = ui.t;
    let locale = ui.locale();
    if rows.is_empty() {
        return text(ui.s(Key::StatsNoData))
            .size(font_size::UI)
            .style(style::text::subtle(t))
            .into();
    }
    let mut list = column![];
    for (at, session) in rows.iter().enumerate() {
        let title = if session.title.is_empty() {
            ui.s(Key::StatsUntitled)
        } else {
            session.title.clone()
        };
        let who = [Some(agent_label(session.agent)), session.model.clone()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
        let separator = || text("·").size(font_size::CAPTION).style(style::text::subtle(t));
        let meta = row![
            hangar_ui::tip::bottom(
                t,
                button(one_line(session.project.name.clone()).size(font_size::CAPTION))
                    .padding(Padding::from([0.0, space::SM]))
                    .height(17)
                    .style(chip(t))
                    .on_press(pick(session.project.key.clone())),
                ui.tr
                    .t_with(Key::StatsShowOnly, &args!("name" => session.project.name.as_str()))
                    .into_owned(),
            ),
            separator(),
            one_line(who)
                .size(font_size::CAPTION)
                .style(style::text::muted(t)),
            separator(),
            one_line(format::day_month(session.last_at, locale))
                .size(font_size::CAPTION)
                .style(style::text::muted(t)),
        ]
        .spacing(space::SM)
        .align_y(alignment::Vertical::Center);

        let main = column![
            one_line(title)
                .size(font_size::UI_LG)
                .font(fonts::ui_medium())
                .style(style::text::primary(t)),
            container(meta).clip(true).width(Length::Fill),
        ]
        .spacing(2);
        let numbers = column![
            one_line(value_of_totals(&session.totals, measure, locale))
                .size(font_size::UI_LG)
                .font(fonts::ui_semibold())
                .style(style::text::primary(t)),
            one_line(
                ui.tr
                    .t_with(
                        Key::StatsSessionActive,
                        &args!("value" => format_duration(session.totals.active_ms as f64, ui.tr)),
                    )
                    .into_owned()
            )
            .size(font_size::CAPTION)
            .style(style::text::subtle(t)),
        ]
        .spacing(2)
        .align_x(alignment::Horizontal::Right);

        let line = row![
            container(
                text((at + 1).to_string())
                    .size(font_size::CAPTION)
                    .font(fonts::mono())
                    .style(style::text::subtle(t))
            )
            .width(18),
            container(main).width(Length::Fill).clip(true),
            numbers,
        ]
        .spacing(space::LG)
        .align_y(alignment::Vertical::Center);

        if at > 0 {
            list = list.push(rule::horizontal(1).style(style::rule::hairline(t)));
        }
        list = list.push(container(line).padding(Padding::from([7.0, 0.0])));
    }
    list.into()
}
