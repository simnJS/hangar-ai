//! 3D model files: which ones the preview may read, and the `hangar-model`
//! scheme it reads them through.
//!
//! The viewer runs in the webview and loads a model by URL, the way three.js
//! loaders expect, so that a glTF's textures and an OBJ's `.mtl` resolve next
//! to it as plain relative URLs. Tauri's asset protocol could serve that, but
//! only by opening whole folders to every file type, and base64 over IPC would
//! hold a 200 MB model several times over in memory. So the app answers a
//! scheme of its own, only for the folders the window allowed — an agent's
//! workspace and its worktrees — and only for the file types a model is made
//! of. The preview's other files — images, video, text, pages — come through
//! the same scheme, from the same folders: see preview.rs.
//!
//! Every check is made on the file's canonical path. A `..`, a symlink and a
//! drive letter typed in the other case all resolve before the folders are
//! compared, so none of them can walk out of an allowed folder.

use std::fs::{File, Metadata};
use std::io::{ErrorKind, Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::sync::{OnceLock, RwLock};
use std::time::UNIX_EPOCH;

use serde::Serialize;
use tauri::http::{header, response::Builder, Method, Request, Response, StatusCode};
use tauri::{Runtime, UriSchemeContext, UriSchemeResponder};

/// Registered in lib.rs. Windows' webview reaches it as
/// `http://hangar-model.localhost/…`, the other platforms as
/// `hangar-model://localhost/…`; `modelUrl` in src/lib/modelFiles.ts builds
/// both.
pub const SCHEME: &str = "hangar-model";

/// What the viewer opens, as `MODEL_FORMATS` in src/lib/modelFiles.ts.
pub(crate) const MODEL_EXTENSIONS: &[&str] = &["glb", "gltf", "obj", "stl", "ply", "fbx", "usdz"];

/// Every file type the scheme serves for a model, with the Content-Type it
/// goes out as: the models, the files they pull in beside them (a glTF's
/// `.bin`, an OBJ's `.mtl`), and the textures either can reference. A file of
/// any other type is the preview's (preview.rs), which shows sources and other
/// text only as `text/plain` and refuses what it cannot show.
const SERVED_TYPES: &[(&str, &str)] = &[
    ("glb", "model/gltf-binary"),
    ("gltf", "model/gltf+json"),
    ("bin", "application/octet-stream"),
    ("obj", "model/obj"),
    ("mtl", "model/mtl"),
    ("stl", "model/stl"),
    ("ply", "application/octet-stream"),
    ("fbx", "application/octet-stream"),
    ("usdz", "model/vnd.usdz+zip"),
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("webp", "image/webp"),
    ("ktx2", "image/ktx2"),
    ("basis", "application/octet-stream"),
    ("tga", "image/x-tga"),
    ("bmp", "image/bmp"),
    ("gif", "image/gif"),
    ("dds", "image/vnd-ms.dds"),
    ("hdr", "image/vnd.radiance"),
    ("exr", "image/x-exr"),
];

/// `MAX_MODEL_BYTES` in src/lib/modelFiles.ts. The scheme holds every file it
/// serves to the same cap, textures included, because it reads each one whole
/// (see `answer`): the cap is what bounds the memory one request can take.
pub(crate) const MAX_BYTES: u64 = 256 * 1024 * 1024;

pub(crate) const MEGABYTE: u64 = 1024 * 1024;

/// The folders the window last allowed, canonical: those of every workspace
/// that has a model open, not only the workspace of the agent asking. Which
/// agent may show which file is decided in the window (src/lib/control.ts);
/// this list only keeps the scheme from reading anywhere else.
///
/// One list for the whole app rather than one per window: whichever window
/// allowed a folder, the window that ends up drawing the model must be able to
/// load from it, and a detached workspace window is not always the one that
/// handled the agent's request.
fn roots() -> &'static RwLock<Vec<PathBuf>> {
    static ROOTS: OnceLock<RwLock<Vec<PathBuf>>> = OnceLock::new();
    ROOTS.get_or_init(Default::default)
}

pub(crate) fn allowed_roots() -> Vec<PathBuf> {
    roots().read().unwrap().clone()
}

/// A path as an agent or the window wrote it, in the platform's own form.
///
/// On Windows, `/c/Users/me/chair.glb` is how Git Bash and MSYS write
/// `C:\Users\me\chair.glb`, and so how an agent working in one of them often
/// does. Read as Windows reads it, it would be `\c\Users\…` on the current
/// drive, a file that does not exist.
pub(crate) fn native_path(raw: &str) -> PathBuf {
    #[cfg(windows)]
    if let [b'/', drive, rest @ ..] = raw.as_bytes() {
        if drive.is_ascii_alphabetic() && rest.first().is_none_or(|&b| b == b'/') {
            let tail = raw.get(3..).unwrap_or_default();
            return PathBuf::from(format!("{}:\\{tail}", drive.to_ascii_uppercase() as char));
        }
    }
    PathBuf::from(raw)
}

/// The canonical form of each folder that exists, without duplicates.
///
/// A relative folder is dropped: it would resolve against the app's own
/// working directory, which is nobody's workspace. So is a folder that is
/// gone — a removed worktree, a pane's deleted directory — rather than failing
/// the whole call: it holds nothing to serve anyway.
fn canonical_roots(roots: &[String]) -> Vec<PathBuf> {
    let mut canonical: Vec<PathBuf> = Vec::new();
    for root in roots {
        let path = native_path(root.trim());
        if !path.is_absolute() {
            continue;
        }
        let Ok(path) = path.canonicalize() else {
            continue;
        };
        if path.is_dir() && !canonical.contains(&path) {
            canonical.push(path);
        }
    }
    canonical
}

