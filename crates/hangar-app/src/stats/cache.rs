//! The usage statistics, kept for the life of the process and fresh for a
//! minute (`lib/stats.ts`'s store and `usageReader` of `lib/remoteControl.ts`,
//! which are one thing here).
//!
//! Reading the transcripts is the slow part: the first scan after a launch
//! goes through every one, the next only through what changed. So the answer
//! of the last scan is kept with the time it finished, a scan that is under
//! way is joined by whoever asks meanwhile, and the page and Hangar Mobile's
//! `remote_stats` (P20) read from the same cache: a phone asking for the
//! statistics half a minute after the person looked at them rescans nothing.
//!
//! P20 calls [`usage`]. The page uses the pieces: [`UsageCache::last`] to show
//! what there is, [`UsageCache::cached_only`] for the instant first answer,
//! [`UsageCache::scan`] for the real one.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use hangar_core::Host;
use hangar_core::stats::UsageStats;
use iced::futures::FutureExt;
use iced::futures::future::{BoxFuture, Shared};

use super::time::now_ms;

/// A scan this recent is still the truth: switching views does not redo it.
pub const FRESH_FOR_MS: i64 = 60_000;

/// How long Hangar Mobile waits on a scan before answering from the cache
/// (`STATS_WAIT_MS`).
pub const REMOTE_WAIT: Duration = Duration::from_secs(6);

/// What a read answers: the statistics, shared, or what went wrong.
pub type Reply = Result<Arc<UsageStats>, String>;

/// Reads the statistics from the transcripts (`false`) or from the cache
/// alone (`true`): `hangar_core::stats::usage_stats` for the app, a script
/// for the tests.
pub type Reader = Arc<dyn Fn(bool) -> BoxFuture<'static, Result<UsageStats, String>> + Send + Sync>;

/// The last full scan and when it finished, ms.
#[derive(Debug, Clone)]
pub struct Last {
    pub data: Arc<UsageStats>,
    pub at: i64,
}

type Scan = Shared<BoxFuture<'static, Reply>>;

#[derive(Default)]
struct Inner {
    last: Option<Last>,
    scan: Option<Scan>,
}

pub struct UsageCache {
    inner: Mutex<Inner>,
    read: Reader,
    clock: Arc<dyn Fn() -> i64 + Send + Sync>,
}

impl UsageCache {
    pub fn new(read: Reader, clock: Arc<dyn Fn() -> i64 + Send + Sync>) -> Arc<UsageCache> {
        Arc::new(UsageCache {
            inner: Mutex::new(Inner::default()),
            read,
            clock,
        })
    }

    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The last full scan, however old.
    pub fn last(&self) -> Option<Last> {
        self.inner().last.clone()
    }

    /// The last full scan, when it is still fresh.
    pub fn fresh_last(&self) -> Option<Last> {
        let now = (self.clock)();
        self.last().filter(|last| now - last.at < FRESH_FOR_MS)
    }

    /// Whether a scan is under way.
    pub fn scanning(&self) -> bool {
        self.inner().scan.is_some()
    }

    /// What the cache holds, without looking at the disk: instant, even right
    /// after a launch, from the copy the last run left on disk. It is not a
    /// scan and does not count as one.
    pub async fn cached_only(&self) -> Reply {
        (self.read)(true).await.map(Arc::new)
    }

    /// A full scan, or the one already under way: a second caller joins it
    /// rather than starting another. The scan runs on its own task, so it
    /// finishes (and fills the cache) even when everyone who asked has
    /// stopped waiting.
    pub async fn scan(self: &Arc<Self>) -> Reply {
        let scan = {
            let mut inner = self.inner();
            match &inner.scan {
                Some(scan) => scan.clone(),
                None => {
                    let cache = self.clone();
                    let run = tokio::spawn(async move {
                        let result = (cache.read)(false).await.map(Arc::new);
                        let mut inner = cache.inner();
                        inner.scan = None;
                        if let Ok(data) = &result {
                            inner.last = Some(Last {
                                data: data.clone(),
                                at: (cache.clock)(),
                            });
                        }
                        result
                    });
                    let scan: Scan = async move {
                        run.await
                            .unwrap_or_else(|error| Err(format!("the scan stopped: {error}")))
                    }
                    .boxed()
                    .shared();
                    inner.scan = Some(scan.clone());
                    scan
                }
            }
        };
        scan.await
    }

    /// The statistics a scan at a time: the last full one while it is fresh,
    /// a call during a scan joining it, and the cache's answer when the scan
    /// takes longer than `wait` (the scan goes on and the next call gets it).
    pub async fn fresh(self: &Arc<Self>, wait: Duration) -> Reply {
        if let Some(last) = self.fresh_last() {
            return Ok(last.data);
        }
        match tokio::time::timeout(wait, self.scan()).await {
            Ok(result) => result,
            Err(_) => match self.last() {
                Some(last) => Ok(last.data),
                None => self.cached_only().await,
            },
        }
    }
}

