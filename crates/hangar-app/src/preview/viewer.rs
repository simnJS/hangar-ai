//! The tab in front when it is a file, other than a 3D model or a kit's tab:
//! its viewer by kind, its bar and stage, and the line that says what the file
//! is (`components/preview/FileView.tsx`).
//!
//! A viewer is built afresh for each file and each revision of it: an agent
//! showing a file again, or rewriting it while it is on screen
//! (file_watch.rs), has it read from scratch.

use std::sync::Arc;

use iced::widget::{Space, column, container, row, text};
use iced::{Alignment, Element, Length};

use hangar_i18n::Key;
use hangar_theme::{style, tokens};
use hangar_ui::selectable::Doc;

use super::code_lines;
use super::glyph::Glyph;
use super::highlight::Lines;
use super::image::{self, Loaded as Pictured, Picture};
use super::markdown::{self, Rendered};
use super::model::{self, FileKind, Kind, language_of};
use super::parts::{self, Notice};
use super::store::{Body, Entry};
use super::text::{self as reading, Head, Load};
use super::webview::Win;
use super::windowed::{self, Action};
use super::{Msg, State};
use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::WsId;

/// A head read from a file, to ride in a message, and for a Markdown file
/// its page, parsed where the file was read: parsing a long page takes a
/// quarter of a second, which the window's thread is not to spend.
#[derive(Clone)]
pub struct Loaded(pub Arc<Head>, pub Parsed);

/// A page parsed off the window's thread, taken once by the viewer.
#[derive(Clone, Default)]
pub struct Parsed(Option<Arc<std::sync::Mutex<Option<Rendered>>>>);

impl Parsed {
    pub fn new(rendered: Rendered) -> Parsed {
        Parsed(Some(Arc::new(std::sync::Mutex::new(Some(rendered)))))
    }

    /// The page, if it was parsed and not taken yet.
    pub fn take(&self) -> Option<Rendered> {
        self.0
            .as_ref()
            .and_then(|held| held.lock().unwrap_or_else(|p| p.into_inner()).take())
    }
}

impl Loaded {
    /// A head with nothing parsed.
    pub fn new(head: Head) -> Loaded {
        Loaded(Arc::new(head), Parsed::default())
    }
}

impl std::fmt::Debug for Loaded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Loaded({} of {} bytes)",
            self.0.bytes.len(),
            self.0.total
        )
    }
}

/// The colours of a text, to ride in a message.
#[derive(Clone)]
pub struct Coloured(pub Arc<Lines>);

impl std::fmt::Debug for Coloured {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Coloured({} lines)", self.0.len())
    }
}

/// How long a viewer of a picture or a Markdown page keeps what it decoded
/// once its pane is out of sight.
pub const RELEASE_AFTER: std::time::Duration = std::time::Duration::from_secs(60);

/// Of the viewers out of sight that long, how many still keep theirs: the
/// last seen, so that going back and forth between a few tabs reads nothing
/// again.
pub const KEEP_HIDDEN: usize = 2;

/// Notes which viewers are on screen, and drops the ones of pictures and
/// Markdown pages that have not been for [`RELEASE_AFTER`], beyond the
/// [`KEEP_HIDDEN`] last seen. A picture holds up to hundreds of megabytes of
/// pixels or frames; a dropped viewer reads its file again when its pane is
/// back on screen (`mount`). Text viewers hold at most a head of 256 KB and
/// are kept, with where they were scrolled to.
pub fn release_hidden(
    viewers: &mut std::collections::HashMap<String, ViewerState>,
    shown: &std::collections::BTreeSet<String>,
    now: std::time::Instant,
) {
    for (id, viewer) in viewers.iter_mut() {
        if shown.contains(id) {
            viewer.seen = now;
        }
    }
    let mut stale: Vec<(std::time::Instant, String)> = viewers
        .iter()
        .filter(|(id, viewer)| {
            !shown.contains(*id)
                && matches!(viewer.kind, FileKind::Image | FileKind::Markdown)
                && now.saturating_duration_since(viewer.seen) >= RELEASE_AFTER
        })
        .map(|(id, viewer)| (viewer.seen, id.clone()))
        .collect();
    stale.sort_unstable_by(|a, b| b.cmp(a));
    for (_, id) in stale.into_iter().skip(KEEP_HIDDEN) {
        viewers.remove(&id);
    }
}

