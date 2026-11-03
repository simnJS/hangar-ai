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

use crate::camera::{Camera, snapshot_matrices};
use crate::formats::LoadError;
use crate::framing::{SnapshotAngle, view_direction};
use crate::load::{Cancel, LoadEvent, LoadOptions, LoadProgress, Phase};
use crate::model::Model;
use crate::render::offscreen::{self, CaptureError};
use crate::render::{CameraMatrices, DisplayOptions, Gpu, NORMALS_LIMIT, Scene, guarded};
use crate::theme::ViewerTheme;

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
}

impl ViewState {
    fn aspect(&self) -> f64 {
        if self.size.width > 0.0 && self.size.height > 0.0 {
            f64::from(self.size.width) / f64::from(self.size.height)
        } else {
            DEFAULT_ASPECT
        }
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
            })),
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
    /// inside it or lost; anything else is framed afresh.
    pub fn set_model(&mut self, model: Arc<Model>) {
        let keep = self.model.as_ref().is_some_and(|old| {
            let ratio = model.radius / old.radius;
            old.name == model.name && ratio > 0.25 && ratio < 4.0
        });
        {
            let mut view = lock(&self.view);
            match (&mut view.camera, keep) {
                (Some(camera), true) => camera.set_limits(&model),
                (camera, _) => *camera = None,
            }
        }
        self.model = Some(model);
        self.state = LoadState::Ready;
    }

    /// Takes the model off, leaving the stage empty.
    pub fn clear(&mut self) {
        self.model = None;
        self.state = LoadState::Empty;
        lock(&self.view).camera = None;
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
    pub fn set_reduced_motion(&mut self, reduced: bool) {
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
        Some(CaptureJob::from_camera(
            model,
            camera,
            self.display,
            size.0,
            size.1,
            theme,
        ))
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
        }
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
                if moving {
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
        let model = self.viewer.model.clone();
        let camera = {
            let mut view = lock(&self.viewer.view);
            view.sync(model.as_deref(), bounds.size());
            let aspect = f64::from(bounds.width.max(1.0)) / f64::from(bounds.height.max(1.0));
            model
                .as_ref()
                .zip(view.camera.as_ref())
                .map(|(model, camera)| camera.matrices(aspect, model.radius))
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
}

/// What iced keeps for every viewer of the window: the renderer, and the pass
/// that copies its pictures in.
struct WindowPipeline {
    /// `None` when the device refused the pipelines: the viewport stays empty
    /// rather than the application going down with it.
    gpu: Option<Gpu>,
    blit: Blit,
}

impl shader::Pipeline for WindowPipeline {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let gpu = guarded(device, || Gpu::new(device, queue))
            .inspect_err(|error| eprintln!("hangar-model3d: the 3D preview cannot draw: {error}"))
            .ok();
        WindowPipeline {
            gpu,
            blit: Blit::new(device, format),
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
        };
        let Some(gpu) = &mut pipeline.gpu else { return };
        if let Err(error) = guarded(device, || gpu.prepare(self.viewer, size, &scene)) {
            eprintln!("hangar-model3d: the model could not be drawn: {error}");
            // Nothing is drawn for this viewer until the next model: its
            // target is gone, and so is the copy of the model that failed.
            gpu.release(self.viewer);
            if let Some(model) = &self.model {
                gpu.forget(model.id);
            }
            pipeline.blit.targets.remove(&self.viewer);
            return;
        }
        if let Some(picture) = gpu.picture(self.viewer) {
            let origin = [
                (bounds.x * scale).round() as i32,
                (bounds.y * scale).round() as i32,
            ];
            pipeline
                .blit
                .prepare(device, queue, self.viewer, picture, origin);
        }
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
            let mut viewer = ModelViewer::new();
            viewer.set_model(cube("cube.glb"));
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
}
