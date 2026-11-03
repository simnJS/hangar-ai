//! A headless window for the interaction tests of the chrome: a view built
//! from a model, laid out and driven with real pointer and key events,
//! without a display (the way hangar-ui's own interaction tests do it).

use std::borrow::Cow;
use std::time::Instant;

use hangar_theme::fonts;
use iced::keyboard::{self, Key, key::Named};
use iced::{Element, Event, Point, Settings, Size, mouse, window};
use iced_test::Simulator;

use crate::app::Packages;
use crate::ctx::{Ctx, Metrics};
use crate::message::Message;
use crate::shell::WindowKind;
use crate::state::Model;
use crate::state::types::{AgentId, AppState, Pane, Workspace, WorkspaceFolder};

/// The window the tests run in: tall enough for every row, as wide as the main
/// window's frame so the toolbar lays out as it does there.
pub const WINDOW: Size = Size::new(1100.0, 700.0);

/// A simulator showing `element` in [`WINDOW`], with the interface font loaded
/// so that text is measured as it is in the app.
pub fn simulate<'a>(element: impl Into<Element<'a, Message>>) -> Simulator<'a, Message> {
    let settings = Settings {
        fonts: vec![Cow::Borrowed(fonts::INTER_VARIABLE)],
        default_font: fonts::ui(),
        ..Settings::default()
    };
    Simulator::with_size(settings, WINDOW, element)
}

/// What the views are drawn with, for a model and the packages' states: the
/// model's main window's.
pub fn ctx<'a>(model: &'a Model, pkg: &'a Packages) -> Ctx<'a> {
    let window = model.main_window().unwrap_or_else(window::Id::unique);
    ctx_in(model, pkg, window)
}

/// The same, for another window of the model.
pub fn ctx_in<'a>(model: &'a Model, pkg: &'a Packages, window: window::Id) -> Ctx<'a> {
    Ctx {
        model,
        pkg,
        i18n: hangar_i18n::Translator::new(hangar_i18n::Locale::En),
        theme: model.theme_in(window),
        metrics: Metrics {
            scale: 1.0,
            body_width: WINDOW.width,
        },
        keymap: &model.live.keymap.index,
        window,
        now: Instant::now(),
    }
}

pub fn pane(id: &str, name: &str, agent: AgentId) -> Pane {
    Pane {
        id: id.into(),
        name: name.into(),
        agent,
        ..Pane::default()
    }
}

pub fn workspace(id: &str, name: &str, folder: Option<&str>, panes: Vec<Pane>) -> Workspace {
    Workspace {
        id: id.into(),
        name: name.into(),
        cwd: format!("/dev/{id}"),
        folder_id: folder.map(str::to_string),
        panes,
        ..Workspace::default()
    }
}

pub fn folder(id: &str, name: &str, collapsed: bool) -> WorkspaceFolder {
    WorkspaceFolder {
        id: id.into(),
        name: name.into(),
        collapsed,
        ..WorkspaceFolder::default()
    }
}

/// A model that has loaded `state`, with its main window open.
pub fn model_of(state: AppState) -> Model {
    let mut model = Model::default();
    model.hydrate(state);
    model
        .live
        .windows
        .insert(window::Id::unique(), WindowKind::Main);
    model
}

/// Opens a window of its own for workspace `ws`, as detaching it does: the
/// workspace is flagged, and the window is the one that draws it.
pub fn detach(model: &mut Model, ws: &str) -> window::Id {
    let mut state = model.state().clone();
    for entry in &mut state.workspaces {
        if entry.id == ws {
            entry.detached = Some(true);
        }
    }
    model.hydrate(state);
    let window = window::Id::unique();
    model
        .live
        .windows
        .insert(window, WindowKind::Workspace(ws.to_string()));
    window
}

/// What a test does with the window, besides what the simulator already does.
pub trait Drive {
    fn move_to(&mut self, x: f32, y: f32);
    fn press(&mut self, button: mouse::Button);
    fn release(&mut self, button: mouse::Button);
    fn key(&mut self, named: Named, modifiers: keyboard::Modifiers) -> iced::event::Status;
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

    fn key(&mut self, named: Named, modifiers: keyboard::Modifiers) -> iced::event::Status {
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
