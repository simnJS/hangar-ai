//! The button and the panel (`NeedsYou.tsx`).

use std::time::Instant;

use iced::widget::{Space, button, column, container, row, scrollable, text};
use iced::{Alignment, Background, Element, Length, Padding};

use hangar_i18n::{Key, args};
use hangar_theme::tokens::{font_size, radius, space};
use hangar_theme::{Theme, fonts, style};
use hangar_ui::{Icon, glyph, icon, layer, tip};

use super::model::{KINDS, Kind, NeedsYouItem, latest_note};
use super::{LandRequest, Msg, State};
use crate::commands::CommandId;
use crate::ctx::Ctx;
use crate::message::Message;
use crate::navigator::{reads, skin};

/// The panel's width (`.tmenu.needs`).
const PANEL_WIDTH: f32 = 440.0;
const PANEL_MAX_HEIGHT: f32 = 520.0;

/// The toolbar's width under which the button keeps its flag and its count
/// and loses its words (`@container toolbar (width < 92em)`).
const LABEL_FROM: f32 = 92.0 * 12.0;

fn msg(msg: Msg) -> Message {
    Message::NeedsYou(msg)
}

/// The button, and the panel that hangs off it while it is open. Nothing at all
/// while the list is empty.
pub fn entry<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    let items = st.items();
    if items.is_empty() {
        return Space::new().into();
    }
    let t = ctx.theme;
    let tr = ctx.i18n;
    let open = ctx.model.ui.needs_you_open;
    let label = tr.t_with(Key::NeedsYouButton, &args!("n" => items.len()));

    // The toolbar is the window less the navigator when that is open.
    let navigator = if ctx.model.settings().navigator_open {
        hangar_theme::tokens::size::SIDEBAR_WIDTH + 1.0
    } else {
        0.0
    };
    let roomy = ctx.metrics.body_width - navigator >= LABEL_FROM;

    let mut face = row![icon(Icon::Flag, 14.0).color(t.colors.state_waiting)]
        .spacing(space::SM)
        .align_y(Alignment::Center);
    if roomy {
        face = face.push(
            text(tr.t(Key::NeedsYouTitle).into_owned())
                .size(font_size::UI)
                .font(fonts::ui_medium())
                .color(t.colors.fg),
        );
    }
    face = face.push(
        text(items.len().to_string())
            .size(font_size::CAPTION)
            .font(fonts::mono())
            .color(t.colors.state_waiting),
    );

    let pressed = iced::widget::button(container(face).center_y(Length::Fill))
        .padding(Padding {
            top: 0.0,
            right: 8.0,
            bottom: 0.0,
            left: 7.0,
        })
        .height(26)
        .on_press(msg(Msg::Toggle))
        .style(move |_: &iced::Theme, status| {
            let lit = open || matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(Background::Color(if lit {
                    t.colors.line_strong
                } else {
                    t.colors.line
                })),
                text_color: t.colors.state_waiting,
                border: skin::border(
                    if lit {
                        t.colors.state_waiting_fill
                    } else {
                        t.colors.line
                    },
                    radius::MD,
                ),
                ..button::Style::default()
            }
        });
    let binding = reads::binding(&ctx, CommandId::ViewNeedsYou);
    let anchor = container(tip::keys(t, pressed, label.into_owned(), &binding)).padding(Padding {
        right: space::SM,
        ..Padding::ZERO
    });

    layer::dropdown(anchor, open.then(|| panel(st, ctx, items)))
        .align_end()
        .tab_inside()
        .on_dismiss(msg(Msg::Close))
        .into()
}