/// Which extensions a check accepts.
#[derive(Clone, Copy)]
pub(crate) enum Accept {
    /// A model the viewer opens: what `model_stat` is asked about.
    Model,
    /// Any file the scheme serves: a model, a file beside it, a texture.
    Asset,
}

impl Accept {
    pub(crate) fn allows(self, path: &Path) -> bool {
        let Some(extension) = extension(path) else {
            return false;
        };
        match self {
            Accept::Model => MODEL_EXTENSIONS.contains(&extension.as_str()),
            Accept::Asset => content_type(&extension).is_some(),
        }
    }
}

pub(crate) fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
}

pub(crate) fn content_type(extension: &str) -> Option<&'static str> {
    SERVED_TYPES
        .iter()
        .find(|(known, _)| *known == extension)
        .map(|(_, content_type)| *content_type)
}

/// A file that passed every check.
#[derive(Debug)]
pub(crate) struct Checked {
    /// Canonical: symlinks followed, `..` resolved.
    pub(crate) path: PathBuf,
    pub(crate) size: u64,
    pub(crate) modified_ms: u64,
}

#[derive(Debug, PartialEq)]
pub(crate) enum Refusal {
    /// A path that does not say where it starts. The window resolves an
    /// agent's relative path before asking, so this is a caller's bug.
    Relative,
    /// Not an extension the check accepts, on the name asked for or on the
    /// file that name leads to.
    Format,
    Missing,
    NotAFile,
    /// Resolves outside every allowed folder.
    Outside,
    TooBig {
        size: u64,
        cap: u64,
    },
    Unreadable(String),
}

impl Refusal {
    /// What `model_stat` says. The window hands it on word for word as the
    /// answer to `model_show`, so each one is a sentence an agent can act on:
    /// what is wrong, then what to do about it.
    pub(crate) fn explain(&self, path: &str) -> String {
        match self {
            Refusal::Relative => format!(
                "{path} is not an absolute path: give the model's full path, or one relative to \
                 your working directory."
            ),
            Refusal::Format => format!(
                "{path} is not a format the preview opens: export the model as one of {} and \
                 show that file. A .glb keeps its textures inside it.",
                MODEL_EXTENSIONS
                    .iter()
                    .map(|extension| format!(".{extension}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Refusal::Missing => format!(
                "There is no file at {path}: check the path, and save or export the model before \
                 showing it."
            ),
            Refusal::NotAFile => {
                format!("{path} is a folder: give the path of the model file inside it.")
            }
            Refusal::Outside => format!(
                "{path} is outside this workspace's folders: copy it into the workspace or one of \
                 its worktrees."
            ),
            Refusal::TooBig { size, cap } => format!(
                "{path} is {} MB, over the preview's {} MB limit: decimate the mesh or compress it \
                 (Draco or meshopt geometry, KTX2 textures) and show the smaller file.",
                size.div_ceil(MEGABYTE),
                cap / MEGABYTE
            ),
            Refusal::Unreadable(err) => format!(
                "{path} cannot be read ({err}): check that no other program holds it locked, then \
                 show it again."
            ),
        }
    }

    /// What the scheme answers: a status, and a reason short enough for the
    /// viewer to show as it is.
    pub(crate) fn status(&self) -> (StatusCode, String) {
        match self {
            Refusal::Relative => (StatusCode::BAD_REQUEST, "not an absolute path".into()),
            Refusal::Format => (StatusCode::FORBIDDEN, "this file type is not served".into()),
            Refusal::Missing => (StatusCode::NOT_FOUND, "no such file".into()),
            Refusal::NotAFile => (StatusCode::NOT_FOUND, "not a file".into()),
            Refusal::Outside => (
                StatusCode::FORBIDDEN,
                "outside the folders the preview may read".into(),
            ),
            Refusal::TooBig { cap, .. } => (
                StatusCode::FORBIDDEN,
                format!("larger than the {} MB the preview reads", cap / MEGABYTE),
            ),
            Refusal::Unreadable(err) => (StatusCode::FORBIDDEN, format!("unreadable: {err}")),
        }
    }
}

/// Judges one file against the allowed folders, the extensions and the cap.
///
/// The extension is read twice: on the name asked for, first, because a
/// format the viewer cannot read is the most useful thing to hear whether or
/// not the file exists; and on the canonical path, because `chair.glb` may be
/// a link to anything.
pub(crate) fn inspect(
    requested: &Path,
    roots: &[PathBuf],
    accept: Accept,
    max_bytes: u64,
) -> Result<Checked, Refusal> {
    if !requested.is_absolute() {
        return Err(Refusal::Relative);
    }
    if !accept.allows(requested) {
        return Err(Refusal::Format);
    }
    let path = requested.canonicalize().map_err(|err| match err.kind() {
        ErrorKind::NotFound => Refusal::Missing,
        _ => Refusal::Unreadable(err.to_string()),
    })?;
    // Component-wise, so an allowed `proj` does not let `proj-secrets` in.
    if !roots.iter().any(|root| path.starts_with(root)) {
        return Err(Refusal::Outside);
    }
    if !accept.allows(&path) {
        return Err(Refusal::Format);
    }
    let metadata = path
        .metadata()
        .map_err(|err| Refusal::Unreadable(err.to_string()))?;
    if !metadata.is_file() {
        return Err(Refusal::NotAFile);
    }
    let size = metadata.len();
    if size > max_bytes {
        return Err(Refusal::TooBig {
            size,
            cap: max_bytes,
        });
    }
    Ok(Checked {
        path,
        size,
        modified_ms: modified_ms(&metadata),
    })
}

/// Epoch milliseconds, or 0 where the platform does not keep the time.
pub(crate) fn modified_ms(metadata: &Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_millis() as u64)
}

/// Replaces the folders the preview may read from. The window calls it each
/// time an agent shows a model, with the folders of every workspace that has
/// one open, so a model shown earlier for another workspace keeps loading its
/// textures and reloading when its file changes.
#[tauri::command]
pub async fn model_allow_roots(roots: Vec<String>) -> Result<(), String> {
    // Canonicalizing touches the disk, a network share's included: off the
    // main thread, like every git command.
    let canonical = crate::git::off_thread(move || Ok(canonical_roots(&roots))).await?;
    *self::roots().write().unwrap() = canonical;
    Ok(())
}

/// What `modelStat` in src/lib/modelFiles.ts receives.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelFile {
    /// Canonical, in the platform's everyday form: `C:\…` rather than the
    /// `\\?\C:\…` Windows canonicalizes to. It is the string the window keys
    /// an open model by and builds its URL from, so the same file always
    /// comes back as the same string.
    path: String,
    size: u64,
    /// Epoch milliseconds. The panel polls this to reload a rewritten model.
    modified_ms: u64,
}

