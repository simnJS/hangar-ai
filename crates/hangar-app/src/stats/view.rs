//! The statistics page: a bar of controls that scopes everything under it,
//! then cards on the work area's own background (`StatsView.tsx`).
//!
//! One row of controls scopes the whole page (the period, the project, and
//! whether the charts measure tokens or cost), so every number on screen is
//! about the same slice. What was picked lasts as long as the window does.

use std::sync::Arc;
use std::time::{Duration, Instant};

use hangar_core::stats::Agent;
use hangar_i18n::{Key, args};
use hangar_theme::tokens::{font_size, radius, size, space};
use hangar_theme::{Theme, fonts, style};
use hangar_ui::button::{self as ui_button};
use hangar_ui::{Icon, icon, menu, tip};
use iced::widget::canvas::{self, Canvas, Path, Stroke};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{Space, button, column, container, responsive, row, rule, scrollable, text};
use iced::{Element, Length, Padding, Point, Radians, Rectangle, Renderer, alignment, mouse};

use super::charts::columns::Columns;
use super::charts::heatmap::Heatmap;
use super::charts::sparkline::sparkline;
use super::charts::{ChartId, KINDS};
use super::format::{
    self, format_change, format_compact, format_duration, format_integer, format_share, format_usd,
};
use super::model::{Granularity, Measure, PERIODS, Period, Summary, agent_key, change};
use super::ranked::{self, List};
use super::ui::{self, Ui, measure_key, one_line};
use super::{Computed, Msg, State, sessions, tables};
use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::types::AGENTS;

/// How many projects show before the rest fold under "N more".
const PROJECTS_SHOWN: usize = 7;
/// The page's container queries: the status line goes under 52 em, the title
/// under 40 em (of the 13 px interface font), the cards stack under 760 px
/// and the legend goes under 900.
const NO_STATUS_BELOW: f32 = 676.0;
const NO_TITLE_BELOW: f32 = 520.0;
const STACK_BELOW: f32 = 760.0;
const NO_LEGEND_BELOW: f32 = 900.0;
const PAGE_MAX_WIDTH: f32 = 1240.0;

fn msg(m: Msg) -> Message {
    Message::Stats(m)
}

/// Region `main` in the stats view.
pub fn view<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    let ui = Ui::new(&ctx);
    let computed = st.computed(&ctx);
    responsive(move |bounds| page(st, ctx, ui, computed.clone(), bounds.width)).into()
}

fn page<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    ui: Ui,
    computed: Option<Arc<Computed>>,
    width: f32,
) -> Element<'a, Message> {
    let live = &ctx.model.live.stats;
    let bar = bar(st, ctx, ui, computed.as_deref(), width);
    let body: Element<'a, Message> = match (&live.data, &computed) {
        (Some(data), Some(computed)) => {
            if data.buckets.is_empty() && !live.refreshing {
                empty_state(ui, data)
            } else {
                content(st, ctx, ui, computed, data, width)
            }
        }
        _ => match &live.error {
            Some(error) => hangar_ui::empty::empty(ui.t, Icon::Alert, ui.s(Key::StatsErrorTitle))
                .description(error.clone())
                .action(
                    ui_button::secondary(ui.t, ui.s(Key::StatsRetry)).on_press(msg(Msg::Refresh)),
                )
                .into(),
            None => hangar_ui::empty::empty(ui.t, Icon::Chart, ui.s(Key::StatsLoadingTitle))
                .description(ui.s(Key::StatsLoadingBody))
                .into(),
        },
    };
    container(column![
        bar,
        rule::horizontal(1).style(style::rule::sep(ui.t)),
        body
    ])
    .width(Length::Fill)
    .height(Length::Fill)
    .style(ui::page(ui.t))
    .into()
}

// ---------------------------------------------------------------------------
// The bar
// ---------------------------------------------------------------------------

/// A radio group drawn as a segmented control.
fn choice<'a, V: Copy + PartialEq + 'a>(
    ui: Ui,
    options: Vec<(V, String, String)>,
    value: V,
    on_select: fn(V) -> Message,
) -> Element<'a, Message> {
    let t = ui.t;
    let segments = options.into_iter().map(|(option, label, hint)| {
        let segment = button(
            container(
                text(label)
                    .size(font_size::UI)
                    .font(fonts::ui_medium())
                    .wrapping(text::Wrapping::None),
            )
            .center_y(Length::Fill),
        )
        .padding(Padding::from([0.0, 9.0]))
        .height(Length::Fill)
        .style(style::button::tab(t, option == value))
        .on_press(on_select(option));
        tip::bottom(t, segment, hint)
    });
    container(row(segments).spacing(space::XXS))
        .padding(space::XXS)
        .height(size::CONTROL_HEIGHT)
        .style(style::container::segmented(t))
        .into()
}

