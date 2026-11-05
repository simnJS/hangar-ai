//! The phone's calls that change something, made at most once.
//!
//! The phone tries a call again when it got no answer in time. Without
//! anything to tell the second try from a new call, a prompt the window was
//! still typing when the server gave up on it is typed twice, and a
//! permission answered once is answered again on whatever prompt came next.
//!
//! So each such call carries a key of the phone's own (`Idempotency-Key`),
//! the same on every try. The first try with a key runs the call, in a task
//! of its own that waits on the window well past what one HTTP answer may
//! wait; a try with a key already seen runs nothing, and waits on, or is
//! answered with, what the first one did. A key is kept a few minutes, and a
//! few hundred at most: far past any phone's retries.

use std::collections::hash_map::DefaultHasher;
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::watch;

/// How long a key's outcome is kept for its retries.
pub const KEPT: Duration = Duration::from_secs(5 * 60);
/// How many keys are kept at most; past it, the oldest settled go first.
pub const CAPACITY: usize = 256;

/// What became of a call.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Answer(Value),
    Refused(String),
    /// The request never reached the window: nothing ran, and a retry runs
    /// it.
    Unreached,
    /// The window took it and never said what it did: it may have run.
    Lost,
}

/// A call's key, scoped to the phone that sent it and its op, and what its
/// arguments were: a key sent again with other arguments is a phone's bug,
/// refused rather than answered with another call's outcome.
struct Entry {
    device: String,
    op: String,
    key: String,
    args: u64,
    at: Instant,
    outcome: watch::Receiver<Option<Outcome>>,
}

impl Entry {
    fn settled(&self) -> bool {
        self.outcome.borrow().is_some()
    }
}

/// What a try with a key is to do.
pub enum Begin {
    /// The first: run the call, and send what it did.
    Run(
        watch::Sender<Option<Outcome>>,
        watch::Receiver<Option<Outcome>>,
    ),
    /// A retry: wait on what the first one does, or did.
    Wait(watch::Receiver<Option<Outcome>>),
    /// The key came with other arguments before.
    Mismatch,
    /// Every key kept is a call still running.
    Full,
}

#[derive(Default)]
pub struct Calls {
    entries: Mutex<VecDeque<Entry>>,
}

fn hash_of(args: &Value) -> u64 {
    let mut hasher = DefaultHasher::new();
    args.to_string().hash(&mut hasher);
    hasher.finish()
}

