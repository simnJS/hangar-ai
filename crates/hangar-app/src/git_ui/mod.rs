//! P9: Diff view and worktrees.
//!
//! Ports `DiffView.tsx`, `WorktreePanel.tsx`, `lib/{diff,git,worktrees}.ts`
//! and `strings/worktrees.ts` (inventory-frontend.md §2.2). Two overlays:
//!
//! - `Overlay::Diff { ws, pane }`: what an agent changed in a folder, the
//!   files on the left and the unified diff of the selected one on the right
//!   (`diff_view`, with `diff_parse` for the text).
//! - `Overlay::Worktrees(ws)`: a workspace's git worktrees, one row each with
//!   what can be done to it, the cleanup of merged ones and the form that
//!   makes a new one (`worktrees/`).
//!
//! Each overlay's state is made when its intent arrives and lives until the
//! next one: a task still running for an earlier opening answers with that
//! opening's generation and is dropped. git itself is asked of hangar-core
//! (`hangar_core::git::*`, async), never on the window's thread.

mod diff_parse;
mod diff_view;
pub mod worktrees;

use iced::{Element, Subscription, Task};

use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::{Overlay, Region};
use crate::state::{Model, PaneId, WsId};

pub use diff_view::{DiffMsg, DiffState};
use worktrees::{Panel, WtMsg};

/// The package's UI state; lives in `App.pkg.git_ui`.
#[derive(Debug, Default)]
pub struct State {
    /// Counts the openings, so that an answer for an earlier one is told.
    generations: u64,
    /// The "Changes" overlay.
    diff: Option<DiffState>,
    /// The "Git worktrees" overlay.
    worktrees: Option<Panel>,
}

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.git_ui`. Nothing here is read by anyone else.
#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::GitUi(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The "Changes" overlay's own messages.
    Diff(DiffMsg),
    /// The overlay's cross, Escape or a click on the veil.
    CloseDiff,
    /// The worktree panel's own messages.
    Wt(WtMsg),
}

/// The "Changes" overlay's messages, as the router carries them.
fn wrap_diff(msg: DiffMsg) -> Message {
    Message::GitUi(Msg::Diff(msg))
}

/// Closes the "Changes" overlay when it is the one in front.
fn close_diff(m: &mut Model) {
    if matches!(m.ui.overlays.last(), Some(Overlay::Diff { .. })) {
        m.ui.overlays.pop();
    }
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    let _ = sv;
    match msg {
        Msg::Diff(msg) => match &mut st.diff {
            Some(diff) => diff.update(msg).map(wrap_diff),
            None => Task::none(),
        },
        Msg::CloseDiff => {
            close_diff(m);
            Task::none()
        }
        Msg::Wt(msg) => match &mut st.worktrees {
            Some(panel) => worktrees::update(panel, m, msg),
            None => Task::none(),
        },
    }
}

pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    let _ = (st, m);
    Subscription::none()
}

/// What takes the focus when F6 reaches a region this package owns. It owns
/// none, so the shell never calls it.
pub fn focus_entry(st: &State, m: &Model, region: Region) -> Option<Task<Message>> {
    let _ = (st, m, region);
    None
}

/// The folder and the name a diff of a workspace, or of one of its panes, is
/// about: the pane's own folder when it has one, and its name (the pane's
/// header names it); the workspace's folder and name otherwise.
fn diff_target(m: &Model, ws: &WsId, pane: Option<&PaneId>) -> Option<(String, String)> {
    let ws = m.workspace(ws)?;
    let pane = pane.and_then(|id| ws.panes.iter().find(|pane| &pane.id == id));
    Some(match pane {
        Some(pane) => (
            pane.cwd.clone().unwrap_or_else(|| ws.cwd.clone()),
            pane.name.clone(),
        ),
        None => (ws.cwd.clone(), ws.name.clone()),
    })
}

fn open_diff(st: &mut State, m: &Model, ws: &WsId, pane: Option<&PaneId>) -> Task<Message> {
    let Some((cwd, label)) = diff_target(m, ws, pane) else {
        st.diff = None;
        return Task::none();
    };
    st.generations += 1;
    let (state, first) = DiffState::open(cwd, Some(label), st.generations);
    st.diff = Some(state);
    first.map(wrap_diff)
}

fn open_worktrees(st: &mut State, m: &Model, ws: &WsId) -> Task<Message> {
    let Some(workspace) = m.workspace(ws) else {
        st.worktrees = None;
        return Task::none();
    };
    st.generations += 1;
    let (panel, first) = Panel::open(workspace.id.clone(), workspace.cwd.clone(), st.generations);
    st.worktrees = Some(panel);
    first
}

/// Every intent, after the shell's own handling of it: `Some` when this
/// package acted on it. The shell has put the overlay on its stack; this sets
/// its state up and starts reading.
pub fn on_intent(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    let _ = sv;
    match intent {
        Intent::OpenDiff { ws, pane } => Some(open_diff(st, m, ws, pane.as_ref())),
        Intent::OpenWorktrees(ws) => Some(open_worktrees(st, m, ws)),
        Intent::OpenOverlay(Overlay::Diff { ws, pane }) => {
            Some(open_diff(st, m, ws, pane.as_ref()))
        }
        Intent::OpenOverlay(Overlay::Worktrees(ws)) => Some(open_worktrees(st, m, ws)),
        _ => None,
    }
}

/// Every core event, after the shell's own handling of it. Git is not on the
/// bus: nothing here hears of a change, and a panel is read again when the
/// person asks (Refresh) or does something to it.
pub fn on_event(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    event: &CoreEvent,
) -> Option<Task<Message>> {
    let _ = (st, m, sv, event);
    None
}

/// `Overlay::Diff`: the changes of a workspace, or of one pane's worktree.
pub fn diff_view<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    ws: &'a WsId,
    pane: Option<&'a PaneId>,
) -> Element<'a, Message> {
    let _ = (ws, pane);
    match &st.diff {
        Some(diff) => diff_view::view(diff, ctx, wrap_diff, Message::GitUi(Msg::CloseDiff)),
        None => iced::widget::space().into(),
    }
}

/// `Overlay::Worktrees`: a workspace's git worktrees.
pub fn worktrees_view<'a>(st: &'a State, ctx: Ctx<'a>, ws: &'a WsId) -> Element<'a, Message> {
    let _ = ws;
    match &st.worktrees {
        Some(panel) => worktrees::view(panel, ctx),
        None => iced::widget::space().into(),
    }
}

#[cfg(test)]
mod snapshot;
#[cfg(test)]
mod tests;
