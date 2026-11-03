//! Text a person can select and copy: iced 0.14 has no selection in its
//! `text` and `rich_text`, only in the editor.
//!
//! ```ignore
//! use hangar_ui::selectable::{Doc, block, selectable};
//!
//! // Once, in the view's state; `begin` again at the start of each view.
//! let doc = Doc::new(t.colors.selection);
//! doc.begin();
//! let page = column![
//!     doc.block(spans_of_a_paragraph).size(13.5),
//!     doc.block(spans_of_another).size(13.5),
//! ];
//! selectable(&doc, page).on_select(Msg::Selected).on_copy(Msg::Copied)
//! ```
//!
//! A [`Block`] is a paragraph of text, drawn as `rich_text` draws it. A
//! [`Selectable`] wraps the blocks of one view (they may be anywhere inside
//! it: in columns, rows, lists) and lets the pointer select across them: a
//! drag, a double click for a word, a triple click for a whole block,
//! Ctrl+A (Cmd+A) for everything, Ctrl+C (Cmd+C) to copy. They share a
//! [`Doc`], which is where the selection is, so that it survives the view
//! being built again after every message.
//!
//! # How it works
//!
//! iced's `Paragraph` knows which character is under a point, and every block
//! keeps its own. Each block tells its [`Doc`] where it is on each event, so
//! the wrapper, which sees the pointer for all of them, can find the block
//! and the character under it. A selection is two positions, a block's
//! number and a byte in its text (a [`Pos`]); each block learns which part of
//! it is selected when it is laid out, and paints that part with the
//! selection colour by splitting its spans there. That is why a change of
//! selection asks for a new layout, and why it costs a reshaping of the
//! blocks it touches and no more.
//!
//! Blocks are numbered in the order they are made ([`Doc::block`]), or by
//! the caller ([`Doc::block_at`]): a code view that builds only the rows in
//! sight numbers each by its line, and gives the [`Doc`] the rest of the
//! text as a [`Source`], so that a selection can run past what is built and
//! Select All takes the whole file.
//!
//! # What it does not do
//!
//! A block with a line break inside it (a hard break in Markdown) is one
//! paragraph to iced and cannot be told apart by line from a byte offset:
//! callers split it into blocks, one per line. A drag past the edge of the
//! view does not scroll it; the wheel does, while the button is held. Keys
//! other than the four above (Shift+arrows) do nothing: there is no caret.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::ops::Range;
use std::sync::{Arc, Mutex, MutexGuard};

use iced::advanced::clipboard::Kind as ClipboardKind;
use iced::advanced::layout::{self, Layout};
use iced::advanced::mouse::{Click, click};
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::text::{Difference, Hit, Paragraph as _, Renderer as _, Span, Text};
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Renderer as _, Shell, Widget};
use iced::keyboard::{self, Key, key::Named};
use iced::widget::text::{Alignment, LineHeight, Shaping, Wrapping};
use iced::{
    Background, Border, Color, Element, Event, Font, Length, Padding, Pixels, Point, Rectangle,
    Size, Vector, alignment, mouse,
};

type Para = <iced::Renderer as iced::advanced::text::Renderer>::Paragraph;

/// A place in the text of a [`Doc`]: a block, and a byte in its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub block: usize,
    pub offset: usize,
}

/// What is selected: from where the pointer went down to where it is now.
/// Either end may come first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Pos,
    pub focus: Pos,
}

