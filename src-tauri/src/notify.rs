//! Desktop notifications you can click.
//!
//! `tauri-plugin-notification` can put a toast on screen but not react to it
//! being clicked, and a pane that just handed back control is exactly the thing
//! worth going back to. So a notification that names a pane is raised here
//! instead, carrying a handler that brings the window up and tells the frontend
//! which pane to reveal.
//!
//! Windows only. Everywhere else the command fails on purpose and the frontend
//! falls back to the plugin: a notification without the click is worth more than
//! no notification.

/// Which pane a click on the toast has to bring back.
#[cfg(windows)]
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Activated {
    workspace_id: String,
    pane_id: String,
}

#[cfg(windows)]
const ACTIVATED_EVENT: &str = "notification-activated";

#[tauri::command]
pub fn notify_send(
    app: tauri::AppHandle,
    title: String,
    body: String,
    workspace_id: String,
    pane_id: String,
) -> Result<(), String> {
    send(app, title, body, workspace_id, pane_id)
}

#[cfg(windows)]
fn send(
    app: tauri::AppHandle,
    title: String,
    body: String,
    workspace_id: String,
    pane_id: String,
) -> Result<(), String> {
    use tauri_winrt_notification::Toast;

    let handle = app.clone();
    Toast::new(&app_id(&app))
        .title(&title)
        .text1(&body)
        .on_activated(move |_| {
            // WinRT calls this from its own thread pool, and every window
            // operation below has to happen on the event loop anyway: hand the
            // whole thing over and let the notification thread go.
            let target = Activated {
                workspace_id: workspace_id.clone(),
                pane_id: pane_id.clone(),
            };
            let app = handle.clone();
            let _ = handle.run_on_main_thread(move || reveal(&app, target));
            Ok(())
        })
        .show()
        .map_err(|e| e.to_string())
}

/// Bring the window forward and point the frontend at the pane.
#[cfg(windows)]
fn reveal(app: &tauri::AppHandle, target: Activated) {
    use tauri::{Emitter, Manager, UserAttentionType};

    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        // Windows hands the foreground to whichever process owns the current
        // input, and refuses it to anyone else — silently, so `set_focus`
        // reporting success proves nothing and the window has to be asked
        // whether it actually got there. When it did not, flashing the taskbar
        // button is as far as the shell will let us go.
        let focused = window.set_focus().is_ok() && window.is_focused().unwrap_or(false);
        if !focused {
            let _ = window.request_user_attention(Some(UserAttentionType::Critical));
        }
    }

    let _ = app.emit(ACTIVATED_EVENT, target);
}

/// The AUMID the toast is filed under.
///
/// Windows only honours an AUMID that a Start Menu shortcut has registered, so
/// the bundle identifier is only usable out of an installed build — passing it
/// from a `cargo`-run binary loses the toast. `tauri-plugin-notification` draws
/// the same line, and by the same test: the executable sitting in `target/debug`
/// or `target/release` means "not installed". The fallback is PowerShell's own
/// AUMID, which is registered on every machine, so in development the toast
/// shows up attributed to Windows PowerShell. Clicking it still works — the
/// handler is wired to the notification object in this process, not to the
/// AUMID.
#[cfg(windows)]
fn app_id(app: &tauri::AppHandle) -> String {
    use std::path::MAIN_SEPARATOR as SEP;
    use tauri_winrt_notification::Toast;

    let installed = tauri::utils::platform::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.display().to_string()))
        .map(|dir| {
            !(dir.ends_with(&format!("{SEP}target{SEP}debug"))
                || dir.ends_with(&format!("{SEP}target{SEP}release")))
        })
        .unwrap_or(false);

    if installed {
        app.config().identifier.clone()
    } else {
        Toast::POWERSHELL_APP_ID.to_string()
    }
}

#[cfg(not(windows))]
fn send(
    _app: tauri::AppHandle,
    _title: String,
    _body: String,
    _workspace_id: String,
    _pane_id: String,
) -> Result<(), String> {
    Err("clickable notifications are only implemented on Windows".into())
}
