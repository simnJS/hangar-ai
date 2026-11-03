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
//!
//! [`on_key`] hears every key pressed in the window, which suits a page or a
//! dialog that is only there while it is in use. A field that stays on screen
//! beside the terminals takes [`on_field_key`], which hears keys only while
//! the field has the keyboard: otherwise the arrows and the Escape typed in a
//! pane would be its.

use iced::advanced::layout::{self, Layout};
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::operation::Focusable;
use iced::advanced::widget::{Id, Operation, Tree};
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
        focused_only: false,
    })
}

/// [`on_key`] for a field: the key presses `handler` answers are turned into
/// messages only while the field (or a field inside `content`) has the
/// keyboard.
pub fn on_field_key<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
    handler: impl Fn(&Key, Modifiers) -> Option<M> + 'a,
) -> Element<'a, M> {
    Element::new(OnKey {
        content: content.into(),
        handler: Box::new(move |event| match event {
            keyboard::Event::KeyPressed { key, modifiers, .. } => handler(key, *modifiers),
            _ => None,
        }),
        focused_only: true,
    })
}

/// What answers a keyboard event with a message, or lets it go by.
type Handler<'a, M> = Box<dyn Fn(&keyboard::Event) -> Option<M> + 'a>;

struct OnKey<'a, M> {
    content: Element<'a, M>,
    handler: Handler<'a, M>,
    /// Only while something inside has the keyboard ([`on_field_key`]).
    focused_only: bool,
}

/// Whether a focusable widget the operation walks has the keyboard.
struct AnyFocused(bool);

impl Operation for AnyFocused {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn focusable(&mut self, _id: Option<&Id>, _bounds: Rectangle, state: &mut dyn Focusable) {
        self.0 |= state.is_focused();
    }
}

impl<M> OnKey<'_, M> {
    fn content_focused(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
    ) -> bool {
        let mut found = AnyFocused(false);
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, &mut found);
        found.0
    }
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
        if let Event::Keyboard(keyboard_event) = event
            && (!self.focused_only || self.content_focused(tree, layout, renderer))
            && let Some(message) = (self.handler)(keyboard_event)
        {
            shell.publish(message);
            shell.capture_event();
            return;
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

#[cfg(test)]
mod tests {
    use iced::keyboard::key::Named;
    use iced::widget::{column, container, text_input};
    use iced::{Element, Length};

    use super::*;
    use crate::testing::{Drive, simulate};

    #[derive(Debug, Clone, PartialEq)]
    enum Msg {
        Typed(String),
        Down,
    }

    type KeyHandler = fn(&Key, Modifiers) -> Option<Msg>;
    type Wrap<'a> = fn(Element<'a, Msg>, KeyHandler) -> Element<'a, Msg>;

    /// A field at the top, wrapped as `wrap` does, above room that holds
    /// nothing: where the keyboard is when a pane has it.
    fn screen<'a>(wrap: Wrap<'a>) -> Element<'a, Msg> {
        let field = text_input("filter", "").on_input(Msg::Typed).width(200);
        let down = |key: &Key, _: Modifiers| {
            matches!(key, Key::Named(Named::ArrowDown)).then_some(Msg::Down)
        };
        column![
            wrap(field.into(), down),
            container("").width(Length::Fill).height(Length::Fill),
        ]
        .into()
    }

    fn downs(messages: impl Iterator<Item = Msg>) -> usize {
        messages.filter(|msg| *msg == Msg::Down).count()
    }

    #[test]
    fn a_field_hears_its_keys_only_while_it_has_the_keyboard() {
        let mut ui = simulate(screen(|field, down| on_field_key(field, down)));
        // The keyboard is elsewhere: the arrow is not the field's.
        assert_eq!(ui.key(Named::ArrowDown), iced::event::Status::Ignored);
        // A click in the field gives it the keyboard.
        ui.click_at(20.0, 10.0);
        assert_eq!(ui.key(Named::ArrowDown), iced::event::Status::Captured);
        // A click elsewhere takes it away again.
        ui.click_at(400.0, 400.0);
        assert_eq!(ui.key(Named::ArrowDown), iced::event::Status::Ignored);
        assert_eq!(downs(ui.into_messages()), 1);
    }

    #[test]
    fn a_page_hears_its_keys_wherever_the_keyboard_is() {
        let mut ui = simulate(screen(|field, down| on_key(field, down)));
        assert_eq!(ui.key(Named::ArrowDown), iced::event::Status::Captured);
        assert_eq!(downs(ui.into_messages()), 1);
    }
}
