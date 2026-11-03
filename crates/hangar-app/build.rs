//! Gives the Windows executable what `tauri-build` used to: its icon and its
//! version information.
//!
//! Without a resource the file Explorer, the taskbar before a window exists, the
//! shortcuts the installer makes and the "Details" tab of the file's properties
//! all show a blank icon and no product name. Nothing else happens here: the
//! installers are made by the release workflow (docs/native/PACKAGING.md), not
//! by cargo, and Hangar Mobile's `dist-mobile/` is watched by hangar-core's own
//! build script, where it is embedded.

fn main() {
    // Without a rerun-if line of its own, Cargo would rerun this script
    // whenever any file of the crate changes.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=packaging/icons/icon.ico");

    // The script runs on the machine building, and the resource is for the
    // target: only a Windows build gets one.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        windows_resource();
    }
}

#[cfg(windows)]
fn windows_resource() {
    let version = std::env::var("CARGO_PKG_VERSION").expect("cargo sets the package version");

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("packaging/icons/icon.ico");
    // The same words the Tauri build wrote from `tauri.conf.json`, which is
    // what the NSIS installer and the old uninstall entry already call the app.
    resource.set("ProductName", "Hangar.AI");
    resource.set("FileDescription", "Hangar.AI");
    resource.set("CompanyName", "simnjs");
    resource.set("OriginalFilename", "hangar-ai.exe");
    resource.set("ProductVersion", &version);
    resource.set("FileVersion", &version);
    resource
        .compile()
        .expect("embedding the Windows icon and version information");
}

/// Building for Windows from another system would need a cross resource
/// compiler (`windres`); the releases are built on Windows, so this is only
/// reached by someone who has not got one, and the build still works without
/// the resource.
#[cfg(not(windows))]
fn windows_resource() {
    println!(
        "cargo:warning=the Windows icon and version information are only embedded when building on Windows"
    );
}
