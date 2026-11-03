//! The capsule in the middle of the toolbar (`ToolbarActivity.tsx`): what the
//! focused pane is doing, in one line, and how many agents of the workspace
//! are waiting on you.
//!
//! Read-only on purpose, except for the two counters, which take you to the
//! pane they count.

use iced::widget::{Space, button, container, row, stack, text, tooltip};
use iced::{Alignment, Element, Length, Padding};

use hangar_i18n::{Key, args};
use hangar_theme::tokens::{font_size, space};
use hangar_theme::{Theme, fonts, style};
use hangar_ui::glyph;

use super::Msg;
use crate::ctx::Ctx;
use crate::message::Message;
use crate::navigator::{reads, skin};
use crate::state::layout;
use crate::state::live::{PaneEventDetail, PaneInsight};
use crate::state::{AgentActivity, Pane, PaneId, Workspace};

/// The detail after the state word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detail {
    None,
    /// Waiting: `· Bash`, the tool it asks about.
    Tool(String),
    /// Working on a board task: `on “Title”`, in full strength.
    Working(String),
    /// Done with a board task: `· “Title”`.
    Done(String),
    /// For want of anything else, the last thing it said.
    LastAnswer(String),
}

/// What the capsule adds to the state word (`ToolbarActivity.tsx`): the task
/// it is on or just finished, what it is asking, or, for want of anything
/// else, the last thing it said.
///
/// Only the latest event of the pane's history can be the question: an older
/// one was answered.
pub fn detail(activity: AgentActivity, insight: &PaneInsight, task_title: Option<&str>) -> Detail {
    match activity {
        // The state word already says it waits for you; what is left to say
        // is what for: the tool.
        AgentActivity::Waiting => match insight.events.first().map(|event| &event.detail) {
            Some(PaneEventDetail::Permission { tool: Some(tool) }) if !tool.is_empty() => {
                Detail::Tool(tool.clone())
            }
            _ => Detail::None,
        },
        AgentActivity::Working if task_title.is_some() => {
            Detail::Working(task_title.unwrap_or_default().to_string())
        }
        AgentActivity::Yours if task_title.is_some() => {
            Detail::Done(task_title.unwrap_or_default().to_string())
        }
        _ => match &insight.last_answer {
            Some(answer) if !answer.is_empty() => Detail::LastAnswer(answer.clone()),
            _ => Detail::None,
        },
    }
}

/// The context window used, in whole percent held between 0 and 100.
pub fn context_pct(insight: &PaneInsight) -> Option<u8> {
    insight
        .context
        .as_ref()
        .map(|gauge| gauge.pct.clamp(0.0, 100.0).round() as u8)
}

/// The next pane in `wanted` after `focused`, wrapping, in reading order: the
/// first click lands on the first such pane, the next ones walk through the
/// rest.
pub fn next_in<'a>(
    ws: &'a Workspace,
    focused: Option<&PaneId>,
    wanted: impl Fn(&PaneId) -> bool,
) -> Option<&'a Pane> {
    let ids: Vec<&PaneId> = layout::reading_order(&ws.panes, ws.tree.as_ref())
        .into_iter()
        .map(|pane| &pane.id)
        .filter(|id| wanted(id))
        .collect();
    if ids.is_empty() {
        return None;
    }
    let at = focused.and_then(|focused| ids.iter().position(|id| *id == focused));
    let next = ids[at.map_or(0, |at| at + 1) % ids.len()];
    ws.panes.iter().find(|pane| &pane.id == next)
}

/// How wide the toolbar must be (in em of its 12 px) for each extra of the
/// capsule to show: the context chip and the badges, then the sentence after
/// the state word.
pub const EXTRAS_FROM_EM: f32 = 71.0;

