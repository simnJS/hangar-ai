//! Hangar's webview helper, seen from the app: the messages it speaks and a
//! client to speak them.
//!
//! The main process of Hangar draws everything itself and runs no browser
//! engine. What needs one (a kit's tab written in HTML, a web preview, a PDF)
//! is shown by `hangar-webview`, a process of its own whose windows are
//! separate from the app's. This crate is both ends of the line between them.
//! The helper itself (`src/main.rs`, behind the `engine` feature, which pulls
//! in wry, tao and on Linux GTK and WebKitGTK) is not part of the library, so
//! an app that depends on the library compiles none of that.
//!
//! ```no_run
//! use hangar_webview::{Client, Content, Event, OpenWindow};
//!
//! let helper = hangar_webview::find_helper().expect("hangar-webview is installed");
//! let (client, events) = Client::spawn(helper)?;
//! client.open(OpenWindow::new("docs", Content::url("https://docs.rs")).title("docs.rs"))?;
//! while let Some(event) = events.recv() {
//!     match event {
//!         Event::Ipc { window, body, .. } => println!("{window} says {body}"),
//!         Event::WindowClosed { .. } => break,
//!         _ => {}
//!     }
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # The protocol
//!
//! JSON lines on the helper's standard input (commands) and output (events),
//! each message an object tagged by `"type"`; see [`Command`] and [`Event`].
//! The helper announces itself with [`Event::Ready`], whose `protocol` is
//! [`PROTOCOL_VERSION`]; the client refuses any other.
//!
//! A page is served by the parent, not by the helper. A window opens with
//! custom [`schemes`](OpenWindow::schemes), and each request the page makes to
//! one comes back as an [`Event::SchemeRequest`] that the parent answers with a
//! [`SchemeResponse`]. What a kit's page may load is therefore decided where
//! the kit's isolation is enforced (`kit_scheme.rs` in hangar-core), and the
//! helper holds no rule of its own about it. It only forwards: the page's
//! `window.ipc.postMessage` strings as [`Event::Ipc`], and the parent's
//! `post_message` into the page.
//!
//! Kit pages find `window.hangar` working through [`KIT_BRIDGE`], which the
//! helper installs for a window opened with [`kit_bridge`](OpenWindow::kit_bridge):
//! the page's `window.parent` becomes a stand-in that speaks over `ipc`, so
//! `/_hangar/kit.js` and the kits written for the frame run as they are.
//!
//! The helper never lets a page open a window of its own, download a file or
//! ask for a permission; it says so with [`Event::NavigationBlocked`] and the
//! parent decides.

mod base64;
mod client;
mod protocol;
mod scheme;

pub use client::{Client, Closed, Events, SpawnOptions, Stderr, find_helper};
pub use protocol::{
    CloseReason, Command, Content, ErrorCode, Event, Header, OpenWindow, PROTOCOL_VERSION, PageLoadState,
    SchemeRequest, SchemeResponse, Size,
};
pub use scheme::{Navigation, scheme_base, scheme_base_for, scheme_url, valid_scheme_name};

/// The script the helper installs for a window opened with `kit_bridge`, for a
/// parent that would rather pass it as an `init_script` itself.
pub const KIT_BRIDGE: &str = include_str!("kit_bridge.js");

/// Base64 as the protocol writes bodies, for a parent that has to read or write
/// one by hand.
pub mod body {
    pub use crate::base64::{DecodeError, decode, encode};
}
