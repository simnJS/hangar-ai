//! The frame around the packages: windows, regions and focus, side panels and
//! drawers, overlays, notices, placeholders (App.tsx's frame, inventory §1.7,
//! §1.8, §1.11).
//!
//! The shell lays the window out and gives each region its size and its
//! surface; what is inside a region is the package's own view (the slot
//! table of PACKAGES.md).

pub mod dev_screenshot;
pub mod drawer;
pub mod frame;
pub mod notice;
pub mod overlay;
pub mod placeholder;
pub mod regions;
pub mod side_panels;
pub mod ui;
pub mod windows;

pub use drawer::{Drawer, DrawerCloseReason};
pub use notice::{Notice, Notification};
pub use overlay::{Overlay, SettingsCategory};
pub use regions::{Focus, Region};
pub use side_panels::{BoardVariant, PanelMode, Panels};
pub use ui::{CloseReason, MenuEntry};
pub use windows::WindowKind;

/// What the main area shows (`view` of App.tsx).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum View {
    #[default]
    Terminals,
    Board,
    Memory,
    Stats,
}

/// The shell's own messages (`Message::Shell`), for what only the frame and
/// the command table ask of it. Packages never send these: their requests are
/// intents.
#[derive(Debug, Clone)]
pub enum Action {
    /// The pointer went over or off the open drawer (its mouse area).
    PointerOverDrawer(bool),
    /// The pointer went over or off the toolbar.
    PointerOverToolbar(bool),
    /// A mouse button went down in a window: a drawer's "press outside".
    Pressed(iced::window::Id),
    /// `view.needsYou`: the Needs-you list opens under its button.
    OpenNeedsYou,
    /// `view.settings`: the settings close when open, else open on General.
    ToggleSettings,
    /// `app.fullscreen`, for this window.
    ToggleFullscreen(iced::window::Id),
    /// The size of the main window's screen is known: workspace `ws`'s
    /// window can open (a detach, or reopening at boot).
    OpenWorkspaceWindow {
        ws: crate::state::WsId,
        screen: Option<iced::Size>,
    },
    /// A workspace window has been still for a while since this move or
    /// resize: its bounds are written if nothing moved it since.
    BoundsSettled(iced::window::Id, u64),
}
