//! The widget tree a kit's `view` gives, every component of Hangar UI
//! (docs/kit-ui.md) as a native widget, and what a native tab needs besides:
//! layout, tables, trees, charts.
//!
//! The tree is generic over `H`, what a widget names to be told about a
//! click or a keystroke. On the wire it is a [`HandlerId`], a number the kit
//! chose; the SDK builds the same tree with its own closures in that place
//! and numbers them when it sends the view, with
//! [`Element::map_handlers`]. Hangar never reads a handler: it sends the
//! number back in a [`UiEvent`](crate::UiEvent).
//!
//! Every struct is `#[non_exhaustive]`, so a field added later breaks no
//! kit: build them with their `new` and setters, or set their public fields.
//! In JSON every field that has a default may be left out, and a field
//! Hangar does not know is ignored.

use serde::{Deserialize, Serialize};

use crate::style::*;

/// The number a kit gives a handler in one view. Hangar sends it back with
/// the event, and the view's `seq`: numbers need only be unique in a view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HandlerId(pub u32);

/// A widget, and through its children a whole page.
///
/// In JSON, an object whose `type` names the widget, in camelCase:
/// `{ "type": "button", "label": "Refresh", "onPress": 3 }`. A `type` this
/// Hangar does not know reads as [`Element::Unsupported`], drawn as a line
/// saying a newer Hangar is needed, so a kit built with a newer SDK still
/// shows the rest of its page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub enum Element<H = HandlerId> {
    // Layout.
    Column(Column<H>),
    Row(Row<H>),
    Grid(Grid<H>),
    Scroll(Scroll<H>),
    Split(Split<H>),
    Container(Container<H>),
    Page(Page<H>),
    Bar(Bar<H>),
    Space(Space),
    Divider(Divider),
    Tooltip(Tooltip<H>),
    // Text.
    Text(Text),
    Spans(Spans<H>),
    Pre(Pre),
    Log(Log),
    KeyValue(KeyValue<H>),
    // The parts of a page.
    Header(Header<H>),
    Section(Section<H>),
    List(List<H>),
    Table(Table<H>),
    Tree(Tree<H>),
    Tabs(Tabs<H>),
    Empty(Empty<H>),
    Banner(Banner<H>),
    // Controls.
    Button(Button<H>),
    Link(Link<H>),
    Menu(Menu<H>),
    TextInput(TextInput<H>),
    TextArea(TextArea<H>),
    Search(Search<H>),
    Select(Select<H>),
    Checkbox(Checkbox<H>),
    Switch(Switch<H>),
    Filter(Filter<H>),
    // States.
    Chip(Chip),
    State(StateLabel),
    Glyph(Glyph),
    Progress(Progress),
    Icon(Symbol),
    // Media and charts.
    Image(Image),
    Sparkline(Sparkline),
    Bars(Bars),
    /// A widget a newer Hangar knows.
    #[serde(other)]
    Unsupported,
}

macro_rules! into_element {
    (generic: $($variant:ident($ty:ident)),* ; plain: $($pvariant:ident($pty:ident)),* $(,)?) => {
        $(
            impl<H> From<$ty<H>> for Element<H> {
                fn from(widget: $ty<H>) -> Self {
                    Element::$variant(widget)
                }
            }
        )*
        $(
            impl<H> From<$pty> for Element<H> {
                fn from(widget: $pty) -> Self {
                    Element::$pvariant(widget)
                }
            }
        )*
    };
}

into_element!(
    generic: Column(Column), Row(Row), Grid(Grid), Scroll(Scroll), Split(Split),
        Container(Container), Page(Page), Bar(Bar), Tooltip(Tooltip), Spans(Spans),
        KeyValue(KeyValue), Header(Header), Section(Section), List(List), Table(Table),
        Tree(Tree), Tabs(Tabs), Empty(Empty), Banner(Banner), Button(Button), Link(Link),
        Menu(Menu), TextInput(TextInput), TextArea(TextArea), Search(Search), Select(Select),
        Checkbox(Checkbox), Switch(Switch), Filter(Filter);
    plain: Space(Space), Divider(Divider), Text(Text), Pre(Pre), Log(Log), Chip(Chip),
        State(StateLabel), Glyph(Glyph), Progress(Progress), Icon(Symbol), Image(Image),
        Sparkline(Sparkline), Bars(Bars),
);

impl<H> From<&str> for Element<H> {
    fn from(text: &str) -> Self {
        Element::Text(Text::new(text))
    }
}

impl<H> From<String> for Element<H> {
    fn from(text: String) -> Self {
        Element::Text(Text::new(text))
    }
}

fn map_all<H, H2, F: FnMut(H) -> H2>(elements: Vec<Element<H>>, f: &mut F) -> Vec<Element<H2>> {
    elements
        .into_iter()
        .map(|element| element.map_handlers(f))
        .collect()
}

fn map_box<H, H2, F: FnMut(H) -> H2>(element: Element<H>, f: &mut F) -> Box<Element<H2>> {
    Box::new(element.map_handlers(f))
}

fn map_opt_box<H, H2, F: FnMut(H) -> H2>(
    element: Option<Box<Element<H>>>,
    f: &mut F,
) -> Option<Box<Element<H2>>> {
    element.map(|element| map_box(*element, f))
}

impl<H> Element<H> {
    /// Calls `f` on this element and on every element under it, depth
    /// first, a parent before its children: to find a widget in a view, or
    /// every image it shows.
    pub fn for_each<F: FnMut(&Element<H>)>(&self, f: &mut F) {
        f(self);
        let all = |elements: &[Element<H>], f: &mut F| {
            elements.iter().for_each(|element| element.for_each(f))
        };
        match self {
            Element::Column(w) => all(&w.children, f),
            Element::Row(w) => all(&w.children, f),
            Element::Grid(w) => all(&w.children, f),
            Element::Page(w) => all(&w.children, f),
            Element::Bar(w) => all(&w.children, f),
            Element::Scroll(w) => w.child.for_each(f),
            Element::Container(w) => w.child.for_each(f),
            Element::Tooltip(w) => w.child.for_each(f),
            Element::Split(w) => {
                w.first.for_each(f);
                w.second.for_each(f);
            }
            Element::KeyValue(w) => w.pairs.iter().for_each(|pair| pair.value.for_each(f)),
            Element::Header(w) => {
                all(&w.state, f);
                all(&w.actions, f);
            }
            Element::Section(w) => {
                all(&w.actions, f);
                all(&w.children, f);
            }
            Element::List(w) => {
                for item in &w.items {
                    if let Some(lead) = &item.lead {
                        lead.for_each(f);
                    }
                    if let Some(end) = &item.end {
                        end.for_each(f);
                    }
                    all(&item.actions, f);
                }
            }
            Element::Table(w) => w
                .rows
                .iter()
                .flat_map(|row| &row.cells)
                .for_each(|cell| cell.content.for_each(f)),
            Element::Empty(w) => all(&w.actions, f),
            Element::Banner(w) => all(&w.actions, f),
            _ => {}
        }
    }