pub fn view<'a>(ctx: Ctx<'a>, ws: &'a Workspace, extras: bool, room: f32) -> Element<'a, Message> {
    let t = ctx.theme;
    let tr = ctx.i18n;
    let focused = ctx
        .model
        .ui
        .focus_by_workspace
        .get(&ws.id)
        .or_else(|| reads::dev_focused_pane());
    let pane = focused.and_then(|id| ws.panes.iter().find(|pane| &pane.id == id));
    let activity = pane.map_or(AgentActivity::Idle, |pane| {
        reads::activity(ctx.model, &pane.id)
    });
    // A pane that never reported has the empty insight; so does no pane.
    let none = PaneInsight::default();
    let insight = pane.map_or(&none, |pane| ctx.model.live.panes_logic.insight(&pane.id));
    // The task the pane is on, as far as the board can tell (`taskForPane`):
    // an open task whose assignee resolves to this pane. The workspace in
    // front is the one this window draws.
    let task = pane.and_then(|pane| {
        ctx.model.live.board.task_for_pane(
            ws,
            &pane.id,
            &ctx.model.state().workspaces,
            Some(&ws.id),
        )
    });
    let counts = reads::counts(ctx.model, ws);

    // Who, and what they are doing.
    let who: Element<'a, Message> = match pane {
        Some(pane) => row![
            glyph::state(t, reads::activity_glyph(activity), 7.0),
            text(pane.name.as_str())
                .size(font_size::UI)
                .font(fonts::ui_semibold())
                .wrapping(text::Wrapping::None)
        ]
        .spacing(7)
        .align_y(Alignment::Center)
        .into(),
        None => text(ws.name.as_str())
            .size(font_size::UI)
            .font(fonts::ui_semibold())
            .wrapping(text::Wrapping::None)
            .into(),
    };

    let message: Element<'a, Message> = match pane {
        None => {
            let line = if counts.working > 0 {
                tr.tn(Key::SidebarActivityWorking, counts.working, &[])
            } else {
                tr.t(Key::ToolbarActivityQuiet)
            };
            text(line.into_owned())
                .size(font_size::UI)
                .style(style::text::secondary(t))
                .wrapping(text::Wrapping::None)
                .into()
        }
        Some(_) => {
            let word = text(tr.t(state_word(activity)).into_owned())
                .size(font_size::UI)
                .font(fonts::ui_medium())
                .style(skin::state_text(t, activity))
                .wrapping(text::Wrapping::None);
            let mut line = row![word].spacing(space::SM).align_y(Alignment::Center);
            // With no room for a few words the state word stands alone, rather
            // than trailing off as "Travaille su…".
            if room >= 30.0 * font_size::UI {
                let (words, strong) =
                    match detail(activity, insight, task.map(|task| task.title.as_str())) {
                        Detail::None => (None, false),
                        Detail::Tool(tool) => (Some(format!("· {tool}")), false),
                        Detail::Working(title) => (
                            Some(
                                tr.t_with(Key::ToolbarActivityOn, &args!("title" => title))
                                    .into_owned(),
                            ),
                            true,
                        ),
                        Detail::Done(title) => (
                            Some(format!(
                                "· {}",
                                tr.t_with(Key::ToolbarActivityTask, &args!("title" => title))
                            )),
                            true,
                        ),
                        Detail::LastAnswer(answer) => (
                            Some(format!(
                                "· {}",
                                tr.t_with(
                                    Key::ToolbarActivityLastAnswer,
                                    &args!("answer" => answer)
                                )
                            )),
                            false,
                        ),
                    };
                if let Some(words) = words {
                    line = line.push(
                        text(words)
                            .size(font_size::UI)
                            .wrapping(text::Wrapping::None)
                            .color(if strong {
                                t.colors.fg
                            } else {
                                t.colors.fg_secondary
                            }),
                    );
                }
            }
            line.into()
        }
    };

    let mut capsule = row![
        container(
            row![who, container(message).width(Length::Fill).clip(true)]
                .spacing(space::MD)
                .align_y(Alignment::Center)
        )
        .width(Length::Fill)
        .clip(true)
    ]
    .spacing(space::MD)
    .align_y(Alignment::Center);

    let pct = context_pct(insight);
    if extras && let Some(pct) = pct {
        capsule = capsule.push(tooltip(
            text(
                tr.t_with(Key::ToolbarActivityContext, &args!("pct" => pct))
                    .into_owned(),
            )
            .size(font_size::CAPTION)
            .font(fonts::mono())
            .style(style::text::secondary(t)),
            tip_text(
                t,
                tr.t_with(Key::ToolbarActivityContextHint, &args!("pct" => pct))
                    .into_owned(),
            ),
            tooltip::Position::Bottom,
        ));
    }

    if extras {
        let waiting = (counts.waiting > 0)
            .then(|| {
                next_in(ws, focused, |id| {
                    reads::activity(ctx.model, id) == AgentActivity::Waiting
                })
            })
            .flatten();
        let yours = (counts.yours > 0)
            .then(|| {
                next_in(ws, focused, |id| {
                    reads::activity(ctx.model, id) == AgentActivity::Yours
                })
            })
            .flatten();
        let badge = |kind: AgentActivity, count: usize, target: Option<&'a Pane>| {
            let target = target?;
            let summary = tr.tn(
                match kind {
                    AgentActivity::Waiting => Key::SidebarActivityWaiting,
                    _ => Key::SidebarActivityYours,
                },
                count,
                &[],
            );
            let label = tr.t_with(
                Key::ToolbarActivityGoTo,
                &args!("summary" => summary.as_ref(), "name" => target.name.as_str()),
            );
            let content = row![
                glyph::state(t, reads::activity_glyph(kind), 7.0),
                text(count.to_string())
                    .size(font_size::CAPTION)
                    .font(fonts::mono())
                    .style(skin::state_text(t, kind)),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center);
            let pressed = button(container(content).center_y(Length::Fill))
                .padding([0.0, space::SM])
                .height(22)
                .style(style::button::icon(t))
                .on_press(Message::Toolbar(Msg::GoTo(
                    ws.id.clone(),
                    target.id.clone(),
                )));
            Some(Element::from(tooltip(
                pressed,
                tip_text(t, label.into_owned()),
                tooltip::Position::Bottom,
            )))
        };
        let badges: Vec<Element<'a, Message>> = [
            badge(AgentActivity::Waiting, counts.waiting, waiting),
            badge(AgentActivity::Yours, counts.yours, yours),
        ]
        .into_iter()
        .flatten()
        .collect();
        if !badges.is_empty() {
            let rule = t.colors.line_strong;
            capsule =
                capsule
                    .push(container(Space::new()).width(1).height(18).style(
                        move |_: &iced::Theme| container::Style {
                            background: Some(iced::Background::Color(rule)),
                            ..container::Style::default()
                        },
                    ))
                    .push(row(badges).spacing(2).align_y(Alignment::Center));
        }
    }

    let body = container(capsule)
        .height(30)
        .padding(Padding {
            top: 0.0,
            right: 5.0,
            bottom: 0.0,
            left: 12.0,
        })
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .style(skin::well(t));

    // The context used, as a hairline along the bottom of the capsule.
    match (extras, pct) {
        (_, Some(pct)) if pct > 0 => {
            let track = t.colors.line;
            let fill = t.colors.accent;
            let used = u16::from(pct);
            let bar = container(
                row![
                    container(Space::new())
                        .width(Length::FillPortion(used))
                        .height(2)
                        .style(move |_: &iced::Theme| container::Style {
                            background: Some(iced::Background::Color(fill)),
                            border: iced::Border {
                                radius: 1.0.into(),
                                ..iced::Border::default()
                            },
                            ..container::Style::default()
                        }),
                    Space::new().width(Length::FillPortion(100 - used)),
                ]
                .height(2),
            )
            .padding([0.0, 8.0])
            .width(Length::Fill)
            .style(move |_: &iced::Theme| container::Style {
                background: Some(iced::Background::Color(track)),
                ..container::Style::default()
            });
            stack![
                body,
                container(bar)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_y(Alignment::End)
            ]
            .into()
        }
        _ => body.into(),
    }
}

/// A state word's key (`pane.state.*`).
fn state_word(activity: AgentActivity) -> Key {
    match activity {
        AgentActivity::Working => Key::PaneStateWorking,
        AgentActivity::Waiting => Key::PaneStateWaiting,
        AgentActivity::Yours => Key::PaneStateYours,
        AgentActivity::Idle => Key::PaneStateIdle,
    }
}

/// The body of a tooltip drawn by hand.
fn tip_text<'a>(t: &'a Theme, words: String) -> Element<'a, Message> {
    container(text(words).size(font_size::CAPTION_LG))
        .padding([space::XS - 1.0, space::MD - 1.0])
        .style(style::container::tooltip(t))
        .into()
}

#[cfg(test)]
mod tests;
