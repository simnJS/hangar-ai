//! A surface that can be pressed anywhere, with the pointer's place, and given
//! the keyboard: what a card and the dock's grip are.
//!
//! iced's `mouse_area` says a press happened but not where, and a drag has to
//! know where it began (the five pixels that make it a drag, the width the
//! dock had). Its buttons take no keyboard focus either, and a card is opened
//! with Enter or Space once it has it. This wrapper is both, for the little
//! the board needs:
//!
//! - a left press that nothing inside took (an inner button keeps its own
//!   click, which is what `startsDrag` of the web app checked) sends
//!   `on_press` with the pointer's window position;
//! - a second press close behind it sends `on_double_click`;
//! - with an [`id`](Hit::id) it is a focus stop: `operation::focus(id)` and a
//!   click give it the keyboard, Enter and Space send `on_activate`, and any
//!   other key `on_key` knows is answered while it has it;
//! - the pointer over it draws a wash, the keyboard on it a ring.
//!
//! Nothing of this is specific to the board; if another package needs the
//! same it can move to hangar-ui.

use iced::advanced::layout::{self, Layout};
use iced::advanced::mouse::{Click, click};
use iced::advanced::overlay;
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::operation::Focusable;
use iced::advanced::widget::{Id, Operation, Tree, tree};
use iced::advanced::{Clipboard, Renderer as _, Shell, Widget};
use iced::keyboard::{self, Key, key::Named};
use iced::{
    Background, Border, Color, Element, Event, Length, Point, Rectangle, Size, Vector, mouse,
};

use super::drag::starts_drag;

type PressAt<'a, M> = Box<dyn Fn(Point) -> M + 'a>;
type KeyAnswer<'a, M> = Box<dyn Fn(&Key) -> Option<M> + 'a>;

/// How a [`Hit`] looks under the pointer, with the keyboard, and when it is
/// asked to look active (the dock's grip while it is dragged).
#[derive(Debug, Clone, Copy)]
pub struct Look {
    /// The wash over the content while the pointer is on it.
    pub hover: Option<Color>,
    /// The ring around it while it has the keyboard.
    pub ring: Option<Color>,
    /// Corner radius of both.
    pub radius: f32,
    /// The wash and the ring stop this far in from the sides.
    pub inset_x: f32,
}

impl Default for Look {
    fn default() -> Self {
        Look {
            hover: None,
            ring: None,
            radius: 0.0,
            inset_x: 0.0,
        }
    }
}

#[must_use]
pub struct Hit<'a, M> {
    content: Element<'a, M>,
    id: Option<Id>,
    on_press: Option<PressAt<'a, M>>,
    on_double_click: Option<M>,
    on_activate: Option<M>,
    on_key: Option<KeyAnswer<'a, M>>,
    interaction: Option<mouse::Interaction>,
    look: Look,
    active: bool,
}

/// `content`, pressable. Does nothing until given something to send.
pub fn hit<'a, M: Clone + 'a>(content: impl Into<Element<'a, M>>) -> Hit<'a, M> {
    Hit {
        content: content.into(),
        id: None,
        on_press: None,
        on_double_click: None,
        on_activate: None,
        on_key: None,
        interaction: None,
        look: Look::default(),
        active: false,
    }
}

impl<'a, M: Clone + 'a> Hit<'a, M> {
    /// Makes it a focus stop with this identity.
    pub fn id(mut self, id: impl Into<Id>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// A left press that nothing inside took, with the pointer's window
    /// position.
    pub fn on_press(mut self, on_press: impl Fn(Point) -> M + 'a) -> Self {
        self.on_press = Some(Box::new(on_press));
        self
    }

    /// A second press near the first, soon after it.
    pub fn on_double_click(mut self, message: M) -> Self {
        self.on_double_click = Some(message);
        self
    }

    /// Enter or Space while it has the keyboard.
    pub fn on_activate(mut self, message: M) -> Self {
        self.on_activate = Some(message);
        self
    }

    /// The other keys it answers while it has the keyboard.
    pub fn on_key(mut self, on_key: impl Fn(&Key) -> Option<M> + 'a) -> Self {
        self.on_key = Some(Box::new(on_key));
        self
    }

    pub fn interaction(mut self, interaction: mouse::Interaction) -> Self {
        self.interaction = Some(interaction);
        self
    }

    pub fn look(mut self, look: Look) -> Self {
        self.look = look;
        self
    }

    /// Drawn as if the pointer were on it.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    fn focusable(&self) -> bool {
        self.id.is_some() || self.on_activate.is_some()
    }
}