    /// The same tree, each handler replaced by what `f` makes of it, in the
    /// order a depth-first walk meets them. The SDK numbers its closures with
    /// this; a kit composing parts of its page can wrap their messages.
    pub fn map_handlers<H2, F: FnMut(H) -> H2>(self, f: &mut F) -> Element<H2> {
        match self {
            Element::Column(w) => Element::Column(Column {
                children: map_all(w.children, f),
                spacing: w.spacing,
                padding: w.padding,
                align: w.align,
                width: w.width,
                height: w.height,
            }),
            Element::Row(w) => Element::Row(Row {
                children: map_all(w.children, f),
                spacing: w.spacing,
                padding: w.padding,
                align: w.align,
                wrap: w.wrap,
                width: w.width,
                height: w.height,
            }),
            Element::Grid(w) => Element::Grid(Grid {
                children: map_all(w.children, f),
                columns: w.columns,
                min_width: w.min_width,
                spacing: w.spacing,
            }),
            Element::Scroll(w) => Element::Scroll(Scroll {
                child: map_box(*w.child, f),
                axis: w.axis,
                width: w.width,
                height: w.height,
                stick_to_end: w.stick_to_end,
            }),
            Element::Split(w) => Element::Split(Split {
                first: map_box(*w.first, f),
                second: map_box(*w.second, f),
                axis: w.axis,
                ratio: w.ratio,
            }),
            Element::Container(w) => Element::Container(Container {
                child: map_box(*w.child, f),
                padding: w.padding,
                surface: w.surface,
                border: w.border,
                radius: w.radius,
                width: w.width,
                height: w.height,
                align_x: w.align_x,
                align_y: w.align_y,
            }),
            Element::Page(w) => Element::Page(Page {
                children: map_all(w.children, f),
                fill: w.fill,
            }),
            Element::Bar(w) => Element::Bar(Bar {
                children: map_all(w.children, f),
                foot: w.foot,
            }),
            Element::Space(w) => Element::Space(w),
            Element::Divider(w) => Element::Divider(w),
            Element::Tooltip(w) => Element::Tooltip(Tooltip {
                child: map_box(*w.child, f),
                text: w.text,
            }),
            Element::Text(w) => Element::Text(w),
            Element::Spans(w) => Element::Spans(Spans {
                spans: w
                    .spans
                    .into_iter()
                    .map(|span| Span {
                        text: span.text,
                        tone: span.tone,
                        weight: span.weight,
                        mono: span.mono,
                        on_press: span.on_press.map(&mut *f),
                    })
                    .collect(),
                size: w.size,
                wrap: w.wrap,
            }),
            Element::Pre(w) => Element::Pre(w),
            Element::Log(w) => Element::Log(w),
            Element::KeyValue(w) => Element::KeyValue(KeyValue {
                pairs: w
                    .pairs
                    .into_iter()
                    .map(|pair| Pair {
                        key: pair.key,
                        value: pair.value.map_handlers(f),
                    })
                    .collect(),
            }),
            Element::Header(w) => Element::Header(Header {
                title: w.title,
                state: map_all(w.state, f),
                actions: map_all(w.actions, f),
            }),
            Element::Section(w) => Element::Section(Section {
                title: w.title,
                count: w.count,
                actions: map_all(w.actions, f),
                children: map_all(w.children, f),
            }),
            Element::List(w) => Element::List(List {
                items: w
                    .items
                    .into_iter()
                    .map(|item| ListItem {
                        lead: map_opt_box(item.lead, f),
                        title: item.title,
                        sub: item.sub,
                        end: map_opt_box(item.end, f),
                        actions: map_all(item.actions, f),
                        selected: item.selected,
                        wrap: item.wrap,
                        tooltip: item.tooltip,
                        on_press: item.on_press.map(&mut *f),
                    })
                    .collect(),
                group: w.group,
            }),
            Element::Table(w) => Element::Table(Table {
                id: w.id,
                columns: w.columns,
                rows: w
                    .rows
                    .into_iter()
                    .map(|row| TableRow {
                        cells: row
                            .cells
                            .into_iter()
                            .map(|cell| Cell {
                                content: cell.content.map_handlers(f),
                                key: cell.key,
                            })
                            .collect(),
                    })
                    .collect(),
                sort: w.sort,
                selected: w.selected,
                height: w.height,
                on_sort: w.on_sort.map(&mut *f),
                on_row: w.on_row.map(&mut *f),
            }),
            Element::Tree(w) => Element::Tree(Tree {
                id: w.id,
                nodes: w.nodes,
                selected: w.selected,
                on_select: w.on_select.map(&mut *f),
            }),
            Element::Tabs(w) => Element::Tabs(Tabs {
                tabs: w.tabs,
                selected: w.selected,
                on_select: w.on_select.map(&mut *f),
            }),
            Element::Empty(w) => Element::Empty(Empty {
                kind: w.kind,
                title: w.title,
                text: w.text,
                actions: map_all(w.actions, f),
            }),
            Element::Banner(w) => Element::Banner(Banner {
                text: w.text,
                kind: w.kind,
                actions: map_all(w.actions, f),
            }),
            Element::Button(w) => Element::Button(Button {
                label: w.label,
                icon: w.icon,
                kind: w.kind,
                small: w.small,
                tooltip: w.tooltip,
                on_press: w.on_press.map(&mut *f),
            }),
            Element::Link(w) => Element::Link(Link {
                label: w.label,
                on_press: w.on_press.map(&mut *f),
            }),
            Element::Menu(w) => Element::Menu(Menu {
                label: w.label,
                icon: w.icon,
                kind: w.kind,
                small: w.small,
                items: w.items,
                on_select: w.on_select.map(&mut *f),
            }),
            Element::TextInput(w) => Element::TextInput(TextInput {
                value: w.value,
                placeholder: w.placeholder,
                label: w.label,
                hint: w.hint,
                error: w.error,
                secure: w.secure,
                mono: w.mono,
                width: w.width,
                on_input: w.on_input.map(&mut *f),
                on_submit: w.on_submit.map(&mut *f),
            }),
            Element::TextArea(w) => Element::TextArea(TextArea {
                id: w.id,
                value: w.value,
                placeholder: w.placeholder,
                label: w.label,
                hint: w.hint,
                error: w.error,
                height: w.height,
                mono: w.mono,
                on_input: w.on_input.map(&mut *f),
            }),
            Element::Search(w) => Element::Search(Search {
                value: w.value,
                placeholder: w.placeholder,
                on_input: w.on_input.map(&mut *f),
                on_submit: w.on_submit.map(&mut *f),
                on_clear: w.on_clear.map(&mut *f),
            }),
            Element::Select(w) => Element::Select(Select {
                options: w.options,
                selected: w.selected,
                placeholder: w.placeholder,
                label: w.label,
                width: w.width,
                on_select: w.on_select.map(&mut *f),
            }),
            Element::Checkbox(w) => Element::Checkbox(Checkbox {
                label: w.label,
                checked: w.checked,
                on_toggle: w.on_toggle.map(&mut *f),
            }),
            Element::Switch(w) => Element::Switch(Switch {
                label: w.label,
                on: w.on,
                on_toggle: w.on_toggle.map(&mut *f),
            }),
            Element::Filter(w) => Element::Filter(Filter {
                label: w.label,
                count: w.count,
                count_tone: w.count_tone,
                pressed: w.pressed,
                on_press: w.on_press.map(&mut *f),
            }),
            Element::Chip(w) => Element::Chip(w),
            Element::State(w) => Element::State(w),
            Element::Glyph(w) => Element::Glyph(w),
            Element::Progress(w) => Element::Progress(w),
            Element::Icon(w) => Element::Icon(w),
            Element::Image(w) => Element::Image(w),
            Element::Sparkline(w) => Element::Sparkline(w),
            Element::Bars(w) => Element::Bars(w),
            Element::Unsupported => Element::Unsupported,
        }
    }
}

