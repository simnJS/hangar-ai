//! The conversation, drawn: a header with the orchestrator and what it does,
//! the messages newest at the bottom (the scroll anchored there), the card of
//! the question or permission it waits on, and the input.

use std::hash::{DefaultHasher, Hash, Hasher};

use hangar_core::chat::{Answer, Body, CallResult, CallState, ChatItem, Question, Sender};
use hangar_i18n::{Key, args};
use hangar_theme::{AgentState, Theme, fonts, style, tokens};
use hangar_ui::{Icon, button as ui_button, empty, icon};
use iced::keyboard::{self, key::Named};
use iced::widget::text_editor::{Binding, KeyPress, Status};
use iced::widget::{
    Space, button, column, container, row, rule, scrollable, text, text_editor, text_input,
};
use iced::{Alignment, Element, Length, Padding};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::mobile::contract::{ChatPending, ChatReason};
use crate::orchestration_ui::actions::can_create_orchestrator;
use crate::state::roles::orchestrator_of;
use crate::state::{AgentActivity, Pane, Workspace};

use super::calls::{self, Piece};
use super::settled::settled;
use super::thread::{Outgoing, Sent};
use super::{Blocked, Chat, Msg, State, blocked, msg, rich, titles};

/// The widest the column of messages gets: lines longer than this read badly.
const COLUMN_WIDTH: f32 = 760.0;
/// How wide a bubble of the person's gets, at most.
const BUBBLE_WIDTH: f32 = 560.0;
/// The lines of a message of Hangar's shown before « Show all ».
const HANGAR_LINES: usize = 3;
/// The lines of a call's detail or result shown expanded, at most.
const DETAIL_LINES: usize = 40;

/// The input's widget id.
pub fn input_id() -> iced::widget::Id {
    iced::widget::Id::new("chat-view-conversation-input")
}

pub fn view<'a>(st: &'a State, ctx: Ctx<'a>, ws: &'a Workspace) -> Element<'a, Message> {
    let t = ctx.theme;
    let Some(pane) = orchestrator_of(ws) else {
        return no_orchestrator(ctx, ws);
    };
    let chat = st.chat(&ws.id).filter(|chat| chat.pane == pane.id);
    let Some(chat) = chat.filter(|chat| chat.loaded) else {
        return loading(t, ctx.t(Key::ChatConvLoading).into_owned());
    };
    let mut body = column![
        header(ctx, pane),
        rule::horizontal(tokens::BORDER_WIDTH).style(style::rule::sep(t)),
        messages(ctx, ws, pane, chat),
    ]
    .height(Length::Fill);
    if let Some(card) = card(ctx, ws, pane, chat) {
        body = body.push(card);
    }
    body = body.push(input(ctx, ws, pane, chat));
    container(body)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(style::container::plain(t))
        .into()
}

/// The column, centred, no wider than reads well.
fn centred<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(container(content).max_width(COLUMN_WIDTH))
        .width(Length::Fill)
        .center_x(Length::Fill)
        .into()
}

fn loading<'a>(t: &'a Theme, words: String) -> Element<'a, Message> {
    container(hangar_ui::text::hint(t, words))
        .center(Length::Fill)
        .style(style::container::plain(t))
        .into()
}

fn no_orchestrator<'a>(ctx: Ctx<'a>, ws: &'a Workspace) -> Element<'a, Message> {
    let t = ctx.theme;
    let state = empty::empty(t, Icon::Orchestrator, ctx.t(Key::ChatConvEmptyTitle))
        .description(ctx.t(Key::ChatConvEmptyBody));
    if can_create_orchestrator(ws) {
        state
            .action(
                ui_button::primary(t, ctx.t(Key::ChatConvEmptyStart))
                    .icon(Icon::Play)
                    .on_press(msg(Msg::Start(ws.id.clone()))),
            )
            .into()
    } else {
        state.description(ctx.t(Key::ChatConvEmptyCannot)).into()
    }
}

fn header<'a>(ctx: Ctx<'a>, pane: &'a Pane) -> Element<'a, Message> {
    let t = ctx.theme;
    let activity = ctx.model.live.panes_logic.activity_or_idle(&pane.id);
    let label = match activity {
        AgentActivity::Working => Key::PaneStateWorking,
        AgentActivity::Waiting => Key::PaneStateWaiting,
        AgentActivity::Yours => Key::PaneStateYoursShort,
        AgentActivity::Idle => Key::PaneStateIdle,
    };
    let state = AgentState::from(activity);
    container(
        row![
            icon(Icon::Orchestrator, 15.0).color(t.colors.fg_muted),
            text(pane.name.as_str())
                .size(tokens::font_size::UI_LG)
                .font(fonts::ui_semibold())
                .style(style::text::primary(t)),
            text(ctx.t(label))
                .size(tokens::font_size::CAPTION)
                .style(style::text::state(t, state)),
        ]
        .spacing(tokens::space::MD)
        .align_y(Alignment::Center),
    )
    .padding([tokens::space::MD, tokens::space::XXL])
    .width(Length::Fill)
    .into()
}