/// The project filter's button: a field-like control, so it reads as a
/// choice; in the accent while a filter is on, because it changes every
/// number (`.stats__picker`).
fn picker_style(
    t: &'static Theme,
    filtered: bool,
    open: bool,
) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_, status| {
        let c = &t.colors;
        let lit = open || matches!(status, button::Status::Hovered | button::Status::Pressed);
        let (background, text_color, border) = if filtered {
            (
                c.accent_soft,
                c.fg,
                iced::Border {
                    color: c.accent_ring,
                    width: 1.0,
                    radius: radius::MD.into(),
                },
            )
        } else if lit {
            (
                c.line_strong,
                c.fg,
                iced::Border {
                    radius: radius::MD.into(),
                    ..iced::Border::default()
                },
            )
        } else {
            (
                c.line,
                c.fg_secondary,
                iced::Border {
                    radius: radius::MD.into(),
                    ..iced::Border::default()
                },
            )
        };
        button::Style {
            background: Some(iced::Background::Color(background)),
            text_color,
            border,
            ..button::Style::default()
        }
    }
}

/// Reading the transcripts: a ring turning, never a bar that pretends to know
/// how far along it is (`.stats__spinner`).
struct Spinner {
    theme: &'static Theme,
    angle: f32,
}

impl canvas::Program<Message> for Spinner {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let centre = Point::new(bounds.width / 2.0, bounds.height / 2.0);
        let radius = bounds.width.min(bounds.height) / 2.0 - 1.0;
        frame.stroke(
            &Path::circle(centre, radius),
            Stroke::default()
                .with_color(self.theme.colors.line_strong)
                .with_width(1.5),
        );
        let arc = Path::new(|path| {
            path.arc(canvas::path::Arc {
                center: centre,
                radius,
                start_angle: Radians(self.angle),
                end_angle: Radians(self.angle + std::f32::consts::FRAC_PI_2),
            });
        });
        frame.stroke(
            &arc,
            Stroke::default()
                .with_color(self.theme.colors.accent)
                .with_width(1.5),
        );
        vec![frame.into_geometry()]
    }
}

/// How far a spinner has turned `since` it started: one turn in 0.9 s.
pub fn spin_angle(since: Option<Instant>, now: Instant) -> f32 {
    let elapsed = since.map_or(Duration::ZERO, |since| now.saturating_duration_since(since));
    (elapsed.as_secs_f32() / 0.9).fract() * std::f32::consts::TAU
}

fn status<'a>(ctx: Ctx<'a>, ui: Ui) -> Option<Element<'a, Message>> {
    let t = ui.t;
    let live = &ctx.model.live.stats;
    let small = |content: String| {
        text(content)
            .size(font_size::CAPTION_LG)
            .wrapping(text::Wrapping::None)
    };
    if live.refreshing {
        let spinner = Canvas::new(Spinner {
            theme: t,
            angle: spin_angle(live.refresh_started, ctx.now),
        })
        .width(10)
        .height(10);
        return Some(
            row![
                spinner,
                small(ui.s(Key::StatsRefreshing)).style(style::text::muted(t))
            ]
            .spacing(7)
            .align_y(alignment::Vertical::Center)
            .into(),
        );
    }
    if let Some(checked) = live.checked_at {
        let time = format::prose_time(checked, ui.locale());
        return Some(
            small(
                ui.tr
                    .t_with(Key::StatsUpToDate, &args!("time" => time))
                    .into_owned(),
            )
            .style(style::text::subtle(t))
            .into(),
        );
    }
    live.data
        .as_ref()
        .filter(|data| data.scan.cached_only)
        .map(|_| {
            small(ui.s(Key::StatsFromCache))
                .style(style::text::subtle(t))
                .into()
        })
}

