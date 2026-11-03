//! Moving a card between the board's columns (`src/lib/boardDrag.ts`, tested
//! by `boardDrag.test.ts`).
//!
//! The web app used pointer events rather than HTML5 drag-and-drop, which
//! never reached its drop on Windows. The native board has no drag-and-drop of
//! the operating system either: a press on a card, the pointer's travel and
//! the release are ordinary events, so these rules carry over unchanged.
//!
//! Where the web app asked the DOM for the columns' rectangles, the native
//! board lets each column say when the pointer is over it (iced's
//! `mouse_area` enter and exit), because a rectangle measured in a scrolled
//! list is not in the window's coordinates. [`column_at_point`] stays as the
//! rule those events follow, shared edge included, and is what the tests hold
//! the behaviour to.

use iced::mouse::Button;
use iced::{Point, Rectangle};

use super::model::Column;

/// How far the pointer has to travel before a press becomes a drag; below it,
/// the press is a click and opens the card.
pub const DRAG_THRESHOLD: f32 = 5.0;

/// True once the pointer has left `from` by at least `threshold` pixels.
pub fn moved_beyond(from: Point, to: Point, threshold: f32) -> bool {
    (to.x - from.x).hypot(to.y - from.y) >= threshold
}

/// The column whose box holds the point, or `None` when it is over none. A
/// point on the edge shared by two boxes belongs to the second.
pub fn column_at_point<C: Copy>(point: Point, columns: &[(C, Rectangle)]) -> Option<C> {
    let mut found = None;
    for (column, box_) in columns {
        let (left, right) = (box_.x, box_.x + box_.width);
        let (top, bottom) = (box_.y, box_.y + box_.height);
        if point.x >= left && point.x < right && point.y >= top && point.y <= bottom {
            found = Some(*column);
        }
    }
    found
}

/// Only the primary button drags, and never from a control inside the card
/// (the assignee's focus button, a link): those keep their own click. In the
/// window a control that took the press has already captured the event, which
/// is what `inside_control` says.
pub fn starts_drag(button: Button, inside_control: bool) -> bool {
    button == Button::Left && !inside_control
}

/// A press on a card, from the press to the release: a click until the
/// pointer has travelled, then a drag, which lands in the column the pointer
/// is over when it is let go.
#[derive(Debug, Clone)]
pub struct Press {
    pub task: String,
    pub from: Column,
    start: Point,
    pub moved: bool,
    /// The column under the pointer: the one it began in, until it leaves.
    pub over: Option<Column>,
}

/// How a press ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ended {
    /// It never became a drag: the card was clicked.
    Click(String),
    /// The card was let go over another column. No dependency check: the
    /// backend decides what a blocked card may do.
    Drop { task: String, to: Column },
    /// A drag that ended where it began, or over no column.
    Nothing,
}

impl Press {
    pub fn new(task: String, from: Column, at: Point) -> Press {
        Press {
            task,
            from,
            start: at,
            moved: false,
            over: Some(from),
        }
    }

    /// The pointer moved. Returns true the moment the press becomes a drag.
    pub fn pointer_moved(&mut self, to: Point) -> bool {
        if self.moved || !moved_beyond(self.start, to, DRAG_THRESHOLD) {
            return false;
        }
        self.moved = true;
        true
    }

    pub fn enter(&mut self, column: Column) {
        self.over = Some(column);
    }

    /// Leaving a column the pointer is no longer in; entering the next one
    /// may have been told first, which must not be undone.
    pub fn leave(&mut self, column: Column) {
        if self.over == Some(column) {
            self.over = None;
        }
    }

