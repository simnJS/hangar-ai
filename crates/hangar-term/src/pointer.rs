//! The pointer: selecting text, scrolling, and the reports a program that
//! asked for the mouse receives.
//!
//! One rule decides who gets a mouse event. When the program has turned a
//! mouse mode on, it gets them, and the terminal's own selection and wheel
//! scrolling step aside, except while Shift is held, which is the escape hatch
//! every terminal gives so that text can still be selected in a program that
//! owns the mouse.

use std::time::{Duration, Instant};

use alacritty_terminal::index::{Column, Point, Side as EmulatorSide};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::{TermMode, viewport_to_point};
use iced::keyboard::Modifiers;

use crate::event::{
    Button, CellPos, ClipboardTarget, Event, Pointer, ScrollTo, Side,
};
use crate::links;
use crate::mouse::{self, Code, Kind, Report};
use crate::terminal::Terminal;

/// The longest gap between clicks that still counts as a double click.
const MULTI_CLICK: Duration = Duration::from_millis(400);

/// The most lines a drag outside the widget scrolls per event, however far
/// out the pointer is.
const MAX_DRAG_SCROLL: i32 = 4;

/// The most wheel reports sent for one event: a fling on a touchpad can carry
/// hundreds of lines, and a program should not be flooded with them.
const MAX_WHEEL_REPORTS: usize = 12;

#[derive(Debug, Default)]
pub(crate) struct PointerState {
    /// The button held, if its press started something here.
    held: Option<Button>,
    /// Whether the press of the held button was sent to the program as a
    /// report, so its release is too whatever has changed since.
    reported: bool,
    /// A selection is being dragged.
    selecting: bool,
    last_click: Option<Click>,
    /// The cell of the last motion report, to send one per cell.
    last_cell: Option<(usize, usize)>,
    /// Wheel movement not yet worth a whole line.
    wheel_remainder: f32,
}

#[derive(Debug, Clone, Copy)]
struct Click {
    at: Instant,
    column: i32,
    row: i32,
    button: Button,
    count: u8,
}

impl Terminal {
    /// Whether the pointer goes to the program rather than to the terminal.
    pub fn mouse_reporting(&self, modifiers: Modifiers) -> bool {
        self.term.mode().intersects(TermMode::MOUSE_MODE) && !modifiers.shift()
    }

    /// The grid point under a position of the visible screen, clamped to it.
    fn grid_point(&self, at: CellPos) -> Point {
        let column = at.column.clamp(0, self.columns as i32 - 1) as usize;
        let row = at.row.clamp(0, self.rows as i32 - 1) as usize;
        viewport_to_point(self.term.grid().display_offset(), Point::new(row, Column(column)))
    }

    pub(crate) fn pointer(&mut self, pointer: Pointer, events: &mut Vec<Event>) {
        match pointer {
            Pointer::Down { button, at, modifiers } => self.pointer_down(button, at, modifiers, events),
            Pointer::Up { button, at, modifiers } => self.pointer_up(button, at, modifiers, events),
            Pointer::Move { at, modifiers } => self.pointer_move(at, modifiers, events),
            Pointer::Wheel { notches, at, modifiers } => self.wheel(notches, at, modifiers, events),
            Pointer::Left => self.pointer.last_cell = None,
        }
    }

    fn report(&self, code: Code, kind: Kind, at: CellPos, modifiers: Modifiers) -> Option<Event> {
        let column = at.column.clamp(0, self.columns as i32 - 1) as usize;
        let row = at.row.clamp(0, self.rows as i32 - 1) as usize;
        mouse::encode(*self.term.mode(), Report { code, kind, column, row, modifiers })
            .map(Event::Write)
    }

