//! P7: Keymap and the shortcut dispatcher.
//!
//! Ports lib/{keys,shortcuts,useShortcuts}.ts. Read inventory-frontend.md
//! §1.10, §2.1/0 and crates/hangar-app/PACKAGES.md. Planned files of this
//! directory (§8.1): mod, chord, keymap, dispatch.
//!
//! This is the stub P0 handed over: the surface the shell calls, with
//! placeholder bodies. The directory is this package's own; replace the
//! bodies, add files, keep the signatures the shell calls.

use std::sync::Arc;

use iced::{Subscription, Task};

use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::Region;
use crate::state::Model;

/// The package's UI state; lives in `App.pkg.keymap`.
/// The dispatcher listens to keys itself (its own `subscription`).
#[derive(Debug, Default)]
pub struct State;

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.keymap`.
/// `index` is read by the shell for `Ctx::keymap` and, later, by hangar-term's
/// key filter: keep that field's name and type.
#[derive(Debug, Default)]
pub struct Live {
    pub index: Arc<KeymapIndex>,
}

/// The package's messages, routed by the shell as `Message::Keymap(Msg)`.
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

/// The resolved keymap: every command's bindings, defaults filled in, and the
/// lookup the dispatcher and the terminals' key filter use. P0 only names
/// the type; P7 defines it.
#[derive(Debug, Default)]
pub struct KeymapIndex;
