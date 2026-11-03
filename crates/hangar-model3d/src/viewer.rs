//! The widget: a [`ModelViewer`] holds what a preview pane shows (a model, a
//! camera, the display toggles, how its load is going) and draws it with
//! iced's shader widget.
//!
//! The pointer and the keyboard are handled inside the widget, on the camera
//! the viewer shares with it, so a drag costs no message to the app and no
//! rebuild of its view; the widget asks for frames itself while the camera is
//! still easing to a stop.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use iced::time::Instant;
use iced::widget::shader::{self, Action, Viewport};
use iced::{
    Element, Event as IcedEvent, Length, Point, Rectangle, Size, Task, keyboard, mouse, wgpu,
    window,
};

use crate::animation::Playhead;
use crate::camera::{Camera, snapshot_matrices};
use crate::formats::LoadError;
use crate::framing::{SnapshotAngle, view_direction};
use crate::health::{Health, Prepared};
use crate::load::{Cancel, LoadEvent, LoadOptions, LoadProgress, Phase};
use crate::model::Model;
use crate::render::offscreen::{self, CaptureError};
use crate::render::{
    CameraMatrices, DisplayOptions, Gpu, NORMALS_LIMIT, Scene, WINDOW_UPLOAD_BUDGET, guarded,
};
use crate::theme::ViewerTheme;

pub use crate::health::Rendering;

/// Where a viewer's model is.
#[derive(Clone, Debug, PartialEq)]
pub enum LoadState {
    /// Nothing was asked for.
    Empty,
    Loading(LoadProgress),
    /// The model on show is loaded.
    Ready,
    /// The load failed, with a sentence for whoever asked.
    Failed(String),
}

/// What the widget tells the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// The user took hold of the camera: it is no longer the framed view.
    Interacted,
    /// How the viewport is drawing changed ([`ModelViewer::rendering`]): the
    /// GPU was found unusable, or lost, or the window turned out to be drawn
    /// in software. The app looks again to put a sentence over the viewport.
    RenderingChanged,
}

/// Where a model's animation is, as the app's bar shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Playback {
    /// Index into the model's clips.
    pub clip: usize,
    /// Seconds into the clip.
    pub time: f32,
    /// The clip's length in seconds.
    pub duration: f32,
    pub playing: bool,
}

/// What a viewer keeps of the animation.
#[derive(Clone, Copy, Debug)]
struct Playing {
    clip: usize,
    time: f32,
    playing: bool,
}

/// Where a model's animation starts: the first clip, playing, from its first
/// frame; or, for a reload of the same file, the clip and the moment the
/// previous version was at (`previous`), when the new one still has that clip.
fn start_playing(model: &Model, previous: Option<Playing>) -> Option<Playing> {
    if model.clips.is_empty() {
        return None;
    }
    Some(match previous {
        Some(before) if before.clip < model.clips.len() => Playing {
            time: before.time.min(model.clips[before.clip].duration),
            ..before
        },
        // A model that moves is shown moving.
        _ => Playing {
            clip: 0,
            time: 0.0,
            playing: true,
        },
    })
}

/// A scroll-wheel notch in the pixels a browser reports one as, which is what
/// the zoom speed of `OrbitControls` is measured in.
const WHEEL_NOTCH: f64 = 100.0;
/// One step of the zoom buttons and keys: a fifth closer.
const ZOOM_STEP: f64 = 0.8;
/// Arrow keys pan by this many pixels, or turn by this many pixels' worth of
/// rotation with Shift or Ctrl held: `OrbitControls`' key speeds.
const KEY_PAN_PIXELS: f64 = 16.0;
const KEY_ROTATE_PIXELS: f64 = 24.0;
/// A frame longer than this is a pause (a hidden window), not time that
/// passed for the camera.
const LONGEST_FRAME: f64 = 0.1;
/// What a view holds when the viewport has not been laid out yet.
const DEFAULT_ASPECT: f64 = 4.0 / 3.0;

static NEXT_VIEWER: AtomicU64 = AtomicU64::new(1);

/// The camera, as the widget and the viewer share it.
struct ViewState {
    camera: Option<Camera>,
    /// The viewport's size in logical pixels, as last drawn.
    size: Size,
    reduced_motion: bool,
    /// Where the model's animation is; `None` for a model with no clip.
    playback: Option<Playing>,
}

impl ViewState {
    fn aspect(&self) -> f64 {
        if self.size.width > 0.0 && self.size.height > 0.0 {
            f64::from(self.size.width) / f64::from(self.size.height)
        } else {
            DEFAULT_ASPECT
        }
    }

    /// Moves a playing animation on by `seconds`, round to the start when it
    /// reaches the end. Returns whether there is more to come: a frame is
    /// wanted after this one.
    fn advance(&mut self, model: Option<&Model>, seconds: f64) -> bool {
        let (Some(playing), Some(model)) = (self.playback.as_mut(), model) else {
            return false;
        };
        let duration = model
            .clips
            .get(playing.clip)
            .map_or(0.0, |clip| clip.duration);
        if !playing.playing || duration <= 0.0 {
            return false;
        }
        playing.time = (playing.time + seconds as f32) % duration;
        true
    }

    fn playhead(&self) -> Option<Playhead> {
        self.playback.map(|playing| Playhead {
            clip: playing.clip,
            time: playing.time,
        })
    }

    /// Frames the model if nothing has yet, and again when the panel changes
    /// shape under a camera nobody has moved.
    fn sync(&mut self, model: Option<&Model>, size: Size) {
        let changed = size != self.size;
        if size.width > 0.0 && size.height > 0.0 {
            self.size = size;
        }
        let Some(model) = model else {
            self.camera = None;
            return;
        };
        let aspect = self.aspect();
        match &mut self.camera {
            None => self.camera = Some(Camera::framed(model, aspect)),
            Some(camera) if changed && camera.auto_framed && !camera.is_moving() => {
                camera.recenter(model, aspect, false);
            }
            Some(_) => {}
        }
    }
}

fn lock(view: &Mutex<ViewState>) -> MutexGuard<'_, ViewState> {
    view.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// What one preview pane shows.
pub struct ModelViewer {
    id: u64,
    model: Option<Arc<Model>>,
    state: LoadState,
    display: DisplayOptions,
    view: Arc<Mutex<ViewState>>,
    /// How the window's renderer is getting on with this viewer's drawing,
    /// written from `prepare` and read by [`ModelViewer::rendering`].
    health: Arc<Health>,
}

impl Default for ModelViewer {
    fn default() -> Self {
        ModelViewer::new()
    }
}

impl ModelViewer {
    pub fn new() -> ModelViewer {
        ModelViewer {
            id: NEXT_VIEWER.fetch_add(1, Ordering::Relaxed),
            model: None,
            state: LoadState::Empty,
            display: DisplayOptions::default(),
            view: Arc::new(Mutex::new(ViewState {
                camera: None,
                size: Size::ZERO,
                reduced_motion: false,
                playback: None,
            })),
            health: Health::new(),
        }
    }

    pub fn state(&self) -> &LoadState {
        &self.state
    }

    pub fn model(&self) -> Option<&Arc<Model>> {
        self.model.as_ref()
    }

    /// Puts a loaded model on show.
    ///
    /// A reload of the same file keeps the camera where the user left it,
    /// unless the model changed scale so much that the old view would be
    /// inside it or lost; anything else is framed afresh. It keeps the
    /// animation where it was too (the clip, the time, playing or paused);
    /// a model that moves otherwise starts playing its first clip.
    pub fn set_model(&mut self, model: Arc<Model>) {
        let same_file = self.model.as_ref().filter(|old| old.name == model.name);
        let keep = same_file.is_some_and(|old| {
            let ratio = model.radius / old.radius;
            ratio > 0.25 && ratio < 4.0
        });
        let before = same_file.and_then(|_| lock(&self.view).playback);
        {
            let mut view = lock(&self.view);
            match (&mut view.camera, keep) {
                (Some(camera), true) => camera.set_limits(&model),
                (camera, _) => *camera = None,
            }
            view.playback = start_playing(&model, before);
        }
        self.model = Some(model);
        self.state = LoadState::Ready;
        self.health.new_model();
    }

    /// Takes the model off, leaving the stage empty.
    pub fn clear(&mut self) {
        self.model = None;
        self.state = LoadState::Empty;
        let mut view = lock(&self.view);
        view.camera = None;
        view.playback = None;
    }

    /// How the viewport is drawing: whether anything is wrong with the GPU or
    /// the renderer under it that leaves the viewport blank. Updated as the
    /// window draws; the widget publishes [`Event::RenderingChanged`] when it
    /// changes.
    pub fn rendering(&self) -> Rendering {
        self.health.rendering()
    }

    /// Sets how the viewport is drawing, as if the window's renderer had
    /// found it so. A seam for an app's tests of what it shows for each state,
    /// since a test cannot make a GPU fail on demand; nothing else calls it.
    #[doc(hidden)]
    pub fn pretend_rendering(&self, rendering: Rendering) {
        self.health.force(rendering);
    }

