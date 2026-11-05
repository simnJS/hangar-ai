//! The updater a window drives: check, download, install, relaunch, with its
//! progress kept as a [`State`] the window can draw a banner from.

use std::ffi::OsString;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use semver::Version;

use crate::error::{Error, Result, Stage};
use crate::install::{self, InstallOutcome, Signed, Staged};
use crate::manifest::{Manifest, parse_version};
use crate::platform::{Install, Target};
use crate::{ENDPOINT, PUBKEY, http, signature};

/// What the updater knows about a release, for the banner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub version: String,
    /// The release notes: the body of the GitHub release, as Markdown.
    pub notes: Option<String>,
    pub date: Option<DateTime<Utc>>,
}

/// Where the updater is, as a banner shows it. Every transition goes through
/// [`Updater::state`] and the observer set with [`Updater::on_change`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Nothing to show: not checked yet, up to date, or this install cannot
    /// update itself (a development build, an unpackaged binary).
    Idle,
    Checking,
    /// A newer release exists and nothing has been downloaded.
    Available(ReleaseInfo),
    /// `total` is `None` when the server did not announce a size. The state
    /// stays here while the finished download is being verified, with
    /// `bytes == total`.
    Downloading {
        release: ReleaseInfo,
        bytes: u64,
        total: Option<u64>,
    },
    /// Downloaded and verified. Nothing has been touched yet: installing is the
    /// person's choice, because it ends with the app restarting.
    Ready(ReleaseInfo),
    /// The new version is being put in place.
    Installing(ReleaseInfo),
    /// Put in place; see [`InstallOutcome`] for what remains.
    Installed(ReleaseInfo),
    /// A step failed. A failed `Check` is meant to be silent (no network is not
    /// news); the other two are shown, and calling [`Updater::download`] again
    /// is the retry.
    Error {
        stage: Stage,
        message: String,
    },
}

/// Everything the updater needs to know that is not a decision of the person.
#[derive(Clone, Debug)]
pub struct Config {
    /// Where `latest.json` is.
    pub endpoint: String,
    /// The minisign public key, base64-encoded, as `tauri.conf.json` held it.
    pub pubkey: String,
    /// The version that is running.
    pub current_version: Version,
    /// Overrides the platform the feed's keys are built for. `None` is this
    /// binary's own.
    pub target: Option<Target>,
    /// Overrides how this copy was installed. `None` finds out from the
    /// environment ([`Install::detect`]).
    pub install: Option<Install>,
    /// Sent as `User-Agent`; GitHub refuses requests without one.
    pub user_agent: String,
}

impl Config {
    /// The configuration of the released app: the feed it has always polled and
    /// the key its releases are signed with. `current_version` is the app's own
    /// (`env!("CARGO_PKG_VERSION")`).
    pub fn hangar(current_version: &str) -> Result<Self> {
        Ok(Self {
            endpoint: ENDPOINT.to_string(),
            pubkey: PUBKEY.to_string(),
            current_version: parse_version(current_version)?,
            target: None,
            install: None,
            user_agent: concat!("hangar-update/", env!("CARGO_PKG_VERSION")).to_string(),
        })
    }
}

/// What a check found and a download needs.
struct Pending {
    info: ReleaseInfo,
    url: String,
    signature: String,
    install: Install,
}

struct Inner {
    state: State,
    pending: Option<Pending>,
    /// The verified download, until it is installed. Dropping it deletes it.
    staged: Option<Staged>,
}

type Observer = Arc<dyn Fn(&State) + Send + Sync>;

struct Shared {
    config: Config,
    inner: Mutex<Inner>,
    cancel: AtomicBool,
    observer: Mutex<Option<Observer>>,
    install: OnceLock<Option<Install>>,
}

/// The updater. Cheap to clone: every clone is the same updater, so a window
/// can keep one for reading [`state`](Self::state) and hand another to the
/// task that does the work.
///
/// The methods that talk to the network or the disk ([`check`](Self::check),
/// [`download`](Self::download), [`install`](Self::install)) block until they
/// are done. Run them on a thread of their own; from an async task that is
/// `tokio::task::spawn_blocking`:
///
/// ```ignore
/// let updater = hangar_update::Updater::new(hangar_update::Config::hangar(env!("CARGO_PKG_VERSION"))?);
/// updater.on_change(move |state| { /* send it to the window */ });
///
/// let checking = updater.clone();
/// tokio::task::spawn_blocking(move || checking.check()).await??;
/// // …the person clicks "Update"…
/// let working = updater.clone();
/// tokio::task::spawn_blocking(move || {
///     working.download()?;
///     match working.install()? {
///         InstallOutcome::Replaced => working.relaunch(),
///         InstallOutcome::InstallerStarted => Ok(()),
///     }
///     // …then save state and exit.
/// }).await??;
/// ```
#[derive(Clone)]
pub struct Updater {
    shared: Arc<Shared>,
}

