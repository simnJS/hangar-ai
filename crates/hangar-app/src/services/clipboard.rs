//! The system clipboard, through iced's (which is the window's, so it works
//! on Wayland where a clipboard needs a surface).
//!
//! TODO(P17): images (a 3D capture copied as PNG) need `arboard`, which the
//! inventory chose (§9.2); add it with the package that copies one.

use iced::Task;

use crate::message::Message;

/// Puts `text` on the clipboard.
pub fn write(text: String) -> Task<Message> {
    iced::clipboard::write(text)
}

/// The clipboard's text, if it holds any.
pub fn read(then: fn(Option<String>) -> Message) -> Task<Message> {
    iced::clipboard::read().map(then)
}
