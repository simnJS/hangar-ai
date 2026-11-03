//! Composed text: dead keys, compose sequences and input methods.
//!
//! Text reaches a window by two roads. The platform's own composition (dead
//! keys, the compose key) arrives inside the key event, as its `text`. An
//! input method (IBus, Fcitx, the IME of macOS or Windows) arrives as separate
//! events: the text being composed (`Preedit`), then the final text
//! (`Commit`). Which road a given keystroke takes depends on the desktop and
//! on whether an input method is running, so both are handled here, and
//! neither may produce the text twice or leave it half done.
//!
//! The rules that keep composed characters from sticking:
//!
//! - A dead key sends nothing when it is pressed. The text comes with the key
//!   that completes the sequence, once, as the platform composed it.
//! - A preedit is only ever displayed. It is never sent, and it is dropped by
//!   the commit that ends it, by the input method closing, by a key that types
//!   something else, and by the pane losing the focus.
//! - A commit that repeats what the key event of the same keystroke already
//!   typed is a duplicate and is dropped. The two events of one keystroke
//!   arrive within the same few milliseconds, so the window is short; two
//!   identical characters typed on purpose are never that close.

use std::ops::Range;
use std::time::{Duration, Instant};

use alacritty_terminal::term::TermMode;
use iced::advanced::input_method;

use crate::keys::{KeyEventKind, KeyOptions, KeyPress, encode_key};

/// How close together the two reports of one keystroke are.
const DUPLICATE_WINDOW: Duration = Duration::from_millis(30);

/// Text being composed, shown at the cursor and not yet sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preedit {
    pub text: String,
    /// The part the input method highlights, in bytes of `text`.
    pub selection: Option<Range<usize>>,
}

/// The state of keyboard input between events: the preedit in progress and the
/// last text that was typed, to recognise the same keystroke arriving twice.
#[derive(Debug, Default)]
pub struct InputState {
    preedit: Option<Preedit>,
    last_text: Option<Typed>,
}

/// Which road some typed text came by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Road {
    KeyEvent,
    InputMethod,
}

#[derive(Debug)]
struct Typed {
    text: String,
    at: Instant,
    road: Road,
}

impl InputState {
    pub fn preedit(&self) -> Option<&Preedit> {
        self.preedit.as_ref()
    }

    /// Forgets any composition in progress. Called when the pane loses the
    /// focus: an input method does not keep composing for a window that is not
    /// listening, and a preedit left behind would be drawn forever.
    pub fn reset(&mut self) {
        self.preedit = None;
        self.last_text = None;
    }

    /// The bytes for a key event, or `None` if it sends nothing.
    pub fn key(
        &mut self,
        press: &KeyPress,
        mode: TermMode,
        options: &KeyOptions,
        now: Instant,
    ) -> Option<Vec<u8>> {
        let bytes = encode_key(press, mode, options)?;
        if press.kind == KeyEventKind::Release {
            return Some(bytes);
        }
        // The same text was just committed by the input method: this event is
        // the other half of that keystroke. Only the other road counts, since
        // a key held down repeats faster than the window is long.
        if let Some(last) = &self.last_text
            && last.road == Road::InputMethod
                && press.text.as_deref() == Some(last.text.as_str())
                && bytes == last.text.as_bytes()
                && now.saturating_duration_since(last.at) < DUPLICATE_WINDOW
            {
                self.last_text = None;
                return None;
            }
        // Something was typed, so no composition is still going on.
        self.preedit = None;
        self.last_text = match &press.text {
            Some(text) if bytes == text.as_bytes() => Some(Typed {
                text: text.clone(),
                at: now,
                road: Road::KeyEvent,
            }),
            _ => None,
        };
        Some(bytes)
    }

