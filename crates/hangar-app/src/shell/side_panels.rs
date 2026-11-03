//! Docked or drawer (`src/lib/sidePanels.ts`, test 209/21; inventory §1.11).
//!
//! The panels on the right of the terminals (the preview, the board dock and
//! the inspector) each sit beside them while that leaves the terminals at
//! least `TERMINALS_MIN` pixels. Without that room a panel stops taking any:
//! it becomes a drawer over the right of the work area, shown only while asked
//! for, so a narrow window keeps its terminals whatever the saved state says.
//!
//! Widths come in interface points and are drawn at `points × scale`, so a
//! larger interface size keeps the same amount of text per line. `place`
//! works in pixels as the TS did (the room and the widths it answers), except
//! the `max`es, which are in points, like the setting a drag writes. iced
//! scales the whole window by the interface size, the terminals included, so
//! the frame hands `place` its room in pixels (layout width × scale) and draws
//! what comes back divided by the scale (`Panels`).

use iced::window;

use crate::shell::{Drawer, View, WindowKind};
use crate::state::Model;

/// What the terminals keep beside docked panels, in pixels: about a 1190 px
/// window with the navigator and the inspector open.
pub const TERMINALS_MIN: f32 = 640.0;
pub const INSPECTOR_MIN: f32 = 240.0;
pub const INSPECTOR_MAX: f32 = 440.0;
pub const DOCK_MIN: f32 = 300.0;
/// A 3D view narrower than this shows a model as a sliver.
pub const MODEL_MIN: f32 = 320.0;
/// The strip of work area a drawer, however wide, always leaves uncovered.
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

/// One panel as `place` is asked about it: open (its setting), and its
/// width in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelInput {
    pub open: bool,
    pub width: f32,
}

/// What `place` is asked (`SidePanelsInput`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SidePanelsInput {
    /// The row the terminals share with the panels, in pixels.
    pub room: f32,
    /// The interface size, 1 for 100 %.
    pub scale: f32,
    pub inspector: PanelInput,
    /// `None` when the terminals are not on screen, the only place it goes.
    pub dock: Option<PanelInput>,
    /// The preview, beside the terminals like the dock. `None` while nothing
    /// is open, off screen with the terminals, or expanded over the whole
    /// work area, where it takes no room from anyone.
    pub model: Option<PanelInput>,
}

/// Where one panel goes (`PanelPlace`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelPlace {
    /// Beside the terminals; otherwise a drawer over them, shown on demand.
    pub fits: bool,
    /// Drawn width, in pixels.
    pub width: f32,
    /// The widest a drag may make it now, in points.
    pub max: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SidePanels {
    pub inspector: PanelPlace,
    pub dock: PanelPlace,
    pub model: PanelPlace,
}

pub fn clamp_inspector(width: f32) -> f32 {
    width.clamp(INSPECTOR_MIN, INSPECTOR_MAX).round()
}

pub fn clamp_dock(width: f32) -> f32 {
    width.max(DOCK_MIN).round()
}

pub fn clamp_model_panel(width: f32) -> f32 {
    width.max(MODEL_MIN).round()
}