/// The list: a head with the total, then the groups in `KINDS` order.
fn panel<'a>(st: &'a State, ctx: Ctx<'a>, items: &[NeedsYouItem]) -> Element<'a, Message> {
    let t = ctx.theme;
    let tr = ctx.i18n;
    let head = container(
        row![
            text(tr.t(Key::NeedsYouTitle).into_owned())
                .size(font_size::BODY)
                .font(fonts::ui_semibold())
                .width(Length::Fill),
            text(items.len().to_string())
                .size(font_size::CAPTION)
                .font(fonts::mono())
                .color(t.colors.state_waiting),
        ]
        .spacing(space::MD)
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: 10.0,
        right: 14.0,
        bottom: 8.0,
        left: 14.0,
    })
    .width(Length::Fill);

    let mut body = column![].spacing(4);
    let mut first_group = true;
    for kind in KINDS {
        let group: Vec<&NeedsYouItem> = items.iter().filter(|item| item.kind() == kind).collect();
        if group.is_empty() {
            continue;
        }
        if !first_group {
            body = body.push(rule(t));
        }
        first_group = false;
        let heading = match kind {
            Kind::Waiting => Key::NeedsYouWaiting,
            Kind::Question => Key::NeedsYouQuestions,
            Kind::Land => Key::NeedsYouLands,
        };
        body = body.push(
            row![
                text(tr.t(heading).to_uppercase())
                    .size(font_size::LABEL)
                    .font(fonts::ui_semibold())
                    .color(t.colors.fg_secondary),
                text(group.len().to_string())
                    .size(font_size::LABEL)
                    .font(fonts::mono())
                    .color(t.colors.fg_secondary),
            ]
            .spacing(space::SM)
            .padding([6.0, 8.0]),
        );
        for item in group {
            body = body.push(item_row(st, ctx, item));
        }
    }

    let list = scrollable(container(body).padding(Padding {
        top: 4.0,
        right: 6.0,
        bottom: 8.0,
        left: 6.0,
    }))
    .style(style::scrollable::default(t));

    container(column![head, rule(t), list])
        .width(PANEL_WIDTH)
        .max_height(PANEL_MAX_HEIGHT)
        .style(style::container::popover(t))
        .into()
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

