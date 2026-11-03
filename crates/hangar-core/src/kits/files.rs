//! What a kit's tab reaches of the project it is shown in, beyond the text
//! `files.read` gives every tab that holds it (kit_scheme/files.rs): changing a
//! text file (`files.write`), making an image (`files.writeBinary`), reading an
//! image or a PDF (`files.readBinary`),
//! listing a folder (`files.list`), and, for a tab that holds both `files.read`
//! and `files.write`, reading a hidden file such as `.env`.
//!
//! The window checks a path first (src/lib/kitApi.ts); these check it again, on
//! disk, where links are followed:
//!
//! - relative to the workspace folder, either slash, no `..`, no drive or stream
//!   (`:`), 1 024 characters at most;
//! - inside the folder once its links and junctions are followed, a write walking
//!   its folders one by one, so nothing is made through a link that leads out;
//! - never `.git`, at any depth, whether a folder or the file a worktree has
//!   there, by any name or link that leads to it: its hooks are programs.
//!
//! The root and whether a tab may read hidden files come from the window's own
//! record of the tab, never from the page.

use std::collections::VecDeque;
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde::Serialize;

use super::data::{read_text, rename_over};
use super::{off_thread, TabError};

/// `KIT_FILE_MAX_BYTES` in src/lib/kits.ts.
pub const MAX_TEXT_BYTES: u64 = 1024 * 1024;
/// `KIT_BINARY_MAX_BYTES` in src/lib/kits.ts.
pub const MAX_BINARY_BYTES: u64 = 10 * 1024 * 1024;
/// `KIT_LIST_MAX_FILES` in src/lib/kits.ts.
pub const MAX_LISTED: usize = 2000;
/// `KIT_LIST_PATTERN_MAX_CHARS` in src/lib/kits.ts.
const MAX_PATTERN_CHARS: usize = 256;
/// What a walk looks at before it stops and says it stopped: a tab may poll, and
/// a pattern matching nothing would otherwise walk a whole monorepo each time.
const MAX_LOOKED_AT: usize = 200_000;
/// The longest path a tab may give.
const MAX_PATH_CHARS: usize = 1024;

/// What `files.list` does not walk into. `.git` is never reached at all.
const SKIPPED_FOLDERS: &[&str] = &["node_modules", "target", "dist"];

/// What `files.readBinary` reads, by extension only.
const BINARY_TYPES: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("avif", "image/avif"),
    ("bmp", "image/bmp"),
    ("ico", "image/x-icon"),
    ("svg", "image/svg+xml"),
    ("pdf", "application/pdf"),
];

fn invalid(what: impl std::fmt::Display) -> TabError {
    TabError::new(TabError::INVALID, format!("path {what}"))
}

fn failed(what: impl std::fmt::Display) -> TabError {
    TabError::new(TabError::FAILED, what.to_string())
}

fn not_found(raw: &str) -> TabError {
    TabError::new(
        TabError::NOT_FOUND,
        format!("{raw} was not found in the workspace folder"),
    )
}

/// A case-insensitive file system takes `.GIT` for `.git`.
fn is_git(name: &str) -> bool {
    name.eq_ignore_ascii_case(".git")
}

/// Whether Windows takes this name for a device, whatever follows its first dot.
fn names_a_device(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or(part).trim_end();
    let stem = stem.to_ascii_uppercase();
    if matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) {
        return true;
    }
    let number = stem
        .strip_prefix("COM")
        .or_else(|| stem.strip_prefix("LPT"));
    number.is_some_and(|number| {
        matches!(
            number,
            "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
        )
    })
}

/// The parts of a path a tab gave, relative to the workspace folder: none for the
/// folder itself. `hidden` says whether a hidden file or folder may be named.
fn parts(raw: &str, hidden: bool) -> Result<Vec<&str>, TabError> {
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
        return Err(invalid("must be relative to the workspace folder"));
    }
    let mut parts = Vec::new();
    for part in raw.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return Err(invalid("must not climb out with `..`")),
            part if part.contains(':') => {
                return Err(invalid("must be relative to the workspace folder"))
            }
            part if is_git(part) => return Err(invalid("must not name anything under .git")),
            part if !hidden && part.starts_with('.') => {
                return Err(invalid("must not name a hidden file or folder"))
            }
            // Windows drops them, so it would name another file than the one
            // checked.
            part if part.ends_with(['.', ' ']) => {
                return Err(invalid("parts must not end with a dot or a space"))
            }
            // There a write would land on the device, or on a file nothing can
            // open afterwards.
            part if cfg!(windows) && names_a_device(part) => {
                return Err(invalid("must not name a device"))
            }
            part => parts.push(part),
        }
    }
    Ok(parts)
}

/// The same, for a path that has to name a file.
fn file_parts(raw: &str, hidden: bool) -> Result<Vec<&str>, TabError> {
    let parts = parts(raw, hidden)?;
    if parts.is_empty() {
        return Err(invalid("must name a file inside the workspace folder"));
    }
    Ok(parts)
}

fn workspace(root: &str) -> Result<PathBuf, TabError> {
    dunce::canonicalize(root).map_err(|_| {
        TabError::new(
            TabError::UNAVAILABLE,
            "the workspace folder is not there any more",
        )
    })
}

fn joined(root: &Path, parts: &[&str]) -> PathBuf {
    parts
        .iter()
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

/// Where a path really is, once on disk: inside the folder, not in `.git`, and
/// not hidden unless the tab may read that. A name that is another's (a link, a
/// short name on Windows) counts for what it leads to.
fn check_real(root: &Path, real: &Path, raw: &str, hidden: bool) -> Result<(), TabError> {
    let inside = real
        .strip_prefix(root)
        .map_err(|_| failed(format!("{raw} leads outside the workspace folder")))?;
    for part in inside.components() {
        let name = part.as_os_str().to_string_lossy();
        if is_git(&name) {
            return Err(failed(format!("{raw} leads into .git")));
        }
        if !hidden && name.starts_with('.') {
            return Err(failed(format!("{raw} leads to a hidden file or folder")));
        }
    }
    Ok(())
}

/// What `parts` names on disk, canonical and checked.
fn existing(root: &Path, parts: &[&str], raw: &str, hidden: bool) -> Result<PathBuf, TabError> {
    let real = dunce::canonicalize(joined(root, parts)).map_err(|err| match err.kind() {
        ErrorKind::NotFound | ErrorKind::NotADirectory => not_found(raw),
        _ => failed(format!("{raw} cannot be read: {err}")),
    })?;
    check_real(root, &real, raw, hidden)?;
    Ok(real)
}

fn existing_file(
    root: &Path,
    parts: &[&str],
    raw: &str,
    hidden: bool,
) -> Result<PathBuf, TabError> {
    let real = existing(root, parts, raw, hidden)?;
    if !real.is_file() {
        return Err(failed(format!("{raw} is not a file")));
    }
    Ok(real)
}

/// `files.read`, for a tab that holds `files.write` as well: a hidden file reads
/// like any other, so the page can show the `.env` it is there to edit.
pub fn read(root: &str, raw: &str) -> Result<String, TabError> {
    let parts = file_parts(raw, true)?;
    let root = workspace(root)?;
    read_text(&existing_file(&root, &parts, raw, true)?, raw)
}

/// What `files.readBinary` answers.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BinaryFile {
    pub base64: String,
    pub mime: &'static str,
}

