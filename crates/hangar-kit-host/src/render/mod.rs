//! A kit's view, drawn with iced: every widget of hangar-kit-ui as Hangar UI
//! draws it in an HTML tab (docs/kit-ui.md), in the tokens of
//! [`KitStyle`]. Buttons, icons, tooltips and menus are hangar-ui's, in the
//! theme in use, so a kit's controls are the window's own. A widget's
//! handler becomes a [`KitMessage::Ui`] naming it and the view it was drawn
//! from; what Hangar keeps itself (a table's order, a tree's open nodes, a
//! text area's cursor, which menu is open) becomes a [`KitMessage::Local`].

mod chart;
mod icons;
mod style;

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fmt;

use iced::alignment::{Horizontal, Vertical};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::text::Wrapping;
use iced::widget::{
    Column, Row, button, canvas, column, container, grid, image, progress_bar, rich_text, row,
    rule, scrollable, space, span, svg, text, text_editor,
};

use hangar_i18n::Key;
use iced::{
    Alignment, Background, Border, Color, ContentFit, Element, Length, Padding, Shadow, Theme,
};

use hangar_kit_ui as ui;
use hangar_kit_ui::api::{self, KitError};
use hangar_kit_ui::{
    ButtonKind, EmptyKind, EventValue, HandlerId, ImageSource, LineKind, Notice, ProgressKind,
    Sort, SortKey, Status, TextSize, Tone, UiEvent, Weight, Wrap,
};

use crate::api::KitApi;
use crate::message::{KitMessage, Local};
use crate::tab::{Failure, FailureKind, KitTab, LogEntry};

pub use style::{KitStyle, Palette};

type El<'a> = Element<'a, KitMessage>;

/// A text area's text, as Hangar holds it between views.
pub(crate) struct Editor {
    pub content: text_editor::Content,
    /// The value the kit gave last: a new one, other than what was typed,
    /// replaces the text.
    pub given: String,
}

pub(crate) enum Picture {
    Raster(image::Handle),
    Vector(svg::Handle),
    Missing(Missing),
}

/// Why an image is not drawn.
pub(crate) enum Missing {
    /// From the kit's data folder, for a tab without `data.read`.
    NeedsData,
    TooLarge,
    /// What the app said.
    Refused(String),
}

impl Missing {
    fn said(&self, style: KitStyle) -> String {
        match self {
            Missing::NeedsData => style.t(Key::KitsTabNativeImageNeedsData).into_owned(),
            Missing::TooLarge => style.t(Key::KitsTabNativeImageTooLarge).into_owned(),
            Missing::Refused(why) => why.clone(),
        }
    }
}

/// What Hangar keeps of a tab's widgets between views.
#[derive(Default)]
pub(crate) struct WidgetState {
    pub sorts: HashMap<String, Sort>,
    pub toggled: HashMap<(String, String), bool>,
    pub editors: HashMap<String, Editor>,
    pub images: HashMap<ImageSource, Picture>,
    /// The menu open, by what it holds ([`menu_key`]): a menu has no id, and
    /// what it lists is what tells one from another across views.
    pub menu: Option<u64>,
}

/// More images than this in a tab's cache, and it starts again.
const MAX_IMAGES: usize = 256;

impl WidgetState {
    /// Brings what Hangar keeps in line with a new view: text areas made or
    /// given a new text, those gone dropped, images read once.
    pub(crate) fn sync(&mut self, view: &ui::abi::View, kit: &dyn KitApi, granted: &[String]) {
        let mut areas = HashSet::new();
        view.root.for_each(&mut |element| match element {
            ui::Element::TextArea(area) => {
                areas.insert(area.id.clone());
                match self.editors.get_mut(&area.id) {
                    None => {
                        let content = text_editor::Content::with_text(&area.value);
                        self.editors.insert(
                            area.id.clone(),
                            Editor {
                                content,
                                given: area.value.clone(),
                            },
                        );
                    }
                    Some(editor) if editor.given != area.value => {
                        if editor.content.text() != area.value {
                            editor.content = text_editor::Content::with_text(&area.value);
                        }
                        editor.given = area.value.clone();
                    }
                    Some(_) => {}
                }
            }
            ui::Element::Image(picture) if !self.images.contains_key(&picture.source) => {
                if self.images.len() >= MAX_IMAGES {
                    self.images.clear();
                }
                let loaded = load_image(&picture.source, kit, granted);
                self.images.insert(picture.source.clone(), loaded);
            }
            _ => {}
        });
        self.editors.retain(|id, _| areas.contains(id));
    }
}

fn load_image(source: &ImageSource, kit: &dyn KitApi, granted: &[String]) -> Picture {
    let path = match source {
        ImageSource::Kit(path) => path,
        ImageSource::Data(path) => {
            if !granted
                .iter()
                .any(|held| held == api::permission::DATA_READ)
            {
                return Picture::Missing(Missing::NeedsData);
            }
            path
        }
    };
    match kit.image(source) {
        Ok(bytes) if bytes.len() > api::limits::BINARY_MAX_BYTES => {
            Picture::Missing(Missing::TooLarge)
        }
        Ok(bytes) if path.to_ascii_lowercase().ends_with(".svg") => {
            Picture::Vector(svg::Handle::from_memory(bytes))
        }
        Ok(bytes) => Picture::Raster(image::Handle::from_bytes(bytes)),
        Err(KitError { message, .. }) => Picture::Missing(Missing::Refused(message)),
    }
}

impl KitTab {
    /// The tab as iced draws it: its page, or why it stopped, or that it is
    /// starting.
    pub fn draw(&self, style: KitStyle) -> El<'_> {
        let body: El<'_> = match (self.failure(), self.view()) {
            (Some(failure), _) => failure_page(style, failure, self.logs().collect()),
            (None, Some(view)) => {
                let cx = Cx {
                    style,
                    seq: view.seq,
                    widgets: &self.widgets,
                };
                element(&cx, &view.root)
            }
            (None, None) => center(state_label(
                style,
                Status::Running,
                style.t(Key::KitsTabNativeStarting),
            )),
        };
        let p = style.palette;
        container(body)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_| container::Style {
                text_color: Some(p.text),
                background: Some(Background::Color(p.bg)),
                ..Default::default()
            })
            .into()
    }
}

struct Cx<'a> {
    style: KitStyle,
    seq: u32,
    widgets: &'a WidgetState,
}

impl Cx<'_> {
    fn message(&self, handler: HandlerId, value: Option<EventValue>) -> KitMessage {
        KitMessage::Ui(UiEvent {
            view: self.seq,
            handler,
            value,
        })
    }
}

fn length(given: Option<ui::Length>, default: Length) -> Length {
    match given {
        None => default,
        Some(ui::Length::Shrink) => Length::Shrink,
        Some(ui::Length::Fill) => Length::Fill,
        Some(ui::Length::Portion(portion)) => Length::FillPortion(portion.max(1)),
        Some(ui::Length::Px(px)) => Length::Fixed(px.max(0.0)),
    }
}

fn padding(given: Option<ui::Padding>) -> Padding {
    match given {
        None => Padding::ZERO,
        Some(given) => {
            let [top, right, bottom, left] = given.sides();
            Padding {
                top,
                right,
                bottom,
                left,
            }
        }
    }
}