/// Size and modification time of a model the window is about to show or is
/// watching, or the reason it may not read it.
#[tauri::command]
pub async fn model_stat(path: String) -> Result<ModelFile, String> {
    let roots = allowed_roots();
    crate::git::off_thread(move || {
        inspect(&native_path(path.trim()), &roots, Accept::Model, MAX_BYTES)
            .map(|checked| ModelFile {
                path: dunce::simplified(&checked.path)
                    .to_string_lossy()
                    .into_owned(),
                size: checked.size,
                modified_ms: checked.modified_ms,
            })
            .map_err(|refusal| refusal.explain(&path))
    })
    .await
}

/// What a capture must be before it is written: a `.png` path (the one the
/// user picked in the save dialog) and bytes that really are a PNG. The
/// command is reachable from the page, so it must not become a way to write
/// anything anywhere.
fn png_capture(path: &str, data: &str) -> Result<(PathBuf, Vec<u8>), String> {
    use base64::Engine;
    let target = PathBuf::from(path.trim());
    if !target
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
    {
        return Err(format!("{} is not a .png file", target.display()));
    }
    let base64 = data.trim();
    let base64 = base64
        .strip_prefix("data:image/png;base64,")
        .unwrap_or(base64);
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64)
        .map_err(|err| format!("the capture is not valid base64: {err}"))?;
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("the capture is not a PNG image".into());
    }
    Ok((target, bytes))
}

/// Writes a capture of the 3D preview to the file the user chose in the
/// save dialog.
#[tauri::command]
pub async fn model_save_png(path: String, data: String) -> Result<(), String> {
    let (target, bytes) = png_capture(&path, &data)?;
    crate::git::off_thread(move || {
        std::fs::write(&target, bytes)
            .map_err(|err| format!("could not write {}: {err}", target.display()))
    })
    .await
}

/// The file a `hangar-model` URL names.
///
/// The URL's path is the file's absolute path, one encoded segment per
/// component, as `modelUrl` builds it: `/C%3A/dir/chair.glb` on Windows,
/// `/home/me/chair.glb` elsewhere. On Windows a first segment that is not a
/// drive is a server, so `/nas/share/chair.glb` is `\\nas\share\chair.glb`.
///
/// A browser resolves `..` out of a URL before sending it, so one that is
/// still there was encoded on purpose (`%2E%2E`, or hidden behind an encoded
/// backslash) and is refused outright, even where it would resolve inside an
/// allowed folder.
fn path_from_url(url_path: &str) -> Result<PathBuf, (StatusCode, String)> {
    let decoded = percent_encoding::percent_decode_str(url_path)
        .decode_utf8()
        .map_err(|_| (StatusCode::BAD_REQUEST, "the path is not UTF-8".to_string()))?;

    #[cfg(windows)]
    let path = {
        let rest = decoded.trim_start_matches('/');
        let (first, tail) = rest.split_once('/').unwrap_or((rest, ""));
        let is_drive =
            first.len() == 2 && first.ends_with(':') && first.as_bytes()[0].is_ascii_alphabetic();
        if is_drive {
            PathBuf::from(format!("{first}\\{tail}"))
        } else {
            PathBuf::from(format!("\\\\{rest}"))
        }
    };
    #[cfg(not(windows))]
    let path = PathBuf::from(decoded.as_ref());

    if path
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Err((StatusCode::FORBIDDEN, "'..' is not allowed".to_string()));
    }
    Ok(path)
}

/// The origins Hangar's own pages load from: the bundled app (Windows serves
/// it as `http://tauri.localhost`, the others as `tauri://localhost`), and the
/// dev server during `tauri dev`.
fn app_origins(config: &tauri::Config) -> Vec<String> {
    let mut origins: Vec<String> = [
        "tauri://localhost",
        "http://tauri.localhost",
        "https://tauri.localhost",
    ]
    .map(String::from)
    .to_vec();
    if tauri::is_dev() {
        if let Some(url) = &config.build.dev_url {
            origins.push(url.origin().ascii_serialization());
        }
    }
    origins
}

