//! A task's card, its priority and its assignee (`BoardView.tsx`'s
//! `renderCard`, `BoardPriority.tsx`, `BoardAssignee.tsx`).

use hangar_i18n::{Key, args};
use hangar_theme::tokens::{font_size, radius, space};
use hangar_theme::{Theme, fonts, style};
use hangar_ui::glyph::{self, GlyphState as Shown};
use hangar_ui::icon::{Icon, icon};
use hangar_ui::tip;
use iced::widget::{Space, button, column, container, row, rule, text};
use iced::{Alignment, Element, Length, Padding, mouse};

use crate::message::Message;
use crate::state::GlyphState;

use super::format::{DateStyle, elapsed_label, priority_key, priority_level, short_task_id, when};
use super::hit::{Look, hit};
use super::look::{self, CardLook};
use super::model::{Blocker, Task, blockers_of};
use super::view::View;
use super::{Msg, card_id};

/// A task's card.
pub(super) fn card<'a>(v: &View<'a>, task: &'a Task) -> Element<'a, Message> {
    let t = v.t();
    let tr = v.ctx.i18n;
    let done = task.column == "done";
    let dragging =
        v.st.press
            .as_ref()
            .is_some_and(|press| press.moved && press.task == task.id);
    let blockers = if done {
        Vec::new()
    } else {
        blockers_of(task, &v.by_id)
    };
    let blocker_names: Vec<String> = blockers
        .iter()
        .map(|blocker| match blocker {
            Blocker::Missing(_) => tr.t(Key::BoardMissingTask).into_owned(),
            Blocker::Task(dep) => format!("#{} · {}", short_task_id(&dep.id), dep.title),
        })
        .collect();

    let id = mono_subtle(t, format!("#{}", short_task_id(&task.id)), font_size::LABEL);
    let top = row![
        tip::over(t, id, task.id.clone()),
        Space::new().width(Length::Fill),
        priority(v, task.priority),
    ]
    .spacing(space::MD)
    .align_y(Alignment::Center);

    let title = text(task.title.as_str())
        .size(font_size::UI_LG)
        .font(fonts::ui_medium())
        .wrapping(text::Wrapping::WordOrGlyph)
        .style(look::ink(if done {
            t.colors.fg_muted
        } else {
            t.colors.fg
        }));

    let mut body = column![top, title].spacing(space::SM);

    if !task.labels.is_empty() {
        let chips = task.labels.iter().map(|label| {
            container(
                text(label.as_str())
                    .size(font_size::MICRO)
                    .font(fonts::mono())
                    .wrapping(text::Wrapping::None),
            )
            .padding([0.0, space::SM])
            .style(look::chip(t))
            .into()
        });
        body = body.push(row(chips).spacing(space::XS).wrap());
    }

    if let Some(blocked) = &task.blocked {
        let recheck = blocked
            .recheck
            .as_deref()
            .map(|date| tr.t_with(Key::BoardBlockedRecheck, &args!("date" => date)));
        let mut hint = vec![blocked.reason.clone()];
        hint.extend(recheck.iter().map(|text| text.to_string()));
        if blocked.human_only {
            hint.push(tr.t(Key::BoardBlockedPerson).into_owned());
        }
        let mut words = format!("{} · {}", tr.t(Key::BoardBlocked), blocked.reason);
        if let Some(recheck) = &recheck {
            words.push_str(&format!(" · {recheck}"));
        }
        let line = row![
            icon(Icon::Pause, 11.0).color(t.colors.fg_muted),
            text(words)
                .size(font_size::CAPTION)
                .style(style::text::muted(t))
                .width(Length::Fill),
        ]
        .spacing(space::SM);
        body = body.push(tip::over(t, line, hint.join(" · ")));
    }

    if !blockers.is_empty() {
        let line = row![
            icon(Icon::Lock, 11.0).color(t.colors.danger),
            text(
                tr.t_with(
                    Key::BoardBlockedBy,
                    &args!("tasks" => blocker_names.join(", ").as_str())
                )
                .into_owned()
            )
            .size(font_size::CAPTION)
            .style(style::text::danger(t))
            .width(Length::Fill),
        ]
        .spacing(space::SM);
        body = body.push(tip::over(t, line, blocker_names.join("\n")));
    }

    body = body
        .push(rule::horizontal(1).style(style::rule::hairline(t)))
        .push(footer(v, task, done, blockers.is_empty()));

    let framed = container(body.padding(Padding {
        top: 9.0,
        right: space::LG,
        bottom: 8.0,
        left: space::LG,
    }))
    .width(Length::Fill)
    .style(look::card(
        t,
        CardLook {
            done,
            on_hold: task.blocked.is_some(),
            dragging,
        },
    ));

    // The card says which board it is on: the window that draws it is not
    // always the main window's active workspace.
    let (pressed, opened) = (task.id.clone(), task.id.clone());
    let (pressed_on, opened_on) = (v.cwd.to_owned(), v.cwd.to_owned());
    hit(framed)
        .id(card_id(&task.id))
        .on_press(move |at| Message::Board(Msg::Press(pressed_on.clone(), pressed.clone(), at)))
        .on_activate(Message::Board(Msg::OpenTask(opened_on, opened)))
        .interaction(mouse::Interaction::Pointer)
        .look(Look {
            hover: Some(look::card_hover(t)),
            ring: Some(t.colors.accent),
            radius: radius::LG,
            inset_x: 0.0,
        })
        .into()
}

