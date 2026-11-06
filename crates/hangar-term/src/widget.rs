//! The iced widget that draws a [`Terminal`] and turns input into [`Action`]s.
//!
//! The widget holds a shared reference to the terminal and never changes it:
//! what happens to it (a key, the mouse, a resize) is published as an
//! [`Action`] for the app to pass to [`Terminal::perform`]. What it keeps
//! itself is what only matters to this window: the measured cell size, the
//! cached drawing of each row, the composition in progress.
//!
//! ## What it costs to draw
//!
//! A row is reduced to a plan (see the `plan` module) that is kept until the
//! terminal says the row changed, so a frame in which only the cursor line
//! changed redoes one row. Text goes to the renderer as one item per run of
//! equally styled cells, not per cell, and `iced` shapes each distinct string
//! once and keeps the result, so a screen that scrolls by (the same strings in
//! other places) costs no shaping. Backgrounds are merged into rectangles per
//! row. Selection, search matches, the cursor and the scrollbar are drawn on
//! top and never touch the plans.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::time::{Duration, Instant};

use iced::advanced::input_method::{self, InputMethod, Preedit};
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Quad};
use iced::advanced::text::{self, Paragraph as _, Text};
use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget, clipboard};
use iced::keyboard::key::Named;
use iced::keyboard::{self, Key, Modifiers};
use iced::mouse;
use iced::{
    Border, Color, Element, Event, Length, Padding, Pixels, Point, Rectangle, Shadow, Size,
    alignment, window,
};

use crate::config::CursorShape;
use crate::event::{Action, Button, CellPos, Metrics, Pointer, ScrollTo, Side};
use crate::font::Font;
use crate::ime::InputState;
use crate::keys::{KeyEventKind, KeyPress};
use crate::links::{self, LinkHit};
use crate::plan::{Decoration, RowPlan, Run, plan_row};
use crate::shapes::{self, Shape};
use crate::terminal::{BELL_FLASH, Terminal};

/// The half period of the cursor's blink.
const BLINK: Duration = Duration::from_millis(530);

/// How often a drag that left the widget keeps scrolling.
const DRAG_SCROLL_EVERY: Duration = Duration::from_millis(60);

const SCROLLBAR_WIDTH: f32 = 8.0;
const SCROLLBAR_MARGIN: f32 = 2.0;
const SCROLLBAR_MIN_THUMB: f32 = 24.0;

/// A command a key can be bound to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binding {
    /// Copy the selection (and send nothing to the program).
    Copy,
    /// Paste the clipboard.
    Paste,
    /// Paste the primary selection, as middle click does.
    PastePrimary,
    SelectAll,
    ScrollPageUp,
    ScrollPageDown,
    ScrollTop,
    ScrollBottom,
}

/// What key a binding is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingKey {
    Named(Named),
    /// A character, lower case. Matched against the Latin letter of the key,
    /// so that a binding works on every layout.
    Char(char),
}

/// Which keys do app-level things instead of reaching the program.
///
/// A terminal cannot use Ctrl+C for copying, since the program needs it for an
/// interrupt, so the usual bindings add Shift: Ctrl+Shift+C and Ctrl+Shift+V.
/// On macOS Cmd+C and Cmd+V do the job, because Cmd is never sent to a program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bindings(Vec<(BindingKey, Modifiers, Binding)>);

impl Bindings {
    /// No bindings: every key goes to the program, and the app handles its
    /// shortcuts itself with [`TerminalView::intercept`].
    pub fn none() -> Self {
        Self(Vec::new())
    }

    /// The bindings of other terminals on the platform.
    pub fn standard() -> Self {
        let mut bindings = Self::none();
        if cfg!(target_os = "macos") {
            bindings = bindings
                .bind(BindingKey::Char('c'), Modifiers::LOGO, Binding::Copy)
                .bind(BindingKey::Char('v'), Modifiers::LOGO, Binding::Paste)
                .bind(BindingKey::Char('a'), Modifiers::LOGO, Binding::SelectAll);
        }
        bindings
            .bind(
                BindingKey::Char('c'),
                Modifiers::CTRL | Modifiers::SHIFT,
                Binding::Copy,
            )
            .bind(
                BindingKey::Char('v'),
                Modifiers::CTRL | Modifiers::SHIFT,
                Binding::Paste,
            )
            .bind(
                BindingKey::Char('a'),
                Modifiers::CTRL | Modifiers::SHIFT,
                Binding::SelectAll,
            )
            .bind(
                BindingKey::Named(Named::Insert),
                Modifiers::CTRL,
                Binding::Copy,
            )
            .bind(
                BindingKey::Named(Named::Insert),
                Modifiers::SHIFT,
                Binding::PastePrimary,
            )
            .bind(
                BindingKey::Named(Named::PageUp),
                Modifiers::SHIFT,
                Binding::ScrollPageUp,
            )
            .bind(
                BindingKey::Named(Named::PageDown),
                Modifiers::SHIFT,
                Binding::ScrollPageDown,
            )
            .bind(
                BindingKey::Named(Named::Home),
                Modifiers::SHIFT,
                Binding::ScrollTop,
            )
            .bind(
                BindingKey::Named(Named::End),
                Modifiers::SHIFT,
                Binding::ScrollBottom,
            )
    }

    /// Adds a binding.
    pub fn bind(mut self, key: BindingKey, modifiers: Modifiers, binding: Binding) -> Self {
        self.0.push((key, modifiers, binding));
        self
    }

    /// The binding a key press triggers, if any.
    pub fn find(&self, press: &KeyPress) -> Option<Binding> {
        if press.kind == KeyEventKind::Release {
            return None;
        }
        let character = press
            .key
            .to_latin(press.physical_key)
            .map(|c| c.to_ascii_lowercase());
        self.0.iter().find_map(|(key, modifiers, binding)| {
            let same_key = match (key, &press.key) {
                (BindingKey::Named(a), Key::Named(b)) => a == b,
                (BindingKey::Char(a), _) => character == Some(*a),
                _ => false,
            };
            (same_key && press.modifiers == *modifiers).then_some(*binding)
        })
    }
}

type Intercept<'a, Message> = Box<dyn Fn(&KeyPress) -> Option<Message> + 'a>;
type KeyFilter<'a> = Box<dyn Fn(&KeyPress) -> bool + 'a>;

/// A terminal as a widget.
///
/// Build it with [`terminal_view`] and put it where it should fill: it takes
/// all the room it is given, and tells the terminal how many cells that is.
pub struct TerminalView<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer> {
    terminal: &'a Terminal,
    on_action: Box<dyn Fn(Action) -> Message + 'a>,
    intercept: Option<Intercept<'a, Message>>,
    key_filter: Option<KeyFilter<'a>>,
    bindings: Bindings,
    padding: Padding,
    scale_factor: Option<f32>,
    scrollbar: bool,
    dim: Option<(Color, f32)>,
    _marker: PhantomData<(Theme, Renderer)>,
}

/// The tag of a [`TerminalView`]'s tree, which does not depend on the
/// terminal: for a wrapper that would otherwise lock a shared terminal just
/// to answer it, as iced asks for it on every rebuild of the view.
pub fn view_tag() -> tree::Tag {
    tree::Tag::of::<State>()
}

/// A fresh [`TerminalView`] tree state, likewise.
pub fn view_state() -> tree::State {
    tree::State::new(State::default())
}

/// A widget that draws `terminal`. `on_action` turns what happens to it into
/// the app's message; pass the result to [`Terminal::perform`].
pub fn terminal_view<'a, Message, Theme, Renderer>(
    terminal: &'a Terminal,
    on_action: impl Fn(Action) -> Message + 'a,
) -> TerminalView<'a, Message, Theme, Renderer> {
    TerminalView {
        terminal,
        on_action: Box::new(on_action),
        intercept: None,
        key_filter: None,
        bindings: Bindings::standard(),
        padding: Padding::ZERO,
        scale_factor: None,
        scrollbar: true,
        dim: None,
        _marker: PhantomData,
    }
}

