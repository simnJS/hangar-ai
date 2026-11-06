//! P17: 3D viewer (app side).
//!
//! Ports the 3D parts of `ModelPanel.tsx` (`ModelView`, `ModelToolbar`,
//! `AnimationBar`, `InfoStrip`, `LoadingCard`, `ProgressBar`),
//! `lib/modelCapture.ts` and the plumbing of `lib/model3d/useModelViewer.ts`.
//! The renderer, the loaders and the camera are the crate `hangar-model3d`;
//! read inventory-frontend.md §2.4/1.7, §2.4/3 and crates/hangar-app/PACKAGES.md.
//!
//! # What is here
//!
//! - **The slots** (`slot.rs`, `loads.rs`): one `ModelViewer` per model open in
//!   the preview store, of every workspace. The store says what is open and
//!   whether it is loading; this package reads each model off the window's
//!   thread (hangar-model3d's `load_task`, with the folders core allows as the
//!   roots its side files may come from), shows the progress, lets the person
//!   cancel, and reports every outcome back with `settle_model`, which is what
//!   an agent waiting on `model_show` hears. A newer revision of the file
//!   replaces the load under way and keeps the camera where the person left it.
//! - **The view** (`mod.rs`'s `view`, `toolbar.rs`, `stage.rs`,
//!   `animation_bar.rs`, `info_strip.rs`): the bar (recentre, wireframe, grid,
//!   axes, normals, capture), the viewport with the state of the load over it,
//!   the animation bar and the numbers. The viewport takes the mouse and the
//!   keys itself (hangar-model3d's widget).
//! - **Animation** (`animation_bar.rs`): a model with clips starts playing the
//!   first one, as the web viewer did. hangar-model3d keeps the clock (the
//!   widget moves it on with each frame it draws); the bar plays and pauses,
//!   picks the clip and scrubs the time through the viewer, and the view is
//!   built again a few times a second while a clip plays in front
//!   (`Msg::Tick`) so that the slider and the time follow.
//! - **When the viewport cannot draw** (`stage.rs`): hangar-model3d reports
//!   how its drawing is going (`ModelViewer::rendering`). A window drawn by
//!   iced's software renderer, a GPU that could not be set up, a model it
//!   refuses and a lost device each put a sentence over the blank viewport,
//!   with Restart where trying again can help.
//! - **Reduced motion** (`motion.rs`): the system's setting is read once and
//!   makes Recenter jump instead of glide.
//! - **The capture button** (`capture.rs`): twice the panel's size, the
//!   person's own framing, to the clipboard as a picture or to a PNG file
//!   through the system's save dialog and hangar-core's `model_save_png`.
//! - **Snapshots for agents** (`snapshots.rs`): this package registers the
//!   preview store's `Snapshotter`, so the control bus's `model_snapshot`
//!   (`PreviewAccess::snapshot`) is answered with a picture drawn off screen
//!   from the model in memory, from a canonical angle, whatever is on screen.
//!
//! # Gaps
//!
//! - On iced's software renderer (tiny-skia) the viewport stays empty, and
//!   says so; captures and snapshots still work, they bring their own device.
//! - F6 does not put the keyboard in the viewport: the widget takes it when it
//!   is clicked (the preview's `focus_entry` owns region `model`).
//! - The system's reduced-motion setting is read on Linux (GNOME's) and macOS;
//!   on Windows it is not, and the camera glides (`motion.rs`).
//! - A snapshot for an agent is of the first frame of the first clip, not of
//!   the moment on screen: the registry it reads holds models, not playheads.
//!   The capture button is of the moment on screen.

mod animation_bar;
mod capture;
mod dev;
mod format;
mod info_strip;
mod loads;
mod motion;
mod slot;
mod snapshots;
mod stage;
mod theme;
mod toolbar;

#[cfg(test)]
mod scratch;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::futures::stream::{self, StreamExt};
use iced::widget::{column, container};
use iced::{Element, Length, Subscription, Task};

