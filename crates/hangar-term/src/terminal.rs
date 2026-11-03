//! The terminal's state: the emulated screen and everything around it.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event as EmulatorEvent, EventListener, WindowSize};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::selection::SelectionRange;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{self, ClipboardType, Term, TermDamage, TermMode};
use alacritty_terminal::vte::ansi::{CursorShape as EmulatorShape, CursorStyle, Processor, Rgb};

use crate::config::{Config, CursorShape, Osc52};
use crate::event::{
    Action, ClipboardReply, ClipboardTarget, Event, Metrics, PtySize, ScrollTo,
};
use crate::font::Font;
use crate::keys::KeyOptions;
use crate::osc::{Osc, OscScanner};
use crate::palette::{Palette, to_rgb8};
use crate::pointer::PointerState;
use crate::search::Search;

/// How long the visual bell flashes.
pub(crate) const BELL_FLASH: Duration = Duration::from_millis(140);

/// The emulator's events, queued as it raises them and collected once it is
/// done with a chunk of output.
///
/// The emulator calls its listener from inside the parser, where nothing can
/// be borrowed from the terminal, so the events wait in a shared queue until
/// [`Terminal::collect`] drains it.
#[derive(Clone, Default)]
pub(crate) struct Listener(Arc<Mutex<Vec<EmulatorEvent>>>);

impl EventListener for Listener {
    fn send_event(&self, event: EmulatorEvent) {
        self.0.lock().expect("event queue lock").push(event);
    }
}

/// The dimensions the emulator wants, which is only the grid's.
struct GridSize {
    columns: usize,
    rows: usize,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.columns
    }
}

/// The cursor as the widget draws it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CursorView {
    pub shape: CursorShape,
    /// Block cursors of an unfocused terminal are outlines.
    pub hollow: bool,
    pub row: usize,
    pub column: usize,
    /// Cells wide: 2 over a wide character.
    pub width: usize,
}

/// A terminal: the screen a program draws, its history, and the state around
/// them (selection, search, focus, the title).
///
/// It owns no process. The app reads the bytes the process prints, passes
/// them to [`feed`](Self::feed), and writes back the bytes that come out of
/// the returned [`Event`]s. Everything else, drawing and input, goes through
/// the widget, [`TerminalView`](crate::TerminalView).
pub struct Terminal {
    pub(crate) term: Term<Listener>,
    parser: Processor,
    queue: Listener,
    osc: OscScanner,
    pub(crate) config: Config,
    palette: Palette,
    font: Font,
    pub(crate) columns: usize,
    pub(crate) rows: usize,
    /// A cell in logical pixels, as the widget last measured it; the process
    /// is told the window's size in pixels as well as in cells.
    cell_size: (f32, f32),
    title: String,
    focused: bool,
    window_focused: bool,
    pub(crate) pointer: PointerState,
    pub(crate) search: Option<Search>,
    /// Bumped by anything that makes every row's cached drawing wrong: a
    /// resize, a new palette, a new font.
    epoch: u64,
    /// Bumped on each batch of changes.
    generation: u64,
    /// The generation at which each visible row last changed.
    row_generation: Vec<u64>,
    /// Bumped when the cursor moves or changes shape, so the widget can
    /// restart its blink: a cursor that just moved is always drawn.
    cursor_generation: u64,
    last_cursor: (Point, EmulatorShape),
    bell_at: Option<Instant>,
}

impl Terminal {
    /// A terminal of `columns` by `rows` cells.
    ///
    /// The app creates the process with this size; the widget will report the
    /// size it really has room for as soon as it is on screen, and that
    /// arrives as an [`Event::Resize`].
    pub fn new(config: Config, columns: u16, rows: u16) -> Self {
        let columns = (columns as usize).max(term::MIN_COLUMNS);
        let rows = (rows as usize).max(term::MIN_SCREEN_LINES);
        let queue = Listener::default();
        let term = Term::new(
            emulator_config(&config),
            &GridSize { columns, rows },
            queue.clone(),
        );
        let last_cursor = (term.grid().cursor.point, EmulatorShape::Block);
        let mut terminal = Self {
            term,
            parser: Processor::new(),
            queue,
            osc: OscScanner::default(),
            config,
            palette: Palette::default(),
            font: Font::default(),
            columns,
            rows,
            cell_size: (8.0, 16.0),
            title: String::new(),
            focused: false,
            window_focused: true,
            pointer: PointerState::default(),
            search: None,
            epoch: 1,
            generation: 1,
            row_generation: vec![1; rows],
            cursor_generation: 0,
            last_cursor,
            bell_at: None,
        };
        terminal.sync_damage();
        terminal
    }

    // ---- Output from the process ------------------------------------------