fn bar<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    ui: Ui,
    computed: Option<&Computed>,
    width: f32,
) -> Element<'a, Message> {
    let t = ui.t;
    let live = &ctx.model.live.stats;

    let periods = PERIODS
        .iter()
        .map(|period| {
            let (label, hint) = period_keys(*period);
            (*period, ui.s(label), ui.s(hint))
        })
        .collect();
    let period = choice(ui, periods, st.period, |period| msg(Msg::SetPeriod(period)));

    let measures = [Measure::Tokens, Measure::Cost]
        .into_iter()
        .map(|measure| {
            let hint = match measure {
                Measure::Tokens => Key::StatsMeasureHintTokens,
                Measure::Cost => Key::StatsMeasureHintCost,
            };
            (measure, ui.s(measure_key(measure)), ui.s(hint))
        })
        .collect();
    let measure = choice(ui, measures, st.measure, |measure| {
        msg(Msg::SetMeasure(measure))
    });

    // A filter on a project the period no longer has keeps its name.
    let project_name = st.project.as_ref().and_then(|key| {
        computed.and_then(|computed| {
            computed
                .summary
                .projects
                .iter()
                .find(|row| &row.key == key)
                .map(|row| row.label.clone())
                .or_else(|| {
                    computed
                        .projects
                        .iter()
                        .find(|project| &project.key == key)
                        .map(|project| project.name.clone())
                })
        })
    });
    let filtered = st.project.is_some();
    let label = project_name.unwrap_or_else(|| ui.s(Key::StatsAllProjects));
    let tint = if filtered {
        t.colors.accent_hi
    } else {
        t.colors.fg_muted
    };
    let picker_face = row![
        icon(Icon::Folder, 14.0).color(tint),
        container(one_line(label).size(font_size::UI).font(fonts::ui_medium()))
            .max_width(210)
            .clip(true),
        icon(Icon::ChevronDown, 12.0).color(t.colors.fg_muted),
    ]
    .spacing(space::SM)
    .align_y(alignment::Vertical::Center);
    let picker = tip::bottom(
        t,
        button(picker_face)
            .padding(Padding {
                top: 0.0,
                right: space::MD,
                bottom: 0.0,
                left: 9.0,
            })
            .height(size::CONTROL_HEIGHT)
            .style(picker_style(t, filtered, st.menu_open))
            .on_press_maybe(computed.map(|_| msg(Msg::ProjectMenu(!st.menu_open)))),
        ui.s(Key::StatsProjectFilter),
    );
    let entries = (st.menu_open)
        .then_some(computed)
        .flatten()
        .map(|computed| project_entries(st, ui, &computed.summary));
    let picker = menu::dropdown(t, picker, entries).on_dismiss(msg(Msg::ProjectMenu(false)));
    let mut project = row![picker].spacing(2).align_y(alignment::Vertical::Center);
    if filtered {
        project = project.push(
            ui_button::icon_button(t, Icon::Close)
                .small()
                .tip(ui.s(Key::StatsClearProject))
                .on_press(msg(Msg::SetProject(None))),
        );
    }

    let refresh = ui_button::icon_button(t, Icon::Restart)
        .tip(ui.s(Key::StatsRefresh))
        .on_press_maybe((!live.refreshing).then(|| msg(Msg::Refresh)));

    let mut left = row![].spacing(10).align_y(alignment::Vertical::Center);
    if width >= NO_TITLE_BELOW {
        left = left.push(
            text(ui.s(Key::StatsTitle))
                .size(font_size::BODY)
                .font(fonts::ui_semibold())
                .style(style::text::primary(t)),
        );
    }
    left = left.push(period).push(project);

    let mut right = row![].spacing(10).align_y(alignment::Vertical::Center);
    if width >= NO_STATUS_BELOW
        && let Some(status) = status(ctx, ui)
    {
        right = right.push(status);
    }
    right = right.push(measure).push(refresh);

    container(
        row![left, Space::new().width(Length::Fill), right]
            .spacing(10)
            .align_y(alignment::Vertical::Center),
    )
    .padding(Padding {
        top: space::MD,
        right: space::XL,
        bottom: space::MD,
        left: space::XXL,
    })
    .width(Length::Fill)
    .style(style::container::pane_head(t, false))
    .into()
}

fn period_keys(period: Period) -> (Key, Key) {
    match period {
        Period::D7 => (Key::StatsPeriod7d, Key::StatsPeriodHint7d),
        Period::D30 => (Key::StatsPeriod30d, Key::StatsPeriodHint30d),
        Period::D90 => (Key::StatsPeriod90d, Key::StatsPeriodHint90d),
        Period::All => (Key::StatsPeriodAll, Key::StatsPeriodHintAll),
    }
}

