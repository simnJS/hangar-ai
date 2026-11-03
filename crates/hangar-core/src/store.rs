use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::host::Host;

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
fn state_path(host: &Host) -> Result<PathBuf, String> {
    let dir = host
        .paths()
        .app_data_dir()
        .map_err(|e| format!("no app data dir: {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("state.json"))
}

/// What became of a `state.json` that could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rescue {
    /// Moved (or copied) out of the way under this name: the next save lands
    /// on a fresh file, and the old one is there to be repaired by hand.
    KeptAs(PathBuf),
    /// It could not be moved: it stays where it is, and `save_state` refuses
    /// to write over it until it can be moved (or someone else moved it).
    Held,
    /// There was no file to keep: the app's folder itself could not be had.
    NoFile,
}

/// Why `state.json` could not be used, and what became of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadError {
    /// What went wrong, as the system or the parser said it.
    pub reason: String,
    pub rescue: Rescue,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let reason = &self.reason;
        match &self.rescue {
            Rescue::KeptAs(target) => write!(
                f,
                "state.json is unreadable ({reason}); kept as {}",
                target.display()
            ),
            Rescue::Held => write!(
                f,
                "state.json is unreadable ({reason}); it could not be moved aside, \
                 and nothing will be saved over it"
            ),
            Rescue::NoFile => f.write_str(reason),
        }
    }
}

/// The state files that could neither be read nor moved aside: `save_state`
/// must not write over them (`release_hold`). A process-wide list, keyed by
/// path, because the window has one state file and each test its own.
static HELD: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

fn held() -> MutexGuard<'static, Vec<PathBuf>> {
    HELD.lock().unwrap_or_else(PoisonError::into_inner)
}

/// How many times a read that failed is tried before the file is given up
/// on, and how long apart. A read can fail for a moment: an antivirus or a
/// backup tool holding the file on Windows, a network home that hiccups. The
/// file is only set aside once it has failed to read every time.
const READ_ATTEMPTS: u32 = 3;
const READ_RETRY: Duration = Duration::from_millis(100);

/// `load_state`, with its error as words.
pub fn load_state(host: &Host) -> Result<Option<serde_json::Value>, String> {
    load_state_checked(host).map_err(|err| err.to_string())
}

/// The raw JSON of `state.json`; `None` when there is no file yet.
///
/// The window starts empty when the load fails and saves that empty state
/// moments later, so a file left in place would have every workspace it
/// still describes written over. Whatever stops it from being used is
/// treated alike, then: a read the system refuses (permissions, an I/O
/// error), bytes that are not UTF-8, JSON that does not parse, a document
/// whose root is not an object (every state ever written is one). The file
/// is moved aside first (`Rescue::KeptAs`), so the save lands on a fresh
/// one. When even that fails, it is held (`Rescue::Held`): `save_state`
/// writes nothing over it until it can be moved.
pub fn load_state_checked(host: &Host) -> Result<Option<serde_json::Value>, LoadError> {
    let path = state_path(host).map_err(|reason| LoadError {
        reason,
        rescue: Rescue::NoFile,
    })?;
    if !path.exists() {
        adopt_previous_state(&path);
    }
    if !path.exists() {
        return Ok(None);
    }
    let Some(raw) = read_text(&path).map_err(|reason| set_aside(&path, reason))? else {
        return Ok(None);
    };
    // Notepad and a few other editors start a UTF-8 file with a byte order
    // mark, which serde_json refuses: a state.json edited by hand would
    // otherwise be set aside for one invisible character.
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
    let mut value: serde_json::Value =
        serde_json::from_str(raw).map_err(|err| set_aside(&path, err.to_string()))?;
    if !value.is_object() {
        return Err(set_aside(&path, "its content is not an object".into()));
    }
    // Rewritten at once rather than left to the frontend's next save, so the
    // plaintext copy is gone even if the app closes before then. A failed
    // rewrite costs nothing more: the value handed back no longer carries the
    // key, and that save drops it from the file.
    if crate::voice::key::migrate(&mut value) {
        let _ = write_state(&path, &value);
    }
    Ok(Some(value))
}

/// The file's text, tried a few times; `None` when it went away meanwhile.
fn read_text(path: &Path) -> Result<Option<String>, String> {
    let mut attempt = 1;
    let bytes = loop {
        match fs::read(path) {
            Ok(bytes) => break bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) if attempt < READ_ATTEMPTS => {
                attempt += 1;
                std::thread::sleep(READ_RETRY);
            }
            Err(err) => return Err(err.to_string()),
        }
    };
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|err| format!("it is not UTF-8 text: {err}"))
}

/// The window could not use what `load_state` handed it, for `reason` (a
/// document its own reader turned down): the file is set aside as an
/// unreadable one is, or held when it cannot be.
pub fn set_aside_state(host: &Host, reason: String) -> LoadError {
    match state_path(host) {
        Ok(path) if path.exists() => set_aside(&path, reason),
        _ => LoadError {
            reason,
            rescue: Rescue::NoFile,
        },
    }
}

fn set_aside(path: &Path, reason: String) -> LoadError {
    match quarantine(path) {
        Some(target) => LoadError {
            reason,
            rescue: Rescue::KeptAs(target),
        },
        None => {
            let mut held = held();
            if !held.iter().any(|p| p == path) {
                held.push(path.to_path_buf());
            }
            LoadError {
                reason,
                rescue: Rescue::Held,
            }
        }
    }
}

