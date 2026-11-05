//! Hangar's self-updater: what `tauri-plugin-updater` was to the old app, with
//! no GUI and no Tauri behind it.
//!
//! See docs/native/PLAN.md, "Updater". The window owns the banner; this crate
//! owns everything the banner is about.
//!
//! # The contract with the release
//!
//! Releases are published by `.github/workflows/release.yml`, and an app
//! installed from any of them updates through the same two things:
//!
//! - **`latest.json`**, at [`ENDPOINT`] (the newest release's asset of that
//!   name): `version`, `notes`, `pub_date`, and a `platforms` map from a key
//!   such as `linux-x86_64-appimage` to the `url` of that build and its
//!   `signature`. The key is `{os}-{arch}` with an optional `-{installer}`;
//!   [`Target::keys`] has the rules, which are `tauri-plugin-updater`'s, so
//!   an install of the old Tauri app finds its entry here the same way.
//! - **A minisign signature** of every artifact, made by `tauri signer` (or
//!   `minisign`) with the private key whose public half is [`PUBKEY`]. The
//!   `signature` in the feed is the `.sig` file's text, base64-encoded. An
//!   artifact whose signature does not verify is deleted unread: the feed is
//!   plain JSON over HTTPS from a place anyone with a GitHub account can
//!   comment on, and the signature is the only thing that makes it Hangar's.
//!
//! What is downloaded and how it is put in place depends on how the running
//! copy was installed ([`Install`]):
//!
//! | Install | Feed key | Artifact | Put in place by |
//! | --- | --- | --- | --- |
//! | AppImage | `linux-x86_64-appimage`, then `linux-x86_64` | the `.AppImage` itself (or a `.tar.gz` holding one) | renaming it over `$APPIMAGE` |
//! | `.deb` | `linux-x86_64-deb`, then `linux-x86_64` | the `.deb` | `pkexec` running `dpkg -i` on a root-owned copy checked against the verified bytes |
//! | `.rpm` | `linux-x86_64-rpm`, then `linux-x86_64` | the `.rpm` | `pkexec` running `rpm -U` the same way |
//! | macOS | `darwin-{arch}-app`, then `darwin-{arch}` | `.app.tar.gz` | swapping the bundle (as root, from a re-checked copy, when the folder is not the person's) |
//! | Windows (NSIS) | `windows-x86_64-nsis`, then `windows-x86_64` | the `-setup.exe` | running it with `/P /R /UPDATE /ARGS …` |
//! | Windows (MSI) | `windows-x86_64-msi`, then `windows-x86_64` | the `.msi` | `msiexec /i … /passive` |
//!
//! A copy that is none of these (a binary started from `target/`, unpacked by
//! hand) does not update, and [`Updater::check`] says "nothing to do" without
//! asking the network anything.
//!
//! # Driving it
//!
//! [`Updater`] is the whole API. It blocks, so it runs on a thread of its own,
//! and it keeps a [`State`] a banner can be drawn from at any moment:
//!
//! ```text
//! Idle ─check→ Checking ─→ Available ─download→ Downloading ─→ Ready ─install→ Installing ─→ Installed
//!   ↑                │          │ ↑                  │              │              │
//!   └─ up to date ───┘   dismiss┘ └── cancelled ─────┤              └──────────────┴─ Error{Install}
//!                       Error{Check}                 └─ Error{Download} (download again to retry)
//! ```
//!
//! The old banner checked once, when the main window opened, and stayed
//! silent when that failed; `Error { stage: Check, .. }` is there for the
//! window to ignore the same way. Nothing here checks on a timer or reads a
//! setting: when to ask, and whether to, is the window's.

mod error;
mod http;
mod install;
mod manifest;
mod platform;
mod sha256;
mod signature;
mod updater;

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod testserver;

pub use error::{Error, Result, Stage};
pub use install::InstallOutcome;
pub use manifest::{Manifest, Platform};
pub use platform::{Install, Target};
pub use signature::{verify_bytes, verify_file};
pub use updater::{Config, ReleaseInfo, State, Updater};

/// The feed, which is a file of the newest published release: GitHub redirects
/// `/releases/latest/download/` to the latest release that is not a draft or a
/// pre-release. It is `plugins.updater.endpoints` of the old `tauri.conf.json`.
pub const ENDPOINT: &str =
    "https://github.com/hangar-ai/hangar/releases/latest/download/latest.json";

/// The public half of the key every release is signed with, base64-encoded as
/// `plugins.updater.pubkey` of the old `tauri.conf.json` held it. Changing it
/// strands every install that has the old one: they can no longer verify what
/// the new key signs.
pub const PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IERFN0M0MjlEQTA4OUM1MwpSV1JUbkFqYUtjVG5EUldHd0F6aGRWSmdpWWRWcXVOanhqUG5Tak5NZVhpdXRKTzZZRmkxQXl3RQo=";
