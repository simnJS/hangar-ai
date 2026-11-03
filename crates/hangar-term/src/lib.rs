//! The terminal of a pane: the emulation is alacritty_terminal's, the drawing
//! and the keyboard are iced's, with dead keys and compose taken as the system
//! gives them.

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
pub use palette::{Palette, xterm_indexed};
pub use search::{MAX_MATCHES, SearchDirection, SearchError, SearchOptions, SearchStatus};
pub use terminal::Terminal;
pub use widget::{Binding, BindingKey, Bindings, TerminalView, terminal_view};
