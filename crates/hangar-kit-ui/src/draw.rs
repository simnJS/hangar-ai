//! What a kit draws on rather than lays out: a canvas of marks over a
//! picture, which a person draws on with the pointer, and the swatches that
//! choose an ink. kit-annotate's capture is the case it was made for.
//!
//! A canvas has its own units, the size of its picture in pixels as a rule,
//! and every mark is written in them: the same marks draw the canvas on
//! screen at any zoom, and the picture Hangar writes (`canvas.export`) at
//! the picture's own size. What a drag draws, Hangar draws itself while the
//! pointer moves, and sends the kit the finished mark once it is let go: the
//! line follows the pointer at the window's rate, with no round trip.
//!
//! Inks are the one place a kit names a colour: what is drawn on a picture
//! is part of the picture, the same in a dark and a light theme, and it is
//! what an agent sees in the file Hangar writes.

use serde::{Deserialize, Serialize};

use crate::ImageSource;
use crate::element::HandlerId;
use crate::style::{Align, is_default};

fn is_false(value: &bool) -> bool {
    !*value
}

/// A point of a canvas, in its units.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub fn new(x: f32, y: f32) -> Self {
        Point { x, y }
    }
}

impl From<(f32, f32)> for Point {
    fn from((x, y): (f32, f32)) -> Self {
        Point { x, y }
    }
}

/// The colour of a mark, or of a swatch. In JSON a name, `"red"`, or a hex
/// colour, `"#e5484d"`. A name this Hangar does not know is drawn black.
///
/// The named inks are kit-annotate's, chosen to read on a screenshot of a
/// dark and of a light window alike; they are the same in every theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum Ink {
    #[default]
    Red,
    Orange,
    Yellow,
    Green,
    Teal,
    Blue,
    Purple,
    Magenta,
    Gray,
    Black,
    White,
    /// Any colour, as red, green and blue.
    Rgb([u8; 3]),
}

impl Ink {
    /// Every named ink, in the order a palette shows them.
    pub const NAMED: [Ink; 11] = [
        Ink::Red,
        Ink::Orange,
        Ink::Yellow,
        Ink::Green,
        Ink::Teal,
        Ink::Blue,
        Ink::Purple,
        Ink::Magenta,
        Ink::Gray,
        Ink::Black,
        Ink::White,
    ];

    /// Its red, green and blue.
    pub fn rgb(self) -> [u8; 3] {
        match self {
            Ink::Red => [0xe5, 0x48, 0x4d],
            Ink::Orange => [0xf0, 0x8c, 0x00],
            Ink::Yellow => [0xf5, 0xc4, 0x00],
            Ink::Green => [0x2f, 0x9e, 0x44],
            Ink::Teal => [0x12, 0xa5, 0x94],
            Ink::Blue => [0x3b, 0x82, 0xf6],
            Ink::Purple => [0x8e, 0x4e, 0xc6],
            Ink::Magenta => [0xd6, 0x40, 0x9f],
            Ink::Gray => [0x8b, 0x8d, 0x98],
            Ink::Black => [0x11, 0x11, 0x11],
            Ink::White => [0xff, 0xff, 0xff],
            Ink::Rgb(rgb) => rgb,
        }
    }

    /// Black or white, whichever reads on this ink: the edge of a pin, the
    /// number inside it. By the perceived luminance kit-annotate uses.
    pub fn contrast(self) -> Ink {
        let [r, g, b] = self.rgb();
        let luminance = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
        if luminance > 150.0 {
            Ink::Black
        } else {
            Ink::White
        }
    }

    fn name(self) -> Option<&'static str> {
        Some(match self {
            Ink::Red => "red",
            Ink::Orange => "orange",
            Ink::Yellow => "yellow",
            Ink::Green => "green",
            Ink::Teal => "teal",
            Ink::Blue => "blue",
            Ink::Purple => "purple",
            Ink::Magenta => "magenta",
            Ink::Gray => "gray",
            Ink::Black => "black",
            Ink::White => "white",
            Ink::Rgb(_) => return None,
        })
    }
}

impl From<String> for Ink {
    fn from(text: String) -> Self {
        let text = text.trim();
        if let Some(found) = Ink::NAMED.into_iter().find(|ink| {
            ink.name()
                .is_some_and(|name| name.eq_ignore_ascii_case(text))
        }) {
            return found;
        }
        let hex = text.strip_prefix('#').unwrap_or("");
        if hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).unwrap_or(0);
            return Ink::Rgb([channel(0), channel(2), channel(4)]);
        }
        Ink::Black
    }
}

impl From<Ink> for String {
    fn from(ink: Ink) -> Self {
        match ink.name() {
            Some(name) => name.to_owned(),
            None => {
                let [r, g, b] = ink.rgb();
                format!("#{r:02x}{g:02x}{b:02x}")
            }
        }
    }
}

