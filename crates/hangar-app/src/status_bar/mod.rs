//! P6: Status bar (`StatusBar.tsx`).
//!
//! The bar along the bottom of the workspace: the broadcast field while the
//! terminals are on screen, the folder and how its agents are doing otherwise,
//! and on the right what is true of the whole workspace (its git branch, the
//! dictation keys, how many panes).
//!
//! The git branch is read again every 20 seconds while the window has the
//! focus, and as soon as the workspace or the focus changes: a branch changed
//! in another program is the likeliest thing to have moved meanwhile. A window
//! nobody is looking at asks nothing at all: one git process every 20 seconds
//! for a bar nobody can see is not nothing.

pub mod tail;

use std::collections::HashSet;
use std::time::Duration;

use iced::widget::{Id, Space, container, responsive, row, text, text_input};
use iced::{Alignment, Element, Event, Length, Padding, Subscription, Task, window};

use hangar_core::git::RepoInfo;
use hangar_i18n::{Key, args};
use hangar_theme::tokens::{font_size, space};
use hangar_theme::{fonts, style};
use hangar_ui::{Icon, button, icon, keys, tip};

use crate::commands::CommandId;
use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::navigator::{reads, skin};
use crate::services::{CoreEvent, Services};
use crate::shell::{Focus, Region, View};
use crate::state::live::BridgeEvent;
use crate::state::{AgentActivity, Model, PaneId};

/// Often enough to follow an agent switching branches, rarely enough to cost
/// nothing.
const GIT_REFRESH: Duration = Duration::from_secs(20);

/// The broadcast field, which the `view.broadcast` shortcut focuses.
fn input_id() -> Id {
    Id::new("hangar-status-broadcast")
}

/// What the bar shows of the repository the workspace folder is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    /// `None` when the head is detached.
    pub branch: Option<String>,
    /// Short sha.
    pub head: Option<String>,
    pub root: Option<String>,
    pub is_linked_worktree: bool,
}

impl Repo {
    /// The repository, or `None` when the folder is in none, or git is not
    /// installed.
    pub fn of(info: RepoInfo) -> Option<Repo> {
        (info.installed && info.is_repo).then_some(Repo {
            branch: info.branch,
            head: info.head,
            root: info.root,
            is_linked_worktree: info.is_linked_worktree,
        })
    }
}

/// The package's UI state; lives in `App.pkg.status_bar`.
#[derive(Debug)]
pub struct State {
    /// The folder `repo` was read for, so an answer for the workspace shown
    /// before is not taken for this one's.
    repo_cwd: String,
    repo: Option<Repo>,
    /// The window has the system's focus; only then is git asked.
    focused: bool,
    /// Panes whose agent has the hangar-bridge mod, which can hold a line
    /// until it is free (`useBridgedCount`).
    bridged: HashSet<PaneId>,
}

impl Default for State {
    fn default() -> Self {
        State {
            repo_cwd: String::new(),
            repo: None,
            focused: true,
            bridged: HashSet::new(),
        }
    }
}

impl State {
    /// A repository read in; the same branch, head and root keep the object
    /// that was there. Returns whether anything changed.
    fn take_repo(&mut self, cwd: &str, repo: Option<Repo>) -> bool {
        if self.repo_cwd == cwd && self.repo == repo {
            return false;
        }
        self.repo_cwd = cwd.to_string();
        self.repo = repo;
        true
    }

    /// A report of a pane's mod: the pane has one, until it says it ended.
    fn note_bridge(&mut self, pane: &PaneId, report: &serde_json::Value) {
        match BridgeEvent::parse(report) {
            Some(BridgeEvent::End { .. }) => {
                self.bridged.remove(pane);
            }
            Some(_) => {
                self.bridged.insert(pane.clone());
            }
            None => {}
        }
    }

