//! A kit's native tab, from its `.wasm` to the window: loaded in wasmtime with
//! only the calls its manifest declares, and drawn with Hangar's widgets.
//!
//! docs/native/kits-native.md is the contract this crate keeps. The app:
//!
//! 1. makes one [`KitRuntime`] for its life, and [`KitRuntime::compile`]s a
//!    tab's module once per file it reads;
//! 2. for each open tab, implements [`KitApi`] (what the tab may call, and
//!    the answers: hangar-core's kit functions, the panes, the window) and
//!    calls [`KitTab::start`];
//! 3. draws it with [`KitTab::draw`], hands back what the drawing sends with
//!    [`KitTab::update`], runs the [`Effects`] it answers (their
//!    [`Effects::task`], in iced), subscribes to [`KitTab::subscription`],
//!    and passes Hangar's events on ([`KitTab::event`], `set_visible`,
//!    `set_theme`, `set_params`).
//!
//! This crate does not depend on hangar-core: the app is what knows both.

mod api;
mod gate;
mod message;
mod render;
mod runtime;
mod tab;

pub use api::{CallFuture, HostCall, KitApi, LoopbackCall, ready};
pub use message::{KitMessage, Local};
pub use render::{KitStyle, Palette};
pub use runtime::{KitModule, KitRuntime, Limits, LoadError};
pub use tab::{Dispatch, Effects, Failure, FailureKind, KitTab, LogEntry, TabStart};

pub use hangar_kit_ui as ui;