/// The project menu: "All projects", a separator, then each project with a
/// check mark and its share.
fn project_entries(st: &State, ui: Ui, summary: &Summary) -> Vec<menu::Entry<Message>> {
    let locale = ui.locale();
    let mut entries: Vec<menu::Entry<Message>> = vec![
        menu::item(ui.s(Key::StatsAllProjects))
            .checked(st.project.is_none())
            .on_select(msg(Msg::SetProject(None)))
            .into(),
        menu::separator(),
    ];
    for row in &summary.projects {
        entries.push(
            menu::item(row.label.clone())
                .checked(st.project.as_deref() == Some(row.key.as_str()))
                .shortcut(format_share(row.share, locale))
                .on_select(msg(Msg::SetProject(Some(row.key.clone()))))
                .into(),
        );
    }
    entries
}

// ---------------------------------------------------------------------------
// States of the whole page
// ---------------------------------------------------------------------------

/// Nothing to show: where it looked, the first thing to check when a machine
/// with transcripts shows none.
fn empty_state<'a>(ui: Ui, data: &hangar_core::stats::UsageStats) -> Element<'a, Message> {
    let t = ui.t;
    let mut state = hangar_ui::empty::empty(t, Icon::Chart, ui.s(Key::StatsEmptyTitle))
        .description(ui.s(Key::StatsEmptyBody));
    if !data.sources.is_empty() {
        let mut sources = column![].spacing(space::XS);
        for source in &data.sources {
            let count = if source.found {
                ui.tr
                    .tn(Key::StatsSourceFiles, source.files, &[])
                    .into_owned()
            } else {
                ui.s(Key::StatsSourceMissing)
            };
            sources = sources.push(
                row![
                    container(
                        one_line(source.root.clone())
                            .size(font_size::CAPTION)
                            .font(fonts::mono())
                            .style(style::text::secondary(t))
                    )
                    .width(Length::Fill)
                    .clip(true),
                    text(count)
                        .size(font_size::CAPTION)
                        .style(style::text::subtle(t)),
                ]
                .spacing(space::XXL)
                .align_y(alignment::Vertical::Center),
            );
        }
        state = state.action(
            container(sources)
                .padding(Padding::from([space::MD, space::XL]))
                .style(style::container::chip(t)),
        );
    }
    state.into()
}

// ---------------------------------------------------------------------------
// Cards
// ---------------------------------------------------------------------------

/// A card of the page: a title, a quiet hint, and what it holds.
fn card<'a>(
    ui: Ui,
    title: String,
    hint: Option<String>,
    actions: Option<Element<'a, Message>>,
    body: Element<'a, Message>,
) -> Element<'a, Message> {
    let t = ui.t;
    let mut titles = row![
        one_line(title)
            .size(font_size::BODY)
            .font(fonts::ui_semibold())
            .style(style::text::primary(t))
    ]
    .spacing(space::MD)
    .align_y(alignment::Vertical::Bottom);
    if let Some(hint) = hint {
        titles = titles.push(
            one_line(hint)
                .size(font_size::CAPTION_LG)
                .style(style::text::subtle(t)),
        );
    }
    let mut head = row![container(titles).width(Length::Fill).clip(true)]
        .spacing(space::XL)
        .align_y(alignment::Vertical::Center);
    if let Some(actions) = actions {
        head = head.push(actions);
    }
    container(column![head, body].spacing(space::XL))
        .padding(Padding {
            top: 13.0,
            right: 15.0,
            bottom: 15.0,
            left: 15.0,
        })
        .width(Length::Fill)
        .style(ui::card(t))
        .into()
}

/// Chart or table: the same values, two ways to read them.
fn table_toggle<'a>(ui: Ui, table: bool, on_press: Message) -> Element<'a, Message> {
    let words = ui.s(if table {
        Key::StatsShowChart
    } else {
        Key::StatsShowTable
    });
    ui_button::toggle_button(ui.t, if table { Icon::Chart } else { Icon::Table }, table)
        .tip(words)
        .on_press(on_press)
        .into()
}

fn legend<'a>(ui: Ui) -> Element<'a, Message> {
    let t = ui.t;
    let mut items = row![]
        .spacing(space::XXL)
        .align_y(alignment::Vertical::Center);
    for kind in KINDS.iter().rev() {
        items = items.push(
            row![
                container(Space::new())
                    .width(8)
                    .height(8)
                    .style(ui::swatch(kind.color(t), 2.0)),
                text(ui.s(kind.key()))
                    .size(font_size::CAPTION_LG)
                    .style(style::text::secondary(t)),
            ]
            .spacing(space::SM)
            .align_y(alignment::Vertical::Center),
        );
    }
    items.into()
}

fn muted_line<'a>(ui: Ui, words: String) -> Element<'a, Message> {
    text(words)
        .size(font_size::UI)
        .style(style::text::subtle(ui.t))
        .into()
}

