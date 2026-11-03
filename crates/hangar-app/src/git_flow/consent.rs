//! The consent waiting for the person (`createConsentQueue` and
//! `gitCloneConsent` of `lib/workspaceFromGit.ts`).
//!
//! An agent's `workspace_create` for a repository that is not the person's
//! own waits here until they click Clone or Refuse on the consent card, or
//! until nobody does. The flow runs on tokio and the card on the UI thread, so
//! the queue is shared and thread-safe, and there is one for the whole app
//! (`queue()`), as there was one module-level object in the web app: the tool
//! waits on it, the card answers it.
//!
//! It is independent of the clock (`Timers`) and of the log (`on_answer`), so
//! a test turns the time by hand.

use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hangar_core::remote::ActivityEntry;
use hangar_i18n::{Key, Locale, args};
use tokio::sync::{oneshot, watch};

use super::paths::{folder_name, same_folder, same_repo_url};

/// How long the person has to decide: past it, nobody did.
pub const CONSENT_TIMEOUT_MS: u64 = 3 * 60_000;

/// What the person is shown before anything is cloned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsentRequest {
    pub id: String,
    pub url: String,
    pub host: String,
    pub owner: String,
    pub name: String,
    /// The folder the clone would be made in.
    pub destination: String,
    /// Why Hangar thinks the repository is not theirs.
    pub reason: String,
    /// The pane that asked.
    pub by: Option<String>,
}

/// `Busy`: another repository is already waiting for the person's word on this
/// very folder. It is not an answer of theirs: the card they see belongs to the
/// other repository, so this request is refused at once and nothing is asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentAnswer {
    Allow,
    Refuse,
    Timeout,
    Busy,
}

/// Stops a timer that has not gone off.
pub type Cancel = Box<dyn FnOnce() + Send>;

/// The clock the queue's time-outs run on.
pub trait Timers: Send + Sync {
    /// Runs `run` once, after `after`, unless the returned function is called
    /// first.
    fn set(&self, run: Box<dyn FnOnce() + Send>, after: Duration) -> Cancel;
}

/// The real clock: a task on tokio's runtime, or a thread when the caller is
/// not on one.
pub struct RuntimeTimers;

impl Timers for RuntimeTimers {
    fn set(&self, run: Box<dyn FnOnce() + Send>, after: Duration) -> Cancel {
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                let task = handle.spawn(async move {
                    tokio::time::sleep(after).await;
                    run();
                });
                Box::new(move || task.abort())
            }
            Err(_) => {
                let cancelled = Arc::new(AtomicBool::new(false));
                let flag = cancelled.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(after);
                    if !flag.load(Ordering::SeqCst) {
                        run();
                    }
                });
                Box::new(move || cancelled.store(true, Ordering::SeqCst))
            }
        }
    }
}

/// Told once for each question the person answered or let run out.
pub type OnAnswer = dyn Fn(&ConsentRequest, ConsentAnswer) + Send + Sync;

type Listener = Arc<dyn Fn() + Send + Sync>;

struct Waiting {
    request: ConsentRequest,
    cancel: Option<Cancel>,
    resolvers: Vec<oneshot::Sender<ConsentAnswer>>,
}

#[derive(Default)]
struct Inner {
    /// In the order the questions came: the card shows the first.
    waiting: Vec<Waiting>,
    snapshot: Arc<Vec<ConsentRequest>>,
    listeners: Vec<(u64, Listener)>,
    next_listener: u64,
}

pub struct ConsentQueue {
    timers: Arc<dyn Timers>,
    inner: Mutex<Inner>,
    on_answer: Mutex<Option<Arc<OnAnswer>>>,
    changes: watch::Sender<Arc<Vec<ConsentRequest>>>,
}

