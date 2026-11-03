//! The widget driven the way `iced` drives it: events in, actions out.
//!
//! This is the test of the input path as a whole: a key event reaches the
//! widget, goes through the filter, the bindings and the encoder, and comes out
//! as the action the app passes to the terminal. It runs on the software
//! renderer, so it needs no window or GPU.

use std::time::Instant;

use hangar_term::{
    Action, Config, Event as TerminalEvent, Font, Palette, Pointer, ScrollTo, Terminal,
    terminal_view,
};
use iced::advanced::Shell;
use iced::advanced::clipboard::{self, Clipboard};
use iced::advanced::input_method;
use iced::advanced::layout::{Layout, Limits};
use iced::advanced::renderer::{Headless, Style};
use iced::advanced::widget::Tree;
use iced::keyboard::key::{Code, Named, NativeCode, Physical};
use iced::keyboard::{self, Key, Location, Modifiers};
use iced::{Color, Element, Event, Pixels, Point, Rectangle, Size, mouse, window};

type Renderer = iced::Renderer;

const SIZE: Size = Size::new(800.0, 480.0);

struct Clip(Option<String>);

impl Clipboard for Clip {
    fn read(&self, _kind: clipboard::Kind) -> Option<String> {
        self.0.clone()
    }
    fn write(&mut self, _kind: clipboard::Kind, _contents: String) {}
}

/// What one event did: the actions published, and whether it was captured.
struct Outcome {
    actions: Vec<Action>,
    captured: bool,
}

struct Harness<'a> {
    element: Element<'a, Action, iced::Theme, Renderer>,
    tree: Tree,
    node: iced::advanced::layout::Node,
    renderer: Renderer,
    clipboard: Clip,
    cursor: mouse::Cursor,
}

impl<'a> Harness<'a> {
    fn new(element: Element<'a, Action, iced::Theme, Renderer>) -> Self {
        let mut renderer = iced::futures::executor::block_on(Renderer::new(
            iced::Font::DEFAULT,
            Pixels(16.0),
            Some("tiny-skia"),
        ))
        .expect("a software renderer");
        let mut element = element;
        let mut tree = Tree::new(&element);
        let node =
            element
                .as_widget_mut()
                .layout(&mut tree, &renderer, &Limits::new(Size::ZERO, SIZE));
        let _ = &mut renderer;
        Self {
            element,
            tree,
            node,
            renderer,
            clipboard: Clip(None),
            cursor: mouse::Cursor::Available(Point::new(100.0, 100.0)),
        }
    }

    fn send(&mut self, event: Event) -> Outcome {
        let mut actions = Vec::new();
        let mut shell = Shell::new(&mut actions);
        self.element.as_widget_mut().update(
            &mut self.tree,
            &event,
            Layout::new(&self.node),
            self.cursor,
            &self.renderer,
            &mut self.clipboard,
            &mut shell,
            &iced::Rectangle::with_size(SIZE),
        );
        let captured = shell.is_event_captured();
        Outcome { actions, captured }
    }

    fn frame(&mut self) -> Outcome {
        self.send(Event::Window(
            window::Event::RedrawRequested(Instant::now()),
        ))
    }

    /// The cursor the widget asks for with the pointer where it is.
    fn interaction(&self) -> mouse::Interaction {
        self.element.as_widget().mouse_interaction(
            &self.tree,
            Layout::new(&self.node),
            self.cursor,
            &Rectangle::with_size(SIZE),
            &self.renderer,
        )
    }

    /// Moves the pointer, as the window would report it.
    fn point_at(&mut self, position: Point) {
        self.cursor = mouse::Cursor::Available(position);
        self.send(Event::Mouse(mouse::Event::CursorMoved { position }));
    }

    /// The widget drawn on the software renderer: RGBA, one pixel per logical
    /// pixel.
    fn draw(&mut self) -> Frame {
        self.element.as_widget().draw(
            &self.tree,
            &mut self.renderer,
            &iced::Theme::Dark,
            &Style {
                text_color: Color::WHITE,
            },
            Layout::new(&self.node),
            self.cursor,
            &Rectangle::with_size(SIZE),
        );
        let size = Size::new(SIZE.width as u32, SIZE.height as u32);
        Frame {
            width: size.width as usize,
            rgba: self.renderer.screenshot(size, 1.0, Color::BLACK),
        }
    }
}

