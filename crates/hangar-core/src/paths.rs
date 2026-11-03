//! Where Hangar keeps its files: the folders Tauri resolved for the app, to
//! the byte, so an install that ran the Tauri app finds its `state.json`,
//! `memory.json`, `remote.json`, voice models and the rest where they are.
//!
//! Tauri 2 named every app folder after the bundle identifier, under the
//! `dirs` crate's folder of that kind (tauri's `path/desktop.rs`, which
//! resolved them through `dirs` 6, as here):
//!
//! | folder          | Linux                          | macOS                                   | Windows                          |
//! |-----------------|--------------------------------|-----------------------------------------|----------------------------------|
//! | `app_data`      | `~/.local/share/<id>`          | `~/Library/Application Support/<id>`    | `%APPDATA%\<id>`                 |
//! | `app_config`    | `~/.config/<id>`               | `~/Library/Application Support/<id>`    | `%APPDATA%\<id>`                 |
//! | `app_local_data`| `~/.local/share/<id>`          | `~/Library/Application Support/<id>`    | `%LOCALAPPDATA%\<id>`            |
//! | `app_cache`     | `~/.cache/<id>`                | `~/Library/Caches/<id>`                 | `%LOCALAPPDATA%\<id>`            |
//! | `app_log`       | `~/.local/share/<id>/logs`     | `~/Library/Logs/<id>`                   | `%LOCALAPPDATA%\<id>\logs`       |
//!
//! (the XDG variables move the Linux ones, as they did for Tauri). Every file
//! the backend wrote went under `app_data`; the other four are here for the
//! app crate, which inherits whatever the window kept in them.
//!
//! Tauri resolved a folder each time it was asked; `Paths` resolves them once,
//! when the host is made, which comes to the same for a running app.

use std::path::{Path, PathBuf};

/// The bundle identifier of the Tauri app (`identifier` in
/// src-tauri/tauri.conf.json). Never to be changed: it names every folder
/// below, and the Windows notifications' AUMID (notify.rs).
pub const IDENTIFIER: &str = "com.simnjs.iabench";

/// What the person sees the app called: Tauri's `productName`, which is also
/// the name its login item is filed under (autostart.rs).
pub const PRODUCT_NAME: &str = "Hangar.AI";

/// A folder the system would not name — no home directory, most likely.
/// Worded as Tauri's `Error::UnknownPath`, which the messages that carry it
/// (`no app data dir: unknown path`) were written around.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnknownPath;

impl std::fmt::Display for UnknownPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("unknown path")
    }
}

impl std::error::Error for UnknownPath {}

/// The app's folders, resolved. Cheap to clone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    data: Option<PathBuf>,
    config: Option<PathBuf>,
    local_data: Option<PathBuf>,
    cache: Option<PathBuf>,
    log: Option<PathBuf>,
}

impl Paths {
    /// The folders Tauri used for this identifier on this system.
    pub fn system() -> Self {
        let app = |base: Option<PathBuf>| base.map(|dir| dir.join(IDENTIFIER));
        #[cfg(target_os = "macos")]
        let log = dirs::home_dir().map(|home| home.join("Library/Logs").join(IDENTIFIER));
        #[cfg(not(target_os = "macos"))]
        let log = dirs::data_local_dir().map(|dir| dir.join(IDENTIFIER).join("logs"));
        Self {
            data: app(dirs::data_dir()),
            config: app(dirs::config_dir()),
            local_data: app(dirs::data_local_dir()),
            cache: app(dirs::cache_dir()),
            log,
        }
    }

    /// `HANGAR_DATA_ROOT`'s folders when it is set and not empty, the
    /// system's otherwise.
    ///
    /// The native app is being written on machines where the released Hangar
    /// is installed and in use, and both read the same `state.json`: a build
    /// started to try something must not rewrite the person's workspaces. Set
    /// it with `HANGAR_ENDPOINT_FILE` (endpoint.rs), which moves the endpoint,
    /// the mods, the kits and the accounts beside it, and nothing of the
    /// installed app is touched.
    pub fn from_env() -> Self {
        match std::env::var_os("HANGAR_DATA_ROOT").filter(|value| !value.is_empty()) {
            Some(root) => Self::rooted(Path::new(&root)),
            None => Self::system(),
        }
    }

    /// Every folder under `root`, each by its own name: for tests, and for a
    /// build that must not touch the installed app's data.
    pub fn rooted(root: &Path) -> Self {
        Self {
            data: Some(root.join("data")),
            config: Some(root.join("config")),
            local_data: Some(root.join("local-data")),
            cache: Some(root.join("cache")),
            log: Some(root.join("logs")),
        }
    }

    /// `app_data_dir`: state.json, memory.json, remote.json, the voice models,
    /// the MCP scope files, the usage cache, the HQ folder.
    pub fn app_data_dir(&self) -> Result<PathBuf, UnknownPath> {
        self.data.clone().ok_or(UnknownPath)
    }

    pub fn app_config_dir(&self) -> Result<PathBuf, UnknownPath> {
        self.config.clone().ok_or(UnknownPath)
    }

    pub fn app_local_data_dir(&self) -> Result<PathBuf, UnknownPath> {
        self.local_data.clone().ok_or(UnknownPath)
    }

    pub fn app_cache_dir(&self) -> Result<PathBuf, UnknownPath> {
        self.cache.clone().ok_or(UnknownPath)
    }

    pub fn app_log_dir(&self) -> Result<PathBuf, UnknownPath> {
        self.log.clone().ok_or(UnknownPath)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_folder_is_named_after_the_tauri_identifier() {
        let paths = Paths::system();
        let expect = |base: Option<PathBuf>, got: Result<PathBuf, UnknownPath>| {
            assert_eq!(got.ok(), base.map(|dir| dir.join("com.simnjs.iabench")));
        };
        expect(dirs::data_dir(), paths.app_data_dir());
        expect(dirs::config_dir(), paths.app_config_dir());
        expect(dirs::data_local_dir(), paths.app_local_data_dir());
        expect(dirs::cache_dir(), paths.app_cache_dir());
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn the_logs_sit_in_the_local_data_folder_outside_macos() {
        assert_eq!(
            Paths::system().app_log_dir().ok(),
            dirs::data_local_dir().map(|dir| dir.join("com.simnjs.iabench").join("logs"))
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_logs_sit_in_library_logs_on_macos() {
        assert_eq!(
            Paths::system().app_log_dir().ok(),
            dirs::home_dir().map(|home| home.join("Library/Logs/com.simnjs.iabench"))
        );
    }

    #[test]
    fn an_unknown_folder_reads_as_tauri_s_error_did() {
        let none = Paths {
            data: None,
            config: None,
            local_data: None,
            cache: None,
            log: None,
        };
        assert_eq!(
            format!("no app data dir: {}", none.app_data_dir().unwrap_err()),
            "no app data dir: unknown path"
        );
    }
}
