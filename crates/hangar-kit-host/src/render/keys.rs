//! A tab's keyboard shortcuts: a wrapper round its page that sends the kit
//! the shortcuts it listens for, pressed while the person is in the tab.
//!
//! "In the tab" is where the last press of the pointer was: a tab beside a
//! terminal does not take the terminal's Ctrl+S. It starts as in, since a
//! tab just opened is where the person looks. A key the page's own widgets
//! used (Ctrl+C in a field, Enter in a menu) is theirs, and goes no further:
//! the wrapper looks at a key only after them.

use iced::advanced::layout::{self, Layout};
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget};
use iced::keyboard::{self, Key, Modifiers, key::Named};
use iced::{Element, Event, Length, Rectangle, Size, Vector, mouse, touch};

use hangar_kit_ui::keys::{Key as Wanted, NamedKey, Shortcut};

use crate::message::KitMessage;

/// `content`, sending [`KitMessage::Shortcut`] for each of `specs` pressed
/// in it. Specs that do not read are left out.
pub(crate) fn shortcuts<'a>(
    content: Element<'a, KitMessage>,
    specs: &[String],
) -> Element<'a, KitMessage> {
    let shortcuts: Vec<(Shortcut, String)> = specs
        .iter()
        .filter_map(|spec| Shortcut::parse(spec).map(|shortcut| (shortcut, spec.clone())))
        .collect();
    if shortcuts.is_empty() {
        return content;
    }
    Element::new(Shortcuts { content, shortcuts })
}

struct Shortcuts<'a> {
    content: Element<'a, KitMessage>,
    shortcuts: Vec<(Shortcut, String)>,
}

/// Whether the person is in the tab.
struct Inside(bool);

/// Whether a key pressed is `wanted`. `mod` is the platform's command key
/// (`Modifiers::command`: Ctrl, Cmd on macOS); the other modifiers must be
/// as written, so `mod+z` is not `mod+shift+z`. A character is compared
/// without regard to case, and shift is not asked of a character that needs
/// it to be typed (`+`) unless the spec says so.
pub(crate) fn matches(wanted: &Shortcut, key: &Key, modifiers: Modifiers) -> bool {
    // On macOS `mod` is Cmd, and Ctrl a modifier of its own; elsewhere `mod`
    // is Ctrl itself, and `ctrl+s` the same shortcut as `mod+s`.
    let (command, ctrl, want_command, want_ctrl) = if cfg!(target_os = "macos") {
        (
            modifiers.logo(),
            modifiers.control(),
            wanted.command,
            wanted.ctrl,
        )
    } else {
        (
            modifiers.control(),
            false,
            wanted.command || wanted.ctrl,
            false,
        )
    };
    if want_command != command || want_ctrl != ctrl || wanted.alt != modifiers.alt() {
        return false;
    }
    match (&wanted.key, key.as_ref()) {
        (Wanted::Char(c), keyboard::key::Key::Character(typed)) => {
            let typed = typed.to_lowercase();
            let mut chars = typed.chars();
            let same = chars.next() == Some(*c) && chars.next().is_none();
            same && (wanted.shift == modifiers.shift() || (!wanted.shift && !c.is_alphanumeric()))
        }
        (Wanted::Named(named), keyboard::key::Key::Named(pressed)) => {
            wanted.shift == modifiers.shift() && named_key(*named) == Some(pressed)
        }
        (Wanted::Char(' '), keyboard::key::Key::Named(Named::Space)) => {
            wanted.shift == modifiers.shift()
        }
        _ => false,
    }
}

fn named_key(named: NamedKey) -> Option<Named> {
    Some(match named {
        NamedKey::Enter => Named::Enter,
        NamedKey::Escape => Named::Escape,
        NamedKey::Tab => Named::Tab,
        NamedKey::Space => Named::Space,
        NamedKey::Backspace => Named::Backspace,
        NamedKey::Delete => Named::Delete,
        NamedKey::Up => Named::ArrowUp,
        NamedKey::Down => Named::ArrowDown,
        NamedKey::Left => Named::ArrowLeft,
        NamedKey::Right => Named::ArrowRight,
        NamedKey::PageUp => Named::PageUp,
        NamedKey::PageDown => Named::PageDown,
        NamedKey::Home => Named::Home,
        NamedKey::End => Named::End,
        NamedKey::F(n) => match n {
            1 => Named::F1,
            2 => Named::F2,
            3 => Named::F3,
            4 => Named::F4,
            5 => Named::F5,
            6 => Named::F6,
            7 => Named::F7,
            8 => Named::F8,
            9 => Named::F9,
            10 => Named::F10,
            11 => Named::F11,
            12 => Named::F12,
            _ => return None,
        },
    })
}

impl Widget<KitMessage, iced::Theme, iced::Renderer> for Shortcuts<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Inside>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Inside(true))
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[self.content.as_widget()]);
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, KitMessage>,
        viewport: &Rectangle,
    ) {
        if let Event::Mouse(mouse::Event::ButtonPressed(_))
        | Event::Touch(touch::Event::FingerPressed { .. }) = event
        {
            tree.state.downcast_mut::<Inside>().0 = cursor.is_over(layout.bounds());
        }
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        if shell.is_event_captured() || !tree.state.downcast_ref::<Inside>().0 {
            return;
        }
        if let Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event
            && let Some((_, spec)) = self
                .shortcuts
                .iter()
                .find(|(shortcut, _)| matches(shortcut, key, *modifiers))
        {
            shell.publish(KitMessage::Shortcut(spec.clone()));
            shell.capture_event();
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, KitMessage, iced::Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}
