//! Writing `state.json` back (inventory §1.4, "Writer rules"): only the main
//! window's model saves, never before hydration, 400 ms after the last change,
//! the whole state each time, and once more on exit so a change made just
//! before quitting is not lost (the TS lost it).
//!
//! The mechanism: a change bumps `Model::revision`; the shell answers with a
//! `Message::SaveDue(revision)` 400 ms later (`schedule`), and saves only if
//! no later change came meanwhile (`is_due`). The write itself runs on a
//! blocking thread (`services::core::save_state`).

use std::time::Duration;

use iced::Task;

use crate::message::Message;

/// How long the model has to be still before it is written.
pub const SAVE_DEBOUNCE: Duration = Duration::from_millis(400);

/// The `SaveDue` that fires once the model has been still long enough.
pub fn schedule(revision: u64) -> Task<Message> {
    Task::perform(tokio::time::sleep(SAVE_DEBOUNCE), move |()| {
        Message::SaveDue(revision)
    })
}

/// Whether a `SaveDue(due)` is the latest one, and not written yet.
pub fn is_due(due: u64, current: u64, saved: u64) -> bool {
    due == current && current != saved
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_last_change_saves_and_only_once() {
        assert!(is_due(3, 3, 2));
        assert!(!is_due(2, 3, 1), "a later change is still settling");
        assert!(!is_due(3, 3, 3), "already written");
    }
}
