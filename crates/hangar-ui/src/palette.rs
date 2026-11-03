//! The command palette: one search field over a list of everything the app
//! can do or go to (`CommandPalette.tsx`).
//!
//! The palette is generic over what it lists. The app builds the items (its
//! commands, its panes, its workspaces), each carrying the value to hand back
//! when it is run, and gets that value back from [`Palette::update`]; the
//! palette does the matching ([`crate::matching`]), the ranking, the
//! highlighting and the keys.
//!
//! ```ignore
//! use hangar_ui::palette::{self, Palette, Item};
//!
//! // Opening it:
//! self.palette = Some(Palette::new(vec![
//!     Item::new(Target::Command(CommandId::Settings), "Commands", "Open the settings")
//!         .icon(Icon::Gear)
//!         .keys("Ctrl+,")
//!         .keywords(["preferences", "options"]),
//!     Item::new(Target::Pane(id), "Panes", "Go to Ben")
//!         .state(GlyphState::Waiting)
//!         .detail(Detail::new().plain("Claude Code · ").state(AgentState::Waiting, "waiting")),
//! ]));
//!
//! // In `update`:
//! Msg::Palette(msg) => {
//!     let Some(open) = &mut self.palette else { return Task::none() };
//!     let (event, task) = open.update(msg);
//!     match event {
//!         palette::Event::Run(target) => { self.palette = None; /* run it */ }
//!         palette::Event::Dismiss => self.palette = None,
//!         palette::Event::None => {}
//!     }
//!     return task.map(Msg::Palette);
//! }
//!
//! // In `view`, over the page:
//! palette::over(
//!     t,
//!     page,
//!     self.palette.as_ref().map(|p| p.view(t, &self.labels).map(Msg::Palette)),
//!     Msg::Palette(palette::Msg::Dismiss),
//! )
//! ```
//!
//! What it does, as the web app's did:
//!
//! - The field has the keyboard when it opens. Up and Down move the choice
//!   round the ends of the list, Page Up and Page Down by pages, Enter runs
//!   it, Escape dismisses, Tab does nothing (the field is the only stop).
//! - The pointer moving over a row chooses it; a click runs it.
//! - The best match is on top: groups with the best entry come first, the
//!   best entries first inside a group, ties in the order given.
//! - The letters that matched are bold, accents and case ignored.
//! - The chosen row is scrolled into view.

use hangar_theme::tokens::{font_size, size, space};
use hangar_theme::{AgentState, Theme, fonts, style};
use iced::advanced::text::Span;
use iced::widget::scrollable::{self, AbsoluteOffset};
use iced::widget::{
    Id, button, column, container, mouse_area, operation, rich_text, row, rule,
    space as iced_space, text, text_input,
};
use iced::{Alignment, Element, Length, Padding, Task};

use crate::glyph::{self, GlyphState};
use crate::icon::{Icon, icon};
use crate::keys::on_key;
use crate::layer::{self, Layer};
use crate::matching::{Ranked, Searchable, filter_palette, highlight};
use crate::menu_nav::{Step, step_index};

/// What the palette says, so that the app can translate it. The defaults are
/// English.
#[derive(Debug, Clone)]
pub struct Labels {
    /// The field's placeholder.
    pub placeholder: String,
    /// "navigate", "run" and "close" in the footer.
    pub navigate: String,
    pub run: String,
    pub close: String,
    /// The Escape key's cap, which the web app translated.
    pub esc: String,
    /// What the list says when nothing matches; `{query}` is replaced.
    pub empty: String,
    /// The footer's right end, naming the shortcut that opens the palette
    /// from anywhere. Left out when `None`.
    pub anywhere: Option<String>,
}

impl Default for Labels {
    fn default() -> Self {
        Labels {
            placeholder: String::from("Type a command or search…"),
            navigate: String::from("navigate"),
            run: String::from("run"),
            close: String::from("close"),
            esc: String::from("Esc"),
            empty: String::from("Nothing matches “{query}”"),
            anywhere: None,
        }
    }
}

