//! P6: Update banner (`UpdateBanner.tsx`; the updater is `hangar-update`).
//!
//! Checked once, when the main window opens, and silent when that fails: no
//! network, or a build that is not an installed package and so has no release
//! to follow, must never surface an error. A newer release is offered in a
//! capsule floating over the top of the window, which never reflows the panes
//! when it appears:
//!
//! - **available**: its version, *Later* (hides it), *Update*;
//! - **downloading**: a progress bar;
//! - **ready**: the update is downloaded and verified, *Restart* installs it
//!   and starts the new version;
//! - **failed**: what step failed (the error is its tooltip), *Retry*.
//!
//! The old app downloaded and installed in one step. `hangar-update` splits
//! them so that putting the new version in place, which ends with the app
//! restarting, is the person's choice and not a side effect of a download.

use iced::widget::{Space, button, container, progress_bar, row, text, tooltip};
use iced::{Alignment, Element, Length, Subscription, Task, window};

use hangar_i18n::{Key, Translator, args};
use hangar_theme::tokens::{font_size, radius, space};
use hangar_theme::{Theme, style};
use hangar_ui::{Icon, icon};
use hangar_update::{Config, InstallOutcome, ReleaseInfo, Stage, Updater};

use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::navigator::skin;
use crate::services::{CoreEvent, Services};
use crate::shell::{Notice, Region, WindowKind};
use crate::state::Model;

/// Where the banner is (`Stage` of the TS).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Phase {
    /// Nothing to show.
    #[default]
    Idle,
    Available,
    Downloading,
    Ready,
    Failed,
}

/// What the updater's state means for the banner.
///
/// A failed *check* is silent, as it was: only a download or an install that
/// fails is shown. `Installing` and `Installed` are the moment between the
/// *Restart* click and the app going away, and read as ready.
pub fn stage_of(state: &hangar_update::State) -> Phase {
    use hangar_update::State;
    match state {
        State::Idle | State::Checking => Phase::Idle,
        State::Available(_) => Phase::Available,
        State::Downloading { .. } => Phase::Downloading,
        State::Ready(_) | State::Installing(_) | State::Installed(_) => Phase::Ready,
        State::Error {
            stage: Stage::Check,
            ..
        } => Phase::Idle,
        State::Error { .. } => Phase::Failed,
    }
}

/// The download's progress in whole percent; 0 while the size is unknown.
pub fn progress_of(bytes: u64, total: Option<u64>) -> u8 {
    match total {
        Some(total) if total > 0 => (bytes.min(total) as f64 / total as f64 * 100.0).round() as u8,
        _ => 0,
    }
}

/// The updater, which has no `Debug` of its own.
#[derive(Clone)]
struct Handle(Updater);

impl std::fmt::Debug for Handle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Updater")
    }
}

/// The package's UI state; lives in `App.pkg.update_banner`.
#[derive(Debug, Default)]
pub struct State {
    updater: Option<Handle>,
    stage: Phase,
    /// The release on offer, kept through a failure so a retry can name it.
    release: Option<ReleaseInfo>,
    progress: u8,
    /// What failed, as the tooltip of the failure.
    error: Option<String>,
    /// A restart is under way: the button waits.
    restarting: bool,
}

impl State {
    pub fn stage(&self) -> Phase {
        self.stage
    }