/// A key the phone may send: what `crypto.randomUUID` gives, or the like.
pub fn valid_key(key: &str) -> bool {
    (8..=128).contains(&key.len())
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

impl Calls {
    pub fn begin(&self, device: &str, op: &str, key: &str, args: &Value) -> Begin {
        self.begin_at(device, op, key, args, Instant::now())
    }

    fn begin_at(&self, device: &str, op: &str, key: &str, args: &Value, now: Instant) -> Begin {
        let args = hash_of(args);
        let mut entries = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        entries.retain(|entry| !entry.settled() || now.duration_since(entry.at) < KEPT);
        if let Some(entry) = entries
            .iter()
            .find(|entry| entry.device == device && entry.op == op && entry.key == key)
        {
            if entry.args != args {
                return Begin::Mismatch;
            }
            return Begin::Wait(entry.outcome.clone());
        }
        while entries.len() >= CAPACITY {
            let Some(oldest) = entries.iter().position(Entry::settled) else {
                return Begin::Full;
            };
            entries.remove(oldest);
        }
        let (sender, receiver) = watch::channel(None);
        entries.push_back(Entry {
            device: device.to_string(),
            op: op.to_string(),
            key: key.to_string(),
            args,
            at: now,
            outcome: receiver.clone(),
        });
        Begin::Run(sender, receiver)
    }

    /// Forgets a key, so its next try runs the call: one that never reached
    /// the window.
    pub fn forget(&self, device: &str, op: &str, key: &str) {
        let mut entries = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        entries.retain(|entry| !(entry.device == device && entry.op == op && entry.key == key));
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.lock().unwrap().len()
    }
}

/// Waits on a call's outcome for `limit` at most: `None` while it runs.
pub async fn outcome_within(
    mut receiver: watch::Receiver<Option<Outcome>>,
    limit: Duration,
) -> Option<Outcome> {
    let waited = tokio::time::timeout(limit, receiver.wait_for(Option::is_some)).await;
    match waited {
        Ok(Ok(outcome)) => outcome.clone(),
        // The task that ran it is gone without a word: as good as lost.
        Ok(Err(_)) => Some(Outcome::Lost),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(
        begin: Begin,
    ) -> (
        watch::Sender<Option<Outcome>>,
        watch::Receiver<Option<Outcome>>,
    ) {
        match begin {
            Begin::Run(sender, receiver) => (sender, receiver),
            _ => panic!("expected the call to run"),
        }
    }

    #[tokio::test]
    async fn a_key_seen_again_waits_on_the_first_call_and_never_runs_it_twice() {
        let calls = Calls::default();
        let args = json!({ "pane": "Ben", "text": "hi" });
        let (sender, _) = run(calls.begin("phone", "send", "key-0001", &args));
        let Begin::Wait(retry) = calls.begin("phone", "send", "key-0001", &args) else {
            panic!("a retry must wait");
        };
        assert_eq!(
            outcome_within(retry.clone(), Duration::from_millis(20)).await,
            None
        );
        sender
            .send(Some(Outcome::Answer(json!({ "sent": true }))))
            .unwrap();
        assert_eq!(
            outcome_within(retry, Duration::from_millis(20)).await,
            Some(Outcome::Answer(json!({ "sent": true })))
        );
        // Settled, it is still answered as it was.
        let Begin::Wait(later) = calls.begin("phone", "send", "key-0001", &args) else {
            panic!("a settled key must be answered");
        };
        assert_eq!(
            outcome_within(later, Duration::from_millis(20)).await,
            Some(Outcome::Answer(json!({ "sent": true })))
        );
    }

    #[test]
    fn a_key_is_the_phone_s_and_the_op_s_and_its_arguments_must_match() {
        let calls = Calls::default();
        let args = json!({ "pane": "Ben" });
        let _first = run(calls.begin("phone", "send", "key-0001", &args));
        let _other_phone = run(calls.begin("tablet", "send", "key-0001", &args));
        let _other_op = run(calls.begin("phone", "manager", "key-0001", &args));
        assert!(matches!(
            calls.begin("phone", "send", "key-0001", &json!({ "pane": "Zed" })),
            Begin::Mismatch
        ));
    }

    #[test]
    fn keys_are_kept_a_few_minutes_and_a_few_hundred_at_most() {
        let calls = Calls::default();
        let start = Instant::now();
        let args = json!({});
        let (sender, _) = run(calls.begin_at("phone", "send", "key-old1", &args, start));
        sender.send(Some(Outcome::Answer(json!(1)))).unwrap();
        // Past KEPT, a settled key is forgotten and runs again.
        let later = start + KEPT + Duration::from_secs(1);
        let _again = run(calls.begin_at("phone", "send", "key-old1", &args, later));

        let calls = Calls::default();
        let mut running = Vec::new();
        for n in 0..CAPACITY {
            running.push(run(calls.begin_at(
                "p",
                "send",
                &format!("key-{n:04}"),
                &args,
                start,
            )));
        }
        assert!(matches!(
            calls.begin_at("p", "send", "key-full", &args, start),
            Begin::Full
        ));
        running[0].0.send(Some(Outcome::Lost)).unwrap();
        let _room = run(calls.begin_at("p", "send", "key-full", &args, start));
        assert_eq!(calls.len(), CAPACITY);
    }

    #[test]
    fn a_forgotten_key_runs_again() {
        let calls = Calls::default();
        let args = json!({});
        let _first = run(calls.begin("phone", "send", "key-0001", &args));
        calls.forget("phone", "send", "key-0001");
        let _again = run(calls.begin("phone", "send", "key-0001", &args));
    }

    #[test]
    fn only_a_plain_key_is_taken() {
        assert!(valid_key("0d5c1f3a-9b2e-4c47-8f1e-2a6b7c8d9e0f"));
        assert!(!valid_key("short"));
        assert!(!valid_key("has space in it"));
        assert!(!valid_key(&"k".repeat(129)));
    }
}
