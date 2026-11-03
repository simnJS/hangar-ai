//! How a custom scheme is spelt on each platform, and how a navigation is
//! matched against the prefixes a window may go to.
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
    if windows { format!("http://{scheme}.localhost") } else { format!("{scheme}://localhost") }
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
    if path.starts_with('/') { format!("{base}{path}") } else { format!("{base}/{path}") }
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
        Self { prefixes: prefixes.iter().flat_map(|prefix| spellings(prefix, schemes)).collect() }
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
        self.prefixes.iter().any(|prefix| match url.strip_prefix(prefix.as_str()) {
            None => false,
            Some(rest) => prefix.ends_with('/') || rest.is_empty() || rest.starts_with(['/', '?', '#']),
        })
    }
}

/// Whether `name` can be registered as a custom scheme without taking over one
/// the engine already has.
pub fn valid_scheme_name(name: &str) -> bool {
    const BUILT_IN: [&str; 9] = ["http", "https", "file", "about", "data", "blob", "javascript", "ws", "wss"];
    let mut chars = name.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '+' | '-' | '.'))
        && !BUILT_IN.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_spelt_for_the_platform() {
        assert_eq!(scheme_base_for("hangar-kit", false), "hangar-kit://localhost");
        assert_eq!(scheme_base_for("hangar-kit", true), "http://hangar-kit.localhost");
        assert_eq!(scheme_url("x", "/a/b.html"), scheme_url("x", "a/b.html"));
        #[cfg(not(windows))]
        assert_eq!(scheme_url("hangar-kit", "/k/index.html"), "hangar-kit://localhost/k/index.html");
        #[cfg(windows)]
        assert_eq!(scheme_url("hangar-kit", "/k/index.html"), "http://hangar-kit.localhost/k/index.html");
    }

    #[test]
    fn navigation_matches_whole_parts_of_a_url() {
        let schemes = ["hangar-kit".to_string()];
        let nav = Navigation::new(&["hangar-kit://localhost/hello/".into(), "https://example.com".into()], &schemes);
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
    fn scheme_names_cannot_shadow_the_engines() {
        for good in ["hangar-kit", "hangar-pdf", "x", "a1.b+c"] {
            assert!(valid_scheme_name(good), "{good}");
        }
        for bad in ["", "HTTP", "http", "https", "file", "data", "1abc", "has space", "a:b", "é", "-a"] {
            assert!(!valid_scheme_name(bad), "{bad}");
        }
    }
}
