use hangar_theme::default_theme;
use iced::keyboard::key::{Code, Named, Physical};
use iced::keyboard::{self, Key, Modifiers};
use iced::{Element, Event};
use iced_test::Simulator;

use super::*;
use crate::shortcut::Outcome;
use crate::testing::{Drive, simulate};

#[derive(Debug, Clone, PartialEq)]
enum Msg {
    Record,
    Input(Input),
    Confirm,
    Cancel,
    Remove(usize),
    Reset,
}

fn keys() -> Vec<String> {
    vec!["Ctrl+K Ctrl+S".into(), "F5".into()]
}

fn at_rest<'a>(keys: &'a [String]) -> Element<'a, Msg> {
    shortcut_field(default_theme(), keys, None)
        .on_record(Msg::Record)
        .on_input(Msg::Input)
        .on_confirm(Msg::Confirm)
        .on_cancel(Msg::Cancel)
        .on_remove(Msg::Remove)
        .on_reset(Msg::Reset)
        .customized(true)
        .into()
}

fn while_recording<'a>(keys: &'a [String], recorder: &'a Recorder) -> Element<'a, Msg> {
    shortcut_field(default_theme(), keys, Some(recorder))
        .on_record(Msg::Record)
        .on_input(Msg::Input)
        .on_confirm(Msg::Confirm)
        .on_cancel(Msg::Cancel)
        .into()
}

fn messages(ui: Simulator<'_, Msg>) -> Vec<Msg> {
    ui.into_messages().collect()
}

/// A key press the way iced reports it.
fn press(key: Key, code: Code, modifiers: Modifiers) -> Event {
    Event::Keyboard(keyboard::Event::KeyPressed {
        key: key.clone(),
        modified_key: key,
        physical_key: Physical::Code(code),
        location: keyboard::Location::Standard,
        modifiers,
        text: None,
        repeat: false,
    })
}

mod at_rest {
    use super::*;

    #[test]
    fn does_not_listen_to_the_keyboard() {
        let keys = keys();
        let mut ui = simulate(at_rest(&keys));
        assert_eq!(ui.key(Named::F5), iced::event::Status::Ignored);
        assert_eq!(messages(ui), []);
    }

    #[test]
    fn a_command_with_no_keys_says_so() {
        let none: Vec<String> = Vec::new();
        let mut ui = simulate(at_rest(&none));
        assert!(ui.find("Not set").is_ok());
    }
}

mod recording {
    use super::*;

    #[test]
    fn takes_every_key_so_that_none_reaches_the_window() {
        let keys = keys();
        let recorder = Recorder::new();
        let mut ui = simulate(while_recording(&keys, &recorder));
        let statuses = ui.simulate([press(
            Key::Character("k".into()),
            Code::KeyK,
            Modifiers::CTRL,
        )]);
        assert_eq!(statuses, [iced::event::Status::Captured]);
        // Even Escape: the recorder decides what it means.
        assert_eq!(ui.key(Named::Escape), iced::event::Status::Captured);
    }

    #[test]
    fn reports_what_was_pressed_in_the_recorders_terms() {
        let keys = keys();
        let recorder = Recorder::new();
        let mut ui = simulate(while_recording(&keys, &recorder));
        ui.simulate([press(
            Key::Character("k".into()),
            Code::KeyK,
            Modifiers::CTRL,
        )]);
        let sent = messages(ui);
        let [Msg::Input(input)] = sent.as_slice() else {
            panic!("{sent:?}");
        };
        let mut fed = Recorder::new();
        assert_eq!(fed.input(input.clone()), Outcome::Pending);
        assert_eq!(fed.binding().as_deref(), Some("Ctrl+K"));
    }

    #[test]
    fn a_modifier_on_its_own_is_reported_so_it_can_be_shown() {
        let keys = keys();
        let recorder = Recorder::new();
        let mut ui = simulate(while_recording(&keys, &recorder));
        ui.simulate([press(
            Key::Named(Named::Shift),
            Code::ShiftLeft,
            Modifiers::SHIFT,
        )]);
        let sent = messages(ui);
        assert!(
            matches!(
                sent.as_slice(),
                [Msg::Input(Input::Press { chord: None, .. })]
            ),
            "{sent:?}"
        );
    }

    #[test]
    fn shows_what_has_been_recorded_and_offers_to_confirm_it() {
        let keys = keys();
        let mut recorder = Recorder::new();
        recorder.input(Input::Press {
            chord: Some(crate::shortcut::Chord::new("K").ctrl()),
            held: crate::shortcut::Held {
                ctrl: true,
                ..Default::default()
            },
        });
        let mut ui = simulate(while_recording(&keys, &recorder));
        // The modifier is drawn as the platform writes it: `Ctrl`, or `⌃` on
        // a Mac.
        let ctrl = crate::shortcut::Held {
            ctrl: true,
            ..Default::default()
        }
        .segments()[0];
        assert!(ui.find(ctrl).is_ok(), "{ctrl} is shown");
    }

    #[test]
    fn prompts_before_anything_is_pressed() {
        let keys = keys();
        let recorder = Recorder::new();
        let mut ui = simulate(while_recording(&keys, &recorder));
        assert!(ui.find("Press the keys…").is_ok());
    }
}
