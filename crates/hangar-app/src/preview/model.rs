//! The decisions the preview makes before it draws anything: what a file is,
//! which viewer draws it, how much of a text file to read, which language
//! colours it, whether JSON is laid out again, where a Markdown image or link
//! leads, how an image sits in its stage (`lib/preview.ts`, `lib/modelFiles.ts`).
//!
//! Nothing here draws or touches the disk: the viewers do. The tests are
//! those of `preview.test.ts` and of `modelStore.test.ts`'s "modelFiles".
//! Three pieces of the TS have no counterpart on purpose, since the native
//! viewers read files and draw colours themselves instead of going through a
//! webview's HTTP and HTML: `rangeTotal` (a `Content-Range` header),
//! `escapeHtml` and `splitHighlighted` (highlighted HTML cut into lines).

use std::path::{Path, PathBuf};

use serde_json::Value;

/// What the preview shows a file as (`PreviewKind`). The order is the one the
/// refusal message lists them in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    pub const ALL: [Kind; 8] = [
        Kind::Model,
        Kind::Image,
        Kind::Video,
        Kind::Audio,
        Kind::Text,
        Kind::Markdown,
        Kind::Pdf,
        Kind::Html,
    ];

    /// The word the control API and `preview_inspect` use.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Model => "model",
            Kind::Image => "image",
            Kind::Video => "video",
            Kind::Audio => "audio",
            Kind::Text => "text",
            Kind::Markdown => "markdown",
            Kind::Pdf => "pdf",
            Kind::Html => "html",
        }
    }

    pub fn parse(word: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|kind| kind.as_str() == word)
    }

    /// The viewer that draws it.
    pub fn viewer(self) -> Viewer {
        viewer_of(self)
    }
}

impl From<hangar_core::preview::Kind> for Kind {
    fn from(kind: hangar_core::preview::Kind) -> Self {
        use hangar_core::preview::Kind as Core;
        match kind {
            Core::Model => Kind::Model,
            Core::Image => Kind::Image,
            Core::Video => Kind::Video,
            Core::Audio => Kind::Audio,
            Core::Text => Kind::Text,
            Core::Markdown => Kind::Markdown,
            Core::Pdf => Kind::Pdf,
            Core::Html => Kind::Html,
        }
    }
}

/// Everything but a 3D model, which has an entry of its own shape
/// (`FileKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileKind {
    Image,
    Video,
    Audio,
    Text,
    Markdown,
    Pdf,
    Html,
}

impl FileKind {
    pub fn kind(self) -> Kind {
        match self {
            FileKind::Image => Kind::Image,
            FileKind::Video => Kind::Video,
            FileKind::Audio => Kind::Audio,
            FileKind::Text => Kind::Text,
            FileKind::Markdown => Kind::Markdown,
            FileKind::Pdf => Kind::Pdf,
            FileKind::Html => Kind::Html,
        }
    }

    pub fn as_str(self) -> &'static str {
        self.kind().as_str()
    }

    pub fn of(kind: Kind) -> Option<FileKind> {
        Some(match kind {
            Kind::Model => return None,
            Kind::Image => FileKind::Image,
            Kind::Video => FileKind::Video,
            Kind::Audio => FileKind::Audio,
            Kind::Text => FileKind::Text,
            Kind::Markdown => FileKind::Markdown,
            Kind::Pdf => FileKind::Pdf,
            Kind::Html => FileKind::Html,
        })
    }
}

/// What `preview_inspect` answers, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewFile {
    /// Canonical absolute path, as Rust resolved it.
    pub path: String,
    pub kind: Kind,
    /// The exact Content-Type the file is served with: `text/plain;
    /// charset=utf-8` for text.
    pub mime: String,
    pub size: u64,
    /// The file's name, without its folder.
    pub name: String,
    /// Epoch milliseconds: a newer one means the file was rewritten, and is
    /// read again.
    pub modified_ms: u64,
}

/// `preview_inspect`'s answer, checked: a kind this window has no viewer for
/// is refused here rather than drawn as an empty tab. The message is read by
/// an agent (`checkPreviewFile`).
pub fn check_preview_file(answer: &Value) -> Result<PreviewFile, String> {
    let raw = answer.as_object();
    let text = |key: &str| raw.and_then(|raw| raw.get(key)).and_then(Value::as_str);
    let path = text("path").unwrap_or("");
    let name = match text("name") {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => path
            .rsplit(['/', '\\'])
            .next()
            .filter(|last| !last.is_empty())
            .unwrap_or(path)
            .to_string(),
    };
    if path.is_empty() {
        return Err("The preview got no path back for that file. Try again.".into());
    }
    let Some(kind) = text("kind").and_then(Kind::parse) else {
        let said = text("kind").map(|kind| format!(" ({kind})")).unwrap_or_default();
        let shown: Vec<&str> = Kind::ALL.iter().map(|kind| kind.as_str()).collect();
        return Err(format!(
            "The preview has no viewer for {name}{said}. It shows: {}.",
            shown.join(", ")
        ));
    };
    // A count is a number that is not negative; anything else counts as 0.
    let count = |key: &str| {
        raw.and_then(|raw| raw.get(key))
            .and_then(Value::as_f64)
            .filter(|number| *number >= 0.0)
            .map_or(0, |number| number as u64)
    };
    Ok(PreviewFile {
        path: path.to_string(),
        kind,
        mime: text("mime").unwrap_or("").to_string(),
        size: count("size"),
        name,
        modified_ms: count("modifiedMs"),
    })
}