    /// How many of `panes` can hold a line until their agent is free.
    fn bridged_among(&self, panes: &[crate::state::Pane]) -> usize {
        panes
            .iter()
            .filter(|pane| self.bridged.contains(&pane.id))
            .count()
    }
}

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.status_bar`.
#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::StatusBar(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The broadcast field was typed in (`Model.ui.broadcast`).
    Input(String),
    /// Enter sends; Alt+Enter, and the *When free* button, hold the line until
    /// each agent is done.
    Send {
        when_free: bool,
    },
    /// Time to ask git about this folder.
    Poll(String),
    Repo(String, Option<Repo>),
    /// The window gained or lost the system's focus.
    Focus(bool),
}

pub fn update(st: &mut State, m: &mut Model, _sv: &Services, msg: Msg) -> Task<Message> {
    match msg {
        Msg::Input(value) => {
            m.ui.broadcast = value;
            Task::none()
        }
        Msg::Send { when_free } => {
            let text = m.ui.broadcast.trim().to_string();
            let Some(ws) = m.active_workspace().map(|ws| ws.id.clone()) else {
                return Task::none();
            };
            if text.is_empty() {
                return Task::none();
            }
            m.ui.broadcast.clear();
            Task::done(Message::Intent(Intent::Broadcast {
                ws,
                text,
                when_free,
            }))
        }
        Msg::Poll(cwd) => {
            Task::perform(hangar_core::git::git_repo_info(cwd.clone()), move |info| {
                Message::StatusBar(Msg::Repo(cwd.clone(), Repo::of(info)))
            })
        }
        Msg::Repo(cwd, repo) => {
            st.take_repo(&cwd, repo);
            Task::none()
        }
        Msg::Focus(on) => {
            st.focused = on;
            Task::none()
        }
    }
}

/// The git beat of the workspace on screen (an immediate read, then every 20
/// seconds), and the window's focus. The beat is a subscription of its own for
/// each folder and each focus, so a change of workspace restarts it, which is
/// the read the TS did when `cwd` changed.
pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    let focus = iced::event::listen_with(|event, _status, _window| match event {
        Event::Window(window::Event::Focused) => Some(Message::StatusBar(Msg::Focus(true))),
        Event::Window(window::Event::Unfocused) => Some(Message::StatusBar(Msg::Focus(false))),
        _ => None,
    });
    let Some(ws) = m.active_workspace().filter(|_| st.focused) else {
        return focus;
    };
    Subscription::batch([focus, Subscription::run_with(ws.cwd.clone(), beat)])
}

// `Subscription::run_with` hands the builder a reference to its data, which
// is the `String` the subscription is identified by.
#[allow(clippy::ptr_arg)]
fn beat(cwd: &String) -> impl iced::futures::Stream<Item = Message> + use<> {
    let cwd = cwd.clone();
    iced::futures::stream::unfold(true, move |first| {
        let cwd = cwd.clone();
        async move {
            if !first {
                tokio::time::sleep(GIT_REFRESH).await;
            }
            Some((Message::StatusBar(Msg::Poll(cwd)), false))
        }
    })
}

/// F6 reaches the broadcast field, which is only there over the terminals.
pub fn focus_entry(_st: &State, m: &Model, _region: Region) -> Option<Task<Message>> {
    (m.ui.view == View::Terminals).then(|| iced::widget::operation::focus(input_id()))
}

/// A dictation lands in the broadcast field when that is where the keyboard
/// is, joined to what is already typed.
pub fn on_intent(
    _st: &mut State,
    m: &mut Model,
    _sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    let Intent::InsertTranscript(words) = intent else {
        return None;
    };
    if m.ui.focus != Some(Focus::Region(Region::Status)) || m.ui.view != View::Terminals {
        return None;
    }
    if m.ui.broadcast.is_empty() {
        m.ui.broadcast = words.clone();
    } else {
        m.ui.broadcast = format!("{} {words}", m.ui.broadcast);
    }
    Some(Task::none())
}

/// What the hangar-bridge mod says of a pane tells the bar it can hold a line
/// for that pane; its end says it no longer can.
pub fn on_event(
    st: &mut State,
    _m: &mut Model,
    _sv: &Services,
    event: &CoreEvent,
) -> Option<Task<Message>> {
    if let CoreEvent::Bridge { pane, event } = event {
        st.note_bridge(pane, event);
    }
    None
}

/// Region `status`: the broadcast input and the bar's readouts.
pub fn view<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    let Some(ws) = ctx.model.active_workspace() else {
        return Space::new().into();
    };
    let t = ctx.theme;
    let tr = ctx.i18n;
    let pane_count = ws.panes.len();

    let lead: Element<'a, Message> = if ctx.model.ui.view == View::Terminals {
        broadcast(st, ctx, ws)
    } else {
        info(ctx, ws)
    };

    // The repository, the dictation keys, the panes: what is true of the whole
    // workspace, mono and quiet.
    let mut items = row![].spacing(2).align_y(Alignment::Center);
    if st.repo_cwd == ws.cwd
        && let Some(repo) = &st.repo
    {
        let root = repo.root.as_deref().unwrap_or(&ws.cwd);
        let hint = match &repo.branch {
            Some(branch) => tr.t_with(
                Key::StatusBranch,
                &args!("branch" => branch.as_str(), "root" => root),
            ),
            None => tr.t_with(
                Key::StatusDetached,
                &args!("head" => repo.head.as_deref().unwrap_or("?"), "root" => root),
            ),
        };
        let name = repo
            .branch
            .as_deref()
            .or(repo.head.as_deref())
            .unwrap_or("");
        let mut item = row![
            icon(Icon::Branch, 13.0),
            text(name.to_string())
                .size(font_size::LABEL)
                .font(fonts::mono()),
        ]
        .spacing(5)
        .align_y(Alignment::Center);
        if repo.is_linked_worktree {
            item = item.push(
                container(
                    text(tr.t(Key::StatusWorktree).into_owned())
                        .size(font_size::MICRO)
                        .font(fonts::mono()),
                )
                .padding([0.0, 4.0])
                .style(style::container::tag(t)),
            );
        }
        items = items.push(tip::over(t, readout(t, item), hint.into_owned()));
    }
    let dictate = if ctx.model.settings().voice_enabled {
        reads::binding(&ctx, CommandId::VoiceDictate)
    } else {
        String::new()
    };
    if !dictate.is_empty() {
        let shown = hangar_ui::shortcut::format_binding(&dictate);
        let item = row![
            icon(Icon::Mic, 13.0),
            text(shown.clone())
                .size(font_size::LABEL)
                .font(fonts::mono()),
        ]
        .spacing(5)
        .align_y(Alignment::Center);
        items = items.push(tip::over(
            t,
            readout(t, item),
            tr.t_with(Key::StatusDictate, &args!("keys" => shown))
                .into_owned(),
        ));
    }
    let panes = row![
        icon(Icon::Layout4, 13.0),
        text(tr.tn(Key::StatusPanes, pane_count, &[]).into_owned())
            .size(font_size::LABEL)
            .font(fonts::mono()),
    ]
    .spacing(5)
    .align_y(Alignment::Center);
    items = items.push(tip::over(
        t,
        readout(t, panes),
        tr.t(Key::StatusPanesHint).into_owned(),
    ));

    let right = row![
        container(Space::new())
            .width(1)
            .height(18)
            .style(move |_: &iced::Theme| container::Style {
                background: Some(iced::Background::Color(t.colors.line)),
                ..container::Style::default()
            }),
        items,
    ]
    .spacing(space::SM)
    .align_y(Alignment::Center);

    row![lead, right]
        .spacing(space::LG)
        .padding(Padding {
            top: 0.0,
            right: space::MD,
            bottom: 0.0,
            left: space::LG,
        })
        .height(Length::Fill)
        .align_y(Alignment::Center)
        .into()
}

/// A readout of the right-hand side: padded, muted.
fn readout<'a>(
    t: &'a hangar_theme::Theme,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    container(content)
        .height(24)
        .padding([0.0, 7.0])
        .align_y(Alignment::Center)
        .style(move |_: &iced::Theme| container::Style {
            text_color: Some(t.colors.fg_secondary),
            ..container::Style::default()
        })
        .into()
}

/// The broadcast field and its two buttons.
fn broadcast<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    ws: &'a crate::state::Workspace,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let tr = ctx.i18n;
    let value = ctx.model.ui.broadcast.as_str();
    let empty = value.trim().is_empty();
    let placeholder = tr
        .t_with(Key::BroadcastPlaceholder, &args!("n" => ws.panes.len()))
        .into_owned();
    let hint = reads::binding(&ctx, CommandId::ViewBroadcast);

    let input = text_input("", value)
        .id(input_id())
        .size(font_size::UI)
        .padding(0)
        .width(Length::Fill)
        .on_input(|value| Message::StatusBar(Msg::Input(value)))
        .on_submit(Message::StatusBar(Msg::Send { when_free: false }))
        .style(style::text_input::bare(t));
    // Alt+Enter holds the line until each agent is done. The key is spent here
    // so the field's own Enter does not also send it at once.
    let input = keys::on_key(input, |key, modifiers| match key {
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter) if modifiers.alt() => {
            Some(Message::StatusBar(Msg::Send { when_free: true }))
        }
        _ => None,
    });
    // The placeholder is drawn by hand, in the secondary grey the stylesheet
    // gave it, since the field's own is a step fainter.
    let shown: Element<'a, Message> = if value.is_empty() {
        iced::widget::stack![
            input,
            container(
                text(placeholder)
                    .size(font_size::UI)
                    .style(style::text::secondary(t))
            )
            .align_y(Alignment::Center)
            .height(Length::Fill)
        ]
        .into()
    } else {
        input
    };

    let mut field = row![shown].align_y(Alignment::Center).spacing(space::SM);
    if !hint.is_empty() {
        field = field.push(tip::over(
            t,
            hangar_ui::text::mono(t, hangar_ui::shortcut::format_binding(&hint))
                .size(font_size::LABEL),
            tr.t(Key::CmdViewBroadcast).into_owned(),
        ));
    }
    let field = container(field)
        .height(24)
        .padding([0.0, space::MD])
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .style(skin::field(t, false));

    let when_free = button::secondary(t, tr.t(Key::BroadcastWhenFree).into_owned())
        .icon(Icon::Clock)
        .on_press_maybe((!empty).then_some(Message::StatusBar(Msg::Send { when_free: true })))
        .tip(
            tr.t_with(
                Key::BroadcastWhenFreeHint,
                &args!("n" => st.bridged_among(&ws.panes)),
            )
            .into_owned(),
        );
    let send = button::primary(t, tr.t(Key::BroadcastSend).into_owned())
        .icon(Icon::Send)
        .on_press_maybe((!empty).then_some(Message::StatusBar(Msg::Send { when_free: false })))
        .tip(tr.t(Key::StatusSendHint).into_owned());

    row![
        container(icon(Icon::Broadcast, 16.0).color(t.colors.fg_muted)).padding([0.0, 2.0]),
        field,
        when_free,
        send,
    ]
    .spacing(space::SM)
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .into()
}

/// The folder, and how the workspace's agents are doing, for every view but
/// the terminals.
fn info<'a>(ctx: Ctx<'a>, ws: &'a crate::state::Workspace) -> Element<'a, Message> {
    let t = ctx.theme;
    let tr = ctx.i18n;
    let counts = reads::counts(ctx.model, ws);
    // Same order as the navigator's row: what needs you first.
    let (kind, summary) = if counts.waiting > 0 {
        (
            Some(AgentActivity::Waiting),
            tr.tn(Key::SidebarActivityWaiting, counts.waiting, &[]),
        )
    } else if counts.yours > 0 {
        (
            Some(AgentActivity::Yours),
            tr.tn(Key::SidebarActivityYours, counts.yours, &[]),
        )
    } else if counts.working > 0 {
        (
            Some(AgentActivity::Working),
            tr.tn(Key::SidebarActivityWorking, counts.working, &[]),
        )
    } else {
        (None, tr.t(Key::ToolbarActivityQuiet))
    };
    let summary = match kind {
        Some(kind) => text(summary.into_owned())
            .size(font_size::UI)
            .style(skin::state_text(t, kind)),
        None => text(summary.into_owned())
            .size(font_size::UI)
            .style(style::text::secondary(t)),
    };

    let cwd = ws.cwd.as_str();
    let path = responsive(move |bounds| {
        let shown = tail::fit(cwd, bounds.width, |s| {
            tail::path_width(s, font_size::CAPTION)
        });
        text(shown)
            .size(font_size::CAPTION)
            .font(fonts::mono())
            .wrapping(text::Wrapping::None)
            .style(style::text::secondary(t))
            .into()
    });
    let path = tip::over(
        t,
        container(path).width(Length::Fill).height(18),
        cwd.to_string(),
    );

    row![
        icon(Icon::Folder, 15.0).color(t.colors.fg_muted),
        path,
        text("·").size(font_size::UI).style(style::text::subtle(t)),
        summary,
    ]
    .spacing(space::SM)
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .into()
}

#[cfg(test)]
mod interaction;
#[cfg(test)]
mod tests;