/// The colours of a page's code blocks, by block, to ride in a message.
#[derive(Clone)]
pub struct CodeColours(pub Vec<(u64, Arc<Lines>)>);

impl std::fmt::Debug for CodeColours {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CodeColours({} blocks)", self.0.len())
    }
}

/// What a viewer is told.
#[derive(Debug, Clone)]
pub enum ViewerMsg {
    /// The head of the text, or why it could not be read.
    Text(Result<Loaded, String>),
    /// The text of this generation, coloured.
    Coloured(u64, Option<Coloured>),
    /// Reads the text again.
    Retry,
    /// An image's bytes, or why they could not be read.
    Bytes(Result<Pictured, String>),
    ToggleSource,
    ToggleWrap,
    /// The lines scrolled: which rows to build next.
    CodeScrolled(code_lines::Scroll),
    Copy,
    /// What is selected in the lines or the page, said when the pointer is let
    /// go: empty when nothing is. The bar's Copy takes it over the whole text.
    Selected(String),
    /// Ctrl+C copied the selection, which the widget put on the clipboard.
    Copied,
    /// The pointer went down in the lines.
    Pressed,
    Image(image::Msg),
    Markdown(markdown::Msg),
    /// The pictures of a Markdown page that were read from the disk.
    MarkdownImages(Vec<(String, markdown::Resolved)>),
    /// The colours of the code blocks of the page of this generation.
    MarkdownCode(u64, CodeColours),
    Window(Action),
}

/// What a viewer holds of one revision of one file.
#[derive(Debug)]
pub struct ViewerState {
    pub revision: u64,
    pub kind: FileKind,
    /// The text, for the kinds that have one to read: asked for at once by
    /// text and Markdown, on demand by a page's or an SVG's source.
    pub load: Option<Load>,
    pub source: bool,
    pub picture: Picture,
    pub rendered: Option<Rendered>,
    /// Which text a colouring is for.
    pub generation: u64,
    /// The text read was given to the highlighter. A Markdown page's source
    /// waits until it is shown: colouring all of it takes seconds of a core,
    /// for lines nobody looks at while the page is rendered.
    pub coloured: bool,
    /// Where the lines of the text are scrolled to.
    pub scroll: code_lines::Scroll,
    /// What is selected among the lines, and what is selected of the lines or
    /// of the rendered page (the viewer shows one at a time).
    pub doc: Doc,
    pub selection: String,
    /// When its pane was last on screen: a viewer out of sight long enough
    /// lets go of what it decoded ([`release_hidden`]).
    pub seen: std::time::Instant,
}

impl ViewerState {
    pub fn new(entry: &Entry, kind: FileKind) -> ViewerState {
        ViewerState {
            revision: entry.revision,
            kind,
            load: None,
            source: false,
            picture: Picture::default(),
            rendered: None,
            generation: 0,
            coloured: false,
            scroll: code_lines::Scroll::default(),
            doc: Doc::new(iced::Color::TRANSPARENT),
            selection: String::new(),
            seen: std::time::Instant::now(),
        }
    }

    /// What the Copy button copies: what is selected when something is, else
    /// the whole text of the file.
    pub fn copy_text(&self) -> Option<String> {
        if self.selection.is_empty() {
            self.text().map(|ready| ready.text.clone())
        } else {
            Some(self.selection.clone())
        }
    }

    /// The text as it is read or being read.
    pub fn text(&self) -> Option<&reading::Ready> {
        match &self.load {
            Some(Load::Ready(ready)) => Some(ready),
            _ => None,
        }
    }
}