/// What stands at the start of a row.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Lead {
    #[default]
    None,
    Icon(Icon),
    /// An agent's state, drawn as it is everywhere else.
    State(GlyphState),
}

/// One piece of a row's secondary line.
#[derive(Debug, Clone, PartialEq)]
pub enum Piece {
    Plain(String),
    /// A word in the colour of an agent state ("waiting").
    State(AgentState, String),
}

/// A row's secondary line: where it is, what runs there, how it is doing.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Detail(Vec<Piece>);

impl Detail {
    pub fn new() -> Detail {
        Detail::default()
    }

    pub fn plain(mut self, text: impl Into<String>) -> Detail {
        self.0.push(Piece::Plain(text.into()));
        self
    }

    pub fn state(mut self, state: AgentState, text: impl Into<String>) -> Detail {
        self.0.push(Piece::State(state, text.into()));
        self
    }
}

/// One thing the palette lists. `T` is what the app gets back when it runs.
#[derive(Debug, Clone)]
pub struct Item<T> {
    pub value: T,
    pub group: String,
    pub label: String,
    pub keywords: Vec<String>,
    pub lead: Lead,
    pub detail: Option<Detail>,
    pub keys: Option<String>,
}

impl<T> Item<T> {
    /// An item listed under `group`, found by `label`.
    pub fn new(value: T, group: impl Into<String>, label: impl Into<String>) -> Item<T> {
        Item {
            value,
            group: group.into(),
            label: label.into(),
            keywords: Vec::new(),
            lead: Lead::None,
            detail: None,
            keys: None,
        }
    }

    /// More words it is found by, ranking below every match on the label:
    /// the other language's label, synonyms, the agent a pane runs.
    pub fn keywords<S: Into<String>>(mut self, words: impl IntoIterator<Item = S>) -> Item<T> {
        self.keywords.extend(words.into_iter().map(Into::into));
        self
    }

    pub fn icon(mut self, icon: Icon) -> Item<T> {
        self.lead = Lead::Icon(icon);
        self
    }

    pub fn state(mut self, state: GlyphState) -> Item<T> {
        self.lead = Lead::State(state);
        self
    }

    pub fn detail(mut self, detail: Detail) -> Item<T> {
        self.detail = Some(detail);
        self
    }

    /// The shortcut that does the same, as the settings store it. Empty is
    /// left out.
    pub fn keys(mut self, binding: impl AsRef<str>) -> Item<T> {
        let binding = binding.as_ref().trim();
        self.keys = (!binding.is_empty()).then(|| crate::shortcut::format_binding(binding));
        self
    }
}

impl<T> Searchable for Item<T> {
    fn group(&self) -> &str {
        &self.group
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn keywords(&self) -> &[String] {
        &self.keywords
    }
}

/// What the palette is told.
#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    /// The field changed.
    Query(String),
    Next,
    Previous,
    PageDown,
    PageUp,
    /// The pointer rested on a row (an index into the shown list).
    Hover(usize),
    /// A row was clicked.
    Click(usize),
    /// Enter.
    Run,
    /// Escape, the Esc cap or a click on the veil: hand this to the layer's
    /// `on_dismiss` as well.
    Dismiss,
    /// The list was scrolled to this offset.
    Scrolled(f32),
}

/// What the palette asks of the app.
#[derive(Debug, Clone, PartialEq)]
pub enum Event<T> {
    None,
    /// An item was run. The palette is still open: closing it is the app's
    /// to do, first, so that what the command opens is not opened under it.
    Run(T),
    Dismiss,
}

/// The palette's state: what it lists, what is typed and what is chosen.
#[derive(Debug, Clone)]
pub struct Palette<T> {
    items: Vec<Item<T>>,
    query: String,
    selected: usize,
    scroll: f32,
}

/// The identities the view and its tasks agree on.
fn input_id() -> Id {
    Id::new("hangar-ui-palette-input")
}

fn list_id() -> Id {
    Id::new("hangar-ui-palette-list")
}

