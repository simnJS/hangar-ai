//! Docked or drawer (`src/lib/sidePanels.ts`, test 209/21; inventory §1.11).
//!
//! The terminals keep 640 px; the preview (min 320), then the board dock
//! (min 300), then the inspector (240–440) get what is left, in that order of
//! priority, widths in points × `uiScale`. A panel that does not fit becomes a
//! drawer over the terminals, 48 px from the edge; its toggle opens the
//! drawer and leaves the setting alone; widening the window ends a drawer.
//!
//! TODO(P0b): `place`, ported with its tests. Until then every panel is
//! docked at its setting's width.

/// What the terminals always keep.
pub const TERMINALS_MIN: f32 = 640.0;
pub const MODEL_MIN: f32 = 320.0;
pub const DOCK_MIN: f32 = 300.0;
pub const INSPECTOR_MIN: f32 = 240.0;
pub const INSPECTOR_MAX: f32 = 440.0;
/// A drawer's distance from the far edge of the terminals.
pub const DRAWER_MARGIN: f32 = 48.0;

/// How the preview panel is drawn (`preview::view_panel`'s `mode`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PanelMode {
    Docked {
        width: f32,
    },
    Drawer {
        width: f32,
    },
    /// Over the whole main area.
    Expanded,
}

/// Which board `board::view` draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BoardVariant {
    /// The main area's board (`view == Board`).
    Full,
    /// The panel beside the terminals.
    Dock { width: f32, drawer: bool },
}