/// The foot of a card: who holds it, and its clock and counts.
fn footer<'a>(
    v: &View<'a>,
    task: &'a Task,
    done: bool,
    dependencies_met: bool,
) -> Element<'a, Message> {
    let t = v.t();
    let tr = v.ctx.i18n;
    let who: Element<'a, Message> = match &task.assignee {
        Some(assignee) => assignee_view(v, assignee, done, false),
        None => text(tr.t(Key::BoardNobody))
            .size(font_size::CAPTION)
            .style(style::text::subtle(t))
            .into(),
    };

    let stat = |glyph: Icon, count: String, words: String| -> Element<'a, Message> {
        tip::over(
            t,
            row![
                icon(glyph, 11.0).color(t.colors.fg_subtle),
                mono_subtle(t, count, font_size::LABEL)
            ]
            .spacing(space::XS)
            .align_y(Alignment::Center),
            words,
        )
    };
    let mut meta = row![].spacing(space::LG).align_y(Alignment::Center);

    // The clock of work under way.
    if let (true, Some(since)) = (task.column == "doing", task.doing_since) {
        meta = meta.push(stat(
            Icon::Clock,
            elapsed_label(v.now - since as i64, tr),
            tr.t_with(
                Key::BoardDoingSince,
                &args!("date" => when(since, tr.locale(), DateStyle::Full).as_str()),
            )
            .into_owned(),
        ));
    }
    // The link shows only once everything the task waited on is done.
    if !task.depends_on.is_empty() && dependencies_met {
        meta = meta.push(stat(
            Icon::Link,
            task.depends_on.len().to_string(),
            tr.t(Key::BoardDependsOn).into_owned(),
        ));
    }
    if !task.comments.is_empty() {
        meta = meta.push(stat(
            Icon::Bubble,
            task.comments.len().to_string(),
            tr.tn(Key::BoardComments, task.comments.len(), &[])
                .into_owned(),
        ));
    }

    row![who, Space::new().width(Length::Fill), meta]
        .spacing(space::LG)
        .align_y(Alignment::Center)
        .into()
}

fn mono_subtle<'a>(t: &'a Theme, content: String, size: f32) -> iced::widget::Text<'a> {
    text(content)
        .size(size)
        .font(fonts::mono())
        .style(style::text::subtle(t))
}