/// The language a text is coloured in: by its file, or by the viewer showing
/// a source.
pub fn language_for(entry: &Entry, kind: FileKind) -> Option<&'static str> {
    match kind {
        FileKind::Text => language_of(&entry.path),
        FileKind::Markdown => Some("markdown"),
        FileKind::Html | FileKind::Image => Some("xml"),
        _ => None,
    }
}

/// `entry`'s panel body: a file viewer, with the note that the file has gone
/// when it has, and the strip about the file under it.
pub fn view<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    ws: &'a WsId,
    entry: &'a Entry,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let Body::File { kind, .. } = &entry.body else {
        return Space::new().into();
    };
    let mut body = column![].width(Length::Fill).height(Length::Fill);
    if let Some(reason) = st.missing.get(&entry.id) {
        body = body.push(
            iced::widget::tooltip(
                parts::note(t, ctx.t(Key::ModelMissing).into_owned(), false),
                text(reason.as_str()).size(tokens::font_size::CAPTION),
                iced::widget::tooltip::Position::Bottom,
            )
            .style(style::container::tooltip(t)),
        );
    }
    let id = entry.id.clone();
    let revision = entry.revision;
    let viewer = st.viewers.get(&entry.id).filter(|v| v.revision == revision);
    let stage: Element<'a, ViewerMsg> = match viewer {
        Some(viewer) => viewer_view(st, ctx, ws, entry, *kind, viewer),
        None => parts::loading(ctx, &entry.title, None),
    };
    body =
        body.push(stage.map(move |msg| Message::Preview(Msg::Viewer(id.clone(), revision, msg))));
    body = body.push(info(ctx, entry, *kind));
    body.into()
}