/// The answer of core's `preview_inspect`, checked as an agent's would be.
pub fn from_core(file: &hangar_core::preview::PreviewFile) -> Result<PreviewFile, String> {
    let value = serde_json::to_value(file).map_err(|err| err.to_string())?;
    check_preview_file(&value)
}

/// The component that draws a kind: video and audio share a player, text and
/// code one view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Viewer {
    Model,
    Image,
    Media,
    Code,
    Markdown,
    Pdf,
    Html,
}

pub fn viewer_of(kind: Kind) -> Viewer {
    match kind {
        Kind::Model => Viewer::Model,
        Kind::Image => Viewer::Image,
        Kind::Video | Kind::Audio => Viewer::Media,
        Kind::Text => Viewer::Code,
        Kind::Markdown => Viewer::Markdown,
        Kind::Pdf => Viewer::Pdf,
        Kind::Html => Viewer::Html,
    }
}

impl Viewer {
    /// Whether the browser-engine helper draws it, in a window of its own:
    /// there is no web view inside the app to draw a PDF, a page or a
    /// player with.
    pub fn in_helper(self) -> bool {
        matches!(self, Viewer::Media | Viewer::Pdf | Viewer::Html)
    }
}

/* ── model files (lib/modelFiles.ts) ───────────────────────────── */

/// The 3D formats the viewer opens (`MODEL_FORMATS`).
pub const MODEL_FORMATS: [&str; 7] = ["glb", "gltf", "obj", "stl", "ply", "fbx", "usdz"];

/// Largest model the viewer accepts, textures not included.
pub const MAX_MODEL_BYTES: u64 = 256 * 1024 * 1024;

/// The format a file name says, or `None` for anything the viewer cannot
/// open (`modelFormatOf`).
pub fn model_format_of(path: &str) -> Option<&'static str> {
    let ext = path[path.rfind('.')? + 1..].to_lowercase();
    MODEL_FORMATS.into_iter().find(|format| *format == ext)
}

/// What `encodeURIComponent` leaves alone.
fn component_safe(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
        )
}

