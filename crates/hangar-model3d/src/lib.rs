//! The 3D model preview: loaders for the formats the old three.js viewer
//! opened, the arithmetic that frames a model, a wgpu renderer, and the iced
//! widget that shows it.
//!
//! See docs/native/PLAN.md. The old viewer is `src/lib/model3d/` (the engine,
//! loaders, framing, grid and statistics) and `src/components/ModelPanel.tsx`;
//! the backend side (which folders may be read, saving a capture) stays in
//! hangar-core's `model.rs`, so this crate takes a path and trusts the app to
//! have checked it.
//!
//! Formats: glTF and GLB (the one agents are told to export; Draco, meshopt
//! and KTX2 are not decoded), OBJ with its `.mtl`, STL (binary and ASCII), PLY
//! (ASCII and both binary byte orders, with vertex colours, and point clouds),
//! and binary FBX 7.x. USDZ, the old viewer's last format, is not read yet:
//! asking for one says so, and what to export instead.
//!
//! The pieces, in the order a model goes through them:
//!
//! - [`load`] reads a file into a [`Model`] on a thread of its own;
//! - [`ModelViewer`] holds what one pane shows — the model, the camera, the
//!   display toggles — and [`ModelViewer::view`] is the widget that draws it
//!   and takes the mouse;
//! - [`CaptureJob`] takes a picture of a model with no window: an agent's
//!   snapshot, or the capture button.
//!
//! In an app, a preview pane holds a [`ModelViewer`]:
//!
//! ```ignore
//! // Opening a file: the task's events are folded into the viewer.
//! let (task, cancel) = hangar_model3d::load_task(path, LoadOptions { roots: Some(allowed) });
//! task.map(Message::ModelLoad)
//!
//! // update
//! Message::ModelLoad(event) => self.viewer.apply(event),
//! Message::Wireframe(on) => self.viewer.set_display(DisplayOptions { wireframe: on, ..self.viewer.display() }),
//! Message::Recenter => self.viewer.recenter(),
//! Message::Capture => {
//!     if let Some(job) = self.viewer.capture(2.0, theme) {
//!         return Task::perform(job.run_async(), Message::Captured);   // PNG bytes
//!     }
//! }
//!
//! // view
//! self.viewer.view(theme).map(Message::Model)
//!
//! // an agent's screenshot, of any loaded model, window or not
//! CaptureJob::snapshot(model, SnapshotAngle::Iso, 1024, 768, theme).run_async()
//! ```
//!
//! The viewport draws only on iced's wgpu renderer. On its software fallback
//! (tiny-skia) it stays empty — captures still work, they bring their own
//! device — and a GPU error inside the widget is reported on stderr instead of
//! taking the window down.

pub mod camera;
pub mod formats;
pub mod framing;
pub mod load;
pub mod model;
pub(crate) mod render;
pub mod samples;
pub mod stats;
pub mod theme;
pub mod viewer;

pub use formats::{LoadError, MAX_MODEL_BYTES, ModelFormat};
pub use framing::{Bounds, SnapshotAngle};
pub use load::{Cancel, LoadEvent, LoadOptions, LoadProgress, Phase};
pub use model::Model;
pub use render::DisplayOptions;
pub use render::offscreen::CaptureError;
pub use stats::ModelInfo;
pub use theme::ViewerTheme;
pub use viewer::{
    CaptureJob, Event, LoadState, ModelViewer, Stamp, capture_name, describe, load_task,
    snapshot_png,
};
