//! P17: 3D viewer (app side).
//!
//! Ports ModelPanel.tsx 3D parts, lib/modelCapture.ts (crate hangar-model3d for
//! the renderer). Read inventory-frontend.md §2.4/1.7, §2.4/3 and
//! crates/hangar-app/PACKAGES.md. Planned files of this directory (§8.1): mod,
//! toolbar, animation_bar, info_strip, capture.
//!
//! This is the stub P0 handed over: the surface the shell calls, with
//! placeholder bodies. The directory is this package's own; replace the
//! bodies, add files, keep the signatures the shell calls.

use iced::{Element, Subscription, Task};

use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::{Region, placeholder};
use crate::state::{Model, WsId};

/// The package's UI state; lives in `App.pkg.model3d_view`.
#[derive(Debug, Default)]
pub struct State;

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.model3d_view`.
#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::Model3d(Msg)`.
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

/// A 3D model in the preview panel's body (called by `preview::view_entry`).
pub fn view<'a>(st: &'a State, ctx: Ctx<'a>, ws: &'a WsId, entry: &'a str) -> Element<'a, Message> {
    placeholder::stub(ctx, "model3d_view::view")
}