fn horizontal(align: ui::Align) -> Horizontal {
    match align {
        ui::Align::Start => Horizontal::Left,
        ui::Align::Center => Horizontal::Center,
        ui::Align::End => Horizontal::Right,
    }
}

fn vertical(align: ui::Align) -> Vertical {
    match align {
        ui::Align::Start => Vertical::Top,
        ui::Align::Center => Vertical::Center,
        ui::Align::End => Vertical::Bottom,
    }
}

fn alignment(align: ui::Align) -> Alignment {
    match align {
        ui::Align::Start => Alignment::Start,
        ui::Align::Center => Alignment::Center,
        ui::Align::End => Alignment::End,
    }
}

fn border(color: Color, radius: f32) -> Border {
    Border {
        color,
        width: 1.0,
        radius: radius.into(),
    }
}

fn surface(
    style: KitStyle,
    background: Option<Color>,
    edge: Option<Color>,
    radius: f32,
) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        text_color: Some(style.palette.text),
        background: background.map(Background::Color),
        border: match edge {
            Some(edge) => border(edge, radius),
            None => Border {
                radius: radius.into(),
                ..Border::default()
            },
        },
        ..Default::default()
    }
}

fn plain<'a>(
    style: KitStyle,
    content: impl text::IntoFragment<'a>,
    size: TextSize,
    tone: Tone,
) -> text::Text<'a> {
    text(content)
        .size(size.px())
        .color(style.tone(tone))
        .font(style.font)
}

fn strong<'a>(
    style: KitStyle,
    content: impl text::IntoFragment<'a>,
    size: TextSize,
) -> text::Text<'a> {
    text(content)
        .size(size.px())
        .color(style.palette.text)
        .font(style.weighted(Weight::Semibold, false))
}

fn center(content: El<'_>) -> El<'_> {
    container(content).center(Length::Fill).into()
}

fn line(style: KitStyle) -> El<'static> {
    let color = style.palette.line;
    rule::horizontal(1)
        .style(move |_| rule::Style {
            color,
            radius: 0.0.into(),
            fill_mode: rule::FillMode::Full,
            snap: true,
        })
        .into()
}

fn vertical_line(style: KitStyle) -> El<'static> {
    let color = style.palette.line;
    rule::vertical(1)
        .style(move |_| rule::Style {
            color,
            radius: 0.0.into(),
            fill_mode: rule::FillMode::Full,
            snap: true,
        })
        .into()
}

/// A kit's icon as hangar-ui has it: the twelve of Hangar UI are among the
/// window's own. One a newer Hangar has is none here.
fn ui_icon(icon: ui::Icon) -> Option<hangar_ui::Icon> {
    use hangar_ui::Icon as Hangar;
    Some(match icon {
        ui::Icon::Alert => Hangar::Alert,
        ui::Icon::Check => Hangar::Check,
        ui::Icon::ChevronDown => Hangar::ChevronDown,
        ui::Icon::Close => Hangar::Close,
        ui::Icon::Copy => Hangar::Copy,
        ui::Icon::Info => Hangar::Info,
        ui::Icon::Lock => Hangar::Lock,
        ui::Icon::More => Hangar::More,
        ui::Icon::Plus => Hangar::Plus,
        ui::Icon::Restart => Hangar::Restart,
        ui::Icon::Search => Hangar::Search,
        ui::Icon::Updown => Hangar::Updown,
        ui::Icon::Unknown => return None,
    })
}

fn icon_image(style: KitStyle, icon: ui::Icon, size: f32, color: Option<Color>) -> El<'static> {
    let color = color.unwrap_or(style.palette.text);
    match ui_icon(icon) {
        Some(which) => hangar_ui::icon(which, size).color(color).into(),
        None => space().width(size).height(size).into(),
    }
}

fn glyph_image(style: KitStyle, status: Status, size: f32) -> El<'static> {
    let color = style.status_fill(status);
    svg(icons::status(status))
        .width(size)
        .height(size)
        .style(move |_, _| svg::Style { color: Some(color) })
        .into()
}

fn state_label<'a>(style: KitStyle, status: Status, label: impl text::IntoFragment<'a>) -> El<'a> {
    row![
        glyph_image(style, status, 10.0),
        plain(style, label, TextSize::Control, Tone::Default).color(style.status_text(status))
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .into()
}

fn with_tooltip<'a>(style: KitStyle, content: El<'a>, words: Option<&'a str>) -> El<'a> {
    let Some(words) = words else { return content };
    hangar_ui::tip::over(style.theme, content, words)
}

fn scroll_style(style: KitStyle) -> impl Fn(&Theme, scrollable::Status) -> scrollable::Style {
    move |_, _| {
        let p = style.palette;
        let rail = scrollable::Rail {
            background: None,
            border: Border::default(),
            scroller: scrollable::Scroller {
                background: Background::Color(p.border),
                border: Border {
                    radius: 3.0.into(),
                    ..Border::default()
                },
            },
        };
        scrollable::Style {
            container: container::Style::default(),
            vertical_rail: rail,
            horizontal_rail: rail,
            gap: None,
            auto_scroll: scrollable::AutoScroll {
                background: Background::Color(p.float),
                border: border(p.border, 6.0),
                shadow: Shadow::default(),
                icon: p.text,
            },
        }
    }
}

fn thin() -> Scrollbar {
    Scrollbar::new().width(6).scroller_width(6)
}