/// A drawn frame.
struct Frame {
    width: usize,
    rgba: Vec<u8>,
}

impl Frame {
    fn at(&self, x: usize, y: usize) -> [u8; 3] {
        let i = (y * self.width + x) * 4;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2]]
    }

    /// The columns of the first `rows` pixel rows that hold anything but
    /// `background`, grouped into the runs of neighbouring columns: where the
    /// glyphs of a line of text are.
    fn ink(&self, rows: usize, background: [u8; 3]) -> Vec<std::ops::Range<usize>> {
        let mut found: Vec<std::ops::Range<usize>> = Vec::new();
        for x in 0..self.width {
            let inked = (0..rows).any(|y| {
                let p = self.at(x, y);
                p.iter().zip(&background).any(|(a, b)| a.abs_diff(*b) > 24)
            });
            if !inked {
                continue;
            }
            match found.last_mut() {
                Some(run) if run.end == x => run.end = x + 1,
                _ => found.push(x..x + 1),
            }
        }
        found
    }
}

fn rgb8(color: Color) -> [u8; 3] {
    let c = color.into_rgba8();
    [c[0], c[1], c[2]]
}

fn key(key: Key, text: Option<&str>, modifiers: Modifiers) -> Event {
    Event::Keyboard(keyboard::Event::KeyPressed {
        modified_key: key.clone(),
        key,
        physical_key: Physical::Unidentified(NativeCode::Unidentified),
        location: Location::Standard,
        modifiers,
        text: text.map(Into::into),
        repeat: false,
    })
}

fn dead_key(character: &str) -> Event {
    Event::Keyboard(keyboard::Event::KeyPressed {
        key: Key::Character(character.into()),
        modified_key: Key::Unidentified,
        physical_key: Physical::Unidentified(NativeCode::Unidentified),
        location: Location::Standard,
        modifiers: Modifiers::empty(),
        text: None,
        repeat: false,
    })
}

fn focused_terminal(config: Config) -> Terminal {
    let mut terminal = Terminal::new(config, 80, 24);
    terminal.set_font(Font::monospace(14.0));
    terminal.set_palette(Palette::default());
    terminal.set_focused(true);
    terminal
}

/// The bytes of the `Input` actions among `actions`.
fn typed(actions: &[Action]) -> Vec<u8> {
    actions
        .iter()
        .filter_map(|a| match a {
            Action::Input(bytes) => Some(bytes.clone()),
            _ => None,
        })
        .flatten()
        .collect()
}

fn none() -> Modifiers {
    Modifiers::empty()
}

#[test]
fn a_dead_key_then_a_letter_types_the_composed_letter_once() {
    let terminal = focused_terminal(Config::default());
    let mut h = Harness::new(terminal_view(&terminal, |a| a).into());
    // US-International: `'` is dead, then `e` arrives composed.
    let first = h.send(dead_key("'"));
    assert!(typed(&first.actions).is_empty(), "a dead key types nothing");
    let second = h.send(key(Key::Character("e".into()), Some("é"), none()));
    assert_eq!(typed(&second.actions), "é".as_bytes());
    assert!(second.captured);
    // Nothing sticks: the next letter is itself.
    let third = h.send(key(Key::Character("a".into()), Some("a"), none()));
    assert_eq!(typed(&third.actions), b"a");
}

#[test]
fn a_dead_key_then_space_types_the_apostrophe() {
    let terminal = focused_terminal(Config::default());
    let mut h = Harness::new(terminal_view(&terminal, |a| a).into());
    h.send(dead_key("'"));
    let out = h.send(key(Key::Named(Named::Space), Some("'"), none()));
    assert_eq!(typed(&out.actions), b"'");
}

#[test]
fn an_input_method_commit_types_once_even_if_the_key_event_says_the_same() {
    let terminal = focused_terminal(Config::default());
    let mut h = Harness::new(terminal_view(&terminal, |a| a).into());
    let preedit = h.send(Event::InputMethod(input_method::Event::Preedit(
        "´".into(),
        None,
    )));
    assert!(typed(&preedit.actions).is_empty());
    let commit = h.send(Event::InputMethod(input_method::Event::Commit("é".into())));
    assert_eq!(typed(&commit.actions), "é".as_bytes());
    // The key event of the same keystroke arrives right behind.
    let echo = h.send(key(Key::Character("e".into()), Some("é"), none()));
    assert!(
        typed(&echo.actions).is_empty(),
        "the same keystroke is not typed twice"
    );
}

