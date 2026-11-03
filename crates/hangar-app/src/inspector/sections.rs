//! The inspector's sections (`InspectorPanels.tsx`): session, terminal,
//! context and cost, the board task, the prompt queue and the recent
//! activity. The Files section is `files.rs`.
//!
//! Each is a function of what it shows, built from a [`Scene`] — the pane,
//! the workspace, the clock, the language — so the column is a pure
//! function of the model and a snapshot test can draw it.

use hangar_core::bridge::Queued;
use hangar_i18n::{Key, Locale, Translator, args};
use hangar_theme::tokens::{font_size, radius, space};
use hangar_theme::{AgentState, Theme, fonts, style};
use hangar_ui::button::icon_button;
use hangar_ui::glyph::{self, GlyphState};
use hangar_ui::section::{self, Section};
use hangar_ui::tail_text::tail_text;
use hangar_ui::{Icon, icon, tip};
use iced::widget::text::{self, Text};
use iced::widget::{button, column, container, mouse_area, row, space as filler};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, mouse};

use super::format;
use super::sources::{self, TaskCard};
use super::widgets::{self, CARD_RADIUS, TILE_RADIUS, kv, stacked};
use super::{Msg, SectionId};
use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::Model;
use crate::state::live::{ContextGauge, PaneEvent, PaneInsight, PaneMcp, PaneMcpMode, RateLimit};
use crate::state::types::{AgentId, Pane, Workspace};
use crate::{board, panes_logic};

/// Everything a section needs besides what it shows.
#[derive(Clone, Copy)]
pub(super) struct Scene<'a> {
    pub ctx: Ctx<'a>,
    pub ws: &'a Workspace,
    pub pane: &'a Pane,
    /// Epoch milliseconds: the clock the sections word elapsed times with.
    pub now: i64,
    /// Section ids folded away (`settings.inspectorCollapsed`).
    pub folded: &'a [String],
    /// The column's width in interface points.
    pub width: f32,
}

impl<'a> Scene<'a> {
    pub fn theme(&self) -> &'a Theme {
        self.ctx.theme
    }

    pub fn tr(&self) -> Translator {
        self.ctx.i18n
    }

    pub fn locale(&self) -> Locale {
        self.ctx.i18n.locale()
    }

    pub fn is_folded(&self, id: SectionId) -> bool {
        self.folded.iter().any(|folded| folded == id.as_str())
    }

    /// The column's own padding at both sides of a section (`--insp-pad`).
    pub const PAD: f32 = 14.0;

    /// A section's frame: its title, a fold that sends `Msg::Toggle`.
    pub fn section(&self, id: SectionId, title: Key) -> Section<'a, Message> {
        section::section(
            self.theme(),
            self.tr().t(title).into_owned(),
            self.is_folded(id),
            Message::Inspector(Msg::Toggle(id)),
        )
        .padding(Self::PAD)
    }

    /// The tooltip of a thing, under it.
    pub fn tip(
        &self,
        el: impl Into<Element<'a, Message>>,
        words: impl Into<String>,
    ) -> Element<'a, Message> {
        tip::bottom(self.theme(), el, words)
    }
}

// ---- Small parts ---------------------------------------------------------------------------

/// A text of a size and an ink.
pub(super) fn ink<'a>(content: impl text::IntoFragment<'a>, size: f32, color: Color) -> Text<'a> {
    text::Text::new(content)
        .size(size)
        .style(move |_: &iced::Theme| text::Style { color: Some(color) })
        .wrapping(text::Wrapping::None)
}

/// A line of the quiet colour that says there is nothing (`.insp-empty`).
pub(super) fn empty_note<'a>(
    t: &'a Theme,
    words: impl text::IntoFragment<'a>,
) -> Element<'a, Message> {
    text::Text::new(words)
        .size(font_size::CAPTION_LG)
        .style(style::text::subtle(t))
        .width(Length::Fill)
        .into()
}