/// A row that takes the pointer: no surface of its own but under it, and
/// the chosen one's.
fn row_style(style: KitStyle, selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let p = style.palette;
        let background = match status {
            _ if selected => Some(p.selected),
            button::Status::Hovered | button::Status::Pressed => Some(p.hover),
            _ => None,
        };
        button::Style {
            background: background.map(Background::Color),
            text_color: p.text,
            border: Border {
                radius: 6.0.into(),
                ..Border::default()
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Draws one widget, and through it its children.
fn element<'a>(cx: &Cx<'a>, given: &'a ui::Element) -> El<'a> {
    let style = cx.style;
    let p = style.palette;
    let all = |children: &'a [ui::Element]| {
        children
            .iter()
            .map(|child| element(cx, child))
            .collect::<Vec<_>>()
    };
    match given {
        ui::Element::Column(w) => Column::with_children(all(&w.children))
            .spacing(w.spacing.unwrap_or(0.0))
            .padding(padding(w.padding))
            .align_x(alignment(w.align))
            .width(length(w.width, Length::Fill))
            .height(length(w.height, Length::Shrink))
            .into(),
        ui::Element::Row(w) => {
            let line = Row::with_children(all(&w.children))
                .spacing(w.spacing.unwrap_or(0.0))
                .padding(padding(w.padding))
                .align_y(alignment(w.align))
                .width(length(w.width, Length::Fill))
                .height(length(w.height, Length::Shrink));
            if w.wrap {
                line.wrap()
                    .vertical_spacing(w.spacing.unwrap_or(0.0))
                    .into()
            } else {
                line.into()
            }
        }
        ui::Element::Grid(w) => {
            let cells = grid(all(&w.children)).spacing(w.spacing.unwrap_or(8.0));
            match (w.columns, w.min_width) {
                (Some(columns), _) => cells.columns(columns.max(1) as usize).into(),
                (None, Some(min_width)) => cells.fluid(min_width.max(40.0)).into(),
                (None, None) => cells.columns(2).into(),
            }
        }
        ui::Element::Scroll(w) => {
            let direction = match w.axis {
                ui::Axis::Vertical => Direction::Vertical(thin()),
                ui::Axis::Horizontal => Direction::Horizontal(thin()),
                ui::Axis::Both => Direction::Both {
                    vertical: thin(),
                    horizontal: thin(),
                },
            };
            let scroll = scrollable(element(cx, &w.child))
                .direction(direction)
                .width(length(w.width, Length::Fill))
                .height(length(w.height, Length::Fill))
                .style(scroll_style(style));
            if w.stick_to_end {
                scroll.anchor_bottom().into()
            } else {
                scroll.into()
            }
        }
        ui::Element::Split(w) => {
            let first = (w.ratio.unwrap_or(0.5).clamp(0.1, 0.9) * 100.0).round() as u16;
            let second = 100 - first;
            match w.axis {
                ui::Axis::Vertical => column![
                    container(element(cx, &w.first))
                        .width(Length::Fill)
                        .height(Length::FillPortion(first)),
                    line(style),
                    container(element(cx, &w.second))
                        .width(Length::Fill)
                        .height(Length::FillPortion(second)),
                ]
                .height(Length::Fill)
                .into(),
                _ => row![
                    container(element(cx, &w.first))
                        .height(Length::Fill)
                        .width(Length::FillPortion(first)),
                    vertical_line(style),
                    container(element(cx, &w.second))
                        .height(Length::Fill)
                        .width(Length::FillPortion(second)),
                ]
                .height(Length::Fill)
                .into(),
            }
        }
        ui::Element::Container(w) => {
            let radius = w.radius.unwrap_or(if w.surface == ui::Surface::None {
                0.0
            } else {
                8.0
            });
            container(element(cx, &w.child))
                .padding(padding(w.padding))
                .width(length(w.width, Length::Shrink))
                .height(length(w.height, Length::Shrink))
                .align_x(horizontal(w.align_x))
                .align_y(vertical(w.align_y))
                .style(surface(
                    style,
                    style.surface(w.surface),
                    w.border.then_some(p.border),
                    radius,
                ))
                .into()
        }
        ui::Element::Page(w) if w.fill => Column::with_children(all(&w.children))
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        ui::Element::Page(w) => scrollable(
            Column::with_children(all(&w.children))
                .spacing(16)
                .padding(12)
                .width(Length::Fill),
        )
        .direction(Direction::Vertical(thin()))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(scroll_style(style))
        .into(),
        ui::Element::Bar(w) => {
            let controls = container(
                Row::with_children(all(&w.children))
                    .spacing(6)
                    .align_y(Alignment::Center)
                    .wrap()
                    .vertical_spacing(4),
            )
            .padding([4, 12])
            .width(Length::Fill)
            .align_y(Vertical::Center);
            if w.foot {
                column![line(style), controls].into()
            } else {
                column![controls, line(style)].into()
            }
        }
        ui::Element::Space(w) => space()
            .width(length(w.width, Length::Shrink))
            .height(length(w.height, Length::Shrink))
            .into(),
        ui::Element::Divider(w) if w.vertical => vertical_line(style),
        ui::Element::Divider(_) => line(style),
        ui::Element::Tooltip(w) => with_tooltip(style, element(cx, &w.child), Some(&w.text)),
        ui::Element::Text(w) => text_widget(style, w),
        ui::Element::Spans(w) => {
            let spans: Vec<text::Span<'a, HandlerId>> = w
                .spans
                .iter()
                .map(|given| {
                    let mut piece = span(given.text.as_str())
                        .color(if given.on_press.is_some() {
                            p.accent_text
                        } else {
                            style.tone(given.tone)
                        })
                        .font(style.weighted(given.weight, given.mono));
                    if let Some(handler) = given.on_press {
                        piece = piece.link(handler).underline(true);
                    }
                    piece
                })
                .collect();
            let seq = cx.seq;
            rich_text(spans)
                .size(w.size.px())
                .wrapping(if w.wrap == Wrap::Truncate {
                    Wrapping::None
                } else {
                    Wrapping::WordOrGlyph
                })
                .on_link_click(move |handler| {
                    KitMessage::Ui(UiEvent {
                        view: seq,
                        handler,
                        value: None,
                    })
                })
                .into()
        }
        ui::Element::Pre(w) => container(
            scrollable(
                text(w.text.as_str())
                    .size(TextSize::Code.px())
                    .font(style.mono)
                    .color(p.text)
                    .wrapping(Wrapping::WordOrGlyph),
            )
            .direction(Direction::Vertical(thin()))
            .height(Length::Shrink)
            .style(scroll_style(style)),
        )
        .padding([6, 10])
        .max_height(w.height.unwrap_or(240.0))
        .width(Length::Fill)
        .style(surface(style, Some(p.well), Some(p.line), 6.0))
        .into(),
        ui::Element::Log(w) => log_widget(style, w),
        ui::Element::KeyValue(w) => Column::with_children(w.pairs.iter().map(|pair| {
            row![
                plain(style, pair.key.as_str(), TextSize::Control, Tone::Dim)
                    .width(Length::FillPortion(1)),
                container(element(cx, &pair.value)).width(Length::FillPortion(2)),
            ]
            .spacing(12)
            .into()
        }))
        .spacing(6)
        .width(Length::Fill)
        .into(),
        ui::Element::Header(w) => {
            let mut main = column![strong(style, w.title.as_str(), TextSize::Title)]
                .spacing(2)
                .width(Length::Fill);
            if !w.state.is_empty() {
                main = main.push(
                    Row::with_children(all(&w.state))
                        .spacing(6)
                        .align_y(Alignment::Center)
                        .wrap(),
                );
            }
            column![
                container(
                    row![
                        main,
                        Row::with_children(all(&w.actions))
                            .spacing(6)
                            .align_y(Alignment::Center)
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center)
                )
                .padding([8, 0]),
                line(style),
            ]
            .into()
        }
        ui::Element::Section(w) => {
            let mut head = vec![
                text(w.title.to_uppercase())
                    .size(TextSize::Label.px())
                    .color(p.text_muted)
                    .font(style.weighted(Weight::Semibold, false))
                    .into(),
            ];
            if let Some(count) = &w.count {
                head.push(plain(style, count.as_str(), TextSize::Label, Tone::Faint).into());
            }
            head.push(space().width(Length::Fill).into());
            head.extend(all(&w.actions));
            column![
                Row::with_children(head)
                    .spacing(6)
                    .align_y(Alignment::Center),
                Column::with_children(all(&w.children)).spacing(6),
            ]
            .spacing(8)
            .width(Length::Fill)
            .into()
        }
        ui::Element::List(w) => {
            let items: Vec<El<'a>> = w.items.iter().map(|item| list_item(cx, item)).collect();
            if w.group {
                let mut rows = Vec::with_capacity(items.len() * 2);
                for (index, item) in items.into_iter().enumerate() {
                    if index > 0 {
                        rows.push(line(style));
                    }
                    rows.push(item);
                }
                container(Column::with_children(rows))
                    .padding(2)
                    .width(Length::Fill)
                    .style(surface(style, Some(p.surface), Some(p.line), 8.0))
                    .into()
            } else {
                Column::with_children(items)
                    .spacing(1)
                    .width(Length::Fill)
                    .into()
            }
        }
        ui::Element::Table(w) => table_widget(cx, w),
        ui::Element::Tree(w) => {
            let mut lines = Vec::new();
            tree_lines(cx, w, &w.nodes, 0, &mut lines);
            Column::with_children(lines)
                .spacing(1)
                .width(Length::Fill)
                .into()
        }
        ui::Element::Tabs(w) => {
            let tabs = w.tabs.iter().map(|tab| {
                let chosen = tab.id == w.selected;
                let mut label =
                    vec![plain(style, tab.label.as_str(), TextSize::Control, Tone::Default).into()];
                if let Some(count) = &tab.count {
                    label.push(plain(style, count.as_str(), TextSize::Meta, Tone::Muted).into());
                }
                button(
                    Row::with_children(label)
                        .spacing(6)
                        .align_y(Alignment::Center),
                )
                .padding([3, 10])
                .on_press_maybe(
                    w.on_select
                        .map(|handler| cx.message(handler, Some(EventValue::Text(tab.id.clone())))),
                )
                .style(move |_, status| {
                    let background = if chosen {
                        Some(p.seg_thumb)
                    } else if matches!(status, button::Status::Hovered) {
                        Some(p.hover)
                    } else {
                        None
                    };
                    button::Style {
                        background: background.map(Background::Color),
                        text_color: if chosen { p.text } else { p.text_dim },
                        border: Border {
                            radius: 5.0.into(),
                            ..Border::default()
                        },
                        shadow: Shadow::default(),
                        snap: true,
                    }
                })
                .into()
            });
            container(Row::with_children(tabs).spacing(2))
                .padding(2)
                .style(surface(style, Some(p.seg_track), None, 6.0))
                .into()
        }
        ui::Element::Empty(w) => {
            let mut parts: Vec<El<'a>> = Vec::new();
            match w.kind {
                EmptyKind::Loading => parts.push(state_label(style, Status::Running, &w.title)),
                EmptyKind::Error => {
                    parts.push(icon_image(style, ui::Icon::Alert, 18.0, Some(p.text_dim)));
                    parts.push(strong(style, w.title.as_str(), TextSize::Body).into());
                }
                EmptyKind::Denied => {
                    parts.push(icon_image(style, ui::Icon::Lock, 18.0, Some(p.text_dim)));
                    parts.push(strong(style, w.title.as_str(), TextSize::Body).into());
                }
                EmptyKind::Empty => {
                    parts.push(strong(style, w.title.as_str(), TextSize::Body).into())
                }
            }
            if let Some(words) = &w.text {
                parts.push(
                    plain(style, words.as_str(), TextSize::Control, Tone::Dim)
                        .align_x(Horizontal::Center)
                        .into(),
                );
            }
            if !w.actions.is_empty() {
                parts.push(Row::with_children(all(&w.actions)).spacing(6).into());
            }
            Column::with_children(parts)
                .spacing(6)
                .align_x(Alignment::Center)
                .padding([24, 16])
                .width(Length::Fill)
                .into()
        }
        ui::Element::Banner(w) => {
            let (background, edge) = match w.kind {
                Notice::Info | Notice::Success => (p.surface, p.line),
                Notice::Warning => (p.surface, p.warn_fill),
                Notice::Error => (p.danger_soft, p.danger),
            };
            let mut parts = vec![
                plain(style, w.text.as_str(), TextSize::Control, Tone::Default)
                    .width(Length::Fill)
                    .into(),
            ];
            parts.extend(all(&w.actions));
            container(
                Row::with_children(parts)
                    .spacing(8)
                    .align_y(Alignment::Center),
            )
            .padding([6, 10])
            .width(Length::Fill)
            .style(surface(style, Some(background), Some(edge), 6.0))
            .into()
        }
        ui::Element::Button(w) => kit_button(
            style,
            w.kind,
            w.label.as_deref(),
            w.icon,
            w.small,
            w.tooltip.as_deref(),
            w.on_press.map(|handler| cx.message(handler, None)),
        ),
        ui::Element::Link(w) => {
            button(plain(style, w.label.as_str(), TextSize::Body, Tone::Accent))
                .padding(0)
                .on_press_maybe(w.on_press.map(|handler| cx.message(handler, None)))
                .style(move |_, status| button::Style {
                    background: None,
                    text_color: if status == button::Status::Hovered {
                        p.accent
                    } else {
                        p.accent_text
                    },
                    ..button::Style::default()
                })
                .into()
        }
        ui::Element::Menu(w) => menu_widget(cx, w),
        // The window's own fields (hangar-ui): the label, the hint and the
        // error are the field's, and so is the look of each state.
        ui::Element::TextInput(w) => {
            let seq = cx.seq;
            let mut input = hangar_ui::input::text_field(style.theme, &w.value)
                .placeholder(&w.placeholder)
                .error(w.error.as_deref())
                .width(length(w.width, Length::Fill));
            if let Some(label) = &w.label {
                input = input.label(label);
            }
            if let Some(hint) = &w.hint {
                input = input.hint(hint);
            }
            if let Some(handler) = w.on_input {
                input = input.on_input(move |value| {
                    KitMessage::Ui(UiEvent {
                        view: seq,
                        handler,
                        value: Some(EventValue::Text(value)),
                    })
                });
            }
            if let Some(handler) = w.on_submit {
                input = input.on_submit(cx.message(handler, None));
            }
            if w.mono {
                input = input.mono();
            }
            if w.secure {
                input = input.secure();
            }
            input.into()
        }
        ui::Element::TextArea(w) => {
            let Some(editor) = cx.widgets.editors.get(&w.id) else {
                return plain(style, w.value.as_str(), TextSize::Control, Tone::Default).into();
            };
            let area = w.id.clone();
            let (seq, handler, invalid) = (cx.seq, w.on_input, w.error.is_some());
            let input = text_editor(&editor.content)
                .placeholder(w.placeholder.as_str())
                .height(w.height.unwrap_or(120.0))
                .size(TextSize::Control.px())
                .padding(8)
                .font(if w.mono { style.mono } else { style.font })
                .on_action(move |action| {
                    KitMessage::Local(Local::Edit {
                        area: area.clone(),
                        action,
                        view: seq,
                        handler,
                    })
                })
                // The look of the window's own text areas (hangar-ui's).
                .style(move |theme, status| {
                    let t = style.theme;
                    if invalid {
                        hangar_theme::style::text_editor::field_invalid(t)(theme, status)
                    } else {
                        hangar_theme::style::text_editor::field(t)(theme, status)
                    }
                });
            field(
                style,
                w.label.as_deref(),
                input.into(),
                w.hint.as_deref(),
                w.error.as_deref(),
            )
        }
        ui::Element::Search(w) => {
            let seq = cx.seq;
            let mut input = hangar_ui::input::search_field(style.theme, &w.value, &w.placeholder)
                .clear_tip(style.t(Key::SettingsSearchClear));
            if let Some(handler) = w.on_input {
                input = input.on_input(move |value| {
                    KitMessage::Ui(UiEvent {
                        view: seq,
                        handler,
                        value: Some(EventValue::Text(value)),
                    })
                });
            }
            if let Some(handler) = w.on_submit {
                input = input.on_submit(cx.message(handler, None));
            }
            // The clear button is the kit's: it is there when the kit named
            // what it does (`Search::on_clear`), and not otherwise.
            match w.on_clear {
                Some(handler) => input = input.on_clear(cx.message(handler, None)),
                None => input = input.clearable(false),
            }
            input.into()
        }
        ui::Element::Select(w) => {
            let options: Vec<Choice> = w
                .options
                .iter()
                .map(|option| Choice {
                    id: option.value.clone(),
                    label: option.label.clone(),
                })
                .collect();
            let selected = w
                .selected
                .as_ref()
                .and_then(|value| options.iter().find(|option| option.id == *value).cloned());
            let picker: El<'a> = match w.on_select {
                Some(handler) => {
                    let seq = cx.seq;
                    hangar_ui::input::select(style.theme, options, selected, move |choice| {
                        KitMessage::Ui(UiEvent {
                            view: seq,
                            handler,
                            value: Some(EventValue::Text(choice.id)),
                        })
                    })
                }
                None => container(plain(
                    style,
                    selected
                        .map(|choice| choice.label)
                        .unwrap_or_else(|| w.placeholder.clone()),
                    TextSize::Control,
                    Tone::Faint,
                ))
                .padding([5, 8])
                .style(surface(style, Some(p.field), Some(p.border), 6.0))
                .into(),
            };
            // A select fills its line, as a text field does (`.kit-select
            // select { width: 100% }`).
            let picker = container(picker).width(length(w.width, Length::Fill));
            field(style, w.label.as_deref(), picker.into(), None, None)
        }
        ui::Element::Checkbox(w) => {
            let seq = cx.seq;
            let mut tick = hangar_ui::input::checkbox(style.theme, &w.label, w.checked);
            if let Some(handler) = w.on_toggle {
                tick = tick.on_toggle(move |on| {
                    KitMessage::Ui(UiEvent {
                        view: seq,
                        handler,
                        value: Some(EventValue::Bool(on)),
                    })
                });
            }
            tick.into()
        }
        ui::Element::Switch(w) => {
            let seq = cx.seq;
            let mut switch = hangar_ui::input::switch(style.theme, w.on).label(&w.label);
            if let Some(handler) = w.on_toggle {
                switch = switch.on_toggle(move |on| {
                    KitMessage::Ui(UiEvent {
                        view: seq,
                        handler,
                        value: Some(EventValue::Bool(on)),
                    })
                });
            }
            switch.into()
        }
        ui::Element::Filter(w) => {
            let mut parts =
                vec![plain(style, w.label.as_str(), TextSize::Small, Tone::Default).into()];
            if let Some(count) = &w.count {
                let tone = if w.count_tone == Tone::Default {
                    Tone::Muted
                } else {
                    w.count_tone
                };
                parts.push(plain(style, count.as_str(), TextSize::Meta, tone).into());
            }
            let pressed = w.pressed;
            button(
                Row::with_children(parts)
                    .spacing(5)
                    .align_y(Alignment::Center),
            )
            .padding([2, 8])
            .on_press_maybe(w.on_press.map(|handler| cx.message(handler, None)))
            .style(move |_, status| button::Style {
                background: Some(Background::Color(if pressed {
                    p.accent_soft
                } else if status == button::Status::Hovered {
                    p.hover
                } else {
                    Color::TRANSPARENT
                })),
                text_color: p.text,
                border: border(if pressed { p.accent_ring } else { p.border }, 12.0),
                shadow: Shadow::default(),
                snap: true,
            })
            .into()
        }
        ui::Element::Chip(w) => {
            let tone = if w.tone == Tone::Default {
                Tone::Dim
            } else {
                w.tone
            };
            container(
                plain(style, w.label.as_str(), TextSize::Chip, tone)
                    .font(style.weighted(Weight::Medium, false)),
            )
            .padding([1, 6])
            .style(surface(style, Some(p.raised), Some(p.line), 4.0))
            .into()
        }
        ui::Element::State(w) => state_label(style, w.status, &w.label),
        ui::Element::Glyph(w) => {
            let mark: El<'a> = if w.dot {
                let color = style.status_fill(w.status);
                container(space().width(8).height(8))
                    .style(move |_| container::Style {
                        background: Some(Background::Color(color)),
                        border: Border {
                            radius: 4.0.into(),
                            ..Border::default()
                        },
                        ..Default::default()
                    })
                    .into()
            } else {
                glyph_image(style, w.status, 10.0)
            };
            with_tooltip(style, mark, Some(&w.label))
        }
        ui::Element::Progress(w) => {
            let (value, bar) = match (w.value, w.kind) {
                (None, _) => (1.0, p.accent_soft),
                (Some(value), ProgressKind::Normal) => (value, p.accent),
                (Some(value), ProgressKind::Stale) => (value, p.text_faint),
                (Some(value), ProgressKind::Success) => (value, p.ok_fill),
                (Some(value), ProgressKind::Failure) => (value, p.danger),
            };
            let track = p.selected;
            let drawn: El<'a> = progress_bar(0.0..=1.0, value.clamp(0.0, 1.0))
                .length(Length::Fill)
                .girth(3)
                .style(move |_| progress_bar::Style {
                    background: Background::Color(track),
                    bar: Background::Color(bar),
                    border: Border {
                        radius: 2.0.into(),
                        ..Border::default()
                    },
                })
                .into();
            with_tooltip(style, drawn, w.label.as_deref())
        }
        ui::Element::Icon(w) => icon_image(
            style,
            w.icon,
            w.size.unwrap_or(14.0),
            Some(style.tone(w.tone)),
        ),
        ui::Element::Image(w) => {
            let fit = match w.fit {
                ui::Fit::Contain => ContentFit::Contain,
                ui::Fit::Cover => ContentFit::Cover,
                ui::Fit::Fill => ContentFit::Fill,
                ui::Fit::ScaleDown => ContentFit::ScaleDown,
            };
            let (width, height) = (
                length(w.width, Length::Shrink),
                length(w.height, Length::Shrink),
            );
            let drawn: El<'a> = match cx.widgets.images.get(&w.source) {
                Some(Picture::Raster(handle)) => image(handle.clone())
                    .width(width)
                    .height(height)
                    .content_fit(fit)
                    .into(),
                Some(Picture::Vector(handle)) => svg(handle.clone())
                    .width(width)
                    .height(height)
                    .content_fit(fit)
                    .into(),
                Some(Picture::Missing(why)) => plain(
                    style,
                    w.description.clone().unwrap_or_else(|| why.said(style)),
                    TextSize::Small,
                    Tone::Faint,
                )
                .into(),
                None => plain(
                    style,
                    w.description.as_deref().unwrap_or(""),
                    TextSize::Small,
                    Tone::Faint,
                )
                .into(),
            };
            with_tooltip(style, drawn, w.description.as_deref())
        }
        ui::Element::Sparkline(w) => {
            let drawn: El<'a> = canvas(chart::Spark {
                values: w.values.clone(),
                color: style.tone(w.tone),
                fill: w.fill,
                min: w.min,
                max: w.max,
            })
            .width(Length::Fill)
            .height(w.height.unwrap_or(24.0))
            .into();
            with_tooltip(style, drawn, w.label.as_deref())
        }
        ui::Element::Bars(w) => bars_widget(style, w),
        ui::Element::Unsupported => plain(
            style,
            style.t(Key::KitsTabNativeNewer),
            TextSize::Small,
            Tone::Faint,
        )
        .into(),
    }
}

