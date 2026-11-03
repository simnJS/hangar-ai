//! The windows of the daemon (inventory §1.7): the main one, and one per
//! detached workspace.
//!
//! One `iced::daemon` holds one model for every window, and terminals and
//! PTYs live in the model, not in a window: detaching a workspace only changes
//! which window draws it. There is no handover, no replay and no cross-window
//! protocol, which is most of what `lib/windows.ts` was. What is left is in
//! app.rs (detach, reattach, the close request, reopening at boot) and here
//! (where a window opens, and the bounds it is remembered at).

use iced::window::{self, Position, Settings};
use iced::{Point, Size, Task};

use crate::message::Message;
use crate::state::WsId;
use crate::state::types::WindowBounds;

/// What a window draws.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WindowKind {
    /// The navigator, every workspace not detached, the settings.
    Main,
    /// One workspace, without navigator or settings.
    Workspace(WsId),
}

pub const MAIN_SIZE: Size = Size::new(1360.0, 860.0);
pub const MAIN_MIN_SIZE: Size = Size::new(900.0, 560.0);
pub const WORKSPACE_SIZE: Size = Size::new(1200.0, 800.0);
pub const WORKSPACE_MIN_SIZE: Size = Size::new(640.0, 420.0);

/// How long a workspace window stays still before its bounds are written
/// (App.tsx: 700 ms after a move or a resize).
pub const BOUNDS_SETTLE: std::time::Duration = std::time::Duration::from_millis(700);

/// Why a workspace's window is being opened, which decides what a window
/// that never opens undoes (App.tsx's `detach` and its reopening at boot).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opening {
    /// The person sent it there: it comes back to the main window, shown,
    /// with the `window.openFailed` notice.
    Detach,
    /// It was in a window of its own when the app last quit: it comes back
    /// into the main window quietly, as the TS reopening did.
    Boot,
}

/// How long a workspace window made by `open_workspace` has to report itself
/// open (`window::Event::Opened`) before it counts as one that failed.
///
/// iced has no answer for a window the system refuses: winit's error makes
/// iced 0.14 panic in its event loop ("Create window"), so that failure ends
/// the app before any code here could undo the detach. What is left to catch
/// is a window that never comes, and this is how long it is waited for. The
/// `Opened` event follows the window's creation within a frame, so ten
/// seconds is only ever reached by a window that is not coming; one that
/// still shows up afterwards is closed (`App::window_event`).
pub const OPEN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// The name the desktop knows the app's windows by, and their icon.
///
/// On Linux the application id is what a compositor matches against the
/// `.desktop` entry (`hangar-ai.desktop`, `StartupWMClass=hangar-ai`, see
/// docs/native/PACKAGING.md): without it the dock shows a generic icon and
/// never groups the windows under Hangar. The icon is the one the installers
/// carry, for the systems that take it from the window rather than from the
/// entry (X11, Windows' title bar).
fn base() -> Settings {
    #[allow(unused_mut)]
    let mut settings = Settings {
        icon: window::icon::from_file_data(
            include_bytes!("../../packaging/icons/128x128.png"),
            None,
        )
        .ok(),
        ..Settings::default()
    };
    #[cfg(target_os = "linux")]
    {
        settings.platform_specific.application_id = "hangar-ai".into();
    }
    settings
}

/// The main window, centred. Its close request is the app's end, which the
/// shell handles itself (core shutdown, last save) before exiting.
pub fn main_settings() -> Settings {
    Settings {
        size: MAIN_SIZE,
        min_size: Some(MAIN_MIN_SIZE),
        position: Position::Centered,
        exit_on_close_request: false,
        ..base()
    }
}

/// A workspace's own window, 1200×800 and centred when it has no bounds to
/// open at. Closing it brings the workspace back to the main window rather
/// than closing anything.
pub fn workspace_settings() -> Settings {
    Settings {
        size: WORKSPACE_SIZE,
        min_size: Some(WORKSPACE_MIN_SIZE),
        position: Position::Centered,
        exit_on_close_request: false,
        ..base()
    }
}