/// The part of a file a `Range` header asks for.
#[derive(Debug, PartialEq)]
pub(crate) enum Span {
    Whole,
    /// Inclusive, as HTTP counts.
    Part {
        start: u64,
        end: u64,
    },
    Unsatisfiable,
}

/// Reads a `Range` header. A single byte range is honoured, so a reader that
/// wants a big file in pieces can have them; anything else — several ranges,
/// another unit, a malformed header — is answered with the whole file, which
/// HTTP lets a server do.
fn span(range: Option<&str>, len: u64) -> Span {
    asked_span(range, len).unwrap_or(Span::Whole)
}

/// `span`, with `None` for a byte range this does not serve: several ranges,
/// or one HTTP calls invalid. Whether that means the whole file or a refusal
/// is the caller's choice, as HTTP leaves it; a header in another unit, which
/// HTTP says must be ignored, is the whole file either way.
pub(crate) fn asked_span(range: Option<&str>, len: u64) -> Option<Span> {
    let Some(spec) = range.and_then(|range| range.trim().strip_prefix("bytes=")) else {
        return Some(Span::Whole);
    };
    if spec.contains(',') {
        return None;
    }
    let (first, last) = spec.split_once('-')?;
    let (first, last) = (first.trim(), last.trim());
    if first.is_empty() {
        // `bytes=-500`: the last 500 bytes.
        let suffix = last.parse::<u64>().ok()?;
        if suffix == 0 || len == 0 {
            return Some(Span::Unsatisfiable);
        }
        return Some(Span::Part {
            start: len.saturating_sub(suffix),
            end: len - 1,
        });
    }
    let start = first.parse::<u64>().ok()?;
    let end = if last.is_empty() {
        u64::MAX
    } else {
        match last.parse::<u64>() {
            Ok(end) if end >= start => end,
            _ => return None,
        }
    };
    if start >= len {
        return Some(Span::Unsatisfiable);
    }
    Some(Span::Part {
        start,
        end: end.min(len - 1),
    })
}

pub(crate) fn read_span(path: &Path, start: u64, count: u64) -> std::io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(start))?;
    let mut body = Vec::with_capacity(count as usize);
    file.take(count).read_to_end(&mut body)?;
    Ok(body)
}

/// A builder can only fail on a header value that is not one, and every
/// value here is either a constant or was a valid header on the way in. A
/// bare 500 still beats a panic on the blocking pool, which would leave the
/// webview waiting for an answer that never comes.
pub(crate) fn finish(builder: Builder, body: Vec<u8>) -> Response<Vec<u8>> {
    builder.body(body).unwrap_or_else(|_| {
        let mut response = Response::new(Vec::new());
        *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
        response
    })
}

/// Answers one request to the scheme.
///
/// Tauri's responder takes a whole body at once, so a file cannot be streamed
/// to the webview: it is read in one pass into a buffer sized from its
/// metadata — never through base64 — and `serve` runs this off the webview's
/// thread. The size cap bounds that buffer; a `Range` request reads only the
/// part it asks for.
pub(crate) fn answer(
    request: &Request<Vec<u8>>,
    roots: &[PathBuf],
    origins: &[String],
) -> Response<Vec<u8>> {
    // The page and the scheme are different origins on every platform, so
    // `fetch` needs the scheme's leave to read. It is given to Hangar's own
    // pages only. A request without an Origin is not a cross-origin `fetch`,
    // or comes from a webview that kept the header to itself, so the wildcard
    // costs nothing there and keeps the viewer working.
    let allow_origin = match request.headers().get(header::ORIGIN) {
        None => "*".to_string(),
        Some(origin) => match origin.to_str() {
            Ok(origin) if origins.iter().any(|own| own == origin) => origin.to_string(),
            _ => {
                return finish(
                    Response::builder()
                        .status(StatusCode::FORBIDDEN)
                        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8"),
                    b"this page may not read models".to_vec(),
                )
            }
        },
    };
    let base = || {
        let builder = Response::builder()
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, allow_origin.as_str())
            // A model is reloaded when its file changes on disk; a cached copy
            // would show the old one.
            .header(header::CACHE_CONTROL, "no-store");
        if allow_origin == "*" {
            builder
        } else {
            builder.header(header::VARY, "Origin")
        }
    };
    let refuse = |status: StatusCode, reason: String| {
        finish(
            base()
                .status(status)
                .header(header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            reason.into_bytes(),
        )
    };

    let method = request.method();
    if method == Method::OPTIONS {
        // A plain GET never needs a preflight; answered anyway, so a loader
        // that adds a header of its own is not stopped by one.
        let asked = request
            .headers()
            .get(header::ACCESS_CONTROL_REQUEST_HEADERS)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("Range")
            .to_string();
        return finish(
            base()
                .status(StatusCode::NO_CONTENT)
                .header(header::ACCESS_CONTROL_ALLOW_METHODS, "GET, HEAD, OPTIONS")
                .header(header::ACCESS_CONTROL_ALLOW_HEADERS, asked)
                .header(header::ACCESS_CONTROL_MAX_AGE, "600"),
            Vec::new(),
        );
    }
    if method != Method::GET && method != Method::HEAD {
        return refuse(
            StatusCode::METHOD_NOT_ALLOWED,
            "only GET and HEAD are served".into(),
        );
    }

    let path = match path_from_url(request.uri().path()) {
        Ok(path) => path,
        Err((status, reason)) => return refuse(status, reason),
    };
    // Any other file is one the preview shows for itself, as what it is:
    // preview.rs. From here on, this is the models' own way, as it was.
    if !Accept::Asset.allows(&path) {
        return crate::preview::answer(request, &path, roots, &base);
    }
    let checked = match inspect(&path, roots, Accept::Asset, MAX_BYTES) {
        Ok(checked) => checked,
        Err(refusal) => {
            let (status, reason) = refusal.status();
            return refuse(status, reason);
        }
    };

    let len = checked.size;
    let range = request
        .headers()
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok());
    let (status, start, count) = match span(range, len) {
        Span::Whole => (StatusCode::OK, 0, len),
        Span::Part { start, end } => (StatusCode::PARTIAL_CONTENT, start, end - start + 1),
        Span::Unsatisfiable => {
            return finish(
                base()
                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                    .header(header::CONTENT_RANGE, format!("bytes */{len}")),
                Vec::new(),
            )
        }
    };

    let body = if method == Method::HEAD {
        Vec::new()
    } else {
        match read_span(&checked.path, start, count) {
            // Shorter than its metadata said: rewritten while it was read. The
            // panel sees the new modification time and asks again.
            Ok(body) if body.len() as u64 != count => {
                return refuse(
                    StatusCode::CONFLICT,
                    "the file changed while it was read".into(),
                )
            }
            Ok(body) => body,
            Err(err) => return refuse(StatusCode::FORBIDDEN, format!("unreadable: {err}")),
        }
    };

    let content_type = extension(&checked.path)
        .and_then(|extension| content_type(&extension))
        .unwrap_or("application/octet-stream");
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
    if status == StatusCode::PARTIAL_CONTENT {
        builder = builder.header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{}/{len}", start + count - 1),
        );
    }
    finish(builder, body)
}

