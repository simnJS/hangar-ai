//! The phone's web app, built into dist-mobile/ by `pnpm mobile:build` and
//! compiled into the binary, so a phone always gets the app of the Hangar it
//! is talking to.
//!
//! A checkout that never built it still compiles — `cargo test` and the CI's
//! Rust job do not build the frontend — and serves a placeholder page at /
//! instead. A debug build reads dist-mobile/ from disk as it is asked, which
//! is rust-embed's way: a new `pnpm mobile:build` shows up without
//! recompiling anything.

use std::borrow::Cow;

use axum::body::Body;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../dist-mobile/"]
#[allow_missing = true]
struct Pwa;

/// Where the web app's files come from: a path relative to its root in, the
/// file out. The embedded dist-mobile/ in the app, a few files in tests.
pub type Lookup = fn(&str) -> Option<Cow<'static, [u8]>>;

pub fn embedded(path: &str) -> Option<Cow<'static, [u8]>> {
    Pwa::get(path).map(|file| file.data)
}

const PLACEHOLDER: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Hangar Mobile</title>
<style>
body { margin: 0; min-height: 100vh; display: grid; place-items: center; background: #0f1115; color: #e6e6e6; font: 16px/1.5 system-ui, sans-serif; }
main { max-width: 28rem; padding: 24px; }
h1 { font-size: 1.25rem; }
code { background: #1d2027; padding: 2px 6px; border-radius: 4px; }
</style>
</head>
<body>
<main>
<h1>Hangar Mobile is not in this build</h1>
<p>This copy of Hangar was built without its mobile app. Build it with <code>pnpm mobile:build</code>, then build Hangar again.</p>
</main>
</body>
</html>
"#;

/// The answer to a GET for `path`: the file, or the app's index.html for a
/// page of its own routing, or the placeholder when there is no app at all.
pub fn respond(lookup: Lookup, path: &str) -> Response {
    let Some(file) = clean(path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !file.is_empty() {
        if let Some(body) = lookup(&file) {
            return serve(&file, body);
        }
        if file
            .rsplit('/')
            .next()
            .is_some_and(|name| name.contains('.'))
        {
            return StatusCode::NOT_FOUND.into_response();
        }
    }
    match lookup("index.html") {
        Some(body) => serve("index.html", body),
        None => (
            [
                (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            PLACEHOLDER,
        )
            .into_response(),
    }
}

fn serve(file: &str, body: Cow<'static, [u8]>) -> Response {
    let body = match body {
        Cow::Borrowed(bytes) => Body::from(bytes),
        Cow::Owned(bytes) => Body::from(bytes),
    };
    (
        [
            (header::CONTENT_TYPE, content_type(file)),
            (header::CACHE_CONTROL, cache_control(file)),
        ],
        body,
    )
        .into_response()
}

/// The file a URL path names, relative to the app's root, or None for a path
/// that tries to step out of it. rust-embed refuses those too when it reads
/// from disk; this keeps them from getting that far.
fn clean(path: &str) -> Option<String> {
    let decoded = percent_encoding::percent_decode_str(path)
        .decode_utf8()
        .ok()?;
    let mut parts = Vec::new();
    for part in decoded.split('/') {
        match part {
            "" => {}
            "." | ".." => return None,
            part if part.contains(['\\', ':', '\0']) => return None,
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

/// Vite names what it writes under assets/ after its content, so such a file
/// never changes: a new build makes new names. Everything else — index.html,
/// the service worker, the manifest — is checked again on every load.
fn cache_control(file: &str) -> &'static str {
    if file.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    }
}

fn content_type(file: &str) -> &'static str {
    let extension = file
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "webmanifest" => "application/manifest+json",
        "txt" => "text/plain; charset=utf-8",
        "xml" => "application/xml",
        "wasm" => "application/wasm",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// No dist-mobile at all.
    pub fn no_app(_: &str) -> Option<Cow<'static, [u8]>> {
        None
    }

    /// A built app of three files.
    pub fn small_app(path: &str) -> Option<Cow<'static, [u8]>> {
        let body: &'static [u8] = match path {
            "index.html" => b"<!doctype html><title>Hangar</title>",
            "assets/app-3f2a.js" => b"console.log('hangar')",
            "manifest.webmanifest" => b"{}",
            _ => return None,
        };
        Some(Cow::Borrowed(body))
    }

    async fn text(response: Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    fn header_of(response: &Response, name: header::HeaderName) -> &str {
        response.headers()[name].to_str().unwrap()
    }

    #[tokio::test]
    async fn without_the_app_the_root_is_a_placeholder() {
        for path in ["/", "/settings"] {
            let response = respond(no_app, path);
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            assert_eq!(
                header_of(&response, header::CONTENT_TYPE),
                "text/html; charset=utf-8"
            );
            assert!(text(response)
                .await
                .contains("Hangar Mobile is not in this build"));
        }
        assert_eq!(
            respond(no_app, "/assets/app.js").status(),
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn the_app_s_files_are_served_with_their_types() {
        let index = respond(small_app, "/");
        assert_eq!(
            header_of(&index, header::CONTENT_TYPE),
            "text/html; charset=utf-8"
        );
        assert_eq!(header_of(&index, header::CACHE_CONTROL), "no-cache");
        assert!(text(index).await.contains("<title>Hangar</title>"));

        let script = respond(small_app, "/assets/app-3f2a.js");
        assert_eq!(
            header_of(&script, header::CONTENT_TYPE),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(
            header_of(&script, header::CACHE_CONTROL),
            "public, max-age=31536000, immutable"
        );

        let manifest = respond(small_app, "/manifest.webmanifest");
        assert_eq!(
            header_of(&manifest, header::CONTENT_TYPE),
            "application/manifest+json"
        );
    }

    #[tokio::test]
    async fn a_page_of_the_app_s_routing_gets_its_index() {
        let page = respond(small_app, "/panes/Ava");
        assert_eq!(page.status(), StatusCode::OK);
        assert!(text(page).await.contains("<title>Hangar</title>"));
        assert_eq!(
            respond(small_app, "/missing.png").status(),
            StatusCode::NOT_FOUND
        );
    }

    #[test]
    fn a_path_cannot_step_out_of_the_app() {
        for path in [
            "/../Cargo.toml",
            "/assets/../../secret",
            "/%2e%2e/secret",
            "/assets\\..\\x",
            "/C:/Windows/win.ini",
        ] {
            assert_eq!(clean(path), None, "{path}");
            assert_eq!(
                respond(small_app, path).status(),
                StatusCode::NOT_FOUND,
                "{path}"
            );
        }
        assert_eq!(clean("//assets//app.js").as_deref(), Some("assets/app.js"));
        assert_eq!(clean("/").as_deref(), Some(""));
    }

    #[test]
    fn the_embedded_app_answers_a_missing_file_with_none() {
        // Whether this checkout has built the app or not.
        assert!(embedded("no/such/file.js").is_none());
        // A file that does exist, one level up: never served.
        assert!(embedded("../package.json").is_none());
    }
}