// ---------------------------------------------------------------------------
// Key figures
// ---------------------------------------------------------------------------

/// The trend under a key figure: "▲ +24 % vs the 30 days before".
fn trend<'a>(ui: Ui, now: f64, before: f64, days: i64) -> Element<'a, Message> {
    let t = ui.t;
    let locale = ui.locale();
    let line = |content: String| {
        text(content)
            .size(font_size::CAPTION_LG)
            .style(style::text::secondary(t))
    };
    let Some(ratio) = change(now, before) else {
        let words = if now > 0.0 {
            ui.tr
                .t_with(Key::StatsTrendNew, &args!("n" => days))
                .into_owned()
        } else {
            String::new()
        };
        return line(words).into();
    };
    // Rounded to nothing, it is no trend.
    if ratio.abs() < 0.005 {
        return line(
            ui.tr
                .t_with(Key::StatsTrendFlat, &args!("n" => days))
                .into_owned(),
        )
        .into();
    }
    // Up or down is not good or bad here (more work done, more tokens spent),
    // so the arrow wears no colour.
    row![
        text(if ratio > 0.0 { "▲" } else { "▼" })
            .size(font_size::CAPTION_LG * 0.72)
            .style(style::text::muted(t)),
        text(format_change(ratio, locale))
            .size(font_size::CAPTION_LG)
            .font(fonts::ui_semibold())
            .style(style::text::secondary(t)),
        line(
            ui.tr
                .t_with(Key::StatsTrendVs, &args!("n" => days))
                .into_owned()
        ),
    ]
    .spacing(4)
    .align_y(alignment::Vertical::Center)
    .into()
}

struct Kpi {
    label: String,
    value: String,
    trend: Option<(f64, f64)>,
    detail: Option<String>,
    note: bool,
    spark: Vec<f64>,
}

fn kpi<'a>(ui: Ui, kpi: Kpi, days: i64, since: Option<String>) -> Element<'a, Message> {
    let t = ui.t;
    let mut body = column![
        row![
            container(
                one_line(kpi.label)
                    .size(font_size::UI)
                    .font(fonts::ui_medium())
                    .style(style::text::secondary(t))
            )
            .width(Length::Fill)
            .clip(true),
            sparkline(t, kpi.spark),
        ]
        .spacing(space::MD)
        .align_y(alignment::Vertical::Top),
        one_line(kpi.value)
            .size(font_size::DISPLAY)
            .font(fonts::ui_semibold())
            .style(style::text::primary(t)),
    ]
    .spacing(3);
    body = body.push(Space::new().height(1));
    match (kpi.trend, &since) {
        (Some((now, before)), _) => body = body.push(trend(ui, now, before, days)),
        (None, Some(since)) => {
            body = body.push(
                text(since.clone())
                    .size(font_size::CAPTION_LG)
                    .style(style::text::secondary(t)),
            );
        }
        _ => {}
    }
    if let Some(detail) = kpi.detail {
        body = body.push(text(detail).size(font_size::CAPTION_LG).color(if kpi.note {
            t.colors.fg_subtle
        } else {
            t.colors.fg_muted
        }));
    }
    container(body)
        .padding(Padding {
            top: 13.0,
            right: 15.0,
            bottom: 14.0,
            left: 15.0,
        })
        .width(Length::Fill)
        .style(ui::card(t))
        .into()
}

// ---------------------------------------------------------------------------
// The page
// ---------------------------------------------------------------------------

fn agent_label(agent: Agent) -> String {
    let id = match agent {
        Agent::Claude => crate::state::types::AgentId::Claude,
        Agent::Codex => crate::state::types::AgentId::Codex,
    };
    AGENTS.iter().find(|entry| entry.id == id).map_or_else(
        || agent_key(agent).to_owned(),
        |entry| entry.label.to_owned(),
    )
}

pub(super) fn agent_name(agent: Agent) -> String {
    agent_label(agent)
}