/// A row's height (`.palette__option`), and a heading's.
const ROW: f32 = 32.0;
const HEADING: f32 = 27.3;
/// The list's height before it scrolls (`min(436, 58vh)`).
const LIST_HEIGHT: f32 = 436.0;
/// How many rows Page Up and Page Down move.
const PAGE: usize = 8;

impl<T: Clone> Palette<T> {
    pub fn new(items: Vec<Item<T>>) -> Palette<T> {
        Palette {
            items,
            query: String::new(),
            selected: 0,
            scroll: 0.0,
        }
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// Replaces what the palette lists, keeping what is typed and the row
    /// chosen (kept inside the list as it shrinks): for a list that follows
    /// live state while it is open, such as the agents' states.
    pub fn set_items(&mut self, items: Vec<Item<T>>) {
        self.items = items;
    }

    /// The items the query keeps, flattened in the order they are listed.
    fn shown(&self) -> Vec<Ranked<'_, Item<T>>> {
        filter_palette(&self.items, &self.query)
            .into_iter()
            .flat_map(|group| group.entries)
            .collect()
    }

    /// How many rows the query leaves.
    pub fn len(&self) -> usize {
        self.shown().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The row chosen, kept inside the list as the query narrows it.
    pub fn selected(&self) -> usize {
        self.selected.min(self.len().saturating_sub(1))
    }

    /// The item chosen, if the list is not empty.
    pub fn chosen(&self) -> Option<&Item<T>> {
        self.shown().get(self.selected()).map(|ranked| ranked.entry)
    }

    pub fn update(&mut self, message: Msg) -> (Event<T>, Task<Msg>) {
        let count = self.len();
        match message {
            Msg::Query(query) => {
                self.query = query;
                self.selected = 0;
                self.scroll = 0.0;
                (
                    Event::None,
                    operation::scroll_to(list_id(), AbsoluteOffset { x: 0.0, y: 0.0 }),
                )
            }
            Msg::Next | Msg::Previous => {
                let step = if message == Msg::Next {
                    Step::Next
                } else {
                    Step::Previous
                };
                self.go(step_index(Some(self.selected()), count, step, true))
            }
            Msg::PageDown => {
                let at = (self.selected() + PAGE).min(count.saturating_sub(1));
                self.go((count > 0).then_some(at))
            }
            Msg::PageUp => {
                let at = self.selected().saturating_sub(PAGE);
                self.go((count > 0).then_some(at))
            }
            Msg::Hover(at) => {
                if at < count {
                    self.selected = at;
                }
                (Event::None, Task::none())
            }
            Msg::Click(at) => match self.shown().get(at) {
                Some(ranked) => (Event::Run(ranked.entry.value.clone()), Task::none()),
                None => (Event::None, Task::none()),
            },
            Msg::Run => match self.chosen() {
                Some(item) => (Event::Run(item.value.clone()), Task::none()),
                None => (Event::None, Task::none()),
            },
            Msg::Dismiss => (Event::Dismiss, Task::none()),
            Msg::Scrolled(offset) => {
                self.scroll = offset;
                (Event::None, Task::none())
            }
        }
    }

    /// Moves the choice and scrolls it into view.
    fn go(&mut self, to: Option<usize>) -> (Event<T>, Task<Msg>) {
        let Some(to) = to else {
            return (Event::None, Task::none());
        };
        self.selected = to;
        let (top, bottom) = self.extent(to);
        let view = LIST_HEIGHT;
        let offset = if top < self.scroll {
            top
        } else if bottom > self.scroll + view {
            bottom - view
        } else {
            return (Event::None, Task::none());
        };
        self.scroll = offset;
        (
            Event::None,
            operation::scroll_to(list_id(), AbsoluteOffset { x: 0.0, y: offset }),
        )
    }

    /// The vertical extent of the row at `index` in the list, its heading
    /// included when it is the first of its group.
    fn extent(&self, index: usize) -> (f32, f32) {
        let mut y = 4.0;
        let mut at = 0;
        for group in filter_palette(&self.items, &self.query) {
            let heading_top = y;
            y += HEADING;
            for (n, _) in group.entries.iter().enumerate() {
                if at == index {
                    let top = if n == 0 { heading_top } else { y };
                    return (top, y + ROW);
                }
                y += ROW;
                at += 1;
            }
        }
        (0.0, 0.0)
    }

    /// The palette's panel, to put in a [`Layer`] with [`over`].
    pub fn view<'a>(&'a self, t: &'a Theme, labels: &'a Labels) -> Element<'a, Msg> {
        let c = &t.colors;
        let shown = self.shown();
        let selected = self.selected();