#[test]
fn keys_the_app_takes_do_not_reach_the_terminal_and_are_not_captured() {
    let terminal = focused_terminal(Config::default());
    let view = terminal_view(&terminal, |a| a)
        // The app's keymap wants Ctrl+Tab and Alt+arrows.
        .key_filter(|press| {
            (press.modifiers.control() && matches!(press.key, Key::Named(Named::Tab)))
                || (press.modifiers.alt() && matches!(press.key, Key::Named(Named::ArrowLeft)))
        });
    let mut h = Harness::new(view.into());

    let ctrl_tab = h.send(key(Key::Named(Named::Tab), None, Modifiers::CTRL));
    assert!(ctrl_tab.actions.is_empty());
    assert!(!ctrl_tab.captured, "an app's key must go on to the app");
    let alt_left = h.send(key(Key::Named(Named::ArrowLeft), None, Modifiers::ALT));
    assert!(alt_left.actions.is_empty() && !alt_left.captured);

    // A key the app does not claim goes to the program.
    let tab = h.send(key(Key::Named(Named::Tab), Some("\t"), none()));
    assert_eq!(typed(&tab.actions), b"\t");
    assert!(tab.captured);
    let left = h.send(key(Key::Named(Named::ArrowLeft), None, none()));
    assert_eq!(typed(&left.actions), b"\x1b[D");
}

#[test]
fn intercept_publishes_the_apps_message_instead_of_typing() {
    let terminal = focused_terminal(Config::default());
    // The app's message type is the action here, so use Copy as the marker.
    let view = terminal_view(&terminal, |a| a).intercept(|press| {
        (press.modifiers.control()
            && press.modifiers.shift()
            && matches!(&press.key, Key::Character(c) if c == "p"))
        .then_some(Action::SelectAll)
    });
    let mut h = Harness::new(view.into());
    let out = h.send(key(
        Key::Character("p".into()),
        Some("P"),
        Modifiers::CTRL | Modifiers::SHIFT,
    ));
    assert!(matches!(out.actions.as_slice(), [Action::SelectAll]));
    assert!(out.captured);
}

#[test]
fn an_unfocused_terminal_ignores_the_keyboard() {
    let mut terminal = focused_terminal(Config::default());
    terminal.set_focused(false);
    let mut h = Harness::new(terminal_view(&terminal, |a| a).into());
    let out = h.send(key(Key::Character("a".into()), Some("a"), none()));
    assert!(out.actions.is_empty());
    assert!(!out.captured);
}

#[test]
fn modified_keys_are_encoded_the_xterm_way() {
    let mut terminal = focused_terminal(Config {
        shift_enter: Some(b"\x1b\r".to_vec()),
        ..Config::default()
    });
    terminal.feed(b"\x1b[?1h");
    let mut h = Harness::new(terminal_view(&terminal, |a| a).into());
    let up = h.send(key(Key::Named(Named::ArrowUp), None, none()));
    assert_eq!(typed(&up.actions), b"\x1bOA", "application cursor mode");
    let ctrl_c = h.send(key(
        Key::Character("c".into()),
        Some("\u{3}"),
        Modifiers::CTRL,
    ));
    assert_eq!(typed(&ctrl_c.actions), b"\x03");
    let alt_b = h.send(key(Key::Character("b".into()), Some("b"), Modifiers::ALT));
    assert_eq!(typed(&alt_b.actions), b"\x1bb");
    let shift_enter = h.send(key(Key::Named(Named::Enter), Some("\r"), Modifiers::SHIFT));
    assert_eq!(
        typed(&shift_enter.actions),
        b"\x1b\r",
        "the agent pane's newline"
    );
    let enter = h.send(key(Key::Named(Named::Enter), Some("\r"), none()));
    assert_eq!(typed(&enter.actions), b"\r");
}

