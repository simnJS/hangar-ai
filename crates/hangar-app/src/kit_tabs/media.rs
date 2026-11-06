//! What a native tab asks of the system that an HTML tab has in its own
//! document: a file the person chooses (`files.pick`, a page's file input),
//! the picture on the clipboard (`ui.pasteImage`, a page's paste), and the
//! images of the workspace a view shows (`{ "file": … }`).
//!
//! hangar-kit-host checks each call (the permission, a person's click) and
//! holds the pictures a tab is handed; this is only the system's side.

use std::path::{Component, Path, PathBuf};

use hangar_kit_host::{ClipboardImage, PickedFile};
use hangar_kit_ui::api::{self, KitError, limits};

fn failed(message: impl Into<String>) -> KitError {
    KitError::new(KitError::FAILED, message)
}

/// `path` relative to `root`, with `/`, when it is inside it and names no
/// hidden part: what the tab may then read as a file of the workspace. A
/// file under a hidden folder is handed to the tab as a held picture instead,
/// as one outside the workspace is, since `files.read` would refuse its path.
fn inside(root: &Path, path: &Path) -> Option<String> {
    // Both in the same form, `\\?\` on Windows included, so one is a prefix
    // of the other when it is inside it.
    let root = std::fs::canonicalize(root).ok()?;
    let real = std::fs::canonicalize(path).ok()?;
    let relative = real.strip_prefix(&root).ok()?;
    let mut parts = Vec::new();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return None;
        };
        let part = part.to_str()?;
        if part.starts_with('.') {
            return None;
        }
        parts.push(part.to_owned());
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// `files.pick`: the system's chooser, opened in the workspace folder `root`,
/// asynchronous so that the window keeps drawing while it is open. A file
/// of the workspace is answered by its path; one elsewhere by its bytes, up
/// to 10 MB, which the tab holds as a picture.
pub async fn pick_file(
    root: String,
    request: api::FilesPick,
) -> Result<Option<PickedFile>, KitError> {
    let mut dialog = rfd::AsyncFileDialog::new().set_directory(&root);
    if let Some(title) = &request.title {
        dialog = dialog.set_title(title);
    }
    if !request.extensions.is_empty() {
        dialog = dialog.add_filter(request.extensions.join(", "), &request.extensions);
    }
    let Some(chosen) = dialog.pick_file().await else {
        return Ok(None);
    };
    let path: PathBuf = chosen.path().to_path_buf();
    // The file read off the window's thread: it may be on a slow disk.
    tokio::task::spawn_blocking(move || read_picked(&root, &path))
        .await
        .map_err(|why| failed(format!("the file chosen: {why}")))?
        .map(Some)
}

fn read_picked(root: &str, path: &Path) -> Result<PickedFile, KitError> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_owned();
    let cannot = |why: std::io::Error| failed(format!("{name} cannot be read: {why}"));
    let size = std::fs::metadata(path).map_err(cannot)?.len();
    if let Some(relative) = inside(Path::new(root), path) {
        return Ok(PickedFile {
            name,
            size,
            workspace_path: Some(relative),
            bytes: None,
        });
    }
    if size > limits::BINARY_MAX_BYTES as u64 {
        return Err(KitError::new(
            KitError::INVALID,
            format!("{name} is larger than 10 MB"),
        ));
    }
    let bytes = std::fs::read(path).map_err(cannot)?;
    Ok(PickedFile {
        name,
        size,
        workspace_path: None,
        bytes: Some(bytes),
    })
}

/// `ui.pasteImage`: the picture on the system's clipboard, `None` when it
/// holds none. Through `arboard`, as a 3D capture is copied (iced's
/// clipboard carries text only), off the window's thread since the system
/// may take its time.
pub async fn clipboard_image() -> Result<Option<ClipboardImage>, KitError> {
    tokio::task::spawn_blocking(|| {
        let mut clipboard =
            arboard::Clipboard::new().map_err(|why| failed(format!("the clipboard: {why}")))?;
        match clipboard.get_image() {
            Ok(image) => Ok(Some(ClipboardImage {
                width: image.width as u32,
                height: image.height as u32,
                rgba: image.bytes.into_owned(),
            })),
            Err(arboard::Error::ContentNotAvailable) => Ok(None),
            Err(why) => Err(failed(format!("the clipboard: {why}"))),
        }
    })
    .await
    .map_err(|why| failed(format!("the clipboard: {why}")))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_of_the_workspace_is_named_by_its_path_and_a_hidden_one_is_not() {
        let root = crate::kit_tabs::testdir::scratch("kit-media");
        let shots = root.join("captures");
        let hidden = root.join(".cache");
        std::fs::create_dir_all(&shots).unwrap();
        std::fs::create_dir_all(&hidden).unwrap();
        std::fs::write(shots.join("a.png"), b"png").unwrap();
        std::fs::write(hidden.join("b.png"), b"png").unwrap();
        assert_eq!(
            inside(&root, &shots.join("a.png")).as_deref(),
            Some("captures/a.png")
        );
        assert_eq!(inside(&root, &hidden.join("b.png")), None);
        assert_eq!(inside(&shots, &hidden.join("b.png")), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