/// Sets an optional field: `.spacing(8.0)`.
macro_rules! opt {
    ($($(#[$doc:meta])* $name:ident: $ty:ty),* $(,)?) => {
        $(
            $(#[$doc])*
            pub fn $name(mut self, $name: impl Into<$ty>) -> Self {
                self.$name = Some($name.into());
                self
            }
        )*
    };
}

/// Sets an optional number: `.spacing(8.0)`, typed as written so that a
/// literal needs no suffix.
macro_rules! num {
    ($($(#[$doc:meta])* $name:ident: $ty:ty),* $(,)?) => {
        $(
            $(#[$doc])*
            pub fn $name(mut self, $name: $ty) -> Self {
                self.$name = Some($name);
                self
            }
        )*
    };
}

/// Sets a field: `.align(Align::Center)`.
macro_rules! set {
    ($($(#[$doc:meta])* $name:ident: $ty:ty),* $(,)?) => {
        $(
            $(#[$doc])*
            pub fn $name(mut self, $name: impl Into<$ty>) -> Self {
                self.$name = $name.into();
                self
            }
        )*
    };
}

fn is_false(value: &bool) -> bool {
    !*value
}

// ---------------------------------------------------------------- Layout.

/// Children top to bottom.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct Column<H = HandlerId> {
    #[serde(default)]
    pub children: Vec<Element<H>>,
    /// Between two children, in logical pixels. Hangar UI's steps are 2, 4,
    /// 6, 8, 10, 12, 16, 20 and 24.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding: Option<Padding>,
    /// Where children sit across: left, centre, right.
    #[serde(default, skip_serializing_if = "is_default")]
    pub align: Align,
    /// `fill` by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<Length>,
    /// `shrink` by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<Length>,
}

impl<H> Column<H> {
    pub fn new(children: Vec<Element<H>>) -> Self {
        Column {
            children,
            spacing: None,
            padding: None,
            align: Align::Start,
            width: None,
            height: None,
        }
    }
    pub fn push(mut self, child: impl Into<Element<H>>) -> Self {
        self.children.push(child.into());
        self
    }
    opt!(padding: Padding, width: Length, height: Length);
    num!(spacing: f32);
    set!(align: Align);
}

/// Children left to right: Hangar UI's `.kit-actions` when it wraps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct Row<H = HandlerId> {
    #[serde(default)]
    pub children: Vec<Element<H>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding: Option<Padding>,
    /// Where children sit across: top, middle, bottom. `center` is the
    /// usual one for a line of controls.
    #[serde(default, skip_serializing_if = "is_default")]
    pub align: Align,
    /// Children that do not fit go to the next line, rather than being cut:
    /// French and English labels of different lengths both fit.
    #[serde(default, skip_serializing_if = "is_false")]
    pub wrap: bool,
    /// `fill` by default, so that a `fill` child pushes what follows it to
    /// the end, as in a `.kit-bar`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<Length>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<Length>,
}

impl<H> Row<H> {
    pub fn new(children: Vec<Element<H>>) -> Self {
        Row {
            children,
            spacing: None,
            padding: None,
            align: Align::Start,
            wrap: false,
            width: None,
            height: None,
        }
    }
    pub fn push(mut self, child: impl Into<Element<H>>) -> Self {
        self.children.push(child.into());
        self
    }
    opt!(padding: Padding, width: Length, height: Length);
    num!(spacing: f32);
    set!(align: Align);
    pub fn wrap(mut self) -> Self {
        self.wrap = true;
        self
    }
}

/// Children in a grid, filled row by row: a fixed number of columns, or as
/// many of at least `minWidth` as fit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct Grid<H = HandlerId> {
    #[serde(default)]
    pub children: Vec<Element<H>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub columns: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_width: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing: Option<f32>,
}

impl<H> Grid<H> {
    pub fn new(children: Vec<Element<H>>) -> Self {
        Grid {
            children,
            columns: None,
            min_width: None,
            spacing: None,
        }
    }
    num!(columns: u16, min_width: f32, spacing: f32);
}

/// The one part of a page that scrolls (Hangar UI's `.kit-grow`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct Scroll<H = HandlerId> {
    pub child: Box<Element<H>>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub axis: Axis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<Length>,
    /// `fill` by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<Length>,
    /// A view at the end stays at the end as content is added: a log.
    #[serde(default, skip_serializing_if = "is_false")]
    pub stick_to_end: bool,
}

impl<H> Scroll<H> {
    pub fn new(child: impl Into<Element<H>>) -> Self {
        Scroll {
            child: Box::new(child.into()),
            axis: Axis::Vertical,
            width: None,
            height: None,
            stick_to_end: false,
        }
    }
    opt!(width: Length, height: Length);
    set!(axis: Axis);
    pub fn stick_to_end(mut self) -> Self {
        self.stick_to_end = true;
        self
    }
}

/// Two parts side by side (`horizontal`) or one above the other
/// (`vertical`), the first taking `ratio` of the room.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct Split<H = HandlerId> {
    pub first: Box<Element<H>>,
    pub second: Box<Element<H>>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub axis: Axis,
    /// From 0.1 to 0.9; 0.5 by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ratio: Option<f32>,
}

impl<H> Split<H> {
    pub fn new(first: impl Into<Element<H>>, second: impl Into<Element<H>>) -> Self {
        Split {
            first: Box::new(first.into()),
            second: Box::new(second.into()),
            axis: Axis::Horizontal,
            ratio: None,
        }
    }
    num!(ratio: f32);
    set!(axis: Axis);
}

/// One child on a surface, with padding, a border, an alignment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct Container<H = HandlerId> {
    pub child: Box<Element<H>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding: Option<Padding>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub surface: Surface,
    /// A hairline round it, in the colour of a control's edge.
    #[serde(default, skip_serializing_if = "is_false")]
    pub border: bool,
    /// Corners, in logical pixels: 4, 5, 6, 8 or 10 in Hangar UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<Length>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<Length>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub align_x: Align,
    #[serde(default, skip_serializing_if = "is_default")]
    pub align_y: Align,
}

impl<H> Container<H> {
    pub fn new(child: impl Into<Element<H>>) -> Self {
        Container {
            child: Box::new(child.into()),
            padding: None,
            surface: Surface::None,
            border: false,
            radius: None,
            width: None,
            height: None,
            align_x: Align::Start,
            align_y: Align::Start,
        }
    }
    opt!(padding: Padding, width: Length, height: Length);
    num!(radius: f32);
    set!(surface: Surface, align_x: Align, align_y: Align);
    pub fn border(mut self) -> Self {
        self.border = true;
        self
    }
}

/// The padded column most pages are, scrolling as a whole (`.kit-page`); or,
/// with `fill`, a page that does not scroll as a whole and holds a part that
/// does (`.kit-fill`): bars and a [`Scroll`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct Page<H = HandlerId> {
    #[serde(default)]
    pub children: Vec<Element<H>>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub fill: bool,
}

impl<H> Page<H> {
    pub fn new(children: Vec<Element<H>>) -> Self {
        Page {
            children,
            fill: false,
        }
    }
    pub fn push(mut self, child: impl Into<Element<H>>) -> Self {
        self.children.push(child.into());
        self
    }
    pub fn fill(mut self) -> Self {
        self.fill = true;
        self
    }
}

/// A line of controls at the top of a page, or at its foot (`.kit-bar`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct Bar<H = HandlerId> {
    #[serde(default)]
    pub children: Vec<Element<H>>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub foot: bool,
}

impl<H> Bar<H> {
    pub fn new(children: Vec<Element<H>>) -> Self {
        Bar {
            children,
            foot: false,
        }
    }
    pub fn push(mut self, child: impl Into<Element<H>>) -> Self {
        self.children.push(child.into());
        self
    }
    pub fn foot(mut self) -> Self {
        self.foot = true;
        self
    }
}

/// Empty room: a fixed gap, or with `fill` what pushes the rest aside.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Space {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<Length>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<Length>,
}

