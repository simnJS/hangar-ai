//! Regions and focus (`src/lib/regions.ts`, test 39/5; inventory §1.8).
//!
//! F6 walks the window's regions in the order they are read: the navigator
//! down the left, the toolbar, the panes (or the board, memory or stats view
//! in their place), the preview, the board panel and the inspector on the
//! right, the status bar along the bottom. It wraps; a region not shown, or
//! with nothing to focus, is stepped over; a dialog keeps the keyboard.
//! Tab cannot do this: a terminal keeps Tab for the shell.
//!
//! Each region has an owner package whose `focus_entry` says what takes the
//! keyboard there (the navigator's current row, the focused pane's terminal,
//! the broadcast input…); the shell calls it (app.rs, `focus_region`).

use iced::widget;

use crate::shell::side_panels::Panels;
use crate::state::PaneId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Region {
    Navigator,
    Toolbar,
    Main,
    /// The preview panel (3D models, files, kit tabs).
    Model,
    /// The board beside the terminals.
    Dock,
    Inspector,
    Status,
}

/// The F6 order (`REGION_ORDER`).
pub const ORDER: [Region; 7] = [
    Region::Navigator,
    Region::Toolbar,
    Region::Main,
    Region::Model,
    Region::Dock,
    Region::Inspector,
    Region::Status,
];

impl Region {
    /// Whether the region is a panel the keyboard can rest on as a whole, as
    /// the TS focused the panel itself: it takes the focus even when its
    /// package has no control of its own to give it.
    pub fn is_panel(self) -> bool {
        matches!(self, Region::Model | Region::Dock | Region::Inspector)
    }
}

/// What has the keyboard (`Model.ui.focus`).
#[derive(Debug, Clone, PartialEq)]
pub enum Focus {
    /// A pane's terminal (hangar-term keeps its own focus flag).
    Terminal(PaneId),
    /// A region as a whole: a drawer, a panel.
    Region(Region),
    /// A widget with an id (`text_input::focus`, `widget::focus_next`).
    Widget(widget::Id),
}

impl Focus {
    /// The region the keyboard is in, when the shell can tell (`regionOf`):
    /// a terminal is in the panes, a widget could be anywhere.
    pub fn region(&self) -> Option<Region> {
        match self {
            Focus::Terminal(_) => Some(Region::Main),
            Focus::Region(region) => Some(*region),
            Focus::Widget(_) => None,
        }
    }
}

/// The region `step` away from `from` among those `shown`, wrapping
/// (`nextRegion`): from outside every region, or from one no longer shown,
/// F6 (`step > 0`) starts at the first and Shift+F6 at the last.
pub fn next_region(shown: &[Region], from: Option<Region>, step: i8) -> Option<Region> {
    let shown: Vec<Region> = ORDER.into_iter().filter(|r| shown.contains(r)).collect();
    if shown.is_empty() {
        return None;
    }
    let len = shown.len() as i64;
    let Some(at) = from.and_then(|from| shown.iter().position(|r| *r == from)) else {
        return Some(if step >= 0 {
            shown[0]
        } else {
            shown[shown.len() - 1]
        });
    };
    let next = (at as i64 + i64::from(step)).rem_euclid(len);
    Some(shown[next as usize])
}

/// The regions on screen in a window (App.tsx `cycleRegion`): the navigator
/// when it is drawn; the toolbar, the main area and the status bar when the
/// window shows a workspace; each side panel when it is shown, docked or as
/// the open drawer.
pub fn shown(panels: &Panels, has_workspace: bool) -> Vec<Region> {
    let mut shown = Vec::new();
    if panels.navigator {
        shown.push(Region::Navigator);
    }
    if has_workspace {
        shown.extend([Region::Toolbar, Region::Main, Region::Status]);
        if panels.model.shown {
            shown.push(Region::Model);
        }
        if panels.dock.shown {
            shown.push(Region::Dock);
        }
        if panels.inspector.shown {
            shown.push(Region::Inspector);
        }
    }
    shown
}

#[cfg(test)]
mod tests {
    //! regions.test.ts, case by case.

    use super::*;

    const ALL: [Region; 7] = ORDER;
    use Region::*;

    #[test]
    fn walks_the_regions_as_they_sit_on_screen() {
        assert_eq!(next_region(&ALL, Some(Navigator), 1), Some(Toolbar));
        assert_eq!(next_region(&ALL, Some(Toolbar), 1), Some(Main));
        assert_eq!(next_region(&ALL, Some(Main), 1), Some(Model));
        assert_eq!(next_region(&ALL, Some(Model), 1), Some(Dock));
        assert_eq!(next_region(&ALL, Some(Dock), 1), Some(Inspector));
        assert_eq!(next_region(&ALL, Some(Inspector), 1), Some(Status));
    }

    #[test]
    fn wraps_around_both_ways() {
        assert_eq!(next_region(&ALL, Some(Status), 1), Some(Navigator));
        assert_eq!(next_region(&ALL, Some(Navigator), -1), Some(Status));
        assert_eq!(next_region(&ALL, Some(Main), -1), Some(Toolbar));
    }

    #[test]
    fn skips_the_regions_that_are_not_on_screen_in_whatever_order_they_are_given() {
        let shown = [Status, Main, Toolbar];
        assert_eq!(next_region(&shown, Some(Toolbar), 1), Some(Main));
        assert_eq!(next_region(&shown, Some(Main), 1), Some(Status));
        assert_eq!(next_region(&shown, Some(Toolbar), -1), Some(Status));
    }

    #[test]
    fn starts_at_either_end_from_outside_every_region() {
        assert_eq!(next_region(&ALL, None, 1), Some(Navigator));
        assert_eq!(next_region(&ALL, None, -1), Some(Status));
        // Focus in a region that has just gone counts as outside.
        assert_eq!(
            next_region(&[Toolbar, Main], Some(Inspector), 1),
            Some(Toolbar)
        );
    }

    #[test]
    fn has_nowhere_to_go_without_a_region() {
        assert_eq!(next_region(&[], None, 1), None);
    }

    #[test]
    fn a_terminal_is_in_the_panes_and_a_widget_could_be_anywhere() {
        assert_eq!(Focus::Terminal("p".into()).region(), Some(Main));
        assert_eq!(Focus::Region(Dock).region(), Some(Dock));
        assert_eq!(Focus::Widget(widget::Id::unique()).region(), None);
    }
}
