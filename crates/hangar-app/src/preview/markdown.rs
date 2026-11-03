//! A Markdown file, rendered, or its source on demand
//! (`components/preview/MarkdownView.tsx`, `markdown.ts`).
//!
//! Parsed and laid out by iced's `markdown` (pulldown-cmark). What the TS got
//! from markdown-it's options is kept:
//!
//! - **No raw HTML.** `html: false` showed a `<script>` or an `onerror=` as the
//!   text it is. iced's parser drops HTML, so `prepare` escapes the `<` of
//!   anything that looks like a tag before the parser sees it, outside code,
//!   and the tag is read as text. There is no HTML engine to run it either way.
//! - **Linkify.** pulldown-cmark does not link a bare address; `prepare` wraps
//!   `http://` and `https://` addresses in angle brackets, which it does.
//! - **Images** load from beside the file or by an absolute path, but only
//!   through core's `preview_inspect`, which the scheme of the old app asked
//!   as well: a file inside the folders agents may show from, of a kind the
//!   preview draws, confirmed by its first bytes, under the 64 MB cap. A
//!   README cannot make the page draw `/dev/zero`, a link to another folder
//!   or a file elsewhere on the disk; what is refused is its alt text. An
//!   inline `data:` image is read from its own bytes (`markdownImageSrc`). A
//!   local SVG is drawn as a vector, a GIF or WebP of several frames plays. A
//!   web image is not fetched: the viewer draws a link in its place, with its
//!   alt text. Fetching on opening a file would be a request the person never
//!   made, to an address the file chose; the old app made it only because its
//!   browser engine did.
//! - **Front matter** (`---` at the top, closed by `---`) is shown as a code
//!   block. iced's parser drops it; markdown-it printed it as a rule and a
//!   heading, which read worse.
//! - **Heading ids** are GitHub's, so `[see](#setting-up)` finds its heading;
//!   a link to one scrolls there.
//! - **Links** never take the window anywhere: a web address or `mailto:`
//!   opens outside, a file beside it does nothing and says so.
//! - **Code blocks** are coloured by the preview's own highlighter, in the
//!   window's theme.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use iced::advanced::widget::operation::{Operation, Outcome, Scrollable};
use iced::widget::markdown::{self, HeadingLevel, Highlight, Item, Settings, Style, Uri};
use iced::widget::scrollable::{self, AbsoluteOffset};
use iced::widget::{column, container, image, rich_text, span, text};
use iced::{Element, Font, Length, Padding, Rectangle, Task, border, padding};

use hangar_theme::{Theme, fonts, style, tokens};

use super::animated;
use super::decode::{self, Animation, Loaded};
use super::highlight::{self, Lines};
use super::model::{self, ImageSource, MarkdownLink};

/// What the rendered page tells `update`.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A link was pressed.
    Link(String),
    /// The page scrolled: its offset, and where its viewport starts.
    Scrolled(f32),
    /// A heading was found, this far down the page.
    Found(f32),
}

/// What a press on a link asks of the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Follow {
    Nothing,
    /// A web address, opened by the system.
    Open(String),
    /// A file beside the page, or a script: it goes nowhere, and says so.
    Inert(String),
}

/// An image the page holds, once it is read.
#[derive(Debug, Clone)]
pub enum Resolved {
    Raster(image::Handle),
    /// An SVG, drawn as a vector.
    Vector(iced::widget::svg::Handle),
    /// A GIF or WebP of several frames.
    Animated(Arc<Animation>),
    /// Shown as a link with its alt text.
    Web(String),
}

pub struct Rendered {
    pub content: markdown::Content,
    base: PathBuf,
    images: HashMap<Uri, Resolved>,
    /// The images that are files on the disk, by the address the page names
    /// them with: read by [`load_images`], never by the view.
    wanted: Vec<(Uri, PathBuf)>,
    offset: f32,
}

impl std::fmt::Debug for Rendered {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rendered")
            .field("items", &self.content.items().len())
            .field("base", &self.base)
            .finish()
    }
}

/// The folder a Markdown file's relative images are read from.
fn folder_of(path: &str) -> PathBuf {
    Path::new(path)
        .parent()
        .map_or_else(PathBuf::new, Path::to_path_buf)
}