impl<'a, Message, Theme, Renderer> TerminalView<'a, Message, Theme, Renderer> {
    /// Space between the widget's edge and the grid.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// The window's scale factor, if the app knows it. The widget rounds a
    /// row to a whole number of physical pixels to keep rows evenly spaced,
    /// which needs the factor; without it, it uses the one the window last
    /// announced and 1 before that.
    pub fn scale_factor(mut self, scale: f32) -> Self {
        self.scale_factor = Some(scale);
        self
    }

    /// Replaces the key bindings ([`Bindings::standard`] by default).
    pub fn bindings(mut self, bindings: Bindings) -> Self {
        self.bindings = bindings;
        self
    }

    /// Lets the app take a key before the terminal does. When the function
    /// returns a message the key is not sent to the program; the message is
    /// published instead. This is how an app binds its own shortcuts (a
    /// search bar, zoom, pane switching) without the terminal typing them.
    pub fn intercept(mut self, f: impl Fn(&KeyPress) -> Option<Message> + 'a) -> Self {
        self.intercept = Some(Box::new(f));
        self
    }

    /// Lets the app's keymap see a key before the terminal consumes it.
    ///
    /// `iced` hands a key to the focused widget first, and a terminal would
    /// take every one of them: Ctrl+Tab, Alt+arrows and Ctrl+Shift+P would
    /// all become input for the program. For each key press (and release)
    /// the function is asked whether the app takes it. If it says yes, the
    /// terminal does nothing with the key and does not capture the event, so
    /// it goes on to the app's own handling (a `keyboard::listen`
    /// subscription, another widget); if it says no, the key is the
    /// terminal's: a binding, or bytes for the program.
    ///
    /// The filter is a plain predicate, so the app can answer it from its
    /// keymap without building a message. [`intercept`](Self::intercept) is
    /// the other way round: the terminal publishes the message itself.
    pub fn key_filter(mut self, f: impl Fn(&KeyPress) -> bool + 'a) -> Self {
        self.key_filter = Some(Box::new(f));
        self
    }

    /// Whether to draw the scrollbar. On by default.
    pub fn scrollbar(mut self, show: bool) -> Self {
        self.scrollbar = show;
        self
    }

    /// Draws the terminal as if it were only `opacity` opaque over `behind`
    /// (CSS `opacity`): every colour on it, the background and the cursor
    /// included, steps that far toward `behind`. It is how a pane that is not
    /// the one being typed into steps back a little. An `opacity` of 1, or
    /// more, draws it as it is.
    ///
    /// The terminal is drawn as usual and a veil of `behind` goes over it, in
    /// a layer of its own because a layer draws its quads before its text.
    pub fn dimmed(mut self, behind: Color, opacity: f32) -> Self {
        self.dim = (opacity.is_finite() && opacity < 1.0).then_some((behind, opacity.max(0.0)));
        self
    }

    fn area(&self, bounds: Rectangle) -> Rectangle {
        bounds.shrink(self.padding)
    }
}

// ---- State ------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CellSize {
    width: f32,
    height: f32,
}

#[derive(Debug, Clone, Copy)]
struct Fonts {
    regular: iced::Font,
    bold: iced::Font,
    italic: iced::Font,
    bold_italic: iced::Font,
}

impl Fonts {
    fn new(font: &Font) -> Self {
        // One family lookup for all four: it asks the font system, which is
        // not free.
        let family = font.family();
        let make = |bold, italic| Font::resolve_with(family, bold, italic);
        Self {
            regular: make(false, false),
            bold: make(true, false),
            italic: make(false, true),
            bold_italic: make(true, true),
        }
    }

    fn pick(&self, bold: bool, italic: bool) -> iced::Font {
        match (bold, italic) {
            (false, false) => self.regular,
            (true, false) => self.bold,
            (false, true) => self.italic,
            (true, true) => self.bold_italic,
        }
    }
}

/// What the widget measured for a font.
#[derive(Debug, Clone)]
struct Measured {
    font: Font,
    scale: f32,
    fonts: Fonts,
    cell: CellSize,
}

/// The cached plans of the visible rows.
#[derive(Default)]
struct RowCache {
    epoch: u64,
    /// The terminal's `row_shift` the plans are placed for.
    shift: i64,
    plans: Vec<Option<(u64, RowPlan)>>,
}

impl RowCache {
    fn sync(&mut self, terminal: &Terminal) {
        let rows = terminal.size().1 as usize;
        if self.epoch != terminal.epoch() || self.plans.len() != rows {
            self.epoch = terminal.epoch();
            self.plans.clear();
            self.plans.resize_with(rows, || None);
        }
        // The view moved over unchanged content: the plans move with it, as
        // the terminal moved the rows' generations (`Terminal::scroll_view`).
        let shift = terminal.row_shift() - self.shift;
        self.shift = terminal.row_shift();
        let by = shift.unsigned_abs() as usize;
        if by >= rows {
            self.plans.iter_mut().for_each(|plan| *plan = None);
        } else if shift > 0 {
            self.plans.rotate_right(by);
            self.plans[..by].iter_mut().for_each(|plan| *plan = None);
        } else if shift < 0 {
            self.plans.rotate_left(by);
            self.plans[rows - by..]
                .iter_mut()
                .for_each(|plan| *plan = None);
        }
        for (row, slot) in self.plans.iter_mut().enumerate() {
            let generation = terminal.row_generation(row);
            if !matches!(slot, Some((g, _)) if *g == generation) {
                *slot = Some((generation, plan_row(terminal, row)));
            }
        }
    }
}

/// The drawing of each shape character at the current metrics.
///
/// A shape is a few rectangles, but building it allocates, and a bordered
/// TUI has hundreds of them on screen, drawn again every frame. The shapes
/// depend only on the character and the metrics, and there are a few hundred
/// characters at most, so they are kept until the metrics change.
#[derive(Default)]
struct ShapeCache {
    metrics: Option<[u32; 5]>,
    shapes: HashMap<char, Option<Shape>>,
}

impl ShapeCache {
    fn get(&mut self, glyph: char, metrics: shapes::Metrics) -> Option<&Shape> {
        let key = metrics.key();
        if self.metrics != Some(key) {
            self.metrics = Some(key);
            self.shapes.clear();
        }
        self.shapes
            .entry(glyph)
            .or_insert_with(|| shapes::shape(glyph, metrics))
            .as_ref()
    }
}

#[derive(Default)]
struct State {
    measured: Option<Measured>,
    scale: Option<f32>,
    rows: RefCell<RowCache>,
    shapes: RefCell<ShapeCache>,
    input: InputState,
    modifiers: Modifiers,
    /// The button whose press started inside the widget and is still down.
    pressed: Option<Button>,
    /// The scrollbar's thumb is being dragged; the offset from its top where
    /// it was grabbed.
    thumb_grab: Option<f32>,
    pointer: Option<Point>,
    hover: Option<LinkHit>,
    hover_generation: u64,
    over_scrollbar: bool,
    /// The size last requested, and the grid size at that time, so that a
    /// resize the app has not applied is asked for once, not every frame.
    resize_sent: Option<((u16, u16), (u16, u16))>,
    /// The time of the last frame.
    now: Cell<Option<Instant>>,
    /// When the cursor last moved or was typed next to, for the blink.
    blink_from: Cell<Option<Instant>>,
    cursor_generation: Cell<u64>,
    focused: bool,
    last_drag_tick: Option<Instant>,
}

impl State {
    fn scale(&self, requested: Option<f32>) -> f32 {
        requested.or(self.scale).filter(|s| *s > 0.0).unwrap_or(1.0)
    }

    /// Measures the font if it, or the scale, is not what was measured.
    fn measure<Renderer>(&mut self, terminal: &Terminal, scale: f32) -> CellSize
    where
        Renderer: text::Renderer<Font = iced::Font>,
    {
        let font = terminal.font();
        if let Some(m) = &self.measured
            && m.font == *font
            && m.scale == scale
        {
            return m.cell;
        }
        let fonts = Fonts::new(font);
        let cell = measure_cell::<Renderer>(font, fonts.regular, scale);
        self.measured = Some(Measured {
            font: font.clone(),
            scale,
            fonts,
            cell,
        });
        cell
    }
}

