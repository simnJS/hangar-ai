//! From cells to things to draw: the plan of one row.
//!
//! Drawing a terminal cell by cell would mean twelve thousand draw calls for a
//! 200x60 screen. A row is instead reduced to a handful of items: background
//! spans, where neighbouring cells of one colour are one rectangle; text runs,
//! where neighbouring cells of one style are one string; decoration spans for
//! underlines and strikethrough; and the few characters drawn as shapes.
//!
//! The plan of a row depends only on that row's cells and on the palette, so
//! it is kept between frames and rebuilt when the terminal says the row
//! changed. Overlays that come and go without touching the cells (selection,
//! search matches, the cursor, link underlines) are not part of it.

use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor};
use iced::Color;

use crate::palette::mix;
use crate::shapes;
use crate::terminal::Terminal;

/// A rectangle of one colour over whole cells: `first` to `end` (exclusive).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Background {
    pub first: usize,
    pub end: usize,
    pub color: Color,
}

/// How a run of text is set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Style {
    pub color: Color,
    pub bold: bool,
    pub italic: bool,
}

/// Text on neighbouring cells in one style.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Run {
    /// The first cell.
    pub column: usize,
    /// How many cells it covers, spaces inside included.
    pub columns: usize,
    pub text: String,
    pub style: Style,
    /// Only characters that every monospace font has, at one cell each. Such
    /// a run can be laid out by the font, since the font's advance is the
    /// cell's width. Other characters are placed one by one in the cells they
    /// belong to, because a fallback font's advance is not.
    pub flowing: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Decoration {
    Underline,
    Double,
    Curly,
    Dotted,
    Dashed,
    Strike,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DecorationSpan {
    pub kind: Decoration,
    pub first: usize,
    pub end: usize,
    pub color: Color,
}

/// A character drawn as a shape (box drawing, blocks, braille).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ShapeCell {
    pub column: usize,
    pub glyph: char,
    pub color: Color,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct RowPlan {
    pub backgrounds: Vec<Background>,
    pub runs: Vec<Run>,
    pub decorations: Vec<DecorationSpan>,
    pub shapes: Vec<ShapeCell>,
}

/// Whether a character is laid out inside a run: ASCII and the Latin, Greek
/// and Cyrillic blocks, which monospace fonts carry at the width of a cell.
fn flows(c: char) -> bool {
    matches!(c as u32, 0x20..=0x7e | 0xa0..=0x24f | 0x370..=0x52f)
}

/// Colours of a cell, after inverse, dim and hidden are applied.
struct Resolved {
    fg: Color,
    bg: Color,
}

fn resolve(terminal: &Terminal, cell: &Cell) -> Resolved {
    let bold_is_bright = terminal.config.bold_is_bright;
    let flags = cell.flags;
    let bold = flags.contains(Flags::BOLD);
    let mut fg = resolve_color(terminal, cell.fg, true, bold && bold_is_bright);
    let mut bg = resolve_color(terminal, cell.bg, false, false);
    if flags.contains(Flags::INVERSE) {
        std::mem::swap(&mut fg, &mut bg);
    }
    if flags.contains(Flags::DIM) {
        fg = mix(fg, terminal.color_of_index(NamedColor::Background as usize), 0.34);
    }
    if flags.contains(Flags::HIDDEN) {
        fg = bg;
    }
    Resolved { fg, bg }
}

pub(crate) fn resolve_color(terminal: &Terminal, color: AnsiColor, foreground: bool, brighten: bool) -> Color {
    match color {
        AnsiColor::Spec(rgb) => Color::from_rgb8(rgb.r, rgb.g, rgb.b),
        AnsiColor::Indexed(index) => {
            let index = if brighten && index < 8 { index + 8 } else { index };
            terminal.color_of_index(index as usize)
        }
        AnsiColor::Named(named) => {
            let n = named as usize;
            match n {
                0..=7 => terminal.color_of_index(if brighten { n + 8 } else { n }),
                8..=15 | 256..=258 => terminal.color_of_index(n),
                // The dim colours are the plain ones, dimmed; nothing in the
                // emulator picks them except through the attribute, which is
                // handled with the flags.
                259..=266 => terminal.color_of_index(n - 259),
                267 => terminal.color_of_index(NamedColor::Foreground as usize),
                _ => {
                    if foreground {
                        terminal.color_of_index(NamedColor::Foreground as usize)
                    } else {
                        terminal.color_of_index(NamedColor::Background as usize)
                    }
                }
            }
        }
    }
}

/// An open run of text, being extended cell by cell.
struct OpenRun {
    start: usize,
    /// The column the next cell must be in to continue the run.
    next: usize,
    text: String,
    pending_spaces: usize,
    style: Style,
}

/// Plans row `row` of the visible screen.
pub(crate) fn plan_row(terminal: &Terminal, row: usize) -> RowPlan {
    let grid = terminal.term.grid();
    let offset = grid.display_offset() as i32;
    let line = &grid[Line(row as i32 - offset)];
    let default_bg = terminal.color_of_index(NamedColor::Background as usize);

    let mut plan = RowPlan::default();
    let mut open: Option<OpenRun> = None;
    let mut background: Option<Background> = None;
    let mut decoration: Option<DecorationSpan> = None;

    let flush = |open: &mut Option<OpenRun>, plan: &mut RowPlan| {
        if let Some(run) = open.take()
            && !run.text.is_empty() {
                plan.runs.push(Run {
                    column: run.start,
                    columns: run.start_columns(),
                    text: run.text,
                    style: run.style,
                    flowing: true,
                });
            }
    };

    for column in 0..terminal.columns {
        let cell = &line[Column(column)];
        let colors = resolve(terminal, cell);

        // Backgrounds: neighbouring cells of one colour are one rectangle.
        if colors.bg != default_bg {
            match &mut background {
                Some(span) if span.color == colors.bg && span.end == column => span.end += 1,
                _ => {
                    if let Some(span) = background.take() {
                        plan.backgrounds.push(span);
                    }
                    background = Some(Background { first: column, end: column + 1, color: colors.bg });
                }
            }
        } else if let Some(span) = background.take() {
            plan.backgrounds.push(span);
        }

        // Decorations: underline styles and strikethrough, per colour.
        let kind = decoration_of(cell.flags);
        let deco_color = cell
            .underline_color()
            .map(|c| resolve_color(terminal, c, true, false))
            .unwrap_or(colors.fg);
        match (kind, &mut decoration) {
            (Some(kind), Some(span)) if span.kind == kind && span.color == deco_color && span.end == column => {
                span.end += 1;
            }
            (kind, _) => {
                if let Some(span) = decoration.take() {
                    plan.decorations.push(span);
                }
                if let Some(kind) = kind {
                    decoration = Some(DecorationSpan { kind, first: column, end: column + 1, color: deco_color });
                }
            }
        }
        // Strikethrough can accompany an underline; it is a span of its own.
        if cell.flags.contains(Flags::STRIKEOUT) && kind != Some(Decoration::Strike) {
            match plan.decorations.last_mut() {
                Some(span) if span.kind == Decoration::Strike && span.end == column && span.color == colors.fg => {
                    span.end += 1;
                }
                _ => plan.decorations.push(DecorationSpan {
                    kind: Decoration::Strike,
                    first: column,
                    end: column + 1,
                    color: colors.fg,
                }),
            }
        }

        // The second half of a wide character, or the filler before one that
        // wrapped, has no character of its own.
        if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
            continue;
        }
        let c = cell.c;
        let zerowidth = cell.zerowidth().filter(|z| !z.is_empty());
        let blank = (c == ' ' || c == '\0' || c == '\t') && zerowidth.is_none();
        if blank || cell.flags.contains(Flags::HIDDEN) {
            if let Some(run) = &mut open {
                // Spaces keep a run together across a short gap; a long one
                // ends it, so a row with far-apart words is not one wide
                // string mostly made of padding.
                run.pending_spaces += 1;
                if run.pending_spaces >= 3 {
                    flush(&mut open, &mut plan);
                }
            }
            continue;
        }

        if shapes::is_shape(c) {
            plan.shapes.push(ShapeCell { column, glyph: c, color: colors.fg });
            // The shape stands where a space would, so it must not break the
            // run it sits in.
            if let Some(run) = &mut open {
                run.pending_spaces += 1;
                if run.pending_spaces >= 3 {
                    flush(&mut open, &mut plan);
                }
            }
            continue;
        }

        let style = Style {
            color: colors.fg,
            bold: cell.flags.intersects(Flags::BOLD),
            italic: cell.flags.contains(Flags::ITALIC),
        };
        let wide = cell.flags.contains(Flags::WIDE_CHAR);
        if wide || zerowidth.is_some() || !flows(c) {
            flush(&mut open, &mut plan);
            let mut text = String::from(c);
            if let Some(extra) = zerowidth {
                text.extend(extra.iter());
            }
            plan.runs.push(Run {
                column,
                columns: if wide { 2 } else { 1 },
                text,
                style,
                flowing: false,
            });
            continue;
        }

        match &mut open {
            Some(run) if run.style == style && run.next + run.pending_spaces == column => {
                for _ in 0..run.pending_spaces {
                    run.text.push(' ');
                }
                run.pending_spaces = 0;
                run.text.push(c);
                run.next = column + 1;
            }
            _ => {
                flush(&mut open, &mut plan);
                open = Some(OpenRun {
                    start: column,
                    next: column + 1,
                    text: String::from(c),
                    pending_spaces: 0,
                    style,
                });
            }
        }
    }
    flush(&mut open, &mut plan);
    plan.backgrounds.extend(background);
    plan.decorations.extend(decoration);
    plan
}