impl Rendered {
    /// The text of a file, parsed, with its images resolved.
    pub fn parse(text: &str, path: &str) -> Rendered {
        let content = markdown::Content::parse(&prepare(text));
        let base = folder_of(path);
        let mut images = HashMap::new();
        let mut wanted = Vec::new();
        for url in content.images() {
            match model::markdown_image_src(url, &base) {
                Some(ImageSource::Path(file)) => wanted.push((url.clone(), file)),
                Some(ImageSource::Data(data)) => {
                    let bytes = data
                        .split_once(',')
                        .and_then(|(_, rest)| hangar_webview::body::decode(rest).ok());
                    if let Some(bytes) = bytes {
                        images.insert(
                            url.clone(),
                            Resolved::Raster(image::Handle::from_bytes(bytes)),
                        );
                    }
                }
                Some(ImageSource::Web(address)) => {
                    images.insert(url.clone(), Resolved::Web(address));
                }
                None => {}
            }
        }
        // In the order of the addresses, so that the loading is the same each
        // time.
        wanted.sort_by(|a, b| a.0.cmp(&b.0));
        Rendered {
            content,
            base,
            images,
            wanted,
            offset: 0.0,
        }
    }

    /// The files the page asks for, to be read by [`load_images`].
    pub fn wanted(&self) -> Vec<(Uri, PathBuf)> {
        self.wanted.clone()
    }

    /// Puts the images [`load_images`] found in their places on the page.
    pub fn resolve(&mut self, found: Vec<(Uri, Resolved)>) {
        self.images.extend(found);
    }

    /// What a press on a link does.
    pub fn follow(&self, href: &str) -> Follow {
        match model::markdown_link(href) {
            MarkdownLink::Anchor(_) => Follow::Nothing,
            MarkdownLink::External(url) => Follow::Open(url),
            MarkdownLink::None => Follow::Inert(href.to_string()),
        }
    }

    /// The id the anchor of a link stands for, if it is one.
    pub fn anchor(href: &str) -> Option<String> {
        match model::markdown_link(href) {
            MarkdownLink::Anchor(id) => Some(id),
            _ => None,
        }
    }

    pub fn scrolled(&mut self, offset: f32) {
        self.offset = offset;
    }

    pub fn offset(&self) -> f32 {
        self.offset
    }
}

/// The widget id of a heading by its anchor.
pub fn heading_widget(id: &str) -> iced::widget::Id {
    iced::widget::Id::from(format!("md:{id}"))
}

/// Finds a heading and answers how far down the page it is: the page is the
/// scrollable `scroll`, and a heading a container of the id the viewer gave it.
struct Locate {
    scroll: iced::widget::Id,
    target: iced::widget::Id,
    content_top: Option<f32>,
    found: Option<f32>,
}

impl Operation<f32> for Locate {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<f32>)) {
        operate(self);
    }

    fn container(&mut self, id: Option<&iced::widget::Id>, bounds: Rectangle) {
        if id == Some(&self.target) {
            self.found = self.content_top.map(|top| bounds.y - top);
        }
    }

    fn scrollable(
        &mut self,
        id: Option<&iced::widget::Id>,
        _bounds: Rectangle,
        content_bounds: Rectangle,
        _translation: iced::Vector,
        _state: &mut dyn Scrollable,
    ) {
        if id == Some(&self.scroll) {
            self.content_top = Some(content_bounds.y);
        }
    }

    fn finish(&self) -> Outcome<f32> {
        match self.found {
            Some(at) => Outcome::Some(at),
            None => Outcome::None,
        }
    }
}

/// Scrolls the page to a heading by its anchor: asks where it is, then goes.
pub fn scroll_to_heading(scroll: iced::widget::Id, anchor: &str) -> Task<Msg> {
    let locate = Locate {
        scroll,
        target: heading_widget(anchor),
        content_top: None,
        found: None,
    };
    iced::advanced::widget::operate(locate).map(Msg::Found)
}

