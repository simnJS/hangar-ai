//! A side panel shown as a drawer (`src/lib/useDrawer.ts`, inventory §1.11).
//!
//! A drawer is a passing thing, like a menu, never a state the window is left
//! in: it takes the keyboard when it opens (it only ever opens because the
//! person asked), and goes away on Escape, on a press anywhere else, and when
//! the keyboard moves on to something else. Escape and its own close button
//! give the keyboard back to whatever had it before, or to the focused pane's
//! terminal; a press elsewhere leaves the keyboard where the press put it.
//! What is drawn over the drawer (a dialog, a menu) is its own business:
//! while an overlay is open, presses and Escape leave the drawer alone.
//!
//! The state machine is plain data in `Model.ui` (`drawer`, `drawer_opener`,
//! `pointer`); the shell runs its transitions (app.rs) and these functions
//! say what each one does.

use crate::shell::{Focus, Region, View};
use crate::state::Ui;

use super::side_panels::Panels;

/// The panels that can be a drawer (`Model.ui.drawer`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Drawer {
    Inspector,
    Dock,
}

impl Drawer {
    /// The region the drawer is (F6 lands on it as on the docked panel).
    pub fn region(self) -> Region {
        match self {
            Drawer::Inspector => Region::Inspector,
            Drawer::Dock => Region::Dock,
        }
    }
}

/// Why a drawer closed (`DrawerCloseReason`), which decides where the focus
/// goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawerCloseReason {
    Escape,
    /// A press or a focus outside it: the keyboard stays where that put it.
    Outside,
    /// Its toggle, a shortcut or its own close button (`button` in the TS).
    Toggle,
}

/// Opens `drawer` in the focused window: what had the keyboard is remembered
/// for its close, and the drawer takes the keyboard. Answers the region to
/// focus.
pub fn open(ui: &mut Ui, drawer: Drawer) -> Region {
    if ui.drawer != Some(drawer) {
        ui.drawer_opener = ui.focus.clone();
    }
    ui.drawer = Some(drawer);
    ui.pointer.over_drawer = false;
    ui.focus = Some(Focus::Region(drawer.region()));
    drawer.region()
}

/// Closes the open drawer for `reason`. Answers where the keyboard goes back
/// to: `None` when it stays put (a press outside, or nothing was open).
pub fn close(ui: &mut Ui, reason: DrawerCloseReason) -> Option<Focus> {
    ui.drawer?;
    ui.drawer = None;
    ui.pointer.over_drawer = false;
    let opener = ui.drawer_opener.take();
    if reason == DrawerCloseReason::Outside {
        return None;
    }
    // While a drawer is open the keyboard is in it (anything outside taking
    // it closes the drawer as `Outside`), so closing it otherwise would leave
    // the keyboard nowhere: back to the opener, else to the panes.
    Some(opener.unwrap_or(Focus::Region(Region::Main)))
}

/// The drawer still open once the window has changed: one the window grew
/// wide enough to dock, or a board drawer whose terminals left the screen, is
/// over, so narrowing again does not bring it back. The keyboard
/// stays where it is: only a real close gives it back.
pub fn settle(drawer: Option<Drawer>, panels: &Panels, view: View) -> Option<Drawer> {
    let terminals = view == View::Terminals;
    match drawer? {
        Drawer::Inspector if !panels.inspector.drawer => None,
        Drawer::Dock if !panels.dock.drawer || !terminals => None,
        open => Some(open),
    }
}

/// Whether a press in the focused window closes its drawer: one is open, no
/// overlay is over it, and the press is neither on it nor on the toolbar,
/// whose buttons toggle it themselves (`data-drawer-toggle`).
pub fn press_closes(ui: &Ui) -> bool {
    ui.drawer.is_some()
        && ui.overlays.is_empty()
        && !ui.pointer.over_drawer
        && !ui.pointer.over_toolbar
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::side_panels::Slot;

    fn panels(inspector: bool, dock: bool) -> Panels {
        let slot = |drawer| Slot {
            shown: true,
            drawer,
            width: 300.0,
            max: 400.0,
        };
        Panels {
            scale: 1.0,
            inspector: slot(inspector),
            dock: slot(dock),
            navigator: true,
        }
    }

    #[test]
    fn opening_takes_the_keyboard_and_escape_gives_it_back() {
        let mut ui = Ui {
            focus: Some(Focus::Terminal("p".into())),
            ..Ui::default()
        };
        assert_eq!(open(&mut ui, Drawer::Inspector), Region::Inspector);
        assert_eq!(ui.focus, Some(Focus::Region(Region::Inspector)));
        assert_eq!(
            close(&mut ui, DrawerCloseReason::Escape),
            Some(Focus::Terminal("p".into()))
        );
        assert_eq!(ui.drawer, None);
    }

    #[test]
    fn a_press_outside_leaves_the_keyboard_where_it_went() {
        let mut ui = Ui::default();
        open(&mut ui, Drawer::Dock);
        assert_eq!(close(&mut ui, DrawerCloseReason::Outside), None);
        assert_eq!(ui.drawer, None);
        assert_eq!(ui.drawer_opener, None);
    }

    #[test]
    fn without_an_opener_the_keyboard_goes_back_to_the_panes() {
        let mut ui = Ui::default();
        open(&mut ui, Drawer::Dock);
        assert_eq!(
            close(&mut ui, DrawerCloseReason::Toggle),
            Some(Focus::Region(Region::Main))
        );
        assert_eq!(
            close(&mut ui, DrawerCloseReason::Toggle),
            None,
            "nothing open"
        );
    }

    #[test]
    fn a_drawer_that_now_fits_or_whose_terminals_left_is_over() {
        let narrow = panels(true, true);
        assert_eq!(
            settle(Some(Drawer::Inspector), &narrow, View::Board),
            Some(Drawer::Inspector)
        );
        assert_eq!(settle(Some(Drawer::Dock), &narrow, View::Board), None);
        assert_eq!(
            settle(Some(Drawer::Dock), &narrow, View::Terminals),
            Some(Drawer::Dock)
        );
        let wide = panels(false, false);

        assert_eq!(
            settle(Some(Drawer::Inspector), &wide, View::Terminals),
            None
        );
        assert_eq!(settle(Some(Drawer::Dock), &wide, View::Terminals), None);
    }

    #[test]
    fn a_press_on_the_drawer_or_the_toolbar_or_under_a_dialog_is_not_outside() {
        let mut ui = Ui::default();
        assert!(!press_closes(&ui), "nothing open");
        open(&mut ui, Drawer::Inspector);
        assert!(press_closes(&ui));
        ui.pointer.over_drawer = true;
        assert!(!press_closes(&ui));
        ui.pointer.over_drawer = false;
        ui.pointer.over_toolbar = true;
        assert!(!press_closes(&ui));
        ui.pointer.over_toolbar = false;
        ui.overlays.push(crate::shell::Overlay::Palette);
        assert!(!press_closes(&ui));
    }
}