    fn pointer_down(&mut self, button: Button, at: CellPos, modifiers: Modifiers, events: &mut Vec<Event>) {
        events.push(Event::Pressed);

        if self.mouse_reporting(modifiers) {
            let code = match button {
                Button::Left => Code::Left,
                Button::Middle => Code::Middle,
                Button::Right => Code::Right,
            };
            events.extend(self.report(code, Kind::Press, at, modifiers));
            self.pointer.held = Some(button);
            self.pointer.reported = true;
            self.pointer.last_cell = Some((at.column.max(0) as usize, at.row.max(0) as usize));
            return;
        }

        if button != Button::Left {
            return;
        }

        // Ctrl, or Cmd on macOS, turns a click on a link into opening it. It
        // is checked before anything else so that following a link never
        // starts a selection.
        if modifiers.control() || modifiers.logo() {
            if let Some(hit) = links::link_at(&self.term, self.grid_point(at)) {
                events.push(Event::OpenLink(hit.link));
                return;
            }
        }

        let now = Instant::now();
        let count = match self.pointer.last_click {
            Some(last)
                if last.button == button
                    && last.column == at.column
                    && last.row == at.row
                    && now.saturating_duration_since(last.at) < MULTI_CLICK =>
            {
                last.count % 3 + 1
            }
            _ => 1,
        };
        self.pointer.last_click = Some(Click {
            at: now,
            column: at.column,
            row: at.row,
            button,
            count,
        });

        let point = self.grid_point(at);
        let side = emulator_side(at.side);
        let kind = match count {
            2 => SelectionType::Semantic,
            3 => SelectionType::Lines,
            _ if modifiers.alt() => SelectionType::Block,
            _ => SelectionType::Simple,
        };

        // Shift extends the selection there is, as it does in a text editor.
        match (&mut self.term.selection, modifiers.shift() && count == 1) {
            (Some(selection), true) => selection.update(point, side),
            _ => self.term.selection = Some(Selection::new(kind, point, side)),
        }
        self.pointer.held = Some(button);
        self.pointer.reported = false;
        self.pointer.selecting = true;
    }

    fn pointer_up(&mut self, button: Button, at: CellPos, modifiers: Modifiers, events: &mut Vec<Event>) {
        if self.pointer.held != Some(button) {
            return;
        }
        self.pointer.held = None;

        if self.pointer.reported {
            self.pointer.reported = false;
            let code = match button {
                Button::Left => Code::Left,
                Button::Middle => Code::Middle,
                Button::Right => Code::Right,
            };
            events.extend(self.report(code, Kind::Release, at, modifiers));
            return;
        }

        if !self.pointer.selecting {
            return;
        }
        self.pointer.selecting = false;

        // A click that went nowhere leaves no selection behind; one that went
        // somewhere is kept, and offered to the primary selection.
        let clicks = self.pointer.last_click.map_or(1, |c| c.count);
        let empty = self
            .term
            .selection
            .as_ref()
            .is_some_and(|s| s.is_empty() && s.ty == SelectionType::Simple);
        if empty && clicks == 1 {
            self.term.selection = None;
            return;
        }
        if self.config.copy_on_select {
            if let Some(text) = self.selection_text() {
                events.push(Event::SetClipboard {
                    target: ClipboardTarget::Primary,
                    text,
                });
            }
        }
    }

    fn pointer_move(&mut self, at: CellPos, modifiers: Modifiers, events: &mut Vec<Event>) {
        if self.pointer.selecting {
            // Dragging past the edge scrolls the history that way, a little
            // faster the further out the pointer is.
            let lines = if at.row < 0 {
                (-at.row).min(MAX_DRAG_SCROLL)
            } else if at.row >= self.rows as i32 {
                -((at.row - self.rows as i32 + 1).min(MAX_DRAG_SCROLL))
            } else {
                0
            };
            if lines != 0 {
                self.scroll(ScrollTo::Lines(lines));
            }
            let point = self.grid_point(at);
            // Past the last row the selection runs to the end of it; past the
            // first, back to its start.
            let side = if at.row < 0 {
                EmulatorSide::Left
            } else if at.row >= self.rows as i32 {
                EmulatorSide::Right
            } else {
                emulator_side(at.side)
            };
            if let Some(selection) = &mut self.term.selection {
                selection.update(point, side);
            }
            return;
        }

        let mode = *self.term.mode();
        if !self.mouse_reporting(modifiers) {
            return;
        }
        let wants_motion = mode.contains(TermMode::MOUSE_MOTION)
            || (mode.contains(TermMode::MOUSE_DRAG) && self.pointer.held.is_some());
        if !wants_motion {
            return;
        }
        let column = at.column.clamp(0, self.columns as i32 - 1) as usize;
        let row = at.row.clamp(0, self.rows as i32 - 1) as usize;
        // One report per cell, not one per pixel.
        if self.pointer.last_cell == Some((column, row)) {
            return;
        }
        self.pointer.last_cell = Some((column, row));
        let code = match self.pointer.held {
            Some(Button::Left) => Code::Left,
            Some(Button::Middle) => Code::Middle,
            Some(Button::Right) => Code::Right,
            None => Code::None,
        };
        events.extend(self.report(code, Kind::Motion, at, modifiers));
    }

