//! Layers: something that floats over the window and is not part of its
//! layout. Dialogs, the command palette, drop-down menus and context menus
//! are all one [`Layer`] with different settings.
//!
//! iced has no such widget built in: its `pick_list` and `tooltip` make their
//! own overlays by hand. A [`Layer`] wraps the widget it belongs to (the
//! button a menu hangs off, the page a dialog covers) and, when it is given
//! something to show, puts that over everything with the placement and the
//! behaviour its settings ask for.
//!
//! ```ignore
//! // A dialog over the whole window. Closed when `dialog` is `None`.
//! layer::modal(page, self.dialog.as_ref().map(|d| d.view(t)))
//!     .on_dismiss(Msg::CloseDialog)
//!
//! // A menu under a button.
//! layer::dropdown(button, self.menu_open.then(|| menu_element))
//!     .on_dismiss(Msg::CloseMenu)
//! ```
//!
//! A layer is *controlled*: it is open when it has content, and it only asks
//! to be closed (with the `on_dismiss` message) when the person presses
//! Escape or clicks outside. Closing it is the view's job, which is what keeps
//! the state of the window in one place.
//!
//! What a layer does for what it shows:
//!
//! - It is placed by [`Placement`]: centred, a way down from the top, under
//!   its anchor (flipping above when there is no room), or at a point in the
//!   anchor, and always kept inside the window.
//! - A modal layer draws a veil over the window, takes every key, keeps Tab
//!   inside what it shows and puts the keyboard on its first field when it
//!   opens. A press and release that both land on the veil dismiss it; a drag
//!   that starts in a field and ends outside does not.
//! - A popover layer takes no more than the keys its content wants, and
//!   dismisses on a press anywhere else.
//!
//! The content is an ordinary [`Element`], so anything can be in it, a
//! `pick_list` included: overlays of the content are passed on.

use iced::advanced::layout::{self, Layout};
use iced::advanced::mouse;
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::operation::Focusable;
use iced::advanced::widget::{self, Operation, Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget};
use iced::keyboard::{self, Key, key::Named};
use iced::widget::container;
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector};

/// Where a layer puts its content.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Placement {
    /// In the middle of the window: a dialog.
    Center,
    /// Centred across, this fraction of the window's height from the top: the
    /// command palette sits a little above the middle.
    Top(f32),
    /// Under the widget the layer wraps, and above it when there is no room
    /// under it. Starts at its left edge, or ends at its right one.
    Below { align_end: bool },
    /// At a point of the widget the layer wraps, given as an offset from its
    /// top-left corner: where a context menu was asked for.
    At(Vector),
    /// In the window's bottom-right corner, `margin` from its edges: a panel
    /// that stands aside from the work (a setup command's question while its
    /// pane is being watched).
    BottomEnd,
}

/// What a press outside the content does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outside {
    /// Dismisses, and lets the press through to what is under it. A press on
    /// the widget the layer hangs off is left alone when `spare_anchor`: a
    /// button that toggles its menu must not close it on the way down and
    /// open it again on the way up.
    Dismiss { spare_anchor: bool },
    /// The veil of a modal layer: nothing outside is reachable, and the
    /// layer is dismissed by a press and release that both land outside.
    Block,
    /// Nothing: the window stays as reachable as it was, and a press on it is
    /// not the layer's business ([`dock`]).
    Ignore,
}

/// The window's margin that content is kept out of.
const MARGIN: f32 = 8.0;

/// What draws the veil of a modal layer.
type Veil<'a> = Box<dyn Fn(&iced::Theme) -> container::Style + 'a>;

/// A layer over the window. See the [module](self).
#[must_use]
pub struct Layer<'a, M> {
    base: Element<'a, M>,
    content: Option<Element<'a, M>>,
    placement: Placement,
    veil: Option<Veil<'a>>,
    on_dismiss: Option<M>,
    escape: bool,
    outside: Outside,
    trap_keys: bool,
    trap_tab: bool,
    tab_dismisses: bool,
    autofocus: bool,
    gap: f32,
    margin: f32,
    index: f32,
}