/// A workspace's window at the `bounds` it was last seen at (logical pixels,
/// as `windowBounds` stores them), when there are some.
///
/// The size is taken as saved, never below the minimum. The position only
/// when the window's title bar (half way across, 20 px down) lies on
/// `screen`, the size of the main window's screen: a position off every
/// screen would open a window nobody can see, and iced 0.14 names no other
/// screen to check against. A window last on a second screen therefore
/// opens centred; on Wayland, where a window cannot place itself, the
/// position is the compositor's anyway.
///
/// iced lays a window out at `size × scale` (the interface size), so the
/// size it is given is the saved one divided by `scale`.
pub fn workspace_settings_at(
    bounds: Option<WindowBounds>,
    screen: Option<Size>,
    scale: f32,
) -> Settings {
    let mut settings = workspace_settings();
    let Some(bounds) = bounds.filter(|b| b.width.is_finite() && b.height.is_finite()) else {
        return settings;
    };
    let scale = if scale > 0.0 { scale } else { 1.0 };
    settings.size = Size::new(
        (bounds.width as f32).max(WORKSPACE_MIN_SIZE.width) / scale,
        (bounds.height as f32).max(WORKSPACE_MIN_SIZE.height) / scale,
    );
    let title_bar = Point::new(
        (bounds.x + bounds.width / 2.0) as f32,
        (bounds.y + 20.0) as f32,
    );
    let on_screen = screen.is_some_and(|screen| {
        (0.0..=screen.width).contains(&title_bar.x) && (0.0..=screen.height).contains(&title_bar.y)
    });
    if on_screen && bounds.x.is_finite() && bounds.y.is_finite() {
        settings.position = Position::Specific(Point::new(bounds.x as f32, bounds.y as f32));
    }
    settings
}

/// The bounds to remember for a window seen at `position` with `size`, both
/// in iced's layout units: back to logical pixels by `scale`. A window that
/// never said where it is (Wayland sends no move) keeps the position it had.
pub fn bounds_of(
    position: Option<Point>,
    size: Size,
    scale: f32,
    before: Option<WindowBounds>,
) -> WindowBounds {
    let scale = f64::from(scale);
    let (x, y) = match position {
        Some(at) => (f64::from(at.x) * scale, f64::from(at.y) * scale),
        None => before.map_or((0.0, 0.0), |b| (b.x, b.y)),
    };
    WindowBounds {
        x: x.round(),
        y: y.round(),
        width: (f64::from(size.width) * scale).round(),
        height: (f64::from(size.height) * scale).round(),
    }
}

/// Opens the main window: its id now, for the model's window table, and the
/// task that opens it.
pub fn open_main() -> (window::Id, Task<Message>) {
    let (id, opened) = window::open(main_settings());
    (id, opened.discard())
}

/// Opens a workspace's window with `settings`.
pub fn open_workspace(settings: Settings) -> (window::Id, Task<Message>) {
    let (id, opened) = window::open(settings);
    (id, opened.discard())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(x: f64, y: f64) -> Option<WindowBounds> {
        Some(WindowBounds {
            x,
            y,
            width: 1000.0,
            height: 700.0,
        })
    }

    #[test]
    fn a_window_opens_where_it_was_when_that_is_on_screen() {
        let screen = Some(Size::new(1920.0, 1080.0));
        let settings = workspace_settings_at(bounds(100.0, 50.0), screen, 1.0);
        assert!(
            matches!(settings.position, Position::Specific(at) if at == Point::new(100.0, 50.0))
        );
        assert_eq!(settings.size, Size::new(1000.0, 700.0));
        // Off this screen: centred, at the size it had.
        let settings = workspace_settings_at(bounds(4000.0, 50.0), screen, 1.0);
        assert!(matches!(settings.position, Position::Centered));
        assert_eq!(settings.size, Size::new(1000.0, 700.0));
        // Unknown screen: centred too.
        assert!(matches!(
            workspace_settings_at(bounds(10.0, 10.0), None, 1.0).position,
            Position::Centered
        ));
    }

    #[test]
    fn without_bounds_a_window_is_1200_by_800_and_centred() {
        let settings = workspace_settings_at(None, Some(Size::new(1920.0, 1080.0)), 1.0);
        assert_eq!(settings.size, WORKSPACE_SIZE);
        assert!(matches!(settings.position, Position::Centered));
        assert_eq!(settings.min_size, Some(WORKSPACE_MIN_SIZE));
        assert!(!settings.exit_on_close_request);
    }

    #[test]
    fn the_interface_size_is_taken_out_and_put_back() {
        let settings =
            workspace_settings_at(bounds(0.0, 0.0), Some(Size::new(1920.0, 1080.0)), 1.25);
        assert_eq!(settings.size, Size::new(800.0, 560.0));
        let remembered = bounds_of(
            Some(Point::new(80.0, 40.0)),
            Size::new(800.0, 560.0),
            1.25,
            None,
        );
        assert_eq!(
            remembered,
            WindowBounds {
                x: 100.0,
                y: 50.0,
                width: 1000.0,
                height: 700.0
            }
        );
        // No position known: the last one stays.
        let kept = bounds_of(None, Size::new(800.0, 560.0), 1.25, bounds(7.0, 9.0));
        assert_eq!((kept.x, kept.y), (7.0, 9.0));
    }
}