    fn wheel(&mut self, notches: f32, at: CellPos, modifiers: Modifiers, events: &mut Vec<Event>) {
        let mode = *self.term.mode();
        let alt_screen = mode.contains(TermMode::ALT_SCREEN);

        if self.mouse_reporting(modifiers) {
            self.pointer.wheel_remainder += notches;
            let whole = self.pointer.wheel_remainder.trunc();
            self.pointer.wheel_remainder -= whole;
            let code = if whole > 0.0 { Code::WheelUp } else { Code::WheelDown };
            for _ in 0..(whole.abs() as usize).min(MAX_WHEEL_REPORTS) {
                events.extend(self.report(code, Kind::Press, at, modifiers));
            }
            return;
        }

        let lines_per_notch = self.config.scroll_multiplier.max(0.0);
        self.pointer.wheel_remainder += notches * lines_per_notch;
        let whole = self.pointer.wheel_remainder.trunc();
        self.pointer.wheel_remainder -= whole;
        let lines = whole as i32;
        if lines == 0 {
            return;
        }

        if alt_screen {
            // A full-screen program that did not ask for the mouse still
            // wants the wheel to do something: it becomes the arrow keys, as
            // in xterm's alternate scroll mode.
            if mode.contains(TermMode::ALTERNATE_SCROLL) {
                let arrow = match (lines > 0, mode.contains(TermMode::APP_CURSOR)) {
                    (true, true) => b"\x1bOA".as_slice(),
                    (true, false) => b"\x1b[A",
                    (false, true) => b"\x1bOB",
                    (false, false) => b"\x1b[B",
                };
                let mut out = Vec::new();
                for _ in 0..(lines.unsigned_abs() as usize).min(MAX_WHEEL_REPORTS * 3) {
                    out.extend_from_slice(arrow);
                }
                events.push(Event::Write(out));
            }
            return;
        }
        self.scroll(ScrollTo::Lines(lines));
    }
}