#[derive(Debug, Default)]
struct State {
    focused: bool,
    last_click: Option<Click>,
}

impl Focusable for State {
    fn is_focused(&self) -> bool {
        self.focused
    }

    fn focus(&mut self) {
        self.focused = true;
    }

    fn unfocus(&mut self) {
        self.focused = false;
    }
}

impl<M: Clone> Widget<M, iced::Theme, iced::Renderer> for Hit<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
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

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        if self.focusable() {
            let state = tree.state.downcast_mut::<State>();
            operation.focusable(self.id.as_ref(), layout.bounds(), state);
        }
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
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
        // The content sees the event first, so that a button inside keeps its
        // own click.
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

        let focusable = self.focusable();
        let state = tree.state.downcast_mut::<State>();
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(button)) => {
                let over = cursor.is_over(layout.bounds());
                if !over {
                    // Pressing elsewhere takes the keyboard away.
                    state.focused = false;
                    return;
                }
                if !starts_drag(*button, shell.is_event_captured()) {
                    return;
                }
                if focusable {
                    state.focused = true;
                }
                if let Some(position) = cursor.position() {
                    let click = Click::new(position, mouse::Button::Left, state.last_click);
                    state.last_click = Some(click);
                    if click.kind() == click::Kind::Double
                        && let Some(message) = &self.on_double_click
                    {
                        shell.publish(message.clone());
                        shell.capture_event();
                        return;
                    }
                    if let Some(on_press) = &self.on_press {
                        shell.publish(on_press(position));
                        shell.capture_event();
                    }
                }
            }
            Event::Keyboard(keyboard::Event::KeyPressed { key, .. })
                if state.focused && !shell.is_event_captured() =>
            {
                let answer = match key {
                    Key::Named(Named::Enter | Named::Space) => self.on_activate.clone(),
                    other => self.on_key.as_ref().and_then(|on_key| on_key(other)),
                };
                if let Some(message) = answer {
                    shell.publish(message);
                    shell.capture_event();
                }
            }
            _ => {}
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
        let inside = self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        );
        match self.interaction {
            Some(interaction)
                if inside == mouse::Interaction::None && cursor.is_over(layout.bounds()) =>
            {
                interaction
            }
            _ => inside,
        }
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

        let bounds = layout.bounds();
        let shape = Rectangle {
            x: bounds.x + self.look.inset_x,
            width: (bounds.width - 2.0 * self.look.inset_x).max(0.0),
            ..bounds
        };
        let state = tree.state.downcast_ref::<State>();
        if let Some(wash) = self.look.hover
            && (self.active || cursor.is_over(bounds))
        {
            renderer.fill_quad(
                Quad {
                    bounds: shape,
                    border: Border {
                        radius: self.look.radius.into(),
                        ..Border::default()
                    },
                    ..Quad::default()
                },
                Background::Color(wash),
            );
        }
        if let Some(ring) = self.look.ring
            && state.focused
        {
            renderer.fill_quad(
                Quad {
                    bounds: shape,
                    border: Border {
                        color: ring,
                        width: hangar_theme::tokens::FOCUS_RING_WIDTH,
                        radius: self.look.radius.into(),
                    },
                    ..Quad::default()
                },
                Background::Color(Color::TRANSPARENT),
            );
        }
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

impl<'a, M: Clone + 'a> From<Hit<'a, M>> for Element<'a, M> {
    fn from(hit: Hit<'a, M>) -> Self {
        Element::new(hit)
    }
}