impl Updater {
    pub fn new(config: Config) -> Self {
        Self {
            shared: Arc::new(Shared {
                config,
                inner: Mutex::new(Inner {
                    state: State::Idle,
                    pending: None,
                    staged: None,
                }),
                cancel: AtomicBool::new(false),
                observer: Mutex::new(None),
                install: OnceLock::new(),
            }),
        }
    }

    /// Calls `observer` after every change of state, from whichever thread
    /// made it. While downloading it is called about ten times a second at
    /// most, and always for the last chunk. It must not block, and must not
    /// call back into the updater from the same thread.
    pub fn on_change(&self, observer: impl Fn(&State) + Send + Sync + 'static) {
        *lock(&self.shared.observer) = Some(Arc::new(observer));
    }

    /// The current state; cheap to call, from any thread.
    pub fn state(&self) -> State {
        lock(&self.shared.inner).state.clone()
    }

    /// How this copy is installed, or `None` if it cannot update itself.
    pub fn install_kind(&self) -> Option<Install> {
        self.shared
            .install
            .get_or_init(|| self.shared.config.install.clone().or_else(Install::detect))
            .clone()
    }

    /// Asks the feed whether there is a newer release for this platform.
    ///
    /// `Ok(None)` is "nothing to do": up to date, or an install that cannot
    /// update itself, which is not asked about over the network at all. Called
    /// again once a release is already downloaded it answers with that release
    /// instead of asking again.
    pub fn check(&self) -> Result<Option<ReleaseInfo>> {
        // The state is read and moved on under one lock, so that two calls at
        // once cannot both start.
        {
            let mut inner = lock(&self.shared.inner);
            match &inner.state {
                State::Checking => return Err(Error::State("a check is already running".into())),
                State::Downloading { .. } | State::Installing(_) => {
                    return Err(Error::State("an update is in progress".into()));
                }
                State::Ready(info) | State::Installed(info) => return Ok(Some(info.clone())),
                State::Idle | State::Available(_) | State::Error { .. } => {}
            }
            inner.state = State::Checking;
        }
        self.notify(&State::Checking);

        match self.check_inner() {
            Ok(Some(pending)) => {
                let info = pending.info.clone();
                lock(&self.shared.inner).pending = Some(pending);
                self.set(State::Available(info.clone()));
                Ok(Some(info))
            }
            Ok(None) => {
                lock(&self.shared.inner).pending = None;
                self.set(State::Idle);
                Ok(None)
            }
            Err(error) => {
                self.fail(Stage::Check, &error);
                Err(error)
            }
        }
    }

    fn check_inner(&self) -> Result<Option<Pending>> {
        let config = &self.shared.config;

        let Some(install) = self.install_kind() else {
            return Ok(None);
        };
        if matches!(install, Install::Nsis | Install::Msi) {
            // The installers earlier updates had to leave behind.
            install::sweep_stale(&std::env::temp_dir(), Duration::from_secs(60 * 60));
        }
        let Some(target) = config.target.or_else(Target::current) else {
            return Ok(None);
        };

        let Some(feed) = http::fetch_feed(&config.endpoint, &config.user_agent)? else {
            return Ok(None);
        };
        let manifest = Manifest::parse(&feed)?;
        if !manifest.is_newer_than(&config.current_version) {
            return Ok(None);
        }

        let keys = target.keys(Some(&install));
        let (_, build) = manifest.platform(&keys)?;
        Ok(Some(Pending {
            info: ReleaseInfo {
                version: manifest.version.to_string(),
                notes: manifest.notes.clone(),
                date: manifest.pub_date,
            },
            url: build.url.clone(),
            signature: build.signature.clone(),
            install,
        }))
    }

