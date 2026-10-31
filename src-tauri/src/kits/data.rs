//! A kit's own data: `~/.hangar/kit-data/<kit id>/`, the same on every Claude
//! account and worktree, never inside a project. A tab reads and writes it
//! through the window (`data.read`, `data.write`, src/lib/kitApi.ts), which
//! names the kit from its own record of the tab; a hook finds it through
//! `HANGAR_KITS_DATA` in the pane's environment.
//!
//! A path is relative to that kit's folder, and stays there: no absolute path,
//! no `..`, no drive or stream, and on disk no link that leads out of the
//! folder, nor a kit folder that is itself a link to another's. A write walks
//! its folders one by one, so a link is never followed to create anything
//! outside, and lands whole: written aside, then renamed over.
//!
//! Removing a kit leaves its data: the folder is not the kit's install.

use std::ffi::OsString;
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::manifest;
use super::{off_thread, TabError};

/// What a pane of a workspace with kits finds the root in.
pub const ENV_VAR: &str = "HANGAR_KITS_DATA";

const FOLDER: &str = "kit-data";

/// `KIT_DATA_MAX_BYTES` in src/lib/kits.ts.
pub const MAX_BYTES: u64 = 1024 * 1024;

/// The longest path a tab may give.
const MAX_PATH_CHARS: usize = 1024;

/// How long a write waits for someone reading the file on Windows, where a
/// rename over a file a program holds open is refused for that moment.
const RENAME_TRIES: usize = 10;
const RENAME_WAIT: Duration = Duration::from_millis(20);

fn root_from(endpoint_file: Option<OsString>, home: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(file) = endpoint_file.filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(file).parent()?.join(FOLDER));
    }
    Some(home?.join(".hangar").join(FOLDER))
}

/// `~/.hangar/kit-data`, or beside the endpoint file a development build run
/// next to an installed Hangar was given, like the kits themselves.
pub fn root() -> Option<PathBuf> {
    root_from(std::env::var_os("HANGAR_ENDPOINT_FILE"), dirs::home_dir())
}

fn invalid(what: impl std::fmt::Display) -> TabError {
    TabError::new(TabError::INVALID, format!("path {what}"))
}

fn failed(what: impl std::fmt::Display) -> TabError {
    TabError::new(TabError::FAILED, what.to_string())
}

fn not_found(raw: &str) -> TabError {
    TabError::new(
        TabError::NOT_FOUND,
        format!("{raw} is not in this kit's data"),
    )
}

/// The parts of a path a tab gave, relative to its kit's folder.
fn parts(raw: &str) -> Result<Vec<&str>, TabError> {
    if raw.trim().is_empty() {
        return Err(invalid("is empty"));
    }
    if raw.chars().count() > MAX_PATH_CHARS {
        return Err(invalid(format!(
            "is longer than {MAX_PATH_CHARS} characters"
        )));
    }
    if raw.contains('\0') {
        return Err(invalid("holds a NUL character"));
    }
    if raw.starts_with(['/', '\\']) {
        return Err(invalid("must be relative to the kit's data folder"));
    }
    let mut parts = Vec::new();
    for part in raw.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return Err(invalid("must not climb out with `..`")),
            part if part.contains(':') => {
                return Err(invalid("must be relative to the kit's data folder"))
            }
            // Windows drops them, so it would name another file than the one
            // checked.
            part if part.ends_with(['.', ' ']) => {
                return Err(invalid("parts must not end with a dot or a space"))
            }
            part => parts.push(part),
        }
    }
    if parts.is_empty() {
        return Err(invalid("must name a file in the kit's data folder"));
    }
    Ok(parts)
}

/// The data of every kit, under one root.
pub struct Data {
    root: PathBuf,
}

impl Data {
    pub fn at(root: PathBuf) -> Self {
        Data { root }
    }