/// `placeSidePanels`.
///
/// When both are open and only one fits, the inspector is the one that turns
/// into a drawer: the board panel is what the person asked to keep beside the
/// panes, while the inspector details one pane that its header and the
/// toolbar already sum up. It reserves the dock's minimum width, not its
/// saved one, so a wide dock narrows before the inspector gives way.
///
/// The preview goes before both: it is what an agent has just put in front of
/// the person, and it only exists while something is open. The dock gives
/// way to it the way the inspector gives way to the dock.
pub fn place(input: SidePanelsInput) -> SidePanels {
    let SidePanelsInput {
        room,
        scale,
        inspector,
        dock,
        model,
    } = input;
    let inspector_px = clamp_inspector(inspector.width) * scale;
    let dock_min_px = DOCK_MIN * scale;
    let dock_wanted = dock.map_or(dock_min_px, |dock| clamp_dock(dock.width) * scale);
    let model_min_px = MODEL_MIN * scale;
    let model_wanted = model.map_or(model_min_px, |model| clamp_model_panel(model.width) * scale);
    let model_open = model.is_some_and(|model| model.open);
    let dock_open = dock.is_some_and(|dock| dock.open);

    let model_counts = model_open && room - model_min_px >= TERMINALS_MIN;
    let beside_model = room - if model_counts { model_min_px } else { 0.0 };
    let dock_counts = dock_open && beside_model - dock_min_px >= TERMINALS_MIN;
    let reserved =
        if model_counts { model_min_px } else { 0.0 } + if dock_counts { dock_min_px } else { 0.0 };
    let inspector_fits = room - inspector_px - reserved >= TERMINALS_MIN;

    let inspector_place = if inspector_fits {
        PanelPlace {
            fits: true,
            width: inspector_px,
            max: clamp_inspector((room - reserved - TERMINALS_MIN) / scale),
        }
    } else {
        PanelPlace {
            fits: false,
            width: inspector_px.min(room - DRAWER_MARGIN).max(0.0),
            max: clamp_inspector((room - DRAWER_MARGIN) / scale),
        }
    };

    // What the preview and the dock share with the terminals: the row, less a
    // docked inspector.
    let workarea = room
        - if inspector.open && inspector_fits {
            inspector_px
        } else {
            0.0
        };

    let model_fits = workarea - model_min_px >= TERMINALS_MIN;
    let dock_kept = if dock_open && workarea - model_min_px - dock_min_px >= TERMINALS_MIN {
        dock_min_px
    } else {
        0.0
    };
    let model_room = workarea
        - if model_fits {
            TERMINALS_MIN + dock_kept
        } else {
            DRAWER_MARGIN
        };
    let model_place = PanelPlace {
        fits: model_fits,
        width: if model_fits {
            model_wanted.min(model_room).max(model_min_px)
        } else {
            model_wanted.min(model_room).max(0.0)
        },
        max: clamp_model_panel(model_room / scale),
    };

    let dock_area = workarea
        - if model_open && model_fits {
            model_place.width
        } else {
            0.0
        };
    let dock_fits = dock_area - dock_min_px >= TERMINALS_MIN;
    let dock_room = dock_area
        - if dock_fits {
            TERMINALS_MIN
        } else {
            DRAWER_MARGIN
        };
    let dock_place = PanelPlace {
        fits: dock_fits,
        width: if dock_fits {
            dock_wanted.min(dock_room).max(dock_min_px)
        } else {
            dock_wanted.min(dock_room).max(0.0)
        },
        max: clamp_dock(dock_room / scale),
    };

    SidePanels {
        inspector: inspector_place,
        dock: dock_place,
        model: model_place,
    }
}

/// One side panel of a window as the frame draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slot {
    /// On screen, docked or as the open drawer.
    pub shown: bool,
    /// Without room beside the terminals: a drawer when shown.
    pub drawer: bool,
    /// Its drawn width, in layout units (what iced lays out in).
    pub width: f32,
    /// The widest a drag may make it, in points (the setting's unit).
    pub max: f32,
}

/// Everything the frame, the regions and the toggles of window `w` need to
/// know about its side panels (App.tsx:509–686).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Panels {
    pub scale: f32,
    pub inspector: Slot,
    pub dock: Slot,
    pub model: Slot,
    /// The workspace has something open in its preview.
    pub models_open: bool,
    /// The preview covers the work area (terminals view only).
    pub model_expanded: bool,
    /// The navigator is drawn (the main window's alone).
    pub navigator: bool,
}

