//! The split grid of one workspace (`PaneGrid.tsx`, inventory §4/2–3).
//!
//! The tree is drawn as flat slots in the stable order of the workspace's
//! panes, never in the tree's: a pane keeps its place among the widget's
//! children whatever the arrangement, and the children's state is matched to
//! them by pane id (`diff`), so a terminal's widget state survives every
//! split, move and close. Only the rectangles change.
//!
//! The grid handles what is between and over the panes itself:
//!
//! - the boundaries: dragged to resize (live, from this widget's state, with
//!   one store write on release, as `paintLayout` painted the DOM and
//!   committed once), double-clicked to even out;
//! - a pane dragged by its header onto another: a ghost with its name, a hint
//!   over the half it would take (or the whole pane for a swap), the move on
//!   release; a cancelled gesture (the window losing the focus) moves
//!   nothing;
//! - a double click on a header, which zooms;
//! - a file dragged from the desktop, over the pane it would land on;
//! - the press anywhere in a pane, which gives it the focus;
//! - the zoom: one pane over the whole grid under the zoom bar, the others
//!   kept, not drawn, not resized, and given no events.

use std::collections::HashMap;

use hangar_theme::{Theme, tokens};
use iced::advanced::layout::{self, Layout};
use iced::advanced::mouse::{self, Click};
use iced::advanced::overlay;
use iced::advanced::renderer::{self, Quad};
use iced::advanced::text::{self as core_text, Paragraph as _, Renderer as _, Text};
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Renderer as _, Shell, Widget};
use iced::{
    Background, Border, Color, Element, Event, Length, Pixels, Point, Rectangle, Size, Vector,
    alignment, window,
};

use crate::message::Message;
use crate::panes::Msg;
use crate::panes::geometry::{self, Handle, Path, Rect};
use crate::panes::zoom_bar;
use crate::state::types::{DropZone, SplitDir, SplitNode};
use crate::state::{PaneId, WsId};

/// How far either side of a boundary its grab area reaches.
const GRAB: f32 = 4.5;
/// How far a header has to be dragged before it is a move, not a click.
const DRAG_ARM: f32 = 6.0;
/// The smallest a pane may be dragged to.
const MIN_PANE: f32 = crate::state::layout::MIN_PANE_PX;

/// One pane's slot.
pub struct Slot<'a> {
    pub id: &'a PaneId,
    pub name: &'a str,
    pub rect: Rect,
    pub element: Element<'a, Message>,
    /// The outline drawn over the pane: the focus ring, or the colour of what
    /// it wants while it waits for the person.
    pub ring: Option<Color>,
}

/// The grid of workspace `ws`.
pub struct Grid<'a> {
    ws: &'a WsId,
    theme: &'static Theme,
    keys: Vec<String>,
    /// The panes' elements in stable order, then the zoom bar's.
    children: Vec<Element<'a, Message>>,
    ids: Vec<&'a PaneId>,
    names: Vec<&'a str>,
    rects: Vec<Rect>,
    rings: Vec<Option<Color>>,
    zoomed: Option<usize>,
    tree: SplitNode,
    handles: Vec<Handle>,
    focused: Option<&'a PaneId>,
    /// The size the panes' state last heard of, to tell it when it changes.
    known: Option<Size>,
    drop_label: Box<dyn Fn(usize) -> String + 'a>,
}

impl<'a> Grid<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ws: &'a WsId,
        theme: &'static Theme,
        tree: SplitNode,
        slots: Vec<Slot<'a>>,
        zoomed: Option<&PaneId>,
        zoom_bar: Option<Element<'a, Message>>,
        focused: Option<&'a PaneId>,
        known: Option<Size>,
        drop_label: impl Fn(usize) -> String + 'a,
    ) -> Grid<'a> {
        let handles = geometry::compute_layout(&tree).handles;
        let zoomed = zoomed.and_then(|id| slots.iter().position(|slot| slot.id == id));
        let mut grid = Grid {
            ws,
            theme,
            keys: Vec::with_capacity(slots.len() + 1),
            children: Vec::with_capacity(slots.len() + 1),
            ids: Vec::new(),
            names: Vec::new(),
            rects: Vec::new(),
            rings: Vec::new(),
            zoomed,
            tree,
            handles,
            focused,
            known,
            drop_label: Box::new(drop_label),
        };
        for slot in slots {
            grid.keys.push(slot.id.clone());
            grid.ids.push(slot.id);
            grid.names.push(slot.name);
            grid.rects.push(slot.rect);
            grid.rings.push(slot.ring);
            grid.children.push(slot.element);
        }
        if let (Some(_), Some(bar)) = (grid.zoomed, zoom_bar) {
            // A key no pane id can be: ids are uuids or the like, never empty.
            grid.keys.push(String::new());
            grid.children.push(bar);
        }
        grid
    }

    fn slots(&self) -> usize {
        self.ids.len()
    }

    fn behind(&self, index: usize) -> bool {
        index < self.slots() && self.zoomed.is_some_and(|zoomed| zoomed != index)
    }

    fn publish(&self, shell: &mut Shell<'_, Message>, msg: Msg) {
        shell.publish(Message::Panes(msg));
    }
}