#[test]
fn ctrl_shift_c_and_v_are_copy_and_paste_and_ctrl_v_only_if_configured() {
    let terminal = focused_terminal(Config::default());
    let mut h = Harness::new(terminal_view(&terminal, |a| a).into());
    h.clipboard = Clip(Some("from the clipboard".into()));

    let copy = h.send(key(
        Key::Character("c".into()),
        Some("\u{3}"),
        Modifiers::CTRL | Modifiers::SHIFT,
    ));
    assert!(matches!(copy.actions.as_slice(), [Action::Copy]));
    let paste = h.send(key(
        Key::Character("v".into()),
        Some("\u{16}"),
        Modifiers::CTRL | Modifiers::SHIFT,
    ));
    assert!(matches!(paste.actions.as_slice(), [Action::Paste(t)] if t == "from the clipboard"));
    // Ctrl+V alone is the program's ^V.
    let ctrl_v = h.send(key(
        Key::Character("v".into()),
        Some("\u{16}"),
        Modifiers::CTRL,
    ));
    assert_eq!(typed(&ctrl_v.actions), b"\x16");

    let terminal = focused_terminal(Config {
        ctrl_v_pastes: true,
        ..Config::default()
    });
    let mut h = Harness::new(terminal_view(&terminal, |a| a).into());
    h.clipboard = Clip(Some("pasted".into()));
    // The platform's paste key: Ctrl+V, or Cmd+V on a Mac, where Ctrl+V stays
    // the program's `^V` whatever the configuration says. `COMMAND` is iced's
    // name for whichever of the two it is.
    let ctrl_v = h.send(key(
        Key::Character("v".into()),
        Some("\u{16}"),
        Modifiers::COMMAND,
    ));
    assert!(matches!(ctrl_v.actions.as_slice(), [Action::Paste(t)] if t == "pasted"));
    assert!(ctrl_v.captured);
}

#[test]
fn shift_page_up_scrolls_the_history_but_not_on_the_alternate_screen() {
    let mut terminal = focused_terminal(Config::default());
    let mut h = Harness::new(terminal_view(&terminal, |a| a).into());
    let out = h.send(key(Key::Named(Named::PageUp), None, Modifiers::SHIFT));
    assert!(matches!(
        out.actions.as_slice(),
        [Action::Scroll(ScrollTo::PageUp)]
    ));
    drop(h);
    terminal.feed(b"\x1b[?1049h");
    let mut h = Harness::new(terminal_view(&terminal, |a| a).into());
    let out = h.send(key(Key::Named(Named::PageUp), None, Modifiers::SHIFT));
    assert_eq!(typed(&out.actions), b"\x1b[5;2~", "the program gets it");
}

#[test]
fn the_first_frame_asks_for_the_grid_that_fits_and_only_once() {
    let terminal = focused_terminal(Config::default());
    let mut h = Harness::new(
        terminal_view(&terminal, |a| a)
            .padding(0)
            .scale_factor(1.0)
            .into(),
    );
    let first = h.frame();
    let resize = first.actions.iter().find_map(|a| match a {
        Action::Resize(m) => Some(*m),
        _ => None,
    });
    let m = resize.expect("a resize request");
    assert!(m.columns > 80 && m.rows > 24, "{m:?}");
    // The cell size is what the widget measured, and the grid fits the box.
    assert!(m.columns as f32 * m.cell_width <= SIZE.width);
    assert!(m.rows as f32 * m.cell_height <= SIZE.height);
    // The app has not applied it yet: the next frame does not ask again.
    let second = h.frame();
    assert!(
        !second
            .actions
            .iter()
            .any(|a| matches!(a, Action::Resize(_)))
    );
}

#[test]
fn applying_the_resize_gives_the_pty_one_event() {
    let mut terminal = focused_terminal(Config::default());
    let action = {
        let mut h = Harness::new(terminal_view(&terminal, |a| a).into());
        h.frame()
            .actions
            .into_iter()
            .find(|a| matches!(a, Action::Resize(_)))
            .unwrap()
    };
    let events = terminal.perform(action);
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, TerminalEvent::Resize(_)))
            .count(),
        1
    );
    // Once the grid is that size, a frame has nothing to ask.
    let mut h = Harness::new(terminal_view(&terminal, |a| a).into());
    assert!(
        !h.frame()
            .actions
            .iter()
            .any(|a| matches!(a, Action::Resize(_)))
    );
}