    /// The kit's folder on disk, canonical; `None` before it has any data. One
    /// that is itself a link leads out of its place, to another kit's perhaps.
    fn folder(&self, kit: &str) -> Result<Option<PathBuf>, TabError> {
        if !manifest::valid_kit_id(kit) {
            return Err(TabError::new(TabError::INVALID, "not a kit id"));
        }
        let real = match dunce::canonicalize(self.root.join(kit)) {
            Ok(real) => real,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(failed(format!("the kit's data folder: {err}"))),
        };
        let root = dunce::canonicalize(&self.root).map_err(failed)?;
        if real != root.join(kit) {
            return Err(failed("the kit's data folder leads outside its place"));
        }
        Ok(Some(real))
    }

    /// `data.read`: the text of `raw`, relative to the kit's folder.
    pub fn read(&self, kit: &str, raw: &str) -> Result<String, TabError> {
        let parts = parts(raw)?;
        let Some(folder) = self.folder(kit)? else {
            return Err(not_found(raw));
        };
        let joined = parts
            .iter()
            .fold(folder.clone(), |path, part| path.join(part));
        let real = match dunce::canonicalize(&joined) {
            Ok(real) => real,
            Err(err) if matches!(err.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
                return Err(not_found(raw))
            }
            Err(err) => return Err(failed(format!("{raw} cannot be read: {err}"))),
        };
        if !real.starts_with(&folder) {
            return Err(failed(format!("{raw} leads outside the kit's data folder")));
        }
        if !real.is_file() {
            return Err(failed(format!("{raw} is not a file")));
        }
        read_text(&real, raw)
    }

    /// `data.write`: `raw` holds `text`, whole, its folders made as needed.
    pub fn write(&self, kit: &str, raw: &str, text: &str) -> Result<(), TabError> {
        if text.len() as u64 > MAX_BYTES {
            return Err(TabError::new(TabError::INVALID, "text is larger than 1 MB"));
        }
        let parts = parts(raw)?;
        if !manifest::valid_kit_id(kit) {
            return Err(TabError::new(TabError::INVALID, "not a kit id"));
        }
        fs::create_dir_all(self.root.join(kit))
            .map_err(|err| failed(format!("the kit's data folder cannot be made: {err}")))?;
        let folder = self
            .folder(kit)?
            .ok_or_else(|| failed("the kit's data folder went away"))?;
        let (name, dirs) = parts
            .split_last()
            .ok_or_else(|| invalid("must name a file in the kit's data folder"))?;
        let mut dir = folder.clone();
        for part in dirs {
            dir = step_into(&folder, &dir, part, raw)?;
        }
        let target = dir.join(name);
        check_target(&folder, &target, raw)?;
        write_whole(&dir, &target, text.as_bytes(), raw)
    }
}

/// The folder `part` of `dir`, made when it is not there. `create_dir` takes
/// nothing already there for its own, a link included, so nothing is made
/// through one before it is looked at.
fn step_into(folder: &Path, dir: &Path, part: &str, raw: &str) -> Result<PathBuf, TabError> {
    let next = dir.join(part);
    match fs::create_dir(&next) {
        Ok(()) => return Ok(next),
        Err(err) if err.kind() == ErrorKind::AlreadyExists => {}
        Err(err) => return Err(failed(format!("{raw} cannot be written: {err}"))),
    }
    let real = dunce::canonicalize(&next)
        .map_err(|err| failed(format!("{raw} cannot be written: {err}")))?;
    if !real.starts_with(folder) {
        return Err(failed(format!("{raw} leads outside the kit's data folder")));
    }
    if !real.is_dir() {
        return Err(failed(format!("{raw}: {part} is not a folder")));
    }
    Ok(real)
}

