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
//!   committed once), double-clicked to even out, and resized by the keyboard
//!   once one is selected (a click that goes nowhere, or `Mod+Alt+R`): see
//!   [`BoundaryKey`];
//! - a pane dragged by its header onto another: a ghost with its name, a hint
//!   over the half it would take (or the whole pane for a swap), the move on
//!   release; a cancelled gesture (the window losing the focus) moves
//!   nothing;
//! - a double click on a header, which zooms;
//! - a file dragged from the desktop, over the pane it would land on: the one
//!   under the pointer where the pointer is known, else the focused one (the
//!   window system reports no position during a drag: see [`Grid::drop_target`]);
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
use iced::keyboard::key::{Named, Physical};
use iced::keyboard::{self, Key, Modifiers};
use iced::{
    Background, Border, Color, Element, Event, Length, Pixels, Point, Rectangle, Size, Vector,
    alignment, window,
};

use crate::message::Message;
use crate::panes::Msg;
use crate::panes::zoom_bar;
use crate::state::layout::{self as geometry, Handle, Path, Rect};
use crate::state::types::{DropZone, SplitDir, SplitNode};
use crate::state::{PaneId, WsId};

/// How far either side of a boundary its grab area reaches.
const GRAB: f32 = 4.5;
/// How far a header has to be dragged before it is a move, not a click.
const DRAG_ARM: f32 = 6.0;
/// The smallest a pane may be dragged to.
const MIN_PANE: f32 = crate::state::layout::MIN_PANE_PX;
/// How far an arrow key moves a selected boundary (`KEY_STEP_PX`), and how
/// many times that with Shift held.
const KEY_STEP: f32 = 24.0;
const KEY_STEP_SHIFT: f32 = 5.0;

/// One pane's slot.
pub struct Slot<'a> {
    pub id: &'a PaneId,
    pub name: &'a str,
    pub rect: Rect,
    pub element: Element<'a, Message>,
    /// The outline drawn over the pane: the focus ring, or the colour of what
    /// it wants while it waits for the person.
    pub ring: Option<Color>,
    /// A terminal, which takes the files dropped on it; a view pane (what a
    /// preview shows) takes none.
    pub terminal: bool,
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
    terminals: Vec<bool>,
    zoomed: Option<usize>,
    tree: SplitNode,
    handles: Vec<Handle>,
    focused: Option<&'a PaneId>,
    /// The keyboard is somewhere else than the grid (the sidebar's tree, a
    /// field): a press on the focused pane has to bring it back too.
    keyboard_elsewhere: bool,
    /// The size the panes' state last heard of, to tell it when it changes.
    known: Option<Size>,
    drop_label: Box<dyn Fn(usize) -> String + 'a>,
    /// What names a boundary on screen while the keyboard has it ("Resize
    /// Ava ↔ Leo"), when the keyboard may have one at all: the grid is where
    /// the keys go (no dialog open, no field with them).
    boundary_label: Option<BoundaryLabel<'a>>,
}

/// What names a boundary: the panes on its two sides, in words.
type BoundaryLabel<'a> = Box<dyn Fn(&Handle) -> String + 'a>;

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
            terminals: Vec::new(),
            zoomed,
            tree,
            handles,
            focused,
            keyboard_elsewhere: false,
            known,
            drop_label: Box::new(drop_label),
            boundary_label: None,
        };
        for slot in slots {
            grid.keys.push(slot.id.clone());
            grid.ids.push(slot.id);
            grid.names.push(slot.name);
            grid.rects.push(slot.rect);
            grid.rings.push(slot.ring);
            grid.terminals.push(slot.terminal);
            grid.children.push(slot.element);
        }
        if let (Some(_), Some(bar)) = (grid.zoomed, zoom_bar) {
            // A key no pane id can be: ids are uuids or the like, never empty.
            grid.keys.push(String::new());
            grid.children.push(bar);
        }
        grid
    }

    /// Lets the keyboard select a boundary and resize it (`Mod+Alt+R`, or a
    /// click on it), with `label` naming it on screen. Off by default: the
    /// grid takes keys only when the app says it is where they go.
    pub fn boundary_keys(mut self, label: impl Fn(&Handle) -> String + 'a) -> Self {
        self.boundary_label = Some(Box::new(label));
        self
    }

    /// Says the keyboard is not the grid's, so a press on the focused pane
    /// asks for the focus as a press on any other one does.
    pub fn keyboard_elsewhere(mut self, elsewhere: bool) -> Self {
        self.keyboard_elsewhere = elsewhere;
        self
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
    /// Where the boundary was, to tell a click from a drag.
    start: f64,
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
    sent: Option<Size>,
    /// The boundary the keyboard has: it takes the arrows, Home, End, Enter
    /// and Tab until another key, a click elsewhere, Escape or the chord
    /// again gives the keyboard back.
    selected: Option<Path>,
}