impl<'a, M: Clone + 'a> Layer<'a, M> {
    fn new(base: Element<'a, M>, content: Option<Element<'a, M>>, placement: Placement) -> Self {
        Layer {
            base,
            content,
            placement,
            veil: None,
            on_dismiss: None,
            escape: true,
            outside: Outside::Dismiss { spare_anchor: true },
            trap_keys: false,
            trap_tab: false,
            tab_dismisses: false,
            autofocus: false,
            gap: 4.0,
            margin: MARGIN,
            index: 5.0,
        }
    }

    /// Asked for when the person presses Escape or clicks outside. Without
    /// it a layer cannot be dismissed from inside.
    pub fn on_dismiss(mut self, message: M) -> Self {
        self.on_dismiss = Some(message);
        self
    }

    /// Whether Escape dismisses (it does). Off for a surface that steers
    /// Escape itself, the settings clearing their search before they close.
    pub fn escape(mut self, escape: bool) -> Self {
        self.escape = escape;
        self
    }

    /// Where the content goes.
    pub fn placement(mut self, placement: Placement) -> Self {
        self.placement = placement;
        self
    }

    /// The veil drawn over the window, from a theme's `scrim` style.
    pub fn veil(mut self, veil: impl Fn(&iced::Theme) -> container::Style + 'a) -> Self {
        self.veil = Some(Box::new(veil));
        self
    }

    /// The space between the content and what it hangs off.
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }

    /// The space kept free between the content and the window's edges.
    pub fn margin(mut self, margin: f32) -> Self {
        self.margin = margin;
        self
    }

    pub fn outside(mut self, outside: Outside) -> Self {
        self.outside = outside;
        self
    }

    /// Keeps Tab inside the content, wrapping round its ends, instead of
    /// dismissing on it: for a popover that holds a question with buttons or
    /// a small form (the toolbar's reset confirmation) and not a list.
    pub fn tab_inside(mut self) -> Self {
        self.trap_tab = true;
        self.tab_dismisses = false;
        self
    }

    /// Gives the keyboard to the first field of the content when it opens.
    pub fn autofocus(mut self) -> Self {
        self.autofocus = true;
        self
    }
}

/// A dialog: centred over a veil that blocks the window, with the keyboard
/// kept inside it. Escape dismisses.
pub fn modal<'a, M: Clone + 'a>(
    base: impl Into<Element<'a, M>>,
    content: Option<Element<'a, M>>,
) -> Layer<'a, M> {
    let mut layer = Layer::new(base.into(), content, Placement::Center);
    layer.outside = Outside::Block;
    layer.trap_keys = true;
    layer.trap_tab = true;
    layer.autofocus = true;
    layer.margin = 24.0;
    layer.index = 10.0;
    layer
}

/// The command palette's layer: a modal one, a way down from the top.
pub fn palette<'a, M: Clone + 'a>(
    base: impl Into<Element<'a, M>>,
    content: Option<Element<'a, M>>,
) -> Layer<'a, M> {
    let mut layer = modal(base, content);
    layer.placement = Placement::Top(0.14);
    layer.margin = 16.0;
    layer.index = 11.0;
    layer
}

/// A menu or a panel under the widget it hangs off, aligned to its left edge.
/// Escape and Tab dismiss it; a press anywhere else does too.
pub fn dropdown<'a, M: Clone + 'a>(
    base: impl Into<Element<'a, M>>,
    content: Option<Element<'a, M>>,
) -> Layer<'a, M> {
    let mut layer = Layer::new(base.into(), content, Placement::Below { align_end: false });
    layer.tab_dismisses = true;
    layer
}

/// A menu at a point of the widget it is asked for on: the offset of the
/// pointer from its top-left corner.
pub fn context<'a, M: Clone + 'a>(
    base: impl Into<Element<'a, M>>,
    content: Option<Element<'a, M>>,
    at: Vector,
) -> Layer<'a, M> {
    let mut layer = Layer::new(base.into(), content, Placement::At(at));
    layer.outside = Outside::Dismiss {
        spare_anchor: false,
    };
    layer.tab_dismisses = true;
    layer
}

/// A panel in the window's bottom-right corner that leaves the rest of the
/// window as it is: no veil, no key trap, and a press elsewhere does nothing.
/// Escape still dismisses it. The window under it stays interactive, which is
/// what a question about something the person is watching in another pane
/// needs.
pub fn dock<'a, M: Clone + 'a>(
    base: impl Into<Element<'a, M>>,
    content: Option<Element<'a, M>>,
) -> Layer<'a, M> {
    let mut layer = Layer::new(base.into(), content, Placement::BottomEnd);
    layer.outside = Outside::Ignore;
    layer.margin = 16.0;
    // Over a popover, under a dialog.
    layer.index = 9.0;
    layer
}

impl<'a, M: Clone + 'a> Layer<'a, M> {
    /// Aligns a dropdown's right edge with its anchor's, for a button at the
    /// right end of a bar.
    pub fn align_end(mut self) -> Self {
        if let Placement::Below { align_end } = &mut self.placement {
            *align_end = true;
        }
        self
    }
}