fn messages<'a>(
    ctx: Ctx<'a>,
    ws: &'a Workspace,
    pane: &'a Pane,
    chat: &'a Chat,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let mut list = column![].spacing(tokens::space::LG).width(Length::Fill);
    if chat.reading_older {
        list = list.push(centre_line(hangar_ui::text::hint(
            t,
            ctx.t(Key::ChatConvOlderLoading),
        )));
    } else if chat.thread.older {
        list = list.push(centre_line(
            ui_button::ghost(t, ctx.t(Key::ChatConvOlder))
                .small()
                .on_press(msg(Msg::LoadOlder(ws.id.clone()))),
        ));
    }
    let tasks = ctx.model.live.board.tasks(&ws.cwd);
    let task_title = move |id: &str| {
        tasks?
            .iter()
            .find(|task| task.id == id)
            .map(|task| task.title.clone())
    };
    let pending_question = match &chat.thread.pending {
        Some(ChatPending::Question { tool_use_id, .. }) => Some(tool_use_id.as_str()),
        _ => None,
    };
    // The question the pane waits on is the card below, not an item.
    let items = chat.thread.items.iter().filter(|item| {
        !matches!(&item.body, Body::Question { tool_use_id, .. }
            if Some(tool_use_id.as_str()) == pending_question)
    });
    for piece in calls::pieces(items) {
        list = list.push(match piece {
            Piece::One(item) => item_view(ctx, ws, chat, item, &task_title),
            Piece::Run(run) => run_view(ctx, ws, chat, &run, &task_title),
        });
    }
    for out in &chat.thread.outgoing {
        list = list.push(outgoing(ctx, pane, out));
    }
    if chat.thread.items.is_empty()
        && chat.thread.outgoing.is_empty()
        && let Some(reason) = chat.thread.reason
    {
        let key = match reason {
            ChatReason::NotClaude => Key::ChatConvReasonNotClaude,
            ChatReason::NoSession => Key::ChatConvReasonNoSession,
            ChatReason::NoTranscript => Key::ChatConvReasonNoTranscript,
        };
        list = list.push(notice(
            t,
            ctx.i18n
                .t_with(key, &args!("name" => pane.name.as_str()))
                .into_owned(),
        ));
    }
    if let Some(error) = &chat.error {
        list = list.push(
            hangar_ui::text::danger(
                t,
                ctx.i18n
                    .t_with(Key::ChatConvReadFailed, &args!("error" => error.as_str()))
                    .into_owned(),
            )
            .size(tokens::font_size::CAPTION),
        );
    }
    let ws_id = ws.id.clone();
    let content = centred(container(list).padding(Padding {
        top: tokens::space::XXL,
        right: tokens::space::XXL,
        bottom: tokens::space::XXL,
        left: tokens::space::XXL,
    }));
    scrollable(settled(layout_key(ctx, pane, chat, tasks), content))
        .anchor_bottom()
        .on_scroll(move |viewport| msg(Msg::Scrolled(ws_id.clone(), viewport)))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(style::scrollable::default(t))
        .into()
}

/// What the messages' words come from: the chat's own state, and the
/// names, titles and language they are written with, and the theme (a
/// Markdown span's colour is set as it is laid out). Laid out again only
/// when it changes.
fn layout_key(
    ctx: Ctx<'_>,
    pane: &Pane,
    chat: &Chat,
    tasks: Option<&[crate::board::model::Task]>,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    chat.revision.hash(&mut hasher);
    ctx.model.locale().hash(&mut hasher);
    ctx.theme.id.hash(&mut hasher);
    pane.name.hash(&mut hasher);
    for task in tasks.unwrap_or_default() {
        task.id.hash(&mut hasher);
        task.title.hash(&mut hasher);
    }
    hasher.finish()
}

fn centre_line<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content).center_x(Length::Fill).into()
}

fn notice<'a>(t: &'a Theme, words: String) -> Element<'a, Message> {
    centre_line(
        text(words)
            .size(tokens::font_size::CAPTION)
            .style(style::text::subtle(t))
            .align_x(iced::alignment::Horizontal::Center),
    )
}

