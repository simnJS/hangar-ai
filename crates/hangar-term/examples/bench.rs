//! What a frame costs: `cargo run --release -p hangar-term --example bench`,
//! or `... --example bench -- recorded.log` to replay a recorded session too.
//!
//! It measures the work this crate does on the CPU for a 200x60 terminal, the
//! size of a maximised pane: feeding output to the emulator, and the widget's
//! `draw` (planning the rows that changed and handing every quad and run of
//! text to the renderer). The GPU's share is not in it, and the software
//! renderer used here only records what it is given until a screenshot is
//! asked for, so the numbers are the cost of everything up to the renderer.

use std::time::{Duration, Instant};

use hangar_term::{Action, Config, Font, Palette, Terminal, terminal_view};
use iced::advanced::Renderer as _;
use iced::advanced::Shell;
use iced::advanced::clipboard;
use iced::advanced::layout::{Layout, Limits};
use iced::advanced::renderer::{Headless, Style};
use iced::advanced::widget::Tree;
use iced::{Color, Element, Pixels, Size, mouse, window};

type Renderer = iced::Renderer;

const COLUMNS: u16 = 200;
const ROWS: u16 = 60;

/// One frame of a busy full-screen interface: coloured words on every row, a
/// frame around the screen, a status bar. `seed` makes each frame differ.
fn tui_frame(seed: u32, columns: usize, rows: usize) -> String {
    let words = [
        "fn", "let", "match", "impl", "struct", "pub", "async", "await", "return", "where",
    ];
    let colors = [31, 32, 33, 34, 35, 36, 90, 97];
    let mut out = String::from("\x1b[H");
    let mut state = seed.wrapping_mul(2654435761).wrapping_add(1);
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };
    for row in 0..rows - 1 {
        out.push_str("\x1b[36m│\x1b[0m ");
        let mut width = 2;
        while width < columns - 4 {
            let word = words[next() as usize % words.len()];
            let color = colors[next() as usize % colors.len()];
            let bold = if next() % 7 == 0 { "1;" } else { "" };
            out.push_str(&format!("\x1b[{bold}{color}m{word}\x1b[0m "));
            width += word.len() + 1;
        }
        out.push_str(&format!("\x1b[{};{}H\x1b[36m│\x1b[0m", row + 1, columns));
        out.push_str("\r\n");
    }
    out.push_str("\x1b[7m status: working                                  \x1b[0m");
    out
}

/// One frame of the widget. The tree persists between frames, as `iced`'s does:
/// it holds the widget's row cache, so a frame that follows another one only
/// redoes what changed.
fn frame(
    terminal: &Terminal,
    renderer: &mut Renderer,
    size: Size,
    tree: &mut Option<Tree>,
) -> (Vec<Action>, Duration) {
    let mut element: Element<'_, Action, iced::Theme, Renderer> =
        terminal_view(terminal, |action| action).into();
    let tree = match tree {
        Some(tree) => {
            tree.diff(&element);
            tree
        }
        None => tree.insert(Tree::new(&element)),
    };
    let node = element
        .as_widget_mut()
        .layout(tree, renderer, &Limits::new(Size::ZERO, size));
    let mut messages = Vec::new();
    let mut shell = Shell::new(&mut messages);
    element.as_widget_mut().update(
        tree,
        &iced::Event::Window(window::Event::RedrawRequested(Instant::now())),
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        renderer,
        &mut clipboard::Null,
        &mut shell,
        &iced::Rectangle::with_size(size),
    );
    renderer.reset(iced::Rectangle::with_size(size));
    let start = Instant::now();
    element.as_widget().draw(
        tree,
        renderer,
        &iced::Theme::Dark,
        &Style {
            text_color: Color::WHITE,
        },
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &iced::Rectangle::with_size(size),
    );
    (messages, start.elapsed())
}

