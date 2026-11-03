//! The windows of the daemon (inventory §1.7): the main one, and one per
//! detached workspace.
//!
//! One `iced::daemon` holds one model for every window, and terminals and
//! PTYs live in the model, not in a window: detaching a workspace only changes
//! which window draws it. There is no handover, no replay and no cross-window
//! protocol, which is most of what `lib/windows.ts` was.
//!
//! TODO(P0b): detach and reattach, the saved bounds (on a screen, else a
//! second screen, else 1200×800 centred), reopening detached windows at boot,
//! a workspace window's close request as a reattach.

use iced::window::{self, Position, Settings};
use iced::{Size, Task};

use crate::message::Message;
use crate::state::WsId;

/// What a window draws.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WindowKind {
    /// The navigator, every workspace not detached, the settings.
    Main,
    /// One workspace, without navigator or settings.
    Workspace(WsId),
}

pub const MAIN_SIZE: Size = Size::new(1360.0, 860.0);
pub const MAIN_MIN_SIZE: Size = Size::new(900.0, 560.0);
pub const WORKSPACE_SIZE: Size = Size::new(1200.0, 800.0);
pub const WORKSPACE_MIN_SIZE: Size = Size::new(640.0, 420.0);

/// The main window, centred. Its close request is the app's end, which the
/// shell handles itself (core shutdown, last save) before exiting.
pub fn main_settings() -> Settings {
    Settings {
        size: MAIN_SIZE,
        min_size: Some(MAIN_MIN_SIZE),
        position: Position::Centered,
        exit_on_close_request: false,
        ..Settings::default()
    }
}

/// A workspace's own window. Closing it brings the workspace back to the
/// main window rather than closing anything.
pub fn workspace_settings() -> Settings {
    Settings {
        size: WORKSPACE_SIZE,
        min_size: Some(WORKSPACE_MIN_SIZE),
        position: Position::Centered,
        exit_on_close_request: false,
        ..Settings::default()
    }
}

/// Opens the main window: its id now, for the model's window table, and the
/// task that opens it.
pub fn open_main() -> (window::Id, Task<Message>) {
    let (id, opened) = window::open(main_settings());
    (id, opened.discard())
}