/// The `hangar-model` handler registered in lib.rs. Tauri calls it on the
/// webview's thread, so the work moves to the blocking pool: a 200 MB model
/// read there would freeze every pane while it lasted.
pub fn serve<R: Runtime>(
    ctx: UriSchemeContext<'_, R>,
    request: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let origins = app_origins(ctx.app_handle().config());
    let roots = allowed_roots();
    tauri::async_runtime::spawn_blocking(move || {
        responder.respond(answer(&request, &roots, &origins));
    });
}

#[cfg(test)]
pub(crate) mod tests {
    #[test]
    fn a_capture_must_be_a_png_named_png() {
        use base64::Engine;
        let png = base64::engine::general_purpose::STANDARD.encode(b"\x89PNG\r\n\x1a\nrest");
        assert!(super::png_capture("C:/out/shot.PNG", &png).is_ok());
        assert!(super::png_capture("C:/out/shot.exe", &png).is_err());
        let text = base64::engine::general_purpose::STANDARD.encode(b"not an image");
        assert!(super::png_capture("C:/out/shot.png", &text).is_err());
        assert!(super::png_capture("C:/out/shot.png", "%%%").is_err());
        let url = format!("data:image/png;base64,{png}");
        assert_eq!(
            super::png_capture("shot.png", &url).unwrap().1,
            b"\x89PNG\r\n\x1a\nrest"
        );
    }

