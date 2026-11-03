//! The board's surfaces, from the theme's colours and the shared tokens
//! (`styles.css`, "Kanban board", "Board panel", "Task dialog").
//!
//! `hangar_theme::style` has the roles every view shares; the few here are the
//! board's own (a card that is being dragged, the column under the dragged
//! card, a filter that hides something). Each takes the theme and returns the
//! closure iced's `.style(..)` wants, and picks every colour from
//! `theme.colors`.

use hangar_theme::Theme;
use hangar_theme::tokens::{BORDER_WIDTH, radius};
use iced::widget::{button, container, text_input};
use iced::{Background, Border, Color, Shadow, Vector};

/// The dot that marks a column (`.column__mark`) and a task dialog's header.
pub const MARK: f32 = 6.0;
/// The mark in the task dialog's header.
pub const MARK_LARGE: f32 = 7.0;
/// The widest a column grows in the full board (`minmax(0, 360px)`).
pub const COLUMN_MAX_WIDTH: f32 = 360.0;
/// Under this width the full board stacks its columns (`@container (max-width: 640px)`).
pub const STACK_BELOW: f32 = 640.0;
/// Under this width the MCP button loses its words (`@container (max-width: 900px)`).
pub const COMPACT_BELOW: f32 = 900.0;
/// A bar of the full board, and of the dock (`.board__bar`).
pub const BAR_HEIGHT: f32 = 44.0;
pub const DOCK_BAR_HEIGHT: f32 = 30.0;
/// A column's header (`.column__head`), and the dock's.
pub const COLUMN_HEAD: f32 = 34.0;
pub const DOCK_COLUMN_HEAD: f32 = 28.0;
/// The row to add a card in, and the cards' spacing.
pub const ADD_HEIGHT: f32 = 28.0;
/// The dock's grip: the area that takes the pointer, and the line it draws.
pub const GRIP_WIDTH: f32 = 5.0;
pub const GRIP_INSET: f32 = 1.0;
/// The bars of a priority (`.prio__bars i`), their width and the tallest.
pub const PRIO_BAR_WIDTH: f32 = 2.5;
pub const PRIO_BAR_HEIGHT: f32 = 9.0;
/// The skeleton's cards (`.skeleton--card`), and its label.
pub const SKELETON_CARD: f32 = 76.0;
pub const SKELETON_CARD_SHORT: f32 = 58.0;
pub const SKELETON_LABEL: (f32, f32) = (72.0, 9.0);
/// The empty board's icon box (`.board__empty-icon`).
pub const EMPTY_ICON_BOX: f32 = 44.0;
/// The widest the empty board's paragraph runs (`50ch`, at 12.5 px).
pub const EMPTY_TEXT_WIDTH: f32 = 380.0;
/// A task dialog (`.modal--task`), the fields in it, its thread's height.
pub const DIALOG_WIDTH: f32 = 640.0;
pub const FIELD_HEIGHT: f32 = 30.0;
pub const THREAD_MAX_HEIGHT: f32 = 250.0;
/// A dialog's body is cut off here and scrolls (86 vh of a 900 px window,
/// less its head and foot).
pub const DIALOG_BODY_MAX: f32 = 560.0;

/// Text in one of the theme's inks, picked at run time.
pub fn ink(color: Color) -> impl Fn(&iced::Theme) -> iced::widget::text::Style {
    move |_| iced::widget::text::Style { color: Some(color) }
}

pub fn alpha(color: Color, alpha: f32) -> Color {
    Color { a: alpha, ..color }
}

/// `color-mix(in srgb, tint amount%, base)`.
pub fn mix(base: Color, tint: Color, amount: f32) -> Color {
    Color {
        r: base.r + (tint.r - base.r) * amount,
        g: base.g + (tint.g - base.g) * amount,
        b: base.b + (tint.b - base.b) * amount,
        a: base.a + (tint.a - base.a) * amount,
    }
}

fn hairline(color: Color, radius: f32) -> Border {
    Border {
        color,
        width: BORDER_WIDTH,
        radius: radius.into(),
    }
}

fn rounded(radius: f32) -> Border {
    Border {
        radius: radius.into(),
        ..Border::default()
    }
}

/// What the columns sit on: the terminal's own colour on a dark theme, where
/// the cards are lifted off it; the grey desk on a light one, where the cards
/// are the white sheets (`light-dark(var(--surface), var(--bg))`).
pub fn floor(t: &Theme) -> Color {
    if t.is_dark() {
        t.colors.bg
    } else {
        t.colors.surface
    }
}

