//! The plan's rate-limit windows, as the last Claude Code session to report
//! saw them (`NavigatorPlan.tsx`): one account behind every pane, so one block
//! for all of them. Nothing shows until a pane with the hangar-bridge mod has
//! reported.

use iced::widget::{Space, column, container, progress_bar, row, text, tooltip};
use iced::{Alignment, Background, Border, Element, Length, Padding};

use hangar_i18n::{Key, Locale, Translator, args};
use hangar_theme::tokens::{font_size, space};
use hangar_theme::{Theme, fonts, style};

use crate::ctx::Ctx;
use crate::inspector::format::{format_percent, reset_when, resets_at_ms};
use crate::message::Message;
use crate::panes_logic::wall_clock_ms;
use crate::state::live::{PlanLimits, RateLimit};

/// Past these, a window is close enough to its limit to say so in colour.
pub const WARN_AT: u8 = 70;
pub const HOT_AT: u8 = 90;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Ok,
    Warn,
    Hot,
}

/// A window's usage as the meter draws it: whole percent, held between 0 and
/// 100.
pub fn meter_pct(percent_used: f64) -> u8 {
    percent_used.round().clamp(0.0, 100.0) as u8
}

pub fn level_of(pct: u8) -> Level {
    if pct >= HOT_AT {
        Level::Hot
    } else if pct >= WARN_AT {
        Level::Warn
    } else {
        Level::Ok
    }
}

/// The name of a window: `five_hour` and `seven_day` are the plan's own, any
/// other is the gateway's, spelled with spaces (`label`).
pub fn window_label(tr: Translator, kind: &str) -> String {
    match kind {
        "five_hour" => tr.t(Key::SidebarPlanFiveHour).into_owned(),
        "seven_day" => tr.t(Key::SidebarPlanWeek).into_owned(),
        other => other.replace('_', " "),
    }
}

/// When the plan next frees up something: the soonest reset still to come
/// (in ms since the epoch), which is what a pane about to hit its limit is
/// waiting for.
pub fn next_reset(limits: &[RateLimit], now: i64) -> Option<i64> {
    limits
        .iter()
        .filter_map(resets_at_ms)
        .filter(|at| *at > now)
        .min()
}

/// A reset as the end of a sentence, in the language of `tr`
/// (`resetWhen`; the inspector says it the same way).
fn reset_text(tr: Translator, at: i64, now: i64) -> String {
    reset_when(at, now, tr.locale()).text(tr)
}

/// The tooltip: what the block is, then one line per window that says when it
/// resets.
pub fn hint(tr: Translator, limits: &[RateLimit], now: i64) -> String {
    let mut lines = vec![tr.t(Key::SidebarPlanHint).into_owned()];
    for limit in limits {
        let Some(at) = resets_at_ms(limit) else {
            continue;
        };
        lines.push(
            tr.t_with(
                Key::SidebarPlanResets,
                &args!(
                    "window" => window_label(tr, &limit.kind),
                    "reset" => reset_text(tr, at, now)
                ),
            )
            .into_owned(),
        );
    }
    lines.join("\n")
}

/// The report of a session, if it says anything: an empty one keeps the last
/// (`setPlanLimits`).
pub fn take(limits: &[RateLimit]) -> Option<PlanLimits> {
    (!limits.is_empty()).then(|| PlanLimits {
        limits: limits.to_vec(),
        // Wall-clock milliseconds, as the shared `PlanLimits` keeps every moment
        // (state/live.rs): the inspector shows them as clock times.
        at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() as u64),
    })
}

pub fn view<'a>(plan: Option<&'a PlanLimits>, ctx: Ctx<'a>) -> Option<Element<'a, Message>> {
    let plan = plan?;
    let t = ctx.theme;
    let tr = ctx.i18n;
    let locale: Locale = tr.locale();
    let now = i64::try_from(wall_clock_ms()).unwrap_or_default();

    let mut head = row![
        text(tr.t(Key::SidebarPlan).into_owned())
            .size(font_size::CAPTION)
            .font(fonts::ui_semibold())
            .color(t.colors.fg_muted),
        Space::new().width(Length::Fill),
    ]
    .spacing(space::MD)
    .align_y(Alignment::Center);
    if let Some(next) = next_reset(&plan.limits, now) {
        head = head.push(
            text(reset_text(tr, next, now))
                .size(font_size::CAPTION)
                .color(t.colors.fg_subtle)
                .wrapping(text::Wrapping::None),
        );
    }

    let mut meters = column![head].spacing(0);
    for limit in &plan.limits {
        let pct = meter_pct(limit.percent_used);
        let level = level_of(pct);
        let ink = match level {
            Level::Ok => t.colors.fg_muted,
            Level::Warn => t.colors.warning,
            Level::Hot => t.colors.danger,
        };
        let track = t.colors.line_strong;
        let bar = progress_bar(0.0..=100.0, f32::from(pct))
            .girth(4)
            .length(Length::Fill)
            .style(move |_: &iced::Theme| progress_bar::Style {
                background: Background::Color(track),
                bar: Background::Color(ink),
                border: Border {
                    radius: 2.0.into(),
                    ..Border::default()
                },
            });
        meters = meters.push(
            row![
                container(
                    text(window_label(tr, &limit.kind))
                        .size(font_size::CAPTION)
                        .color(t.colors.fg_muted)
                        .wrapping(text::Wrapping::None)
                )
                .width(24),
                container(bar).width(Length::Fill).center_y(Length::Fill),
                container(
                    text(format_percent(f64::from(pct), locale))
                        .size(font_size::LABEL)
                        .font(fonts::mono())
                        .color(if level == Level::Ok {
                            t.colors.fg_muted
                        } else {
                            ink
                        })
                )
                .width(34)
                .align_x(Alignment::End),
            ]
            .spacing(space::MD)
            .align_y(Alignment::Center)
            .height(16),
        );
    }

    let block = container(meters).width(Length::Fill).padding(Padding {
        top: 9.0,
        right: 16.0,
        bottom: 10.0,
        left: 16.0,
    });
    let words = hint(tr, &plan.limits, now);
    Some(
        column![
            rule(t),
            tooltip(
                block,
                container(text(words).size(font_size::CAPTION_LG))
                    .padding([space::XS - 1.0, space::MD - 1.0])
                    .style(style::container::tooltip(t)),
                tooltip::Position::Top,
            )
        ]
        .into(),
    )
}

fn rule<'a>(t: &'a Theme) -> Element<'a, Message> {
    container(Space::new())
        .width(Length::Fill)
        .height(1)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(Background::Color(t.colors.line)),
            ..container::Style::default()
        })
        .into()
}

#[cfg(test)]
mod tests;