#[test]
fn mouse_presses_and_the_wheel_become_pointer_actions() {
    let terminal = focused_terminal(Config::default());
    let mut h = Harness::new(terminal_view(&terminal, |a| a).into());
    let down = h.send(Event::Mouse(mouse::Event::ButtonPressed(
        mouse::Button::Left,
    )));
    assert!(matches!(
        down.actions.as_slice(),
        [Action::Pointer(Pointer::Down { .. })]
    ));
    assert!(down.captured);
    let up = h.send(Event::Mouse(mouse::Event::ButtonReleased(
        mouse::Button::Left,
    )));
    assert!(matches!(
        up.actions.as_slice(),
        [Action::Pointer(Pointer::Up { .. })]
    ));
    let wheel = h.send(Event::Mouse(mouse::Event::WheelScrolled {
        delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
    }));
    assert!(
        matches!(wheel.actions.as_slice(), [Action::Pointer(Pointer::Wheel { notches, .. })] if *notches == 1.0)
    );
}

#[test]
fn a_press_outside_the_widget_is_not_ours() {
    let terminal = focused_terminal(Config::default());
    let mut h = Harness::new(terminal_view(&terminal, |a| a).into());
    h.cursor = mouse::Cursor::Available(Point::new(SIZE.width + 50.0, 10.0));
    let down = h.send(Event::Mouse(mouse::Event::ButtonPressed(
        mouse::Button::Left,
    )));
    assert!(down.actions.is_empty() && !down.captured);
}

#[test]
fn middle_click_pastes_the_primary_selection_unless_the_program_has_the_mouse() {
    let terminal = focused_terminal(Config::default());
    let mut h = Harness::new(terminal_view(&terminal, |a| a).into());
    h.clipboard = Clip(Some("primary".into()));
    let out = h.send(Event::Mouse(mouse::Event::ButtonPressed(
        mouse::Button::Middle,
    )));
    assert!(matches!(out.actions.as_slice(), [Action::Paste(t)] if t == "primary"));

    let mut terminal = focused_terminal(Config::default());
    terminal.feed(b"\x1b[?1000h\x1b[?1006h");
    let mut h = Harness::new(terminal_view(&terminal, |a| a).into());
    h.clipboard = Clip(Some("primary".into()));
    let out = h.send(Event::Mouse(mouse::Event::ButtonPressed(
        mouse::Button::Middle,
    )));
    assert!(matches!(
        out.actions.as_slice(),
        [Action::Pointer(Pointer::Down { .. })]
    ));
}

#[test]
fn focus_changes_of_the_window_are_reported() {
    let terminal = focused_terminal(Config::default());
    let mut h = Harness::new(terminal_view(&terminal, |a| a).into());
    let out = h.send(Event::Window(window::Event::Unfocused));
    assert!(matches!(
        out.actions.as_slice(),
        [Action::WindowFocus(false)]
    ));
    let out = h.send(Event::Window(window::Event::Focused));
    assert!(matches!(
        out.actions.as_slice(),
        [Action::WindowFocus(true)]
    ));
}

#[test]
fn a_file_dropped_on_the_terminal_is_reported() {
    let terminal = focused_terminal(Config::default());
    let mut h = Harness::new(terminal_view(&terminal, |a| a).into());
    let out = h.send(Event::Window(window::Event::FileDropped(
        "/tmp/a b.txt".into(),
    )));
    assert!(
        matches!(out.actions.as_slice(), [Action::FileDropped(p)] if p.to_str() == Some("/tmp/a b.txt"))
    );
    h.cursor = mouse::Cursor::Available(Point::new(SIZE.width + 50.0, 10.0));
    let out = h.send(Event::Window(window::Event::FileDropped(
        "/tmp/other".into(),
    )));
    assert!(out.actions.is_empty());
}

#[test]
fn the_physical_key_of_another_layout_still_binds() {
    let terminal = focused_terminal(Config::default());
    let mut h = Harness::new(terminal_view(&terminal, |a| a).into());
    // Ctrl+Shift+C on a Russian layout: the key names `с`, the physical key is C.
    let event = Event::Keyboard(keyboard::Event::KeyPressed {
        key: Key::Character("с".into()),
        modified_key: Key::Character("С".into()),
        physical_key: Physical::Code(Code::KeyC),
        location: Location::Standard,
        modifiers: Modifiers::CTRL | Modifiers::SHIFT,
        text: Some("\u{3}".into()),
        repeat: false,
    });
    let out = h.send(event);
    assert!(matches!(out.actions.as_slice(), [Action::Copy]));
}

