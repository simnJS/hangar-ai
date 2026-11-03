//! Keys for a subtree: a wrapper that turns some key presses into messages
//! before the widget it wraps can use them.
//!
//! ```ignore
//! use hangar_ui::keys::on_key;
//! use iced::keyboard::{Key, key::Named};
//!
//! on_key(search_and_list, |key, _modifiers| match key {
//!     Key::Named(Named::ArrowDown) => Some(Msg::Next),
//!     Key::Named(Named::ArrowUp) => Some(Msg::Previous),
//!     _ => None,
//! })
//! ```
//!
//! A key the handler answers is spent: it is neither passed to the widget
//! inside (a text field would move its caret on an arrow) nor to the window's
//! shortcuts. A key it does not answer goes on as if the wrapper were not
//! there.

use iced::advanced::layout::{self, Layout};
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{Clipboard, Shell, Widget};
use iced::keyboard::{self, Key, Modifiers};
use iced::{Element, Event, Length, Rectangle, Size, Vector, mouse};

/// `content`, with the key presses `handler` answers turned into messages.
pub fn on_key<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
    handler: impl Fn(&Key, Modifiers) -> Option<M> + 'a,
) -> Element<'a, M> {
    on_keyboard(content, move |event| match event {
        keyboard::Event::KeyPressed { key, modifiers, .. } => handler(key, *modifiers),
        _ => None,
    })
}

/// `content`, with every keyboard event `handler` answers turned into a
/// message: presses, releases and changes of modifiers alike. For what has to
/// see the keyboard whole, such as the field that records a shortcut.
pub fn on_keyboard<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
    handler: impl Fn(&keyboard::Event) -> Option<M> + 'a,
) -> Element<'a, M> {
    Element::new(OnKey {
        content: content.into(),
        handler: Box::new(handler),
    })
}

struct OnKey<'a, M> {
    content: Element<'a, M>,
    handler: Box<dyn Fn(&keyboard::Event) -> Option<M> + 'a>,
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for OnKey<'_, M> {
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
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        if let Event::Keyboard(keyboard_event) = event {
            if let Some(message) = (self.handler)(keyboard_event) {
                shell.publish(message);
                shell.capture_event();
                return;
            }
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
    ) -> Option<overlay::Element<'b, M, iced::Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}