/// A task's priority as three rising bars and a word (Low, Medium, High): the
/// way a signal is read, how many bars are lit is the urgency. The number
/// agents set stays in the tooltip.
pub(super) fn priority<'a>(v: &View<'a>, priority: u8) -> Element<'a, Message> {
    let t = v.t();
    let tr = v.ctx.i18n;
    let value = f64::from(priority);
    let level = priority_level(value);
    let word = tr.t(priority_key(value)).into_owned();
    // The bars rise to the right: 44 %, 72 % and the whole of the tallest.
    let bars = row((0..3).map(|n| {
        let rise = [0.44, 0.72, 1.0][n];
        container(Space::new())
            .width(look::PRIO_BAR_WIDTH)
            .height(look::PRIO_BAR_HEIGHT * rise)
            .style(look::prio_bar(t, n < level.bars()))
            .into()
    }))
    .spacing(1.5)
    .align_y(Alignment::End);
    let ink = look::ink(if level.bars() == 3 {
        t.colors.fg_muted
    } else {
        t.colors.fg_subtle
    });
    let words = tr
        .t_with(
            Key::BoardPriorityTitle,
            &args!("n" => priority, "level" => word.as_str()),
        )
        .into_owned();
    tip::over(
        t,
        row![
            container(bars).height(look::PRIO_BAR_HEIGHT),
            text(word)
                .size(font_size::LABEL)
                .font(fonts::ui_medium())
                .style(ink)
        ]
        .spacing(5.0)
        .align_y(Alignment::Center),
        words,
    )
}

/// How the glyph of a pane's state is drawn.
pub(super) fn glyph_of(state: GlyphState) -> Shown {
    match state {
        GlyphState::Starting => Shown::Starting,
        GlyphState::Exited => Shown::Exited,
        GlyphState::Working => Shown::Working,
        GlyphState::Waiting => Shown::Waiting,
        GlyphState::Yours => Shown::Yours,
        GlyphState::Idle => Shown::Idle,
    }
}

/// Who holds a task, with the live state of their pane in front of the name:
/// a card in progress whose agent is waiting for an answer reads as such
/// without leaving the board. An assignee no pane answers to is drawn idle. A
/// finished card has no glyph at all: the glyph only ever means what the agent
/// is doing now, and the work on that card is over.
pub(super) fn assignee_view<'a>(
    v: &View<'a>,
    assignee: &'a str,
    done: bool,
    in_dialog: bool,
) -> Element<'a, Message> {
    let t = v.t();
    let tr = v.ctx.i18n;
    let pane = v.pane(assignee);
    let name = pane.map_or_else(|| assignee.to_owned(), |pane| pane.name.clone());
    let ink = look::ink(if done { t.colors.fg_muted } else { t.colors.fg });

    let mut line = row![].spacing(space::SM).align_y(Alignment::Center);
    if !done {
        let state = pane.map_or(GlyphState::Idle, |pane| pane.state);
        line = line.push(glyph::state(t, glyph_of(state), 7.0));
    }
    line = line.push(
        text(name.clone())
            .size(font_size::CAPTION)
            .font(fonts::ui_medium())
            .style(ink)
            .wrapping(text::Wrapping::None),
    );
    if let Some(pane) = pane {
        line = line.push(
            text(format!("· {}", pane.agent))
                .size(font_size::CAPTION)
                .style(style::text::muted(t))
                .wrapping(text::Wrapping::None),
        );
    }

    match pane {
        // A button to the pane: its click is its own, and opens no task.
        Some(pane) => {
            let go = Msg::FocusAgent {
                ws: pane.workspace.clone(),
                pane: pane.id.clone(),
                from_dialog: in_dialog,
            };
            tip::over(
                t,
                button(line)
                    .padding([2.0, space::XS])
                    .style(look::fold_button(t))
                    .on_press(Message::Board(go)),
                tr.t_with(Key::BoardFocusAgent, &args!("name" => name.as_str()))
                    .into_owned(),
            )
        }
        None => tip::over(
            t,
            line,
            tr.t_with(Key::BoardAssigneeTitle, &args!("name" => assignee))
                .into_owned(),
        ),
    }
}
