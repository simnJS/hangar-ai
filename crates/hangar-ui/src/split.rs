//! A divider between two regions that can be dragged (`.splitter`): the
//! navigator and the grid, the grid and the inspector, a pane and the next.
//!
//! ```ignore
//! use hangar_ui::split::{self, Arrange};
//!
//! split::split(t, navigator, workspace)
//!     .arrange(Arrange::Columns)      // side by side; Rows stacks them
//!     .ratio(self.nav_ratio)          // the first region's share, 0 to 1
//!     .min_first(180.0)
//!     .min_second(400.0)
//!     .on_resize(Msg::NavRatio)       // sent as the divider moves
//!     .on_resize_end(Msg::SaveLayout) // sent when it is let go
//!     .on_reset(Msg::NavRatio(0.2))   // a double click puts it back
//! ```
//!
//! The ratio is the app's: the split asks for a new one and draws the one it
//! has. It fills the room it is given.
//!
//! The divider is a one-pixel line in the separator colour, with a hit area
//! nine pixels wide around it, as the web app had. It turns the accent while
//! the pointer is on it, while it is dragged and while it has the keyboard
//! (twice as thick then, as the web app's ring was). With the keyboard the
//! arrows move it by 16 px, with Shift by 64, and Home and End send it to
//! its limits.

use hangar_theme::Theme;
use iced::advanced::layout::{self, Layout};
use iced::advanced::mouse::{Click, click};
use iced::advanced::overlay;
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::operation::Focusable;
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Renderer as _, Shell, Widget};
use iced::keyboard::{self, Key, key::Named};
use iced::{Background, Element, Event, Length, Point, Rectangle, Size, Vector, mouse};

/// How the two regions lie.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Arrange {
    /// Side by side, with a vertical divider (a `splitter--col`).
    #[default]
    Columns,
    /// One over the other, with a horizontal divider (a `splitter--row`).
    Rows,
}

/// The line between the regions, and the room around it that grabs.
const DIVIDER: f32 = 1.0;
const GRAB: f32 = 9.0;
/// How far an arrow key moves the divider.
const STEP: f32 = 16.0;
const BIG_STEP: f32 = 64.0;

#[must_use]
pub struct Split<'a, M> {
    theme: &'a Theme,
    first: Element<'a, M>,
    second: Element<'a, M>,
    arrange: Arrange,
    ratio: f32,
    min_first: f32,
    min_second: f32,
    on_resize: Option<Box<dyn Fn(f32) -> M + 'a>>,
    on_resize_end: Option<M>,
    on_reset: Option<M>,
}

/// Two regions with a divider between them.
pub fn split<'a, M>(
    t: &'a Theme,
    first: impl Into<Element<'a, M>>,
    second: impl Into<Element<'a, M>>,
) -> Split<'a, M> {
    Split {
        theme: t,
        first: first.into(),
        second: second.into(),
        arrange: Arrange::Columns,
        ratio: 0.5,
        min_first: 0.0,
        min_second: 0.0,
        on_resize: None,
        on_resize_end: None,
        on_reset: None,
    }
}

impl<'a, M> Split<'a, M> {
    pub fn arrange(mut self, arrange: Arrange) -> Self {
        self.arrange = arrange;
        self
    }

    /// The first region's share of the room, from 0 to 1.
    pub fn ratio(mut self, ratio: f32) -> Self {
        self.ratio = ratio;
        self
    }

    /// The least room the first region keeps, in pixels.
    pub fn min_first(mut self, min: f32) -> Self {
        self.min_first = min;
        self
    }

    pub fn min_second(mut self, min: f32) -> Self {
        self.min_second = min;
        self
    }

    /// Sent with the new ratio while the divider is dragged or moved with
    /// the keyboard.
    pub fn on_resize(mut self, on_resize: impl Fn(f32) -> M + 'a) -> Self {
        self.on_resize = Some(Box::new(on_resize));
        self
    }

