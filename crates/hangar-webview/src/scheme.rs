//! How URLs are spelt for the engine: a custom scheme on each platform, a file,
//! and how a navigation is matched against the prefixes a window may go to.
//!
//! The engines disagree about custom schemes. WebKitGTK and WKWebView load
//! `hangar-kit://localhost/a.html` as it is written, and WebView2 cannot, so
//! wry rewrites it to `http://hangar-kit.localhost/a.html` (the way
//! `kitFileUrl` in the old window did). Nothing here needs a webview, so all of
//! it can be tested on any platform: the platform is a parameter, and the
//! functions without one pass the platform they are built for.

/// The start of a scheme's URLs on a platform: `hangar-kit://localhost` where
/// the engine takes the scheme as it is, `http://hangar-kit.localhost` where it
/// does not.
pub fn scheme_base_for(scheme: &str, windows: bool) -> String {
    if windows {
        format!("http://{scheme}.localhost")
    } else {
        format!("{scheme}://localhost")
    }
}

/// [`scheme_base_for`] for the platform this is built for.
pub fn scheme_base(scheme: &str) -> String {
    scheme_base_for(scheme, cfg!(windows))
}

/// The URL of `path` under a scheme, as a window of this platform opens it and
/// as the requests for it will name it. A path without its leading slash gets
/// one.
pub fn scheme_url(scheme: &str, path: &str) -> String {
    let base = scheme_base(scheme);
    if path.starts_with('/') {
        format!("{base}{path}")
    } else {
        format!("{base}/{path}")
    }
}

/// Every spelling a prefix of a scheme's URLs can arrive in: the one given, and
/// the two forms WebView2 uses for it. A prefix of any other URL is returned as
/// it is.
fn spellings(prefix: &str, schemes: &[String]) -> Vec<String> {
    let mut all = vec![prefix.to_string()];
    for scheme in schemes {
        if let Some(rest) = prefix.strip_prefix(&format!("{scheme}://localhost")) {
            all.push(format!("http://{scheme}.localhost{rest}"));
            all.push(format!("https://{scheme}.localhost{rest}"));
        }
    }
    all
}

/// The URLs a window may navigate to, as `allow_navigation` says them:
/// prefixes, spelt as [`scheme_url`] does or as WebView2 would.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Navigation {
    prefixes: Vec<String>,
}

impl Navigation {
    /// The prefixes of a window with these `schemes`.
    pub fn new(prefixes: &[String], schemes: &[String]) -> Self {
        Self {
            prefixes: prefixes
                .iter()
                .flat_map(|prefix| spellings(prefix, schemes))
                .collect(),
        }
    }

    /// Whether the window may go to `url`. A prefix matches a whole part of the
    /// URL: `https://example.com` allows `https://example.com/a` and
    /// `https://example.com?a`, and does not allow `https://example.com.evil.test`,
    /// which a plain string prefix would. `about:blank` is always allowed, since
    /// it is where a window with inline HTML starts.
    pub fn allows(&self, url: &str) -> bool {
        if url == "about:blank" {
            return true;
        }
        self.prefixes
            .iter()
            .any(|prefix| match url.strip_prefix(prefix.as_str()) {
                None => false,
                Some(rest) => {
                    prefix.ends_with('/') || rest.is_empty() || rest.starts_with(['/', '?', '#'])
                }
            })
    }
}

/// Everything a URL path can hold without being escaped, and the separator.
fn unescaped(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/')
}

fn escape(bytes: &[u8], into: &mut String) {
    for &byte in bytes {
        if unescaped(byte) {
            into.push(char::from(byte));
        } else {
            into.push_str(&format!("%{byte:02X}"));
        }
    }
}

/// The `file://` URL of an absolute path as `windows` spells paths, or `None`
/// for one that is not absolute. A drive is `file:///C:/…`, a share
/// `file://server/share/…`, and a `\\?\` prefix (what `canonicalize` gives on
/// Windows) is dropped.
pub fn file_url_for(path: &str, windows: bool) -> Option<String> {
    let mut url = String::from("file://");
    if !windows {
        if !path.starts_with('/') {
            return None;
        }
        escape(path.as_bytes(), &mut url);
        return Some(url);
    }
    let slashed = path.replace('\\', "/");
    let plain = slashed.strip_prefix("//?/").unwrap_or(&slashed);
    if let Some(share) = plain.strip_prefix("//") {
        escape(share.as_bytes(), &mut url);
        return Some(url);
    }
    let bytes = plain.as_bytes();
    if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/' {
        url.push('/');
        url.push(char::from(bytes[0]));
        url.push(':');
        escape(&bytes[2..], &mut url);
        return Some(url);
    }
    None
}

/// The `file://` URL of `path`, which has to be absolute, for a window to load a
/// local file such as a PDF. Escaped, since a path holds spaces and more.
pub fn file_url(path: &std::path::Path) -> Option<String> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        // Bytes, not text: a Unix path need not be UTF-8, and the URL has to
        // name the file it was made from all the same.
        let bytes = path.as_os_str().as_bytes();
        if bytes.first() != Some(&b'/') {
            return None;
        }
        let mut url = String::from("file://");
        escape(bytes, &mut url);
        Some(url)
    }
    #[cfg(not(unix))]
    {
        file_url_for(&path.to_string_lossy(), cfg!(windows))
    }
}

