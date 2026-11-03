//! Building a view: a function per widget of hangar-kit-ui, and the
//! `on_*` methods that give a widget its handler, as iced writes them.
//!
//! ```ignore
//! col![
//!     header("Panes").state(state_label(Status::Success, "Ready")),
//!     text_input("owner/name", &self.repo).on_input(Message::RepoChanged),
//!     button("Refresh").primary().on_press(Message::Refresh),
//! ]
//! .spacing(12.0)
//! ```
//!
//! The functions answer hangar-kit-ui's own structs, so every setter of
//! theirs works here too (`.primary()`, `.spacing(8.0)`); this module only
//! adds what needs the kit's message type.

use std::rc::Rc;

use hangar_kit_ui as ui;
use hangar_kit_ui::{EmptyKind, Icon, ImageSource, Notice, Sort, Status};

use crate::handler::Handler;

/// A widget of a kit whose messages are `M`.
pub type Element<M> = ui::Element<Handler<M>>;

/// Children top to bottom. [`col!`](crate::col) takes widgets of
/// different kinds.
pub fn column<M>(children: impl IntoIterator<Item = Element<M>>) -> ui::Column<Handler<M>> {
    ui::Column::new(children.into_iter().collect())
}

/// Children left to right. [`row!`](crate::row) takes widgets of different
/// kinds.
pub fn row<M>(children: impl IntoIterator<Item = Element<M>>) -> ui::Row<Handler<M>> {
    ui::Row::new(children.into_iter().collect())
}

pub fn grid<M>(children: impl IntoIterator<Item = Element<M>>) -> ui::Grid<Handler<M>> {
    ui::Grid::new(children.into_iter().collect())
}

pub fn scroll<M>(child: impl Into<Element<M>>) -> ui::Scroll<Handler<M>> {
    ui::Scroll::new(child)
}

pub fn split<M>(
    first: impl Into<Element<M>>,
    second: impl Into<Element<M>>,
) -> ui::Split<Handler<M>> {
    ui::Split::new(first, second)
}

pub fn container<M>(child: impl Into<Element<M>>) -> ui::Container<Handler<M>> {
    ui::Container::new(child)
}

/// The padded column most pages are, scrolling as a whole.
pub fn page<M>(children: impl IntoIterator<Item = Element<M>>) -> ui::Page<Handler<M>> {
    ui::Page::new(children.into_iter().collect())
}

pub fn bar<M>(children: impl IntoIterator<Item = Element<M>>) -> ui::Bar<Handler<M>> {
    ui::Bar::new(children.into_iter().collect())
}

pub fn space() -> ui::Space {
    ui::Space::new()
}

/// Room that takes what is left: pushes what follows it to the end.
pub fn fill() -> ui::Space {
    ui::Space::new().width(ui::Length::Fill)
}

pub fn divider() -> ui::Divider {
    ui::Divider::new()
}

pub fn tooltip<M>(
    child: impl Into<Element<M>>,
    text: impl Into<String>,
) -> ui::Tooltip<Handler<M>> {
    ui::Tooltip::new(child, text)
}

pub fn text(text: impl Into<String>) -> ui::Text {
    ui::Text::new(text)
}

pub fn spans<M>(spans: impl IntoIterator<Item = ui::Span<Handler<M>>>) -> ui::Spans<Handler<M>> {
    ui::Spans::new(spans.into_iter().collect())
}

pub fn span<M>(text: impl Into<String>) -> ui::Span<Handler<M>> {
    ui::Span::new(text)
}

pub fn pre(text: impl Into<String>) -> ui::Pre {
    ui::Pre::new(text)
}

pub fn log_lines(lines: impl IntoIterator<Item = ui::LogLine>) -> ui::Log {
    ui::Log::new(lines.into_iter().collect())
}

pub fn log_line(text: impl Into<String>) -> ui::LogLine {
    ui::LogLine::new(text)
}

/// Names and values in two columns.
pub fn key_value<M, K: Into<String>, V: Into<Element<M>>>(
    pairs: impl IntoIterator<Item = (K, V)>,
) -> ui::KeyValue<Handler<M>> {
    ui::KeyValue::new(
        pairs
            .into_iter()
            .map(|(key, value)| ui::Pair::new(key, value))
            .collect(),
    )
}

pub fn header<M>(title: impl Into<String>) -> ui::Header<Handler<M>> {
    ui::Header::new(title)
}