/// The board itself (`.board`): the page; in the dock, the chrome's colour,
/// like the inspector.
pub fn surface(t: &Theme, in_dock: bool) -> impl Fn(&iced::Theme) -> container::Style + '_ {
    move |_| container::Style {
        background: Some(Background::Color(if in_dock {
            t.colors.chrome
        } else {
            t.colors.bg
        })),
        text_color: Some(t.colors.fg),
        ..container::Style::default()
    }
}

/// A column. The one a dragged card would land in is tinted with the accent
/// and ringed (`.column--over`).
pub fn column(
    t: &Theme,
    over: bool,
    in_dock: bool,
) -> impl Fn(&iced::Theme) -> container::Style + '_ {
    move |_| {
        let c = &t.colors;
        if over {
            return container::Style {
                background: Some(Background::Color(mix(floor(t), c.accent, 0.07))),
                border: hairline(c.accent_ring, radius::NONE),
                text_color: Some(c.fg),
                ..container::Style::default()
            };
        }
        container::Style {
            // The dock's columns are sections on the panel, not columns.
            background: (!in_dock).then(|| Background::Color(floor(t))),
            text_color: Some(c.fg),
            ..container::Style::default()
        }
    }
}

/// A card's states (`.card--done`, `--onhold`, `--dragging`).
#[derive(Debug, Clone, Copy, Default)]
pub struct CardLook {
    pub done: bool,
    pub on_hold: bool,
    pub dragging: bool,
}

/// A card: lifted off the column, a hairline around it. Finished work recedes
/// (no fill), a task on hold is a little quieter, and the one being dragged is
/// dimmed to 40 %.
pub fn card(t: &Theme, look: CardLook) -> impl Fn(&iced::Theme) -> container::Style + '_ {
    move |_| {
        let c = &t.colors;
        let fade = if look.dragging {
            0.4
        } else if look.on_hold {
            0.85
        } else {
            1.0
        };
        let fill = (!look.done).then(|| Background::Color(alpha(c.card, c.card.a * fade)));
        container::Style {
            background: fill,
            border: hairline(alpha(c.line, c.line.a * fade), radius::LG),
            shadow: Shadow::default(),
            text_color: Some(alpha(c.fg, fade)),
            ..container::Style::default()
        }
    }
}

/// The wash over a card under the pointer.
pub fn card_hover(t: &Theme) -> Color {
    alpha(t.colors.fg, 0.04)
}

/// A label on a card (`.chip`).
pub fn chip(t: &Theme) -> impl Fn(&iced::Theme) -> container::Style + '_ {
    move |_| container::Style {
        background: Some(Background::Color(t.colors.line)),
        border: rounded(radius::XS),
        text_color: Some(t.colors.fg_secondary),
        ..container::Style::default()
    }
}

/// The dot of a column or a dialog (`.column__mark`): neutral for what waits,
/// the working colour for what is under way, "your turn" for what waits on
/// the person, and an empty ring for what is over.
pub fn mark(
    t: &Theme,
    column: super::model::Column,
    side: f32,
) -> impl Fn(&iced::Theme) -> container::Style + '_ {
    use super::model::Column;
    move |_| {
        let c = &t.colors;
        let round = (side / 2.0).into();
        match column {
            Column::Done => container::Style {
                border: Border {
                    color: c.fg_subtle,
                    width: 1.5,
                    radius: round,
                },
                ..container::Style::default()
            },
            other => {
                let fill = match other {
                    Column::Doing => c.state_working_fill,
                    Column::Review => c.state_yours_fill,
                    _ => c.fg_subtle,
                };
                container::Style {
                    background: Some(Background::Color(fill)),
                    border: Border {
                        radius: round,
                        ..Border::default()
                    },
                    ..container::Style::default()
                }
            }
        }
    }
}

/// A filter's button (`.board-filter__button`): a tint with a hairline, and
/// the accent when the filter hides cards, so it is seen from across the room
/// and never mistaken for the whole board.
pub fn filter_button(
    t: &Theme,
    active: bool,
) -> impl Fn(&iced::Theme, button::Status) -> button::Style + '_ {
    move |_, status| {
        let c = &t.colors;
        let (background, ring, text) = if active {
            (c.accent_soft, c.accent_ring, c.accent_hi)
        } else {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            (if hovered { c.line_strong } else { c.line }, c.line, c.fg)
        };
        button::Style {
            background: Some(Background::Color(background)),
            text_color: text,
            border: hairline(ring, radius::MD),
            shadow: Shadow::default(),
            ..button::Style::default()
        }
    }
}

/// The "Clear" next to the filters (`.board__clear`): the accent as text,
/// a wash of it under the pointer.
pub fn clear_button(t: &Theme) -> impl Fn(&iced::Theme, button::Status) -> button::Style + '_ {
    move |_, status| {
        let c = &t.colors;
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: hovered.then_some(Background::Color(c.accent_soft)),
            text_color: c.accent_hi,
            border: rounded(radius::MD),
            shadow: Shadow::default(),
            ..button::Style::default()
        }
    }
}