    /// Starts the 3D drawing over after it stopped (the device was lost, or
    /// refused the model): the GPU's copy of everything is made again, and the
    /// verdict forgotten. Another viewer of the same window starts over too,
    /// since the GPU is theirs as well.
    pub fn restart(&self) {
        restart_drawing();
        self.health.restart();
    }

    /// Where the model's animation is, if it has any.
    pub fn playback(&self) -> Option<Playback> {
        let model = self.model.as_ref()?;
        let playing = lock(&self.view).playback?;
        Some(Playback {
            clip: playing.clip,
            time: playing.time,
            duration: model.clips.get(playing.clip).map_or(0.0, |c| c.duration),
            playing: playing.playing,
        })
    }

    /// Plays or pauses the animation.
    pub fn set_playing(&self, playing: bool) {
        if let Some(held) = &mut lock(&self.view).playback {
            held.playing = playing;
        }
    }

    /// Plays another clip, from its start.
    pub fn set_clip(&self, clip: usize) {
        let Some(model) = &self.model else { return };
        if clip >= model.clips.len() {
            return;
        }
        lock(&self.view).playback = Some(Playing {
            clip,
            time: 0.0,
            playing: true,
        });
    }

    /// Moves the animation to `time` seconds into its clip (held to the
    /// clip's length), playing or not.
    pub fn seek(&self, time: f32) {
        let Some(model) = &self.model else { return };
        if let Some(held) = &mut lock(&self.view).playback {
            let duration = model.clips.get(held.clip).map_or(0.0, |c| c.duration);
            held.time = if time.is_finite() {
                time.clamp(0.0, duration)
            } else {
                0.0
            };
        }
    }

    pub fn set_failed(&mut self, error: impl Into<String>) {
        self.state = LoadState::Failed(error.into());
    }

    /// Says a load has begun. The model on show stays on the stage while its
    /// newer version loads.
    pub fn set_loading(&mut self, progress: LoadProgress) {
        self.state = LoadState::Loading(progress);
    }

    /// Folds in what a running load reports.
    pub fn apply(&mut self, event: LoadEvent) {
        match event {
            LoadEvent::Progress(progress) => self.set_loading(progress),
            LoadEvent::Done(Ok(model)) => self.set_model(model),
            LoadEvent::Done(Err(LoadError::Cancelled)) => {}
            LoadEvent::Done(Err(error)) => self.set_failed(error.to_string()),
        }
    }

    pub fn display(&self) -> DisplayOptions {
        self.display
    }

    pub fn set_display(&mut self, display: DisplayOptions) {
        self.display = display;
    }

    /// Whether the normals of the model on show can be drawn at all.
    pub fn can_draw_normals(&self) -> bool {
        self.model
            .as_ref()
            .is_some_and(|model| model.info.vertices <= NORMALS_LIMIT)
    }

    /// Whether motion is to be avoided: the camera jumps back instead of
    /// gliding.
    pub fn set_reduced_motion(&self, reduced: bool) {
        lock(&self.view).reduced_motion = reduced;
    }

    /// Glides back to the framed view.
    pub fn recenter(&self) {
        let Some(model) = &self.model else { return };
        let mut view = lock(&self.view);
        let aspect = view.aspect();
        let animate = !view.reduced_motion;
        if let Some(camera) = &mut view.camera {
            camera.recenter(model, aspect, animate);
        }
    }

    /// One step closer (positive) or further (negative).
    pub fn zoom(&self, steps: i32) {
        if let Some(camera) = &mut lock(&self.view).camera {
            camera.dolly(ZOOM_STEP.powi(steps));
        }
    }

    /// The viewport: a widget that fills the space it is given.
    pub fn view<'a>(&'a self, theme: ViewerTheme) -> Element<'a, Event> {
        iced::widget::shader(Canvas {
            viewer: self,
            theme,
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// A job that takes a picture of what the user sees, `scale` times as
    /// large, to run on a thread of its own ([`CaptureJob::run_async`]).
    pub fn capture(&self, scale: f32, theme: ViewerTheme) -> Option<CaptureJob> {
        let model = self.model.clone()?;
        let view = lock(&self.view);
        let camera = view.camera.clone()?;
        let size = (
            (view.size.width * scale).round().clamp(1.0, 4096.0) as u32,
            (view.size.height * scale).round().clamp(1.0, 4096.0) as u32,
        );
        // Of the moment the user sees, animation included.
        let playhead = view.playhead();
        Some(
            CaptureJob::from_camera(model, camera, self.display, size.0, size.1, theme)
                .at(playhead),
        )
    }
}

/// Starts reading a model, as a task the app runs: its events are folded into
/// a viewer with [`ModelViewer::apply`]. The returned token cancels the load.
pub fn load_task(path: std::path::PathBuf, options: LoadOptions) -> (Task<LoadEvent>, Cancel) {
    let (events, cancel) = crate::load::start(path, options);
    (Task::run(events, |event| event), cancel)
}

/// The progress of a load as a sentence, for the loading card.
pub fn describe(progress: &LoadProgress) -> String {
    match progress.phase {
        Phase::Read if progress.total > 0 => format!(
            "Reading, {} of {} MB",
            progress.loaded / (1024 * 1024),
            progress.total.div_ceil(1024 * 1024)
        ),
        Phase::Read => "Reading".into(),
        Phase::Parse => "Decoding".into(),
        Phase::Prepare => "Preparing".into(),
    }
}

// ---------------------------------------------------------------------------
// Pictures
// ---------------------------------------------------------------------------

enum Shot {
    /// A canonical angle on the whole model: an agent's snapshot.
    Angle(SnapshotAngle),
    /// The user's own view.
    Camera {
        camera: Box<Camera>,
        display: DisplayOptions,
    },
}

/// A picture to take: everything it needs, owned, so it can run on any
/// thread while the window goes on.
pub struct CaptureJob {
    model: Arc<Model>,
    shot: Shot,
    size: (u32, u32),
    theme: ViewerTheme,
    /// Where in the animation to draw; `None` takes the start of the first
    /// clip for a model that has one (the pose it opens in).
    playhead: Option<Playhead>,
}

impl CaptureJob {
    /// A picture of any loaded model from a canonical angle, for the agent
    /// that asked. The user's camera and toggles are not involved: the helpers
    /// that are the user's business — wireframe, normals, axes — are left
    /// out, and the floor stays, because it grounds the model and gives it a
    /// scale (seen from below it would only be lines across the underside).
    pub fn snapshot(
        model: Arc<Model>,
        angle: SnapshotAngle,
        width: u32,
        height: u32,
        theme: ViewerTheme,
    ) -> CaptureJob {
        CaptureJob {
            model,
            shot: Shot::Angle(angle),
            size: (width.max(1), height.max(1)),
            theme,
            playhead: None,
        }
    }

    /// A picture from a camera of the caller's: the model as `camera` and
    /// `display` show it.
    pub fn from_camera(
        model: Arc<Model>,
        camera: Camera,
        display: DisplayOptions,
        width: u32,
        height: u32,
        theme: ViewerTheme,
    ) -> CaptureJob {
        CaptureJob {
            model,
            shot: Shot::Camera {
                camera: Box::new(camera),
                display,
            },
            size: (width.max(1), height.max(1)),
            theme,
            playhead: None,
        }
    }

    /// Takes the picture at this moment of the model's animation instead of
    /// its start.
    pub fn at(mut self, playhead: Option<Playhead>) -> CaptureJob {
        self.playhead = playhead;
        self
    }

    /// The picture's size in pixels, as asked.
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Draws the picture and returns it as PNG bytes, blocking.
    pub fn run(self) -> Result<Vec<u8>, CaptureError> {
        let (pixels, size) = self.render()?;
        offscreen::encode_png(pixels, size)
    }

    /// The picture as RGBA pixels, top row first, and its size: what it is
    /// before being encoded.
    pub fn render(&self) -> Result<(Vec<u8>, (u32, u32)), CaptureError> {
        let aspect = f64::from(self.size.0) / f64::from(self.size.1);
        let (camera, display) = match &self.shot {
            Shot::Angle(angle) => (
                snapshot_matrices(&self.model, &view_direction(*angle), aspect),
                DisplayOptions {
                    wireframe: false,
                    grid: *angle != SnapshotAngle::Bottom,
                    axes: false,
                    normals: false,
                },
            ),
            Shot::Camera { camera, display } => {
                (camera.matrices(aspect, self.model.radius), *display)
            }
        };
        let scene = Scene {
            model: Some(&self.model),
            camera,
            display,
            theme: &self.theme,
            pixel_ratio: 1.0,
            playhead: self.playhead.or(Some(Playhead::START)),
            upload_budget: usize::MAX,
        };
        offscreen::render(&scene, self.size)
    }

    /// [`CaptureJob::run`] on a thread of its own, as a future: the shape
    /// `Task::perform` wants.
    pub async fn run_async(self) -> Result<Vec<u8>, CaptureError> {
        let (sender, receiver) = iced::futures::channel::oneshot::channel();
        let spawned = std::thread::Builder::new()
            .name("model3d-capture".into())
            .spawn(move || {
                let _ = sender.send(self.run());
            });
        if let Err(error) = spawned {
            return Err(CaptureError::Gpu(error.to_string()));
        }
        receiver
            .await
            .unwrap_or_else(|_| Err(CaptureError::Gpu("the capture thread stopped".into())))
    }
}

/// `CaptureJob::snapshot(..).run()`.
pub fn snapshot_png(
    model: Arc<Model>,
    angle: SnapshotAngle,
    width: u32,
    height: u32,
    theme: ViewerTheme,
) -> Result<Vec<u8>, CaptureError> {
    CaptureJob::snapshot(model, angle, width, height, theme).run()
}

/// The name a capture is proposed under: `chair-20261026-204512.png` for
/// `chair.glb`.
pub fn capture_name(title: &str, stamp: Stamp) -> String {
    let stem = title.rsplit_once('.').map_or(title, |(stem, _)| stem);
    // Each run of characters a file name cannot hold becomes one dash.
    let mut cleaned = String::new();
    let mut in_run = false;
    for c in stem.chars() {
        let forbidden = "\\/:*?\"<>|".contains(c);
        if !(forbidden && in_run) {
            cleaned.push(if forbidden { '-' } else { c });
        }
        in_run = forbidden;
    }
    let stem = if cleaned.is_empty() {
        "model".to_string()
    } else {
        cleaned
    };
    format!(
        "{stem}-{:04}{:02}{:02}-{:02}{:02}{:02}.png",
        stamp.year, stamp.month, stamp.day, stamp.hour, stamp.minute, stamp.second
    )
}

/// A local date and time, as the app reads it from its clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub year: u32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

// ---------------------------------------------------------------------------
// The widget
// ---------------------------------------------------------------------------

struct Canvas<'a> {
    viewer: &'a ModelViewer,
    theme: ViewerTheme,
}