fn item_view<'a>(
    ctx: Ctx<'a>,
    ws: &'a Workspace,
    chat: &'a Chat,
    item: &'a ChatItem,
    task_title: &dyn Fn(&str) -> Option<String>,
) -> Element<'a, Message> {
    let t = ctx.theme;
    match &item.body {
        Body::User {
            text: words,
            from: Sender::Person,
            images,
        } => {
            let mut bubble = column![
                text(words.as_str())
                    .size(tokens::font_size::UI_LG)
                    .style(style::text::primary(t))
            ]
            .spacing(tokens::space::XS);
            if *images > 0 {
                bubble = bubble.push(
                    text(ctx.i18n.tn(Key::ChatConvImages, *images, &[]).into_owned())
                        .size(tokens::font_size::CAPTION)
                        .style(style::text::muted(t)),
                );
            }
            mine(t, bubble, false)
        }
        Body::User {
            text: words,
            from: Sender::Hangar,
            ..
        } => hangar_message(ctx, ws, chat, item, words),
        Body::Assistant { .. } => {
            let Some((_, content)) = chat.prose.get(&item.id) else {
                return Space::new().into();
            };
            container(rich::view(t, content, &chat.code))
                .padding([tokens::space::LG, tokens::space::XL])
                .width(Length::Fill)
                .style(style::container::card(t))
                .into()
        }
        Body::Action {
            tool,
            title,
            detail,
            result,
            state,
        } => action(
            ctx,
            ws,
            chat,
            item,
            Action {
                tool,
                title: &titles::title(ctx.i18n, tool, title, detail.as_deref(), task_title),
                detail: detail.as_deref(),
                result: result.as_ref(),
                state: *state,
            },
        ),
        Body::Question {
            questions, answer, ..
        } => asked(ctx, questions, answer.as_deref()),
        Body::Notice { text: words } => notice(t, words.clone()),
    }
}

/// A run of calls, folded into one quiet line saying what they did; the
/// calls, each its own line as alone, once pressed.
fn run_view<'a>(
    ctx: Ctx<'a>,
    ws: &'a Workspace,
    chat: &'a Chat,
    run: &[&'a ChatItem],
    task_title: &dyn Fn(&str) -> Option<String>,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let Some(first) = run.first() else {
        return Space::new().into();
    };
    let key = Piece::key(first);
    let open = chat.expanded.contains(&key);
    let tally = calls::tally(run);
    let mut line = row![
        icon(
            if open {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            },
            11.0,
        )
        .color(t.colors.fg_subtle),
        text(calls::summary(ctx.i18n, &tally))
            .size(tokens::font_size::CAPTION)
            .style(style::text::subtle(t)),
    ]
    .spacing(tokens::space::SM)
    .align_y(Alignment::Center);
    if tally.failed > 0 {
        line = line.push(
            text(format!(
                "· {}",
                ctx.i18n.tn(Key::ChatConvCallsFailed, tally.failed, &[])
            ))
            .size(tokens::font_size::CAPTION)
            .color(t.colors.warning),
        );
    }
    if tally.running > 0 {
        line = line.push(
            text(format!(
                "· {}",
                ctx.i18n.tn(Key::ChatConvCallsRunning, tally.running, &[])
            ))
            .size(tokens::font_size::CAPTION)
            .color(t.colors.state_working),
        );
    }
    let head = button(line)
        .padding([tokens::space::XXS, tokens::space::MD])
        .width(Length::Fill)
        .style(style::button::row(t, false))
        .on_press(msg(Msg::Toggle(ws.id.clone(), key)));
    if !open {
        return head.into();
    }
    let mut list = column![].spacing(tokens::space::XXS);
    for item in run {
        list = list.push(item_view(ctx, ws, chat, item, task_title));
    }
    column![
        head,
        container(list).padding(Padding {
            left: tokens::space::LG,
            ..Padding::ZERO
        })
    ]
    .spacing(tokens::space::XXS)
    .into()
}

/// A bubble of the person's, on the right.
fn mine<'a>(
    t: &'a Theme,
    content: impl Into<Element<'a, Message>>,
    faded: bool,
) -> Element<'a, Message> {
    let bubble = container(content)
        .padding([tokens::space::MD, tokens::space::XL])
        .max_width(BUBBLE_WIDTH)
        .style(move |_| {
            let mut style = style::container::accent_soft(t)(&iced::Theme::Dark);
            style.border.radius = tokens::radius::XL.into();
            if faded {
                style.background = style.background.map(|bg| bg.scale_alpha(0.6));
            }
            style
        });
    row![Space::new().width(Length::Fill), bubble].into()
}

/// A message Hangar sent the orchestrator (a briefing, a worker's news):
/// discreet, its first lines until shown whole.
fn hangar_message<'a>(
    ctx: Ctx<'a>,
    ws: &'a Workspace,
    chat: &'a Chat,
    item: &'a ChatItem,
    words: &'a str,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let whole = chat.expanded.contains(&item.id);
    let lines = words.lines().count();
    let shown = if whole || lines <= HANGAR_LINES {
        words.to_string()
    } else {
        let mut head: Vec<&str> = words.lines().take(HANGAR_LINES).collect();
        if let Some(last) = head.last_mut() {
            *last = last.trim_end();
        }
        format!("{}…", head.join("\n"))
    };
    let mut stack = column![
        text(ctx.t(Key::ChatConvHangar))
            .size(tokens::font_size::MICRO)
            .font(fonts::ui_semibold())
            .style(style::text::subtle(t)),
        text(shown)
            .size(tokens::font_size::CAPTION)
            .style(style::text::muted(t)),
    ]
    .spacing(tokens::space::XXS);
    if lines > HANGAR_LINES {
        let label = if whole {
            Key::ChatConvShowLess
        } else {
            Key::ChatConvShowMore
        };
        stack = stack.push(
            button(
                text(ctx.t(label))
                    .size(tokens::font_size::CAPTION)
                    .style(style::text::accent(t)),
            )
            .padding(0)
            .style(style::button::link(t))
            .on_press(msg(Msg::Toggle(ws.id.clone(), item.id.clone()))),
        );
    }
    container(stack)
        .padding([tokens::space::SM, tokens::space::LG])
        .width(Length::Fill)
        .style(style::container::well(t))
        .into()
}