/// Brings the page to `y`, from the top.
pub fn scroll_by(scroll: iced::widget::Id, y: f32) -> Task<Msg> {
    iced::widget::operation::scroll_to(
        scroll,
        AbsoluteOffset {
            x: None,
            y: Some(y),
        },
    )
}

/* ── the images of a page ──────────────────────────────────────── */

/// How many pictures a page may ask the disk for, and how many bytes of file
/// they may come to together: a README with a thousand screenshots shows the
/// first of them, not all the memory there is.
const MAX_IMAGES: usize = 200;
const MAX_IMAGE_BYTES: u64 = 256 * 1024 * 1024;
/// The most pixels one picture may have, as its header says.
const MAX_PIXELS: u64 = 120_000_000;

/// Reads the files a page asks for. Each goes through core's
/// `preview_inspect`, as the scheme of the old app made every image request
/// do: the file is under a folder agents may show from (symbolic links and
/// `..` are resolved first), is a regular file, is an image of the kind its
/// name says, and is no larger than the preview's cap. What passes is read
/// and decoded here, off the UI thread; what does not is left out, and the
/// page draws its alt text.
pub async fn load_images(wanted: Vec<(Uri, PathBuf)>) -> Vec<(Uri, Resolved)> {
    let mut left = MAX_IMAGE_BYTES;
    let mut found = Vec::new();
    for (url, path) in wanted.into_iter().take(MAX_IMAGES) {
        let Ok(shown) =
            hangar_core::preview::preview_inspect(path.to_string_lossy().into_owned()).await
        else {
            continue;
        };
        if shown.kind != hangar_core::preview::Kind::Image || shown.size > left {
            continue;
        }
        left -= shown.size;
        let file = PathBuf::from(shown.path);
        let read = tokio::task::spawn_blocking(move || read_image(&file)).await;
        if let Ok(Some(resolved)) = read {
            found.push((url, resolved));
        }
    }
    found
}

/// A checked file as a picture of the page, or `None` when it cannot be one.
fn read_image(file: &Path) -> Option<Resolved> {
    let bytes = decode::read_file(file).ok()?;
    if model::has_source(&file.to_string_lossy()) {
        return Some(Resolved::Vector(iced::widget::svg::Handle::from_memory(
            bytes,
        )));
    }
    // The header first: a file whose size is unreadable, or absurd, never
    // reaches the renderer, which would decode it on the UI thread.
    let (width, height) = ::image::ImageReader::new(std::io::Cursor::new(&bytes[..]))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
        .or_else(|| {
            // AVIF is read by `decode`, which refuses what it cannot read.
            super::avif::is_avif(&bytes[..bytes.len().min(512)]).then_some((1, 1))
        })?;
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return None;
    }
    Some(match decode::load(bytes).ok()? {
        Loaded::Encoded(bytes) => Resolved::Raster(image::Handle::from_bytes(bytes.0.to_vec())),
        Loaded::Pixels(handle, ..) => Resolved::Raster(handle),
        Loaded::Animated(animation) => Resolved::Animated(animation),
    })
}

/* ── before the parser ─────────────────────────────────────────── */

/// The source as the parser should see it: tags escaped so that they are text
/// (`html: false`), and bare addresses wrapped so that they are links
/// (`linkify`). Fenced code, indented code and inline code are left alone.
pub fn prepare(source: &str) -> String {
    let mut out = String::with_capacity(source.len() + source.len() / 16);
    let source = match front_matter(source) {
        Some((block, used)) => {
            out.push_str(&block);
            &source[used..]
        }
        None => source,
    };
    let mut fence: Option<(char, usize)> = None;
    let mut blank_before = true;
    let mut in_indented = false;
    for line in source.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let trimmed = body.trim_start();
        let indent = body.len() - trimmed.len();
        if let Some((mark, count)) = fence {
            // The fence ends on a line of the same mark, at least as long.
            let run = trimmed.chars().take_while(|c| *c == mark).count();
            if indent < 4 && run >= count && trimmed[run..].trim().is_empty() {
                fence = None;
            }
            out.push_str(line);
            continue;
        }
        if indent < 4 {
            for mark in ['`', '~'] {
                let run = trimmed.chars().take_while(|c| *c == mark).count();
                if run >= 3 && !(mark == '`' && trimmed[run..].contains('`')) {
                    fence = Some((mark, run));
                    break;
                }
            }
            if fence.is_some() {
                out.push_str(line);
                blank_before = false;
                in_indented = false;
                continue;
            }
        }
        let indented = (body.starts_with("    ") || body.starts_with('\t')) && !trimmed.is_empty();
        if indented && (blank_before || in_indented) {
            in_indented = true;
            out.push_str(line);
            blank_before = false;
            continue;
        }
        in_indented = false;
        if trimmed.is_empty() {
            blank_before = true;
            out.push_str(line);
            continue;
        }
        blank_before = false;
        // A reference definition `[id]: address` is not text to link.
        if is_reference(trimmed) {
            out.push_str(line);
            continue;
        }
        escape_line(body, &mut out);
        out.push_str(&line[body.len()..]);
    }
    out
}

