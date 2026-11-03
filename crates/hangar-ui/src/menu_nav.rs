//! Where an arrow key lands in a list of entries: a menu, the session picker,
//! the settings sidebar. A port of `src/lib/menuNav.ts`, with the pieces a
//! menu with separators and greyed lines needs on top.
//!
//! `current` is the focused entry, or `None` when focus is outside the list
//! (on the button that opened it, say): Down then enters at the top and Up at
//! the bottom. A menu wraps around its ends, the way the system's do; a list
//! stops at them. `None` back means there is nowhere to go.

use iced::keyboard::{Key, key::Named};

/// One entry down or up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Next,
    Previous,
}

/// Where the keys of a list move to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Step(Step),
    First,
    Last,
}

/// Where a step from `current` lands among `count` entries.
pub fn step_index(current: Option<usize>, count: usize, step: Step, wrap: bool) -> Option<usize> {
    if count == 0 {
        return None;
    }
    // A stale index past the end is as good as none.
    let Some(current) = current.filter(|current| *current < count) else {
        return Some(match step {
            Step::Next => 0,
            Step::Previous => count - 1,
        });
    };
    match step {
        Step::Next if current + 1 < count => Some(current + 1),
        Step::Previous if current > 0 => Some(current - 1),
        Step::Next => wrap.then_some(0),
        Step::Previous => wrap.then_some(count - 1),
    }
}

/// The step an arrow key asks for, or `None` for any other key.
pub fn arrow_step(key: &Key) -> Option<Step> {
    match key {
        Key::Named(Named::ArrowDown) => Some(Step::Next),
        Key::Named(Named::ArrowUp) => Some(Step::Previous),
        _ => None,
    }
}

/// What a key does in a menu: the arrows step, Home and End jump.
pub fn menu_move(key: &Key) -> Option<Move> {
    match key {
        Key::Named(Named::Home) => Some(Move::First),
        Key::Named(Named::End) => Some(Move::Last),
        other => arrow_step(other).map(Move::Step),
    }
}

/// [`step_index`] over a list where some entries cannot take the highlight
/// (separators, headings, lines that are greyed out): `selectable[i]` says
/// whether entry `i` can. Skips over the others, and gives up after a full
/// turn.
pub fn step_selectable(
    current: Option<usize>,
    selectable: &[bool],
    step: Step,
    wrap: bool,
) -> Option<usize> {
    let count = selectable.len();
    let mut at = current;
    for _ in 0..count {
        let next = step_index(at, count, step, wrap)?;
        if selectable[next] {
            return Some(next);
        }
        at = Some(next);
    }
    None
}