fn text_widget<'a>(style: KitStyle, w: &'a ui::Text) -> El<'a> {
    let p = style.palette;
    let mut drawn = text(w.text.as_str())
        .size(w.size.px())
        .color(style.tone(w.tone))
        .font(style.weighted(w.weight, w.mono || w.code || w.size == TextSize::Code))
        .wrapping(if w.wrap == Wrap::Truncate {
            Wrapping::None
        } else {
            Wrapping::WordOrGlyph
        });
    if w.numeric || w.align != ui::Align::Start {
        let align = if w.numeric && w.align == ui::Align::Start {
            ui::Align::End
        } else {
            w.align
        };
        drawn = drawn.align_x(horizontal(align)).width(Length::Fill);
    }
    if w.code {
        return container(drawn)
            .padding([1, 5])
            .style(surface(style, Some(p.well), Some(p.line), 4.0))
            .into();
    }
    if w.wrap == Wrap::Truncate {
        return container(drawn).width(Length::Fill).clip(true).into();
    }
    drawn.into()
}

fn log_widget<'a>(style: KitStyle, w: &'a ui::Log) -> El<'a> {
    let p = style.palette;
    let numbers = w.lines.iter().any(|line| line.number.is_some());
    let lines = w.lines.iter().map(|given| {
        let color = match given.kind {
            LineKind::Error => p.danger,
            LineKind::Warning => p.warn,
            LineKind::Head => p.text,
            LineKind::Normal => p.text_dim,
        };
        let weight = if given.kind == LineKind::Head {
            Weight::Semibold
        } else {
            Weight::Normal
        };
        let mut parts: Vec<El<'a>> = Vec::new();
        if numbers {
            parts.push(
                text(
                    given
                        .number
                        .map(|number| number.to_string())
                        .unwrap_or_default(),
                )
                .size(TextSize::Code.px())
                .font(style.mono)
                .color(p.text_faint)
                .align_x(Horizontal::Right)
                .width(36)
                .into(),
            );
        }
        parts.push(
            text(given.text.as_str())
                .size(TextSize::Code.px())
                .font(style.weighted(weight, true))
                .color(color)
                .wrapping(Wrapping::WordOrGlyph)
                .width(Length::Fill)
                .into(),
        );
        let current = given.current;
        container(Row::with_children(parts).spacing(10))
            .padding([1, 8])
            .width(Length::Fill)
            .style(move |_| container::Style {
                background: current.then_some(Background::Color(p.selected)),
                ..Default::default()
            })
            .into()
    });
    let scroll = scrollable(Column::with_children(lines).padding([4, 0]))
        .direction(Direction::Vertical(thin()))
        .height(Length::Shrink)
        .style(scroll_style(style));
    let scroll: El<'a> = if w.at_end {
        scroll.anchor_bottom().into()
    } else {
        scroll.into()
    };
    container(scroll)
        .max_height(w.height.unwrap_or(240.0))
        .width(Length::Fill)
        .style(surface(style, Some(p.well), Some(p.line), 6.0))
        .into()
}