fn viewer_view<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    ws: &'a WsId,
    entry: &'a Entry,
    kind: FileKind,
    viewer: &'a ViewerState,
) -> Element<'a, ViewerMsg> {
    let t = ctx.theme;
    let notice: Option<&'a Notice> = st.notices.get(&entry.id).map(|(_, notice)| notice);
    match kind {
        FileKind::Text => {
            let load = viewer.load.as_ref().unwrap_or(&Load::Loading);
            let pretty = matches!(load, Load::Ready(ready) if ready.pretty);
            let copy = viewer.text().map(|_| ViewerMsg::Copy);
            let language = language_for(entry, kind);
            let mut aside: Vec<Element<'a, ViewerMsg>> = Vec::new();
            if let Some(notice) = notice {
                aside.push(parts::notice_text(t, notice));
            }
            if pretty {
                aside.push(hangar_ui::tip::bottom(
                    t,
                    parts::chip(t, ctx.t(Key::PreviewPretty).into_owned(), false),
                    ctx.t(Key::PreviewPrettyHint).into_owned(),
                ));
            }
            if let Some(language) = language {
                aside.push(parts::chip(t, language, true));
            }
            let tools = vec![
                parts::glyph_tool(
                    t,
                    Glyph::Wrap,
                    ctx.t(Key::PreviewWrap),
                    Some(st.wrap),
                    Some(ViewerMsg::ToggleWrap),
                ),
                parts::icon_tool(
                    t,
                    hangar_ui::Icon::Copy,
                    ctx.t(Key::PreviewCopy),
                    None,
                    copy,
                ),
            ];
            column![
                parts::bar(t, tools, aside),
                code_stage(st, ctx, load, entry, viewer),
            ]
            .into()
        }
        FileKind::Markdown => {
            let load = viewer.load.as_ref().unwrap_or(&Load::Loading);
            let copy = viewer.text().map(|_| ViewerMsg::Copy);
            let mut tools = vec![parts::glyph_tool(
                t,
                Glyph::Code,
                ctx.t(Key::PreviewSourceHint),
                Some(viewer.source),
                Some(ViewerMsg::ToggleSource),
            )];
            if viewer.source {
                tools.push(parts::glyph_tool(
                    t,
                    Glyph::Wrap,
                    ctx.t(Key::PreviewWrap),
                    Some(st.wrap),
                    Some(ViewerMsg::ToggleWrap),
                ));
            }
            tools.push(parts::icon_tool(
                t,
                hangar_ui::Icon::Copy,
                ctx.t(Key::PreviewCopy),
                None,
                copy,
            ));
            let aside: Vec<Element<'a, ViewerMsg>> = notice
                .map(|n| parts::notice_text(t, n))
                .into_iter()
                .collect();
            let stage: Element<'a, ViewerMsg> =
                if viewer.source {
                    code_stage(st, ctx, load, entry, viewer)
                } else {
                    reading::stage(ctx, load, &entry.title, ViewerMsg::Retry, |_| match &viewer
                        .rendered
                    {
                        Some(rendered) => markdown::view(t, rendered, scroll_id(&entry.id))
                            .map(ViewerMsg::Markdown),
                        None => parts::loading(ctx, &entry.title, None),
                    })
                };
            column![parts::bar(t, tools, aside), stage].into()
        }
        FileKind::Image => {
            let svg = model::has_source(&entry.path);
            if svg && viewer.source {
                let load = viewer.load.as_ref().unwrap_or(&Load::Loading);
                let copy = viewer.text().map(|_| ViewerMsg::Copy);
                let tools = vec![
                    parts::glyph_tool(
                        t,
                        Glyph::Code,
                        ctx.t(Key::PreviewSourceHint),
                        Some(true),
                        Some(ViewerMsg::ToggleSource),
                    ),
                    parts::separator(t),
                    parts::glyph_tool(
                        t,
                        Glyph::Wrap,
                        ctx.t(Key::PreviewWrap),
                        Some(st.wrap),
                        Some(ViewerMsg::ToggleWrap),
                    ),
                    parts::icon_tool(
                        t,
                        hangar_ui::Icon::Copy,
                        ctx.t(Key::PreviewCopy),
                        None,
                        copy,
                    ),
                ];
                let aside: Vec<Element<'a, ViewerMsg>> = notice
                    .map(|n| parts::notice_text(t, n))
                    .into_iter()
                    .collect();
                return column![
                    parts::bar(t, tools, aside),
                    code_stage(st, ctx, load, entry, viewer)
                ]
                .into();
            }
            // An SVG has its source behind a button at the start of the bar.
            let toggle = svg.then(|| {
                parts::glyph_tool(
                    t,
                    Glyph::Code,
                    ctx.t(Key::PreviewSourceHint),
                    Some(false),
                    Some(image::Msg::ToggleSource),
                )
            });
            image::view(
                ctx,
                &viewer.picture,
                &entry.path,
                stage_id(&entry.id),
                toggle,
            )
            .map(ViewerMsg::Image)
        }
        FileKind::Html | FileKind::Pdf | FileKind::Video | FileKind::Audio => {
            let win: Option<&'a Win> = st.windows.get(&entry.id);
            let kind_of: Kind = kind.kind();
            let bar = windowed::Bar {
                kind,
                notice,
                source: viewer.source,
                wrap: st.wrap,
                looping: win.is_some_and(|win| win.looping),
                speed: win.map_or(1.0, |win| win.speed),
                win,
            };
            let copy = viewer.text().map(|_| ViewerMsg::Copy);
            let bar = windowed::bar(ctx, &bar, ViewerMsg::Window, ViewerMsg::ToggleWrap, copy);
            let stage: Element<'a, ViewerMsg> = if kind == FileKind::Html && viewer.source {
                let load = viewer.load.as_ref().unwrap_or(&Load::Loading);
                code_stage(st, ctx, load, entry, viewer)
            } else {
                let card = windowed::card(ctx, entry, kind_of, win, ViewerMsg::Window);
                if kind == FileKind::Html {
                    column![
                        parts::note(t, ctx.t(Key::PreviewInlineOnly).into_owned(), true),
                        card
                    ]
                    .into()
                } else {
                    card
                }
            };
            let _ = ws;
            column![bar, stage].into()
        }
    }
}

