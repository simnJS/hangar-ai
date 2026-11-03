//! The board driven with real events, through `iced_test`'s simulator: what a
//! press, a click and a key on the drawn board turn into.

use std::borrow::Cow;

use hangar_i18n::Locale;
use iced::keyboard::{Key, key::Named};
use iced::{Element, Settings, Size, mouse, window};
use iced_test::Simulator;

use crate::app::Packages;
use crate::ctx::{Ctx, Metrics};
use crate::message::Message;
use crate::shell::BoardVariant;
use crate::state::Model;

use super::snapshot::{CWD, SITE, model, tasks, two_windows};
use super::{Msg, State, dock};

const REVIEW: &str = "6f7a0000-aaaa-bbbb-cccc-dddddddddddd";

fn simulator<'a>(
    m: &'a Model,
    size: Size,
    build: impl FnOnce(Ctx<'a>) -> Element<'a, Message>,
) -> Simulator<'a, Message> {
    simulator_in(m, window::Id::unique(), size, build)
}

/// The same, in a given window of the model.
fn simulator_in<'a>(
    m: &'a Model,
    window: window::Id,
    size: Size,
    build: impl FnOnce(Ctx<'a>) -> Element<'a, Message>,
) -> Simulator<'a, Message> {
    let packages: &'a Packages = Box::leak(Box::default());
    let ctx = Ctx {
        model: m,
        pkg: packages,
        i18n: hangar_i18n::Translator::new(m.locale()),
        theme: m.theme(),
        metrics: Metrics::default(),
        keymap: &m.live.keymap.index,
        window,
        now: std::time::Instant::now(),
    };
    let settings = Settings {
        fonts: vec![Cow::Borrowed(hangar_theme::fonts::INTER_VARIABLE)],
        default_font: hangar_theme::fonts::ui(),
        ..Settings::default()
    };
    Simulator::with_size(settings, size, build(ctx))
}

fn model_with_tasks() -> Model {
    model(Locale::En, "graphite", Some(tasks()))
}

fn full<'a>(m: &'a Model, st: &'a State) -> Simulator<'a, Message> {
    simulator(m, Size::new(1200.0, 760.0), |ctx| {
        super::view(st, ctx, BoardVariant::Full)
    })
}

#[test]
fn pressing_a_card_tells_where_and_which() {
    let (m, st) = (model_with_tasks(), State::default());
    let mut ui = full(&m, &st);
    ui.click("Review the boards cache").expect("a card");
    let pressed: Vec<_> = ui
        .into_messages()
        .filter_map(|message| match message {
            Message::Board(Msg::Press(cwd, id, at)) => {
                assert_eq!(cwd, CWD);
                Some((id, at))
            }
            _ => None,
        })
        .collect();
    assert_eq!(pressed.len(), 1);
    assert_eq!(pressed[0].0, REVIEW);
    // The window's own position, inside the board.
    assert!(pressed[0].1.x > 0.0 && pressed[0].1.y > 0.0);
}

#[test]
fn the_assignees_button_goes_to_its_pane_and_does_not_press_the_card() {
    let (m, st) = (model_with_tasks(), State::default());
    let mut ui = full(&m, &st);
    ui.click("Ben").expect("the assignee");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|message| matches!(message, Message::Board(Msg::FocusAgent { pane, .. }) if pane == "p2")),
        "{messages:?}"
    );
    assert!(
        !messages
            .iter()
            .any(|message| matches!(message, Message::Board(Msg::Press(..)))),
        "{messages:?}"
    );
}

#[test]
fn a_card_that_has_the_keyboard_opens_with_enter_and_space() {
    for key in [Named::Enter, Named::Space] {
        let (m, st) = (model_with_tasks(), State::default());
        let mut ui = full(&m, &st);
        // A press gives it the keyboard; the key then opens it.
        ui.click("Review the boards cache").expect("a card");
        ui.tap_key(Key::Named(key));
        let opened = ui
            .into_messages()
            .any(|message| matches!(message, Message::Board(Msg::OpenTask(cwd, id)) if cwd == CWD && id == REVIEW));
        assert!(opened, "{key:?} opens the task");
    }
}

#[test]
fn a_key_with_no_card_focused_does_nothing() {
    let (m, st) = (model_with_tasks(), State::default());
    let mut ui = full(&m, &st);
    ui.tap_key(Key::Named(Named::Enter));
    assert!(
        !ui.into_messages()
            .any(|message| matches!(message, Message::Board(Msg::OpenTask(..))))
    );
}

#[test]
fn the_bars_buttons_say_what_they_do() {
    let (m, st) = (model_with_tasks(), State::default());
    let mut ui = full(&m, &st);
    ui.click("Add…").expect("the bar's button");
    ui.click("Agents read and edit this board over MCP")
        .expect("the MCP button");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(messages.iter().any(|message| matches!(
        message,
        Message::Board(Msg::StartAdding(cwd, super::Column::Todo)) if cwd == CWD
    )));
    assert!(
        messages
            .iter()
            .any(|message| matches!(message, Message::Board(Msg::OpenMcp)))
    );
}

#[test]
fn a_filters_button_asks_to_open_its_list() {
    // The list itself is a menu layer, which hangar-ui tests.
    let (m, st) = (model_with_tasks(), State::default());
    let mut ui = full(&m, &st);
    ui.click("Agent").expect("the filter");
    assert!(ui.into_messages().any(|message| matches!(
        message,
        Message::Board(Msg::OpenFilter(Some(super::FilterKind::Assignee)))
    )));
}

