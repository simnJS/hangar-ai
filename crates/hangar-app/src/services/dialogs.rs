//! The system's file dialogs (`rfd`, as `tauri-plugin-dialog` was), as
//! futures for `Task::perform`. Asynchronous, so the window keeps drawing
//! while one is open; on Linux they go through the desktop portal.

use std::path::PathBuf;

/// A folder, or `None` when the person cancelled.
pub async fn pick_folder(title: String, start: Option<PathBuf>) -> Option<PathBuf> {
    let mut dialog = rfd::AsyncFileDialog::new().set_title(title);
    if let Some(start) = start {
        dialog = dialog.set_directory(start);
    }
    dialog
        .pick_folder()
        .await
        .map(|handle| handle.path().to_path_buf())
}

/// A file among those with one of `extensions` (none: any file).
pub async fn pick_file(title: String, filter: Option<(String, Vec<String>)>) -> Option<PathBuf> {
    let mut dialog = rfd::AsyncFileDialog::new().set_title(title);
    if let Some((name, extensions)) = &filter {
        dialog = dialog.add_filter(name, extensions);
    }
    dialog
        .pick_file()
        .await
        .map(|handle| handle.path().to_path_buf())
}
