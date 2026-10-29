//! Every file an agent can show the person in the preview panel: images,
//! video, audio, PDF, Markdown, HTML pages, text and source code, beside the
//! 3D models of model.rs.
//!
//! They come through the same `hangar-model` scheme, from the same allowed
//! folders and through the same path checks. model.rs answers a request for
//! one of a model's files exactly as it always has and hands every other one
//! here, where a file is served as what it is:
//!
//! - its kind is read from its extension and confirmed on its first bytes, so
//!   a `.jpg` that is a saved error page, or a `.json` that is binary, is
//!   refused rather than shown as what its name claims;
//! - text, code and Markdown always go out as `text/plain` with `nosniff`:
//!   the webview shows them, it never renders or runs them;
//! - an HTML page or an SVG goes out with a `sandbox` Content-Security-Policy.
//!
//! The sandbox is what keeps a page away from Hangar. Tauri counts the scheme
//! among the app's own origins (`is_local_url` in tauri's webview), so a page
//! served from it as it is would stand where Hangar's own page stands. The
//! policy gives it an opaque origin instead, wherever the document is opened —
//! the panel's iframe, a popup it opens, a window of its own:
//!
//! - its IPC requests carry `Origin: null`, which Tauri refuses before it
//!   reads anything else, and every IPC request must carry an invoke key that
//!   only Tauri's main-frame script holds, beside `__TAURI_INTERNALS__`: a
//!   frame of another origin can read neither from its parent;
//! - it has no storage of its own, and cannot reach Hangar's;
//! - its `fetch` carries `Origin: null` too, which model.rs refuses for every
//!   file, inside the allowed folders or not. What it loads without CORS —
//!   the images beside it, a video — it can show but never read, and a script
//!   or a stylesheet beside it goes out as `text/plain` with `nosniff`, which
//!   no webview runs or applies: a page's scripts and styles must be inline.

use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::http::{header, response::Builder, Method, Request, Response, StatusCode};

use crate::model::{self, Accept, Span, MAX_BYTES, MEGABYTE};

/// What the panel shows a file as.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Model,
    Image,
    Video,
    Audio,
    Text,
    Markdown,
    Pdf,
    Html,
}

impl Kind {
    /// The largest file of this kind the preview takes. Video and audio have
    /// none: a player reads them a range at a time (see `CHUNK`). A larger
    /// text is refused, never cut to its head: a log shown without its end
    /// would look complete when it is not.
    fn cap(self) -> Option<u64> {
        match self {
            Kind::Model | Kind::Pdf => Some(MAX_BYTES),
            // A page made to be sent alone carries its pictures inside it.
            Kind::Image | Kind::Html => Some(64 * MEGABYTE),
            Kind::Text | Kind::Markdown => Some(10 * MEGABYTE),
            Kind::Video | Kind::Audio => None,
        }
    }

    fn streamed(self) -> bool {
        self.cap().is_none()
    }

    fn noun(self) -> &'static str {
        match self {
            Kind::Model => "a 3D model",
            Kind::Image => "an image",
            Kind::Video => "a video",
            Kind::Audio => "an audio file",
            Kind::Text => "a text file",
            Kind::Markdown => "a Markdown file",
            Kind::Pdf => "a PDF",
            Kind::Html => "an HTML page",
        }
    }
}

/// How the first bytes of a format begin.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Magic {
    Png,
    Jpeg,
    Gif,
    Webp,
    Bmp,
    /// MP4, QuickTime, M4A and AVIF: ISO base media, boxes from the first byte.
    IsoMedia,
    Webm,
    Mp3,
    Wav,
    Ogg,
    Flac,
    Pdf,
    Svg,
    /// UTF-8 without a NUL byte.
    Text,
}

impl Magic {
    /// `truncated`: `head` stops before the end of the file.
    fn matches(self, head: &[u8], truncated: bool) -> bool {
        let riff = |form: &[u8]| head.starts_with(b"RIFF") && head.get(8..12) == Some(form);
        match self {
            Magic::Png => head.starts_with(b"\x89PNG\r\n\x1a\n"),
            Magic::Jpeg => head.starts_with(&[0xFF, 0xD8, 0xFF]),
            Magic::Gif => head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a"),
            Magic::Webp => riff(b"WEBP"),
            // "BM" alone starts plenty of text, so the size of the header that
            // follows must be one a bitmap has.
            Magic::Bmp => {
                head.starts_with(b"BM")
                    && head.get(14..18).is_some_and(|size| {
                        let size = u32::from_le_bytes([size[0], size[1], size[2], size[3]]);
                        [12, 40, 52, 56, 64, 108, 124].contains(&size)
                    })
            }
            // `ftyp` is the first box of every MP4, M4A and AVIF; a QuickTime
            // movie may start with any of the others.
            Magic::IsoMedia => head.get(4..8).is_some_and(|kind| {
                [
                    b"ftyp", b"moov", b"mdat", b"free", b"skip", b"wide", b"pnot",
                ]
                .iter()
                .any(|box_type| kind == box_type.as_slice())
            }),
            Magic::Webm => head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]),
            // A tag, or straight into the first frame's sync bits.
            Magic::Mp3 => {
                head.starts_with(b"ID3")
                    || matches!(head, [0xFF, second, ..] if second & 0xE0 == 0xE0)
            }
            Magic::Wav => riff(b"WAVE"),
            Magic::Ogg => head.starts_with(b"OggS"),
            Magic::Flac => head.starts_with(b"fLaC"),
            // Readers accept the header anywhere in the first kilobyte.
            Magic::Pdf => head[..head.len().min(1024)]
                .windows(5)
                .any(|window| window == b"%PDF-"),
            Magic::Svg => {
                is_text(head, truncated)
                    && head
                        .windows(4)
                        .any(|window| window.eq_ignore_ascii_case(b"<svg"))
            }
            Magic::Text => is_text(head, truncated),
        }
    }
}

/// Text the panel can show as UTF-8. A head cut in the middle of a character
/// is still text; a NUL byte never is, whatever the encoding claims.
fn is_text(head: &[u8], truncated: bool) -> bool {
    if head.contains(&0) {
        return false;
    }
    match std::str::from_utf8(head) {
        Ok(_) => true,
        Err(err) => truncated && err.error_len().is_none(),
    }
}