/// One row: what waits and where, and the thing to do about it.
fn item_row<'a>(st: &'a State, ctx: Ctx<'a>, item: &NeedsYouItem) -> Element<'a, Message> {
    let t = ctx.theme;
    let tr = ctx.i18n;
    let base = item.base();
    let board = || -> Element<'a, Message> {
        tip::bottom(
            t,
            hangar_ui::button::icon_button(t, Icon::Board)
                .small()
                .icon_size(13.0)
                .on_press(msg(Msg::OpenBoard(base.workspace_id.clone()))),
            tr.t(Key::NeedsYouBoard).into_owned(),
        )
    };
    let what = |words: &str| {
        text(words.to_string())
            .size(font_size::BODY)
            .font(fonts::ui_medium())
            .color(t.colors.fg)
            .wrapping(text::Wrapping::None)
    };
    let meta = |words: String| {
        text(words)
            .size(font_size::CAPTION)
            .color(t.colors.fg_secondary)
            .wrapping(text::Wrapping::None)
    };

    let (mark, text_col, actions): (
        Element<'a, Message>,
        Element<'a, Message>,
        Element<'a, Message>,
    ) = match item {
        NeedsYouItem::Waiting {
            base,
            pane_id,
            pane_name,
        } => (
            glyph::state(
                t,
                reads::activity_glyph(crate::state::AgentActivity::Waiting),
                7.0,
            )
            .into(),
            column![
                what(pane_name),
                meta(format!(
                    "{} · {}",
                    base.workspace_name,
                    tr.t(Key::NeedsYouWaitingDetail)
                )),
            ]
            .spacing(1)
            .into(),
            tip::bottom(
                t,
                hangar_ui::button::secondary(t, tr.t(Key::NeedsYouGo).into_owned())
                    .small()
                    .on_press(msg(Msg::GoTo {
                        ws: base.workspace_id.clone(),
                        pane: pane_id.clone(),
                    })),
                tr.t_with(Key::NeedsYouGoTo, &args!("name" => pane_name.as_str()))
                    .into_owned(),
            ),
        ),
        NeedsYouItem::Question {
            base,
            task,
            orchestrator_id,
            ..
        } => {
            let mut lines = column![
                what(&task.title),
                meta(match &task.assignee {
                    Some(who) => format!("{} · {who}", base.workspace_name),
                    None => base.workspace_name.clone(),
                }),
            ]
            .spacing(1);
            if let Some(note) = latest_note(task) {
                lines = lines.push(note_line(t, note));
            }
            // Answered where the orchestrator is, when there is one.
            let orchestrator = orchestrator_id.as_ref().and_then(|id| {
                let ws = ctx.model.workspace(&base.workspace_id)?;
                let pane = ws.panes.iter().find(|pane| &pane.id == id)?;
                Some((id.clone(), pane.name.clone()))
            });
            let mut actions = row![].spacing(2).align_y(Alignment::Center);
            if let Some((id, name)) = orchestrator {
                actions = actions.push(tip::bottom(
                    t,
                    hangar_ui::button::secondary(t, tr.t(Key::NeedsYouAnswer).into_owned())
                        .small()
                        .on_press(msg(Msg::GoTo {
                            ws: base.workspace_id.clone(),
                            pane: id,
                        })),
                    tr.t_with(Key::NeedsYouAnswerIn, &args!("name" => name))
                        .into_owned(),
                ));
            }
            actions = actions.push(board());
            (
                icon(Icon::Flag, 13.0).color(t.colors.state_waiting).into(),
                lines.into(),
                actions.into(),
            )
        }
        NeedsYouItem::Land { base, task, .. } => {
            let mut lines = column![
                what(&task.title),
                meta(match &task.assignee {
                    Some(who) => format!("{} · {who}", base.workspace_name),
                    None => base.workspace_name.clone(),
                }),
            ]
            .spacing(1);
            if let Some(note) = latest_note(task) {
                lines = lines.push(note_line(t, note));
            }
            if let Some(error) = st.failure(&base.key) {
                lines = lines.push(
                    row![
                        icon(Icon::Alert, 11.0).color(t.colors.danger),
                        text(
                            tr.t_with(Key::NeedsYouLandFailed, &args!("error" => error))
                                .into_owned()
                        )
                        .size(font_size::CAPTION)
                        .color(t.colors.danger),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Start),
                );
            }
            let landing = st.is_landing(ctx.model, base, &task.id, Instant::now());
            let land = hangar_ui::button::primary(
                t,
                tr.t(if landing {
                    Key::NeedsYouLanding
                } else {
                    Key::NeedsYouLand
                })
                .into_owned(),
            )
            .icon(Icon::Merge)
            .small()
            .on_press_maybe((!landing).then(|| {
                msg(Msg::Land(LandRequest {
                    key: base.key.clone(),
                    ws: base.workspace_id.clone(),
                    task: task.id.clone(),
                }))
            }))
            .tip(tr.t(Key::NeedsYouLandHint).into_owned());
            (
                icon(Icon::Merge, 13.0).color(t.colors.state_working).into(),
                lines.into(),
                row![land, board()]
                    .spacing(2)
                    .align_y(Alignment::Center)
                    .into(),
            )
        }
    };

    row![
        container(mark).width(18).align_x(Alignment::Center),
        container(text_col).width(Length::Fill).clip(true),
        actions,
    ]
    .spacing(space::MD)
    .align_y(Alignment::Start)
    .padding(Padding {
        top: 5.0,
        right: 6.0,
        bottom: 5.0,
        left: 8.0,
    })
    .into()
}

/// The question, or the review's verdict: two lines of it at most.
fn note_line<'a>(t: &'a Theme, note: String) -> Element<'a, Message> {
    const LINE: f32 = font_size::CAPTION_LG * 1.4;
    container(
        text(note)
            .size(font_size::CAPTION_LG)
            .color(t.colors.fg_secondary),
    )
    .max_height(LINE * 2.0)
    .clip(true)
    .padding(Padding {
        top: 2.0,
        ..Padding::ZERO
    })
    .into()
}