    /// The bytes for an input-method event, if it completes some text.
    pub fn input_method(&mut self, event: &input_method::Event, now: Instant) -> Option<Vec<u8>> {
        match event {
            input_method::Event::Opened | input_method::Event::Closed => {
                self.reset();
                None
            }
            input_method::Event::Preedit(text, selection) => {
                self.preedit = (!text.is_empty()).then(|| Preedit {
                    text: text.clone(),
                    selection: selection.clone(),
                });
                None
            }
            input_method::Event::Commit(text) => {
                self.preedit = None;
                let text: String = text.chars().filter(|c| !c.is_control() || *c == '\n').collect();
                if text.is_empty() {
                    return None;
                }
                if let Some(last) = &self.last_text
                    && last.road == Road::KeyEvent
                        && last.text == text
                        && now.saturating_duration_since(last.at) < DUPLICATE_WINDOW
                    {
                        self.last_text = None;
                        return None;
                    }
                self.last_text = Some(Typed {
                    text: text.clone(),
                    at: now,
                    road: Road::InputMethod,
                });
                // A newline from an input method is an Enter.
                Some(text.replace('\n', "\r").into_bytes())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::key::{Named, Physical};
    use iced::keyboard::{Key, Location, Modifiers};

    fn press(key: Key, text: Option<&str>) -> KeyPress {
        KeyPress {
            modified_key: key.clone(),
            key,
            physical_key: Physical::Unidentified(iced::keyboard::key::NativeCode::Unidentified),
            location: Location::Standard,
            modifiers: Modifiers::empty(),
            text: text.map(str::to_owned),
            kind: KeyEventKind::Press,
        }
    }

    fn dead(key: &str) -> KeyPress {
        KeyPress {
            modified_key: Key::Unidentified,
            ..press(Key::Character(key.into()), None)
        }
    }

    fn type_key(state: &mut InputState, p: &KeyPress, at: Instant) -> Option<String> {
        state
            .key(p, TermMode::NONE, &KeyOptions::default(), at)
            .map(|b| String::from_utf8(b).unwrap())
    }

    #[test]
    fn apostrophe_then_e_gives_one_accented_letter() {
        let mut state = InputState::default();
        let t0 = Instant::now();
        assert_eq!(type_key(&mut state, &dead("'"), t0), None);
        let e = press(Key::Character("e".into()), Some("é"));
        assert_eq!(type_key(&mut state, &e, t0 + Duration::from_millis(120)).as_deref(), Some("é"));
        // The next key is just itself: nothing was left pending.
        let a = press(Key::Character("a".into()), Some("a"));
        assert_eq!(type_key(&mut state, &a, t0 + Duration::from_millis(300)).as_deref(), Some("a"));
    }

    #[test]
    fn apostrophe_then_space_gives_the_apostrophe() {
        let mut state = InputState::default();
        let t0 = Instant::now();
        assert_eq!(type_key(&mut state, &dead("'"), t0), None);
        let space = press(Key::Named(Named::Space), Some("'"));
        assert_eq!(type_key(&mut state, &space, t0 + Duration::from_millis(90)).as_deref(), Some("'"));
    }

    #[test]
    fn two_dead_keys_in_a_row_give_the_accent_itself() {
        let mut state = InputState::default();
        let t0 = Instant::now();
        assert_eq!(type_key(&mut state, &dead("'"), t0), None);
        let second = press(Key::Character("'".into()), Some("´"));
        assert_eq!(type_key(&mut state, &second, t0 + Duration::from_millis(80)).as_deref(), Some("´"));
    }

    #[test]
    fn a_cancelled_dead_key_leaves_nothing_behind() {
        let mut state = InputState::default();
        let t0 = Instant::now();
        assert_eq!(type_key(&mut state, &dead("'"), t0), None);
        // Escape cancels the composition on the platform side.
        let esc = press(Key::Named(Named::Escape), None);
        assert_eq!(type_key(&mut state, &esc, t0 + Duration::from_millis(80)).as_deref(), Some("\u{1b}"));
        let e = press(Key::Character("e".into()), Some("e"));
        assert_eq!(type_key(&mut state, &e, t0 + Duration::from_millis(160)).as_deref(), Some("e"));
    }

    #[test]
    fn an_input_method_composes_in_the_preedit_and_commits_once() {
        let mut state = InputState::default();
        let t0 = Instant::now();
        assert_eq!(state.input_method(&input_method::Event::Opened, t0), None);
        assert_eq!(state.input_method(&input_method::Event::Preedit("´".into(), Some(0..2)), t0), None);
        assert_eq!(state.preedit().map(|p| p.text.as_str()), Some("´"));
        let out = state.input_method(&input_method::Event::Commit("é".into()), t0);
        assert_eq!(out, Some("é".as_bytes().to_vec()));
        assert_eq!(state.preedit(), None);
    }

    #[test]
    fn the_key_event_and_the_commit_of_one_keystroke_type_once() {
        let mut state = InputState::default();
        let t0 = Instant::now();
        // The key event first, then the commit a millisecond later.
        let e = press(Key::Character("e".into()), Some("é"));
        assert_eq!(type_key(&mut state, &e, t0).as_deref(), Some("é"));
        let dup = state.input_method(&input_method::Event::Commit("é".into()), t0 + Duration::from_millis(1));
        assert_eq!(dup, None);
        // And the other way round.
        let mut state = InputState::default();
        let first = state.input_method(&input_method::Event::Commit("é".into()), t0);
        assert_eq!(first, Some("é".as_bytes().to_vec()));
        assert_eq!(type_key(&mut state, &e, t0 + Duration::from_millis(1)), None);
    }

    #[test]
    fn typing_the_same_letter_twice_on_purpose_is_not_a_duplicate() {
        let mut state = InputState::default();
        let t0 = Instant::now();
        let l = press(Key::Character("l".into()), Some("l"));
        assert_eq!(type_key(&mut state, &l, t0).as_deref(), Some("l"));
        assert_eq!(type_key(&mut state, &l, t0 + Duration::from_millis(80)).as_deref(), Some("l"));
        // Likewise a commit that comes well after the key.
        let again = state.input_method(&input_method::Event::Commit("l".into()), t0 + Duration::from_millis(400));
        assert_eq!(again, Some(b"l".to_vec()));
    }

    #[test]
    fn a_key_held_down_repeats_faster_than_the_duplicate_window() {
        let mut state = InputState::default();
        let t0 = Instant::now();
        let l = press(Key::Character("l".into()), Some("l"));
        // Repeats 20 ms apart, well inside the window: all of them are typed,
        // because the window only pairs a key event with an input-method one.
        for i in 0..5 {
            let at = t0 + Duration::from_millis(20 * i);
            assert_eq!(type_key(&mut state, &l, at).as_deref(), Some("l"), "repeat {i}");
        }
    }

    #[test]
    fn a_typed_key_ends_a_composition_that_was_left_open() {
        let mut state = InputState::default();
        let t0 = Instant::now();
        state.input_method(&input_method::Event::Preedit("あ".into(), None), t0);
        assert!(state.preedit().is_some());
        let a = press(Key::Character("a".into()), Some("a"));
        type_key(&mut state, &a, t0 + Duration::from_millis(500));
        assert_eq!(state.preedit(), None);
    }

    #[test]
    fn losing_the_focus_drops_the_composition() {
        let mut state = InputState::default();
        let t0 = Instant::now();
        state.input_method(&input_method::Event::Preedit("´".into(), None), t0);
        state.reset();
        assert_eq!(state.preedit(), None);
        // An empty preedit is how an input method clears its own.
        state.input_method(&input_method::Event::Preedit("x".into(), None), t0);
        state.input_method(&input_method::Event::Preedit(String::new(), None), t0);
        assert_eq!(state.preedit(), None);
    }

    #[test]
    fn control_characters_in_a_commit_are_not_typed() {
        let mut state = InputState::default();
        let out = state.input_method(&input_method::Event::Commit("\u{1b}[31m".into()), Instant::now());
        assert_eq!(out, Some(b"[31m".to_vec()));
    }
}
