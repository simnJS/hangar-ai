//! Where the capture button's picture goes (`lib/modelCapture.ts`): on the
//! clipboard, or in a PNG file the person picks with the system's save dialog.
//!
//! The picture itself is hangar-model3d's `CaptureJob`, drawn with a device of
//! its own at twice the panel's size, so the person's framing is kept and the
//! result is sharp on a dense screen. This file only moves it.

use std::path::PathBuf;

use base64::Engine as _;
use chrono::{Datelike, Local, Timelike};

use hangar_model3d::{CaptureJob, Stamp, capture_name};

/// Where a capture is to go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    /// The system clipboard, as a picture.
    Clipboard,
    /// A PNG file, in the place the person picks.
    Dialog,
    /// A PNG file at a given path, with no dialog: what a debug build's
    /// `HANGAR_DEV_MODEL3D_CAPTURE` asks for, to look at a capture without a
    /// person at the dialog.
    File(PathBuf),
}

/// How a capture ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Done {
    Copied,
    Saved(PathBuf),
    /// The person closed the dialog.
    Cancelled,
}

/// The local date and time as `capture_name` wants them.
pub fn stamp(now: chrono::DateTime<Local>) -> Stamp {
    Stamp {
        year: now.year().max(0) as u32,
        month: now.month(),
        day: now.day(),
        hour: now.hour(),
        minute: now.minute(),
        second: now.second(),
    }
}

/// `chair-20261026-204512.png` for `chair.glb`: the name the dialog proposes.
pub fn suggested_name(title: &str, now: chrono::DateTime<Local>) -> String {
    capture_name(title, stamp(now))
}

/// Takes the picture and delivers it. Everything that blocks (the GPU, the
/// dialog's portal, the disk, the clipboard) runs off the window's thread.
pub async fn deliver(
    job: CaptureJob,
    destination: Destination,
    suggested: String,
) -> Result<Done, String> {
    match destination {
        Destination::Clipboard => {
            // The clipboard takes pixels, not a file: skip the PNG.
            let (pixels, (width, height)) = tokio::task::spawn_blocking(move || job.render())
                .await
                .map_err(|error| error.to_string())?
                .map_err(|error| error.to_string())?;
            tokio::task::spawn_blocking(move || {
                crate::services::clipboard::write_image(pixels, width, height)
            })
            .await
            .map_err(|error| error.to_string())??;
            Ok(Done::Copied)
        }
        Destination::Dialog => {
            // The picture first, then the question, as the web app did: it is
            // what was on screen when the button was pressed.
            let png = job.run_async().await.map_err(|error| error.to_string())?;
            let Some(path) = choose(suggested).await else {
                return Ok(Done::Cancelled);
            };
            save(path, png).await
        }
        Destination::File(path) => {
            let png = job.run_async().await.map_err(|error| error.to_string())?;
            save(path, png).await
        }
    }
}

/// Asks where to save, proposing `suggested`. `None` when the person cancels.
async fn choose(suggested: String) -> Option<PathBuf> {
    let picked = rfd::AsyncFileDialog::new()
        .set_file_name(suggested)
        .add_filter("PNG", &["png"])
        .save_file()
        .await?;
    let mut path = picked.path().to_path_buf();
    // hangar-core writes only to a `.png`: a name typed without its extension
    // gets it, as the dialogs of most systems would add it themselves.
    if path.extension().is_none() {
        path.set_extension("png");
    }
    Some(path)
}

/// Writes the PNG where `path` says, through hangar-core, which refuses
/// anything that is not a `.png` path or not a PNG.
pub async fn save(path: PathBuf, png: Vec<u8>) -> Result<Done, String> {
    let encoded = base64::engine::general_purpose::STANDARD.encode(&png);
    hangar_core::model::model_save_png(path.to_string_lossy().into_owned(), encoded).await?;
    Ok(Done::Saved(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model3d_view::scratch::Scratch;
    use chrono::TimeZone;

    fn at(hour: u32, minute: u32, second: u32) -> chrono::DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 10, 26, hour, minute, second)
            .single()
            .expect("an unambiguous local time")
    }

    #[test]
    fn the_proposed_name_is_the_models_with_the_time() {
        assert_eq!(
            suggested_name("chair.glb", at(20, 45, 12)),
            "chair-20261026-204512.png"
        );
        assert_eq!(
            suggested_name("scene: a/b.gltf", at(1, 2, 3)),
            "scene- a-b-20261026-010203.png"
        );
        assert_eq!(
            suggested_name(".glb", at(1, 2, 3)),
            "model-20261026-010203.png"
        );
    }

    fn png() -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, 2, 2);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("a header");
        writer.write_image_data(&[255; 16]).expect("pixels");
        writer.finish().expect("a finished PNG");
        bytes
    }

    #[tokio::test]
    async fn a_saved_capture_is_a_png_at_the_path() {
        let dir = Scratch::new("save");
        let path = dir.join("chair.png");
        let done = save(path.clone(), png()).await.expect("it is written");
        assert_eq!(done, Done::Saved(path.clone()));
        assert!(std::fs::read(&path).unwrap().starts_with(b"\x89PNG"));
    }

    #[tokio::test]
    async fn hangar_core_refuses_a_path_that_is_not_a_png() {
        let dir = Scratch::new("refuse-name");
        let path = dir.join("chair.txt");
        let error = save(path.clone(), png()).await.unwrap_err();
        assert!(error.contains(".png"), "{error}");
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn bytes_that_are_not_a_png_are_refused() {
        let dir = Scratch::new("refuse-bytes");
        let path = dir.join("chair.png");
        let error = save(path.clone(), b"not a picture".to_vec())
            .await
            .unwrap_err();
        assert!(error.contains("PNG"), "{error}");
        assert!(!path.exists());
    }
}
