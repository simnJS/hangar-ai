//! Desktop notifications, and the ones you can click.
//!
//! `tauri-plugin-notification` could put a toast on screen but not react to it
//! being clicked, and a pane that just handed back control is exactly the thing
//! worth going back to. So a notification that names a pane is raised here
//! instead, carrying a handler that tells the window which pane to reveal
//! (`Event::NotificationActivated`); bringing the window up is the window's.
//!
//! Windows only. Everywhere else `notify_send` fails on purpose and the window
//! falls back to `show`, the plain notification the plugin used to show: one
//! without the click is worth more than no notification.

use serde::Serialize;

#[cfg(windows)]
use crate::host::Event;
use crate::host::Host;

/// Which pane a click on the toast has to bring back: `Event::NotificationActivated`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Activated {
    pub workspace_id: String,
    pub pane_id: String,
}

pub fn notify_send(
    host: &Host,
    title: String,
    body: String,
    workspace_id: String,
    pane_id: String,
) -> Result<(), String> {
    send(host, title, body, workspace_id, pane_id)
}

#[cfg(windows)]
fn send(
    host: &Host,
    title: String,
    body: String,
    workspace_id: String,
    pane_id: String,
) -> Result<(), String> {
    use tauri_winrt_notification::Toast;

    let host = host.clone();
    Toast::new(&app_id())
        .title(&title)
        .text1(&body)
        .on_activated(move |_| {
            // WinRT calls this from its own thread pool. The window, which
            // hears the event on its own thread, shows itself — or the
            // workspace's own window when it has one — and the pane: every
            // window operation has to happen on its event loop anyway.
            let _ = host.emit(Event::NotificationActivated(Activated {
                workspace_id: workspace_id.clone(),
                pane_id: pane_id.clone(),
            }));
            Ok(())
        })
        .show()
        .map_err(|e| e.to_string())
}

/// The AUMID the toast is filed under.
///
/// Windows only honours an AUMID that a Start Menu shortcut has registered, so
/// the bundle identifier is only usable out of an installed build — passing it
/// from a `cargo`-run binary loses the toast. `tauri-plugin-notification` drew
/// the same line, and by the same test: the executable sitting in `target/debug`
/// or `target/release` means "not installed". The fallback is PowerShell's own
/// AUMID, which is registered on every machine, so in development the toast
/// shows up attributed to Windows PowerShell. Clicking it still works — the
/// handler is wired to the notification object in this process, not to the
/// AUMID.
#[cfg(windows)]
fn app_id() -> String {
    use std::path::MAIN_SEPARATOR as SEP;
    use tauri_winrt_notification::Toast;

    let installed = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.display().to_string()))
        .map(|dir| {
            !(dir.ends_with(&format!("{SEP}target{SEP}debug"))
                || dir.ends_with(&format!("{SEP}target{SEP}release")))
        })
        .unwrap_or(false);

    if installed {
        crate::paths::IDENTIFIER.to_string()
    } else {
        Toast::POWERSHELL_APP_ID.to_string()
    }
}

#[cfg(not(windows))]
fn send(
    _host: &Host,
    _title: String,
    _body: String,
    _workspace_id: String,
    _pane_id: String,
) -> Result<(), String> {
    Err("clickable notifications are only implemented on Windows".into())
}

/// A plain notification, nothing to click: what the window showed through
/// `tauri-plugin-notification`'s `sendNotification({ title, body })`, for a
/// notification that names no pane, and for one `notify_send` could not raise.
///
/// Shown from a thread of its own, as the plugin did, and without a word back:
/// a notification server that is not there costs the notification, never the
/// caller. The plugin's permission prompt answered "granted" on every desktop,
/// so there is none to ask.
pub fn show(title: String, body: String) {
    std::thread::spawn(move || show_now(&title, &body));
}

#[cfg(windows)]
fn show_now(title: &str, body: &str) {
    // What notify-rust, under the plugin, drew on Windows: this toast, filed
    // under the identifier once installed.
    let _ = tauri_winrt_notification::Toast::new(&app_id())
        .title(title)
        .text1(body)
        .show();
}

#[cfg(not(windows))]
fn show_now(title: &str, body: &str) {
    // The plugin's way: the app's own icon, and on macOS the notification
    // filed under the bundle, or under Terminal for a development build,
    // which has no bundle to be filed under (Tauri's `is_dev`, a debug build
    // here). `set_application` takes the first name it is given, so the
    // later calls are refused, harmlessly.
    #[cfg(target_os = "macos")]
    let _ = notify_rust::set_application(if cfg!(debug_assertions) {
        "com.apple.Terminal"
    } else {
        crate::paths::IDENTIFIER
    });
    let _ = notify_rust::Notification::new()
        .summary(title)
        .body(body)
        .auto_icon()
        .show();
}
