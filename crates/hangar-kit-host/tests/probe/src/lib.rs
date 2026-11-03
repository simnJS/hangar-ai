//! A tab that does what its params say, for the host's tests: `mode` makes
//! it loop, allocate or panic while starting; `calls` lists methods it calls
//! at once, each answer shown as `method: ok` or `method: <code>`. Its
//! buttons loop, panic or copy from an update, where a person's click is.

use std::hint::black_box;
use std::time::Duration;

use serde_json::{Value, json};

use hangar_kit_sdk::prelude::*;

pub struct Probe {
    answers: Vec<String>,
    events: u32,
    ticks: u32,
}

#[derive(Clone, Debug)]
pub enum Message {
    Answer(String, Result<Value, KitError>),
    Event,
    Tick,
    Copy,
    Spin,
    Hog,
    Panic,
}

/// Params a method takes, so that only its permission can refuse it.
fn sample(method: &str) -> Value {
    match method {
        "panes.send" => json!({ "pane": "Ava", "text": "hi" }),
        "notify" => json!({ "title": "Probe" }),
        "files.read" => json!({ "path": "README.md" }),
        "data.read" => json!({ "path": "probe.txt" }),
        "loopback.fetch" => json!({ "port": 8765, "path": "/" }),
        "command.run" => json!({ "name": "ci-run" }),
        "tab.setTitle" => json!({ "title": "Probe" }),
        "ui.copy" => json!({ "text": "from init" }),
        _ => Value::Null,
    }
}

fn spin() -> ! {
    let mut turns = 0u64;
    loop {
        turns = black_box(turns.wrapping_add(1));
    }
}

fn hog() -> ! {
    let mut held: Vec<Vec<u8>> = Vec::new();
    loop {
        held.push(vec![1; 8 << 20]);
        black_box(&held);
    }
}

impl Kit for Probe {
    type Message = Message;

    fn init(start: &Start, cx: &mut Context<Message>) -> Self {
        match start.tab.params.get("mode").map(String::as_str) {
            Some("loop") => spin(),
            Some("alloc") => hog(),
            Some("panic") => panic!("the probe was told to panic"),
            _ => {}
        }
        let calls = start.tab.params.get("calls").cloned().unwrap_or_default();
        for method in calls.split(',').filter(|method| !method.is_empty()) {
            let name = method.to_owned();
            cx.call_raw(method, sample(method), move |answer| {
                Message::Answer(name, answer)
            });
        }
        Probe {
            answers: Vec::new(),
            events: 0,
            ticks: 0,
        }
    }

    fn update(&mut self, message: Message, cx: &mut Context<Message>) {
        match message {
            Message::Answer(method, Ok(_)) => self.answers.push(format!("{method}: ok")),
            Message::Answer(method, Err(error)) => {
                self.answers.push(format!("{method}: {}", error.code))
            }
            Message::Event => self.events += 1,
            Message::Tick => self.ticks += 1,
            Message::Copy => cx.copy("from a click"),
            Message::Spin => spin(),
            Message::Hog => hog(),
            Message::Panic => panic!("the probe's button panics"),
        }
    }

    fn view(&self) -> Element<Message> {
        page![
            text(format!("events: {}", self.events)),
            text(format!("ticks: {}", self.ticks)),
            list(self.answers.iter().map(|answer| item(answer.clone()))),
            row![
                button("Copy").on_press(Message::Copy),
                button("Spin").on_press(Message::Spin),
                button("Hog").on_press(Message::Hog),
                button("Panic").on_press(Message::Panic),
            ],
        ]
        .into()
    }

    fn subscriptions(&self) -> Subscriptions<Message> {
        // 10 ms, which Hangar holds to 100.
        Subscriptions::none().panes_changed(Message::Event).every(
            "tick",
            Duration::from_millis(10),
            Message::Tick,
        )
    }
}

export_kit!(Probe);