/// What a change tells, once the lock is let go.
struct Published {
    snapshot: Arc<Vec<ConsentRequest>>,
    listeners: Vec<Listener>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

impl ConsentQueue {
    pub fn new(timers: Arc<dyn Timers>, on_answer: Option<Arc<OnAnswer>>) -> Arc<ConsentQueue> {
        let (changes, _) = watch::channel(Arc::new(Vec::new()));
        Arc::new(ConsentQueue {
            timers,
            inner: Mutex::new(Inner::default()),
            on_answer: Mutex::new(on_answer),
            changes,
        })
    }

    /// Where the trace of each answer goes from now on.
    pub fn set_on_answer(&self, on_answer: Arc<OnAnswer>) {
        *lock(&self.on_answer) = Some(on_answer);
    }

    /// Whether a trace is set already.
    pub fn has_on_answer(&self) -> bool {
        lock(&self.on_answer).is_some()
    }

    /// Resolves with the person's answer, or `Timeout`. A repeated request for
    /// the same repository in the same folder shares the first one's; another
    /// repository for that folder is `Busy` at once.
    ///
    /// The question is registered when this is called, not when the future is
    /// first polled, so `pending()` shows it straight away.
    pub fn ask(
        self: &Arc<Self>,
        request: ConsentRequest,
        timeout_ms: u64,
    ) -> impl Future<Output = ConsentAnswer> + Send + 'static {
        let (tx, rx) = oneshot::channel();
        let mut early = None;
        let mut published = None;
        {
            let mut inner = lock(&self.inner);
            let taken = inner
                .waiting
                .iter_mut()
                .find(|entry| same_folder(&entry.request.destination, &request.destination));
            match taken {
                // The card shows one repository: only a request for that very
                // one may ride on its answer.
                Some(taken) => {
                    if same_repo_url(&taken.request.url, &request.url) {
                        taken.resolvers.push(tx);
                    } else {
                        early = Some(ConsentAnswer::Busy);
                    }
                }
                None => {
                    let id = request.id.clone();
                    let queue = Arc::downgrade(self);
                    let cancel = self.timers.set(
                        Box::new(move || {
                            if let Some(queue) = queue.upgrade() {
                                queue.settle(&id, ConsentAnswer::Timeout);
                            }
                        }),
                        Duration::from_millis(timeout_ms),
                    );
                    inner.waiting.push(Waiting {
                        request,
                        cancel: Some(cancel),
                        resolvers: vec![tx],
                    });
                    published = Some(publish(&mut inner));
                }
            }
        }
        if let Some(published) = published {
            self.deliver(published);
        }
        async move {
            match early {
                Some(answer) => answer,
                // The sender is only dropped unsent when the queue itself is
                // gone, which nobody could have answered after.
                None => rx.await.unwrap_or(ConsentAnswer::Timeout),
            }
        }
    }

    /// The person's answer; `false` when nothing waits under that id any more.
    pub fn answer(&self, id: &str, allow: bool) -> bool {
        self.settle(
            id,
            if allow {
                ConsentAnswer::Allow
            } else {
                ConsentAnswer::Refuse
            },
        )
    }

    /// The questions waiting, the same list until something changes, so that
    /// a view can tell by the pointer whether it needs drawing again.
    pub fn pending(&self) -> Arc<Vec<ConsentRequest>> {
        lock(&self.inner).snapshot.clone()
    }

    /// The questions waiting, now and at every change.
    pub fn watch(&self) -> watch::Receiver<Arc<Vec<ConsentRequest>>> {
        self.changes.subscribe()
    }

    /// Called after every change; the returned id stops it (`unsubscribe`).
    pub fn subscribe(&self, listener: impl Fn() + Send + Sync + 'static) -> u64 {
        let mut inner = lock(&self.inner);
        inner.next_listener += 1;
        let id = inner.next_listener;
        inner.listeners.push((id, Arc::new(listener)));
        id
    }

    pub fn unsubscribe(&self, id: u64) {
        lock(&self.inner)
            .listeners
            .retain(|(listener, _)| *listener != id);
    }

