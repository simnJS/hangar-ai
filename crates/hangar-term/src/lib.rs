//! The terminal of a pane: the emulation is alacritty_terminal's, the drawing
//! and the keyboard are iced's, with dead keys and compose taken as the system
//! gives them.
//!
//! The crate knows nothing about processes. The app owns the PTY (Hangar's is
//! `hangar-core`'s manager) and this crate is the two halves that sit on either
//! side of it: a [`Terminal`], which is the state of one screen, and a widget,
//! [`TerminalView`], which draws it and turns the keyboard, the mouse and input
//! methods into bytes.
//!
//! # Wiring it up
//!
//! Output goes in with [`Terminal::feed`]. What the widget sees (keys, the
//! mouse, a resize) comes out as [`Action`]s that go in with
//! [`Terminal::perform`]. Both return [`Event`]s, and that is everything the
//! terminal has to say: bytes for the process, the new size of the PTY, a
//! title, a bell, text for the clipboard.
//!
//! ```no_run
//! use hangar_term::{Action, Config, Event, Terminal, terminal_view};
//! use iced::{Element, Task};
//!
//! # fn write_to_pty(_: &[u8]) {}
//! # fn resize_pty(_: hangar_term::PtySize) {}
//! #[derive(Debug, Clone)]
//! enum Message {
//!     /// Something happened to the widget.
//!     Terminal(Action),
//!     /// The process printed something (from the thread that reads the PTY).
//!     Output(Vec<u8>),
//! }
//!
//! struct Pane {
//!     terminal: Terminal,
//! }
//!
//! impl Pane {
//!     fn new() -> Self {
//!         // Created at the size the PTY was spawned with. The widget reports
//!         // the size it really has room for on its first frame.
//!         let mut terminal = Terminal::new(Config::default(), 80, 24);
//!         terminal.set_focused(true);
//!         Self { terminal }
//!     }
//!
//!     fn update(&mut self, message: Message) -> Task<Message> {
//!         let events = match message {
//!             Message::Terminal(action) => self.terminal.perform(action),
//!             Message::Output(bytes) => self.terminal.feed(&bytes),
//!         };
//!         for event in events {
//!             match event {
//!                 Event::Write(bytes) => write_to_pty(&bytes),
//!                 Event::Resize(size) => resize_pty(size),
//!                 Event::SetClipboard { text, .. } => return iced::clipboard::write(text),
//!                 _ => {}
//!             }
//!         }
//!         Task::none()
//!     }
//!
//!     fn view(&self) -> Element<'_, Message> {
//!         terminal_view(&self.terminal, Message::Terminal).into()
//!     }
//! }
//! ```
//!
//! The example `shell` (`cargo run -p hangar-term --example shell`) is this
//! with a real PTY, a search bar and the clipboard, and the tests in
//! `tests/pty.rs` run the same loop against real processes.
//!
//! ## What the app has to do
//!
//! - **Write [`Event::Write`] to the process**, in order, and nothing else.
//!   It carries typing, pastes, mouse and focus reports and the terminal's
//!   answers to the program's queries (cursor position, device attributes,
//!   the background colour that TUIs read to pick a light or dark theme).
//!   Write from a thread of its own: a process that stopped reading must not
//!   freeze the interface.
//! - **Resize the PTY on [`Event::Resize`].** The widget asks once per frame
//!   at most, and only when the number of columns or rows changed.
//! - **Give the pane the focus on [`Event::Pressed`]** and tell the terminal
//!   with [`Terminal::set_focused`]. Only a focused terminal gets the
//!   keyboard, draws a solid cursor and reports focus to programs that asked
//!   (`CSI ? 1004 h`). Hand the keyboard to a text field (a search bar) by
//!   taking the focus away from the terminal.
//! - **Handle the clipboard** ([`Event::SetClipboard`], and
//!   [`Event::ClipboardLoad`] if [`Config::osc52`] allows it).
//! - **Open links** on [`Event::OpenLink`] with the system browser: a click
//!   on a link (it is reported when the button is let go in the cell it was
//!   pressed in, so a drag that starts on one still selects), or a click with
//!   Ctrl or Cmd held, which is also how one is opened over a program that has
//!   the mouse (with Shift, which takes the mouse back). Only web links
//!   (`http`, `https`) are ever reported, or underlined under the pointer
//!   ([`is_web_link`]).
//!
//! The rest is optional: [`Event::Notification`] (`OSC 9` and `OSC 777`, which
//! agents use to say a turn is over), [`Event::Bell`], [`Event::UserInput`]
//! (real typing, as opposed to answers to queries), [`Event::FileDropped`]
//! (what to type for a file is the app's call: quoting depends on the shell),
//! [`Event::Title`] and [`Event::WorkingDirectory`].
//!
//! # The messages
//!
//! | The widget publishes | When | What [`Terminal::perform`] does |
//! |---|---|---|
//! | [`Action::Input`] | a key or composed text | scrolls to the bottom, drops the selection, [`Event::Write`] |
//! | [`Action::Report`] | key releases of the kitty protocol | [`Event::Write`] |
//! | [`Action::Paste`] | Ctrl+Shift+V, middle click | bracketed if the program asked, [`Event::Write`] |
//! | [`Action::Resize`] | the widget's size changed | resizes the grid, [`Event::Resize`] |
//! | [`Action::Pointer`] | press, release, move, wheel | selection, scrolling, mouse reports, [`Event::OpenLink`] |
//! | [`Action::Scroll`] | Shift+PageUp, the scrollbar | scrolls the history |
//! | [`Action::Copy`], [`Action::SelectAll`] | Ctrl+Shift+C, Ctrl+Shift+A | [`Event::SetClipboard`], selects |
//! | [`Action::WindowFocus`] | the window gained or lost the focus | focus report if asked |
//! | [`Action::FileDropped`] | a file was dropped over the widget | [`Event::FileDropped`] |
//! | [`Action::Tick`] | something the terminal waited for is due | ends a synchronized update, refreshes the search |
//!
//! An app can build the same actions itself, for instance [`Action::Copy`] from
//! a menu, and call the methods behind them directly:
//! [`selection_text`](Terminal::selection_text),
//! [`paste_bytes`](Terminal::paste_bytes), [`clear`](Terminal::clear),
//! [`scroll_to_bottom`](Terminal::scroll_to_bottom).
//!
//! # Keys
//!
//! Keys go to the program as xterm encodes them (modifiers, application
//! cursor and keypad modes, function keys, Alt as an escape prefix), and as
//! the kitty keyboard protocol encodes them for the programs that ask for it,
//! which is what lets an agent tell Shift+Enter from Enter. [`encode_key`] is the
//! whole table, and a pure function.
//!
//! Text is taken from what the platform composed, never rebuilt from the key,
//! and input methods are handled through `iced`'s input-method events. That
//! is how dead keys work: on a US-International layout `'` then `e` gives one
//! `é`, `'` then space gives `'`, and nothing is left pending in between.
//! [`InputState`] holds the rules; they are tested through the widget in
//! `tests/widget.rs`.
//!
//! The app's keymap comes first, in this order:
//!
//! 1. [`TerminalView::key_filter`]: a predicate that says "the app takes this
//!    one". The key is neither typed nor captured, so it goes on to the app's
//!    own handling. Use it for Ctrl+Tab, Alt+arrows, Ctrl+Shift+P, and every
//!    key the app has a binding for.
//! 2. [`TerminalView::intercept`]: the same, but the terminal publishes the
//!    app's message itself.
//! 3. [`Bindings`]: Ctrl+Shift+C and V, Shift+Insert, Shift+PageUp and the
//!    like. Replace or empty them with [`TerminalView::bindings`].
//! 4. [`Config::ctrl_v_pastes`]: Ctrl+V pastes instead of sending `^V`.
//!
//! What is left is the program's. [`Config::shift_enter`] turns Shift+Enter
//! into `ESC CR` for the agent CLIs that read it as a newline.
//!
//! # Looks
//!
//! The crate has no themes. The app maps its own onto a [`Palette`]
//! (foreground, background, cursor, selection, the 16 ANSI colours, optionally
//! all 256) and a [`Font`] (family list, size, line height) and sets them with
//! [`Terminal::set_palette`] and [`Terminal::set_font`]. A change applies on
//! the next frame without restarting anything. Colours a program picks itself
//! pass through: true colour is drawn as it is, and `OSC 4`/`10`/`11`/`12`
//! override the palette until reset.
//!
//! A [`Font`] also has a [`letter_spacing`](Font::letter_spacing), in pixels
//! after every character, which widens the cell and sets each character in
//! its own; and [`TerminalView::dimmed`] draws a terminal that is not the one
//! being typed into a little more transparent.
//!
//! The widget needs the window's scale factor to round rows to whole pixels:
//! pass it with [`TerminalView::scale_factor`] (the display's scale times any
//! zoom the app applies to the window), or it uses the last one the window
//! announced, which a widget created after that never hears.
//!
//! # Searching
//!
//! [`Terminal::search`] starts a search (plain or regex, case sensitive or
//! not) and highlights the matches; [`Terminal::search_step`] moves between
//! them; [`Terminal::search_status`] gives the `index/count` for a bar, capped
//! at [`MAX_MATCHES`]. The bar itself is the app's. A search keeps itself up
//! to date while output arrives.
//!
//! # Reading the screen, history and replay
//!
//! - [`Terminal::screen_text`] is the live screen as text, row by row;
//!   [`Terminal::read_lines`] is the last *n* lines including the history,
//!   with wrapped rows joined and the empty rows below the text dropped. They
//!   are for code that reads a pane the way a person would.
//! - [`Terminal::replay`] feeds output that was already seen, a saved
//!   history, for a window that takes a pane over. It parses it like
//!   [`feed`](Terminal::feed) but says nothing back: the queries it contains
//!   are not answered, so no replies land in the process's input as phantom
//!   keystrokes, and no bell, notification or clipboard copy fires again.
//! - A terminal needs no window: the panes of a workspace that is not on
//!   screen keep being fed.
//!
//! # How it draws
//!
//! A row is reduced to a plan, background rectangles and text runs per style,
//! and kept until the terminal says the row changed. Text is handed to `iced`
//! as one item per run, which `iced` shapes once and caches, so scrolling
//! output reuses its strings. Box drawing, block elements and braille are
//! drawn as geometry, so the frames of a TUI meet at any line height; wide
//! characters and symbols from fallback fonts are placed cell by cell so a
//! font with another advance cannot shift the columns. Selection, search
//! matches, the cursor and the scrollbar are overlays and never invalidate a
//! plan. `cargo run --release -p hangar-term --example bench` measures it;
//! `--example snapshot` renders a test screen to a PNG without a window.
//!
//! Each [`Terminal`] reserves a 2 MiB buffer in the parser for synchronized
//! updates, which the operating system commits only when it is used.
//!
//! # Not done
//!
//! Ligatures beyond the font's own (off by default, and not with a letter
//! spacing), the sixel and kitty graphics protocols, and screen-reader support
//! are not there. The font's real metrics are not available to a widget, so
//! underlines sit at where a typical monospace font's baseline is.

mod config;
mod event;
mod font;
mod ime;
mod keys;
mod links;
mod mouse;
mod osc;
mod palette;
mod plan;
mod pointer;
mod search;
mod shapes;
mod terminal;
mod widget;

pub use alacritty_terminal::term::TermMode;
pub use config::{Config, CursorShape, Osc52};
pub use event::{
    Action, Button, CellPos, ClipboardReply, ClipboardTarget, Event, Link, LinkSource, Metrics,
    Pointer, PtySize, ScrollTo, Side,
};
pub use font::Font;
pub use ime::{InputState, Preedit};
pub use keys::{KeyEventKind, KeyOptions, KeyPress, encode_key};
pub use links::is_web_link;
pub use palette::{Palette, xterm_indexed};
pub use search::{MAX_MATCHES, SearchDirection, SearchError, SearchOptions, SearchStatus};
pub use terminal::Terminal;
pub use widget::{Binding, BindingKey, Bindings, TerminalView, terminal_view};
