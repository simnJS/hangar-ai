//! Buttons (`.btn`, `.nav-tb`, `.segmented__btn`, `.ctx-menu__item`). Use as
//! `button(..).style(style::button::primary(t))`.
//!
//! A pressed button goes one step further than a hovered one, which the web
//! app did not show (it had `:active` on none of these); iced reports the
//! press, and a button that does not answer it feels dead.

use iced::widget::button::{Status, Style};
use iced::{Background, Border, Color, Shadow};

use super::{brightness, drop_shadow, faded, faded_border, hairline, rounded, with_alpha};
use crate::Theme;
use crate::tokens::radius;

/// A button's look in each state, before the disabled one is derived from the
/// resting one.
struct Look {
    background: Option<Color>,
    text: Color,
    border: Border,
    shadow: Shadow,
}

impl Look {
    fn style(self) -> Style {
        Style {
            background: self.background.map(Background::Color),
            text_color: self.text,
            border: self.border,
            shadow: self.shadow,
            ..Style::default()
        }
    }

    /// A disabled button: the resting look at 45 % opacity, with no shadow.
    fn disabled(self) -> Style {
        Style {
            background: self.background.map(|color| Background::Color(faded(color))),
            text_color: faded(self.text),
            border: faded_border(self.border),
            shadow: Shadow::default(),
            ..Style::default()
        }
    }
}

/// Resolves a button's four states from a function of the interaction.
fn by_status(status: Status, rest: Look, hover: Look, press: Look) -> Style {
    match status {
        Status::Active => rest.style(),
        Status::Hovered => hover.style(),
        Status::Pressed => press.style(),
        Status::Disabled => rest.disabled(),
    }
}

/// The primary action: the accent, with the text on it that reads
/// (`.btn--primary`). It brightens under the pointer and dims when pressed.
pub fn primary(t: &Theme) -> impl Fn(&iced::Theme, Status) -> Style + '_ {
    move |_, status| {
        let c = &t.colors;
        let look = |background: Color| Look {
            background: Some(background),
            text: c.accent_fg,
            border: rounded(radius::MD),
            shadow: drop_shadow(Color::from_rgba(0.0, 0.0, 0.0, 0.3), 1.0, 2.0),
        };
        by_status(
            status,
            look(c.accent),
            look(brightness(c.accent, 1.08)),
            look(brightness(c.accent, 0.94)),
        )
    }
}

/// The ordinary button (`.btn`): raised, with a stronger hairline.
pub fn secondary(t: &Theme) -> impl Fn(&iced::Theme, Status) -> Style + '_ {
    move |_, status| {
        let c = &t.colors;
        let look = |background: Color, border: Color| Look {
            background: Some(background),
            text: c.fg,
            border: hairline(border, radius::MD),
            shadow: Shadow::default(),
        };
        by_status(
            status,
            look(c.raised, c.line_strong),
            look(c.surface_hover, c.border_strong),
            look(c.surface_active, c.border_strong),
        )
    }
}

/// A button with nothing around it until it is pointed at (`.btn--ghost`).
pub fn ghost(t: &Theme) -> impl Fn(&iced::Theme, Status) -> Style + '_ {
    move |_, status| {
        let c = &t.colors;
        let look = |background: Option<Color>| Look {
            background,
            text: c.fg,
            border: rounded(radius::MD),
            shadow: Shadow::default(),
        };
        by_status(
            status,
            look(None),
            look(Some(c.surface_hover)),
            look(Some(c.surface_active)),
        )
    }
}

/// A destructive action (`.btn--danger`): the danger colour in the text and
/// the outline, and a tint of it under the pointer.
pub fn danger(t: &Theme) -> impl Fn(&iced::Theme, Status) -> Style + '_ {
    move |_, status| {
        let c = &t.colors;
        let look = |background: Color| Look {
            background: Some(background),
            text: c.danger,
            border: hairline(with_alpha(c.danger, 0.4), radius::MD),
            shadow: Shadow::default(),
        };
        by_status(
            status,
            look(c.raised),
            look(c.danger_soft),
            look(with_alpha(c.danger, 0.24)),
        )
    }
}

/// An icon button with no border, the navigator's only kind (`.nav-tb`).
pub fn icon(t: &Theme) -> impl Fn(&iced::Theme, Status) -> Style + '_ {
    move |_, status| {
        let c = &t.colors;
        let look = |background: Option<Color>, text: Color| Look {
            background,
            text,
            border: rounded(radius::MD),
            shadow: Shadow::default(),
        };
        by_status(
            status,
            look(None, c.fg_muted),
            look(Some(c.surface_hover), c.fg),
            look(Some(c.surface_active), c.fg),
        )
    }
}

/// A segment of a segmented control (`.segmented__btn`). The chosen one is
/// raised out of the track: lighter on a dark theme, white on a light one.
pub fn tab(t: &Theme, active: bool) -> impl Fn(&iced::Theme, Status) -> Style + '_ {
    move |_, status| {
        let c = &t.colors;
        if active {
            let look = Look {
                background: Some(c.seg_thumb),
                text: c.fg,
                border: hairline(c.line, radius::SM),
                shadow: drop_shadow(c.shadow, 1.0, 2.0),
            };
            return match status {
                Status::Disabled => look.disabled(),
                _ => look.style(),
            };
        }
        let look = |text: Color| Look {
            background: None,
            text,
            border: rounded(radius::SM),
            shadow: Shadow::default(),
        };
        by_status(status, look(c.fg_secondary), look(c.fg), look(c.fg))
    }
}