    /// Downloads the release [`check`](Self::check) found and verifies it
    /// against its signature. Nothing that fails verification is kept.
    ///
    /// Can be called again after a failed download or install, which is how a
    /// banner's "try again" works. [`cancel`](Self::cancel) stops it and leaves
    /// the state at `Available`.
    pub fn download(&self) -> Result<()> {
        let (pending_url, signature, install, info) = {
            let mut inner = lock(&self.shared.inner);
            match &inner.state {
                State::Ready(_) | State::Installed(_) => return Ok(()),
                State::Available(_) | State::Error { .. } => {}
                _ => return Err(Error::State("there is no update to download".into())),
            }
            let pending = inner
                .pending
                .as_ref()
                .ok_or_else(|| Error::State("there is no update to download".into()))?;
            let taken = (
                pending.url.clone(),
                pending.signature.clone(),
                pending.install.clone(),
                pending.info.clone(),
            );
            // Moved on under the same lock as the check above: a second call
            // now finds `Downloading` and is refused.
            inner.state = State::Downloading {
                release: taken.3.clone(),
                bytes: 0,
                total: None,
            };
            self.shared.cancel.store(false, Ordering::Relaxed);
            taken
        };
        self.notify(&State::Downloading {
            release: info.clone(),
            bytes: 0,
            total: None,
        });

        match self.download_inner(&pending_url, &signature, &install, &info) {
            Ok(staged) => {
                lock(&self.shared.inner).staged = Some(staged);
                self.set(State::Ready(info));
                Ok(())
            }
            Err(Error::Cancelled) => {
                self.set(State::Available(info));
                Err(Error::Cancelled)
            }
            Err(error) => {
                self.fail(Stage::Download, &error);
                Err(error)
            }
        }
    }

    fn download_inner(
        &self,
        url: &str,
        signature: &str,
        install: &Install,
        info: &ReleaseInfo,
    ) -> Result<Staged> {
        let config = &self.shared.config;
        let (mut file, staged) = install::create_staging(install)?;

        let mut last_notified = Instant::now();
        http::download(
            url,
            &config.user_agent,
            &mut file,
            &self.shared.cancel,
            |bytes, total| {
                let done = total == Some(bytes);
                let notify =
                    bytes == 0 || done || last_notified.elapsed() >= Duration::from_millis(100);
                if notify {
                    last_notified = Instant::now();
                }
                self.progress(info, bytes, total, notify);
            },
        )?;
        file.flush()
            .map_err(|e| Error::io("writing the download", e))?;
        drop(file);

        signature::verify_file(staged.path(), &config.pubkey, signature)?;
        Ok(staged)
    }

    /// Stops a running [`download`](Self::download) at its next chunk. It has no
    /// effect on the other steps: a check is short, and an install must not be
    /// left halfway.
    pub fn cancel(&self) {
        self.shared.cancel.store(true, Ordering::Relaxed);
    }

    /// Forgets the offer, as "Later" does: the state goes back to `Idle` until
    /// the next check. Does nothing while a download or install is running, or
    /// once the update has been downloaded.
    pub fn dismiss(&self) {
        let dismissible = matches!(
            lock(&self.shared.inner).state,
            State::Available(_) | State::Error { .. }
        );
        if dismissible {
            self.set(State::Idle);
        }
    }

    /// Puts the downloaded update in place of the running version.
    ///
    /// Blocks while the platform's installer works, which for a package asks
    /// the person for a password. On success the next step depends on the
    /// outcome; see [`InstallOutcome`].
    pub fn install(&self) -> Result<InstallOutcome> {
        let (staged, install, signature, info) = {
            let mut inner = lock(&self.shared.inner);
            let info = match &inner.state {
                State::Ready(info) => info.clone(),
                _ => return Err(Error::State("the update has not been downloaded".into())),
            };
            let (Some(staged), Some(pending)) = (inner.staged.take(), inner.pending.as_ref())
            else {
                return Err(Error::State("the update has not been downloaded".into()));
            };
            let taken = (
                staged,
                pending.install.clone(),
                pending.signature.clone(),
                info,
            );
            inner.state = State::Installing(taken.3.clone());
            taken
        };
        self.notify(&State::Installing(info.clone()));
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();
        let signed = Signed {
            pubkey: &self.shared.config.pubkey,
            signature: &signature,
        };
        match install::apply(&install, staged, signed, &args) {
            Ok(outcome) => {
                self.set(State::Installed(info));
                Ok(outcome)
            }
            Err(error) => {
                self.fail(Stage::Install, &error);
                Err(error)
            }
        }
    }

