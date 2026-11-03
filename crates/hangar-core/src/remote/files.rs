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
//!
//! A share is told apart as the PC's preview tells a file apart (preview.rs),
//! and the phone shows it in place by that kind: a picture, a player, a text,
//! a page in its sandbox; anything else it hands over to be saved.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::model::{self, Span};
use crate::preview;

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
    /// The Content-Type `/m/files/{id}` answers with.
    pub mime: String,
    /// The viewer the phone shows it in. A record from before kinds were
    /// told reads as `File`, and is judged again when the index loads.
    #[serde(default)]
    pub kind: Kind,
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
        let mut kept: Vec<Share> = fs::read_to_string(dir.join(INDEX))
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        // A file is judged on its first bytes, so this is quick; and a format
        // the preview learns to show shows up on the phone.
        for share in kept.iter_mut().filter(|share| share.kind == Kind::File) {
            (share.kind, share.mime) = judge(&dir.join(&share.id).join(&share.name));
        }
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
        // The copy, not the original: what is judged is what is served.
        let (kind, mime) = judge(&target);
        let share = Share {
            id,
            at: now,
            workspace: new.workspace,
            pane: new.pane,
            mime,
            kind,
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

/// What the phone shows a share as: the kinds of the PC's preview, but the
/// 3D model, which the phone has no viewer for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Image,
    Video,
    Audio,
    /// Text and source code.
    Text,
    Markdown,
    Pdf,
    Html,
    /// Nothing the phone shows: it is handed over, to be opened or saved.
    #[default]
    File,
}

impl Kind {
    /// The Content-Security-Policy its file goes out with, for when it is
    /// opened as a page of its own as much as for the app's viewers.
    pub fn policy(self) -> Option<&'static str> {
        match self {
            // Scripts, in an origin of their own: no `allow-same-origin`, so
            // never the app's, its cookie's or its storage's.
            Kind::Html => Some("sandbox allow-scripts"),
            // An SVG is a picture here, never a program.
            Kind::Image | Kind::Text | Kind::Markdown => Some("sandbox"),
            // A player or a PDF viewer the policy would only get in the way
            // of: nothing in them runs in a page.
            Kind::Video | Kind::Audio | Kind::Pdf => None,
            Kind::File => Some("default-src 'none'; sandbox"),
        }
    }
}

/// What a file goes out as when the phone does not show it.
const OPAQUE: &str = "application/octet-stream";
const TEXT_PLAIN: &str = "text/plain; charset=utf-8";

/// What a share's copy is, and the Content-Type it is served with, as the
/// PC's preview judges it (preview.rs): by its extension, confirmed on its
/// first bytes, so a `.png` that is a saved error page is a file to open,
/// never a picture, and a page is only ever a page by name and by bytes.
///
/// The preview refuses a text over 10 MB; a share may be 50, and the phone
/// shows the head of any text, so such a text is still one.
fn judge(copy: &Path) -> (Kind, String) {
    let Some(folder) = copy.parent().and_then(|folder| folder.canonicalize().ok()) else {
        return (Kind::File, OPAQUE.into());
    };
    match preview::inspect(copy, &[folder]) {
        Ok(shown) => {
            // What preview.rs tells the window of a file is all it tells
            // anyone, and what the phone is told too: kind and served type.
            let told = serde_json::to_value(preview::PreviewFile::from(shown)).unwrap_or_default();
            match (
                serde_json::from_value::<Kind>(told["kind"].clone()),
                told["mime"].as_str(),
            ) {
                (Ok(kind), Some(mime)) if kind != Kind::File => (kind, mime.to_string()),
                _ => (Kind::File, OPAQUE.into()),
            }
        }
        Err(preview::Refusal::TooBig {
            kind: preview::Kind::Text,
            ..
        }) => (Kind::Text, TEXT_PLAIN.into()),
        Err(preview::Refusal::TooBig {
            kind: preview::Kind::Markdown,
            ..
        }) => (Kind::Markdown, TEXT_PLAIN.into()),
        Err(_) => (Kind::File, OPAQUE.into()),
    }
}

/// The part of a file a request's `Range` header asks for, read.
#[derive(Debug, PartialEq)]
pub enum Part {
    Whole {
        len: u64,
        body: Vec<u8>,
    },
    /// Inclusive, as HTTP counts; `len` is the whole file's.
    Range {
        start: u64,
        end: u64,
        len: u64,
        body: Vec<u8>,
    },
    /// Several ranges, a malformed one, or one past the end: 416.
    Unsatisfiable {
        len: u64,
    },
}