fn content<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    ui: Ui,
    computed: &Arc<Computed>,
    data: &Arc<hangar_core::stats::UsageStats>,
    width: f32,
) -> Element<'a, Message> {
    let t = ui.t;
    let locale = ui.locale();
    let live = &ctx.model.live.stats;
    let summary = &computed.summary;
    let page_width = width - 40.0;
    let narrow = page_width < STACK_BELOW;
    let days = summary.range.days;
    let measure = st.measure;
    let measure_name = ui.s(measure_key(measure));

    let mut page = column![].spacing(14);
    if let Some(error) = &live.error {
        page = page.push(
            container(
                row![
                    icon(Icon::Alert, 14.0).color(t.colors.danger),
                    text(format!("{} — {error}", ui.s(Key::StatsErrorTitle)))
                        .size(font_size::UI)
                        .style(style::text::danger(t)),
                ]
                .spacing(space::MD)
                .align_y(alignment::Vertical::Center),
            )
            .padding(Padding::from([space::MD, space::XL]))
            .width(Length::Fill)
            .style(style::container::badge_danger(t)),
        );
    }

    // Key figures.
    let totals = &summary.totals;
    let previous = summary.previous.as_ref();
    let since = (st.period == Period::All)
        .then_some(summary.first_at)
        .flatten()
        .map(|first| {
            ui.tr
                .t_with(
                    Key::StatsSince,
                    &args!("date" => format::long_date(first, locale)),
                )
                .into_owned()
        });
    let spark = |figure: &dyn Fn(&super::model::Totals) -> f64| -> Vec<f64> {
        summary
            .series
            .iter()
            .map(|point| figure(&point.totals))
            .collect()
    };
    let cache_share = if totals.tokens > 0 {
        totals.cache_read as f64 / totals.tokens as f64
    } else {
        0.0
    };
    let kpis = [
        Kpi {
            label: ui.s(Key::StatsKpiTokens),
            value: format_compact(totals.tokens as f64, locale),
            trend: previous.map(|before| (totals.tokens as f64, before.tokens as f64)),
            detail: Some(
                ui.tr
                    .t_with(
                        Key::StatsKpiTokensDetail,
                        &args!("share" => format_share(cache_share, locale)),
                    )
                    .into_owned(),
            ),
            note: false,
            spark: spark(&|totals| totals.tokens as f64),
        },
        Kpi {
            label: ui.s(Key::StatsKpiCost),
            value: format_usd(totals.cost, locale, false),
            trend: previous.map(|before| (totals.cost, before.cost)),
            detail: Some(ui.s(Key::StatsKpiCostNote)),
            note: true,
            spark: spark(&|totals| totals.cost),
        },
        Kpi {
            label: ui.s(Key::StatsKpiActive),
            value: format_duration(totals.active_ms as f64, ui.tr),
            trend: previous.map(|before| (totals.active_ms as f64, before.active_ms as f64)),
            detail: (summary.active_days > 0).then(|| {
                ui.tr
                    .t_with(
                        Key::StatsKpiActiveDetail,
                        &args!("value" => format_duration(
                            totals.active_ms as f64 / summary.active_days as f64,
                            ui.tr,
                        )),
                    )
                    .into_owned()
            }),
            note: false,
            spark: spark(&|totals| totals.active_ms as f64),
        },
        Kpi {
            label: ui.s(Key::StatsKpiSessions),
            value: format_integer(totals.sessions as f64, locale),
            trend: previous.map(|before| (totals.sessions as f64, before.sessions as f64)),
            detail: Some(ui.tr.tn(Key::StatsPrompts, totals.turns, &[]).into_owned()),
            note: false,
            spark: spark(&|totals| totals.sessions as f64),
        },
    ];
    let mut cells: Vec<Element<'a, Message>> = kpis
        .into_iter()
        .map(|figure| kpi(ui, figure, days, since.clone()))
        .collect();
    if narrow {
        // Two by two.
        let second = cells.split_off(2);
        let mut rows = column![].spacing(space::XL + 0.0);
        rows = rows.push(row(cells).spacing(space::XL));
        rows = rows.push(row(second).spacing(space::XL));
        page = page.push(rows);
    } else {
        page = page.push(row(cells).spacing(space::XL));
    }

    // The period, column by column.
    let week = summary.granularity == Granularity::Week;
    let daily_title = ui.s(if week {
        Key::StatsWeeklyTitle
    } else {
        Key::StatsDailyTitle
    });
    let daily_hint = ui
        .tr
        .t_with(
            Key::StatsDailyHint,
            &args!("measure" => measure_name.as_str()),
        )
        .into_owned();
    let mut actions = row![]
        .spacing(space::XL)
        .align_y(alignment::Vertical::Center);
    if page_width >= NO_LEGEND_BELOW {
        actions = actions.push(legend(ui));
    }
    actions = actions.push(table_toggle(ui, st.daily_table, msg(Msg::ToggleDailyTable)));
    let daily_body: Element<'a, Message> = if st.daily_table {
        tables::columns(ui, summary, measure)
    } else {
        Columns {
            theme: t,
            locale,
            measure,
            summary: summary.clone(),
            active: st.columns,
            keyboard: st.keyboard,
            focused: st.focus == Some(ChartId::Columns),
            empty: (ui.s(Key::StatsNoData), ui.s(Key::StatsNoDataHint)),
            on_event: |event| msg(Msg::Chart(ChartId::Columns, event)),
        }
        .view(ui.tr)
    };
    page = page.push(card(
        ui,
        daily_title,
        Some(daily_hint),
        Some(actions.into()),
        daily_body,
    ));

    // Two columns that stack on their own, so a long ranking never leaves a
    // hole beside a short card.
    let first = column![projects_card(st, ui, summary), agents_card(st, ui, summary)]
        .spacing(14)
        .width(Length::Fill);
    let second = column![
        heat_card(st, ui, summary),
        sessions_card(ui, summary, measure, &measure_name)
    ]
    .spacing(14)
    .width(Length::Fill);
    if narrow {
        page = page.push(first).push(second);
    } else {
        page = page.push(
            row![first, second]
                .spacing(14)
                .align_y(alignment::Vertical::Top),
        );
    }

    page = page.push(method(ui, data));

    scrollable(
        container(container(page).max_width(PAGE_MAX_WIDTH))
            .width(Length::Fill)
            .center_x(Length::Fill)
            .padding(Padding {
                top: 18.0,
                right: 20.0,
                bottom: 32.0,
                left: 20.0,
            }),
    )
    .direction(Direction::Vertical(Scrollbar::new().width(size::SCROLLBAR)))
    .style(style::scrollable::default(t))
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn sessions_and_time(ui: Ui) -> impl Fn(&super::model::Ranked) -> String {
    move |row| {
        format!(
            "{} · {}",
            ui.tr
                .tn(Key::StatsSessions, row.totals.sessions, &[])
                .into_owned(),
            format_duration(row.totals.active_ms as f64, ui.tr)
        )
    }
}