struct Action<'b> {
    tool: &'b str,
    title: &'b str,
    detail: Option<&'b str>,
    result: Option<&'b CallResult>,
    state: CallState,
}

/// The name of a tool as a chip shows it: an MCP tool's own name.
fn tool_name(tool: &str) -> &str {
    tool.strip_prefix("mcp__")
        .and_then(|rest| rest.split_once("__"))
        .map_or(tool, |(_, name)| name)
}

/// Lines `at_most` of `words`, the rest said to be there.
fn clipped(words: &str, at_most: usize) -> String {
    let count = words.lines().count();
    if count <= at_most {
        return words.to_string();
    }
    let head: Vec<&str> = words.lines().take(at_most).collect();
    format!("{}\n…", head.join("\n"))
}

/// A call, on one line: what it did and how it went; its detail below once
/// pressed.
fn action<'a>(
    ctx: Ctx<'a>,
    ws: &'a Workspace,
    chat: &'a Chat,
    item: &'a ChatItem,
    call: Action<'_>,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let (mark, colour, hint) = match call.state {
        CallState::Running => (
            Icon::Clock,
            t.colors.state_working,
            Some(ctx.t(Key::ChatConvActionRunning)),
        ),
        CallState::Done => (Icon::Check, t.colors.success, None),
        CallState::Error => (
            Icon::Alert,
            t.colors.danger,
            Some(ctx.t(Key::ChatConvActionFailed)),
        ),
    };
    let more = call.detail.is_some() || call.result.is_some_and(|r| !r.text.trim().is_empty());
    let open = more && chat.expanded.contains(&item.id);
    let mut line = row![
        icon(mark, 13.0).color(colour),
        container(
            text(call.title.to_string())
                .size(tokens::font_size::UI)
                .style(style::text::secondary(t)),
        )
        .width(Length::Fill),
    ]
    .spacing(tokens::space::MD)
    .align_y(Alignment::Center);
    if let Some(hint) = hint {
        line = line.push(text(hint).size(tokens::font_size::MICRO).color(colour));
    }
    line = line.push(
        container(
            text(tool_name(call.tool).to_string())
                .size(tokens::font_size::MICRO)
                .font(fonts::mono())
                .style(style::text::subtle(t)),
        )
        .padding([0.0, tokens::space::XS])
        .style(style::container::chip(t)),
    );
    if more {
        line = line.push(
            icon(
                if open {
                    Icon::ChevronDown
                } else {
                    Icon::ChevronRight
                },
                12.0,
            )
            .color(t.colors.fg_subtle),
        );
    }
    let head = button(line)
        .padding([tokens::space::XS, tokens::space::MD])
        .width(Length::Fill)
        .style(style::button::row(t, false))
        .on_press_maybe(more.then(|| msg(Msg::Toggle(ws.id.clone(), item.id.clone()))));
    if !open {
        return head.into();
    }
    let mut detail = column![].spacing(tokens::space::SM);
    if let Some(words) = call.detail {
        detail = detail.push(mono_block(
            t,
            clipped(&titles::detail(call.tool, words), DETAIL_LINES),
        ));
    }
    if let Some(result) = call.result.filter(|r| !r.text.trim().is_empty()) {
        detail = detail.push(
            text(ctx.t(Key::ChatConvActionResult))
                .size(tokens::font_size::MICRO)
                .font(fonts::ui_semibold())
                .style(style::text::subtle(t)),
        );
        let block = mono_block(t, clipped(&result.text, DETAIL_LINES));
        detail = detail.push(block);
        if !result.ok {
            detail = detail.push(
                text(ctx.t(Key::ChatConvActionFailed))
                    .size(tokens::font_size::MICRO)
                    .style(style::text::danger(t)),
            );
        }
    }
    column![
        head,
        container(detail).padding(Padding {
            top: 0.0,
            right: tokens::space::MD,
            bottom: tokens::space::SM,
            left: tokens::space::XXXL,
        })
    ]
    .into()
}

