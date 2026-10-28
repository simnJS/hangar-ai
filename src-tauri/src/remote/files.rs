//! Files both ways between an agent and the phone.
//!
//! - Shares: an agent's `phone_share` copies a file into
//!   `<app data>/remote/shares/<id>/` and records it, so the phone can show it
//!   even after the agent deletes or changes the original. Only the last 200
//!   shares, or 2 GB, are kept.
//! - Uploads: a file the phone sends is saved in the workspace's own folder,
//!   under `.hangar/uploads/`, and its path goes back to the phone.
//!
//! Every name that reaches the disk comes through `sanitize_name`: a phone or
//! an agent does not get to choose a path, only a file name.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// The largest file an agent may share.
pub const MAX_SHARE: u64 = 50 * 1024 * 1024;
/// The largest file a phone may upload.
pub const MAX_UPLOAD: usize = 25 * 1024 * 1024;
const KEEP_SHARES: usize = 200;
const KEEP_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// A caption is a line under a picture, not a report.
const CAPTION_LIMIT: usize = 500;
/// File names are cut to this many characters, extension kept.
const NAME_LIMIT: usize = 100;
const INDEX: &str = "shares.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Share {
    pub id: String,
    /// Milliseconds since the epoch.
    pub at: u64,
    /// The folder of the workspace the agent works in.
    pub workspace: String,
    pub pane: String,
    pub name: String,
    pub mime: String,
    pub size: u64,
    pub caption: Option<String>,
}

pub struct NewShare {
    pub source: PathBuf,
    pub caption: Option<String>,
    pub workspace: String,
    pub pane: String,
}

pub struct Shares {
    dir: PathBuf,
    kept: Mutex<Vec<Share>>,
    max_count: usize,
    max_bytes: u64,
}

impl Shares {
    pub fn open(dir: PathBuf) -> Self {
        Self::with_limits(dir, KEEP_SHARES, KEEP_BYTES)
    }

    fn with_limits(dir: PathBuf, max_count: usize, max_bytes: u64) -> Self {
        let kept = fs::read_to_string(dir.join(INDEX))
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        Self {
            dir,
            kept: Mutex::new(kept),
            max_count,
            max_bytes,
        }
    }

    /// Copies the file in and records it. Nothing is recorded unless the copy
    /// and the index were both written.
    pub fn add(&self, new: NewShare, now: u64) -> Result<Share, String> {
        let meta = fs::metadata(&new.source)
            .map_err(|err| format!("cannot read {}: {err}", new.source.display()))?;
        if !meta.is_file() {
            return Err(format!("{} is not a file", new.source.display()));
        }
        if meta.len() > MAX_SHARE {
            return Err(format!(
                "{} is {} MB; the limit for a share is {} MB",
                new.source.display(),
                meta.len().div_ceil(1024 * 1024),
                MAX_SHARE / (1024 * 1024)
            ));
        }
        let name = sanitize_name(
            &new.source
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
        );
        let id = uuid::Uuid::new_v4().simple().to_string();
        let folder = self.dir.join(&id);
        fs::create_dir_all(&folder)
            .map_err(|err| format!("could not create {}: {err}", folder.display()))?;
        let target = folder.join(&name);
        let copied = fs::copy(&new.source, &target).map_err(|err| {
            let _ = fs::remove_dir_all(&folder);
            format!("could not copy {}: {err}", new.source.display())
        })?;
        let share = Share {
            id,
            at: now,
            workspace: new.workspace,
            pane: new.pane,
            mime: mime_of(&name).to_string(),
            name,
            size: copied,
            caption: new
                .caption
                .map(|caption| {
                    caption
                        .trim()
                        .chars()
                        .take(CAPTION_LIMIT)
                        .collect::<String>()
                })
                .filter(|caption| !caption.is_empty()),
        };

        let mut kept = self.kept.lock().unwrap();
        let mut next = kept.clone();
        next.push(share.clone());
        let dropped = retain(&mut next, self.max_count, self.max_bytes);
        if let Err(err) = self.write(&next) {
            let _ = fs::remove_dir_all(&folder);
            return Err(err);
        }
        *kept = next;
        for old in dropped {
            let _ = fs::remove_dir_all(self.dir.join(old.id));
        }
        Ok(share)
    }

    /// Newest first, those after `since` only.
    pub fn list(&self, since: u64) -> Vec<Share> {
        let mut shares: Vec<Share> = self
            .kept
            .lock()
            .unwrap()
            .iter()
            .filter(|share| share.at > since)
            .cloned()
            .collect();
        shares.sort_by_key(|share| std::cmp::Reverse(share.at));
        shares
    }

