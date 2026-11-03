//! What can go wrong, and at which step.

use std::fmt;
use std::io;

/// The step of an update an error came from. The window treats them
/// differently: a failed check is silent (no network, or a build that has no
/// feed, must never reach the person), while a failed download or install is
/// shown with a way to try again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Check,
    Download,
    Install,
}

#[derive(Debug)]
pub enum Error {
    /// The running binary is not the platform's update target (see
    /// [`crate::platform::Install`]).
    Unsupported(String),
    /// The feed or an artifact could not be fetched.
    Network(String),
    /// `latest.json` is not a release feed this crate understands.
    Manifest(String),
    /// The feed carries no entry for this platform; the keys tried are listed.
    NoPlatformEntry(Vec<String>),
    /// The download does not match its signature, or the signature or key
    /// cannot be read. Nothing from such a download is ever installed.
    Signature(String),
    /// The person (or the app shutting down) asked to stop.
    Cancelled,
    Io {
        context: String,
        source: io::Error,
    },
    /// The artifact was fine but putting it in place failed.
    Install(String),
    /// A call made at a moment the updater cannot honour it, e.g. installing
    /// before anything was downloaded.
    State(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub(crate) fn io(context: impl Into<String>, source: io::Error) -> Self {
        Error::Io {
            context: context.into(),
            source,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unsupported(why) => write!(f, "this install cannot update itself: {why}"),
            Error::Network(why) => write!(f, "network error: {why}"),
            Error::Manifest(why) => write!(f, "invalid release feed: {why}"),
            Error::NoPlatformEntry(keys) => {
                write!(
                    f,
                    "the release has no build for this platform (tried {})",
                    keys.join(", ")
                )
            }
            Error::Signature(why) => write!(f, "signature check failed: {why}"),
            Error::Cancelled => f.write_str("cancelled"),
            Error::Io { context, source } => write!(f, "{context}: {source}"),
            Error::Install(why) => write!(f, "install failed: {why}"),
            Error::State(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