fn emulator_side(side: Side) -> EmulatorSide {
    match side {
        Side::Left => EmulatorSide::Left,
        Side::Right => EmulatorSide::Right,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::event::Action;

    const NONE: Modifiers = Modifiers::empty();

    fn terminal(text: &str, columns: u16, rows: u16) -> Terminal {
        let mut term = Terminal::new(Config::default(), columns, rows);
        term.feed(text.as_bytes());
        term
    }

    fn at(column: i32, row: i32, side: Side) -> CellPos {
        CellPos { column, row, side }
    }

    fn down(term: &mut Terminal, button: Button, pos: CellPos, modifiers: Modifiers) -> Vec<Event> {
        term.perform(Action::Pointer(Pointer::Down { button, at: pos, modifiers }))
    }

    fn up(term: &mut Terminal, button: Button, pos: CellPos) -> Vec<Event> {
        term.perform(Action::Pointer(Pointer::Up { button, at: pos, modifiers: NONE }))
    }

    fn drag(term: &mut Terminal, pos: CellPos) -> Vec<Event> {
        term.perform(Action::Pointer(Pointer::Move { at: pos, modifiers: NONE }))
    }

    fn select(term: &mut Terminal, from: CellPos, to: CellPos) {
        down(term, Button::Left, from, NONE);
        drag(term, to);
        up(term, Button::Left, to);
    }

    fn writes(events: &[Event]) -> Vec<u8> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::Write(b) => Some(b.clone()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    #[test]
    fn dragging_selects_the_characters_between_the_halves_pointed_at() {
        let mut term = terminal("hello world\r\n", 20, 4);
        // From the left half of 'h' to the right half of 'o'.
        select(&mut term, at(0, 0, Side::Left), at(4, 0, Side::Right));
        assert_eq!(term.selection_text().as_deref(), Some("hello"));
        // From the right half of 'h' leaves the 'h' out. A second terminal,
        // so that the press does not count as a double click on the first.
        let mut term = terminal("hello world\r\n", 20, 4);
        select(&mut term, at(0, 0, Side::Right), at(4, 0, Side::Right));
        assert_eq!(term.selection_text().as_deref(), Some("ello"));
    }

    #[test]
    fn a_plain_click_selects_nothing_and_clears_the_selection() {
        let mut term = terminal("hello world\r\n", 20, 4);
        select(&mut term, at(0, 0, Side::Left), at(4, 0, Side::Right));
        assert!(term.has_selection());
        down(&mut term, Button::Left, at(8, 0, Side::Left), NONE);
        up(&mut term, Button::Left, at(8, 0, Side::Left));
        assert!(!term.has_selection());
    }

    #[test]
    fn double_click_selects_a_word_and_triple_click_the_line() {
        let mut term = terminal("hello brave world\r\nnext line\r\n", 30, 4);
        let word = at(8, 0, Side::Left);
        down(&mut term, Button::Left, word, NONE);
        up(&mut term, Button::Left, word);
        down(&mut term, Button::Left, word, NONE);
        up(&mut term, Button::Left, word);
        assert_eq!(term.selection_text().as_deref(), Some("brave"));
        down(&mut term, Button::Left, word, NONE);
        up(&mut term, Button::Left, word);
        assert_eq!(term.selection_text().as_deref(), Some("hello brave world\n"));
    }

    #[test]
    fn shift_extends_the_selection() {
        let mut term = terminal("hello world\r\n", 20, 4);
        select(&mut term, at(0, 0, Side::Left), at(4, 0, Side::Right));
        down(&mut term, Button::Left, at(10, 0, Side::Right), Modifiers::SHIFT);
        up(&mut term, Button::Left, at(10, 0, Side::Right));
        assert_eq!(term.selection_text().as_deref(), Some("hello world"));
    }

    #[test]
    fn alt_drag_selects_a_block() {
        let mut term = terminal("abcd\r\nefgh\r\nijkl\r\n", 10, 5);
        down(&mut term, Button::Left, at(1, 0, Side::Left), Modifiers::ALT);
        drag(&mut term, at(2, 2, Side::Right));
        up(&mut term, Button::Left, at(2, 2, Side::Right));
        assert_eq!(term.selection_text().as_deref(), Some("bc\nfg\njk"));
    }

    #[test]
    fn a_selection_survives_scrolling_with_the_text() {
        let mut term = terminal("", 20, 3);
        term.feed(b"target\r\n");
        select(&mut term, at(0, 0, Side::Left), at(5, 0, Side::Right));
        term.feed(b"a\r\nb\r\nc\r\n");
        assert_eq!(term.selection_text().as_deref(), Some("target"));
    }

    #[test]
    fn selecting_all_takes_the_history_too() {
        let mut term = terminal("", 10, 2);
        for i in 0..5 {
            term.feed(format!("l{i}\r\n").as_bytes());
        }
        term.perform(Action::SelectAll);
        let text = term.selection_text().unwrap();
        assert!(text.starts_with("l0\n"), "{text:?}");
        assert!(text.contains("l4"));
    }

    #[test]
    fn copy_puts_the_selection_on_the_clipboard() {
        let mut term = terminal("hello\r\n", 20, 3);
        select(&mut term, at(0, 0, Side::Left), at(4, 0, Side::Right));
        let events = term.perform(Action::Copy);
        assert!(matches!(
            &events[0],
            Event::SetClipboard { target: ClipboardTarget::Clipboard, text } if text == "hello"
        ));
        // Nothing selected, nothing copied.
        term.perform(Action::ClearSelection);
        assert!(term.perform(Action::Copy).is_empty());
    }

    #[test]
    fn copy_on_select_offers_the_primary_selection() {
        let mut term = Terminal::new(Config { copy_on_select: true, ..Config::default() }, 20, 3);
        term.feed(b"hello\r\n");
        down(&mut term, Button::Left, at(0, 0, Side::Left), NONE);
        drag(&mut term, at(4, 0, Side::Right));
        let events = up(&mut term, Button::Left, at(4, 0, Side::Right));
        assert!(matches!(
            &events[0],
            Event::SetClipboard { target: ClipboardTarget::Primary, text } if text == "hello"
        ));
    }

    #[test]
    fn pressing_asks_for_the_focus() {
        let mut term = terminal("", 20, 3);
        let events = down(&mut term, Button::Left, at(0, 0, Side::Left), NONE);
        assert!(matches!(events[0], Event::Pressed));
    }

    #[test]
    fn dragging_above_the_screen_scrolls_the_history_and_extends_the_selection() {
        let mut term = terminal("", 20, 3);
        for i in 0..20 {
            term.feed(format!("row {i}\r\n").as_bytes());
        }
        down(&mut term, Button::Left, at(0, 1, Side::Left), NONE);
        drag(&mut term, at(0, -2, Side::Left));
        assert!(term.display_offset() > 0);
        let text = term.selection_text().unwrap();
        assert!(text.lines().count() >= 3, "{text:?}");
    }

    #[test]
    fn ctrl_click_on_a_url_opens_it_and_does_not_select() {
        let mut term = terminal("see https://hangar.ai/docs now\r\n", 40, 3);
        let events = down(&mut term, Button::Left, at(10, 0, Side::Left), Modifiers::CTRL);
        let link = events.iter().find_map(|e| match e {
            Event::OpenLink(link) => Some(link.clone()),
            _ => None,
        });
        assert_eq!(link.map(|l| l.uri).as_deref(), Some("https://hangar.ai/docs"));
        assert!(!term.has_selection());
        // Without Ctrl it is an ordinary click.
        let events = down(&mut term, Button::Left, at(10, 0, Side::Left), NONE);
        assert!(!events.iter().any(|e| matches!(e, Event::OpenLink(_))));
    }

    #[test]
    fn osc_8_hyperlinks_carry_their_own_target() {
        let mut term = terminal("\x1b]8;;https://hangar.ai/real\x1b\\click me\x1b]8;;\x1b\\ plain\r\n", 40, 3);
        let events = down(&mut term, Button::Left, at(2, 0, Side::Left), Modifiers::CTRL);
        let link = events.iter().find_map(|e| match e {
            Event::OpenLink(link) => Some(link.clone()),
            _ => None,
        });
        let link = link.expect("a link");
        assert_eq!(link.uri, "https://hangar.ai/real");
        assert_eq!(link.source, crate::event::LinkSource::Hyperlink);
        // The text after the link is not part of it.
        let events = down(&mut term, Button::Left, at(12, 0, Side::Left), Modifiers::CTRL);
        assert!(!events.iter().any(|e| matches!(e, Event::OpenLink(_))));
    }

    #[test]
    fn urls_that_wrap_over_rows_are_one_link() {
        let mut term = terminal("https://hangar.ai/a/long/path/to/somewhere\r\n", 20, 4);
        let events = down(&mut term, Button::Left, at(3, 1, Side::Left), Modifiers::CTRL);
        let uri = events.iter().find_map(|e| match e {
            Event::OpenLink(link) => Some(link.uri.clone()),
            _ => None,
        });
        assert_eq!(uri.as_deref(), Some("https://hangar.ai/a/long/path/to/somewhere"));
    }

    // ---- reporting ----

    #[test]
    fn a_program_that_enabled_the_mouse_gets_the_clicks() {
        let mut term = terminal("\x1b[?1000h\x1b[?1006h", 20, 5);
        let events = down(&mut term, Button::Left, at(3, 2, Side::Left), NONE);
        assert_eq!(writes(&events), b"\x1b[<0;4;3M");
        let events = up(&mut term, Button::Left, at(3, 2, Side::Left));
        assert_eq!(writes(&events), b"\x1b[<0;4;3m");
        // And the terminal selects nothing meanwhile.
        assert!(!term.has_selection());
    }

    #[test]
    fn shift_takes_the_mouse_back_for_selecting() {
        let mut term = terminal("hello\r\n\x1b[?1000h\x1b[?1006h", 20, 5);
        let events = down(&mut term, Button::Left, at(0, 0, Side::Left), Modifiers::SHIFT);
        assert!(writes(&events).is_empty());
        drag(&mut term, at(4, 0, Side::Right));
        up(&mut term, Button::Left, at(4, 0, Side::Right));
        assert_eq!(term.selection_text().as_deref(), Some("hello"));
    }

    #[test]
    fn drag_mode_reports_motion_with_a_button_down_once_per_cell() {
        let mut term = terminal("\x1b[?1002h\x1b[?1006h", 20, 5);
        // Motion with no button is not asked for in this mode.
        assert!(writes(&drag(&mut term, at(1, 1, Side::Left))).is_empty());
        down(&mut term, Button::Left, at(1, 1, Side::Left), NONE);
        assert_eq!(writes(&drag(&mut term, at(2, 1, Side::Left))), b"\x1b[<32;3;2M");
        // Still in the same cell: nothing.
        assert!(writes(&drag(&mut term, at(2, 1, Side::Right))).is_empty());
    }

    #[test]
    fn any_motion_mode_reports_the_pointer_without_a_button() {
        let mut term = terminal("\x1b[?1003h\x1b[?1006h", 20, 5);
        assert_eq!(writes(&drag(&mut term, at(4, 3, Side::Left))), b"\x1b[<35;5;4M");
    }

    #[test]
    fn the_wheel_is_reported_when_the_mouse_is_on() {
        let mut term = terminal("\x1b[?1000h\x1b[?1006h", 20, 5);
        let events = term.perform(Action::Pointer(Pointer::Wheel {
            notches: 2.0,
            at: at(0, 0, Side::Left),
            modifiers: NONE,
        }));
        assert_eq!(writes(&events), b"\x1b[<64;1;1M\x1b[<64;1;1M");
        let events = term.perform(Action::Pointer(Pointer::Wheel {
            notches: -1.0,
            at: at(0, 0, Side::Left),
            modifiers: NONE,
        }));
        assert_eq!(writes(&events), b"\x1b[<65;1;1M");
    }

    #[test]
    fn the_wheel_scrolls_the_history_otherwise_three_lines_a_notch() {
        let mut term = terminal("", 20, 3);
        for i in 0..30 {
            term.feed(format!("row {i}\r\n").as_bytes());
        }
        term.perform(Action::Pointer(Pointer::Wheel {
            notches: 1.0,
            at: at(0, 0, Side::Left),
            modifiers: NONE,
        }));
        assert_eq!(term.display_offset(), 3);
        term.perform(Action::Pointer(Pointer::Wheel {
            notches: -1.0,
            at: at(0, 0, Side::Left),
            modifiers: NONE,
        }));
        assert_eq!(term.display_offset(), 0);
    }

    #[test]
    fn small_touchpad_movements_add_up() {
        let mut term = terminal("", 20, 3);
        for i in 0..30 {
            term.feed(format!("row {i}\r\n").as_bytes());
        }
        let wheel = |term: &mut Terminal, notches| {
            term.perform(Action::Pointer(Pointer::Wheel {
                notches,
                at: at(0, 0, Side::Left),
                modifiers: NONE,
            }));
        };
        wheel(&mut term, 0.2);
        wheel(&mut term, 0.2);
        assert_eq!(term.display_offset(), 1);
        wheel(&mut term, 0.2);
        wheel(&mut term, 0.2);
        wheel(&mut term, 0.2);
        assert_eq!(term.display_offset(), 3);
    }

    #[test]
    fn on_the_alternate_screen_the_wheel_becomes_arrow_keys() {
        let mut term = terminal("\x1b[?1049h", 20, 5);
        let events = term.perform(Action::Pointer(Pointer::Wheel {
            notches: 1.0,
            at: at(0, 0, Side::Left),
            modifiers: NONE,
        }));
        assert_eq!(writes(&events), b"\x1b[A\x1b[A\x1b[A");
        term.feed(b"\x1b[?1h");
        let events = term.perform(Action::Pointer(Pointer::Wheel {
            notches: -1.0,
            at: at(0, 0, Side::Left),
            modifiers: NONE,
        }));
        assert_eq!(writes(&events), b"\x1bOB\x1bOB\x1bOB");
    }
}