/// The front matter at the top of a file as a fenced code block, and how much
/// of the source it took: a first line of `---` (YAML) or `+++` (TOML), then
/// the lines of the block, then the line that closes it (`---` or `...`, or
/// `+++`). It is the parser's own rule for what it hides. A block with
/// nothing in it is dropped.
fn front_matter(source: &str) -> Option<(String, usize)> {
    let mut lines = source.split_inclusive('\n');
    let first = lines.next()?;
    let (language, closers): (&str, &[&str]) = match first.trim_end() {
        "---" => ("yaml", &["---", "..."]),
        "+++" => ("toml", &["+++"]),
        _ => return None,
    };
    let mut used = first.len();
    let mut body = String::new();
    for line in lines {
        used += line.len();
        if closers.contains(&line.trim_end()) {
            if body.trim().is_empty() {
                return Some((String::new(), used));
            }
            // A fence longer than any run of backticks inside the block.
            let longest = body.split(|c| c != '`').map(str::len).max().unwrap_or(0);
            let fence = "`".repeat((longest + 1).max(3));
            return Some((format!("{fence}{language}\n{body}{fence}\n\n"), used));
        }
        body.push_str(line);
    }
    None
}

fn is_reference(line: &str) -> bool {
    line.starts_with('[') && line.find("]:").is_some_and(|end| end > 1)
}

/// One line of prose, with its inline code and its link destinations left as
/// they are.
fn escape_line(line: &str, out: &mut String) {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' => {
                out.push(c);
                if let Some(next) = chars.get(i + 1) {
                    out.push(*next);
                    i += 1;
                }
            }
            '`' => {
                let run = chars[i..].iter().take_while(|c| **c == '`').count();
                let closing = (i + run..chars.len()).find(|start| {
                    chars[*start..].iter().take_while(|c| **c == '`').count() == run
                        && (*start == 0 || chars[*start - 1] != '`')
                });
                match closing {
                    Some(start) => {
                        out.extend(&chars[i..start + run]);
                        i = start + run - 1;
                    }
                    None => out.extend(&chars[i..i + run]),
                }
                if closing.is_none() {
                    i += run - 1;
                }
            }
            ']' if chars.get(i + 1) == Some(&'(') => {
                // The destination of a link or an image, to its closing
                // parenthesis (nested ones counted).
                let mut depth = 0;
                let mut end = i + 1;
                while end < chars.len() {
                    match chars[end] {
                        '(' => depth += 1,
                        ')' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        '\\' => end += 1,
                        _ => {}
                    }
                    end += 1;
                }
                let stop = end.min(chars.len() - 1);
                out.extend(&chars[i..=stop]);
                i = stop;
            }
            '<' => match autolink_end(&chars, i) {
                Some(end) => {
                    out.extend(&chars[i..=end]);
                    i = end;
                }
                None => {
                    if looks_like_a_tag(&chars, i) {
                        out.push('\\');
                    }
                    out.push('<');
                }
            },
            'h' | 'H' if starts_address(&chars, i) => {
                let end = address_end(&chars, i);
                out.push('<');
                out.extend(&chars[i..end]);
                out.push('>');
                i = end - 1;
            }
            _ => out.push(c),
        }
        i += 1;
    }
}

