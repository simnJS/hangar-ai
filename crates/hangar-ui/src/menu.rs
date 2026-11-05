//! Menus: a list of lines that run something when picked, for a button's
//! drop-down or a right click.
//!
//! A [`Menu`] is the list; where it appears and how it goes away is a
//! [`Layer`]'s business, so that the same list serves as
//! a drop-down under a button ([`dropdown`]) and as a context menu at the
//! pointer ([`context_menu`]).
//!
//! ```ignore
//! use hangar_ui::menu::{self, item, separator, heading};
//!
//! // Under a "…" button; the view owns whether it is open.
//! let entries = vec![
//!     heading("Pane"),
//!     item("Split right").icon(Icon::SplitRight).shortcut("Ctrl+D").on_select(Msg::Split).into(),
//!     item("Restart").icon(Icon::Restart).on_select(Msg::Restart).into(),
//!     separator(),
//!     item("Close").icon(Icon::Close).danger().on_select(Msg::Close).into(),
//! ];
//! menu::dropdown(t, more_button, self.menu_open.then_some(entries))
//!     .on_dismiss(Msg::CloseMenu)
//!
//! // At the pointer, over a row of the navigator:
//! menu::context_menu(t, row, self.menu_at(id), || entries(id))
//!     .on_open(move |at| Msg::OpenMenu(id, at))
//!     .on_dismiss(Msg::CloseMenu)
//! ```
//!
//! What the menu does, as the web app's did:
//!
//! - One highlight, moved by the pointer and the arrow keys alike, and gone
//!   when the pointer leaves, as a native menu's is. Up and Down wrap round
//!   the ends (`menuNav.ts`); Home and End jump; Enter and Space pick.
//! - A line can be disabled, greyed and skipped by the arrows; given a
//!   reason, it stays reachable and says why in a tip beside it, so that
//!   someone using the keyboard finds out too.
//! - A line can be a choice among several, ticked when it is the current one;
//!   can carry a shortcut, right-aligned; can be dangerous, in the danger
//!   colour; and can carry small actions (edit, remove) that appear while it
//!   is highlighted, as the saved-commands menu's rows do.
//! - Headings and separators group the lines and take no highlight.
//! - A menu taller than its limit scrolls with the wheel and with the keys.
//!
//! The list is drawn by one widget rather than built of buttons, because the
//! highlight is shared state between the pointer and the keyboard.

use hangar_theme::tokens::{font_size, radius, size};
use hangar_theme::{Theme, fonts, style};
use iced::advanced::layout::{self, Layout};
use iced::advanced::overlay;
use iced::advanced::renderer::{self, Quad};
use iced::advanced::text::{self, Renderer as _, Text};
use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Clipboard, Renderer as _, Shell, Widget};
use iced::keyboard::{self, Key, key::Named};
use iced::widget::container;
use iced::{
    Background, Border, Color, Element, Event, Font, Length, Pixels, Point, Radians, Rectangle,
    Size, Vector, alignment, mouse,
};

use crate::glyph::{self, GlyphState, Tint};
use crate::icon::{self, Icon};
use crate::layer::{self, Layer};
use crate::menu_nav::{Move, menu_move, move_selectable};
use crate::text_metrics::text_width;

/// The menu's inner padding around its lines.
const PAD: f32 = 5.0;
/// A line's height (`.ctx-menu__item`).
const LINE: f32 = size::MENU_ITEM_HEIGHT;
/// A heading's height.
const HEADING: f32 = 24.0;
/// A separator's height: a hairline with 4 px above and below.
const SEPARATOR: f32 = 9.0;
/// The space inside a line at both ends.
const INSET: f32 = 8.0;
/// The gap between a line's parts.
const GAP: f32 = 8.0;
/// The icon column.
const ICON: f32 = size::ICON;
/// A line's small action buttons.
const ACTION: f32 = 20.0;