fn main() {
    let mut terminal = Terminal::new(Config::default(), COLUMNS, ROWS);
    terminal.set_font(Font::from_css_stack(
        "\"DejaVu Sans Mono\", monospace",
        13.0,
        1.2,
    ));
    terminal.set_palette(Palette::default());
    terminal.set_focused(true);

    // Feed: the emulator's throughput on the kind of output agents produce.
    let sample: String = (0..64)
        .map(|i| tui_frame(i, COLUMNS as usize, ROWS as usize))
        .collect();
    let bytes = sample.as_bytes();
    let start = Instant::now();
    let mut fed = 0usize;
    for _ in 0..16 {
        for chunk in bytes.chunks(16 * 1024) {
            terminal.feed(chunk);
            fed += chunk.len();
        }
    }
    let took = start.elapsed();
    println!(
        "feed: {:.1} MB in {:.0} ms = {:.0} MB/s (coloured TUI frames)",
        fed as f64 / 1e6,
        took.as_secs_f64() * 1e3,
        fed as f64 / 1e6 / took.as_secs_f64()
    );
    let plain = "The quick brown fox jumps over the lazy dog. 0123456789\r\n".repeat(200_000);
    let start = Instant::now();
    for chunk in plain.as_bytes().chunks(16 * 1024) {
        terminal.feed(chunk);
    }
    let took = start.elapsed();
    println!(
        "feed: {:.1} MB in {:.0} ms = {:.0} MB/s (plain scrolling text)",
        plain.len() as f64 / 1e6,
        took.as_secs_f64() * 1e3,
        plain.len() as f64 / 1e6 / took.as_secs_f64()
    );

    // Draw.
    let mut renderer = iced::futures::executor::block_on(Renderer::new(
        iced::Font::DEFAULT,
        Pixels(16.0),
        Some("tiny-skia"),
    ))
    .expect("a software renderer");
    let size = Size::new(1700.0, 1000.0);
    let mut tree = None;
    for action in frame(&terminal, &mut renderer, size, &mut tree).0 {
        terminal.perform(action);
    }
    println!("grid: {:?} cells", terminal.size());

    terminal.feed(tui_frame(1, COLUMNS as usize, ROWS as usize).as_bytes());
    let (_, cold) = frame(&terminal, &mut renderer, size, &mut None);
    println!(
        "draw, first frame (every row planned):  {:>7.2} ms",
        cold.as_secs_f64() * 1e3
    );

    let mut warm = Vec::new();
    for _ in 0..50 {
        warm.push(frame(&terminal, &mut renderer, size, &mut tree).1);
    }
    println!(
        "draw, nothing changed (rows cached):     {:>7.2} ms",
        median(&mut warm)
    );

    let mut one = Vec::new();
    for i in 0..50 {
        terminal.feed(format!("\x1b[30;1H\x1b[32mline {i} changed\x1b[K").as_bytes());
        one.push(frame(&terminal, &mut renderer, size, &mut tree).1);
    }
    println!(
        "draw, one row changed:                   {:>7.2} ms",
        median(&mut one)
    );

    let mut full = Vec::new();
    for i in 0..50 {
        terminal.feed(tui_frame(100 + i, COLUMNS as usize, ROWS as usize).as_bytes());
        full.push(frame(&terminal, &mut renderer, size, &mut tree).1);
    }
    println!(
        "draw, whole screen repainted:            {:>7.2} ms",
        median(&mut full)
    );
    println!("(a frame at 60 fps is 16.67 ms)");

    sixteen_panes(&mut renderer);
    if let Some(path) = std::env::args().nth(1) {
        replay(&path, &mut renderer);
    }
}

/// A workspace of sixteen busy panes, all on screen: every pane is fed a chunk
/// of a redrawing interface and then drawn, as one frame of the app would.
fn sixteen_panes(renderer: &mut Renderer) {
    let size = Size::new(430.0, 260.0);
    let (columns, rows) = (50usize, 14usize);
    let mut panes: Vec<Terminal> = (0..16)
        .map(|_| {
            let mut t = Terminal::new(Config::default(), columns as u16, rows as u16);
            t.set_font(Font::from_css_stack(
                "\"DejaVu Sans Mono\", monospace",
                13.0,
                1.2,
            ));
            t.set_focused(false);
            t
        })
        .collect();
    let mut trees: Vec<Option<Tree>> = (0..16).map(|_| None).collect();
    for (pane, tree) in panes.iter_mut().zip(&mut trees) {
        for action in frame(pane, renderer, size, tree).0 {
            pane.perform(action);
        }
    }
    let (columns, rows) = {
        let (c, r) = panes[0].size();
        (c as usize, r as usize)
    };
    let mut parse = Vec::new();
    let mut draw = Vec::new();
    for tick in 0..60u32 {
        let start = Instant::now();
        for (n, pane) in panes.iter_mut().enumerate() {
            pane.feed(tui_frame(tick * 16 + n as u32, columns, rows).as_bytes());
        }
        parse.push(start.elapsed());
        let start = Instant::now();
        for (pane, tree) in panes.iter().zip(&mut trees) {
            frame(pane, renderer, size, tree);
        }
        // `frame` includes the layout and event handling of the widget; the
        // draw alone is what it reports, so measure the loop around it.
        draw.push(start.elapsed());
    }
    println!();
    println!(
        "16 panes of {columns}x{rows}, every pane repainted every frame:\n  parse: {:.2} ms, layout+update+draw: {:.2} ms per frame for all sixteen",
        median(&mut parse),
        median(&mut draw)
    );
}

/// A recorded stream from a file, fed in chunks the way the PTY delivers it,
/// with a frame drawn after every 64 KiB, which is generous: at 60 frames a
/// second that is 3.8 MB/s of output.
fn replay(path: &str, renderer: &mut Renderer) {
    let bytes = std::fs::read(path).expect("read the recorded stream");
    let mut terminal = Terminal::new(Config::default(), COLUMNS, ROWS);
    terminal.set_font(Font::from_css_stack(
        "\"DejaVu Sans Mono\", monospace",
        13.0,
        1.2,
    ));
    terminal.set_focused(true);
    let size = Size::new(1700.0, 1000.0);
    let mut tree = None;
    for action in frame(&terminal, renderer, size, &mut tree).0 {
        terminal.perform(action);
    }
    let (mut parse, mut draw, mut frames) = (Duration::ZERO, Duration::ZERO, 0);
    for block in bytes.chunks(64 * 1024) {
        let start = Instant::now();
        for chunk in block.chunks(16 * 1024) {
            terminal.feed(chunk);
        }
        parse += start.elapsed();
        draw += frame(&terminal, renderer, size, &mut tree).1;
        frames += 1;
    }
    println!();
    println!(
        "{path}: {:.1} MB, {frames} frames: parse {:.0} ms ({:.0} MB/s), draw {:.0} ms ({:.2} ms per frame)",
        bytes.len() as f64 / 1e6,
        parse.as_secs_f64() * 1e3,
        bytes.len() as f64 / 1e6 / parse.as_secs_f64(),
        draw.as_secs_f64() * 1e3,
        draw.as_secs_f64() * 1e3 / frames as f64,
    );
}

fn median(samples: &mut [Duration]) -> f64 {
    samples.sort();
    samples[samples.len() / 2].as_secs_f64() * 1e3
}