/// Reads what `range` asks of the file at `path` — through the scheme's own
/// reading of a `Range` header (model.rs) — and no byte of it when `body` is
/// false, for a HEAD. A player asks for a range, Safari's before it plays
/// anything at all.
pub fn read_part(path: &Path, range: Option<&str>, body: bool) -> std::io::Result<Part> {
    let len = fs::metadata(path)?.len();
    let read = |start: u64, count: u64| {
        if body {
            model::read_span(path, start, count)
        } else {
            Ok(Vec::new())
        }
    };
    Ok(match model::asked_span(range, len) {
        Some(Span::Whole) => Part::Whole {
            len,
            body: read(0, len)?,
        },
        Some(Span::Part { start, end }) => Part::Range {
            start,
            end,
            len,
            body: read(start, end - start + 1)?,
        },
        Some(Span::Unsatisfiable) | None => Part::Unsatisfiable { len },
    })
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
pub(super) mod tests {
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
        let source = file_of(&work, "shot.png", PNG);
        let shares = Shares::open(scratch("kept"));

        let share = shares.add(new_share(source.clone()), 1_000).unwrap();
        assert_eq!(share.name, "shot.png");
        assert_eq!(share.mime, "image/png");
        assert_eq!(share.kind, Kind::Image);
        assert_eq!(share.size, PNG.len() as u64);
        assert_eq!(share.at, 1_000);
        assert_eq!(share.workspace, "C:/work");
        assert_eq!(share.pane, "Ava");
        assert_eq!(share.caption.as_deref(), Some("the render"));

        // Still there when the agent deletes the original.
        fs::remove_file(&source).unwrap();
        let (found, path) = shares.find(&share.id).unwrap();
        assert_eq!(found, share);
        assert_eq!(fs::read(path).unwrap(), PNG);
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

    /// One file of each kind, as a program writing it starts it.
    pub(crate) const SAMPLES: &[(&str, &[u8], Kind, &str)] = &[
        ("shot.png", PNG, Kind::Image, "image/png"),
        (
            "photo.JPG",
            b"\xFF\xD8\xFF\xE0\0\x10JFIF\0",
            Kind::Image,
            "image/jpeg",
        ),
        (
            "logo.svg",
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
            Kind::Image,
            "image/svg+xml",
        ),
        (
            "clip.mp4",
            b"\0\0\0\x18ftypisom\0\0\x02\0",
            Kind::Video,
            "video/mp4",
        ),
        (
            "take.mov",
            b"\0\0\0\x08wide\0\0\0\x10mdat",
            Kind::Video,
            "video/quicktime",
        ),
        (
            "memo.m4a",
            b"\0\0\0\x20ftypM4A \0\0\0\0",
            Kind::Audio,
            "audio/mp4",
        ),
        (
            "song.mp3",
            b"ID3\x04\0\0\0\0\0\x23",
            Kind::Audio,
            "audio/mpeg",
        ),
        ("build.log", b"ok\n", Kind::Text, TEXT_PLAIN),
        ("main.rs", b"fn main() {}", Kind::Text, TEXT_PLAIN),
        ("data.csv", b"a,b\n1,2\n", Kind::Text, TEXT_PLAIN),
        ("Makefile", b"all:\n\techo hi\n", Kind::Text, TEXT_PLAIN),
        (
            "README.md",
            "# Déjà vu\n".as_bytes(),
            Kind::Markdown,
            TEXT_PLAIN,
        ),
        (
            "report.pdf",
            b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n",
            Kind::Pdf,
            "application/pdf",
        ),
        (
            "page.html",
            b"<!doctype html><script>alert(1)</script>",
            Kind::Html,
            "text/html; charset=utf-8",
        ),
        // No viewer on the phone: a 3D model, an archive, a program.
        ("chair.glb", b"glTF\x02\0\0\0", Kind::File, OPAQUE),
        ("release.zip", b"PK\x03\x04\x14\0\0\0", Kind::File, OPAQUE),
        ("setup.exe", b"MZ\x90\0\x03\0\0\0", Kind::File, OPAQUE),
        // Named as one thing, holding another: never shown as what it says.
        (
            "photo.jpg",
            b"<html><body>404</body></html>",
            Kind::File,
            OPAQUE,
        ),
        ("notes.html", b"\x89PNG\r\n\x1a\n\0\0", Kind::File, OPAQUE),
    ];

    pub(crate) const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

    #[test]
    fn each_share_is_told_apart_as_the_preview_tells_a_file_apart() {
        let work = scratch("kinds");
        let shares = Shares::open(scratch("kept"));
        for (name, bytes, kind, mime) in SAMPLES {
            let share = shares
                .add(new_share(file_of(&work, name, bytes)), 1)
                .unwrap();
            assert_eq!((share.kind, share.mime.as_str()), (*kind, *mime), "{name}");
        }
    }

    /// The preview stops at 10 MB of text; a share goes to 50, and the phone
    /// reads a long one's head.
    #[test]
    fn a_text_past_the_preview_s_cap_is_still_a_text() {
        let work = scratch("long");
        let shares = Shares::open(scratch("kept"));
        let log = file_of(&work, "huge.log", &vec![b'a'; 11 * 1024 * 1024]);
        let notes = file_of(&work, "huge.md", &vec![b'#'; 11 * 1024 * 1024]);
        assert_eq!(shares.add(new_share(log), 1).unwrap().kind, Kind::Text);
        assert_eq!(
            shares.add(new_share(notes), 2).unwrap().kind,
            Kind::Markdown
        );
    }

    /// shares.json from before kinds: each record is judged when it loads.
    #[test]
    fn a_share_recorded_before_kinds_is_judged_when_the_index_loads() {
        let work = scratch("before");
        let dir = scratch("kept");
        let shares = Shares::open(dir.clone());
        let picture = shares
            .add(new_share(file_of(&work, "shot.png", PNG)), 1)
            .unwrap();
        let page = shares
            .add(new_share(file_of(&work, "page.html", b"<p>hi</p>")), 2)
            .unwrap();
        let mut index: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join(INDEX)).unwrap()).unwrap();
        for record in index.as_array_mut().unwrap() {
            record.as_object_mut().unwrap().remove("kind");
            record["mime"] = if record["name"] == "shot.png" {
                "image/png".into()
            } else {
                "text/html".into()
            };
        }
        fs::write(dir.join(INDEX), index.to_string()).unwrap();

        let again = Shares::open(dir);
        assert_eq!(again.find(&picture.id).unwrap().0, picture);
        let (page_again, _) = again.find(&page.id).unwrap();
        assert_eq!(page_again.kind, Kind::Html);
        assert_eq!(page_again.mime, "text/html; charset=utf-8");
    }

    #[test]
    fn each_kind_has_its_policy() {
        assert_eq!(Kind::Html.policy(), Some("sandbox allow-scripts"));
        for kind in [Kind::Image, Kind::Text, Kind::Markdown] {
            assert_eq!(kind.policy(), Some("sandbox"), "{kind:?}");
        }
        for kind in [Kind::Video, Kind::Audio, Kind::Pdf] {
            assert_eq!(kind.policy(), None, "{kind:?}");
        }
        assert_eq!(Kind::File.policy(), Some("default-src 'none'; sandbox"));
        assert_eq!(serde_json::to_value(Kind::Markdown).unwrap(), "markdown");
    }

    #[test]
    fn a_part_is_read_as_a_range_asks_for_it() {
        let work = scratch("part");
        let path = file_of(&work, "voice.wav", b"0123456789");
        let part = |range: Option<&str>| read_part(&path, range, true).unwrap();

        assert_eq!(
            part(None),
            Part::Whole {
                len: 10,
                body: b"0123456789".to_vec()
            }
        );
        assert_eq!(
            part(Some("bytes=0-1")),
            Part::Range {
                start: 0,
                end: 1,
                len: 10,
                body: b"01".to_vec()
            }
        );
        assert_eq!(
            part(Some("bytes=7-")),
            Part::Range {
                start: 7,
                end: 9,
                len: 10,
                body: b"789".to_vec()
            }
        );
        assert_eq!(
            part(Some("bytes=-2")),
            Part::Range {
                start: 8,
                end: 9,
                len: 10,
                body: b"89".to_vec()
            }
        );
        for refused in ["bytes=10-", "bytes=5-2", "bytes=0-1,4-5", "bytes=x-"] {
            assert_eq!(
                part(Some(refused)),
                Part::Unsatisfiable { len: 10 },
                "{refused}"
            );
        }
        // A HEAD: what would be sent, without a byte of it.
        assert_eq!(
            read_part(&path, Some("bytes=2-4"), false).unwrap(),
            Part::Range {
                start: 2,
                end: 4,
                len: 10,
                body: Vec::new()
            }
        );
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