impl Selection {
    /// The two ends, the earlier first.
    pub fn ordered(&self) -> (Pos, Pos) {
        if self.anchor <= self.focus {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.focus
    }

    /// The part of block `block`, which is `len` bytes long, that is selected.
    pub fn within(&self, block: usize, len: usize) -> Option<Range<usize>> {
        let (start, end) = self.ordered();
        if block < start.block || block > end.block || self.is_empty() {
            return None;
        }
        let from = if block == start.block {
            start.offset
        } else {
            0
        };
        let to = if block == end.block { end.offset } else { len };
        let (from, to) = (from.min(len), to.min(len));
        (from < to).then_some(from..to)
    }
}

/// The text of a [`Doc`] that is not built: the lines of a long file, of
/// which a view builds the few in sight. Without one, a [`Doc`] knows the
/// text of the blocks that were made.
pub trait Source: Send + Sync {
    /// How many blocks there are.
    fn count(&self) -> usize;
    /// The text of block `block`.
    fn text(&self, block: usize) -> String;
}

/// What the blocks of one view share: the selection, the text of each block,
/// and where each is on screen. Cloning it shares it.
#[derive(Clone)]
pub struct Doc {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    selection: Option<Selection>,
    paint: Color,
    /// The next number [`Doc::block`] gives.
    next: usize,
    /// The text of each block made since `begin`.
    texts: BTreeMap<usize, Arc<str>>,
    /// Where each block was when it last heard of an event.
    places: BTreeMap<usize, Place>,
    source: Option<Arc<dyn Source>>,
}

struct Place {
    bounds: Rectangle,
    paragraph: Para,
    len: usize,
}

impl fmt::Debug for Doc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inner = self.lock();
        f.debug_struct("Doc")
            .field("selection", &inner.selection)
            .field("blocks", &inner.texts.len())
            .finish()
    }
}