struct Drag {
    button: mouse::Button,
    last: Point,
}

/// What the widget remembers between events: it lives in iced's widget tree,
/// not in the viewer.
#[derive(Default)]
struct Interaction {
    drag: Option<Drag>,
    modifiers: keyboard::Modifiers,
    /// The view has the keyboard: it was clicked and nothing else has been
    /// since.
    focused: bool,
    last_frame: Option<Instant>,
}

impl Canvas<'_> {
    fn wheel(&self, delta: mouse::ScrollDelta, bounds: Rectangle, cursor: mouse::Cursor) -> bool {
        let Some(position) = cursor.position_in(bounds) else {
            return false;
        };
        // Browser convention: a notch is 100 and scrolling up is negative.
        let amount = match delta {
            mouse::ScrollDelta::Lines { y, .. } => -f64::from(y) * WHEEL_NOTCH,
            mouse::ScrollDelta::Pixels { y, .. } => -f64::from(y),
        }
        .clamp(-3.0 * WHEEL_NOTCH, 3.0 * WHEEL_NOTCH);
        if amount == 0.0 {
            return true;
        }
        let scale = 0.95f64.powf(amount.abs() * 0.01);
        let scale = if amount < 0.0 { scale } else { 1.0 / scale };
        let ndc = (
            f64::from(position.x / bounds.width) * 2.0 - 1.0,
            1.0 - f64::from(position.y / bounds.height) * 2.0,
        );
        let mut view = lock(&self.viewer.view);
        let aspect = view.aspect();
        if let Some(camera) = &mut view.camera {
            camera.dolly_to(scale, ndc, aspect);
        }
        true
    }

    fn drag(
        &self,
        button: mouse::Button,
        modifiers: keyboard::Modifiers,
        delta: Point,
        height: f32,
    ) {
        let mut view = lock(&self.viewer.view);
        let Some(camera) = &mut view.camera else {
            return;
        };
        let height = f64::from(height.max(1.0));
        let (dx, dy) = (f64::from(delta.x), f64::from(delta.y));
        let panning = button == mouse::Button::Right
            || (button == mouse::Button::Left
                && (modifiers.shift() || modifiers.control() || modifiers.logo()));
        if panning {
            camera.pan_pixels(dx, dy, height);
        } else if button == mouse::Button::Left {
            let turn = std::f64::consts::TAU / height;
            camera.rotate(dx * turn, dy * turn);
        } else if button == mouse::Button::Middle && dy != 0.0 {
            let scale = 0.95f64.powf(dy.abs() * 0.01);
            camera.dolly(if dy > 0.0 { 1.0 / scale } else { scale });
        }
    }

    /// The keys of the old viewer: arrows pan, with Shift or Ctrl they turn;
    /// plus and minus zoom; Home recentres. Returns whether it was one.
    fn key(
        &self,
        key: &keyboard::Key,
        modified: &keyboard::Key,
        modifiers: keyboard::Modifiers,
        height: f32,
    ) -> bool {
        use keyboard::Key;
        use keyboard::key::Named;
        let turning = modifiers.shift() || modifiers.control() || modifiers.logo();
        let height = f64::from(height.max(1.0));
        let turn = std::f64::consts::TAU * KEY_ROTATE_PIXELS / height;
        let arrow = |dx: f64, dy: f64, left: f64, up: f64| {
            let mut view = lock(&self.viewer.view);
            if let Some(camera) = &mut view.camera {
                if turning {
                    camera.rotate(left * turn, up * turn);
                } else {
                    camera.pan_pixels(dx * KEY_PAN_PIXELS, dy * KEY_PAN_PIXELS, height);
                }
            }
        };
        match key {
            Key::Named(Named::ArrowUp) => arrow(0.0, 1.0, 0.0, 1.0),
            Key::Named(Named::ArrowDown) => arrow(0.0, -1.0, 0.0, -1.0),
            Key::Named(Named::ArrowLeft) => arrow(1.0, 0.0, 1.0, 0.0),
            Key::Named(Named::ArrowRight) => arrow(-1.0, 0.0, -1.0, 0.0),
            Key::Named(Named::Home) if !modifiers.alt() && !modifiers.command() => {
                self.viewer.recenter();
            }
            // The character the key makes, with Shift applied or not: plus is
            // Shift+= on a US keyboard, and either is a step closer.
            _ if !modifiers.alt() && !modifiers.command() => {
                let typed = |key: &Key| match key {
                    Key::Character(text) => text.as_str().chars().next(),
                    _ => None,
                };
                match typed(modified).or_else(|| typed(key)) {
                    Some('+' | '=') => self.viewer.zoom(1),
                    Some('-' | '_') => self.viewer.zoom(-1),
                    _ => return false,
                }
            }
            _ => return false,
        }
        true
    }
}