/// One line of a menu.
#[must_use]
pub struct Item<M> {
    label: String,
    /// The label is code, set in the mono face: a saved command with no name.
    mono: bool,
    /// A second, quieter text after the label, in the mono face: the command
    /// a saved command runs.
    detail: Option<String>,
    icon: Option<Icon>,
    glyph: Option<GlyphState>,
    shortcut: Option<String>,
    checked: Option<bool>,
    danger: bool,
    disabled: bool,
    reason: Option<String>,
    on_select: Option<M>,
    actions: Vec<Action<M>>,
}

/// A small button at the end of a line, shown while the line is highlighted.
#[derive(Debug, Clone)]
pub struct Action<M> {
    pub icon: Icon,
    /// What it does, for the tip. Not shown by the menu itself.
    pub label: String,
    pub message: M,
    /// Takes the danger colour under the pointer: a remove.
    pub danger: bool,
}

/// A line that runs `on_select` when picked.
pub fn item<M>(label: impl Into<String>) -> Item<M> {
    Item {
        label: label.into(),
        mono: false,
        detail: None,
        icon: None,
        glyph: None,
        shortcut: None,
        checked: None,
        danger: false,
        disabled: false,
        reason: None,
        on_select: None,
        actions: Vec::new(),
    }
}

impl<M> Item<M> {
    /// Sets the label in the mono face, for a line whose label is code.
    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }

    /// A second text after the label, smaller, quieter and in the mono face,
    /// as the saved-commands menu shows the command under a name. Empty is
    /// left out.
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        self.detail = (!detail.is_empty()).then_some(detail);
        self
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// An agent's state, drawn where an icon would be.
    pub fn glyph(mut self, state: GlyphState) -> Self {
        self.glyph = Some(state);
        self
    }

    /// The key that does the same, written as the settings store it
    /// (`"Ctrl+D"`). It is shown in the platform's spelling, right-aligned,
    /// and left out when empty so a caller can pass whatever the keymap says.
    pub fn shortcut(mut self, binding: impl AsRef<str>) -> Self {
        let binding = binding.as_ref().trim();
        self.shortcut = (!binding.is_empty()).then(|| crate::shortcut::format_binding(binding));
        self
    }

    /// A choice among several: ticked when `checked`.
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }

    /// Destroys something: drawn in the danger colour.
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    /// Greyed, skipped by the arrow keys, does nothing.
    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }

    /// Greyed, and says why in a tip beside it. Such a line stays in the
    /// arrow keys' walk, doing nothing, so the keyboard reaches the reason
    /// too.
    pub fn disabled_because(mut self, reason: impl Into<String>) -> Self {
        self.disabled = true;
        self.reason = Some(reason.into());
        self
    }

    pub fn on_select(mut self, message: M) -> Self {
        self.on_select = Some(message);
        self
    }

    /// A small button that appears at the end of the line while it is
    /// highlighted, in place of the shortcut.
    pub fn action(mut self, icon: Icon, label: impl Into<String>, message: M) -> Self {
        self.actions.push(Action {
            icon,
            label: label.into(),
            message,
            danger: false,
        });
        self
    }

    /// Like [`action`](Item::action), and dangerous.
    pub fn danger_action(mut self, icon: Icon, label: impl Into<String>, message: M) -> Self {
        self.actions.push(Action {
            icon,
            label: label.into(),
            message,
            danger: true,
        });
        self
    }

    fn label_font(&self) -> Font {
        if self.mono {
            fonts::mono()
        } else {
            fonts::ui()
        }
    }

    /// The highlight can land on it.
    fn selectable(&self) -> bool {
        !self.disabled || self.reason.is_some()
    }

    /// Picking it does something.
    fn live(&self) -> bool {
        !self.disabled && self.on_select.is_some()
    }
}

/// A line, a heading or a separator.
pub enum Entry<M> {
    Item(Item<M>),
    /// A caption over the lines that follow (`.ctx-menu__head`).
    Heading(String),
    Separator,
}

impl<M> From<Item<M>> for Entry<M> {
    fn from(item: Item<M>) -> Self {
        Entry::Item(item)
    }
}

/// A caption over the lines that follow.
pub fn heading<M>(label: impl Into<String>) -> Entry<M> {
    Entry::Heading(label.into())
}