fn mime_of(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    BINARY_TYPES
        .iter()
        .find(|(known, _)| *known == extension)
        .map(|(_, mime)| *mime)
}

/// `files.readBinary`: an image or a PDF, by its extension and nothing else.
pub fn read_binary(root: &str, raw: &str, hidden: bool) -> Result<BinaryFile, TabError> {
    let parts = file_parts(raw, hidden)?;
    let unsupported = || {
        TabError::new(
            TabError::UNSUPPORTED,
            format!("{raw} is not an image or a PDF"),
        )
    };
    let named = parts.last().map(Path::new);
    if named.and_then(mime_of).is_none() {
        return Err(unsupported());
    }
    let root = workspace(root)?;
    let real = existing_file(&root, &parts, raw, hidden)?;
    // A link named `.png` does not make a text file an image.
    let mime = mime_of(&real).ok_or_else(unsupported)?;
    let too_big = || failed(format!("{raw} is larger than 10 MB"));
    let cannot = |err: std::io::Error| failed(format!("{raw} cannot be read: {err}"));
    let len = fs::metadata(&real).map_err(cannot)?.len();
    if len > MAX_BINARY_BYTES {
        return Err(too_big());
    }
    let mut bytes = Vec::with_capacity(len as usize);
    fs::File::open(&real)
        .and_then(|file| file.take(MAX_BINARY_BYTES + 1).read_to_end(&mut bytes))
        .map_err(cannot)?;
    // Grown since it was measured.
    if bytes.len() as u64 > MAX_BINARY_BYTES {
        return Err(too_big());
    }
    Ok(BinaryFile {
        base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
        mime,
    })
}

/// The file a write would land on, canonical: followed through a link that stays
/// inside, so the link stays as well. `None` when there is none yet.
fn current_file(root: &Path, parts: &[&str], raw: &str) -> Result<Option<PathBuf>, TabError> {
    let path = joined(root, parts);
    match fs::symlink_metadata(&path) {
        Ok(_) => {}
        Err(err) if matches!(err.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
            return Ok(None)
        }
        Err(err) => return Err(failed(format!("{raw} cannot be written: {err}"))),
    }
    let real = dunce::canonicalize(&path)
        .map_err(|_| failed(format!("{raw} leads outside the workspace folder")))?;
    check_real(root, &real, raw, true)?;
    if real.is_dir() {
        return Err(failed(format!("{raw} is a folder")));
    }
    if !real.is_file() {
        return Err(failed(format!("{raw} is not a file")));
    }
    Ok(Some(real))
}

/// The folders of a new file, made where they are missing. `create_dir` takes
/// nothing already there for its own, a link included, so nothing is made through
/// one before it is looked at.
fn make_folders(root: &Path, dirs: &[&str], raw: &str) -> Result<PathBuf, TabError> {
    let mut dir = root.to_path_buf();
    for part in dirs {
        let next = dir.join(part);
        match fs::create_dir(&next) {
            Ok(()) => {
                dir = next;
                continue;
            }
            Err(err) if err.kind() == ErrorKind::AlreadyExists => {}
            Err(err) => return Err(failed(format!("{raw} cannot be written: {err}"))),
        }
        let real = dunce::canonicalize(&next)
            .map_err(|err| failed(format!("{raw} cannot be written: {err}")))?;
        check_real(root, &real, raw, true)?;
        if !real.is_dir() {
            return Err(failed(format!("{raw}: {part} is not a folder")));
        }
        dir = real;
    }
    Ok(dir)
}

/// Whether the file holds what the page last read, as `files.read` gives it: a
/// BOM dropped, and anything it could not read being something else.
fn unchanged(target: Option<&Path>, expected: &str, raw: &str) -> Result<(), TabError> {
    let same = target.is_some_and(|path| read_text(path, raw).is_ok_and(|text| text == expected));
    if same {
        return Ok(());
    }
    Err(TabError::new(
        TabError::CONFLICT,
        format!("{raw} changed since it was read: nothing was written"),
    ))
}

/// Written aside in the same folder and renamed over, so a reader finds the whole
/// of the old text or the whole of the new. A file keeps its permissions: a `.env`
/// only its owner reads stays that way.
fn land(
    dir: &Path,
    target: &Path,
    text: &str,
    raw: &str,
    expected: Option<&str>,
    permissions: Option<fs::Permissions>,
) -> Result<(), TabError> {
    let cannot = |err: std::io::Error| failed(format!("{raw} cannot be written: {err}"));
    let tmp = dir.join(format!(".hangar-{}.tmp", uuid::Uuid::new_v4().simple()));
    let done = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(cannot)?;
        file.write_all(text.as_bytes()).map_err(cannot)?;
        file.sync_all().map_err(cannot)?;
        drop(file);
        if let Some(permissions) = permissions {
            fs::set_permissions(&tmp, permissions).map_err(cannot)?;
        }
        // Asked again as late as it can be: writing took a moment.
        if let Some(expected) = expected {
            unchanged(Some(target), expected, raw)?;
        }
        rename_over(&tmp, target).map_err(cannot)
    })();
    if done.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    done
}

/// `files.write`: `raw` holds `text`, whole, its folders made as needed. Given
/// `expected`, only if the file still holds that. What exists has to be a text
/// file: what the person accepted is a tab changing text files.
pub fn write(root: &str, raw: &str, text: &str, expected: Option<&str>) -> Result<(), TabError> {
    if text.len() as u64 > MAX_TEXT_BYTES {
        return Err(TabError::new(TabError::INVALID, "text is larger than 1 MB"));
    }
    if expected.is_some_and(|text| text.len() as u64 > MAX_TEXT_BYTES) {
        return Err(TabError::new(
            TabError::INVALID,
            "expected is larger than 1 MB",
        ));
    }
    let parts = file_parts(raw, true)?;
    let root = workspace(root)?;
    // Looked at before anything is made, so a conflict leaves nothing behind.
    let current = current_file(&root, &parts, raw)?;
    if let Some(expected) = expected {
        unchanged(current.as_deref(), expected, raw)?;
    }
    let Some(real) = current else {
        let (name, dirs) = parts
            .split_last()
            .ok_or_else(|| invalid("must name a file inside the workspace folder"))?;
        let dir = make_folders(&root, dirs, raw)?;
        return land(&dir, &dir.join(name), text, raw, expected, None);
    };
    if expected.is_none() {
        read_text(&real, raw)?;
    }
    let permissions = fs::metadata(&real)
        .map_err(|err| failed(format!("{raw} cannot be written: {err}")))?
        .permissions();
    // A rename would replace it all the same, on a unix.
    if permissions.readonly() {
        return Err(failed(format!("{raw} is read-only")));
    }
    let dir = real
        .parent()
        .ok_or_else(|| failed(format!("{raw} has no folder")))?;
    land(dir, &real, text, raw, expected, Some(permissions))
}