impl shader::Program<Event> for Canvas<'_> {
    type State = Interaction;
    type Primitive = Primitive;

    fn update(
        &self,
        state: &mut Interaction,
        event: &IcedEvent,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Event>> {
        match event {
            IcedEvent::Window(window::Event::RedrawRequested(now)) => {
                let mut view = lock(&self.viewer.view);
                view.sync(self.viewer.model.as_deref(), bounds.size());
                let seconds = state
                    .last_frame
                    .map_or(0.0, |before| {
                        now.saturating_duration_since(before).as_secs_f64()
                    })
                    .min(LONGEST_FRAME);
                state.last_frame = Some(*now);
                let moving = view.camera.as_mut().is_some_and(|camera| {
                    camera.advance(seconds);
                    camera.is_moving()
                });
                // A playing animation asks for a frame after each one.
                let animating = view.advance(self.viewer.model.as_deref(), seconds);
                drop(view);
                // The renderer has had a frame since the last look: is it
                // drawing? A verdict is told to the app, which looks again.
                let review = self.viewer.health.review();
                if review.changed {
                    return Some(Action::publish(Event::RenderingChanged));
                }
                if moving || animating || review.redraw {
                    return Some(Action::request_redraw());
                }
                state.last_frame = None;
                None
            }
            IcedEvent::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers;
                None
            }
            IcedEvent::Keyboard(keyboard::Event::KeyPressed {
                key,
                modified_key,
                modifiers,
                ..
            }) => {
                if state.focused && self.key(key, modified_key, *modifiers, bounds.height) {
                    return Some(Action::request_redraw().and_capture());
                }
                None
            }
            IcedEvent::Mouse(mouse::Event::ButtonPressed(button)) => {
                if cursor.position_in(bounds).is_none() {
                    state.focused = false;
                    return None;
                }
                state.focused = true;
                if !matches!(
                    button,
                    mouse::Button::Left | mouse::Button::Middle | mouse::Button::Right
                ) {
                    return None;
                }
                state.drag = cursor.position().map(|last| Drag {
                    button: *button,
                    last,
                });
                if let Some(camera) = &mut lock(&self.viewer.view).camera {
                    camera.take_control();
                }
                Some(Action::publish(Event::Interacted).and_capture())
            }
            IcedEvent::Mouse(mouse::Event::CursorMoved { position }) => {
                let drag = state.drag.as_mut()?;
                let delta = Point::new(position.x - drag.last.x, position.y - drag.last.y);
                drag.last = *position;
                let button = drag.button;
                self.drag(button, state.modifiers, delta, bounds.height);
                Some(Action::request_redraw().and_capture())
            }
            IcedEvent::Mouse(mouse::Event::ButtonReleased(button)) => {
                if state
                    .drag
                    .as_ref()
                    .is_some_and(|drag| drag.button == *button)
                {
                    state.drag = None;
                    // The camera is still easing to a stop.
                    return Some(Action::request_redraw().and_capture());
                }
                None
            }
            IcedEvent::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if self.wheel(*delta, bounds, cursor) {
                    return Some(Action::request_redraw().and_capture());
                }
                None
            }
            _ => None,
        }
    }

    fn draw(&self, _state: &Interaction, _cursor: mouse::Cursor, bounds: Rectangle) -> Primitive {
        // Counted so that a renderer that never runs the primitive's
        // `prepare` (iced's software one) shows up as the difference.
        self.viewer.health.drawn();
        let model = self.viewer.model.clone();
        let (camera, playhead) = {
            let mut view = lock(&self.viewer.view);
            view.sync(model.as_deref(), bounds.size());
            let aspect = f64::from(bounds.width.max(1.0)) / f64::from(bounds.height.max(1.0));
            (
                model
                    .as_ref()
                    .zip(view.camera.as_ref())
                    .map(|(model, camera)| camera.matrices(aspect, model.radius)),
                view.playhead(),
            )
        };
        Primitive {
            viewer: self.viewer.id,
            model,
            camera: camera.unwrap_or(CameraMatrices {
                view_proj: glam::Mat4::IDENTITY,
                eye: [0.0; 3],
            }),
            display: self.viewer.display,
            theme: self.theme,
            playhead,
            health: self.viewer.health.clone(),
        }
    }

    fn mouse_interaction(
        &self,
        state: &Interaction,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.drag.is_some() {
            mouse::Interaction::Grabbing
        } else if cursor.is_over(bounds) && self.viewer.model.is_some() {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::None
        }
    }
}

// ---------------------------------------------------------------------------
// The window's side of the drawing
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Primitive {
    viewer: u64,
    model: Option<Arc<Model>>,
    camera: CameraMatrices,
    display: DisplayOptions,
    theme: ViewerTheme,
    playhead: Option<Playhead>,
    /// Where `prepare` says how it went.
    health: Arc<Health>,
}

/// Counts the restarts the app has asked for ([`ModelViewer::restart`]). A
/// window's pipeline starts its renderer over when it sees a number it has not
/// seen: the pipeline belongs to iced and is out of the app's reach.
static RESTARTS: AtomicU64 = AtomicU64::new(0);

/// Why the window's device was lost, if it was.
static DEVICE_LOST: Mutex<Option<String>> = Mutex::new(None);

fn restart_drawing() {
    *DEVICE_LOST
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    RESTARTS.fetch_add(1, Ordering::SeqCst);
}

fn device_lost() -> Option<String> {
    DEVICE_LOST
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// The renderer on a device, or why it could not be made.
fn start(device: &wgpu::Device, queue: &wgpu::Queue) -> (Option<Gpu>, Option<String>) {
    match guarded(device, || Gpu::new(device, queue)) {
        Ok(gpu) => (Some(gpu), None),
        Err(error) => {
            eprintln!("hangar-model3d: the 3D preview cannot draw: {error}");
            (None, Some(error))
        }
    }
}

/// What iced keeps for every viewer of the window: the renderer, and the pass
/// that copies its pictures in.
struct WindowPipeline {
    /// `None` when the device refused the pipelines: the viewport stays empty
    /// rather than the application going down with it, and says why
    /// (`failure`).
    gpu: Option<Gpu>,
    failure: Option<String>,
    blit: Blit,
    /// The restarts this pipeline has been through.
    restarts: u64,
}

impl WindowPipeline {
    /// Starts the renderer over if the app asked since the last frame.
    fn follow_restarts(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let asked = RESTARTS.load(Ordering::SeqCst);
        if asked == self.restarts {
            return;
        }
        self.restarts = asked;
        (self.gpu, self.failure) = start(device, queue);
        self.blit.targets.clear();
    }
}

impl shader::Pipeline for WindowPipeline {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        // Told when the device goes: nothing else can say so, since iced does
        // not look. A device dropped on purpose (the window closing) is not a
        // loss.
        device.set_device_lost_callback(|reason, message| {
            if reason != wgpu::DeviceLostReason::Destroyed {
                eprintln!("hangar-model3d: the graphics device was lost: {message}");
                *DEVICE_LOST
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                    Some(format!("{reason:?}: {message}"));
            }
        });
        let (gpu, failure) = start(device, queue);
        WindowPipeline {
            gpu,
            failure,
            blit: Blit::new(device, format),
            restarts: RESTARTS.load(Ordering::SeqCst),
        }
    }

    fn trim(&mut self) {
        let Some(gpu) = &mut self.gpu else { return };
        gpu.trim();
        let gpu = &*gpu;
        self.blit
            .targets
            .retain(|viewer, _| gpu.picture(*viewer).is_some());
    }
}

impl shader::Primitive for Primitive {
    type Pipeline = WindowPipeline;

    fn prepare(
        &self,
        pipeline: &mut WindowPipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &Viewport,
    ) {
        pipeline.follow_restarts(device, queue);
        if let Some(reason) = device_lost() {
            self.health.prepared(Prepared::Lost(reason));
            return;
        }
        if self.health.failed() {
            // Until the next model or a restart (`Health::new_model`).
            return;
        }
        let scale = viewport.scale_factor();
        let size = (
            (bounds.width * scale).round().max(1.0) as u32,
            (bounds.height * scale).round().max(1.0) as u32,
        );
        let scene = Scene {
            model: self.model.as_ref(),
            camera: self.camera,
            display: self.display,
            theme: &self.theme,
            pixel_ratio: scale,
            playhead: self.playhead,
            upload_budget: WINDOW_UPLOAD_BUDGET,
        };
        let Some(gpu) = &mut pipeline.gpu else {
            let reason = pipeline
                .failure
                .clone()
                .unwrap_or_else(|| "the graphics device could not be set up".into());
            self.health.prepared(Prepared::Unavailable(reason));
            return;
        };
        let uploading = match guarded(device, || gpu.prepare(self.viewer, size, &scene)) {
            Ok(uploading) => uploading,
            Err(error) => {
                eprintln!("hangar-model3d: the model could not be drawn: {error}");
                // Nothing is drawn for this viewer until the next model: its
                // target is gone, and so is the copy of the model that failed.
                gpu.release(self.viewer);
                if let Some(model) = &self.model {
                    gpu.forget(model.id);
                }
                pipeline.blit.targets.remove(&self.viewer);
                self.health.prepared(Prepared::Failed(error));
                return;
            }
        };
        if let Some(picture) = gpu.picture(self.viewer) {
            let origin = [
                (bounds.x * scale).round() as i32,
                (bounds.y * scale).round() as i32,
            ];
            pipeline
                .blit
                .prepare(device, queue, self.viewer, picture, origin);
        }
        self.health.prepared(Prepared::Drew { uploading });
    }

    fn render(
        &self,
        pipeline: &WindowPipeline,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip_bounds: &Rectangle<u32>,
    ) {
        let Some(gpu) = &pipeline.gpu else { return };
        gpu.encode(encoder, self.viewer);
        pipeline
            .blit
            .render(encoder, target, clip_bounds, self.viewer);
    }
}

/// Copies a viewer's finished picture into the window's frame.
struct Blit {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    /// The window's format is not sRGB, so the copy encodes by hand.
    encode: bool,
    targets: HashMap<u64, BlitTarget>,
}

