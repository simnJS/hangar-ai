//! The design tokens that are not colours, taken from `src/styles.css` and
//! `src/styles/`.
//!
//! The stylesheets name only the radii and the transition (`:root`); the
//! rest were repeated numbers, and the ones that repeat are named here once.
//! All lengths are in logical pixels at an interface size of 100 %: the web
//! app multiplied each by `--ui-scale`, and the app does the same by handing
//! iced a scale factor, so nothing here is ever multiplied again.

/// Corner radii. "Small, consistent radii: controls 4, cards 6 to 8, floating
/// panels 10. Panes themselves stay square and share 1px separators."
pub mod radius {
    /// `--radius-xs`: a segment of a segmented control, a checkbox.
    pub const XS: f32 = 4.0;
    /// `--radius-sm`: a field, a menu item, a tab.
    pub const SM: f32 = 5.0;
    /// `--radius`: a button.
    pub const MD: f32 = 6.0;
    /// `--radius-lg`: a card, a context menu.
    pub const LG: f32 = 8.0;
    /// `--radius-xl`: a dialog, the command palette.
    pub const XL: f32 = 10.0;
    /// The track of a switch, a pill, a progress bar: any radius larger than
    /// half the shortest side, which iced clamps.
    pub const FULL: f32 = 999.0;
    /// A pane, a column: the panes are square.
    pub const NONE: f32 = 0.0;
}

/// The spacing scale: the gaps and paddings that recur in the stylesheets.
/// Others occur (1, 3, 5, 7, 9, 14), nearly always to nudge a glyph into line
/// with its text; those are written where they are needed.
pub mod space {
    /// A hairline between two things, and the width of a border.
    pub const HAIRLINE: f32 = 1.0;
    /// The gap in a segmented control, an inner padding.
    pub const XXS: f32 = 2.0;
    pub const XS: f32 = 4.0;
    /// The gap between an icon and its label.
    pub const SM: f32 = 6.0;
    /// The usual gap and padding: 68 of the stylesheets' gaps.
    pub const MD: f32 = 8.0;
    pub const LG: f32 = 10.0;
    pub const XL: f32 = 12.0;
    pub const XXL: f32 = 16.0;
    /// The padding of a dialog's body.
    pub const XXXL: f32 = 24.0;
}

/// Font sizes. The window reads in 12 and 13; the small ones carry metadata.
pub mod font_size {
    // Every text size is the stylesheet's plus a pixel (the three smallest a
    // pixel and a half, the 11.5 px step a pixel and a quarter, to keep the
    // scale strictly rising): a browser drew them with the system's subpixel
    // smoothing, and smoothed in grey the same sizes read too thin. The icon
    // size is the stylesheet's own.
    /// A mono counter or a version (`.nav-top__version`).
    pub const MICRO: f32 = 11.5;
    /// A group label in small capitals (`.nav-section`), a key hint.
    pub const LABEL: f32 = 12.0;
    /// A caption, a metadata line.
    pub const CAPTION: f32 = 12.5;
    /// A tiny button, a row's secondary line.
    pub const CAPTION_LG: f32 = 12.75;
    /// The toolbar, the navigator, the status bar, fields, the palette.
    pub const UI: f32 = 13.0;
    /// The navigator's title.
    pub const UI_LG: f32 = 13.5;
    /// The page's text: `body`, menu items.
    pub const BODY: f32 = 14.0;
    /// A dialog's or a page's title.
    pub const TITLE: f32 = 16.0;
    /// The palette's search glyph.
    pub const ICON_LG: f32 = 17.0;
    /// The big number of a statistic.
    pub const DISPLAY: f32 = 27.0;
}

/// Font weights, as the numbers of CSS. The stylesheets also use 550 and 650
/// once each, which iced's four weights cannot tell from 500 and 600.
pub mod font_weight {
    pub const REGULAR: u16 = 400;
    /// Buttons, tabs, segments.
    pub const MEDIUM: u16 = 500;
    /// Titles, labels, a primary button: the most used after 500.
    pub const SEMIBOLD: u16 = 600;
    pub const BOLD: u16 = 700;
}

/// Line heights, as a multiple of the font size (what iced's
/// `LineHeight::Relative` takes).
pub mod line_height {
    /// A single line in a control, an icon.
    pub const NONE: f32 = 1.0;
    pub const TIGHT: f32 = 1.25;
    /// A title that may wrap.
    pub const TITLE: f32 = 1.3;
    /// Fields and most text: the one used most.
    pub const UI: f32 = 1.4;
    pub const BODY: f32 = 1.5;
    /// A paragraph of prose (the markdown preview).
    pub const PROSE: f32 = 1.6;
}

/// Letter spacing in em. iced has no letter spacing; kept so the one place
/// that needs it (small capitals) can be approximated, and for the day it has.
pub mod letter_spacing {
    pub const TIGHT: f32 = -0.01;
    pub const NORMAL: f32 = 0.0;
    pub const CAPITALS: f32 = 0.06;
}

