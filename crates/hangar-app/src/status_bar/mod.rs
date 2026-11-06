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
//!
//! Every window has a bar of its own, about its own workspace: the line typed
//! in one is not the other's, and a broadcast reaches the panes of the
//! workspace its window draws, never the main window's active one.

pub mod tail;

use std::collections::{BTreeSet, HashMap, HashSet};
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
use crate::state::{AgentActivity, Model, WsId};

/// Often enough to follow an agent switching branches, rarely enough to cost
/// nothing.
const GIT_REFRESH: Duration = Duration::from_secs(20);

/// The broadcast field of a window, which the `view.broadcast` shortcut
/// focuses: one per window, so that focusing it in one never lands in another.
fn input_id(window: window::Id) -> Id {
    Id::from(format!("hangar-status-broadcast-{window:?}"))
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
#[derive(Debug, Default)]
pub struct State {
    /// What git said of each folder a bar was asked about, so an answer for
    /// the workspace shown before is not taken for this one's, and two
    /// windows on two folders each read their own. `None`: no repository.
    repos: HashMap<String, Option<Repo>>,
    /// The windows that lost the system's focus. Git is only asked for a
    /// window that has it; one that never said it lost it counts as having
    /// it, as every window does at start.
    unfocused: HashSet<window::Id>,
    /// What each window's broadcast field holds. Not persisted, and not shared
    /// between windows: each one's App had its own (`useState("")`). This is
    /// the only copy: a send carries the text in `Intent::Broadcast`, and
    /// nothing else (the palette's `view.broadcast`, the phone's ops) reads
    /// the field. A window that closes takes its line with it.
    broadcast: HashMap<window::Id, String>,
}

impl State {
    /// A repository read in; the same branch, head and root keep the object
    /// that was there. Returns whether anything changed.
    fn take_repo(&mut self, cwd: &str, repo: Option<Repo>) -> bool {
        if self.repos.get(cwd) == Some(&repo) {
            return false;
        }
        self.repos.insert(cwd.to_string(), repo);
        true
    }

    /// What the window's broadcast field holds.
    pub fn broadcast_text(&self, window: window::Id) -> &str {
        self.broadcast.get(&window).map_or("", String::as_str)
    }

    /// A window closed: its line and its focus are nobody's any more.
    fn forget(&mut self, window: window::Id) {
        self.unfocused.remove(&window);
        self.broadcast.remove(&window);
    }

    fn set_broadcast_text(&mut self, window: window::Id, text: String) {
        if text.is_empty() {
            self.broadcast.remove(&window);
        } else {
            self.broadcast.insert(window, text);
        }
    }
}

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.status_bar`.
#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::StatusBar(Msg)`.
///
/// A message names the window it came from, and a send the workspace that
/// window drew: when it is handled, the focused window or the main window's
/// active workspace may be others.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The broadcast field of a window was typed in.
    Input(window::Id, String),
    /// Enter sends; Alt+Enter, and the *When free* button, hold the line until
    /// each agent is done.
    Send {
        window: window::Id,
        ws: WsId,
        when_free: bool,
    },
    /// Time to ask git about this folder.
    Poll(String),
    Repo(String, Option<Repo>),
    /// A window gained or lost the system's focus.
    Focus(window::Id, bool),
    /// A window closed: what it held is let go.
    Closed(window::Id),
}

/// What a send comes to: the line the window holds, for the workspace that
/// window drew when the button was pressed, which must still exist. The field
/// empties; a line with nothing in it, or for a workspace that is gone, sends
/// nothing and leaves the field as it is.
fn send(
    st: &mut State,
    m: &Model,
    window: window::Id,
    ws: WsId,
    when_free: bool,
) -> Option<Intent> {
    let text = st.broadcast_text(window).trim().to_string();
    if text.is_empty() || m.workspace(&ws).is_none() {
        return None;
    }
    st.set_broadcast_text(window, String::new());
    Some(Intent::Broadcast {
        ws,
        text,
        when_free,
    })
}

pub fn update(st: &mut State, m: &mut Model, _sv: &Services, msg: Msg) -> Task<Message> {
    match msg {
        Msg::Input(window, value) => {
            st.set_broadcast_text(window, value);
            Task::none()
        }
        Msg::Send {
            window,
            ws,
            when_free,
        } => match send(st, m, window, ws, when_free) {
            Some(intent) => Task::done(Message::Intent(intent)),
            None => Task::none(),
        },
        Msg::Poll(cwd) => {
            Task::perform(hangar_core::git::git_repo_info(cwd.clone()), move |info| {
                Message::StatusBar(Msg::Repo(cwd.clone(), Repo::of(info)))
            })
        }
        Msg::Repo(cwd, repo) => {
            st.take_repo(&cwd, repo);
            Task::none()
        }
        Msg::Focus(window, on) => {
            if on {
                st.unfocused.remove(&window);
            } else {
                st.unfocused.insert(window);
            }
            Task::none()
        }
        Msg::Closed(window) => {
            st.forget(window);
            Task::none()
        }
    }
}