    /// Starts the new version, to take over once this process has exited.
    /// Call it after [`install`](Self::install) returned
    /// [`InstallOutcome::Replaced`], then save state and exit: the new copy
    /// waits for this process to be gone before it starts. Does nothing where
    /// the installer relaunches the app itself.
    pub fn relaunch(&self) -> Result<()> {
        let install = {
            let inner = lock(&self.shared.inner);
            if !matches!(inner.state, State::Installed(_)) {
                return Err(Error::State("the update has not been installed".into()));
            }
            inner.pending.as_ref().map(|p| p.install.clone())
        };
        let Some(install) = install else {
            return Err(Error::State("the update has not been installed".into()));
        };

        let exe = std::env::current_exe()
            .map(crate::platform::strip_deleted_marker)
            .map_err(|e| Error::io("finding the running program", e))?;
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();
        match install::relaunch_command(&install, &exe, &args, std::process::id()) {
            Some(mut command) => {
                command
                    .spawn()
                    .map_err(|e| Error::io("starting the new version", e))?;
                Ok(())
            }
            None => Ok(()),
        }
    }

    // ------------------------------------------------------------ plumbing

    fn set(&self, state: State) {
        lock(&self.shared.inner).state = state.clone();
        self.notify(&state);
    }

    fn fail(&self, stage: Stage, error: &Error) {
        self.set(State::Error {
            stage,
            message: error.to_string(),
        });
    }

    fn progress(&self, release: &ReleaseInfo, bytes: u64, total: Option<u64>, notify: bool) {
        let state = State::Downloading {
            release: release.clone(),
            bytes,
            total,
        };
        lock(&self.shared.inner).state = state.clone();
        if notify {
            self.notify(&state);
        }
    }

    fn notify(&self, state: &State) {
        // Cloned out so the observer runs with no lock held.
        let observer = lock(&self.shared.observer).clone();
        if let Some(observer) = observer {
            observer(state);
        }
    }
}

