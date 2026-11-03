//! Buttons: the four kinds of the web app's `.btn`, the icon button, the
//! toggle, the tab and the link.
//!
//! ```ignore
//! use hangar_ui::{button, Icon};
//!
//! row![
//!     button::ghost(t, "Cancel").on_press(Msg::Cancel),
//!     button::primary(t, "Save").icon(Icon::Check).on_press(Msg::Save),
//! ]
//!
//! // A button that cannot be pressed yet has no message:
//! button::primary(t, "Save").on_press_maybe(valid.then_some(Msg::Save))
//!
//! // An icon alone says what it does in a tooltip:
//! button::icon_button(t, Icon::Gear).tip("Settings").on_press(Msg::OpenSettings)
//! ```
//!
//! Pressed and disabled states come from the style, not from the view: a
//! button without a message is drawn at 45 % and does nothing.
//!
//! Gap: iced 0.14's buttons take no keyboard focus, so there is no focus ring
//! to draw; a button is reached by the pointer or by the shortcut that does
//! the same thing.

use hangar_theme::tokens::{font_size, size, space};
use hangar_theme::{Theme, fonts, style};
use iced::widget::{Space, button, container, row, text};
use iced::{Alignment, Element, Length, Padding, alignment};

use crate::icon::{Icon, icon};
use crate::tip;

/// The space above and below the label of a [`Btn`]: what is left around a
/// label that wraps onto a second line.
const PADDING_Y: f32 = 4.0;

/// Which of the web app's `.btn` variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    /// The one action of a dialog: the accent.
    Primary,
    /// The ordinary button: raised, with a hairline.
    #[default]
    Secondary,
    /// Nothing around it until it is pointed at: cancel, close.
    Ghost,
    /// What destroys something.
    Danger,
}

/// The height of a button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Size {
    /// 26 px, a field's height.
    #[default]
    Medium,
    /// 22 px (`.btn--tiny`): a row of actions inside a list item.
    Small,
}

impl Size {
    pub const fn height(self) -> f32 {
        match self {
            Size::Medium => size::CONTROL_HEIGHT,
            Size::Small => size::CONTROL_HEIGHT_SM,
        }
    }

    const fn padding_x(self) -> f32 {
        match self {
            Size::Medium => 11.0,
            Size::Small => 9.0,
        }
    }

    const fn text(self) -> f32 {
        match self {
            Size::Medium => font_size::BODY,
            Size::Small => font_size::CAPTION_LG,
        }
    }
}

/// A button with a label, an icon or both.
#[must_use]
pub struct Btn<'a, M> {
    theme: &'a Theme,
    kind: Kind,
    size: Size,
    label: Option<String>,
    icon: Option<Icon>,
    icon_after: bool,
    on_press: Option<M>,
    width: Length,
    tip: Option<String>,
}

impl<'a, M: Clone + 'a> Btn<'a, M> {
    pub fn new(theme: &'a Theme, kind: Kind, label: impl Into<String>) -> Self {
        Btn {
            theme,
            kind,
            size: Size::Medium,
            label: Some(label.into()),
            icon: None,
            icon_after: false,
            on_press: None,
            width: Length::Shrink,
            tip: None,
        }
    }

    /// An icon before the label.
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self.icon_after = false;
        self
    }

    /// An icon after the label: a disclosure arrow.
    pub fn icon_after(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self.icon_after = true;
        self
    }

    pub fn on_press(mut self, message: M) -> Self {
        self.on_press = Some(message);
        self
    }

    /// Without a message the button is disabled.
    pub fn on_press_maybe(mut self, message: Option<M>) -> Self {
        self.on_press = message;
        self
    }

    pub fn size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    pub fn small(self) -> Self {
        self.size(Size::Small)
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// A tooltip over the button.
    pub fn tip(mut self, words: impl Into<String>) -> Self {
        self.tip = Some(words.into());
        self
    }
}

