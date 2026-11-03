use std::time::Duration;

use serde_json::json;

use crate::prelude::*;
use crate::testing::{self, Harness, StartExt};
use crate::ui::EventValue;

struct Counter {
    count: u32,
    name: String,
    panes: Option<Result<Vec<api::Pane>, KitError>>,
}

#[derive(Clone, Debug)]
enum Message {
    Add,
    Name(String),
    Panes(Result<Vec<api::Pane>, KitError>),
    Tick,
}

impl Kit for Counter {
    type Message = Message;

    fn init(_start: &Start, cx: &mut Context<Message>) -> Self {
        cx.call(api::PanesList {}, Message::Panes);
        Counter {
            count: 0,
            name: String::new(),
            panes: None,
        }
    }

    fn update(&mut self, message: Message, cx: &mut Context<Message>) {
        match message {
            Message::Add => {
                self.count += 1;
                cx.copy(format!("{}", self.count));
            }
            Message::Name(name) => self.name = name,
            Message::Panes(panes) => self.panes = Some(panes),
            Message::Tick => self.count += 10,
        }
    }

    fn view(&self) -> Element<Message> {
        let panes: Element<Message> = match &self.panes {
            None => loading("Reading the panes…").into(),
            Some(Err(error)) if error.is(KitError::DENIED) => {
                denied("This tab may not list the panes").into()
            }
            Some(Err(error)) => failed(error.message.clone()).into(),
            Some(Ok(panes)) => list(panes.iter().map(|pane| item(pane.name.clone()))).into(),
        };
        page![
            header("Counter").state(state_label(
                Status::Success,
                format!("{} so far", self.count)
            )),
            text_input("Your name", &self.name).on_input(Message::Name),
            button("Add").primary().on_press(Message::Add),
            panes,
        ]
        .into()
    }

    fn subscriptions(&self) -> Subscriptions<Message> {
        Subscriptions::none().every("tick", Duration::from_secs(1), Message::Tick)
    }
}

#[test]
fn a_kit_starts_calls_and_draws_its_answers() {
    let mut tab = Harness::<Counter>::new(testing::start().grant(["panes.read"]));
    assert!(tab.shows("Reading the panes…"));
    assert_eq!(tab.methods(), vec!["panes.list"]);
    tab.reply_to("panes.list", Ok(json!([{ "name": "Ava", "agent": "claude", "role": null, "state": "idle", "canSend": true }])));
    assert!(tab.shows("Ava"));
    assert!(tab.calls().is_empty());
}

#[test]
fn a_refusal_is_a_state_of_the_page() {
    let mut tab = Harness::<Counter>::new(testing::start());
    tab.reply_to(
        "panes.list",
        Err(KitError::new(
            KitError::DENIED,
            "this tab was not granted panes.read",
        )),
    );
    assert!(tab.shows("This tab may not list the panes"));
}

#[test]
fn widgets_send_their_messages_back() {
    let mut tab = Harness::<Counter>::new(testing::start());
    tab.press("Add");
    tab.press("Add");
    assert_eq!(tab.kit().count, 2);
    assert!(tab.shows("2 so far"));
    assert_eq!(
        tab.calls()
            .iter()
            .filter(|call| call.method == "ui.copy")
            .count(),
        2
    );
    tab.type_into("Your name", "Zoé");
    assert_eq!(tab.kit().name, "Zoé");
}

#[test]
fn an_event_from_the_view_before_still_lands() {
    let mut tab = Harness::<Counter>::new(testing::start());
    let old = tab.view().seq;
    let mut add = None;
    tab.view().root.for_each(&mut |element| {
        if let crate::ui::Element::Button(button) = element {
            add = button.on_press;
        }
    });
    tab.press("Add");
    assert_ne!(tab.view().seq, old);
    // The press drawn from the older view, delivered after a newer one.
    tab.ui_from(old, add.unwrap(), None);
    assert_eq!(tab.kit().count, 2);
}

#[test]
fn timers_are_declared_and_come_round() {
    let mut tab = Harness::<Counter>::new(testing::start());
    assert_eq!(
        tab.subscriptions().timers,
        vec![crate::ui::abi::Timer {
            id: "tick".into(),
            every_ms: 1000
        }]
    );
    tab.timer("tick");
    assert_eq!(tab.kit().count, 10);
    tab.timer("unknown");
    assert_eq!(tab.kit().count, 10);
}

#[test]
fn a_value_of_another_kind_is_dropped() {
    let mut tab = Harness::<Counter>::new(testing::start());
    let mut input = None;
    tab.view().root.for_each(&mut |element| {
        if let crate::ui::Element::TextInput(field) = element {
            input = field.on_input;
        }
    });
    tab.ui(input.unwrap(), Some(EventValue::Bool(true)));
    assert_eq!(tab.kit().name, "");
}
