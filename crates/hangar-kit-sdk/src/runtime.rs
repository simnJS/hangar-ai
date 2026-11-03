//! The kit's side of the ABI, in Rust terms: a [`Kit`] driven by inputs, its
//! views numbered and its handlers kept. [`export_kit!`](crate::export_kit)
//! puts one behind the module's exports; [`Harness`](crate::testing::Harness)
//! drives one in a test, on the host.

use std::collections::{HashMap, VecDeque};

use hangar_kit_ui::abi::{CallId, Input, LogLevel, Output, Start, View};
use hangar_kit_ui::{HandlerId, UiEvent};

use crate::Kit;
use crate::context::{Context, Pending};
use crate::handler::Handler;
use crate::subscriptions::Subscriptions;

/// How many views back an event may come from. Hangar draws each view as it
/// gets it, and a click on the one before can arrive after: eight is far
/// more than a person can click through between two frames.
const KEPT_VIEWS: usize = 8;

pub struct Runtime<K: Kit> {
    kit: Option<K>,
    pending: HashMap<CallId, Pending<K::Message>>,
    next_call: CallId,
    views: VecDeque<(u32, Vec<Handler<K::Message>>)>,
    seq: u32,
    subscriptions: Subscriptions<K::Message>,
}

impl<K: Kit> Default for Runtime<K> {
    fn default() -> Self {
        Runtime::new()
    }
}

impl<K: Kit> Runtime<K> {
    pub fn new() -> Self {
        Runtime {
            kit: None,
            pending: HashMap::new(),
            next_call: 1,
            views: VecDeque::new(),
            seq: 0,
            subscriptions: Subscriptions::none(),
        }
    }

    /// The kit, once started.
    pub fn kit(&self) -> Option<&K> {
        self.kit.as_ref()
    }

    pub fn init(&mut self, start: Start) -> Output {
        let mut cx = Context::new(self.next_call, false);
        self.kit = Some(K::init(&start, &mut cx));
        self.finish(cx)
    }

    pub fn update(&mut self, input: Input) -> Output {
        let gesture = matches!(input, Input::Ui(_));
        let message = match input {
            Input::Ui(event) => self.resolve(event),
            Input::Reply(reply) => match self.pending.remove(&reply.id) {
                Some(pending) => pending(reply.outcome.into()),
                None => {
                    crate::sys::log(
                        LogLevel::Debug,
                        &format!("an answer to no call: {}", reply.id),
                    );
                    None
                }
            },
            Input::Timer(timer) => self
                .subscriptions
                .timers
                .iter()
                .find(|(id, _, _)| *id == timer.id)
                .map(|(_, _, message)| message.clone()),
            Input::Event(event) => self
                .subscriptions
                .events
                .iter()
                .find(|(name, _)| *name == event.name)
                .and_then(|(_, handler)| handler(event.data)),
        };
        let mut cx = Context::new(self.next_call, gesture);
        if let (Some(kit), Some(message)) = (self.kit.as_mut(), message) {
            kit.update(message, &mut cx);
        }
        self.finish(cx)
    }

    /// The page as it is now, its handlers numbered and kept for the events
    /// that will name them.
    pub fn view(&mut self) -> View {
        let Some(kit) = self.kit.as_ref() else {
            return View {
                seq: self.seq,
                root: hangar_kit_ui::Element::Column(hangar_kit_ui::Column::new(Vec::new())),
            };
        };
        let mut handlers = Vec::new();
        let root = kit.view().map_handlers(&mut |handler| {
            handlers.push(handler);
            HandlerId(handlers.len() as u32 - 1)
        });
        self.seq = self.seq.wrapping_add(1);
        self.views.push_back((self.seq, handlers));
        while self.views.len() > KEPT_VIEWS {
            self.views.pop_front();
        }
        View {
            seq: self.seq,
            root,
        }
    }

    fn resolve(&self, event: UiEvent) -> Option<K::Message> {
        let Some((_, handlers)) = self.views.iter().find(|(seq, _)| *seq == event.view) else {
            crate::sys::log(
                LogLevel::Debug,
                &format!("an event of view {}, which is gone", event.view),
            );
            return None;
        };
        handlers.get(event.handler.0 as usize)?.resolve(event.value)
    }

    fn finish(&mut self, cx: Context<K::Message>) -> Output {
        self.next_call = cx.next_id();
        let mut calls = Vec::with_capacity(cx.calls.len());
        for (call, pending) in cx.calls {
            self.pending.insert(call.id, pending);
            calls.push(call);
        }
        if let Some(kit) = self.kit.as_ref() {
            self.subscriptions = kit.subscriptions();
        }
        Output {
            calls,
            subscriptions: Some(self.subscriptions.wire()),
        }
    }
}