/// What text goes out as, code and Markdown included.
const TEXT_PLAIN: &str = "text/plain; charset=utf-8";

const SVG: &str = "image/svg+xml";

/// What a page and an SVG are served with: scripts, forms and popups, in an
/// origin of their own — no `allow-same-origin`, so never the scheme's.
const PAGE_POLICY: &str = "sandbox allow-scripts allow-forms allow-popups";

/// What text is served with: nothing to run, even if a webview took it for a
/// page.
const TEXT_POLICY: &str = "sandbox";

/// The formats the preview shows, by extension: their kind, the Content-Type
/// they go out as, and what their first bytes must be.
const FORMATS: &[(&str, Kind, &str, Magic)] = &[
    ("png", Kind::Image, "image/png", Magic::Png),
    ("jpg", Kind::Image, "image/jpeg", Magic::Jpeg),
    ("jpeg", Kind::Image, "image/jpeg", Magic::Jpeg),
    ("gif", Kind::Image, "image/gif", Magic::Gif),
    ("webp", Kind::Image, "image/webp", Magic::Webp),
    ("svg", Kind::Image, SVG, Magic::Svg),
    ("bmp", Kind::Image, "image/bmp", Magic::Bmp),
    ("avif", Kind::Image, "image/avif", Magic::IsoMedia),
    ("mp4", Kind::Video, "video/mp4", Magic::IsoMedia),
    ("m4v", Kind::Video, "video/mp4", Magic::IsoMedia),
    ("mov", Kind::Video, "video/quicktime", Magic::IsoMedia),
    ("webm", Kind::Video, "video/webm", Magic::Webm),
    ("mp3", Kind::Audio, "audio/mpeg", Magic::Mp3),
    ("wav", Kind::Audio, "audio/wav", Magic::Wav),
    ("ogg", Kind::Audio, "audio/ogg", Magic::Ogg),
    ("m4a", Kind::Audio, "audio/mp4", Magic::IsoMedia),
    ("flac", Kind::Audio, "audio/flac", Magic::Flac),
    ("pdf", Kind::Pdf, "application/pdf", Magic::Pdf),
    ("md", Kind::Markdown, TEXT_PLAIN, Magic::Text),
    ("markdown", Kind::Markdown, TEXT_PLAIN, Magic::Text),
    ("html", Kind::Html, "text/html; charset=utf-8", Magic::Text),
    ("htm", Kind::Html, "text/html; charset=utf-8", Magic::Text),
];

/// Text by name. Any other file is text too when its bytes are, but these are
/// held to it: a binary `.json` is refused rather than shown as nothing.
const TEXT_EXTENSIONS: &[&str] = &[
    "txt", "log", "json", "jsonc", "yaml", "yml", "toml", "csv", "xml", "ts", "tsx", "js", "jsx",
    "mjs", "cjs", "rs", "py", "go", "java", "kt", "c", "h", "cpp", "hpp", "cs", "rb", "php", "sh",
    "ps1", "bat", "css", "scss", "sql", "lua", "verse", "gd", "swift", "dart", "vue", "svelte",
];

/// Enough of a file to tell what it is: a format's signature sits in its first
/// bytes, and a text file is judged on this much of it.
const HEAD_BYTES: u64 = 64 * 1024;

/// The most of a video or an audio file one response carries. A player asks
/// for `bytes=0-` and is given this much, then asks for the next part as it
/// plays: HTTP lets a server send less than an open range asks for, and
/// Tauri's responder holds each body whole in memory.
const CHUNK: u64 = 8 * MEGABYTE;

