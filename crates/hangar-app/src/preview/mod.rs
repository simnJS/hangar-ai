//! P15: Preview panel and 2D viewers.
//!
//! Ports ModelPanel.tsx panel parts, components/preview/* except KitTabView,
//! lib/{preview,modelStore,modelHost,modelFiles}.ts. Read inventory-frontend.md
//! §2.4/1.5–1.6, §2.4/3 and crates/hangar-app/PACKAGES.md. Planned files of
//! this directory (§8.1): mod, store, model, panel, tabs, file_view, image,
//! text, code_lines, highlight, markdown, media, pdf, html, parts, file_watch,
//! webview.
//!
//! This is the stub P0 handed over: the surface the shell calls, with
//! placeholder bodies. The directory is this package's own; replace the
//! bodies, add files, keep the signatures the shell calls.

use std::sync::Arc;

use iced::{Element, Subscription, Task};

use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, PreviewAccess, Services, preview::NoPreview};
use crate::shell::{PanelMode, Region, placeholder};
use crate::state::{Model, WsId};

/// The package's UI state; lives in `App.pkg.preview`.
#[derive(Debug, Default)]
pub struct State;

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.preview`. The preview of each workspace: entries, the active one, panel open, expanded,
/// unseen.
#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::Preview(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    match msg {}
}

pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    Subscription::none()
}

/// What takes the focus when F6 reaches region `Model`, which this package
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

/// Region `model`: the preview panel, docked, as a drawer, or expanded.
pub fn view_panel<'a>(st: &'a State, ctx: Ctx<'a>, mode: PanelMode) -> Element<'a, Message> {
    placeholder::stub(ctx, "preview::view_panel")
}

/// The body of the panel for one entry: a file viewer, or a slot for
/// `model3d_view::view` and `kit_tabs::view`.
pub fn view_entry<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    ws: &'a WsId,
    entry: &'a str,
) -> Element<'a, Message> {
    placeholder::stub(ctx, "preview::view_entry")
}

/// What the engines may do with the previews (`Services::preview`). The shell
/// asks once, at boot, after `State::default()`.
pub fn preview_access(st: &State) -> Arc<dyn PreviewAccess> {
    Arc::new(NoPreview)
}
