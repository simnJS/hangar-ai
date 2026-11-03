//! P4: Pane logic and live stores.
//!
//! Ports lib/{agentState,paneInsight,bridge,activity,hibernate,hibernateStore,u
//! seHibernation,agents,agentPane,context,shells,paneBranch,fileDrop}.ts,
//! strings/paneBranch.ts. Read inventory-frontend.md §4/1.5–1.7, 1.10, 1.13,
//! §4/4 and crates/hangar-app/PACKAGES.md. Planned files of this directory
//! (§8.1): mod, agent_state, pane_insight, bridge_events, activity, hibernate,
//! hibernation, agents, session_watch, agent_pane, context_gauge, shells,
//! pane_branch, file_drop.
//!
//! This is the stub P0 handed over: the surface the shell calls, with
//! placeholder bodies. The directory is this package's own; replace the
//! bodies, add files, keep the signatures the shell calls.

use iced::{Subscription, Task};

use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::Region;
use crate::state::{GlyphState, Model, PaneId};

/// The package's UI state; lives in `App.pkg.panes_logic`.
#[derive(Debug, Default)]
pub struct State;

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.panes_logic`. Activity per pane, plan limits, insight per pane, hibernation (`touched`, `rest`,
/// `asleep`), branch cache: the shared value types are in `state::live`.
#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::PanesLogic(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    match msg {}
}

pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    Subscription::none()
}

/// What takes the focus when F6 reaches a region this package owns. It owns
/// none, so the shell never calls it.
pub fn focus_entry(st: &State, m: &Model, region: Region) -> Option<Task<Message>> {
    None
}

/// Every intent, after the shell's own handling of it: `Some` when this
/// package acted on it.
pub fn on_intent(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    None
}

/// Every core event, after the shell's own handling of it.
pub fn on_event(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    event: &CoreEvent,
) -> Option<Task<Message>> {
    None
}

/// The glyph a pane shows: its shell's life until it runs, then its agent's
/// activity (`paneGlyphState`). Used by everything that draws a glyph.
pub fn state_glyph_state(m: &Model, pane: &PaneId) -> GlyphState {
    GlyphState::Idle
}