fn mono_block<'a>(t: &'a Theme, words: String) -> Element<'a, Message> {
    container(
        text(words)
            .size(tokens::font_size::LABEL)
            .font(fonts::mono())
            .style(style::text::secondary(t)),
    )
    .padding([tokens::space::SM, tokens::space::MD])
    .width(Length::Fill)
    .style(style::container::well(t))
    .into()
}

/// A question asked and answered, or left without an answer.
fn asked<'a>(
    ctx: Ctx<'a>,
    questions: &'a [Question],
    answers: Option<&'a [Answer]>,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let mut stack = column![].spacing(tokens::space::XS);
    for (at, question) in questions.iter().enumerate() {
        stack = stack.push(
            text(
                ctx.i18n
                    .t_with(
                        Key::ChatConvQuestionAsked,
                        &args!("question" => question.question.as_str()),
                    )
                    .into_owned(),
            )
            .size(tokens::font_size::UI)
            .style(style::text::secondary(t)),
        );
        let answer = answers.and_then(|answers| answers.get(at));
        let words: Vec<String> = answer
            .map(|answer| {
                answer
                    .selected
                    .iter()
                    .cloned()
                    .chain(answer.other.clone())
                    .collect()
            })
            .unwrap_or_default();
        let line = if words.is_empty() {
            text(ctx.t(Key::ChatConvQuestionNoAnswer))
                .size(tokens::font_size::CAPTION)
                .style(style::text::subtle(t))
        } else {
            text(format!("→ {}", words.join(", ")))
                .size(tokens::font_size::CAPTION)
                .style(style::text::accent(t))
        };
        stack = stack.push(line);
    }
    container(stack)
        .padding([tokens::space::SM, tokens::space::LG])
        .width(Length::Fill)
        .style(style::container::well(t))
        .into()
}

/// A message sent from here the transcript does not have yet.
fn outgoing<'a>(ctx: Ctx<'a>, pane: &'a Pane, out: &'a Outgoing) -> Element<'a, Message> {
    let t = ctx.theme;
    let state = match &out.state {
        Sent::Sending => Some(
            text(ctx.t(Key::ChatConvSending))
                .size(tokens::font_size::MICRO)
                .style(style::text::muted(t)),
        ),
        Sent::Typed => None,
        Sent::Queued => Some(
            text(
                ctx.i18n
                    .t_with(Key::ChatConvQueued, &args!("name" => pane.name.as_str()))
                    .into_owned(),
            )
            .size(tokens::font_size::MICRO)
            .style(style::text::muted(t)),
        ),
        Sent::Failed(error) => Some(
            text(
                ctx.i18n
                    .t_with(Key::ChatConvNotSent, &args!("error" => error.as_str()))
                    .into_owned(),
            )
            .size(tokens::font_size::MICRO)
            .style(style::text::danger(t)),
        ),
    };
    let mut bubble = column![
        text(out.text.as_str())
            .size(tokens::font_size::UI_LG)
            .style(style::text::primary(t))
    ]
    .spacing(tokens::space::XS);
    if let Some(state) = state {
        bubble = bubble.push(state);
    }
    mine(t, bubble, !matches!(out.state, Sent::Typed))
}

/// The tallest the body of a card gets: past it, it scrolls, and its
/// title and buttons stay in sight.
const CARD_BODY_HEIGHT: f32 = 340.0;

/// A card's parts: its title row, what it shows (scrolled when tall) and
/// its buttons (always shown).
struct CardParts<'a> {
    title: Element<'a, Message>,
    body: Element<'a, Message>,
    actions: Element<'a, Message>,
    /// The body's scroll, per question shown: a new one starts at its top.
    scroll: iced::widget::Id,
}

/// The card of what the pane waits on the person for.
fn card<'a>(
    ctx: Ctx<'a>,
    ws: &'a Workspace,
    pane: &'a Pane,
    chat: &'a Chat,
) -> Option<Element<'a, Message>> {
    let t = ctx.theme;
    let parts = match chat.thread.pending.as_ref()? {
        ChatPending::Question { questions, .. } => question_card(ctx, ws, pane, chat, questions),
        ChatPending::Permission {
            tool,
            title,
            detail,
            ..
        } => permission_card(ctx, ws, pane, chat, tool, title, detail.as_deref()),
    };
    let body = container(
        scrollable(container(parts.body).padding(Padding {
            right: tokens::space::LG,
            ..Padding::ZERO
        }))
        .id(parts.scroll)
        .height(Length::Shrink)
        .style(style::scrollable::default(t)),
    )
    .max_height(CARD_BODY_HEIGHT);
    let mut stack = column![parts.title, body].spacing(tokens::space::LG);
    // What happened to the last answer, right above the buttons: never out
    // of sight.
    if chat.deciding {
        stack = stack.push(hangar_ui::text::hint(t, ctx.t(Key::ChatConvBusy)));
    }
    if let Some(refusal) = &chat.refusal {
        stack = stack.push(
            hangar_ui::text::danger(
                t,
                ctx.i18n
                    .t_with(Key::ChatConvRefused, &args!("error" => refusal.as_str()))
                    .into_owned(),
            )
            .size(tokens::font_size::CAPTION),
        );
    }
    stack = stack.push(parts.actions);
    Some(centred(
        container(
            container(stack)
                .padding(tokens::space::XL)
                .width(Length::Fill)
                .style(move |_| {
                    let mut style = style::container::card(t)(&iced::Theme::Dark);
                    style.border.color = t.colors.state_waiting;
                    style.border.width = 1.0;
                    style
                }),
        )
        .padding([tokens::space::SM, tokens::space::XXL]),
    ))
}