fn projects_card<'a>(st: &State, ui: Ui, summary: &Summary) -> Element<'a, Message> {
    let rows = &summary.projects;
    let body: Element<'a, Message> = if rows.is_empty() {
        muted_line(ui, ui.s(Key::StatsNoData))
    } else {
        // Seven show; a project that is filtered on stays in sight.
        let foldable = rows.len() > PROJECTS_SHOWN + 1;
        let shown: Vec<_> = if st.all_projects || !foldable {
            rows.clone()
        } else {
            rows.iter()
                .enumerate()
                .filter(|(at, row)| *at < PROJECTS_SHOWN || st.project.as_deref() == Some(&row.key))
                .map(|(_, row)| row.clone())
                .collect()
        };
        let hidden = rows.len() - shown.len();
        let sub = sessions_and_time(ui);
        let selected = st.project.clone();
        let tip_ui = ui;
        let pick_tip = move |row: &super::model::Ranked| {
            if selected.as_deref() == Some(row.key.as_str()) {
                tip_ui.s(Key::StatsClearProject)
            } else {
                tip_ui
                    .tr
                    .t_with(Key::StatsShowOnly, &args!("name" => row.label.as_str()))
                    .into_owned()
            }
        };
        let mut body = column![ranked::list(
            ui,
            List {
                rows: &shown,
                measure: st.measure,
                selected: st.project.as_deref(),
                pick: Some(|key| msg(Msg::TogglePick(key))),
                pick_tip: Some(&pick_tip),
                sub: Some(&sub),
            },
        )]
        .spacing(space::MD);
        if foldable && (hidden > 0 || st.all_projects) {
            let words = if st.all_projects {
                ui.s(Key::StatsFewerProjects)
            } else {
                ui.tr.tn(Key::StatsMoreProjects, hidden, &[]).into_owned()
            };
            body = body.push(
                ui_button::ghost(ui.t, words)
                    .small()
                    .on_press(msg(Msg::ToggleAllProjects)),
            );
        }
        body.into()
    };
    card(
        ui,
        ui.s(Key::StatsProjectsTitle),
        Some(ui.s(Key::StatsProjectsHint)),
        None,
        body,
    )
}