fn field<'a>(
    style: KitStyle,
    label: Option<&'a str>,
    input: El<'a>,
    hint: Option<&'a str>,
    error: Option<&'a str>,
) -> El<'a> {
    if label.is_none() && hint.is_none() && error.is_none() {
        return input;
    }
    let mut parts = Vec::new();
    if let Some(label) = label {
        parts.push(plain(style, label, TextSize::Small, Tone::Dim).into());
    }
    parts.push(input);
    if let Some(hint) = hint {
        parts.push(plain(style, hint, TextSize::Meta, Tone::Muted).into());
    }
    if let Some(error) = error {
        parts.push(plain(style, error, TextSize::Meta, Tone::Danger).into());
    }
    Column::with_children(parts)
        .spacing(4)
        .width(Length::Fill)
        .into()
}

fn list_item<'a>(cx: &Cx<'a>, item: &'a ui::ListItem) -> El<'a> {
    let style = cx.style;
    let wrapping = if item.wrap {
        Wrapping::WordOrGlyph
    } else {
        Wrapping::None
    };
    let mut main =
        column![plain(style, item.title.as_str(), TextSize::Row, Tone::Default).wrapping(wrapping)]
            .spacing(1);
    if let Some(sub) = &item.sub {
        main = main.push(plain(style, sub.as_str(), TextSize::Small, Tone::Dim).wrapping(wrapping));
    }
    let mut parts: Vec<El<'a>> = Vec::new();
    if let Some(lead) = &item.lead {
        parts.push(container(element(cx, lead)).center_x(16).into());
    }
    parts.push(container(main).width(Length::Fill).clip(!item.wrap).into());
    if let Some(end) = &item.end {
        parts.push(element(cx, end));
    }
    parts.extend(item.actions.iter().map(|action| element(cx, action)));
    let line = Row::with_children(parts)
        .spacing(8)
        .align_y(Alignment::Center)
        .padding([5, 8]);
    let drawn: El<'a> = match item.on_press {
        Some(handler) => button(line)
            .padding(0)
            .width(Length::Fill)
            .on_press(cx.message(handler, None))
            .style(row_style(style, item.selected))
            .into(),
        None => {
            let p = style.palette;
            let selected = item.selected;
            container(line)
                .width(Length::Fill)
                .style(move |_| container::Style {
                    background: selected.then_some(Background::Color(p.selected)),
                    border: Border {
                        radius: 6.0.into(),
                        ..Border::default()
                    },
                    ..Default::default()
                })
                .into()
        }
    };
    with_tooltip(style, drawn, item.tooltip.as_deref())
}

