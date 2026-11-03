//! A side panel shown as a drawer (`src/lib/useDrawer.ts`, inventory §1.11):
//! it takes the focus when it opens, closes on a press or a focus outside it
//! (except over modals, menus and the drawer toggles) and on Escape, then
//! gives the focus back to the opener or to the terminal.
//!
//! TODO(P0b): the state machine (open → focused → closing with a reason →
//! focus restored), as a plain struct the shell updates, not a hook.

/// The panels that can be a drawer (`Model.ui.drawer`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Drawer {
    Inspector,
    Dock,
    Model,
}

/// Why a drawer closed (`DrawerCloseReason`), which decides where the focus
/// goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawerCloseReason {
    Escape,
    Outside,
    Toggle,
}
