//! What the session the helper runs in lets a window do, worked out without
//! starting the helper: the app has to know it before it decides which windows
//! to open by itself, and the helper is only started for the first window.

/// Whether a window can be opened without taking the keyboard: `focus: false`
/// in [`OpenWindow`](crate::OpenWindow).
///
/// It can where a window system lets a program say so when it maps a window:
/// X11 (the window's input hint), Windows and macOS. It cannot on Wayland. The
/// protocol has no such request, and the compositor decides alone. Observed on
/// GNOME Shell 50 (Mutter): a window mapped by a program that has no focus is
/// given the keyboard at once, from the window that held it, whatever the
/// program asked. Hiding a window and showing it again does the same.
///
/// So the answer is "no" on a Wayland session, and the app does not open a
/// window there for something an agent showed: it offers the window and waits
/// for the person (a click is the one thing a Wayland window system takes as a
/// reason to give the keyboard). The answer follows the backend GTK will pick,
/// which the helper inherits from the app's environment: `GDK_BACKEND` when it
/// is set, a Wayland display otherwise.
pub fn opens_unfocused() -> bool {
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let backend = std::env::var("GDK_BACKEND").ok();
        let display = std::env::var("WAYLAND_DISPLAY").ok();
        !wayland_session(backend.as_deref(), display.as_deref())
    }
    #[cfg(any(windows, target_os = "macos"))]
    {
        true
    }
}

/// Which backend GTK 3 picks: the first one named in `GDK_BACKEND` (a comma
/// list, `*` meaning whichever works), and with none named, Wayland whenever
/// there is a Wayland display to connect to.
#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
fn wayland_session(gdk_backend: Option<&str>, wayland_display: Option<&str>) -> bool {
    fn set(value: Option<&str>) -> Option<&str> {
        value.map(str::trim).filter(|value| !value.is_empty())
    }
    match set(gdk_backend) {
        Some(list) => list.split(',').next().map(str::trim) == Some("wayland"),
        None => set(wayland_display).is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::wayland_session;

    #[test]
    fn a_wayland_display_means_wayland_unless_the_backend_is_named() {
        assert!(wayland_session(None, Some("wayland-0")));
        assert!(!wayland_session(None, None));
        assert!(!wayland_session(None, Some("")));
        assert!(!wayland_session(None, Some("  ")));
        // The person's own choice wins, as it does for GTK.
        assert!(!wayland_session(Some("x11"), Some("wayland-0")));
        assert!(wayland_session(Some("wayland"), None));
        // The first of a list is the one tried first.
        assert!(!wayland_session(Some("x11,wayland"), Some("wayland-0")));
        assert!(wayland_session(Some("wayland, x11"), Some("wayland-0")));
        // An empty one names nothing.
        assert!(wayland_session(Some(""), Some("wayland-0")));
    }
}
