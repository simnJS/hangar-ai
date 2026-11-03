//! The board as a panel beside the terminals (`BoardDock.tsx`): its resize
//! grip. The room it is given is the shell's to say (`Panels::of(..).dock`).
//!
//! The panel sits beside the grids rather than over them, so the terminals
//! only get narrower, and nothing is remounted: no PTY restarts when the panel
//! opens, closes or changes width. Where that would leave the terminals too
//! little room the shell draws it as a drawer over them instead
//! (`BoardVariant::Dock { drawer: true }`), opened on demand; the shell owns
//! that, and Escape, the outside click and where the focus goes after it.

use hangar_i18n::Key;
use hangar_ui::tip;
use iced::keyboard::{Key as Pressed, key::Named};
use iced::widget::{Space, row};
use iced::{Element, Length, Point, mouse};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::shell::side_panels::DOCK_MIN;

use super::hit::{Look, hit};
use super::look;
use super::{Msg, State};

/// How far an arrow key moves the panel's edge, in interface points.
pub const KEY_STEP: f32 = 24.0;

/// The width the panel is given back by a double click on its grip
/// (`DEFAULT_SETTINGS.boardDockWidth`).
pub fn default_width() -> f32 {
    crate::state::Settings::default().board_dock_width as f32
}

/// `clampDock`, bounded by the room actually available, which a window resize
/// changes: not under the minimum, not over the maximum, whole points.
pub fn clamp_width(width: f32, max: f32) -> f32 {
    max.min(DOCK_MIN.max(width.round()))
}

/// A drag of the grip. The panel grows leftwards, into the terminals, so the
/// width is how far the pointer has gone left of where it took hold.
#[derive(Debug, Clone)]
pub struct Grip {
    start_x: f32,
    start_width: f32,
    max: f32,
    /// Where the pointer is now.
    x: f32,
}

impl Grip {
    pub fn new(at: Point, width: f32, max: f32) -> Grip {
        Grip {
            start_x: at.x,
            start_width: width,
            max,
            x: at.x,
        }
    }

    pub fn moved_to(&mut self, x: f32) {
        self.x = x;
    }

    /// The width the panel gets if the pointer is let go now.
    pub fn pending(&self) -> f32 {
        self.width_at(self.x)
    }

    pub fn width_at(&self, x: f32) -> f32 {
        clamp_width(self.start_width + (self.start_x - x), self.max)
    }
}

/// The width an arrow key asks for: Left widens (the panel grows leftwards),
/// Right narrows it.
pub fn key_width(width: f32, max: f32, widen: bool) -> f32 {
    let step = if widen { KEY_STEP } else { -KEY_STEP };
    clamp_width(width + step, max)
}

/// The grip: a line on the panel's left edge, drawn only while it is used
/// (`.board-dock__grip`).
pub(super) fn grip<'a>(st: &'a State, ctx: Ctx<'a>, width: f32, max: f32) -> Element<'a, Message> {
    let t = ctx.theme;
    let line = hit(Space::new().width(look::GRIP_WIDTH).height(Length::Fill))
        .id("board-dock-grip")
        .on_press(move |at| Message::Board(Msg::GripPress { at, width, max }))
        .on_double_click(Message::Board(Msg::GripReset))
        .on_key(move |key| {
            let nudge = |widen| Message::Board(Msg::GripKey { widen, width, max });
            match key {
                Pressed::Named(Named::ArrowLeft) => Some(nudge(true)),
                Pressed::Named(Named::ArrowRight) => Some(nudge(false)),
                _ => None,
            }
        })
        .interaction(mouse::Interaction::ResizingHorizontally)
        .look(Look {
            hover: Some(t.colors.accent),
            ring: Some(t.colors.accent),
            radius: 0.0,
            inset_x: look::GRIP_INSET,
        })
        .active(st.grip.is_some());
    tip::over(t, line, ctx.t(Key::BoardDockResize))
}

/// The panel: the grip, then the board.
pub(super) fn panel<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    width: f32,
    max: f32,
    board: Element<'a, Message>,
) -> Element<'a, Message> {
    row![grip(st, ctx, width, max), board]
        .height(Length::Fill)
        .width(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_width_between_the_minimum_and_what_the_room_allows() {
        assert_eq!(clamp_width(100.0, 700.0), DOCK_MIN);
        assert_eq!(clamp_width(900.0, 700.0), 700.0);
        assert_eq!(clamp_width(420.4, 700.0), 420.0);
        // A window too narrow for the minimum still gets it.
        assert_eq!(clamp_width(500.0, 280.0), 280.0);
    }

    #[test]
    fn dragging_left_widens_the_panel_and_right_narrows_it() {
        let grip = Grip::new(Point::new(1000.0, 50.0), 420.0, 800.0);
        assert_eq!(grip.width_at(1000.0), 420.0);
        assert_eq!(grip.width_at(940.0), 480.0);
        assert_eq!(grip.width_at(1100.0), 320.0);
        assert_eq!(grip.width_at(1300.0), DOCK_MIN);
        assert_eq!(grip.width_at(0.0), 800.0);
    }

    #[test]
    fn the_arrows_move_the_edge_by_a_step() {
        assert_eq!(key_width(420.0, 800.0, true), 444.0);
        assert_eq!(key_width(420.0, 800.0, false), 396.0);
        assert_eq!(key_width(790.0, 800.0, true), 800.0);
        assert_eq!(key_width(310.0, 800.0, false), DOCK_MIN);
    }

    #[test]
    fn the_default_width_is_the_settings_default() {
        assert_eq!(default_width(), 420.0);
    }

    #[test]
    fn the_pending_width_follows_the_pointer_until_it_is_let_go() {
        let mut grip = Grip::new(Point::new(1000.0, 50.0), 420.0, 800.0);
        assert_eq!(grip.pending(), 420.0);
        grip.moved_to(940.0);
        assert_eq!(grip.pending(), 480.0);
        grip.moved_to(5000.0);
        assert_eq!(grip.pending(), DOCK_MIN);
    }
}