    /// A share and where its copy is. The id is looked up, never joined into
    /// a path: what a phone sends cannot name a file of its own.
    pub fn find(&self, id: &str) -> Option<(Share, PathBuf)> {
        let share = self
            .kept
            .lock()
            .unwrap()
            .iter()
            .find(|share| share.id == id)
            .cloned()?;
        let path = self.dir.join(&share.id).join(&share.name);
        Some((share, path))
    }

    fn write(&self, shares: &[Share]) -> Result<(), String> {
        fs::create_dir_all(&self.dir)
            .map_err(|err| format!("could not create {}: {err}", self.dir.display()))?;
        let body = serde_json::to_string_pretty(shares).map_err(|err| err.to_string())?;
        let tmp = self.dir.join("shares.json.tmp");
        fs::write(&tmp, body).map_err(|err| format!("could not save the shares: {err}"))?;
        fs::rename(&tmp, self.dir.join(INDEX))
            .map_err(|err| format!("could not save the shares: {err}"))
    }
}

/// Drops the oldest shares until at most `count` remain, weighing at most
/// `bytes` together — the newest share always stays, however big. Returns
/// those dropped.
fn retain(shares: &mut Vec<Share>, count: usize, bytes: u64) -> Vec<Share> {
    shares.sort_by_key(|share| share.at);
    let mut total: u64 = shares.iter().map(|share| share.size).sum();
    let mut dropped = Vec::new();
    while shares.len() > 1 && (shares.len() > count || total > bytes) {
        let oldest = shares.remove(0);
        total -= oldest.size;
        dropped.push(oldest);
    }
    dropped
}

/// A file name and nothing else: no folder, no `..`, no character Windows or
/// a shell would read as more, and a length a file system accepts.
pub fn sanitize_name(raw: &str) -> String {
    let last = raw.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = last
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches(|c: char| c == '.' || c.is_whitespace());
    if cleaned.is_empty() {
        return "file".to_string();
    }
    if cleaned.chars().count() <= NAME_LIMIT {
        return cleaned.to_string();
    }
    // Over long: the stem is cut, so the extension still says what it is.
    let (stem, ext) = match cleaned.rfind('.') {
        Some(dot) if cleaned.chars().count() - cleaned[..dot].chars().count() <= 16 => {
            cleaned.split_at(dot)
        }
        _ => (cleaned, ""),
    };
    let room = NAME_LIMIT - ext.chars().count();
    let stem: String = stem.chars().take(room).collect();
    format!("{}{ext}", stem.trim_end_matches(['.', ' ']))
}

pub fn mime_of(name: &str) -> &'static str {
    let ext = name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "txt" | "log" | "md" | "csv" | "ts" | "tsx" | "js" | "rs" | "py" | "toml" | "yml"
        | "yaml" => "text/plain",
        "json" => "application/json",
        "html" | "htm" => "text/html",
        "xml" => "application/xml",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

/// Only these are shown in place; a phone is never handed anything that
/// could run in the page.
pub fn inline_image(mime: &str) -> bool {
    matches!(
        mime,
        "image/png" | "image/jpeg" | "image/gif" | "image/webp"
    )
}