    /// Feeds bytes the process printed.
    ///
    /// Chunks may end anywhere, in the middle of an escape sequence or of a
    /// UTF-8 character; the state is kept for the next call. Several reads
    /// are better passed together than one at a time, since the work per call
    /// is the same however little it carries.
    ///
    /// A terminal needs no window to be fed: the panes of a workspace that is
    /// not on screen keep parsing their output, and what they report (the
    /// answers to queries, a notification) is still returned.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Event> {
        self.feed_inner(bytes, false)
    }

    /// Feeds output that was already seen once: the history a window takes
    /// over from another, or a saved buffer.
    ///
    /// The bytes are parsed like any other, but nothing is said back. The
    /// history holds the queries the program sent when it started (cursor
    /// position, device attributes, colours), and answering them now would
    /// write replies nobody is waiting for into the process's input, where
    /// they show up as typed characters. The bell does not ring and
    /// notifications and clipboard requests do not fire for the same reason:
    /// they happened long ago. Titles and the working directory are still
    /// reported, since they describe the state the replay ends in.
    pub fn replay(&mut self, bytes: &[u8]) -> Vec<Event> {
        self.feed_inner(bytes, true)
    }

    fn feed_inner(&mut self, bytes: &[u8], replay: bool) -> Vec<Event> {
        let mut events = Vec::new();
        self.expire_sync();
        let mut found = Vec::new();
        self.osc.scan(bytes, &mut found);
        self.parser.advance(&mut self.term, bytes);
        self.collect(&mut events, replay);
        for osc in found {
            match osc {
                Osc::Notification { .. } if replay => {}
                Osc::Notification { title, body } => events.push(Event::Notification { title, body }),
                Osc::WorkingDirectory(path) => events.push(Event::WorkingDirectory(path)),
            }
        }
        if let Some(search) = &mut self.search {
            search.mark_stale();
        }
        self.refresh_search(Instant::now());
        events
    }

    /// Ends a synchronized update whose time is up.
    ///
    /// Programs wrap a redraw in `CSI ? 2026 h` and `l` so that the screen
    /// changes in one step instead of showing half a frame. The emulator holds
    /// the bytes back until the update ends; if the program never ends it, it
    /// must still be shown eventually.
    fn expire_sync(&mut self) {
        let due = self.parser.sync_timeout().sync_timeout();
        if due.is_some_and(|at| Instant::now() >= at) {
            self.parser.stop_sync(&mut self.term);
        }
    }

    /// When something is due without any more input: a synchronized update
    /// that has to be shown, a search that has to be refreshed, the end of the
    /// visual bell. The widget asks for a redraw then and sends
    /// [`Action::Tick`].
    pub fn deadline(&self) -> Option<Instant> {
        let sync = self.parser.sync_timeout().sync_timeout();
        let search = self.search.as_ref().and_then(Search::refresh_due);
        let bell = self.bell_at.map(|at| at + BELL_FLASH);
        [sync, search, bell].into_iter().flatten().min()
    }

    /// Does what [`deadline`](Self::deadline) was waiting for, if its time has
    /// come.
    pub fn tick(&mut self, now: Instant) -> Vec<Event> {
        let mut events = Vec::new();
        let sync_due = self.parser.sync_timeout().sync_timeout();
        if sync_due.is_some_and(|at| now >= at) {
            self.parser.stop_sync(&mut self.term);
            self.collect(&mut events, false);
        }
        self.refresh_search(now);
        if self.bell_at.is_some_and(|at| now >= at + BELL_FLASH) {
            self.bell_at = None;
        }
        events
    }

    /// Drains the emulator's queue into events and brings the damage
    /// bookkeeping up to date.
    fn collect(&mut self, events: &mut Vec<Event>, replay: bool) {
        let queued = std::mem::take(&mut *self.queue.0.lock().expect("event queue lock"));
        // Replies to queries are collapsed into one write: a program that
        // sends several in a row (device attributes, cursor position, colours)
        // reads them back as one burst.
        let mut reply: Vec<u8> = Vec::new();
        for event in queued {
            match event {
                // Answers, and things that happen once, are for the present
                // only; a replay swallows them.
                EmulatorEvent::PtyWrite(_)
                | EmulatorEvent::ColorRequest(..)
                | EmulatorEvent::TextAreaSizeRequest(_)
                | EmulatorEvent::Bell
                | EmulatorEvent::ClipboardStore(..)
                | EmulatorEvent::ClipboardLoad(..)
                    if replay => {}
                EmulatorEvent::PtyWrite(text) => reply.extend_from_slice(text.as_bytes()),
                EmulatorEvent::ColorRequest(index, format) => {
                    let (r, g, b) = to_rgb8(self.color_of_index(index));
                    reply.extend_from_slice(format(Rgb { r, g, b }).as_bytes());
                }
                EmulatorEvent::TextAreaSizeRequest(format) => {
                    let reply_text = format(WindowSize {
                        num_lines: self.rows as u16,
                        num_cols: self.columns as u16,
                        cell_width: self.cell_size.0.round() as u16,
                        cell_height: self.cell_size.1.round() as u16,
                    });
                    reply.extend_from_slice(reply_text.as_bytes());
                }
                EmulatorEvent::Title(title) => {
                    self.title.clone_from(&title);
                    events.push(Event::Title(title));
                }
                EmulatorEvent::ResetTitle => {
                    self.title.clear();
                    events.push(Event::ResetTitle);
                }
                EmulatorEvent::Bell => {
                    if self.config.visual_bell {
                        self.bell_at = Some(Instant::now());
                    }
                    events.push(Event::Bell);
                }
                EmulatorEvent::ClipboardStore(kind, text) => events.push(Event::SetClipboard {
                    target: target_of(kind),
                    text,
                }),
                EmulatorEvent::ClipboardLoad(kind, format) => events.push(Event::ClipboardLoad {
                    target: target_of(kind),
                    reply: ClipboardReply(format),
                }),
                EmulatorEvent::CursorBlinkingChange => self.cursor_generation += 1,
                // Nothing to do for these: the widget redraws on the damage,
                // and the process is not ours to wait for.
                EmulatorEvent::MouseCursorDirty
                | EmulatorEvent::Wakeup
                | EmulatorEvent::Exit
                | EmulatorEvent::ChildExit(_) => {}
            }
        }
        if !reply.is_empty() {
            events.push(Event::Write(reply));
        }
        self.sync_damage();
    }

    /// Records which rows changed since the last call.
    ///
    /// The widget caches the drawing of each row and redoes only the rows
    /// whose number changed here. Full damage (a scroll, a clear, a palette
    /// change) is the common case while output streams, and costs one pass over
    /// the rows, which is nothing next to drawing them.
    pub(crate) fn sync_damage(&mut self) {
        self.generation += 1;
        let generation = self.generation;
        match self.term.damage() {
            TermDamage::Full => self.row_generation.iter_mut().for_each(|g| *g = generation),
            TermDamage::Partial(lines) => {
                for damaged in lines {
                    if let Some(g) = self.row_generation.get_mut(damaged.line) {
                        *g = generation;
                    }
                }
            }
        }
        self.term.reset_damage();

        let cursor = (self.term.grid().cursor.point, self.term.cursor_style().shape);
        if cursor != self.last_cursor {
            self.last_cursor = cursor;
            self.cursor_generation += 1;
        }
    }

    // ---- What the app can change ------------------------------------------

    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    /// Sets the colours. Rows are redrawn with them.
    pub fn set_palette(&mut self, palette: Palette) {
        if self.palette != palette {
            self.palette = palette;
            self.epoch += 1;
        }
    }

    pub fn font(&self) -> &Font {
        &self.font
    }

    /// Sets the font. The widget measures it and reports the new size of the
    /// grid as a [`Event::Resize`].
    pub fn set_font(&mut self, font: Font) {
        if self.font != font {
            self.font = font;
            self.epoch += 1;
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Changes the behaviour options. The scrollback keeps what fits.
    pub fn set_config(&mut self, config: Config) {
        if self.config == config {
            return;
        }
        self.term.set_options(emulator_config(&config));
        // Setting the options makes the emulator announce the title again,
        // which has not changed.
        self.queue.0.lock().expect("event queue lock").clear();
        self.config = config;
        self.epoch += 1;
        self.sync_damage();
    }

    /// Gives or takes the focus.
    ///
    /// The focused terminal draws a solid cursor and gets the keyboard. A
    /// program that asked to know (`CSI ? 1004 h`) is told when it changes.
    pub fn set_focused(&mut self, focused: bool) -> Vec<Event> {
        if self.focused == focused {
            return Vec::new();
        }
        let before = self.has_focus();
        self.focused = focused;
        self.focus_changed(before)
    }

    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// Whether the pane and the window both have the focus: the state the
    /// process hears about.
    pub(crate) fn has_focus(&self) -> bool {
        self.focused && self.window_focused
    }

    fn set_window_focus(&mut self, focused: bool) -> Vec<Event> {
        if self.window_focused == focused {
            return Vec::new();
        }
        let before = self.has_focus();
        self.window_focused = focused;
        self.focus_changed(before)
    }

    fn focus_changed(&mut self, before: bool) -> Vec<Event> {
        let now = self.has_focus();
        self.term.is_focused = now;
        self.bump_cursor_rows();
        if before == now || !self.term.mode().contains(TermMode::FOCUS_IN_OUT) {
            return Vec::new();
        }
        vec![Event::Write(if now { b"\x1b[I".to_vec() } else { b"\x1b[O".to_vec() })]
    }

    fn bump_cursor_rows(&mut self) {
        self.cursor_generation += 1;
    }

    // ---- Actions from the widget ------------------------------------------

    /// Applies something that happened to the widget.
    pub fn perform(&mut self, action: Action) -> Vec<Event> {
        let mut events = Vec::new();
        match action {
            Action::Input(bytes) => {
                self.scroll_to_bottom();
                self.term.selection = None;
                events.push(Event::UserInput);
                events.push(Event::Write(bytes));
            }
            Action::Report(bytes) => events.push(Event::Write(bytes)),
            Action::Paste(text) => {
                self.scroll_to_bottom();
                self.term.selection = None;
                events.push(Event::UserInput);
                events.push(Event::Write(self.paste_bytes(&text)));
            }
            Action::Resize(metrics) => self.resize_to(metrics, &mut events),
            Action::Pointer(pointer) => self.pointer(pointer, &mut events),
            Action::Scroll(to) => self.scroll(to),
            Action::WindowFocus(focused) => events.extend(self.set_window_focus(focused)),
            Action::Copy => {
                if let Some(text) = self.selection_text() {
                    events.push(Event::SetClipboard {
                        target: ClipboardTarget::Clipboard,
                        text,
                    });
                }
            }
            Action::SelectAll => self.select_all(),
            Action::ClearSelection => self.term.selection = None,
            Action::FileDropped(path) => events.push(Event::FileDropped(path)),
            Action::Tick => events.extend(self.tick(Instant::now())),
        }
        self.sync_damage();
        events
    }

    /// The bytes that paste `text`: with the markers of bracketed paste if the
    /// program turned it on, so it can tell a paste from typing, and with
    /// line breaks as the Enter key sends them.
    ///
    /// Escape characters are removed from what is pasted in that mode. Left in,
    /// text could end the bracket early with its own `ESC [ 201 ~` and have
    /// the rest read as typed commands.
    pub fn paste_bytes(&self, text: &str) -> Vec<u8> {
        let normalised = text.replace("\r\n", "\r").replace('\n', "\r");
        if self.term.mode().contains(TermMode::BRACKETED_PASTE) {
            let mut out = b"\x1b[200~".to_vec();
            out.extend(normalised.replace('\x1b', "").into_bytes());
            out.extend_from_slice(b"\x1b[201~");
            out
        } else {
            normalised.into_bytes()
        }
    }

    /// Resizes the grid to the cells the widget has room for.
    fn resize_to(&mut self, metrics: Metrics, events: &mut Vec<Event>) {
        let columns = (metrics.columns as usize).max(term::MIN_COLUMNS);
        let rows = (metrics.rows as usize).max(term::MIN_SCREEN_LINES);
        self.cell_size = (metrics.cell_width, metrics.cell_height);
        if columns == self.columns && rows == self.rows {
            return;
        }
        self.term.resize(GridSize { columns, rows });
        self.columns = columns;
        self.rows = rows;
        self.row_generation = vec![self.generation; rows];
        self.epoch += 1;
        if let Some(search) = &mut self.search {
            search.mark_stale();
        }
        events.push(Event::Resize(PtySize {
            rows: rows as u16,
            columns: columns as u16,
            pixel_width: (columns as f32 * metrics.cell_width).round() as u16,
            pixel_height: (rows as f32 * metrics.cell_height).round() as u16,
        }));
    }

    /// Resizes without a widget, for an app that sizes the grid itself.
    pub fn resize(&mut self, columns: u16, rows: u16) -> Vec<Event> {
        self.perform(Action::Resize(Metrics {
            columns,
            rows,
            cell_width: self.cell_size.0,
            cell_height: self.cell_size.1,
        }))
    }

    // ---- Scrolling ---------------------------------------------------------

    pub(crate) fn scroll(&mut self, to: ScrollTo) {
        let scroll = match to {
            ScrollTo::Lines(n) => Scroll::Delta(n),
            ScrollTo::PageUp => Scroll::PageUp,
            ScrollTo::PageDown => Scroll::PageDown,
            ScrollTo::Top => Scroll::Top,
            ScrollTo::Bottom => Scroll::Bottom,
            ScrollTo::Offset(offset) => {
                self.set_display_offset(offset);
                return;
            }
        };
        self.term.scroll_display(scroll);
    }

    pub(crate) fn set_display_offset(&mut self, offset: usize) {
        let current = self.term.grid().display_offset() as i32;
        let target = offset.min(self.term.history_size()) as i32;
        if target != current {
            self.term.scroll_display(Scroll::Delta(target - current));
        }
    }

    /// Jumps back to the live screen, as typing does.
    pub fn scroll_to_bottom(&mut self) {
        self.term.scroll_display(Scroll::Bottom);
    }

    /// How many lines above the live screen the view is. Zero at the bottom.
    pub fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    /// Lines of history above the screen.
    pub fn history_size(&self) -> usize {
        self.term.history_size()
    }

    // ---- Selection ---------------------------------------------------------

    /// The selected text, if anything is selected.
    pub fn selection_text(&self) -> Option<String> {
        self.term.selection_to_string().filter(|text| !text.is_empty())
    }

    pub fn has_selection(&self) -> bool {
        self.selection_range().is_some()
    }

    pub fn select_all(&mut self) {
        use alacritty_terminal::index::Side;
        use alacritty_terminal::selection::{Selection, SelectionType};
        let start = Point::new(self.term.topmost_line(), Column(0));
        let end = Point::new(self.term.bottommost_line(), self.term.last_column());
        let mut selection = Selection::new(SelectionType::Simple, start, Side::Left);
        selection.update(end, Side::Right);
        self.term.selection = Some(selection);
    }

    pub fn clear_selection(&mut self) {
        self.term.selection = None;
    }

    pub(crate) fn selection_range(&self) -> Option<SelectionRange> {
        self.term.selection.as_ref().and_then(|s| s.to_range(&self.term))
    }

    // ---- What is on screen ------------------------------------------------

    /// The grid's size in cells, columns then rows.
    pub fn size(&self) -> (u16, u16) {
        (self.columns as u16, self.rows as u16)
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    /// The modes the program has set, for the few decisions that depend on
    /// them (is the mouse wanted, are we on the alternate screen).
    pub fn mode(&self) -> TermMode {
        *self.term.mode()
    }

    /// Whether the program is on the alternate screen, as full-screen
    /// programs are and shells are not.
    pub fn is_alt_screen(&self) -> bool {
        self.term.mode().contains(TermMode::ALT_SCREEN)
    }

    /// The text of the live screen, one string per row without trailing
    /// spaces. For code that reads the screen the way a person would, such as
    /// spotting a question the agent is waiting on.
    pub fn screen_text(&self) -> Vec<String> {
        (0..self.rows as i32).map(|row| self.row_text(Line(row), true)).collect()
    }

    /// The text of one row of the grid; `trim` drops the spaces after the
    /// last character.
    fn row_text(&self, line: Line, trim: bool) -> String {
        let grid = self.term.grid();
        let mut text = String::new();
        for column in 0..self.columns {
            let cell = &grid[line][Column(column)];
            if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                continue;
            }
            text.push(if cell.c == '\0' { ' ' } else { cell.c });
            for extra in cell.zerowidth().into_iter().flatten() {
                text.push(*extra);
            }
        }
        if trim {
            text.truncate(text.trim_end().len());
        }
        text
    }

    fn row_is_blank(&self, line: Line) -> bool {
        let grid = self.term.grid();
        (0..self.columns).all(|column| {
            let cell = &grid[line][Column(column)];
            matches!(cell.c, ' ' | '\0') && cell.zerowidth().is_none_or(|z| z.is_empty())
        })
    }

    /// The last `n` lines of text, history included, oldest first.
    ///
    /// A line is a logical one: a long line the emulator wrapped over several
    /// rows is read as the one line it was printed as. The blank rows below
    /// the last text, which the screen is padded with, are not lines. This is
    /// what the app's control API reads when an agent asks for the output of a
    /// pane.
    pub fn read_lines(&self, n: usize) -> Vec<String> {
        if n == 0 {
            return Vec::new();
        }
        let grid = self.term.grid();
        let top = self.term.topmost_line();
        let mut last = self.term.bottommost_line();
        while last > top && self.row_is_blank(last) {
            last = Line(last.0 - 1);
        }
        if self.row_is_blank(last) {
            return Vec::new();
        }
        let wraps_into_next = |line: Line| {
            line > top && grid[Line(line.0 - 1)][Column(self.columns - 1)].flags.contains(Flags::WRAPLINE)
        };
        let mut lines = Vec::new();
        let mut end = last;
        loop {
            let mut first = end;
            while wraps_into_next(first) {
                first = Line(first.0 - 1);
            }
            let mut text = String::new();
            for line in first.0..=end.0 {
                text.push_str(&self.row_text(Line(line), line == end.0));
            }
            lines.push(text);
            if lines.len() == n || first == top {
                break;
            }
            end = Line(first.0 - 1);
        }
        lines.reverse();
        lines
    }

    /// Clears the screen and the history, keeping the line the cursor is on
    /// as the first one, which is what xterm does: the prompt stays, so the
    /// shell does not have to be asked to draw it again. Nothing happens on
    /// the alternate screen, which has no history and belongs to the program.
    pub fn clear(&mut self) {
        if self.is_alt_screen() {
            return;
        }
        let cursor = self.term.grid().cursor.point;
        let row = self.term.grid()[cursor.line].clone();
        // Erasing the screen pushes it into the history; then the history goes.
        self.parser.advance(&mut self.term, b"\x1b[2J\x1b[3J");
        let grid = self.term.grid_mut();
        grid[Line(0)] = row;
        grid.cursor.point.line = Line(0);
        self.term.selection = None;
        self.term.scroll_display(Scroll::Bottom);
        self.invalidate_rows();
        self.sync_damage();
    }

    /// Whether the program asked for application cursor keys (`CSI ? 1 h`),
    /// which changes what the arrow keys send. The app's control API reports
    /// it so that keys it types on a pane's behalf are encoded right.
    pub fn application_cursor(&self) -> bool {
        self.term.mode().contains(TermMode::APP_CURSOR)
    }

    /// Marks every visible row as changed.
    fn invalidate_rows(&mut self) {
        self.generation += 1;
        let generation = self.generation;
        self.row_generation.iter_mut().for_each(|g| *g = generation);
    }

    pub(crate) fn key_options(&self) -> KeyOptions {
        KeyOptions {
            alt_sends_escape: self.config.alt_sends_escape,
            shift_enter: self.config.shift_enter.clone(),
        }
    }

    /// When the bell last rang and is still flashing.
    pub(crate) fn bell_age(&self, now: Instant) -> Option<Duration> {
        self.bell_at
            .map(|at| now.saturating_duration_since(at))
            .filter(|age| *age < BELL_FLASH)
    }

    pub(crate) fn epoch(&self) -> u64 {
        self.epoch
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn row_generation(&self, row: usize) -> u64 {
        self.row_generation.get(row).copied().unwrap_or(0)
    }

    pub(crate) fn cursor_generation(&self) -> u64 {
        self.cursor_generation
    }

    /// Whether the cursor blinks right now.
    pub(crate) fn cursor_blinks(&self) -> bool {
        self.term.cursor_style().blinking
    }

    /// The cursor, if it is on screen.
    pub(crate) fn cursor(&self) -> Option<CursorView> {
        if !self.term.mode().contains(TermMode::SHOW_CURSOR) {
            return None;
        }
        let point = self.term.grid().cursor.point;
        let offset = self.term.grid().display_offset();
        let row = point.line.0 as usize + offset;
        if row >= self.rows {
            return None;
        }
        let mut column = point.column.0.min(self.columns - 1);
        let cell = &self.term.grid()[point.line][Column(column)];
        if cell.flags.contains(Flags::WIDE_CHAR_SPACER) && column > 0 {
            column -= 1;
        }
        let wide = self.term.grid()[point.line][Column(column)].flags.contains(Flags::WIDE_CHAR);
        let shape = match self.term.cursor_style().shape {
            EmulatorShape::Block | EmulatorShape::HollowBlock => CursorShape::Block,
            EmulatorShape::Beam => CursorShape::Beam,
            EmulatorShape::Underline => CursorShape::Underline,
            EmulatorShape::Hidden => return None,
        };
        let hollow = (!self.has_focus() && shape == CursorShape::Block)
            || self.term.cursor_style().shape == EmulatorShape::HollowBlock;
        Some(CursorView {
            shape,
            hollow,
            row,
            column,
            width: if wide { 2 } else { 1 },
        })
    }

    /// The text of the cell under the cursor and whether it is bold and
    /// italic, if there is anything to draw there.
    pub(crate) fn char_at_cursor(&self, cursor: CursorView) -> Option<(String, bool, bool)> {
        let line = Line(cursor.row as i32 - self.display_offset() as i32);
        let cell = &self.term.grid()[line][Column(cursor.column)];
        let zerowidth = cell.zerowidth().filter(|z| !z.is_empty());
        if matches!(cell.c, ' ' | '\0' | '\t') && zerowidth.is_none() {
            return None;
        }
        let mut text = String::from(cell.c);
        if let Some(extra) = zerowidth {
            text.extend(extra.iter());
        }
        Some((text, cell.flags.contains(Flags::BOLD), cell.flags.contains(Flags::ITALIC)))
    }

    /// The colour of a palette index as the emulator numbers them: 0 to 255
    /// are the indexed colours, 256 the foreground, 257 the background and 258
    /// the cursor. Colours the program changed itself win over the palette.
    pub(crate) fn color_of_index(&self, index: usize) -> iced::Color {
        if let Some(Rgb { r, g, b }) = self.term.colors()[index] {
            return iced::Color::from_rgb8(r, g, b);
        }
        match index {
            0..=255 => self.palette.indexed(index as u8),
            257 => self.palette.background,
            258 => self.palette.cursor,
            _ => self.palette.foreground,
        }
    }
}

fn target_of(kind: ClipboardType) -> ClipboardTarget {
    match kind {
        ClipboardType::Clipboard => ClipboardTarget::Clipboard,
        ClipboardType::Selection => ClipboardTarget::Primary,
    }
}

fn emulator_config(config: &Config) -> term::Config {
    term::Config {
        scrolling_history: config.scrollback,
        default_cursor_style: CursorStyle {
            shape: match config.cursor_shape {
                CursorShape::Block => EmulatorShape::Block,
                CursorShape::Beam => EmulatorShape::Beam,
                CursorShape::Underline => EmulatorShape::Underline,
            },
            blinking: config.cursor_blink,
        },
        vi_mode_cursor_style: None,
        semantic_escape_chars: config.word_separators.clone(),
        kitty_keyboard: config.kitty_keyboard,
        osc52: match config.osc52 {
            Osc52::Disabled => term::Osc52::Disabled,
            Osc52::CopyOnly => term::Osc52::OnlyCopy,
            Osc52::PasteOnly => term::Osc52::OnlyPaste,
            Osc52::CopyAndPaste => term::Osc52::CopyPaste,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terminal(columns: u16, rows: u16) -> Terminal {
        Terminal::new(Config::default(), columns, rows)
    }

    fn writes(events: &[Event]) -> Vec<u8> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::Write(bytes) => Some(bytes.clone()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    #[test]
    fn printed_text_lands_on_the_screen() {
        let mut term = terminal(20, 4);
        term.feed(b"hello\r\nworld");
        assert_eq!(term.screen_text(), ["hello", "world", "", ""]);
    }

    #[test]
    fn utf8_split_between_chunks_is_not_lost() {
        let mut term = terminal(20, 2);
        let bytes = "é日".as_bytes();
        term.feed(&bytes[..1]);
        term.feed(&bytes[1..3]);
        term.feed(&bytes[3..]);
        assert_eq!(term.screen_text()[0], "é日");
    }

    #[test]
    fn queries_are_answered_in_one_write() {
        let mut term = terminal(20, 4);
        // Move to row 2, column 5, then ask where the cursor is.
        let events = term.feed(b"\x1b[2;5H\x1b[6n");
        assert_eq!(writes(&events), b"\x1b[2;5R");
        // Primary device attributes.
        let events = term.feed(b"\x1b[c");
        assert_eq!(writes(&events), b"\x1b[?6c");
    }

    #[test]
    fn the_background_colour_query_is_answered_from_the_palette() {
        let mut term = terminal(20, 4);
        term.set_palette(Palette {
            background: iced::Color::from_rgb8(0x12, 0x34, 0x56),
            ..Palette::default()
        });
        let events = term.feed(b"\x1b]11;?\x07");
        assert_eq!(writes(&events), b"\x1b]11;rgb:1212/3434/5656\x07");
    }

    #[test]
    fn a_colour_the_program_set_wins_over_the_palette() {
        let mut term = terminal(20, 4);
        term.feed(b"\x1b]4;1;rgb:ff/00/00\x07");
        assert_eq!(term.color_of_index(1), iced::Color::from_rgb8(255, 0, 0));
        term.feed(b"\x1b]104;1\x07");
        assert_eq!(term.color_of_index(1), term.palette().ansi[1]);
    }

    #[test]
    fn titles_bells_and_clipboard_requests_are_events() {
        let mut term = terminal(20, 4);
        let events = term.feed(b"\x1b]2;my title\x07\x07");
        assert!(matches!(&events[0], Event::Title(t) if t == "my title"));
        assert!(matches!(events[1], Event::Bell));
        assert_eq!(term.title(), "my title");
        // Base64 of "copied".
        let events = term.feed(b"\x1b]52;c;Y29waWVk\x07");
        assert!(matches!(
            &events[0],
            Event::SetClipboard { target: ClipboardTarget::Clipboard, text } if text == "copied"
        ));
    }

    #[test]
    fn clipboard_reads_are_refused_unless_enabled() {
        let mut term = terminal(20, 4);
        let events = term.feed(b"\x1b]52;c;?\x07");
        assert!(events.is_empty());
        term.set_config(Config { osc52: Osc52::CopyAndPaste, ..Config::default() });
        let events = term.feed(b"\x1b]52;c;?\x07");
        match &events[0] {
            Event::ClipboardLoad { reply, .. } => {
                assert_eq!(reply.bytes("copied"), b"\x1b]52;c;Y29waWVk\x07");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn notifications_and_the_working_directory_come_from_the_scanner() {
        let mut term = terminal(20, 4);
        let events = term.feed(b"\x1b]9;done\x07\x1b]7;file://h/tmp/x\x07");
        assert!(matches!(&events[0], Event::Notification { title: None, body } if body == "done"));
        assert!(matches!(&events[1], Event::WorkingDirectory(p) if p == "/tmp/x"));
    }

    #[test]
    fn resizing_reports_the_new_size_once() {
        let mut term = terminal(80, 24);
        let events = term.resize(100, 30);
        assert!(matches!(
            events[0],
            Event::Resize(PtySize { rows: 30, columns: 100, .. })
        ));
        assert_eq!(term.size(), (100, 30));
        assert!(term.resize(100, 30).is_empty());
    }

    #[test]
    fn the_pixel_size_follows_the_cells() {
        let mut term = terminal(80, 24);
        let events = term.perform(Action::Resize(Metrics {
            columns: 100,
            rows: 30,
            cell_width: 7.5,
            cell_height: 17.0,
        }));
        match events[0] {
            Event::Resize(size) => {
                assert_eq!(size.pixel_width, 750);
                assert_eq!(size.pixel_height, 510);
            }
            _ => panic!("no resize"),
        }
    }

    #[test]
    fn shrinking_reflows_wrapped_lines_and_keeps_the_content() {
        let mut term = terminal(20, 4);
        term.feed(b"0123456789abcdefghij0123");
        term.resize(10, 6);
        assert_eq!(term.size(), (10, 6));
        // Some of it may have moved into the history; none of it is lost, and
        // it is still one logical line.
        term.select_all();
        assert_eq!(
            term.selection_text().map(|t| t.trim_end().to_owned()).as_deref(),
            Some("0123456789abcdefghij0123")
        );
    }

    #[test]
    fn scrollback_is_kept_and_typing_returns_to_the_bottom() {
        let mut term = terminal(20, 3);
        for i in 0..20 {
            term.feed(format!("row {i}\r\n").as_bytes());
        }
        assert!(term.history_size() >= 15);
        term.perform(Action::Scroll(ScrollTo::Lines(5)));
        assert_eq!(term.display_offset(), 5);
        let events = term.perform(Action::Input(b"x".to_vec()));
        assert_eq!(term.display_offset(), 0);
        assert_eq!(writes(&events), b"x");
        // Protocol reports leave the view where it is.
        term.perform(Action::Scroll(ScrollTo::Top));
        term.perform(Action::Report(b"\x1b[I".to_vec()));
        assert!(term.display_offset() > 0);
    }

    #[test]
    fn history_is_capped_by_the_configuration() {
        let mut term = Terminal::new(Config { scrollback: 5, ..Config::default() }, 20, 3);
        for i in 0..50 {
            term.feed(format!("row {i}\r\n").as_bytes());
        }
        assert_eq!(term.history_size(), 5);
    }

    #[test]
    fn bracketed_paste_wraps_and_cleans_the_text() {
        let mut term = terminal(20, 4);
        assert_eq!(term.paste_bytes("a\nb\r\nc"), b"a\rb\rc");
        term.feed(b"\x1b[?2004h");
        assert_eq!(
            term.paste_bytes("ls\x1b[201~; rm -rf ~"),
            b"\x1b[200~ls[201~; rm -rf ~\x1b[201~".to_vec()
        );
        let events = term.perform(Action::Paste("x".into()));
        assert_eq!(writes(&events), b"\x1b[200~x\x1b[201~");
    }

    #[test]
    fn focus_is_reported_only_when_the_program_asked() {
        let mut term = terminal(20, 4);
        assert!(term.set_focused(true).is_empty());
        term.set_focused(false);
        term.feed(b"\x1b[?1004h");
        assert_eq!(writes(&term.set_focused(true)), b"\x1b[I");
        assert_eq!(writes(&term.set_focused(false)), b"\x1b[O");
        // A window losing the focus counts only while the pane has it.
        term.set_focused(true);
        assert_eq!(writes(&term.perform(Action::WindowFocus(false))), b"\x1b[O");
        assert_eq!(writes(&term.perform(Action::WindowFocus(true))), b"\x1b[I");
    }

    #[test]
    fn synchronized_updates_hide_the_frame_until_it_ends() {
        let mut term = terminal(20, 3);
        term.feed(b"old");
        term.feed(b"\x1b[?2026h\x1b[2J\x1b[Hnew");
        assert_eq!(term.screen_text()[0], "old");
        assert!(term.deadline().is_some());
        term.feed(b"\x1b[?2026l");
        assert_eq!(term.screen_text()[0], "new");
        assert!(term.deadline().is_none());
    }

    #[test]
    fn a_synchronized_update_that_never_ends_is_shown_at_its_deadline() {
        let mut term = terminal(20, 3);
        term.feed(b"\x1b[?2026hstuck");
        assert_eq!(term.screen_text()[0], "");
        let deadline = term.deadline().expect("a deadline");
        term.tick(deadline + Duration::from_millis(1));
        assert_eq!(term.screen_text()[0], "stuck");
    }

    #[test]
    fn rows_are_marked_changed_only_when_they_were() {
        let mut term = terminal(20, 5);
        term.feed(b"one\r\ntwo\r\nthree");
        let before: Vec<u64> = (0..5).map(|r| term.row_generation(r)).collect();
        // Overwrite a single cell of the last row.
        term.feed(b"!");
        let after: Vec<u64> = (0..5).map(|r| term.row_generation(r)).collect();
        assert_eq!(&before[..2], &after[..2]);
        assert!(after[2] > before[2]);
    }

    #[test]
    fn the_cursor_is_where_the_program_left_it_and_hides_on_request() {
        let mut term = terminal(20, 4);
        term.feed(b"ab\r\nc");
        let cursor = term.cursor().unwrap();
        assert_eq!((cursor.row, cursor.column), (1, 1));
        term.feed(b"\x1b[?25l");
        assert!(term.cursor().is_none());
        term.feed(b"\x1b[?25h\x1b[5 q");
        assert_eq!(term.cursor().unwrap().shape, CursorShape::Beam);
        assert!(term.cursor_blinks());
    }

    #[test]
    fn an_unfocused_block_cursor_is_hollow() {
        let mut term = terminal(20, 4);
        assert!(term.cursor().unwrap().hollow);
        term.set_focused(true);
        assert!(!term.cursor().unwrap().hollow);
    }

    #[test]
    fn wide_characters_take_two_cells_and_the_cursor_follows() {
        let mut term = terminal(20, 2);
        term.feed("日本".as_bytes());
        assert_eq!(term.screen_text()[0], "日本");
        assert_eq!(term.cursor().unwrap().column, 4);
    }

    #[test]
    fn a_replay_parses_the_history_and_says_nothing_back() {
        let mut term = terminal(20, 4);
        // What a saved buffer holds: the program's start-up queries among its
        // output, a bell, a notification and a clipboard copy.
        let history = b"\x1b[c\x1b[6n\x1b]11;?\x07hello\x07\x1b]9;done\x07\x1b]52;c;Y29waWVk\x07\x1b]2;a title\x07\x1b]7;file://h/tmp/x\x07";
        let events = term.replay(history);
        assert_eq!(term.screen_text()[0], "hello");
        assert!(writes(&events).is_empty(), "{events:?}");
        assert!(!events.iter().any(|e| matches!(
            e,
            Event::Bell | Event::Notification { .. } | Event::SetClipboard { .. }
        )));
        assert!(term.bell_age(Instant::now()).is_none());
        // The state the replay ends in is reported.
        assert!(events.iter().any(|e| matches!(e, Event::Title(t) if t == "a title")));
        assert!(events.iter().any(|e| matches!(e, Event::WorkingDirectory(p) if p == "/tmp/x")));
        // And after it the terminal answers again.
        assert_eq!(writes(&term.feed(b"\x1b[c")), b"\x1b[?6c");
    }

    #[test]
    fn a_replay_that_ends_inside_a_sequence_carries_on_in_the_next_feed() {
        let mut term = terminal(20, 4);
        term.replay(b"abc\x1b[3");
        let events = term.feed(b"1mred");
        assert_eq!(term.screen_text()[0], "abcred");
        assert!(events.is_empty());
    }

    #[test]
    fn a_256_kib_history_replays_quickly_and_silently() {
        let mut term = terminal(100, 30);
        let line = "some output of an agent, with \x1b[1mcolour\x1b[0m and a query \x1b[6n\r\n";
        let history = line.repeat(256 * 1024 / line.len());
        let start = Instant::now();
        let events = term.replay(history.as_bytes());
        assert!(writes(&events).is_empty());
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(term.history_size() > 1000);
    }

    #[test]
    fn typing_is_announced_apart_from_the_answers_to_queries() {
        let mut term = terminal(20, 4);
        let answer = term.feed(b"\x1b[6n");
        assert!(!answer.iter().any(|e| matches!(e, Event::UserInput)));
        let typed = term.perform(Action::Input(b"a".to_vec()));
        assert!(matches!(typed[0], Event::UserInput));
        let pasted = term.perform(Action::Paste("a".into()));
        assert!(matches!(pasted[0], Event::UserInput));
        // Protocol reports are not typing.
        let report = term.perform(Action::Report(b"\x1b[I".to_vec()));
        assert!(!report.iter().any(|e| matches!(e, Event::UserInput)));
    }

    #[test]
    fn read_lines_returns_the_last_lines_oldest_first() {
        let mut term = terminal(20, 4);
        for i in 0..10 {
            term.feed(format!("line {i}\r\n").as_bytes());
        }
        assert_eq!(term.read_lines(3), ["line 7", "line 8", "line 9"]);
        // More than there is: all of it, history included.
        assert_eq!(term.read_lines(100).len(), 10);
        assert_eq!(term.read_lines(100)[0], "line 0");
        assert!(term.read_lines(0).is_empty());
    }

    #[test]
    fn read_lines_joins_wrapped_rows_and_drops_the_blank_ones_below() {
        let mut term = terminal(10, 6);
        term.feed(b"short\r\n0123456789abcdefghij012\r\nend");
        // Nothing below the last text: the screen's blank rows are not lines.
        assert_eq!(term.read_lines(10), ["short", "0123456789abcdefghij012", "end"]);
        assert_eq!(term.read_lines(2), ["0123456789abcdefghij012", "end"]);
        // Blank lines the program printed are kept in the middle.
        let mut term = terminal(10, 6);
        term.feed(b"a\r\n\r\nb\r\n\r\n");
        assert_eq!(term.read_lines(10), ["a", "", "b"]);
    }

    #[test]
    fn read_lines_of_an_empty_terminal_is_empty() {
        assert!(terminal(10, 4).read_lines(5).is_empty());
    }

    #[test]
    fn clear_keeps_the_cursor_line_and_drops_the_rest() {
        let mut term = terminal(20, 4);
        for i in 0..10 {
            term.feed(format!("row {i}\r\n").as_bytes());
        }
        term.feed(b"$ prompt");
        term.clear();
        assert_eq!(term.history_size(), 0);
        assert_eq!(term.screen_text(), ["$ prompt", "", "", ""]);
        let cursor = term.cursor().unwrap();
        assert_eq!((cursor.row, cursor.column), (0, 8));
        // Typing goes on from there.
        term.feed(b" ls");
        assert_eq!(term.screen_text()[0], "$ prompt ls");
    }

    #[test]
    fn clear_leaves_the_alternate_screen_alone() {
        let mut term = terminal(20, 4);
        term.feed(b"\x1b[?1049hfull screen");
        term.clear();
        assert_eq!(term.screen_text()[0], "full screen");
    }

    #[test]
    fn application_cursor_mode_is_visible_to_the_app() {
        let mut term = terminal(20, 4);
        assert!(!term.application_cursor());
        term.feed(b"\x1b[?1h");
        assert!(term.application_cursor());
    }

    #[test]
    fn emoji_and_symbols_have_the_width_unicode_gives_them() {
        // Unicode 11 and later: emoji are two cells, the symbols agent UIs
        // draw with are one. A wrong width shifts every column after it.
        let mut term = terminal(20, 2);
        term.feed("🚀✅❯●x".as_bytes());
        assert_eq!(term.screen_text()[0], "🚀✅❯●x");
        // 🚀 (2) + ✅ (2) + ❯ (1) + ● (1) + x (1) leaves the cursor at 7.
        assert_eq!(term.cursor().unwrap().column, 7);
    }

    #[test]
    fn many_terminals_can_be_fed_without_a_window() {
        // The panes of workspaces that are not on screen keep parsing.
        let mut terms: Vec<Terminal> = (0..16).map(|_| terminal(100, 30)).collect();
        for (i, term) in terms.iter_mut().enumerate() {
            term.feed(format!("pane {i}\r\n").as_bytes());
        }
        assert_eq!(terms[15].screen_text()[0], "pane 15");
    }

    #[test]
    fn the_alternate_screen_is_told_apart() {
        let mut term = terminal(20, 4);
        assert!(!term.is_alt_screen());
        term.feed(b"\x1b[?1049h");
        assert!(term.is_alt_screen());
    }
}