use hangar_i18n::{Key, args};
use hangar_model3d::{DisplayOptions, LoadEvent};

use self::animation_bar::Clips;
use self::capture::{Destination, Done};
use self::slot::Slot;
use self::snapshots::{Registry, Snapshots};
use self::theme::viewer_theme_of;
use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::preview::access::Shared;
use crate::preview::parts::{self, Notice};
use crate::preview::store::Body;
use crate::services::{CoreEvent, Inbox, Services};
use crate::state::{Model, WsId};

/// How often the view looks again at what the store and the shell say, with
/// models open: a workspace put to sleep lets its models go, a theme change
/// reaches the snapshots.
const REVIEW: Duration = Duration::from_secs(2);

/// How often the view is built again while a clip plays in front, for the
/// slider and the time: ten times a second moves them smoothly enough, and
/// building the whole window's view at the pace of the frames would cost far
/// more than it shows. The model itself moves at the frames' pace on its own.
const ANIMATION_TICK: Duration = Duration::from_millis(100);

/// A display toggle of the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggle {
    Wireframe,
    Grid,
    Axes,
    Normals,
}

/// The package's UI state; lives in `App.pkg.model3d_view`.
#[derive(Debug)]
pub struct State {
    /// The preview store: what is open, and where the snapshots are asked.
    shared: Arc<Shared>,
    /// A viewer and its load for each model open, by entry id.
    slots: HashMap<String, Slot>,
    /// The toggles of the bar. One choice for every model, as the web app's
    /// bar had: a switch of tab keeps the wireframe on.
    display: DisplayOptions,
    /// The models in memory and the themes, for the snapshots' tokio tasks.
    registry: Arc<Registry>,
    /// The snapshotter has been given to the store.
    registered: bool,
    /// A clock of this view's own, to tell which model was last in front.
    clock: u64,
    /// Numbers the notices, so one is not cleared by the timer of another.
    notice_seq: u64,
    /// `HANGAR_DEV_MODEL3D_CAPTURE` has been served.
    dev_captured: bool,
    /// The system asks for less motion (`motion.rs`); Recenter then jumps.
    reduced_motion: bool,
    /// The system's setting has been asked for.
    motion_asked: bool,
    /// The models a snapshot asked for (`Msg::Ensure`), held open until the
    /// time given even when their workspace is asleep, so the load is not
    /// let go of before the snapshot has its model (`loads::settle`).
    ensured: HashMap<String, Instant>,
}

impl Default for State {
    fn default() -> Self {
        State::with_shared(Shared::global())
    }
}