impl Doc {
    /// A document whose selection is painted with `paint`.
    pub fn new(paint: Color) -> Doc {
        Doc {
            inner: Arc::new(Mutex::new(Inner {
                selection: None,
                paint,
                next: 0,
                texts: BTreeMap::new(),
                places: BTreeMap::new(),
                source: None,
            })),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A panic while the lock was held leaves nothing half-written that
        // matters: the selection is a pair of numbers.
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Starts a view: the blocks are numbered from 0 again, and the text and
    /// places of the last view's blocks are forgotten. The selection stays.
    pub fn begin(&self) {
        let mut inner = self.lock();
        inner.next = 0;
        inner.texts.clear();
        inner.places.clear();
    }

    /// The colour the selection is painted in.
    pub fn paint(&self, paint: Color) {
        self.lock().paint = paint;
    }

    /// Where the text of the blocks that are not built can be had.
    pub fn source(&self, source: Option<Arc<dyn Source>>) {
        self.lock().source = source;
    }

    pub fn selection(&self) -> Option<Selection> {
        self.lock()
            .selection
            .filter(|selection| !selection.is_empty())
    }

    pub fn set_selection(&self, selection: Option<Selection>) {
        self.lock().selection = selection;
    }

    /// Selects nothing.
    pub fn clear(&self) {
        self.set_selection(None);
    }

    /// A block made now, numbered after the last.
    pub fn block<'a, Link: Clone + 'static, M>(
        &self,
        spans: impl AsRef<[Span<'a, Link, Font>]> + 'a,
    ) -> Block<'a, Link, M> {
        let number = {
            let mut inner = self.lock();
            let number = inner.next;
            inner.next += 1;
            number
        };
        self.block_at(number, spans)
    }

    /// A block with the number the caller gives it.
    pub fn block_at<'a, Link: Clone + 'static, M>(
        &self,
        number: usize,
        spans: impl AsRef<[Span<'a, Link, Font>]> + 'a,
    ) -> Block<'a, Link, M> {
        let text: String = spans
            .as_ref()
            .iter()
            .map(|span| span.text.as_ref())
            .collect();
        let len = text.len();
        let text: Arc<str> = Arc::from(text);
        self.lock().texts.insert(number, text.clone());
        Block {
            doc: self.clone(),
            number,
            len,
            text,
            spans: Box::new(spans),
            size: None,
            line_height: LineHeight::default(),
            width: Length::Shrink,
            font: None,
            wrapping: Wrapping::default(),
            color: None,
            on_link_click: None,
            hovered_link: None,
        }
    }

    /// How many blocks there are, built or not.
    fn count(&self) -> usize {
        let inner = self.lock();
        match &inner.source {
            Some(source) => source.count(),
            None => inner.texts.keys().next_back().map_or(0, |last| last + 1),
        }
    }

    fn text_of(&self, block: usize) -> Option<String> {
        let inner = self.lock();
        match &inner.source {
            Some(source) => (block < source.count()).then(|| source.text(block)),
            None => inner.texts.get(&block).map(|text| text.to_string()),
        }
    }

    /// The selected text: the parts of the first and the last block, and the
    /// blocks between, a line each.
    pub fn selected_text(&self) -> String {
        let Some(selection) = self.selection() else {
            return String::new();
        };
        let (start, end) = selection.ordered();
        let mut out = String::new();
        for block in start.block..=end.block {
            let text = self.text_of(block).unwrap_or_default();
            let part = selection.within(block, text.len());
            if block > start.block {
                out.push('\n');
            }
            if let Some(part) = part {
                out.push_str(&text[floor(&text, part.start)..floor(&text, part.end)]);
            }
        }
        out
    }

    /// Everything selected.
    pub fn select_all(&self) {
        let last = self.count().checked_sub(1);
        let selection = last.map(|block| Selection {
            anchor: Pos {
                block: 0,
                offset: 0,
            },
            focus: Pos {
                block,
                offset: self.text_of(block).map_or(0, |text| text.len()),
            },
        });
        self.set_selection(selection);
    }

    /// The part of block `block`, `len` bytes long, that is selected.
    fn selected_in(&self, block: usize, len: usize) -> Option<Range<usize>> {
        self.lock()
            .selection
            .and_then(|selection| selection.within(block, len))
    }

    fn place(&self, block: usize, bounds: Rectangle, paragraph: &Para, len: usize) {
        self.lock().places.insert(
            block,
            Place {
                bounds,
                paragraph: paragraph.clone(),
                len,
            },
        );
    }

    /// The place in the text that a point is over, or the nearest to it: a
    /// point between blocks, past the end of a line or outside the view
    /// still has a place, which is what lets a drag leave the text.
    fn pos_at(&self, point: Point) -> Option<Pos> {
        let inner = self.lock();
        let nearest = inner.places.iter().min_by(|(_, a), (_, b)| {
            let key = |place: &Place| {
                let bounds = place.bounds;
                let dy = (bounds.y - point.y)
                    .max(point.y - (bounds.y + bounds.height))
                    .max(0.0);
                let dx = (bounds.x - point.x)
                    .max(point.x - (bounds.x + bounds.width))
                    .max(0.0);
                (dy, dx)
            };
            key(a)
                .partial_cmp(&key(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let (block, place) = nearest?;
        let local = Point::new(
            (point.x - place.bounds.x).clamp(0.0, place.bounds.width),
            (point.y - place.bounds.y).clamp(0.0, place.bounds.height),
        );
        let offset = match place.paragraph.hit_test(local) {
            Some(Hit::CharOffset(offset)) => offset.min(place.len),
            None if point.x <= place.bounds.x => 0,
            None => place.len,
        };
        Some(Pos {
            block: *block,
            offset,
        })
    }

    /// The word around a place, or the stretch of spaces or signs.
    fn word_at(&self, pos: Pos) -> Selection {
        let text = self.text_of(pos.block).unwrap_or_default();
        let range = word_bounds(&text, pos.offset);
        Selection {
            anchor: Pos {
                block: pos.block,
                offset: range.start,
            },
            focus: Pos {
                block: pos.block,
                offset: range.end,
            },
        }
    }

    /// The whole of a block.
    fn block_of(&self, pos: Pos) -> Selection {
        let len = self.text_of(pos.block).map_or(0, |text| text.len());
        Selection {
            anchor: Pos {
                block: pos.block,
                offset: 0,
            },
            focus: Pos {
                block: pos.block,
                offset: len,
            },
        }
    }
}

/// The nearest byte at or before `at` that starts a character.
fn floor(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// What a character is for double-clicking: words are made of letters, digits
/// and `_`; a stretch of spaces and a stretch of signs are words of their own.
#[derive(PartialEq, Eq, Clone, Copy)]
enum Class {
    Word,
    Space,
    Sign,
}

fn class(c: char) -> Class {
    if c.is_alphanumeric() || c == '_' {
        Class::Word
    } else if c.is_whitespace() {
        Class::Space
    } else {
        Class::Sign
    }
}

/// The word at a place in a text: the stretch of one class around it. A
/// place is between two characters, and a word wins over what is beside it,
/// so that the end of a word is the word's; of a space and a sign, the one
/// after the place is taken. At the end of the text it is the last character.
pub fn word_bounds(text: &str, at: usize) -> Range<usize> {
    let at = floor(text, at);
    let after = text[at..].chars().next();
    let before = text[..at].chars().next_back();
    let kind = match (after, before) {
        (Some(c), _) if class(c) == Class::Word => Class::Word,
        (_, Some(c)) if class(c) == Class::Word => Class::Word,
        (Some(c), _) | (None, Some(c)) => class(c),
        (None, None) => return at..at,
    };
    let run = |chars: &mut dyn Iterator<Item = char>| -> usize {
        chars
            .take_while(|c| class(*c) == kind)
            .map(char::len_utf8)
            .sum()
    };
    let end = at + run(&mut text[at..].chars());
    let start = at - run(&mut text[..at].chars().rev());
    start..end
}

/// `spans` with the part of their text in `range` painted: split there, the
/// selected pieces given the highlight.
fn split_spans<'a, Link: Clone>(
    spans: &[Span<'a, Link, Font>],
    range: &Range<usize>,
    paint: Color,
) -> Vec<Span<'a, Link, Font>> {
    let highlight = iced::advanced::text::Highlight {
        background: Background::Color(paint),
        border: Border::default(),
    };
    let mut out = Vec::with_capacity(spans.len() + 2);
    let mut at = 0;
    for span in spans {
        let text = span.text.as_ref();
        let (from, to) = (at, at + text.len());
        at = to;
        let start = floor(text, range.start.clamp(from, to) - from);
        let end = floor(text, range.end.clamp(from, to) - from);
        if start >= end {
            out.push(span.clone());
            continue;
        }
        let piece = |part: &str| {
            let mut piece = span.clone();
            piece.text = Cow::Owned(part.to_string());
            piece
        };
        if start > 0 {
            out.push(piece(&text[..start]));
        }
        let mut selected = piece(&text[start..end]);
        selected.highlight = Some(highlight);
        selected.padding = Padding::ZERO;
        out.push(selected);
        if end < text.len() {
            out.push(piece(&text[end..]));
        }
    }
    out
}

/* ── the paragraph ─────────────────────────────────────────────── */

/// A paragraph of text that can take part in a selection. Made by
/// [`Doc::block`]; it is drawn as `rich_text` draws, and has its options.
pub struct Block<'a, Link: Clone + 'static, M> {
    doc: Doc,
    number: usize,
    len: usize,
    /// The text of the spans, which an operation reads (a test finds a
    /// block by it).
    text: Arc<str>,
    spans: Box<dyn AsRef<[Span<'a, Link, Font>]> + 'a>,
    size: Option<Pixels>,
    line_height: LineHeight,
    width: Length,
    font: Option<Font>,
    wrapping: Wrapping,
    color: Option<Color>,
    on_link_click: Option<Box<dyn Fn(Link) -> M + 'a>>,
    hovered_link: Option<usize>,
}

impl<'a, Link: Clone + 'static, M> Block<'a, Link, M> {
    pub fn size(mut self, size: impl Into<Pixels>) -> Self {
        self.size = Some(size.into());
        self
    }

    pub fn line_height(mut self, line_height: impl Into<LineHeight>) -> Self {
        self.line_height = line_height.into();
        self
    }

    pub fn font(mut self, font: impl Into<Font>) -> Self {
        self.font = Some(font.into());
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn wrapping(mut self, wrapping: Wrapping) -> Self {
        self.wrapping = wrapping;
        self
    }

    /// The colour of the text no span gives one to.
    pub fn color(mut self, color: impl Into<Color>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// What a press and release on a link span says.
    pub fn on_link_click(mut self, on_link_click: impl Fn(Link) -> M + 'a) -> Self {
        self.on_link_click = Some(Box::new(on_link_click));
        self
    }
}

struct BlockState<Link> {
    /// The spans the paragraph was made of: the block's own, or split where
    /// the selection starts and ends.
    spans: Vec<Span<'static, Link, Font>>,
    span_pressed: Option<usize>,
    paragraph: Para,
}

impl<Link: Clone + 'static, M> Widget<M, iced::Theme, iced::Renderer> for Block<'_, Link, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<BlockState<Link>>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(BlockState::<Link> {
            spans: Vec::new(),
            span_pressed: None,
            paragraph: Para::default(),
        })
    }

    fn size(&self) -> Size<Length> {
        Size::new(self.width, Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<BlockState<Link>>();
        let own = self.spans.as_ref().as_ref();
        let painted;
        let spans: &[Span<'_, Link, Font>] = match self.doc.selected_in(self.number, self.len) {
            Some(range) => {
                painted = split_spans(own, &range, self.doc.lock().paint);
                &painted
            }
            None => own,
        };
        layout::sized(limits, self.width, Length::Shrink, |limits| {
            let bounds = limits.max();
            let size = self.size.unwrap_or_else(|| renderer.default_size());
            let font = self.font.unwrap_or_else(|| renderer.default_font());
            let whole = || Text {
                content: spans,
                bounds,
                size,
                line_height: self.line_height,
                font,
                align_x: Alignment::Default,
                align_y: alignment::Vertical::Top,
                shaping: Shaping::Advanced,
                wrapping: self.wrapping,
            };
            if state.spans != spans {
                state.paragraph = Para::with_spans(whole());
                state.spans = spans.iter().cloned().map(Span::to_static).collect();
            } else {
                match state.paragraph.compare(Text {
                    content: (),
                    bounds,
                    size,
                    line_height: self.line_height,
                    font,
                    align_x: Alignment::Default,
                    align_y: alignment::Vertical::Top,
                    shaping: Shaping::Advanced,
                    wrapping: self.wrapping,
                }) {
                    Difference::None => {}
                    Difference::Bounds => state.paragraph.resize(bounds),
                    Difference::Shape => state.paragraph = Para::with_spans(whole()),
                }
            }
            // What is drawn over or under the text (the selection's paint, an
            // underline) does not change its shape, and iced's `Span` does
            // not count it when it compares spans: whole spans that were
            // painted or cleared would be taken for the same.
            if state.spans.len() != spans.len()
                || state.spans.iter().zip(spans).any(|(kept, now)| {
                    kept.highlight != now.highlight
                        || kept.underline != now.underline
                        || kept.strikethrough != now.strikethrough
                        || kept.link.is_some() != now.link.is_some()
                })
            {
                state.spans = spans.iter().cloned().map(Span::to_static).collect();
            }
            state.paragraph.min_bounds()
        })
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &iced::Theme,
        defaults: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        if !layout.bounds().intersects(viewport) {
            return;
        }
        let state = tree.state.downcast_ref::<BlockState<Link>>();
        let translation = layout.position() - Point::ORIGIN;
        for (index, span) in state.spans.iter().enumerate() {
            let hovered = self.on_link_click.is_some() && self.hovered_link == Some(index);
            if span.highlight.is_none() && !span.underline && !span.strikethrough && !hovered {
                continue;
            }
            let regions = state.paragraph.span_bounds(index);
            if let Some(highlight) = span.highlight {
                for region in &regions {
                    let bounds = Rectangle::new(
                        region.position() - Vector::new(span.padding.left, span.padding.top),
                        region.size() + Size::new(span.padding.x(), span.padding.y()),
                    );
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: bounds + translation,
                            border: highlight.border,
                            ..Default::default()
                        },
                        highlight.background,
                    );
                }
            }
            if span.underline || span.strikethrough || hovered {
                let size = span.size.or(self.size).unwrap_or(renderer.default_size());
                let line_height = span
                    .line_height
                    .unwrap_or(self.line_height)
                    .to_absolute(size);
                let color = span.color.or(self.color).unwrap_or(defaults.text_color);
                let baseline =
                    translation + Vector::new(0.0, size.0 + (line_height.0 - size.0) / 2.0);
                if span.underline || hovered {
                    for region in &regions {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle::new(
                                    region.position() + baseline - Vector::new(0.0, size.0 * 0.08),
                                    Size::new(region.width, 1.0),
                                ),
                                ..Default::default()
                            },
                            color,
                        );
                    }
                }
                if span.strikethrough {
                    for region in &regions {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle::new(
                                    region.position() + baseline - Vector::new(0.0, size.0 / 2.0),
                                    Size::new(region.width, 1.0),
                                ),
                                ..Default::default()
                            },
                            color,
                        );
                    }
                }
            }
        }
        iced::advanced::widget::text::draw(
            renderer,
            defaults,
            layout.bounds(),
            &state.paragraph,
            iced::advanced::widget::text::Style { color: self.color },
            viewport,
        );
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<BlockState<Link>>();
        // Where this block is, for the selection that runs across several.
        self.doc
            .place(self.number, layout.bounds(), &state.paragraph, self.len);
        let Some(on_link_clicked) = &self.on_link_click else {
            return;
        };
        let was_hovered = self.hovered_link.is_some();
        self.hovered_link = cursor.position_in(layout.bounds()).and_then(|position| {
            let span = state.paragraph.hit_span(position)?;
            state.spans.get(span)?.link.is_some().then_some(span)
        });
        if was_hovered != self.hovered_link.is_some() {
            shell.request_redraw();
        }
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if self.hovered_link.is_some() {
                    state.span_pressed = self.hovered_link;
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if let Some(span) = state.span_pressed.take()
                    && Some(span) == self.hovered_link
                    && let Some(link) = state.spans.get(span).and_then(|span| span.link.clone())
                {
                    shell.publish(on_link_clicked(link));
                }
            }
            _ => {}
        }
    }