/// `encodeURIComponent`.
fn encode_component(part: &str) -> String {
    let mut out = String::with_capacity(part.len());
    for &byte in part.as_bytes() {
        if component_safe(byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The URL a web page loads `path` from, through the `hangar-model` scheme
/// (`modelUrl`). Windows' webview only routes custom schemes as
/// `http://<scheme>.localhost`; the others take the scheme itself. Segments
/// are encoded one by one, never the separators, so relative URLs inside a
/// page keep working.
pub fn model_url_for(path: &str, windows: bool) -> String {
    let slashed = path.replace('\\', "/");
    let encoded: Vec<String> = slashed
        .split('/')
        .enumerate()
        .filter(|(at, part)| !part.is_empty() || *at == 0)
        .map(|(_, part)| encode_component(part))
        .collect();
    let joined = encoded.join("/");
    let base = if windows {
        "http://hangar-model.localhost"
    } else {
        "hangar-model://localhost"
    };
    format!("{base}/{}", joined.strip_prefix('/').unwrap_or(&joined))
}

/// [`model_url_for`] for the platform this is built for.
pub fn model_url(path: &str) -> String {
    model_url_for(path, cfg!(windows))
}

/// Where the helper's windows load a file from. Shown again, a file has a new
/// revision, and the address says so: a page or a video the web view already
/// holds would otherwise come back from its own memory, whatever the
/// scheme's no-store. `as_text` asks for an HTML page's source, served as
/// plain text (`previewUrl`).
pub fn preview_url_for(path: &str, revision: u64, as_text: bool, windows: bool) -> String {
    let mut query = Vec::new();
    if as_text {
        query.push("as=text".to_string());
    }
    if revision != 0 {
        query.push(format!("rev={revision}"));
    }
    let base = model_url_for(path, windows);
    if query.is_empty() {
        base
    } else {
        format!("{base}?{}", query.join("&"))
    }
}

pub fn preview_url(path: &str, revision: u64, as_text: bool) -> String {
    preview_url_for(path, revision, as_text, cfg!(windows))
}

/// A Content-Type without its parameters, for a line that says what a file is
/// (`mimeType`).
pub fn mime_type(mime: &str) -> &str {
    mime.split(';').next().unwrap_or("").trim()
}

/// The extension `?as=text` gives the source of, besides an HTML page's
/// (`hasSource`).
pub fn has_source(path: &str) -> bool {
    path.to_lowercase().ends_with(".svg")
}

/// The title of a tab for a file shown without one: its name.
pub fn file_title(path: &str) -> String {
    path.replace('\\', "/")
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
        .to_string()
}

/* ── text and code ─────────────────────────────────────────────── */

/// How much of a text file is read: its head, so a 2 GB log never lands in
/// the view.
pub const TEXT_HEAD_BYTES: usize = 256 * 1024;
/// And how many of its lines are drawn, each one a row of the view.
pub const TEXT_HEAD_LINES: usize = 5000;

/// The highlighter's language for a file, by its extension or its whole
/// name; `None` for plain text, which is drawn as it is rather than guessed
/// at. The names are highlight.js's, which the TS registered; `highlight.rs`
/// maps each to a syntax (`languageOf`).
pub fn language_of(path: &str) -> Option<&'static str> {
    let name = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .to_lowercase();
    if let Some((_, language)) = NAMES.iter().find(|(whole, _)| *whole == name) {
        return Some(language);
    }
    let dot = name.rfind('.');
    // No extension, and no leading dot to make the whole name one.
    if dot.is_none_or(|dot| dot == 0) && !name.starts_with('.') {
        return None;
    }
    let extension = &name[dot? + 1..];
    EXTENSIONS
        .iter()
        .find(|(known, _)| *known == extension)
        .map(|(_, language)| *language)
}

const NAMES: &[(&str, &str)] = &[
    ("dockerfile", "dockerfile"),
    ("containerfile", "dockerfile"),
    ("makefile", "makefile"),
    ("gnumakefile", "makefile"),
    (".bashrc", "bash"),
    (".zshrc", "bash"),
    (".profile", "bash"),
    (".env", "ini"),
    (".editorconfig", "ini"),
    (".gitconfig", "ini"),
];

const EXTENSIONS: &[(&str, &str)] = &[
    ("js", "javascript"),
    ("mjs", "javascript"),
    ("cjs", "javascript"),
    ("jsx", "javascript"),
    ("ts", "typescript"),
    ("mts", "typescript"),
    ("cts", "typescript"),
    ("tsx", "typescript"),
    ("json", "json"),
    ("jsonc", "json"),
    ("json5", "json"),
    ("jsonl", "json"),
    ("ndjson", "json"),
    ("geojson", "json"),
    ("webmanifest", "json"),
    ("html", "xml"),
    ("htm", "xml"),
    ("xhtml", "xml"),
    ("xml", "xml"),
    ("svg", "xml"),
    ("plist", "xml"),
    ("csproj", "xml"),
    ("vue", "xml"),
    ("css", "css"),
    ("scss", "scss"),
    ("less", "less"),
    ("md", "markdown"),
    ("markdown", "markdown"),
    ("mdx", "markdown"),
    ("py", "python"),
    ("pyi", "python"),
    ("pyw", "python"),
    ("rs", "rust"),
    ("go", "go"),
    ("java", "java"),
    ("kt", "kotlin"),
    ("kts", "kotlin"),
    ("swift", "swift"),
    ("c", "c"),
    ("h", "c"),
    ("cpp", "cpp"),
    ("cc", "cpp"),
    ("cxx", "cpp"),
    ("hpp", "cpp"),
    ("hh", "cpp"),
    ("hxx", "cpp"),
    ("cs", "csharp"),
    ("sh", "bash"),
    ("bash", "bash"),
    ("zsh", "bash"),
    ("ps1", "powershell"),
    ("psm1", "powershell"),
    ("psd1", "powershell"),
    ("bat", "dos"),
    ("cmd", "dos"),
    ("yml", "yaml"),
    ("yaml", "yaml"),
    ("toml", "ini"),
    ("ini", "ini"),
    ("cfg", "ini"),
    ("conf", "ini"),
    ("sql", "sql"),
    ("diff", "diff"),
    ("patch", "diff"),
    ("php", "php"),
    ("rb", "ruby"),
    ("lua", "lua"),
];

/// Every language name `language_of` can answer, for the highlighter to
/// check it knows each.
pub fn languages() -> Vec<&'static str> {
    let mut all: Vec<&'static str> = NAMES
        .iter()
        .chain(EXTENSIONS.iter())
        .map(|(_, language)| *language)
        .collect();
    all.sort_unstable();
    all.dedup();
    all
}

/// A text file as the view draws it: the head that was read, and whether the
/// file goes on past it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextHead {
    pub text: String,
    pub truncated: bool,
}