    /// Takes the updater's state in. `Idle` after a *Later* is the updater
    /// dismissing the offer, which hides the banner as the TS's did.
    fn take(&mut self, state: &hangar_update::State) {
        use hangar_update::State as U;
        match state {
            U::Available(release)
            | U::Ready(release)
            | U::Installing(release)
            | U::Installed(release) => {
                self.release = Some(release.clone());
            }
            U::Downloading {
                release,
                bytes,
                total,
            } => {
                self.release = Some(release.clone());
                self.progress = progress_of(*bytes, *total);
            }
            U::Error { stage, message } if *stage != Stage::Check => {
                self.error = Some(message.clone());
            }
            _ => {}
        }
        self.stage = stage_of(state);
        if matches!(state, U::Ready(_)) {
            self.progress = 100;
        }
    }
}

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.update_banner`.
#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::UpdateBanner(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The window is up: ask the feed, once.
    Check,
    /// The updater's state changed (from its worker thread, by the inbox).
    Changed(hangar_update::State),
    Later,
    Install,
    /// *Restart*: put the new version in place and start it.
    Restart,
    /// *Retry* after a failure: back to the offer.
    Retry,
}

fn updater_of(st: &mut State, sv: &Services) -> Option<Updater> {
    if let Some(Handle(updater)) = &st.updater {
        return Some(updater.clone());
    }
    let config = Config::hangar(env!("CARGO_PKG_VERSION")).ok()?;
    let updater = Updater::new(config);
    let inbox = sv.inbox().clone();
    updater.on_change(move |state| {
        inbox.send(Message::UpdateBanner(Msg::Changed(state.clone())));
    });
    st.updater = Some(Handle(updater.clone()));
    Some(updater)
}

/// `f` on a thread that may block: the updater talks to the network and the
/// disk until it is done.
fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
    done: impl Fn(T) -> Message + Send + 'static,
) -> Task<Message> {
    Task::perform(
        async move { tokio::task::spawn_blocking(f).await },
        move |result| match result {
            Ok(value) => done(value),
            Err(_) => Message::Noop,
        },
    )
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    match msg {
        Msg::Check => {
            if let Some(stage) = dev_stage() {
                st.apply_dev(stage);
                return Task::none();
            }
            let Some(updater) = updater_of(st, sv) else {
                return Task::none();
            };
            // The outcome is the state the observer reports; a failed check is
            // silent, so the result itself is not read.
            blocking(move || updater.check().is_ok(), |_| Message::Noop)
        }
        Msg::Changed(state) => {
            st.take(&state);
            Task::none()
        }
        Msg::Later => {
            if let Some(Handle(updater)) = &st.updater {
                updater.dismiss();
            }
            st.stage = Phase::Idle;
            Task::none()
        }
        Msg::Retry => {
            st.stage = Phase::Available;
            Task::none()
        }
        Msg::Install => {
            st.stage = Phase::Downloading;
            st.progress = 0;
            st.error = None;
            let Some(Handle(updater)) = st.updater.clone() else {
                return Task::none();
            };
            // Progress and failure come back through the observer; a cancelled
            // download leaves the offer standing.
            blocking(move || updater.download().is_ok(), |_| Message::Noop)
        }
        Msg::Restart => {
            if st.restarting {
                return Task::none();
            }
            let Some(Handle(updater)) = st.updater.clone() else {
                return Task::none();
            };
            st.restarting = true;
            let main = m
                .live
                .windows
                .iter()
                .find(|(_, kind)| **kind == WindowKind::Main)
                .map(|(id, _)| *id);
            blocking(
                move || restart(&updater),
                move |result| match result {
                    // The shell's way out of the main window: the last change
                    // saved, the core shut down, then the process ends. The
                    // new version is already starting; it waits for this
                    // process to be gone (hangar-update's relaunch helper).
                    Ok(()) => match main {
                        Some(id) => Message::Window(id, window::Event::CloseRequested),
                        None => Message::Noop,
                    },
                    Err(error) => Message::Intent(Intent::Notice(Notice::Problem(error))),
                },
            )
        }
    }
}

/// Puts the downloaded version in place and starts it. An installer that waits
/// for this process to end (Windows) starts the new version itself.
fn restart(updater: &Updater) -> Result<(), String> {
    match updater.install().map_err(|error| error.to_string())? {
        InstallOutcome::Replaced => updater.relaunch().map_err(|error| error.to_string()),
        InstallOutcome::InstallerStarted => Ok(()),
    }
}

/// The check, once, when the window has its state (the TS did it on mount).
/// The stream sends `Check` and then waits for good, so the subscription keeps
/// its identity and runs a single time.
pub fn subscription(_st: &State, m: &Model) -> Subscription<Message> {
    if !m.live.hydrated {
        return Subscription::none();
    }
    Subscription::run(check_once)
}

fn check_once() -> impl iced::futures::Stream<Item = Message> {
    use iced::futures::StreamExt;
    iced::futures::stream::once(async { Message::UpdateBanner(Msg::Check) })
        .chain(iced::futures::stream::pending())
}

/// What takes the focus when F6 reaches a region this package owns. It owns
/// none, so the shell never calls it.
pub fn focus_entry(_st: &State, _m: &Model, _region: Region) -> Option<Task<Message>> {
    None
}

/// Every intent, after the shell's own handling of it: `Some` when this
/// package acted on it.
pub fn on_intent(
    _st: &mut State,
    _m: &mut Model,
    _sv: &Services,
    _intent: &Intent,
) -> Option<Task<Message>> {
    None
}

/// Every core event, after the shell's own handling of it.
pub fn on_event(
    _st: &mut State,
    _m: &mut Model,
    _sv: &Services,
    _event: &CoreEvent,
) -> Option<Task<Message>> {
    None
}

// ---- Development aid -----------------------------------------------------------------------------

/// `HANGAR_DEV_UPDATE_BANNER=available|downloading|ready|failed` shows that
/// stage of the banner without a release to follow, to see it in a theme and a
/// language. Debug builds only: a release build never reads it.
fn dev_stage() -> Option<Phase> {
    if !cfg!(debug_assertions) {
        return None;
    }
    match std::env::var("HANGAR_DEV_UPDATE_BANNER").ok()?.as_str() {
        "available" => Some(Phase::Available),
        "downloading" => Some(Phase::Downloading),
        "ready" => Some(Phase::Ready),
        "failed" => Some(Phase::Failed),
        _ => None,
    }
}

impl State {
    fn apply_dev(&mut self, stage: Phase) {
        self.stage = stage;
        self.release = Some(ReleaseInfo {
            version: "1.13.0".into(),
            notes: None,
            date: None,
        });
        self.progress = if stage == Phase::Downloading { 42 } else { 0 };
        self.error = (stage == Phase::Failed).then(|| "the signature does not match".to_string());
    }
}

// ---- The view --------------------------------------------------------------------------------------

/// The banner at the top of the main window; nothing while there is no update.
pub fn view<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    let Some(release) = st.release.as_ref().filter(|_| st.stage != Phase::Idle) else {
        return Space::new().into();
    };
    let t = ctx.theme;
    let tr = Translator::new(ctx.model.locale());
    let (glyph, glyph_color) = match st.stage {
        Phase::Failed => (Icon::Alert, t.colors.danger),
        Phase::Ready => (Icon::Check, t.colors.success),
        _ => (Icon::Download, t.colors.accent_hi),
    };
    let message = |label: String, error: bool| -> Element<'a, Message> {
        let words = text(label).size(font_size::UI_LG).color(if error {
            t.colors.danger
        } else {
            t.colors.fg
        });
        match (&st.error, error) {
            (Some(why), true) => tooltip(
                words,
                container(text(why.clone()).size(font_size::CAPTION_LG))
                    .padding([4, 8])
                    .style(style::container::tooltip(t)),
                tooltip::Position::Bottom,
            )
            .into(),
            _ => words.into(),
        }
    };

    let mut content = row![icon(glyph, 14.0).color(glyph_color)]
        .spacing(space::MD)
        .align_y(Alignment::Center);
    match st.stage {
        Phase::Available => {
            content = content
                .push(message(
                    tr.t_with(
                        Key::UpdateAvailable,
                        &args!("version" => release.version.as_str()),
                    )
                    .into_owned(),
                    false,
                ))
                .push(pill_button(
                    t,
                    tr.t(Key::UpdateLater).into_owned(),
                    false,
                    Some(Message::UpdateBanner(Msg::Later)),
                ))
                .push(pill_button(
                    t,
                    tr.t(Key::UpdateInstall).into_owned(),
                    true,
                    Some(Message::UpdateBanner(Msg::Install)),
                ));
        }
        Phase::Downloading => {
            let progress = if st.progress > 0 {
                format!("{}%", st.progress)
            } else {
                String::new()
            };
            content = content
                .push(message(
                    tr.t_with(Key::UpdateDownloading, &args!("progress" => progress))
                        .trim_end()
                        .to_string(),
                    false,
                ))
                .push(
                    progress_bar(0.0..=100.0, f32::from(st.progress))
                        .length(96)
                        .girth(4)
                        .style(style::progress_bar::accent(t)),
                );
        }
        Phase::Ready => {
            content = content
                .push(message(tr.t(Key::UpdateReady).into_owned(), false))
                .push(pill_button(
                    t,
                    tr.t(Key::UpdateRestart).into_owned(),
                    true,
                    (!st.restarting).then_some(Message::UpdateBanner(Msg::Restart)),
                ));
        }
        Phase::Failed => {
            content = content
                .push(message(tr.t(Key::UpdateFailed).into_owned(), true))
                .push(pill_button(
                    t,
                    tr.t(Key::UpdateRetry).into_owned(),
                    false,
                    Some(Message::UpdateBanner(Msg::Retry)),
                ));
        }
        Phase::Idle => {}
    }

    // Round at the ends, with the buttons round as well: a square button in a
    // round capsule looks dropped in.
    container(
        container(content)
            .height(40)
            .padding(iced::Padding {
                top: 0.0,
                right: if st.stage == Phase::Downloading {
                    16.0
                } else {
                    6.0
                },
                bottom: 0.0,
                left: 14.0,
            })
            .align_y(Alignment::Center)
            .style(skin::capsule(t)),
    )
    .width(Length::Shrink)
    .padding(iced::Padding {
        top: space::XL,
        ..iced::Padding::ZERO
    })
    .into()
}

/// A button as round as the capsule it sits in, in the theme's own colours for
/// a primary or a ghost button.
fn pill_button<'a>(
    t: &'a Theme,
    label: String,
    primary: bool,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    button(text(label).size(font_size::UI))
        .padding([0.0, 12.0])
        .height(28)
        .style(move |theme: &iced::Theme, status| {
            let mut look = if primary {
                style::button::primary(t)(theme, status)
            } else {
                style::button::ghost(t)(theme, status)
            };
            look.border.radius = radius::FULL.into();
            look
        })
        .on_press_maybe(on_press)
        .into()
}

#[cfg(test)]
mod tests;