/// What a key does while the keyboard has a boundary (`resizeByKey` of
/// `PaneGrid.tsx`, which a focused separator answered). The old terminal made
/// every separator a tab stop; iced has no focus for them, so the keyboard
/// reaches one with the chord [`ENTER_BOUNDARY`] or a click, and gives it
/// back with Escape, the chord again, or any key that is not one of these:
/// that one goes on to the terminal as if nothing had happened.
#[derive(Debug, Clone, Copy, PartialEq)]
enum BoundaryKey {
    /// Take the keyboard for the boundary nearest the focused pane, or give it
    /// back.
    Toggle,
    /// Move the boundary by this many pixels, negative toward the top or
    /// left.
    Nudge(f32),
    /// As far as the panes allow, up or left (Home), down or right (End).
    Min,
    Max,
    /// Evenly (Enter).
    Even,
    /// The next or the previous boundary of the grid (Tab, Shift+Tab).
    Step(isize),
    /// Escape: the keyboard goes back to the terminal, and the key is spent.
    Leave,
}

/// The chord that gives the keyboard to a boundary: Mod+Alt+R, for resize.
/// Mod is Ctrl, or Cmd on a Mac, as everywhere in the keymap; the letter is
/// the Latin one of the physical key, so it works on every layout.
fn is_boundary_chord(key: &Key, physical: Physical, modifiers: Modifiers) -> bool {
    modifiers == Modifiers::COMMAND | Modifiers::ALT
        && key
            .to_latin(physical)
            .is_some_and(|c| c.eq_ignore_ascii_case(&'r'))
}

/// Whether `key` is one that changes what the next key means and sends
/// nothing itself.
fn is_modifier_key(key: &Key) -> bool {
    matches!(
        key,
        Key::Named(
            Named::Shift
                | Named::Control
                | Named::Alt
                | Named::AltGraph
                | Named::Super
                | Named::Meta
                | Named::Hyper
                | Named::CapsLock
                | Named::NumLock
                | Named::ScrollLock
                | Named::Fn
                | Named::FnLock
        )
    )
}

/// What `key` means to a boundary that runs in direction `dir` and has the
/// keyboard; `None` for the keys that are not for it.
fn boundary_key(
    key: &Key,
    physical: Physical,
    modifiers: Modifiers,
    dir: SplitDir,
) -> Option<BoundaryKey> {
    if is_boundary_chord(key, physical, modifiers) {
        return Some(BoundaryKey::Toggle);
    }
    if modifiers.control() || modifiers.alt() || modifiers.logo() {
        return None;
    }
    let step = if modifiers.shift() {
        KEY_STEP * KEY_STEP_SHIFT
    } else {
        KEY_STEP
    };
    // A boundary between side by side panes (a row split) moves along the
    // row; one between stacked panes, along the column.
    let (less, more) = match dir {
        SplitDir::Row => (Named::ArrowLeft, Named::ArrowRight),
        SplitDir::Col => (Named::ArrowUp, Named::ArrowDown),
    };
    let Key::Named(named) = key else {
        return None;
    };
    Some(match *named {
        n if n == less => BoundaryKey::Nudge(-step),
        n if n == more => BoundaryKey::Nudge(step),
        // The arrows across it are the terminal's own.
        Named::Home => BoundaryKey::Min,
        Named::End => BoundaryKey::Max,
        Named::Enter => BoundaryKey::Even,
        Named::Tab => BoundaryKey::Step(if modifiers.shift() { -1 } else { 1 }),
        Named::Escape => BoundaryKey::Leave,
        _ => return None,
    })
}

/// Where a boundary goes for `key`: the ratio, kept so that no pane gets
/// narrower than `MIN_PANE` (the same floor a drag has), or `None` when it
/// would not move. `span` is the size in pixels of what the boundary divides
/// along its axis.
fn ratio_after(current: f64, span: f32, key: BoundaryKey) -> Option<f64> {
    let span = span.max(1.0);
    let min = f64::from((MIN_PANE / span).min(0.45));
    let next = match key {
        BoundaryKey::Nudge(px) => current + f64::from(px / span),
        BoundaryKey::Min => min,
        BoundaryKey::Max => 1.0 - min,
        BoundaryKey::Even => 0.5,
        _ => return None,
    }
    .clamp(min, 1.0 - min);
    ((next - current).abs() > 1e-9).then_some(next)
}

