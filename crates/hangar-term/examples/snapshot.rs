//! Renders a terminal to a PNG without opening a window:
//! `cargo run -p hangar-term --example snapshot -- out.png`.
//!
//! The screen it draws is a tour of what the renderer has to get right: the
//! 16, 256 and true colours, every text attribute, box drawing, blocks and
//! braille, wide characters, the cursor, a selection, a search and a link. It
//! uses the software renderer, so it needs no GPU and gives the same pixels on
//! every machine, which makes it the quickest way to see what a change to the
//! drawing did.

use hangar_term::{
    Action, Button, CellPos, Config, Font, Palette, Pointer, SearchOptions, Side, Terminal,
    terminal_view,
};
use iced::advanced::Shell;
use iced::advanced::clipboard;
use iced::advanced::layout::{Layout, Limits};
use iced::advanced::renderer::{Headless, Style};
use iced::advanced::widget::Tree;
use iced::keyboard::Modifiers;
use iced::{Color, Element, Pixels, Size, mouse, window};

type Renderer = iced::Renderer;

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "snapshot.png".into());
    let scale: f32 = std::env::var("SCALE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.0);
    let columns: u16 = 100;
    let rows: u16 = 34;

    let mut terminal = Terminal::new(Config::default(), columns, rows);
    terminal.set_font(Font::from_css_stack(
        &std::env::var("HANGAR_TERM_FONT")
            .unwrap_or_else(|_| "\"DejaVu Sans Mono\", monospace".into()),
        14.0,
        1.25,
    ));
    terminal.set_palette(Palette::default());
    terminal.set_focused(true);
    terminal.feed(SCREEN.as_bytes());

    let renderer_size = Size::new(900.0, 640.0);
    let physical = Size::new(
        (renderer_size.width * scale) as u32,
        (renderer_size.height * scale) as u32,
    );
    let mut renderer = iced::futures::executor::block_on(Renderer::new(
        iced::Font::DEFAULT,
        Pixels(16.0),
        Some("tiny-skia"),
    ))
    .expect("a software renderer");

    // The widget asks for the grid it has room for on its first frame; apply
    // that the way an app would, then draw.
    let frame = |terminal: &Terminal, renderer: &mut Renderer, draw: bool| -> Vec<Action> {
        let mut element: Element<'_, Action, iced::Theme, Renderer> =
            terminal_view(terminal, |action| action)
                .scale_factor(scale)
                .padding(8)
                .into();
        let mut tree = Tree::new(&element);
        let node = element.as_widget_mut().layout(
            &mut tree,
            renderer,
            &Limits::new(Size::ZERO, renderer_size),
        );
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        element.as_widget_mut().update(
            &mut tree,
            &iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now())),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            renderer,
            &mut clipboard::Null,
            &mut shell,
            &iced::Rectangle::with_size(renderer_size),
        );
        if draw {
            element.as_widget().draw(
                &tree,
                renderer,
                &iced::Theme::Dark,
                &Style {
                    text_color: Color::WHITE,
                },
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &iced::Rectangle::with_size(renderer_size),
            );
        }
        messages
    };
    for action in frame(&terminal, &mut renderer, false) {
        terminal.perform(action);
    }
    // A selection over the word "selected", as a drag would leave it.
    let at = |column, row, side| CellPos { column, row, side };
    let none = Modifiers::empty();
    let (c0, r0) = locate(&terminal, "selected");
    for pointer in [
        Pointer::Down {
            button: Button::Left,
            at: at(c0, r0, Side::Left),
            modifiers: none,
        },
        Pointer::Move {
            at: at(c0 + 7, r0, Side::Right),
            modifiers: none,
        },
        Pointer::Up {
            button: Button::Left,
            at: at(c0 + 7, r0, Side::Right),
            modifiers: none,
        },
    ] {
        terminal.perform(Action::Pointer(pointer));
    }
    let _ = terminal.search("needle", SearchOptions::default());

    frame(&terminal, &mut renderer, true);

    let rgba = renderer.screenshot(physical, scale, Palette::default().background);
    std::fs::write(&path, png(physical.width, physical.height, &rgba)).expect("write the png");
    println!("{path}: {}x{}", physical.width, physical.height);
}

/// Where `needle` is on the screen, as (column, row).
fn locate(terminal: &Terminal, needle: &str) -> (i32, i32) {
    for (row, line) in terminal.screen_text().iter().enumerate() {
        if let Some(byte) = line.find(needle) {
            return (line[..byte].chars().count() as i32, row as i32);
        }
    }
    (0, 0)
}