/// The size of a cell in a font.
///
/// The width is the advance of the font, measured over a run of characters
/// and divided, which keeps the fraction that a single glyph's rounding would
/// lose; the cells of a long row then land where the font lays its glyphs.
fn measure_cell<Renderer>(font: &Font, resolved: iced::Font, scale: f32) -> CellSize
where
    Renderer: text::Renderer<Font = iced::Font>,
{
    const SAMPLE: &str = "MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM";
    let height = font.cell_height(scale);
    let paragraph = Renderer::Paragraph::with_text(Text {
        content: SAMPLE,
        bounds: Size::INFINITE,
        size: Pixels(font.size),
        line_height: text::LineHeight::Absolute(Pixels(height)),
        font: resolved,
        align_x: text::Alignment::Left,
        align_y: alignment::Vertical::Top,
        shaping: text::Shaping::Basic,
        wrapping: text::Wrapping::None,
    });
    let advance = paragraph.min_bounds().width / SAMPLE.len() as f32;
    let advance = if advance.is_finite() && advance > 0.5 {
        advance
    } else {
        font.size * 0.6
    };
    CellSize {
        // The letter spacing is part of the cell, so every column, the
        // pointer's cell and the number of columns that fit follow it. A
        // tight spacing never takes the cell below a pixel: a hand-edited
        // setting must not collapse the grid.
        width: (advance + font.letter_spacing).max(1.0),
        height,
    }
}

/// How many cells fit in `size`.
pub(crate) fn grid_size(size: Size, cell: CellSize) -> (u16, u16) {
    let columns = (size.width / cell.width).floor().clamp(1.0, 1000.0) as u16;
    let rows = (size.height / cell.height).floor().clamp(1.0, 1000.0) as u16;
    (columns, rows)
}

/// The cell under `position`, and the half of it, for a grid whose top left is
/// at `origin`. Positions outside the grid give coordinates outside it.
pub(crate) fn cell_at(position: Point, origin: Point, cell: CellSize) -> CellPos {
    let x = (position.x - origin.x) / cell.width;
    let y = (position.y - origin.y) / cell.height;
    CellPos {
        column: x.floor() as i32,
        row: y.floor() as i32,
        side: if x - x.floor() < 0.5 {
            Side::Left
        } else {
            Side::Right
        },
    }
}

// ---- The scrollbar -----------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
struct Scrollbar {
    track: Rectangle,
    thumb: Rectangle,
}

/// The scrollbar of a grid area, if there is anything to scroll.
fn scrollbar(area: Rectangle, rows: usize, history: usize, offset: usize) -> Option<Scrollbar> {
    if history == 0 {
        return None;
    }
    let track = Rectangle::new(
        Point::new(
            area.x + area.width - SCROLLBAR_WIDTH - SCROLLBAR_MARGIN,
            area.y,
        ),
        Size::new(SCROLLBAR_WIDTH, area.height),
    );
    let total = (history + rows) as f32;
    let height = (track.height * rows as f32 / total)
        .clamp(SCROLLBAR_MIN_THUMB.min(track.height), track.height);
    let travel = track.height - height;
    let top = track.y + travel * (1.0 - offset as f32 / history as f32);
    Some(Scrollbar {
        track,
        thumb: Rectangle::new(Point::new(track.x, top), Size::new(SCROLLBAR_WIDTH, height)),
    })
}

impl Scrollbar {
    /// The offset that puts the thumb's top at `y`.
    fn offset_at(&self, y: f32, grab: f32, history: usize) -> usize {
        let travel = (self.track.height - self.thumb.height).max(1.0);
        let t = ((y - grab - self.track.y) / travel).clamp(0.0, 1.0);
        ((1.0 - t) * history as f32).round() as usize
    }

    /// Where the strip that wakes the scrollbar is: wider than the bar.
    fn hot_zone(&self) -> Rectangle {
        Rectangle::new(
            Point::new(self.track.x - 6.0, self.track.y),
            Size::new(self.track.width + 6.0 + SCROLLBAR_MARGIN, self.track.height),
        )
    }
}

