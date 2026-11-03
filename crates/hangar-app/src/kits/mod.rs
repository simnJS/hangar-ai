//! P14: Kits management.
//!
//! Ports KitActivation.tsx, settings/KitsSettings.tsx,
//! settings/KitsGallery.tsx,
//! lib/{kits,kitActivation,kitSetup,kitGallery,kitIcon}.ts,
//! strings/kitsManage.ts. Read inventory-frontend.md §2.4/1.1–1.3 and
//! crates/hangar-app/PACKAGES.md. Planned files of this directory (§8.1): mod,
//! model, activation, setup, gallery, icon,
//! views/{settings,gallery,activation,workspace_section}.
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

/// The package's UI state; lives in `App.pkg.kits`.
#[derive(Debug, Default)]
pub struct State;

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.kits`. The installed kits (`kits_list` + `CoreEvent::KitsChanged`).
#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::Kits(Msg)`.
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

/// `Overlay::KitActivation`: enabling a kit, on a workspace or everywhere.
pub fn activation_view<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    kit: &'a str,
    ws: Option<&'a WsId>,
) -> Element<'a, Message> {
    placeholder::stub_modal(ctx, "kits::activation_view")
}

/// The kits section of the workspace dialog and of `Overlay::KitsDialog`;
/// `ws` is `None` for a workspace being created.
pub fn workspace_section<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    ws: Option<&'a WsId>,
) -> Element<'a, Message> {
    placeholder::stub(ctx, "kits::workspace_section")
}

/// Settings › Kits (embedded by the settings).
pub fn settings_view<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    placeholder::stub(ctx, "kits::settings_view")
}