    fn settle(&self, id: &str, answer: ConsentAnswer) -> bool {
        let (mut entry, published) = {
            let mut inner = lock(&self.inner);
            let Some(index) = inner.waiting.iter().position(|e| e.request.id == id) else {
                return false;
            };
            let entry = inner.waiting.remove(index);
            (entry, publish(&mut inner))
        };
        if let Some(cancel) = entry.cancel.take() {
            cancel();
        }
        self.deliver(published);
        let on_answer = lock(&self.on_answer).clone();
        if let Some(on_answer) = on_answer {
            // A trace that cannot be written never holds the answer back.
            let _ = catch_unwind(AssertUnwindSafe(|| on_answer(&entry.request, answer)));
        }
        for resolver in entry.resolvers {
            let _ = resolver.send(answer);
        }
        true
    }

    fn deliver(&self, published: Published) {
        self.changes.send_replace(published.snapshot);
        for listener in published.listeners {
            listener();
        }
    }
}

fn publish(inner: &mut Inner) -> Published {
    inner.snapshot = Arc::new(inner.waiting.iter().map(|e| e.request.clone()).collect());
    Published {
        snapshot: inner.snapshot.clone(),
        listeners: inner
            .listeners
            .iter()
            .map(|(_, listener)| listener.clone())
            .collect(),
    }
}

/// The app's one queue: the tool waits on it, the consent card answers it.
pub fn queue() -> &'static Arc<ConsentQueue> {
    static QUEUE: OnceLock<Arc<ConsentQueue>> = OnceLock::new();
    QUEUE.get_or_init(|| ConsentQueue::new(Arc::new(RuntimeTimers), None))
}

// ── the trace of each answer ──────────────────────────────────────────

/// The line Hangar's activity log keeps of a consent answer: the repository,
/// the folder's name (never its path, which holds the person's home) and how
/// it ended. In the PC's language, like the rest of the log. `None` for a
/// request refused at once, which the person never saw.
pub fn consent_trace(
    request: &ConsentRequest,
    answer: ConsentAnswer,
    locale: Locale,
    at: u64,
) -> Option<ActivityEntry> {
    let key = match answer {
        ConsentAnswer::Allow => Key::GitConsentLogAllow,
        ConsentAnswer::Refuse => Key::GitConsentLogRefuse,
        ConsentAnswer::Timeout => Key::GitConsentLogTimeout,
        ConsentAnswer::Busy => return None,
    };
    let folder = folder_name(&request.destination);
    let text = hangar_i18n::t_with(
        locale,
        key,
        &args!("repo" => request.url.as_str(), "folder" => folder.as_str()),
    );
    Some(ActivityEntry {
        at,
        workspace: String::new(),
        kind: "gitConsent".into(),
        text: text.into_owned(),
    })
}

/// Writes the line to the activity log (`sink`); a log that cannot be written
/// never gets in the way. Returns whether a line was handed over.
pub fn record_consent(
    sink: &dyn Fn(Vec<ActivityEntry>) -> Result<(), String>,
    request: &ConsentRequest,
    answer: ConsentAnswer,
    locale: Locale,
    at: u64,
) -> bool {
    let Some(entry) = consent_trace(request, answer, locale, at) else {
        return false;
    };
    let _ = sink(vec![entry]);
    true
}

/// Milliseconds since the epoch.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// Sends the trace of every answer to Hangar's activity log (the one Hangar
/// Mobile's "since your last visit" reads), in the system's language as the
/// rest of that log is. Once is enough.
pub fn log_answers_to(remote: Arc<hangar_core::remote::Remote>) {
    let queue = queue();
    if queue.has_on_answer() {
        return;
    }
    queue.set_on_answer(Arc::new(move |request, answer| {
        record_consent(
            &|entries| remote.activity_append(&entries),
            request,
            answer,
            Locale::system(),
            now_ms(),
        );
    }));
}