/// What a drag on a canvas draws: Hangar draws it as the pointer moves and
/// sends the [`Mark`] when it is let go. `point` draws nothing: a press is
/// a point at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tool {
    Line,
    Arrow,
    Rect,
    Ellipse,
    /// A freehand stroke.
    Free,
    Point,
}

/// The shape of a [`Mark`]. In JSON its `type` and its geometry, beside the
/// mark's ink and width: `{ "type": "arrow", "from": {…}, "to": {…}, "ink": "red" }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Shape {
    /// A straight line; with `arrow`, a head at `to`.
    Line {
        from: Point,
        to: Point,
        #[serde(default, skip_serializing_if = "is_false")]
        arrow: bool,
    },
    /// A box, from its top left corner.
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    },
    /// The ellipse inside that box.
    Ellipse {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    },
    /// A line through these points: a freehand stroke.
    Path {
        points: Vec<Point>,
    },
    Circle {
        center: Point,
        r: f32,
    },
    /// A line of text, its middle at `at.y`. `align` says which end of it is
    /// at `at.x`. When it would leave the canvas there and `fallback` is
    /// given, it is drawn at `fallback` instead, aligned the other way: a
    /// label beside a pin that turns to the pin's other side at the edge of
    /// the picture, which a kit cannot tell without measuring the text.
    Text {
        at: Point,
        text: String,
        /// In the canvas's units.
        size: f32,
        #[serde(default, skip_serializing_if = "is_false")]
        bold: bool,
        #[serde(default, skip_serializing_if = "is_default")]
        align: Align,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fallback: Option<Point>,
    },
    /// A shape a newer Hangar draws: nothing here.
    #[serde(other)]
    Unsupported,
}

/// A mark on a canvas: a shape, its ink and the width of its line, in the
/// canvas's units, and what fills it. A text is written in its ink, on a box
/// of its `fill` when it has one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Mark {
    #[serde(flatten)]
    pub shape: Shape,
    #[serde(default)]
    pub ink: Ink,
    /// The line's width; the canvas's `stroke` by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Ink>,
}

impl Mark {
    pub fn new(shape: Shape) -> Self {
        Mark {
            shape,
            ink: Ink::Red,
            width: None,
            fill: None,
        }
    }

    pub fn line(from: impl Into<Point>, to: impl Into<Point>) -> Self {
        Mark::new(Shape::Line {
            from: from.into(),
            to: to.into(),
            arrow: false,
        })
    }

    pub fn arrow(from: impl Into<Point>, to: impl Into<Point>) -> Self {
        Mark::new(Shape::Line {
            from: from.into(),
            to: to.into(),
            arrow: true,
        })
    }

    pub fn rect(x: f32, y: f32, w: f32, h: f32) -> Self {
        Mark::new(Shape::Rect { x, y, w, h })
    }

    pub fn ellipse(x: f32, y: f32, w: f32, h: f32) -> Self {
        Mark::new(Shape::Ellipse { x, y, w, h })
    }

    pub fn path(points: impl IntoIterator<Item = Point>) -> Self {
        Mark::new(Shape::Path {
            points: points.into_iter().collect(),
        })
    }

    pub fn circle(center: impl Into<Point>, r: f32) -> Self {
        Mark::new(Shape::Circle {
            center: center.into(),
            r,
        })
    }

    /// A line of text, starting at `at`, `size` units tall.
    pub fn text(at: impl Into<Point>, text: impl Into<String>, size: f32) -> Self {
        Mark::new(Shape::Text {
            at: at.into(),
            text: text.into(),
            size,
            bold: false,
            align: Align::Start,
            fallback: None,
        })
    }