// ---- Links ---------------------------------------------------------------------

/// A terminal that printed a line with a URL in it, with the grid the widget
/// asked for applied, so that its cells are where the widget measured them.
fn terminal_with_a_link() -> (Terminal, hangar_term::Metrics) {
    let mut terminal = focused_terminal(Config::default());
    terminal.feed(b"see https://hangar.ai/docs now\r\n");
    let metrics = {
        let mut h = Harness::new(
            terminal_view(&terminal, |a| a)
                .padding(0)
                .scale_factor(1.0)
                .into(),
        );
        h.frame()
            .actions
            .into_iter()
            .find_map(|a| match a {
                Action::Resize(m) => Some(m),
                _ => None,
            })
            .expect("a resize request")
    };
    terminal.perform(Action::Resize(metrics));
    (terminal, metrics)
}

/// The middle of cell (`column`, `row`).
fn cell_centre(m: hangar_term::Metrics, column: usize, row: usize) -> Point {
    Point::new(
        (column as f32 + 0.5) * m.cell_width,
        (row as f32 + 0.5) * m.cell_height,
    )
}

#[test]
fn a_link_under_the_pointer_looks_clickable_without_a_key_held() {
    let (terminal, m) = terminal_with_a_link();
    let mut h = Harness::new(
        terminal_view(&terminal, |a| a)
            .padding(0)
            .scale_factor(1.0)
            .into(),
    );
    h.frame();
    h.point_at(cell_centre(m, 1, 0));
    assert_eq!(h.interaction(), mouse::Interaction::Text, "plain text");
    h.point_at(cell_centre(m, 12, 0));
    assert_eq!(
        h.interaction(),
        mouse::Interaction::Pointer,
        "a click opens it"
    );
    // Shift is a selection's: the click would not open it, so it does not
    // promise to.
    h.send(Event::Keyboard(keyboard::Event::ModifiersChanged(
        Modifiers::SHIFT,
    )));
    assert_eq!(h.interaction(), mouse::Interaction::Text);
    h.send(Event::Keyboard(keyboard::Event::ModifiersChanged(
        Modifiers::CTRL | Modifiers::SHIFT,
    )));
    assert_eq!(h.interaction(), mouse::Interaction::Pointer);
}

#[test]
fn a_link_is_not_clickable_over_a_program_that_has_the_mouse() {
    let (mut terminal, m) = terminal_with_a_link();
    terminal.feed(b"\x1b[?1000h\x1b[?1006h");
    let mut h = Harness::new(
        terminal_view(&terminal, |a| a)
            .padding(0)
            .scale_factor(1.0)
            .into(),
    );
    h.frame();
    h.point_at(cell_centre(m, 12, 0));
    assert_eq!(h.interaction(), mouse::Interaction::Idle);
}

#[test]
fn a_click_on_a_link_comes_out_of_the_terminal_as_an_open() {
    let (mut terminal, m) = terminal_with_a_link();
    let at = cell_centre(m, 12, 0);
    let actions = {
        let mut h = Harness::new(
            terminal_view(&terminal, |a| a)
                .padding(0)
                .scale_factor(1.0)
                .into(),
        );
        h.frame();
        h.point_at(at);
        let mut actions = h
            .send(Event::Mouse(mouse::Event::ButtonPressed(
                mouse::Button::Left,
            )))
            .actions;
        actions.extend(
            h.send(Event::Mouse(mouse::Event::ButtonReleased(
                mouse::Button::Left,
            )))
            .actions,
        );
        actions
    };
    let mut opened = Vec::new();
    for action in actions {
        for event in terminal.perform(action) {
            if let TerminalEvent::OpenLink(link) = event {
                opened.push(link.uri);
            }
        }
    }
    assert_eq!(opened, ["https://hangar.ai/docs"]);
}

// ---- Looks ---------------------------------------------------------------------

