//! The 3D model preview: loaders for the formats the old three.js viewer
//! opened, the arithmetic that frames a model, a wgpu renderer, and the iced
//! widget that shows it.
//!
//! See docs/native/PLAN.md. The old viewer is `src/lib/model3d/` (the engine,
//! loaders, framing, grid and statistics) and `src/components/ModelPanel.tsx`;
//! the backend side (which folders may be read, saving a capture) stays in
//! hangar-core's `model.rs`, so this crate takes a path and trusts the app to
//! have checked it.

pub mod formats;
pub mod framing;
pub mod model;
pub mod stats;
pub mod theme;

pub use formats::{LoadError, MAX_MODEL_BYTES, ModelFormat};
pub use framing::{Bounds, SnapshotAngle};
pub use model::Model;
pub use stats::ModelInfo;
pub use theme::ViewerTheme;