/// The cache of this process, made on first use over the app's `Host`.
pub fn shared(host: &Host) -> &'static Arc<UsageCache> {
    static SHARED: OnceLock<Arc<UsageCache>> = OnceLock::new();
    SHARED.get_or_init(|| {
        let host = host.clone();
        UsageCache::new(
            Arc::new(move |cached_only| {
                let host = host.clone();
                async move {
                    hangar_core::stats::usage_stats(host, None, None, Some(cached_only)).await
                }
                .boxed()
            }),
            Arc::new(now_ms),
        )
    })
}

/// What `remote_stats` reads (P20): the statistics, scanned at most once a
/// minute, shared with the statistics page. A scan that takes more than
/// [`REMOTE_WAIT`] answers with the cache's version and carries on.
#[allow(dead_code)] // P20 (mobile) calls it; nothing in the window does.
pub async fn usage(host: &Host) -> Reply {
    shared(host).fresh(REMOTE_WAIT).await
}

/// Worktrees change seldom: git is asked again after this
/// (`WORKTREES_FRESH_MS`).
pub const WORKTREES_FRESH_MS: i64 = 5 * 60_000;

/// The worktrees of each repository `remote_stats` asked about, with when git
/// answered. The page itself asks again at every visit, as the web page did.
pub struct WorktreeCache {
    seen: Mutex<std::collections::HashMap<String, (i64, Vec<String>)>>,
    clock: fn() -> i64,
}

impl WorktreeCache {
    pub fn new(clock: fn() -> i64) -> WorktreeCache {
        WorktreeCache {
            seen: Mutex::new(std::collections::HashMap::new()),
            clock,
        }
    }

    /// The worktree folders of the repository at `cwd`, from `ask` unless it
    /// answered less than five minutes ago. Not a repository, or no git: the
    /// workspace's own folders say enough, so a failure is an empty list.
    pub async fn of<F, Fut>(&self, cwd: &str, ask: F) -> Vec<String>
    where
        F: FnOnce(String) -> Fut,
        Fut: std::future::Future<Output = Result<Vec<String>, String>>,
    {
        let now = (self.clock)();
        let known = self
            .seen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(cwd)
            .filter(|(at, _)| now - at < WORKTREES_FRESH_MS)
            .map(|(_, paths)| paths.clone());
        if let Some(paths) = known {
            return paths;
        }
        let paths = ask(cwd.to_owned()).await.unwrap_or_default();
        self.seen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(cwd.to_owned(), ((self.clock)(), paths.clone()));
        paths
    }
}

