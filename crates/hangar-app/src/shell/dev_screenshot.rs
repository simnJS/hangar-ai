//! A picture of the main window, for whoever builds the app without a way to
//! take one: on a GNOME Wayland session nothing but the person may capture
//! the screen, and the agents porting the views need to see them.
//!
//! `HANGAR_DEV_SCREENSHOT=/some/where.png scripts/native-dev.sh` opens the app,
//! waits for it to settle (`HANGAR_DEV_SCREENSHOT_AFTER_MS`, 3000 by default),
//! writes what the main window shows as a PNG, and quits. Debug builds only:
//! a release build ignores the variables.

use std::path::PathBuf;
use std::time::Duration;

use iced::{Task, window};

use crate::message::Message;

/// The file to write, when a debug build was asked for a picture.
pub fn requested() -> Option<PathBuf> {
    if !cfg!(debug_assertions) {
        return None;
    }
    std::env::var_os("HANGAR_DEV_SCREENSHOT")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

/// Waits, then captures `window`.
pub fn capture(window: window::Id) -> Task<Message> {
    let after = std::env::var("HANGAR_DEV_SCREENSHOT_AFTER_MS")
        .ok()
        .and_then(|ms| ms.parse().ok())
        .unwrap_or(3000);
    Task::perform(tokio::time::sleep(Duration::from_millis(after)), |()| ())
        .then(move |()| window::screenshot(window))
        .map(Message::DevScreenshot)
}

/// Writes the capture as a PNG.
pub fn write(path: &std::path::Path, shot: &window::Screenshot) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|err| err.to_string())?;
    let mut encoder = png::Encoder::new(
        std::io::BufWriter::new(file),
        shot.size.width,
        shot.size.height,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|err| err.to_string())?;
    writer
        .write_image_data(&shot.rgba)
        .map_err(|err| err.to_string())
}