impl State {
    /// The view over a given preview store (the process's, in the app).
    pub fn with_shared(shared: Arc<Shared>) -> State {
        State {
            shared,
            slots: HashMap::new(),
            display: DisplayOptions::default(),
            registry: Arc::new(Registry::default()),
            registered: false,
            clock: 0,
            notice_seq: 0,
            dev_captured: false,
            reduced_motion: false,
            motion_asked: false,
            ensured: HashMap::new(),
        }
    }
}

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.model3d_view`. Nothing reads this package: the store holds what
/// the others need (`settle_model`).
#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::Model3d(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The preview store changed, or this is the first look.
    Store,
    /// The folders a model's side files may come from are known: the read
    /// starts.
    Roots(String, u64, Vec<std::path::PathBuf>),
    /// What a read reported.
    Load(String, u64, LoadEvent),
    /// A read's stream is over: if it said nothing of how it ended, the load
    /// failed (`loads::ended`).
    LoadEnded(String, u64),
    /// The person stopped a load.
    CancelLoad(String),
    /// Load a model that could not be, or no longer can be, again.
    Retry(String),
    /// The viewport's widget: the person took hold of the camera, or how it
    /// draws changed (the view is built again and reads it).
    Viewer(String, hangar_model3d::Event),
    /// Play or pause the model's animation.
    PlayPause(String),
    /// Play the clip at this index of the file's, from its start.
    Clip(String, usize),
    /// Move the animation to this many seconds into its clip.
    Seek(String, f32),
    /// Start the viewport's drawing over (the GPU was lost or refused).
    Restart(String),
    /// A clip is playing in front: build the view again for the slider.
    Tick,
    /// The system's reduced-motion setting has been read.
    ReducedMotion(bool),
    Toggle(Toggle),
    Recenter(String),
    /// A step closer (positive) or further (negative) with the bar's buttons.
    Zoom(String, i32),
    CaptureMenu(String, bool),
    ClipsMenu(String, bool),
    Capture(String, Destination),
    Captured(String, Result<Done, String>),
    NoticeGone(String, u64),
    /// A snapshot needs this model in memory.
    Ensure(String),
    /// A debug build was asked to capture (`dev.rs`).
    DevCapture,
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    step(st, m, sv.inbox(), msg)
}

/// `update`, for whoever has an inbox but not the services.
fn step(st: &mut State, m: &mut Model, inbox: &Inbox, msg: Msg) -> Task<Message> {
    // Only there to make the shell build the view again: nothing in the
    // store has changed, so the look at it is left for the slower tick.
    if matches!(msg, Msg::Tick) {
        return Task::none();
    }
    // A read's progress changes only its own slot: looking at every model
    // again for each report would cost a full reconcile per step of a file.
    if matches!(msg, Msg::Load(_, _, LoadEvent::Progress(_))) {
        return handle(st, m, msg);
    }
    register(st, inbox);
    let asked = ask_motion(st);
    let first = handle(st, m, msg);
    let then = loads::reconcile(st, m);
    Task::batch([asked, first, then])
}

/// Asks the system, once, whether the person wants less motion. The answer
/// comes back as `Msg::ReducedMotion`.
fn ask_motion(st: &mut State) -> Task<Message> {
    if std::mem::replace(&mut st.motion_asked, true) {
        return Task::none();
    }
    Task::perform(
        async {
            tokio::task::spawn_blocking(motion::reduced_motion)
                .await
                .unwrap_or(false)
        },
        |reduced| Message::Model3d(Msg::ReducedMotion(reduced)),
    )
}

/// Gives the preview store the way to render a snapshot, once. It needs the
/// inbox, to ask this view to load a model that is not in memory.
fn register(st: &mut State, inbox: &Inbox) {
    if std::mem::replace(&mut st.registered, true) {
        return;
    }
    let snapshots = Snapshots::through(st.registry.clone(), st.shared.clone(), inbox.clone());
    st.shared.register_snapshotter(Some(Arc::new(snapshots)));
}

fn handle(st: &mut State, m: &mut Model, msg: Msg) -> Task<Message> {
    match msg {
        Msg::Store => Task::none(),
        Msg::Roots(id, revision, roots) => loads::read(st, &id, revision, roots),
        Msg::Load(id, revision, event) => {
            loads::progressed(st, &id, revision, event);
            Task::none()
        }
        Msg::LoadEnded(id, revision) => {
            loads::ended(st, &id, revision);
            Task::none()
        }
        Msg::CancelLoad(id) => {
            loads::cancel(st, &id);
            Task::none()
        }
        Msg::Retry(id) => {
            retry(st, &id);
            Task::none()
        }
        // Nothing to do for either: the first changes nothing the app holds,
        // and the second is only the news that the view reads the viewer's
        // `rendering()` again, which building it does.
        Msg::Viewer(..) => Task::none(),
        Msg::PlayPause(id) => {
            if let Some(slot) = st.slots.get(&id)
                && let Some(playback) = slot.viewer.playback()
            {
                slot.viewer.set_playing(!playback.playing);
            }
            Task::none()
        }
        Msg::Clip(id, clip) => {
            if let Some(slot) = st.slots.get_mut(&id) {
                slot.viewer.set_clip(clip);
                slot.clips_menu = false;
            }
            Task::none()
        }
        Msg::Seek(id, time) => {
            if let Some(slot) = st.slots.get(&id) {
                slot.viewer.seek(time);
            }
            Task::none()
        }
        Msg::Restart(id) => {
            if let Some(slot) = st.slots.get(&id) {
                slot.viewer.restart();
            }
            Task::none()
        }
        Msg::Tick => Task::none(),
        Msg::ReducedMotion(reduced) => {
            st.reduced_motion = reduced;
            for slot in st.slots.values() {
                slot.viewer.set_reduced_motion(reduced);
            }
            Task::none()
        }
        Msg::Toggle(which) => {
            toggle(st, which);
            Task::none()
        }
        Msg::Recenter(id) => {
            if let Some(slot) = st.slots.get(&id) {
                slot.viewer.recenter();
            }
            Task::none()
        }
        Msg::Zoom(id, steps) => {
            if let Some(slot) = st.slots.get(&id) {
                slot.viewer.zoom(steps);
            }
            Task::none()
        }
        Msg::CaptureMenu(id, open) => {
            if let Some(slot) = st.slots.get_mut(&id) {
                slot.menu = open;
            }
            Task::none()
        }
        Msg::ClipsMenu(id, open) => {
            if let Some(slot) = st.slots.get_mut(&id) {
                slot.clips_menu = open;
            }
            Task::none()
        }
        Msg::Capture(id, destination) => capture_view(st, m, &id, destination),
        Msg::Captured(id, result) => captured(st, m, &id, result),
        Msg::NoticeGone(id, seq) => {
            if let Some(slot) = st.slots.get_mut(&id)
                && slot.notice.as_ref().is_some_and(|(held, _)| *held == seq)
            {
                slot.notice = None;
            }
            Task::none()
        }
        Msg::Ensure(id) => ensure(st, &id),
        Msg::DevCapture => dev_capture(st, m),
    }
}

/// The retry of a failed load, or of a file that could not be read: the store
/// loads the same file again under a new revision (`reloadModel`).
fn retry(st: &mut State, id: &str) {
    let Some(modified_ms) = st
        .shared
        .read(|store| store.entry(id).map(|entry| entry.modified_ms))
    else {
        return;
    };
    if let Some(slot) = st.slots.get_mut(id) {
        slot.failed = None;
    }
    st.shared
        .edit(|store| store.reload_model(id, modified_ms, None));
}

fn toggle(st: &mut State, which: Toggle) {
    let mut display = st.display;
    match which {
        Toggle::Wireframe => display.wireframe = !display.wireframe,
        Toggle::Grid => display.grid = !display.grid,
        Toggle::Axes => display.axes = !display.axes,
        Toggle::Normals => display.normals = !display.normals,
    }
    st.display = display;
    for slot in st.slots.values_mut() {
        slot.viewer.set_display(display);
    }
}

/// A model asked for by a snapshot: loaded now if it is not in memory.
fn ensure(st: &mut State, id: &str) -> Task<Message> {
    let Some(want) = loads::wants(&st.shared)
        .into_iter()
        .find(|want| want.id == id)
    else {
        return Task::none();
    };
    let slot = st
        .slots
        .entry(want.id.clone())
        .or_insert_with(|| Slot::new(want.ws.clone(), st.display, st.reduced_motion));
    if slot.failed == Some(want.revision) {
        return Task::none();
    }
    // Held open until the load lands and the snapshot has had a moment to
    // take the model, even if its workspace sleeps.
    st.ensured
        .insert(want.id.clone(), Instant::now() + loads::ENSURE_HOLD);
    if slot.loading(want.revision) {
        return Task::none();
    }
    if slot.loaded == Some(want.revision)
        && let Some(model) = slot.viewer.model()
    {
        // In memory, but the registry let it go (or never had it).
        st.registry
            .put(&want.id, want.revision, &want.ws, model.clone());
        loads::release_hold(st, &want.id);
        return Task::none();
    }
    loads::begin(st, &want)
}

/// The capture button: the person's own view, twice as large.
fn capture_view(st: &mut State, m: &Model, id: &str, destination: Destination) -> Task<Message> {
    let Some(slot) = st.slots.get_mut(id) else {
        return Task::none();
    };
    slot.menu = false;
    let Some(job) = slot.viewer.capture(2.0, viewer_theme_of(m, &slot.ws)) else {
        return Task::none();
    };
    let title = st
        .shared
        .read(|store| store.entry(id).map(|entry| entry.title.clone()))
        .unwrap_or_default();
    let suggested = capture::suggested_name(&title, chrono::Local::now());
    let id = id.to_string();
    Task::perform(
        capture::deliver(job, destination, suggested),
        move |result| Message::Model3d(Msg::Captured(id.clone(), result)),
    )
}

/// What a capture ended with, said on the bar for a moment.
fn captured(st: &mut State, m: &Model, id: &str, result: Result<Done, String>) -> Task<Message> {
    let locale = m.locale();
    let notice = match result {
        Ok(Done::Copied) => Notice::info(hangar_i18n::t(locale, Key::ModelCopied)),
        Ok(Done::Saved(path)) => {
            if dev::capture_folder().is_some() {
                eprintln!("hangar: 3D capture written to {}", path.display());
            }
            Notice::info(hangar_i18n::t(locale, Key::ModelSaved))
        }
        // The person closed the dialog: nothing to say.
        Ok(Done::Cancelled) => return Task::none(),
        Err(error) => Notice::error(
            hangar_i18n::t_with(locale, Key::ModelCaptureFailed, &args!("error" => error))
                .into_owned(),
        ),
    };
    notify(st, id, notice)
}

/// A word on the bar, gone after a moment.
fn notify(st: &mut State, id: &str, notice: Notice) -> Task<Message> {
    let Some(slot) = st.slots.get_mut(id) else {
        return Task::none();
    };
    st.notice_seq += 1;
    let seq = st.notice_seq;
    let lasts = notice.lasts();
    slot.notice = Some((seq, notice));
    let id = id.to_string();
    // The timer is made when the task is polled, on the runtime, not here.
    Task::perform(async move { tokio::time::sleep(lasts).await }, move |()| {
        Message::Model3d(Msg::NoticeGone(id.clone(), seq))
    })
}

/// A debug build asked for a capture: the first model whose viewport has been
/// drawn is captured, once.
fn dev_capture(st: &mut State, m: &Model) -> Task<Message> {
    let Some(folder) = dev::capture_folder() else {
        return Task::none();
    };
    if st.dev_captured {
        return Task::none();
    }
    let ready = st
        .slots
        .iter()
        .find(|(_, slot)| {
            slot.viewer
                .capture(2.0, viewer_theme_of(m, &slot.ws))
                .is_some()
        })
        .map(|(id, _)| id.clone());
    let Some(id) = ready else {
        return Task::none();
    };
    st.dev_captured = true;
    let title = st
        .shared
        .read(|store| store.entry(&id).map(|entry| entry.title.clone()))
        .unwrap_or_default();
    let name = capture::suggested_name(&title, chrono::Local::now());
    capture_view(st, m, &id, Destination::File(folder.join(name)))
}

/// Follows the store's changes, once at start for the first look.
#[derive(Clone)]
struct Follow(Arc<Shared>);

impl std::hash::Hash for Follow {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (Arc::as_ptr(&self.0) as usize).hash(state);
    }
}

fn changes(shared: &Arc<Shared>) -> Subscription<Message> {
    Subscription::run_with(Follow(shared.clone()), |follow| {
        let receiver = follow.0.follow();
        let first = stream::once(std::future::ready(Message::Model3d(Msg::Store)));
        let rest = stream::unfold(receiver, |mut receiver| async move {
            receiver.changed().await.ok()?;
            Some((Message::Model3d(Msg::Store), receiver))
        });
        first.chain(rest)
    })
}

pub fn subscription(st: &State, _m: &Model) -> Subscription<Message> {
    let mut subscriptions = vec![changes(&st.shared)];
    if !st.slots.is_empty() {
        subscriptions.push(iced::time::every(REVIEW).map(|_| Message::Model3d(Msg::Store)));
    }
    if dev::capture_folder().is_some() && !st.dev_captured {
        subscriptions.push(iced::time::every(dev::POLL).map(|_| Message::Model3d(Msg::DevCapture)));
    }
    if animating_in_front(st) {
        subscriptions.push(iced::time::every(ANIMATION_TICK).map(|_| Message::Model3d(Msg::Tick)));
    }
    Subscription::batch(subscriptions)
}

/// A clip is playing in a model whose pane is on screen: the slider of its
/// bar has to move. A model nobody can see does not move (its widget is not
/// drawn), so it asks for nothing.
fn animating_in_front(st: &State) -> bool {
    if st.slots.is_empty() {
        return false;
    }
    st.shared.read(|store| {
        st.slots.iter().any(|(id, slot)| {
            slot.viewer
                .playback()
                .is_some_and(|playback| playback.playing)
                && !slot.viewer.rendering().is_problem()
                && store.entry(id).is_some()
                && store.view().shows(id)
        })
    })
}

/// Every intent, after the shell's own handling of it: `Some` when this
/// package acted on it. None is for this package: a workspace's previews are
/// closed by the preview's own handling, and the store tells this view.
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

/// A 3D model in the body of its view pane (called by `preview::view_entry`).
pub fn view<'a>(st: &'a State, ctx: Ctx<'a>, ws: &'a WsId, entry: &'a str) -> Element<'a, Message> {
    let Some(found) = ctx
        .model
        .live
        .preview
        .scope(ws)
        .and_then(|scope| scope.entry(entry))
    else {
        return parts::loading(ctx, entry, None);
    };
    let slot = st.slots.get(entry);
    let model = slot.and_then(|slot| slot.viewer.model());
    let ready = model.is_some();
    // Why the file can no longer be read, if the preview's watch found it
    // gone, and the step of its spinner for the bars that travel.
    let missing = ctx.pkg.preview.missing.get(entry);
    let spin = ctx.pkg.preview.spin;

    let bar = toolbar::view(
        ctx,
        toolbar::Bar {
            id: entry,
            ready,
            display: st.display,
            normals_allowed: slot.is_some_and(|slot| slot.viewer.can_draw_normals()),
            menu: slot.is_some_and(|slot| slot.menu),
            notice: slot.and_then(|slot| slot.notice.as_ref().map(|(_, notice)| notice)),
        },
    );
    let stage = stage::view(ctx, found, slot, missing, spin);

    let mut page = column![
        bar,
        container(stage).width(Length::Fill).height(Length::Fill)
    ];
    // A model with clips gets the bar (`animation && hasContent`).
    if let (Some(model), Some(playback)) = (model, slot.and_then(|slot| slot.viewer.playback())) {
        page = page.push(animation_bar::view(
            ctx,
            Clips {
                id: entry,
                names: hangar_model3d::stats::clip_names(&model.clips),
                playback,
                menu: slot.is_some_and(|slot| slot.clips_menu),
            },
        ));
    }
    let format_name = match &found.body {
        Body::Model { format, .. } => format.as_str(),
        _ => "",
    };
    let info = ready.then(|| found.model_info()).flatten();
    page = page.push(info_strip::view(
        ctx,
        info_strip::facts(ctx, format_name, info),
    ));
    page.into()
}