/// What a cell sorts by: its key, or its text.
fn sort_key(cell: &ui::Cell) -> Option<SortKey> {
    cell.key.clone().or_else(|| match &cell.content {
        ui::Element::Text(given) => Some(SortKey::Text(given.text.clone())),
        _ => None,
    })
}

fn compare(a: &Option<SortKey>, b: &Option<SortKey>, descending: bool) -> Ordering {
    let order = match (a, b) {
        (None, None) => return Ordering::Equal,
        // What has no key comes last, whichever the order.
        (None, Some(_)) => return Ordering::Greater,
        (Some(_), None) => return Ordering::Less,
        (Some(SortKey::Number(a)), Some(SortKey::Number(b))) => {
            a.partial_cmp(b).unwrap_or(Ordering::Equal)
        }
        (Some(SortKey::Number(_)), Some(SortKey::Text(_))) => Ordering::Less,
        (Some(SortKey::Text(_)), Some(SortKey::Number(_))) => Ordering::Greater,
        (Some(SortKey::Text(a)), Some(SortKey::Text(b))) => a.to_lowercase().cmp(&b.to_lowercase()),
    };
    if descending { order.reverse() } else { order }
}

/// The order a table's rows are drawn in: the kit's when it sorts, else by
/// the sort Hangar keeps for it.
pub(crate) fn table_order(table: &ui::Table, kept: Option<Sort>) -> (Vec<usize>, Option<Sort>) {
    let mut order: Vec<usize> = (0..table.rows.len()).collect();
    if table.on_sort.is_some() {
        return (order, table.sort);
    }
    let sort = kept.or(table.sort);
    if let Some(sort) = sort {
        let column = sort.column as usize;
        let keys: Vec<Option<SortKey>> = table
            .rows
            .iter()
            .map(|row| row.cells.get(column).and_then(sort_key))
            .collect();
        order.sort_by(|a, b| compare(&keys[*a], &keys[*b], sort.descending));
    }
    (order, sort)
}