fn card_title<'a>(t: &'a Theme, words: String) -> Element<'a, Message> {
    row![
        icon(Icon::Bell, 14.0).color(t.colors.state_waiting),
        text(words)
            .size(tokens::font_size::UI_LG)
            .font(fonts::ui_semibold())
            .style(style::text::primary(t)),
    ]
    .spacing(tokens::space::MD)
    .align_y(Alignment::Center)
    .into()
}

/// A question's header, as a chip.
fn header_chip<'a>(t: &'a Theme, header: &'a str) -> Element<'a, Message> {
    container(
        text(header)
            .size(tokens::font_size::MICRO)
            .style(style::text::muted(t)),
    )
    .padding([0.0, tokens::space::SM])
    .style(style::container::chip(t))
    .into()
}

/// An option of a question, chosen or not.
fn choice<'a>(
    t: &'a Theme,
    label: String,
    description: Option<&'a str>,
    multi: bool,
    on: bool,
    press: Option<Message>,
) -> Element<'a, Message> {
    let mark = match (multi, on) {
        (_, true) => Icon::Check,
        (true, false) => Icon::Minus,
        (false, false) => Icon::ChevronRight,
    };
    let mut words = column![
        text(label)
            .size(tokens::font_size::UI)
            .style(style::text::primary(t))
    ]
    .width(Length::Fill);
    if let Some(description) = description.filter(|d| !d.trim().is_empty()) {
        words = words.push(
            text(description)
                .size(tokens::font_size::CAPTION)
                .style(style::text::muted(t)),
        );
    }
    button(
        row![
            icon(mark, 12.0).color(if on {
                t.colors.accent
            } else {
                t.colors.fg_subtle
            }),
            words
        ]
        .spacing(tokens::space::MD)
        .align_y(Alignment::Center),
    )
    .padding([tokens::space::SM, tokens::space::MD])
    .width(Length::Fill)
    .style(style::button::toggle(t, on))
    .on_press_maybe(press)
    .into()
}