        let field = text_input(&labels.placeholder, &self.query)
            .id(input_id())
            .on_input(Msg::Query)
            .on_submit(Msg::Run)
            .size(font_size::ICON_LG)
            .padding(0)
            .style(style::text_input::bare(t));
        let esc = button(
            container(
                text(labels.esc.clone())
                    .size(font_size::LABEL)
                    .font(fonts::mono())
                    .style(style::text::secondary(t)),
            )
            .padding(Padding::from([space::XXS, space::SM]))
            .style(style::container::outline(t)),
        )
        .padding(0)
        .style(style::button::icon(t))
        .on_press(Msg::Dismiss);
        let search = container(
            row![icon(Icon::Search, 18.0).color(c.fg_muted), field, esc,]
                .spacing(space::XL)
                .align_y(Alignment::Center),
        )
        .padding(Padding {
            top: 0.0,
            right: 14.0,
            bottom: 0.0,
            left: space::XXL,
        })
        .height(size::PALETTE_SEARCH_HEIGHT)
        .align_y(Alignment::Center);

        let mut list = column![].padding(Padding {
            top: 4.0,
            right: 6.0,
            bottom: 10.0,
            left: 6.0,
        });
        let mut previous_group: Option<&str> = None;
        for (at, ranked) in shown.iter().enumerate() {
            let item = ranked.entry;
            if previous_group != Some(item.group.as_str()) {
                list = list.push(
                    container(
                        text(item.group.clone())
                            .size(font_size::CAPTION)
                            .font(fonts::ui_semibold())
                            .style(style::text::secondary(t)),
                    )
                    .padding(Padding {
                        top: 9.0,
                        right: space::LG,
                        bottom: 4.0,
                        left: space::LG,
                    }),
                );
                previous_group = Some(item.group.as_str());
            }
            list = list.push(self.row(t, item, ranked, at, at == selected));
        }
        let body: Element<'a, Msg> = if shown.is_empty() {
            container(
                text(labels.empty.replace("{query}", self.query.trim()))
                    .size(font_size::UI)
                    .style(style::text::secondary(t)),
            )
            .padding(Padding::from([26.0, 16.0]))
            .center_x(Length::Fill)
            .into()
        } else {
            list.into()
        };
        let list = scrollable::Scrollable::new(body)
            .id(list_id())
            .on_scroll(|viewport| Msg::Scrolled(viewport.absolute_offset().y))
            .style(style::scrollable::default(t))
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::new().width(0).scroller_width(0),
            ));
        let list = container(list).max_height(LIST_HEIGHT);

        let key = |cap: String, words: String| {
            row![
                text(cap)
                    .size(font_size::LABEL)
                    .font(fonts::mono())
                    .style(style::text::primary(t)),
                text(words)
                    .size(font_size::CAPTION)
                    .style(style::text::secondary(t)),
            ]
            .spacing(5)
            .align_y(Alignment::Center)
        };
        let mut foot = row![
            key("↑↓".into(), labels.navigate.clone()),
            key("↵".into(), labels.run.clone()),
            key(labels.esc.clone(), labels.close.clone()),
        ]
        .spacing(space::XXL)
        .align_y(Alignment::Center);
        if let Some(anywhere) = &labels.anywhere {
            foot = foot.push(iced_space::horizontal());
            foot = foot.push(
                text(anywhere.clone())
                    .size(font_size::CAPTION)
                    .style(style::text::secondary(t)),
            );
        }
        let foot = container(foot)
            .padding(Padding::from([0.0, space::XXL]))
            .height(32.0)
            .align_y(Alignment::Center);

        let line = || rule::horizontal(1).style(style::rule::strong(t));
        let panel = container(column![search, line(), list, line(), foot])
            .width(size::PALETTE_WIDTH)
            .style(style::container::palette(t));

        on_key(panel, |key, modifiers| {
            use iced::keyboard::Key;
            use iced::keyboard::key::Named;
            // A key with Ctrl or Alt is the window's shortcut, not the list's.
            if modifiers.control() || modifiers.alt() || modifiers.logo() {
                return None;
            }
            match key {
                Key::Named(Named::ArrowDown) => Some(Msg::Next),
                Key::Named(Named::ArrowUp) => Some(Msg::Previous),
                Key::Named(Named::PageDown) => Some(Msg::PageDown),
                Key::Named(Named::PageUp) => Some(Msg::PageUp),
                _ => None,
            }
        })
    }

    fn row<'a>(
        &'a self,
        t: &'a Theme,
        item: &'a Item<T>,
        ranked: &Ranked<'_, Item<T>>,
        at: usize,
        chosen: bool,
    ) -> Element<'a, Msg> {
        let c = &t.colors;

        let lead: Element<'a, Msg> = match &item.lead {
            Lead::None => iced_space::horizontal().width(0).into(),
            Lead::Icon(which) => icon(*which, 13.0).into(),
            Lead::State(state) => {
                let glyph = glyph::state(t, *state, 7.0);
                if chosen {
                    glyph.tint(c.accent_fg).into()
                } else {
                    glyph.into()
                }
            }
        };
        let lead = container(lead)
            .center(Length::Fixed(22.0))
            .style(style::container::palette_icon(t, chosen));

        // The label with the letters that matched in bold.
        let bold = fonts::ui_weight(700);
        let spans: Vec<Span<'_, (), iced::Font>> = highlight(&item.label, &ranked.matched.ranges)
            .into_iter()
            .map(|part| {
                let span = Span::new(part.text.to_string());
                if part.hit { span.font(bold) } else { span }
            })
            .collect();
        let label = rich_text(spans).size(font_size::BODY);

        let mut line = row![lead, label]
            .spacing(space::LG)
            .align_y(Alignment::Center);

        if let Some(detail) = &item.detail {
            let spans: Vec<Span<'_, (), iced::Font>> = detail
                .0
                .iter()
                .map(|piece| match piece {
                    Piece::Plain(words) => {
                        let span = Span::new(words.clone());
                        if chosen {
                            span
                        } else {
                            span.color(c.fg_secondary)
                        }
                    }
                    Piece::State(state, words) => {
                        let span = Span::new(words.clone());
                        if chosen {
                            span
                        } else {
                            span.color(c.state(*state).text)
                        }
                    }
                })
                .collect();
            line = line.push(
                rich_text(spans)
                    .size(font_size::UI)
                    .wrapping(text::Wrapping::None)
                    .width(Length::Fill),
            );
        } else {
            line = line.push(iced_space::horizontal());
        }
        if let Some(keys) = &item.keys {
            let keys = text(keys.clone())
                .size(font_size::CAPTION)
                .font(fonts::mono());
            line = line.push(if chosen {
                keys
            } else {
                keys.style(style::text::secondary(t))
            });
        }

        let row = button(container(line).center_y(Length::Fill))
            .padding(Padding::from([0.0, space::LG]))
            .width(Length::Fill)
            .height(ROW)
            .style(style::button::palette_row(t, chosen))
            .on_press(Msg::Click(at));
        mouse_area(row).on_enter(Msg::Hover(at)).into()
    }
}

/// `page` with the palette over it, if it is open.
///
/// `dismiss` is what Escape and a click on the veil send: the palette's
/// [`Msg::Dismiss`], wrapped as the app wraps its messages.
pub fn over<'a, M: Clone + 'a>(
    t: &'a Theme,
    page: impl Into<Element<'a, M>>,
    panel: Option<Element<'a, M>>,
    dismiss: M,
) -> Layer<'a, M> {
    layer::palette(page, panel)
        .veil(style::container::palette_scrim(t))
        .on_dismiss(dismiss)
}

#[cfg(test)]
mod tests;