/// A file held at load is moved aside before the first save goes over it.
/// While it still cannot be, nothing is saved; once it can, or once it is
/// gone (the person moved it), saving goes on as usual.
fn release_hold(path: &Path) -> Result<(), String> {
    let mut held = held();
    let Some(at) = held.iter().position(|p| p == path) else {
        return Ok(());
    };
    if path.exists() && quarantine(path).is_none() {
        return Err(
            "state.json could not be read when Hangar.AI started, and it still cannot be \
             moved aside"
                .into(),
        );
    }
    held.remove(at);
    Ok(())
}

pub fn save_state(host: &Host, state: serde_json::Value) -> Result<(), String> {
    let path = state_path(host)?;
    release_hold(&path)?;
    write_state(&path, &state)
}

fn write_state(path: &Path, state: &serde_json::Value) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    // Write-then-rename so a crash mid-save cannot truncate the real file.
    fs::write(&tmp, body).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

pub fn dir_exists(path: String) -> bool {
    std::path::Path::new(&path).is_dir()
}

pub fn home_dir() -> String {
    dirs::home_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;
    use serde_json::json;

    /// A host on a folder of its own, and where its state file is.
    fn host(tag: &str) -> (Host, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "hangar-store-{tag}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let host = Host::with_paths(Paths::rooted(&root));
        let path = state_path(&host).expect("a data folder");
        (host, path)
    }

    fn cleanup(path: &Path) {
        if let Some(root) = path.parent().and_then(Path::parent) {
            let _ = fs::remove_dir_all(root);
        }
    }

    /// The rescues beside `path`, with their bytes.
    fn rescues(path: &Path) -> Vec<Vec<u8>> {
        fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".corrupt."))
            .map(|entry| fs::read(entry.path()).unwrap())
            .collect()
    }

    #[test]
    fn a_file_that_is_not_utf8_is_moved_aside_before_anything_is_saved() {
        let (host, path) = host("utf8");
        // A truncated write: the start of a document, then bytes that are no
        // text at all.
        let bytes = b"{\"workspaces\": [{\"name\": \"caf\xc3".to_vec();
        fs::write(&path, &bytes).unwrap();

        let err = load_state_checked(&host).expect_err("unreadable");
        assert!(matches!(err.rescue, Rescue::KeptAs(_)), "{err:?}");
        assert!(err.reason.contains("UTF-8"), "{}", err.reason);
        assert_eq!(rescues(&path), std::slice::from_ref(&bytes));

        // The empty state the window saves next lands on a fresh file.
        save_state(&host, json!({})).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}");
        assert_eq!(rescues(&path), [bytes]);
        cleanup(&path);
    }

    #[test]
    fn a_document_that_is_not_an_object_is_moved_aside_too() {
        let (host, path) = host("root");
        fs::write(&path, "[1, 2]").unwrap();
        let err = load_state_checked(&host).expect_err("not a state");
        assert!(matches!(err.rescue, Rescue::KeptAs(_)), "{err:?}");
        assert_eq!(rescues(&path), [b"[1, 2]".to_vec()]);
        assert!(!path.exists());
        cleanup(&path);
    }

    #[test]
    fn a_byte_order_mark_is_not_a_reason_to_set_the_file_aside() {
        let (host, path) = host("bom");
        fs::write(&path, "\u{feff}{\"activeWorkspaceId\": \"w1\"}").unwrap();
        assert_eq!(
            load_state_checked(&host).unwrap(),
            Some(json!({"activeWorkspaceId": "w1"}))
        );
        assert!(rescues(&path).is_empty());
        cleanup(&path);
    }

    #[test]
    fn no_file_is_no_state_and_no_error() {
        let (host, path) = host("none");
        assert_eq!(load_state_checked(&host), Ok(None));
        cleanup(&path);
    }

    /// The hole this exists for: a file that can be neither read nor moved
    /// aside used to be written over by the first save of an empty window.
    #[cfg(unix)]
    #[test]
    fn a_file_that_cannot_be_moved_aside_is_never_written_over() {
        use std::os::unix::fs::PermissionsExt;
        // Permissions do not stop root, and nothing would be held.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let (host, path) = host("held");
        let dir = path.parent().unwrap().to_path_buf();
        fs::write(&path, "{\"workspaces\": [").unwrap();
        // A folder nobody may write to: no rename, no copy beside the file.
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();

        let err = load_state_checked(&host).expect_err("unreadable");
        assert_eq!(err.rescue, Rescue::Held);
        let refused = save_state(&host, json!({}));
        assert!(refused.is_err(), "{refused:?}");
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"workspaces\": [");

        // Once it can be moved aside, it is, and saving goes on.
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        save_state(&host, json!({})).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}");
        assert_eq!(rescues(&path), [b"{\"workspaces\": [".to_vec()]);
        // And only once: the hold is over.
        save_state(&host, json!({"a": 1})).unwrap();
        assert_eq!(rescues(&path).len(), 1);
        cleanup(&path);
    }

    #[test]
    fn a_document_the_window_turns_down_is_set_aside_on_its_word() {
        let (host, path) = host("refused");
        fs::write(&path, "{\"workspaces\": 3}").unwrap();
        assert!(load_state_checked(&host).unwrap().is_some());
        let err = set_aside_state(&host, "no workspace list".into());
        assert!(matches!(err.rescue, Rescue::KeptAs(_)), "{err:?}");
        assert_eq!(rescues(&path), [b"{\"workspaces\": 3}".to_vec()]);
        cleanup(&path);
    }
}