/// The git beat of the workspace of each window that has the focus (an
/// immediate read, then every 20 seconds), and the windows' focus. The beat is
/// a subscription of its own for each folder, so a change of workspace
/// restarts it, which is the read the TS did when `cwd` changed. Two windows
/// on one folder share a beat.
pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    let focus = iced::event::listen_with(|event, _status, window| match event {
        Event::Window(window::Event::Focused) => Some(Message::StatusBar(Msg::Focus(window, true))),
        Event::Window(window::Event::Unfocused) => {
            Some(Message::StatusBar(Msg::Focus(window, false)))
        }
        Event::Window(window::Event::Closed) => Some(Message::StatusBar(Msg::Closed(window))),
        _ => None,
    });
    Subscription::batch(
        std::iter::once(focus).chain(
            beats(st, m)
                .into_iter()
                .map(|cwd| Subscription::run_with(cwd, beat)),
        ),
    )
}

/// The folders to ask git about: those of the workspaces the windows with the
/// system's focus draw, each once.
fn beats(st: &State, m: &Model) -> BTreeSet<String> {
    m.live
        .windows
        .keys()
        .filter(|window| !st.unfocused.contains(window))
        .filter_map(|window| m.window_workspace(*window))
        .map(|ws| ws.cwd.clone())
        .collect()
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

/// F6 reaches the broadcast field of the window the person is in, which is
/// only there over the terminals.
pub fn focus_entry(_st: &State, m: &Model, _region: Region) -> Option<Task<Message>> {
    let window = m.focused_window()?;
    let chat = m
        .window_workspace(window)
        .is_some_and(|ws| crate::chat_view::shown(m, ws));
    (m.view_in(window) == View::Terminals && !chat)
        .then(|| iced::widget::operation::focus(input_id(window)))
}

/// A dictation lands in the broadcast field when that is where the keyboard
/// is, joined to what is already typed.
pub fn on_intent(
    st: &mut State,
    m: &mut Model,
    _sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    let Intent::InsertTranscript(words) = intent else {
        return None;
    };
    let window = m.focused_window()?;
    if m.ui.focus != Some(Focus::Region(Region::Status)) || m.view_in(window) != View::Terminals {
        return None;
    }
    let typed = st.broadcast_text(window);
    let joined = if typed.is_empty() {
        words.clone()
    } else {
        format!("{typed} {words}")
    };
    st.set_broadcast_text(window, joined);
    Some(Task::none())
}

/// Nothing in the core's events is the bar's: which panes can hold a line
/// until their agent is free is the pane logic's (`bridged_count`).
pub fn on_event(
    _st: &mut State,
    _m: &mut Model,
    _sv: &Services,
    _event: &CoreEvent,
) -> Option<Task<Message>> {
    None
}

/// Region `status`: the broadcast input and the bar's readouts.
pub fn view<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    // The workspace this window draws: a workspace window's own, never the
    // main window's active one.
    let Some(ws) = ctx.workspace() else {
        return Space::new().into();
    };
    let t = ctx.theme;
    let tr = ctx.i18n;
    let pane_count = ws.panes.len();

    // The chat view has its own input: the broadcast field would type into
    // terminals that are not on screen.
    let chat = crate::chat_view::shown(ctx.model, ws);
    let lead: Element<'a, Message> = if ctx.view() == View::Terminals && !chat {
        broadcast(st, ctx, ws)
    } else {
        info(ctx, ws)
    };

    // The repository, the dictation keys, the panes: what is true of the whole
    // workspace, mono and quiet.
    let mut items = row![].spacing(2).align_y(Alignment::Center);
    if let Some(Some(repo)) = st.repos.get(&ws.cwd) {
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
    let value = st.broadcast_text(ctx.window);
    let empty = value.trim().is_empty();
    let send = |when_free: bool| {
        Message::StatusBar(Msg::Send {
            window: ctx.window,
            ws: ws.id.clone(),
            when_free,
        })
    };
    let placeholder = tr
        .t_with(Key::BroadcastPlaceholder, &args!("n" => ws.panes.len()))
        .into_owned();
    let hint = reads::binding(&ctx, CommandId::ViewBroadcast);

    let window = ctx.window;
    let input = text_input("", value)
        .id(input_id(window))
        .size(font_size::UI)
        .padding(0)
        .width(Length::Fill)
        .on_input(move |value| Message::StatusBar(Msg::Input(window, value)))
        .on_submit(send(false))
        .style(style::text_input::bare(t));
    // Alt+Enter holds the line until each agent is done. The key is spent here
    // so the field's own Enter does not also send it at once.
    let held = send(true);
    let input = keys::on_field_key(input, move |key, modifiers| match key {
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter) if modifiers.alt() => {
            Some(held.clone())
        }
        _ => None,
    });
    // The keymap goes first: Ctrl+Shift+Enter typed in the field splits the
    // pane behind it, and must not also send the line.
    let input = crate::keymap::guard(ctx, input);
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

    // The panes whose Claude Code loaded the mod, which can hold a line until
    // it is free (`useBridgedCount`).
    let bridged = ctx
        .model
        .live
        .panes_logic
        .bridged_count(ws.panes.iter().map(|pane| &pane.id));
    let when_free = button::secondary(t, tr.t(Key::BroadcastWhenFree).into_owned())
        .icon(Icon::Clock)
        .on_press_maybe((!empty).then(|| send(true)))
        .tip(
            tr.t_with(Key::BroadcastWhenFreeHint, &args!("n" => bridged))
                .into_owned(),
        );
    let now = button::primary(t, tr.t(Key::BroadcastSend).into_owned())
        .icon(Icon::Send)
        .on_press_maybe((!empty).then(|| send(false)))
        .tip(tr.t(Key::StatusSendHint).into_owned());

    row![
        container(icon(Icon::Broadcast, 16.0).color(t.colors.fg_muted)).padding([0.0, 2.0]),
        field,
        when_free,
        now,
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