/// What the rename would replace: nothing, a file, or a link that stays inside.
fn check_target(folder: &Path, target: &Path, raw: &str) -> Result<(), TabError> {
    let meta = match fs::symlink_metadata(target) {
        Ok(meta) => meta,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(failed(format!("{raw} cannot be written: {err}"))),
    };
    if meta.file_type().is_symlink() {
        let inside = dunce::canonicalize(target).is_ok_and(|real| real.starts_with(folder));
        if !inside {
            return Err(failed(format!("{raw} leads outside the kit's data folder")));
        }
    } else if meta.is_dir() {
        return Err(failed(format!("{raw} is a folder")));
    }
    Ok(())
}

/// Written aside in the same folder and renamed over, so a reader finds the
/// whole of the old text or the whole of the new.
fn write_whole(dir: &Path, target: &Path, body: &[u8], raw: &str) -> Result<(), TabError> {
    let cannot = |err: std::io::Error| failed(format!("{raw} cannot be written: {err}"));
    let tmp = dir.join(format!(".{}.tmp", uuid::Uuid::new_v4().simple()));
    let written = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        file.write_all(body)?;
        file.sync_all()
    })();
    let renamed = written.and_then(|()| rename_over(&tmp, target));
    renamed.map_err(|err| {
        let _ = fs::remove_file(&tmp);
        cannot(err)
    })
}

fn rename_over(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut tries = RENAME_TRIES;
    loop {
        match fs::rename(from, to) {
            Err(err) if err.kind() == ErrorKind::PermissionDenied && tries > 1 => {
                tries -= 1;
                std::thread::sleep(RENAME_WAIT);
            }
            done => return done,
        }
    }
}

/// UTF-8 text of `MAX_BYTES` at most, a BOM dropped: what `files.read` takes.
fn read_text(path: &Path, raw: &str) -> Result<String, TabError> {
    let too_big = || failed(format!("{raw} is larger than 1 MB"));
    let cannot = |err: std::io::Error| failed(format!("{raw} cannot be read: {err}"));
    let len = fs::metadata(path).map_err(cannot)?.len();
    if len > MAX_BYTES {
        return Err(too_big());
    }
    let mut bytes = Vec::with_capacity(len as usize);
    fs::File::open(path)
        .and_then(|file| file.take(MAX_BYTES + 1).read_to_end(&mut bytes))
        .map_err(cannot)?;
    // Grown since it was measured.
    if bytes.len() as u64 > MAX_BYTES {
        return Err(too_big());
    }
    let text =
        String::from_utf8(bytes).map_err(|_| failed(format!("{raw} is not a UTF-8 text file")))?;
    if text.contains('\0') {
        return Err(failed(format!("{raw} is not a text file")));
    }
    Ok(text
        .strip_prefix('\u{feff}')
        .map(str::to_string)
        .unwrap_or(text))
}

fn open() -> Result<Data, TabError> {
    root()
        .map(Data::at)
        .ok_or_else(|| failed("no home directory"))
}

/// `data.read`. `kit` is the window's own record of the tab, never the page's.
#[tauri::command]
pub async fn kit_data_read(kit: String, path: String) -> Result<String, TabError> {
    off_thread(move || open()?.read(&kit, &path)).await
}