/// A text action: the accent as text, and an arrow after it when it leaves
/// for another surface (`.insp-link`).
pub(super) fn link<'a>(
    t: &'a Theme,
    label: impl Into<String>,
    hint: impl Into<String>,
    arrow: bool,
    msg: Msg,
) -> Element<'a, Message> {
    let mut words = row![
        text::Text::new(label.into())
            .size(font_size::CAPTION)
            .font(fonts::ui_medium())
            .wrapping(text::Wrapping::None)
    ]
    .spacing(space::XS)
    .align_y(Alignment::Center);
    if arrow {
        words = words.push(icon(Icon::ArrowUpRight, 10.0));
    }
    let action = button(words)
        .padding(Padding::from([0.0, 2.0]))
        .style(style::button::link(t))
        .on_press(Message::Inspector(msg));
    tip::bottom(t, action, hint)
}

/// A value and, at its right end, an action (`.insp-kv__split`).
fn split<'a>(
    value: Element<'a, Message>,
    action: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut line = row![value].spacing(space::MD).align_y(Alignment::Center);
    if let Some(action) = action {
        line = line.push(action);
    }
    line.into()
}

/// An id or a name in the mono face, cut at its end when the cell is short
/// (`.insp-mono`).
fn mono_cut<'a>(content: impl Into<String>, color: Color) -> Element<'a, Message> {
    tail_text(content)
        .cut_end()
        .size(font_size::LABEL)
        .font(fonts::mono())
        .color(color)
        .into()
}

/// A path in a cell too narrow for it loses its beginning, not its end: the
/// last folders are the ones that tell two checkouts apart (`TailPath`).
pub(super) fn tail_path<'a>(t: &'a Theme, path: &str) -> Element<'a, Message> {
    tip::bottom(
        t,
        tail_text(path.to_string())
            .size(font_size::LABEL)
            .font(fonts::mono())
            .color(t.colors.fg),
        path.to_string(),
    )
}

/// The columns a pane's task can be in are `doing` and `review`: the glyph
/// beside the card's state says whose turn it is.
fn task_glyph(card: &TaskCard) -> GlyphState {
    if card.in_review() {
        GlyphState::Yours
    } else {
        GlyphState::Working
    }
}

// ---- Session and terminal -----------------------------------------------------------------

pub(super) fn session<'a>(
    s: &Scene<'a>,
    insight: &'a PaneInsight,
    folder: &'a str,
) -> Section<'a, Message> {
    let (t, tr) = (s.theme(), s.tr());
    let c = &t.colors;
    let agent = s.pane.agent.info();
    let ws = s.ws.id.clone();
    let mut rows: Vec<Element<'a, Message>> = Vec::new();

    if agent.resumable {
        let id: Element<'a, Message> = match &s.pane.session_id {
            Some(id) => s.tip(mono_cut(id.clone(), c.fg), id.clone()),
            None => ink(
                tr.t(Key::InspectorResumeNew),
                font_size::CAPTION_LG,
                c.fg_subtle,
            )
            .width(Length::Fill)
            .into(),
        };
        let change = link(
            t,
            tr.t(Key::InspectorResumeChange),
            tr.t(Key::InspectorResumeChangeHint),
            false,
            Msg::OpenSessions(ws.clone(), s.pane.id.clone()),
        );
        rows.push(kv(t, tr.t(Key::InspectorResume), split(id, Some(change))));
    }

    if let Some(since) = insight.session_since.map(sources::at_ms) {
        let words = format::since(since, s.now, s.locale()).text(tr);
        let shown = ink(words, font_size::CAPTION_LG, c.fg).width(Length::Shrink);
        rows.push(kv(
            t,
            tr.t(Key::InspectorSince),
            s.tip(shown, format::full_date(since, s.locale())),
        ));
    }

    if let Some(model) = insight.model.as_deref().filter(|model| !model.is_empty()) {
        rows.push(kv(
            t,
            tr.t(Key::InspectorModel),
            s.tip(mono_cut(model.to_string(), c.fg), model.to_string()),
        ));
    }

    // The MCP servers its Claude Code started with; only Claude Code has them.
    if s.pane.agent == AgentId::Claude
        && let Some(servers) = insight.mcp.as_ref().map(mcp_servers)
    {
        let value: Element<'a, Message> = match servers {
            Some(list) => s.tip(mono_cut(list.clone(), c.fg), list),
            None => ink(
                tr.t(Key::InspectorMcpInherit),
                font_size::CAPTION_LG,
                c.fg_subtle,
            )
            .width(Length::Fill)
            .into(),
        };
        let change = link(
            t,
            tr.t(Key::InspectorMcpChange),
            tr.t(Key::InspectorMcpChangeHint),
            false,
            Msg::OpenMcpServers(ws),
        );
        rows.push(kv(t, tr.t(Key::InspectorMcp), split(value, Some(change))));
    }

    rows.push(kv(t, tr.t(Key::InspectorFolder), tail_path(t, folder)));
    s.section(SectionId::Session, Key::InspectorSectionSession)
        .body(stacked(5.0, rows))
}