    use super::*;
    use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};

    pub(crate) fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hangar-model-{tag}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    pub(crate) fn write(root: &Path, relative: &str, body: &[u8]) -> PathBuf {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parents");
        }
        std::fs::write(&path, body).expect("write");
        path
    }

    pub(crate) fn roots_of(dirs: &[&Path]) -> Vec<PathBuf> {
        canonical_roots(
            &dirs
                .iter()
                .map(|dir| dir.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
        )
    }

    fn model(path: &Path, roots: &[PathBuf]) -> Result<Checked, Refusal> {
        inspect(path, roots, Accept::Model, MAX_BYTES)
    }

    /// What `encodeURIComponent` leaves alone, so the tests ask for files the
    /// way `modelUrl` in src/lib/modelFiles.ts does.
    const URI_COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'!')
        .remove(b'~')
        .remove(b'*')
        .remove(b'\'')
        .remove(b'(')
        .remove(b')');

    /// `modelUrl` without the scheme and host: the path the handler sees.
    pub(crate) fn url_path(path: &Path) -> String {
        let plain = dunce::simplified(path).to_string_lossy().replace('\\', "/");
        let encoded: Vec<String> = plain
            .split('/')
            .enumerate()
            .filter(|(i, part)| !part.is_empty() || *i == 0)
            .map(|(_, part)| utf8_percent_encode(part, URI_COMPONENT).to_string())
            .collect();
        format!("/{}", encoded.join("/").trim_start_matches('/'))
    }

    pub(crate) fn get(path: &str, origin: Option<&str>, range: Option<&str>) -> Request<Vec<u8>> {
        let mut request = Request::builder().uri(format!("hangar-model://localhost{path}"));
        if let Some(origin) = origin {
            request = request.header(header::ORIGIN, origin);
        }
        if let Some(range) = range {
            request = request.header(header::RANGE, range);
        }
        request.body(Vec::new()).expect("request")
    }

    pub(crate) fn origins() -> Vec<String> {
        vec!["http://tauri.localhost".to_string()]
    }

    pub(crate) fn header_of(response: &Response<Vec<u8>>, name: header::HeaderName) -> &str {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
    }

    #[test]
    fn a_model_inside_an_allowed_folder_is_described() {
        let root = temp_dir("inside");
        let file = write(&root, "assets/Chair.GLB", b"glTF");
        let roots = roots_of(&[&root]);

        let checked = model(&file, &roots).expect("allowed");

        assert_eq!(checked.path, file.canonicalize().unwrap());
        assert_eq!(checked.size, 4);
        assert!(checked.modified_ms > 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `..` is resolved before the folder is compared: it cannot climb out,
    /// and a path that climbs and comes back down inside is still fine.
    #[test]
    fn a_path_climbing_out_with_dot_dot_is_refused() {
        let base = temp_dir("traversal");
        let root = base.join("project");
        write(&root, "sub/keep.txt", b"");
        write(&root, "inside.glb", b"glTF");
        write(&base, "outside/secret.glb", b"glTF");
        let roots = roots_of(&[&root]);

        let climbing = root
            .join("sub")
            .join("..")
            .join("..")
            .join("outside/secret.glb");
        assert_eq!(model(&climbing, &roots).err(), Some(Refusal::Outside));

        let detour = root.join("sub").join("..").join("inside.glb");
        assert!(model(&detour, &roots).is_ok());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Component-wise: allowing `project` must not let `project-secrets` in.
    #[test]
    fn a_sibling_sharing_the_folder_s_prefix_is_outside() {
        let base = temp_dir("prefix");
        let root = base.join("project");
        std::fs::create_dir_all(&root).unwrap();
        let sibling = write(&base, "project-secrets/key.glb", b"glTF");
        let roots = roots_of(&[&root]);

        assert_eq!(model(&sibling, &roots).err(), Some(Refusal::Outside));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    pub(crate) fn link(target: &Path, link: &Path) -> bool {
        std::os::unix::fs::symlink(target, link).is_ok()
    }

    /// Creating a symlink on Windows takes Developer Mode or an elevated
    /// shell. Without either the symlink cases are skipped, not failed: the
    /// policy they exercise is the one the other tests already pin.
    #[cfg(windows)]
    pub(crate) fn link(target: &Path, link: &Path) -> bool {
        let made = if target.is_dir() {
            std::os::windows::fs::symlink_dir(target, link)
        } else {
            std::os::windows::fs::symlink_file(target, link)
        };
        if let Err(err) = &made {
            eprintln!("symlinks unavailable here, case skipped: {err}");
        }
        made.is_ok()
    }

    #[test]
    fn a_symlink_pointing_outside_is_refused() {
        let base = temp_dir("symlink");
        let root = base.join("project");
        std::fs::create_dir_all(&root).unwrap();
        let secret = write(&base, "outside/secret.glb", b"glTF");
        let notes = write(&root, "notes.txt", b"not a model");
        let roots = roots_of(&[&root]);

        // A file link, a folder link, and a link that stays inside but lands
        // on a file type the preview does not read.
        if link(&secret, &root.join("chair.glb")) {
            assert_eq!(
                model(&root.join("chair.glb"), &roots).err(),
                Some(Refusal::Outside)
            );
        }
        if link(&base.join("outside"), &root.join("assets")) {
            assert_eq!(
                model(&root.join("assets/secret.glb"), &roots).err(),
                Some(Refusal::Outside)
            );
        }
        if link(&notes, &root.join("notes.glb")) {
            assert_eq!(
                model(&root.join("notes.glb"), &roots).err(),
                Some(Refusal::Format)
            );
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_file_the_viewer_cannot_open_is_refused_before_the_disk_is_asked() {
        let root = temp_dir("extension");
        let env = write(&root, ".env", b"TOKEN=1");
        let texture = write(&root, "wood.png", b"png");
        let roots = roots_of(&[&root]);

        assert_eq!(model(&env, &roots).err(), Some(Refusal::Format));
        assert_eq!(
            model(&root.join("missing.blend"), &roots).err(),
            Some(Refusal::Format)
        );
        // A texture is served beside a model, but it is not one.
        assert_eq!(model(&texture, &roots).err(), Some(Refusal::Format));
        assert!(inspect(&texture, &roots, Accept::Asset, MAX_BYTES).is_ok());
        assert_eq!(
            inspect(&env, &roots, Accept::Asset, MAX_BYTES).err(),
            Some(Refusal::Format)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_file_over_the_cap_is_refused() {
        let root = temp_dir("cap");
        let file = write(&root, "big.stl", b"12345");
        let roots = roots_of(&[&root]);

        assert_eq!(
            inspect(&file, &roots, Accept::Model, 4).err(),
            Some(Refusal::TooBig { size: 5, cap: 4 })
        );
        assert!(inspect(&file, &roots, Accept::Model, 5).is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_files_folders_and_relative_paths_are_refused() {
        let root = temp_dir("kinds");
        std::fs::create_dir_all(root.join("box.glb")).unwrap();
        let roots = roots_of(&[&root]);

        assert_eq!(
            model(&root.join("gone.glb"), &roots).err(),
            Some(Refusal::Missing)
        );
        assert_eq!(
            model(&root.join("box.glb"), &roots).err(),
            Some(Refusal::NotAFile)
        );
        assert_eq!(
            model(Path::new("models/chair.glb"), &roots).err(),
            Some(Refusal::Relative)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// An empty or relative folder would mean the app's own working
    /// directory, and nothing is allowed until the window says so.
    #[test]
    fn only_existing_absolute_folders_become_roots() {
        let root = temp_dir("roots");
        let file = write(&root, "chair.glb", b"glTF");
        let listed = [
            String::new(),
            ".".to_string(),
            "relative/dir".to_string(),
            root.join("gone").to_string_lossy().into_owned(),
            file.to_string_lossy().into_owned(),
            root.to_string_lossy().into_owned(),
            root.to_string_lossy().into_owned(),
        ];

        assert_eq!(canonical_roots(&listed), vec![root.canonicalize().unwrap()]);
        assert_eq!(model(&file, &[]).err(), Some(Refusal::Outside));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    fn swap_drive_case(path: &Path) -> String {
        let text = dunce::simplified(path).to_string_lossy().into_owned();
        let mut chars = text.chars();
        let drive = chars.next().expect("a drive letter");
        let swapped = if drive.is_ascii_uppercase() {
            drive.to_ascii_lowercase()
        } else {
            drive.to_ascii_uppercase()
        };
        format!("{swapped}{}", chars.as_str())
    }

    /// The same folder typed with `c:` and `C:`, or with its names in another
    /// case, is one folder: canonicalizing settles both sides.
    #[cfg(windows)]
    #[test]
    fn drive_letter_casing_does_not_matter_on_windows() {
        let root = temp_dir("casing");
        let file = write(&root, "Chair.glb", b"glTF");
        let roots = canonical_roots(&[swap_drive_case(&root).to_lowercase()]);
        assert_eq!(roots.len(), 1);

        let asked = PathBuf::from(swap_drive_case(&file).to_uppercase());
        let checked = model(&asked, &roots).expect("same folder");
        assert_eq!(checked.path, file.canonicalize().unwrap());

        let other_case = swap_drive_case(&file.canonicalize().unwrap());
        let response = answer(
            &get(&url_path(Path::new(&other_case)), None, None),
            &roots,
            &origins(),
        );
        assert_eq!(response.status(), StatusCode::OK);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn msys_paths_are_read_as_windows_paths() {
        assert_eq!(
            native_path("/c/Users/me/chair.glb"),
            PathBuf::from(r"C:\Users/me/chair.glb")
        );
        assert_eq!(native_path("/d"), PathBuf::from(r"D:\"));
        assert_eq!(
            native_path(r"C:\Users\me\chair.glb"),
            PathBuf::from(r"C:\Users\me\chair.glb")
        );
        // Not a drive: left for Windows to read as it would.
        assert_eq!(native_path("/cd/chair.glb"), PathBuf::from("/cd/chair.glb"));
    }

    #[test]
    fn the_scheme_serves_a_model_to_the_app_s_own_page() {
        let root = temp_dir("serve");
        let file = write(&root, "scene folder/chair #1.glb", b"glTF-bytes");
        let roots = roots_of(&[&root]);
        let url = url_path(&file.canonicalize().unwrap());

        let response = answer(
            &get(&url, Some("http://tauri.localhost"), None),
            &roots,
            &origins(),
        );

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body(), b"glTF-bytes");
        assert_eq!(
            header_of(&response, header::CONTENT_TYPE),
            "model/gltf-binary"
        );
        assert_eq!(header_of(&response, header::CONTENT_LENGTH), "10");
        assert_eq!(
            header_of(&response, header::ACCESS_CONTROL_ALLOW_ORIGIN),
            "http://tauri.localhost"
        );
        assert_eq!(header_of(&response, header::CACHE_CONTROL), "no-store");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A glTF's textures and buffers come through the same door as the
    /// model, by the relative URLs it names them with. Anything else is the
    /// preview's: a source as text it never runs, a binary it cannot show
    /// not at all (preview.rs).
    #[test]
    fn the_scheme_serves_textures_and_buffers_as_a_model_s_files() {
        let root = temp_dir("assets");
        let texture = write(&root, "textures/wood.JPG", b"jpeg");
        let buffer = write(&root, "chair.bin", b"buffer");
        let source = write(&root, "src/main.rs", b"fn main() {}");
        let blend = write(&root, "chair.blend", b"BLENDER\0\x01\x02");
        let roots = roots_of(&[&root]);
        let serve = |path: &Path| {
            answer(
                &get(&url_path(&path.canonicalize().unwrap()), None, None),
                &roots,
                &origins(),
            )
        };

        let texture = serve(&texture);
        assert_eq!(texture.status(), StatusCode::OK);
        assert_eq!(header_of(&texture, header::CONTENT_TYPE), "image/jpeg");
        assert_eq!(
            header_of(&texture, header::ACCESS_CONTROL_ALLOW_ORIGIN),
            "*"
        );
        assert_eq!(serve(&buffer).status(), StatusCode::OK);
        let source = serve(&source);
        assert_eq!(source.status(), StatusCode::OK);
        assert_eq!(
            header_of(&source, header::CONTENT_TYPE),
            "text/plain; charset=utf-8"
        );
        assert_eq!(serve(&blend).status(), StatusCode::FORBIDDEN);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_scheme_refuses_with_a_status_and_a_reason() {
        let base = temp_dir("refusals");
        let root = base.join("project");
        write(&root, "chair.glb", b"glTF");
        let outside = write(&base, "outside.glb", b"glTF");
        let roots = roots_of(&[&root]);
        let inside = url_path(&root.canonicalize().unwrap().join("chair.glb"));
        let missing = url_path(&root.canonicalize().unwrap().join("gone.glb"));
        let status = |path: &str, origin: Option<&str>| {
            answer(&get(path, origin, None), &roots, &origins()).status()
        };

        assert_eq!(
            status(&url_path(&outside.canonicalize().unwrap()), None),
            StatusCode::FORBIDDEN
        );
        assert_eq!(status(&missing, None), StatusCode::NOT_FOUND);
        // Another page, even for a file that is allowed.
        assert_eq!(
            status(&inside, Some("https://evil.example")),
            StatusCode::FORBIDDEN
        );
        assert_eq!(status(&inside, Some("null")), StatusCode::FORBIDDEN);

        let refused = answer(&get(&missing, None, None), &roots, &origins());
        assert_eq!(refused.body(), b"no such file");
        assert_eq!(
            header_of(&refused, header::ACCESS_CONTROL_ALLOW_ORIGIN),
            "*",
            "the viewer must be able to read why"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A browser never sends a `..` it could resolve itself; an encoded one
    /// is an attempt, refused even when it would land inside.
    #[test]
    fn the_scheme_refuses_an_encoded_dot_dot() {
        let root = temp_dir("encoded");
        write(&root, "sub/keep.txt", b"");
        write(&root, "chair.glb", b"glTF");
        let roots = roots_of(&[&root]);
        let sub = url_path(&root.canonicalize().unwrap().join("sub"));

        let mut attempts = vec![
            format!("{sub}/%2E%2E/chair.glb"),
            format!("{sub}/%2e%2e%2Fchair.glb"),
        ];
        if cfg!(windows) {
            attempts.push(format!("{sub}/x%5C..%5C..%5Cchair.glb"));
        }
        for path in attempts {
            let response = answer(&get(&path, None, None), &roots, &origins());
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_scheme_answers_a_range_with_that_range() {
        let root = temp_dir("range");
        let file = write(&root, "mesh.ply", b"0123456789");
        let roots = roots_of(&[&root]);
        let url = url_path(&file.canonicalize().unwrap());

        let part = answer(&get(&url, None, Some("bytes=2-5")), &roots, &origins());
        assert_eq!(part.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(part.body(), b"2345");
        assert_eq!(header_of(&part, header::CONTENT_RANGE), "bytes 2-5/10");

        let tail = answer(&get(&url, None, Some("bytes=-3")), &roots, &origins());
        assert_eq!(tail.body(), b"789");

        let past = answer(&get(&url, None, Some("bytes=10-")), &roots, &origins());
        assert_eq!(past.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(header_of(&past, header::CONTENT_RANGE), "bytes */10");

        let head = Request::builder()
            .method(Method::HEAD)
            .uri(format!("hangar-model://localhost{url}"))
            .body(Vec::new())
            .unwrap();
        let head = answer(&head, &roots, &origins());
        assert_eq!(head.status(), StatusCode::OK);
        assert!(head.body().is_empty());
        assert_eq!(header_of(&head, header::CONTENT_LENGTH), "10");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ranges_are_read_the_way_http_reads_them() {
        assert_eq!(span(None, 10), Span::Whole);
        assert_eq!(span(Some("bytes=0-"), 10), Span::Part { start: 0, end: 9 });
        assert_eq!(
            span(Some("bytes=4-100"), 10),
            Span::Part { start: 4, end: 9 }
        );
        assert_eq!(span(Some("bytes=-20"), 10), Span::Part { start: 0, end: 9 });
        assert_eq!(span(Some("bytes=-0"), 10), Span::Unsatisfiable);
        assert_eq!(span(Some("bytes=12-"), 10), Span::Unsatisfiable);
        // Ignored rather than refused, as HTTP allows.
        assert_eq!(span(Some("bytes=0-1,4-5"), 10), Span::Whole);
        assert_eq!(span(Some("bytes=5-2"), 10), Span::Whole);
        assert_eq!(span(Some("items=0-1"), 10), Span::Whole);
        assert_eq!(span(Some("bytes=x-"), 10), Span::Whole);
    }

    #[test]
    fn a_refusal_tells_an_agent_what_to_do() {
        let too_big = Refusal::TooBig {
            size: 300 * MEGABYTE,
            cap: MAX_BYTES,
        };
        assert!(too_big
            .explain("big.fbx")
            .starts_with("big.fbx is 300 MB, over the preview's 256 MB limit: "));
        assert!(Refusal::Format
            .explain("chair.blend")
            .contains("one of .glb, .gltf, .obj, .stl, .ply, .fbx, .usdz and show that file"));
        assert_eq!(
            Refusal::Outside.explain(r"C:\x\y.glb"),
            r"C:\x\y.glb is outside this workspace's folders: copy it into the workspace or one of its worktrees."
        );
        // Every refusal says what to do next, after a colon.
        for refusal in [
            Refusal::Relative,
            Refusal::Format,
            Refusal::Missing,
            Refusal::NotAFile,
            Refusal::Outside,
            too_big,
            Refusal::Unreadable("os error 32".into()),
        ] {
            let sentence = refusal.explain("chair.glb");
            assert!(sentence.contains(": "), "{sentence}");
            assert!(sentence.ends_with('.'), "{sentence}");
        }
    }
}
