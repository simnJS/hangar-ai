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

pub use drawer::Drawer;
pub use notice::{Notice, Notification};
pub use overlay::{Overlay, SettingsCategory};
pub use regions::{Focus, Region};
pub use side_panels::{BoardVariant, PanelMode};
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