/// The servers a custom launch named, joined; `None` when the pane inherits
/// Claude Code's own (`PaneMcp`).
fn mcp_servers(mcp: &PaneMcp) -> Option<String> {
    match &mcp.mode {
        PaneMcpMode::Inherit => None,
        PaneMcpMode::Custom { servers } => Some(servers.join(", ")),
    }
}

pub(super) fn terminal<'a>(
    s: &Scene<'a>,
    shell: Option<&str>,
    folder: &'a str,
) -> Section<'a, Message> {
    let (t, tr) = (s.theme(), s.tr());
    let mut rows = Vec::new();
    if let Some(shell) = shell {
        rows.push(kv(
            t,
            tr.t(Key::InspectorShell),
            ink(shell.to_string(), font_size::CAPTION_LG, t.colors.fg).width(Length::Fill),
        ));
    }
    rows.push(kv(t, tr.t(Key::InspectorFolder), tail_path(t, folder)));
    s.section(SectionId::Terminal, Key::InspectorSectionTerminal)
        .body(stacked(5.0, rows))
}

// ---- Context and cost ---------------------------------------------------------------------

pub(super) fn context<'a>(
    s: &Scene<'a>,
    insight: &'a PaneInsight,
    agent_label: &'a str,
    limits: Option<&'a [RateLimit]>,
) -> Section<'a, Message> {
    let (t, tr) = (s.theme(), s.tr());
    let c = &t.colors;
    let show_plan = s.pane.agent == AgentId::Claude;
    let five_hour = limits
        .filter(|_| show_plan)
        .and_then(|limits| format::plan_usage(limits, "five_hour"));
    let week = limits
        .filter(|_| show_plan)
        .and_then(|limits| format::plan_usage(limits, "seven_day"));
    let has_plan = five_hour.is_some() || week.is_some();
    // Cost and plan get their tiles once either is known; an agent that
    // reports neither gets no row of dashes.
    let stats = insight.cost_usd.is_some() || has_plan;

    let mut body: Vec<Element<'a, Message>> = Vec::new();
    body.push(match &insight.context {
        Some(gauge) => gauge_row(s, gauge),
        None => empty_note(t, tr.t(Key::InspectorContextEmpty)),
    });

    if stats {
        let cost = cost_tile(s, insight, agent_label);
        let mut tiles: Vec<Element<'a, Message>> = vec![cost];
        if show_plan {
            tiles.push(plan_tile(s, five_hour, week));
        }
        // Side by side while both fit (two tiles of 118 and the gap), stacked
        // in a narrow column.
        let room = s.width - 2.0 * Scene::PAD;
        body.push(if tiles.len() == 2 && room >= 2.0 * 118.0 + 8.0 {
            let plan = tiles.pop().unwrap_or_else(|| filler::vertical().into());
            let cost = tiles.pop().unwrap_or_else(|| filler::vertical().into());
            row![
                container(cost).width(Length::FillPortion(1)),
                container(plan).width(Length::FillPortion(1)),
            ]
            .spacing(space::MD)
            .into()
        } else {
            column(tiles).spacing(space::MD).into()
        });
    }

    s.section(SectionId::Context, Key::InspectorSectionContext)
        .body(column(body).spacing(10).width(Length::Fill))
}