/// What a layer remembers between frames.
#[derive(Debug, Default)]
struct State {
    /// The content has been given the keyboard (or the base lost it).
    started: bool,
    /// A press landed on the veil, and is waiting for its release.
    pressed_outside: bool,
}

/// The settings the overlay needs, borrowed from the widget.
struct Settings<'b, M> {
    placement: Placement,
    veil: Option<&'b (dyn Fn(&iced::Theme) -> container::Style + 'b)>,
    on_dismiss: Option<&'b M>,
    escape: bool,
    outside: Outside,
    trap_keys: bool,
    trap_tab: bool,
    tab_dismisses: bool,
    autofocus: bool,
    gap: f32,
    margin: f32,
    index: f32,
}

impl<M: Clone> Widget<M, iced::Theme, iced::Renderer> for Layer<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        let mut children = vec![Tree::new(&self.base)];
        if let Some(content) = &self.content {
            children.push(Tree::new(content));
        }
        children
    }

    fn diff(&self, tree: &mut Tree) {
        match &self.content {
            Some(content) => {
                tree.diff_children(&[self.base.as_widget(), content.as_widget()]);
            }
            None => tree.diff_children(&[self.base.as_widget()]),
        }
    }

    fn size(&self) -> Size<Length> {
        self.base.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.base.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.base
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
        let state = tree.state.downcast_mut::<State>();
        if self.content.is_none() {
            state.started = false;
            state.pressed_outside = false;
        } else if self.trap_keys && !state.started {
            // A dialog has opened over the window: whatever field had the
            // keyboard under it must not keep a caret, nor take the keys.
            state.started = true;
            self.base.as_widget_mut().operate(
                &mut tree.children[0],
                layout,
                renderer,
                &mut Unfocus,
            );
        }
        self.base.as_widget_mut().update(
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
        self.base.as_widget().mouse_interaction(
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
        self.base.as_widget().draw(
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
        self.base
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
        let state = tree.state.downcast_mut::<State>();
        let mut trees = tree.children.iter_mut();
        let base_tree = trees.next()?;
        let content_tree = trees.next();

        let base =
            self.base
                .as_widget_mut()
                .overlay(base_tree, layout, renderer, viewport, translation);

        let ours = match (self.content.as_mut(), content_tree) {
            (Some(content), Some(tree)) => {
                let bounds = layout.bounds();
                let anchor = Rectangle {
                    x: bounds.x + translation.x,
                    y: bounds.y + translation.y,
                    ..bounds
                };
                Some(overlay::Element::new(Box::new(LayerOverlay {
                    content,
                    tree,
                    state,
                    anchor,
                    settings: Settings {
                        placement: self.placement,
                        veil: self.veil.as_deref(),
                        on_dismiss: self.on_dismiss.as_ref(),
                        escape: self.escape,
                        outside: self.outside,
                        trap_keys: self.trap_keys,
                        trap_tab: self.trap_tab,
                        tab_dismisses: self.tab_dismisses,
                        autofocus: self.autofocus,
                        gap: self.gap,
                        margin: self.margin,
                        index: self.index,
                    },
                })))
            }
            _ => None,
        };

        match (base, ours) {
            (None, None) => None,
            (Some(only), None) | (None, Some(only)) => Some(only),
            (Some(base), Some(ours)) => {
                Some(overlay::Group::with_children(vec![base, ours]).overlay())
            }
        }
    }
}

impl<'a, M: Clone + 'a> From<Layer<'a, M>> for Element<'a, M> {
    fn from(layer: Layer<'a, M>) -> Self {
        Element::new(layer)
    }
}

/// The overlay a [`Layer`] puts over the window.
struct LayerOverlay<'a, 'b, M> {
    content: &'b mut Element<'a, M>,
    tree: &'b mut Tree,
    state: &'b mut State,
    /// The widget the layer wraps, in the window's coordinates.
    anchor: Rectangle,
    settings: Settings<'b, M>,
}

impl<M: Clone> LayerOverlay<'_, '_, M> {
    /// The layout of the content, inside the layer's own.
    fn panel<'l>(layout: Layout<'l>) -> Layout<'l> {
        layout
            .children()
            .next()
            .expect("a layer's layout always holds its content")
    }

    fn dismiss(&self, shell: &mut Shell<'_, M>) {
        if let Some(message) = self.settings.on_dismiss {
            shell.publish(message.clone());
        }
    }

    /// Moves the keyboard to the next (or previous) field of the content,
    /// wrapping round its ends. iced's own `focus_next` falls off the end
    /// into "nothing focused", which in a dialog is a hole in the trap.
    fn move_focus(&mut self, layout: Layout<'_>, renderer: &iced::Renderer, step: FocusStep) {
        let mut count = Counter::default();
        self.content
            .as_widget_mut()
            .operate(self.tree, layout, renderer, &mut count);
        if count.total == 0 {
            return;
        }
        let target = match (step, count.focused) {
            (FocusStep::First, _) | (FocusStep::Next, None) => 0,
            (FocusStep::Previous, None) => count.total - 1,
            (FocusStep::Next, Some(at)) => (at + 1) % count.total,
            (FocusStep::Previous, Some(at)) => (at + count.total - 1) % count.total,
        };
        self.content.as_widget_mut().operate(
            self.tree,
            layout,
            renderer,
            &mut FocusAt { target, seen: 0 },
        );
    }
}

