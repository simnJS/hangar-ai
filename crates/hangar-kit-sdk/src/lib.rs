//! The kit side of a native tab: build the view, answer events, call Hangar.
//!
//! A tab is a [`Kit`], an iced-like program: `init` makes its state, `update`
//! answers a message, `view` draws the state, `subscriptions` says what it
//! wants to hear. [`export_kit!`] gives it the module's exports; built for
//! `wasm32-unknown-unknown`, it is what a manifest's `entry` names.
//!
//! ```ignore
//! use hangar_kit_sdk::prelude::*;
//!
//! struct Panes { panes: Option<Result<Vec<api::Pane>, KitError>> }
//!
//! #[derive(Clone)]
//! enum Message { Refresh, Panes(Result<Vec<api::Pane>, KitError>) }
//!
//! impl Kit for Panes {
//!     type Message = Message;
//!
//!     fn init(_start: &Start, cx: &mut Context<Message>) -> Self {
//!         cx.call(api::PanesList {}, Message::Panes);
//!         Panes { panes: None }
//!     }
//!
//!     fn update(&mut self, message: Message, cx: &mut Context<Message>) {
//!         match message {
//!             Message::Refresh => cx.call(api::PanesList {}, Message::Panes),
//!             Message::Panes(panes) => self.panes = Some(panes),
//!         }
//!     }
//!
//!     fn view(&self) -> Element<Message> {
//!         page![header("Panes").action(button("Refresh").on_press(Message::Refresh))].into()
//!     }
//!
//!     fn subscriptions(&self) -> Subscriptions<Message> {
//!         Subscriptions::none().panes_changed(Message::Refresh)
//!     }
//! }
//!
//! export_kit!(Panes);
//! ```
//!
//! docs/native/kits-native.md is the contract; hangar-kit-ui, re-exported as
//! [`ui`], is its data.

mod context;
pub mod format;
mod handler;
pub mod i18n;
mod runtime;
mod subscriptions;
mod sys;
#[cfg(not(target_arch = "wasm32"))]
pub mod testing;
pub mod widgets;

#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub mod glue;

pub use hangar_kit_ui as ui;
pub use hangar_kit_ui::abi::Start;
pub use hangar_kit_ui::api::{self, KitError};

pub use context::Context;
pub use handler::Handler;
pub use runtime::Runtime;
pub use subscriptions::Subscriptions;
pub use sys::{console, log, now_ms};
pub use widgets::Element;

/// A native tab. Hangar runs one per open tab, each in its own instance:
/// nothing is shared between two tabs of the same kit but what they keep
/// in the kit's data folder.
pub trait Kit: Sized + 'static {
    /// What `update` is told. Cloned for each press of a button that names
    /// it, so keep it small.
    type Message: Clone + 'static;

    /// The state the tab starts in, and the calls it makes first.
    fn init(start: &Start, cx: &mut Context<Self::Message>) -> Self;

    /// Answers a message: a widget used, a call answered, a timer, an event.
    fn update(&mut self, message: Self::Message, cx: &mut Context<Self::Message>);

    /// The whole page, as the state is now. Hangar asks for it after every
    /// update, so keep it a function of the state alone.
    fn view(&self) -> Element<Self::Message>;

    /// The timers and events it wants to hear, all of them, after every
    /// update. None by default.
    fn subscriptions(&self) -> Subscriptions<Self::Message> {
        Subscriptions::none()
    }
}

/// Everything a kit's tab usually needs.
pub mod prelude {
    pub use crate::api::{self, KitError};
    pub use crate::format;
    pub use crate::i18n::Strings;
    pub use crate::ui::{
        Align, Axis, ButtonKind, EmptyKind, Fit, Icon, ImageSource, Ink, Length, LineKind, Mark,
        Notice, Padding, Point, ProgressKind, Range, Shape, Sort, SortKey, Status, Surface,
        TextSize, Tone, Tool, Weight, Wrap,
    };
    pub use crate::widgets::*;
    pub use crate::{
        Context, Element, Kit, Start, Subscriptions, col, console, export_kit, page, row,
    };
}

/// Gives a [`Kit`] the exports of the ABI: `export_kit!(MyTab);`, once, in
/// the crate built for `wasm32-unknown-unknown`. On another target it writes
/// nothing, so the same crate builds for the host and runs its tests there.
#[macro_export]
macro_rules! export_kit {
    ($kit:ty) => {
        #[cfg(target_arch = "wasm32")]
        const _: () = {
            ::std::thread_local! {
                static RUNTIME: ::core::cell::RefCell<$crate::Runtime<$kit>> =
                    ::core::cell::RefCell::new($crate::Runtime::new());
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn hangar_abi_version() -> u32 {
                $crate::ui::abi::ABI_VERSION
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn hangar_alloc(len: u32) -> u32 {
                $crate::glue::alloc(len)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn hangar_init(ptr: u32, len: u32) -> u64 {
                // SAFETY: Hangar passes a buffer `hangar_alloc` gave, filled.
                RUNTIME.with(|runtime| unsafe { $crate::glue::init(runtime, ptr, len) })
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn hangar_update(ptr: u32, len: u32) -> u64 {
                // SAFETY: Hangar passes a buffer `hangar_alloc` gave, filled.
                RUNTIME.with(|runtime| unsafe { $crate::glue::update(runtime, ptr, len) })
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn hangar_view() -> u64 {
                RUNTIME.with(|runtime| $crate::glue::view(runtime))
            }
        };
    };
}

#[cfg(test)]
mod tests;