    pub fn ink(mut self, ink: Ink) -> Self {
        self.ink = ink;
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn fill(mut self, fill: Ink) -> Self {
        self.fill = Some(fill);
        self
    }

    /// A text in bold; nothing for another shape.
    pub fn bold(mut self) -> Self {
        if let Shape::Text { bold, .. } = &mut self.shape {
            *bold = true;
        }
        self
    }

    /// Which end of a text is at its point; nothing for another shape.
    pub fn align(mut self, to: Align) -> Self {
        if let Shape::Text { align, .. } = &mut self.shape {
            *align = to;
        }
        self
    }

    /// Where a text goes, aligned the other way, when it would leave the
    /// canvas at its point; nothing for another shape.
    pub fn fallback(mut self, point: impl Into<Point>) -> Self {
        if let Shape::Text { fallback, .. } = &mut self.shape {
            *fallback = Some(point.into());
        }
        self
    }

    /// The box the mark covers, `[x, y, w, h]`, its line's width aside; none
    /// for a text, whose size only Hangar knows, and for what it cannot draw.
    pub fn bounds(&self) -> Option<[f32; 4]> {
        let spread = |points: &mut dyn Iterator<Item = Point>| {
            let mut low = Point::new(f32::INFINITY, f32::INFINITY);
            let mut high = Point::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
            for point in points {
                low = Point::new(low.x.min(point.x), low.y.min(point.y));
                high = Point::new(high.x.max(point.x), high.y.max(point.y));
            }
            low.x
                .is_finite()
                .then_some([low.x, low.y, high.x - low.x, high.y - low.y])
        };
        match &self.shape {
            Shape::Line { from, to, .. } => spread(&mut [*from, *to].into_iter()),
            Shape::Rect { x, y, w, h } | Shape::Ellipse { x, y, w, h } => Some([*x, *y, *w, *h]),
            Shape::Path { points } => spread(&mut points.iter().copied()),
            Shape::Circle { center, r } => Some([center.x - r, center.y - r, 2.0 * r, 2.0 * r]),
            Shape::Text { .. } | Shape::Unsupported => None,
        }
    }
}

/// The width of a line Hangar draws by default on a canvas of this size:
/// kit-annotate's pen, a 250th of the longer side, from 2 to 12 units.
pub fn default_stroke(width: f32, height: f32) -> f32 {
    (width.max(height) / 250.0).round().clamp(2.0, 12.0)
}

/// A picture with marks on it, which a person draws on with the pointer
/// (`tool`). Its size is in its own units (`width`, `height`), a picture's
/// pixels as a rule, and so is every mark.
///
/// Drawn at `zoom` logical pixels per unit, or, without one, as large as the
/// width it is given allows and `fitHeight` (480 by default), never larger
/// than its own size: put it in a `scroll` of `both` to zoom past the panel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct Canvas<H = HandlerId> {
    /// Unique in the view: what Hangar keeps a drag in progress under.
    pub id: String,
    pub width: f32,
    pub height: f32,
    /// Drawn under the marks, stretched to the canvas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<ImageSource>,
    #[serde(default)]
    pub marks: Vec<Mark>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit_height: Option<f32>,
    /// What a drag draws; none, nothing (a press is still a point).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<Tool>,
    /// The ink of what a drag draws.
    #[serde(default)]
    pub ink: Ink,
    /// The width of what a drag draws, in units; [`default_stroke`] by
    /// default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<f32>,
    /// What the picture shows, in words: shown on hover.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// A drag finished: the [`Mark`] it drew, inside the canvas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_draw: Option<H>,
    /// A press that did not become a drag (or any press, with the `point`
    /// tool): its [`Point`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_point: Option<H>,
    /// Without a `zoom`, the scale it is drawn at, a number, each time it
    /// changes: what a zoom control shows as "fit".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_fit: Option<H>,
}

impl<H> Canvas<H> {
    pub fn new(id: impl Into<String>, width: f32, height: f32) -> Self {
        Canvas {
            id: id.into(),
            width,
            height,
            background: None,
            marks: Vec::new(),
            zoom: None,
            fit_height: None,
            tool: None,
            ink: Ink::Red,
            stroke: None,
            description: None,
            on_draw: None,
            on_point: None,
            on_fit: None,
        }
    }

    pub fn background(mut self, source: ImageSource) -> Self {
        self.background = Some(source);
        self
    }

    pub fn marks(mut self, marks: impl IntoIterator<Item = Mark>) -> Self {
        self.marks = marks.into_iter().collect();
        self
    }

    pub fn mark(mut self, mark: Mark) -> Self {
        self.marks.push(mark);
        self
    }

    /// Logical pixels per unit; none fits it to its room.
    pub fn zoom(mut self, zoom: Option<f32>) -> Self {
        self.zoom = zoom;
        self
    }

    pub fn fit_height(mut self, height: f32) -> Self {
        self.fit_height = Some(height);
        self
    }

    pub fn tool(mut self, tool: Option<Tool>) -> Self {
        self.tool = tool;
        self
    }

    pub fn ink(mut self, ink: Ink) -> Self {
        self.ink = ink;
        self
    }

    pub fn stroke(mut self, width: f32) -> Self {
        self.stroke = Some(width);
        self
    }

    pub fn description(mut self, words: impl Into<String>) -> Self {
        self.description = Some(words.into());
        self
    }

    /// The width a mark without its own is drawn with.
    pub fn line_width(&self) -> f32 {
        self.stroke
            .unwrap_or_else(|| default_stroke(self.width, self.height))
    }

    /// The scale it is drawn at in `room` logical pixels of width, without a
    /// zoom: as large as fits, never larger than its own size.
    pub fn fit(&self, room: f32) -> f32 {
        let height = self.fit_height.unwrap_or(480.0).max(40.0);
        let by_width = room / self.width.max(1.0);
        let by_height = height / self.height.max(1.0);
        by_width.min(by_height).clamp(0.01, 1.0)
    }
}

/// A square of an ink, for choosing one (`.annotate-swatch`): pressed, no
/// value; `selected` draws it ringed. `label` is its name, shown on hover.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct Swatch<H = HandlerId> {
    pub ink: Ink,
    pub label: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub selected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_press: Option<H>,
}

impl<H> Swatch<H> {
    pub fn new(ink: Ink, label: impl Into<String>) -> Self {
        Swatch {
            ink,
            label: label.into(),
            selected: false,
            on_press: None,
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}
