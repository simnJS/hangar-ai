//! P20: Hangar Mobile, window side.
//!
//! Ports lib/{remoteControl,remoteAnswer,remoteDelivery,remoteApps,remotePush,a
//! ctivityLog}.ts, src/remote/*. Read inventory-frontend.md §5/1.8–1.9, §5/2,
//! Appendix A6 and crates/hangar-app/PACKAGES.md. Planned files of this
//! directory (§8.1): mod, ops, chat_live, answer, delivery, apps, stats, push,
//! activity_log.
//!
//! This is the stub P0 handed over: the surface the shell calls, with
//! placeholder bodies. The directory is this package's own; replace the
//! bodies, add files, keep the signatures the shell calls.

use iced::{Subscription, Task};

use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::Region;
use crate::state::Model;

/// The package's UI state; lives in `App.pkg.mobile`.
#[derive(Debug, Default)]
pub struct State;

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.mobile`.
#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::Mobile(Msg)`.
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