/// The ring and, beside it, how much of the window is used and what that
/// means (`.insp-ctx`).
fn gauge_row<'a>(s: &Scene<'a>, gauge: &'a ContextGauge) -> Element<'a, Message> {
    let (t, tr) = (s.theme(), s.tr());
    let c = &t.colors;
    let pct = gauge.pct.round() as i64;
    let used = format::spaced_unit(&panes_logic::format_tokens(gauge.used_tokens), s.locale());
    let window = format::spaced_unit(&panes_logic::format_tokens(gauge.window), s.locale());
    let hint = if gauge.pct >= 90.0 {
        Key::InspectorContextHintSoon
    } else {
        Key::InspectorContextHint
    };
    let info = column![
        text::Text::new(
            tr.t_with(Key::InspectorContextUsed, &args!("pct" => pct))
                .into_owned()
        )
        .size(font_size::UI)
        .font(fonts::ui_medium())
        .style(style::text::primary(t)),
        text::Text::new(
            tr.t_with(
                Key::InspectorContextTokens,
                &args!("used" => used.as_str(), "window" => window.as_str()),
            )
            .into_owned()
        )
        .size(font_size::CAPTION)
        .style(style::text::muted(t)),
        text::Text::new(tr.t(hint).into_owned())
            .size(font_size::CAPTION)
            .style(move |_: &iced::Theme| text::Style {
                color: Some(c.fg_subtle),
            })
            .width(Length::Fill),
    ]
    .spacing(3)
    .width(Length::Fill);
    row![widgets::ring(t, gauge.pct), info]
        .spacing(14)
        .align_y(Alignment::Center)
        .into()
}

fn tile<'a>(s: &Scene<'a>, label: String, body: Element<'a, Message>) -> Element<'a, Message> {
    let t = s.theme();
    container(column![ink(label, font_size::LABEL, t.colors.fg_subtle), body].spacing(1))
        .padding(Padding::from([7.0, 9.0]))
        .width(Length::Fill)
        .style(widgets::tile(t, TILE_RADIUS))
        .into()
}

/// The "—" a tile shows when the agent does not report the figure, with the
/// reason as its tooltip (`.insp-stat__value--none`).
fn unknown<'a>(s: &Scene<'a>, why: String) -> Element<'a, Message> {
    let t = s.theme();
    s.tip(
        ink("—", font_size::BODY, t.colors.fg_subtle).font(fonts::ui_medium()),
        why,
    )
}

fn cost_tile<'a>(
    s: &Scene<'a>,
    insight: &'a PaneInsight,
    agent_label: &str,
) -> Element<'a, Message> {
    let (t, tr) = (s.theme(), s.tr());
    let value: Element<'a, Message> = match insight.cost_usd {
        Some(usd) => {
            let mut line = row![
                ink(
                    format::format_cost(usd, s.locale()),
                    font_size::BODY,
                    t.colors.fg
                )
                .font(fonts::ui_semibold())
            ]
            .spacing(space::SM)
            .align_y(Alignment::End);
            if let Some(since) = insight.session_since.map(sources::at_ms) {
                let over = board::format::elapsed_label(s.now - since, tr);
                line = line.push(ink(
                    tr.t_with(Key::InspectorCostOver, &args!("duration" => over.as_str()))
                        .into_owned(),
                    font_size::LABEL,
                    t.colors.fg_subtle,
                ));
            }
            line.into()
        }
        None => unknown(
            s,
            tr.t_with(Key::InspectorCostUnknown, &args!("agent" => agent_label))
                .into_owned(),
        ),
    };
    tile(s, tr.t(Key::InspectorCost).into_owned(), value)
}

fn plan_tile<'a>(
    s: &Scene<'a>,
    five_hour: Option<RateLimit>,
    week: Option<RateLimit>,
) -> Element<'a, Message> {
    let (t, tr) = (s.theme(), s.tr());
    let body: Element<'a, Message> = if five_hour.is_some() || week.is_some() {
        let meters = [
            (
                Key::InspectorPlanFiveHour,
                Key::InspectorPlanWindowFiveHour,
                five_hour,
            ),
            (Key::InspectorPlanWeek, Key::InspectorPlanWindowWeek, week),
        ]
        .into_iter()
        .filter_map(|(label, window, limit)| Some(plan_meter(s, label, window, limit?)));
        column(meters).spacing(2).into()
    } else {
        unknown(s, tr.t(Key::InspectorPlanUnknown).into_owned())
    };
    tile(s, tr.t(Key::InspectorPlan).into_owned(), body)
}