fn table_widget<'a>(cx: &Cx<'a>, w: &'a ui::Table) -> El<'a> {
    let style = cx.style;
    let p = style.palette;
    let (order, current) = table_order(w, cx.widgets.sorts.get(&w.id).copied());
    let widths: Vec<Length> = w
        .columns
        .iter()
        .map(|column| match column.width {
            None | Some(ui::Length::Shrink) => Length::FillPortion(1),
            given => length(given, Length::Fill),
        })
        .collect();
    let header = w.columns.iter().enumerate().map(|(index, column)| {
        let mut title = vec![
            text(column.title.as_str())
                .size(TextSize::Label.px())
                .color(p.text_muted)
                .font(style.weighted(Weight::Semibold, false))
                .into(),
        ];
        if let Some(sort) = current.filter(|sort| sort.column as usize == index) {
            title.push(
                plain(
                    style,
                    if sort.descending { "↓" } else { "↑" },
                    TextSize::Label,
                    Tone::Muted,
                )
                .into(),
            );
        }
        let label = container(Row::with_children(title).spacing(4))
            .width(Length::Fill)
            .align_x(horizontal(column.align));
        let next = Sort::next(current, index as u32);
        let press = match (column.sortable, w.on_sort) {
            (false, _) => None,
            (true, Some(handler)) => Some(cx.message(handler, Some(EventValue::Sort(next)))),
            (true, None) => Some(KitMessage::Local(Local::Sort {
                table: w.id.clone(),
                sort: next,
            })),
        };
        let cell: El<'a> = match press {
            Some(press) => button(label)
                .padding(0)
                .on_press(press)
                .style(row_style(style, false))
                .into(),
            None => label.into(),
        };
        container(cell).width(widths[index]).into()
    });
    let mut lines: Vec<El<'a>> = vec![
        Row::with_children(header).spacing(8).padding([4, 8]).into(),
        line(style),
    ];
    for index in order {
        let given = &w.rows[index];
        let cells = given.cells.iter().enumerate().map(|(column, cell)| {
            let align = w
                .columns
                .get(column)
                .map(|column| column.align)
                .unwrap_or_default();
            container(element(cx, &cell.content))
                .width(
                    widths
                        .get(column)
                        .copied()
                        .unwrap_or(Length::FillPortion(1)),
                )
                .align_x(horizontal(align))
                .clip(true)
                .into()
        });
        let line = Row::with_children(cells)
            .spacing(8)
            .align_y(Alignment::Center);
        let selected = w.selected == Some(index as u32);
        lines.push(match w.on_row {
            // A row that takes the pointer is the window's list row, as tall
            // as its cells.
            Some(handler) => hangar_ui::list::row(style.theme, line, selected)
                .fit()
                .padding([5.0, 8.0])
                .on_press(cx.message(handler, Some(EventValue::Index(index as u32))))
                .into(),
            None => container(line)
                .padding([5, 8])
                .width(Length::Fill)
                .style(move |_| container::Style {
                    background: selected.then_some(Background::Color(p.selected)),
                    ..Default::default()
                })
                .into(),
        });
    }
    let rows = Column::with_children(lines).width(Length::Fill);
    match w.height {
        Some(height) => scrollable(rows)
            .direction(Direction::Vertical(thin()))
            .height(length(Some(height), Length::Shrink))
            .style(scroll_style(style))
            .into(),
        None => rows.into(),
    }
}

fn tree_lines<'a>(
    cx: &Cx<'a>,
    tree: &'a ui::Tree,
    nodes: &'a [ui::TreeNode],
    depth: u16,
    lines: &mut Vec<El<'a>>,
) {
    let style = cx.style;
    let p = style.palette;
    for node in nodes {
        let open = cx
            .widgets
            .toggled
            .get(&(tree.id.clone(), node.id.clone()))
            .copied()
            .unwrap_or(node.open);
        let toggle: El<'a> = if node.children.is_empty() {
            space().width(20).into()
        } else {
            hangar_ui::button::icon_button(style.theme, hangar_ui::Icon::ChevronDown)
                .side(20.0)
                .icon_size(12.0)
                .rotate(if open {
                    0.0
                } else {
                    -std::f32::consts::FRAC_PI_2
                })
                .on_press(KitMessage::Local(Local::Toggle {
                    tree: tree.id.clone(),
                    node: node.id.clone(),
                    open: !open,
                }))
                .into()
        };
        let mut label = Vec::new();
        if let Some(icon) = node.icon {
            label.push(icon_image(style, icon, 13.0, Some(p.text_dim)));
        }
        label.push(
            plain(style, node.label.as_str(), TextSize::Row, Tone::Default)
                .wrapping(Wrapping::None)
                .into(),
        );
        let selected = tree.selected.as_deref() == Some(node.id.as_str());
        let name = hangar_ui::list::row(
            style.theme,
            Row::with_children(label)
                .spacing(6)
                .align_y(Alignment::Center),
            selected,
        )
        .height(24.0)
        .padding([0.0, 6.0])
        .on_press_maybe(
            tree.on_select
                .map(|handler| cx.message(handler, Some(EventValue::Text(node.id.clone())))),
        );
        lines.push(
            row![space().width(depth as f32 * 14.0), toggle, name]
                .spacing(2)
                .align_y(Alignment::Center)
                .into(),
        );
        if open {
            tree_lines(cx, tree, &node.children, depth + 1, lines);
        }
    }
}

/// An item of a menu or an option of a select, as a pick list shows it.
#[derive(Debug, Clone, PartialEq)]
struct Choice {
    id: String,
    label: String,
}

impl fmt::Display for Choice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

/// What tells a menu from another across views, a menu having no id: its
/// label, icon, kind and lines. A view that keeps the menu as it is keeps it
/// open; one that changes what it lists closes it.
pub(crate) fn menu_key(w: &ui::Menu) -> u64 {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    w.label.hash(&mut hasher);
    format!("{:?}{:?}{}", w.icon, w.kind, w.small).hash(&mut hasher);
    for entry in &w.items {
        match entry {
            ui::MenuEntry::Item(item) => {
                (&item.id, &item.label, &item.hint).hash(&mut hasher);
                (item.danger, item.disabled, item.checked).hash(&mut hasher);
            }
            ui::MenuEntry::Separator => 0u8.hash(&mut hasher),
        }
    }
    hasher.finish()
}

