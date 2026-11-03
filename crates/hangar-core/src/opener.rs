//! Links, files and folders handed to the system: what the window asked of
//! `tauri-plugin-opener` (`openUrl`, `openPath`, `revealItemInDir`), with the
//! limits its capability gave it (src-tauri/capabilities/default.json):
//!
//! - a link opens in the browser, or the mail or phone app, only for the
//!   schemes the plugin's default scope allowed: `http`, `https`, `mailto`,
//!   `tel`. A link in a Markdown file an agent wrote is not a way to start a
//!   program;
//! - a file opens in the app the system gives it only when it is a PDF, the
//!   one kind the preview hands on that way (src/lib/preview.ts); any other
//!   is refused, and the window shows it in its folder instead;
//! - any file or folder can be shown in the file manager, selected.
//!
//! The opening itself is the `open` crate's, as it was under the plugin.

use std::path::Path;

/// The schemes a link may have (the plugin's `allow-default-urls`).
const URL_SCHEMES: &[&str] = &["http://", "https://", "mailto:", "tel:"];

/// Opens a link with the system's handler for it: `openUrl` in the window.
pub fn open_url(url: &str) -> Result<(), String> {
    let allowed = URL_SCHEMES.iter().any(|scheme| {
        url.get(..scheme.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(scheme))
    });
    if !allowed {
        return Err(format!(
            "only http, https, mailto and tel links are opened: {url}"
        ));
    }
    open::that_detached(url).map_err(|err| err.to_string())
}

/// Opens a PDF with the app the system opens PDFs with: `openPath` in the
/// window. The capability allowed `**/*.pdf`; the extension is compared
/// without case here, a `.PDF` being as much a PDF.
pub fn open_path(path: &str) -> Result<(), String> {
    let path = Path::new(path);
    let is_pdf = path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"));
    if !is_pdf {
        return Err(format!(
            "only PDF files are opened in another app: {}",
            path.display()
        ));
    }
    // A missing file is an error to report rather than a handler started on
    // nothing, as the plugin had it.
    path.metadata().map_err(|err| err.to_string())?;
    open::that_detached(path).map_err(|err| err.to_string())
}

/// Shows a file or a folder in the system's file manager, selected:
/// `revealItemInDir` in the window.
pub fn reveal_item_in_dir(path: &str) -> Result<(), String> {
    let path = dunce::canonicalize(path).map_err(|err| err.to_string())?;
    reveal(&path)
}

#[cfg(windows)]
fn reveal(path: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    // `/select,` takes the rest of the line as the path, quotes included, so
    // it is passed as it is rather than quoted again by `arg`. Explorer
    // answers 1 whether or not it found the file: only failing to start it is
    // an error.
    std::process::Command::new("explorer.exe")
        .raw_arg(format!("/select,\"{}\"", path.display()))
        .spawn()
        .map(drop)
        .map_err(|err| err.to_string())
}

#[cfg(target_os = "macos")]
fn reveal(path: &Path) -> Result<(), String> {
    let status = std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .status()
        .map_err(|err| err.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("Finder could not show {}", path.display()))
    }
}

/// The file manager's own D-Bus interface, as the plugin asked it, which
/// Nautilus, Dolphin, Nemo, Thunar and Caja answer; without one, the folder
/// opens without the file selected.
#[cfg(not(any(windows, target_os = "macos")))]
fn reveal(path: &Path) -> Result<(), String> {
    if show_items(path).is_ok() {
        return Ok(());
    }
    let folder: std::path::PathBuf = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent().map(Path::to_path_buf).unwrap_or_default()
    };
    open::that_detached(&folder).map_err(|err| err.to_string())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn show_items(path: &Path) -> zbus::Result<()> {
    let connection = zbus::blocking::Connection::session()?;
    connection.call_method(
        Some("org.freedesktop.FileManager1"),
        "/org/freedesktop/FileManager1",
        Some("org.freedesktop.FileManager1"),
        "ShowItems",
        &(vec![file_uri(path)], ""),
    )?;
    Ok(())
}

/// `file://` and the path, every byte but the unreserved ones and the
/// separators encoded, as a file manager expects a URI.
#[cfg(not(any(windows, target_os = "macos")))]
fn file_uri(path: &Path) -> String {
    use percent_encoding::{percent_encode, AsciiSet, NON_ALPHANUMERIC};
    use std::os::unix::ffi::OsStrExt;
    const KEPT: &AsciiSet = &NON_ALPHANUMERIC
        .remove(b'/')
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'~');
    format!(
        "file://{}",
        percent_encode(path.as_os_str().as_bytes(), KEPT)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_default_schemes_open() {
        for refused in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "vscode://open",
            "/usr/bin/xterm",
            "",
        ] {
            assert!(open_url(refused).is_err(), "{refused}");
        }
    }

    #[test]
    fn only_a_pdf_opens_in_another_app() {
        assert!(open_path("/tmp/run.sh").unwrap_err().contains("only PDF"));
        assert!(open_path("C:/tools/setup.exe")
            .unwrap_err()
            .contains("only PDF"));
        // A PDF that is not there is reported, not handed on.
        let missing = std::env::temp_dir().join("hangar-opener-missing.pdf");
        assert!(!open_path(&missing.to_string_lossy())
            .unwrap_err()
            .contains("only PDF"));
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn a_file_uri_encodes_what_a_uri_cannot_hold() {
        assert_eq!(
            file_uri(Path::new("/home/me/My notes, v2/plan#1.md")),
            "file:///home/me/My%20notes%2C%20v2/plan%231.md"
        );
    }
}
