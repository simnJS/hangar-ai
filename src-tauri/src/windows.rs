//! A workspace in a window of its own, for a second screen.
//!
//! The main window keeps the store and everything that writes `state.json`; a
//! workspace window is the same frontend, opened on `?workspace=<id>`, that
//! mirrors the store and draws that one workspace (see src/lib/windows.ts).
//! Its panes are not tied to the window: closing it hands them back to the
//! main window with their PTYs still running, which is why only the main
//! window's end tears the terminals down.

use serde::Deserialize;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, Position, Size, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

/// The label a workspace's window goes by.
pub fn label(workspace_id: &str) -> String {
    format!("workspace-{workspace_id}")
}

pub fn is_workspace_window(label: &str) -> bool {
    label.starts_with("workspace-")
}

/// Where the window last stood, in logical pixels, as the frontend saved it.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Workspace ids come from the frontend's own `newId`; anything else is not
/// one, and is refused before it turns into a label or a URL.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn reveal(window: &WebviewWindow) {
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
}

/// Saved bounds still worth using: on a screen that is connected right now.
/// A window restored onto a monitor that has since been unplugged would open
/// somewhere nobody can reach.
fn on_a_screen(app: &AppHandle, bounds: &Bounds) -> bool {
    let Ok(monitors) = app.available_monitors() else {
        return false;
    };
    monitors.iter().any(|monitor| {
        let scale = monitor.scale_factor();
        let origin = monitor.position().to_logical::<f64>(scale);
        let size = monitor.size().to_logical::<f64>(scale);
        // The title bar has to be on that screen, not just a corner of the window.
        let (cx, cy) = (bounds.x + bounds.width / 2.0, bounds.y + 20.0);
        cx >= origin.x
            && cx < origin.x + size.width
            && cy >= origin.y
            && cy < origin.y + size.height
    })
}

/// A first-time placement: centred on a screen other than the main window's,
/// when there is one — that is what the window is for — at most 1360×860.
fn second_screen(app: &AppHandle) -> Option<(Position, Size)> {
    let main = app.get_webview_window("main")?;
    let here = main.current_monitor().ok().flatten();
    let other = app.available_monitors().ok()?.into_iter().find(|monitor| {
        here.as_ref()
            .is_none_or(|here| here.position() != monitor.position())
    })?;
    let scale = other.scale_factor();
    let area = other.size().to_logical::<f64>(scale);
    let width = (area.width * 0.85).min(1360.0);
    let height = (area.height * 0.85).min(860.0);
    let origin = other.position();
    let x = origin.x + ((area.width - width) / 2.0 * scale) as i32;
    let y = origin.y + ((area.height - height) / 2.0 * scale) as i32;
    Some((
        Position::Physical(PhysicalPosition { x, y }),
        Size::Logical(tauri::LogicalSize { width, height }),
    ))
}

/// Opens the window of a workspace, or brings it forward when it is already
/// open.
#[tauri::command]
pub async fn open_workspace_window(
    app: AppHandle,
    workspace_id: String,
    title: String,
    bounds: Option<Bounds>,
) -> Result<(), String> {
    if !valid_id(&workspace_id) {
        return Err(format!("'{workspace_id}' is not a workspace id"));
    }
    let label = label(&workspace_id);
    if let Some(window) = app.get_webview_window(&label) {
        reveal(&window);
        return Ok(());
    }

    let url = WebviewUrl::App(format!("index.html?workspace={workspace_id}").into());
    let mut builder = WebviewWindowBuilder::new(&app, &label, url)
        .title(format!("{title} — Hangar.AI"))
        .min_inner_size(640.0, 420.0);

    let saved = bounds.filter(|bounds| on_a_screen(&app, bounds));
    let placement = if saved.is_none() {
        second_screen(&app)
    } else {
        None
    };
    builder = match (saved, placement) {
        (Some(bounds), _) => builder
            .position(bounds.x, bounds.y)
            .inner_size(bounds.width, bounds.height),
        (None, Some((_, Size::Logical(size)))) => builder.inner_size(size.width, size.height),
        _ => builder.inner_size(1200.0, 800.0).center(),
    };

    let window = builder.build().map_err(|e| e.to_string())?;
    if let (None, Some((position, _))) = (saved, placement) {
        // Placed after creation and in physical pixels: the builder's
        // position is logical, and logical coordinates on a second screen with
        // its own scale factor land somewhere else.
        let _ = window.set_position(position);
    }
    reveal(&window);
    Ok(())
}

#[tauri::command]
pub fn focus_workspace_window(app: AppHandle, workspace_id: String) -> bool {
    match app.get_webview_window(&label(&workspace_id)) {
        Some(window) => {
            reveal(&window);
            true
        }
        None => false,
    }
}

/// Closes the window for good, once its frontend has handed the panes back.
#[tauri::command]
pub fn close_workspace_window(app: AppHandle, workspace_id: String) {
    if let Some(window) = app.get_webview_window(&label(&workspace_id)) {
        let _ = window.destroy();
    }
    if let Some(main) = app.get_webview_window("main") {
        reveal(&main);
    }
}

/// The close button of a workspace window: the frontend gets to hand its
/// panes back to the main window first, and closes the window itself through
/// `close_workspace_window`. A frontend too broken to answer does not get to
/// keep the window open — it is destroyed after a few seconds regardless.
pub fn on_close_requested(app: &AppHandle, label: &str) {
    let _ = app.emit_to(label, "window:close-requested", ());
    let app = app.clone();
    let label = label.to_string();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(4));
        if let Some(window) = app.get_webview_window(&label) {
            let _ = window.destroy();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_frontend_ids_become_labels() {
        assert!(valid_id("8f3c2a1e-4b5d-4c6e-9f7a-0b1c2d3e4f5a"));
        assert!(!valid_id(""));
        assert!(!valid_id("../evil"));
        assert!(!valid_id("a?b=c"));
        assert!(is_workspace_window(&label("abc")));
        assert!(!is_workspace_window("main"));
    }
}