/// A hairline between two groups of lines.
pub fn separator<M>() -> Entry<M> {
    Entry::Separator
}

impl<M> Entry<M> {
    fn height(&self) -> f32 {
        match self {
            Entry::Item(_) => LINE,
            Entry::Heading(_) => HEADING,
            Entry::Separator => SEPARATOR,
        }
    }

    fn selectable(&self) -> bool {
        matches!(self, Entry::Item(item) if item.selectable())
    }
}

/// The list. See the [module](self).
#[must_use]
pub struct Menu<'a, M> {
    theme: &'a Theme,
    entries: Vec<Entry<M>>,
    min_width: f32,
    max_width: f32,
    max_height: f32,
    from_keyboard: bool,
}

/// A menu of these entries.
pub fn menu<M>(t: &Theme, entries: Vec<Entry<M>>) -> Menu<'_, M> {
    Menu {
        theme: t,
        entries,
        min_width: size::MENU_MIN_WIDTH,
        max_width: size::MENU_MAX_WIDTH,
        max_height: 480.0,
        from_keyboard: false,
    }
}

impl<M> Menu<'_, M> {
    /// The bounds the width follows its widest line between: 200 to 280 for a
    /// context menu, 232 to 320 for a pane's.
    pub fn width_between(mut self, min: f32, max: f32) -> Self {
        self.min_width = min;
        self.max_width = max;
        self
    }

    /// Past this height the menu scrolls (60 % of the window in the web app).
    pub fn max_height(mut self, max_height: f32) -> Self {
        self.max_height = max_height;
        self
    }

    /// Opened from the keyboard: the first line takes the highlight at once,
    /// so that the arrows work straight away. Opened by a pointer, nothing is
    /// highlighted until it moves.
    pub fn from_keyboard(mut self, from_keyboard: bool) -> Self {
        self.from_keyboard = from_keyboard;
        self
    }

    fn content_height(&self) -> f32 {
        2.0 * PAD + self.entries.iter().map(Entry::height).sum::<f32>()
    }

    fn selectable(&self) -> Vec<bool> {
        self.entries.iter().map(Entry::selectable).collect()
    }

    /// Whether any line has something in the icon column, so that the labels
    /// line up whether or not each has its own.
    fn has_icons(&self) -> bool {
        self.entries.iter().any(|entry| {
            matches!(entry, Entry::Item(item) if item.icon.is_some() || item.glyph.is_some())
        })
    }

    fn has_checks(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| matches!(entry, Entry::Item(item) if item.checked.is_some()))
    }

    /// The line under a point of the menu's own coordinates, and the action
    /// of it, if the point is on one.
    fn hit(
        &self,
        point: Point,
        scroll: f32,
        width: f32,
        highlighted: Option<usize>,
    ) -> Option<Hit> {
        let y = point.y + scroll - PAD;
        if y < 0.0 || point.x < PAD || point.x > width - PAD {
            return None;
        }
        let mut top = 0.0;
        for (index, entry) in self.entries.iter().enumerate() {
            let height = entry.height();
            if y < top + height {
                let action = match entry {
                    Entry::Item(item) if highlighted == Some(index) => {
                        let first = width - PAD - 4.0 - ACTION * item.actions.len() as f32;
                        (point.x >= first && !item.actions.is_empty())
                            .then(|| ((point.x - first) / ACTION) as usize)
                            .filter(|at| *at < item.actions.len())
                    }
                    _ => None,
                };
                return Some(Hit { index, action });
            }
            top += height;
        }
        None
    }

    /// The vertical extent of a line, in the menu's own (unscrolled)
    /// coordinates.
    fn extent(&self, index: usize) -> (f32, f32) {
        let top: f32 = PAD + self.entries[..index].iter().map(Entry::height).sum::<f32>();
        (top, self.entries[index].height())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Hit {
    index: usize,
    action: Option<usize>,
}

#[derive(Debug, Default)]
struct State {
    highlighted: Option<usize>,
    pressed: Option<Hit>,
    scroll: f32,
}

impl<M: Clone> Menu<'_, M> {
    /// The width the menu takes: its widest line, kept between the bounds.
    fn natural_width(&self) -> f32 {
        let icons = if self.has_icons() { ICON + GAP } else { 0.0 };
        let checks = if self.has_checks() { GAP + ICON } else { 0.0 };
        let widest = self
            .entries
            .iter()
            .map(|entry| match entry {
                Entry::Item(item) => {
                    let label = text_width(&item.label, font_size::BODY, item.label_font())
                        + item.detail.as_ref().map_or(0.0, |detail| {
                            GAP + text_width(detail, font_size::LABEL, fonts::mono())
                        });
                    let end = if item.actions.is_empty() {
                        item.shortcut.as_ref().map_or(0.0, |keys| {
                            // The gap before the keys is wider than the one
                            // between an icon and its label.
                            18.0 + text_width(keys, font_size::LABEL, fonts::mono())
                        })
                    } else {
                        GAP + (ACTION * item.actions.len() as f32)
                    };
                    INSET + icons + label + end + checks + INSET
                }
                Entry::Heading(label) => {
                    INSET + text_width(label, font_size::CAPTION, fonts::ui_semibold()) + INSET
                }
                Entry::Separator => 0.0,
            })
            .fold(0.0, f32::max);
        (2.0 * PAD + widest).clamp(self.min_width, self.max_width)
    }

    fn visible_height(&self) -> f32 {
        self.content_height().min(self.max_height)
    }

    /// How far the content can scroll in a box `view` tall: the height the
    /// menu was really given, which a short window keeps under
    /// `visible_height`.
    fn max_scroll(&self, view: f32) -> f32 {
        (self.content_height() - view).max(0.0)
    }

    /// Scrolls just far enough for a line to be in view.
    fn reveal(&self, state: &mut State, index: usize, view: f32) {
        let (top, height) = self.extent(index);
        if top - PAD < state.scroll {
            state.scroll = (top - PAD).max(0.0);
        } else if top + height + PAD > state.scroll + view {
            state.scroll = top + height + PAD - view;
        }
        state.scroll = state.scroll.clamp(0.0, self.max_scroll(view));
    }

    /// What picking the line (or one of its actions) sends.
    fn message_of(&self, hit: Hit) -> Option<M> {
        let Entry::Item(item) = self.entries.get(hit.index)? else {
            return None;
        };
        match hit.action {
            Some(action) => item.actions.get(action).map(|a| a.message.clone()),
            None if item.live() => item.on_select.clone(),
            None => None,
        }
    }
}