/// Cuts what was read down to what is drawn. Read short of the whole file,
/// the last line is a piece of one and goes, unless it is the only one: a
/// minified file is all one line. Past `max_lines`, the rest goes too.
/// Windows line ends and a byte-order mark are not the text's (`textHead`).
pub fn text_head(read: &str, cut: bool, max_lines: usize) -> TextHead {
    let mut text = read
        .strip_prefix('\u{feff}')
        .unwrap_or(read)
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let mut truncated = cut;
    if cut {
        match text.rfind('\n') {
            Some(end) if end > 0 => text.truncate(end),
            _ => {}
        }
    }
    let lines: Vec<&str> = text.split('\n').collect();
    if lines.len() > max_lines {
        text = lines[..max_lines].join("\n");
        truncated = true;
    }
    TextHead { text, truncated }
}

/// The lines a text is drawn as: a final line break does not open an empty
/// last row (`textLines`).
pub fn text_lines(text: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.len() > 1 && lines.last() == Some(&"") {
        lines.pop();
    }
    lines
}

/// The head of a file's bytes as text: what a viewer decodes. Invalid UTF-8
/// is replaced, not refused, as `TextDecoder` does.
pub fn decode_head(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// JSON laid out for reading, only when it is all on one line, as a minified
/// file is. A file its author laid out is shown as written, so its line
/// numbers are the file's; JSON Lines is one value per line and stays so.
/// Invalid JSON is shown as it is. The layout moves whitespace only: numbers
/// and strings keep their exact spelling, where a parse and a stringify
/// would round 12345678901234567890 (`prettyJson`).
pub fn pretty_json(text: &str) -> Option<String> {
    let body = text.trim();
    if body.is_empty() || body.contains('\n') || !body.starts_with(['[', '{']) {
        return None;
    }
    if serde_json::from_str::<serde::de::IgnoredAny>(body).is_err() {
        return None;
    }
    let chars: Vec<char> = body.chars().collect();
    let indent = |depth: usize| format!("\n{}", "  ".repeat(depth));
    let mut out = String::new();
    let mut depth = 0usize;
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        match ch {
            '"' => {
                let mut end = i + 1;
                while end < chars.len() && chars[end] != '"' {
                    end += if chars[end] == '\\' { 2 } else { 1 };
                }
                out.extend(&chars[i..=end.min(chars.len() - 1)]);
                i = end;
            }
            '{' | '[' => {
                let mut next = i + 1;
                while chars.get(next).is_some_and(|c| c.is_whitespace()) {
                    next += 1;
                }
                let close = if ch == '{' { '}' } else { ']' };
                if chars.get(next) == Some(&close) {
                    out.push(ch);
                    out.push(close);
                    i = next;
                } else {
                    depth += 1;
                    out.push(ch);
                    out.push_str(&indent(depth));
                }
            }
            '}' | ']' => {
                depth = depth.saturating_sub(1);
                out.push_str(&indent(depth));
                out.push(ch);
            }
            ',' => {
                out.push(',');
                out.push_str(&indent(depth));
            }
            ':' => out.push_str(": "),
            c if !c.is_whitespace() => out.push(c),
            _ => {}
        }
        i += 1;
    }
    Some(out)
}

/* ── Markdown ──────────────────────────────────────────────────── */

/// Where a Markdown image is loaded from (`markdownImageSrc`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageSource {
    /// A file on the disk: beside the Markdown file for a relative path, or
    /// an absolute one. The folders the preview may read are still the
    /// scheme's to enforce for a page; a native viewer reads what it is
    /// given, which is why `base` is the Markdown file's own folder.
    Path(PathBuf),
    /// A web image. The native viewer does not fetch from the network; it
    /// shows the alt text, and the link opens in the browser.
    Web(String),
    /// An image inline in the document: `data:image/(png|gif|jpeg|webp|avif);…`.
    Data(String),
}

/// Where a Markdown image is loaded from: beside the file for a relative
/// path, an absolute path on the disk as it is, a web or inline image as it
/// is. Anything else (`javascript:`, `file:`) loads nothing.
pub fn markdown_image_src(src: &str, base: &Path) -> Option<ImageSource> {
    let given = src.trim();
    if given.is_empty() {
        return None;
    }
    let bytes = given.as_bytes();
    let drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\');
    if drive || given.starts_with("\\\\") {
        return Some(ImageSource::Path(PathBuf::from(given)));
    }
    let lower = given.to_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Some(ImageSource::Web(given.to_string()));
    }
    if let Some(rest) = given.strip_prefix("//") {
        return Some(ImageSource::Web(format!("https://{rest}")));
    }
    if is_inline_image(&lower) {
        return Some(ImageSource::Data(given.to_string()));
    }
    if has_scheme(given) {
        return None;
    }
    let relative = PathBuf::from(given.replace('\\', "/"));
    Some(ImageSource::Path(normalize(&base.join(relative))))
}