fn agents_card<'a>(st: &State, ui: Ui, summary: &Summary) -> Element<'a, Message> {
    let t = ui.t;
    let sub = sessions_and_time(ui);
    let mut body = column![].spacing(space::XL);
    if summary.agents.is_empty() {
        body = body.push(muted_line(ui, ui.s(Key::StatsNoData)));
    } else {
        body = body.push(ranked::list(
            ui,
            List {
                rows: &summary.agents,
                measure: st.measure,
                selected: None,
                pick: None,
                pick_tip: None,
                sub: Some(&sub),
            },
        ));
    }
    body = body.push(
        column![
            rule::horizontal(1).style(style::rule::hairline(t)),
            Space::new().height(space::MD),
            text(ui.s(Key::StatsModelsTitle))
                .size(font_size::BODY)
                .font(fonts::ui_semibold())
                .style(style::text::primary(t)),
        ]
        .spacing(space::XS),
    );
    if summary.models.is_empty() {
        body = body.push(muted_line(ui, ui.s(Key::StatsNoData)));
    } else {
        body = body.push(ranked::list(
            ui,
            List {
                rows: &summary.models,
                measure: st.measure,
                selected: None,
                pick: None,
                pick_tip: None,
                sub: None,
            },
        ));
    }
    if summary.unpriced_tokens > 0 {
        body = body.push(
            text(
                ui.tr
                    .t_with(
                        Key::StatsUnpricedNote,
                        &args!("tokens" => format_compact(summary.unpriced_tokens as f64, ui.locale())),
                    )
                    .into_owned(),
            )
            .size(font_size::CAPTION_LG)
            .style(style::text::subtle(t)),
        );
    }
    card(ui, ui.s(Key::StatsAgentsTitle), None, None, body.into())
}

fn heat_card<'a>(st: &State, ui: Ui, summary: &Arc<Summary>) -> Element<'a, Message> {
    let title = ui.s(Key::StatsHeatTitle);
    let body: Element<'a, Message> = if st.heat_table {
        tables::heat(ui, summary)
    } else {
        Heatmap {
            theme: ui.t,
            locale: ui.locale(),
            summary: summary.clone(),
            active: st.heat,
            keyboard: st.keyboard,
            focused: st.focus == Some(ChartId::Heat),
            hours: [0usize, 6, 12, 18].map(|hour| {
                ui.tr
                    .t_with(Key::StatsHeatHour, &args!("h" => hour))
                    .into_owned()
            }),
            on_event: |event| msg(Msg::Chart(ChartId::Heat, event)),
        }
        .view(ui.tr)
    };
    card(
        ui,
        title,
        Some(ui.s(Key::StatsHeatHint)),
        Some(table_toggle(ui, st.heat_table, msg(Msg::ToggleHeatTable))),
        body,
    )
}

fn sessions_card<'a>(
    ui: Ui,
    summary: &Summary,
    measure: Measure,
    measure_name: &str,
) -> Element<'a, Message> {
    let hint = ui
        .tr
        .t_with(
            Key::StatsSessionsHint,
            &args!("measure" => measure_name.to_lowercase()),
        )
        .into_owned();
    card(
        ui,
        ui.s(Key::StatsSessionsTitle),
        Some(hint),
        None,
        sessions::list(ui, &summary.sessions, measure, &agent_label, |key| {
            msg(Msg::SetProject(Some(key)))
        }),
    )
}

/// The footer: where the numbers come from.
fn method<'a>(ui: Ui, data: &hangar_core::stats::UsageStats) -> Element<'a, Message> {
    let t = ui.t;
    let line = |words: String| {
        text(words)
            .size(font_size::CAPTION_LG)
            .style(style::text::subtle(t))
    };
    let mut lines = column![
        row![
            icon(Icon::Info, 13.0).color(t.colors.fg_muted),
            text(ui.s(Key::StatsMethodTitle))
                .size(font_size::UI)
                .font(fonts::ui_semibold())
                .style(style::text::muted(t)),
        ]
        .spacing(6)
        .align_y(alignment::Vertical::Center),
        line(ui.s(Key::StatsMethodSources)),
        line(
            ui.tr
                .t_with(
                    Key::StatsMethodActive,
                    &args!("cap" => format_duration(data.active_gap_cap_ms as f64, ui.tr)),
                )
                .into_owned()
        ),
        line(ui.s(Key::StatsMethodCost)),
    ]
    .spacing(5);
    if !data.scan.cached_only {
        lines = lines.push(
            text(
                ui.tr
                    .t_with(
                        Key::StatsMethodScan,
                        &args!("files" => data.scan.files, "read" => data.scan.parsed, "ms" => data.scan.ms),
                    )
                    .into_owned(),
            )
            .size(font_size::LABEL)
            .font(fonts::mono())
            .style(style::text::subtle(t)),
        );
    }
    container(lines)
        .max_width(780)
        .padding(Padding {
            top: 4.0,
            right: 2.0,
            bottom: 0.0,
            left: 2.0,
        })
        .into()
}