    /// Sent once when a drag ends: the moment to save the layout.
    pub fn on_resize_end(mut self, message: M) -> Self {
        self.on_resize_end = Some(message);
        self
    }

    /// Sent by a double click on the divider.
    pub fn on_reset(mut self, message: M) -> Self {
        self.on_reset = Some(message);
        self
    }
}

/// The first region's size along the split for a ratio, kept between the two
/// minimums when there is room for both and split in proportion to them when
/// there is not.
pub fn first_size(ratio: f32, usable: f32, min_first: f32, min_second: f32) -> f32 {
    let usable = usable.max(0.0);
    let wanted = ratio.clamp(0.0, 1.0) * usable;
    if min_first + min_second > usable {
        let total = (min_first + min_second).max(f32::EPSILON);
        return usable * min_first / total;
    }
    wanted.clamp(min_first, usable - min_second)
}

/// The ratio for a first region of this size, the inverse of [`first_size`].
pub fn ratio_for(size: f32, usable: f32) -> f32 {
    if usable <= 0.0 {
        return 0.5;
    }
    (size / usable).clamp(0.0, 1.0)
}

#[derive(Debug, Default)]
struct State {
    dragging: bool,
    /// How far the pointer was from the divider's line when it took hold.
    grab_offset: f32,
    hovered: bool,
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

/// The layout of what a region holds, inside the region's own.
fn inner(region: Layout<'_>) -> Layout<'_> {
    region
        .children()
        .next()
        .expect("a split's region always holds its content")
}

impl<M> Split<'_, M> {
    fn main(&self, size: Size) -> f32 {
        match self.arrange {
            Arrange::Columns => size.width,
            Arrange::Rows => size.height,
        }
    }

    /// The divider's line and the area that grabs it, from the first
    /// region's bounds.
    fn handle(&self, first: Rectangle) -> (Rectangle, Rectangle) {
        match self.arrange {
            Arrange::Columns => (
                Rectangle::new(
                    Point::new(first.x + first.width, first.y),
                    Size::new(DIVIDER, first.height),
                ),
                Rectangle::new(
                    Point::new(first.x + first.width - (GRAB - DIVIDER) / 2.0, first.y),
                    Size::new(GRAB, first.height),
                ),
            ),
            Arrange::Rows => (
                Rectangle::new(
                    Point::new(first.x, first.y + first.height),
                    Size::new(first.width, DIVIDER),
                ),
                Rectangle::new(
                    Point::new(first.x, first.y + first.height - (GRAB - DIVIDER) / 2.0),
                    Size::new(first.width, GRAB),
                ),
            ),
        }
    }

    /// The coordinate of a point along the split.
    fn along(&self, point: Point) -> f32 {
        match self.arrange {
            Arrange::Columns => point.x,
            Arrange::Rows => point.y,
        }
    }

    /// Where `size` along the split puts the ratio, given the whole bounds.
    fn ratio_at(&self, size: f32, bounds: Rectangle) -> f32 {
        let usable = self.main(bounds.size()) - DIVIDER;
        let clamped = first_size(
            ratio_for(size, usable),
            usable,
            self.min_first,
            self.min_second,
        );
        ratio_for(clamped, usable)
    }
}

impl<M: Clone> Widget<M, iced::Theme, iced::Renderer> for Split<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.first), Tree::new(&self.second)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[self.first.as_widget(), self.second.as_widget()]);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let size = limits.max();
        let usable = self.main(size) - DIVIDER;
        let first = first_size(self.ratio, usable, self.min_first, self.min_second);
        let second = (usable - first).max(0.0);

        let (first_size, second_size) = match self.arrange {
            Arrange::Columns => (
                Size::new(first, size.height),
                Size::new(second, size.height),
            ),
            Arrange::Rows => (Size::new(size.width, first), Size::new(size.width, second)),
        };
        // A region's content is laid out in the room it may take, and stays
        // as small as it is if it is smaller: only a region that asks to
        // fill does. The region's own node has the whole room, which is what
        // the divider is measured from.
        let room = |size: Size| layout::Limits::new(Size::ZERO, size);
        let (first_at, second_at) = match self.arrange {
            Arrange::Columns => (Point::ORIGIN, Point::new(first + DIVIDER, 0.0)),
            Arrange::Rows => (Point::ORIGIN, Point::new(0.0, first + DIVIDER)),
        };
        let first_content =
            self.first
                .as_widget_mut()
                .layout(&mut tree.children[0], renderer, &room(first_size));
        let first_node =
            layout::Node::with_children(first_size, vec![first_content]).move_to(first_at);
        let second_content =
            self.second
                .as_widget_mut()
                .layout(&mut tree.children[1], renderer, &room(second_size));
        let second_node =
            layout::Node::with_children(second_size, vec![second_content]).move_to(second_at);
        layout::Node::with_children(size, vec![first_node, second_node])
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, iced::Theme, iced::Renderer>> {
        // What floats over either region (a menu, a dialog) is still the
        // window's: the split only has to pass it up.
        let mut regions = layout.children();
        let (first_tree, second_tree) = tree.children.split_at_mut(1);
        let first = self.first.as_widget_mut().overlay(
            &mut first_tree[0],
            inner(regions.next()?),
            renderer,
            viewport,
            translation,
        );
        let second = self.second.as_widget_mut().overlay(
            &mut second_tree[0],
            inner(regions.next()?),
            renderer,
            viewport,
            translation,
        );
        let overlays: Vec<_> = first.into_iter().chain(second).collect();
        (!overlays.is_empty()).then(|| overlay::Group::with_children(overlays).overlay())
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        let state = tree.state.downcast_mut::<State>();
        operation.focusable(None, layout.bounds(), state);
        operation.traverse(&mut |operation| {
            let mut regions = layout.children();
            if let Some(first) = regions.next() {
                self.first.as_widget_mut().operate(
                    &mut tree.children[0],
                    inner(first),
                    renderer,
                    operation,
                );
            }
            if let Some(second) = regions.next() {
                self.second.as_widget_mut().operate(
                    &mut tree.children[1],
                    inner(second),
                    renderer,
                    operation,
                );
            }
        });
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
        let bounds = layout.bounds();
        let mut regions = layout.children();
        let (Some(first), Some(second)) = (regions.next(), regions.next()) else {
            return;
        };
        let (_, grab) = self.handle(first.bounds());
        let over_handle = cursor.is_over(grab);

        // The regions see the event first, so that a button or a field near
        // the divider is not robbed of its press; the divider only takes what
        // lands on its own nine pixels.
        self.first.as_widget_mut().update(
            &mut tree.children[0],
            event,
            inner(first),
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        self.second.as_widget_mut().update(
            &mut tree.children[1],
            event,
            inner(second),
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );

        let state = tree.state.downcast_mut::<State>();
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if over_handle && !shell.is_event_captured() {
                    if let Some(point) = cursor.position() {
                        let (line, _) = self.handle(first.bounds());
                        state.dragging = true;
                        state.focused = true;
                        state.grab_offset = self.along(point) - self.along(line.position());
                        let click = Click::new(point, mouse::Button::Left, state.last_click);
                        state.last_click = Some(click);
                        if click.kind() == click::Kind::Double {
                            state.dragging = false;
                            if let Some(message) = self.on_reset.clone() {
                                shell.publish(message);
                            }
                        }
                        shell.capture_event();
                        shell.request_redraw();
                    }
                } else if state.focused && !over_handle {
                    // A press anywhere else takes the keyboard back.
                    state.focused = false;
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if state.dragging {
                    if let Some(point) = cursor.position() {
                        let along = self.along(point) - self.along(bounds.position());
                        let ratio = self.ratio_at(along - state.grab_offset, bounds);
                        if (ratio - self.ratio).abs() > f32::EPSILON
                            && let Some(on_resize) = &self.on_resize
                        {
                            shell.publish(on_resize(ratio));
                        }
                        shell.capture_event();
                        shell.request_redraw();
                    }
                } else if over_handle != state.hovered {
                    state.hovered = over_handle;
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.dragging => {
                state.dragging = false;
                if let Some(message) = self.on_resize_end.clone() {
                    shell.publish(message);
                }
                shell.capture_event();
                shell.request_redraw();
            }
            Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
                if state.focused && !state.dragging =>
            {
                let usable = self.main(bounds.size()) - DIVIDER;
                let current = first_size(self.ratio, usable, self.min_first, self.min_second);
                let step = if modifiers.shift() { BIG_STEP } else { STEP };
                let (back, forward) = match self.arrange {
                    Arrange::Columns => (Named::ArrowLeft, Named::ArrowRight),
                    Arrange::Rows => (Named::ArrowUp, Named::ArrowDown),
                };
                let target = match key {
                    Key::Named(named) if *named == back => Some(current - step),
                    Key::Named(named) if *named == forward => Some(current + step),
                    Key::Named(Named::Home) => Some(0.0),
                    Key::Named(Named::End) => Some(usable),
                    _ => None,
                };
                if let Some(target) = target {
                    let ratio = self.ratio_at(target, bounds);
                    if let Some(on_resize) = &self.on_resize {
                        shell.publish(on_resize(ratio));
                    }
                    if let Some(message) = self.on_resize_end.clone() {
                        shell.publish(message);
                    }
                    shell.capture_event();
                    shell.request_redraw();
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
        let state = tree.state.downcast_ref::<State>();
        let mut regions = layout.children();
        let (Some(first), Some(second)) = (regions.next(), regions.next()) else {
            return mouse::Interaction::None;
        };
        let (_, grab) = self.handle(first.bounds());
        if state.dragging || cursor.is_over(grab) {
            return match self.arrange {
                Arrange::Columns => mouse::Interaction::ResizingHorizontally,
                Arrange::Rows => mouse::Interaction::ResizingVertically,
            };
        }
        self.first
            .as_widget()
            .mouse_interaction(&tree.children[0], inner(first), cursor, viewport, renderer)
            .max(self.second.as_widget().mouse_interaction(
                &tree.children[1],
                inner(second),
                cursor,
                viewport,
                renderer,
            ))
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
        let state = tree.state.downcast_ref::<State>();
        let mut regions = layout.children();
        let (Some(first), Some(second)) = (regions.next(), regions.next()) else {
            return;
        };
        self.first.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            inner(first),
            cursor,
            viewport,
        );
        self.second.as_widget().draw(
            &tree.children[1],
            renderer,
            theme,
            style,
            inner(second),
            cursor,
            viewport,
        );

        // The line is the separator's colour; it turns the accent where the
        // pointer, a drag or the keyboard has it.
        let c = &self.theme.colors;
        let (line, _) = self.handle(first.bounds());
        renderer.fill_quad(
            Quad {
                bounds: line,
                ..Quad::default()
            },
            Background::Color(c.sep),
        );
        if state.hovered || state.dragging || state.focused {
            let thick = if state.focused && !state.dragging {
                2.0
            } else {
                DIVIDER
            };
            let bounds = match self.arrange {
                Arrange::Columns => Rectangle {
                    x: line.x - (thick - DIVIDER),
                    width: thick,
                    ..line
                },
                Arrange::Rows => Rectangle {
                    y: line.y - (thick - DIVIDER),
                    height: thick,
                    ..line
                },
            };
            renderer.fill_quad(
                Quad {
                    bounds,
                    ..Quad::default()
                },
                Background::Color(c.accent),
            );
        }
    }
}

impl<'a, M: Clone + 'a> From<Split<'a, M>> for Element<'a, M> {
    fn from(split: Split<'a, M>) -> Self {
        Element::new(split)
    }
}

#[cfg(test)]
mod tests;