impl Panels {
    /// Where the panels of window `w` go now.
    pub fn of(m: &Model, w: window::Id) -> Panels {
        let settings = m.settings();
        let scale = settings.scale_factor();
        let terminals = m.view_in(w) == View::Terminals;
        let drawer = m.drawer_in(w);
        let preview = m
            .window_workspace(w)
            .and_then(|ws| m.live.signals.preview.get(&ws.id))
            .copied()
            .unwrap_or_default();
        let models_open = preview.entries > 0;
        let model_expanded = models_open && preview.expanded && terminals;
        // Until the row has been measured, everything counts as fitting.
        let room =
            m.ui.body_width
                .get(&w)
                .map_or(f32::INFINITY, |width| width * scale);
        let sides = place(SidePanelsInput {
            room,
            scale,
            inspector: PanelInput {
                open: settings.inspector_open,
                width: settings.inspector_width as f32,
            },
            dock: terminals.then_some(PanelInput {
                open: settings.board_dock_open,
                width: settings.board_dock_width as f32,
            }),
            model: (terminals && models_open && !model_expanded).then_some(PanelInput {
                open: preview.open,
                width: settings.model_panel_width as f32,
            }),
        });
        let slot = |place: PanelPlace, shown: bool| Slot {
            shown,
            drawer: !place.fits,
            width: place.width / scale,
            max: place.max,
        };
        let inspector_shown = if sides.inspector.fits {
            settings.inspector_open
        } else {
            drawer == Some(Drawer::Inspector)
        };
        let dock_shown = terminals
            && if sides.dock.fits {
                settings.board_dock_open
            } else {
                drawer == Some(Drawer::Dock)
            };
        let model_shown = terminals
            && models_open
            && (model_expanded
                || if sides.model.fits {
                    preview.open
                } else {
                    drawer == Some(Drawer::Model)
                });
        let has_workspace = m.window_workspace(w).is_some();
        Panels {
            scale,
            inspector: slot(sides.inspector, has_workspace && inspector_shown),
            dock: slot(sides.dock, has_workspace && dock_shown),
            model: slot(sides.model, has_workspace && model_shown),
            models_open,
            model_expanded,
            navigator: m.live.windows.get(&w) == Some(&WindowKind::Main) && settings.navigator_open,
        }
    }

    /// The panel `drawer` names, as placed.
    pub fn slot(&self, drawer: Drawer) -> Slot {
        match drawer {
            Drawer::Inspector => self.inspector,
            Drawer::Dock => self.dock,
            Drawer::Model => self.model,
        }
    }

