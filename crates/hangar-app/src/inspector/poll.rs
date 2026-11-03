//! git's view of the pane's working tree, kept fresh while the section is on
//! screen (`useGitChanges` of `InspectorFiles.tsx`).
//!
//! The web app ran this as an effect over `[folder, enabled, trigger]`. Here
//! it is a subscription whose identity is that same list ([`PollKey`]): when
//! the pane, its folder, the files it edited or its turn change, or the
//! window comes back from being away, iced drops the old stream and starts a
//! new one, which is exactly what React did with the old effect.
//!
//! The stream looks once shortly after it starts ([`Timing::debounce`]), so a
//! burst of edits is one look, then every [`Timing::interval`]. The next look
//! is booked only once this one answered, so reads never overlap, and **a
//! failure ends the stream**: a folder git cannot read (not a repository) is
//! asked again only when something changes, never on the timer, because
//! polling a failure would only spawn processes.

use std::future::Future;
use std::hash::{Hash, Hasher};
use std::time::Duration;

use hangar_core::git::Changes;
use iced::futures::{Stream, stream};

use crate::state::PaneId;

/// What a subscription is for: the pane, the folder asked about, and a
/// digest of what makes the answer stale.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PollKey {
    pub pane: PaneId,
    pub folder: String,
    pub trigger: u64,
}

/// Below this, a refresh asked for twice in a row is asked for once.
pub const DEBOUNCE: Duration = Duration::from_millis(400);
/// The slowest refresh that still catches what an agent did by hand.
pub const INTERVAL: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub debounce: Duration,
    pub interval: Duration,
}

impl Timing {
    pub const LIVE: Timing = Timing {
        debounce: DEBOUNCE,
        interval: INTERVAL,
    };
}

/// A digest of what makes a reading stale: the files the agent reported
/// editing, and when its turn started. A turn ending is when files settle,
/// so a new turn is worth a fresh look too.
pub fn trigger(edited: &[String], turn_started_at: Option<u64>) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    edited.hash(&mut hasher);
    turn_started_at.hash(&mut hasher);
    hasher.finish()
}

/// One look at the folder, and then the next, for as long as they succeed.
///
/// `fetch` is the look (`git_changes(folder, None)` in the app); it is a
/// parameter so that the timing can be tested without git or a minute.
pub fn stream<F, Fut>(
    key: PollKey,
    timing: Timing,
    fetch: F,
) -> impl Stream<Item = (PollKey, Result<Changes, String>)>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Result<Changes, String>>,
{
    stream::unfold(Some((key, fetch, true)), move |state| async move {
        let (key, fetch, first) = state?;
        tokio::time::sleep(if first {
            timing.debounce
        } else {
            timing.interval
        })
        .await;
        let result = fetch(key.folder.clone()).await;
        // A failure is the last item: it is shown, and nothing more is asked
        // until the key changes.
        let next = result.is_ok().then(|| (key.clone(), fetch, false));
        Some(((key, result), next))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::Instant;

    use iced::futures::StreamExt;

    fn key() -> PollKey {
        PollKey {
            pane: "p1".into(),
            folder: "/work/app".into(),
            trigger: trigger(&[], None),
        }
    }

    const FAST: Timing = Timing {
        debounce: Duration::from_millis(5),
        interval: Duration::from_millis(10),
    };

    fn changes(root: &str) -> Changes {
        Changes {
            root: root.into(),
            ..Changes::default()
        }
    }

    #[test]
    fn what_makes_a_reading_stale_changes_the_key() {
        let at = 1_700_000_000_000_u64;
        let edited = vec!["/a.rs".to_string(), "/b.rs".to_string()];
        let base = trigger(&edited, Some(at));
        assert_eq!(base, trigger(&edited, Some(at)));
        // A new edit, a new turn, an end of turn: a new look.
        assert_ne!(
            base,
            trigger(&["/c.rs".to_string(), "/a.rs".to_string()], Some(at))
        );
        assert_ne!(base, trigger(&edited, Some(at + 1_000)));
        assert_ne!(base, trigger(&edited, None));
        assert_ne!(trigger(&[], None), trigger(&["/a.rs".to_string()], None));
    }

    #[tokio::test]
    async fn it_looks_once_then_again_while_the_answers_are_good() {
        let asked = Rc::new(Cell::new(0));
        let counter = asked.clone();
        let started = Instant::now();
        let mut looks = Box::pin(stream(key(), FAST, move |folder| {
            counter.set(counter.get() + 1);
            async move { Ok(changes(&folder)) }
        }));

        let (answered, first) = looks.next().await.unwrap();
        assert_eq!(answered, key());
        assert_eq!(first.unwrap().root, "/work/app");
        assert!(started.elapsed() >= FAST.debounce);
        assert_eq!(asked.get(), 1);

        looks.next().await.unwrap().1.unwrap();
        looks.next().await.unwrap().1.unwrap();
        assert_eq!(asked.get(), 3);
        // The later looks wait for the interval, not the debounce.
        assert!(started.elapsed() >= FAST.debounce + 2 * FAST.interval);
    }

    #[tokio::test]
    async fn a_failure_is_the_last_thing_the_stream_says() {
        let asked = Rc::new(Cell::new(0));
        let counter = asked.clone();
        let mut looks = Box::pin(stream(key(), FAST, move |_| {
            counter.set(counter.get() + 1);
            async { Err("not a git repository".to_string()) }
        }));

        let (_, answer) = looks.next().await.unwrap();
        assert_eq!(answer.unwrap_err(), "not a git repository");
        assert!(looks.next().await.is_none());
        assert_eq!(asked.get(), 1);
    }

    #[tokio::test]
    async fn the_next_look_waits_for_this_one_to_answer() {
        // A slow git (longer than the interval) is never asked twice at once.
        let running = Rc::new(Cell::new(0));
        let overlap = Rc::new(Cell::new(false));
        let (now, seen) = (running.clone(), overlap.clone());
        let mut looks = Box::pin(stream(key(), FAST, move |folder| {
            let (now, seen) = (now.clone(), seen.clone());
            async move {
                now.set(now.get() + 1);
                if now.get() > 1 {
                    seen.set(true);
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
                now.set(now.get() - 1);
                Ok(changes(&folder))
            }
        }));
        for _ in 0..3 {
            looks.next().await.unwrap().1.unwrap();
        }
        assert!(!overlap.get());
    }
}
