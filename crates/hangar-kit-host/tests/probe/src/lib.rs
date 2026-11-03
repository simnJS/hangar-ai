//! A tab that does what its params say, for the host's tests: `mode` makes
//! it loop, allocate or panic while starting; `calls` lists methods it calls
//! at once, each answer shown as `method: ok` or `method: <code>`. Its
//! buttons loop, panic or copy from an update, where a person's click is;
//! pick a file, paste a picture, export a canvas and focus a field, the
//! answers shown the same way; and Ctrl+S counts its saves.

use std::hint::black_box;
use std::time::Duration;

use serde_json::{Value, json};

use hangar_kit_sdk::prelude::*;

pub struct Probe {
    answers: Vec<String>,
    events: u32,
    ticks: u32,
    saves: u32,
    offset: i32,
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
    Save,
    Pick,
    Paste,
    Export,
    Focus,
}

/// What a call answered, as the probe shows it: `ok`, the held picture or
/// the path a pick or an export gave, or the error's code.
fn said(method: &str, answer: &Result<Value, KitError>) -> String {
    match answer {
        Ok(value) => match value
            .get("held")
            .or_else(|| value.get("path"))
            .and_then(Value::as_str)
        {
            Some(what) => format!("{method}: {what}"),
            None if method == "image.size" => {
                format!("{method}: {}x{}", value["width"], value["height"])
            }
            None if value.is_null() && method == "files.pick" => format!("{method}: cancelled"),
            None => format!("{method}: ok"),
        },
        Err(error) => format!("{method}: {}", error.code),
    }
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
        "files.pick" => json!({ "extensions": ["png"] }),
        "canvas.export" => export(),
        "image.size" => json!({ "source": { "file": "shots/a.png" } }),
        _ => Value::Null,
    }
}

/// A canvas of 64 by 32, a box on it, to write as `shots/probe.png`.
fn export() -> Value {
    serde_json::to_value(api::CanvasExport::of(
        &canvas::<Message>("shot", 64.0, 32.0)
            .mark(Mark::rect(8.0, 8.0, 20.0, 10.0).ink(Ink::Blue)),
        "shots/probe.png",
    ))
    .unwrap_or(Value::Null)
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
            saves: 0,
            offset: start.utc_offset,
        }
    }

    fn update(&mut self, message: Message, cx: &mut Context<Message>) {
        match message {
            Message::Answer(method, answer) => self.answers.push(said(&method, &answer)),
            Message::Event => self.events += 1,
            Message::Tick => self.ticks += 1,
            Message::Copy => cx.copy("from a click"),
            Message::Spin => spin(),
            Message::Hog => hog(),
            Message::Panic => panic!("the probe's button panics"),
            Message::Save => self.saves += 1,
            Message::Pick => cx.call_raw("files.pick", sample("files.pick"), |answer| {
                Message::Answer("files.pick".into(), answer)
            }),
            Message::Paste => cx.call_raw("ui.pasteImage", json!({}), |answer| {
                Message::Answer("ui.pasteImage".into(), answer)
            }),
            Message::Export => cx.call_raw("canvas.export", export(), |answer| {
                Message::Answer("canvas.export".into(), answer)
            }),
            Message::Focus => {
                cx.focus("name");
                cx.focus("nowhere");
                cx.scroll_to_line("job", 40);
            }
        }
    }

    fn view(&self) -> Element<Message> {
        page![
            text(format!("events: {}", self.events)),
            text(format!("ticks: {}", self.ticks)),
            text(format!("saves: {}", self.saves)),
            text(format!("offset: {}", self.offset)),
            list(self.answers.iter().map(|answer| item(answer.clone()))),
            row![
                button("Copy").on_press(Message::Copy),
                button("Spin").on_press(Message::Spin),
                button("Hog").on_press(Message::Hog),
                button("Panic").on_press(Message::Panic),
                button("Pick").on_press(Message::Pick),
                button("Paste").on_press(Message::Paste),
                button("Export").on_press(Message::Export),
                button("Focus").on_press(Message::Focus),
            ],
            text_input("Name", "").id("name"),
            log_lines((0..100).map(|n| log_line(format!("line {n}")))).id("job"),
        ]
        .into()
    }

    fn subscriptions(&self) -> Subscriptions<Message> {
        // 10 ms, which Hangar holds to 100.
        Subscriptions::none()
            .panes_changed(Message::Event)
            .every("tick", Duration::from_millis(10), Message::Tick)
            .shortcut("mod+s", Message::Save)
    }
}

export_kit!(Probe);