/// What `files.writeBinary` makes: raster images only, by extension, and the first
/// bytes of the content have to agree. No `.svg`, which carries script, and no PDF.
const WRITABLE_IMAGES: &[&str] = &["png", "jpg", "jpeg", "webp"];

/// Whether `bytes` begin as an image of the kind `extension` names.
fn is_image_of(extension: &str, bytes: &[u8]) -> bool {
    match extension {
        "png" => bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]),
        "jpg" | "jpeg" => bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
        "webp" => bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        _ => false,
    }
}

fn exists(raw: &str) -> TabError {
    TabError::new(
        TabError::CONFLICT,
        format!("{raw} already exists: nothing was written"),
    )
}

/// Written aside in the same folder, then linked at the target: the link fails if
/// anything is there by then, so nothing is ever replaced, and a reader finds the
/// whole of the image or nothing.
fn land_new(dir: &Path, target: &Path, bytes: &[u8], raw: &str) -> Result<(), TabError> {
    let cannot = |err: std::io::Error| failed(format!("{raw} cannot be written: {err}"));
    let tmp = dir.join(format!(".hangar-{}.tmp", uuid::Uuid::new_v4().simple()));
    let done = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(cannot)?;
        file.write_all(bytes).map_err(cannot)?;
        file.sync_all().map_err(cannot)?;
        drop(file);
        match fs::hard_link(&tmp, target) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == ErrorKind::AlreadyExists => Err(exists(raw)),
            Err(err) => Err(cannot(err)),
        }
    })();
    let _ = fs::remove_file(&tmp);
    done
}

/// `files.writeBinary`: a new image at `raw`, its folders made as needed, from
/// `base64`. A file that exists, of any kind, is a conflict and stays as it is.
pub fn write_binary(root: &str, raw: &str, base64: &str) -> Result<(), TabError> {
    let too_big = || TabError::new(TabError::INVALID, "the image is larger than 10 MB");
    // Before decoding: a string this long cannot hold 10 MB.
    if base64.len() as u64 > MAX_BINARY_BYTES / 3 * 4 + 4 {
        return Err(too_big());
    }
    let parts = file_parts(raw, true)?;
    let extension = parts
        .last()
        .and_then(|name| Path::new(name).extension())
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|extension| WRITABLE_IMAGES.contains(&extension.as_str()))
        .ok_or_else(|| {
            invalid(format!(
                "{raw} must end in .png, .jpg, .jpeg or .webp: only those images are written"
            ))
        })?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64.as_bytes())
        .map_err(|_| invalid("base64 is not valid base64"))?;
    if bytes.len() as u64 > MAX_BINARY_BYTES {
        return Err(too_big());
    }
    if !is_image_of(&extension, &bytes) {
        return Err(TabError::new(
            TabError::INVALID,
            format!("{raw} was not written: its content is not a {extension} image (the first bytes say otherwise)"),
        ));
    }
    let root = workspace(root)?;
    // Looked at before anything is made, so a conflict leaves nothing behind.
    if current_file(&root, &parts, raw)?.is_some() {
        return Err(exists(raw));
    }
    let (name, dirs) = parts
        .split_last()
        .ok_or_else(|| invalid("must name a file inside the workspace folder"))?;
    let dir = make_folders(&root, dirs, raw)?;
    land_new(&dir, &dir.join(name), &bytes, raw)
}

/// A simple glob: `*` is any characters but a slash, `?` one such character, `**`
/// any characters at all, and `**/` any folders, none included. Nothing else is
/// special, and case counts. Without a slash it is held to a file's name, at any
/// depth; with one, to its path from the folder listed. Made into a regular
/// expression, which matches in linear time, whatever a page writes.
struct Glob {
    rule: regex::Regex,
    on_name: bool,
}

impl Glob {
    fn new(pattern: &str) -> Result<Self, TabError> {
        let bad = |why: String| TabError::new(TabError::INVALID, format!("pattern {why}"));
        if pattern.trim().is_empty() {
            return Err(bad("is empty".to_string()));
        }
        if pattern.chars().count() > MAX_PATTERN_CHARS {
            return Err(bad(format!(
                "is longer than {MAX_PATTERN_CHARS} characters"
            )));
        }
        let pattern = pattern.replace('\\', "/");
        let pattern = pattern.strip_prefix("./").unwrap_or(&pattern);
        let mut chars = pattern.chars().peekable();
        let mut rule = String::from(r"(?s)\A");
        while let Some(char) = chars.next() {
            match char {
                '*' if chars.peek() == Some(&'*') => {
                    while chars.peek() == Some(&'*') {
                        chars.next();
                    }
                    if chars.peek() == Some(&'/') {
                        chars.next();
                        rule.push_str("(?:.*/)?");
                    } else {
                        rule.push_str(".*");
                    }
                }
                '*' => rule.push_str("[^/]*"),
                '?' => rule.push_str("[^/]"),
                other => rule.push_str(&regex::escape(other.encode_utf8(&mut [0; 4]))),
            }
        }
        rule.push_str(r"\z");
        Ok(Glob {
            rule: regex::Regex::new(&rule).map_err(|_| bad("cannot be read".to_string()))?,
            on_name: !pattern.contains('/'),
        })
    }

    fn matches(&self, name: &str, path: &str) -> bool {
        self.rule.is_match(if self.on_name { name } else { path })
    }
}

/// A file as `files.list` shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ListedFile {
    /// Relative to the workspace folder, with `/`: what the other calls take.
    pub path: String,
    /// In bytes.
    pub size: u64,
    /// In milliseconds since the epoch; 0 when the system cannot say.
    pub modified: u64,
}

/// What `files.list` answers.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Listing {
    pub files: Vec<ListedFile>,
    /// Whether more files were there than it gives.
    pub truncated: bool,
}

/// `path` from `base`, with `/`.
fn relative(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn millis(at: std::io::Result<SystemTime>) -> u64 {
    at.ok()
        .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_millis() as u64)
}

/// `files.list`: the files under a folder, the nearest first when there are too
/// many, then in path order. A link is neither followed nor listed, a folder that
/// cannot be read is passed over, and a name that is not UTF-8 is left out: no
/// other call could name it. What is skipped is skipped on the way down: a folder
/// asked for by name is listed, `.git` aside.
pub fn list(root: &str, raw: Option<&str>, pattern: Option<&str>) -> Result<Listing, TabError> {
    let glob = pattern.map(Glob::new).transpose()?;
    let root = workspace(root)?;
    let start = match raw {
        None => root.clone(),
        Some(raw) => {
            let parts = parts(raw, true)?;
            let real = existing(&root, &parts, raw, true)?;
            if !real.is_dir() {
                return Err(failed(format!("{raw} is not a folder")));
            }
            real
        }
    };
    let mut folders = VecDeque::from([start.clone()]);
    let mut files = Vec::new();
    let mut looked_at = 0;
    let mut truncated = false;
    'walk: while let Some(folder) = folders.pop_front() {
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            looked_at += 1;
            if looked_at > MAX_LOOKED_AT {
                truncated = true;
                break 'walk;
            }
            let file_name = entry.file_name();
            let Some(name) = file_name.to_str() else {
                continue;
            };
            if is_git(name) {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                if !SKIPPED_FOLDERS.contains(&name) {
                    folders.push_back(entry.path());
                }
                continue;
            }
            if !kind.is_file() {
                continue;
            }
            let path = entry.path();
            if let Some(glob) = &glob {
                if !glob.matches(name, &relative(&start, &path)) {
                    continue;
                }
            }
            if files.len() == MAX_LISTED {
                truncated = true;
                break 'walk;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            files.push(ListedFile {
                path: relative(&root, &path),
                size: meta.len(),
                modified: millis(meta.modified()),
            });
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Listing { files, truncated })
}

