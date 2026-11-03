//! Regions and focus (`src/lib/regions.ts`, test 39/5; inventory §1.8).
//!
//! F6 walks the regions in a fixed order and wraps; a region not shown, or
//! with nothing to focus, is skipped; a dialog keeps the keyboard. Each region
//! has an owner package whose `focus_entry` says what takes the focus there
//! (the navigator's current row, the focused pane's terminal, the broadcast
//! input…); the shell calls it (app.rs, `focus_region`).
//!
//! TODO(P0b): which regions are shown in a window (`shown`) and the focus
//! restore rules; the order and the stepping are here.

use iced::widget;

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

/// The F6 order.
pub const ORDER: [Region; 7] = [
    Region::Navigator,
    Region::Toolbar,
    Region::Main,
    Region::Model,
    Region::Dock,
    Region::Inspector,
    Region::Status,
];

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

/// The region `step` away from `from` among those `shown`, wrapping: from
/// outside every region, F6 (`step > 0`) starts at the first and Shift+F6 at
/// the last.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f6_wraps_and_starts_from_either_end() {
        let shown = [Region::Main, Region::Navigator, Region::Status];
        assert_eq!(next_region(&shown, None, 1), Some(Region::Navigator));
        assert_eq!(next_region(&shown, None, -1), Some(Region::Status));
        assert_eq!(
            next_region(&shown, Some(Region::Status), 1),
            Some(Region::Navigator)
        );
        assert_eq!(
            next_region(&shown, Some(Region::Navigator), -1),
            Some(Region::Status)
        );
        assert_eq!(next_region(&[], None, 1), None);
    }
}
