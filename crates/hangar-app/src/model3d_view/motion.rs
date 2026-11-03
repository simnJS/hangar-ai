//! Whether the person asked the system for less motion
//! (`prefers-reduced-motion: reduce` in the web app): the 3D viewer's camera
//! then jumps back to the framed view on Recenter instead of gliding.
//!
//! A web page is told by its browser; a native window has to ask the desktop,
//! and each has its own way. It is read once, off the window's thread, from
//! the tools the desktop already has, so the app takes no dependency for it:
//!
//! - GNOME (and what follows its settings): `gsettings` has
//!   `org.gnome.desktop.interface enable-animations`, which is `false` when
//!   animations are turned off;
//! - macOS: `defaults` reads `com.apple.universalaccess reduceMotion`, `1`
//!   when Reduce motion is on;
//! - Windows: not read. Its setting is behind `SystemParametersInfo`, and this
//!   app has no Windows API bindings of its own to ask it with, so the camera
//!   glides there as it does where nothing says otherwise.
//!
//! Anything that fails to run, or answers in a way this does not know, is "no
//! preference": motion stays.

use std::process::{Command, Stdio};

/// What the system asks for, read now. Blocks on a process: call it where
/// blocking is fine.
pub fn reduced_motion() -> bool {
    if let Some(asked) = dev_override(std::env::var("HANGAR_DEV_REDUCED_MOTION").ok().as_deref()) {
        return asked;
    }
    if cfg!(target_os = "linux") {
        output_of(
            "gsettings",
            &["get", "org.gnome.desktop.interface", "enable-animations"],
        )
        .is_some_and(|out| animations_off(&out))
    } else if cfg!(target_os = "macos") {
        output_of(
            "defaults",
            &["read", "com.apple.universalaccess", "reduceMotion"],
        )
        .is_some_and(|out| reduce_motion_on(&out))
    } else {
        false
    }
}

/// A debug build can be told what the system says, to see both behaviours
/// without touching the desktop's settings: `HANGAR_DEV_REDUCED_MOTION=1` or
/// `=0`. A release build never looks at it.
fn dev_override(value: Option<&str>) -> Option<bool> {
    if !cfg!(debug_assertions) {
        return None;
    }
    match value?.trim() {
        "1" | "true" => Some(true),
        "0" | "false" => Some(false),
        _ => None,
    }
}

fn output_of(program: &str, arguments: &[&str]) -> Option<String> {
    let output = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `gsettings get … enable-animations`: `false` is "animations off".
pub fn animations_off(output: &str) -> bool {
    output.trim() == "false"
}

/// `defaults read … reduceMotion`: `1` is "Reduce motion on".
pub fn reduce_motion_on(output: &str) -> bool {
    output.trim() == "1"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gnome_turns_animations_off_with_a_false() {
        assert!(animations_off("false\n"));
        assert!(!animations_off("true\n"));
        // Anything else is no preference.
        assert!(!animations_off(""));
        assert!(!animations_off("No such schema"));
    }

    #[test]
    fn macos_reports_reduce_motion_as_one() {
        assert!(reduce_motion_on("1\n"));
        assert!(!reduce_motion_on("0\n"));
        assert!(!reduce_motion_on("The domain/default pair does not exist"));
    }

    #[test]
    fn a_debug_build_can_be_told_what_the_system_says() {
        assert_eq!(dev_override(None), None);
        assert_eq!(dev_override(Some("")), None);
        assert_eq!(dev_override(Some("maybe")), None);
        if cfg!(debug_assertions) {
            assert_eq!(dev_override(Some("1")), Some(true));
            assert_eq!(dev_override(Some("true")), Some(true));
            assert_eq!(dev_override(Some("0")), Some(false));
        } else {
            assert_eq!(dev_override(Some("1")), None);
        }
    }

    #[test]
    fn a_program_that_is_not_there_is_no_preference() {
        assert_eq!(output_of("hangar-no-such-program", &[]), None);
    }
}
