//! Stand-ins for the hangar-ui types the shell's contracts name, until that
//! crate lands (W2.ui, being built in parallel).
//!
//! TODO(hangar-ui): replace each with a `pub use` of hangar-ui's own type
//! (`hangar_ui::popover::CloseReason`, the context menu's entry model), so
//! the packages that already name `crate::shell::CloseReason` and
//! `crate::shell::MenuEntry` keep compiling.

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