const SCREEN: &str = concat!(
    "\x1b[1mhangar-term\x1b[0m  snapshot of the renderer\r\n\r\n",
    "\x1b[30m black \x1b[31m red \x1b[32m green \x1b[33m yellow \x1b[34m blue \x1b[35m magenta \x1b[36m cyan \x1b[37m white \x1b[0m\r\n",
    "\x1b[90m black \x1b[91m red \x1b[92m green \x1b[93m yellow \x1b[94m blue \x1b[95m magenta \x1b[96m cyan \x1b[97m white \x1b[0m\r\n",
    "\x1b[40m   \x1b[41m   \x1b[42m   \x1b[43m   \x1b[44m   \x1b[45m   \x1b[46m   \x1b[47m   \x1b[0m ",
    "\x1b[100m   \x1b[101m   \x1b[102m   \x1b[103m   \x1b[104m   \x1b[105m   \x1b[106m   \x1b[107m   \x1b[0m\r\n",
    "\x1b[48;5;196m  \x1b[48;5;202m  \x1b[48;5;208m  \x1b[48;5;214m  \x1b[48;5;220m  \x1b[48;5;226m  \x1b[48;5;190m  \x1b[48;5;154m  \x1b[48;5;118m  \x1b[48;5;46m  \x1b[48;5;47m  \x1b[48;5;48m  \x1b[48;5;49m  \x1b[48;5;51m  \x1b[48;5;39m  \x1b[48;5;27m  \x1b[48;5;21m  \x1b[48;5;57m  \x1b[48;5;93m  \x1b[48;5;129m  \x1b[0m  256 colours\r\n",
    "\x1b[48;2;255;0;80m \x1b[48;2;230;20;100m \x1b[48;2;200;40;130m \x1b[48;2;170;60;160m \x1b[48;2;140;80;190m \x1b[48;2;110;100;220m \x1b[48;2;80;120;255m \x1b[48;2;60;160;255m \x1b[48;2;40;200;255m \x1b[48;2;20;230;255m \x1b[0m  true colour\r\n\r\n",
    "\x1b[1mbold\x1b[0m \x1b[3mitalic\x1b[0m \x1b[1;3mbold italic\x1b[0m \x1b[2mdim\x1b[0m \x1b[7minverse\x1b[0m \x1b[9mstrikethrough\x1b[0m \x1b[8mhidden\x1b[0m\r\n",
    "\x1b[4munderline\x1b[0m \x1b[21mdouble\x1b[0m \x1b[4:3m\x1b[58;2;255;80;80mcurly red\x1b[0m \x1b[4:4mdotted\x1b[0m \x1b[4:5mdashed\x1b[0m \x1b[31;1;4mbold red underline\x1b[0m\r\n\r\n",
    "\x1b[36m┌───────────────┐\x1b[0m \x1b[33m╭───────────────╮\x1b[0m \x1b[35m╔═══════════════╗\x1b[0m \x1b[32m┏━━━━━━━━━━━━━━━┓\x1b[0m\r\n",
    "\x1b[36m│\x1b[0m selected text \x1b[36m│\x1b[0m \x1b[33m│\x1b[0m  rounded box  \x1b[33m│\x1b[0m \x1b[35m║\x1b[0m  double box   \x1b[35m║\x1b[0m \x1b[32m┃\x1b[0m   heavy box   \x1b[32m┃\x1b[0m\r\n",
    "\x1b[36m├───────┬───────┤\x1b[0m \x1b[33m│\x1b[0m    a needle   \x1b[33m│\x1b[0m \x1b[35m╠═══════╦═══════╣\x1b[0m \x1b[32m┣━━━━━━━┳━━━━━━━┫\x1b[0m\r\n",
    "\x1b[36m│ cell  │ cell  │\x1b[0m \x1b[33m│\x1b[0m    needle     \x1b[33m│\x1b[0m \x1b[35m║ cell  ║ cell  ║\x1b[0m \x1b[32m┃ cell  ┃ cell  ┃\x1b[0m\r\n",
    "\x1b[36m└───────┴───────┘\x1b[0m \x1b[33m╰───────────────╯\x1b[0m \x1b[35m╚═══════╩═══════╝\x1b[0m \x1b[32m┗━━━━━━━┻━━━━━━━┛\x1b[0m\r\n\r\n",
    "blocks \x1b[36m▀▄█▌▐ ▁▂▃▄▅▆▇█ ░▒▓ ▖▗▘▙▚▛▜▝▞▟\x1b[0m  braille \x1b[33m⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ ⣿ ⡇\x1b[0m\r\n",
    "wide 日本語のテキスト 한국어 emoji 😀 🚀 ✅ ❯ ● ✻ accents café déjà vu ñ ü\r\n",
    "link: \x1b[34mhttps://hangar.ai/docs\x1b[0m and \x1b]8;;https://example.com\x1b\\an OSC 8 link\x1b]8;;\x1b\\\r\n",
    "searching for a needle in a haystack with another needle\r\n\r\n",
    "$ echo ready\x1b[2 q\x1b[5D"
);

// ---- A PNG writer: stored (uncompressed) deflate, which is all a look needs.

fn png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity((width as usize * 4 + 1) * height as usize);
    for row in rgba.chunks(width as usize * 4) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut zlib = vec![0x78, 0x01];
    let mut blocks = raw.chunks(65535).peekable();
    while let Some(block) = blocks.next() {
        zlib.push(blocks.peek().is_none() as u8);
        zlib.extend_from_slice(&(block.len() as u16).to_le_bytes());
        zlib.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());

    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::new();
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &zlib);
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc = 0xffff_ffffu32;
    for &byte in &out[start..] {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                0xedb8_8320 ^ (crc >> 1)
            } else {
                crc >> 1
            };
        }
    }
    out.extend_from_slice(&(!crc).to_be_bytes());
}
