//! What goes into a [`Terminal`](crate::Terminal) and what comes out of it.
//!
//! The widget never touches the terminal directly: it holds a shared
//! reference to it for drawing, and turns what happens to it (keys, the mouse,
//! a resize) into [`Action`]s that the app passes to
//! [`Terminal::perform`](crate::Terminal::perform). What the terminal has to
//! say back, such as bytes for the process or a new title, comes out as
//! [`Event`]s. That keeps the state in one place, the app's, and makes every
//! behaviour testable without a window.

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use iced::keyboard::Modifiers;

/// The size of the grid and of the window the process is told about.
///
/// The fields are those of `portable_pty::PtySize`, so the app can convert it
/// with a struct literal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtySize {
    pub rows: u16,
    pub columns: u16,
    pub pixel_width: u16,
    pub pixel_height: u16,
}

/// Where a copied text goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardTarget {
    /// The clipboard of Ctrl+C and Ctrl+V.
    Clipboard,
    /// The selection buffer of X11 and Wayland, pasted with the middle button.
    Primary,
}

/// How to answer a program that asked what is on the clipboard (`OSC 52`).
///
/// The reply is an escape sequence built around the clipboard's text, which
/// only the terminal knows how to build.
#[derive(Clone)]
pub struct ClipboardReply(pub(crate) Arc<dyn Fn(&str) -> String + Send + Sync>);

impl ClipboardReply {
    /// The bytes to write to the process once the clipboard's text is known.
    pub fn bytes(&self, text: &str) -> Vec<u8> {
        (self.0)(text).into_bytes()
    }
}

impl fmt::Debug for ClipboardReply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ClipboardReply")
    }
}

/// Where a link was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkSource {
    /// The program marked it with `OSC 8`, so its text and target may differ.
    Hyperlink,
    /// It looked like a URL in the text.
    Detected,
}

/// A link under the pointer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub uri: String,
    pub source: LinkSource,
}

/// What a terminal reports to the app.
///
/// The enum is `non_exhaustive`: new kinds of event are added without breaking
/// the app's `match`, which should end with a `_ => {}` arm.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Event {
    /// Bytes for the process's stdin: what was typed, pasted or reported by
    /// the mouse, and the answers to the queries programs send (cursor
    /// position, device attributes, colours).
    Write(Vec<u8>),
    /// The grid changed size, and the process has to be told (a `SIGWINCH`
    /// through the PTY).
    Resize(PtySize),
    /// The program set the window title (`OSC 0` and `OSC 2`).
    Title(String),
    /// The program asked for the default title back.
    ResetTitle,
    /// The bell rang.
    Bell,
    /// Text to put on a clipboard: a program's `OSC 52`, or a selection when
    /// [`Config::copy_on_select`](crate::Config::copy_on_select) is on, or
    /// [`Action::Copy`].
    SetClipboard {
        target: ClipboardTarget,
        text: String,
    },
    /// A program asked to read the clipboard (`OSC 52`, if the configuration
    /// allows it). Answer with `reply.bytes(text)` written to the process.
    ClipboardLoad {
        target: ClipboardTarget,
        reply: ClipboardReply,
    },
    /// A desktop notification the program asked for (`OSC 9` and `OSC 777`).
    /// Agents use it to say that a turn is over.
    Notification {
        title: Option<String>,
        body: String,
    },
    /// The shell reported its working directory (`OSC 7`), as a path.
    WorkingDirectory(String),
    /// A link was clicked with Ctrl, or Cmd on macOS, held.
    OpenLink(Link),
    /// The pointer went down in the terminal. The app uses it to give the
    /// pane the focus.
    Pressed,
    /// The person typed or pasted something, as opposed to the terminal
    /// answering a query of the program (both arrive as [`Event::Write`]).
    /// An agent pane that was waiting for the person is waiting no more.
    UserInput,
    /// A file was dropped on the terminal. What to type for it is the app's
    /// to decide, since it depends on the shell (quoting) and on where the
    /// shell runs (a Windows path under WSL): the app builds the text and
    /// passes it to [`Terminal::paste_bytes`](crate::Terminal::paste_bytes)
    /// or as an [`Action::Paste`].
    FileDropped(PathBuf),
}

/// A mouse button as the terminal cares about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Middle,
    Right,
}

/// Which half of a cell the pointer is in. Selections start and end on a half
/// so that dragging across the middle of a character includes it or not, as in
/// every text editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// A position in the visible grid.
///
/// Both coordinates may be outside it, negative or past the last row: a drag
/// that leaves the widget keeps reporting where the pointer went, and the
/// terminal scrolls and clamps accordingly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellPos {
    pub column: i32,
    pub row: i32,
    pub side: Side,
}

/// What the pointer did, in terms of the grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pointer {
    Down {
        button: Button,
        at: CellPos,
        modifiers: Modifiers,
    },
    Up {
        button: Button,
        at: CellPos,
        modifiers: Modifiers,
    },
    Move {
        at: CellPos,
        modifiers: Modifiers,
    },
    /// The wheel. `notches` is positive for "up", towards older output; a
    /// notch of a mouse is 1, and a touchpad reports fractions of a line.
    Wheel {
        notches: f32,
        at: CellPos,
        modifiers: Modifiers,
    },
    /// The pointer left the widget with no button down.
    Left,
}

/// Where to scroll the history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollTo {
    /// Lines up (positive) or down (negative).
    Lines(i32),
    PageUp,
    PageDown,
    Top,
    Bottom,
    /// An absolute offset: the number of lines between the bottom and the
    /// first visible one. What dragging the scrollbar produces.
    Offset(usize),
}

/// The measurements the widget computed for the terminal it is drawing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    pub columns: u16,
    pub rows: u16,
    /// A cell's size in logical pixels, for the pixel size the process is told.
    pub cell_width: f32,
    pub cell_height: f32,
}

/// What the widget asks of a terminal.
///
/// Each variant is something that happened to the widget; passing them to
/// [`Terminal::perform`](crate::Terminal::perform) in the app's `update` is
/// all the wiring they need. An app may also build them itself, for instance
/// [`Action::Copy`] from its own menu.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Action {
    /// Bytes the user typed. Typing scrolls back to the bottom and drops the
    /// selection, like it does in every terminal.
    Input(Vec<u8>),
    /// Bytes that are not typing (key releases of the kitty protocol, focus
    /// reports): they reach the process and change nothing else.
    Report(Vec<u8>),
    /// Text to paste, wrapped for bracketed paste when the program asked.
    Paste(String),
    /// The widget's size, in cells.
    Resize(Metrics),
    Pointer(Pointer),
    Scroll(ScrollTo),
    /// The window gained or lost the focus. The terminal only reports focus to
    /// the process, and draws a solid cursor, when both the window and the
    /// pane have it.
    WindowFocus(bool),
    /// Put the selection on the clipboard.
    Copy,
    SelectAll,
    ClearSelection,
    /// A file was dropped while the pointer was over the terminal.
    FileDropped(PathBuf),
    /// Time passed: something the terminal was waiting for (a synchronized
    /// update's timeout, a search refresh) may be due. The widget sends it
    /// when [`Terminal::deadline`](crate::Terminal::deadline) is reached.
    Tick,
}
