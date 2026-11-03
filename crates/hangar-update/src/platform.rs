//! Which build of a release is this process, and so which one replaces it.
//!
//! `tauri-plugin-updater` looked for `{os}-{arch}-{installer}` and then
//! `{os}-{arch}` in the feed, where the installer came from a marker the Tauri
//! bundler patched into the binary. A binary built by cargo carries no such
//! marker, so the installer is found by looking at how this process was
//! started instead: an AppImage says so in its environment, a package is
//! known to the package manager, a macOS app lives in a `.app`.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The operating system and CPU the feed's keys are built from, in the words
/// the plugin used: `linux`, `darwin` and `windows`; `x86_64`, `aarch64`,
/// `i686`, `armv7` and `riscv64`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub os: &'static str,
    pub arch: &'static str,
}

impl Target {
    pub const fn new(os: &'static str, arch: &'static str) -> Self {
        Self { os, arch }
    }

    /// The platform this binary was compiled for, or `None` for one the feed
    /// has no name for.
    pub fn current() -> Option<Self> {
        let os = if cfg!(target_os = "linux") {
            "linux"
        } else if cfg!(target_os = "macos") {
            // The plugin's own word for it; the feed's keys say `darwin`.
            "darwin"
        } else if cfg!(target_os = "windows") {
            "windows"
        } else {
            return None;
        };
        let arch = if cfg!(target_arch = "x86_64") {
            "x86_64"
        } else if cfg!(target_arch = "aarch64") {
            "aarch64"
        } else if cfg!(target_arch = "x86") {
            "i686"
        } else if cfg!(target_arch = "arm") {
            "armv7"
        } else if cfg!(target_arch = "riscv64") {
            "riscv64"
        } else {
            return None;
        };
        Some(Self { os, arch })
    }

    /// The keys to look for in `platforms`, most specific first: the build for
    /// this kind of install, then the platform's default. They are the plugin's
    /// rules, which is what keeps the feed of the old app working unchanged.
    pub fn keys(&self, install: Option<&Install>) -> Vec<String> {
        let base = format!("{}-{}", self.os, self.arch);
        let mut keys = Vec::with_capacity(2);
        if let Some(install) = install {
            keys.push(format!("{base}-{}", install.name()));
        }
        keys.push(base);
        keys
    }
}

/// How this copy of Hangar was installed, which decides both the build that
/// replaces it and the way it is put in place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Install {
    /// A single file the person runs; replaced where it stands.
    AppImage { path: PathBuf },
    /// Installed by `dpkg`; updated with `dpkg -i`, which needs administrator
    /// rights.
    Deb,
    /// Installed by `rpm`; updated with `rpm -U`, likewise.
    Rpm,
    /// An `.app` bundle, replaced as a directory.
    MacApp { bundle: PathBuf },
    /// The per-user NSIS installer; the new one runs in update mode.
    Nsis,
    /// The Windows Installer package.
    Msi,
}

impl Install {
    /// The suffix of the feed key, as the plugin spelled it.
    pub fn name(&self) -> &'static str {
        match self {
            Install::AppImage { .. } => "appimage",
            Install::Deb => "deb",
            Install::Rpm => "rpm",
            Install::MacApp { .. } => "app",
            Install::Nsis => "nsis",
            Install::Msi => "msi",
        }
    }

    /// The install of the running process, or `None` when it is not one that
    /// can update itself: a binary started from `target/`, a tarball unpacked
    /// by hand, an `.app` that is not where it should be.
    ///
    /// Not being able to tell is a "no" on purpose. The plugin fell back to
    /// treating an unknown Linux binary as an AppImage and overwrote it, which
    /// for a development build is the build the developer is standing on.
    pub fn detect() -> Option<Self> {
        let exe = std::env::current_exe().ok()?;
        let exe = strip_deleted_marker(exe);

        if cfg!(target_os = "linux") {
            let exe = exe.canonicalize().unwrap_or(exe);
            detect_linux(
                std::env::var_os("APPIMAGE").as_deref(),
                std::env::var_os("APPDIR").as_deref(),
                &exe,
                package_manager_owns,
            )
        } else if cfg!(target_os = "macos") {
            mac_bundle(&exe).map(|bundle| Install::MacApp { bundle })
        } else if cfg!(target_os = "windows") {
            // The NSIS installer is the one the feed's plain `windows-*` key
            // has always meant, and the one that is shipped. A build that
            // goes out as an MSI will need to say so (see docs in lib.rs).
            Some(Install::Nsis)
        } else {
            None
        }
    }
}