// ---- The widget --------------------------------------------------------------

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for TerminalView<'a, Message, Theme, Renderer>
where
    Renderer: text::Renderer<Font = iced::Font>,
{
    fn tag(&self) -> tree::Tag {
        view_tag()
    }

    fn state(&self) -> tree::State {
        view_state()
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<State>();
        let scale = state.scale(self.scale_factor);
        state.measure::<Renderer>(self.terminal, scale);
        layout::Node::new(limits.width(Length::Fill).height(Length::Fill).resolve(
            Length::Fill,
            Length::Fill,
            Size::ZERO,
        ))
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();
        let area = self.area(bounds);
        let Some(cell) = state.measured.as_ref().map(|m| m.cell) else {
            return;
        };
        let terminal = self.terminal;
        let publish = |shell: &mut Shell<'_, Message>, action: Action| {
            shell.publish((self.on_action)(action))
        };

        match event {
            Event::Window(window::Event::RedrawRequested(now)) => {
                state.now.set(Some(*now));
                let focused = terminal.is_focused();
                if state.focused && !focused {
                    // Losing the focus ends any composition: an input method
                    // does not compose for a pane that is not listening.
                    state.input.reset();
                }
                state.focused = focused;

                // Tell the terminal how many cells there is room for.
                let target = grid_size(area.size(), cell);
                let current = terminal.size();
                let asked = state.resize_sent == Some((target, current));
                if target != current && !asked {
                    state.resize_sent = Some((target, current));
                    publish(
                        shell,
                        Action::Resize(Metrics {
                            columns: target.0,
                            rows: target.1,
                            cell_width: cell.width,
                            cell_height: cell.height,
                        }),
                    );
                }

                // Things the terminal is waiting on.
                if let Some(deadline) = terminal.deadline() {
                    if *now >= deadline {
                        publish(shell, Action::Tick);
                    } else {
                        shell.request_redraw_at(deadline);
                    }
                }
                if terminal.bell_age(*now).is_some() {
                    shell.request_redraw_at(*now + Duration::from_millis(16));
                }

                // The cursor's blink restarts whenever the cursor moves.
                if terminal.cursor_generation() != state.cursor_generation.get() {
                    state.cursor_generation.set(terminal.cursor_generation());
                    state.blink_from.set(Some(*now));
                }
                if focused && terminal.cursor_blinks() && terminal.cursor().is_some() {
                    let from = state.blink_from.get().unwrap_or(*now);
                    let phase = (*now - from).as_millis() % (2 * BLINK.as_millis());
                    let until_flip = BLINK.as_millis() - phase % BLINK.as_millis();
                    shell.request_redraw_at(*now + Duration::from_millis(until_flip as u64));
                }

                // A drag that left the widget keeps scrolling.
                if let (Some(Button::Left), Some(position)) = (state.pressed, state.pointer) {
                    if !bounds.contains(position)
                        && state
                            .last_drag_tick
                            .is_none_or(|t| *now - t >= DRAG_SCROLL_EVERY)
                    {
                        state.last_drag_tick = Some(*now);
                        publish(
                            shell,
                            Action::Pointer(Pointer::Move {
                                at: cell_at(position, area.position(), cell),
                                modifiers: state.modifiers,
                            }),
                        );
                    }
                    shell.request_redraw_at(*now + DRAG_SCROLL_EVERY);
                }

                // The link under a pointer that has not moved changes when
                // the text under it does.
                if terminal.generation() != state.hover_generation {
                    state.hover_generation = terminal.generation();
                    if let Some(position) = state.pointer.filter(|p| area.contains(*p)) {
                        let at = cell_at(position, area.position(), cell);
                        state.hover = link_under(terminal, at);
                    }
                }

                // Composition happens at the cursor.
                if focused {
                    let (column, row) = terminal
                        .cursor()
                        .map(|c| (c.column, c.row))
                        .unwrap_or((0, 0));
                    let caret = Rectangle::new(
                        Point::new(
                            area.x + column as f32 * cell.width,
                            area.y + row as f32 * cell.height,
                        ),
                        Size::new(cell.width, cell.height),
                    );
                    let preedit = state.input.preedit().map(|p| Preedit {
                        content: p.text.as_str(),
                        selection: p.selection.clone(),
                        text_size: Some(Pixels(terminal.font().size)),
                    });
                    shell.request_input_method(&InputMethod::Enabled {
                        cursor: caret,
                        purpose: input_method::Purpose::Terminal,
                        preedit,
                    });
                }
            }

            Event::Window(window::Event::FileDropped(path)) => {
                // The window reports the drop without a position, so the
                // terminal under the pointer takes it.
                if cursor.is_over(bounds) {
                    publish(shell, Action::FileDropped(path.clone()));
                }
            }

            Event::Window(window::Event::Focused) => publish(shell, Action::WindowFocus(true)),
            Event::Window(window::Event::Unfocused) => {
                state.input.reset();
                publish(shell, Action::WindowFocus(false));
            }
            Event::Window(window::Event::Rescaled(factor)) => {
                state.scale = Some(*factor);
                shell.invalidate_layout();
                shell.request_redraw();
            }

            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers;
                if state.hover.is_some() {
                    shell.request_redraw();
                }
            }

            Event::Keyboard(key_event) if terminal.is_focused() => {
                let Some(press) = KeyPress::from_event(key_event) else {
                    return;
                };
                state.modifiers = press.modifiers;
                if self.key_filter.as_ref().is_some_and(|f| f(&press)) {
                    return;
                }
                if let Some(message) = self.intercept.as_ref().and_then(|f| f(&press)) {
                    shell.publish(message);
                    shell.capture_event();
                    return;
                }
                let binding = self.bindings.find(&press).or_else(|| {
                    (terminal.config().ctrl_v_pastes && is_paste_key(&press))
                        .then_some(Binding::Paste)
                });
                if let Some(binding) = binding {
                    let handled = self.run_binding(binding, terminal, clipboard, shell);
                    if handled {
                        shell.capture_event();
                        return;
                    }
                }
                let mode = terminal.mode();
                let options = terminal.key_options();
                if let Some(bytes) = state.input.key(&press, mode, &options, Instant::now()) {
                    state.blink_from.set(state.now.get());
                    let action = if press.kind == KeyEventKind::Release {
                        Action::Report(bytes)
                    } else {
                        Action::Input(bytes)
                    };
                    publish(shell, action);
                    shell.capture_event();
                    shell.request_redraw();
                }
            }

            Event::InputMethod(ime_event) if terminal.is_focused() => {
                if let Some(bytes) = state.input.input_method(ime_event, Instant::now()) {
                    publish(shell, Action::Input(bytes));
                }
                shell.capture_event();
                shell.request_redraw();
            }

            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                state.pointer = Some(*position);
                let inside = area.contains(*position);
                let at = cell_at(*position, area.position(), cell);

                if let Some(grab) = state.thumb_grab {
                    let (rows, history, offset) = scroll_numbers(terminal);
                    if let Some(bar) = scrollbar(area, rows, history, offset) {
                        let target = bar.offset_at(position.y, grab, history);
                        publish(shell, Action::Scroll(ScrollTo::Offset(target)));
                        shell.capture_event();
                    }
                    return;
                }

                let (rows, history, offset) = scroll_numbers(terminal);
                let over_bar = self.scrollbar
                    && scrollbar(area, rows, history, offset)
                        .is_some_and(|b| b.hot_zone().contains(*position));
                if over_bar != state.over_scrollbar {
                    state.over_scrollbar = over_bar;
                    shell.request_redraw();
                }

                if state.pressed.is_some() || (inside && cursor.is_over(bounds)) {
                    publish(
                        shell,
                        Action::Pointer(Pointer::Move {
                            at,
                            modifiers: state.modifiers,
                        }),
                    );
                }

                let hover = if inside && state.pressed.is_none() && cursor.is_over(bounds) {
                    link_under(terminal, at)
                } else {
                    None
                };
                if hover != state.hover {
                    state.hover = hover;
                    shell.request_redraw();
                }
            }

            Event::Mouse(mouse::Event::CursorLeft) => {
                if state.hover.take().is_some() {
                    shell.request_redraw();
                }
                state.over_scrollbar = false;
                if state.pressed.is_none() {
                    publish(shell, Action::Pointer(Pointer::Left));
                }
            }

            Event::Mouse(mouse::Event::ButtonPressed(button)) => {
                let Some(position) = cursor.position_over(bounds) else {
                    return;
                };
                let Some(button) = convert_button(*button) else {
                    return;
                };
                state.pointer = Some(position);
                let at = cell_at(position, area.position(), cell);

                // The scrollbar takes the press before the grid does.
                let (rows, history, offset) = scroll_numbers(terminal);
                if button == Button::Left
                    && self.scrollbar
                    && let Some(bar) = scrollbar(area, rows, history, offset)
                    && bar.hot_zone().contains(position)
                {
                    let grab = if bar.thumb.contains(position) {
                        position.y - bar.thumb.y
                    } else {
                        // A click on the track puts the thumb's
                        // middle there, and the drag goes on from it.
                        bar.thumb.height / 2.0
                    };
                    state.thumb_grab = Some(grab);
                    publish(
                        shell,
                        Action::Scroll(ScrollTo::Offset(bar.offset_at(position.y, grab, history))),
                    );
                    shell.capture_event();
                    return;
                }

                if button == Button::Middle && !terminal.mouse_reporting(state.modifiers) {
                    if let Some(text) = clipboard
                        .read(clipboard::Kind::Primary)
                        .filter(|t| !t.is_empty())
                    {
                        publish(shell, Action::Paste(text));
                    }
                    shell.capture_event();
                    return;
                }

                state.pressed = Some(button);
                publish(
                    shell,
                    Action::Pointer(Pointer::Down {
                        button,
                        at,
                        modifiers: state.modifiers,
                    }),
                );
                // Right clicks are left for the app's own menus unless the
                // program is listening to them.
                if button != Button::Right || terminal.mouse_reporting(state.modifiers) {
                    shell.capture_event();
                }
            }

            Event::Mouse(mouse::Event::ButtonReleased(button)) => {
                let Some(button) = convert_button(*button) else {
                    return;
                };
                if button == Button::Left && state.thumb_grab.take().is_some() {
                    shell.capture_event();
                    return;
                }
                if state.pressed != Some(button) {
                    return;
                }
                state.pressed = None;
                state.last_drag_tick = None;
                let position = cursor
                    .position()
                    .or(state.pointer)
                    .unwrap_or(area.position());
                publish(
                    shell,
                    Action::Pointer(Pointer::Up {
                        button,
                        at: cell_at(position, area.position(), cell),
                        modifiers: state.modifiers,
                    }),
                );
                shell.capture_event();
            }

            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let Some(position) = cursor.position_over(bounds) else {
                    return;
                };
                // Ctrl+wheel is the app's zoom, not ours.
                if state.modifiers.control() && !terminal.mouse_reporting(state.modifiers) {
                    return;
                }
                let notches = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    // A touchpad reports pixels; a line is a cell high.
                    mouse::ScrollDelta::Pixels { y, .. } => *y / cell.height,
                };
                if notches == 0.0 {
                    return;
                }
                publish(
                    shell,
                    Action::Pointer(Pointer::Wheel {
                        // The notches of a mouse are lines, of which a wheel
                        // click is several; the terminal scales them.
                        notches: match delta {
                            mouse::ScrollDelta::Lines { .. } => notches,
                            mouse::ScrollDelta::Pixels { .. } => {
                                notches / terminal.config().scroll_multiplier.max(1.0)
                            }
                        },
                        at: cell_at(position, area.position(), cell),
                        modifiers: state.modifiers,
                    }),
                );
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
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        if !cursor.is_over(bounds) {
            return mouse::Interaction::None;
        }
        if state.over_scrollbar || state.thumb_grab.is_some() {
            return mouse::Interaction::Idle;
        }
        if state.hover.is_some() && click_opens_links(self.terminal, state.modifiers) {
            return mouse::Interaction::Pointer;
        }
        if self.terminal.mouse_reporting(state.modifiers) {
            mouse::Interaction::Idle
        } else {
            mouse::Interaction::Text
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let terminal = self.terminal;
        let palette = terminal.palette();
        let background = terminal.color_of_index(257);

        let Some(measured) = &state.measured else {
            renderer.fill_quad(quad(bounds), background);
            return;
        };
        let cell = measured.cell;
        let area = self.area(bounds);
        let (columns, rows) = terminal.size();
        let (columns, rows) = (columns as usize, rows as usize);
        let scale = state.scale(self.scale_factor);

        let mut cache = state.rows.borrow_mut();
        cache.sync(terminal);

        let at = |column: usize, row: usize| {
            Point::new(
                area.x + column as f32 * cell.width,
                area.y + row as f32 * cell.height,
            )
        };
        let cells = |first: usize, end: usize, row: usize| {
            Rectangle::new(
                at(first, row),
                Size::new((end - first) as f32 * cell.width, cell.height),
            )
        };

        renderer.with_layer(bounds, |renderer| {
            renderer.fill_quad(quad(bounds), background);

            // Backgrounds.
            for (row, slot) in cache.plans.iter().enumerate() {
                let Some((_, plan)) = slot else { continue };
                for span in &plan.backgrounds {
                    renderer.fill_quad(
                        quad(cells(span.first, span.end.min(columns), row)),
                        span.color,
                    );
                }
            }

            // The selection and the search, over the backgrounds and under the
            // text.
            let offset = terminal.display_offset() as i32;
            if let Some(range) = terminal.selection_range() {
                for row in 0..rows {
                    let line = row as i32 - offset;
                    if line < range.start.line.0 || line > range.end.line.0 {
                        continue;
                    }
                    let (first, last) = if range.is_block {
                        (range.start.column.0, range.end.column.0)
                    } else {
                        (
                            if line == range.start.line.0 {
                                range.start.column.0
                            } else {
                                0
                            },
                            if line == range.end.line.0 {
                                range.end.column.0
                            } else {
                                columns - 1
                            },
                        )
                    };
                    if first <= last {
                        renderer.fill_quad(
                            quad(cells(first, (last + 1).min(columns), row)),
                            palette.selection_background,
                        );
                    }
                }
            }
            for (found, current) in terminal.visible_matches() {
                let tint = if current {
                    palette.search_active
                } else {
                    palette.search_match
                };
                let color = Color {
                    a: if current { 0.55 } else { 0.35 },
                    ..tint
                };
                for row in 0..rows {
                    let line = row as i32 - offset;
                    if line < found.start().line.0 || line > found.end().line.0 {
                        continue;
                    }
                    let first = if line == found.start().line.0 {
                        found.start().column.0
                    } else {
                        0
                    };
                    let last = if line == found.end().line.0 {
                        found.end().column.0
                    } else {
                        columns - 1
                    };
                    let area = cells(first, (last + 1).min(columns), row);
                    renderer.fill_quad(quad(area), color);
                    if current {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: area,
                                border: Border {
                                    color: tint,
                                    width: 1.0,
                                    radius: 0.0.into(),
                                },
                                shadow: Shadow::default(),
                                snap: true,
                            },
                            Color::TRANSPARENT,
                        );
                    }
                }
            }

            // Shapes and decorations: lines that fill cells.
            let metrics_for =
                shapes::Metrics::new(cell.width, cell.height, measured.font.size, scale);
            let mut shape_cache = state.shapes.borrow_mut();
            // The rectangles of a row's lines, with the ones that touch side
            // by side joined: a border of twenty cells is one quad, which no
            // renderer can leave a seam in. One list, emptied for each row.
            let mut lines: Vec<(Rectangle, Color)> = Vec::new();
            for (row, slot) in cache.plans.iter().enumerate() {
                let Some((_, plan)) = slot else { continue };
                for shape_cell in &plan.shapes {
                    if shape_cell.column >= columns {
                        continue;
                    }
                    let Some(shape) = shape_cache.get(shape_cell.glyph, metrics_for) else {
                        continue;
                    };
                    draw_shape(
                        renderer,
                        &mut lines,
                        shape,
                        shape_cell.color,
                        at(shape_cell.column, row),
                        metrics_for,
                    );
                }
                for (rect, color) in lines.drain(..) {
                    renderer.fill_quad(quad(rect), color);
                }
                for span in &plan.decorations {
                    draw_decoration(
                        renderer,
                        span.kind,
                        span.color,
                        cells(span.first, span.end.min(columns), row),
                        metrics_for,
                    );
                }
            }
            // The link under the pointer, when a click would open it.
            if let Some(hit) = state
                .hover
                .as_ref()
                .filter(|_| click_opens_links(terminal, state.modifiers))
            {
                for span in &hit.spans {
                    let row = span.line.0 + offset;
                    if row < 0 || row as usize >= rows {
                        continue;
                    }
                    let color = palette.foreground;
                    draw_decoration(
                        renderer,
                        Decoration::Underline,
                        color,
                        cells(span.first, (span.last + 1).min(columns), row as usize),
                        metrics_for,
                    );
                }
            }

            // Text.
            let setup = TextSetup {
                cell,
                fonts: &measured.fonts,
                size: measured.font.size,
                ligatures: measured.font.ligatures,
                clip: bounds,
            };
            for (row, slot) in cache.plans.iter().enumerate() {
                let Some((_, plan)) = slot else { continue };
                for run in &plan.runs {
                    if run.column >= columns {
                        continue;
                    }
                    fill_run(renderer, run, at(run.column, row), &setup);
                }
            }

            // The cursor, in a layer of its own: it covers the cell under it,
            // character included, and puts that character back in the
            // cursor's colours, without the row's plan having to know.
            if let Some(cursor_view) = terminal.cursor() {
                let now = state.now.get().unwrap_or_else(Instant::now);
                let visible = !(terminal.has_focus() && terminal.cursor_blinks()) || {
                    let from = state.blink_from.get().unwrap_or(now);
                    ((now - from).as_millis() / BLINK.as_millis()).is_multiple_of(2)
                };
                if visible && cursor_view.column < columns {
                    draw_cursor(
                        renderer,
                        terminal,
                        cursor_view,
                        at(cursor_view.column, cursor_view.row),
                        cell,
                        &measured.fonts,
                        measured.font.size,
                        scale,
                        bounds,
                    );
                }
            }

            // The visual bell: a short fade of the foreground over everything.
            if let Some(age) = terminal.bell_age(state.now.get().unwrap_or_else(Instant::now)) {
                let strength = 0.18 * (1.0 - age.as_secs_f32() / BELL_FLASH.as_secs_f32());
                renderer.fill_quad(
                    quad(bounds),
                    Color {
                        a: strength.max(0.0),
                        ..palette.foreground
                    },
                );
            }

            // The scrollbar.
            if self.scrollbar {
                let (rows_n, history, offset) = scroll_numbers(terminal);
                if let Some(bar) = scrollbar(area, rows_n, history, offset)
                    && (offset > 0 || state.over_scrollbar || state.thumb_grab.is_some())
                {
                    let active = state.over_scrollbar || state.thumb_grab.is_some();
                    let radius = SCROLLBAR_WIDTH / 2.0;
                    renderer.fill_quad(
                        rounded(bar.track, radius),
                        Color {
                            a: 0.10,
                            ..palette.foreground
                        },
                    );
                    renderer.fill_quad(
                        rounded(bar.thumb, radius),
                        Color {
                            a: if active { 0.50 } else { 0.30 },
                            ..palette.foreground
                        },
                    );
                }
            }
        });

        // A terminal that steps back: what `behind` shows through it.
        if let Some((behind, opacity)) = self.dim {
            renderer.with_layer(bounds, |renderer| {
                renderer.fill_quad(
                    quad(bounds),
                    Color {
                        a: 1.0 - opacity,
                        ..behind
                    },
                );
            });
        }
    }
}