impl Space {
    pub fn new() -> Self {
        Space::default()
    }
    opt!(width: Length, height: Length);
}

/// A hairline between parts, across a column (or, `vertical`, a row).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Divider {
    #[serde(default, skip_serializing_if = "is_false")]
    pub vertical: bool,
}

impl Divider {
    pub fn new() -> Self {
        Divider::default()
    }
}

/// A child, and a text shown when the pointer rests on it: the whole of a
/// truncated value, the name of an icon. It answers the pointer only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct Tooltip<H = HandlerId> {
    pub child: Box<Element<H>>,
    pub text: String,
}

impl<H> Tooltip<H> {
    pub fn new(child: impl Into<Element<H>>, text: impl Into<String>) -> Self {
        Tooltip {
            child: Box::new(child.into()),
            text: text.into(),
        }
    }
}

// ---------------------------------------------------------------- Text.

/// Plain text, in one style (Hangar UI's text classes).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Text {
    pub text: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub size: TextSize,
    #[serde(default, skip_serializing_if = "is_default")]
    pub tone: Tone,
    #[serde(default, skip_serializing_if = "is_default")]
    pub weight: Weight,
    /// In the terminals' face (`.kit-mono`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub mono: bool,
    /// A number: aligned to the end of its box (`.kit-num`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub numeric: bool,
    /// A command to type, on a well (`.kit-code`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub code: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub wrap: Wrap,
    #[serde(default, skip_serializing_if = "is_default")]
    pub align: Align,
}

impl Text {
    pub fn new(text: impl Into<String>) -> Self {
        Text {
            text: text.into(),
            size: TextSize::Body,
            tone: Tone::Default,
            weight: Weight::Normal,
            mono: false,
            numeric: false,
            code: false,
            wrap: Wrap::Word,
            align: Align::Start,
        }
    }
    set!(size: TextSize, tone: Tone, weight: Weight, wrap: Wrap, align: Align);
    pub fn title(self) -> Self {
        self.size(TextSize::Title).weight(Weight::Semibold)
    }
    pub fn small(self) -> Self {
        self.size(TextSize::Small)
    }
    pub fn dim(self) -> Self {
        self.tone(Tone::Dim)
    }
    pub fn muted(self) -> Self {
        self.tone(Tone::Muted)
    }
    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }
    pub fn numeric(mut self) -> Self {
        self.numeric = true;
        self
    }
    pub fn code(mut self) -> Self {
        self.code = true;
        self.mono = true;
        self
    }
    pub fn truncate(self) -> Self {
        self.wrap(Wrap::Truncate)
    }
}

/// Text in several styles on the same lines; a span with a handler is a link.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct Spans<H = HandlerId> {
    #[serde(default)]
    pub spans: Vec<Span<H>>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub size: TextSize,
    #[serde(default, skip_serializing_if = "is_default")]
    pub wrap: Wrap,
}