impl<'a, M: Clone + 'a> From<Btn<'a, M>> for Element<'a, M> {
    fn from(b: Btn<'a, M>) -> Self {
        let t = b.theme;
        let mut content = row![].spacing(space::SM).align_y(Alignment::Center);
        let glyph = b.icon.map(|which| icon(which, size::ICON));
        let label = b.label.map(|label| {
            let font = if b.kind == Kind::Primary {
                fonts::ui_semibold()
            } else {
                fonts::ui_medium()
            };
            text(label)
                .size(b.size.text())
                .font(font)
                .align_x(alignment::Horizontal::Center)
        });
        match (glyph, label, b.icon_after) {
            (Some(glyph), Some(label), false) => content = content.push(glyph).push(label),
            (Some(glyph), Some(label), true) => content = content.push(label).push(glyph),
            (Some(glyph), None, _) => content = content.push(glyph),
            (None, Some(label), _) => content = content.push(label),
            (None, None, _) => {}
        }

        // The strut keeps the button at its size's height; a label that wraps
        // in a narrow button makes it taller instead of spilling out of it.
        let strut = Space::new()
            .width(0)
            .height(b.size.height() - 2.0 * PADDING_Y);
        let content = row![strut, content].align_y(Alignment::Center);
        let mut centred = container(content);
        if b.width == Length::Fill {
            centred = centred.center_x(Length::Fill);
        }
        let kind = b.kind;
        let look = move |theme: &iced::Theme, status| match kind {
            Kind::Primary => style::button::primary(t)(theme, status),
            Kind::Secondary => style::button::secondary(t)(theme, status),
            Kind::Ghost => style::button::ghost(t)(theme, status),
            Kind::Danger => style::button::danger(t)(theme, status),
        };
        let built = button(centred)
            .padding(Padding::from([PADDING_Y, b.size.padding_x()]))
            .width(b.width)
            .style(look)
            .on_press_maybe(b.on_press);
        match b.tip {
            Some(words) => tip::bottom(t, built, words),
            None => built.into(),
        }
    }
}