impl OpenRun {
    fn start_columns(&self) -> usize {
        self.next - self.start
    }
}

fn decoration_of(flags: Flags) -> Option<Decoration> {
    if flags.contains(Flags::UNDERCURL) {
        Some(Decoration::Curly)
    } else if flags.contains(Flags::DOUBLE_UNDERLINE) {
        Some(Decoration::Double)
    } else if flags.contains(Flags::DOTTED_UNDERLINE) {
        Some(Decoration::Dotted)
    } else if flags.contains(Flags::DASHED_UNDERLINE) {
        Some(Decoration::Dashed)
    } else if flags.contains(Flags::UNDERLINE) {
        Some(Decoration::Underline)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::palette::Palette;

    fn terminal(text: &str, columns: u16, rows: u16) -> Terminal {
        let mut term = Terminal::new(Config::default(), columns, rows);
        term.feed(text.as_bytes());
        term
    }

    fn runs(plan: &RowPlan) -> Vec<(usize, &str)> {
        plan.runs.iter().map(|r| (r.column, r.text.as_str())).collect()
    }

    #[test]
    fn plain_text_is_one_run() {
        let plan = plan_row(&terminal("hello world", 40, 3), 0);
        assert_eq!(runs(&plan), [(0, "hello world")]);
        assert_eq!(plan.runs[0].columns, 11);
        assert!(plan.backgrounds.is_empty());
    }

    #[test]
    fn blank_rows_have_nothing_to_draw() {
        let plan = plan_row(&terminal("x", 40, 3), 2);
        assert_eq!(plan, RowPlan::default());
    }

    #[test]
    fn a_change_of_colour_or_weight_starts_a_new_run() {
        let plan = plan_row(&terminal("ab\x1b[31mcd\x1b[0m \x1b[1mef", 40, 3), 0);
        assert_eq!(runs(&plan), [(0, "ab"), (2, "cd"), (5, "ef")]);
        assert_eq!(plan.runs[1].style.color, Palette::default().ansi[1]);
        assert!(plan.runs[2].style.bold);
    }

    #[test]
    fn a_single_space_does_not_split_a_run_but_a_wide_gap_does() {
        let plan = plan_row(&terminal("a b    c", 40, 3), 0);
        assert_eq!(runs(&plan), [(0, "a b"), (7, "c")]);
        assert_eq!(plan.runs[0].columns, 3);
    }

    #[test]
    fn background_spans_merge_neighbouring_cells() {
        let plan = plan_row(&terminal("\x1b[44mab\x1b[42mcd\x1b[0m e\x1b[44m \x1b[44m ", 40, 3), 0);
        let spans: Vec<_> = plan.backgrounds.iter().map(|b| (b.first, b.end)).collect();
        assert_eq!(spans, [(0, 2), (2, 4), (6, 8)]);
    }

    #[test]
    fn erasing_with_a_background_colour_paints_the_row() {
        // What a TUI does to paint a bar: set a colour and erase the line.
        let plan = plan_row(&terminal("\x1b[44m\x1b[2K", 20, 3), 0);
        assert_eq!(plan.backgrounds.len(), 1);
        assert_eq!((plan.backgrounds[0].first, plan.backgrounds[0].end), (0, 20));
    }

    #[test]
    fn inverse_swaps_the_colours() {
        let term = terminal("\x1b[7mab", 20, 3);
        let plan = plan_row(&term, 0);
        let palette = Palette::default();
        assert_eq!(plan.runs[0].style.color, palette.background);
        assert_eq!(plan.backgrounds[0].color, palette.foreground);
    }

    #[test]
    fn bold_brightens_the_dark_ansi_colours_unless_told_not_to() {
        let term = terminal("\x1b[1;31mx", 20, 3);
        assert_eq!(plan_row(&term, 0).runs[0].style.color, Palette::default().ansi[9]);
        let mut term = Terminal::new(Config { bold_is_bright: false, ..Config::default() }, 20, 3);
        term.feed(b"\x1b[1;31mx");
        assert_eq!(plan_row(&term, 0).runs[0].style.color, Palette::default().ansi[1]);
    }

    #[test]
    fn truecolour_and_the_256_palette_pass_through() {
        let term = terminal("\x1b[38;2;10;20;30mt\x1b[38;5;196mi", 20, 3);
        let plan = plan_row(&term, 0);
        assert_eq!(plan.runs[0].style.color, Color::from_rgb8(10, 20, 30));
        assert_eq!(plan.runs[1].style.color, Color::from_rgb8(255, 0, 0));
    }

    #[test]
    fn dim_text_is_pulled_towards_the_background() {
        let term = terminal("\x1b[2mdim", 20, 3);
        let plan = plan_row(&term, 0);
        let palette = Palette::default();
        let c = plan.runs[0].style.color;
        assert!(c.r < palette.foreground.r && c.r > palette.background.r);
    }

    #[test]
    fn hidden_text_is_not_drawn() {
        let plan = plan_row(&terminal("a\x1b[8msecret\x1b[0mb", 20, 3), 0);
        assert_eq!(runs(&plan).iter().map(|r| r.1).collect::<String>(), "ab");
    }

    #[test]
    fn wide_characters_and_symbols_are_placed_one_by_one() {
        let plan = plan_row(&terminal("a日b❯c", 20, 3), 0);
        assert_eq!(runs(&plan), [(0, "a"), (1, "日"), (3, "b"), (4, "❯"), (5, "c")]);
        assert!(!plan.runs[1].flowing);
        assert_eq!(plan.runs[1].columns, 2);
        assert!(plan.runs[0].flowing);
    }

    #[test]
    fn accented_latin_stays_in_the_run() {
        let plan = plan_row(&terminal("café au lait", 20, 3), 0);
        assert_eq!(runs(&plan), [(0, "café au lait")]);
    }

    #[test]
    fn combining_marks_travel_with_their_base() {
        let plan = plan_row(&terminal("e\u{301}x", 20, 3), 0);
        assert_eq!(runs(&plan), [(0, "e\u{301}"), (1, "x")]);
    }

    #[test]
    fn box_drawing_becomes_shapes_without_breaking_the_text() {
        let plan = plan_row(&terminal("│ hello │", 20, 3), 0);
        let cells: Vec<_> = plan.shapes.iter().map(|s| (s.column, s.glyph)).collect();
        assert_eq!(cells, [(0, '│'), (8, '│')]);
        assert_eq!(runs(&plan), [(2, "hello")]);
    }

    #[test]
    fn underline_strikethrough_and_curly_are_spans() {
        let plan = plan_row(&terminal("\x1b[4mab\x1b[0m\x1b[9mcd\x1b[0m\x1b[4:3mef", 20, 3), 0);
        let spans: Vec<_> = plan.decorations.iter().map(|d| (d.kind, d.first, d.end)).collect();
        assert!(spans.contains(&(Decoration::Underline, 0, 2)));
        assert!(spans.contains(&(Decoration::Strike, 2, 4)));
        assert!(spans.contains(&(Decoration::Curly, 4, 6)));
    }

    #[test]
    fn the_underline_colour_is_its_own() {
        let plan = plan_row(&terminal("\x1b[4m\x1b[58;2;1;2;3mab", 20, 3), 0);
        assert_eq!(plan.decorations[0].color, Color::from_rgb8(1, 2, 3));
    }

    #[test]
    fn scrolled_back_rows_come_from_the_history() {
        let mut term = terminal("", 20, 3);
        for i in 0..10 {
            term.feed(format!("row {i}\r\n").as_bytes());
        }
        term.perform(crate::Action::Scroll(crate::ScrollTo::Top));
        let plan = plan_row(&term, 0);
        assert_eq!(runs(&plan), [(0, "row 0")]);
    }
}