impl<'a, Message, Theme, Renderer> TerminalView<'a, Message, Theme, Renderer> {
    /// Runs a key binding. Returns whether the key was used; a copy with
    /// nothing selected is not, so that the key goes to the program.
    fn run_binding(
        &self,
        binding: Binding,
        terminal: &Terminal,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) -> bool {
        let mut publish = |action: Action| shell.publish((self.on_action)(action));
        match binding {
            Binding::Copy => {
                // Ctrl+Shift+C always copies; Cmd+C only when there is
                // something, since a program cannot receive it anyway.
                publish(Action::Copy);
                true
            }
            // With no text on the clipboard the key is the program's, typed as
            // `^V`: what is there may be an image, which a program like
            // Claude Code fetches itself when it sees Ctrl+V.
            Binding::Paste => match clipboard
                .read(clipboard::Kind::Standard)
                .filter(|t| !t.is_empty())
            {
                Some(text) => {
                    publish(Action::Paste(text));
                    true
                }
                None => false,
            },
            Binding::PastePrimary => {
                let text = clipboard
                    .read(clipboard::Kind::Primary)
                    .or_else(|| clipboard.read(clipboard::Kind::Standard))
                    .filter(|t| !t.is_empty());
                if let Some(text) = text {
                    publish(Action::Paste(text));
                }
                true
            }
            Binding::SelectAll => {
                publish(Action::SelectAll);
                true
            }
            // On the alternate screen there is no history, and these keys are
            // the program's (Shift+PageUp in a pager).
            Binding::ScrollPageUp
            | Binding::ScrollPageDown
            | Binding::ScrollTop
            | Binding::ScrollBottom => {
                if terminal.is_alt_screen() {
                    return false;
                }
                publish(Action::Scroll(match binding {
                    Binding::ScrollPageUp => ScrollTo::PageUp,
                    Binding::ScrollPageDown => ScrollTo::PageDown,
                    Binding::ScrollTop => ScrollTo::Top,
                    _ => ScrollTo::Bottom,
                }));
                true
            }
        }
    }
}

