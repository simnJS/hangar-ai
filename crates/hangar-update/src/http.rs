//! The two requests an update makes: the feed, and the artifact it points at.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::error::{Error, Result};

/// A feed is a few kilobytes; one much larger is not a feed.
const MAX_FEED_BYTES: u64 = 4 * 1024 * 1024;

/// The longest a download may take. `ureq` times a whole body rather than each
/// read, so this is a ceiling for a connection that stalls (a cancelled
/// download cannot interrupt a blocked read) and not a speed requirement: an
/// installer of a few hundred megabytes fits at well under 1 Mbit/s.
const MAX_DOWNLOAD: Duration = Duration::from_secs(60 * 60);

/// The largest artifact accepted. Hangar's builds are a few hundred megabytes
/// at most; this keeps a hostile or broken server from filling the disk, and
/// bounds what a legacy signature makes [`crate::verify_file`] read into memory.
const MAX_DOWNLOAD_BYTES: u64 = 1024 * 1024 * 1024;

/// `url` is plain HTTP to this machine, which only a test serves.
fn is_loopback_http(url: &str) -> bool {
    !url.to_ascii_lowercase().starts_with("https://") && require_secure(url).is_ok()
}

fn agent(url: &str, user_agent: &str, global_timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(user_agent)
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_global(Some(global_timeout))
        // GitHub answers a release download with a redirect to its CDN. Nothing
        // sent to the first host (no credentials are sent at all) is meant for
        // the second, and the default limit of ten hops is plenty. Every hop
        // must be HTTPS, as the first one is: a redirect to plain HTTP would
        // let anyone on the network answer "no update". A test's loopback
        // server is the one exception, as in `require_secure`.
        .https_only(!is_loopback_http(url))
        .build()
        .into()
}

/// Whether `url` may be fetched: HTTPS, or plain HTTP to this machine so a
/// test can serve a feed without a certificate. The plugin required HTTPS the
/// same way; the signature would catch a tampered artifact either way, but a
/// feed fetched in the clear can still be made to say "no update".
pub(crate) fn require_secure(url: &str) -> Result<()> {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("https://") {
        return Ok(());
    }
    if let Some(rest) = lower.strip_prefix("http://") {
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        let host = match host.strip_prefix('[') {
            Some(v6) => v6.split(']').next().unwrap_or_default(),
            None => host.split(':').next().unwrap_or_default(),
        };
        if matches!(host, "localhost" | "127.0.0.1" | "::1") {
            return Ok(());
        }
    }
    Err(Error::Network(format!(
        "refusing to fetch {url}: it is not an https address"
    )))
}

fn network(error: ureq::Error) -> Error {
    Error::Network(error.to_string())
}

/// Fetches the feed. `None` is the "no content" answer a dynamic update server
/// gives when there is nothing newer for the caller.
pub(crate) fn fetch_feed(url: &str, user_agent: &str) -> Result<Option<String>> {
    require_secure(url)?;
    let mut response = agent(url, user_agent, Duration::from_secs(30))
        .get(url)
        .header("Accept", "application/json")
        .call()
        .map_err(network)?;
    if response.status().as_u16() == 204 {
        return Ok(None);
    }
    response
        .body_mut()
        .with_config()
        .limit(MAX_FEED_BYTES)
        .read_to_string()
        .map(Some)
        .map_err(network)
}

/// Streams `url` into `out`, reporting `(bytes so far, total if announced)`
/// after every chunk. Stops with [`Error::Cancelled`] as soon as `cancel` is
/// set, which is checked between chunks.
pub(crate) fn download(
    url: &str,
    user_agent: &str,
    out: &mut impl Write,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<u64> {
    require_secure(url)?;
    let mut response = agent(url, user_agent, MAX_DOWNLOAD)
        .get(url)
        .header("Accept", "application/octet-stream")
        .call()
        .map_err(network)?;

    let total = response.body().content_length();
    if total.is_some_and(|total| total > MAX_DOWNLOAD_BYTES) {
        return Err(too_large());
    }
    progress(0, total);

    let mut reader = response.body_mut().as_reader();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut received = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let read = reader
            .read(&mut buffer)
            .map_err(|e| Error::Network(format!("the download was interrupted: {e}")))?;
        if read == 0 {
            break;
        }
        if received + read as u64 > MAX_DOWNLOAD_BYTES {
            return Err(too_large());
        }
        out.write_all(&buffer[..read])
            .map_err(|e| Error::io("writing the download", e))?;
        received += read as u64;
        progress(received, total);
    }

    match total {
        Some(total) if total != received => Err(Error::Network(format!(
            "the download stopped after {received} of {total} bytes"
        ))),
        _ => Ok(received),
    }
}

fn too_large() -> Error {
    Error::Network(format!(
        "the download is larger than {} MiB, which no release is",
        MAX_DOWNLOAD_BYTES / 1024 / 1024
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_or_this_machine() {
        for ok in [
            "https://github.com/hangar-ai/hangar/releases/latest/download/latest.json",
            "HTTPS://example.com/x",
            "http://localhost:8080/latest.json",
            "http://127.0.0.1:1234/x",
            "http://[::1]:9/x",
        ] {
            require_secure(ok).unwrap_or_else(|e| panic!("{ok} should pass: {e}"));
        }
        for bad in [
            "http://github.com/x",
            "http://example.com/x",
            // A host that merely starts like a local one.
            "http://localhost.evil.example/x",
            "http://127.0.0.1.evil.example/x",
            "http://localhost@evil.example/x",
            "ftp://example.com/x",
            "file:///etc/passwd",
            "latest.json",
        ] {
            assert!(require_secure(bad).is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn only_a_loopback_test_server_may_be_reached_without_https() {
        // Every hop of a real request, redirects included, must be HTTPS.
        assert!(!is_loopback_http("https://github.com/x"));
        assert!(!is_loopback_http("HTTPS://localhost/x"));
        assert!(!is_loopback_http("http://example.com/x"));
        assert!(is_loopback_http("http://127.0.0.1:21240/latest.json"));
        assert!(
            agent("https://github.com/x", "t", Duration::from_secs(1))
                .config()
                .https_only()
        );
        assert!(
            !agent("http://localhost:1/x", "t", Duration::from_secs(1))
                .config()
                .https_only()
        );
    }
}