/// A line of a menu (`.ctx-menu__item`). The one under the pointer or the
/// keyboard is the accent, with the text that reads on it. A disabled line
/// is greyed to `fg_subtle` and not faded, as the menu always was.
pub fn menu_item(t: &Theme) -> impl Fn(&iced::Theme, Status) -> Style + '_ {
    menu_line(t, false)
}

/// A menu line that deletes or discards (`.ctx-menu__item--danger`).
pub fn menu_item_danger(t: &Theme) -> impl Fn(&iced::Theme, Status) -> Style + '_ {
    menu_line(t, true)
}

fn menu_line(t: &Theme, destructive: bool) -> impl Fn(&iced::Theme, Status) -> Style + '_ {
    move |_, status| {
        let c = &t.colors;
        let rest = if destructive { c.danger } else { c.fg };
        let (background, text) = match status {
            Status::Active => (None, rest),
            Status::Hovered | Status::Pressed => (Some(Background::Color(c.accent)), c.accent_fg),
            Status::Disabled => (None, c.fg_subtle),
        };
        Style {
            background,
            text_color: text,
            border: rounded(radius::SM),
            shadow: Shadow::default(),
            ..Style::default()
        }
    }
}

/// A row of a list: the navigator's workspaces, a settings page's entries.
/// The selected one is `surface_active`; the others light up under the
/// pointer.
pub fn row(t: &Theme, selected: bool) -> impl Fn(&iced::Theme, Status) -> Style + '_ {
    move |_, status| {
        let c = &t.colors;
        let look = |background: Option<Color>| Look {
            background,
            text: c.fg,
            border: rounded(radius::SM),
            shadow: Shadow::default(),
        };
        let selected_look = || look(Some(c.surface_active));
        match status {
            Status::Active if selected => selected_look().style(),
            Status::Active => look(None).style(),
            Status::Hovered | Status::Pressed if selected => selected_look().style(),
            Status::Hovered => look(Some(c.surface_hover)).style(),
            Status::Pressed => look(Some(c.surface_active)).style(),
            Status::Disabled => look(None).disabled(),
        }
    }
}

/// A button that reads as a link: the accent as text, no box.
pub fn link(t: &Theme) -> impl Fn(&iced::Theme, Status) -> Style + '_ {
    move |_, status| {
        let c = &t.colors;
        let text = match status {
            Status::Active => c.accent_hi,
            Status::Hovered => brightness(c.accent_hi, 1.12),
            Status::Pressed => brightness(c.accent_hi, 0.9),
            Status::Disabled => faded(c.accent_hi),
        };
        Style {
            background: None,
            text_color: text,
            border: Border::default(),
            shadow: Shadow::default(),
            ..Style::default()
        }
    }
}

/// A button with nothing around it that only changes the colour of its text:
/// the title row of a folding section (`.insp-section__toggle`), which sits
/// in a column of other things and must not light up like a tool.
pub fn plain(t: &Theme) -> impl Fn(&iced::Theme, Status) -> Style + '_ {
    move |_, status| {
        let c = &t.colors;
        let text = match status {
            Status::Active => c.fg_muted,
            Status::Hovered | Status::Pressed => c.fg,
            Status::Disabled => faded(c.fg_muted),
        };
        Style {
            background: None,
            text_color: text,
            border: Border::default(),
            shadow: Shadow::default(),
            ..Style::default()
        }
    }
}

/// An icon button that can be switched on (a toolbar's view toggles): quiet
/// when off like [`icon`], lit with the accent as text on the selection's
/// colour when on, so it reads as pressed in.
pub fn toggle(t: &Theme, on: bool) -> impl Fn(&iced::Theme, Status) -> Style + '_ {
    move |_, status| {
        let c = &t.colors;
        let look = |background: Option<Color>, text: Color| Look {
            background,
            text,
            border: rounded(radius::MD),
            shadow: Shadow::default(),
        };
        if on {
            return by_status(
                status,
                look(Some(c.surface_active), c.accent_hi),
                look(Some(c.surface_active), brightness(c.accent_hi, 1.1)),
                look(Some(c.surface_active), brightness(c.accent_hi, 0.9)),
            );
        }
        by_status(
            status,
            look(None, c.fg_muted),
            look(Some(c.surface_hover), c.fg),
            look(Some(c.surface_active), c.fg),
        )
    }
}

/// A tab of a strip of document tabs (`.model-tab`): the one in front takes
/// the panel's own surface and joins it, the others sit back on the header
/// and light up a little under the pointer.
pub fn tab_strip(t: &Theme, active: bool) -> impl Fn(&iced::Theme, Status) -> Style + '_ {
    move |_, status| {
        let c = &t.colors;
        let look = |background: Option<Color>, text: Color| Look {
            background,
            text,
            border: rounded(radius::NONE),
            shadow: Shadow::default(),
        };
        if active {
            let front = look(Some(c.chrome), c.fg);
            return match status {
                Status::Disabled => front.disabled(),
                _ => front.style(),
            };
        }
        by_status(
            status,
            look(None, c.fg_muted),
            look(Some(with_alpha(c.line, c.line.a * 0.6)), c.fg),
            look(Some(c.line), c.fg),
        )
    }
}

/// A row of the command palette. The chosen one (the keyboard's, or the
/// pointer's, since the palette moves one with the other) is one solid accent
/// bar with everything on it in the accent's own foreground.
pub fn palette_row(t: &Theme, chosen: bool) -> impl Fn(&iced::Theme, Status) -> Style + '_ {
    move |_, _| {
        let c = &t.colors;
        Style {
            background: chosen.then_some(Background::Color(c.accent)),
            text_color: if chosen { c.accent_fg } else { c.fg },
            border: rounded(radius::MD),
            shadow: Shadow::default(),
            ..Style::default()
        }
    }
}
