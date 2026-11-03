//! P12: Board.
//!
//! Ports Board*.tsx, TaskDialog.tsx,
//! lib/{board,boardDrag,boardFormat,paneTask}.ts. Read inventory-frontend.md
//! §2.3/1 and crates/hangar-app/PACKAGES.md. Planned files of this directory
//! (§8.1): mod, model, cache, view, card, filter, task_dialog, dock, format,
//! drag.
//!
//! This is the stub P0 handed over: the surface the shell calls, with
//! placeholder bodies. The directory is this package's own; replace the
//! bodies, add files, keep the signatures the shell calls.

use iced::{Element, Subscription, Task};

use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::{BoardVariant, Region, placeholder};
use crate::state::Model;

/// The package's UI state; lives in `App.pkg.board`.
/// Owns region `main` in the board view and region `dock`.
#[derive(Debug, Default)]
pub struct State;

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.board`. The boards cache by folder (`board_load` + `CoreEvent::BoardChanged`), which
/// P6, P9 and P19 read.
#[derive(Debug, Default)]
pub struct Live;

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