pub fn section<M>(
    title: impl Into<String>,
    children: impl IntoIterator<Item = Element<M>>,
) -> ui::Section<Handler<M>> {
    ui::Section::new(title, children.into_iter().collect())
}

pub fn list<M>(items: impl IntoIterator<Item = ui::ListItem<Handler<M>>>) -> ui::List<Handler<M>> {
    ui::List::new(items.into_iter().collect())
}

pub fn item<M>(title: impl Into<String>) -> ui::ListItem<Handler<M>> {
    ui::ListItem::new(title)
}

pub fn table<M>(
    columns: impl IntoIterator<Item = ui::TableColumn>,
    rows: impl IntoIterator<Item = ui::TableRow<Handler<M>>>,
) -> ui::Table<Handler<M>> {
    ui::Table::new(columns.into_iter().collect(), rows.into_iter().collect())
}

pub fn table_column(title: impl Into<String>) -> ui::TableColumn {
    ui::TableColumn::new(title)
}

pub fn table_row<M>(
    cells: impl IntoIterator<Item = ui::Cell<Handler<M>>>,
) -> ui::TableRow<Handler<M>> {
    ui::TableRow::new(cells.into_iter().collect())
}

pub fn cell<M>(content: impl Into<Element<M>>) -> ui::Cell<Handler<M>> {
    ui::Cell::new(content)
}

pub fn tree<M>(
    id: impl Into<String>,
    nodes: impl IntoIterator<Item = ui::TreeNode>,
) -> ui::Tree<Handler<M>> {
    ui::Tree::new(id, nodes.into_iter().collect())
}

pub fn tree_node(id: impl Into<String>, label: impl Into<String>) -> ui::TreeNode {
    ui::TreeNode::new(id, label)
}

pub fn tabs<M>(
    tabs: impl IntoIterator<Item = ui::TabItem>,
    selected: impl Into<String>,
) -> ui::Tabs<Handler<M>> {
    ui::Tabs::new(tabs.into_iter().collect(), selected)
}

pub fn tab_item(id: impl Into<String>, label: impl Into<String>) -> ui::TabItem {
    ui::TabItem::new(id, label)
}

/// Nothing yet: what will fill it.
pub fn empty<M>(title: impl Into<String>) -> ui::Empty<Handler<M>> {
    ui::Empty::new(EmptyKind::Empty, title)
}

/// Being read: what is being done.
pub fn loading<M>(title: impl Into<String>) -> ui::Empty<Handler<M>> {
    ui::Empty::new(EmptyKind::Loading, title)
}

/// It went wrong: what, and what to do.
pub fn failed<M>(title: impl Into<String>) -> ui::Empty<Handler<M>> {
    ui::Empty::new(EmptyKind::Error, title)
}

/// A permission is missing: which, and where it is given.
pub fn denied<M>(title: impl Into<String>) -> ui::Empty<Handler<M>> {
    ui::Empty::new(EmptyKind::Denied, title)
}

pub fn banner<M>(kind: Notice, text: impl Into<String>) -> ui::Banner<Handler<M>> {
    ui::Banner::new(kind, text)
}

pub fn button<M>(label: impl Into<String>) -> ui::Button<Handler<M>> {
    ui::Button::new(label)
}

/// An icon alone, named by its tooltip.
pub fn icon_button<M>(icon: Icon, name: impl Into<String>) -> ui::Button<Handler<M>> {
    ui::Button::icon_only(icon, name)
}

pub fn link<M>(label: impl Into<String>) -> ui::Link<Handler<M>> {
    ui::Link::new(label)
}

pub fn menu<M>(items: impl IntoIterator<Item = ui::MenuEntry>) -> ui::Menu<Handler<M>> {
    ui::Menu::new(items.into_iter().collect())
}

pub fn menu_item(id: impl Into<String>, label: impl Into<String>) -> ui::MenuItem {
    ui::MenuItem::new(id, label)
}

pub fn separator() -> ui::MenuEntry {
    ui::MenuEntry::Separator
}

pub fn text_input<M>(
    placeholder: impl Into<String>,
    value: impl Into<String>,
) -> ui::TextInput<Handler<M>> {
    ui::TextInput::new(placeholder, value)
}

pub fn text_area<M>(id: impl Into<String>, value: impl Into<String>) -> ui::TextArea<Handler<M>> {
    ui::TextArea::new(id, value)
}

pub fn search<M>(
    placeholder: impl Into<String>,
    value: impl Into<String>,
) -> ui::Search<Handler<M>> {
    ui::Search::new(placeholder, value)
}