    fn operate(
        &mut self,
        _tree: &mut Tree,
        layout: Layout<'_>,
        _renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.text(None, layout.bounds(), &self.text);
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if self.hovered_link.is_some() {
            mouse::Interaction::Pointer
        } else if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::None
        }
    }
}

impl<'a, Link: Clone + 'static, M: 'a> From<Block<'a, Link, M>> for Element<'a, M> {
    fn from(block: Block<'a, Link, M>) -> Self {
        Element::new(block)
    }
}

/* ── the wrapper ───────────────────────────────────────────────── */

/// `content`, whose [`Block`]s of `doc` can be selected with the pointer.
pub fn selectable<'a, M: 'a>(doc: &Doc, content: impl Into<Element<'a, M>>) -> Selectable<'a, M> {
    Selectable {
        doc: doc.clone(),
        content: content.into(),
        on_select: None,
        on_copy: None,
        on_press: None,
    }
}

pub struct Selectable<'a, M> {
    doc: Doc,
    content: Element<'a, M>,
    on_select: Option<Box<dyn Fn(String) -> M + 'a>>,
    on_copy: Option<Box<dyn Fn() -> M + 'a>>,
    on_press: Option<Box<dyn Fn() -> M + 'a>>,
}

impl<'a, M> Selectable<'a, M> {
    /// What the selection comes to, said when the pointer is let go and the
    /// selection is made or cleared (an empty text when it is gone): for a
    /// Copy button that copies what is selected, when something is.
    pub fn on_select(mut self, on_select: impl Fn(String) -> M + 'a) -> Self {
        self.on_select = Some(Box::new(on_select));
        self
    }