#[test]
fn rows_land_on_whole_physical_pixels_at_the_scale_the_app_gives() {
    // The app hands over the system's scale times the interface zoom; rounding
    // at 1 instead leaves rows of 18.7 physical pixels at a zoom of 1.1, which
    // fall on 18 and 19 in turn.
    for scale in [1.0, 1.1, 1.25, 1.5, 2.0] {
        let terminal = focused_terminal(Config::default());
        let mut h = Harness::new(
            terminal_view(&terminal, |a| a)
                .padding(0)
                .scale_factor(scale)
                .into(),
        );
        let m = h
            .frame()
            .actions
            .into_iter()
            .find_map(|a| match a {
                Action::Resize(m) => Some(m),
                _ => None,
            })
            .expect("a resize request");
        let physical = m.cell_height * scale;
        assert!(
            (physical - physical.round()).abs() < 1e-3,
            "{scale}: a row is {physical} physical pixels"
        );
    }
}

#[test]
fn a_letter_spacing_widens_the_cell_and_leaves_fewer_columns() {
    let measure = |spacing: f32| {
        let mut terminal = focused_terminal(Config::default());
        terminal.set_font(Font::monospace(14.0).with_letter_spacing(spacing));
        let mut h = Harness::new(
            terminal_view(&terminal, |a| a)
                .padding(0)
                .scale_factor(1.0)
                .into(),
        );
        h.frame()
            .actions
            .into_iter()
            .find_map(|a| match a {
                Action::Resize(m) => Some(m),
                _ => None,
            })
            .expect("a resize request")
    };
    let plain = measure(0.0);
    let wide = measure(4.0);
    let tight = measure(-1.0);
    assert!((wide.cell_width - plain.cell_width - 4.0).abs() < 0.01);
    assert!((plain.cell_width - tight.cell_width - 1.0).abs() < 0.01);
    assert_eq!(wide.cell_height, plain.cell_height, "rows are not touched");
    assert!(wide.columns < plain.columns && plain.columns < tight.columns);
    // The grid still fits the box.
    assert!(wide.columns as f32 * wide.cell_width <= SIZE.width);
}

#[test]
fn a_letter_spacing_sets_each_character_apart_by_that_much() {
    // The distance between the first ink of two neighbouring capitals is a
    // cell wide, so the spacing is the difference between two terminals.
    let distance = |spacing: f32| {
        let mut terminal = focused_terminal(Config::default());
        terminal.set_font(Font::monospace(14.0).with_letter_spacing(spacing));
        terminal.feed(b"\x1b[?25lHH");
        let mut h = Harness::new(
            terminal_view(&terminal, |a| a)
                .padding(0)
                .scale_factor(1.0)
                .scrollbar(false)
                .into(),
        );
        h.frame();
        let background = rgb8(terminal.palette().background);
        let ink = h.draw().ink(10, background);
        // The crossbar joins the stems of an H, so each H is one run of ink.
        assert_eq!(ink.len(), 2, "{ink:?}");
        (ink[1].start as f32 - ink[0].start as f32, ink.len())
    };
    let (plain, plain_runs) = distance(0.0);
    let (wide, wide_runs) = distance(6.0);
    assert_eq!(plain_runs, wide_runs, "the same glyphs are drawn");
    assert!((wide - plain - 6.0).abs() <= 1.5, "{plain} -> {wide}");
}

#[test]
fn a_dimmed_terminal_lets_what_is_behind_it_show_through() {
    let terminal = focused_terminal(Config::default());
    let background = rgb8(terminal.palette().background);
    let behind = Color::from_rgb8(255, 255, 255);

    let mut h = Harness::new(
        terminal_view(&terminal, |a| a)
            .padding(0)
            .scale_factor(1.0)
            .into(),
    );
    assert_eq!(h.draw().at(700, 400), background);

    let mut h = Harness::new(
        terminal_view(&terminal, |a| a)
            .padding(0)
            .scale_factor(1.0)
            .dimmed(behind, 0.94)
            .into(),
    );
    let dim = h.draw().at(700, 400);
    for channel in 0..3 {
        let expected = background[channel] as f32 * 0.94 + 255.0 * 0.06;
        assert!(
            (dim[channel] as f32 - expected).abs() <= 1.5,
            "{dim:?} against {background:?}"
        );
    }

    // Opaque is as if it were not dimmed.
    let mut h = Harness::new(
        terminal_view(&terminal, |a| a)
            .padding(0)
            .scale_factor(1.0)
            .dimmed(behind, 1.0)
            .into(),
    );
    assert_eq!(h.draw().at(700, 400), background);
}