/// `data.write`. `kit` is the window's own record of the tab, never the page's.
#[tauri::command]
pub async fn kit_data_write(kit: String, path: String, text: String) -> Result<(), TabError> {
    off_thread(move || open()?.write(&kit, &path, &text)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kits::store::Store;
    use crate::kits::testing::Folder;
    use crate::model::tests::link;

    fn data(tag: &str) -> (Folder, Data) {
        let home = Folder::new(tag);
        let data = Data::at(home.root.join(FOLDER));
        (home, data)
    }

    fn code(result: Result<String, TabError>) -> &'static str {
        result.expect_err("should have been refused").code
    }

    #[test]
    fn the_root_is_in_hangars_folder_or_beside_a_dev_builds_endpoint_file() {
        let home = PathBuf::from("/home/me");
        assert_eq!(
            root_from(None, Some(home.clone())),
            Some(home.join(".hangar").join("kit-data"))
        );
        assert_eq!(
            root_from(Some(OsString::new()), Some(home.clone())),
            Some(home.join(".hangar").join("kit-data")),
            "an empty variable counts as none"
        );
        assert_eq!(
            root_from(
                Some(OsString::from("/dev/hangar/endpoint.json")),
                Some(home)
            ),
            Some(PathBuf::from("/dev/hangar").join("kit-data"))
        );
        assert_eq!(root_from(None, None), None);
    }

    #[test]
    fn what_is_written_is_read_back_subfolders_made() {
        let (_home, data) = data("data-roundtrip");
        assert_eq!(code(data.read("alpha", "state.json")), TabError::NOT_FOUND);
        data.write("alpha", "state.json", "{\"n\":1}").unwrap();
        assert_eq!(data.read("alpha", "state.json").unwrap(), "{\"n\":1}");

        data.write("alpha", "queue\\today/./log.txt", "é").unwrap();
        assert_eq!(data.read("alpha", "queue/today/log.txt").unwrap(), "é");
        assert_eq!(
            code(data.read("alpha", "queue/none.txt")),
            TabError::NOT_FOUND
        );
        assert_eq!(
            code(data.read("alpha", "state.json/inside.txt")),
            TabError::NOT_FOUND,
            "under a file"
        );

        // Replaced, and an empty text is a text.
        data.write("alpha", "state.json", "").unwrap();
        assert_eq!(data.read("alpha", "state.json").unwrap(), "");

        // A hidden name is the kit's own business, in its own folder.
        data.write("alpha", ".cache/x", "y").unwrap();
        assert_eq!(data.read("alpha", ".cache/x").unwrap(), "y");
    }

    #[test]
    fn paths_that_leave_the_folder_are_refused() {
        let (home, data) = data("data-paths");
        data.write("alpha", "ok.txt", "ok").unwrap();
        home.file("secret.txt", "secret");
        home.file(&format!("{FOLDER}/secret.txt"), "secret");
        home.file(&format!("{FOLDER}/beta/secret.txt"), "secret");
        let absolute = home.root.join("secret.txt").to_string_lossy().into_owned();
        for path in [
            "",
            "  ",
            ".",
            "./",
            "../secret.txt",
            "../beta/secret.txt",
            "../../secret.txt",
            "a/../../secret.txt",
            "a\\..\\..\\secret.txt",
            "/etc/passwd",
            "\\\\server\\share\\x.txt",
            "C:/Windows/win.ini",
            "C:\\Windows\\win.ini",
            "ok.txt:stream",
            "ok.txt.",
            "ok.txt ",
            "a\0b",
            absolute.as_str(),
        ] {
            assert_eq!(
                code(data.read("alpha", path)),
                TabError::INVALID,
                "{path:?}"
            );
            assert_eq!(
                data.write("alpha", path, "x").unwrap_err().code,
                TabError::INVALID,
                "{path:?}"
            );
        }
        let long = "a/".repeat(600) + "x.txt";
        assert_eq!(code(data.read("alpha", &long)), TabError::INVALID);
        assert_eq!(
            std::fs::read_to_string(home.root.join("secret.txt")).unwrap(),
            "secret"
        );
    }

    #[test]
    fn a_kit_id_that_is_no_kit_id_reaches_nothing() {
        let (home, data) = data("data-ids");
        home.file(&format!("{FOLDER}/beta/x.txt"), "beta's");
        for kit in ["..", "", "a", "../beta", "Beta", "beta/..", "beta\\x"] {
            assert_eq!(code(data.read(kit, "x.txt")), TabError::INVALID, "{kit:?}");
            assert_eq!(
                data.write(kit, "x.txt", "y").unwrap_err().code,
                TabError::INVALID,
                "{kit:?}"
            );
        }
    }

    #[test]
    fn a_kit_reaches_its_own_folder_only() {
        let (_home, data) = data("data-kits");
        data.write("alpha", "state.json", "alpha's").unwrap();
        data.write("beta", "state.json", "beta's").unwrap();
        assert_eq!(data.read("alpha", "state.json").unwrap(), "alpha's");
        assert_eq!(data.read("beta", "state.json").unwrap(), "beta's");
        data.write("beta", "only-beta.txt", "x").unwrap();
        assert_eq!(
            code(data.read("alpha", "only-beta.txt")),
            TabError::NOT_FOUND
        );
    }

    /// `link` needs Developer Mode on Windows, so the same cases run through a
    /// junction there too (a folder only), which any user can make.
    fn link_folder(target: &Path, at: &Path) -> bool {
        #[cfg(windows)]
        {
            std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(at)
                .arg(target)
                .output()
                .is_ok_and(|output| output.status.success())
        }
        #[cfg(not(windows))]
        {
            link(target, at)
        }
    }

    #[test]
    fn a_link_out_of_the_folder_is_refused() {
        let (home, data) = data("data-links");
        data.write("alpha", "ok.txt", "ok").unwrap();
        data.write("beta", "state.json", "beta's").unwrap();
        home.file("outside/secret.txt", "secret");
        let root = home.root.join(FOLDER);
        let alpha = root.join("alpha");

        // Through a folder: nothing read, and nothing made out there.
        assert!(link_folder(&home.root.join("outside"), &alpha.join("away")));
        assert!(data
            .read("alpha", "away/secret.txt")
            .unwrap_err()
            .message
            .contains("outside"));
        assert!(data
            .write("alpha", "away/new.txt", "x")
            .unwrap_err()
            .message
            .contains("outside"));
        assert!(data
            .write("alpha", "away/deep/new.txt", "x")
            .unwrap_err()
            .message
            .contains("outside"));
        assert!(!home.root.join("outside/new.txt").exists());
        assert!(!home.root.join("outside/deep").exists());

        // To another kit's folder.
        assert!(link_folder(&root.join("beta"), &alpha.join("beta-link")));
        assert!(data.read("alpha", "beta-link/state.json").is_err());
        assert!(data.write("alpha", "beta-link/state.json", "x").is_err());
        assert_eq!(data.read("beta", "state.json").unwrap(), "beta's");

        // The kit's folder itself made a link to another kit's.
        assert!(link_folder(&root.join("beta"), &root.join("gamma")));
        assert!(data
            .read("gamma", "state.json")
            .unwrap_err()
            .message
            .contains("outside"));
        assert!(data
            .write("gamma", "state.json", "x")
            .unwrap_err()
            .message
            .contains("outside"));
        assert_eq!(data.read("beta", "state.json").unwrap(), "beta's");

        // A file link, where this machine allows one.
        if link(
            &home.root.join("outside/secret.txt"),
            &alpha.join("leak.txt"),
        ) {
            assert!(data
                .read("alpha", "leak.txt")
                .unwrap_err()
                .message
                .contains("outside"));
            assert!(data
                .write("alpha", "leak.txt", "x")
                .unwrap_err()
                .message
                .contains("outside"));
            assert_eq!(
                std::fs::read_to_string(home.root.join("outside/secret.txt")).unwrap(),
                "secret"
            );
        }
        // Junctions are not taken apart by the folder's removal below.
        for junction in [
            alpha.join("away"),
            alpha.join("beta-link"),
            root.join("gamma"),
        ] {
            let _ = fs::remove_dir(junction);
        }
    }

    #[test]
    fn a_device_name_neither_hangs_nor_reads_a_device() {
        let (_home, data) = data("data-devices");
        data.write("alpha", "ok.txt", "ok").unwrap();
        for path in ["CON", "NUL", "aux.txt", "folder/COM1"] {
            // On a unix these are ordinary names, absent here; on Windows,
            // whatever it makes of them, they are not a file of the kit's.
            assert!(data.read("alpha", path).is_err(), "{path}");
        }
    }

    #[test]
    fn sizes_are_held_to_a_megabyte() {
        let (home, data) = data("data-sizes");
        let at_cap = "a".repeat(MAX_BYTES as usize);
        data.write("alpha", "cap.txt", &at_cap).unwrap();
        assert_eq!(data.read("alpha", "cap.txt").unwrap().len(), at_cap.len());

        let over = at_cap + "a";
        let refused = data.write("alpha", "over.txt", &over).unwrap_err();
        assert_eq!(refused.code, TabError::INVALID);
        assert!(refused.message.contains("1 MB"), "{}", refused.message);
        assert_eq!(code(data.read("alpha", "over.txt")), TabError::NOT_FOUND);

        // Written by a hook, then.
        home.file(&format!("{FOLDER}/alpha/over.txt"), &over);
        let refused = data.read("alpha", "over.txt").unwrap_err();
        assert_eq!(refused.code, TabError::FAILED);
        assert!(refused.message.contains("1 MB"), "{}", refused.message);
    }

    #[test]
    fn only_text_is_read() {
        let (home, data) = data("data-text");
        let folder = home.root.join(FOLDER).join("alpha");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("image.png"), b"\x89PNG\r\n\x1a\n\xff\xfe").unwrap();
        fs::write(folder.join("zero.txt"), b"text\0more").unwrap();
        fs::write(folder.join("bom.txt"), "\u{feff}text").unwrap();
        fs::create_dir(folder.join("dir")).unwrap();
        assert!(data
            .read("alpha", "image.png")
            .unwrap_err()
            .message
            .contains("UTF-8"));
        assert!(data
            .read("alpha", "zero.txt")
            .unwrap_err()
            .message
            .contains("not a text file"));
        assert_eq!(data.read("alpha", "bom.txt").unwrap(), "text");
        assert!(data
            .read("alpha", "dir")
            .unwrap_err()
            .message
            .contains("not a file"));
        assert!(data
            .write("alpha", "dir", "x")
            .unwrap_err()
            .message
            .contains("is a folder"));
    }

    #[test]
    fn a_write_lands_whole_and_leaves_nothing_aside() {
        let (home, data) = data("data-atomic");
        let folder = home.root.join(FOLDER).join("alpha");
        let (one, two) = ("1".repeat(256 * 1024), "2".repeat(256 * 1024));
        data.write("alpha", "state.txt", &one).unwrap();

        // A reader polling while it is replaced: only ever one text or the other.
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let reader = {
            let (stop, data) = (stop.clone(), Data::at(data.root.clone()));
            std::thread::spawn(move || {
                let mut reads = 0;
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    if let Ok(text) = data.read("alpha", "state.txt") {
                        let first = text.as_bytes()[0];
                        assert_eq!(text.len(), 256 * 1024, "half a file");
                        assert!(text.bytes().all(|b| b == first), "two files mixed");
                        reads += 1;
                    }
                }
                reads
            })
        };
        for round in 0..40 {
            let body = if round % 2 == 0 { &two } else { &one };
            data.write("alpha", "state.txt", body).unwrap();
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(reader.join().unwrap() > 0, "the reader read something");

        let left: Vec<_> = fs::read_dir(&folder)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(left, ["state.txt"], "nothing left aside");
    }

    #[test]
    fn removing_a_kit_leaves_its_data() {
        let (home, data) = data("data-remove");
        let store = Store::at(home.root.join("kits"));
        let kit = Folder::new("data-remove-src");
        kit.kit(".", "alpha", "1.0.0");
        store.add_folder(&kit.root.to_string_lossy(), 1).unwrap();
        data.write("alpha", "state.json", "kept").unwrap();
        store.remove("alpha").unwrap();
        assert_eq!(data.read("alpha", "state.json").unwrap(), "kept");
    }
}
