//! The resize handle on the panel's left edge (`Grip` of `ModelPanel.tsx`):
//! dragging it widens the panel into the terminals, a double click puts the
//! width back to its default, and, with the keyboard on it, the arrow keys
//! move it by 24 points.
//!
//! The widget keeps hold of the pointer while a drag lasts, wherever it goes,
//! and says how far it is from where it took hold; what that does to the width
//! is the panel's (`dragged_width`).

use iced::advanced::layout::{self, Layout};
use iced::advanced::mouse::{self, Click, click};
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget};
use iced::keyboard::{self, Key, key::Named};
use iced::{Background, Color, Element, Event, Length, Point, Rectangle, Size};

/// One press of an arrow key, in points (`KEY_STEP`).
pub const KEY_STEP: f32 = 24.0;

/// How wide the strip that takes hold is.
pub const WIDTH: f32 = 5.0;

/// What the grip says.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GripEvent {
    /// The pointer took hold.
    Begin,
    /// The pointer is this many pixels to the left of where it took hold
    /// (negative to the right): how much wider the panel wants to be.
    Drag(f32),
    End,
    /// A double click.
    Reset,
    /// An arrow key: this many points wider (negative: narrower).
    Nudge(f32),
}

pub struct Grip<'a, M> {
    colour: Color,
    on_event: Box<dyn Fn(GripEvent) -> M + 'a>,
}

impl<'a, M> Grip<'a, M> {
    pub fn new(t: &hangar_theme::Theme, on_event: impl Fn(GripEvent) -> M + 'a) -> Self {
        Grip {
            colour: t.colors.accent,
            on_event: Box::new(on_event),
        }
    }
}

#[derive(Default)]
struct State {
    dragging: bool,
    start: f32,
    focused: bool,
    last_click: Option<Click>,
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Grip<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(WIDTH), Length::Fill)
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(Size::new(WIDTH, limits.max().height))
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let over = cursor.is_over(layout.bounds());
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if over && let Some(point) = cursor.position() {
                    state.focused = true;
                    let click = Click::new(point, mouse::Button::Left, state.last_click);
                    state.last_click = Some(click);
                    if click.kind() == click::Kind::Double {
                        state.dragging = false;
                        shell.publish((self.on_event)(GripEvent::Reset));
                    } else {
                        state.dragging = true;
                        state.start = point.x;
                        shell.publish((self.on_event)(GripEvent::Begin));
                    }
                    shell.capture_event();
                    shell.request_redraw();
                } else if state.focused {
                    state.focused = false;
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) if state.dragging => {
                shell.publish((self.on_event)(GripEvent::Drag(state.start - position.x)));
                shell.capture_event();
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.dragging => {
                state.dragging = false;
                shell.publish((self.on_event)(GripEvent::End));
                shell.capture_event();
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                shell.request_redraw();
            }
            Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) if state.focused => {
                // The panel grows leftwards, into the terminals.
                let step = match key {
                    Key::Named(Named::ArrowLeft) => KEY_STEP,
                    Key::Named(Named::ArrowRight) => -KEY_STEP,
                    _ => return,
                };
                shell.publish((self.on_event)(GripEvent::Nudge(step)));
                shell.capture_event();
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        if state.dragging || cursor.is_over(layout.bounds()) {
            mouse::Interaction::ResizingHorizontally
        } else {
            mouse::Interaction::None
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &iced::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        // A line of 3 px that takes the accent under the pointer, in a drag
        // and with the keyboard on it.
        if state.dragging || state.focused || cursor.is_over(bounds) {
            renderer.fill_quad(
                Quad {
                    bounds: Rectangle::new(
                        Point::new(bounds.x + 1.0, bounds.y),
                        Size::new(3.0, bounds.height),
                    ),
                    ..Quad::default()
                },
                Background::Color(self.colour),
            );
        }
    }
}

impl<'a, M: 'a> From<Grip<'a, M>> for Element<'a, M> {
    fn from(grip: Grip<'a, M>) -> Self {
        Element::new(grip)
    }
}

/// The width a drag, or a key, makes of the panel: what it started at, plus
/// how far the pointer went left (in points), held between the narrowest the
/// panel is allowed and the widest the room leaves it (`clampModelPanel`,
/// `maxWidth`).
pub fn dragged_width(start: f64, moved: f64, max: f64) -> f64 {
    clamp_width(start + moved, max)
}

/// `Math.min(maxWidth, clampModelPanel(width))`: rounded, and no narrower than
/// the least that shows a model more than as a sliver.
pub fn clamp_width(width: f64, max: f64) -> f64 {
    max.min(width.max(model_min()).round())
}

fn model_min() -> f64 {
    f64::from(crate::shell::side_panels::MODEL_MIN)
}

/// The default width a double click goes back to.
pub fn default_width() -> f64 {
    crate::state::Settings::default().model_panel_width
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_widens_the_panel_by_how_far_the_pointer_went_left() {
        assert_eq!(dragged_width(440.0, 60.0, 900.0), 500.0);
        assert_eq!(dragged_width(440.0, -60.0, 900.0), 380.0);
    }

    #[test]
    fn the_width_is_held_between_the_least_and_the_most() {
        assert_eq!(dragged_width(440.0, -400.0, 900.0), 320.0);
        assert_eq!(dragged_width(440.0, 900.0, 700.0), 700.0);
        assert_eq!(clamp_width(412.4, 900.0), 412.0);
    }

    #[test]
    fn a_double_click_goes_back_to_the_default_width() {
        assert_eq!(default_width(), 440.0);
    }
}