/// Where content of this `size` goes in a window of `bounds`, hanging off
/// `anchor` if the placement says so.
///
/// Always inside the window, `margin` from its edges when it fits between
/// them and pinned to the start when it does not.
pub fn position(
    placement: Placement,
    anchor: Rectangle,
    size: Size,
    bounds: Size,
    gap: f32,
    margin: f32,
) -> Point {
    let clamp = |value: f32, room: f32| value.clamp(margin, (room - margin).max(margin));
    match placement {
        Placement::Center => Point::new(
            ((bounds.width - size.width) / 2.0).max(0.0),
            ((bounds.height - size.height) / 2.0).max(0.0),
        ),
        Placement::Top(fraction) => Point::new(
            ((bounds.width - size.width) / 2.0).max(0.0),
            clamp(bounds.height * fraction, bounds.height - size.height),
        ),
        Placement::Below { align_end } => {
            let left = if align_end {
                anchor.x + anchor.width - size.width
            } else {
                anchor.x
            };
            let below = anchor.y + anchor.height + gap;
            let top = if below + size.height + margin <= bounds.height {
                below
            } else {
                // No room under it: above it, or as high as it fits.
                anchor.y - gap - size.height
            };
            Point::new(
                clamp(left, bounds.width - size.width),
                clamp(top, bounds.height - size.height),
            )
        }
        Placement::At(offset) => Point::new(
            clamp(anchor.x + offset.x, bounds.width - size.width),
            clamp(anchor.y + offset.y, bounds.height - size.height),
        ),
        Placement::BottomEnd => Point::new(
            clamp(
                bounds.width - size.width - margin,
                bounds.width - size.width,
            ),
            clamp(
                bounds.height - size.height - margin,
                bounds.height - size.height,
            ),
        ),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusStep {
    First,
    Next,
    Previous,
}

/// Counts the fields that can take the keyboard and finds the one that has it.
#[derive(Debug, Default)]
struct Counter {
    total: usize,
    focused: Option<usize>,
}

impl Operation for Counter {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn focusable(
        &mut self,
        _id: Option<&widget::Id>,
        _bounds: Rectangle,
        state: &mut dyn Focusable,
    ) {
        if state.is_focused() {
            self.focused = Some(self.total);
        }
        self.total += 1;
    }
}

/// Gives the keyboard to the field at a position, and takes it from the others.
struct FocusAt {
    target: usize,
    seen: usize,
}

impl Operation for FocusAt {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn focusable(
        &mut self,
        _id: Option<&widget::Id>,
        _bounds: Rectangle,
        state: &mut dyn Focusable,
    ) {
        if self.seen == self.target {
            state.focus();
        } else {
            state.unfocus();
        }
        self.seen += 1;
    }
}

/// Takes the keyboard from every field.
struct Unfocus;

impl Operation for Unfocus {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn focusable(
        &mut self,
        _id: Option<&widget::Id>,
        _bounds: Rectangle,
        state: &mut dyn Focusable,
    ) {
        state.unfocus();
    }
}

impl<M: Clone> overlay::Overlay<M, iced::Theme, iced::Renderer> for LayerOverlay<'_, '_, M> {
    fn layout(&mut self, renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        let room = Size::new(
            (bounds.width - 2.0 * self.settings.margin).max(0.0),
            (bounds.height - 2.0 * self.settings.margin).max(0.0),
        );
        let limits = layout::Limits::new(Size::ZERO, room);
        let content = self
            .content
            .as_widget_mut()
            .layout(self.tree, renderer, &limits);
        let at = position(
            self.settings.placement,
            self.anchor,
            content.size(),
            bounds,
            self.settings.gap,
            self.settings.margin,
        );

        if self.settings.veil.is_some() || self.settings.outside == Outside::Block {
            // The layer is the whole window, so that its veil covers it and
            // a press anywhere on it is the layer's.
            layout::Node::with_children(bounds, vec![content.move_to(at)])
        } else {
            layout::Node::with_children(content.size(), vec![content.move_to(Point::ORIGIN)])
                .move_to(at)
        }
    }

    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        if let Some(veil) = self.settings.veil {
            container::draw_background(renderer, &veil(theme), layout.bounds());
        }
        self.content.as_widget().draw(
            self.tree,
            renderer,
            theme,
            style,
            Self::panel(layout),
            cursor,
            &Rectangle::with_size(Size::INFINITE),
        );
    }

    fn operate(
        &mut self,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.traverse(&mut |operation| {
            self.content.as_widget_mut().operate(
                self.tree,
                Self::panel(layout),
                renderer,
                operation,
            );
        });
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
    ) {
        let panel = Self::panel(layout);

        // The first thing a dialog does is take the keyboard.
        if self.settings.autofocus && !self.state.started {
            self.state.started = true;
            self.move_focus(panel, renderer, FocusStep::First);
            shell.request_redraw();
        }

        if let Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event {
            match key.as_ref() {
                // Escape is the layer's before it is anyone's: a field that
                // has the keyboard would otherwise swallow it to let go of it.
                Key::Named(Named::Escape) if self.settings.escape => {
                    self.dismiss(shell);
                    shell.capture_event();
                    return;
                }
                Key::Named(Named::Tab) if self.settings.trap_tab => {
                    let step = if modifiers.shift() {
                        FocusStep::Previous
                    } else {
                        FocusStep::Next
                    };
                    self.move_focus(panel, renderer, step);
                    shell.request_redraw();
                    shell.capture_event();
                    return;
                }
                // Tab leaves a menu rather than walking through it, as on
                // every desktop.
                Key::Named(Named::Tab) if self.settings.tab_dismisses => {
                    self.dismiss(shell);
                    shell.capture_event();
                    return;
                }
                _ => {}
            }
        }

        self.content.as_widget_mut().update(
            self.tree,
            event,
            panel,
            cursor,
            renderer,
            clipboard,
            shell,
            &Rectangle::with_size(Size::INFINITE),
        );

        let over_panel = cursor.is_over(panel.bounds());
        let over_anchor = cursor.is_over(self.anchor);
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(_)) if !over_panel => {
                match self.settings.outside {
                    Outside::Dismiss { spare_anchor } => {
                        if !(spare_anchor && over_anchor) && !shell.is_event_captured() {
                            self.dismiss(shell);
                        }
                    }
                    Outside::Block => {
                        self.state.pressed_outside = true;
                        shell.capture_event();
                    }
                    Outside::Ignore => {}
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(_)) => self.state.pressed_outside = false,
            Event::Mouse(mouse::Event::ButtonReleased(_))
                if self.settings.outside == Outside::Block =>
            {
                // Only a click that began outside is a click on the veil: a
                // drag that selected text in a field and let go outside the
                // dialog must not close it mid-edit, and a press on the veil
                // let go inside must not leave a click waiting for later.
                let began_outside = std::mem::take(&mut self.state.pressed_outside);
                if !over_panel {
                    if began_outside {
                        self.dismiss(shell);
                    }
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { .. })
            | Event::Touch(_)
            | Event::Mouse(mouse::Event::CursorMoved { .. })
                if self.settings.outside == Outside::Block && !over_panel =>
            {
                shell.capture_event();
            }
            Event::Keyboard(_) if self.settings.trap_keys => shell.capture_event(),
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let panel = Self::panel(layout);
        if cursor.is_over(panel.bounds()) {
            // Over the content the cursor is the content's; and it is never
            // "none", which is what lets the widgets under it see it.
            return self
                .content
                .as_widget()
                .mouse_interaction(
                    self.tree,
                    panel,
                    cursor,
                    &Rectangle::with_size(Size::INFINITE),
                    renderer,
                )
                .max(mouse::Interaction::Idle);
        }
        if self.settings.outside == Outside::Block {
            mouse::Interaction::Idle
        } else {
            mouse::Interaction::None
        }
    }

    fn overlay<'c>(
        &'c mut self,
        layout: Layout<'c>,
        renderer: &iced::Renderer,
    ) -> Option<overlay::Element<'c, M, iced::Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(
            self.tree,
            Self::panel(layout),
            renderer,
            &Rectangle::with_size(Size::INFINITE),
            Vector::ZERO,
        )
    }

    fn index(&self) -> f32 {
        self.settings.index
    }
}

#[cfg(test)]
mod tests;