    /// The preview's mode, when it is shown.
    pub fn model_mode(&self) -> Option<PanelMode> {
        if !self.model.shown {
            return None;
        }
        Some(if self.model_expanded {
            PanelMode::Expanded
        } else if self.model.drawer {
            PanelMode::Drawer {
                width: self.model.width,
            }
        } else {
            PanelMode::Docked {
                width: self.model.width,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    //! sidePanels.test.ts, case by case.

    use super::*;

    /// The window less the navigator (252 px at 100 %).
    const NAV: f32 = 252.0;

    fn input(patch: impl FnOnce(&mut SidePanelsInput)) -> SidePanelsInput {
        let mut input = SidePanelsInput {
            room: 1600.0 - NAV,
            scale: 1.0,
            inspector: PanelInput {
                open: true,
                width: 296.0,
            },
            dock: None,
            model: None,
        };
        patch(&mut input);
        input
    }
    fn open(width: f32) -> Option<PanelInput> {
        Some(PanelInput { open: true, width })
    }
    fn closed_inspector() -> PanelInput {
        PanelInput {
            open: false,
            width: 296.0,
        }
    }
    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn docks_the_inspector_while_the_terminals_keep_their_room() {
        let inspector = place(input(|_| {})).inspector;
        assert!(inspector.fits);
        assert_eq!(inspector.width, 296.0);
        assert_eq!(inspector.max, INSPECTOR_MAX);
    }

    #[test]
    fn turns_the_inspector_into_a_drawer_in_half_of_a_1920_screen() {
        let inspector = place(input(|i| i.room = 960.0 - NAV)).inspector;
        assert!(!inspector.fits);
        // Same width as docked: switching modes never resizes what is inside.
        assert_eq!(inspector.width, 296.0);
    }

    #[test]
    fn measures_the_switch_in_pixels_so_a_larger_interface_size_switches_sooner() {
        let room = 1280.0 - NAV * 1.3;
        assert!(place(input(|i| i.room = room)).inspector.fits);
        assert!(
            !place(input(|i| {
                i.room = room;
                i.scale = 1.3;
            }))
            .inspector
            .fits
        );
    }

    #[test]
    fn switches_exactly_where_the_terminals_would_drop_below_their_minimum() {
        let at = TERMINALS_MIN + 296.0;
        assert!(place(input(|i| i.room = at)).inspector.fits);
        assert!(!place(input(|i| i.room = at - 1.0)).inspector.fits);
    }

    #[test]
    fn never_lets_a_drag_take_the_terminals_room() {
        let inspector = place(input(|i| i.room = TERMINALS_MIN + 300.0)).inspector;
        assert_eq!(inspector.max, 300.0);
        // Nor below its own minimum, even when that is all there is.
        let narrow = input(|i| {
            i.room = TERMINALS_MIN + INSPECTOR_MIN;
            i.inspector = PanelInput {
                open: true,
                width: INSPECTOR_MIN,
            };
        });
        assert_eq!(place(narrow).inspector.max, INSPECTOR_MIN);
    }

    #[test]
    fn leaves_a_strip_of_work_area_uncovered_by_a_drawer() {
        let inspector = place(input(|i| {
            i.room = 300.0;
            i.inspector = PanelInput {
                open: true,
                width: 440.0,
            };
        }))
        .inspector;
        assert_eq!(inspector.width, 300.0 - DRAWER_MARGIN);
    }

    #[test]
    fn clamps_the_dock_at_render_not_only_while_it_is_dragged() {
        // 1000 px window, no navigator, no inspector: a stored 420 would
        // leave 580.
        let dock = place(input(|i| {
            i.room = 1000.0;
            i.inspector = closed_inspector();
            i.dock = open(420.0);
        }))
        .dock;
        assert!(dock.fits);
        assert_eq!(dock.width, 1000.0 - TERMINALS_MIN);
        assert_eq!(dock.max, 1000.0 - TERMINALS_MIN);
    }

    #[test]
    fn draws_the_dock_s_width_in_points_times_the_interface_size() {
        let dock = place(input(|i| {
            i.room = 2400.0;
            i.scale = 1.25;
            i.inspector = closed_inspector();
            i.dock = open(400.0);
        }))
        .dock;
        assert_eq!(dock.width, 500.0);
    }

    #[test]
    fn turns_the_dock_into_a_drawer_when_even_its_minimum_would_crush_the_terminals() {
        // 1000 px window with the navigator: four panes used to share 328 px.
        let sides = place(input(|i| {
            i.room = 1000.0 - NAV;
            i.dock = open(420.0);
        }));
        assert!(!sides.dock.fits);
        assert!(!sides.inspector.fits);
        assert_eq!(sides.dock.width, 420.0);
    }

    #[test]
    fn makes_the_inspector_a_drawer_before_it_squeezes_an_open_dock_below_its_minimum() {
        let sides = place(input(|i| {
            i.room = 1280.0 - NAV;
            i.dock = open(420.0);
        }));
        assert!(!sides.inspector.fits);
        assert!(sides.dock.fits);
        assert_eq!(sides.dock.width, 1280.0 - NAV - TERMINALS_MIN);
    }

    #[test]
    fn docks_both_when_there_is_room_for_the_inspector_and_the_dock_s_minimum() {
        let room = TERMINALS_MIN + 296.0 + DOCK_MIN;
        let sides = place(input(|i| {
            i.room = room;
            i.dock = open(420.0);
        }));
        assert!(sides.inspector.fits);
        assert!(sides.dock.fits);
        // The dock gives way first: down to its minimum, never past it.
        assert_eq!(sides.dock.width, DOCK_MIN);
        assert_eq!(sides.inspector.max, 296.0);
    }

    #[test]
    fn ignores_a_dock_that_is_closed_or_off_screen_with_the_board_or_memory_view() {
        let room = TERMINALS_MIN + 296.0;
        assert!(
            place(input(|i| {
                i.room = room;
                i.dock = Some(PanelInput {
                    open: false,
                    width: 420.0,
                });
            }))
            .inspector
            .fits
        );
        assert!(place(input(|i| i.room = room)).inspector.fits);
    }

    #[test]
    fn ignores_a_dock_that_would_be_a_drawer_itself_when_placing_the_inspector() {
        // Too narrow for the dock alone, wide enough for a narrow inspector.
        let sides = place(input(|i| {
            i.room = TERMINALS_MIN + 250.0;
            i.inspector = PanelInput {
                open: true,
                width: 250.0,
            };
            i.dock = open(420.0);
        }));
        assert!(!sides.dock.fits);
        assert!(sides.inspector.fits);
    }

    #[test]
    fn gives_a_closed_inspector_s_room_to_the_dock() {
        let room = 1280.0 - NAV;
        let with_open = place(input(|i| {
            i.room = room;
            i.dock = open(600.0);
        }));
        let with_closed = place(input(|i| {
            i.room = room;
            i.inspector = closed_inspector();
            i.dock = open(600.0);
        }));
        // Open but without room, it is a drawer, which takes nothing either.
        assert_eq!(with_open.dock.width, with_closed.dock.width);
    }

    // with the 3D preview

    fn with_model(patch: impl FnOnce(&mut SidePanelsInput)) -> SidePanelsInput {
        input(|i| {
            i.model = open(440.0);
            patch(i);
        })
    }

    #[test]
    fn places_nothing_differently_while_no_model_is_open() {
        let before = place(input(|i| i.dock = open(420.0)));
        let after = place(input(|i| {
            i.dock = open(420.0);
            i.model = None;
        }));
        assert_eq!(after.inspector, before.inspector);
        assert_eq!(after.dock, before.dock);
    }

    #[test]
    fn docks_the_preview_at_its_width_when_the_terminals_keep_their_room() {
        let sides = place(with_model(|i| i.room = 1920.0 - NAV));
        assert!(sides.model.fits);
        assert_eq!(sides.model.width, 440.0);
        assert!(sides.inspector.fits);
    }

    #[test]
    fn makes_the_inspector_a_drawer_before_it_squeezes_the_preview() {
        let sides = place(with_model(|i| i.room = 1280.0 - NAV));
        assert!(!sides.inspector.fits);
        assert!(sides.model.fits);
        assert!(near(
            sides.model.width,
            440f32.min(1280.0 - NAV - TERMINALS_MIN)
        ));
    }

    #[test]
    fn keeps_the_preview_beside_the_terminals_and_makes_the_dock_the_drawer_when_only_one_fits() {
        let sides = place(with_model(|i| {
            i.room = TERMINALS_MIN + MODEL_MIN + 100.0;
            i.inspector = closed_inspector();
            i.dock = open(420.0);
        }));
        assert!(sides.model.fits);
        assert!(!sides.dock.fits);
    }

    #[test]
    fn leaves_the_dock_its_minimum_beside_the_preview_while_there_is_room_for_both() {
        let room = TERMINALS_MIN + MODEL_MIN + DOCK_MIN + 60.0;
        let sides = place(with_model(|i| {
            i.room = room;
            i.inspector = closed_inspector();
            i.dock = open(420.0);
        }));
        assert!(sides.model.fits);
        assert!(sides.dock.fits);
        assert!(near(
            sides.model.width + sides.dock.width,
            room - TERMINALS_MIN
        ));
        assert_eq!(sides.dock.width, DOCK_MIN);
    }

    #[test]
    fn becomes_a_drawer_in_half_of_a_1920_screen_the_same_width_as_docked() {
        let model = place(with_model(|i| i.room = 960.0 - NAV)).model;
        assert!(!model.fits);
        assert_eq!(model.width, 440.0);
    }

    #[test]
    fn never_lets_a_drag_take_the_terminals_room_with_the_preview() {
        let model = place(with_model(|i| {
            i.room = TERMINALS_MIN + 500.0;
            i.inspector = closed_inspector();
        }))
        .model;
        assert_eq!(model.max, 500.0);
    }

    #[test]
    fn everything_fits_until_the_row_is_measured() {
        let sides = place(with_model(|i| {
            i.room = f32::INFINITY;
            i.dock = open(420.0);
        }));
        assert!(sides.inspector.fits && sides.dock.fits && sides.model.fits);
        assert_eq!(sides.model.width, 440.0);
        assert_eq!(sides.dock.width, 420.0);
        assert_eq!(sides.inspector.max, INSPECTOR_MAX);
    }
}