/// `files.read` for a tab holding `files.write` as well. `root` is the window's
/// own record of the tab, never the page's, as for all of these.
pub async fn kit_files_read(root: String, path: String) -> Result<String, TabError> {
    off_thread(move || read(&root, &path)).await
}

/// `files.write`.
pub async fn kit_files_write(
    root: String,
    path: String,
    text: String,
    expected: Option<String>,
) -> Result<(), TabError> {
    off_thread(move || write(&root, &path, &text, expected.as_deref())).await
}

/// `files.writeBinary`.
pub async fn kit_files_write_binary(
    root: String,
    path: String,
    base64: String,
) -> Result<(), TabError> {
    off_thread(move || write_binary(&root, &path, &base64)).await
}

/// `files.readBinary`. `hidden`: the tab holds `files.write` as well.
pub async fn kit_files_read_binary(
    root: String,
    path: String,
    hidden: bool,
) -> Result<BinaryFile, TabError> {
    off_thread(move || read_binary(&root, &path, hidden)).await
}

/// `files.list`.
pub async fn kit_files_list(
    root: String,
    path: Option<String>,
    pattern: Option<String>,
) -> Result<Listing, TabError> {
    off_thread(move || list(&root, path.as_deref(), pattern.as_deref())).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kits::testing::{link_folder, Folder};
    use crate::model::tests::link;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    /// A workspace folder, and the same as what the commands take.
    fn project(tag: &str) -> (Folder, String) {
        let folder = Folder::new(&format!("files-{tag}"));
        let root = folder.root.to_string_lossy().into_owned();
        (folder, root)
    }

    fn body(folder: &Folder, path: &str) -> String {
        fs::read_to_string(folder.root.join(path)).unwrap()
    }

    fn paths(listing: &Listing) -> Vec<&str> {
        listing
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect()
    }

    /// What leaves the folder or is not a path, in every form a page can write.
    const REFUSED: &[&str] = &[
        "",
        "  ",
        "../secret.txt",
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
    ];

    const INTO_GIT: &[&str] = &[
        ".git",
        ".git/config",
        ".git\\config",
        ".git/hooks/pre-commit",
        ".GIT/config",
        ".Git/hooks/pre-commit",
        "./.git/config",
        "sub/.git/config",
        "sub\\.GIT\\config",
    ];

    #[test]
    fn a_write_replaces_a_text_or_makes_it_with_its_folders() {
        let (folder, root) = project("write");
        folder.file("config/app.toml", "old\r\n");
        write(&root, "config/app.toml", "new\n", None).unwrap();
        // What the page sends, line endings included: nothing is made of them.
        assert_eq!(body(&folder, "config/app.toml"), "new\n");
        write(&root, "config\\win.txt", "a\r\nb\r\n", None).unwrap();
        assert_eq!(body(&folder, "config/win.txt"), "a\r\nb\r\n");

        write(&root, "a/b/./c/new.txt", "é", None).unwrap();
        assert_eq!(body(&folder, "a/b/c/new.txt"), "é");

        write(&root, "empty.txt", "", None).unwrap();
        assert_eq!(body(&folder, "empty.txt"), "");
    }

    /// The smallest bytes each kind starts with: the check is on the first bytes.
    fn image_b64(extension: &str) -> String {
        let bytes: Vec<u8> = match extension {
            "png" => vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3],
            "jpg" => vec![0xFF, 0xD8, 0xFF, 0xE0, 1, 2],
            _ => b"RIFF\x04\0\0\0WEBPVP8 ".to_vec(),
        };
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    fn is_empty(folder: &Folder) -> bool {
        fs::read_dir(&folder.root).unwrap().next().is_none()
    }

    #[test]
    fn write_binary_makes_an_image_with_its_folders_and_reads_back() {
        let (folder, root) = project("wb");
        for (path, kind) in [
            (".hangar/annotations/a.png", "png"),
            ("b/c.JPG", "jpg"),
            ("d.jpeg", "jpg"),
            ("e.webp", "webp"),
        ] {
            write_binary(&root, path, &image_b64(kind)).unwrap();
            let read = read_binary(&root, path, true).unwrap();
            assert_eq!(read.base64, image_b64(kind), "{path}");
        }
        assert!(folder.root.join(".hangar/annotations/a.png").is_file());
    }

    #[test]
    fn write_binary_refuses_other_extensions() {
        let (folder, root) = project("wb-ext");
        for path in [
            "a.svg",
            "a.pdf",
            "a.gif",
            "a.txt",
            "a",
            "a.png.exe",
            "a.png.",
        ] {
            let err = write_binary(&root, path, &image_b64("png")).unwrap_err();
            assert_eq!(err.code, TabError::INVALID, "{path}");
        }
        assert!(is_empty(&folder));
    }

    #[test]
    fn write_binary_checks_the_first_bytes_against_the_extension() {
        let (folder, root) = project("wb-magic");
        let text = base64::engine::general_purpose::STANDARD.encode("<svg onload=alert(1)>");
        let riff = base64::engine::general_purpose::STANDARD.encode("RIFF");
        for (path, content) in [
            ("a.png", text.clone()),
            ("a.jpg", text.clone()),
            ("a.webp", text),
            ("b.png", image_b64("jpg")),
            ("b.jpg", image_b64("webp")),
            ("b.webp", image_b64("png")),
            ("empty.png", String::new()),
            ("short.webp", riff),
        ] {
            let err = write_binary(&root, path, &content).unwrap_err();
            assert_eq!(err.code, TabError::INVALID, "{path}");
        }
        assert!(is_empty(&folder));
    }

    #[test]
    fn write_binary_says_in_a_sentence_which_file_is_not_the_image_it_names() {
        let (_folder, root) = project("wb-sentence");
        let text = base64::engine::general_purpose::STANDARD.encode("<svg onload=alert(1)>");
        let err = write_binary(&root, "shots/a.png", &text).unwrap_err();
        assert_eq!(
            err.message,
            "shots/a.png was not written: its content is not a png image (the first bytes say otherwise)"
        );
    }

    #[test]
    fn write_binary_refuses_bad_base64() {
        let (folder, root) = project("wb-b64");
        for content in ["not base64!", "iVBO=Rw0", "a"] {
            let err = write_binary(&root, "a.png", content).unwrap_err();
            assert_eq!(err.code, TabError::INVALID, "{content}");
        }
        assert!(is_empty(&folder));
    }

    #[test]
    fn write_binary_refuses_more_than_ten_megabytes() {
        let (folder, root) = project("wb-big");
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        bytes.resize(MAX_BINARY_BYTES as usize + 1, 0);
        let big = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let err = write_binary(&root, "big.png", &big).unwrap_err();
        assert_eq!(err.code, TabError::INVALID);
        // Far over: refused before it is decoded.
        let err = write_binary(&root, "huge.png", &"A".repeat(20 * 1024 * 1024)).unwrap_err();
        assert_eq!(err.code, TabError::INVALID);
        assert!(is_empty(&folder));
        // Exactly 10 MB goes through.
        bytes.truncate(MAX_BINARY_BYTES as usize);
        let exact = base64::engine::general_purpose::STANDARD.encode(&bytes);
        write_binary(&root, "exact.png", &exact).unwrap();
    }

    #[test]
    fn write_binary_never_replaces_whatever_exists() {
        let (folder, root) = project("wb-exists");
        folder.file("text.png", "not an image");
        write_binary(&root, "img.png", &image_b64("png")).unwrap();
        for path in ["text.png", "img.png"] {
            let err = write_binary(&root, path, &image_b64("png")).unwrap_err();
            assert_eq!(err.code, TabError::CONFLICT, "{path}");
        }
        assert_eq!(body(&folder, "text.png"), "not an image");
        // A folder in the way is no file.
        fs::create_dir(folder.root.join("dir.png")).unwrap();
        assert!(write_binary(&root, "dir.png", &image_b64("png")).is_err());
        // Nothing is left aside.
        let left: Vec<String> = fs::read_dir(&folder.root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(left.is_empty(), "{left:?}");
    }

    #[test]
    fn write_binary_keeps_every_path_rule_of_files_write() {
        let (folder, root) = project("wb-paths");
        // The names that are not paths at all ("", "ok.txt.") only turn into
        // valid ones once an extension is added; the rest leave the folder.
        let paths = REFUSED
            .iter()
            .filter(|path| path.contains(['/', '\\', ':', '\0']));
        for path in paths {
            let named = format!("{path}.png");
            assert!(
                write_binary(&root, &named, &image_b64("png")).is_err(),
                "{named}"
            );
        }
        for path in INTO_GIT {
            let named = format!("{path}/x.png");
            assert!(
                write_binary(&root, &named, &image_b64("png")).is_err(),
                "{named}"
            );
        }
        assert!(!folder.root.join(".git").exists());
    }

    #[test]
    fn write_binary_makes_nothing_through_a_link_that_leaves_the_folder() {
        let (folder, root) = project("wb-link");
        let outside = Folder::new("wb-link-outside");
        if link_folder(&outside.root, &folder.root.join("out")) {
            assert!(write_binary(&root, "out/x.png", &image_b64("png")).is_err());
            assert!(!outside.root.join("x.png").exists());
        }
    }

    #[test]
    fn dotfiles_are_what_this_is_for() {
        let (folder, root) = project("dotfiles");
        write(&root, ".env", "A=1\n", None).unwrap();
        assert_eq!(body(&folder, ".env"), "A=1\n");
        assert_eq!(read(&root, ".env").unwrap(), "A=1\n");
        write(&root, ".config/app/.settings", "x", None).unwrap();
        assert_eq!(read(&root, ".config/app/.settings").unwrap(), "x");
        // Only the very name: these are not `.git`.
        for path in [".gitignore", ".github/workflows/ci.yml", ".gitattributes"] {
            write(&root, path, "ok", None).unwrap();
            assert_eq!(read(&root, path).unwrap(), "ok", "{path}");
        }
    }

    #[test]
    fn every_path_that_leaves_the_folder_is_refused_by_every_call() {
        let (folder, root) = project("paths");
        folder.file("ok.txt", "ok");
        let outside = Folder::new("files-paths-outside");
        outside.file("secret.txt", "secret");
        outside.file("shot.png", "png");
        let absolute = outside.root.join("secret.txt");
        let absolute = absolute.to_string_lossy();
        for path in REFUSED.iter().copied().chain([absolute.as_ref()]) {
            assert_eq!(
                write(&root, path, "x", None).unwrap_err().code,
                TabError::INVALID,
                "write {path:?}"
            );
            assert_eq!(
                read(&root, path).unwrap_err().code,
                TabError::INVALID,
                "read {path:?}"
            );
            assert_eq!(
                read_binary(&root, path, true).unwrap_err().code,
                TabError::INVALID,
                "read_binary {path:?}"
            );
            assert_eq!(
                list(&root, Some(path), None).unwrap_err().code,
                TabError::INVALID,
                "list {path:?}"
            );
        }
        // A file has to be named.
        for path in [".", "./", "./."] {
            assert_eq!(
                write(&root, path, "x", None).unwrap_err().code,
                TabError::INVALID,
                "{path:?}"
            );
            assert_eq!(read(&root, path).unwrap_err().code, TabError::INVALID);
        }
        let long = "a/".repeat(600) + "x.txt";
        assert_eq!(
            write(&root, &long, "x", None).unwrap_err().code,
            TabError::INVALID
        );
        assert_eq!(read(&root, &long).unwrap_err().code, TabError::INVALID);
        assert_eq!(body(&outside, "secret.txt"), "secret");
        assert_eq!(body(&folder, "ok.txt"), "ok");
        assert!(!folder.root.join("a").exists());
    }

    #[test]
    fn nothing_under_git_is_reached_however_it_is_written() {
        let (folder, root) = project("git");
        folder.file(".git/config", "[core]");
        folder.file(".git/hooks/pre-commit", "#!/bin/sh");
        folder.file(".git/shot.png", "png");
        folder.file("sub/.git/config", "[core]");
        for path in INTO_GIT.iter().copied() {
            assert_eq!(
                write(&root, path, "evil", None).unwrap_err().code,
                TabError::INVALID,
                "write {path:?}"
            );
            assert_eq!(
                read(&root, path).unwrap_err().code,
                TabError::INVALID,
                "read {path:?}"
            );
            assert_eq!(
                list(&root, Some(path), None).unwrap_err().code,
                TabError::INVALID,
                "list {path:?}"
            );
        }
        assert_eq!(
            read_binary(&root, ".git/shot.png", true).unwrap_err().code,
            TabError::INVALID
        );
        assert_eq!(body(&folder, ".git/config"), "[core]");
        assert_eq!(body(&folder, ".git/hooks/pre-commit"), "#!/bin/sh");
        assert_eq!(body(&folder, "sub/.git/config"), "[core]");

        // Through a link that is not called that: nothing read, nothing made.
        assert!(link_folder(
            &folder.root.join(".git"),
            &folder.root.join("gitdir")
        ));
        let into = read(&root, "gitdir/config").unwrap_err();
        assert!(into.message.contains(".git"), "{}", into.message);
        for path in [
            "gitdir/config",
            "gitdir/hooks/pre-commit",
            "gitdir/new.txt",
            "gitdir/new/x",
        ] {
            let refused = write(&root, path, "evil", None).unwrap_err();
            assert!(
                refused.message.contains(".git"),
                "{path}: {}",
                refused.message
            );
        }
        assert!(list(&root, Some("gitdir/hooks"), None).is_err());
        assert_eq!(body(&folder, ".git/hooks/pre-commit"), "#!/bin/sh");
        assert!(!folder.root.join(".git/new.txt").exists());
        assert!(!folder.root.join(".git/new").exists());
        let _ = fs::remove_dir(folder.root.join("gitdir"));
    }

    #[test]
    fn the_git_file_of_a_worktree_is_git_too() {
        let (folder, root) = project("git-file");
        folder.file(".git", "gitdir: D:/elsewhere/.git/worktrees/x");
        assert_eq!(
            write(&root, ".git", "gitdir: D:/evil", None)
                .unwrap_err()
                .code,
            TabError::INVALID
        );
        assert_eq!(read(&root, ".git").unwrap_err().code, TabError::INVALID);
        assert!(body(&folder, ".git").contains("elsewhere"));
        assert_eq!(paths(&list(&root, None, None).unwrap()), Vec::<&str>::new());
    }

    #[test]
    fn a_link_out_of_the_folder_is_refused_and_one_inside_stays_a_link() {
        let (folder, root) = project("links");
        let outside = Folder::new("files-links-outside");
        outside.file("secret.txt", "secret");
        outside.file("shot.png", "png");
        assert!(link_folder(&outside.root, &folder.root.join("away")));
        for result in [
            write(&root, "away/secret.txt", "x", None),
            write(&root, "away/new.txt", "x", None),
            write(&root, "away/deep/new.txt", "x", None),
        ] {
            let refused = result.unwrap_err();
            assert!(refused.message.contains("outside"), "{}", refused.message);
        }
        assert!(read(&root, "away/secret.txt")
            .unwrap_err()
            .message
            .contains("outside"));
        assert!(read_binary(&root, "away/shot.png", true)
            .unwrap_err()
            .message
            .contains("outside"));
        assert!(list(&root, Some("away"), None)
            .unwrap_err()
            .message
            .contains("outside"));
        assert_eq!(body(&outside, "secret.txt"), "secret");
        assert!(!outside.root.join("new.txt").exists());
        assert!(!outside.root.join("deep").exists());
        // Neither followed nor listed by a walk.
        folder.file("here.txt", "x");
        assert_eq!(paths(&list(&root, None, None).unwrap()), ["here.txt"]);
        let _ = fs::remove_dir(folder.root.join("away"));

        // A file link, where this machine allows one.
        folder.file("real.env", "real");
        if link(
            &outside.root.join("secret.txt"),
            &folder.root.join("leak.txt"),
        ) {
            assert!(write(&root, "leak.txt", "x", None)
                .unwrap_err()
                .message
                .contains("outside"));
            assert!(read(&root, "leak.txt")
                .unwrap_err()
                .message
                .contains("outside"));
            assert_eq!(body(&outside, "secret.txt"), "secret");
        }
        if link(&folder.root.join("real.env"), &folder.root.join(".env")) {
            write(&root, ".env", "through", None).unwrap();
            assert_eq!(body(&folder, "real.env"), "through");
            assert!(fs::symlink_metadata(folder.root.join(".env"))
                .unwrap()
                .file_type()
                .is_symlink());
        }
        // A link leading nowhere says nothing of where it would have gone.
        if link(
            &folder.root.join("gone.txt"),
            &folder.root.join("broken.txt"),
        ) {
            assert!(write(&root, "broken.txt", "x", None).is_err());
            assert!(!folder.root.join("gone.txt").exists());
        }
    }

    #[test]
    fn a_conflict_leaves_the_file_as_it_is() {
        let (folder, root) = project("conflict");
        folder.file(".env", "AGENT=1\n");
        for last_read in ["", "AGENT=0\n", "AGENT=1", "agent=1\n"] {
            let refused = write(&root, ".env", "PERSON=1\n", Some(last_read)).unwrap_err();
            assert_eq!(refused.code, TabError::CONFLICT, "{last_read:?}");
            assert_eq!(body(&folder, ".env"), "AGENT=1\n");
        }
        write(&root, ".env", "PERSON=1\n", Some("AGENT=1\n")).unwrap();
        assert_eq!(body(&folder, ".env"), "PERSON=1\n");
        let left: Vec<_> = fs::read_dir(&folder.root)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(left, [".env"], "nothing left aside");
    }

    #[test]
    fn a_file_gone_since_it_was_read_is_a_conflict_with_nothing_made() {
        let (folder, root) = project("conflict-gone");
        assert_eq!(
            write(&root, "a/b/.env", "x", Some("was here"))
                .unwrap_err()
                .code,
            TabError::CONFLICT
        );
        assert_eq!(
            write(&root, "a/b/.env", "x", Some("")).unwrap_err().code,
            TabError::CONFLICT
        );
        assert!(!folder.root.join("a").exists());
    }

    #[test]
    fn expected_is_what_a_read_gave() {
        let (folder, root) = project("conflict-bom");
        folder.file("bom.txt", "\u{feff}text");
        let seen = read(&root, "bom.txt").unwrap();
        assert_eq!(seen, "text");
        write(&root, "bom.txt", "new", Some(&seen)).unwrap();
        assert_eq!(body(&folder, "bom.txt"), "new");

        // What cannot have been read cannot be what was.
        fs::write(folder.root.join("image.png"), b"\x89PNG\xff\xfe").unwrap();
        assert_eq!(
            write(&root, "image.png", "x", Some("anything"))
                .unwrap_err()
                .code,
            TabError::CONFLICT
        );
    }

    #[test]
    fn sizes_are_held_to_a_megabyte_for_text() {
        let (folder, root) = project("sizes");
        let at_cap = "a".repeat(MAX_TEXT_BYTES as usize);
        write(&root, "cap.txt", &at_cap, None).unwrap();
        assert_eq!(read(&root, "cap.txt").unwrap().len(), at_cap.len());

        let over = at_cap + "a";
        let refused = write(&root, "over.txt", &over, None).unwrap_err();
        assert_eq!(refused.code, TabError::INVALID);
        assert!(refused.message.contains("1 MB"), "{}", refused.message);
        assert!(!folder.root.join("over.txt").exists());
        assert_eq!(
            write(&root, "cap.txt", "x", Some(&over)).unwrap_err().code,
            TabError::INVALID
        );

        folder.file("big.log", &over);
        let refused = read(&root, "big.log").unwrap_err();
        assert_eq!(refused.code, TabError::FAILED);
        assert!(refused.message.contains("1 MB"), "{}", refused.message);
    }

    #[test]
    fn only_a_text_file_is_replaced() {
        let (folder, root) = project("text");
        fs::write(folder.root.join("image.png"), b"\x89PNG\xff\xfe").unwrap();
        fs::write(folder.root.join("zero.txt"), b"text\0more").unwrap();
        folder.file("big.log", &"a".repeat(MAX_TEXT_BYTES as usize + 1));
        for path in ["image.png", "zero.txt", "big.log"] {
            assert_eq!(
                write(&root, path, "x", None).unwrap_err().code,
                TabError::FAILED,
                "{path}"
            );
        }
        assert_eq!(
            fs::read(folder.root.join("image.png")).unwrap(),
            b"\x89PNG\xff\xfe"
        );
        fs::create_dir(folder.root.join("dir")).unwrap();
        assert!(write(&root, "dir", "x", None)
            .unwrap_err()
            .message
            .contains("is a folder"));
        folder.file("file.txt", "x");
        assert!(write(&root, "file.txt/inside.txt", "x", None)
            .unwrap_err()
            .message
            .contains("not a folder"));
    }

    #[test]
    fn a_read_only_file_stays_as_it_is() {
        let (folder, root) = project("read-only");
        folder.file("locked.txt", "kept");
        let path = folder.root.join("locked.txt");
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&path, permissions.clone()).unwrap();
        let refused = write(&root, "locked.txt", "x", None).unwrap_err();
        assert!(refused.message.contains("read-only"), "{}", refused.message);
        assert_eq!(body(&folder, "locked.txt"), "kept");
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        fs::set_permissions(&path, permissions).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_file_keeps_its_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let (folder, root) = project("permissions");
        folder.file(".env", "A=1");
        let path = folder.root.join(".env");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        write(&root, ".env", "A=2", None).unwrap();
        assert_eq!(body(&folder, ".env"), "A=2");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn a_device_name_neither_hangs_nor_reaches_a_device() {
        let (folder, root) = project("devices");
        for path in ["CON", "NUL", "aux.txt", "folder/COM1"] {
            assert!(read(&root, path).is_err(), "{path}");
        }
        assert!(read_binary(&root, "NUL.png", true).is_err());
        // On a unix these are ordinary names.
        #[cfg(windows)]
        for path in [
            "CON",
            "con",
            "NUL",
            "aux.txt",
            "AUX .txt",
            "folder/COM1",
            "LPT9.log",
            "COM¹",
            "CONOUT$",
            "con/x.txt",
        ] {
            assert_eq!(
                write(&root, path, "x", None).unwrap_err().code,
                TabError::INVALID,
                "{path}"
            );
        }
        for (name, device) in [
            ("CON", true),
            ("com1.txt", true),
            ("Lpt0", true),
            ("console.log", false),
            ("COM10", false),
            ("comet", false),
            ("auxiliary.txt", false),
            ("nulls", false),
        ] {
            assert_eq!(names_a_device(name), device, "{name}");
        }
        assert_eq!(
            fs::read_dir(&folder.root).unwrap().count(),
            0,
            "nothing was made"
        );
    }

    #[test]
    fn a_write_lands_whole_and_leaves_nothing_aside() {
        let (folder, root) = project("atomic");
        let (one, two) = ("1".repeat(256 * 1024), "2".repeat(256 * 1024));
        write(&root, "state.txt", &one, None).unwrap();

        // A reader polling while it is replaced: only ever one text or the other.
        let stop = Arc::new(AtomicBool::new(false));
        let reader = {
            let (stop, root) = (stop.clone(), root.clone());
            std::thread::spawn(move || {
                let mut reads = 0;
                while !stop.load(Ordering::Relaxed) {
                    if let Ok(text) = read(&root, "state.txt") {
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
            write(&root, "state.txt", body, None).unwrap();
        }
        stop.store(true, Ordering::Relaxed);
        assert!(reader.join().unwrap() > 0, "the reader read something");

        let left: Vec<_> = fs::read_dir(&folder.root)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(left, ["state.txt"], "nothing left aside");
    }

    #[test]
    fn a_workspace_folder_that_is_gone_is_unavailable() {
        let (folder, root) = project("gone");
        let gone = folder.root.join("gone").to_string_lossy().into_owned();
        drop(root);
        assert_eq!(
            write(&gone, "x.txt", "x", None).unwrap_err().code,
            TabError::UNAVAILABLE
        );
        assert_eq!(
            read(&gone, "x.txt").unwrap_err().code,
            TabError::UNAVAILABLE
        );
        assert_eq!(
            read_binary(&gone, "x.png", false).unwrap_err().code,
            TabError::UNAVAILABLE
        );
        assert_eq!(
            list(&gone, None, None).unwrap_err().code,
            TabError::UNAVAILABLE
        );
    }

    #[test]
    fn a_read_says_when_there_is_no_such_file() {
        let (folder, root) = project("missing");
        assert_eq!(read(&root, ".env").unwrap_err().code, TabError::NOT_FOUND);
        assert_eq!(
            read(&root, "a/b.txt").unwrap_err().code,
            TabError::NOT_FOUND
        );
        folder.file("file.txt", "x");
        assert_eq!(
            read(&root, "file.txt/inside.txt").unwrap_err().code,
            TabError::NOT_FOUND
        );
        fs::create_dir(folder.root.join("dir")).unwrap();
        assert_eq!(read(&root, "dir").unwrap_err().code, TabError::FAILED);
        assert_eq!(
            read_binary(&root, "shot.png", false).unwrap_err().code,
            TabError::NOT_FOUND
        );
        assert_eq!(
            list(&root, Some("nowhere"), None).unwrap_err().code,
            TabError::NOT_FOUND
        );
        assert_eq!(
            list(&root, Some("file.txt"), None).unwrap_err().code,
            TabError::FAILED
        );
    }

    #[test]
    fn an_image_or_a_pdf_comes_back_with_its_type() {
        let (folder, root) = project("binary");
        let bytes = b"\x89PNG\r\n\x1a\n\xff\xfe\x00";
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        for (name, mime) in [
            ("shots/a.png", "image/png"),
            ("shots/B.PNG", "image/png"),
            ("a.jpg", "image/jpeg"),
            ("a.JpEg", "image/jpeg"),
            ("a.gif", "image/gif"),
            ("a.webp", "image/webp"),
            ("a.avif", "image/avif"),
            ("a.bmp", "image/bmp"),
            ("a.ico", "image/x-icon"),
            ("a.svg", "image/svg+xml"),
            ("doc.pdf", "application/pdf"),
        ] {
            folder.file(name, "");
            fs::write(folder.root.join(name), bytes).unwrap();
            assert_eq!(
                read_binary(&root, name, false).unwrap(),
                BinaryFile {
                    base64: encoded.clone(),
                    mime
                },
                "{name}"
            );
        }
    }

    #[test]
    fn anything_else_is_unsupported_whatever_it_holds() {
        let (folder, root) = project("unsupported");
        folder.file("image.txt", "not read");
        folder.file("noextension", "not read");
        folder.file("archive.png.zip", "not read");
        folder.file("page.html", "<svg/>");
        for path in [
            "image.txt",
            "noextension",
            "archive.png.zip",
            "page.html",
            "absent.exe",
        ] {
            assert_eq!(
                read_binary(&root, path, true).unwrap_err().code,
                TabError::UNSUPPORTED,
                "{path}"
            );
        }
        // A link named like an image is what it leads to.
        folder.file("notes.txt", "private");
        if link(
            &folder.root.join("notes.txt"),
            &folder.root.join("shot.png"),
        ) {
            assert_eq!(
                read_binary(&root, "shot.png", true).unwrap_err().code,
                TabError::UNSUPPORTED
            );
        }
    }

    #[test]
    fn hidden_images_are_read_only_by_a_tab_that_may() {
        let (folder, root) = project("binary-hidden");
        folder.file(".shots/a.png", "png");
        folder.file(".b.png", "png");
        for path in [".shots/a.png", ".b.png"] {
            assert_eq!(
                read_binary(&root, path, false).unwrap_err().code,
                TabError::INVALID,
                "{path}"
            );
            assert_eq!(read_binary(&root, path, true).unwrap().mime, "image/png");
        }
        // Reached through a link that is not hidden itself.
        assert!(link_folder(
            &folder.root.join(".shots"),
            &folder.root.join("shots")
        ));
        let refused = read_binary(&root, "shots/a.png", false).unwrap_err();
        assert!(refused.message.contains("hidden"), "{}", refused.message);
        assert_eq!(
            read_binary(&root, "shots/a.png", true).unwrap().mime,
            "image/png"
        );
        let _ = fs::remove_dir(folder.root.join("shots"));
    }

    #[test]
    fn a_binary_file_is_held_to_ten_megabytes() {
        let (folder, root) = project("binary-size");
        let at_cap = vec![b'a'; MAX_BINARY_BYTES as usize];
        fs::write(folder.root.join("cap.png"), &at_cap).unwrap();
        assert!(read_binary(&root, "cap.png", false).is_ok());
        fs::write(
            folder.root.join("over.png"),
            [at_cap.as_slice(), b"a"].concat(),
        )
        .unwrap();
        let refused = read_binary(&root, "over.png", false).unwrap_err();
        assert_eq!(refused.code, TabError::FAILED);
        assert!(refused.message.contains("10 MB"), "{}", refused.message);
    }

    #[test]
    fn a_listing_gives_paths_sizes_and_dates_dotfiles_included() {
        let (folder, root) = project("list");
        folder.file("b.txt", "bb");
        folder.file(".env", "A=1\n");
        folder.file("src/main.rs", "fn main() {}");
        folder.file(".github/workflows/ci.yml", "on: push");
        let listing = list(&root, None, None).unwrap();
        assert!(!listing.truncated);
        assert_eq!(
            paths(&listing),
            [".env", ".github/workflows/ci.yml", "b.txt", "src/main.rs"]
        );
        let env = &listing.files[0];
        assert_eq!(env.size, 4);
        let now = crate::kits::now_ms();
        assert!(
            env.modified > now - 60_000 && env.modified <= now + 60_000,
            "{}",
            env.modified
        );
        // What it names, the others take.
        for file in &listing.files {
            assert!(read(&root, &file.path).is_ok(), "{}", file.path);
        }
    }

    #[test]
    fn a_listing_of_a_folder_says_where_from_the_workspace() {
        let (folder, root) = project("list-folder");
        folder.file("top.txt", "");
        folder.file("src/a.rs", "");
        folder.file("src/deep/b.rs", "");
        for path in ["src", "src/", "./src", "src\\"] {
            assert_eq!(
                paths(&list(&root, Some(path), None).unwrap()),
                ["src/a.rs", "src/deep/b.rs"],
                "{path}"
            );
        }
        for path in [".", "./"] {
            assert_eq!(list(&root, Some(path), None).unwrap().files.len(), 3);
        }
    }

    #[test]
    fn a_listing_skips_what_it_says_it_skips() {
        let (folder, root) = project("list-skips");
        for path in [
            ".git/config",
            "node_modules/x/index.js",
            "target/debug/app.exe",
            "dist/app.js",
            "packages/ui/node_modules/y/index.js",
            "packages/ui/dist/app.js",
            "crates/core/target/x",
            "crates/.git/config",
        ] {
            folder.file(path, "");
        }
        folder.file("kept.txt", "");
        folder.file("packages/ui/kept.txt", "");
        // A file of that name is a file.
        folder.file("packages/target", "");
        folder.file("packages/dist", "");
        assert_eq!(
            paths(&list(&root, None, None).unwrap()),
            [
                "kept.txt",
                "packages/dist",
                "packages/target",
                "packages/ui/kept.txt"
            ]
        );
        // Skipped on the way down: a folder asked for by name is what was asked.
        assert_eq!(
            paths(&list(&root, Some("dist"), None).unwrap()),
            ["dist/app.js"]
        );
    }

    #[test]
    fn a_listing_is_capped_and_says_so() {
        let (folder, root) = project("list-cap");
        fs::create_dir(folder.root.join("many")).unwrap();
        for index in 0..MAX_LISTED {
            fs::write(folder.root.join(format!("many/{index:05}.txt")), "").unwrap();
        }
        let exact = list(&root, None, None).unwrap();
        assert_eq!(exact.files.len(), MAX_LISTED);
        assert!(!exact.truncated, "as many as it gives is not cut");
        folder.file("top.txt", "");
        let over = list(&root, None, None).unwrap();
        assert_eq!(over.files.len(), MAX_LISTED);
        assert!(over.truncated);
        assert!(
            paths(&over).contains(&"top.txt"),
            "the nearest come first when there are too many"
        );
    }

    #[test]
    fn a_pattern_is_a_simple_glob() {
        let (folder, root) = project("list-glob");
        for path in [
            ".env",
            ".env.local",
            "app.json",
            "config/db.json",
            "config/deep/x.json",
            "config/deep/y.toml",
            "a+b.txt",
            "(x).txt",
        ] {
            folder.file(path, "");
        }
        let found = |pattern: &str, within: Option<&str>| -> Vec<String> {
            list(&root, within, Some(pattern))
                .unwrap()
                .files
                .into_iter()
                .map(|file| file.path)
                .collect()
        };
        assert_eq!(found(".env*", None), [".env", ".env.local"]);
        assert_eq!(
            found("*.json", None),
            ["app.json", "config/db.json", "config/deep/x.json"]
        );
        assert_eq!(found("?.json", None), ["config/deep/x.json"]);
        assert_eq!(found("config/*.json", None), ["config/db.json"]);
        assert_eq!(found("config\\*.json", None), ["config/db.json"]);
        assert_eq!(
            found("config/**/*.json", None),
            ["config/db.json", "config/deep/x.json"]
        );
        assert_eq!(found("**/x.json", None), ["config/deep/x.json"]);
        assert_eq!(found("./config/*.json", None), ["config/db.json"]);
        // From the folder listed.
        assert_eq!(found("deep/*.toml", Some("config")), ["config/deep/y.toml"]);
        assert_eq!(found("*.json", Some("config/deep")), ["config/deep/x.json"]);
        // Nothing else is special, and case counts.
        assert_eq!(found("a+b.txt", None), ["a+b.txt"]);
        assert_eq!(found("(x).txt", None), ["(x).txt"]);
        assert_eq!(found("[ab].json", None), Vec::<String>::new());
        assert_eq!(found("*.JSON", None), Vec::<String>::new());
        assert_eq!(found("*.tom", None), Vec::<String>::new());

        for pattern in ["", "  ", &"a".repeat(MAX_PATTERN_CHARS + 1)] {
            assert_eq!(
                list(&root, None, Some(pattern)).unwrap_err().code,
                TabError::INVALID,
                "{pattern}"
            );
        }
        assert!(list(&root, None, Some(&"*a".repeat(MAX_PATTERN_CHARS / 2))).is_ok());
    }
}