/// What `remote_stats` reads for each workspace (P20): the folders of the
/// worktrees of the repository at `cwd`, which count as the workspace's own.
#[allow(dead_code)] // P20 (mobile) calls it; nothing in the window does.
pub async fn worktrees_of(cwd: &str) -> Vec<String> {
    static SEEN: OnceLock<WorktreeCache> = OnceLock::new();
    SEEN.get_or_init(|| WorktreeCache::new(now_ms))
        .of(cwd, |cwd| async move {
            hangar_core::git::git_worktree_list(cwd)
                .await
                .map(|list| list.into_iter().map(|tree| tree.path).collect())
        })
        .await
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

    use hangar_core::stats::ScanInfo;
    use tokio::sync::oneshot;

    use super::*;

    fn data(cached_only: bool, generated_at: i64) -> UsageStats {
        UsageStats {
            generated_at,
            scan: ScanInfo {
                cached_only,
                ..ScanInfo::default()
            },
            sources: Vec::new(),
            active_gap_cap_ms: 0,
            sessions: Vec::new(),
            models: Vec::new(),
            buckets: Vec::new(),
        }
    }

    fn clock(now: &Arc<AtomicI64>) -> Arc<dyn Fn() -> i64 + Send + Sync> {
        let now = now.clone();
        Arc::new(move || now.load(Ordering::SeqCst))
    }

    #[tokio::test]
    async fn reuses_a_fresh_scan_and_scans_again_once_it_is_not() {
        let now = Arc::new(AtomicI64::new(0));
        let reads = Arc::new(AtomicUsize::new(0));
        let cache = UsageCache::new(
            {
                let (now, reads) = (now.clone(), reads.clone());
                Arc::new(move |_| {
                    reads.fetch_add(1, Ordering::SeqCst);
                    let at = now.load(Ordering::SeqCst);
                    async move { Ok(data(false, at)) }.boxed()
                })
            },
            clock(&now),
        );
        let wait = Duration::from_secs(60);
        assert_eq!(cache.fresh(wait).await.unwrap().generated_at, 0);
        now.store(59_000, Ordering::SeqCst);
        assert_eq!(cache.fresh(wait).await.unwrap().generated_at, 0);
        now.store(61_000, Ordering::SeqCst);
        assert_eq!(cache.fresh(wait).await.unwrap().generated_at, 61_000);
        assert_eq!(reads.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn joins_a_scan_under_way_rather_than_starting_a_second() {
        let (finish, finished) = oneshot::channel::<()>();
        let finished = Arc::new(Mutex::new(Some(finished)));
        let reads = Arc::new(AtomicUsize::new(0));
        let cache = UsageCache::new(
            {
                let reads = reads.clone();
                Arc::new(move |_| {
                    reads.fetch_add(1, Ordering::SeqCst);
                    let gate = finished.lock().unwrap().take();
                    async move {
                        if let Some(gate) = gate {
                            let _ = gate.await;
                        }
                        Ok(data(false, 1))
                    }
                    .boxed()
                })
            },
            Arc::new(|| 0),
        );
        let first = tokio::spawn({
            let cache = cache.clone();
            async move { cache.scan().await }
        });
        let second = tokio::spawn({
            let cache = cache.clone();
            async move { cache.scan().await }
        });
        // Both are waiting on the one scan before it is allowed to finish.
        while !cache.scanning() {
            tokio::task::yield_now().await;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
        finish.send(()).unwrap();
        let (first, second) = (
            first.await.unwrap().unwrap(),
            second.await.unwrap().unwrap(),
        );
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert!(!cache.scanning());
    }

    #[tokio::test]
    async fn answers_from_the_cache_when_the_scan_takes_too_long_and_from_the_scan_once_done() {
        let (finish, finished) = oneshot::channel::<()>();
        let finished = Arc::new(Mutex::new(Some(finished)));
        let scans = Arc::new(AtomicUsize::new(0));
        let cache = UsageCache::new(
            {
                let scans = scans.clone();
                Arc::new(move |cached_only| {
                    if cached_only {
                        return async { Ok(data(true, 1)) }.boxed();
                    }
                    scans.fetch_add(1, Ordering::SeqCst);
                    let gate = finished.lock().unwrap().take();
                    async move {
                        if let Some(gate) = gate {
                            let _ = gate.await;
                        }
                        Ok(data(false, 2))
                    }
                    .boxed()
                })
            },
            Arc::new(|| 0),
        );
        let answer = cache.fresh(Duration::from_millis(30)).await.unwrap();
        assert!(answer.scan.cached_only);
        finish.send(()).unwrap();
        // The scan went on without anyone waiting for it.
        while cache.scanning() {
            tokio::task::yield_now().await;
        }
        let later = cache.fresh(Duration::from_millis(30)).await.unwrap();
        assert!(!later.scan.cached_only);
        assert_eq!(scans.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn passes_a_failed_scan_on_and_scans_again_on_the_next_call() {
        let calls = Arc::new(AtomicUsize::new(0));
        let cache = UsageCache::new(
            {
                let calls = calls.clone();
                Arc::new(move |_| {
                    let call = calls.fetch_add(1, Ordering::SeqCst);
                    async move {
                        if call == 0 {
                            Err("no transcripts".to_owned())
                        } else {
                            Ok(data(false, 3))
                        }
                    }
                    .boxed()
                })
            },
            Arc::new(|| 0),
        );
        let wait = Duration::from_secs(5);
        assert_eq!(cache.fresh(wait).await.unwrap_err(), "no transcripts");
        assert_eq!(cache.fresh(wait).await.unwrap().generated_at, 3);
    }

    #[tokio::test]
    async fn worktrees_are_asked_again_after_five_minutes_and_a_failure_is_an_empty_list() {
        static NOW: AtomicI64 = AtomicI64::new(0);
        let cache = WorktreeCache::new(|| NOW.load(Ordering::SeqCst));
        let asked = AtomicUsize::new(0);
        let ask = |answer: Result<Vec<String>, String>| {
            let asked = &asked;
            move |_: String| {
                asked.fetch_add(1, Ordering::SeqCst);
                async move { answer }
            }
        };
        let trees = vec!["/home/me/app".to_owned(), "/home/me/app-wt".to_owned()];
        assert_eq!(
            cache.of("/home/me/app", ask(Ok(trees.clone()))).await,
            trees
        );
        NOW.store(299_000, Ordering::SeqCst);
        assert_eq!(cache.of("/home/me/app", ask(Ok(Vec::new()))).await, trees);
        assert_eq!(asked.load(Ordering::SeqCst), 1);
        NOW.store(301_000, Ordering::SeqCst);
        assert!(
            cache
                .of("/home/me/app", ask(Err("no git".into())))
                .await
                .is_empty()
        );
        assert_eq!(asked.load(Ordering::SeqCst), 2);
        // Each folder has its own answer.
        assert_eq!(cache.of("/other", ask(Ok(trees.clone()))).await, trees);
    }

    #[tokio::test]
    async fn the_instant_answer_is_not_a_scan() {
        let cache = UsageCache::new(
            Arc::new(|cached_only| async move { Ok(data(cached_only, 7)) }.boxed()),
            Arc::new(|| 0),
        );
        assert!(cache.cached_only().await.unwrap().scan.cached_only);
        assert!(cache.last().is_none());
        assert!(cache.fresh_last().is_none());
        cache.scan().await.unwrap();
        assert_eq!(cache.last().unwrap().data.generated_at, 7);
    }
}