impl<'a, Message, Theme, Renderer> From<TerminalView<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: text::Renderer<Font = iced::Font> + 'a,
{
    fn from(view: TerminalView<'a, Message, Theme, Renderer>) -> Self {
        Element::new(view)
    }
}

// ---- Helpers -----------------------------------------------------------------

/// Ctrl+V, or Cmd+V on macOS, for [`Config::ctrl_v_pastes`](crate::Config).
fn is_paste_key(press: &KeyPress) -> bool {
    if press.kind == KeyEventKind::Release {
        return false;
    }
    let paste_modifier = if cfg!(target_os = "macos") {
        Modifiers::LOGO
    } else {
        Modifiers::CTRL
    };
    press.modifiers == paste_modifier
        && press
            .key
            .to_latin(press.physical_key)
            .is_some_and(|c| c.eq_ignore_ascii_case(&'v'))
}

fn convert_button(button: mouse::Button) -> Option<Button> {
    match button {
        mouse::Button::Left => Some(Button::Left),
        mouse::Button::Middle => Some(Button::Middle),
        mouse::Button::Right => Some(Button::Right),
        _ => None,
    }
}

fn scroll_numbers(terminal: &Terminal) -> (usize, usize, usize) {
    (
        terminal.size().1 as usize,
        terminal.history_size(),
        terminal.display_offset(),
    )
}

/// The link at a cell, converted from screen coordinates.
fn link_under(terminal: &Terminal, at: CellPos) -> Option<LinkHit> {
    let (columns, rows) = terminal.size();
    if at.column < 0 || at.row < 0 || at.column >= columns as i32 || at.row >= rows as i32 {
        return None;
    }
    let point = alacritty_terminal::term::viewport_to_point(
        terminal.display_offset(),
        alacritty_terminal::index::Point::new(
            at.row as usize,
            alacritty_terminal::index::Column(at.column as usize),
        ),
    );
    links::link_at(&terminal.term, point)
}

fn quad(bounds: Rectangle) -> Quad {
    Quad {
        bounds,
        border: Border::default(),
        shadow: Shadow::default(),
        snap: true,
    }
}

