//! P12: Board.
//!
//! Ports Board*.tsx, TaskDialog.tsx,
//! lib/{board,boardDrag,boardFormat,paneTask}.ts. Read inventory-frontend.md
//! §2.3/1 and crates/hangar-app/PACKAGES.md.
//!
//! What other packages read is [`Live`] (the boards cache, `cache.rs`) and the
//! pure helpers re-exported below; the views are filled in by the commits
//! that follow.

pub mod agent_pane;
pub mod cache;
pub mod drag;
pub mod format;
pub mod model;
pub mod pane_task;

use iced::{Element, Subscription, Task};

use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::{BoardVariant, Region, placeholder};
use crate::state::Model;

pub use cache::Live;
// `model::Task` is not re-exported here: this module already names iced's.
#[allow(unused_imports)]
pub use model::{Blocked, Column, TaskComment};

/// The package's UI state; lives in `App.pkg.board`.
/// Owns region `main` in the board view and region `dock`.
#[derive(Debug, Default)]
pub struct State;

/// The package's messages, routed by the shell as `Message::Board(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    match msg {}
}

pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    Subscription::none()
}

/// What takes the focus when F6 reaches region `Main`, which this package
/// owns.
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

/// Region `main` in the board view (`BoardVariant::Full`), or region `dock`
/// beside the terminals (`BoardVariant::Dock`).
pub fn view<'a>(st: &'a State, ctx: Ctx<'a>, variant: BoardVariant) -> Element<'a, Message> {
    placeholder::stub(ctx, "board::view")
}

/// `Overlay::Task`: a board task, new (`task` is `None`) or opened.
pub fn task_view<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    cwd: &'a str,
    task: Option<&'a str>,
) -> Element<'a, Message> {
    placeholder::stub_modal(ctx, "board::task_view")
}