/// Whether `name` can be registered as a custom scheme without taking over one
/// the engine already has.
pub fn valid_scheme_name(name: &str) -> bool {
    const BUILT_IN: [&str; 9] = [
        "http",
        "https",
        "file",
        "about",
        "data",
        "blob",
        "javascript",
        "ws",
        "wss",
    ];
    let mut chars = name.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '+' | '-' | '.'))
        && !BUILT_IN.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_spelt_for_the_platform() {
        assert_eq!(
            scheme_base_for("hangar-kit", false),
            "hangar-kit://localhost"
        );
        assert_eq!(
            scheme_base_for("hangar-kit", true),
            "http://hangar-kit.localhost"
        );
        assert_eq!(scheme_url("x", "/a/b.html"), scheme_url("x", "a/b.html"));
        #[cfg(not(windows))]
        assert_eq!(
            scheme_url("hangar-kit", "/k/index.html"),
            "hangar-kit://localhost/k/index.html"
        );
        #[cfg(windows)]
        assert_eq!(
            scheme_url("hangar-kit", "/k/index.html"),
            "http://hangar-kit.localhost/k/index.html"
        );
    }

    #[test]
    fn navigation_matches_whole_parts_of_a_url() {
        let schemes = ["hangar-kit".to_string()];
        let nav = Navigation::new(
            &[
                "hangar-kit://localhost/hello/".into(),
                "https://example.com".into(),
            ],
            &schemes,
        );
        assert!(nav.allows("hangar-kit://localhost/hello/index.html"));
        assert!(nav.allows("hangar-kit://localhost/hello/"));
        assert!(!nav.allows("hangar-kit://localhost/hello"));
        assert!(!nav.allows("hangar-kit://localhost/hello-evil/index.html"));
        assert!(nav.allows("https://example.com"));
        assert!(nav.allows("https://example.com/a?b#c"));
        assert!(nav.allows("https://example.com?q=1"));
        assert!(!nav.allows("https://example.com.evil.test/"));
        assert!(!nav.allows("https://example.comx"));
        assert!(!nav.allows("file:///etc/passwd"));
        assert!(nav.allows("about:blank"));
    }

    #[test]
    fn a_scheme_prefix_also_matches_the_windows_spellings() {
        let schemes = ["hangar-kit".to_string()];
        let nav = Navigation::new(&["hangar-kit://localhost/hello/".into()], &schemes);
        assert!(nav.allows("http://hangar-kit.localhost/hello/a.html"));
        assert!(nav.allows("https://hangar-kit.localhost/hello/a.html"));
        assert!(!nav.allows("http://hangar-kit.localhost/other/a.html"));
        // Another window's scheme does not get the aliases.
        let alone = Navigation::new(&["hangar-kit://localhost/hello/".into()], &[]);
        assert!(!alone.allows("http://hangar-kit.localhost/hello/a.html"));
    }

    #[test]
    fn files_become_escaped_file_urls() {
        assert_eq!(
            file_url_for("/tmp/a b/é.pdf", false).as_deref(),
            Some("file:///tmp/a%20b/%C3%A9.pdf")
        );
        assert_eq!(
            file_url_for("/a#b?c%d", false).as_deref(),
            Some("file:///a%23b%3Fc%25d")
        );
        assert_eq!(file_url_for("relative/x.pdf", false), None);
        assert_eq!(
            file_url_for("C:\\Users\\me\\a b.pdf", true).as_deref(),
            Some("file:///C:/Users/me/a%20b.pdf")
        );
        assert_eq!(
            file_url_for("\\\\?\\C:\\x.pdf", true).as_deref(),
            Some("file:///C:/x.pdf")
        );
        assert_eq!(
            file_url_for("\\\\srv\\share\\x.pdf", true).as_deref(),
            Some("file://srv/share/x.pdf")
        );
        assert_eq!(file_url_for("x.pdf", true), None);
        #[cfg(unix)]
        {
            use std::ffi::OsStr;
            use std::os::unix::ffi::OsStrExt;
            use std::path::Path;
            assert_eq!(
                file_url(Path::new("/tmp/x y.pdf")).as_deref(),
                Some("file:///tmp/x%20y.pdf")
            );
            assert_eq!(
                file_url(Path::new(OsStr::from_bytes(b"/tmp/\xff.pdf"))).as_deref(),
                Some("file:///tmp/%FF.pdf")
            );
            assert_eq!(file_url(Path::new("x.pdf")), None);
        }
    }

    #[test]
    fn scheme_names_cannot_shadow_the_engines() {
        for good in ["hangar-kit", "hangar-pdf", "x", "a1.b+c"] {
            assert!(valid_scheme_name(good), "{good}");
        }
        for bad in [
            "",
            "HTTP",
            "http",
            "https",
            "file",
            "data",
            "1abc",
            "has space",
            "a:b",
            "é",
            "-a",
        ] {
            assert!(!valid_scheme_name(bad), "{bad}");
        }
    }
}