    /// Said after Ctrl+C (Cmd+C) copied the selection, for a word of
    /// thanks.
    pub fn on_copy(mut self, on_copy: impl Fn() -> M + 'a) -> Self {
        self.on_copy = Some(Box::new(on_copy));
        self
    }

    /// Said each time the left button goes down in the view, whatever was
    /// under it (a link, text, nothing): the keyboard is this view's from
    /// then on, and whatever else had it, a terminal for one, is to be told.
    pub fn on_press(mut self, on_press: impl Fn() -> M + 'a) -> Self {
        self.on_press = Some(Box::new(on_press));
        self
    }
}

#[derive(Default)]
struct State {
    /// The pointer went down in the view, and the keys are this view's until
    /// it goes down elsewhere.
    focused: bool,
    dragging: bool,
    last_click: Option<Click>,
}

impl<'a, M> Selectable<'a, M> {
    /// Moves the end of the selection to where the pointer is.
    fn drag_to(&self, cursor: mouse::Cursor, shell: &mut Shell<'_, M>) {
        let Some(point) = cursor.position() else {
            return;
        };
        let Some(pos) = self.doc.pos_at(point) else {
            return;
        };
        let current = self.doc.lock().selection;
        if let Some(selection) = current
            && selection.focus != pos
        {
            self.doc.set_selection(Some(Selection {
                focus: pos,
                ..selection
            }));
            shell.invalidate_layout();
            shell.request_redraw();
        }
    }