pub fn select<M>(
    options: impl IntoIterator<Item = ui::SelectOption>,
    selected: Option<impl Into<String>>,
) -> ui::Select<Handler<M>> {
    ui::Select::new(options.into_iter().collect(), selected.map(Into::into))
}

pub fn select_option(value: impl Into<String>, label: impl Into<String>) -> ui::SelectOption {
    ui::SelectOption::new(value, label)
}

pub fn checkbox<M>(label: impl Into<String>, checked: bool) -> ui::Checkbox<Handler<M>> {
    ui::Checkbox::new(label, checked)
}

pub fn switch<M>(label: impl Into<String>, on: bool) -> ui::Switch<Handler<M>> {
    ui::Switch::new(label, on)
}

pub fn filter<M>(label: impl Into<String>, pressed: bool) -> ui::Filter<Handler<M>> {
    ui::Filter::new(label, pressed)
}

pub fn chip(label: impl Into<String>) -> ui::Chip {
    ui::Chip::new(label)
}

/// A state as a glyph and a word: how a page says a state.
pub fn state_label(status: Status, label: impl Into<String>) -> ui::StateLabel {
    ui::StateLabel::new(status, label)
}

pub fn glyph(status: Status, label: impl Into<String>) -> ui::Glyph {
    ui::Glyph::new(status, label)
}

/// From 0 to 1; none when there is nothing to measure against.
pub fn progress(value: Option<f32>) -> ui::Progress {
    ui::Progress::new(value)
}

pub fn icon(icon: Icon) -> ui::Symbol {
    ui::Symbol::new(icon)
}

/// An image of the kit's own folder: needs no permission.
pub fn kit_image(path: impl Into<String>) -> ui::Image {
    ui::Image::new(ImageSource::Kit(path.into()))
}

/// An image of the kit's data folder: needs `data.read`.
pub fn data_image(path: impl Into<String>) -> ui::Image {
    ui::Image::new(ImageSource::Data(path.into()))
}

pub fn sparkline(values: impl IntoIterator<Item = f32>) -> ui::Sparkline {
    ui::Sparkline::new(values.into_iter().collect())
}

pub fn bars(bars: impl IntoIterator<Item = ui::BarItem>) -> ui::Bars {
    ui::Bars::new(bars.into_iter().collect())
}

pub fn bar_item(label: impl Into<String>, value: f32) -> ui::BarItem {
    ui::BarItem::new(label, value)
}

// ---------------------------------------------------------------- Handlers.

/// Widgets that are pressed: buttons, links, rows, filters, the clear
/// button of a search.
pub trait OnPress<M> {
    /// Pressed, it sends `message`.
    fn on_press(self, message: M) -> Self;
    /// Pressed, it sends `message`; without one it is drawn disabled.
    fn on_press_maybe(self, message: Option<M>) -> Self;
}

macro_rules! on_press {
    ($($ty:ident),*) => {
        $(
            impl<M> OnPress<M> for ui::$ty<Handler<M>> {
                fn on_press(mut self, message: M) -> Self {
                    self.on_press = Some(Handler::Message(message));
                    self
                }
                fn on_press_maybe(mut self, message: Option<M>) -> Self {
                    self.on_press = message.map(Handler::Message);
                    self
                }
            }
        )*
    };
}

on_press!(Button, Link, ListItem, Filter, Span);

