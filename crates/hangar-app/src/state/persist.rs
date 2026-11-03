//! Writing `state.json` back (inventory §1.4, "Writer rules"): only the main
//! window's model saves, never before hydration, 400 ms after the last change,
//! the whole state each time, and once more on exit so a change made just
//! before quitting is not lost (the TS lost it).
//!
//! The mechanism: a change bumps `Model::revision`; the shell answers with a
//! `Message::SaveDue(revision)` 400 ms later (`schedule`), and saves only if
//! no later change came meanwhile (`is_due`). The write itself runs on a
//! blocking thread (`services::core::save_state`), through a `Writer` that
//! never lets an older revision land over a newer one: the exit's own write
//! can overtake a debounced one still running.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use iced::Task;
use serde_json::Value;

use super::types::AppState;
use super::unread::Unread;
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

/// The JSON `save_state` writes: the whole state, camelCase as the TS wrote
/// it, with the values set aside at load put back (unread.rs).
pub fn to_json(state: &AppState, unread: &Unread) -> Value {
    let mut value = serde_json::to_value(state).unwrap_or_else(|err| {
        // The types here only hold what serde_json writes; this cannot
        // happen, and an empty object would be worse than no save at all.
        panic!("the state could not be written as JSON: {err}")
    });
    unread.restore(&mut value);
    value
}

/// The revisions written so far, shared by every write: a write waits for the
/// one before it, and gives up when a newer revision was written meanwhile.
#[derive(Debug, Clone, Default)]
pub struct Writer(Arc<Mutex<u64>>);

impl Writer {
    /// Writes `revision` with `write`, unless a later revision already went
    /// out. Whether it wrote.
    pub fn write(
        &self,
        revision: u64,
        write: impl FnOnce() -> Result<(), String>,
    ) -> Result<bool, String> {
        let mut written = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if *written >= revision {
            return Ok(false);
        }
        write()?;
        *written = revision;
        Ok(true)
    }
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

    #[test]
    fn an_older_revision_never_lands_over_a_newer_one() {
        let writer = Writer::default();
        let mut files = Vec::new();
        assert_eq!(
            writer.write(5, || {
                files.push(5);
                Ok(())
            }),
            Ok(true)
        );
        assert_eq!(
            writer.write(4, || {
                files.push(4);
                Ok(())
            }),
            Ok(false)
        );
        assert_eq!(
            writer.write(5, || {
                files.push(5);
                Ok(())
            }),
            Ok(false)
        );
        assert_eq!(
            writer.write(6, || Err("disk full".into())),
            Err("disk full".into())
        );
        assert_eq!(
            writer.write(6, || {
                files.push(6);
                Ok(())
            }),
            Ok(true)
        );
        assert_eq!(files, [5, 6]);
    }
}
