//! Driving a kit in a test, on the host, with no WebAssembly and no Hangar:
//! the same [`Runtime`] the module runs, fed by hand.
//!
//! ```ignore
//! let mut tab = Harness::<Hello>::new(testing::start().grant(["panes.read"]));
//! tab.reply_to("panes.list", Ok(json!([])));
//! assert!(tab.shows("No pane yet"));
//! tab.press("Refresh");
//! assert_eq!(tab.calls()[0].method, "panes.list");
//! ```

use std::collections::BTreeMap;

use serde_json::Value;

use hangar_kit_ui::abi::{
    self, Call, CallId, HangarEvent, Input, KitInfo, Reply, Start, TabInfo, TimerFired, View,
};
use hangar_kit_ui::api::{KitError, Theme};
use hangar_kit_ui::{Element, EventValue, HandlerId, UiEvent};

use crate::Kit;
use crate::runtime::Runtime;

/// A [`Start`] for a test: kit `test`, tab `tab`, no params, no permission.
pub fn start() -> Start {
    Start {
        abi: abi::ABI_VERSION,
        hangar: "0.0.0-test".to_owned(),
        kit: KitInfo {
            id: "test".to_owned(),
            name: "Test".to_owned(),
            version: "0.0.0".to_owned(),
        },
        tab: TabInfo {
            id: "tab".to_owned(),
            params: BTreeMap::new(),
            caller: None,
        },
        theme: Theme::default(),
        permissions: Vec::new(),
        visible: true,
    }
}

/// Setting up a test [`Start`].
pub trait StartExt {
    /// The permissions the tab holds.
    fn grant<S: Into<String>>(self, permissions: impl IntoIterator<Item = S>) -> Self;
    fn param(self, name: impl Into<String>, value: impl Into<String>) -> Self;
}

impl StartExt for Start {
    fn grant<S: Into<String>>(mut self, permissions: impl IntoIterator<Item = S>) -> Self {
        self.permissions
            .extend(permissions.into_iter().map(Into::into));
        self
    }

    fn param(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.tab.params.insert(name.into(), value.into());
        self
    }
}

/// Sets the clock [`Context::now`](crate::Context::now) reads, in this
/// thread; `None` gives the system's back.
pub fn set_now(ms: Option<i64>) {
    crate::sys::host::NOW.with(|now| now.set(ms));
}

/// What the kit logged in this thread, emptied.
pub fn take_logs() -> Vec<(abi::LogLevel, String)> {
    crate::sys::host::LOGGED.with(|logged| std::mem::take(&mut *logged.borrow_mut()))
}

/// A kit, started, its calls held until the test answers them.
pub struct Harness<K: Kit> {
    runtime: Runtime<K>,
    calls: Vec<Call>,
    subscriptions: abi::Subscriptions,
    view: View,
}

impl<K: Kit> Harness<K> {
    /// Runs `init`, then `view`.
    pub fn new(start: Start) -> Self {
        let mut runtime = Runtime::new();
        let output = runtime.init(start);
        let view = runtime.view();
        let mut harness = Harness {
            runtime,
            calls: Vec::new(),
            subscriptions: abi::Subscriptions::default(),
            view,
        };
        harness.absorb(output);
        harness
    }

    pub fn kit(&self) -> &K {
        self.runtime.kit().expect("started")
    }

    /// The page as it is now.
    pub fn view(&self) -> &View {
        &self.view
    }

    /// The calls not answered yet, oldest first.
    pub fn calls(&self) -> &[Call] {
        &self.calls
    }

    pub fn subscriptions(&self) -> &abi::Subscriptions {
        &self.subscriptions
    }

    /// Answers the call `id`.
    pub fn reply(&mut self, id: CallId, answer: Result<Value, KitError>) {
        self.calls.retain(|call| call.id != id);
        self.send(Input::Reply(Reply {
            id,
            outcome: answer.into(),
        }));
    }

    /// Answers the oldest call to `method`, and gives it.
    pub fn reply_to(&mut self, method: &str, answer: Result<Value, KitError>) -> Call {
        let call = self
            .calls
            .iter()
            .find(|call| call.method == method)
            .cloned()
            .unwrap_or_else(|| panic!("no call to {method} waits; waiting: {:?}", self.methods()));
        self.reply(call.id, answer);
        call
    }

    /// The methods of the calls waiting.
    pub fn methods(&self) -> Vec<&str> {
        self.calls.iter().map(|call| call.method.as_str()).collect()
    }

