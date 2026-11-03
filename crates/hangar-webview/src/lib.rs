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
//! [`kit_relay`] reads what such a page posts and builds what it is told: the
//! protocol's envelope, which the app relays to the same gate a native tab's
//! calls go through.
//!
//! The helper never lets a page open a window of its own, download a file or
//! ask for a permission; it says so with [`Event::NavigationBlocked`] and the
//! parent decides.
//!
//! # Platforms
//!
//! Linux drives WebKitGTK through GTK 3 (Wayland and X11), Windows WebView2,
//! macOS WKWebView. Only Linux is tested. What differs is spelt by this crate,
//! not left to the parent:
//!
//! - a custom scheme's URLs are `hangar-kit://localhost/…` on Linux and macOS
//!   and `http://hangar-kit.localhost/…` on Windows; [`scheme_url`] builds the
//!   right one, and an `allow_navigation` prefix written the first way also
//!   matches the second. A [`SchemeRequest::uri`] has the form of its platform.
//! - the helper sets `WEBKIT_DISABLE_DMABUF_RENDERER=1` on Linux when it is
//!   unset (a blank window on Wayland with an NVIDIA GPU otherwise);
//! - a window keeps nothing on disk unless `ephemeral` is turned off.
//!
//! # PDFs
//!
//! Open one as any page: `Content::url(file_url(path))`, or serve its bytes
//! through a scheme with `Content-Type: application/pdf`. Whether it shows
//! depends on the engine having a PDF viewer:
//!
//! - **WebKitGTK 2.52.6** (checked on Linux, through wry): yes. WebKit wraps a
//!   PDF in a document whose frame is its own build of pdf.js
//!   (`webkit-pdfjs-viewer://`), with the sidebar, search, page and zoom
//!   controls, print and save; `navigator.pdfViewerEnabled` is `true`. Both a
//!   `file://` PDF and one served by a scheme render. The viewer is part of
//!   newer WebKitGTK builds, which an older or a stripped distribution build may
//!   not have.
//! - **WebView2** has Edge's viewer and **WKWebView** renders PDFs itself;
//!   neither has been run here.
//!
//! A parent that cannot count on it asks the page once it has loaded
//! (`eval_with_token(window, "navigator.pdfViewerEnabled", …)`, which
//! `examples/demo.rs --pdf` does) and falls back. The fallback is not written;
//! the choice is between two, and neither needs the helper to change:
//!
//! 1. **pdf.js, bundled** (`pdfjs-dist`, Apache-2.0, a few megabytes with its
//!    worker): the parent serves its viewer page and the PDF through a scheme of
//!    its own, so it looks the same on every engine, and text selection and
//!    search work. The cost is the bundle, and the viewer's own code to keep
//!    current.
//! 2. **A native renderer** (`hayro`, pure Rust; or `pdfium-render`, which
//!    needs a pdfium library shipped per platform) draws pages to bitmaps in
//!    the main process: no browser engine, so no helper, but no text layer
//!    either unless it is built.

mod base64;
mod client;
pub mod kit_relay;
mod protocol;
mod scheme;
mod session;

pub use client::{Client, Closed, Events, SpawnOptions, Stderr, find_helper};
pub use protocol::{
    CloseReason, Command, Content, ErrorCode, Event, Header, OpenWindow, PROTOCOL_VERSION,
    PageLoadState, SchemeRequest, SchemeResponse, Size,
};
pub use scheme::{
    Navigation, file_url, file_url_for, scheme_base, scheme_base_for, scheme_url, valid_scheme_name,
};
pub use session::opens_unfocused;

/// The script the helper installs for a window opened with `kit_bridge`, for a
/// parent that would rather pass it as an `init_script` itself.
pub const KIT_BRIDGE: &str = include_str!("kit_bridge.js");

/// Base64 as the protocol writes bodies, for a parent that has to read or write
/// one by hand.
pub mod body {
    pub use crate::base64::{DecodeError, decode, encode};
}
