//! The kit's side of the ABI, in Rust terms: a [`Kit`] driven by inputs, its
//! views numbered and its handlers kept. [`export_kit!`](crate::export_kit)
//! puts one behind the module's exports; [`Harness`](crate::testing::Harness)
//! drives one in a test, on the host.

use std::collections::{HashMap, VecDeque};

use hangar_kit_ui::abi::{self, CallId, Input, LogLevel, MAX_TIMERS, Output, Start, View};
use hangar_kit_ui::{HandlerId, UiEvent};

use crate::Kit;
use crate::context::{Context, Pending};
use crate::handler::Handler;
use crate::subscriptions::Subscriptions;

/// How many views back an event may come from. Hangar draws each view as it
/// gets it, and a click on the one before can arrive after: eight is far
/// more than a person can click through between two frames.
const KEPT_VIEWS: usize = 8;

/// A message [`Context::after`](crate::Context::after) waits to send.
struct Timeout<M> {
    key: String,
    /// The timer's id on the wire: the key and a number of its own, so that
    /// asking again under a key is another timer, which starts from zero,
    /// where one of the same id and period would keep running.
    id: String,
    every_ms: u32,
    message: M,
}

pub struct Runtime<K: Kit> {
    kit: Option<K>,
    pending: HashMap<CallId, Pending<K::Message>>,
    next_call: CallId,
    views: VecDeque<(u32, Vec<Handler<K::Message>>)>,
    seq: u32,
    subscriptions: Subscriptions<K::Message>,
    timeouts: Vec<Timeout<K::Message>>,
    next_timeout: u64,
    utc_offset: i32,
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
            timeouts: Vec::new(),
            next_timeout: 0,
            utc_offset: 0,
        }
    }

    /// The kit, once started.
    pub fn kit(&self) -> Option<&K> {
        self.kit.as_ref()
    }

    pub fn init(&mut self, start: Start) -> Output {
        self.utc_offset = start.utc_offset;
        let mut cx = Context::new(self.next_call, false, self.utc_offset);
        self.kit = Some(K::init(&start, &mut cx));
        self.finish(cx)
    }

    pub fn update(&mut self, input: Input) -> Output {
        let gesture = matches!(input, Input::Ui(_) | Input::Shortcut(_));
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
            Input::Timer(timer) => match self
                .timeouts
                .iter()
                .position(|timeout| timeout.id == timer.id)
            {
                Some(at) => Some(self.timeouts.remove(at).message),
                None => self
                    .subscriptions
                    .timers
                    .iter()
                    .find(|(id, _, _)| *id == timer.id)
                    .map(|(_, _, message)| message.clone()),
            },
            Input::Shortcut(pressed) => self
                .subscriptions
                .shortcuts
                .iter()
                .find(|(keys, _)| *keys == pressed.keys)
                .map(|(_, message)| message.clone()),
            Input::Event(event) => self
                .subscriptions
                .events
                .iter()
                .find(|(name, _)| *name == event.name)
                .and_then(|(_, handler)| handler(event.data)),
        };
        let mut cx = Context::new(self.next_call, gesture, self.utc_offset);
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
        for key in cx.cancelled {
            self.timeouts.retain(|timeout| timeout.key != key);
        }
        for (key, every_ms, message) in cx.after {
            self.timeouts.retain(|timeout| timeout.key != key);
            self.next_timeout += 1;
            self.timeouts.push(Timeout {
                id: format!("~after:{key}:{}", self.next_timeout),
                key,
                every_ms,
                message,
            });
        }
        if let Some(kit) = self.kit.as_ref() {
            self.subscriptions = kit.subscriptions();
        }
        let mut subscriptions = self.subscriptions.wire();
        subscriptions
            .timers
            .extend(self.timeouts.iter().map(|timeout| abi::Timer {
                id: timeout.id.clone(),
                every_ms: timeout.every_ms,
            }));
        if subscriptions.timers.len() > MAX_TIMERS {
            crate::sys::log(
                LogLevel::Warn,
                &format!(
                    "{} timers asked for, of which Hangar runs {MAX_TIMERS}: the others never come",
                    subscriptions.timers.len()
                ),
            );
        }
        Output {
            calls,
            subscriptions: Some(subscriptions),
            operations: cx.operations,
        }
    }
}
