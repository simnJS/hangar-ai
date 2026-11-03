//! Two value types the shell's contracts name, kept here for good.
//!
//! They were stand-ins for hangar-ui's own, until that crate landed. It has
//! no counterpart to either: no close reason, since a hangar-ui layer
//! reports a dismissal and leaves the reason to its owner, and its menu
//! entry (`hangar_ui::menu::Entry`) is the widget's own input, built with an
//! `Item` per line and holding no `Clone`, which a menu kept in a package's
//! state and an entry embedded across packages (`accounts::menu_items`) both
//! need. So they stay the shell's: a package describes its menu with
//! `MenuEntry` and its view turns that into hangar-ui's entries.

/// Why a popover or a menu closed (`ToolbarMenu`'s close reasons): the owner
/// decides where the focus goes from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CloseReason {
    Escape,
    Tab,
    /// A press outside it (a press on its anchor is not one: the anchor
    /// toggles).
    Outside,
    /// The window lost the focus, or was resized.
    Blur,
}

/// One line of a context menu (`NavigatorMenu`'s entries, inventory §2.1/0),
/// which another package's menu embeds (`accounts::menu_items`).
#[derive(Debug, Clone)]
pub enum MenuEntry<Message = crate::message::Message> {
    Item {
        label: String,
        /// The icon's name, as `Icon.tsx` named it.
        icon: Option<&'static str>,
        danger: bool,
        /// A radio item's mark.
        checked: Option<bool>,
        /// Greyed out but still reachable by the arrows, with its reason as
        /// a tooltip.
        disabled: Option<String>,
        on_select: Message,
    },
    Heading(String),
    Separator,
}