/// A boundary being dragged.
#[derive(Debug, Clone)]
struct Resize {
    path: Path,
    dir: SplitDir,
    latest: f64,
}

/// A press on a header, which becomes a move once dragged far enough.
#[derive(Debug, Clone)]
struct Press {
    slot: usize,
    origin: Point,
    armed: bool,
    target: Option<(usize, DropZone)>,
}

#[derive(Debug, Default)]
struct State {
    keys: Vec<String>,
    resize: Option<Resize>,
    hovered_handle: Option<Path>,
    press: Option<Press>,
    last_click: Option<(usize, Click)>,
    /// Files dragged from the desktop over the grid: how many, and over
    /// which pane. Only the first hover event of a drag carries a file, one
    /// per event, so they are counted.
    files: Option<(usize, Option<usize>)>,
    cursor: Option<Point>,
    sent: Option<Size>,
}

/// A fraction of the grid in pixels, snapped to whole pixels so that panes
/// meet without seams.
fn to_px(rect: Rect, bounds: Rectangle) -> Rectangle {
    let x0 = (bounds.x + rect.left * bounds.width).round();
    let y0 = (bounds.y + rect.top * bounds.height).round();
    let x1 = (bounds.x + (rect.left + rect.width) * bounds.width).round();
    let y1 = (bounds.y + (rect.top + rect.height) * bounds.height).round();
    Rectangle {
        x: x0,
        y: y0,
        width: (x1 - x0).max(0.0),
        height: (y1 - y0).max(0.0),
    }
}

/// The pixels of a boundary's grab area.
fn grab_area(handle: &Handle, bounds: Rectangle) -> Rectangle {
    let line = to_px(handle.rect, bounds);
    match handle.dir {
        SplitDir::Row => Rectangle {
            x: line.x - GRAB,
            width: GRAB * 2.0,
            ..line
        },
        SplitDir::Col => Rectangle {
            y: line.y - GRAB,
            height: GRAB * 2.0,
            ..line
        },
    }
}

/// Where a pointer at `position` puts the boundary of `handle`, kept so that
/// no pane gets narrower than `MIN_PANE` (`startResize`'s `move`).
fn ratio_for(handle: &Handle, bounds: Rectangle, position: Point) -> f64 {
    let area = to_px(handle.bounds, bounds);
    let (start, span, at) = match handle.dir {
        SplitDir::Row => (area.x, area.width, position.x),
        SplitDir::Col => (area.y, area.height, position.y),
    };
    let min = (MIN_PANE / span.max(1.0)).min(0.45);
    f64::from(((at - start) / span.max(1e-6)).clamp(min, 1.0 - min))
}

impl Grid<'_> {
    /// The rectangles, with a boundary being dragged at its live ratio.
    fn live_rects(&self, state: &State) -> Vec<Rect> {
        let Some(resize) = &state.resize else {
            return self.rects.clone();
        };
        let live = geometry::compute_layout(&geometry::with_ratio(
            &self.tree,
            &resize.path,
            resize.latest,
        ));
        self.ids
            .iter()
            .zip(&self.rects)
            .map(|(id, rect)| live.rect(id).unwrap_or(*rect))
            .collect()
    }

    fn live_handles(&self, state: &State) -> Vec<Handle> {
        match &state.resize {
            Some(resize) => {
                geometry::compute_layout(&geometry::with_ratio(
                    &self.tree,
                    &resize.path,
                    resize.latest,
                ))
                .handles
            }
            None => self.handles.clone(),
        }
    }

    /// The slot whose pane is under `position`, ignoring those behind a zoom.
    fn slot_at(&self, layout: Layout<'_>, position: Point) -> Option<usize> {
        layout
            .children()
            .take(self.slots())
            .enumerate()
            .find(|(index, child)| !self.behind(*index) && child.bounds().contains(position))
            .map(|(index, _)| index)
    }
}