/// The Content-Type a file is served with. Whatever a browser could read as a
/// page or a script goes out as plain bytes.
pub fn served_type(mime: &str) -> &'static str {
    match mime {
        "image/png" => "image/png",
        "image/jpeg" => "image/jpeg",
        "image/gif" => "image/gif",
        "image/webp" => "image/webp",
        "application/pdf" => "application/pdf",
        "text/plain" => "text/plain; charset=utf-8",
        "application/json" => "application/json",
        "video/mp4" => "video/mp4",
        "video/webm" => "video/webm",
        "video/quicktime" => "video/quicktime",
        "audio/mpeg" => "audio/mpeg",
        "audio/wav" => "audio/wav",
        "application/zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

/// Saves a phone's file as `<cwd>/.hangar/uploads/<stamp>-<name>`, and
/// returns where. A second of the same stamp and name does not overwrite the
/// first.
pub fn save_upload(cwd: &Path, name: &str, bytes: &[u8], stamp: &str) -> Result<PathBuf, String> {
    if !cwd.is_dir() {
        return Err(format!("{} is not a folder", cwd.display()));
    }
    let dir = cwd.join(".hangar").join("uploads");
    fs::create_dir_all(&dir).map_err(|err| format!("could not create {}: {err}", dir.display()))?;
    // An upload is no part of the project: kept out of its git status.
    let ignore = dir.join(".gitignore");
    if !ignore.exists() {
        let _ = fs::write(&ignore, "*\n");
    }
    let name = sanitize_name(name);
    let mut target = dir.join(format!("{stamp}-{name}"));
    let mut n = 1;
    while target.exists() {
        n += 1;
        target = dir.join(format!("{stamp}-{n}-{name}"));
    }
    fs::write(&target, bytes)
        .map_err(|err| format!("could not save {}: {err}", target.display()))?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hangar-files-{tag}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn file_of(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }

    fn new_share(source: PathBuf) -> NewShare {
        NewShare {
            source,
            caption: Some("  the render  ".into()),
            workspace: "C:/work".into(),
            pane: "Ava".into(),
        }
    }

    #[test]
    fn a_share_is_copied_and_recorded() {
        let work = scratch("work");
        let source = file_of(&work, "shot.png", b"png bytes");
        let shares = Shares::open(scratch("kept"));

        let share = shares.add(new_share(source.clone()), 1_000).unwrap();
        assert_eq!(share.name, "shot.png");
        assert_eq!(share.mime, "image/png");
        assert_eq!(share.size, 9);
        assert_eq!(share.at, 1_000);
        assert_eq!(share.workspace, "C:/work");
        assert_eq!(share.pane, "Ava");
        assert_eq!(share.caption.as_deref(), Some("the render"));

        // Still there when the agent deletes the original.
        fs::remove_file(&source).unwrap();
        let (found, path) = shares.find(&share.id).unwrap();
        assert_eq!(found, share);
        assert_eq!(fs::read(path).unwrap(), b"png bytes");
        assert_eq!(shares.list(0), vec![share.clone()]);
        assert!(shares.list(1_000).is_empty());

        // And across a restart.
        let again = Shares::open(shares.dir.clone());
        assert_eq!(again.list(0), vec![share]);
    }

    #[test]
    fn a_blank_caption_is_none_and_a_long_one_is_cut() {
        let work = scratch("caption");
        let source = file_of(&work, "a.txt", b"a");
        let shares = Shares::open(scratch("kept"));
        let mut blank = new_share(source.clone());
        blank.caption = Some("   ".into());
        assert_eq!(shares.add(blank, 1).unwrap().caption, None);
        let mut long = new_share(source);
        long.caption = Some("x".repeat(900));
        assert_eq!(
            shares.add(long, 2).unwrap().caption.unwrap().len(),
            CAPTION_LIMIT
        );
    }

    #[test]
    fn a_file_over_50_mb_is_refused_and_leaves_nothing() {
        let work = scratch("big");
        let source = work.join("big.bin");
        let big = fs::File::create(&source).unwrap();
        big.set_len(MAX_SHARE + 1).unwrap();
        let dir = scratch("kept");
        let shares = Shares::open(dir.clone());

        let refused = shares.add(new_share(source), 1).unwrap_err();
        assert!(refused.contains("50 MB"), "{refused}");
        assert!(shares.list(0).is_empty());
        assert!(!dir.join(INDEX).exists());

        let exactly = work.join("exact.bin");
        fs::File::create(&exactly)
            .unwrap()
            .set_len(MAX_SHARE)
            .unwrap();
        assert!(shares.add(new_share(exactly), 2).is_ok());
    }

    #[test]
    fn a_folder_or_a_missing_file_is_refused() {
        let work = scratch("nofile");
        let shares = Shares::open(scratch("kept"));
        assert!(shares
            .add(new_share(work.clone()), 1)
            .unwrap_err()
            .contains("not a file"));
        assert!(shares
            .add(new_share(work.join("nope.png")), 1)
            .unwrap_err()
            .contains("cannot read"));
    }

    #[test]
    fn the_oldest_shares_go_past_the_count() {
        let work = scratch("count");
        let source = file_of(&work, "a.txt", b"a");
        let shares = Shares::with_limits(scratch("kept"), 3, u64::MAX);
        let ids: Vec<String> = (1..=5)
            .map(|at| shares.add(new_share(source.clone()), at).unwrap().id)
            .collect();

        let left: Vec<u64> = shares.list(0).iter().map(|share| share.at).collect();
        assert_eq!(left, [5, 4, 3]);
        for gone in &ids[..2] {
            assert!(shares.find(gone).is_none());
            assert!(!shares.dir.join(gone).exists(), "{gone} still on disk");
        }
        for kept in &ids[2..] {
            assert!(shares.find(kept).unwrap().1.exists());
        }
    }

    #[test]
    fn the_oldest_shares_go_past_the_weight_but_the_newest_stays() {
        let work = scratch("weight");
        let source = file_of(&work, "ten.bin", &[0u8; 10]);
        let shares = Shares::with_limits(scratch("kept"), 100, 25);
        for at in 1..=4 {
            shares.add(new_share(source.clone()), at).unwrap();
        }
        let left: Vec<u64> = shares.list(0).iter().map(|share| share.at).collect();
        assert_eq!(left, [4, 3]);

        let alone = Shares::with_limits(scratch("kept"), 100, 5);
        let share = alone.add(new_share(source), 1).unwrap();
        assert_eq!(alone.list(0), vec![share]);
    }

    #[test]
    fn the_retention_defaults_are_200_shares_and_2_gb() {
        let shares = Shares::open(scratch("kept"));
        assert_eq!((shares.max_count, shares.max_bytes), (200, 2 << 30));
    }

    #[test]
    fn names_are_only_names() {
        for (given, expected) in [
            ("report.pdf", "report.pdf"),
            ("../../etc/passwd", "passwd"),
            ("..\\..\\Windows\\win.ini", "win.ini"),
            ("..", "file"),
            ("...", "file"),
            ("", "file"),
            ("/", "file"),
            ("a/b/", "file"),
            (".hidden", "hidden"),
            ("na<me>:x|y?.png", "na_me__x_y_.png"),
            ("tab\tname\0.txt", "tab_name_.txt"),
            ("  spaced  .txt ", "spaced  .txt"),
            ("photo été.jpg", "photo été.jpg"),
        ] {
            assert_eq!(sanitize_name(given), expected, "{given:?}");
        }
    }

    #[test]
    fn a_long_name_is_cut_and_keeps_its_extension() {
        let name = sanitize_name(&format!("{}.jpeg", "é".repeat(400)));
        assert_eq!(name.chars().count(), NAME_LIMIT);
        assert!(name.ends_with(".jpeg"));
        let no_ext = sanitize_name(&"a".repeat(400));
        assert_eq!(no_ext.len(), NAME_LIMIT);
        // A "dot" far from the end is no extension.
        let odd = sanitize_name(&format!("{}.{}", "a".repeat(10), "b".repeat(300)));
        assert_eq!(odd.chars().count(), NAME_LIMIT);
    }

    #[test]
    fn only_four_kinds_of_image_are_shown_in_place() {
        for name in ["a.png", "a.JPG", "a.jpeg", "a.gif", "a.webp"] {
            assert!(inline_image(mime_of(name)), "{name}");
        }
        for name in ["a.svg", "a.html", "a.htm", "a.xml", "a.pdf", "a.txt", "a"] {
            assert!(!inline_image(mime_of(name)), "{name}");
        }
        for risky in ["text/html", "image/svg+xml", "application/xml"] {
            assert_eq!(served_type(risky), "application/octet-stream");
        }
    }

    #[test]
    fn an_upload_lands_under_hangar_uploads_with_its_stamp() {
        let cwd = scratch("upload");
        let path = save_upload(&cwd, "../../x/IMG 1.jpg", b"jpeg", "2026-10-28-203800").unwrap();
        assert_eq!(
            path,
            cwd.join(".hangar")
                .join("uploads")
                .join("2026-10-28-203800-IMG 1.jpg")
        );
        assert_eq!(fs::read(&path).unwrap(), b"jpeg");
        assert_eq!(
            fs::read_to_string(cwd.join(".hangar/uploads/.gitignore")).unwrap(),
            "*\n"
        );
        // The same second and name: a file of its own, the first untouched.
        let second = save_upload(&cwd, "IMG 1.jpg", b"other", "2026-10-28-203800").unwrap();
        assert_ne!(second, path);
        assert_eq!(fs::read(&path).unwrap(), b"jpeg");
        assert!(second.starts_with(cwd.join(".hangar/uploads")));
    }

    #[test]
    fn an_upload_into_nothing_is_refused() {
        let cwd = scratch("upload-none");
        let missing = cwd.join("nope");
        assert!(save_upload(&missing, "a.png", b"x", "s")
            .unwrap_err()
            .contains("not a folder"));
        assert!(!missing.exists());
    }
}