    pub fn timer(&mut self, id: &str) {
        self.send(Input::Timer(TimerFired { id: id.to_owned() }));
    }

    pub fn event(&mut self, name: &str, data: Value) {
        self.send(Input::Event(HangarEvent {
            name: name.to_owned(),
            data,
        }));
    }

    /// Uses a widget by its handler in the current view.
    pub fn ui(&mut self, handler: HandlerId, value: Option<EventValue>) {
        let view = self.view.seq;
        self.send(Input::Ui(UiEvent {
            view,
            handler,
            value,
        }));
    }

    /// Uses a widget of an earlier view: what a click drawn from a view
    /// Hangar has replaced since does.
    pub fn ui_from(&mut self, view: u32, handler: HandlerId, value: Option<EventValue>) {
        self.send(Input::Ui(UiEvent {
            view,
            handler,
            value,
        }));
    }

    /// Presses the button, link or row whose label is `label`.
    pub fn press(&mut self, label: &str) {
        let handler = self
            .find(|element| match element {
                Element::Button(button)
                    if button.label.as_deref() == Some(label)
                        || button.tooltip.as_deref() == Some(label) =>
                {
                    button.on_press
                }
                Element::Link(link) if link.label == label => link.on_press,
                Element::Filter(filter) if filter.label == label => filter.on_press,
                Element::List(list) => list
                    .items
                    .iter()
                    .find(|item| item.title == label)
                    .and_then(|item| item.on_press),
                _ => None,
            })
            .unwrap_or_else(|| panic!("nothing to press is labelled {label:?}"));
        self.ui(handler, None);
    }

    /// Types `text` into the field whose placeholder or label is `name`.
    pub fn type_into(&mut self, name: &str, text: &str) {
        let handler = self
            .find(|element| match element {
                Element::TextInput(input)
                    if input.placeholder == name || input.label.as_deref() == Some(name) =>
                {
                    input.on_input
                }
                Element::Search(search) if search.placeholder == name => search.on_input,
                Element::TextArea(area)
                    if area.id == name || area.label.as_deref() == Some(name) =>
                {
                    area.on_input
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("no field is named {name:?}"));
        self.ui(handler, Some(EventValue::Text(text.to_owned())));
    }

    /// Whether some text of the page holds `needle`.
    pub fn shows(&self, needle: &str) -> bool {
        self.texts().iter().any(|text| text.contains(needle))
    }

    /// Every text of the page, in order: what a person would read.
    pub fn texts(&self) -> Vec<String> {
        let mut texts = Vec::new();
        self.view.root.for_each(&mut |element| match element {
            Element::Text(text) => texts.push(text.text.clone()),
            Element::Spans(spans) => {
                texts.push(spans.spans.iter().map(|span| span.text.as_str()).collect())
            }
            Element::Header(header) => texts.push(header.title.clone()),
            Element::Section(section) => texts.push(section.title.clone()),
            Element::Button(button) => texts.extend(button.label.clone()),
            Element::Link(link) => texts.push(link.label.clone()),
            Element::List(list) => {
                for item in &list.items {
                    texts.push(item.title.clone());
                    texts.extend(item.sub.clone());
                }
            }
            Element::Empty(empty) => {
                texts.push(empty.title.clone());
                texts.extend(empty.text.clone());
            }
            Element::Banner(banner) => texts.push(banner.text.clone()),
            Element::State(state) => texts.push(state.label.clone()),
            Element::Chip(chip) => texts.push(chip.label.clone()),
            Element::Pre(pre) => texts.push(pre.text.clone()),
            Element::KeyValue(pairs) => {
                texts.extend(pairs.pairs.iter().map(|pair| pair.key.clone()))
            }
            Element::Table(table) => {
                texts.extend(table.columns.iter().map(|column| column.title.clone()))
            }
            _ => {}
        });
        texts
    }

    fn find(&self, mut matches: impl FnMut(&Element) -> Option<HandlerId>) -> Option<HandlerId> {
        let mut found = None;
        self.view.root.for_each(&mut |element| {
            if found.is_none() {
                found = matches(element);
            }
        });
        found
    }

    fn send(&mut self, input: Input) {
        let output = self.runtime.update(input);
        self.absorb(output);
        self.view = self.runtime.view();
    }

    fn absorb(&mut self, output: abi::Output) {
        self.calls.extend(output.calls);
        if let Some(subscriptions) = output.subscriptions {
            self.subscriptions = subscriptions;
        }
    }
}