/// `data:image/(png|gif|jpe?g|webp|avif);`
fn is_inline_image(lower: &str) -> bool {
    let Some(rest) = lower.strip_prefix("data:image/") else {
        return false;
    };
    ["png;", "gif;", "jpg;", "jpeg;", "webp;", "avif;"]
        .iter()
        .any(|kind| rest.starts_with(kind))
}

/// `^[a-zA-Z][a-zA-Z0-9+.-]*:`
fn has_scheme(text: &str) -> bool {
    let mut chars = text.chars();
    if !chars.next().is_some_and(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    for c in chars {
        if c == ':' {
            return true;
        }
        if !(c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-')) {
            return false;
        }
    }
    false
}

/// A path with its `.` and `..` resolved without touching the disk, as a URL
/// resolves them.
fn normalize(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// What a click on a link of a rendered Markdown file does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkdownLink {
    /// A heading of the same document, by its id.
    Anchor(String),
    /// A web page or an address, opened by the system, never in the app's
    /// window.
    External(String),
    /// Anything else (a file beside it, a script) does nothing.
    None,
}

pub fn markdown_link(href: &str) -> MarkdownLink {
    let given = href.trim();
    if let Some(id) = given.strip_prefix('#').filter(|id| !id.is_empty()) {
        return MarkdownLink::Anchor(percent_decode(id));
    }
    let lower = given.to_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")
    {
        return MarkdownLink::External(given.to_string());
    }
    MarkdownLink::None
}

/// `decodeURIComponent`, keeping the text as written when it is not valid.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok());
            match hex {
                Some(byte) => {
                    out.push(byte);
                    i += 3;
                    continue;
                }
                None => return text.to_string(),
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}

/// A heading's id, the way GitHub makes them, so `[see](#setting-up)` in a
/// README finds its heading: lower case, punctuation gone, spaces as dashes.
/// `taken` counts the ids already given, for the -1, -2 of a repeated title
/// (`headingId`).
pub fn heading_id(text: &str, taken: &mut std::collections::HashMap<String, usize>) -> String {
    let cleaned: String = text
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || matches!(c, '_' | '-'))
        .map(|c| if c.is_whitespace() { '-' } else { c })
        .collect();
    let base = if cleaned.is_empty() {
        "section".to_string()
    } else {
        cleaned
    };
    let seen = taken.entry(base.clone()).or_insert(0);
    let n = *seen;
    *seen += 1;
    if n > 0 { format!("{base}-{n}") } else { base }
}

/* ── images ────────────────────────────────────────────────────── */

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub const fn new(width: f32, height: f32) -> Size {
        Size { width, height }
    }
}

/// How an image sits in its stage: its scale, and its centre's offset from
/// the stage's centre, in pixels (`ImageFrame`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageFrame {
    pub zoom: f32,
    pub x: f32,
    pub y: f32,
}

pub const ZOOM_MIN: f32 = 0.02;
pub const ZOOM_MAX: f32 = 32.0;
/// One press of + or -, or of the bar's buttons.
pub const ZOOM_STEP: f32 = 1.25;

/// The zoom that shows the whole image with a margin around it, never past
/// its own pixels: an icon shows at its size, sharp, rather than blown up
/// (`fitZoom`).
pub fn fit_zoom(image: Size, stage: Size, margin: f32) -> f32 {
    if image.width <= 0.0 || image.height <= 0.0 {
        return 1.0;
    }
    let room = |side: f32| (side - margin * 2.0).max(1.0);
    let zoom = 1.0f32
        .min(room(stage.width) / image.width)
        .min(room(stage.height) / image.height);
    zoom.clamp(ZOOM_MIN, ZOOM_MAX)
}

/// The margin a fitted image keeps from the stage's edges.
pub const FIT_MARGIN: f32 = 16.0;

