//! The column itself (`Inspector.tsx`): the head, then the sections of the
//! focused pane, with the grip on its left edge; or, with no pane in focus,
//! a quiet note.

use super::files_model::Indexed;
use hangar_i18n::Key;
use hangar_theme::tokens::{BORDER_WIDTH, font_size};
use hangar_theme::{AgentState, fonts, style};
use hangar_ui::Icon;
use hangar_ui::button::icon_button;
use iced::widget::{column, container, row, rule, scrollable, space, stack, text};
use iced::{Alignment, Element, Length, Padding};

use super::header::{self, Head};
use super::sections::{self, Scene};
use super::sources::TaskCard;
use super::widgets::{GripEvents, grip};
use super::{Msg, State, files, format, sources};
use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::live::{PaneInsight, RateLimit};
use crate::state::types::{AgentId, Pane, Workspace};

/// Region `inspector`: everything about the focused pane.
pub fn view<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    let m = ctx.model;
    let t = ctx.theme;
    // The workspace this window draws: a window of its own shows its own, not
    // whichever the main window has active.
    let content: Element<'a, Message> = match ctx.workspace() {
        Some(ws) => match sources::focused_pane(m, ws) {
            Some((pane, index)) => pane_panel(st, ctx, ws, pane, index),
            None => no_pane(ctx, ws.id.clone()),
        },
        None => no_pane(ctx, String::new()),
    };

    // The widest a drag may make the column in this window now, as the frame
    // placed it: read when the grip is pressed, as the TS did.
    let max = ctx.panels().inspector.max;
    let edge = grip(
        t,
        GripEvents {
            pressed: Box::new(move |x| Message::Inspector(Msg::GripPressed { x, max })),
            moved: Box::new(|x| Message::Inspector(Msg::GripMoved(x))),
            released: Message::Inspector(Msg::GripReleased),
            cancelled: Message::Inspector(Msg::GripCancelled),
            reset: Message::Inspector(Msg::ResetWidth),
        },
    );
    // The grip lies over the column's left edge, not beside it.
    stack![
        content,
        row![edge, space::horizontal()].height(Length::Fill)
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// Everything the sections show of a pane, read from the model and the
/// other packages; the column is drawn from this alone, which is what lets a
/// test draw it with whatever a pane could have reported.
pub(super) struct Data<'a> {
    pub insight: &'a PaneInsight,
    pub state: AgentState,
    /// The state comes from the hangar-bridge mod rather than from a guess.
    pub bridged: bool,
    pub limits: Option<&'a [RateLimit]>,
    pub task: Option<TaskCard>,
    /// A shell pane's shell.
    pub shell: Option<String>,
    pub changes: Option<&'a Indexed>,
    /// 1-based place in reading order.
    pub index: usize,
}

/// Everything about one pane: the head, then the sections.
fn pane_panel<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    ws: &'a Workspace,
    pane: &'a Pane,
    index: usize,
) -> Element<'a, Message> {
    let m = ctx.model;
    let settings = m.settings();
    let scene = scene(ctx, ws, pane);
    let activity = sources::activity(m, &pane.id);
    let folder: &str = pane.cwd.as_deref().unwrap_or(&ws.cwd);
    let data = Data {
        insight: sources::insight(m, &pane.id),
        state: activity
            .map(|activity| AgentState::from(activity.activity))
            .unwrap_or(AgentState::Idle),
        bridged: activity.is_some_and(|activity| activity.bridged),
        limits: sources::plan_limits(m).map(|plan| plan.limits.as_slice()),
        task: sources::task_for_pane(m, ws, &pane.id),
        shell: crate::panes_logic::shells::resolve_shell(
            &m.ui.shells,
            &[
                pane.shell_id.as_deref(),
                ws.shell_id.as_deref(),
                settings.shell_id.as_deref(),
            ],
        )
        .map(|found| found.label.clone()),
        changes: st.changes.get(folder),
        index,
    };
    column_of(&scene, data)
}