impl Widget<Message, iced::Theme, iced::Renderer> for Grid<'_> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            keys: self.keys.clone(),
            ..State::default()
        })
    }

    fn children(&self) -> Vec<Tree> {
        self.children.iter().map(Tree::new).collect()
    }

    /// Children matched by key, not by position: a pane closed or added in
    /// the middle leaves every other pane's terminal state where it was.
    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        let mut old: HashMap<String, Tree> =
            state.keys.drain(..).zip(tree.children.drain(..)).collect();
        let children = self
            .keys
            .iter()
            .zip(&self.children)
            .map(|(key, child)| match old.remove(key) {
                Some(mut kept) => {
                    kept.diff(child.as_widget());
                    kept
                }
                None => Tree::new(child.as_widget()),
            })
            .collect();
        tree.children = children;
        state.keys.clone_from(&self.keys);
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let size = limits.max();
        let bounds = Rectangle::new(Point::ORIGIN, size);
        let rects = self.live_rects(tree.state.downcast_ref::<State>());
        let zoomed = self.zoomed;
        let mut nodes = Vec::with_capacity(self.children.len());
        for (index, (child, child_tree)) in
            self.children.iter_mut().zip(&mut tree.children).enumerate()
        {
            let area = if index >= rects.len() {
                // The zoom bar, along the top.
                Rectangle::new(Point::ORIGIN, Size::new(size.width, zoom_bar::HEIGHT))
            } else if zoomed == Some(index) {
                Rectangle::new(
                    Point::new(0.0, zoom_bar::HEIGHT),
                    Size::new(size.width, (size.height - zoom_bar::HEIGHT).max(0.0)),
                )
            } else {
                // The 1 px separators: a pane off the grid's left or top edge
                // gives its first column or row to one, inside its own slot,
                // so a separator never covers the focus ring of the pane
                // beside it.
                let mut area = to_px(rects[index], bounds);
                if zoomed.is_none() {
                    if rects[index].left > 1e-6 {
                        area.x += 1.0;
                        area.width = (area.width - 1.0).max(0.0);
                    }
                    if rects[index].top > 1e-6 {
                        area.y += 1.0;
                        area.height = (area.height - 1.0).max(0.0);
                    }
                }
                area
            };
            let exact = layout::Limits::new(area.size(), area.size());
            let node = child
                .as_widget_mut()
                .layout(child_tree, renderer, &exact)
                .move_to(area.position());
            nodes.push(node);
        }
        layout::Node::with_children(size, nodes)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.container(None, layout.bounds());
        operation.traverse(&mut |operation| {
            for ((child, state), layout) in self
                .children
                .iter_mut()
                .zip(&mut tree.children)
                .zip(layout.children())
            {
                child
                    .as_widget_mut()
                    .operate(state, layout, renderer, operation);
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
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let zoomed = self.zoomed.is_some();

        {
            let state = tree.state.downcast_mut::<State>();
            match event {
                // The panes' headers fold their details by width: they are
                // told the grid's size whenever it changes.
                Event::Window(window::Event::RedrawRequested(_)) => {
                    let size = bounds.size();
                    if state.sent != Some(size) && self.known != Some(size) {
                        state.sent = Some(size);
                        self.publish(shell, Msg::GridSized(self.ws.clone(), size));
                    }
                }
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    state.cursor = Some(*position)
                }
                _ => {}
            }

            // A boundary being dragged takes the pointer until it is let go.
            if let Some(resize) = &mut state.resize {
                match event {
                    Event::Mouse(mouse::Event::CursorMoved { position }) => {
                        if let Some(handle) = self.handles.iter().find(|h| h.path == resize.path) {
                            resize.latest = ratio_for(handle, bounds, *position);
                        }
                        shell.invalidate_layout();
                        shell.request_redraw();
                        shell.capture_event();
                        return;
                    }
                    // A gesture the window took away commits like a release:
                    // the panes are already drawn at that size.
                    Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                    | Event::Window(window::Event::Unfocused) => {
                        let done = state.resize.take().expect("a resize");
                        self.publish(
                            shell,
                            Msg::SetRatio(self.ws.clone(), done.path, done.latest),
                        );
                        shell.invalidate_layout();
                        shell.request_redraw();
                        shell.capture_event();
                        return;
                    }
                    _ => {}
                }
            }

            // A header pressed, and maybe dragged.
            if let Some(press) = &mut state.press {
                match event {
                    Event::Mouse(mouse::Event::CursorMoved { position }) => {
                        if !press.armed && position.distance(press.origin) >= DRAG_ARM && !zoomed {
                            press.armed = true;
                        }
                        if press.armed {
                            let moving = press.slot;
                            let target = layout
                                .children()
                                .take(self.ids.len())
                                .enumerate()
                                .find(|(index, child)| {
                                    *index != moving && child.bounds().contains(*position)
                                })
                                .map(|(index, child)| {
                                    (
                                        index,
                                        geometry::zone_at(child.bounds(), position.x, position.y),
                                    )
                                });
                            press.target = target;
                            shell.request_redraw();
                            shell.capture_event();
                            return;
                        }
                    }
                    Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                        let done = state.press.take().expect("a press");
                        if done.armed {
                            if let Some((target, zone)) = done.target {
                                self.publish(
                                    shell,
                                    Msg::Move {
                                        ws: self.ws.clone(),
                                        drag: self.ids[done.slot].clone(),
                                        target: self.ids[target].clone(),
                                        zone,
                                    },
                                );
                            }
                            shell.request_redraw();
                            shell.capture_event();
                            return;
                        }
                    }
                    // A move the person never confirmed by letting go moves
                    // nothing: unlike a resize, a drop is a discrete edit.
                    Event::Window(window::Event::Unfocused) => {
                        state.press = None;
                        shell.request_redraw();
                    }
                    _ => {}
                }
            }

            // A press on a boundary starts a resize; a double click evens it.
            if !zoomed
                && let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
                && let Some(position) = cursor.position_over(bounds)
                && let Some(handle) = self
                    .handles
                    .iter()
                    .find(|handle| grab_area(handle, bounds).contains(position))
            {
                let click = Click::new(
                    position,
                    mouse::Button::Left,
                    state.last_click.map(|(_, click)| click),
                );
                state.last_click = Some((usize::MAX, click));
                if click.kind() == mouse::click::Kind::Double {
                    self.publish(
                        shell,
                        Msg::SetRatio(self.ws.clone(), handle.path.clone(), 0.5),
                    );
                } else {
                    state.resize = Some(Resize {
                        path: handle.path.clone(),
                        dir: handle.dir,
                        latest: geometry::ratio_at(&self.tree, &handle.path),
                    });
                }
                shell.capture_event();
                return;
            }

            if let Event::Mouse(mouse::Event::CursorMoved { position }) = event {
                let hovered = (!zoomed && state.press.is_none())
                    .then(|| {
                        self.handles
                            .iter()
                            .find(|handle| grab_area(handle, bounds).contains(*position))
                            .map(|handle| handle.path.clone())
                    })
                    .flatten();
                if hovered != state.hovered_handle {
                    state.hovered_handle = hovered;
                    shell.request_redraw();
                }
            }

            // A file from the desktop, over the pane it would land on.
            match event {
                Event::Window(window::Event::FileHovered(_)) => {
                    let over = state.cursor.and_then(|at| self.slot_at(layout, at));
                    let count = state.files.map_or(0, |(count, _)| count) + 1;
                    state.files = Some((count, over));
                    shell.request_redraw();
                }
                Event::Window(window::Event::FilesHoveredLeft) => {
                    state.files = None;
                    shell.request_redraw();
                }
                _ => {}
            }
        }

        // The panes and the zoom bar, but none behind a zoom: those keep
        // their size and hear nothing until they are shown again.
        for (index, ((child, child_tree), child_layout)) in self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
            .enumerate()
        {
            if index < self.ids.len() && self.zoomed.is_some_and(|zoomed| zoomed != index) {
                continue;
            }
            child.as_widget_mut().update(
                child_tree,
                event,
                child_layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        }

        let state = tree.state.downcast_mut::<State>();
        if let Event::Window(window::Event::FileDropped(_)) = event {
            // The terminal under the pointer took it; the hint can go.
            state.files = None;
            shell.request_redraw();
        }

        // A press in a pane gives it the focus, wherever it landed; on its
        // header and on nothing in it that took the press, it may be the
        // start of a move or the second click of a zoom.
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
            && let Some(position) = cursor.position_over(bounds)
            && let Some(index) = self.slot_at(layout, position)
        {
            let pane = self.ids[index];
            if self.focused != Some(pane) {
                self.publish(shell, Msg::Focus(self.ws.clone(), pane.clone()));
            }
            let slot = layout.children().nth(index).map(|child| child.bounds());
            let on_header =
                slot.is_some_and(|slot| position.y < slot.y + tokens::size::PANE_HEAD_HEIGHT);
            if on_header && !shell.is_event_captured() {
                let previous = state
                    .last_click
                    .filter(|(slot, _)| *slot == index)
                    .map(|(_, click)| click);
                let click = Click::new(position, mouse::Button::Left, previous);
                state.last_click = Some((index, click));
                if click.kind() == mouse::click::Kind::Double {
                    state.press = None;
                    self.publish(shell, Msg::ToggleZoom(self.ws.clone(), pane.clone()));
                } else {
                    state.press = Some(Press {
                        slot: index,
                        origin: position,
                        armed: false,
                        target: None,
                    });
                }
                shell.capture_event();
            }
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
        if let Some(resize) = &state.resize {
            return resize_cursor(resize.dir);
        }
        if state.press.as_ref().is_some_and(|press| press.armed) {
            return mouse::Interaction::Grabbing;
        }
        if let Some(path) = &state.hovered_handle
            && let Some(handle) = self.handles.iter().find(|h| &h.path == path)
        {
            return resize_cursor(handle.dir);
        }
        for (index, ((child, child_tree), child_layout)) in self
            .children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .enumerate()
        {
            if self.behind(index) {
                continue;
            }
            let interaction = child.as_widget().mouse_interaction(
                child_tree,
                child_layout,
                cursor,
                viewport,
                renderer,
            );
            if interaction != mouse::Interaction::None {
                return interaction;
            }
            let bounds = child_layout.bounds();
            if index < self.slots()
                && self.zoomed.is_none()
                && cursor.position().is_some_and(|at| {
                    bounds.contains(at) && at.y < bounds.y + tokens::size::PANE_HEAD_HEIGHT
                })
            {
                return mouse::Interaction::Grab;
            }
        }
        mouse::Interaction::None
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
        let c = &self.theme.colors;
        let bounds = layout.bounds();
        // What shows between the panes is the separator colour.
        fill(renderer, bounds, c.sep, None);

        let dragging = state
            .press
            .as_ref()
            .filter(|press| press.armed)
            .map(|press| press.slot);
        for (index, ((child, child_tree), child_layout)) in self
            .children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .enumerate()
        {
            if self.behind(index) {
                continue;
            }
            child.as_widget().draw(
                child_tree,
                renderer,
                theme,
                style,
                child_layout,
                cursor,
                viewport,
            );
        }

        // Over the panes, in a layer of their own: iced draws a layer's
        // quads before its text, and these have to cover the terminals' text.
        renderer.with_layer(bounds, |renderer| {
            for (index, child_layout) in layout.children().take(self.slots()).enumerate() {
                if self.behind(index) {
                    continue;
                }
                let slot = child_layout.bounds();
                // The pane being dragged stays in place, dimmed, as its own
                // drop origin.
                if dragging == Some(index) {
                    fill(renderer, slot, c.bg.scale_alpha(0.55), None);
                }
                if let Some(ring) = self.rings[index] {
                    fill(renderer, slot, Color::TRANSPARENT, Some((ring, 1.0)));
                }
            }

            // The boundary under the pointer, or being dragged: an accent
            // hairline exactly over the separator.
            let lit = state
                .resize
                .as_ref()
                .map(|resize| &resize.path)
                .or(state.hovered_handle.as_ref());
            if let Some(path) = lit
                && let Some(handle) = self
                    .live_handles(state)
                    .into_iter()
                    .find(|h| &h.path == path)
            {
                let line = to_px(handle.rect, bounds);
                let hairline = match handle.dir {
                    SplitDir::Row => Rectangle { width: 1.0, ..line },
                    SplitDir::Col => Rectangle {
                        height: 1.0,
                        ..line
                    },
                };
                fill(renderer, hairline, c.accent, None);
            }

            // Where a pane being dragged would land.
            if let Some(Press {
                target: Some((target, zone)),
                ..
            }) = state.press.as_ref().filter(|press| press.armed)
                && let Some(slot) = layout.children().nth(*target)
            {
                let rect = slot.bounds();
                let hint = geometry::zone_rect(
                    Rect {
                        left: 0.0,
                        top: 0.0,
                        width: 1.0,
                        height: 1.0,
                    },
                    *zone,
                );
                let area = Rectangle {
                    x: rect.x + hint.left * rect.width,
                    y: rect.y + hint.top * rect.height,
                    width: hint.width * rect.width,
                    height: hint.height * rect.height,
                };
                fill(renderer, area, c.accent_soft, Some((c.accent, 1.0)));
            }

            // A file from the desktop, over the pane it would land on.
            if let Some((count, Some(over))) = state.files
                && let Some(slot) = layout.children().nth(over)
            {
                let rect = slot.bounds();
                fill(renderer, rect, c.accent_soft, Some((c.accent, 1.0)));
                let label = (self.drop_label)(count.max(1));
                chip(
                    renderer,
                    self.theme,
                    &label,
                    rect.center(),
                    true,
                    c.line_strong,
                );
            }
        });

        // The pane on the move, by name, just off the pointer.
        if let Some(slot) = dragging
            && let Some(at) = cursor.position()
        {
            renderer.with_layer(*viewport, |renderer| {
                let name = self.names[slot];
                let origin = Point::new(at.x + 12.0, at.y + 10.0);
                chip(renderer, self.theme, name, origin, false, c.accent);
            });
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        overlay::from_children(
            &mut self.children,
            tree,
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

fn resize_cursor(dir: SplitDir) -> mouse::Interaction {
    // A row split is separated by a vertical bar.
    match dir {
        SplitDir::Row => mouse::Interaction::ResizingHorizontally,
        SplitDir::Col => mouse::Interaction::ResizingVertically,
    }
}

fn fill(
    renderer: &mut iced::Renderer,
    bounds: Rectangle,
    color: Color,
    border: Option<(Color, f32)>,
) {
    let border = border.map_or(Border::default(), |(color, width)| Border {
        color,
        width,
        radius: 0.0.into(),
    });
    renderer.fill_quad(
        Quad {
            bounds,
            border,
            ..Quad::default()
        },
        Background::Color(color),
    );
}

/// A label on the floating surface (`.drag-ghost`, `.grid__filedrop-label`):
/// centred on `at`, or with its corner there.
fn chip(
    renderer: &mut iced::Renderer,
    theme: &Theme,
    label: &str,
    at: Point,
    centred: bool,
    edge: Color,
) {
    let c = &theme.colors;
    let size = tokens::font_size::UI;
    let font = hangar_theme::fonts::ui_semibold();
    let paragraph = <iced::Renderer as core_text::Renderer>::Paragraph::with_text(Text {
        content: label,
        bounds: Size::INFINITE,
        size: Pixels(size),
        line_height: core_text::LineHeight::default(),
        font,
        align_x: core_text::Alignment::Left,
        align_y: alignment::Vertical::Top,
        shaping: core_text::Shaping::Advanced,
        wrapping: core_text::Wrapping::None,
    });
    let text_size = paragraph.min_bounds();
    let padding = Vector::new(10.0, 5.0);
    let box_size = Size::new(
        text_size.width + padding.x * 2.0,
        text_size.height + padding.y * 2.0,
    );
    let origin = if centred {
        Point::new(at.x - box_size.width / 2.0, at.y - box_size.height / 2.0)
    } else {
        at
    };
    let area = Rectangle::new(origin, box_size);
    renderer.fill_quad(
        Quad {
            bounds: area,
            border: Border {
                color: edge,
                width: 1.0,
                radius: tokens::radius::MD.into(),
            },
            ..Quad::default()
        },
        Background::Color(c.chrome_bar),
    );
    renderer.fill_text(
        Text {
            content: label.to_owned(),
            bounds: text_size,
            size: Pixels(size),
            line_height: core_text::LineHeight::default(),
            font,
            align_x: core_text::Alignment::Left,
            align_y: alignment::Vertical::Top,
            shaping: core_text::Shaping::Advanced,
            wrapping: core_text::Wrapping::None,
        },
        origin + padding,
        c.fg,
        area,
    );
}

impl<'a> From<Grid<'a>> for Element<'a, Message> {
    fn from(grid: Grid<'a>) -> Self {
        Element::new(grid)
    }
}