impl<M: Clone> Widget<M, iced::Theme, iced::Renderer> for Menu<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        let highlighted = if self.from_keyboard {
            move_selectable(None, &self.selectable(), Move::First, true)
        } else {
            None
        };
        tree::State::new(State {
            highlighted,
            ..State::default()
        })
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Shrink)
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let width = self.natural_width().min(limits.max().width);
        let height = self.visible_height().min(limits.max().height);
        layout::Node::new(limits.resolve(Length::Shrink, Length::Shrink, Size::new(width, height)))
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
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();
        // The entries may have shrunk under a scrolled menu.
        state.scroll = state.scroll.clamp(0.0, self.max_scroll(bounds.height));
        let at = cursor.position_in(bounds);
        let hit =
            at.and_then(|point| self.hit(point, state.scroll, bounds.width, state.highlighted));

        match event {
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let under = hit
                    .map(|hit| hit.index)
                    .filter(|index| self.entries[*index].selectable());
                // Only the pointer moving moves the highlight: a keyboard's
                // choice must survive redraws under a resting pointer.
                if under != state.highlighted && (at.is_some() || state.highlighted.is_some()) {
                    state.highlighted = under;
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::CursorLeft) => {
                if state.highlighted.take().is_some() {
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) if hit.is_some() => {
                state.pressed = hit;
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if let Some(pressed) = state.pressed.take() {
                    // A click is a press and a release on the same line.
                    if hit == Some(pressed)
                        && let Some(message) = self.message_of(pressed)
                    {
                        shell.publish(message);
                    }
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if at.is_some() => {
                if self.max_scroll(bounds.height) > 0.0 {
                    let lines = match delta {
                        mouse::ScrollDelta::Lines { y, .. } => y * LINE,
                        mouse::ScrollDelta::Pixels { y, .. } => *y,
                    };
                    state.scroll =
                        (state.scroll - lines).clamp(0.0, self.max_scroll(bounds.height));
                    shell.request_redraw();
                }
                shell.capture_event();
            }
            Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) => {
                if let Some(movement) = menu_move(key) {
                    let next =
                        move_selectable(state.highlighted, &self.selectable(), movement, true);
                    if let Some(next) = next {
                        state.highlighted = Some(next);
                        self.reveal(state, next, bounds.height);
                    }
                    shell.request_redraw();
                    shell.capture_event();
                } else if matches!(key, Key::Named(Named::Enter | Named::Space))
                    && let Some(index) = state.highlighted
                {
                    let hit = Hit {
                        index,
                        action: None,
                    };
                    if let Some(message) = self.message_of(hit) {
                        shell.publish(message);
                    }
                    shell.capture_event();
                }
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Idle
        } else {
            mouse::Interaction::None
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let c = &self.theme.colors;
        let scroll = state.scroll.clamp(0.0, self.max_scroll(bounds.height));

        container::draw_background(
            renderer,
            &style::container::popover(self.theme)(theme),
            bounds,
        );

        let inner = Rectangle {
            x: bounds.x,
            y: bounds.y + 1.0,
            width: bounds.width,
            height: bounds.height - 2.0,
        };
        let icons = self.has_icons();
        let checks = self.has_checks();
        renderer.with_layer(inner, |renderer| {
            let mut top = bounds.y + PAD - scroll;
            for (index, entry) in self.entries.iter().enumerate() {
                let height = entry.height();
                let row = Rectangle {
                    x: bounds.x + PAD,
                    y: top,
                    width: bounds.width - 2.0 * PAD,
                    height,
                };
                top += height;
                if row.y + row.height < inner.y || row.y > inner.y + inner.height {
                    continue;
                }
                match entry {
                    Entry::Separator => {
                        let line = Rectangle {
                            x: row.x + INSET - PAD + 3.0,
                            y: row.y + (height - 1.0) / 2.0,
                            width: row.width - 2.0 * (INSET - PAD + 3.0),
                            height: 1.0,
                        };
                        renderer.fill_quad(
                            Quad {
                                bounds: line,
                                ..Quad::default()
                            },
                            Background::Color(c.line_strong),
                        );
                    }
                    Entry::Heading(label) => {
                        draw_text(
                            renderer,
                            label,
                            font_size::CAPTION,
                            fonts::ui_semibold(),
                            c.fg_muted,
                            Line {
                                anchor: Point::new(row.x + INSET, row.y + (height + 2.0) / 2.0),
                                align: alignment::Horizontal::Left,
                                clip: row,
                            },
                        );
                    }
                    Entry::Item(item) => {
                        let on = state.highlighted == Some(index);
                        self.draw_item(renderer, item, row, on, icons, checks);
                    }
                }
            }
        });
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        _renderer: &iced::Renderer,
        _viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, iced::Theme, iced::Renderer>> {
        let state = tree.state.downcast_ref::<State>();
        let index = state.highlighted?;
        let Entry::Item(item) = self.entries.get(index)? else {
            return None;
        };
        let reason = item.reason.clone()?;
        let bounds = layout.bounds();
        let (top, height) = self.extent(index);
        let line = Rectangle {
            x: bounds.x + translation.x,
            y: bounds.y + translation.y + top
                - state.scroll.clamp(0.0, self.max_scroll(bounds.height)),
            width: bounds.width,
            height,
        };
        Some(overlay::Element::new(Box::new(ReasonTip {
            theme: self.theme,
            reason,
            line,
        })))
    }
}

/// Where a text sits: `anchor` is its left, centre or right edge by `align`,
/// at the vertical middle of its line, and nothing of it is drawn outside
/// `clip`.
struct Line {
    anchor: Point,
    align: alignment::Horizontal,
    clip: Rectangle,
}

fn draw_text(
    renderer: &mut iced::Renderer,
    content: &str,
    size: f32,
    font: Font,
    color: Color,
    line: Line,
) {
    let Line {
        anchor,
        align,
        clip,
    } = line;
    renderer.fill_text(
        Text {
            content: content.to_string(),
            bounds: Size::new(clip.width, clip.height),
            size: Pixels(size),
            line_height: text::LineHeight::default(),
            font,
            align_x: align.into(),
            align_y: alignment::Vertical::Center,
            shaping: text::Shaping::Advanced,
            wrapping: text::Wrapping::None,
        },
        anchor,
        color,
        clip,
    );
}

impl<M: Clone> Menu<'_, M> {
    fn draw_item(
        &self,
        renderer: &mut iced::Renderer,
        item: &Item<M>,
        row: Rectangle,
        highlighted: bool,
        icons: bool,
        checks: bool,
    ) {
        let c = &self.theme.colors;
        let (background, text_color, icon_color, keys_color) = if highlighted && item.live() {
            (
                Some(c.accent),
                c.accent_fg,
                c.accent_fg,
                c.accent_fg.scale_alpha(0.75),
            )
        } else if highlighted {
            // Greyed with a reason: lit enough to show it was reached, never
            // enough to look pressable.
            (Some(c.surface_hover), c.fg_subtle, c.fg_subtle, c.fg_subtle)
        } else if item.disabled {
            (None, c.fg_subtle, c.fg_subtle, c.fg_subtle)
        } else if item.danger {
            (None, c.danger, c.danger, c.fg_muted)
        } else {
            (None, c.fg, c.fg_muted, c.fg_muted)
        };

        if let Some(background) = background {
            renderer.fill_quad(
                Quad {
                    bounds: row,
                    border: Border {
                        radius: radius::SM.into(),
                        ..Border::default()
                    },
                    ..Quad::default()
                },
                Background::Color(background),
            );
        }

        let middle = row.y + row.height / 2.0;
        let mut x = row.x + INSET;
        if icons {
            let slot = Rectangle::new(Point::new(x, middle - ICON / 2.0), Size::new(ICON, ICON));
            if let Some(which) = item.icon {
                icon::draw(renderer, which, slot, icon_color, Radians(0.0));
            } else if let Some(state) = item.glyph {
                let side = 7.0;
                let glyph_slot = Rectangle::new(
                    Point::new(slot.center_x() - side / 2.0, middle - side / 2.0),
                    Size::new(side, side),
                );
                let tint = if highlighted && item.live() {
                    Tint { fill: icon_color }
                } else {
                    Tint {
                        fill: if item.disabled {
                            icon_color
                        } else {
                            glyph_tint(self.theme, state)
                        },
                    }
                };
                glyph::draw(renderer, state, glyph_slot, tint, false);
            }
            x += ICON + GAP;
        }

        // From the right edge back: the tick, then the actions or the keys.
        let mut end = row.x + row.width - INSET;
        if checks {
            if item.checked == Some(true) {
                let slot = Rectangle::new(
                    Point::new(end - ICON, middle - ICON / 2.0),
                    Size::new(ICON, ICON),
                );
                let tick = if highlighted && item.live() {
                    c.accent_fg
                } else {
                    c.fg_muted
                };
                icon::draw(renderer, Icon::Check, slot, tick, Radians(0.0));
            }
            end -= ICON + GAP;
        }

        let showing_actions = highlighted && !item.actions.is_empty();
        if showing_actions {
            let first = row.x + row.width - 4.0 - ACTION * item.actions.len() as f32;
            for (at, action) in item.actions.iter().enumerate() {
                let slot = Rectangle::new(
                    Point::new(first + ACTION * at as f32, middle - ACTION / 2.0),
                    Size::new(ACTION, ACTION),
                );
                icon::draw(
                    renderer,
                    action.icon,
                    Rectangle::new(
                        Point::new(slot.center_x() - 6.0, middle - 6.0),
                        Size::new(12.0, 12.0),
                    ),
                    text_color,
                    Radians(0.0),
                );
            }
            end = first - GAP;
        } else if let Some(keys) = &item.shortcut {
            draw_text(
                renderer,
                keys,
                font_size::LABEL,
                fonts::mono(),
                keys_color,
                Line {
                    anchor: Point::new(end, middle),
                    align: alignment::Horizontal::Right,
                    clip: row,
                },
            );
            end -= 18.0 + text_width(keys, font_size::LABEL, fonts::mono());
        }

        let room = Rectangle {
            x,
            y: row.y,
            width: (end - x).max(0.0),
            height: row.height,
        };
        draw_text(
            renderer,
            &item.label,
            font_size::BODY,
            item.label_font(),
            text_color,
            Line {
                anchor: Point::new(x, middle),
                align: alignment::Horizontal::Left,
                clip: room,
            },
        );
        if let Some(detail) = &item.detail {
            let from = x + text_width(&item.label, font_size::BODY, item.label_font()) + GAP;
            if from < end {
                draw_text(
                    renderer,
                    detail,
                    font_size::LABEL,
                    fonts::mono(),
                    keys_color,
                    Line {
                        anchor: Point::new(from, middle),
                        align: alignment::Horizontal::Left,
                        clip: Rectangle {
                            x: from,
                            width: end - from,
                            ..room
                        },
                    },
                );
            }
        }
    }
}

/// The colour a state glyph keeps in a menu: its own.
fn glyph_tint(t: &Theme, state: GlyphState) -> Color {
    Tint::of(t, state).fill
}

impl<'a, M: Clone + 'a> From<Menu<'a, M>> for Element<'a, M> {
    fn from(menu: Menu<'a, M>) -> Self {
        Element::new(menu)
    }
}

/// The tip beside a greyed line that says why it is.
struct ReasonTip<'a> {
    theme: &'a Theme,
    reason: String,
    /// The line it is about, in the window's coordinates.
    line: Rectangle,
}

impl<M> overlay::Overlay<M, iced::Theme, iced::Renderer> for ReasonTip<'_> {
    fn layout(&mut self, _renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        let width = text_width(&self.reason, font_size::CAPTION_LG, fonts::ui())
            .min(bounds.width * 0.5)
            + 16.0;
        let height = font_size::CAPTION_LG * 1.3 + 8.0;
        // Beside the menu, on the right; on the left when the window ends there.
        let right = self.line.x + self.line.width + 6.0;
        let x = if right + width + 8.0 <= bounds.width {
            right
        } else {
            (self.line.x - 6.0 - width).max(8.0)
        };
        let y = (self.line.y + (self.line.height - height) / 2.0)
            .clamp(8.0, (bounds.height - height - 8.0).max(8.0));
        layout::Node::new(Size::new(width, height)).move_to(Point::new(x, y))
    }

    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
    ) {
        let bounds = layout.bounds();
        container::draw_background(
            renderer,
            &style::container::tooltip(self.theme)(theme),
            bounds,
        );
        draw_text(
            renderer,
            &self.reason,
            font_size::CAPTION_LG,
            fonts::ui(),
            self.theme.colors.fg,
            Line {
                anchor: Point::new(bounds.x + 8.0, bounds.center_y()),
                align: alignment::Horizontal::Left,
                clip: bounds,
            },
        );
    }

    fn index(&self) -> f32 {
        20.0
    }
}

