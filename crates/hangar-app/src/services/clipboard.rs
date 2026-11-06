//! The system clipboard. Text goes through iced's (which is the window's, so
//! it works on Wayland where a clipboard needs a surface); a picture (a 3D
//! capture) through `arboard`, which the inventory chose for images (§9.2)
//! since iced's clipboard carries text only.

use std::borrow::Cow;
use std::sync::{Mutex, OnceLock};

use iced::Task;

use crate::message::Message;

/// Puts `text` on the clipboard.
pub fn write(text: String) -> Task<Message> {
    iced::clipboard::write(text)
}

/// The process's `arboard` clipboard. On Linux the contents are only served
/// while the `Clipboard` that set them is alive, so one is kept for the whole
/// run instead of one per copy.
fn pictures() -> &'static Mutex<Option<arboard::Clipboard>> {
    static CLIPBOARD: OnceLock<Mutex<Option<arboard::Clipboard>>> = OnceLock::new();
    CLIPBOARD.get_or_init(|| Mutex::new(None))
}

/// Puts a picture on the clipboard: `pixels` are RGBA, `width` by `height`.
/// It blocks (the system's clipboard may take its time), so it is called off
/// the window's thread.
pub fn write_image(pixels: Vec<u8>, width: u32, height: u32) -> Result<(), String> {
    let mut held = pictures()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    if held.is_none() {
        // Not remembered when it fails, so the next copy tries again.
        *held = Some(arboard::Clipboard::new().map_err(|error| error.to_string())?);
    }
    let clipboard = held.as_mut().ok_or("no clipboard")?;
    clipboard
        .set_image(arboard::ImageData {
            width: width as usize,
            height: height as usize,
            bytes: Cow::Owned(pixels),
        })
        .map_err(|error| error.to_string())
}