fn rounded(bounds: Rectangle, radius: f32) -> Quad {
    Quad {
        bounds,
        border: Border {
            radius: radius.into(),
            ..Border::default()
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

/// What text is set with, which is the same for every run of a frame.
struct TextSetup<'a> {
    cell: CellSize,
    fonts: &'a Fonts,
    size: f32,
    ligatures: bool,
    clip: Rectangle,
}

fn fill_run<Renderer>(renderer: &mut Renderer, run: &Run, position: Point, setup: &TextSetup<'_>)
where
    Renderer: text::Renderer<Font = iced::Font>,
{
    let TextSetup {
        cell,
        fonts,
        size,
        ligatures,
        clip,
    } = *setup;
    // `iced` anchors centred text at the middle of its bounds, not at their
    // left edge.
    let width = run.columns as f32 * cell.width;
    let position = if run.flowing {
        position
    } else {
        Point::new(position.x + width / 2.0, position.y)
    };
    // iced 0.14's text cache lays every `fill_text` out with cosmic-text's
    // default wrapping, whatever `wrapping` says, so a run whose glyphs add
    // up to a hair more than `columns × cell.width` (the measured advance is
    // a float, and so is their sum) had its last character wrapped onto the
    // row below. A run laid out from its left edge gets a cell of room to
    // spare; the clip still stops it at the widget's edge. A centred one is
    // a single character, which cannot wrap.
    let bounds = if run.flowing {
        width + cell.width
    } else {
        width
    };
    renderer.fill_text(
        Text {
            content: run.text.clone(),
            bounds: Size::new(bounds, cell.height),
            size: Pixels(size),
            line_height: text::LineHeight::Absolute(Pixels(cell.height)),
            font: fonts.pick(run.style.bold, run.style.italic),
            // A run is laid out by the font from its first cell. A character
            // placed alone is centred in the cells it owns, so that a glyph
            // from a fallback font with another advance still sits inside
            // them.
            align_x: if run.flowing {
                text::Alignment::Left
            } else {
                text::Alignment::Center
            },
            align_y: alignment::Vertical::Top,
            // A flowing run is plain text. So is a lone character that
            // monospace fonts all carry, which a letter spacing leaves
            // alone in its cell: it needs no fallback font, and the cheaper
            // shaping is the one that runs for every character of a screen.
            shaping: if (run.flowing && !ligatures) || is_plain_cell(&run.text) {
                text::Shaping::Basic
            } else {
                text::Shaping::Advanced
            },
            wrapping: text::Wrapping::None,
        },
        position,
        run.style.color,
        clip,
    );
}

/// Whether a click with `modifiers` opens the link under the pointer, which
/// is when it is underlined and the pointer turns into a hand. A program that
/// has the mouse gets the click instead; Shift and Alt are the selection's
/// (extending it, a block); Ctrl or Cmd opens whatever else is held, and
/// together with Shift is how a link is opened in a program that has the
/// mouse.
fn click_opens_links(terminal: &Terminal, modifiers: Modifiers) -> bool {
    !terminal.mouse_reporting(modifiers)
        && (modifiers.control() || modifiers.logo() || !(modifiers.shift() || modifiers.alt()))
}

/// Whether `text` is one character that every monospace font has (the ones
/// a flowing run is made of).
fn is_plain_cell(text: &str) -> bool {
    let mut chars = text.chars();
    matches!((chars.next(), chars.next()), (Some(c), None) if crate::plan::flows(c))
}

/// Where the baseline sits in a row, as a fraction of the row's height from
/// its top. The font's real metrics are not available to a widget; this is
/// the usual monospace proportion (ascent 0.95 em and descent 0.25 em
/// centred in the line height), close enough to put underlines where they
/// belong.
fn baseline(cell_height: f32, size: f32) -> f32 {
    let content = size * 1.2;
    (cell_height - content) / 2.0 + size * 0.95
}

fn draw_decoration<Renderer>(
    renderer: &mut Renderer,
    kind: Decoration,
    color: Color,
    span: Rectangle,
    metrics: shapes::Metrics,
) where
    Renderer: text::Renderer<Font = iced::Font>,
{
    let thickness = metrics.light.max(metrics.pixel);
    let base = span.y + baseline(span.height, metrics.size);
    let line = |renderer: &mut Renderer, y: f32, h: f32| {
        renderer.fill_quad(
            quad(Rectangle::new(
                Point::new(span.x, y),
                Size::new(span.width, h),
            )),
            color,
        );
    };
    match kind {
        Decoration::Underline => line(renderer, base + thickness, thickness),
        Decoration::Double => {
            line(renderer, base + thickness, thickness);
            line(renderer, base + thickness * 3.0, thickness);
        }
        Decoration::Strike => line(
            renderer,
            span.y + span.height * 0.5 - thickness / 2.0,
            thickness,
        ),
        Decoration::Dotted | Decoration::Dashed | Decoration::Curly => {
            // Pieces of the line, one cell's width divided into a pattern.
            let y0 = base + thickness;
            let cells = (span.width / metrics.width.max(1.0)).round().max(1.0) as usize;
            let (on, off) = match kind {
                Decoration::Dotted => (thickness, thickness),
                Decoration::Dashed => (thickness * 3.0, thickness * 2.0),
                _ => (thickness, 0.0),
            };
            if kind == Decoration::Curly {
                // A zigzag of one-pixel steps: up for half a cell, down for
                // the other half.
                let steps = (metrics.width / metrics.pixel).round().max(2.0) as usize;
                for i in 0..steps * cells {
                    let phase = (i % steps) as f32 / steps as f32;
                    let wave = if phase < 0.5 { 0.0 } else { thickness };
                    renderer.fill_quad(
                        quad(Rectangle::new(
                            Point::new(span.x + i as f32 * metrics.pixel, y0 + wave),
                            Size::new(metrics.pixel, thickness),
                        )),
                        color,
                    );
                }
            } else {
                let mut x = span.x;
                while x < span.x + span.width {
                    let w = on.min(span.x + span.width - x);
                    renderer.fill_quad(
                        quad(Rectangle::new(Point::new(x, y0), Size::new(w, thickness))),
                        color,
                    );
                    x += on + off;
                }
            }
        }
    }
}

/// Adds a rectangle to a row's lines, extending the previous one if this one
/// starts where it ends and is the same height and colour.
fn push_joined(lines: &mut Vec<(Rectangle, Color)>, rect: Rectangle, color: Color) {
    for (last, last_color) in lines.iter_mut().rev().take(4) {
        if *last_color == color
            && (last.y - rect.y).abs() < 1e-3
            && (last.height - rect.height).abs() < 1e-3
            && (last.x + last.width - rect.x).abs() < 1e-3
        {
            last.width = rect.x + rect.width - last.x;
            return;
        }
    }
    lines.push((rect, color));
}

fn draw_shape<Renderer>(
    renderer: &mut Renderer,
    lines: &mut Vec<(Rectangle, Color)>,
    shape: &Shape,
    color: Color,
    origin: Point,
    metrics: shapes::Metrics,
) where
    Renderer: text::Renderer<Font = iced::Font>,
{
    let shift = |r: shapes::Rect| {
        Rectangle::new(
            Point::new(origin.x + r.x, origin.y + r.y),
            Size::new(r.w, r.h),
        )
    };
    match shape {
        Shape::Rects(rects) => {
            for rect in rects {
                push_joined(lines, shift(*rect), color);
            }
        }
        Shape::Shade(alpha) => {
            let whole = Rectangle::new(origin, Size::new(metrics.width, metrics.height));
            renderer.fill_quad(quad(whole), Color { a: *alpha, ..color });
        }
        Shape::Arc(arc) => {
            for stub in &arc.stubs {
                renderer.fill_quad(quad(shift(*stub)), color);
            }
            // The ring is a rounded square with a border, cut down to the
            // quadrant that is the corner by a clipping layer.
            let ring = shift(arc.ring);
            let clip = shift(arc.clip);
            renderer.with_layer(clip, |renderer| {
                renderer.fill_quad(
                    Quad {
                        bounds: ring,
                        border: Border {
                            color,
                            width: arc.thickness,
                            radius: (ring.width / 2.0).into(),
                        },
                        shadow: Shadow::default(),
                        snap: false,
                    },
                    Color::TRANSPARENT,
                );
            });
        }
        Shape::Dots { diameter, centres } => {
            for (x, y) in centres {
                let dot = Rectangle::new(
                    Point::new(origin.x + x - diameter / 2.0, origin.y + y - diameter / 2.0),
                    Size::new(*diameter, *diameter),
                );
                renderer.fill_quad(
                    Quad {
                        bounds: dot,
                        border: Border {
                            radius: (diameter / 2.0).into(),
                            ..Border::default()
                        },
                        shadow: Shadow::default(),
                        snap: false,
                    },
                    color,
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_cursor<Renderer>(
    renderer: &mut Renderer,
    terminal: &Terminal,
    cursor: crate::terminal::CursorView,
    origin: Point,
    cell: CellSize,
    fonts: &Fonts,
    size: f32,
    scale: f32,
    clip: Rectangle,
) where
    Renderer: text::Renderer<Font = iced::Font>,
{
    let palette = terminal.palette();
    let color = terminal.color_of_index(258);
    let width = cell.width * cursor.width as f32;
    let whole = Rectangle::new(origin, Size::new(width, cell.height));
    let pixel = 1.0 / scale;
    let thickness = (size / 8.0 / pixel).round().max(1.0) * pixel;

    if cursor.hollow {
        // An outline: the terminal does not have the focus.
        renderer.fill_quad(
            Quad {
                bounds: whole,
                border: Border {
                    color,
                    width: pixel.max(1.0),
                    radius: 0.0.into(),
                },
                shadow: Shadow::default(),
                snap: true,
            },
            Color::TRANSPARENT,
        );
        return;
    }

    match cursor.shape {
        CursorShape::Block => {
            // The character under the cursor, redrawn in the colour that
            // shows on the cursor's fill, in a layer that covers the cell.
            renderer.with_layer(whole, |renderer| {
                renderer.fill_quad(quad(whole), color);
                let under = terminal.char_at_cursor(cursor);
                if let Some((text, bold, italic)) = under {
                    renderer.fill_text(
                        Text {
                            content: text,
                            bounds: Size::new(width, cell.height),
                            size: Pixels(size),
                            line_height: text::LineHeight::Absolute(Pixels(cell.height)),
                            font: fonts.pick(bold, italic),
                            align_x: text::Alignment::Center,
                            align_y: alignment::Vertical::Top,
                            shaping: text::Shaping::Advanced,
                            wrapping: text::Wrapping::None,
                        },
                        Point::new(origin.x + width / 2.0, origin.y),
                        palette.cursor_text,
                        clip,
                    );
                }
            });
        }
        CursorShape::Beam => {
            renderer.fill_quad(
                quad(Rectangle::new(origin, Size::new(thickness, cell.height))),
                color,
            );
        }
        CursorShape::Underline => {
            renderer.fill_quad(
                quad(Rectangle::new(
                    Point::new(origin.x, origin.y + cell.height - thickness),
                    Size::new(width, thickness),
                )),
                color,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::key::{Code, Physical};

    const CELL: CellSize = CellSize {
        width: 8.0,
        height: 16.0,
    };

    /// Whether every cached plan is the plan of the row as it is now.
    fn plans_are_current(cache: &RowCache, terminal: &Terminal) -> bool {
        cache.plans.iter().enumerate().all(|(row, slot)| {
            slot.as_ref()
                .is_some_and(|(_, plan)| *plan == plan_row(terminal, row))
        })
    }

    #[test]
    fn the_row_cache_follows_scrolls_and_output_row_for_row() {
        let mut terminal = Terminal::new(crate::Config::default(), 30, 12);
        for i in 0..300 {
            let colour = 31 + i % 7;
            terminal.feed(
                format!("\x1b[{colour}mline\x1b[0m {i} {}\r\n", "x".repeat(i % 20)).as_bytes(),
            );
        }
        let mut cache = RowCache::default();
        cache.sync(&terminal);
        assert!(plans_are_current(&cache, &terminal));
        let steps: &[&dyn Fn(&mut Terminal)] = &[
            &|t| drop(t.perform(Action::Scroll(ScrollTo::Lines(3)))),
            &|t| drop(t.perform(Action::Scroll(ScrollTo::Lines(1)))),
            &|t| drop(t.perform(Action::Scroll(ScrollTo::Lines(-2)))),
            &|t| drop(t.perform(Action::Scroll(ScrollTo::PageUp))),
            &|t| drop(t.feed(b"new output\r\n")),
            &|t| drop(t.perform(Action::Scroll(ScrollTo::Lines(11)))),
            &|t| drop(t.perform(Action::Scroll(ScrollTo::Offset(50)))),
            &|t| drop(t.perform(Action::Scroll(ScrollTo::Top))),
            &|t| drop(t.perform(Action::Scroll(ScrollTo::Lines(-4)))),
            &|t| t.scroll_to_bottom(),
            &|t| drop(t.feed(b"\x1b[2Jcleared\r\n")),
        ];
        for (i, step) in steps.iter().enumerate() {
            step(&mut terminal);
            cache.sync(&terminal);
            assert!(plans_are_current(&cache, &terminal), "after step {i}");
        }
    }

    #[test]
    fn a_notch_of_the_wheel_plans_only_the_rows_that_came_into_sight() {
        let mut terminal = Terminal::new(crate::Config::default(), 30, 12);
        for i in 0..300 {
            terminal.feed(format!("line {i}\r\n").as_bytes());
        }
        let mut cache = RowCache::default();
        cache.sync(&terminal);
        let planned: Vec<u64> = cache
            .plans
            .iter()
            .map(|slot| slot.as_ref().unwrap().0)
            .collect();
        terminal.perform(Action::Scroll(ScrollTo::Lines(3)));
        cache.sync(&terminal);
        // The nine rows kept their plans (planned at their old generation).
        let kept: Vec<u64> = cache.plans[3..]
            .iter()
            .map(|slot| slot.as_ref().unwrap().0)
            .collect();
        assert_eq!(kept, planned[..9]);
        assert!(plans_are_current(&cache, &terminal));
    }

    #[test]
    fn the_tree_a_wrapper_makes_without_the_terminal_is_the_views_own() {
        // A wrapper answers the tag and the state with `view_tag` and
        // `view_state` rather than locking its terminal: they must be what
        // the view itself answers, or its state would be reset every frame.
        let terminal = Terminal::new(crate::Config::default(), 80, 24);
        let view: TerminalView<'_, (), iced::Theme, iced::Renderer> =
            terminal_view(&terminal, |_| ());
        assert_eq!(
            view_tag(),
            Widget::<(), iced::Theme, iced::Renderer>::tag(&view)
        );
        let state = view_state();
        let tree::State::Some(state) = state else {
            panic!("a state");
        };
        assert!(state.downcast_ref::<State>().is_some());
        assert!(Widget::<(), iced::Theme, iced::Renderer>::children(&view).is_empty());
    }

    #[test]
    fn shapes_are_built_once_per_metrics() {
        let mut cache = ShapeCache::default();
        let small = shapes::Metrics::new(8.0, 17.0, 14.0, 1.0);
        let large = shapes::Metrics::new(10.0, 21.0, 18.0, 2.0);
        assert_eq!(cache.get('╭', small), shapes::shape('╭', small).as_ref());
        assert_eq!(cache.get('─', small), shapes::shape('─', small).as_ref());
        assert_eq!(cache.get('a', small), None);
        assert_eq!(cache.shapes.len(), 3);
        // Other metrics: drawn again, at their size.
        assert_eq!(cache.get('─', large), shapes::shape('─', large).as_ref());
        assert_eq!(cache.shapes.len(), 1);
    }

    #[test]
    fn the_grid_is_the_whole_cells_that_fit() {
        assert_eq!(grid_size(Size::new(800.0, 480.0), CELL), (100, 30));
        assert_eq!(grid_size(Size::new(807.9, 495.9), CELL), (100, 30));
        // Never zero: a widget squeezed to nothing still has a grid.
        assert_eq!(grid_size(Size::new(3.0, 3.0), CELL), (1, 1));
    }

    #[test]
    fn a_position_is_a_cell_and_a_half_of_it() {
        let origin = Point::new(10.0, 20.0);
        let pos = cell_at(
            Point::new(10.0 + 8.0 * 3.0 + 1.0, 20.0 + 16.0 * 2.0 + 3.0),
            origin,
            CELL,
        );
        assert_eq!((pos.column, pos.row, pos.side), (3, 2, Side::Left));
        let pos = cell_at(Point::new(10.0 + 8.0 * 3.0 + 6.0, 20.0), origin, CELL);
        assert_eq!((pos.column, pos.side), (3, Side::Right));
    }

    #[test]
    fn positions_outside_the_grid_are_outside_it() {
        let origin = Point::new(10.0, 20.0);
        let pos = cell_at(Point::new(5.0, 12.0), origin, CELL);
        assert_eq!((pos.column, pos.row), (-1, -1));
    }

    #[test]
    fn the_scrollbar_thumb_sits_at_the_bottom_when_not_scrolled() {
        let area = Rectangle::new(Point::new(0.0, 0.0), Size::new(800.0, 480.0));
        let bar = scrollbar(area, 30, 270, 0).unwrap();
        assert!((bar.thumb.y + bar.thumb.height - 480.0).abs() < 0.01);
        // Scrolled to the top, at the top.
        let bar = scrollbar(area, 30, 270, 270).unwrap();
        assert!(bar.thumb.y.abs() < 0.01);
        // Its size is the share of the history that is on screen.
        assert!((bar.thumb.height - 48.0).abs() < 0.01);
    }

    #[test]
    fn there_is_no_scrollbar_without_history() {
        let area = Rectangle::new(Point::new(0.0, 0.0), Size::new(800.0, 480.0));
        assert!(scrollbar(area, 30, 0, 0).is_none());
    }

    #[test]
    fn dragging_the_thumb_maps_back_to_an_offset() {
        let area = Rectangle::new(Point::new(0.0, 0.0), Size::new(800.0, 480.0));
        let bar = scrollbar(area, 30, 270, 0).unwrap();
        // Grabbed at its middle, moved to the very top of the track.
        assert_eq!(bar.offset_at(0.0, bar.thumb.height / 2.0, 270), 270);
        assert_eq!(bar.offset_at(480.0, bar.thumb.height / 2.0, 270), 0);
        let middle = bar.offset_at(240.0, bar.thumb.height / 2.0, 270);
        assert!((120..=150).contains(&middle), "{middle}");
    }

    fn press(key: Key, modifiers: Modifiers) -> KeyPress {
        KeyPress {
            modified_key: key.clone(),
            key,
            physical_key: Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
            location: keyboard::Location::Standard,
            modifiers,
            text: None,
            kind: KeyEventKind::Press,
        }
    }

    #[test]
    fn the_standard_bindings_leave_ctrl_c_to_the_program() {
        let bindings = Bindings::standard();
        let c = Key::Character("c".into());
        assert_eq!(bindings.find(&press(c.clone(), Modifiers::CTRL)), None);
        assert_eq!(
            bindings.find(&press(c, Modifiers::CTRL | Modifiers::SHIFT)),
            Some(Binding::Copy)
        );
        let v = Key::Character("v".into());
        assert_eq!(
            bindings.find(&press(v, Modifiers::CTRL | Modifiers::SHIFT)),
            Some(Binding::Paste)
        );
        assert_eq!(
            bindings.find(&press(Key::Named(Named::Insert), Modifiers::SHIFT)),
            Some(Binding::PastePrimary)
        );
        assert_eq!(
            bindings.find(&press(Key::Named(Named::PageUp), Modifiers::SHIFT)),
            Some(Binding::ScrollPageUp)
        );
    }

    #[test]
    fn bindings_follow_the_latin_letter_on_other_layouts() {
        let mut p = press(
            Key::Character("с".into()),
            Modifiers::CTRL | Modifiers::SHIFT,
        );
        p.physical_key = Physical::Code(Code::KeyC);
        assert_eq!(Bindings::standard().find(&p), Some(Binding::Copy));
    }

    #[test]
    fn ctrl_v_is_a_paste_only_when_the_configuration_says_so() {
        let v = Key::Character("v".into());
        let paste_modifier = if cfg!(target_os = "macos") {
            Modifiers::LOGO
        } else {
            Modifiers::CTRL
        };
        assert!(is_paste_key(&press(v.clone(), paste_modifier)));
        assert!(!is_paste_key(&press(
            v.clone(),
            paste_modifier | Modifiers::ALT
        )));
        assert!(!is_paste_key(&press(
            Key::Character("b".into()),
            paste_modifier
        )));
        // The standard bindings alone leave Ctrl+V to the program.
        assert_eq!(Bindings::standard().find(&press(v, Modifiers::CTRL)), None);
    }

    #[test]
    fn releases_trigger_nothing_and_an_empty_set_binds_nothing() {
        let mut p = press(
            Key::Character("c".into()),
            Modifiers::CTRL | Modifiers::SHIFT,
        );
        p.kind = KeyEventKind::Release;
        assert_eq!(Bindings::standard().find(&p), None);
        p.kind = KeyEventKind::Press;
        assert_eq!(Bindings::none().find(&p), None);
    }
}