// ---- Putting a menu somewhere -----------------------------------------------------------------

/// A menu under a button, open while `entries` is `Some`. Escape, Tab and a
/// press anywhere else ask for [`on_dismiss`](Layer::on_dismiss); picking a
/// line sends its message, and closing is the view's to do on that message.
pub fn dropdown<'a, M: Clone + 'a>(
    t: &'a Theme,
    anchor: impl Into<Element<'a, M>>,
    entries: Option<Vec<Entry<M>>>,
) -> Layer<'a, M> {
    layer::dropdown(anchor, entries.map(|entries| menu(t, entries).into()))
}

/// What a popover holds when it is not a list of lines: a question with
/// buttons, a small form. A card on the popover's surface, for
/// [`layer::dropdown`] (with [`Layer::tab_inside`] and [`Layer::autofocus`]
/// if it has fields).
pub fn panel<'a, M: 'a>(t: &'a Theme, content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    container(content)
        .padding(iced::Padding::new(10.0))
        .style(style::container::popover(t))
        .into()
}

/// A context menu: `row` is where it is asked for with the right button, `at`
/// is where it is open (the pointer's offset from the row's corner), and
/// `entries` builds its lines.
#[must_use]
pub struct ContextMenu<'a, M> {
    row: Element<'a, M>,
    /// Where it is open, and what is in it, or `None` while it is closed.
    open: Option<(Vector, Menu<'a, M>)>,
    on_dismiss: Option<M>,
    on_open: Option<Box<dyn Fn(Vector) -> M + 'a>>,
}