/// What a name says a file is, before its bytes are read.
enum Named {
    Format(Kind, &'static str, Magic),
    /// Not a name the preview knows: text if its bytes are.
    Unknown,
    /// One of a model's other files — a buffer, a texture the webview cannot
    /// draw — served beside the model by model.rs, and nothing to show alone.
    ModelPart,
}

fn named(extension: Option<&str>) -> Named {
    let Some(extension) = extension else {
        return Named::Unknown;
    };
    if let Some((_, kind, mime, magic)) = FORMATS.iter().find(|(known, ..)| *known == extension) {
        return Named::Format(*kind, mime, *magic);
    }
    if TEXT_EXTENSIONS.contains(&extension) {
        return Named::Format(Kind::Text, TEXT_PLAIN, Magic::Text);
    }
    if model::content_type(extension).is_some() {
        return Named::ModelPart;
    }
    Named::Unknown
}

/// A file the preview may show.
#[derive(Debug)]
pub(crate) struct Shown {
    /// Canonical: symlinks followed, `..` resolved.
    path: PathBuf,
    kind: Kind,
    /// The Content-Type the scheme serves it with.
    mime: &'static str,
    size: u64,
    modified_ms: u64,
}

impl Shown {
    /// A page or an SVG: markup a webview would run scripts in.
    fn is_markup(&self) -> bool {
        self.kind == Kind::Html || self.mime == SVG
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum Refusal {
    /// A model, refused the way model_show refuses it.
    Model(model::Refusal),
    Relative,
    Missing,
    NotAFile,
    Outside,
    /// A name that leads, through a link, to a file of another type.
    Retyped(PathBuf),
    /// No kind the panel shows.
    Unsupported,
    /// Named as one kind, holding something else.
    Mismatch(Kind),
    TooBig {
        kind: Kind,
        size: u64,
        cap: u64,
    },
    Unreadable(String),
}

fn listed(extensions: impl IntoIterator<Item = &'static str>) -> String {
    extensions
        .into_iter()
        .map(|extension| format!(".{extension}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn extensions_of(kind: Kind) -> String {
    listed(
        FORMATS
            .iter()
            .filter(|(_, of, ..)| *of == kind)
            .map(|(extension, ..)| *extension),
    )
}

impl Refusal {
    /// What `preview_inspect` says, and so what an agent hears from
    /// preview_show: what is wrong, then what to do about it.
    fn explain(&self, path: &str) -> String {
        match self {
            Refusal::Model(refusal) => refusal.explain(path),
            Refusal::Relative => format!(
                "{path} is not an absolute path: give the file's full path, or one relative to \
                 your working directory."
            ),
            Refusal::Missing => format!(
                "There is no file at {path}: check the path, and save or export the file before \
                 showing it."
            ),
            Refusal::NotAFile => {
                format!("{path} is a folder: give the path of a file inside it.")
            }
            Refusal::Outside => format!(
                "{path} is outside this workspace's folders: copy it into the workspace or one of \
                 its worktrees."
            ),
            Refusal::Retyped(target) => format!(
                "{path} is a link to {}, a file of another type: show that file itself.",
                dunce::simplified(target).display()
            ),
            Refusal::Unsupported => format!(
                "{path} is not a file the preview shows: it shows images ({}), video ({}), audio \
                 ({}), PDF, Markdown, HTML pages, text and source code in UTF-8, and 3D models \
                 ({}). Convert it to one of those, or render it to an image, and show that file.",
                extensions_of(Kind::Image),
                extensions_of(Kind::Video),
                extensions_of(Kind::Audio),
                listed(model::MODEL_EXTENSIONS.iter().copied()),
            ),
            Refusal::Mismatch(kind @ (Kind::Text | Kind::Markdown | Kind::Html)) => format!(
                "{path} is named as {} but holds binary data, and the preview shows text only in \
                 UTF-8: convert it, or show it under the extension of what it really is.",
                kind.noun()
            ),
            Refusal::Mismatch(kind) => format!(
                "{path} is named as {} but its first bytes are not those of one: check what wrote \
                 it (a download that saved an error page, say), or show it under the extension \
                 of what it really is.",
                kind.noun()
            ),
            Refusal::TooBig { kind, size, cap } => format!(
                "{path} is {} MB, over the preview's {} MB limit for {}: {}",
                size.div_ceil(MEGABYTE),
                cap / MEGABYTE,
                kind.noun(),
                match kind {
                    Kind::Image =>
                        "scale it down or save it compressed (JPEG, WebP) and show \
                                    the smaller file.",
                    Kind::Pdf =>
                        "split it, or render the pages that matter to images, and show \
                                  those.",
                    Kind::Html =>
                        "save its pictures beside it rather than inside it, and show \
                                   the lighter page.",
                    _ =>
                        "the preview does not show part of a file, so save the part worth \
                          reading (the end of a log, say) to a file of its own and show that.",
                }
            ),
            Refusal::Unreadable(err) => format!(
                "{path} cannot be read ({err}): check that no other program holds it locked, then \
                 show it again."
            ),
        }
    }

    /// What the scheme answers: a status, and a reason short enough for the
    /// panel to show as it is.
    fn status(&self) -> (StatusCode, String) {
        match self {
            Refusal::Model(refusal) => refusal.status(),
            Refusal::Relative => (StatusCode::BAD_REQUEST, "not an absolute path".into()),
            Refusal::Missing => (StatusCode::NOT_FOUND, "no such file".into()),
            Refusal::NotAFile => (StatusCode::NOT_FOUND, "not a file".into()),
            Refusal::Outside => (
                StatusCode::FORBIDDEN,
                "outside the folders the preview may read".into(),
            ),
            Refusal::Retyped(_) => (
                StatusCode::FORBIDDEN,
                "a link to a file of another type".into(),
            ),
            Refusal::Unsupported => (StatusCode::FORBIDDEN, "this file type is not served".into()),
            Refusal::Mismatch(kind) => (
                StatusCode::FORBIDDEN,
                format!("not {}, whatever its name says", kind.noun()),
            ),
            Refusal::TooBig { cap, .. } => (
                StatusCode::FORBIDDEN,
                format!("larger than the {} MB the preview reads", cap / MEGABYTE),
            ),
            Refusal::Unreadable(err) => (StatusCode::FORBIDDEN, format!("unreadable: {err}")),
        }
    }
}

fn read_head(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut head = Vec::new();
    File::open(path)?.take(HEAD_BYTES).read_to_end(&mut head)?;
    Ok(head)
}

/// Judges one file against the allowed folders, the kinds and their caps.
///
/// A model is judged by model.rs, as model_show judges it. Any other name is
/// read twice, like a model's: on the name asked for, because a type the
/// preview never shows is worth hearing whether or not the file exists; and
/// on the file it leads to, which must have the same extension, so that what
/// the panel was told and what the scheme serves can never differ.
pub(crate) fn inspect(requested: &Path, roots: &[PathBuf]) -> Result<Shown, Refusal> {
    if !requested.is_absolute() {
        return Err(Refusal::Relative);
    }
    if Accept::Model.allows(requested) {
        let checked =
            model::inspect(requested, roots, Accept::Model, MAX_BYTES).map_err(Refusal::Model)?;
        let mime = model::extension(&checked.path)
            .and_then(|extension| model::content_type(&extension))
            .unwrap_or("application/octet-stream");
        return Ok(Shown {
            path: checked.path,
            kind: Kind::Model,
            mime,
            size: checked.size,
            modified_ms: checked.modified_ms,
        });
    }
    let extension = model::extension(requested);
    let named = named(extension.as_deref());
    if let Named::ModelPart = named {
        return Err(Refusal::Unsupported);
    }
    let path = requested.canonicalize().map_err(|err| match err.kind() {
        ErrorKind::NotFound => Refusal::Missing,
        _ => Refusal::Unreadable(err.to_string()),
    })?;
    // Component-wise, so an allowed `proj` does not let `proj-secrets` in.
    if !roots.iter().any(|root| path.starts_with(root)) {
        return Err(Refusal::Outside);
    }
    if model::extension(&path) != extension {
        return Err(Refusal::Retyped(path));
    }
    let metadata = path
        .metadata()
        .map_err(|err| Refusal::Unreadable(err.to_string()))?;
    if !metadata.is_file() {
        return Err(Refusal::NotAFile);
    }
    let size = metadata.len();
    let head = read_head(&path).map_err(|err| Refusal::Unreadable(err.to_string()))?;
    let truncated = (head.len() as u64) < size;
    let (kind, mime) = match named {
        Named::Format(kind, mime, magic) if magic.matches(&head, truncated) => (kind, mime),
        Named::Format(kind, ..) => return Err(Refusal::Mismatch(kind)),
        _ if is_text(&head, truncated) => (Kind::Text, TEXT_PLAIN),
        _ => return Err(Refusal::Unsupported),
    };
    if let Some(cap) = kind.cap().filter(|cap| size > *cap) {
        return Err(Refusal::TooBig { kind, size, cap });
    }
    Ok(Shown {
        path,
        kind,
        mime,
        size,
        modified_ms: model::modified_ms(&metadata),
    })
}

/// What `previewInspect` in the window receives.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewFile {
    /// Canonical, in the platform's everyday form, as `model_stat` gives it:
    /// the string the panel builds the file's URL from.
    path: String,
    kind: Kind,
    /// The Content-Type the file's URL answers with.
    mime: &'static str,
    size: u64,
    /// The file's name, for its tab.
    name: String,
    /// Epoch milliseconds, for the panel to notice a rewritten file.
    modified_ms: u64,
}

impl From<Shown> for PreviewFile {
    fn from(shown: Shown) -> Self {
        PreviewFile {
            name: shown
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            path: dunce::simplified(&shown.path)
                .to_string_lossy()
                .into_owned(),
            kind: shown.kind,
            mime: shown.mime,
            size: shown.size,
            modified_ms: shown.modified_ms,
        }
    }
}

/// What a file the window is about to show is, or the reason it may not show
/// it. The folders are those `model_allow_roots` last allowed, as for a
/// model.
#[tauri::command]
pub async fn preview_inspect(path: String) -> Result<PreviewFile, String> {
    let roots = model::allowed_roots();
    crate::git::off_thread(move || {
        inspect(&model::native_path(path.trim()), &roots)
            .map(PreviewFile::from)
            .map_err(|refusal| refusal.explain(&path))
    })
    .await
}

/// `?as=text`: a page's or an SVG's source, to read rather than to run.
fn wants_source(query: Option<&str>) -> bool {
    query.is_some_and(|query| query.split('&').any(|pair| pair == "as=text"))
}

/// Answers a request for `requested`, a file that is not one of a model's.
///
/// model.rs has checked the method and the page asking, and `base` starts the
/// response as it starts its own: who may read it, and no caching.
pub(crate) fn answer(
    request: &Request<Vec<u8>>,
    requested: &Path,
    roots: &[PathBuf],
    base: &dyn Fn() -> Builder,
) -> Response<Vec<u8>> {
    let refuse = |status: StatusCode, reason: String| {
        model::finish(
            base()
                .status(status)
                .header(header::CONTENT_TYPE, TEXT_PLAIN)
                .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            reason.into_bytes(),
        )
    };
    let shown = match inspect(requested, roots) {
        Ok(shown) => shown,
        Err(refusal) => {
            let (status, reason) = refusal.status();
            return refuse(status, reason);
        }
    };
    let source = shown.is_markup() && wants_source(request.uri().query());

    let len = shown.size;
    let range = request
        .headers()
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok());
    let reads_body = request.method() != Method::HEAD;
    let (status, start, count) = match model::asked_span(range, len) {
        // A video has no cap, and a body is held whole: past the models' cap,
        // only a range is read.
        Some(Span::Whole) if shown.kind.streamed() && len > MAX_BYTES && reads_body => {
            return refuse(
                StatusCode::FORBIDDEN,
                format!(
                    "larger than the {} MB read in one piece: ask for a byte range",
                    MAX_BYTES / MEGABYTE
                ),
            )
        }
        Some(Span::Whole) => (StatusCode::OK, 0, len),
        Some(Span::Part { start, end }) => {
            let end = if shown.kind.streamed() {
                end.min(start.saturating_add(CHUNK - 1))
            } else {
                end
            };
            (StatusCode::PARTIAL_CONTENT, start, end - start + 1)
        }
        // Several ranges, a malformed one, or one past the end. A model's
        // file would be sent whole; a video, which has no cap, cannot be.
        Some(Span::Unsatisfiable) | None => {
            return model::finish(
                base()
                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                    .header(header::CONTENT_RANGE, format!("bytes */{len}"))
                    .header(header::ACCEPT_RANGES, "bytes")
                    .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
                Vec::new(),
            )
        }
    };

    let body = if reads_body {
        match model::read_span(&shown.path, start, count) {
            // Shorter than its metadata said: rewritten while it was read.
            Ok(body) if body.len() as u64 != count => {
                return refuse(
                    StatusCode::CONFLICT,
                    "the file changed while it was read".into(),
                )
            }
            Ok(body) => body,
            Err(err) => return refuse(StatusCode::FORBIDDEN, format!("unreadable: {err}")),
        }
    } else {
        Vec::new()
    };

    let content_type = if source { TEXT_PLAIN } else { shown.mime };
    let policy = if shown.is_markup() && !source {
        Some(PAGE_POLICY)
    } else if content_type == TEXT_PLAIN {
        Some(TEXT_POLICY)
    } else {
        None
    };
    let mut builder = base()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, count)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(
            header::ACCESS_CONTROL_EXPOSE_HEADERS,
            "Content-Range, Accept-Ranges",
        )
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    if let Some(policy) = policy {
        builder = builder.header(header::CONTENT_SECURITY_POLICY, policy);
    }
    if status == StatusCode::PARTIAL_CONTENT {
        builder = builder.header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{}/{len}", start + count - 1),
        );
    }
    model::finish(builder, body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::{get, header_of, link, origins, roots_of, temp_dir, url_path, write};
    use std::io::Write;

    const HTML: &str = "text/html; charset=utf-8";

    /// One file of each format, as a program writing that format starts it.
    const SAMPLES: &[(&str, &[u8], Kind, &str)] = &[
        (
            "chair.glb",
            b"glTF\x02\0\0\0",
            Kind::Model,
            "model/gltf-binary",
        ),
        (
            "shot.png",
            b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR",
            Kind::Image,
            "image/png",
        ),
        (
            "photo.jpg",
            b"\xFF\xD8\xFF\xE0\0\x10JFIF\0",
            Kind::Image,
            "image/jpeg",
        ),
        (
            "photo.JPEG",
            b"\xFF\xD8\xFF\xDB\0\x43",
            Kind::Image,
            "image/jpeg",
        ),
        (
            "anim.gif",
            b"GIF89a\x01\0\x01\0\x80",
            Kind::Image,
            "image/gif",
        ),
        (
            "pic.webp",
            b"RIFF\x1a\0\0\0WEBPVP8 ",
            Kind::Image,
            "image/webp",
        ),
        (
            "logo.svg",
            b"<?xml version=\"1.0\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
            Kind::Image,
            SVG,
        ),
        (
            "old.bmp",
            b"BM\x46\0\0\0\0\0\0\0\x36\0\0\0\x28\0\0\0",
            Kind::Image,
            "image/bmp",
        ),
        (
            "next.avif",
            b"\0\0\0\x1cftypavif\0\0\0\0",
            Kind::Image,
            "image/avif",
        ),
        (
            "clip.mp4",
            b"\0\0\0\x18ftypisom\0\0\x02\0",
            Kind::Video,
            "video/mp4",
        ),
        (
            "clip.m4v",
            b"\0\0\0\x18ftypM4V \0\0\0\x01",
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
            "clip.webm",
            b"\x1a\x45\xdf\xa3\x9f\x42\x86\x81",
            Kind::Video,
            "video/webm",
        ),
        (
            "song.mp3",
            b"ID3\x04\0\0\0\0\0\x23",
            Kind::Audio,
            "audio/mpeg",
        ),
        (
            "raw.mp3",
            b"\xFF\xFB\x90\x64\0\0",
            Kind::Audio,
            "audio/mpeg",
        ),
        (
            "voice.wav",
            b"RIFF\x24\0\0\0WAVEfmt ",
            Kind::Audio,
            "audio/wav",
        ),
        ("track.ogg", b"OggS\0\x02\0\0", Kind::Audio, "audio/ogg"),
        (
            "memo.m4a",
            b"\0\0\0\x20ftypM4A \0\0\0\0",
            Kind::Audio,
            "audio/mp4",
        ),
        ("master.flac", b"fLaC\0\0\0\x22", Kind::Audio, "audio/flac"),
        (
            "report.pdf",
            b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n",
            Kind::Pdf,
            "application/pdf",
        ),
        // Some writers put a few bytes before the header; readers allow it.
        (
            "scan.pdf",
            b"\r\n\0\x01%PDF-1.4\n",
            Kind::Pdf,
            "application/pdf",
        ),
        (
            "README.md",
            "# Déjà vu\n".as_bytes(),
            Kind::Markdown,
            TEXT_PLAIN,
        ),
        (
            "notes.markdown",
            b"- one\n- two\n",
            Kind::Markdown,
            TEXT_PLAIN,
        ),
        ("page.html", b"<!doctype html><p>hi</p>", Kind::Html, HTML),
        ("old.HTM", b"<html><body>hi", Kind::Html, HTML),
        ("build.log", b"ok\n", Kind::Text, TEXT_PLAIN),
        ("data.json", b"{\"a\": 1}", Kind::Text, TEXT_PLAIN),
        (
            "feed.xml",
            b"<?xml version=\"1.0\"?><a/>",
            Kind::Text,
            TEXT_PLAIN,
        ),
        ("main.rs", b"fn main() {}", Kind::Text, TEXT_PLAIN),
        ("app.js", b"alert(1)", Kind::Text, TEXT_PLAIN),
        ("deploy.ps1", b"Write-Host hi", Kind::Text, TEXT_PLAIN),
        (
            "game.verse",
            b"using { /Verse.org }",
            Kind::Text,
            TEXT_PLAIN,
        ),
        ("App.svelte", b"<script></script>", Kind::Text, TEXT_PLAIN),
        ("empty.txt", b"", Kind::Text, TEXT_PLAIN),
        // Names the preview does not know: text, because their bytes are.
        ("Makefile", b"all:\n\techo hi\n", Kind::Text, TEXT_PLAIN),
        (".env", b"TOKEN=1\n", Kind::Text, TEXT_PLAIN),
        (
            "notes.weird",
            "plain UTF-8, ünïcode".as_bytes(),
            Kind::Text,
            TEXT_PLAIN,
        ),
    ];

    fn sample(root: &Path, name: &str) -> PathBuf {
        let (_, body, ..) = SAMPLES.iter().find(|(known, ..)| *known == name).unwrap();
        write(root, name, body)
    }

    /// A file of `len` bytes that starts with `head`, made without writing
    /// them all.
    fn sized(root: &Path, name: &str, head: &[u8], len: u64) -> PathBuf {
        let path = root.join(name);
        let mut file = File::create(&path).expect("create");
        file.write_all(head).expect("head");
        file.set_len(len).expect("length");
        path
    }

    /// Through the scheme's own entry point, as the panel asks for it.
    fn fetch(
        path: &Path,
        query: &str,
        range: Option<&str>,
        roots: &[PathBuf],
    ) -> Response<Vec<u8>> {
        let url = format!("{}{query}", url_path(&path.canonicalize().unwrap()));
        model::answer(&get(&url, None, range), roots, &origins())
    }

    #[test]
    fn every_kind_is_told_by_its_name_and_its_bytes() {
        let root = temp_dir("preview-kinds");
        let roots = roots_of(&[&root]);
        let mut seen = Vec::new();
        for (name, body, kind, mime) in SAMPLES {
            let file = write(&root, name, body);
            let shown = inspect(&file, &roots).unwrap_or_else(|err| panic!("{name}: {err:?}"));
            assert_eq!((shown.kind, shown.mime), (*kind, *mime), "{name}");
            assert_eq!(shown.size, body.len() as u64, "{name}");
            assert_eq!(shown.path, file.canonicalize().unwrap(), "{name}");
            seen.push(*kind);
        }
        for kind in [
            Kind::Model,
            Kind::Image,
            Kind::Video,
            Kind::Audio,
            Kind::Text,
            Kind::Markdown,
            Kind::Pdf,
            Kind::Html,
        ] {
            assert!(seen.contains(&kind), "no sample of {kind:?}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The contract the panel reads: these names, these spellings.
    #[test]
    fn the_window_is_told_path_kind_mime_size_and_name() {
        let root = temp_dir("preview-answer");
        let roots = roots_of(&[&root]);
        let file = sample(&root, "README.md");

        let answer =
            serde_json::to_value(PreviewFile::from(inspect(&file, &roots).unwrap())).unwrap();

        assert_eq!(answer["kind"], "markdown");
        assert_eq!(answer["mime"], TEXT_PLAIN);
        assert_eq!(answer["size"], "# Déjà vu\n".len());
        assert_eq!(answer["name"], "README.md");
        assert_eq!(
            answer["path"],
            dunce::simplified(&file.canonicalize().unwrap())
                .to_string_lossy()
                .as_ref()
        );
        assert!(answer["modifiedMs"].as_u64().unwrap() > 0);
        for (kind, spelled) in [
            (Kind::Model, "model"),
            (Kind::Pdf, "pdf"),
            (Kind::Html, "html"),
        ] {
            assert_eq!(serde_json::to_value(kind).unwrap(), spelled);
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_file_whose_bytes_belie_its_name_is_refused() {
        let root = temp_dir("preview-mismatch");
        let roots = roots_of(&[&root]);
        let cases: &[(&str, &[u8], Refusal)] = &[
            (
                "data.json",
                b"{\"a\":\0\x01\x02}",
                Refusal::Mismatch(Kind::Text),
            ),
            (
                "page.html",
                b"\x89PNG\r\n\x1a\n\0\0",
                Refusal::Mismatch(Kind::Html),
            ),
            // Latin-1, not UTF-8.
            (
                "notes.md",
                b"caf\xe9 cr\xe8me",
                Refusal::Mismatch(Kind::Markdown),
            ),
            // What curl saves when the server answered with an error page.
            (
                "photo.jpg",
                b"<html><body>404</body></html>",
                Refusal::Mismatch(Kind::Image),
            ),
            (
                "logo.svg",
                b"<html>not a drawing</html>",
                Refusal::Mismatch(Kind::Image),
            ),
            (
                "next.avif",
                b"\x89PNG\r\n\x1a\n",
                Refusal::Mismatch(Kind::Image),
            ),
            (
                "old.bmp",
                b"BMW is a car, not a bitmap",
                Refusal::Mismatch(Kind::Image),
            ),
            (
                "clip.mp4",
                b"just some text",
                Refusal::Mismatch(Kind::Video),
            ),
            ("song.mp3", b"not a song", Refusal::Mismatch(Kind::Audio)),
            (
                "report.pdf",
                b"a report, typed out",
                Refusal::Mismatch(Kind::Pdf),
            ),
            // No name to go by, and bytes that are not text.
            ("setup.exe", b"MZ\x90\0\x03\0\0\0", Refusal::Unsupported),
            ("chair.blend", b"BLENDER-v300\0\0", Refusal::Unsupported),
            ("log.utf16", b"\xFF\xFEh\0i\0", Refusal::Unsupported),
        ];
        for (name, body, refusal) in cases {
            let file = write(&root, name, body);
            assert_eq!(
                inspect(&file, &roots).err().as_ref(),
                Some(refusal),
                "{name}"
            );
            let served = fetch(&file, "", None, &roots).status();
            // A texture's name is model.rs's to serve, as it always was.
            if !Accept::Asset.allows(&file) {
                assert_eq!(served, StatusCode::FORBIDDEN, "{name}");
            }
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A character cut in two by the end of the head is not a reason to call
    /// a text binary; a broken one inside the head is.
    #[test]
    fn text_is_judged_on_its_head_without_tripping_on_its_end() {
        let root = temp_dir("preview-head");
        let roots = roots_of(&[&root]);
        let mut long = vec![b'a'; HEAD_BYTES as usize - 1];
        long.extend_from_slice("é and more".as_bytes());
        let cut = write(&root, "long.txt", &long);
        let unnamed = write(&root, "long", &long);
        let broken = write(&root, "broken", b"ok \xC3 then not");

        assert_eq!(inspect(&cut, &roots).unwrap().kind, Kind::Text);
        assert_eq!(inspect(&unnamed, &roots).unwrap().kind, Kind::Text);
        assert_eq!(inspect(&broken, &roots).err(), Some(Refusal::Unsupported));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_outside_folders_and_relative_paths_are_refused() {
        let base = temp_dir("preview-refusals");
        let root = base.join("project");
        std::fs::create_dir_all(root.join("shots.png")).unwrap();
        let outside = write(&base, "outside/secret.txt", b"secret");
        let sibling = write(&base, "project-secrets/key.txt", b"secret");
        let roots = roots_of(&[&root]);

        assert_eq!(
            inspect(&root.join("gone.png"), &roots).err(),
            Some(Refusal::Missing)
        );
        assert_eq!(
            inspect(&root.join("shots.png"), &roots).err(),
            Some(Refusal::NotAFile)
        );
        assert_eq!(inspect(&outside, &roots).err(), Some(Refusal::Outside));
        assert_eq!(inspect(&sibling, &roots).err(), Some(Refusal::Outside));
        let climbing = root.join("..").join("outside").join("secret.txt");
        assert_eq!(inspect(&climbing, &roots).err(), Some(Refusal::Outside));
        assert_eq!(
            inspect(Path::new("shots/a.png"), &roots).err(),
            Some(Refusal::Relative)
        );
        // A model is refused as model_show refuses it.
        assert_eq!(
            inspect(&root.join("gone.glb"), &roots).err(),
            Some(Refusal::Model(model::Refusal::Missing))
        );
        // A model's own parts are no file to show, whether or not they exist.
        for part in ["chair.bin", "wood.tga", "sky.hdr", "chair.mtl"] {
            assert_eq!(
                inspect(&root.join(part), &roots).err(),
                Some(Refusal::Unsupported),
                "{part}"
            );
        }
        assert_eq!(
            fetch(&outside, "", None, &roots).status(),
            StatusCode::FORBIDDEN
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A link named as one type that leads to another would be shown as one
    /// thing and served as the other.
    #[test]
    fn a_link_to_a_file_of_another_type_is_refused() {
        let base = temp_dir("preview-links");
        let root = base.join("project");
        let notes = write(&root, "notes.txt", b"<script>alert(1)</script>");
        let secret = write(&base, "outside/secret.txt", b"secret");
        let roots = roots_of(&[&root]);

        if link(&notes, &root.join("page.html")) {
            let page = root.join("page.html");
            assert!(matches!(inspect(&page, &roots), Err(Refusal::Retyped(_))));
        }
        if link(&secret, &root.join("secret.txt")) {
            assert_eq!(
                inspect(&root.join("secret.txt"), &roots).err(),
                Some(Refusal::Outside)
            );
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn each_kind_has_its_cap() {
        assert_eq!(Kind::Model.cap(), Some(256 * MEGABYTE));
        assert_eq!(Kind::Pdf.cap(), Some(256 * MEGABYTE));
        assert_eq!(Kind::Image.cap(), Some(64 * MEGABYTE));
        assert_eq!(Kind::Html.cap(), Some(64 * MEGABYTE));
        assert_eq!(Kind::Text.cap(), Some(10 * MEGABYTE));
        assert_eq!(Kind::Markdown.cap(), Some(10 * MEGABYTE));
        assert_eq!(Kind::Video.cap(), None);
        assert_eq!(Kind::Audio.cap(), None);

        let root = temp_dir("preview-caps");
        let roots = roots_of(&[&root]);
        let png = b"\x89PNG\r\n\x1a\n";
        let image = sized(&root, "huge.png", png, 64 * MEGABYTE + 1);
        let fits = sized(&root, "fits.png", png, 64 * MEGABYTE);
        // Text all through: a file padded with NUL bytes would be binary.
        let log = write(&root, "huge.log", &vec![b'a'; 10 * MEGABYTE as usize + 1]);

        assert_eq!(
            inspect(&image, &roots).err(),
            Some(Refusal::TooBig {
                kind: Kind::Image,
                size: 64 * MEGABYTE + 1,
                cap: 64 * MEGABYTE
            })
        );
        assert!(inspect(&fits, &roots).is_ok());
        assert!(matches!(
            inspect(&log, &roots),
            Err(Refusal::TooBig {
                kind: Kind::Text,
                ..
            })
        ));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_range_is_answered_with_that_range() {
        let root = temp_dir("preview-range");
        let roots = roots_of(&[&root]);
        let body = b"RIFF\x24\0\0\0WAVE0123456789";
        let file = write(&root, "voice.wav", body);
        let len = body.len();
        let range = |range: &str| fetch(&file, "", Some(range), &roots);

        let first = range("bytes=0-3");
        assert_eq!(first.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(first.body(), b"RIFF");
        assert_eq!(
            header_of(&first, header::CONTENT_RANGE),
            format!("bytes 0-3/{len}")
        );
        assert_eq!(header_of(&first, header::ACCEPT_RANGES), "bytes");
        assert_eq!(header_of(&first, header::CONTENT_LENGTH), "4");
        assert_eq!(header_of(&first, header::CONTENT_TYPE), "audio/wav");

        let middle = range("bytes=8-11");
        assert_eq!(middle.body(), b"WAVE");
        assert_eq!(
            header_of(&middle, header::CONTENT_RANGE),
            format!("bytes 8-11/{len}")
        );

        let suffix = range("bytes=-3");
        assert_eq!(suffix.body(), b"789");
        let rest = range("bytes=12-");
        assert_eq!(rest.body(), b"0123456789");

        for invalid in [
            "bytes=5-2",
            "bytes=x-",
            "bytes=0-1,4-5",
            "bytes=-0",
            "bytes=22-",
        ] {
            let refused = range(invalid);
            assert_eq!(
                refused.status(),
                StatusCode::RANGE_NOT_SATISFIABLE,
                "{invalid}"
            );
            assert_eq!(
                header_of(&refused, header::CONTENT_RANGE),
                format!("bytes */{len}"),
                "{invalid}"
            );
            assert!(refused.body().is_empty());
        }
        // Another unit is ignored, as HTTP says it must be.
        let whole = range("items=0-1");
        assert_eq!(whole.status(), StatusCode::OK);
        assert_eq!(whole.body(), body);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A video has no cap, so it is never read whole: one part at a time,
    /// however much a range asks for.
    #[test]
    fn a_long_video_is_served_a_part_at_a_time() {
        let root = temp_dir("preview-stream");
        let roots = roots_of(&[&root]);
        let len = MAX_BYTES + 1;
        let file = sized(&root, "long.mp4", b"\0\0\0\x18ftypisom", len);
        assert_eq!(inspect(&file, &roots).unwrap().kind, Kind::Video);

        let start = fetch(&file, "", Some("bytes=0-"), &roots);
        assert_eq!(start.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(start.body().len() as u64, CHUNK);
        assert!(start.body().starts_with(b"\0\0\0\x18ftypisom"));
        assert_eq!(
            header_of(&start, header::CONTENT_RANGE),
            format!("bytes 0-{}/{len}", CHUNK - 1)
        );

        let end = fetch(&file, "", Some(&format!("bytes={}-", len - 10)), &roots);
        assert_eq!(end.body().len(), 10);
        assert_eq!(
            header_of(&end, header::CONTENT_RANGE),
            format!("bytes {}-{}/{len}", len - 10, len - 1)
        );

        assert_eq!(
            fetch(&file, "", None, &roots).status(),
            StatusCode::FORBIDDEN
        );
        let head = Request::builder()
            .method(Method::HEAD)
            .uri(format!(
                "hangar-model://localhost{}",
                url_path(&file.canonicalize().unwrap())
            ))
            .body(Vec::new())
            .unwrap();
        let head = model::answer(&head, &roots, &origins());
        assert_eq!(head.status(), StatusCode::OK);
        assert_eq!(header_of(&head, header::CONTENT_LENGTH), len.to_string());
        assert!(head.body().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn each_kind_goes_out_with_its_own_headers() {
        let root = temp_dir("preview-headers");
        let roots = roots_of(&[&root]);
        // (file, query, Content-Type, Content-Security-Policy)
        let cases = [
            ("page.html", "", HTML, PAGE_POLICY),
            ("page.html", "?as=text", TEXT_PLAIN, TEXT_POLICY),
            ("old.HTM", "?x=1&as=text", TEXT_PLAIN, TEXT_POLICY),
            ("logo.svg", "", SVG, PAGE_POLICY),
            ("logo.svg", "?as=text", TEXT_PLAIN, TEXT_POLICY),
            ("README.md", "", TEXT_PLAIN, TEXT_POLICY),
            ("main.rs", "", TEXT_PLAIN, TEXT_POLICY),
            ("app.js", "", TEXT_PLAIN, TEXT_POLICY),
            ("feed.xml", "", TEXT_PLAIN, TEXT_POLICY),
            ("Makefile", "", TEXT_PLAIN, TEXT_POLICY),
            ("report.pdf", "", "application/pdf", ""),
            ("clip.mp4", "", "video/mp4", ""),
            // Only markup has a source to show.
            ("clip.mp4", "?as=text", "video/mp4", ""),
            ("master.flac", "", "audio/flac", ""),
            ("next.avif", "", "image/avif", ""),
            // A texture as well as a picture: served as model.rs serves it.
            ("shot.png", "", "image/png", ""),
        ];
        for (name, query, content_type, policy) in cases {
            let file = sample(&root, name);
            let response = fetch(&file, query, None, &roots);
            let what = format!("{name}{query}");
            assert_eq!(response.status(), StatusCode::OK, "{what}");
            assert_eq!(
                header_of(&response, header::CONTENT_TYPE),
                content_type,
                "{what}"
            );
            assert_eq!(
                header_of(&response, header::CONTENT_SECURITY_POLICY),
                policy,
                "{what}"
            );
            assert_eq!(
                header_of(&response, header::X_CONTENT_TYPE_OPTIONS),
                "nosniff",
                "{what}"
            );
            assert_eq!(header_of(&response, header::CACHE_CONTROL), "no-store");
            assert_eq!(response.body(), &std::fs::read(&file).unwrap(), "{what}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// What a page in the panel's sandboxed iframe sends: `Origin: null`. It
    /// may not read a single file through the scheme, its own folder's
    /// included; Hangar's own page may.
    #[test]
    fn a_sandboxed_page_cannot_read_files_hangar_s_page_can() {
        let root = temp_dir("preview-origin");
        let roots = roots_of(&[&root]);
        sample(&root, "page.html");
        let beside = sample(&root, "data.json");
        let url = url_path(&beside.canonicalize().unwrap());
        let ask = |origin| model::answer(&get(&url, Some(origin), None), &roots, &origins());

        for page in [
            "null",
            "https://evil.example",
            "http://hangar-model.localhost",
        ] {
            let refused = ask(page);
            assert_eq!(refused.status(), StatusCode::FORBIDDEN, "{page}");
            assert!(refused
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .is_none());
        }
        let own = ask("http://tauri.localhost");
        assert_eq!(own.status(), StatusCode::OK);
        assert_eq!(
            header_of(&own, header::ACCESS_CONTROL_ALLOW_ORIGIN),
            "http://tauri.localhost"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_refused_file_is_refused_with_a_reason_the_panel_can_show() {
        let root = temp_dir("preview-reason");
        let roots = roots_of(&[&root]);
        let fake = write(&root, "clip.mp4", b"<html>404</html>");

        let refused = fetch(&fake, "", None, &roots);
        assert_eq!(refused.status(), StatusCode::FORBIDDEN);
        assert_eq!(refused.body(), b"not a video, whatever its name says");
        assert_eq!(header_of(&refused, header::CONTENT_TYPE), TEXT_PLAIN);
        assert_eq!(
            header_of(&refused, header::X_CONTENT_TYPE_OPTIONS),
            "nosniff"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_refusal_tells_an_agent_what_to_do() {
        let unsupported = Refusal::Unsupported.explain("scene.blend");
        for named in [
            ".png", ".avif", ".mp4", ".flac", "PDF", "Markdown", "HTML", ".glb",
        ] {
            assert!(unsupported.contains(named), "{named}");
        }
        let text = Refusal::TooBig {
            kind: Kind::Text,
            size: 30 * MEGABYTE,
            cap: 10 * MEGABYTE,
        }
        .explain("big.log");
        assert!(text.starts_with("big.log is 30 MB, over the preview's 10 MB limit for a text"));
        assert!(text.contains("does not show part of a file"));
        assert!(Refusal::Mismatch(Kind::Image)
            .explain("photo.jpg")
            .contains("an error page"));
        // Every refusal says what to do next, after a colon.
        for refusal in [
            Refusal::Model(model::Refusal::Missing),
            Refusal::Relative,
            Refusal::Missing,
            Refusal::NotAFile,
            Refusal::Outside,
            Refusal::Retyped(PathBuf::from("notes.txt")),
            Refusal::Unsupported,
            Refusal::Mismatch(Kind::Html),
            Refusal::Mismatch(Kind::Video),
            Refusal::TooBig {
                kind: Kind::Image,
                size: 65 * MEGABYTE,
                cap: 64 * MEGABYTE,
            },
            Refusal::TooBig {
                kind: Kind::Pdf,
                size: 300 * MEGABYTE,
                cap: 256 * MEGABYTE,
            },
            Refusal::TooBig {
                kind: Kind::Html,
                size: 65 * MEGABYTE,
                cap: 64 * MEGABYTE,
            },
            Refusal::Unreadable("os error 32".into()),
        ] {
            let sentence = refusal.explain("file");
            assert!(sentence.contains(": "), "{sentence}");
            assert!(sentence.ends_with('.'), "{sentence}");
        }
    }
}
