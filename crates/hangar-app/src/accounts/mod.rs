//! P8: Claude accounts.
//!
//! Ports components/accounts/*, lib/{accounts,accountSwitch}.ts. Read
//! inventory-frontend.md §2.1/13 and crates/hangar-app/PACKAGES.md. Planned
//! files of this directory (§8.1): mod, model, switch, usage, switcher, badge,
//! picker, pane_items, settings_page, menu_items.
//!
//! This is the stub P0 handed over: the surface the shell calls, with
//! placeholder bodies. The directory is this package's own; replace the
//! bodies, add files, keep the signatures the shell calls.

use iced::{Element, Subscription, Task};

use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::{MenuEntry, Region, placeholder};
use crate::state::{Model, PaneId, WsId};

/// The package's UI state; lives in `App.pkg.accounts`.
#[derive(Debug, Default)]
pub struct State;

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.accounts`. Nothing yet.
#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::Accounts(Msg)`.
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

/// The toolbar's account switcher (embedded by the toolbar).
pub fn switcher<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    placeholder::stub_inline(ctx, "accounts")
}

/// A pane header's account badge.
pub fn pane_badge<'a>(ctx: Ctx<'a>, ws: &'a WsId, pane: &'a PaneId) -> Element<'a, Message> {
    placeholder::stub_inline(ctx, "account")
}

/// A navigator row's account badge.
pub fn badge<'a>(ctx: Ctx<'a>, ws: &'a WsId) -> Element<'a, Message> {
    placeholder::stub_inline(ctx, "account")
}

/// The account entries of the navigator's and the pane's menus.
pub fn menu_items<'a>(ctx: Ctx<'a>, ws: &'a WsId, pane: Option<&'a PaneId>) -> Vec<MenuEntry> {
    Vec::new()
}

/// The workspace dialog's account picker.
pub fn workspace_picker<'a>(ctx: Ctx<'a>, value: Option<&'a str>) -> Element<'a, Message> {
    placeholder::stub_inline(ctx, "accounts::workspace_picker")
}

/// Settings › Accounts (embedded by the settings).
pub fn settings_page<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    placeholder::stub(ctx, "accounts::settings_page")
}