impl<H> Spans<H> {
    pub fn new(spans: Vec<Span<H>>) -> Self {
        Spans {
            spans,
            size: TextSize::Body,
            wrap: Wrap::Word,
        }
    }
    set!(size: TextSize, wrap: Wrap);
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct Span<H = HandlerId> {
    pub text: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub tone: Tone,
    #[serde(default, skip_serializing_if = "is_default")]
    pub weight: Weight,
    #[serde(default, skip_serializing_if = "is_false")]
    pub mono: bool,
    /// Pressed: no value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_press: Option<H>,
}

impl<H> Span<H> {
    pub fn new(text: impl Into<String>) -> Self {
        Span {
            text: text.into(),
            tone: Tone::Default,
            weight: Weight::Normal,
            mono: false,
            on_press: None,
        }
    }
    set!(tone: Tone, weight: Weight);
    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }
}

/// Preformatted text in the terminals' face, wrapped, scrolling inside
/// itself past `height` (`.kit-pre`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Pre {
    pub text: String,
    /// How tall it grows before it scrolls, in logical pixels; 240 by
    /// default (`--kit-log-height`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f32>,
}

impl Pre {
    pub fn new(text: impl Into<String>) -> Self {
        Pre {
            text: text.into(),
            height: None,
        }
    }
    num!(height: f32);
}

/// A log: lines with their kind and number, scrolling inside itself
/// (`.kit-log`, `hangar.ui.log`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Log {
    #[serde(default)]
    pub lines: Vec<LogLine>,
    /// How tall it grows before it scrolls; 240 by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f32>,
    /// Opens at its end, and stays there as lines are added.
    #[serde(default, skip_serializing_if = "is_false")]
    pub at_end: bool,
}

impl Log {
    pub fn new(lines: Vec<LogLine>) -> Self {
        Log {
            lines,
            height: None,
            at_end: false,
        }
    }
    num!(height: f32);
    pub fn at_end(mut self) -> Self {
        self.at_end = true;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct LogLine {
    pub text: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub kind: LineKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<u32>,
    /// The line being looked at: drawn as chosen.
    #[serde(default, skip_serializing_if = "is_false")]
    pub current: bool,
}

impl LogLine {
    pub fn new(text: impl Into<String>) -> Self {
        LogLine {
            text: text.into(),
            kind: LineKind::Normal,
            number: None,
            current: false,
        }
    }
    set!(kind: LineKind);
    num!(number: u32);
}

/// Names and values, in two columns (`.kit-kv`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct KeyValue<H = HandlerId> {
    #[serde(default)]
    pub pairs: Vec<Pair<H>>,
}

impl<H> KeyValue<H> {
    pub fn new(pairs: Vec<Pair<H>>) -> Self {
        KeyValue { pairs }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct Pair<H = HandlerId> {
    pub key: String,
    pub value: Element<H>,
}

impl<H> Pair<H> {
    pub fn new(key: impl Into<String>, value: impl Into<Element<H>>) -> Self {
        Pair {
            key: key.into(),
            value: value.into(),
        }
    }
}

// ---------------------------------------------------------------- The parts of a page.

/// A title, one line of state, actions at the end (`.kit-header`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct Header<H = HandlerId> {
    pub title: String,
    /// The line under the title: a [`StateLabel`] and a few words.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub state: Vec<Element<H>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<Element<H>>,
}

impl<H> Header<H> {
    pub fn new(title: impl Into<String>) -> Self {
        Header {
            title: title.into(),
            state: Vec::new(),
            actions: Vec::new(),
        }
    }
    /// Adds to the line under the title.
    pub fn state(mut self, part: impl Into<Element<H>>) -> Self {
        self.state.push(part.into());
        self
    }
    pub fn action(mut self, action: impl Into<Element<H>>) -> Self {
        self.actions.push(action.into());
        self
    }
}

/// A title in small capitals, a count, actions, and what it holds
/// (`.kit-section`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct Section<H = HandlerId> {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<Element<H>>,
    #[serde(default)]
    pub children: Vec<Element<H>>,
}

impl<H> Section<H> {
    pub fn new(title: impl Into<String>, children: Vec<Element<H>>) -> Self {
        Section {
            title: title.into(),
            count: None,
            actions: Vec::new(),
            children,
        }
    }
    pub fn action(mut self, action: impl Into<Element<H>>) -> Self {
        self.actions.push(action.into());
        self
    }
    opt!(count: String);
}

/// Rows of a leading glyph, a title, a second line and a trailing value
/// (`.kit-rows`); `group` draws them as a box split by hairlines, like
/// Settings' rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct List<H = HandlerId> {
    #[serde(default)]
    pub items: Vec<ListItem<H>>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub group: bool,
}

impl<H> List<H> {
    pub fn new(items: Vec<ListItem<H>>) -> Self {
        List {
            items,
            group: false,
        }
    }
    pub fn group(mut self) -> Self {
        self.group = true;
        self
    }
}

/// A row of a [`List`] (`.kit-row`). With `onPress` it takes the pointer and
/// the keyboard.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct ListItem<H = HandlerId> {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead: Option<Box<Element<H>>>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,
    /// A value at the end, right-aligned and tabular.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<Box<Element<H>>>,
    /// Controls after the value.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<Element<H>>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub selected: bool,
    /// Title and second line wrap, rather than truncate.
    #[serde(default, skip_serializing_if = "is_false")]
    pub wrap: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tooltip: Option<String>,
    /// Pressed: no value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_press: Option<H>,
}

impl<H> ListItem<H> {
    pub fn new(title: impl Into<String>) -> Self {
        ListItem {
            lead: None,
            title: title.into(),
            sub: None,
            end: None,
            actions: Vec::new(),
            selected: false,
            wrap: false,
            tooltip: None,
            on_press: None,
        }
    }
    opt!(
        #[allow(clippy::should_implement_trait)]
        sub: String,
        tooltip: String,
    );
    pub fn lead(mut self, lead: impl Into<Element<H>>) -> Self {
        self.lead = Some(Box::new(lead.into()));
        self
    }
    pub fn end(mut self, end: impl Into<Element<H>>) -> Self {
        self.end = Some(Box::new(end.into()));
        self
    }
    pub fn action(mut self, action: impl Into<Element<H>>) -> Self {
        self.actions.push(action.into());
        self
    }
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
    pub fn wrap(mut self) -> Self {
        self.wrap = true;
        self
    }
}