/// Whether the `<` at `start` opens raw HTML: a comment (which may go on for
/// lines), or a tag, closing tag, declaration or instruction closed by a `>`
/// before the line ends. `a < b` and `x<y` are prose.
fn looks_like_a_tag(chars: &[char], start: usize) -> bool {
    let Some(next) = chars.get(start + 1) else {
        return false;
    };
    if !(next.is_ascii_alphabetic() || matches!(next, '/' | '!' | '?')) {
        return false;
    }
    if chars[start + 1..].starts_with(&['!', '-', '-']) {
        return true;
    }
    for ch in &chars[start + 1..] {
        match ch {
            '>' => return true,
            '<' => return false,
            _ => {}
        }
    }
    false
}

/// `<scheme:…>` or `<name@host>`: what the parser already links.
fn autolink_end(chars: &[char], start: usize) -> Option<usize> {
    let end = (start + 1..chars.len()).find(|at| chars[*at] == '>')?;
    let inside: String = chars[start + 1..end].iter().collect();
    if inside.is_empty() || inside.contains([' ', '<', '\t']) {
        return None;
    }
    let scheme = inside.split_once(':').is_some_and(|(scheme, _)| {
        scheme
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
    });
    let email = inside
        .split_once('@')
        .is_some_and(|(name, host)| !name.is_empty() && !host.is_empty());
    (scheme || email).then_some(end)
}

/// Whether an `http://` or `https://` address starts here, on its own: not
/// inside a word, an attribute or an angle bracket.
fn starts_address(chars: &[char], at: usize) -> bool {
    let rest: String = chars[at..]
        .iter()
        .take(8)
        .collect::<String>()
        .to_lowercase();
    let scheme = if rest.starts_with("https://") {
        8
    } else if rest.starts_with("http://") {
        7
    } else {
        return false;
    };
    let before = at.checked_sub(1).map(|b| chars[b]);
    let alone =
        !before.is_some_and(|c| c.is_alphanumeric() || matches!(c, '<' | '"' | '\'' | '=' | '/'));
    alone
        && chars
            .get(at + scheme)
            .is_some_and(|c| !c.is_whitespace() && *c != '<' && *c != '>')
}

/// Where an address ends: at a space or an angle bracket, less the
/// punctuation a sentence puts after it, and a closing parenthesis nothing
/// opened.
fn address_end(chars: &[char], start: usize) -> usize {
    let mut end = start;
    while end < chars.len() && !chars[end].is_whitespace() && !matches!(chars[end], '<' | '>') {
        end += 1;
    }
    while end > start {
        let last = chars[end - 1];
        let opened = chars[start..end].iter().filter(|c| **c == '(').count();
        let closed = chars[start..end].iter().filter(|c| **c == ')').count();
        if matches!(
            last,
            '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '"' | '*' | '_'
        ) || (last == ')' && closed > opened)
        {
            end -= 1;
        } else {
            break;
        }
    }
    end
}

/* ── the page ──────────────────────────────────────────────────── */

/// The size of the text of a page (`font-size: 13.5px`).
const TEXT: f32 = 13.5;

/// Headings, in ems of the text (`h1 1.75em`…).
fn heading_size(level: HeadingLevel) -> f32 {
    TEXT * match level {
        HeadingLevel::H1 => 1.75,
        HeadingLevel::H2 => 1.4,
        HeadingLevel::H3 => 1.18,
        HeadingLevel::H4 => 1.05,
        HeadingLevel::H5 | HeadingLevel::H6 => 0.95,
    }
}

fn settings(t: &Theme) -> Settings {
    let c = &t.colors;
    let mut settings = Settings::with_text_size(
        TEXT,
        Style {
            font: fonts::ui(),
            inline_code_highlight: Highlight {
                background: iced::Background::Color(c.surface_hover),
                border: border::rounded(tokens::radius::XS),
            },
            inline_code_padding: padding::left(3).right(3),
            inline_code_color: c.fg,
            inline_code_font: fonts::mono(),
            code_block_font: fonts::mono(),
            link_color: c.accent_hi,
        },
    );
    settings.h1_size = heading_size(HeadingLevel::H1).into();
    settings.h2_size = heading_size(HeadingLevel::H2).into();
    settings.h3_size = heading_size(HeadingLevel::H3).into();
    settings.h4_size = heading_size(HeadingLevel::H4).into();
    settings.h5_size = heading_size(HeadingLevel::H5).into();
    settings.h6_size = heading_size(HeadingLevel::H6).into();
    settings.code_size = (TEXT * 0.86).into();
    settings.spacing = TEXT.into();
    settings
}