/// A question's answer in words: the labels picked, then the person's own.
fn said(answer: &Answer) -> String {
    answer
        .selected
        .iter()
        .cloned()
        .chain(answer.other.clone())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The question card: one question at a time, as Claude Code asks them —
/// its header and words, its options and "Other", the step it is (1/3),
/// Back and Next, and on the last one the answers given and Send.
fn question_card<'a>(
    ctx: Ctx<'a>,
    ws: &'a Workspace,
    pane: &'a Pane,
    chat: &'a Chat,
    questions: &'a [Question],
) -> CardParts<'a> {
    let t = ctx.theme;
    let form = &chat.form;
    let free = !chat.deciding;
    let count = questions.len();
    let at = form.step();
    let last = at + 1 >= count;
    let mut title = row![
        container(card_title(
            t,
            ctx.i18n
                .t_with(
                    Key::ChatConvQuestionTitle,
                    &args!("name" => pane.name.as_str())
                )
                .into_owned(),
        ))
        .width(Length::Fill)
    ]
    .align_y(Alignment::Center);
    if count > 1 {
        title = title.push(
            text(format!("{}/{count}", at + 1))
                .size(tokens::font_size::CAPTION)
                .font(fonts::mono())
                .style(style::text::muted(t)),
        );
    }
    let mut block = column![].spacing(tokens::space::SM);
    if let Some(question) = questions.get(at) {
        if !question.header.trim().is_empty() {
            block = block.push(header_chip(t, question.header.as_str()));
        }
        block = block.push(
            text(question.question.as_str())
                .size(tokens::font_size::UI_LG)
                .width(Length::Fill)
                .style(style::text::primary(t)),
        );
        if question.multi_select {
            block = block.push(hangar_ui::text::hint(t, ctx.t(Key::ChatConvQuestionMulti)));
        }
        let picked = form.picked.get(at).map(Vec::as_slice).unwrap_or(&[]);
        for (option_at, option) in question.options.iter().enumerate() {
            block = block.push(choice(
                t,
                option.label.clone(),
                Some(option.description.as_str()),
                question.multi_select,
                picked.contains(&option_at),
                free.then(|| {
                    msg(Msg::Pick {
                        ws: ws.id.clone(),
                        question: at,
                        option: option_at,
                    })
                }),
            ));
        }
        let other_on = form.other_on.get(at).copied().unwrap_or(false);
        block = block.push(choice(
            t,
            ctx.t(Key::ChatConvQuestionOther).into_owned(),
            None,
            question.multi_select,
            other_on,
            free.then(|| {
                msg(Msg::Other {
                    ws: ws.id.clone(),
                    question: at,
                })
            }),
        ));
        if other_on {
            let words = form.other.get(at).map(String::as_str).unwrap_or("");
            let ws_id = ws.id.clone();
            let field = text_input(&ctx.t(Key::ChatConvQuestionOtherPlaceholder), words)
                .size(tokens::font_size::UI)
                .padding([tokens::space::SM, tokens::space::MD])
                .style(style::text_input::field(t))
                .on_input_maybe(free.then_some(move |text| {
                    msg(Msg::OtherText {
                        ws: ws_id.clone(),
                        question: at,
                        text,
                    })
                }));
            block = block.push(crate::keymap::guard(ctx, field));
        }
    }
    if last && count > 1 {
        block = block.push(recap(ctx, ws, form, questions, free));
    }
    let answered = form.answered(questions, at);
    let mut actions = row![].spacing(tokens::space::MD).align_y(Alignment::Center);
    if at > 0 {
        actions = actions.push(
            ui_button::secondary(t, ctx.t(Key::ChatConvQuestionBack)).on_press_maybe(free.then(
                || {
                    msg(Msg::Step {
                        ws: ws.id.clone(),
                        to: at - 1,
                    })
                },
            )),
        );
    }
    // Why the button waits, said rather than a button that does nothing.
    let waits: Element<'a, Message> = if answered {
        Space::new().width(Length::Fill).into()
    } else {
        container(hangar_ui::text::hint(
            t,
            ctx.t(Key::ChatConvQuestionNeedsAnswer),
        ))
        .width(Length::Fill)
        .into()
    };
    actions = actions.push(waits);
    actions = actions.push(if last {
        let ready = form.answers(questions).is_some();
        ui_button::primary(t, ctx.t(Key::ChatConvQuestionSubmit))
            .icon(Icon::Send)
            .on_press_maybe((ready && free).then(|| msg(Msg::SubmitAnswer(ws.id.clone()))))
    } else {
        ui_button::primary(t, ctx.t(Key::ChatConvQuestionNext))
            .icon_after(Icon::ChevronRight)
            .on_press_maybe((answered && free).then(|| {
                msg(Msg::Step {
                    ws: ws.id.clone(),
                    to: at + 1,
                })
            }))
    });
    CardParts {
        title: title.into(),
        body: block.into(),
        actions: actions.into(),
        scroll: iced::widget::Id::from(format!("chat-view-question-{}-{at}", form.tool_use_id)),
    }
}

/// The answers given so far, each a way back to its question.
fn recap<'a>(
    ctx: Ctx<'a>,
    ws: &'a Workspace,
    form: &'a super::Form,
    questions: &'a [Question],
    free: bool,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let mut list = column![
        text(ctx.t(Key::ChatConvQuestionRecap))
            .size(tokens::font_size::MICRO)
            .font(fonts::ui_semibold())
            .style(style::text::subtle(t))
    ]
    .spacing(tokens::space::XS);
    for (at, question) in questions.iter().enumerate() {
        let answer = form
            .answers_at(questions, at)
            .map(|answer| said(&answer))
            .unwrap_or_else(|| ctx.t(Key::ChatConvQuestionNoAnswer).into_owned());
        let named = if question.header.trim().is_empty() {
            (at + 1).to_string()
        } else {
            question.header.clone()
        };
        list = list.push(
            button(
                row![
                    container(
                        text(named)
                            .size(tokens::font_size::MICRO)
                            .style(style::text::muted(t)),
                    )
                    .padding([0.0, tokens::space::SM])
                    .style(style::container::chip(t)),
                    text(answer)
                        .size(tokens::font_size::CAPTION)
                        .width(Length::Fill)
                        .style(style::text::secondary(t)),
                ]
                .spacing(tokens::space::MD)
                .align_y(Alignment::Center),
            )
            .padding([tokens::space::XS, tokens::space::MD])
            .width(Length::Fill)
            .style(style::button::row(t, false))
            .on_press_maybe(free.then(|| {
                msg(Msg::Step {
                    ws: ws.id.clone(),
                    to: at,
                })
            })),
        );
    }
    container(list)
        .padding(Padding {
            top: tokens::space::MD,
            ..Padding::ZERO
        })
        .into()
}