/// `frame` zoomed by `factor` around `point` (from the stage's centre): the
/// spot under the pointer stays under it, as a map does. Held within
/// `ZOOM_MIN`..`ZOOM_MAX` (`zoomAround`).
pub fn zoom_around(frame: ImageFrame, factor: f32, point: (f32, f32)) -> ImageFrame {
    let zoom = (frame.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
    let k = zoom / frame.zoom;
    ImageFrame {
        zoom,
        x: point.0 - (point.0 - frame.x) * k,
        y: point.1 - (point.1 - frame.y) * k,
    }
}

/* ── PDF ───────────────────────────────────────────────────────── */

/// Whether the helper's engine draws a PDF itself, from what the page says
/// (`navigator.pdfViewerEnabled`): WebView2 and WebKit on macOS do; WebKitGTK
/// has one in recent builds, and says nothing or false in others. Only a yes
/// is taken: a window the engine cannot fill stays blank or downloads the
/// file (`pdfInline`).
pub fn pdf_inline(viewer_enabled: Option<bool>) -> bool {
    viewer_enabled == Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    fn inspected(kind: &str, patch: Value) -> Value {
        let mut base = json!({
            "path": format!("C:\\dev\\out\\file.{kind}"),
            "kind": kind,
            "mime": "x/y",
            "size": 12,
            "name": format!("file.{kind}"),
            "modifiedMs": 1_700_000_000_000u64,
        });
        if let (Some(base), Some(patch)) = (base.as_object_mut(), patch.as_object()) {
            for (key, value) in patch {
                if value.is_null() {
                    base.remove(key);
                } else {
                    base.insert(key.clone(), value.clone());
                }
            }
        }
        base
    }

    // ── from preview_inspect to a viewer ──

    #[test]
    fn draws_every_kind_rust_reports_with_its_own_viewer() {
        let viewers: Vec<(&str, Viewer)> = Kind::ALL
            .iter()
            .map(|kind| {
                let file = check_preview_file(&inspected(kind.as_str(), json!({}))).unwrap();
                (kind.as_str(), viewer_of(file.kind))
            })
            .collect();
        assert_eq!(
            viewers,
            vec![
                ("model", Viewer::Model),
                ("image", Viewer::Image),
                ("video", Viewer::Media),
                ("audio", Viewer::Media),
                ("text", Viewer::Code),
                ("markdown", Viewer::Markdown),
                ("pdf", Viewer::Pdf),
                ("html", Viewer::Html),
            ]
        );
    }

    #[test]
    fn keeps_what_rust_said_and_names_a_file_it_gave_no_name() {
        assert_eq!(
            check_preview_file(&inspected("pdf", json!({}))).unwrap(),
            PreviewFile {
                path: "C:\\dev\\out\\file.pdf".into(),
                kind: Kind::Pdf,
                mime: "x/y".into(),
                size: 12,
                name: "file.pdf".into(),
                modified_ms: 1_700_000_000_000,
            }
        );
        let odd = check_preview_file(&inspected(
            "image",
            json!({"name": null, "size": -3, "mime": 4, "modifiedMs": "now"}),
        ))
        .unwrap();
        assert_eq!(odd.name, "file.image");
        assert_eq!((odd.size, odd.mime.as_str(), odd.modified_ms), (0, "", 0));
    }

    #[test]
    fn refuses_a_kind_no_viewer_draws_and_an_answer_without_a_path() {
        let error = check_preview_file(&inspected("binary", json!({}))).unwrap_err();
        assert!(
            error.contains("no viewer for file.binary (binary). It shows: model, image, video, audio, text, markdown, pdf, html"),
            "{error}"
        );
        assert!(
            check_preview_file(&json!({"kind": "image"}))
                .unwrap_err()
                .contains("no path")
        );
        assert!(check_preview_file(&Value::Null).unwrap_err().contains("no path"));
    }

    #[test]
    fn says_what_a_file_is_by_its_type_alone_and_has_a_source_to_show_for_svg() {
        assert_eq!(mime_type("text/plain; charset=utf-8"), "text/plain");
        assert_eq!(mime_type("image/png"), "image/png");
        assert!(has_source("C:\\dev\\art\\logo.SVG"));
        assert!(!has_source("C:\\dev\\art\\logo.png"));
    }

    #[test]
    fn loads_a_file_again_under_a_new_address_each_time_it_is_shown_again() {
        let path = "C:\\dev\\site\\index.html";
        let base = model_url_for(path, false);
        assert_eq!(preview_url_for(path, 0, false, false), base);
        assert_eq!(preview_url_for(path, 2, false, false), format!("{base}?rev=2"));
        assert_eq!(preview_url_for(path, 0, true, false), format!("{base}?as=text"));
        assert_eq!(
            preview_url_for(path, 3, true, false),
            format!("{base}?as=text&rev=3")
        );
    }

    #[test]
    fn model_files_are_known_by_extension_whatever_the_case() {
        assert_eq!(model_format_of("a/b/Robot.GLB"), Some("glb"));
        assert_eq!(model_format_of("a/b/notes.txt"), None);
        assert_eq!(model_format_of("README"), None);
    }

    #[test]
    fn encodes_segments_but_keeps_separators_so_relative_urls_resolve() {
        assert_eq!(
            model_url_for("C:\\Users\\me\\my models\\cube.gltf", true),
            "http://hangar-model.localhost/C%3A/Users/me/my%20models/cube.gltf"
        );
        assert_eq!(
            model_url_for("/home/me/cube.gltf", false),
            "hangar-model://localhost/home/me/cube.gltf"
        );
    }

    // ── text and code ──

    #[test]
    fn colours_a_file_by_its_extension_or_its_name_and_leaves_plain_text_plain() {
        assert_eq!(language_of("C:\\dev\\src\\App.tsx"), Some("typescript"));
        assert_eq!(language_of("/home/me/Cargo.toml"), Some("ini"));
        assert_eq!(language_of("data.JSON"), Some("json"));
        assert_eq!(language_of("events.jsonl"), Some("json"));
        assert_eq!(language_of("site/index.html"), Some("xml"));
        assert_eq!(language_of("docker/Dockerfile"), Some("dockerfile"));
        assert_eq!(language_of(".env"), Some("ini"));
        assert_eq!(language_of("notes.txt"), None);
        assert_eq!(language_of("LICENSE"), None);
        assert_eq!(language_of(".gitignore"), None);
    }

    #[test]
    fn keeps_a_whole_file_whole_without_its_byte_order_mark_or_windows_line_ends() {
        assert_eq!(
            text_head("\u{feff}one\r\ntwo\r\n", false, TEXT_HEAD_LINES),
            TextHead {
                text: "one\ntwo\n".into(),
                truncated: false
            }
        );
    }

    #[test]
    fn drops_the_piece_of_a_line_a_cut_leaves_unless_it_is_the_only_line() {
        assert_eq!(
            text_head("one\ntwo\nthr", true, TEXT_HEAD_LINES),
            TextHead {
                text: "one\ntwo".into(),
                truncated: true
            }
        );
        assert_eq!(
            text_head("{\"minified\":true,\"and\":\"cut", true, TEXT_HEAD_LINES),
            TextHead {
                text: "{\"minified\":true,\"and\":\"cut".into(),
                truncated: true
            }
        );
    }

    #[test]
    fn stops_at_the_line_cap_and_says_the_file_goes_on() {
        let lines: Vec<String> = (1..=12).map(|i| format!("line {i}")).collect();
        let head = text_head(&lines.join("\n"), false, 10);
        assert!(head.truncated);
        assert_eq!(text_lines(&head.text).len(), 10);
        assert!(!text_head("a\nb", false, 10).truncated);
    }

    #[test]
    fn does_not_draw_an_empty_row_for_a_final_line_break() {
        assert_eq!(text_lines("a\nb\n"), vec!["a", "b"]);
        assert_eq!(text_lines("a\n\n"), vec!["a", ""]);
        assert_eq!(text_lines(""), vec![""]);
    }

    #[test]
    fn lays_out_json_written_on_one_line_keeping_numbers_and_strings_as_they_are() {
        let pretty = pretty_json(
            r#"{"id":12345678901234567890,"tags":["a,b","{c}"],"empty":{},"none":[],"q":"say \"hi\""}"#,
        );
        let expected = [
            "{",
            r#"  "id": 12345678901234567890,"#,
            r#"  "tags": ["#,
            r#"    "a,b","#,
            r#"    "{c}""#,
            "  ],",
            r#"  "empty": {},"#,
            r#"  "none": [],"#,
            r#"  "q": "say \"hi\"""#,
            "}",
        ]
        .join("\n");
        assert_eq!(pretty.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn leaves_json_its_author_laid_out_json_lines_invalid_json_and_bare_values_as_written() {
        assert_eq!(pretty_json("{\n    \"a\": 1\n}"), None);
        assert_eq!(pretty_json("{\"a\":1}\n{\"a\":2}"), None);
        assert_eq!(pretty_json("{\"a\":1"), None);
        assert_eq!(pretty_json("42"), None);
        assert_eq!(pretty_json(""), None);
    }

    // ── Markdown ──

    #[test]
    fn loads_a_relative_image_from_beside_the_file_a_disk_path_as_it_is_a_web_one_as_it_is() {
        let base = Path::new("/dev/docs");
        assert_eq!(
            markdown_image_src("img/a b.png", base),
            Some(ImageSource::Path(PathBuf::from("/dev/docs/img/a b.png")))
        );
        assert_eq!(
            markdown_image_src("../art/logo.svg", base),
            Some(ImageSource::Path(PathBuf::from("/dev/art/logo.svg")))
        );
        assert_eq!(
            markdown_image_src("C:\\shots\\one.png", base),
            Some(ImageSource::Path(PathBuf::from("C:\\shots\\one.png")))
        );
        assert_eq!(
            markdown_image_src("https://example.com/badge.svg", base),
            Some(ImageSource::Web("https://example.com/badge.svg".into()))
        );
        assert_eq!(
            markdown_image_src("//cdn.example.com/a.png", base),
            Some(ImageSource::Web("https://cdn.example.com/a.png".into()))
        );
        assert_eq!(
            markdown_image_src("data:image/png;base64,AAAA", base),
            Some(ImageSource::Data("data:image/png;base64,AAAA".into()))
        );
    }

    #[test]
    fn loads_nothing_from_another_scheme() {
        let base = Path::new("/dev/docs");
        assert_eq!(markdown_image_src("javascript:alert(1)", base), None);
        assert_eq!(markdown_image_src("file:///C:/secret.png", base), None);
        assert_eq!(
            markdown_image_src("data:text/html;base64,PHNjcmlwdD4=", base),
            None
        );
        assert_eq!(markdown_image_src("  ", base), None);
    }

    #[test]
    fn scrolls_to_a_heading_opens_the_web_outside_and_goes_nowhere_for_anything_else() {
        assert_eq!(
            markdown_link("#setting-up"),
            MarkdownLink::Anchor("setting-up".into())
        );
        assert_eq!(
            markdown_link("#caf%C3%A9"),
            MarkdownLink::Anchor("café".into())
        );
        assert_eq!(
            markdown_link("https://hangar.simnjs.fr"),
            MarkdownLink::External("https://hangar.simnjs.fr".into())
        );
        assert_eq!(
            markdown_link("mailto:me@example.com"),
            MarkdownLink::External("mailto:me@example.com".into())
        );
        assert_eq!(markdown_link("docs/other.md"), MarkdownLink::None);
        assert_eq!(markdown_link("javascript:alert(1)"), MarkdownLink::None);
        assert_eq!(markdown_link("#"), MarkdownLink::None);
    }

    #[test]
    fn gives_headings_githubs_ids_numbered_when_a_title_comes_back() {
        let mut taken = HashMap::new();
        assert_eq!(heading_id("Setting up", &mut taken), "setting-up");
        assert_eq!(heading_id("Setting up", &mut taken), "setting-up-1");
        assert_eq!(heading_id("What's new? (v1.2)", &mut taken), "whats-new-v12");
        assert_eq!(heading_id("Café crème", &mut taken), "café-crème");
        assert_eq!(heading_id("!!!", &mut taken), "section");
    }

    // ── PDF ──

    #[test]
    fn a_pdf_is_drawn_in_the_helper_only_where_the_engine_says_it_has_a_viewer() {
        assert!(pdf_inline(Some(true)));
        assert!(!pdf_inline(Some(false)));
        assert!(!pdf_inline(None));
    }

    // ── image zoom ──

    #[test]
    fn fits_an_image_inside_the_stage_with_a_margin_never_past_its_own_pixels() {
        let fit = |image: Size, stage: Size| fit_zoom(image, stage, FIT_MARGIN);
        assert_eq!(
            fit(Size::new(2000.0, 1000.0), Size::new(1032.0, 800.0)),
            0.5
        );
        assert_eq!(fit(Size::new(64.0, 64.0), Size::new(800.0, 600.0)), 1.0);
        assert_eq!(fit(Size::new(0.0, 0.0), Size::new(800.0, 600.0)), 1.0);
    }

    #[test]
    fn keeps_the_spot_under_the_pointer_where_it_is_within_the_limits() {
        let start = ImageFrame {
            zoom: 1.0,
            x: 0.0,
            y: 0.0,
        };
        let zoomed = zoom_around(start, 2.0, (100.0, -50.0));
        assert_eq!(
            zoomed,
            ImageFrame {
                zoom: 2.0,
                x: -100.0,
                y: 50.0
            }
        );
        // The image point that was under the pointer is under it still.
        let image_point = |frame: ImageFrame, p: (f32, f32)| {
            ((p.0 - frame.x) / frame.zoom, (p.1 - frame.y) / frame.zoom)
        };
        assert_eq!(
            image_point(zoomed, (100.0, -50.0)),
            image_point(start, (100.0, -50.0))
        );
        let far = zoom_around(
            ImageFrame {
                zoom: 20.0,
                x: 0.0,
                y: 0.0,
            },
            10.0,
            (0.0, 0.0),
        );
        assert_eq!(far.zoom, ZOOM_MAX);
    }

    // ── what the helper takes ──

    #[test]
    fn only_a_page_a_player_and_a_pdf_go_to_the_helper() {
        let in_helper: Vec<&str> = Kind::ALL
            .into_iter()
            .filter(|kind| kind.viewer().in_helper())
            .map(Kind::as_str)
            .collect();
        assert_eq!(in_helper, ["video", "audio", "pdf", "html"]);
    }

    #[test]
    fn a_tab_is_titled_by_its_file_name() {
        assert_eq!(file_title("C:\\a\\b\\shot.png"), "shot.png");
        assert_eq!(file_title("/a/b/NOTES.md"), "NOTES.md");
        assert_eq!(file_title("lone"), "lone");
    }
}