struct Page<'a> {
    t: &'a Theme,
    images: &'a HashMap<Uri, Resolved>,
    /// The ids given so far, for the -1, -2 of a repeated title.
    ids: RefCell<HashMap<String, usize>>,
}

impl<'a> markdown::Viewer<'a, Msg, iced::Theme, iced::Renderer> for Page<'a> {
    fn on_link_click(url: Uri) -> Msg {
        Msg::Link(url)
    }

    fn image(
        &self,
        settings: Settings,
        url: &'a Uri,
        _title: &'a str,
        alt: &markdown::Text,
    ) -> Element<'a, Msg> {
        match self.images.get(url) {
            Some(Resolved::Raster(handle)) => image(handle.clone())
                .content_fit(iced::ContentFit::ScaleDown)
                .into(),
            Some(Resolved::Vector(handle)) => iced::widget::svg(handle.clone())
                .width(Length::Shrink)
                .height(Length::Shrink)
                .content_fit(iced::ContentFit::ScaleDown)
                .into(),
            Some(Resolved::Animated(animation)) => animated::Inline::new(animation.clone()).into(),
            Some(Resolved::Web(address)) => {
                // The window fetches nothing from the network: the picture is
                // its alt text, and a link to where it lives.
                let words: String = alt
                    .spans(settings.style)
                    .iter()
                    .map(|part| part.text.as_ref())
                    .collect();
                let label = if words.trim().is_empty() {
                    address.clone()
                } else {
                    words
                };
                rich_text([span(label)
                    .color(self.t.colors.accent_hi)
                    .underline(true)
                    .link(address.clone())])
                .size(settings.text_size)
                .on_link_click(Msg::Link)
                .into()
            }
            // An image that may not load is its alt text, rather than a
            // broken frame.
            None => rich_text(alt.spans(settings.style))
                .size(settings.text_size)
                .on_link_click(Msg::Link)
                .into(),
        }
    }

    fn heading(
        &self,
        settings: Settings,
        level: &'a HeadingLevel,
        heading: &'a markdown::Text,
        index: usize,
    ) -> Element<'a, Msg> {
        let words: String = heading
            .spans(settings.style)
            .iter()
            .map(|part| part.text.as_ref())
            .collect();
        let id = model::heading_id(&words, &mut self.ids.borrow_mut());
        // Semibold, with a line under the first two levels (`h1`, `h2`).
        let spans: Vec<text::Span<'static, Uri, Font>> = heading
            .spans(settings.style)
            .iter()
            .cloned()
            .map(|part| part.font(fonts::ui_semibold()))
            .collect();
        let size = match level {
            HeadingLevel::H1 => settings.h1_size,
            HeadingLevel::H2 => settings.h2_size,
            HeadingLevel::H3 => settings.h3_size,
            HeadingLevel::H4 => settings.h4_size,
            HeadingLevel::H5 => settings.h5_size,
            HeadingLevel::H6 => settings.h6_size,
        };
        let mut words = column![
            rich_text(spans)
                .size(size)
                .on_link_click(Self::on_link_click)
        ]
        .spacing(4);
        match level {
            HeadingLevel::H1 => {
                words = words
                    .push(iced::widget::rule::horizontal(1).style(style::rule::strong(self.t)));
            }
            HeadingLevel::H2 => {
                words = words
                    .push(iced::widget::rule::horizontal(1).style(style::rule::hairline(self.t)));
            }
            _ => {}
        }
        container(words)
            .padding(padding::top(if index > 0 {
                settings.text_size / 2.0
            } else {
                0.0.into()
            }))
            .id(heading_widget(&id))
            .into()
    }

    fn code_block(
        &self,
        settings: Settings,
        language: Option<&'a str>,
        code: &'a str,
        lines: &'a [markdown::Text],
    ) -> Element<'a, Msg> {
        let t = self.t;
        let coloured: Option<Lines> =
            language.and_then(|language| highlight::highlight_block(code, language));
        let mono = fonts::mono();
        let rows: Vec<Element<'a, Msg>> = match coloured {
            Some(coloured) => coloured
                .into_iter()
                .map(|runs| {
                    let spans: Vec<text::Span<'a, Uri, Font>> = runs
                        .into_iter()
                        .map(|run| {
                            let part = span(run.text).font(mono);
                            match highlight::colour(t, run.class) {
                                Some(colour) => part.color(colour),
                                None => part,
                            }
                        })
                        .collect();
                    rich_text(spans).size(settings.code_size).into()
                })
                .collect(),
            None => lines
                .iter()
                .map(|line| {
                    rich_text(line.spans(settings.style))
                        .font(mono)
                        .size(settings.code_size)
                        .color(t.colors.fg)
                        .into()
                })
                .collect(),
        };
        container(
            iced::widget::scrollable(container(column(rows)).padding(settings.code_size))
                .direction(scrollable::Direction::Horizontal(
                    scrollable::Scrollbar::default()
                        .width(settings.code_size / 2.0)
                        .scroller_width(settings.code_size / 2.0),
                ))
                .style(style::scrollable::default(t)),
        )
        .width(Length::Fill)
        .style(move |_| iced::widget::container::Style {
            background: Some(iced::Background::Color(t.colors.bg_sunken)),
            border: iced::Border {
                color: t.colors.line,
                width: 1.0,
                radius: tokens::radius::MD.into(),
            },
            ..Default::default()
        })
        .into()
    }

    fn rule(&self, _settings: Settings) -> Element<'a, Msg> {
        iced::widget::rule::horizontal(1)
            .style(style::rule::strong(self.t))
            .into()
    }
}