/// One plan window: its name, a bar and the percentage used (`.insp-meter`),
/// and when it resets in the tooltip.
fn plan_meter<'a>(
    s: &Scene<'a>,
    label: Key,
    window: Key,
    limit: RateLimit,
) -> Element<'a, Message> {
    let (t, tr) = (s.theme(), s.tr());
    let c = &t.colors;
    let pct = limit.percent_used;
    let window = tr.t(window);
    let hint = match format::resets_at_ms(&limit) {
        Some(at) => {
            let reset = format::reset_when(at, s.now, s.locale()).text(tr);
            tr.t_with(
                Key::InspectorPlanHintResets,
                &args!("window" => &*window, "pct" => pct as i64, "reset" => reset.as_str()),
            )
            .into_owned()
        }
        None => tr
            .t_with(
                Key::InspectorPlanHint,
                &args!("window" => &*window, "pct" => pct as i64),
            )
            .into_owned(),
    };
    let line = row![
        container(ink(tr.t(label).into_owned(), font_size::LABEL, c.fg_subtle)).width(20.0),
        widgets::meter_bar(t, pct),
        container(
            ink(
                format::format_percent(pct, s.locale()),
                font_size::LABEL,
                c.fg_muted
            )
            .font(fonts::mono())
            .align_x(iced::alignment::Horizontal::Right)
        )
        .width(Length::Shrink)
        .align_x(iced::alignment::Horizontal::Right),
    ]
    .spacing(space::SM)
    .align_y(Alignment::Center)
    .height(15.0);
    s.tip(line, hint)
}

// ---- The board task -----------------------------------------------------------------------

/// The priority as a signal: how many of the three bars are lit, and the
/// word (`BoardPriority`).
///
/// The board draws the same signal on its cards, privately; the level and the
/// word come from `board::format`.
fn priority<'a>(s: &Scene<'a>, level_of: u8) -> Element<'a, Message> {
    let (t, tr) = (s.theme(), s.tr());
    let c = &t.colors;
    let level = board::format::priority_level(f64::from(level_of)).bars();
    let word = tr.t(board::format::priority_key(f64::from(level_of)));
    let bar = |lit: bool, height: f32| -> Element<'a, Message> {
        let color = if lit { c.fg_muted } else { c.line_strong };
        container(filler::horizontal())
            .width(2.5)
            .height(height)
            .style(move |_: &iced::Theme| container::Style {
                background: Some(Background::Color(color)),
                border: Border {
                    radius: 1.0.into(),
                    ..Border::default()
                },
                ..container::Style::default()
            })
            .into()
    };
    let bars = row![bar(true, 4.0), bar(level >= 2, 6.5), bar(level >= 3, 9.0)]
        .spacing(1.5)
        .align_y(Alignment::End);
    let ink_of = if level == 3 { c.fg_muted } else { c.fg_subtle };
    let line = row![
        bars,
        ink(word.to_string(), font_size::LABEL, ink_of).font(fonts::ui_medium())
    ]
    .spacing(5)
    .align_y(Alignment::Center);
    s.tip(
        line,
        tr.t_with(
            Key::BoardPriorityTitle,
            &args!("n" => i64::from(level_of), "level" => &*word),
        )
        .into_owned(),
    )
}