/// Where `movement` takes the highlight from `current` among entries of
/// which only some are `selectable`.
pub fn move_selectable(
    current: Option<usize>,
    selectable: &[bool],
    movement: Move,
    wrap: bool,
) -> Option<usize> {
    match movement {
        Move::Step(step) => step_selectable(current, selectable, step, wrap),
        Move::First => selectable.iter().position(|can| *can),
        Move::Last => selectable.iter().rposition(|can| *can),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod step_index {
        use super::*;

        #[test]
        fn has_nowhere_to_go_in_an_empty_list() {
            assert_eq!(step_index(None, 0, Step::Next, true), None);
            assert_eq!(step_index(Some(0), 0, Step::Previous, false), None);
        }

        #[test]
        fn enters_from_outside_at_the_top_going_down_at_the_bottom_going_up() {
            assert_eq!(step_index(None, 4, Step::Next, true), Some(0));
            assert_eq!(step_index(None, 4, Step::Previous, true), Some(3));
            assert_eq!(step_index(None, 4, Step::Previous, false), Some(3));
        }

        #[test]
        fn treats_a_stale_index_past_the_end_as_outside() {
            assert_eq!(step_index(Some(7), 3, Step::Next, false), Some(0));
        }

        #[test]
        fn moves_one_entry_at_a_time_inside_the_list() {
            assert_eq!(step_index(Some(1), 4, Step::Next, false), Some(2));
            assert_eq!(step_index(Some(2), 4, Step::Previous, false), Some(1));
        }

        #[test]
        fn wraps_around_the_ends_of_a_menu() {
            assert_eq!(step_index(Some(3), 4, Step::Next, true), Some(0));
            assert_eq!(step_index(Some(0), 4, Step::Previous, true), Some(3));
        }

        #[test]
        fn stops_at_the_ends_of_a_list() {
            assert_eq!(step_index(Some(3), 4, Step::Next, false), None);
            assert_eq!(step_index(Some(0), 4, Step::Previous, false), None);
        }

        #[test]
        fn stays_put_on_a_single_entry_that_wraps() {
            assert_eq!(step_index(Some(0), 1, Step::Next, true), Some(0));
            assert_eq!(step_index(Some(0), 1, Step::Previous, true), Some(0));
        }
    }

    mod arrow_step {
        use super::*;

        #[test]
        fn reads_up_and_down_and_nothing_else() {
            assert_eq!(arrow_step(&Key::Named(Named::ArrowDown)), Some(Step::Next));
            assert_eq!(
                arrow_step(&Key::Named(Named::ArrowUp)),
                Some(Step::Previous)
            );
            assert_eq!(arrow_step(&Key::Named(Named::ArrowLeft)), None);
            assert_eq!(arrow_step(&Key::Named(Named::Tab)), None);
            assert_eq!(arrow_step(&Key::Character("j".into())), None);
        }
    }

    mod menu_move {
        use super::*;

        #[test]
        fn adds_home_and_end_to_the_arrows() {
            assert_eq!(menu_move(&Key::Named(Named::Home)), Some(Move::First));
            assert_eq!(menu_move(&Key::Named(Named::End)), Some(Move::Last));
            assert_eq!(
                menu_move(&Key::Named(Named::ArrowDown)),
                Some(Move::Step(Step::Next))
            );
            assert_eq!(menu_move(&Key::Named(Named::PageDown)), None);
        }
    }

    mod selectable {
        use super::*;

        // Item, separator, item, greyed, item.
        const LINES: [bool; 5] = [true, false, true, false, true];

        #[test]
        fn skips_what_cannot_take_the_highlight() {
            assert_eq!(step_selectable(Some(0), &LINES, Step::Next, true), Some(2));
            assert_eq!(step_selectable(Some(2), &LINES, Step::Next, true), Some(4));
            assert_eq!(
                step_selectable(Some(4), &LINES, Step::Previous, true),
                Some(2)
            );
        }

        #[test]
        fn wraps_past_the_ends_and_over_what_it_skips() {
            assert_eq!(step_selectable(Some(4), &LINES, Step::Next, true), Some(0));
            assert_eq!(
                step_selectable(Some(0), &LINES, Step::Previous, true),
                Some(4)
            );
        }

        #[test]
        fn stops_at_the_ends_of_a_list_that_does_not_wrap() {
            assert_eq!(step_selectable(Some(4), &LINES, Step::Next, false), None);
            assert_eq!(
                step_selectable(Some(0), &LINES, Step::Previous, false),
                None
            );
        }

        #[test]
        fn enters_from_outside_on_the_first_or_last_line_that_can_take_it() {
            let lines = [false, true, true, false];
            assert_eq!(step_selectable(None, &lines, Step::Next, true), Some(1));
            assert_eq!(step_selectable(None, &lines, Step::Previous, true), Some(2));
        }

        #[test]
        fn has_nowhere_to_go_when_nothing_can_take_it() {
            assert_eq!(step_selectable(None, &[false, false], Step::Next, true), None);
            assert_eq!(
                move_selectable(None, &[false, false], Move::First, true),
                None
            );
        }

        #[test]
        fn home_and_end_land_on_the_first_and_last_selectable_line() {
            let lines = [false, true, false, true, false];
            assert_eq!(move_selectable(Some(3), &lines, Move::First, true), Some(1));
            assert_eq!(move_selectable(Some(1), &lines, Move::Last, true), Some(3));
        }
    }
}