/// Widgets that are typed into.
pub trait OnInput<M> {
    /// Each change, `f` of the whole text.
    fn on_input(self, f: impl Fn(String) -> M + 'static) -> Self;
}

macro_rules! on_input {
    ($($ty:ident),*) => {
        $(
            impl<M> OnInput<M> for ui::$ty<Handler<M>> {
                fn on_input(mut self, f: impl Fn(String) -> M + 'static) -> Self {
                    self.on_input = Some(Handler::Text(Rc::new(f)));
                    self
                }
            }
        )*
    };
}

on_input!(TextInput, TextArea, Search);

/// Fields Enter submits.
pub trait OnSubmit<M> {
    fn on_submit(self, message: M) -> Self;
}

impl<M> OnSubmit<M> for ui::TextInput<Handler<M>> {
    fn on_submit(mut self, message: M) -> Self {
        self.on_submit = Some(Handler::Message(message));
        self
    }
}

impl<M> OnSubmit<M> for ui::Search<Handler<M>> {
    fn on_submit(mut self, message: M) -> Self {
        self.on_submit = Some(Handler::Message(message));
        self
    }
}

/// A search's clear button: shown while there is text.
pub trait OnClear<M> {
    fn on_clear(self, message: M) -> Self;
}

impl<M> OnClear<M> for ui::Search<Handler<M>> {
    fn on_clear(mut self, message: M) -> Self {
        self.on_clear = Some(Handler::Message(message));
        self
    }
}

/// Widgets that are on or off.
pub trait OnToggle<M> {
    /// Toggled, `f` of the new state.
    fn on_toggle(self, f: impl Fn(bool) -> M + 'static) -> Self;
}

impl<M> OnToggle<M> for ui::Checkbox<Handler<M>> {
    fn on_toggle(mut self, f: impl Fn(bool) -> M + 'static) -> Self {
        self.on_toggle = Some(Handler::Bool(Rc::new(f)));
        self
    }
}

impl<M> OnToggle<M> for ui::Switch<Handler<M>> {
    fn on_toggle(mut self, f: impl Fn(bool) -> M + 'static) -> Self {
        self.on_toggle = Some(Handler::Bool(Rc::new(f)));
        self
    }
}

/// Widgets that choose one of theirs: an option's value, a tab's, a menu
/// item's or a tree node's id.
pub trait OnSelect<M> {
    fn on_select(self, f: impl Fn(String) -> M + 'static) -> Self;
}

macro_rules! on_select {
    ($($ty:ident),*) => {
        $(
            impl<M> OnSelect<M> for ui::$ty<Handler<M>> {
                fn on_select(mut self, f: impl Fn(String) -> M + 'static) -> Self {
                    self.on_select = Some(Handler::Text(Rc::new(f)));
                    self
                }
            }
        )*
    };
}

on_select!(Select, Tabs, Menu, Tree);

/// A table's handlers.
pub trait TableEvents<M> {
    /// The kit sorts: Hangar sends the order asked for, and draws the rows as
    /// given. Without it, Hangar sorts by the cells' keys by itself.
    fn on_sort(self, f: impl Fn(Sort) -> M + 'static) -> Self;
    /// A row pressed: its index in the rows given, whatever order they are
    /// drawn in.
    fn on_row(self, f: impl Fn(usize) -> M + 'static) -> Self;
}

impl<M> TableEvents<M> for ui::Table<Handler<M>> {
    fn on_sort(mut self, f: impl Fn(Sort) -> M + 'static) -> Self {
        self.on_sort = Some(Handler::Sort(Rc::new(f)));
        self
    }

    fn on_row(mut self, f: impl Fn(usize) -> M + 'static) -> Self {
        self.on_row = Some(Handler::Index(Rc::new(f)));
        self
    }
}

/// What every widget can do once it is an [`Element`].
pub trait ElementExt<M> {
    /// The same widget, its messages passed through `f`: a part of a page
    /// written with its own messages, set in a larger one.
    fn map<N: 'static>(self, f: impl Fn(M) -> N + 'static) -> Element<N>;
}

impl<M: 'static> ElementExt<M> for Element<M> {
    fn map<N: 'static>(self, f: impl Fn(M) -> N + 'static) -> Element<N> {
        let f: Rc<dyn Fn(M) -> N> = Rc::new(f);
        self.map_handlers(&mut |handler: Handler<M>| handler.map(f.clone()))
    }
}

/// Children top to bottom, of any kinds: `col![header("x"), button("y")]`. Not
/// `column!`, which is std's own and would be ambiguous in a glob import.
#[macro_export]
macro_rules! col {
    () => {
        $crate::widgets::column(::std::vec::Vec::new())
    };
    ($($child:expr),+ $(,)?) => {
        $crate::widgets::column(::std::vec![$(::core::convert::Into::into($child)),+])
    };
}

/// Children left to right, of any kinds: `row![text("x"), fill(), button("y")]`.
#[macro_export]
macro_rules! row {
    () => {
        $crate::widgets::row(::std::vec::Vec::new())
    };
    ($($child:expr),+ $(,)?) => {
        $crate::widgets::row(::std::vec![$(::core::convert::Into::into($child)),+])
    };
}

/// A page of any kinds of children: `page![header("x"), section(…)]`.
#[macro_export]
macro_rules! page {
    () => {
        $crate::widgets::page(::std::vec::Vec::new())
    };
    ($($child:expr),+ $(,)?) => {
        $crate::widgets::page(::std::vec![$(::core::convert::Into::into($child)),+])
    };
}