/// The stage of a text: its lines, or the state of the reading of them.
fn code_stage<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    load: &'a Load,
    entry: &'a Entry,
    viewer: &'a ViewerState,
) -> Element<'a, ViewerMsg> {
    let t = ctx.theme;
    let wrap = st.wrap;
    let id = code_id(&entry.id);
    let (scroll, doc) = (viewer.scroll, viewer.doc.clone());
    reading::stage(ctx, load, &entry.title, ViewerMsg::Retry, move |ready| {
        let select = code_lines::Select {
            doc,
            on_select: ViewerMsg::Selected,
            on_copy: || ViewerMsg::Copied,
            on_press: || ViewerMsg::Pressed,
        };
        code_lines::view(
            t,
            &ready.lines,
            wrap,
            scroll,
            ViewerMsg::CodeScrolled,
            id,
            select,
        )
    })
}

pub fn scroll_id(entry: &str) -> iced::widget::Id {
    iced::widget::Id::from(format!("preview-page:{entry}"))
}

/// The scrollable of a text's lines.
pub fn code_id(entry: &str) -> iced::widget::Id {
    iced::widget::Id::from(format!("preview-code:{entry}"))
}

pub fn stage_id(entry: &str) -> iced::widget::Id {
    iced::widget::Id::from(format!("preview-stage:{entry}"))
}

/// The line that says what the file is (`FileInfo`): its extension, its kind,
/// its size and its type.
fn info<'a>(ctx: Ctx<'a>, entry: &'a Entry, kind: FileKind) -> Element<'a, Message> {
    let t = ctx.theme;
    let Body::File { mime, size, .. } = &entry.body else {
        return Space::new().into();
    };
    let kind_name = ctx.t(kind_key(kind)).into_owned();
    let extension = entry
        .path
        .rsplit_once('.')
        .map(|(_, extension)| extension)
        .filter(|extension| !extension.contains(['/', '\\']));
    let mut line = row![
        text(extension.map_or_else(|| kind_name.clone(), str::to_string))
            .size(tokens::font_size::LABEL)
            .font(hangar_theme::fonts::mono())
            .style(style::text::subtle(t)),
        text(kind_name)
            .size(tokens::font_size::LABEL)
            .style(style::text::muted(t)),
        text(parts::byte_size(ctx, *size))
            .size(tokens::font_size::LABEL)
            .font(hangar_theme::fonts::mono())
            .style(style::text::secondary(t)),
    ]
    .spacing(tokens::space::LG)
    .align_y(Alignment::Center);
    if !mime.is_empty() {
        // The only item that gives way when the panel is narrow, ending in an
        // ellipsis (`.model-info__item--size`: `flex: 0 1 auto`, `min-width:
        // 0`); the others keep their room.
        line = line.push(
            hangar_ui::tail_text::tail_text(model::mime_type(mime).to_string())
                .cut_end()
                .width(Length::Shrink)
                .size(tokens::font_size::LABEL)
                .font(hangar_theme::fonts::mono())
                .color(t.colors.fg_subtle),
        );
    }
    column![
        iced::widget::rule::horizontal(tokens::BORDER_WIDTH).style(style::rule::hairline(t)),
        container(line)
            .padding([0.0, 12.0])
            .height(26.0)
            .width(Length::Fill)
            .align_y(Alignment::Center)
            .clip(true)
            .style(style::container::panel(t)),
    ]
    .into()
}