/// After a package upgrade replaces the running binary, Linux reports the old
/// file as `/usr/bin/hangar-ai (deleted)`. The new one is at the path without
/// the suffix, which is the one to start again.
pub(crate) fn strip_deleted_marker(path: PathBuf) -> PathBuf {
    match path.to_str().and_then(|p| p.strip_suffix(" (deleted)")) {
        Some(stripped) => PathBuf::from(stripped),
        None => path,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PackageManager {
    Dpkg,
    Rpm,
}

/// The Linux half of [`Install::detect`], with the environment and the package
/// databases passed in so the rules can be tested without either.
pub(crate) fn detect_linux(
    appimage: Option<&OsStr>,
    appdir: Option<&OsStr>,
    exe: &Path,
    owned_by: impl Fn(PackageManager, &Path) -> bool,
) -> Option<Install> {
    if let Some(appimage) = appimage.filter(|p| !p.is_empty()) {
        let path = PathBuf::from(appimage);
        // `APPIMAGE` is inherited by everything an AppImage starts, including
        // the shells in Hangar's own panes. A binary built and run from one of
        // them is not that AppImage, and must not replace it: the AppImage
        // runtime also sets `APPDIR` to where it mounted itself, and only a
        // process running from there is the AppImage.
        let inside = match appdir.filter(|p| !p.is_empty()) {
            Some(dir) => {
                let dir = Path::new(dir);
                exe.starts_with(dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf()))
            }
            None => true,
        };
        if inside && path.is_file() {
            return Some(Install::AppImage {
                // Replaced by renaming over it: through a symlink that would
                // swap the link for a file and leave the real one behind.
                path: path.canonicalize().unwrap_or(path),
            });
        }
    }

    if owned_by(PackageManager::Dpkg, exe) {
        Some(Install::Deb)
    } else if owned_by(PackageManager::Rpm, exe) {
        Some(Install::Rpm)
    } else {
        None
    }
}

/// Asks the package database whether it installed `exe`. A missing tool is a
/// "no", which is right: without `dpkg` nothing was installed by it.
fn package_manager_owns(manager: PackageManager, exe: &Path) -> bool {
    let mut command = match manager {
        PackageManager::Dpkg => {
            let mut c = Command::new("dpkg-query");
            c.arg("-S");
            c
        }
        PackageManager::Rpm => {
            let mut c = Command::new("rpm");
            c.arg("-qf");
            c
        }
    };
    command
        .arg(exe)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// `/Applications/Hangar.AI.app` out of
/// `/Applications/Hangar.AI.app/Contents/MacOS/hangar-ai`.
pub(crate) fn mac_bundle(exe: &Path) -> Option<PathBuf> {
    exe.ancestors()
        .find(|dir| dir.extension() == Some(OsStr::new("app")))
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn never(_: PackageManager, _: &Path) -> bool {
        false
    }

    #[test]
    fn keys_are_the_installer_then_the_platform() {
        let linux = Target::new("linux", "x86_64");
        let appimage = Install::AppImage { path: "/x".into() };
        assert_eq!(
            linux.keys(Some(&appimage)),
            ["linux-x86_64-appimage", "linux-x86_64"]
        );
        assert_eq!(
            linux.keys(Some(&Install::Deb)),
            ["linux-x86_64-deb", "linux-x86_64"]
        );
        assert_eq!(
            linux.keys(Some(&Install::Rpm)),
            ["linux-x86_64-rpm", "linux-x86_64"]
        );
        assert_eq!(linux.keys(None), ["linux-x86_64"]);

        let mac = Install::MacApp {
            bundle: "/Applications/Hangar.AI.app".into(),
        };
        assert_eq!(
            Target::new("darwin", "aarch64").keys(Some(&mac)),
            ["darwin-aarch64-app", "darwin-aarch64"]
        );
        assert_eq!(
            Target::new("darwin", "x86_64").keys(Some(&mac)),
            ["darwin-x86_64-app", "darwin-x86_64"]
        );

        let windows = Target::new("windows", "x86_64");
        assert_eq!(
            windows.keys(Some(&Install::Nsis)),
            ["windows-x86_64-nsis", "windows-x86_64"]
        );
        assert_eq!(
            windows.keys(Some(&Install::Msi)),
            ["windows-x86_64-msi", "windows-x86_64"]
        );
    }

    #[test]
    fn the_compiled_target_is_named_in_the_feeds_words() {
        let target = Target::current().expect("tests run on a platform the feed names");
        assert!(["linux", "darwin", "windows"].contains(&target.os));
        assert!(["x86_64", "aarch64", "i686", "armv7", "riscv64"].contains(&target.arch));
        if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            assert_eq!(target.keys(None), ["linux-x86_64"]);
        }
    }

    #[test]
    fn an_appimage_is_the_file_the_runtime_names() {
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("Hangar.AI.AppImage");
        std::fs::write(&image, b"x").unwrap();
        let mount = dir.path().join("mount");
        let exe = mount.join("usr/bin/hangar-ai");

        let found = detect_linux(
            Some(image.as_os_str()),
            Some(mount.as_os_str()),
            &exe,
            never,
        );
        assert_eq!(
            found,
            Some(Install::AppImage {
                path: image.canonicalize().unwrap()
            })
        );

        // The runtime's variables copied into a shell, running a binary of its own.
        let elsewhere = Path::new("/home/me/dev/hangar/target/debug/hangar-ai");
        assert_eq!(
            detect_linux(
                Some(image.as_os_str()),
                Some(mount.as_os_str()),
                elsewhere,
                never
            ),
            None
        );

        // `APPIMAGE` pointing at nothing is not an AppImage either.
        let gone = OsString::from("/nonexistent/Hangar.AI.AppImage");
        assert_eq!(
            detect_linux(Some(&gone), Some(mount.as_os_str()), &exe, never),
            None
        );
    }

    #[test]
    fn a_package_is_what_its_manager_says() {
        let exe = Path::new("/usr/bin/hangar-ai");
        let dpkg = |m: PackageManager, _: &Path| m == PackageManager::Dpkg;
        let rpm = |m: PackageManager, _: &Path| m == PackageManager::Rpm;
        assert_eq!(detect_linux(None, None, exe, dpkg), Some(Install::Deb));
        assert_eq!(detect_linux(None, None, exe, rpm), Some(Install::Rpm));
        assert_eq!(detect_linux(None, None, exe, never), None);
    }

    #[test]
    fn the_mac_bundle_is_the_app_directory_around_the_binary() {
        assert_eq!(
            mac_bundle(Path::new(
                "/Applications/Hangar.AI.app/Contents/MacOS/hangar-ai"
            )),
            Some(PathBuf::from("/Applications/Hangar.AI.app"))
        );
        assert_eq!(
            mac_bundle(Path::new("/Users/me/dev/hangar/target/debug/hangar-ai")),
            None
        );
    }

    #[test]
    fn a_replaced_binary_is_started_from_its_own_path() {
        assert_eq!(
            strip_deleted_marker(PathBuf::from("/usr/bin/hangar-ai (deleted)")),
            PathBuf::from("/usr/bin/hangar-ai")
        );
        assert_eq!(
            strip_deleted_marker(PathBuf::from("/usr/bin/hangar-ai")),
            PathBuf::from("/usr/bin/hangar-ai")
        );
    }
}