struct BlitTarget {
    uniform: wgpu::Buffer,
    group: wgpu::BindGroup,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BlitUniform {
    origin: [i32; 2],
    encode: u32,
    padding: u32,
}

impl Blit {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Blit {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("model3d blit"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("model3d blit"),
            source: wgpu::ShaderSource::Wgsl(include_str!("render/shaders/blit.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("model3d blit"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("model3d blit"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        Blit {
            pipeline,
            layout,
            encode: !format.is_srgb(),
            targets: HashMap::new(),
        }
    }

    fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        viewer: u64,
        picture: &wgpu::Texture,
        origin: [i32; 2],
    ) {
        let uniform = BlitUniform {
            origin,
            encode: u32::from(self.encode),
            padding: 0,
        };
        let buffer = match self.targets.remove(&viewer) {
            Some(target) => target.uniform,
            None => device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("model3d blit"),
                size: std::mem::size_of::<BlitUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
        };
        queue.write_buffer(&buffer, 0, bytemuck::bytes_of(&uniform));
        // Made again for every frame: it names the picture's texture, which is
        // a new one whenever the viewport changes size.
        let view = picture.create_view(&wgpu::TextureViewDescriptor::default());
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("model3d blit"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: buffer.as_entire_binding(),
                },
            ],
        });
        self.targets.insert(
            viewer,
            BlitTarget {
                uniform: buffer,
                group,
            },
        );
    }

    fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip: &Rectangle<u32>,
        viewer: u64,
    ) {
        let Some(blit) = self.targets.get(&viewer) else {
            return;
        };
        if clip.width == 0 || clip.height == 0 {
            return;
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("model3d blit"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_scissor_rect(clip.x, clip.y, clip.width, clip.height);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &blit.group, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proposes_a_file_name_from_the_models_and_the_time() {
        let stamp = Stamp {
            year: 2026,
            month: 10,
            day: 26,
            hour: 20,
            minute: 45,
            second: 12,
        };
        assert_eq!(
            capture_name("chair.glb", stamp),
            "chair-20261026-204512.png"
        );
        assert_eq!(
            capture_name("my:chair?.glb", stamp),
            "my-chair--20261026-204512.png"
        );
        assert_eq!(capture_name(".glb", stamp), "model-20261026-204512.png");
        assert_eq!(capture_name("scan", stamp), "scan-20261026-204512.png");
    }

    #[test]
    fn describes_the_phases_of_a_load() {
        let read = LoadProgress {
            phase: Phase::Read,
            loaded: 5 * 1024 * 1024,
            total: 20 * 1024 * 1024,
        };
        assert_eq!(describe(&read), "Reading, 5 of 20 MB");
        assert_eq!(
            describe(&LoadProgress {
                phase: Phase::Parse,
                ..read
            }),
            "Decoding"
        );
        assert_eq!(
            describe(&LoadProgress {
                phase: Phase::Prepare,
                ..read
            }),
            "Preparing"
        );
    }

    // The widget's pointer and keyboard handling, driven with events made by
    // hand: no compositor is needed to know what a drag does.

    use crate::formats::ModelFormat;
    use crate::load::{Cancel, load_bytes};
    use crate::samples;
    use iced::keyboard::{Key, Location, Modifiers, key::Named};
    use std::time::Duration;

    fn cube(name: &str) -> Arc<Model> {
        Arc::new(
            load_bytes(
                name,
                ModelFormat::Glb,
                &samples::cube_glb(),
                std::path::Path::new("."),
                &LoadOptions::default(),
                &Cancel::default(),
                &mut |_| {},
            )
            .unwrap(),
        )
    }

    const BOUNDS: Rectangle = Rectangle {
        x: 100.0,
        y: 50.0,
        width: 800.0,
        height: 600.0,
    };

    struct Rig {
        viewer: ModelViewer,
        state: Interaction,
        clock: Instant,
    }

    impl Rig {
        fn new() -> Rig {
            Rig::with(cube("cube.glb"))
        }

        fn with(model: Arc<Model>) -> Rig {
            let mut viewer = ModelViewer::new();
            viewer.set_model(model);
            let mut rig = Rig {
                viewer,
                state: Interaction::default(),
                clock: Instant::now(),
            };
            // The first frame lays the view out and frames the model.
            rig.frame(0);
            rig
        }

        fn send(&mut self, event: IcedEvent, at: Point) -> Option<Action<Event>> {
            let canvas = Canvas {
                viewer: &self.viewer,
                theme: ViewerTheme::dark(),
            };
            shader::Program::update(
                &canvas,
                &mut self.state,
                &event,
                BOUNDS,
                mouse::Cursor::Available(at),
            )
        }

        /// A frame `ms` after the last.
        fn frame(&mut self, ms: u64) -> Option<Action<Event>> {
            self.clock += Duration::from_millis(ms);
            let now = self.clock;
            self.send(
                IcedEvent::Window(window::Event::RedrawRequested(now)),
                Point::ORIGIN,
            )
        }

        /// Frames at 60 Hz until the camera stops, with a bound.
        fn settle(&mut self) {
            for _ in 0..600 {
                if self.frame(16).is_none() {
                    return;
                }
            }
            panic!("the camera never stopped");
        }

        fn camera<T>(&self, read: impl FnOnce(&Camera) -> T) -> T {
            read(lock(&self.viewer.view).camera.as_ref().unwrap())
        }

        fn eye(&self) -> glam::DVec3 {
            self.camera(|c| c.eye)
        }

        fn target(&self) -> glam::DVec3 {
            self.camera(|c| c.target)
        }

        fn distance(&self) -> f64 {
            self.camera(Camera::distance)
        }

        fn auto(&self) -> bool {
            self.camera(|c| c.auto_framed)
        }

        fn centre() -> Point {
            Point::new(
                BOUNDS.x + BOUNDS.width / 2.0,
                BOUNDS.y + BOUNDS.height / 2.0,
            )
        }

        fn press(&mut self, button: mouse::Button, at: Point) -> Option<Action<Event>> {
            self.send(IcedEvent::Mouse(mouse::Event::ButtonPressed(button)), at)
        }

        fn release(&mut self, button: mouse::Button, at: Point) -> Option<Action<Event>> {
            self.send(IcedEvent::Mouse(mouse::Event::ButtonReleased(button)), at)
        }

        fn drag_to(&mut self, to: Point) -> Option<Action<Event>> {
            self.send(
                IcedEvent::Mouse(mouse::Event::CursorMoved { position: to }),
                to,
            )
        }

        fn wheel(&mut self, y: f32, at: Point) -> Option<Action<Event>> {
            self.send(
                IcedEvent::Mouse(mouse::Event::WheelScrolled {
                    delta: mouse::ScrollDelta::Lines { x: 0.0, y },
                }),
                at,
            )
        }

        fn key(&mut self, key: Key, modifiers: Modifiers) -> Option<Action<Event>> {
            let event = IcedEvent::Keyboard(keyboard::Event::KeyPressed {
                modified_key: key.clone(),
                key,
                physical_key: keyboard::key::Physical::Unidentified(
                    keyboard::key::NativeCode::Unidentified,
                ),
                location: Location::Standard,
                modifiers,
                text: None,
                repeat: false,
            });
            self.send(event, Rig::centre())
        }

        /// A click on the view, which is what gives it the keyboard.
        fn click(&mut self) {
            self.press(mouse::Button::Left, Rig::centre());
            self.release(mouse::Button::Left, Rig::centre());
        }
    }

    fn captured(action: Option<Action<Event>>) -> bool {
        action.is_some_and(|action| action.into_inner().2 == iced::event::Status::Captured)
    }

    #[test]
    fn frames_the_model_on_the_first_frame() {
        let rig = Rig::new();
        assert!(rig.auto());
        assert!((lock(&rig.viewer.view).aspect() - 800.0 / 600.0).abs() < 1e-9);
    }

    #[test]
    fn turns_the_camera_when_the_left_button_drags() {
        let mut rig = Rig::new();
        let before = rig.eye();
        let target = rig.target();
        let start = Rig::centre();
        let (message, _, status) = rig.press(mouse::Button::Left, start).unwrap().into_inner();
        // The press tells the app, and is not passed on to what is under it.
        assert_eq!(message, Some(Event::Interacted));
        assert_eq!(status, iced::event::Status::Captured);
        let end = Point::new(start.x + 120.0, start.y);
        assert!(captured(rig.drag_to(end)));
        rig.release(mouse::Button::Left, end);
        rig.settle();
        assert!((rig.eye() - before).length() > 0.1, "the camera moved");
        assert!(!rig.auto());
        // Around the target, not toward it.
        assert!((rig.distance() - (before - target).length()).abs() < 1e-6);
        assert!((rig.target() - target).length() < 1e-9);
    }

    #[test]
    fn eases_to_a_stop_after_the_button_is_released() {
        let mut rig = Rig::new();
        let start = Rig::centre();
        let end = Point::new(start.x + 200.0, start.y);
        rig.press(mouse::Button::Left, start);
        rig.drag_to(end);
        rig.release(mouse::Button::Left, end);
        // Asks for another frame while it eases, and stops asking once at rest.
        assert!(rig.frame(16).is_some());
        rig.settle();
        assert!(rig.frame(16).is_none());
    }

    #[test]
    fn pans_with_the_right_button_and_with_shift() {
        let mut rig = Rig::new();
        let before = rig.target();
        let start = Rig::centre();
        rig.press(mouse::Button::Right, start);
        rig.drag_to(Point::new(start.x + 80.0, start.y + 40.0));
        rig.release(mouse::Button::Right, start);
        rig.settle();
        assert!((rig.target() - before).length() > 0.05, "right button pans");

        let before = rig.target();
        rig.send(
            IcedEvent::Keyboard(keyboard::Event::ModifiersChanged(Modifiers::SHIFT)),
            start,
        );
        rig.press(mouse::Button::Left, start);
        rig.drag_to(Point::new(start.x - 80.0, start.y));
        rig.release(mouse::Button::Left, start);
        rig.settle();
        assert!(
            (rig.target() - before).length() > 0.05,
            "shift and left pans"
        );
    }

    #[test]
    fn zooms_with_the_wheel_only_over_the_view() {
        let mut rig = Rig::new();
        let before = rig.distance();
        assert!(captured(rig.wheel(1.0, Rig::centre())));
        let closer = rig.distance();
        assert!(
            (closer / before - 0.95).abs() < 1e-9,
            "one notch is a twentieth closer"
        );
        rig.wheel(-2.0, Rig::centre());
        assert!(rig.distance() > closer, "away again");
        // Over the toolbar, the wheel is not the view's.
        let distance = rig.distance();
        assert!(rig.wheel(1.0, Point::new(10.0, 10.0)).is_none());
        assert_eq!(rig.distance(), distance);
    }

    #[test]
    fn zooms_toward_the_pointer_not_the_middle() {
        let mut rig = Rig::new();
        let corner = Point::new(
            BOUNDS.x + BOUNDS.width * 0.9,
            BOUNDS.y + BOUNDS.height * 0.2,
        );
        let before = rig.target();
        rig.wheel(3.0, corner);
        assert!((rig.target() - before).length() > 0.01);
        // At the middle it goes straight in.
        let mut rig = Rig::new();
        let before = rig.target();
        rig.wheel(3.0, Rig::centre());
        assert!((rig.target() - before).length() < 1e-9);
    }

    #[test]
    fn takes_the_keyboard_only_after_a_click_on_the_view() {
        let mut rig = Rig::new();
        let before = rig.distance();
        assert!(
            rig.key(Key::Character("+".into()), Modifiers::default())
                .is_none()
        );
        assert_eq!(rig.distance(), before);

        rig.click();
        assert!(captured(
            rig.key(Key::Character("+".into()), Modifiers::SHIFT)
        ));
        assert!(
            (rig.distance() / before - 0.8).abs() < 1e-9,
            "a step closer"
        );
        rig.key(Key::Character("-".into()), Modifiers::default());
        assert!((rig.distance() - before).abs() < 1e-9);

        // A click elsewhere gives it back.
        rig.press(mouse::Button::Left, Point::new(5.0, 5.0));
        assert!(
            rig.key(Key::Character("+".into()), Modifiers::default())
                .is_none()
        );
    }

    #[test]
    fn pans_and_turns_with_the_arrows_and_recentres_with_home() {
        let mut rig = Rig::new();
        rig.click();
        let start = rig.target();
        rig.key(Key::Named(Named::ArrowLeft), Modifiers::default());
        rig.settle();
        assert!((rig.target() - start).length() > 0.001, "an arrow pans");

        let eye = rig.eye();
        rig.key(Key::Named(Named::ArrowRight), Modifiers::SHIFT);
        rig.settle();
        assert!(
            (rig.eye() - eye).length() > 0.01,
            "shift and an arrow turns"
        );

        rig.key(Key::Named(Named::Home), Modifiers::default());
        assert!(rig.auto());
        rig.settle();
        assert!(
            (rig.target() - start).length() < 1e-9,
            "home is the framed view"
        );
    }

    #[test]
    fn frames_the_model_again_when_the_panel_changes_shape_untouched() {
        let mut rig = Rig::new();
        let wide = rig.distance();
        // The same camera, in a narrower panel.
        let narrow = Rectangle {
            width: 300.0,
            ..BOUNDS
        };
        let canvas = Canvas {
            viewer: &rig.viewer,
            theme: ViewerTheme::dark(),
        };
        let now = rig.clock + Duration::from_millis(16);
        shader::Program::update(
            &canvas,
            &mut rig.state,
            &IcedEvent::Window(window::Event::RedrawRequested(now)),
            narrow,
            mouse::Cursor::Unavailable,
        );
        assert!(
            rig.distance() > wide,
            "stands further back in a narrow panel"
        );
        assert!(rig.auto());

        // Once the user has moved it, the panel's shape is no business of the
        // framing.
        rig.clock = now;
        rig.press(mouse::Button::Left, Rig::centre());
        rig.drag_to(Point::new(Rig::centre().x + 30.0, Rig::centre().y));
        rig.release(mouse::Button::Left, Rig::centre());
        rig.settle();
        let distance = rig.distance();
        rig.frame(500);
        assert!((rig.distance() - distance).abs() < 1e-9);
    }

    #[test]
    fn keeps_the_camera_across_a_reload_of_the_same_file_only() {
        let mut rig = Rig::new();
        rig.viewer.zoom(3);
        let moved = rig.distance();
        rig.viewer.set_model(cube("cube.glb"));
        rig.frame(16);
        assert!(
            (rig.distance() - moved).abs() < 1e-9,
            "a reload leaves the camera"
        );

        rig.viewer.set_model(cube("another.glb"));
        rig.frame(16);
        assert!(
            rig.distance() > moved * 1.5,
            "another file is framed afresh"
        );
        assert!(rig.auto());
    }

    #[test]
    fn follows_a_load_through_to_a_model_or_an_error() {
        let mut viewer = ModelViewer::new();
        assert_eq!(viewer.state(), &LoadState::Empty);
        let progress = LoadProgress {
            phase: Phase::Read,
            loaded: 1,
            total: 2,
        };
        viewer.apply(LoadEvent::Progress(progress));
        assert_eq!(viewer.state(), &LoadState::Loading(progress));
        viewer.apply(LoadEvent::Done(Err(LoadError::Cancelled)));
        assert_eq!(
            viewer.state(),
            &LoadState::Loading(progress),
            "a cancelled load says nothing"
        );
        viewer.apply(LoadEvent::Done(Err(LoadError::Read("gone".into()))));
        assert_eq!(viewer.state(), &LoadState::Failed("gone".into()));
        viewer.apply(LoadEvent::Done(Ok(cube("cube.glb"))));
        assert_eq!(viewer.state(), &LoadState::Ready);
        assert!(viewer.can_draw_normals());
        viewer.clear();
        assert!(viewer.model().is_none());
    }

    #[test]
    fn shows_the_cursor_of_a_thing_that_can_be_grabbed() {
        let mut rig = Rig::new();
        let interaction = |rig: &Rig, cursor| {
            let canvas = Canvas {
                viewer: &rig.viewer,
                theme: ViewerTheme::dark(),
            };
            shader::Program::mouse_interaction(&canvas, &rig.state, BOUNDS, cursor)
        };
        let over = interaction(&rig, mouse::Cursor::Available(Rig::centre()));
        assert_eq!(over, mouse::Interaction::Grab);
        rig.press(mouse::Button::Left, Rig::centre());
        assert_eq!(
            interaction(&rig, mouse::Cursor::Unavailable),
            mouse::Interaction::Grabbing
        );
    }

    // Animation: the clock, the controls, and what a reload keeps.

    fn glb(name: &str, bytes: Vec<u8>) -> Arc<Model> {
        Arc::new(
            load_bytes(
                name,
                ModelFormat::Glb,
                &bytes,
                std::path::Path::new("."),
                &LoadOptions::default(),
                &Cancel::default(),
                &mut |_| {},
            )
            .unwrap(),
        )
    }

    /// A rig over the cube that slides for two seconds and spins for one.
    fn sliding() -> Rig {
        Rig::with(glb("slide.glb", samples::animated_cube_glb()))
    }

    fn playback(rig: &Rig) -> Playback {
        rig.viewer.playback().expect("the model has clips")
    }

    #[test]
    fn a_model_that_moves_starts_playing_its_first_clip() {
        let rig = sliding();
        assert_eq!(
            playback(&rig),
            Playback {
                clip: 0,
                time: 0.0,
                duration: 2.0,
                playing: true
            }
        );
    }

    #[test]
    fn a_model_that_stands_still_has_no_playback() {
        let rig = Rig::new();
        assert_eq!(rig.viewer.playback(), None);
        // Nothing to control: the calls do nothing.
        rig.viewer.set_playing(false);
        rig.viewer.set_clip(1);
        rig.viewer.seek(1.0);
        assert_eq!(rig.viewer.playback(), None);
    }

    #[test]
    fn a_playing_clip_moves_on_with_each_frame_and_asks_for_the_next() {
        // The frame that laid the view out only started the clock.
        let mut rig = sliding();
        assert!(playback(&rig).time.abs() < 1e-6);
        assert!(rig.frame(50).is_some(), "a frame is wanted");
        assert!((playback(&rig).time - 0.05).abs() < 1e-5);
        assert!(rig.frame(50).is_some());
        assert!((playback(&rig).time - 0.10).abs() < 1e-5);
    }

    #[test]
    fn a_clip_that_reaches_its_end_starts_again() {
        let mut rig = sliding();
        rig.viewer.seek(1.95);
        rig.frame(0);
        rig.frame(100);
        let time = playback(&rig).time;
        assert!((time - 0.05).abs() < 1e-4, "round the end: {time}");
        assert!(playback(&rig).playing);
    }

    #[test]
    fn a_long_pause_of_the_window_is_not_time_for_the_animation() {
        let mut rig = sliding();
        rig.frame(0);
        // The window was hidden for a minute.
        rig.frame(60_000);
        assert!(playback(&rig).time <= LONGEST_FRAME as f32 + 1e-6);
    }

    #[test]
    fn a_paused_clip_stands_still_and_asks_for_nothing() {
        let mut rig = sliding();
        rig.frame(0);
        rig.frame(100);
        rig.viewer.set_playing(false);
        let held = playback(&rig).time;
        assert!(
            rig.frame(100).is_none(),
            "no more frames while it is paused"
        );
        assert_eq!(playback(&rig).time, held);
        assert!(!playback(&rig).playing);
        // Played again, it carries on from there.
        rig.viewer.set_playing(true);
        rig.frame(0);
        assert!(rig.frame(100).is_some());
        assert!((playback(&rig).time - (held + 0.1)).abs() < 1e-4);
    }

    #[test]
    fn seeking_is_held_to_the_clip_and_leaves_it_paused_or_playing() {
        let rig = sliding();
        rig.viewer.set_playing(false);
        rig.viewer.seek(1.25);
        assert_eq!(playback(&rig).time, 1.25);
        rig.viewer.seek(99.0);
        assert_eq!(playback(&rig).time, 2.0);
        rig.viewer.seek(-3.0);
        assert_eq!(playback(&rig).time, 0.0);
        rig.viewer.seek(f32::NAN);
        assert_eq!(playback(&rig).time, 0.0);
        assert!(!playback(&rig).playing, "seeking does not play");
        rig.viewer.set_playing(true);
        rig.viewer.seek(0.5);
        assert!(playback(&rig).playing, "and does not pause");
    }

    #[test]
    fn another_clip_plays_from_its_beginning() {
        let rig = sliding();
        rig.viewer.seek(1.5);
        rig.viewer.set_playing(false);
        rig.viewer.set_clip(1);
        assert_eq!(
            playback(&rig),
            Playback {
                clip: 1,
                time: 0.0,
                duration: 1.0,
                playing: true
            }
        );
        // There is no third.
        rig.viewer.set_clip(2);
        assert_eq!(playback(&rig).clip, 1);
    }

    #[test]
    fn a_reload_of_the_same_file_carries_on_where_it_was() {
        let mut rig = sliding();
        rig.viewer.set_clip(1);
        rig.viewer.seek(0.4);
        rig.viewer.set_playing(false);
        rig.viewer
            .set_model(glb("slide.glb", samples::animated_cube_glb()));
        assert_eq!(
            playback(&rig),
            Playback {
                clip: 1,
                time: 0.4,
                duration: 1.0,
                playing: false
            }
        );

        // Another file starts afresh, even when it has the same clips.
        rig.viewer
            .set_model(glb("other.glb", samples::animated_cube_glb()));
        assert_eq!(playback(&rig).clip, 0);
        assert!(playback(&rig).playing);
        rig.frame(16);
    }

    #[test]
    fn a_reload_that_lost_the_clip_starts_over_and_one_that_shortened_it_clamps() {
        let mut rig = sliding();
        rig.viewer.set_clip(1);
        rig.viewer.seek(0.9);
        rig.viewer.set_playing(false);
        // The new version has one clip: the second is gone, so it starts
        // afresh, playing.
        rig.viewer
            .set_model(glb("slide.glb", samples::skinned_glb()));
        assert_eq!(playback(&rig).clip, 0);
        assert_eq!(playback(&rig).time, 0.0);
        assert!(playback(&rig).playing);

        // The first clip is now a second long, and the time was past that.
        let mut rig = sliding();
        rig.viewer.seek(1.9);
        rig.viewer
            .set_model(glb("slide.glb", samples::skinned_glb()));
        assert_eq!(playback(&rig).clip, 0);
        assert_eq!(playback(&rig).time, 1.0);
    }

    #[test]
    fn a_capture_is_of_the_moment_on_screen() {
        let rig = sliding();
        rig.viewer.seek(1.0);
        let job = rig
            .viewer
            .capture(1.0, ViewerTheme::dark())
            .expect("a capture");
        assert_eq!(job.playhead, Some(Playhead { clip: 0, time: 1.0 }));
    }

    #[test]
    fn recentring_glides_back_unless_motion_is_reduced_and_then_jumps() {
        let mut rig = Rig::new();
        let framed = rig.distance();
        rig.viewer.zoom(5);
        rig.settle();
        assert!(rig.distance() < framed);

        rig.viewer.recenter();
        assert!(rig.camera(Camera::is_moving), "a glide is under way");
        assert!(rig.distance() < framed, "and has not arrived");
        rig.settle();
        assert!((rig.distance() - framed).abs() < 1e-6);

        rig.viewer.zoom(5);
        rig.settle();
        rig.viewer.set_reduced_motion(true);
        rig.viewer.recenter();
        assert!(!rig.camera(Camera::is_moving), "it jumped");
        assert!((rig.distance() - framed).abs() < 1e-6, "and has arrived");
        // Back to gliding when the setting is turned off again.
        rig.viewer.set_reduced_motion(false);
        rig.viewer.zoom(5);
        rig.settle();
        rig.viewer.recenter();
        assert!(rig.camera(Camera::is_moving));
    }

    #[test]
    fn clearing_the_model_clears_the_animation() {
        let mut rig = sliding();
        rig.viewer.clear();
        assert_eq!(rig.viewer.playback(), None);
    }

    // How the viewport is drawing: the software renderer, the GPU, restarts.

    fn drawn(rig: &Rig) -> Primitive {
        let canvas = Canvas {
            viewer: &rig.viewer,
            theme: ViewerTheme::dark(),
        };
        shader::Program::draw(&canvas, &rig.state, mouse::Cursor::Unavailable, BOUNDS)
    }

    #[test]
    fn a_renderer_that_never_prepares_is_found_out_to_be_software_and_the_app_is_told() {
        let mut rig = Rig::new();
        assert_eq!(rig.viewer.rendering(), Rendering::Starting);
        // iced's software renderer takes the primitive and drops it. Frames
        // keep being asked for until there is a verdict.
        let mut told = None;
        for _ in 0..6 {
            let _ = drawn(&rig);
            let action = rig.frame(16);
            if let Some((Some(message), _, _)) = action.map(Action::into_inner) {
                told = Some(message);
                break;
            }
        }
        assert_eq!(told, Some(Event::RenderingChanged));
        assert_eq!(rig.viewer.rendering(), Rendering::Software);
        assert!(rig.viewer.rendering().is_problem());
        // And that is the last it asks of the window.
        assert!(rig.frame(16).is_none());
    }

    #[test]
    fn a_restart_forgets_a_verdict_and_tells_the_app_so() {
        let mut rig = Rig::new();
        for _ in 0..6 {
            let _ = drawn(&rig);
            rig.frame(16);
        }
        assert_eq!(rig.viewer.rendering(), Rendering::Software);
        rig.viewer.restart();
        assert_eq!(rig.viewer.rendering(), Rendering::Starting);
        let told = rig.frame(16).map(Action::into_inner);
        assert!(matches!(told, Some((Some(Event::RenderingChanged), _, _))));
    }

    // The window's own path, with a device of the machine's: skipped where
    // there is none. The statics these touch (restarts, a lost device) are
    // shared by every test of the process, so they run one at a time.

    static SERIAL: Mutex<()> = Mutex::new(());

    /// A target to render the window's frame into, and its size.
    const FRAME: Size<u32> = Size::new(256, 192);
    const FRAME_AT: Rectangle = Rectangle {
        x: 0.0,
        y: 0.0,
        width: 256.0,
        height: 192.0,
    };

    struct Window {
        device: wgpu::Device,
        queue: wgpu::Queue,
        pipeline: WindowPipeline,
        target: wgpu::Texture,
    }

    impl Window {
        fn open() -> Option<Window> {
            let (device, queue) = offscreen::test_device()?;
            let format = wgpu::TextureFormat::Rgba8UnormSrgb;
            let pipeline = <WindowPipeline as shader::Pipeline>::new(&device, &queue, format);
            let target = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("test window"),
                size: wgpu::Extent3d {
                    width: FRAME.width,
                    height: FRAME.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            Some(Window {
                device,
                queue,
                pipeline,
                target,
            })
        }

        /// One frame of the window: `prepare`, then `render`, then the pixels.
        fn frame(&mut self, primitive: &Primitive) -> Vec<u8> {
            use shader::Primitive as _;
            let viewport = Viewport::with_physical_size(FRAME, 1.0);
            primitive.prepare(
                &mut self.pipeline,
                &self.device,
                &self.queue,
                &FRAME_AT,
                &viewport,
            );
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            let view = self
                .target
                .create_view(&wgpu::TextureViewDescriptor::default());
            {
                // Cleared first, as iced's frame is before the widget's turn.
                let _ = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("test clear"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
            }
            primitive.render(
                &self.pipeline,
                &mut encoder,
                &view,
                &Rectangle {
                    x: 0,
                    y: 0,
                    width: FRAME.width,
                    height: FRAME.height,
                },
            );
            // 256 pixels of 4 bytes is a row the copy takes as it is.
            let row = FRAME.width * 4;
            let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("test readback"),
                size: u64::from(row * FRAME.height),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.target,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(row),
                        rows_per_image: Some(FRAME.height),
                    },
                },
                wgpu::Extent3d {
                    width: FRAME.width,
                    height: FRAME.height,
                    depth_or_array_layers: 1,
                },
            );
            self.queue.submit([encoder.finish()]);
            readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            self.device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("the device answers");
            let pixels = readback.slice(..).get_mapped_range().to_vec();
            readback.unmap();
            pixels
        }
    }

    /// Pixels that are not the background the viewer clears to (the corner's):
    /// where the model is. A frame the viewer drew nothing into is the
    /// window's black all over, which has none.
    fn lit(pixels: &[u8]) -> usize {
        let background = [pixels[0], pixels[1], pixels[2]];
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| (0..3).any(|c| p[c].abs_diff(background[c]) > 8))
            .count()
    }

    /// Pixels that differ between two frames of the same size.
    fn differing(a: &[u8], b: &[u8]) -> usize {
        a.as_chunks::<4>()
            .0
            .iter()
            .zip(b.as_chunks::<4>().0)
            .filter(|(a, b)| a.iter().zip(*b).any(|(a, b)| a.abs_diff(*b) > 8))
            .count()
    }

    /// A rig laid out for the test window's size.
    fn rig_in_window(model: Arc<Model>) -> Rig {
        let mut rig = Rig::with(model);
        rig.viewer.set_display(DisplayOptions {
            grid: false,
            ..DisplayOptions::default()
        });
        // Frames the model for the window's shape.
        let canvas = Canvas {
            viewer: &rig.viewer,
            theme: ViewerTheme::dark(),
        };
        let _ = shader::Program::draw(&canvas, &rig.state, mouse::Cursor::Unavailable, FRAME_AT);
        rig.frame(0);
        rig
    }

    fn drawn_in_window(rig: &Rig) -> Primitive {
        let canvas = Canvas {
            viewer: &rig.viewer,
            theme: ViewerTheme::dark(),
        };
        shader::Program::draw(&canvas, &rig.state, mouse::Cursor::Unavailable, FRAME_AT)
    }

    #[test]
    fn the_window_draws_a_skinned_box_and_it_grows_with_the_clip() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let Some(mut window) = Window::open() else {
            return;
        };
        let rig = rig_in_window(glb("box.glb", samples::skinned_glb()));
        rig.viewer.set_playing(false);

        rig.viewer.seek(0.0);
        let rest = window.frame(&drawn_in_window(&rig));
        assert_eq!(rig.viewer.rendering(), Rendering::Drawing);
        assert!(lit(&rest) > 200, "the box is there");

        rig.viewer.seek(1.0);
        let stretched = window.frame(&drawn_in_window(&rig));
        // Three times as tall, with the camera where it was (it framed the
        // box at rest, which already filled most of the window): it covers
        // more of the picture, and not the same part.
        assert!(
            lit(&stretched) > lit(&rest) + 1000,
            "{} against {}",
            lit(&stretched),
            lit(&rest)
        );
        assert!(differing(&rest, &stretched) > 1000);

        // Back at the start it is the picture it was.
        rig.viewer.seek(0.0);
        let again = window.frame(&drawn_in_window(&rig));
        assert_eq!(differing(&rest, &again), 0);
    }

    #[test]
    fn the_window_moves_a_node_and_a_morphing_mesh_too() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let Some(mut window) = Window::open() else {
            return;
        };
        let slide = rig_in_window(glb("slide.glb", samples::animated_cube_glb()));
        slide.viewer.set_playing(false);
        slide.viewer.seek(0.0);
        let before = window.frame(&drawn_in_window(&slide));
        slide.viewer.seek(1.0);
        let after = window.frame(&drawn_in_window(&slide));
        assert!(
            differing(&before, &after) > 500,
            "the cube is somewhere else"
        );

        let grow = rig_in_window(glb("grow.glb", samples::morphing_glb()));
        grow.viewer.set_playing(false);
        grow.viewer.seek(0.0);
        let small = window.frame(&drawn_in_window(&grow));
        grow.viewer.seek(1.0);
        let tall = window.frame(&drawn_in_window(&grow));
        assert!(lit(&tall) > lit(&small) + 1000, "it grew");
        assert!(differing(&small, &tall) > 1000);
    }

    #[test]
    fn the_window_paints_the_wireframe_and_the_normals_of_a_bent_mesh() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let Some(mut window) = Window::open() else {
            return;
        };
        let mut rig = rig_in_window(glb("box.glb", samples::skinned_glb()));
        rig.viewer.set_playing(false);
        rig.viewer.set_display(DisplayOptions {
            grid: false,
            wireframe: true,
            normals: true,
            axes: false,
        });
        rig.viewer.seek(0.0);
        let rest = window.frame(&drawn_in_window(&rig));
        rig.viewer.seek(1.0);
        let stretched = window.frame(&drawn_in_window(&rig));
        // No error from the device, and the lines moved with the box.
        assert_eq!(rig.viewer.rendering(), Rendering::Drawing);
        assert!(differing(&rest, &stretched) > 500);
    }

    #[test]
    fn a_window_whose_gpu_could_not_be_set_up_says_so_and_a_restart_brings_it_back() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let Some(mut window) = Window::open() else {
            return;
        };
        let rig = rig_in_window(glb("box.glb", samples::skinned_glb()));
        window.pipeline.gpu = None;
        window.pipeline.failure = Some("no memory".into());
        let blank = window.frame(&drawn_in_window(&rig));
        assert_eq!(lit(&blank), 0, "nothing is drawn");
        assert_eq!(
            rig.viewer.rendering(),
            Rendering::Unavailable("no memory".into())
        );
        assert!(rig.viewer.health.review().changed, "the app is told");

        rig.viewer.restart();
        let back = window.frame(&drawn_in_window(&rig));
        assert!(lit(&back) > 200, "drawn again");
        assert_eq!(rig.viewer.rendering(), Rendering::Drawing);
    }

    #[test]
    fn a_lost_device_is_reported_and_a_restart_clears_it() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let Some(mut window) = Window::open() else {
            return;
        };
        let rig = rig_in_window(glb("box.glb", samples::skinned_glb()));
        *DEVICE_LOST.lock().unwrap() = Some("Unknown: the card was reset".into());
        let blank = window.frame(&drawn_in_window(&rig));
        assert_eq!(lit(&blank), 0);
        assert_eq!(
            rig.viewer.rendering(),
            Rendering::Lost("Unknown: the card was reset".into())
        );
        rig.viewer.restart();
        assert!(device_lost().is_none());
        let back = window.frame(&drawn_in_window(&rig));
        assert!(lit(&back) > 200);
        assert_eq!(rig.viewer.rendering(), Rendering::Drawing);
    }

    #[test]
    fn a_model_the_gpu_refuses_is_reported_once_and_the_next_model_starts_afresh() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let Some(mut window) = Window::open() else {
            return;
        };
        let rig = rig_in_window(glb("box.glb", samples::skinned_glb()));
        let _ = window.frame(&drawn_in_window(&rig));
        assert_eq!(rig.viewer.rendering(), Rendering::Drawing);

        // The device refuses what the next frame uploads.
        window.pipeline.gpu.as_mut().unwrap().refuse_next = true;
        let other = rig_in_window(glb("another.glb", samples::morphing_glb()));
        let blank = window.frame(&drawn_in_window(&other));
        assert_eq!(lit(&blank), 0, "nothing is drawn for it");
        let Rendering::Failed(reason) = other.viewer.rendering() else {
            panic!("the device refused: {:?}", other.viewer.rendering());
        };
        assert!(!reason.is_empty());
        assert!(other.viewer.health.review().changed, "the app is told");

        // Not tried again every frame: the next one draws nothing either.
        let still = window.frame(&drawn_in_window(&other));
        assert_eq!(lit(&still), 0);
        assert!(matches!(other.viewer.rendering(), Rendering::Failed(_)));
        // The first viewer was never touched.
        let first = window.frame(&drawn_in_window(&rig));
        assert!(lit(&first) > 200);

        // A model that draws clears it.
        let mut other = other;
        other
            .viewer
            .set_model(glb("box.glb", samples::skinned_glb()));
        let back = window.frame(&drawn_in_window(&other));
        assert!(lit(&back) > 200);
        assert_eq!(other.viewer.rendering(), Rendering::Drawing);
    }
}