/// A table: a header of columns, rows of cells, sorting by a column.
///
/// Sorting is Hangar's to do when the table has no `onSort`: a click on a
/// `sortable` column's header orders the rows by their cells' keys, and
/// Hangar keeps that choice for the table's `id`. With `onSort` the kit
/// sorts: Hangar sends the [`Sort`](crate::Sort) asked for and draws the
/// rows in the order given, the arrow where `sort` says.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct Table<H = HandlerId> {
    /// What Hangar keeps its sort under, unique in the view.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub columns: Vec<TableColumn>,
    #[serde(default)]
    pub rows: Vec<TableRow<H>>,
    /// The order the rows are in: given by the kit, or the first one Hangar
    /// applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<crate::Sort>,
    /// The chosen row, by its index in `rows`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<Length>,
    /// A header pressed: a [`Sort`](crate::Sort).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_sort: Option<H>,
    /// A row pressed: its index in `rows`, whatever order it is drawn in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_row: Option<H>,
}

impl<H> Table<H> {
    pub fn new(columns: Vec<TableColumn>, rows: Vec<TableRow<H>>) -> Self {
        Table {
            id: String::new(),
            columns,
            rows,
            sort: None,
            selected: None,
            height: None,
            on_sort: None,
            on_row: None,
        }
    }
    set!(id: String);
    opt!(sort: crate::Sort, height: Length);
    num!(selected: u32);
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TableColumn {
    pub title: String,
    /// `end` for numbers.
    #[serde(default, skip_serializing_if = "is_default")]
    pub align: Align,
    /// `fill` by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<Length>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub sortable: bool,
}

impl TableColumn {
    pub fn new(title: impl Into<String>) -> Self {
        TableColumn {
            title: title.into(),
            align: Align::Start,
            width: None,
            sortable: false,
        }
    }
    set!(align: Align);
    opt!(width: Length);
    pub fn sortable(mut self) -> Self {
        self.sortable = true;
        self
    }
    pub fn numeric(self) -> Self {
        self.align(Align::End)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct TableRow<H = HandlerId> {
    pub cells: Vec<Cell<H>>,
}

impl<H> TableRow<H> {
    pub fn new(cells: Vec<Cell<H>>) -> Self {
        TableRow { cells }
    }
}

/// A cell: what it shows, and what it sorts by. Without a `key`, a cell
/// that is a text sorts by that text, and any other comes last.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct Cell<H = HandlerId> {
    pub content: Element<H>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<SortKey>,
}

impl<H> Cell<H> {
    pub fn new(content: impl Into<Element<H>>) -> Self {
        Cell {
            content: content.into(),
            key: None,
        }
    }
    opt!(key: SortKey);
}

impl<H> From<&str> for Cell<H> {
    fn from(text: &str) -> Self {
        Cell::new(text)
    }
}

impl<H> From<String> for Cell<H> {
    fn from(text: String) -> Self {
        Cell::new(text)
    }
}

/// What a cell sorts by: numbers before texts, texts without regard to case.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SortKey {
    Number(f64),
    Text(String),
}

impl From<f64> for SortKey {
    fn from(number: f64) -> Self {
        SortKey::Number(number)
    }
}

impl From<i64> for SortKey {
    fn from(number: i64) -> Self {
        SortKey::Number(number as f64)
    }
}

impl From<&str> for SortKey {
    fn from(text: &str) -> Self {
        SortKey::Text(text.to_owned())
    }
}

impl From<String> for SortKey {
    fn from(text: String) -> Self {
        SortKey::Text(text)
    }
}

/// Nodes that open and close: files, a scene, a schema. Which are open is
/// Hangar's to keep for the tree's `id`, starting from each node's `open`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct Tree<H = HandlerId> {
    pub id: String,
    #[serde(default)]
    pub nodes: Vec<TreeNode>,
    /// The chosen node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected: Option<String>,
    /// A node pressed: its id, as a text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_select: Option<H>,
}

impl<H> Tree<H> {
    pub fn new(id: impl Into<String>, nodes: Vec<TreeNode>) -> Self {
        Tree {
            id: id.into(),
            nodes,
            selected: None,
            on_select: None,
        }
    }
    opt!(selected: String);
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TreeNode {
    /// Unique in the tree.
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
    /// Open when the tree first shows it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub open: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<TreeNode>,
}

impl TreeNode {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        TreeNode {
            id: id.into(),
            label: label.into(),
            icon: None,
            open: false,
            children: Vec::new(),
        }
    }
    opt!(icon: Icon);
    pub fn open(mut self) -> Self {
        self.open = true;
        self
    }
    pub fn children(mut self, children: Vec<TreeNode>) -> Self {
        self.children = children;
        self
    }
}

/// A row of tabs on a recessed track (`.kit-tabs`). The kit draws the chosen
/// tab's panel itself, under it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct Tabs<H = HandlerId> {
    pub tabs: Vec<TabItem>,
    pub selected: String,
    /// A tab pressed: its id, as a text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_select: Option<H>,
}

impl<H> Tabs<H> {
    pub fn new(tabs: Vec<TabItem>, selected: impl Into<String>) -> Self {
        Tabs {
            tabs,
            selected: selected.into(),
            on_select: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TabItem {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<String>,
}

impl TabItem {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        TabItem {
            id: id.into(),
            label: label.into(),
            count: None,
        }
    }
    opt!(count: String);
}

/// What a part of a page is instead of its content: empty, loading, failed,
/// denied (`.kit-empty`). Never blank: it says what it is and what to do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct Empty<H = HandlerId> {
    #[serde(default, skip_serializing_if = "is_default")]
    pub kind: EmptyKind,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<Element<H>>,
}

impl<H> Empty<H> {
    pub fn new(kind: EmptyKind, title: impl Into<String>) -> Self {
        Empty {
            kind,
            title: title.into(),
            text: None,
            actions: Vec::new(),
        }
    }
    opt!(text: String);
    pub fn action(mut self, action: impl Into<Element<H>>) -> Self {
        self.actions.push(action.into());
        self
    }
}

/// A line in the flow of the page, for what holds until it changes
/// (`.kit-banner`). What only just happened is a toast (`ui.toast`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct Banner<H = HandlerId> {
    pub text: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub kind: Notice,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<Element<H>>,
}

impl<H> Banner<H> {
    pub fn new(kind: Notice, text: impl Into<String>) -> Self {
        Banner {
            text: text.into(),
            kind,
            actions: Vec::new(),
        }
    }
    pub fn action(mut self, action: impl Into<Element<H>>) -> Self {
        self.actions.push(action.into());
        self
    }
}

// ---------------------------------------------------------------- Controls.

