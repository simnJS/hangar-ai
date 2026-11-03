//! P18: Control bus and agent ops.
//!
//! Ports lib/{control,managerControl,deliveryControl,deliveryRuns}.ts. Read
//! inventory-frontend.md §5/1, Appendix A6 and crates/hangar-app/PACKAGES.md.
//! Planned files of this directory (§8.1): mod, port, types, panes, preview,
//! kit_tabs, workspace, manager, delivery, delivery_text.
//!
//! This is the stub P0 handed over: the surface the shell calls, with
//! placeholder bodies. The directory is this package's own; replace the
//! bodies, add files, keep the signatures the shell calls.

use iced::{Subscription, Task};

use crate::message::{Intent, Message};
use crate::services::{ControlRequest, CoreEvent, Services, inbox::Answer};
use crate::shell::Region;
use crate::state::{Model, StoreOutcome};

/// The package's UI state; lives in `App.pkg.control`.
#[derive(Debug, Default)]
pub struct State;

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.control`.
/// Deliveries running, in flight and settled; what the snapshot's `delivering`
/// is made from.
#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::Control(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A control request from core (an agent's, a briefing, a phone's call).
    /// Before hydration the shell answers it itself and it never comes here.
    Request(ControlRequest),
    /// A model write from an engine (`AppPort::apply`), answered on the UI
    /// thread once applied.
    Model(ModelCmd, Answer<Result<StoreOutcome, String>>),
}

/// What an engine asks the model to do, on the UI thread
/// (`Msg::Model(cmd, reply)`, inventory A6: `AddPane{init}`, `ClosePane`,
/// `RespawnPane`, `UpdatePane`, `UpdateWorkspace`, `OpenWorkspace`, `Touch`,
/// `Activate`, `Detach`, `Reattach`, `Announce`, `CreateWorkspace`,
/// `RemoveWorkspace`, `BeginDelivery`, `EndDelivery`). P18 defines it.
#[derive(Debug, Clone)]
pub enum ModelCmd {}

/// Until P18 lands, an agent's request is answered at once rather than left
/// to time out.
const NOT_YET: &str = "pane control is not available in this build of Hangar yet";

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    match msg {
        Msg::Request(request) => {
            request.reply.send(Err(NOT_YET.into()));
            Task::none()
        }
        Msg::Model(cmd, _) => match cmd {},
    }
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
