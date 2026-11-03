//! A headless window for the tests: widgets built, laid out and driven with
//! real events, without a display.
//!
//! The interaction tests of this crate (a menu's keys, a dialog's trap) go
//! through `iced_test`'s simulator, so that what they check is what the
//! runtime does with the widgets and not what a unit test imagines it does.

use std::borrow::Cow;

use hangar_theme::fonts;
use iced::keyboard::{self, Key, key::Named};
use iced::{Element, Event, Point, Settings, Size, mouse};
use iced_test::Simulator;

/// The window the tests run in.
pub const WINDOW: Size = Size::new(800.0, 600.0);

/// A simulator showing `element` in a window of [`WINDOW`], with the
/// interface font loaded so that text is measured as it is in the app.
pub fn simulate<'a, M>(element: impl Into<Element<'a, M>>) -> Simulator<'a, M> {
    let settings = Settings {
        fonts: vec![Cow::Borrowed(fonts::INTER_VARIABLE)],
        default_font: fonts::ui(),
        ..Settings::default()
    };
    Simulator::with_size(settings, WINDOW, element)
}

/// What a test does with the window.
pub trait Drive {
    /// Moves the pointer, the way a mouse does: the position and the event.
    fn move_to(&mut self, x: f32, y: f32);
    fn press(&mut self, button: mouse::Button);
    fn release(&mut self, button: mouse::Button);
    /// A press and a release of the left button where the pointer is.
    fn click_at(&mut self, x: f32, y: f32);
    fn key(&mut self, named: Named) -> iced::event::Status;
    fn key_with(&mut self, named: Named, modifiers: keyboard::Modifiers) -> iced::event::Status;
}

impl<M> Drive for Simulator<'_, M> {
    fn move_to(&mut self, x: f32, y: f32) {
        let position = Point::new(x, y);
        self.point_at(position);
        self.simulate([Event::Mouse(mouse::Event::CursorMoved { position })]);
    }

    fn press(&mut self, button: mouse::Button) {
        self.simulate([Event::Mouse(mouse::Event::ButtonPressed(button))]);
    }

    fn release(&mut self, button: mouse::Button) {
        self.simulate([Event::Mouse(mouse::Event::ButtonReleased(button))]);
    }

    fn click_at(&mut self, x: f32, y: f32) {
        self.move_to(x, y);
        self.press(mouse::Button::Left);
        self.release(mouse::Button::Left);
    }

    fn key(&mut self, named: Named) -> iced::event::Status {
        self.key_with(named, keyboard::Modifiers::empty())
    }

    fn key_with(&mut self, named: Named, modifiers: keyboard::Modifiers) -> iced::event::Status {
        let key = Key::Named(named);
        let press = Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            location: keyboard::Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        });
        self.simulate([press])
            .first()
            .copied()
            .unwrap_or(iced::event::Status::Ignored)
    }
}