/// A button (`.kit-btn`): a label, an icon, or both. Without `onPress` it
/// is drawn disabled.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct Button<H = HandlerId> {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub kind: ButtonKind,
    #[serde(default, skip_serializing_if = "is_false")]
    pub small: bool,
    /// An icon alone takes one: it is its name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tooltip: Option<String>,
    /// Pressed: no value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_press: Option<H>,
}

impl<H> Button<H> {
    pub fn new(label: impl Into<String>) -> Self {
        Button {
            label: Some(label.into()),
            icon: None,
            kind: ButtonKind::Default,
            small: false,
            tooltip: None,
            on_press: None,
        }
    }
    /// An icon alone, named by its tooltip.
    pub fn icon_only(icon: Icon, name: impl Into<String>) -> Self {
        Button {
            label: None,
            icon: Some(icon),
            kind: ButtonKind::Default,
            small: false,
            tooltip: Some(name.into()),
            on_press: None,
        }
    }
    set!(kind: ButtonKind);
    opt!(icon: Icon, tooltip: String);
    pub fn primary(self) -> Self {
        self.kind(ButtonKind::Primary)
    }
    pub fn danger(self) -> Self {
        self.kind(ButtonKind::Danger)
    }
    pub fn ghost(self) -> Self {
        self.kind(ButtonKind::Ghost)
    }
    pub fn small(mut self) -> Self {
        self.small = true;
        self
    }
}

/// Text in the accent's colour that acts like a button.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct Link<H = HandlerId> {
    pub label: String,
    /// Pressed: no value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_press: Option<H>,
}

impl<H> Link<H> {
    pub fn new(label: impl Into<String>) -> Self {
        Link {
            label: label.into(),
            on_press: None,
        }
    }
}

/// A button that opens a list of actions (`hangar.ui.menu`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct Menu<H = HandlerId> {
    /// Without one, the button is the `more` icon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub kind: ButtonKind,
    #[serde(default, skip_serializing_if = "is_false")]
    pub small: bool,
    pub items: Vec<MenuEntry>,
    /// An item chosen: its id, as a text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_select: Option<H>,
}

impl<H> Menu<H> {
    pub fn new(items: Vec<MenuEntry>) -> Self {
        Menu {
            label: None,
            icon: None,
            kind: ButtonKind::Ghost,
            small: true,
            items,
            on_select: None,
        }
    }
    opt!(label: String, icon: Icon);
    set!(kind: ButtonKind);
}

/// An item of a [`Menu`], or the line between two groups of them. In JSON,
/// `{ "item": { … } }` or `"separator"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MenuEntry {
    Item(MenuItem),
    Separator,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct MenuItem {
    pub id: String,
    pub label: String,
    /// A shortcut, shown at the end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub danger: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub disabled: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub checked: bool,
}

impl MenuItem {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        MenuItem {
            id: id.into(),
            label: label.into(),
            hint: None,
            danger: false,
            disabled: false,
            checked: false,
        }
    }
    opt!(hint: String);
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }
}

impl From<MenuItem> for MenuEntry {
    fn from(item: MenuItem) -> Self {
        MenuEntry::Item(item)
    }
}

/// A line of text to type, with its label, a hint and what was refused
/// (`.kit-field`, `.kit-input`). Controlled: it shows `value`, and the kit
/// gives the new one back in its next view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct TextInput<H = HandlerId> {
    #[serde(default)]
    pub value: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub placeholder: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    /// What was refused, in the interface's voice: draws the field as
    /// invalid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Dots in place of what is typed.
    #[serde(default, skip_serializing_if = "is_false")]
    pub secure: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub mono: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<Length>,
    /// Each change: the whole text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_input: Option<H>,
    /// Enter: no value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_submit: Option<H>,
}

impl<H> TextInput<H> {
    pub fn new(placeholder: impl Into<String>, value: impl Into<String>) -> Self {
        TextInput {
            value: value.into(),
            placeholder: placeholder.into(),
            label: None,
            hint: None,
            error: None,
            secure: false,
            mono: false,
            width: None,
            on_input: None,
            on_submit: None,
        }
    }
    opt!(label: String, hint: String, error: String, width: Length);
    pub fn secure(mut self) -> Self {
        self.secure = true;
        self
    }
    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }
}

/// Several lines of text to type (`textarea.kit-input`). Hangar keeps the
/// cursor and the selection under `id`, and starts again from `value` when
/// the kit gives another text than the one typed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct TextArea<H = HandlerId> {
    /// Unique in the view.
    pub id: String,
    #[serde(default)]
    pub value: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub placeholder: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// In logical pixels; 120 by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f32>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub mono: bool,
    /// Each change: the whole text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_input: Option<H>,
}

impl<H> TextArea<H> {
    pub fn new(id: impl Into<String>, value: impl Into<String>) -> Self {
        TextArea {
            id: id.into(),
            value: value.into(),
            placeholder: String::new(),
            label: None,
            hint: None,
            error: None,
            height: None,
            mono: false,
            on_input: None,
        }
    }
    set!(placeholder: String);
    opt!(label: String, hint: String, error: String);
    num!(height: f32);
    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }
}

/// A search field, with its icon and a button that clears it
/// (`.kit-search`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct Search<H = HandlerId> {
    #[serde(default)]
    pub value: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub placeholder: String,
    /// Each change: the whole text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_input: Option<H>,
    /// Enter: no value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_submit: Option<H>,
    /// The clear button, shown while there is text: no value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_clear: Option<H>,
}

impl<H> Search<H> {
    pub fn new(placeholder: impl Into<String>, value: impl Into<String>) -> Self {
        Search {
            value: value.into(),
            placeholder: placeholder.into(),
            on_input: None,
            on_submit: None,
            on_clear: None,
        }
    }
}

/// One value among a few (`.kit-select`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct Select<H = HandlerId> {
    pub options: Vec<SelectOption>,
    /// The chosen option's value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub placeholder: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<Length>,
    /// An option chosen: its value, as a text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_select: Option<H>,
}