/// A fraction of the grid in pixels, snapped to whole pixels so that panes
/// meet without seams.
fn to_px(rect: Rect, bounds: Rectangle) -> Rectangle {
    let (left, top) = (rect.left as f32, rect.top as f32);
    let (width, height) = (rect.width as f32, rect.height as f32);
    let x0 = (bounds.x + left * bounds.width).round();
    let y0 = (bounds.y + top * bounds.height).round();
    let x1 = (bounds.x + (left + width) * bounds.width).round();
    let y1 = (bounds.y + (top + height) * bounds.height).round();
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
    /// Gives a key to the boundary the keyboard has, or takes the keyboard
    /// for one if that is what the key asks. Whether the key was spent.
    fn boundary_key_pressed(
        &self,
        state: &mut State,
        bounds: Rectangle,
        shell: &mut Shell<'_, Message>,
        key: &Key,
        physical: Physical,
        modifiers: Modifiers,
    ) -> bool {
        // Which boundary the key is about, and what it means to it.
        let current = state
            .selected
            .as_ref()
            .and_then(|path| self.handles.iter().find(|h| &h.path == path));
        let Some(handle) = current else {
            // The keyboard has none: only the chord takes one, the boundary
            // beside the focused pane.
            if !is_boundary_chord(key, physical, modifiers) {
                return false;
            }
            let beside = |handle: &&Handle| {
                self.focused
                    .is_some_and(|pane| handle.before.contains(pane) || handle.after.contains(pane))
            };
            let chosen = self
                .handles
                .iter()
                .find(beside)
                .or_else(|| self.handles.first());
            state.selected = chosen.map(|handle| handle.path.clone());
            shell.request_redraw();
            return state.selected.is_some();
        };
        let Some(action) = boundary_key(key, physical, modifiers, handle.dir) else {
            // Holding a modifier down to press Shift+Arrow is not another
            // key: it changes nothing, and the terminal has no use for it.
            if is_modifier_key(key) {
                return false;
            }
            // Any other key is the terminal's, and ends this.
            state.selected = None;
            shell.request_redraw();
            return false;
        };
        match action {
            BoundaryKey::Toggle | BoundaryKey::Leave => {
                state.selected = None;
            }
            BoundaryKey::Step(by) => {
                let at = self
                    .handles
                    .iter()
                    .position(|h| h.path == handle.path)
                    .unwrap_or(0) as isize;
                let next = (at + by).rem_euclid(self.handles.len() as isize) as usize;
                state.selected = Some(self.handles[next].path.clone());
            }
            moving => {
                let area = to_px(handle.bounds, bounds);
                let span = match handle.dir {
                    SplitDir::Row => area.width,
                    SplitDir::Col => area.height,
                };
                let current = geometry::ratio_at(&self.tree, &handle.path);
                if let Some(ratio) = ratio_after(current, span, moving) {
                    self.publish(
                        shell,
                        Msg::SetRatio(self.ws.clone(), handle.path.clone(), ratio),
                    );
                }
            }
        }
        shell.request_redraw();
        true
    }

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
            .map(|(id, rect)| live.rects.get(*id).copied().unwrap_or(*rect))
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

    /// The pane a file dragged from the desktop is over, or would land on.
    ///
    /// The window systems report no position while a file is dragged over a
    /// window: winit gives `FileHovered` once and `FileDropped` at the end,
    /// and no pointer movement between them (the system holds the pointer),
    /// so the pointer iced knows is the one from before the drag, if it is
    /// known at all; a drag that began in a file manager finds it gone. When
    /// the pointer is over the grid, the pane under it takes the file; when
    /// it is not known, the focused pane does, which is where the person is
    /// working; when it is known to be elsewhere (over a side panel), no
    /// pane does.
    ///
    /// Only a terminal takes a file: over a view pane, nothing does.
    fn drop_target(&self, layout: Layout<'_>, cursor: mouse::Cursor) -> Option<usize> {
        match cursor.position() {
            Some(at) => self.slot_at(layout, at),
            None => self
                .focused
                .and_then(|focused| self.ids.iter().position(|id| *id == focused))
                .filter(|index| !self.behind(*index)),
        }
        .filter(|index| self.terminals.get(*index).copied().unwrap_or(false))
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
            // The panes' headers fold their details by width: they are told
            // the grid's size whenever it changes.
            if let Event::Window(window::Event::RedrawRequested(_)) = event {
                let size = bounds.size();
                if state.sent != Some(size) && self.known != Some(size) {
                    state.sent = Some(size);
                    self.publish(shell, Msg::GridSized(self.ws.clone(), size));
                }
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
                        if (done.latest - done.start).abs() < 1e-9
                            && self.boundary_label.is_some()
                            && matches!(
                                event,
                                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                            )
                        {
                            // A click that went nowhere moved nothing, so it
                            // writes nothing: the keyboard has the boundary.
                            state.selected = Some(done.path);
                        } else {
                            self.publish(
                                shell,
                                Msg::SetRatio(self.ws.clone(), done.path, done.latest),
                            );
                        }
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
                                    let b = child.bounds();
                                    let rect = Rect {
                                        left: f64::from(b.x),
                                        top: f64::from(b.y),
                                        width: f64::from(b.width),
                                        height: f64::from(b.height),
                                    };
                                    let zone = geometry::zone_at(
                                        rect,
                                        f64::from(position.x),
                                        f64::from(position.y),
                                    );
                                    (index, zone)
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

            // The keyboard: a boundary it has takes its keys before the
            // terminal under the pointer can type them.
            if self.boundary_label.is_some() {
                if zoomed || state.resize.is_some() {
                    state.selected = None;
                }
                if state
                    .selected
                    .as_ref()
                    .is_some_and(|path| !self.handles.iter().any(|h| &h.path == path))
                {
                    // The boundary went (a pane closed, a preset applied).
                    state.selected = None;
                    shell.request_redraw();
                }
                match event {
                    Event::Keyboard(keyboard::Event::KeyPressed {
                        key,
                        physical_key,
                        modifiers,
                        ..
                    }) if !zoomed => {
                        if self.boundary_key_pressed(
                            state,
                            bounds,
                            shell,
                            key,
                            *physical_key,
                            *modifiers,
                        ) {
                            shell.capture_event();
                            return;
                        }
                    }
                    // A click elsewhere gives the keyboard back.
                    Event::Mouse(mouse::Event::ButtonPressed(_)) => {
                        let on_it = cursor.position_over(bounds).is_some_and(|at| {
                            self.handles.iter().any(|handle| {
                                Some(&handle.path) == state.selected.as_ref()
                                    && grab_area(handle, bounds).contains(at)
                            })
                        });
                        if state.selected.is_some() && !on_it {
                            state.selected = None;
                            shell.request_redraw();
                        }
                    }
                    Event::Window(window::Event::Unfocused) if state.selected.is_some() => {
                        state.selected = None;
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
                    let at = geometry::ratio_at(&self.tree, &handle.path);
                    state.selected = None;
                    state.resize = Some(Resize {
                        path: handle.path.clone(),
                        dir: handle.dir,
                        start: at,
                        latest: at,
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
                    let over = self.drop_target(layout, cursor);
                    let count = state.files.map_or(0, |(count, _)| count) + 1;
                    state.files = Some((count, over));
                    shell.request_redraw();
                }
                Event::Window(window::Event::FilesHoveredLeft) => {
                    state.files = None;
                    shell.request_redraw();
                }
                // The grid says which pane the file is for, as it has said
                // which one the hint is over, so the panes do not each decide
                // by a pointer that may not be known. Where there is none, the
                // terminal under it takes the drop as it did.
                Event::Window(window::Event::FileDropped(path)) => {
                    if let Some(index) = self.drop_target(layout, cursor) {
                        self.publish(
                            shell,
                            Msg::Term(
                                self.ids[index].clone(),
                                hangar_term::Action::FileDropped(path.clone()),
                            ),
                        );
                        state.files = None;
                        shell.request_redraw();
                        shell.capture_event();
                        return;
                    }
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
            // The pane the workspace remembers is not always the one with the
            // keyboard: after a click in the sidebar it keeps the arrows.
            if self.focused != Some(pane) || self.keyboard_elsewhere {
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
                .or(state.selected.as_ref())
                .or(state.hovered_handle.as_ref());
            if let Some(path) = lit
                && let Some(handle) = self
                    .live_handles(state)
                    .into_iter()
                    .find(|h| &h.path == path)
            {
                let line = to_px(handle.rect, bounds);
                // The one the keyboard has is thicker, to be told from a
                // boundary that is only hovered.
                let held = state.selected.as_ref() == Some(path) && state.resize.is_none();
                let thickness = if held { 3.0 } else { 1.0 };
                let hairline = match handle.dir {
                    SplitDir::Row => Rectangle {
                        x: line.x - (thickness - 1.0) / 2.0,
                        width: thickness,
                        ..line
                    },
                    SplitDir::Col => Rectangle {
                        y: line.y - (thickness - 1.0) / 2.0,
                        height: thickness,
                        ..line
                    },
                };
                fill(renderer, hairline, c.accent, None);
                // And named, so that it is known which two sides the arrows
                // move (the separator's `aria-label` of the old grid).
                if held && let Some(label) = &self.boundary_label {
                    chip(
                        renderer,
                        self.theme,
                        &label(&handle),
                        to_px(handle.rect, bounds).center(),
                        true,
                        c.accent,
                    );
                }
            }

            // Where a pane being dragged would land.
            if let Some(Press {
                target: Some((target, zone)),
                ..
            }) = state.press.as_ref().filter(|press| press.armed)
                && let Some(slot) = layout.children().nth(*target)
            {
                let rect = slot.bounds();
                let hint = geometry::zone_rect(Rect::FULL, *zone);
                let area = Rectangle {
                    x: rect.x + hint.left as f32 * rect.width,
                    y: rect.y + hint.top as f32 * rect.height,
                    width: hint.width as f32 * rect.width,
                    height: hint.height as f32 * rect.height,
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

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::Space;
    use iced::{Event, Point, Settings};
    use iced_test::Simulator;

    const WINDOW: Size = Size::new(800.0, 600.0);

    fn leaf(id: &str) -> SplitNode {
        SplitNode::Pane { id: id.into() }
    }

    /// Two panes side by side, `a` focused, nothing in them but space.
    fn grid<'a>(ws: &'a WsId, ids: &'a [PaneId; 2], zoomed: Option<&PaneId>) -> Grid<'a> {
        let tree = SplitNode::Split {
            dir: SplitDir::Row,
            ratio: 0.5,
            a: Box::new(leaf(&ids[0])),
            b: Box::new(leaf(&ids[1])),
        };
        let layout = geometry::compute_layout(&tree);
        let slots = ids
            .iter()
            .map(|id| Slot {
                id,
                name: id,
                rect: layout.rects[id],
                element: Space::new().width(Length::Fill).height(Length::Fill).into(),
                ring: None,
                terminal: true,
            })
            .collect();
        Grid::new(
            ws,
            hangar_theme::default_theme(),
            tree,
            slots,
            zoomed,
            None,
            Some(&ids[0]),
            Some(WINDOW),
            |count| count.to_string(),
        )
    }

    fn simulate(grid: Grid<'_>) -> Simulator<'_, Message> {
        Simulator::with_size(Settings::default(), WINDOW, grid)
    }

    fn at(ui: &mut Simulator<'_, Message>, x: f32, y: f32) {
        let position = Point::new(x, y);
        ui.point_at(position);
        ui.simulate([Event::Mouse(mouse::Event::CursorMoved { position })]);
    }

    fn press(ui: &mut Simulator<'_, Message>) {
        ui.simulate([Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Left,
        ))]);
    }

    fn release(ui: &mut Simulator<'_, Message>) {
        ui.simulate([Event::Mouse(mouse::Event::ButtonReleased(
            mouse::Button::Left,
        ))]);
    }

    fn panes(ui: Simulator<'_, Message>) -> Vec<Msg> {
        ui.into_messages()
            .filter_map(|message| match message {
                Message::Panes(msg) => Some(msg),
                _ => None,
            })
            .collect()
    }

    fn ids() -> (WsId, [PaneId; 2]) {
        ("ws".into(), ["a".into(), "b".into()])
    }

    #[test]
    fn a_boundary_dragged_commits_its_ratio_once_on_release() {
        let (ws, ids) = ids();
        let mut ui = simulate(grid(&ws, &ids, None));
        at(&mut ui, 400.0, 300.0);
        press(&mut ui);
        at(&mut ui, 500.0, 300.0);
        at(&mut ui, 600.0, 300.0);
        release(&mut ui);
        let ratios: Vec<f64> = panes(ui)
            .into_iter()
            .filter_map(|msg| match msg {
                Msg::SetRatio(_, path, ratio) if path.is_empty() => Some(ratio),
                _ => None,
            })
            .collect();
        assert_eq!(ratios.len(), 1, "one store write, on release");
        assert!((ratios[0] - 0.75).abs() < 0.01, "{ratios:?}");
    }

    #[test]
    fn a_boundary_keeps_each_pane_at_its_minimum() {
        let (ws, ids) = ids();
        let mut ui = simulate(grid(&ws, &ids, None));
        at(&mut ui, 400.0, 300.0);
        press(&mut ui);
        at(&mut ui, 10.0, 300.0);
        release(&mut ui);
        let ratio = panes(ui).into_iter().find_map(|msg| match msg {
            Msg::SetRatio(_, _, ratio) => Some(ratio),
            _ => None,
        });
        // 150 px of an 800 px split.
        assert!((ratio.unwrap() - 150.0 / 800.0).abs() < 0.01);
    }

    #[test]
    fn a_double_click_on_a_boundary_evens_it_out() {
        let (ws, ids) = ids();
        let mut ui = simulate(grid(&ws, &ids, None));
        at(&mut ui, 400.0, 300.0);
        press(&mut ui);
        release(&mut ui);
        press(&mut ui);
        release(&mut ui);
        assert!(
            panes(ui)
                .iter()
                .any(|msg| matches!(msg, Msg::SetRatio(_, _, ratio) if *ratio == 0.5))
        );
    }

    #[test]
    fn a_header_dragged_onto_another_pane_moves_it_to_that_side() {
        let (ws, ids) = ids();
        let mut ui = simulate(grid(&ws, &ids, None));
        at(&mut ui, 100.0, 10.0);
        press(&mut ui);
        at(&mut ui, 300.0, 200.0);
        at(&mut ui, 750.0, 300.0);
        release(&mut ui);
        let moved = panes(ui).into_iter().find_map(|msg| match msg {
            Msg::Move {
                drag, target, zone, ..
            } => Some((drag, target, zone)),
            _ => None,
        });
        assert_eq!(moved, Some(("a".into(), "b".into(), DropZone::Right)));
    }

    #[test]
    fn a_press_that_does_not_travel_moves_nothing() {
        let (ws, ids) = ids();
        let mut ui = simulate(grid(&ws, &ids, None));
        at(&mut ui, 100.0, 10.0);
        press(&mut ui);
        at(&mut ui, 103.0, 12.0);
        release(&mut ui);
        assert!(!panes(ui).iter().any(|msg| matches!(msg, Msg::Move { .. })));
    }

    #[test]
    fn a_press_on_the_focused_pane_takes_the_keyboard_back_from_elsewhere() {
        let (ws, ids) = ids();
        let focuses = |elsewhere: bool| {
            let mut ui = simulate(grid(&ws, &ids, None).keyboard_elsewhere(elsewhere));
            at(&mut ui, 100.0, 200.0);
            press(&mut ui);
            release(&mut ui);
            panes(ui)
                .iter()
                .filter(|msg| matches!(msg, Msg::Focus(_, pane) if pane == "a"))
                .count()
        };
        // `a` is the workspace's focused pane: with the keyboard on the grid
        // there is nothing to ask for; after a click in the sidebar's tree
        // the press has to bring the arrows back to it.
        assert_eq!(focuses(false), 0);
        assert_eq!(focuses(true), 1);
    }

    #[test]
    fn a_double_click_on_a_header_zooms_and_a_press_focuses() {
        let (ws, ids) = ids();
        let mut ui = simulate(grid(&ws, &ids, None));
        at(&mut ui, 600.0, 10.0);
        press(&mut ui);
        release(&mut ui);
        press(&mut ui);
        release(&mut ui);
        let msgs = panes(ui);
        assert!(
            msgs.iter()
                .any(|msg| matches!(msg, Msg::Focus(_, pane) if pane == "b"))
        );
        assert!(
            msgs.iter()
                .any(|msg| matches!(msg, Msg::ToggleZoom(_, pane) if pane == "b"))
        );
    }

    #[test]
    fn a_zoomed_grid_has_no_boundary_and_no_move() {
        let (ws, ids) = ids();
        let zoomed: PaneId = "a".into();
        let mut ui = simulate(grid(&ws, &ids, Some(&zoomed)));
        at(&mut ui, 400.0, 300.0);
        press(&mut ui);
        at(&mut ui, 600.0, 300.0);
        release(&mut ui);
        at(&mut ui, 100.0, zoom_bar::HEIGHT + 10.0);
        press(&mut ui);
        at(&mut ui, 700.0, 300.0);
        release(&mut ui);
        let msgs = panes(ui);
        assert!(
            !msgs
                .iter()
                .any(|msg| matches!(msg, Msg::SetRatio(..) | Msg::Move { .. }))
        );
    }
}

#[cfg(test)]
mod keyboard_tests {
    use super::*;
    use iced::keyboard::key::Code;
    use iced::widget::Space;
    use iced::{Point, Settings};
    use iced_test::Simulator;

    const WINDOW: Size = Size::new(800.0, 600.0);

    fn leaf(id: &str) -> SplitNode {
        SplitNode::Pane { id: id.into() }
    }

    /// Two panes side by side, `a` focused, with the keyboard allowed (or
    /// not) to reach the boundary between them.
    fn grid<'a>(
        ws: &'a WsId,
        ids: &'a [PaneId; 2],
        zoomed: Option<&PaneId>,
        keys: bool,
    ) -> Grid<'a> {
        let tree = SplitNode::Split {
            dir: SplitDir::Row,
            ratio: 0.5,
            a: Box::new(leaf(&ids[0])),
            b: Box::new(leaf(&ids[1])),
        };
        let layout = geometry::compute_layout(&tree);
        let slots = ids
            .iter()
            .map(|id| Slot {
                id,
                name: id,
                rect: layout.rects[id],
                element: Space::new().width(Length::Fill).height(Length::Fill).into(),
                ring: None,
                terminal: true,
            })
            .collect();
        let grid = Grid::new(
            ws,
            hangar_theme::default_theme(),
            tree,
            slots,
            zoomed,
            None,
            Some(&ids[0]),
            Some(WINDOW),
            |count| count.to_string(),
        );
        if keys {
            grid.boundary_keys(|handle| format!("{:?} | {:?}", handle.before, handle.after))
        } else {
            grid
        }
    }

    fn ids() -> (WsId, [PaneId; 2]) {
        ("ws".into(), ["a".into(), "b".into()])
    }

    fn key(key: Key, physical: Physical, modifiers: Modifiers) -> Event {
        Event::Keyboard(keyboard::Event::KeyPressed {
            modified_key: key.clone(),
            key,
            physical_key: physical,
            location: keyboard::Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        })
    }

    fn unidentified() -> Physical {
        Physical::Unidentified(keyboard::key::NativeCode::Unidentified)
    }

    fn named(named: Named, modifiers: Modifiers) -> Event {
        key(Key::Named(named), unidentified(), modifiers)
    }

    fn chord() -> Event {
        key(
            Key::Character("r".into()),
            Physical::Code(Code::KeyR),
            Modifiers::COMMAND | Modifiers::ALT,
        )
    }

    fn ratios(ui: Simulator<'_, Message>) -> Vec<f64> {
        ui.into_messages()
            .filter_map(|message| match message {
                Message::Panes(Msg::SetRatio(_, _, ratio)) => Some(ratio),
                _ => None,
            })
            .collect()
    }

    fn simulate(grid: Grid<'_>) -> Simulator<'_, Message> {
        Simulator::with_size(Settings::default(), WINDOW, grid)
    }

    fn arrow(key_: Named) -> Event {
        named(key_, Modifiers::empty())
    }

    // The pure part.

    #[test]
    fn an_arrow_along_the_boundary_moves_it_a_step_and_shift_five() {
        let on = |named: Named, mods: Modifiers, dir: SplitDir| {
            boundary_key(&Key::Named(named), unidentified(), mods, dir)
        };
        let none = Modifiers::empty();
        assert_eq!(
            on(Named::ArrowRight, none, SplitDir::Row),
            Some(BoundaryKey::Nudge(24.0))
        );
        assert_eq!(
            on(Named::ArrowLeft, Modifiers::SHIFT, SplitDir::Row),
            Some(BoundaryKey::Nudge(-120.0))
        );
        assert_eq!(
            on(Named::ArrowDown, none, SplitDir::Col),
            Some(BoundaryKey::Nudge(24.0))
        );
        assert_eq!(
            on(Named::ArrowUp, none, SplitDir::Col),
            Some(BoundaryKey::Nudge(-24.0))
        );
        // The arrows across it, and any with Ctrl, Alt or Cmd, are not its.
        assert_eq!(on(Named::ArrowUp, none, SplitDir::Row), None);
        assert_eq!(on(Named::ArrowLeft, none, SplitDir::Col), None);
        assert_eq!(on(Named::ArrowRight, Modifiers::ALT, SplitDir::Row), None);
        assert_eq!(on(Named::ArrowRight, Modifiers::CTRL, SplitDir::Row), None);
        assert_eq!(on(Named::Home, none, SplitDir::Row), Some(BoundaryKey::Min));
        assert_eq!(on(Named::End, none, SplitDir::Row), Some(BoundaryKey::Max));
        assert_eq!(
            on(Named::Enter, none, SplitDir::Row),
            Some(BoundaryKey::Even)
        );
        assert_eq!(
            on(Named::Tab, none, SplitDir::Row),
            Some(BoundaryKey::Step(1))
        );
        assert_eq!(
            on(Named::Tab, Modifiers::SHIFT, SplitDir::Row),
            Some(BoundaryKey::Step(-1))
        );
        assert_eq!(
            on(Named::Escape, none, SplitDir::Row),
            Some(BoundaryKey::Leave)
        );
        assert_eq!(on(Named::PageUp, none, SplitDir::Row), None);
    }

    #[test]
    fn the_chord_is_mod_alt_r_on_any_layout() {
        let mods = Modifiers::COMMAND | Modifiers::ALT;
        let r = Physical::Code(Code::KeyR);
        assert!(is_boundary_chord(&Key::Character("r".into()), r, mods));
        // A Russian layout names the key `к`; the physical key is R.
        assert!(is_boundary_chord(&Key::Character("к".into()), r, mods));
        // Not with another modifier, nor on another key.
        assert!(!is_boundary_chord(
            &Key::Character("r".into()),
            r,
            mods | Modifiers::SHIFT
        ));
        assert!(!is_boundary_chord(
            &Key::Character("r".into()),
            r,
            Modifiers::COMMAND
        ));
        assert!(!is_boundary_chord(
            &Key::Character("t".into()),
            Physical::Code(Code::KeyT),
            mods
        ));
    }

    #[test]
    fn a_boundary_moves_by_the_step_and_stops_where_the_panes_do() {
        // An 800 px split: 24 px is 0.03.
        let next = ratio_after(0.5, 800.0, BoundaryKey::Nudge(24.0)).unwrap();
        assert!((next - 0.53).abs() < 1e-6);
        // No pane gets under 150 px: 0.1875 of 800.
        let min = 150.0 / 800.0;
        assert!((ratio_after(0.5, 800.0, BoundaryKey::Min).unwrap() - min).abs() < 1e-6);
        assert!((ratio_after(0.5, 800.0, BoundaryKey::Max).unwrap() - (1.0 - min)).abs() < 1e-6);
        assert!((ratio_after(0.2, 800.0, BoundaryKey::Nudge(-120.0)).unwrap() - min).abs() < 1e-6);
        // At the floor, pushing on moves nothing, so nothing is written.
        assert_eq!(ratio_after(min, 800.0, BoundaryKey::Nudge(-24.0)), None);
        assert_eq!(ratio_after(0.5, 800.0, BoundaryKey::Even), None);
        assert!((ratio_after(0.3, 800.0, BoundaryKey::Even).unwrap() - 0.5).abs() < 1e-9);
        // A split too small for two minimum panes is held at 0.45.
        let tight = ratio_after(0.5, 200.0, BoundaryKey::Min).unwrap();
        assert!((tight - 0.45).abs() < 1e-6, "{tight}");
    }

    // Through the widget.

    #[test]
    fn the_chord_takes_the_boundary_and_the_arrows_move_it() {
        let (ws, ids) = ids();
        let mut ui = simulate(grid(&ws, &ids, None, true));
        ui.simulate([chord()]);
        ui.simulate([arrow(Named::ArrowRight)]);
        ui.simulate([named(Named::ArrowRight, Modifiers::SHIFT)]);
        let moved = ratios(ui);
        // 24 px, then 120 px, of the 800 px split. The app rebuilds the grid
        // between two keys; here it is not, so each is measured from the half.
        assert_eq!(moved.len(), 2, "{moved:?}");
        assert!((moved[0] - 0.53).abs() < 1e-6, "{moved:?}");
        assert!((moved[1] - 0.65).abs() < 1e-6, "{moved:?}");
    }

    #[test]
    fn holding_shift_down_to_press_an_arrow_does_not_give_the_boundary_back() {
        let (ws, ids) = ids();
        let mut ui = simulate(grid(&ws, &ids, None, true));
        ui.simulate([chord()]);
        // The press of Shift itself comes first, with the modifier not yet
        // reported on it.
        ui.simulate([named(Named::Shift, Modifiers::empty())]);
        ui.simulate([named(Named::ArrowRight, Modifiers::SHIFT)]);
        let moved = ratios(ui);
        assert_eq!(moved.len(), 1, "{moved:?}");
        assert!((moved[0] - 0.65).abs() < 1e-6, "{moved:?}");
    }

    #[test]
    fn home_end_and_enter_go_to_the_ends_and_the_middle() {
        let (ws, ids) = ids();
        let mut ui = simulate(grid(&ws, &ids, None, true));
        ui.simulate([chord()]);
        ui.simulate([arrow(Named::Home)]);
        ui.simulate([arrow(Named::End)]);
        // Even, but the boundary is at the half: nothing to write.
        ui.simulate([arrow(Named::Enter)]);
        let moved = ratios(ui);
        assert_eq!(moved.len(), 2, "{moved:?}");
        assert!((moved[0] - 150.0 / 800.0).abs() < 1e-6);
        assert!((moved[1] - (1.0 - 150.0 / 800.0)).abs() < 1e-6);
    }

    #[test]
    fn escape_and_the_chord_again_give_the_keyboard_back() {
        let (ws, ids) = ids();
        for leave in [arrow(Named::Escape), chord()] {
            let mut ui = simulate(grid(&ws, &ids, None, true));
            ui.simulate([chord()]);
            ui.simulate([leave]);
            ui.simulate([arrow(Named::ArrowRight)]);
            assert!(ratios(ui).is_empty());
        }
    }

    #[test]
    fn a_key_that_is_not_the_boundarys_ends_it_and_goes_to_the_terminal() {
        let (ws, ids) = ids();
        let mut ui = simulate(grid(&ws, &ids, None, true));
        ui.simulate([chord()]);
        // The arrow across the boundary is the terminal's: it ends this.
        ui.simulate([arrow(Named::ArrowUp)]);
        ui.simulate([arrow(Named::ArrowRight)]);
        assert!(ratios(ui).is_empty());
    }

    #[test]
    fn without_the_keyboard_allowed_the_grid_takes_no_key() {
        let (ws, ids) = ids();
        let mut ui = simulate(grid(&ws, &ids, None, false));
        ui.simulate([chord()]);
        ui.simulate([arrow(Named::ArrowRight)]);
        assert!(ratios(ui).is_empty());
        // And a zoomed grid has no boundary to take.
        let zoomed: PaneId = "a".into();
        let mut ui = simulate(grid(&ws, &ids, Some(&zoomed), true));
        ui.simulate([chord()]);
        ui.simulate([arrow(Named::ArrowRight)]);
        assert!(ratios(ui).is_empty());
    }

    #[test]
    fn a_click_that_goes_nowhere_selects_the_boundary_and_writes_nothing() {
        let (ws, ids) = ids();
        let mut ui = simulate(grid(&ws, &ids, None, true));
        let position = Point::new(400.0, 300.0);
        ui.point_at(position);
        ui.simulate([Event::Mouse(mouse::Event::CursorMoved { position })]);
        ui.simulate([Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Left,
        ))]);
        ui.simulate([Event::Mouse(mouse::Event::ButtonReleased(
            mouse::Button::Left,
        ))]);
        ui.simulate([arrow(Named::ArrowLeft)]);
        let moved = ratios(ui);
        assert_eq!(
            moved.len(),
            1,
            "the click wrote nothing, the arrow one move"
        );
        assert!((moved[0] - 0.47).abs() < 1e-6, "{moved:?}");
    }

    #[test]
    fn a_drag_still_commits_its_ratio_with_the_keyboard_allowed() {
        let (ws, ids) = ids();
        let mut ui = simulate(grid(&ws, &ids, None, true));
        let position = Point::new(400.0, 300.0);
        ui.point_at(position);
        ui.simulate([Event::Mouse(mouse::Event::CursorMoved { position })]);
        ui.simulate([Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Left,
        ))]);
        let dragged = Point::new(600.0, 300.0);
        ui.simulate([Event::Mouse(mouse::Event::CursorMoved {
            position: dragged,
        })]);
        ui.simulate([Event::Mouse(mouse::Event::ButtonReleased(
            mouse::Button::Left,
        ))]);
        // The drag is the write; it does not leave the keyboard on the
        // boundary.
        ui.simulate([arrow(Named::ArrowLeft)]);
        let moved = ratios(ui);
        assert_eq!(moved.len(), 1, "{moved:?}");
        assert!((moved[0] - 0.75).abs() < 0.01, "{moved:?}");
    }

    fn drops(ui: Simulator<'_, Message>) -> Vec<(String, std::path::PathBuf)> {
        ui.into_messages()
            .filter_map(|message| match message {
                Message::Panes(Msg::Term(pane, hangar_term::Action::FileDropped(path))) => {
                    Some((pane, path))
                }
                _ => None,
            })
            .collect()
    }

    fn drop_file(ui: &mut Simulator<'_, Message>, name: &str) {
        ui.simulate([Event::Window(window::Event::FileDropped(name.into()))]);
    }

    #[test]
    fn a_file_dropped_where_the_pointer_is_unknown_goes_to_the_focused_pane() {
        // The window systems give no position during a drag, and a drag from
        // a file manager leaves the pointer unknown: the focused pane takes it.
        let (ws, ids) = ids();
        let mut ui = simulate(grid(&ws, &ids, None, false));
        drop_file(&mut ui, "/tmp/a.png");
        assert_eq!(drops(ui), [("a".to_string(), "/tmp/a.png".into())]);
    }

    #[test]
    fn a_file_dropped_over_a_pane_goes_to_that_pane_once() {
        let (ws, ids) = ids();
        let mut ui = simulate(grid(&ws, &ids, None, false));
        let over_b = Point::new(700.0, 300.0);
        ui.point_at(over_b);
        ui.simulate([Event::Mouse(mouse::Event::CursorMoved { position: over_b })]);
        drop_file(&mut ui, "/tmp/b.png");
        drop_file(&mut ui, "/tmp/c.png");
        assert_eq!(
            drops(ui),
            [
                ("b".to_string(), "/tmp/b.png".into()),
                ("b".to_string(), "/tmp/c.png".into())
            ]
        );
    }

    #[test]
    fn a_file_dropped_on_a_zoomed_grid_goes_to_the_pane_in_front() {
        let (ws, ids) = ids();
        let zoomed: PaneId = "a".into();
        let mut ui = simulate(grid(&ws, &ids, Some(&zoomed), false));
        drop_file(&mut ui, "/tmp/a.png");
        assert_eq!(drops(ui), [("a".to_string(), "/tmp/a.png".into())]);
    }

    #[test]
    fn a_click_elsewhere_gives_the_boundary_back() {
        let (ws, ids) = ids();
        let mut ui = simulate(grid(&ws, &ids, None, true));
        ui.simulate([chord()]);
        let away = Point::new(100.0, 300.0);
        ui.point_at(away);
        ui.simulate([Event::Mouse(mouse::Event::CursorMoved { position: away })]);
        ui.simulate([Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Left,
        ))]);
        ui.simulate([arrow(Named::ArrowRight)]);
        assert!(ratios(ui).is_empty());
    }
}