    pub fn release(self) -> Ended {
        if !self.moved {
            return Ended::Click(self.task);
        }
        match self.over {
            Some(to) if to != self.from => Ended::Drop {
                task: self.task,
                to,
            },
            _ => Ended::Nothing,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Size;

    fn press() -> Press {
        Press::new("t".into(), Column::Todo, Point::new(10.0, 10.0))
    }

    #[test]
    fn a_press_that_barely_moves_is_a_click() {
        let mut press = press();
        assert!(!press.pointer_moved(Point::new(12.0, 11.0)));
        assert_eq!(press.release(), Ended::Click("t".into()));
    }

    #[test]
    fn a_press_becomes_a_drag_once_and_lands_where_it_is_let_go() {
        let mut press = press();
        assert!(press.pointer_moved(Point::new(40.0, 10.0)));
        // Already a drag: not told again.
        assert!(!press.pointer_moved(Point::new(80.0, 10.0)));
        press.leave(Column::Todo);
        press.enter(Column::Review);
        assert_eq!(
            press.release(),
            Ended::Drop {
                task: "t".into(),
                to: Column::Review
            }
        );
    }

    #[test]
    fn a_drag_over_its_own_column_or_none_moves_nothing() {
        let mut press = press();
        press.pointer_moved(Point::new(40.0, 10.0));
        assert_eq!(press.clone().release(), Ended::Nothing);
        press.leave(Column::Todo);
        assert_eq!(press.release(), Ended::Nothing);
    }

    #[test]
    fn leaving_a_column_does_not_undo_entering_the_next() {
        // The pointer crosses a shared edge: the next column may be told
        // before the one it left.
        let mut press = press();
        press.pointer_moved(Point::new(40.0, 10.0));
        press.enter(Column::Doing);
        press.leave(Column::Todo);
        assert_eq!(press.over, Some(Column::Doing));
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Col {
        Todo,
        Doing,
        Done,
    }

    fn boxes() -> [(Col, Rectangle); 3] {
        let at = |left: f32| Rectangle::new(Point::new(left, 0.0), Size::new(100.0, 500.0));
        [
            (Col::Todo, at(0.0)),
            (Col::Doing, at(100.0)),
            (Col::Done, at(200.0)),
        ]
    }

    #[test]
    fn keeps_a_press_that_barely_moves_a_click() {
        assert!(!moved_beyond(
            Point::new(10.0, 10.0),
            Point::new(12.0, 11.0),
            DRAG_THRESHOLD
        ));
    }

    #[test]
    fn starts_the_drag_at_the_threshold_in_any_direction() {
        let origin = Point::new(0.0, 0.0);
        assert!(moved_beyond(
            origin,
            Point::new(DRAG_THRESHOLD, 0.0),
            DRAG_THRESHOLD
        ));
        assert!(moved_beyond(
            origin,
            Point::new(0.0, -DRAG_THRESHOLD),
            DRAG_THRESHOLD
        ));
        assert!(moved_beyond(origin, Point::new(3.0, 4.0), DRAG_THRESHOLD));
    }

    #[test]
    fn finds_the_column_under_the_pointer() {
        assert_eq!(
            column_at_point(Point::new(50.0, 200.0), &boxes()),
            Some(Col::Todo)
        );
        assert_eq!(
            column_at_point(Point::new(250.0, 10.0), &boxes()),
            Some(Col::Done)
        );
    }

    #[test]
    fn gives_the_shared_edge_to_one_column_only() {
        assert_eq!(
            column_at_point(Point::new(100.0, 10.0), &boxes()),
            Some(Col::Doing)
        );
    }

    #[test]
    fn is_none_outside_every_column() {
        assert_eq!(column_at_point(Point::new(350.0, 10.0), &boxes()), None);
        assert_eq!(column_at_point(Point::new(50.0, 600.0), &boxes()), None);
        assert_eq!(column_at_point::<Col>(Point::new(50.0, 10.0), &[]), None);
    }

    #[test]
    fn drags_with_the_primary_button_only() {
        assert!(starts_drag(Button::Left, false));
        assert!(!starts_drag(Button::Right, false));
        assert!(!starts_drag(Button::Middle, false));
    }

    #[test]
    fn leaves_the_controls_inside_a_card_alone() {
        assert!(!starts_drag(Button::Left, true));
    }
}
