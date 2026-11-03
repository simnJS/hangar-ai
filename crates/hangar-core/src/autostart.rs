//! Hangar at login: an option of the Mobile settings (remote/), so that a PC
//! that restarts during a week away is reachable again without anyone there.
//!
//! The Tauri app had `tauri-plugin-autostart` 2.5.1 do it, which is a thin
//! wrapper over the `auto-launch` crate. Here is the same crate at the same
//! version, given what the plugin gave it, so the login item an earlier
//! version made is the one found, kept, or taken out — not a second one beside
//! it:
//!
//! - the name, Tauri's `productName`: the registry value under `…\Run` on
//!   Windows, `~/Library/LaunchAgents/Hangar.AI.plist` on macOS (a launch
//!   agent, the plugin's `MacosLauncher::LaunchAgent`), and
//!   `~/.config/autostart/Hangar.AI.desktop` on Linux;
//! - the executable as it runs now — canonical on macOS, the AppImage rather
//!   than its mounted copy when there is one on Linux — with no arguments.

use std::sync::OnceLock;

use auto_launch::{AutoLaunch, AutoLaunchBuilder};

/// The login item, made once: what the plugin built in its `setup`. A
/// failure is kept too, and given to every caller, since asking again would
/// fail the same way.
fn launcher() -> Result<&'static AutoLaunch, String> {
    static LAUNCHER: OnceLock<Result<AutoLaunch, String>> = OnceLock::new();
    LAUNCHER.get_or_init(build).as_ref().map_err(Clone::clone)
}

fn build() -> Result<AutoLaunch, String> {
    let mut builder = AutoLaunchBuilder::new();
    builder.set_app_name(crate::paths::PRODUCT_NAME);
    builder.set_args(&[] as &[&str]);
    builder.set_app_path(&app_path()?);
    #[cfg(target_os = "macos")]
    builder.set_use_launch_agent(true);
    builder.build().map_err(|err| err.to_string())
}

/// What the item starts, as the plugin worked it out.
fn app_path() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    #[cfg(target_os = "macos")]
    let exe = exe.canonicalize().map_err(|err| err.to_string())?;
    // An AppImage runs from a mount that changes at every launch: the item has
    // to start the image itself, which the runtime names in `APPIMAGE`.
    #[cfg(target_os = "linux")]
    if let Some(image) = std::env::var_os("APPIMAGE").and_then(|image| image.into_string().ok()) {
        return Ok(image);
    }
    Ok(exe.display().to_string())
}

/// Whether Hangar starts at login.
pub fn is_enabled() -> Result<bool, String> {
    launcher()?.is_enabled().map_err(|err| err.to_string())
}

/// Makes Hangar start at login, or stop doing so.
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let launcher = launcher()?;
    if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    }
    .map_err(|err| err.to_string())
}