/// A clock a test turns by hand, for this queue's tests and the flow's.
#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    #[derive(Default)]
    pub struct Clock {
        now: Mutex<u64>,
        due: Mutex<Vec<(u64, Arc<Mutex<Option<Box<dyn FnOnce() + Send>>>>)>>,
    }

    impl Timers for Arc<Clock> {
        fn set(&self, run: Box<dyn FnOnce() + Send>, after: Duration) -> Cancel {
            let slot = Arc::new(Mutex::new(Some(run)));
            let at = *lock(&self.now) + after.as_millis() as u64;
            lock(&self.due).push((at, slot.clone()));
            Box::new(move || {
                lock(&slot).take();
            })
        }
    }

    impl Clock {
        pub fn advance(&self, ms: u64) {
            let now = {
                let mut now = lock(&self.now);
                *now += ms;
                *now
            };
            let due: Vec<_> = lock(&self.due)
                .iter()
                .filter(|(at, _)| *at <= now)
                .map(|(_, slot)| slot.clone())
                .collect();
            for slot in due {
                let run = lock(&slot).take();
                if let Some(run) = run {
                    run();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use super::testing::Clock;
    use super::*;

    /// Drives a future that is already decided to its value.
    fn block_on<T>(future: impl Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("a runtime")
            .block_on(future)
    }

    fn request(id: &str, destination: &str) -> ConsentRequest {
        ConsentRequest {
            id: id.into(),
            url: format!("https://github.com/x/{id}.git"),
            host: "github.com".into(),
            owner: "x".into(),
            name: id.into(),
            destination: destination.into(),
            reason: String::new(),
            by: Some("Ava".into()),
        }
    }

    fn here(id: &str) -> ConsentRequest {
        request(id, &format!("C:\\dev\\{id}"))
    }

    fn queue_on(clock: &Arc<Clock>) -> Arc<ConsentQueue> {
        ConsentQueue::new(Arc::new(clock.clone()), None)
    }

    fn ids(queue: &ConsentQueue) -> Vec<String> {
        queue.pending().iter().map(|entry| entry.id.clone()).collect()
    }

    #[test]
    fn holds_a_request_until_the_person_answers_then_lets_it_go() {
        let queue = queue_on(&Arc::new(Clock::default()));
        let heard = Arc::new(AtomicUsize::new(0));
        let counter = heard.clone();
        queue.subscribe(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        let answer = queue.ask(here("a"), CONSENT_TIMEOUT_MS);
        assert_eq!(ids(&queue), ["a"]);
        assert!(queue.answer("a", true));
        assert_eq!(block_on(answer), ConsentAnswer::Allow);
        assert!(queue.pending().is_empty());
        assert_eq!(heard.load(Ordering::SeqCst), 2);
        assert!(!queue.answer("a", true));
    }

    #[test]
    fn answers_refuse_when_the_person_refuses() {
        let queue = queue_on(&Arc::new(Clock::default()));
        let answer = queue.ask(here("a"), CONSENT_TIMEOUT_MS);
        queue.answer("a", false);
        assert_eq!(block_on(answer), ConsentAnswer::Refuse);
    }

    #[test]
    fn gives_up_after_a_few_minutes_because_nobody_decided() {
        let clock = Arc::new(Clock::default());
        let queue = queue_on(&clock);
        let answer = queue.ask(here("a"), CONSENT_TIMEOUT_MS);
        clock.advance(CONSENT_TIMEOUT_MS - 1);
        assert_eq!(queue.pending().len(), 1);
        clock.advance(1);
        assert_eq!(block_on(answer), ConsentAnswer::Timeout);
        assert!(queue.pending().is_empty());
        assert!(CONSENT_TIMEOUT_MS >= 2 * 60_000);
        assert!(CONSENT_TIMEOUT_MS <= 10 * 60_000);
    }

    #[test]
    fn lets_a_late_click_fall_on_nothing() {
        let clock = Arc::new(Clock::default());
        let queue = queue_on(&clock);
        let answer = queue.ask(here("a"), CONSENT_TIMEOUT_MS);
        clock.advance(CONSENT_TIMEOUT_MS);
        block_on(answer);
        assert!(!queue.answer("a", true));
    }

    #[test]
    fn shares_one_question_between_two_requests_for_one_repository_in_one_folder() {
        let queue = queue_on(&Arc::new(Clock::default()));
        let first = queue.ask(request("a", "C:\\dev\\same"), CONSENT_TIMEOUT_MS);
        let second = queue.ask(
            ConsentRequest {
                url: "https://github.com/x/a".into(),
                ..request("b", "c:/dev/same")
            },
            CONSENT_TIMEOUT_MS,
        );
        assert_eq!(queue.pending().len(), 1);
        queue.answer("a", true);
        assert_eq!(block_on(first), ConsentAnswer::Allow);
        assert_eq!(block_on(second), ConsentAnswer::Allow);
    }

    #[test]
    fn never_lets_another_repository_ride_on_the_answer_to_a_card_it_never_showed() {
        let queue = queue_on(&Arc::new(Clock::default()));
        let good = queue.ask(
            ConsentRequest {
                url: "https://github.com/good/theirs.git".into(),
                ..request("a", "C:\\dev\\theirs")
            },
            CONSENT_TIMEOUT_MS,
        );
        let evil = queue.ask(
            ConsentRequest {
                url: "https://github.com/evil/theirs.git".into(),
                ..request("b", "C:\\dev\\theirs")
            },
            CONSENT_TIMEOUT_MS,
        );
        // Refused at once, with no second card.
        assert_eq!(block_on(evil), ConsentAnswer::Busy);
        let urls: Vec<_> = queue.pending().iter().map(|r| r.url.clone()).collect();
        assert_eq!(urls, ["https://github.com/good/theirs.git"]);
        queue.answer("a", true);
        assert_eq!(block_on(good), ConsentAnswer::Allow);
    }

    #[test]
    fn tells_the_trace_once_per_question_shared_askers_included() {
        let clock = Arc::new(Clock::default());
        let log: Arc<Mutex<Vec<(String, ConsentAnswer)>>> = Arc::default();
        let sink = log.clone();
        let queue = ConsentQueue::new(
            Arc::new(clock.clone()),
            Some(Arc::new(move |request, answer| {
                lock(&sink).push((request.id.clone(), answer));
            })),
        );
        let _ = queue.ask(here("a"), CONSENT_TIMEOUT_MS);
        let _ = queue.ask(here("a"), CONSENT_TIMEOUT_MS);
        let _ = queue.ask(
            ConsentRequest {
                url: "https://github.com/other/a.git".into(),
                ..request("b", "C:\\dev\\a")
            },
            CONSENT_TIMEOUT_MS,
        );
        assert!(lock(&log).is_empty());
        queue.answer("a", false);
        assert_eq!(*lock(&log), [("a".to_string(), ConsentAnswer::Refuse)]);
        let _ = queue.ask(here("c"), CONSENT_TIMEOUT_MS);
        clock.advance(CONSENT_TIMEOUT_MS);
        assert_eq!(
            *lock(&log),
            [
                ("a".to_string(), ConsentAnswer::Refuse),
                ("c".to_string(), ConsentAnswer::Timeout)
            ]
        );
    }

    #[test]
    fn goes_on_when_the_trace_panics() {
        let queue = ConsentQueue::new(
            Arc::new(Arc::new(Clock::default())),
            Some(Arc::new(|_, _| panic!("disk full"))),
        );
        let answer = queue.ask(here("a"), CONSENT_TIMEOUT_MS);
        queue.answer("a", true);
        assert_eq!(block_on(answer), ConsentAnswer::Allow);
    }

    #[test]
    fn keeps_the_order_of_different_questions_and_a_stable_list_between_changes() {
        let queue = queue_on(&Arc::new(Clock::default()));
        let _ = queue.ask(here("a"), CONSENT_TIMEOUT_MS);
        let before = queue.pending();
        let _ = queue.ask(here("b"), CONSENT_TIMEOUT_MS);
        assert_eq!(ids(&queue), ["a", "b"]);
        assert!(Arc::ptr_eq(&queue.pending(), &queue.pending()));
        assert!(!Arc::ptr_eq(&queue.pending(), &before));
    }

    #[test]
    fn a_watcher_sees_the_list_as_it_changes() {
        let queue = queue_on(&Arc::new(Clock::default()));
        let watcher = queue.watch();
        assert!(watcher.borrow().is_empty());
        let _ = queue.ask(here("a"), CONSENT_TIMEOUT_MS);
        assert_eq!(watcher.borrow().len(), 1);
        queue.answer("a", true);
        assert!(watcher.borrow().is_empty());
    }

    #[test]
    fn a_listener_that_unsubscribed_hears_nothing_more() {
        let queue = queue_on(&Arc::new(Clock::default()));
        let heard = Arc::new(AtomicUsize::new(0));
        let counter = heard.clone();
        let id = queue.subscribe(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        let _ = queue.ask(here("a"), CONSENT_TIMEOUT_MS);
        queue.unsubscribe(id);
        queue.answer("a", true);
        assert_eq!(heard.load(Ordering::SeqCst), 1);
    }

    fn asked(destination: &str) -> ConsentRequest {
        ConsentRequest {
            id: "c1".into(),
            url: "https://github.com/evil/theirs.git".into(),
            host: "github.com".into(),
            owner: "evil".into(),
            name: "theirs".into(),
            destination: destination.into(),
            reason: String::new(),
            by: Some("Ava".into()),
        }
    }

    #[test]
    fn the_trace_names_the_repository_the_folders_name_and_how_it_ended_never_the_path() {
        let request = asked("C:\\Users\\me\\Documents\\projects\\theirs");
        let lines: Vec<_> = [
            ConsentAnswer::Allow,
            ConsentAnswer::Refuse,
            ConsentAnswer::Timeout,
        ]
        .into_iter()
        .map(|answer| consent_trace(&request, answer, Locale::En, 5).expect("a line"))
        .collect();
        let texts: Vec<_> = lines.iter().map(|line| line.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "Clone of https://github.com/evil/theirs.git into the folder \"theirs\": you clicked Clone",
                "Clone of https://github.com/evil/theirs.git into the folder \"theirs\": you clicked Refuse",
                "Clone of https://github.com/evil/theirs.git into the folder \"theirs\": no answer in time, nothing cloned",
            ]
        );
        for line in &lines {
            assert_eq!(line.at, 5);
            assert_eq!(line.workspace, "");
            assert_eq!(line.kind, "gitConsent");
            assert!(!line.text.contains("Users"));
            assert!(!line.text.contains("Documents"));
        }
        let french = consent_trace(&request, ConsentAnswer::Allow, Locale::Fr, 5).expect("a line");
        assert!(french.text.contains("tu as cliqué sur Cloner"));
    }

    #[test]
    fn writes_one_line_to_the_log_per_answer_and_none_for_a_refused_at_once_request() {
        let written: Mutex<Vec<ActivityEntry>> = Mutex::default();
        let sink = |entries: Vec<ActivityEntry>| {
            lock(&written).extend(entries);
            Ok(())
        };
        let request = asked("C:\\x\\theirs");
        assert!(record_consent(&sink, &request, ConsentAnswer::Allow, Locale::En, 1));
        assert!(!record_consent(&sink, &request, ConsentAnswer::Busy, Locale::En, 1));
        assert_eq!(lock(&written).len(), 1);
        assert_eq!(lock(&written)[0].kind, "gitConsent");
        // A log that cannot be written is not an error to anyone.
        let broken = |_: Vec<ActivityEntry>| Err("disk".to_string());
        assert!(record_consent(&broken, &request, ConsentAnswer::Refuse, Locale::En, 1));
    }
}
