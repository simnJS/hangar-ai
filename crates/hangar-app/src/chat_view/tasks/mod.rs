//! The tasks panel of the chat view: the right column, at a fixed width. The
//! workspace's board as a list, with its progress:
//!
//! ```text
//! Tasks                       64 %
//! ▓▓▓▓▓▓▓▓▓░░░░░  Done 1/4
//! NEEDS YOU                          agents in a dialog, questions,
//!   Ava · waits for your answer      deliveries ready to land
//!   Search page          [Merge]
//! IN PROGRESS
//!   Search page · Finn
//!   ▓▓▓▓▓▓░░░ 60 %  index built · 3 min ago
//! TO DO …
//! ```
//!
//! The vocabulary is the board's own (its column names, « Blocked: … »), and
//! so are its sources: the board package's shared cache (`m.live.board`), the
//! panes' activity, and the needs-you rules for questions and deliveries.
//! Landing a delivery goes through the needs-you package, which keeps the
//! landing state and its failure for both lists.
//!
//! What the panel shows is worked out when what it reads changes, not on each
//! frame ([`model::Cache`]).

pub mod model;
mod view;

use std::collections::HashSet;

use iced::{Element, Subscription, Task};

use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::services::CoreEvent;
use crate::state::{Model, PaneId, StoreAction, Workspace, WsId};

/// What the region keeps between frames.
#[derive(Debug, Default)]
pub struct State {
    /// The tasks whose row is open on its description and last comment.
    expanded: HashSet<String>,
    cache: model::Cache,
}

impl State {
    pub fn is_expanded(&self, task: &str) -> bool {
        self.expanded.contains(task)
    }
}

/// What the region answers.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A row was clicked: open or close it.
    Toggle(String),
    /// Go to a pane (an agent in a dialog, the orchestrator to answer): the
    /// panes come back, the pane focused.
    GoTo { ws: WsId, pane: PaneId },
}

pub(crate) fn msg(msg: Msg) -> Message {
    Message::ChatView(super::Msg::Tasks(msg))
}

pub fn update(st: &mut State, m: &mut Model, msg: Msg) -> Task<Message> {
    match msg {
        Msg::Toggle(task) => {
            if !st.expanded.remove(&task) {
                st.expanded.insert(task);
            }
            Task::none()
        }
        Msg::GoTo { ws, pane } => {
            // The pane is drawn by the grid only: it shows again first.
            m.apply(StoreAction::SetChatView {
                ws: ws.clone(),
                on: false,
            });
            Task::done(Message::Intent(Intent::FocusPane {
                ws,
                pane,
                terminal: true,
            }))
        }
    }
}

/// Nothing to listen to: the panel follows the board package's cache, which
/// reads a board again on its own when core says it changed.
#[allow(dead_code)] // wired in chat_view/mod.rs by the orchestrator
pub fn subscription(_st: &State, _m: &Model) -> Subscription<Message> {
    Subscription::none()
}

/// Nothing either: a new read of the board is a new `Arc` in the cache,
/// which the next frame sees ([`model::Cache`]); an event comes before it.
#[allow(dead_code)] // wired in chat_view/mod.rs by the orchestrator
pub fn on_event(_st: &mut State, _m: &mut Model, _event: &CoreEvent) -> Option<Task<Message>> {
    None
}

pub fn view<'a>(st: &'a State, ctx: Ctx<'a>, ws: &'a Workspace) -> Element<'a, Message> {
    let panel = st.cache.panel(ctx.model, ws);
    view::panel(st, ctx, ws, &panel)
}

#[cfg(test)]
mod tests;