/// The page, in a scrollable of its own (`.preview-markdown`).
pub fn view<'a>(
    t: &'a Theme,
    rendered: &'a Rendered,
    scroll: iced::widget::Id,
) -> Element<'a, Msg> {
    let page = Page {
        t,
        images: &rendered.images,
        ids: RefCell::new(HashMap::new()),
    };
    let body = markdown::view_with(rendered.content.items(), settings(t), &page);
    iced::widget::scrollable(
        container(body)
            .padding(Padding {
                top: 20.0,
                right: 32.0,
                bottom: 44.0,
                left: 32.0,
            })
            .width(Length::Fill)
            .style(move |_| iced::widget::container::Style {
                background: Some(iced::Background::Color(t.colors.bg)),
                text_color: Some(t.colors.fg),
                ..Default::default()
            }),
    )
    .id(scroll)
    .on_scroll(|viewport| Msg::Scrolled(viewport.absolute_offset().y))
    .width(Length::Fill)
    .height(Length::Fill)
    .style(style::scrollable::default(t))
    .into()
}

/// The ids the headings of a page get, in order (what a `#anchor` can name).
pub fn heading_ids(content: &markdown::Content, t: &Theme) -> Vec<String> {
    let settings = settings(t);
    let mut taken = HashMap::new();
    let mut ids = Vec::new();
    fn walk(
        items: &[Item],
        settings: Settings,
        taken: &mut HashMap<String, usize>,
        ids: &mut Vec<String>,
    ) {
        for item in items {
            match item {
                Item::Heading(_, heading) => {
                    let words: String = heading
                        .spans(settings.style)
                        .iter()
                        .map(|part| part.text.as_ref())
                        .collect();
                    ids.push(model::heading_id(&words, taken));
                }
                Item::Quote(inner) => walk(inner, settings, taken, ids),
                Item::List { bullets, .. } => {
                    for bullet in bullets {
                        match bullet {
                            markdown::Bullet::Point { items }
                            | markdown::Bullet::Task { items, .. } => {
                                walk(items, settings, taken, ids)
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    walk(content.items(), settings, &mut taken, &mut ids);
    ids
}

#[cfg(test)]
mod tests;