impl<H> Select<H> {
    pub fn new(options: Vec<SelectOption>, selected: Option<String>) -> Self {
        Select {
            options,
            selected,
            placeholder: String::new(),
            label: None,
            width: None,
            on_select: None,
        }
    }
    set!(placeholder: String);
    opt!(label: String, width: Length);
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct SelectOption {
    pub value: String,
    pub label: String,
}

impl SelectOption {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        SelectOption {
            value: value.into(),
            label: label.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct Checkbox<H = HandlerId> {
    pub label: String,
    #[serde(default)]
    pub checked: bool,
    /// Toggled: the new state, a boolean.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_toggle: Option<H>,
}

impl<H> Checkbox<H> {
    pub fn new(label: impl Into<String>, checked: bool) -> Self {
        Checkbox {
            label: label.into(),
            checked,
            on_toggle: None,
        }
    }
}

/// On or off, for a setting that applies at once (`.kit-switch`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct Switch<H = HandlerId> {
    pub label: String,
    #[serde(default)]
    pub on: bool,
    /// Toggled: the new state, a boolean.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_toggle: Option<H>,
}

impl<H> Switch<H> {
    pub fn new(label: impl Into<String>, on: bool) -> Self {
        Switch {
            label: label.into(),
            on,
            on_toggle: None,
        }
    }
}

/// A chip that is a toggle, with a count (`.kit-filter`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
#[serde(bound(serialize = "H: Serialize", deserialize = "H: Deserialize<'de>"))]
pub struct Filter<H = HandlerId> {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<String>,
    /// `danger` for a count of errors.
    #[serde(default, skip_serializing_if = "is_default")]
    pub count_tone: Tone,
    #[serde(default)]
    pub pressed: bool,
    /// Pressed: no value. The kit flips `pressed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_press: Option<H>,
}

impl<H> Filter<H> {
    pub fn new(label: impl Into<String>, pressed: bool) -> Self {
        Filter {
            label: label.into(),
            count: None,
            count_tone: Tone::Default,
            pressed,
            on_press: None,
        }
    }
    opt!(count: String);
    set!(count_tone: Tone);
}

// ---------------------------------------------------------------- States.

/// A short word something is filed under (`.kit-chip`), or with a tone a
/// badge: a count, a version, a state in a word.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Chip {
    pub label: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub tone: Tone,
}

impl Chip {
    pub fn new(label: impl Into<String>) -> Self {
        Chip {
            label: label.into(),
            tone: Tone::Default,
        }
    }
    set!(tone: Tone);
}

/// A state as a glyph and a word: how a page says a state (`.kit-state`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct StateLabel {
    pub status: Status,
    pub label: String,
}

impl StateLabel {
    pub fn new(status: Status, label: impl Into<String>) -> Self {
        StateLabel {
            status,
            label: label.into(),
        }
    }
}

/// A state's glyph alone, as at the head of a row (`.kit-glyph`), or its
/// smallest form (`.kit-dot`). `label` is its name, shown on hover: a shape
/// says nothing to someone who cannot see it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Glyph {
    pub status: Status,
    pub label: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub dot: bool,
}

impl Glyph {
    pub fn new(status: Status, label: impl Into<String>) -> Self {
        Glyph {
            status,
            label: label.into(),
            dot: false,
        }
    }
    pub fn dot(mut self) -> Self {
        self.dot = true;
        self
    }
}

/// A bar of how far something got (`.kit-progress`): `value` from 0 to 1, or
/// none when there is nothing to measure against.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Progress {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f32>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub kind: ProgressKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl Progress {
    pub fn new(value: Option<f32>) -> Self {
        Progress {
            value,
            kind: ProgressKind::Normal,
            label: None,
        }
    }
    set!(kind: ProgressKind);
    opt!(label: String);
}

/// One of Hangar's icons (`.kit-icon`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Symbol {
    pub icon: Icon,
    #[serde(default, skip_serializing_if = "is_default")]
    pub tone: Tone,
    /// In logical pixels; 14 by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
}

impl Symbol {
    pub fn new(icon: Icon) -> Self {
        Symbol {
            icon,
            tone: Tone::Default,
            size: None,
        }
    }
    set!(tone: Tone);
    num!(size: f32);
}

// ---------------------------------------------------------------- Media and charts.

/// A picture from the kit's own folder, or from its data folder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Image {
    pub source: ImageSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<Length>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<Length>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub fit: Fit,
    /// What it shows, in words: shown on hover, and in its place while it
    /// cannot be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl Image {
    pub fn new(source: ImageSource) -> Self {
        Image {
            source,
            width: None,
            height: None,
            fit: Fit::Contain,
            description: None,
        }
    }
    opt!(width: Length, height: Length, description: String);
    set!(fit: Fit);
}

/// Where an image is read from: a path relative to the kit's root, which
/// needs no permission, or to its data folder, which needs `data.read`. In
/// JSON, `{ "kit": "assets/logo.png" }` or `{ "data": "charts/today.png" }`.
/// PNG, JPEG, GIF, WebP and SVG; 10 MB at most.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImageSource {
    Kit(String),
    Data(String),
}

/// A line of values, small, in a row or a header: a trend at a glance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Sparkline {
    pub values: Vec<f32>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub tone: Tone,
    /// In logical pixels; 24 by default. It takes the width it is given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f32>,
    /// The bottom and top of the scale; the values' own by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f32>,
    /// The area under the line tinted.
    #[serde(default, skip_serializing_if = "is_false")]
    pub fill: bool,
    /// What it shows, in words, shown on hover.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl Sparkline {
    pub fn new(values: Vec<f32>) -> Self {
        Sparkline {
            values,
            tone: Tone::Accent,
            height: None,
            min: None,
            max: None,
            fill: false,
            label: None,
        }
    }
    set!(tone: Tone);
    opt!(label: String);
    num!(height: f32, min: f32, max: f32);
    pub fn fill(mut self) -> Self {
        self.fill = true;
        self
    }
}

/// Bars, one per labelled value: across, label and value at each end
/// (`horizontal`), or standing in a row (`vertical`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Bars {
    pub bars: Vec<BarItem>,
    /// The top of the scale; the largest value by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f32>,
    /// `horizontal` by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis: Option<Axis>,
    /// Of the whole chart, for `vertical`; 80 by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f32>,
}

impl Bars {
    pub fn new(bars: Vec<BarItem>) -> Self {
        Bars {
            bars,
            max: None,
            axis: None,
            height: None,
        }
    }
    opt!(axis: Axis);
    num!(max: f32, height: f32);
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct BarItem {
    pub label: String,
    pub value: f32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub tone: Tone,
    /// The value as the page writes it (`1.2 s`); the number by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl BarItem {
    pub fn new(label: impl Into<String>, value: f32) -> Self {
        BarItem {
            label: label.into(),
            value,
            tone: Tone::Accent,
            text: None,
        }
    }
    set!(tone: Tone);
    opt!(text: String);
}