/// A poisoned lock means a panic in an observer or a thread of the app; the
/// state behind it is plain data and still true, so it is used as it is.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl std::fmt::Debug for Updater {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Updater")
            .field("state", &self.state())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{PUBKEY as TEST_PUBKEY, latest_json, payload};
    use crate::testserver::{Reply, Server};
    use std::fs;
    use std::path::PathBuf;

    const RELEASE_BASE: &str = "https://github.com/hangar-ai/hangar/releases/download/v1.13.0";

    struct Fixture {
        server: Server,
        dir: tempfile::TempDir,
        image: PathBuf,
        updater: Updater,
        states: Arc<Mutex<Vec<State>>>,
    }

    impl Fixture {
        /// A server publishing 1.13.0 with a signed AppImage, and an updater
        /// running 1.12.1 as an AppImage in a folder of its own.
        fn new() -> Self {
            let server = Server::start();
            let feed = latest_json().replace(RELEASE_BASE, &server.base());
            server.reply("/latest.json", Reply::ok(feed.into_bytes()));
            server.reply("/Hangar.AI_1.13.0_amd64.AppImage", Reply::ok(payload()));

            let dir = tempfile::tempdir().unwrap();
            let image = dir.path().join("Hangar.AI.AppImage");
            fs::write(&image, b"1.12.1 image").unwrap();

            let config = Config {
                endpoint: format!("{}/latest.json", server.base()),
                pubkey: TEST_PUBKEY.to_string(),
                current_version: Version::new(1, 12, 1),
                target: Some(Target::new("linux", "x86_64")),
                install: Some(Install::AppImage {
                    path: image.clone(),
                }),
                user_agent: "hangar-update-tests".into(),
            };
            let updater = Updater::new(config);
            let states = Arc::new(Mutex::new(Vec::new()));
            let seen = states.clone();
            updater.on_change(move |state| seen.lock().unwrap().push(state.clone()));
            Self {
                server,
                dir,
                image,
                updater,
                states,
            }
        }

        fn states(&self) -> Vec<State> {
            self.states.lock().unwrap().clone()
        }

        fn folder(&self) -> Vec<String> {
            let mut names: Vec<_> = fs::read_dir(self.dir.path())
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        }
    }

    fn released() -> ReleaseInfo {
        ReleaseInfo {
            version: "1.13.0".into(),
            notes: Some(
                "- **Windows** — the `.exe` under **Assets**. Installs for you alone,\n  with no administrator rights.\n- **macOS** — the `.dmg`, universal for Apple Silicon and Intel.\n- **Linux** — the `.AppImage` runs without installing; the `.deb` and\n  `.rpm` are there for a package manager."
                    .into(),
            ),
            date: Some("2026-11-03T09:41:07Z".parse().unwrap()),
        }
    }

    #[test]
    fn a_newer_release_is_offered_with_its_notes_and_date() {
        let f = Fixture::new();
        assert_eq!(f.updater.state(), State::Idle);

        let found = f.updater.check().unwrap();
        assert_eq!(found, Some(released()));
        assert_eq!(f.updater.state(), State::Available(released()));
        assert_eq!(f.states(), [State::Checking, State::Available(released())]);
        assert_eq!(f.server.requests(), ["/latest.json"]);
    }

    #[test]
    fn being_up_to_date_is_not_an_offer() {
        let f = Fixture::new();
        let mut config = f.updater.shared.config.clone();
        config.current_version = Version::new(1, 13, 0);
        let updater = Updater::new(config);
        assert_eq!(updater.check().unwrap(), None);
        assert_eq!(updater.state(), State::Idle);

        // Nor is an older release listed on the feed.
        let mut config = f.updater.shared.config.clone();
        config.current_version = Version::new(2, 0, 0);
        assert_eq!(Updater::new(config).check().unwrap(), None);
    }

    #[test]
    fn a_204_from_a_dynamic_server_means_nothing_newer() {
        let f = Fixture::new();
        f.server.reply("/latest.json", Reply::status(204));
        assert_eq!(f.updater.check().unwrap(), None);
        assert_eq!(f.updater.state(), State::Idle);
    }

    #[test]
    fn the_key_of_this_install_picks_the_build() {
        let f = Fixture::new();
        let mut config = f.updater.shared.config.clone();
        // A platform the release was not built for.
        config.target = Some(Target::new("linux", "riscv64"));
        let updater = Updater::new(config);
        match updater.check() {
            Err(Error::NoPlatformEntry(keys)) => {
                assert_eq!(keys, ["linux-riscv64-appimage", "linux-riscv64"])
            }
            other => panic!("expected no platform entry, got {other:?}"),
        }
        match updater.state() {
            State::Error {
                stage: Stage::Check,
                message,
            } => {
                assert!(message.contains("linux-riscv64"), "{message}")
            }
            other => panic!("expected a check error, got {other:?}"),
        }
    }

    #[test]
    fn a_failed_check_is_a_check_error_and_can_be_retried() {
        let f = Fixture::new();
        f.server.reply("/latest.json", Reply::status(503));
        assert!(matches!(f.updater.check(), Err(Error::Network(_))));
        assert!(matches!(
            f.updater.state(),
            State::Error {
                stage: Stage::Check,
                ..
            }
        ));

        f.server
            .reply("/latest.json", Reply::ok(b"{ not the feed".to_vec()));
        assert!(matches!(f.updater.check(), Err(Error::Manifest(_))));

        f.server.reply(
            "/latest.json",
            Reply::ok(
                latest_json()
                    .replace(RELEASE_BASE, &f.server.base())
                    .into_bytes(),
            ),
        );
        assert_eq!(f.updater.check().unwrap(), Some(released()));
    }

    #[test]
    fn an_install_that_cannot_update_itself_never_touches_the_network() {
        let f = Fixture::new();
        let mut config = f.updater.shared.config.clone();
        config.install = None;
        let updater = Updater::new(config);
        // Under `cargo test` the running binary is neither an AppImage nor a
        // package, so detection says "no".
        if updater.install_kind().is_none() {
            assert_eq!(updater.check().unwrap(), None);
            assert_eq!(updater.state(), State::Idle);
            assert!(f.server.requests().is_empty());
        }
    }

    #[test]
    fn a_feed_over_plain_http_is_refused() {
        let f = Fixture::new();
        let mut config = f.updater.shared.config.clone();
        config.endpoint = "http://updates.example.com/latest.json".into();
        match Updater::new(config).check() {
            Err(Error::Network(why)) => assert!(why.contains("https"), "{why}"),
            other => panic!("expected the feed to be refused, got {other:?}"),
        }
    }

    #[test]
    fn check_download_install_replaces_the_appimage() {
        let f = Fixture::new();
        f.updater.check().unwrap();
        f.updater.download().unwrap();
        assert_eq!(f.updater.state(), State::Ready(released()));
        // Downloaded, not installed: the running version is untouched.
        assert_eq!(fs::read(&f.image).unwrap(), b"1.12.1 image");

        let outcome = f.updater.install().unwrap();
        assert_eq!(outcome, InstallOutcome::Replaced);
        assert_eq!(f.updater.state(), State::Installed(released()));
        assert_eq!(fs::read(&f.image).unwrap(), payload());
        assert_eq!(
            f.folder(),
            ["Hangar.AI.AppImage"],
            "no scratch file is left"
        );
    }

    #[test]
    fn progress_is_reported_up_to_the_announced_size() {
        let f = Fixture::new();
        f.updater.check().unwrap();
        f.updater.download().unwrap();

        let total = payload().len() as u64;
        let progress: Vec<(u64, Option<u64>)> = f
            .states()
            .into_iter()
            .filter_map(|s| match s {
                State::Downloading { bytes, total, .. } => Some((bytes, total)),
                _ => None,
            })
            .collect();
        // Starts at nothing, then learns the size the server announced.
        assert_eq!(progress.first(), Some(&(0, None)));
        assert_eq!(progress.get(1), Some(&(0, Some(total))));
        assert_eq!(progress.last(), Some(&(total, Some(total))));
        assert!(
            progress.windows(2).all(|w| w[0].0 <= w[1].0),
            "never goes backwards"
        );
    }

    #[test]
    fn a_download_that_is_redirected_is_followed() {
        let f = Fixture::new();
        f.server.reply(
            "/Hangar.AI_1.13.0_amd64.AppImage",
            Reply::redirect("/cdn/objects/abc"),
        );
        f.server.reply("/cdn/objects/abc", Reply::ok(payload()));
        f.updater.check().unwrap();
        f.updater.download().unwrap();
        f.updater.install().unwrap();
        assert_eq!(fs::read(&f.image).unwrap(), payload());
    }

    #[test]
    fn a_tampered_download_is_refused_and_leaves_nothing() {
        let f = Fixture::new();
        let mut tampered = payload();
        tampered[100] ^= 0xff;
        f.server
            .reply("/Hangar.AI_1.13.0_amd64.AppImage", Reply::ok(tampered));

        f.updater.check().unwrap();
        match f.updater.download() {
            Err(Error::Signature(why)) => assert!(why.contains("does not match"), "{why}"),
            other => panic!("expected a signature error, got {other:?}"),
        }
        assert!(matches!(
            f.updater.state(),
            State::Error {
                stage: Stage::Download,
                ..
            }
        ));
        assert!(
            matches!(f.updater.install(), Err(Error::State(_))),
            "nothing to install"
        );
        assert_eq!(fs::read(&f.image).unwrap(), b"1.12.1 image");
        assert_eq!(
            f.folder(),
            ["Hangar.AI.AppImage"],
            "the bad download is deleted"
        );

        // The server is fixed: calling download again is the retry.
        f.server
            .reply("/Hangar.AI_1.13.0_amd64.AppImage", Reply::ok(payload()));
        f.updater.download().unwrap();
        f.updater.install().unwrap();
        assert_eq!(fs::read(&f.image).unwrap(), payload());
    }

    #[test]
    fn a_download_cut_short_is_an_error() {
        let f = Fixture::new();
        f.server.reply(
            "/Hangar.AI_1.13.0_amd64.AppImage",
            Reply::ok(payload()).truncated_to(1000),
        );
        f.updater.check().unwrap();
        assert!(f.updater.download().is_err());
        assert!(matches!(
            f.updater.state(),
            State::Error {
                stage: Stage::Download,
                ..
            }
        ));
        assert_eq!(f.folder(), ["Hangar.AI.AppImage"]);
    }

    #[test]
    fn cancelling_stops_the_download_and_goes_back_to_available() {
        let f = Fixture::new();
        f.server.reply(
            "/Hangar.AI_1.13.0_amd64.AppImage",
            Reply::ok(payload()).slowly(4096, Duration::from_millis(30)),
        );
        f.updater.check().unwrap();

        // Cancel from the observer, as a window's button would from its thread.
        let canceller = f.updater.clone();
        let seen = f.states.clone();
        f.updater.on_change(move |state| {
            seen.lock().unwrap().push(state.clone());
            if matches!(state, State::Downloading { bytes, .. } if *bytes > 0) {
                canceller.cancel();
            }
        });

        assert!(matches!(f.updater.download(), Err(Error::Cancelled)));
        assert_eq!(f.updater.state(), State::Available(released()));
        assert_eq!(fs::read(&f.image).unwrap(), b"1.12.1 image");
        assert_eq!(f.folder(), ["Hangar.AI.AppImage"]);

        // And it can be started again.
        f.updater.on_change(|_| {});
        f.server
            .reply("/Hangar.AI_1.13.0_amd64.AppImage", Reply::ok(payload()));
        f.updater.download().unwrap();
        assert_eq!(f.updater.state(), State::Ready(released()));
    }

    #[test]
    fn steps_out_of_order_are_errors_not_actions() {
        let f = Fixture::new();
        assert!(matches!(f.updater.download(), Err(Error::State(_))));
        assert!(matches!(f.updater.install(), Err(Error::State(_))));
        assert!(matches!(f.updater.relaunch(), Err(Error::State(_))));
        f.updater.check().unwrap();
        assert!(matches!(f.updater.install(), Err(Error::State(_))));
        assert_eq!(fs::read(&f.image).unwrap(), b"1.12.1 image");
    }

    #[test]
    fn dismissing_an_offer_hides_it() {
        let f = Fixture::new();
        f.updater.check().unwrap();
        f.updater.dismiss();
        assert_eq!(f.updater.state(), State::Idle);
        // Offered again by the next check.
        assert_eq!(f.updater.check().unwrap(), Some(released()));
    }

    #[test]
    fn the_released_app_is_configured_with_the_old_feed_and_key() {
        let config = Config::hangar("1.12.1").unwrap();
        assert_eq!(
            config.endpoint,
            "https://github.com/hangar-ai/hangar/releases/latest/download/latest.json"
        );
        assert_eq!(config.current_version, Version::new(1, 12, 1));
        assert!(Config::hangar("not a version").is_err());
    }

    #[test]
    fn the_updater_can_be_shared_between_threads() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Updater>();
    }

    /// The staged download beside the image, as a local process would find it.
    fn staged_file(f: &Fixture) -> PathBuf {
        fs::read_dir(f.dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.to_string_lossy().ends_with(".part"))
            .expect("the download is staged beside the image")
    }

    #[test]
    fn a_download_swapped_between_check_and_install_is_refused() {
        let f = Fixture::new();
        f.updater.check().unwrap();
        f.updater.download().unwrap();
        // Verified and waiting for Restart; another process rewrites it.
        let mut swapped = payload();
        swapped[200] ^= 0xff;
        fs::write(staged_file(&f), swapped).unwrap();

        match f.updater.install() {
            Err(Error::Signature(why)) => assert!(why.contains("does not match"), "{why}"),
            other => panic!("expected the swapped file to be refused, got {other:?}"),
        }
        assert!(matches!(
            f.updater.state(),
            State::Error {
                stage: Stage::Install,
                ..
            }
        ));
        assert_eq!(fs::read(&f.image).unwrap(), b"1.12.1 image");
        assert_eq!(
            f.folder(),
            ["Hangar.AI.AppImage"],
            "the bad file is deleted"
        );

        // Downloading again is the retry, and that one installs.
        f.updater.download().unwrap();
        f.updater.install().unwrap();
        assert_eq!(fs::read(&f.image).unwrap(), payload());
    }

    #[test]
    fn a_second_download_while_one_runs_is_refused() {
        let f = Fixture::new();
        f.server.reply(
            "/Hangar.AI_1.13.0_amd64.AppImage",
            Reply::ok(payload()).slowly(4096, Duration::from_millis(30)),
        );
        f.updater.check().unwrap();
        let first = f.updater.clone();
        let running = std::thread::spawn(move || first.download());
        let deadline = Instant::now() + Duration::from_secs(10);
        while !matches!(f.updater.state(), State::Downloading { .. }) {
            assert!(Instant::now() < deadline, "the download never started");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(matches!(f.updater.download(), Err(Error::State(_))));
        assert!(matches!(f.updater.check(), Err(Error::State(_))));
        f.updater.cancel();
        assert!(matches!(running.join().unwrap(), Err(Error::Cancelled)));
        assert_eq!(f.folder(), ["Hangar.AI.AppImage"], "one staging file, gone");
    }
}