    fn said(&self, shell: &mut Shell<'_, M>) {
        if let Some(on_select) = &self.on_select {
            shell.publish(on_select(self.doc.selected_text()));
        }
    }
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Selectable<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[self.content.as_widget()]);
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        // The blocks first: they tell the document where they are, and a
        // press on a link is theirs.
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        let state = tree.state.downcast_mut::<State>();
        let over = cursor.is_over(layout.bounds());
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if !over {
                    // The keys are someone else's now. What is selected stays
                    // shown, as it does in a page.
                    state.focused = false;
                    return;
                }
                if let Some(on_press) = &self.on_press {
                    shell.publish(on_press());
                }
                if shell.is_event_captured() {
                    return;
                }
                let Some(point) = cursor.position() else {
                    return;
                };
                let Some(pos) = self.doc.pos_at(point) else {
                    return;
                };
                let click = Click::new(point, mouse::Button::Left, state.last_click);
                state.last_click = Some(click);
                state.focused = true;
                let selection = match click.kind() {
                    click::Kind::Single => {
                        state.dragging = true;
                        Selection {
                            anchor: pos,
                            focus: pos,
                        }
                    }
                    click::Kind::Double => {
                        state.dragging = false;
                        self.doc.word_at(pos)
                    }
                    click::Kind::Triple => {
                        state.dragging = false;
                        self.doc.block_of(pos)
                    }
                };
                self.doc.set_selection(Some(selection));
                shell.capture_event();
                shell.invalidate_layout();
                shell.request_redraw();
                if !state.dragging {
                    self.said(shell);
                }
            }
            // The pointer moves, or the view moves under a still pointer
            // (the wheel, a redraw after one).
            Event::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::WheelScrolled { .. })
            | Event::Window(iced::window::Event::RedrawRequested(_))
                if state.dragging =>
            {
                self.drag_to(cursor, shell);
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.dragging => {
                state.dragging = false;
                self.drag_to(cursor, shell);
                self.said(shell);
                shell.request_redraw();
            }
            Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
                if state.focused =>
            {
                match key.as_ref() {
                    Key::Character("c" | "C") if modifiers.command() => {
                        let text = self.doc.selected_text();
                        if !text.is_empty() {
                            clipboard.write(ClipboardKind::Standard, text);
                            if let Some(on_copy) = &self.on_copy {
                                shell.publish(on_copy());
                            }
                            shell.capture_event();
                        }
                    }
                    Key::Character("a" | "A") if modifiers.command() => {
                        self.doc.select_all();
                        shell.capture_event();
                        shell.invalidate_layout();
                        shell.request_redraw();
                        self.said(shell);
                    }
                    Key::Named(Named::Escape) if self.doc.selection().is_some() => {
                        self.doc.clear();
                        shell.capture_event();
                        shell.invalidate_layout();
                        shell.request_redraw();
                        self.said(shell);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, iced::Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, M: 'a> From<Selectable<'a, M>> for Element<'a, M> {
    fn from(selectable: Selectable<'a, M>) -> Self {
        Element::new(selectable)
    }
}

#[cfg(test)]
mod tests;