/// hangar-ui's menu under its button: separators, disabled lines drawn dim,
/// a check on the chosen ones, the hint at the end. Open is Hangar's to
/// keep ([`WidgetState::menu`]); choosing a line, or anything else the
/// person does in the tab, closes it.
fn menu_widget<'a>(cx: &Cx<'a>, w: &'a ui::Menu) -> El<'a> {
    let style = cx.style;
    let t = style.theme;
    let key = menu_key(w);
    let open = cx.widgets.menu == Some(key);
    let usable = w.on_select.is_some()
        && w.items
            .iter()
            .any(|entry| matches!(entry, ui::MenuEntry::Item(item) if !item.disabled));
    let toggle = usable.then(|| KitMessage::Local(Local::Menu((!open).then_some(key))));
    let anchor = match (&w.label, w.icon) {
        (None, icon) => kit_button(
            style,
            w.kind,
            None,
            Some(icon.unwrap_or(ui::Icon::More)),
            w.small,
            None,
            toggle,
        ),
        (Some(label), icon) => kit_button(
            style,
            w.kind,
            Some(label.as_str()),
            icon,
            w.small,
            None,
            toggle,
        ),
    };
    let entries = (open && usable).then(|| {
        let handler = w.on_select;
        let seq = cx.seq;
        w.items
            .iter()
            .map(|entry| match entry {
                ui::MenuEntry::Separator => hangar_ui::menu::separator(),
                ui::MenuEntry::Item(item) => {
                    let mut line = hangar_ui::menu::item(item.label.clone()).checked(item.checked);
                    if let Some(hint) = &item.hint {
                        line = line.shortcut(hint);
                    }
                    if item.danger {
                        line = line.danger();
                    }
                    if item.disabled {
                        line = line.disabled();
                    } else if let Some(handler) = handler {
                        line = line.on_select(KitMessage::Ui(UiEvent {
                            view: seq,
                            handler,
                            value: Some(EventValue::Text(item.id.clone())),
                        }));
                    }
                    line.into()
                }
            })
            .collect()
    });
    hangar_ui::menu::dropdown(t, anchor, entries)
        .on_dismiss(KitMessage::Local(Local::Menu(None)))
        .into()
}

/// A kit's button as hangar-ui draws one: its kinds are Hangar UI's, an icon
/// alone makes a square one, and no handler is a button that cannot be
/// pressed.
fn kit_button<'a>(
    style: KitStyle,
    kind: ButtonKind,
    label: Option<&str>,
    icon: Option<ui::Icon>,
    small: bool,
    tip: Option<&str>,
    on_press: Option<KitMessage>,
) -> El<'a> {
    let t = style.theme;
    let icon = icon.and_then(ui_icon);
    match (label, icon) {
        (None, Some(which)) => {
            let mut square = hangar_ui::button::icon_button(t, which).on_press_maybe(on_press);
            if small {
                square = square.small();
            }
            if kind == ButtonKind::Danger {
                square = square.danger();
            }
            if let Some(tip) = tip {
                square = square.tip(tip);
            }
            square.into()
        }
        (label, icon) => {
            let label = label.unwrap_or_default().to_owned();
            let mut pressable = match kind {
                ButtonKind::Primary => hangar_ui::button::primary(t, label),
                ButtonKind::Danger => hangar_ui::button::danger(t, label),
                ButtonKind::Ghost => hangar_ui::button::ghost(t, label),
                ButtonKind::Default => hangar_ui::button::secondary(t, label),
            };
            if let Some(which) = icon {
                pressable = pressable.icon(which);
            }
            if small {
                pressable = pressable.small();
            }
            if let Some(tip) = tip {
                pressable = pressable.tip(tip);
            }
            pressable.on_press_maybe(on_press).into()
        }
    }
}

fn bars_widget<'a>(style: KitStyle, w: &'a ui::Bars) -> El<'a> {
    let max = w
        .max
        .unwrap_or_else(|| w.bars.iter().map(|bar| bar.value).fold(0.0, f32::max))
        .max(f32::EPSILON);
    let share = |value: f32| ((value / max).clamp(0.0, 1.0) * 1000.0).round() as u16;
    let fill = |color: Color, width: Length, height: Length| -> El<'a> {
        container(space().width(Length::Fill).height(Length::Fill))
            .width(width)
            .height(height)
            .style(move |_| container::Style {
                background: Some(Background::Color(color)),
                border: Border {
                    radius: 2.0.into(),
                    ..Border::default()
                },
                ..Default::default()
            })
            .into()
    };
    let shown = |bar: &'a ui::BarItem| bar.text.clone().unwrap_or_else(|| format!("{}", bar.value));
    match w.axis.unwrap_or(ui::Axis::Horizontal) {
        ui::Axis::Vertical => {
            let height = w.height.unwrap_or(80.0);
            let columns = w.bars.iter().map(|bar| {
                let filled = share(bar.value);
                let mut parts: Vec<El<'a>> = Vec::new();
                if filled < 1000 {
                    parts.push(space().height(Length::FillPortion(1000 - filled)).into());
                }
                if filled > 0 {
                    parts.push(fill(
                        style.tone(bar.tone),
                        Length::Fill,
                        Length::FillPortion(filled),
                    ));
                }
                with_tooltip(
                    style,
                    column![
                        Column::with_children(parts)
                            .height(height)
                            .width(Length::Fill),
                        plain(style, bar.label.as_str(), TextSize::Meta, Tone::Muted)
                            .wrapping(Wrapping::None),
                    ]
                    .spacing(4)
                    .align_x(Alignment::Center)
                    .width(Length::Fill)
                    .into(),
                    None,
                )
            });
            Row::with_children(columns)
                .spacing(4)
                .width(Length::Fill)
                .into()
        }
        _ => {
            let lines = w.bars.iter().map(|bar| {
                let filled = share(bar.value);
                let mut track: Vec<El<'a>> = Vec::new();
                if filled > 0 {
                    track.push(fill(
                        style.tone(bar.tone),
                        Length::FillPortion(filled),
                        Length::Fixed(8.0),
                    ));
                }
                if filled < 1000 {
                    track.push(space().width(Length::FillPortion(1000 - filled)).into());
                }
                row![
                    plain(style, bar.label.as_str(), TextSize::Small, Tone::Dim)
                        .wrapping(Wrapping::None)
                        .width(Length::FillPortion(2)),
                    Row::with_children(track)
                        .width(Length::FillPortion(3))
                        .align_y(Alignment::Center),
                    text(shown(bar))
                        .size(TextSize::Small.px())
                        .color(style.palette.text)
                        .align_x(Horizontal::Right)
                        .width(56),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .into()
            });
            Column::with_children(lines)
                .spacing(4)
                .width(Length::Fill)
                .into()
        }
    }
}

fn failure_page<'a>(style: KitStyle, failure: &'a Failure, logs: Vec<&'a LogEntry>) -> El<'a> {
    let p = style.palette;
    let title = match failure.kind {
        FailureKind::Load => Key::KitsTabNativeNotStarted,
        _ => Key::KitsTabNativeStopped,
    };
    let mut parts: Vec<El<'a>> = vec![
        icon_image(style, ui::Icon::Alert, 18.0, Some(p.text_dim)),
        strong(style, style.t(title), TextSize::Body).into(),
        // Why, in the words of the runtime: what a kit's author needs to read.
        plain(style, failure.reason.as_str(), TextSize::Control, Tone::Dim)
            .align_x(Horizontal::Center)
            .into(),
        hangar_ui::button::primary(style.theme, style.t(Key::KitsTabNativeStartAgain))
            .icon(hangar_ui::Icon::Restart)
            .on_press(KitMessage::Reload)
            .into(),
    ];
    // What it said last, which is often why.
    let last: Vec<&str> = logs
        .iter()
        .rev()
        .take(6)
        .rev()
        .map(|entry| entry.text.as_str())
        .collect();
    if !last.is_empty() {
        parts.push(
            container(
                text(last.join("\n"))
                    .size(TextSize::Code.px())
                    .font(style.mono)
                    .color(p.text_dim),
            )
            .padding([6, 10])
            .width(Length::Fill)
            .style(surface(style, Some(p.well), Some(p.line), 6.0))
            .into(),
        );
    }
    Column::with_children(parts)
        .spacing(8)
        .align_x(Alignment::Center)
        .padding([24, 16])
        .width(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests;