#[test]
fn the_dock_buttons_expand_and_hide_it() {
    let (m, st) = (model_with_tasks(), State::default());
    let mut ui = simulator(&m, Size::new(420.0, 760.0), |ctx| {
        super::view(
            &st,
            ctx,
            BoardVariant::Dock {
                width: 420.0,
                drawer: false,
            },
        )
    });
    // A section's header folds it.
    ui.click("To do").expect("a section");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|message| matches!(message, Message::Board(Msg::Fold(super::Column::Todo))))
    );
}

#[test]
fn the_dock_grip_takes_a_press_and_a_double_click() {
    let (m, st) = (model_with_tasks(), State::default());
    let mut ui = simulator(&m, Size::new(420.0, 760.0), |ctx| {
        super::view(
            &st,
            ctx,
            BoardVariant::Dock {
                width: 420.0,
                drawer: false,
            },
        )
    });
    // The grip is the strip on the panel's left edge.
    ui.point_at(iced::Point::new(2.0, 400.0));
    let press = || {
        [
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ]
    };
    ui.simulate(press());
    ui.simulate(press());
    let messages: Vec<_> = ui.into_messages().collect();
    let grips = messages
        .iter()
        .filter(|message| matches!(message, Message::Board(Msg::GripPress { .. })))
        .count();
    let resets = messages
        .iter()
        .filter(|message| matches!(message, Message::Board(Msg::GripReset)))
        .count();
    assert_eq!((grips, resets), (1, 1), "{messages:?}");
    let _ = dock::KEY_STEP;
}

#[test]
fn a_detached_window_draws_and_names_its_own_workspaces_board() {
    // The person is in the detached `site` window; the main window's active
    // workspace is another one. What the site's board sends names the site.
    let (m, main, site) = two_windows();
    let st = State::default();
    let mut ui = simulator_in(&m, site, Size::new(1200.0, 760.0), |ctx| {
        super::view(&st, ctx, BoardVariant::Full)
    });
    ui.click("Fix the footer").expect("the site's card");
    ui.click("Add…").expect("the bar's button");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages.iter().any(|message| matches!(
            message,
            Message::Board(Msg::Press(cwd, id, _)) if cwd == SITE && id == "site-task"
        )),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|message| matches!(
            message,
            Message::Board(Msg::StartAdding(cwd, super::Column::Todo)) if cwd == SITE
        )),
        "{messages:?}"
    );
    // And nothing of the main window's board is on it.
    assert!(
        !messages.iter().any(|message| matches!(
            message,
            Message::Board(Msg::Press(cwd, ..)) if cwd == CWD
        )),
        "{messages:?}"
    );
}

#[test]
fn the_docks_grip_is_given_the_room_the_shell_placed_for_its_window() {
    let (mut m, main, site) = two_windows();
    // This window is narrow: the shell leaves its dock less than the main
    // window's would get.
    m.ui.body_width.insert(site, 900.0);
    let st = State::default();
    let mut ui = simulator_in(&m, site, Size::new(420.0, 760.0), |ctx| {
        super::view(
            &st,
            ctx,
            BoardVariant::Dock {
                width: 420.0,
                drawer: false,
            },
        )
    });
    ui.point_at(iced::Point::new(2.0, 400.0));
    ui.simulate([iced::Event::Mouse(mouse::Event::ButtonPressed(
        mouse::Button::Left,
    ))]);
    let max = ui
        .into_messages()
        .find_map(|message| match message {
            Message::Board(Msg::GripPress { max, .. }) => Some(max),
            _ => None,
        })
        .expect("a press on the grip");
    let expected = crate::shell::Panels::of(&m, site).dock.max;
    assert_eq!(max, expected);
    // A narrower window than the default one allows the dock less.
    assert!(max < crate::shell::Panels::of(&m, main).dock.max);
}

/// The press of the palette's chord, Mod+Shift+P, which the keymap takes.
/// `Mod` is held as Ctrl on Windows and Linux and as Cmd on a Mac, which is
/// what `Modifiers::COMMAND` is on each: a literal Ctrl would be no shortcut
/// at all on a Mac, and the guards would rightly let the key through.
fn palette_chord() -> iced::Event {
    use iced::keyboard::{Event, Location, Modifiers, key::Code, key::Physical};
    iced::Event::Keyboard(Event::KeyPressed {
        key: Key::Character("p".into()),
        modified_key: Key::Character("P".into()),
        physical_key: Physical::Code(Code::KeyP),
        location: Location::Standard,
        modifiers: Modifiers::COMMAND | Modifiers::SHIFT,
        text: None,
        repeat: false,
    })
}

#[test]
fn the_add_field_asks_the_keymap_before_it_uses_a_key() {
    let m = model_with_tasks();
    let st = State {
        adding: Some((CWD.to_owned(), super::Column::Todo)),
        ..State::default()
    };
    let mut ui = full(&m, &st);
    ui.simulate([palette_chord()]);
    // The guard spends the key (`Noop`); the subscription runs the command.
    assert!(
        ui.into_messages()
            .any(|message| matches!(message, Message::Noop))
    );
}

#[test]
fn the_task_dialogs_fields_ask_the_keymap_before_they_use_a_key() {
    let m = model_with_tasks();
    let st = State {
        dialog: Some(super::task_dialog::Dialog::open(
            CWD,
            None,
            m.live.board.tasks(CWD).unwrap_or(&[]),
        )),
        ..State::default()
    };
    let mut ui = simulator(&m, Size::new(900.0, 800.0), |ctx| {
        super::task_view(&st, ctx, CWD, None)
    });
    ui.simulate([palette_chord()]);
    // The title, the description, the priority and the files each guard
    // their own field, and each spends the key.
    let spent = ui
        .into_messages()
        .filter(|message| matches!(message, Message::Noop))
        .count();
    assert!(spent >= 3, "the chord was spent {spent} times");
}