/// The word for a kind of file (`preview.kind.*`).
pub fn kind_key(kind: FileKind) -> Key {
    match kind {
        FileKind::Image => Key::PreviewKindImage,
        FileKind::Video => Key::PreviewKindVideo,
        FileKind::Audio => Key::PreviewKindAudio,
        FileKind::Text => Key::PreviewKindText,
        FileKind::Markdown => Key::PreviewKindMarkdown,
        FileKind::Pdf => Key::PreviewKindPdf,
        FileKind::Html => Key::PreviewKindHtml,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_of_file_has_its_word() {
        for kind in Kind::ALL {
            let Some(file) = FileKind::of(kind) else {
                continue;
            };
            let word = hangar_i18n::t(hangar_i18n::Locale::En, kind_key(file));
            assert!(
                !word.is_empty() && !word.starts_with("preview."),
                "{kind:?}: {word}"
            );
        }
    }

    #[test]
    fn a_text_is_coloured_in_the_language_of_its_file_or_of_the_source_shown() {
        let mut store = super::super::store::Store::with_clock(|| 1);
        let mut entry = |path: &str, kind: FileKind| {
            let id = store.show_file(super::super::store::ShowFile {
                workspace_id: "w".into(),
                path: path.into(),
                title: None,
                opened_by: None,
                modified_ms: 1,
                what: super::super::store::What::File {
                    kind,
                    mime: String::new(),
                    size: 1,
                },
            });
            store.entry(&id).unwrap().clone()
        };
        let rust = entry("/a/main.rs", FileKind::Text);
        assert_eq!(language_for(&rust, FileKind::Text), Some("rust"));
        let notes = entry("/a/NOTES.md", FileKind::Markdown);
        assert_eq!(language_for(&notes, FileKind::Markdown), Some("markdown"));
        let page = entry("/a/index.html", FileKind::Html);
        assert_eq!(language_for(&page, FileKind::Html), Some("xml"));
        let log = entry("/a/run.log", FileKind::Text);
        assert_eq!(language_for(&log, FileKind::Text), None);
    }

    #[test]
    fn viewers_of_pictures_out_of_sight_for_a_while_let_go_beyond_the_last_seen() {
        let mut store = super::super::store::Store::with_clock(|| 1);
        let mut viewers = std::collections::HashMap::new();
        let mut open = |path: &str, kind: FileKind| {
            let id = store.show_file(super::super::store::ShowFile {
                workspace_id: "w".into(),
                path: path.into(),
                title: None,
                opened_by: None,
                modified_ms: 1,
                what: super::super::store::What::File {
                    kind,
                    mime: String::new(),
                    size: 1,
                },
            });
            let entry = store.entry(&id).unwrap().clone();
            viewers.insert(id.clone(), ViewerState::new(&entry, kind));
            id
        };
        let pictures: Vec<String> = (0..4)
            .map(|n| open(&format!("/a/{n}.png"), FileKind::Image))
            .collect();
        let page = open("/a/README.md", FileKind::Markdown);
        let text = open("/a/main.rs", FileKind::Text);
        let start = std::time::Instant::now();
        // Each seen in turn, a second apart, the first picture last of all.
        let order = [
            &pictures[1],
            &pictures[2],
            &pictures[3],
            &page,
            &text,
            &pictures[0],
        ];
        for (n, id) in order.iter().enumerate() {
            let shown = std::collections::BTreeSet::from([(*id).clone()]);
            release_hidden(
                &mut viewers,
                &shown,
                start + std::time::Duration::from_secs(n as u64),
            );
        }
        assert_eq!(viewers.len(), 6, "nothing goes while it was seen lately");

        // A while later, the first picture still on screen.
        let later = start + RELEASE_AFTER + std::time::Duration::from_secs(10);
        let shown = std::collections::BTreeSet::from([pictures[0].clone()]);
        release_hidden(&mut viewers, &shown, later);
        // On screen: kept. The text: kept. Of the pictures and pages out of
        // sight, the two seen last (the page, then the fourth picture) are kept.
        assert!(viewers.contains_key(&pictures[0]));
        assert!(viewers.contains_key(&text));
        assert!(viewers.contains_key(&page));
        assert!(viewers.contains_key(&pictures[3]));
        assert!(!viewers.contains_key(&pictures[1]));
        assert!(!viewers.contains_key(&pictures[2]));
        assert_eq!(viewers[&pictures[0]].seen, later);
    }
}