pub fn context_menu<'a, M: Clone + 'a>(
    t: &'a Theme,
    row: impl Into<Element<'a, M>>,
    at: Option<Vector>,
    entries: impl FnOnce() -> Vec<Entry<M>>,
) -> ContextMenu<'a, M> {
    ContextMenu {
        row: row.into(),
        open: at.map(|at| (at, menu(t, entries()))),
        on_dismiss: None,
        on_open: None,
    }
}

impl<'a, M: Clone + 'a> ContextMenu<'a, M> {
    /// Opened from the keyboard (the menu key on a focused row): the first
    /// line has the highlight at once, so that the arrows work straight away.
    pub fn from_keyboard(mut self, from_keyboard: bool) -> Self {
        self.open = self
            .open
            .map(|(at, menu)| (at, menu.from_keyboard(from_keyboard)));
        self
    }

    /// Sent when the menu is asked for: the right button pressed on the row.
    /// The offset is the pointer's from the row's corner; give it back as
    /// `at` to open the menu there.
    pub fn on_open(mut self, on_open: impl Fn(Vector) -> M + 'a) -> Self {
        self.on_open = Some(Box::new(on_open));
        self
    }

    /// Sent when Escape, Tab or a press outside asks for the menu to close.
    pub fn on_dismiss(mut self, message: M) -> Self {
        self.on_dismiss = Some(message);
        self
    }
}

