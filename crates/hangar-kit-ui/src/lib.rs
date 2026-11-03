//! What a kit's native tab is made of: a tree of widgets it describes, and the
//! events Hangar sends back when someone clicks, types or picks. Shared, as
//! data, by the kit (hangar-kit-sdk, in WebAssembly) and by Hangar
//! (hangar-kit-host), and so the public contract of native tabs:
//! docs/native/kits-native.md is its prose.
//!
//! - [`Element`] and its widgets: the page, as data. Hangar draws it with its
//!   own widgets, in the person's theme: a kit never names a colour.
//! - [`UiEvent`]: a widget used, naming the handler the kit gave it.
//! - [`abi`]: the envelope between Hangar and the module, what crosses and
//!   who owns the bytes.
//! - [`api`]: the host API, the methods of HTML tabs' protocol with the same
//!   names, params, results and permissions.
//!
//! No iced here, nor anything that does not build for `wasm32-unknown-unknown`.
//! Everything is serde, as JSON: a field added later with a default, a widget
//! or a method added, is ignored by an older side, so a kit and a Hangar of
//! different ages still understand each other. What would break them changes
//! [`abi::ABI_VERSION`].

pub mod abi;
pub mod api;
mod element;
mod event;
mod style;

pub use element::*;
pub use event::*;
pub use style::*;

#[cfg(test)]
mod tests;
