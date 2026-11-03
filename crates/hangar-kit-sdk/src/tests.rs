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

/// A sketch of kit-annotate and kit-ci together: marks drawn on a canvas,
/// undone with a shortcut, saved after a pause, a note focused, a long log
/// given in parts.
struct Sketch {
    marks: Vec<Mark>,
    saves: u32,
    window: (u32, u32),
    copied: bool,
}

#[derive(Clone, Debug)]
enum Sketched {
    Drew(Mark),
    Note(Point),
    Undo,
    Save,
    Saved,
    Copy,
    Range(Range),
}

impl Kit for Sketch {
    type Message = Sketched;

    fn init(_start: &Start, _cx: &mut Context<Sketched>) -> Self {
        Sketch {
            marks: Vec::new(),
            saves: 0,
            window: (0, 100),
            copied: false,
        }
    }

    fn update(&mut self, message: Sketched, cx: &mut Context<Sketched>) {
        match message {
            Sketched::Drew(mark) => {
                self.marks.push(mark);
                cx.after("save", Duration::from_millis(1500), Sketched::Save);
            }
            Sketched::Note(at) => {
                self.marks.push(Mark::circle(at, 6.0).fill(Ink::Red));
                cx.focus(format!("note-{}", self.marks.len()));
            }
            Sketched::Undo => {
                self.marks.pop();
                cx.after("save", Duration::from_millis(1500), Sketched::Save);
            }
            Sketched::Save => self.saves += 1,
            Sketched::Saved => {}
            Sketched::Copy => {
                self.copied = cx.is_gesture();
                cx.copy("marks");
            }
            Sketched::Range(range) => {
                self.window = (range.start, range.end);
                cx.scroll_to_line("job", range.start);
            }
        }
    }

    fn view(&self) -> Element<Sketched> {
        let lines = (self.window.0..self.window.1).map(|n| log_line(format!("line {n}")));
        col![
            canvas("shot", 800.0, 600.0)
                .tool(Some(Tool::Arrow))
                .marks(self.marks.clone())
                .on_draw(Sketched::Drew)
                .on_point(Sketched::Note),
            swatch(Ink::Blue, "Blue").on_press(Sketched::Saved),
            log_lines(lines)
                .id("job")
                .window(self.window.0, 20_000)
                .on_range(Sketched::Range),
        ]
        .into()
    }

    fn subscriptions(&self) -> Subscriptions<Sketched> {
        Subscriptions::none()
            .shortcut("Mod+Z", Sketched::Undo)
            .shortcut("mod+shift+c", Sketched::Copy)
            .shortcut("hyper+q", Sketched::Saved)
    }
}

#[test]
fn a_canvas_sends_what_was_drawn_and_where_it_was_pressed() {
    let mut tab = Harness::<Sketch>::new(testing::start());
    tab.draw("shot", Mark::arrow((10.0, 10.0), (200.0, 90.0)));
    tab.point("shot", Point::new(40.0, 50.0));
    assert_eq!(tab.kit().marks.len(), 2);
    assert_eq!(
        tab.take_operations(),
        vec![crate::ui::abi::Operation::Focus {
            id: "note-2".into()
        }]
    );
    tab.press("Blue");
}

#[test]
fn shortcuts_are_subscribed_by_one_spelling_and_are_gestures() {
    let mut tab = Harness::<Sketch>::new(testing::start());
    assert_eq!(
        tab.subscriptions().shortcuts,
        vec!["mod+z", "mod+shift+c"],
        "the spec that does not read is left out"
    );
    tab.draw("shot", Mark::arrow((0.0, 0.0), (5.0, 5.0)));
    tab.shortcut("mod+z");
    assert!(tab.kit().marks.is_empty());
    tab.shortcut("shift+mod+c");
    assert!(tab.kit().copied);
    assert_eq!(tab.methods(), vec!["ui.copy"]);
}

#[test]
fn after_waits_once_and_asking_again_starts_the_wait_again() {
    let mut tab = Harness::<Sketch>::new(testing::start());
    tab.draw("shot", Mark::arrow((0.0, 0.0), (5.0, 5.0)));
    let first = tab.subscriptions().timers.clone();
    tab.draw("shot", Mark::arrow((1.0, 0.0), (5.0, 5.0)));
    let second = tab.subscriptions().timers.clone();
    assert_eq!(first.len(), 1);
    assert_eq!(second.len(), 1);
    assert_ne!(first[0].id, second[0].id, "a new wait is a new timer");
    assert_eq!(second[0].every_ms, 1500);
    // The first wait's timer, had the host not stopped it yet, sends nothing.
    tab.timer(&first[0].id);
    assert_eq!(tab.kit().saves, 0);
    tab.elapse("save");
    assert_eq!(tab.kit().saves, 1);
    assert!(!tab.waits("save"), "it is sent once");
}

#[test]
fn a_long_log_is_given_the_lines_it_asks_for() {
    let mut tab = Harness::<Sketch>::new(testing::start());
    assert!(tab.shows("line 99"));
    tab.range("job", 4000, 4200);
    assert!(tab.shows("line 4100"));
    assert!(!tab.shows("line 99"));
    assert_eq!(
        tab.take_operations(),
        vec![crate::ui::abi::Operation::ScrollTo {
            id: "job".into(),
            line: Some(4000),
            y: None,
            end: false
        }]
    );
}