/// The scene a pane's sections are drawn in.
pub(super) fn scene<'a>(ctx: Ctx<'a>, ws: &'a Workspace, pane: &'a Pane) -> Scene<'a> {
    let settings = ctx.model.settings();
    Scene {
        ctx,
        ws,
        pane,
        now: sources::now_ms(),
        folded: &settings.inspector_collapsed,
        // What the frame drew the column at: the saved width, cut to the room
        // this window has.
        width: ctx.panels().inspector.width,
    }
}

/// The head over the scrolling sections.
pub(super) fn column_of<'a>(scene: &Scene<'a>, data: Data<'a>) -> Element<'a, Message> {
    let (ctx, ws, pane) = (scene.ctx, scene.ws, scene.pane);
    let (t, tr) = (ctx.theme, ctx.i18n);
    let insight = data.insight;
    let agent = pane.agent.info();
    let is_shell = pane.agent == AgentId::Shell;
    let claude = pane.agent == AgentId::Claude;
    let folder: &str = pane.cwd.as_deref().unwrap_or(&ws.cwd);

    // Read for Claude Code only: a shell gets the activity section once
    // something happened in it, so never empty.
    let nothing_yet = tr
        .t(if data.bridged {
            Key::InspectorActivityQuiet
        } else {
            Key::InspectorActivityEmpty
        })
        .into_owned();

    let mut sections = Vec::new();
    sections.push(if is_shell {
        sections::terminal(scene, data.shell.as_deref(), folder)
    } else {
        sections::session(scene, insight, folder)
    });
    if !is_shell {
        sections.push(sections::context(scene, insight, agent.label, data.limits));
    }
    sections.push(sections::task(scene, data.task.as_ref()));
    // Prompts are queued through the hangar-bridge mod, which Claude Code
    // loads; a shell only shows a queue it has.
    if claude || !insight.queue.is_empty() {
        sections.push(sections::queue(scene, &insight.queue));
    }
    sections.push(files::section(scene, insight, folder, data.changes));
    // A shell has no one to report on it; it gets the section only once
    // something did.
    if !is_shell || !insight.events.is_empty() {
        sections.push(sections::activity(scene, &insight.events, nothing_yet));
    }
    if let Some(last) = sections.pop() {
        sections.push(last.last());
    }

    let head = Head {
        state: data.state,
        agent_label: agent.label,
        is_shell,
        version: insight.agent_version.as_deref(),
        model: format::pretty_model(insight.model.as_deref()),
        shell: data.shell,
        index: data.index,
    };
    column![
        header::view(scene, head),
        rule::horizontal(BORDER_WIDTH).style(style::rule::hairline(t)),
        scrollable(column(sections.into_iter().map(Element::from)).width(Length::Fill))
            .height(Length::Fill)
            .style(style::scrollable::default(t)),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// No pane in focus (`.insp-none`): the close cross, a mark where the
/// monogram would be, and what to do.
fn no_pane<'a>(ctx: Ctx<'a>, ws: String) -> Element<'a, Message> {
    let t = ctx.theme;
    let tr = ctx.i18n;
    let edge = t.colors.line_strong;
    let mark = container(space::horizontal())
        .width(30.0)
        .height(30.0)
        .style(move |_: &iced::Theme| container::Style {
            border: iced::Border {
                color: edge,
                width: 1.0,
                radius: 7.0.into(),
            },
            ..container::Style::default()
        });
    let note = column![
        mark,
        text(tr.t(Key::InspectorNoPane).into_owned())
            .size(font_size::UI)
            .font(fonts::ui_semibold())
            .style(style::text::muted(t)),
        container(
            text(tr.t(Key::InspectorNoPaneBody).into_owned())
                .size(font_size::CAPTION_LG)
                .style(style::text::subtle(t))
                .align_x(iced::alignment::Horizontal::Center),
        )
        .max_width(200.0),
    ]
    .spacing(6)
    .align_x(Alignment::Center);
    let close = icon_button(t, Icon::Close)
        .side(20.0)
        .icon_size(12.0)
        .tip(tr.t(Key::InspectorClose).into_owned())
        .on_press(Message::Inspector(Msg::Close(ws, None)));
    stack![
        container(note).center(Length::Fill).padding(Padding {
            bottom: 80.0,
            ..Padding::new(24.0)
        }),
        container(close)
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Right)
            .padding(10.0),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
