use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager};

/// Bundle identifiers this app has shipped under before.
///
/// The app data directory is keyed by the identifier, so renaming the product
/// moves it. Without this list an existing install reopens empty and its
/// workspaces stay stranded in a folder nothing reads any more.
const PREVIOUS_IDENTIFIERS: &[&str] = &["com.simnjs.iabench", "com.simnjs.hangar-ai"];

/// Brings over the state left behind by a previous identifier. Only ever runs
/// when the current identifier has no state of its own, so it cannot overwrite
/// anything, and the old file is left in place as a fallback.
fn adopt_previous_state(path: &Path) {
    let Some(root) = path.parent().and_then(Path::parent) else {
        return;
    };
    for previous in PREVIOUS_IDENTIFIERS {
        let old = root.join(previous).join("state.json");
        if old.is_file() && fs::copy(&old, path).is_ok() {
            return;
        }
    }
}

/// Moves an unreadable state file out of the way, under a name no rescue
/// already holds, and returns where it went.
///
/// The name has to be unique: a fixed `state.json.corrupt` looks safe but only
/// survives one failure. The frontend starts empty after a failed load and
/// saves that empty state moments later, so the second corruption is of a file
/// worth nothing — and renaming it over the first rescue is what actually
/// loses the workspaces. The timestamp keeps every rescue (seconds since the
/// epoch, which needs no date library); the counter covers two failures inside
/// the same second.
///
/// `pub(crate)` because memory.rs rescues its own unparseable file with the
/// exact same move — one implementation, one behavior.
pub(crate) fn quarantine(path: &Path) -> Option<PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let mut target = path.with_extension(format!("json.corrupt.{stamp}"));
    let mut nth = 1;
    while target.exists() && nth < 100 {
        target = path.with_extension(format!("json.corrupt.{stamp}-{nth}"));
        nth += 1;
    }
    if target.exists() {
        return None;
    }
    if fs::rename(path, &target).is_ok() {
        return Some(target);
    }
    // A rename can still be refused while the file is held open by something
    // else — an antivirus or an indexer, on Windows. Copying leaves the
    // unreadable original where it is, but it does get a snapshot out of reach
    // of the empty save that follows, which is the whole point.
    fs::copy(path, &target).ok().map(|_| target)
}

/// The frontend owns the state schema; Rust only persists it atomically.
fn state_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("no app data dir: {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("state.json"))
}

#[tauri::command]
pub fn load_state(app: AppHandle) -> Result<Option<serde_json::Value>, String> {
    let path = state_path(&app)?;
    if !path.exists() {
        adopt_previous_state(&path);
    }
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    match serde_json::from_str(&raw) {
        Ok(value) => Ok(Some(value)),
        Err(err) => {
            // The frontend starts empty when the load fails and saves that
            // empty state moments later, so leaving the unreadable file in
            // place would destroy every workspace it still describes. Move it
            // aside first: the save lands on a fresh file and the old one is
            // still there to be repaired by hand.
            //
            // A rescue that could not be written is worth reporting, not worth
            // refusing to start over: either way the frontend gets an error and
            // opens empty, which is all it knows how to do here anyway.
            Err(match quarantine(&path) {
                Some(target) => format!(
                    "state.json is unreadable ({err}); kept as {}",
                    target.display()
                ),
                None => format!("state.json is unreadable ({err}); it could not be moved aside"),
            })
        }
    }
}

#[tauri::command]
pub fn save_state(app: AppHandle, state: serde_json::Value) -> Result<(), String> {
    let path = state_path(&app)?;
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_string_pretty(&state).map_err(|e| e.to_string())?;
    // Write-then-rename so a crash mid-save cannot truncate the real file.
    fs::write(&tmp, body).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn dir_exists(path: String) -> bool {
    std::path::Path::new(&path).is_dir()
}

#[tauri::command]
pub fn home_dir() -> String {
    dirs::home_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}