/// Asks for a context menu when the right button goes down over its content.
struct OpenOnRightClick<'a, M> {
    inner: Element<'a, M>,
    on_open: Box<dyn Fn(Vector) -> M + 'a>,
}

impl<M: Clone> Widget<M, iced::Theme, iced::Renderer> for OpenOnRightClick<'_, M> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.inner)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[self.inner.as_widget()]);
    }

    fn size(&self) -> Size<Length> {
        self.inner.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.inner.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.inner
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
        self.inner.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) = event
            && let Some(point) = cursor.position_in(layout.bounds())
        {
            shell.publish((self.on_open)(Vector::new(point.x, point.y)));
            shell.capture_event();
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
        self.inner.as_widget().mouse_interaction(
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
        self.inner.as_widget().draw(
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
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        self.inner
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
        self.inner.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, M: Clone + 'a> From<ContextMenu<'a, M>> for Element<'a, M> {
    fn from(menu: ContextMenu<'a, M>) -> Self {
        let mut layer = match menu.open {
            Some((at, list)) => layer::context(menu.row, Some(list.into()), at),
            None => layer::context(menu.row, None, Vector::ZERO),
        };
        if let Some(message) = menu.on_dismiss {
            layer = layer.on_dismiss(message);
        }
        let layer: Element<'a, M> = layer.into();
        match menu.on_open {
            Some(on_open) => Element::new(OpenOnRightClick {
                inner: layer,
                on_open,
            }),
            None => layer,
        }
    }
}

#[cfg(test)]
mod tests;