/// How long things take to change.
pub mod duration {
    use std::time::Duration;

    /// `--dur`: a hover, a focus, most transitions.
    pub const BASE: Duration = Duration::from_millis(140);
    /// A context menu appearing.
    pub const FAST: Duration = Duration::from_millis(100);
    /// A dialog or a pill rising into place.
    pub const MODAL: Duration = Duration::from_millis(180);
    /// The command palette: `--dur` and 30 ms.
    pub const PALETTE: Duration = Duration::from_millis(170);
    /// The update banner sliding in.
    pub const BANNER: Duration = Duration::from_millis(220);
    /// A board revealing itself.
    pub const REVEAL: Duration = Duration::from_millis(240);
    /// The pulse of a working agent's glyph.
    pub const PULSE: Duration = Duration::from_millis(1400);

    /// `--ease`: `cubic-bezier(0.2, 0.7, 0.2, 1)`, as the control points
    /// `[x1, y1, x2, y2]`.
    pub const EASE: [f32; 4] = [0.2, 0.7, 0.2, 1.0];
}

/// Sizes of the window's parts and of its controls.
pub mod size {
    /// The navigator's width (`--sidebar-w`).
    pub const SIDEBAR_WIDTH: f32 = 252.0;
    /// The toolbar and the navigator's top row, level so the window reads as
    /// one band.
    pub const TOOLBAR_HEIGHT: f32 = 44.0;
    /// The status bar, as tall as the navigator's bottom bar.
    pub const STATUS_BAR_HEIGHT: f32 = 36.0;
    /// A workspace in the navigator (`--nav-row`).
    pub const NAV_ROW_HEIGHT: f32 = 28.0;
    /// A button, a field, a segmented control.
    pub const CONTROL_HEIGHT: f32 = 26.0;
    /// A tiny button, a segment in the toolbar.
    pub const CONTROL_HEIGHT_SM: f32 = 22.0;
    /// A line of a menu.
    pub const MENU_ITEM_HEIGHT: f32 = 26.0;
    /// A pane's header.
    pub const PANE_HEAD_HEIGHT: f32 = 30.0;
    /// The palette's search field.
    pub const PALETTE_SEARCH_HEIGHT: f32 = 50.0;
    /// Most icons.
    pub const ICON: f32 = 14.0;
    pub const ICON_LG: f32 = 16.0;
    /// A scrollbar, rail and thumb.
    pub const SCROLLBAR: f32 = 9.0;
    /// A switch: its track, and its knob.
    pub const SWITCH_WIDTH: f32 = 30.0;
    pub const SWITCH_HEIGHT: f32 = 18.0;
    pub const SWITCH_KNOB: f32 = 14.0;
    /// A dialog's width, and a wider list dialog's.
    pub const DIALOG_WIDTH: f32 = 520.0;
    pub const DIALOG_LIST_WIDTH: f32 = 560.0;
    /// The command palette's width.
    pub const PALETTE_WIDTH: f32 = 640.0;
    /// A context menu's bounds.
    pub const MENU_MIN_WIDTH: f32 = 200.0;
    pub const MENU_MAX_WIDTH: f32 = 280.0;
}

/// The focus ring: a 2 px outline in `accent_ring`, in the web app drawn
/// outside the control and here as the control's border, since iced has no
/// outline.
pub const FOCUS_RING_WIDTH: f32 = 2.0;

/// How far a disabled control fades.
pub const DISABLED_OPACITY: f32 = 0.45;

/// The width of every hairline border.
pub const BORDER_WIDTH: f32 = 1.0;

/// Bounds of the interface size slider, in percent (`UI_SCALE_MIN` and
/// `UI_SCALE_MAX` of `src/types.ts`). The app turns the setting into the
/// scale factor it gives iced.
pub const UI_SCALE_MIN_PERCENT: u32 = 90;
pub const UI_SCALE_MAX_PERCENT: u32 = 130;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_radii_step_up() {
        let steps = [radius::XS, radius::SM, radius::MD, radius::LG, radius::XL];
        assert!(steps.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn the_spacing_scale_steps_up() {
        let steps = [
            space::HAIRLINE,
            space::XXS,
            space::XS,
            space::SM,
            space::MD,
            space::LG,
            space::XL,
            space::XXL,
            space::XXXL,
        ];
        assert!(steps.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn the_font_sizes_step_up() {
        let steps = [
            font_size::MICRO,
            font_size::LABEL,
            font_size::CAPTION,
            font_size::CAPTION_LG,
            font_size::UI,
            font_size::UI_LG,
            font_size::BODY,
            font_size::TITLE,
            font_size::ICON_LG,
            font_size::DISPLAY,
        ];
        assert!(steps.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