fn permission_card<'a>(
    ctx: Ctx<'a>,
    ws: &'a Workspace,
    pane: &'a Pane,
    chat: &'a Chat,
    tool: &'a str,
    title: &'a str,
    detail: Option<&'a str>,
) -> CardParts<'a> {
    let t = ctx.theme;
    let free = !chat.deciding;
    let tasks = ctx.model.live.board.tasks(&ws.cwd);
    let task_title = move |id: &str| {
        tasks?
            .iter()
            .find(|task| task.id == id)
            .map(|task| task.title.clone())
    };
    let what = titles::title(ctx.i18n, tool, title, detail, &task_title);
    let mut body = column![
        text(what)
            .size(tokens::font_size::UI_LG)
            .style(style::text::secondary(t)),
    ]
    .spacing(tokens::space::MD);
    if let Some(detail) = detail {
        body = body.push(mono_block(
            t,
            clipped(&titles::detail(tool, detail), DETAIL_LINES),
        ));
    }
    let actions = row![
        Space::new().width(Length::Fill),
        ui_button::secondary(t, ctx.t(Key::ChatConvPermissionDeny))
            .on_press_maybe(free.then(|| msg(Msg::Permit(ws.id.clone(), false)))),
        ui_button::primary(t, ctx.t(Key::ChatConvPermissionAllow))
            .icon(Icon::Check)
            .on_press_maybe(free.then(|| msg(Msg::Permit(ws.id.clone(), true)))),
    ]
    .spacing(tokens::space::MD)
    .align_y(Alignment::Center);
    CardParts {
        title: card_title(
            t,
            ctx.i18n
                .t_with(
                    Key::ChatConvPermissionTitle,
                    &args!("name" => pane.name.as_str(), "tool" => tool_name(tool)),
                )
                .into_owned(),
        ),
        body: body.into(),
        actions: actions.into(),
        scroll: iced::widget::Id::new("chat-view-permission"),
    }
}

/// The words that say why the input is disabled.
pub fn blocked_words(ctx: Ctx<'_>, pane: &Pane, why: Blocked) -> String {
    let name = args!("name" => pane.name.as_str());
    match why {
        Blocked::Loading => ctx.t(Key::ChatConvDisabledLoading).into_owned(),
        Blocked::NotClaude => ctx
            .i18n
            .t_with(Key::ChatConvDisabledNotClaude, &name)
            .into_owned(),
        Blocked::Answering => ctx
            .i18n
            .t_with(Key::ChatConvDisabledAnswering, &name)
            .into_owned(),
        Blocked::Resuming => ctx
            .i18n
            .t_with(Key::ChatConvDisabledResuming, &name)
            .into_owned(),
        Blocked::Question => ctx.t(Key::ChatConvDisabledQuestion).into_owned(),
        Blocked::Permission => ctx.t(Key::ChatConvDisabledPermission).into_owned(),
    }
}

/// Enter sends, Shift+Enter breaks the line; everything else as the editor
/// does it.
fn keys(ws: String) -> impl Fn(KeyPress) -> Option<Binding<Message>> {
    move |press: KeyPress| {
        let enter = matches!(press.key, keyboard::Key::Named(Named::Enter));
        if enter && matches!(press.status, Status::Focused { .. }) && !press.modifiers.shift() {
            return Some(Binding::Custom(msg(Msg::Send(ws.clone()))));
        }
        Binding::from_key_press(press)
    }
}

fn input<'a>(
    ctx: Ctx<'a>,
    ws: &'a Workspace,
    pane: &'a Pane,
    chat: &'a Chat,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let why = blocked(chat, pane);
    let placeholder = ctx
        .i18n
        .t_with(
            Key::ChatConvPlaceholder,
            &args!("name" => pane.name.as_str()),
        )
        .into_owned();
    let mut editor = text_editor(&chat.draft)
        .id(input_id())
        .placeholder(placeholder)
        .size(tokens::font_size::UI_LG)
        .padding([tokens::space::MD, tokens::space::LG])
        .min_height(40.0)
        .max_height(200.0)
        .style(style::text_editor::field(t));
    if why.is_none() {
        let ws_id = ws.id.clone();
        editor = editor
            .on_action(move |action| msg(Msg::Edit(ws_id.clone(), action)))
            .key_binding(keys(ws.id.clone()));
    }
    let can_send = why.is_none() && !chat.draft.text().trim().is_empty();
    let send = ui_button::primary(t, ctx.t(Key::ChatConvSend))
        .icon(Icon::Send)
        .on_press_maybe(can_send.then(|| msg(Msg::Send(ws.id.clone()))));
    let under = match why {
        Some(why) => text(blocked_words(ctx, pane, why))
            .size(tokens::font_size::CAPTION)
            .style(style::text::warning(t)),
        None => text(ctx.t(Key::ChatConvInputHint))
            .size(tokens::font_size::CAPTION)
            .style(style::text::subtle(t)),
    };
    centred(
        container(
            column![
                row![crate::keymap::guard(ctx, editor), send]
                    .spacing(tokens::space::MD)
                    .align_y(Alignment::End),
                under,
            ]
            .spacing(tokens::space::XS),
        )
        .padding(Padding {
            top: tokens::space::SM,
            right: tokens::space::XXL,
            bottom: tokens::space::XL,
            left: tokens::space::XXL,
        }),
    )
}
