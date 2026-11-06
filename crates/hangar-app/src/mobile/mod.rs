//! P20: Hangar Mobile, the window's side.
//!
//! The phone app stays a web app (`src/mobile`), served by core's mobile
//! server (`hangar_core::remote`) from the embedded `dist-mobile/`. Each call
//! a phone makes (`POST /m/api/call/<op>`) is relayed by that server to the
//! window as the internal control request `remote_<op>`, which P18's bus
//! hands to the handler this package installs (`Hook::Mobile`). The window's
//! half of the contract (inventory §5/2) is here:
//!
//! - `engine`: the state the TS kept in module variables (`Mobile`), the
//!   dispatch of the `remote_*` operations, and `Sources`, what they read
//!   beyond P18's port;
//! - `ops`: state, screen, send, permission, manager, wake, history,
//!   workspace, and typing as the person (`type`, `hold`, `wake`);
//! - `chat_live` and `calls`: a pane's conversation, with what its mod
//!   reports before the transcript has it;
//! - `answer`: answering an AskUserQuestion through its dialog;
//! - `desk`: the same calls typed, for the window's own chat view;
//! - `delivery`: a delivery that waits for the person, landed or sent back;
//! - `apps`: the agents' dev servers the phone may open;
//! - `stats`: the statistics, summed up the page's way;
//! - `push` and `activity_log`: the notifications pushed to the phones, and
//!   the history kept on disk;
//! - `contract`: the JSON of the answers (`src/remote/types.ts`);
//! - `live`: all of it over the running app.
//!
//! The timeouts hold together: the phone waits 12 s for the server, the
//! server 10 s for the window (core's `CALL_TIMEOUT`), and the window answers
//! within 8 s for a pane to start (`ops::OPEN_WAIT_MS`), 7 s for a landing
//! (`delivery::LAND_WAIT_MS`) and 6 s for a statistics scan (P11's
//! `cache::REMOTE_WAIT`).

pub mod activity_log;
pub mod answer;
pub mod apps;
pub mod calls;
pub mod chat_live;
pub mod contract;
pub mod delivery;
pub mod desk;
pub mod engine;
mod live;
pub mod ops;
pub mod push;
pub mod stats;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use iced::futures::stream;
use iced::{Subscription, Task};

use crate::control::port::Hook;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::state::Model;
use crate::state::live::BridgeEvent;

/// The package's UI state; lives in `App.pkg.mobile`: the runtime, once
/// started.
#[derive(Debug, Default)]
pub struct State {
    runtime: Option<live::Runtime>,
}

impl State {
    /// Hangar Mobile's runtime state, for `desk`'s calls: `None` until the
    /// state is in and the orchestration runtime runs (`Msg::Start`).
    #[allow(dead_code)] // The chat view (chat_view/) calls it; nothing does yet.
    pub fn mobile(&self) -> Option<Arc<engine::Mobile>> {
        self.runtime.as_ref().map(|runtime| runtime.mobile.clone())
    }
}

/// The package's shared read-model; lives in `Model.live.mobile`. Nothing of
/// Hangar Mobile is drawn in the window but its settings (P13), which ask
/// core directly.
#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::Mobile(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The state is in and the orchestration runtime runs: Hangar Mobile's
    /// side starts.
    Start,
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    match msg {
        Msg::Start => {
            if st.runtime.is_some() || !m.live.hydrated {
                return Task::none();
            }
            // The push sits beside the manager feed, and the history reads
            // the journals: both are P19's runtime's.
            let Some(handle) = m.live.orchestration.handle().cloned() else {
                return Task::none();
            };
            let runtime = live::Runtime::start(sv.clone(), handle);
            let hook = Hook::Mobile(Arc::new(engine::Hook(runtime.mobile.clone())));
            st.runtime = Some(runtime);
            Task::done(Message::Control(crate::control::Msg::Install(hook)))
        }
    }
}

/// Once hydrated, with P19's runtime up, one message starts the package;
/// the subscription is gone as soon as it has.
pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    if st.runtime.is_none() && m.live.hydrated && m.live.orchestration.handle().is_some() {
        Subscription::run(|| stream::once(async { Message::Mobile(Msg::Start) }))
    } else {
        Subscription::none()
    }
}

/// Every intent, after the shell's own handling of it. None is this
/// package's.
pub fn on_intent(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    let _ = (st, m, sv, intent);
    None
}

/// The core events Hangar Mobile follows: the mods' reports (calls running,
/// dialogs, the plan), a phone paired or revoked, a file shared, a board or
/// the memory written.
pub fn on_event(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    event: &CoreEvent,
) -> Option<Task<Message>> {
    let _ = (m, sv);
    let runtime = st.runtime.as_ref()?;
    match event {
        CoreEvent::Bridge { pane, event } => {
            // A kind this version does not know (a newer mod) is ignored, as
            // the TS ignored it.
            if let Some(event) = BridgeEvent::parse(event) {
                runtime.bridge(pane, &event);
            }
        }
        CoreEvent::RemoteChanged => runtime.remote_changed(),
        CoreEvent::RemoteShared(share) => runtime.shared(share),
        CoreEvent::BoardChanged { .. } | CoreEvent::MemoryChanged => runtime.projects_changed(),
        _ => {}
    }
    None
}