/// The quiet words-and-icon button of the bar (`.board__mcp`).
pub fn quiet_button(t: &Theme) -> impl Fn(&iced::Theme, button::Status) -> button::Style + '_ {
    move |_, status| {
        let c = &t.colors;
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: hovered.then_some(Background::Color(c.line)),
            text_color: if hovered { c.fg_muted } else { c.fg_subtle },
            border: rounded(radius::MD),
            shadow: Shadow::default(),
            ..button::Style::default()
        }
    }
}

/// A button that shows a flag: a dot in the corner of the dock's filter tool
/// when a filter is on while its row is folded away.
pub fn flag(t: &Theme) -> impl Fn(&iced::Theme) -> container::Style + '_ {
    move |_| container::Style {
        background: Some(Background::Color(t.colors.accent)),
        border: Border {
            color: t.colors.pane_head,
            width: 1.5,
            radius: 999.0.into(),
        },
        ..container::Style::default()
    }
}

/// The dock's section header, a button across the row (`.column__toggle`).
pub fn fold_button(t: &Theme) -> impl Fn(&iced::Theme, button::Status) -> button::Style + '_ {
    move |_, status| {
        let c = &t.colors;
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: hovered.then_some(Background::Color(c.line)),
            text_color: c.fg,
            border: rounded(radius::SM),
            shadow: Shadow::default(),
            ..button::Style::default()
        }
    }
}

/// The row at the foot of a column that a card is typed into
/// (`.column__add`): nothing around it until it is pointed at, then the page
/// with the accent around it while it has the keyboard.
pub fn add_field(t: &Theme) -> impl Fn(&iced::Theme, text_input::Status) -> text_input::Style + '_ {
    move |_, status| {
        let c = &t.colors;
        let (background, border) = match status {
            text_input::Status::Focused { .. } => (c.surface, hairline(c.accent, radius::LG)),
            text_input::Status::Hovered => (c.line, rounded(radius::LG)),
            _ => (Color::TRANSPARENT, rounded(radius::LG)),
        };
        text_input::Style {
            background: Background::Color(background),
            border,
            icon: c.fg_muted,
            placeholder: c.fg_subtle,
            value: c.fg,
            selection: c.accent_ring,
        }
    }
}

/// The outline of the first read (`.skeleton`).
pub fn skeleton(t: &Theme) -> impl Fn(&iced::Theme) -> container::Style + '_ {
    move |_| container::Style {
        background: Some(Background::Color(t.colors.line)),
        border: rounded(radius::LG),
        ..container::Style::default()
    }
}

/// The icon in the empty board (`.board__empty-icon`).
pub fn empty_icon(t: &Theme) -> impl Fn(&iced::Theme) -> container::Style + '_ {
    move |_| container::Style {
        background: Some(Background::Color(t.colors.raised)),
        border: hairline(t.colors.line, radius::XL),
        text_color: Some(t.colors.fg_muted),
        ..container::Style::default()
    }
}

/// Why a move was refused, over the board's foot (`.board__toast`).
pub fn toast(t: &Theme) -> impl Fn(&iced::Theme) -> container::Style + '_ {
    move |_| container::Style {
        background: Some(Background::Color(t.colors.chrome_bar)),
        border: hairline(t.colors.line_strong, radius::LG),
        text_color: Some(t.colors.fg),
        shadow: Shadow {
            color: t.colors.shadow,
            offset: Vector::new(0.0, 8.0),
            blur_radius: 24.0,
        },
        ..container::Style::default()
    }
}

/// A bar of lit and unlit priority (`.prio__bars i`).
pub fn prio_bar(t: &Theme, lit: bool) -> impl Fn(&iced::Theme) -> container::Style + '_ {
    move |_| container::Style {
        background: Some(Background::Color(if lit {
            t.colors.fg_muted
        } else {
            t.colors.line_strong
        })),
        border: rounded(1.0),
        ..container::Style::default()
    }
}

/// The list of a task's dependencies, a hairline around rows.
pub fn deps(t: &Theme) -> impl Fn(&iced::Theme) -> container::Style + '_ {
    move |_| container::Style {
        border: hairline(t.colors.line, radius::LG),
        ..container::Style::default()
    }
}

/// A task dialog's own surface (`.modal--task`): the chrome's colour.
pub fn dialog(t: &Theme) -> impl Fn(&iced::Theme) -> container::Style + '_ {
    hangar_theme::style::container::modal(t)
}