pub(super) fn task<'a>(s: &Scene<'a>, card: Option<&TaskCard>) -> Section<'a, Message> {
    let (t, tr) = (s.theme(), s.tr());
    let c = &t.colors;
    let open = Msg::OpenBoard;
    let mut section = s.section(SectionId::Task, Key::InspectorSectionTask);
    let Some(card) = card else {
        return section.body(empty_note(t, tr.t(Key::InspectorTaskNone).into_owned()));
    };
    section = section.action(link(
        t,
        tr.t(Key::InspectorTaskOpen),
        tr.t(Key::InspectorTaskOpenHint),
        true,
        open.clone(),
    ));

    let id = format!("#{}", board::format::short_task_id(&card.id));
    let top = row![
        s.tip(
            ink(id, font_size::LABEL, c.fg_subtle).font(fonts::mono()),
            tr.t_with(Key::InspectorTaskId, &args!("id" => card.id.as_str()))
                .into_owned()
        ),
        filler::horizontal(),
        priority(s, card.priority),
    ]
    .spacing(space::MD)
    .align_y(Alignment::Center);

    let title = container(
        text::Text::new(card.title.clone())
            .size(font_size::UI_LG)
            .font(fonts::ui_medium())
            .style(style::text::primary(t)),
    )
    // Two lines, then cut.
    .max_height(2.0 * font_size::UI_LG * 1.35 + 1.0)
    .clip(true);

    let doing = card.doing_since.filter(|_| !card.in_review());
    let state: Element<'a, Message> = if card.in_review() {
        ink(
            tr.t(Key::InspectorTaskReview).into_owned(),
            font_size::CAPTION,
            c.fg_muted,
        )
        .into()
    } else if let Some(since) = doing {
        let elapsed = board::format::elapsed_label(s.now - since, tr);
        let words = tr
            .t_with(
                Key::InspectorTaskDoing,
                &args!("duration" => elapsed.as_str()),
            )
            .into_owned();
        let date = format::full_date(since, s.locale());
        s.tip(
            ink(words, font_size::CAPTION, c.fg_muted),
            tr.t_with(Key::BoardDoingSince, &args!("date" => date.as_str()))
                .into_owned(),
        )
    } else {
        ink(
            tr.t(Key::InspectorTaskDoingNow).into_owned(),
            font_size::CAPTION,
            c.fg_muted,
        )
        .into()
    };
    let mut foot = row![glyph::state(t, task_glyph(card), 7.0), state,]
        .spacing(space::SM)
        .align_y(Alignment::Center);
    if card.comments > 0 {
        foot = foot.push(filler::horizontal());
        foot = foot.push(
            s.tip(
                row![
                    icon(Icon::Bubble, 11.0).color(c.fg_subtle),
                    ink(card.comments.to_string(), font_size::LABEL, c.fg_subtle)
                        .font(fonts::mono()),
                ]
                .spacing(space::XS)
                .align_y(Alignment::Center),
                tr.tn(Key::InspectorTaskComments, card.comments, &[])
                    .into_owned(),
            ),
        );
    }

    let card_body = container(column![top, title, foot].spacing(5))
        .padding(Padding::from([9.0, 10.0]))
        .width(Length::Fill)
        .style(widgets::tile(t, CARD_RADIUS));
    // The whole card answers a click; the "Open" link beside the title is
    // the same action for people who aim at text.
    section.body(
        mouse_area(card_body)
            .on_press(Message::Inspector(open))
            .interaction(mouse::Interaction::Pointer),
    )
}

// ---- The prompt queue ---------------------------------------------------------------------

pub(super) fn queue<'a>(s: &Scene<'a>, queue: &'a [Queued]) -> Section<'a, Message> {
    let (t, tr) = (s.theme(), s.tr());
    let c = &t.colors;
    let mut section = s.section(SectionId::Queue, Key::InspectorSectionQueue);
    if !queue.is_empty() {
        section = section.meta(queue.len().to_string());
    }
    if queue.is_empty() {
        return section.body(empty_note(t, tr.t(Key::InspectorQueueEmpty).into_owned()));
    }
    let lines = queue.iter().enumerate().map(|(at, entry)| {
        let mut line = row![
            container(ink((at + 1).to_string(), font_size::MICRO, c.fg_subtle).font(fonts::mono()))
                .width(Length::Shrink),
            container(
                tail_text(entry.text.clone())
                    .cut_end()
                    .size(font_size::CAPTION_LG)
                    .color(c.fg)
            )
            .width(Length::Fill),
        ]
        .spacing(space::MD)
        .align_y(Alignment::Center);
        if let Some(by) = panes_logic::queued_by(entry.from.as_deref()) {
            line = line.push(
                s.tip(
                    ink(by.clone(), font_size::LABEL, c.fg_subtle),
                    tr.t_with(Key::InspectorQueueFrom, &args!("name" => by.as_str()))
                        .into_owned(),
                ),
            );
        }
        line = line.push(
            icon_button(t, Icon::Close)
                .small()
                .side(20.0)
                .icon_size(10.0)
                .tip(tr.t(Key::InspectorQueueCancel).into_owned())
                .on_press(Message::Inspector(Msg::CancelQueued(
                    s.pane.id.clone(),
                    entry.id.clone(),
                ))),
        );
        let item: Element<'a, Message> = container(line)
            .padding(Padding {
                top: 2.0,
                right: 3.0,
                bottom: 2.0,
                left: 8.0,
            })
            .width(Length::Fill)
            .style(widgets::tile(t, TILE_RADIUS))
            .into();
        s.tip(item, entry.text.clone())
    });
    section.body(column(lines).spacing(4).width(Length::Fill))
}

// ---- Recent activity ----------------------------------------------------------------------

/// How many events the section keeps: a glance back, not a log.
const ACTIVITY_SHOWN: usize = 8;