/// The accent button: the one action of a dialog.
pub fn primary<'a, M: Clone + 'a>(t: &'a Theme, label: impl Into<String>) -> Btn<'a, M> {
    Btn::new(t, Kind::Primary, label)
}

/// The ordinary button.
pub fn secondary<'a, M: Clone + 'a>(t: &'a Theme, label: impl Into<String>) -> Btn<'a, M> {
    Btn::new(t, Kind::Secondary, label)
}

/// A button with nothing around it until it is pointed at.
pub fn ghost<'a, M: Clone + 'a>(t: &'a Theme, label: impl Into<String>) -> Btn<'a, M> {
    Btn::new(t, Kind::Ghost, label)
}

/// A destructive action.
pub fn danger<'a, M: Clone + 'a>(t: &'a Theme, label: impl Into<String>) -> Btn<'a, M> {
    Btn::new(t, Kind::Danger, label)
}

/// A square button holding one icon: the toolbar's tools and every "…".
///
/// Without a [`tip`](IconBtn::tip) it says nothing to someone who cannot see
/// it, so give it one.
#[must_use]
pub struct IconBtn<'a, M> {
    theme: &'a Theme,
    which: Icon,
    side: f32,
    icon_size: f32,
    on_press: Option<M>,
    tip: Option<String>,
    keys: Option<String>,
    toggled: Option<bool>,
    danger: bool,
    rotation: f32,
}

impl<'a, M: Clone + 'a> IconBtn<'a, M> {
    pub fn new(theme: &'a Theme, which: Icon) -> Self {
        IconBtn {
            theme,
            which,
            side: size::CONTROL_HEIGHT,
            icon_size: size::ICON,
            on_press: None,
            tip: None,
            keys: None,
            toggled: None,
            danger: false,
            rotation: 0.0,
        }
    }

    pub fn on_press(mut self, message: M) -> Self {
        self.on_press = Some(message);
        self
    }

    pub fn on_press_maybe(mut self, message: Option<M>) -> Self {
        self.on_press = message;
        self
    }

    /// The tooltip, which is also what the button is called.
    pub fn tip(mut self, words: impl Into<String>) -> Self {
        self.tip = Some(words.into());
        self
    }

    /// The shortcut shown after the tooltip, written as the settings store it.
    pub fn keys(mut self, binding: impl Into<String>) -> Self {
        self.keys = Some(binding.into());
        self
    }

    /// A button that stays pressed in while `on`: a view that can be shown or
    /// hidden.
    pub fn toggled(mut self, on: bool) -> Self {
        self.toggled = Some(on);
        self
    }

    /// 22 px instead of 26 (`.btn--tiny`).
    pub fn small(mut self) -> Self {
        self.side = size::CONTROL_HEIGHT_SM;
        self
    }

    pub fn side(mut self, side: f32) -> Self {
        self.side = side;
        self
    }

    pub fn icon_size(mut self, icon_size: f32) -> Self {
        self.icon_size = icon_size;
        self
    }

    /// The danger colour under the pointer: a delete.
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    /// Turns the glyph, in radians.
    pub fn rotate(mut self, rotation: f32) -> Self {
        self.rotation = rotation;
        self
    }
}

impl<'a, M: Clone + 'a> From<IconBtn<'a, M>> for Element<'a, M> {
    fn from(b: IconBtn<'a, M>) -> Self {
        let t = b.theme;
        let glyph = icon(b.which, b.icon_size).rotate(b.rotation);
        let toggled = b.toggled;
        let destructive = b.danger;
        let look = move |theme: &iced::Theme, status| {
            if let Some(on) = toggled {
                return style::button::toggle(t, on)(theme, status);
            }
            let mut look = style::button::icon(t)(theme, status);
            if destructive && matches!(status, button::Status::Hovered | button::Status::Pressed) {
                look.text_color = t.colors.danger;
                look.background = Some(iced::Background::Color(t.colors.danger_soft));
            }
            look
        };
        let built = button(container(glyph).center(Length::Fill))
            .padding(0)
            .width(b.side)
            .height(b.side)
            .style(look)
            .on_press_maybe(b.on_press);
        match (b.tip, b.keys) {
            (Some(words), Some(keys)) => tip::keys(t, built, words, &keys),
            (Some(words), None) => tip::bottom(t, built, words),
            (None, _) => built.into(),
        }
    }
}

/// A square icon button.
pub fn icon_button<'a, M: Clone + 'a>(t: &'a Theme, which: Icon) -> IconBtn<'a, M> {
    IconBtn::new(t, which)
}

/// An icon button that stays pressed in while `on`.
pub fn toggle_button<'a, M: Clone + 'a>(t: &'a Theme, which: Icon, on: bool) -> IconBtn<'a, M> {
    IconBtn::new(t, which).toggled(on)
}

/// A tab of a strip of tabs: the one in front is on the panel's own surface.
///
/// For a row of them with a close button each, see `tabs::strip`; this is
/// the single tab, for a view that lays them out itself.
#[must_use]
pub struct TabBtn<'a, M> {
    theme: &'a Theme,
    label: String,
    icon: Option<Icon>,
    active: bool,
    on_press: Option<M>,
}

impl<'a, M: Clone + 'a> TabBtn<'a, M> {
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn on_press(mut self, message: M) -> Self {
        self.on_press = Some(message);
        self
    }
}

impl<'a, M: Clone + 'a> From<TabBtn<'a, M>> for Element<'a, M> {
    fn from(b: TabBtn<'a, M>) -> Self {
        let t = b.theme;
        let mut content = row![].spacing(space::SM).align_y(Alignment::Center);
        if let Some(which) = b.icon {
            content = content.push(icon(which, 12.0));
        }
        content = content.push(text(b.label).size(font_size::UI));
        button(container(content).center_y(Length::Fill))
            .padding(Padding::from([0.0, space::LG]))
            .height(Length::Fill)
            .style(style::button::tab_strip(t, b.active))
            .on_press_maybe(b.on_press)
            .into()
    }
}

/// A tab, `active` when it is the one in front.
pub fn tab_button<'a, M: Clone + 'a>(
    t: &'a Theme,
    label: impl Into<String>,
    active: bool,
) -> TabBtn<'a, M> {
    TabBtn {
        theme: t,
        label: label.into(),
        icon: None,
        active,
        on_press: None,
    }
}

/// A button that reads as a link: the accent as text, no box.
pub fn link<'a, M: Clone + 'a>(
    t: &'a Theme,
    label: impl Into<String>,
    on_press: M,
) -> Element<'a, M> {
    button(
        text(label.into())
            .size(font_size::UI)
            .font(fonts::ui_medium()),
    )
    .padding(0)
    .style(style::button::link(t))
    .on_press(on_press)
    .into()
}