/// The dot's colour for an event kind (`.insp-event__dot--*`).
fn dot_color(t: &Theme, kind: &str) -> Option<Color> {
    let c = &t.colors;
    match kind {
        "turn-start" => Some(c.state_working_fill),
        "permission" => Some(c.state_waiting_fill),
        "turn-end" => Some(c.state_yours_fill),
        "dropped" => Some(c.danger),
        // The exit is a ring, not a dot: the caller draws it.
        "exit" => None,
        _ => Some(c.fg_subtle),
    }
}

fn dot<'a>(t: &Theme, kind: &str) -> Element<'a, Message> {
    let c = &t.colors;
    let fill = dot_color(t, kind);
    let ring = c.state_idle_fill;
    container(filler::horizontal())
        .width(5.0)
        .height(5.0)
        .style(move |_: &iced::Theme| container::Style {
            background: fill.map(Background::Color),
            border: Border {
                radius: radius::FULL.into(),
                width: if fill.is_none() { 1.0 } else { 0.0 },
                color: ring,
            },
            ..container::Style::default()
        })
        .into()
}

pub(super) fn activity<'a>(
    s: &Scene<'a>,
    events: &'a [PaneEvent],
    nothing: String,
) -> Section<'a, Message> {
    let (t, tr) = (s.theme(), s.tr());
    let c = &t.colors;
    let section = s.section(SectionId::Activity, Key::InspectorSectionActivity);
    if events.is_empty() {
        return section.body(empty_note(t, nothing));
    }
    let lines = events.iter().take(ACTIVITY_SHOWN).map(|event| {
        let at = sources::at_ms(event.at);
        let time = s.tip(
            container(
                ink(format::clock_time(at), font_size::LABEL, c.fg_subtle).font(fonts::mono()),
            )
            .width(34.0)
            .align_x(iced::alignment::Horizontal::Right),
            format::full_date(at, s.locale()),
        );
        // The dot sits on the first line, whatever the text wraps to.
        let marker = container(dot(t, event.detail.kind())).padding(Padding {
            top: 5.0,
            ..Padding::ZERO
        });
        // At most two lines, then cut.
        let words = container(
            text::Text::new(panes_logic::pane_event_text(&event.detail, tr))
                .size(font_size::CAPTION_LG)
                .style(style::text::muted(t))
                .width(Length::Fill),
        )
        .max_height(2.0 * font_size::CAPTION_LG * 1.4 + 1.0)
        .clip(true)
        .width(Length::Fill);
        row![time, marker, words]
            .spacing(space::MD)
            .align_y(Alignment::Start)
            .into()
    });
    section.body(column(lines).spacing(2).width(Length::Fill))
}

// ---- The clock ----------------------------------------------------------------------------

/// Whether something on screen counts time, so that the half-minute clock
/// is worth running: a start time or a cost "over" it, or a task in `doing`,
/// in a section that is not folded.
pub(super) fn counts_time(m: &Model) -> bool {
    if !super::shown(m) {
        return false;
    }
    let Some(ws) = m.active_workspace() else {
        return false;
    };
    let Some((pane, _)) = sources::focused_pane(m, ws) else {
        return false;
    };
    let folded = |id: SectionId| {
        m.settings()
            .inspector_collapsed
            .iter()
            .any(|folded| folded == id.as_str())
    };
    let insight = sources::insight(m, &pane.id);
    let agent = pane.agent != AgentId::Shell;
    let session = agent && !folded(SectionId::Session) && insight.session_since.is_some();
    let cost = agent
        && !folded(SectionId::Context)
        && insight.cost_usd.is_some()
        && insight.session_since.is_some();
    let doing = !folded(SectionId::Task)
        && sources::task_for_pane(m, ws, &pane.id)
            .is_some_and(|card| !card.in_review() && card.doing_since.is_some());
    session || cost || doing
}

/// The state word and colour of a header, from the pane's activity.
pub(super) fn state_word(tr: Translator, state: AgentState) -> String {
    tr.t(match state {
        AgentState::Working => Key::PaneStateWorking,
        AgentState::Waiting => Key::PaneStateWaiting,
        AgentState::Yours => Key::PaneStateYours,
        AgentState::Idle => Key::PaneStateIdle,
    })
    .into_owned()
}
