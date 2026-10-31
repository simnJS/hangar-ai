//! The `hangar-kit` scheme: the pages of kits' tabs and the files beside
//! them, and the SDK every tab loads, `/_hangar/kit.js` (docs/kits.md).
//!
//! A kit is code written by someone else, so its pages are kept away from
//! Hangar the way preview.rs keeps an agent's page away, and further:
//!
//! - every answer carries a `sandbox allow-scripts` Content-Security-Policy,
//!   so a kit's document has an opaque origin wherever it is opened — the
//!   tab's iframe (sandboxed too, by the window), a window of its own. Tauri
//!   counts this scheme among the app's own origins, so a page served
//!   without it would stand where Hangar's own page stands; with it, its IPC
//!   requests carry `Origin: null`, which Tauri refuses, and it has no
//!   storage of its own nor any way to Hangar's;
//! - the same policy allows the kit's own files and the SDK and nothing
//!   else: no network, no frames, no workers, no forms, no plugins. Inline
//!   scripts and styles run, since that is how a small tab is written;
//! - only installed kits that can be used are served, only files inside the
//!   kit's folder once links are followed, only web file types, and never a
//!   hidden file (`.env`, `.git/…`).
//!
//! The tab reaches Hangar only by `postMessage`, answered by the window
//! (src/lib/kitApi.ts) with what the person granted the kit on the
//! workspace. The two commands that read the workspace's files for it are in
//! kit_scheme/files.rs.
//!
//! URLs are `<base>/<kit id>/<path in the kit>`, the base being
//! `http://hangar-kit.localhost` on Windows and `hangar-kit://localhost`
//! elsewhere (`kitTabUrl` in src/lib/kitTabs.ts builds both).

pub mod files;

use std::path::{Path, PathBuf};

use tauri::http::{header, response::Builder, Method, Request, Response, StatusCode};
use tauri::{Runtime, UriSchemeContext, UriSchemeResponder};

use crate::model::{self, Span, MEGABYTE};

/// Registered in lib.rs.
pub const SCHEME: &str = "hangar-kit";

/// `KIT_SDK_PATH` in src/lib/kits.ts.
pub const SDK_PATH: &str = "/_hangar/kit.js";

/// window.hangar, small and without dependencies.
const SDK: &str = include_str!("../kit-sdk/kit.js");

/// How a kit's page reaches the scheme on each platform: Windows' webview as
/// `http(s)://hangar-kit.localhost`, the others by the scheme itself. Every
/// form is named in the policy, since the policy cannot say `'self'` for a
/// document whose origin the sandbox made opaque.
const BASES: [&str; 3] = [
    "http://hangar-kit.localhost",
    "https://hangar-kit.localhost",
    "hangar-kit://localhost",
];

/// Hangar's own pages, the only other origins that may read kit files.
const APP_ORIGINS: [&str; 3] = [
    "tauri://localhost",
    "http://tauri.localhost",
    "https://tauri.localhost",
];

/// The largest kit file served: each is read whole into memory.
const MAX_BYTES: u64 = 64 * MEGABYTE;

/// What a kit may serve, by extension, with the Content-Type it goes out as.
/// Anything else stays on disk.
const TYPES: &[(&str, &str)] = &[
    ("html", "text/html; charset=utf-8"),
    ("htm", "text/html; charset=utf-8"),
    ("js", "text/javascript; charset=utf-8"),
    ("mjs", "text/javascript; charset=utf-8"),
    ("css", "text/css; charset=utf-8"),
    ("json", "application/json; charset=utf-8"),
    ("map", "application/json; charset=utf-8"),
    ("txt", "text/plain; charset=utf-8"),
    ("md", "text/plain; charset=utf-8"),
    ("csv", "text/csv; charset=utf-8"),
    ("svg", "image/svg+xml"),
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("avif", "image/avif"),
    ("ico", "image/x-icon"),
    ("woff", "font/woff"),
    ("woff2", "font/woff2"),
    ("ttf", "font/ttf"),
    ("otf", "font/otf"),
    ("wasm", "application/wasm"),
    ("mp3", "audio/mpeg"),
    ("wav", "audio/wav"),
    ("ogg", "audio/ogg"),
    ("m4a", "audio/mp4"),
    ("mp4", "video/mp4"),
    ("webm", "video/webm"),
];

fn content_type(path: &Path) -> Option<&'static str> {
    let extension = model::extension(path)?;
    TYPES
        .iter()
        .find(|(known, _)| *known == extension)
        .map(|(_, mime)| *mime)
}

/// An installed kit the scheme may serve: one that can be used right now.
#[derive(Debug, Clone)]
pub(crate) struct KitRoot {
    pub id: String,
    /// Canonical.
    pub root: PathBuf,
}

/// The policy every file of the kit `id` goes out with.
pub(crate) fn policy(id: &str) -> String {
    let own = BASES
        .iter()
        .map(|base| format!("{base}/{id}/"))
        .collect::<Vec<_>>()
        .join(" ");
    let sdk = BASES
        .iter()
        .map(|base| format!("{base}{SDK_PATH}"))
        .collect::<Vec<_>>()
        .join(" ");
    [
        "sandbox allow-scripts".to_string(),
        "default-src 'none'".to_string(),
        format!("script-src 'unsafe-inline' 'wasm-unsafe-eval' {own} {sdk}"),
        format!("style-src 'unsafe-inline' {own}"),
        format!("img-src {own} data: blob:"),
        format!("font-src {own} data:"),
        format!("media-src {own} data: blob:"),
        format!("connect-src {own}"),
        "frame-src 'none'".to_string(),
        "child-src 'none'".to_string(),
        "worker-src 'none'".to_string(),
        "object-src 'none'".to_string(),
        "manifest-src 'none'".to_string(),
        "form-action 'none'".to_string(),
        "base-uri 'none'".to_string(),
    ]
    .join("; ")
}

/// What answers that are not a kit's file go out with: an error, or the SDK.
const BARE_POLICY: &str = "sandbox; default-src 'none'";

/// What a request names.
#[derive(Debug, PartialEq)]
enum Target {
    Sdk,
    /// A file of a kit, by its path's parts, each decoded.
    File {
        kit: String,
        parts: Vec<String>,
    },
}

fn valid_kit_id(id: &str) -> bool {
    (2..=40).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The request's path, read strictly. A browser resolves `.` and `..` out
/// of a URL before sending it, so one still there was encoded on purpose
/// and is refused, as is an encoded separator, a drive or a stream (`:`),
/// and a NUL. A hidden file or folder is never served.
fn target(url_path: &str) -> Result<Target, (StatusCode, String)> {
    if url_path == SDK_PATH {
        return Ok(Target::Sdk);
    }
    let not_found = |why: &str| Err((StatusCode::NOT_FOUND, why.to_string()));
    let forbidden = |why: &str| Err((StatusCode::FORBIDDEN, why.to_string()));
    let mut raw: Vec<&str> = url_path.trim_start_matches('/').split('/').collect();
    let kit = raw.remove(0);
    if !valid_kit_id(kit) {
        return not_found("no such kit");
    }
    // A folder, asked for with its trailing slash: there is no listing.
    if raw.last() == Some(&"") {
        return not_found("name a file of the kit");
    }
    let mut parts = Vec::new();
    for segment in raw {
        let decoded = match percent_encoding::percent_decode_str(segment).decode_utf8() {
            Ok(decoded) => decoded.into_owned(),
            Err(_) => return Err((StatusCode::BAD_REQUEST, "the path is not UTF-8".into())),
        };
        if decoded.is_empty() || decoded == "." || decoded == ".." {
            return forbidden("the path must name a file inside the kit, without `.` or `..`");
        }
        if decoded.contains(['/', '\\', ':', '\0']) {
            return forbidden("the path holds a character a kit path cannot have");
        }
        // Windows drops a trailing dot or space, so `index.html.` would be
        // read as another name than the one checked.
        if decoded.ends_with(['.', ' ']) {
            return forbidden("a path part must not end with a dot or a space");
        }
        if decoded.starts_with('.') {
            return not_found("hidden files are not served");
        }
        parts.push(decoded);
    }
    if parts.is_empty() {
        return not_found("name a file of the kit");
    }
    Ok(Target::File {
        kit: kit.to_string(),
        parts,
    })
}

/// The base of every answer: readable by the kit's own pages (their module
/// scripts and fonts are fetched with CORS from an opaque origin), never
/// cached, never sniffed, never sending a referrer or prefetching a name.
fn base(policy: &str) -> Builder {
    Response::builder()
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(header::CONTENT_SECURITY_POLICY, policy)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::REFERRER_POLICY, "no-referrer")
        .header("X-DNS-Prefetch-Control", "off")
}

fn refuse(status: StatusCode, reason: String) -> Response<Vec<u8>> {
    model::finish(
        base(BARE_POLICY)
            .status(status)
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8"),
        reason.into_bytes(),
    )
}

/// Answers one request to the scheme, `kits` being the kits it may serve.
pub(crate) fn answer(request: &Request<Vec<u8>>, kits: &[KitRoot]) -> Response<Vec<u8>> {
    // A kit's page is opaque (`null`); a navigation or a plain `<img>` sends
    // no Origin. Any other origin is a page that has no business here: one a
    // tab navigated its frame to, for instance.
    if let Some(origin) = request.headers().get(header::ORIGIN) {
        let allowed = origin
            .to_str()
            .is_ok_and(|origin| origin == "null" || APP_ORIGINS.contains(&origin));
        if !allowed {
            return refuse(
                StatusCode::FORBIDDEN,
                "this page may not read kit files".into(),
            );
        }
    }

    let method = request.method();
    if method == Method::OPTIONS {
        return model::finish(
            base(BARE_POLICY)
                .status(StatusCode::NO_CONTENT)
                .header(header::ACCESS_CONTROL_ALLOW_METHODS, "GET, HEAD, OPTIONS")
                .header(header::ACCESS_CONTROL_ALLOW_HEADERS, "Range")
                .header(header::ACCESS_CONTROL_MAX_AGE, "600"),
            Vec::new(),
        );
    }
    if method != Method::GET && method != Method::HEAD {
        return refuse(
            StatusCode::METHOD_NOT_ALLOWED,
            "only GET and HEAD are served".into(),
        );
    }
    let body_of = |bytes: Vec<u8>| {
        if method == Method::HEAD {
            Vec::new()
        } else {
            bytes
        }
    };

    let (kit, parts) = match target(request.uri().path()) {
        Ok(Target::Sdk) => {
            return model::finish(
                base(BARE_POLICY)
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "text/javascript; charset=utf-8")
                    .header(header::CONTENT_LENGTH, SDK.len()),
                body_of(SDK.as_bytes().to_vec()),
            )
        }
        Ok(Target::File { kit, parts }) => (kit, parts),
        Err((status, reason)) => return refuse(status, reason),
    };
    let Some(kit) = kits.iter().find(|entry| entry.id == kit) else {
        return refuse(
            StatusCode::NOT_FOUND,
            format!("no usable kit {kit:?} is installed"),
        );
    };

    let joined = parts
        .iter()
        .fold(kit.root.clone(), |path, part| path.join(part));
    let Ok(path) = dunce::canonicalize(&joined) else {
        return refuse(StatusCode::NOT_FOUND, "not found in the kit".into());
    };
    // A link inside the kit that leads out of it.
    if !path.starts_with(&kit.root) {
        return refuse(StatusCode::FORBIDDEN, "leads outside the kit folder".into());
    }
    let Ok(metadata) = std::fs::metadata(&path) else {
        return refuse(StatusCode::NOT_FOUND, "not found in the kit".into());
    };
    if !metadata.is_file() {
        return refuse(StatusCode::NOT_FOUND, "not a file".into());
    }
    let Some(mime) = content_type(&path) else {
        return refuse(
            StatusCode::FORBIDDEN,
            "a kit serves only web files: pages, scripts, styles, data, images, fonts, sound and video".into(),
        );
    };
    let len = metadata.len();
    if len > MAX_BYTES {
        return refuse(
            StatusCode::PAYLOAD_TOO_LARGE,
            format!("kit files are served up to {} MB", MAX_BYTES / MEGABYTE),
        );
    }

    let range = request
        .headers()
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok());
    let (status, start, count) = match model::asked_span(range, len).unwrap_or(Span::Whole) {
        Span::Whole => (StatusCode::OK, 0, len),
        Span::Part { start, end } => (StatusCode::PARTIAL_CONTENT, start, end - start + 1),
        Span::Unsatisfiable => {
            return model::finish(
                base(BARE_POLICY)
                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                    .header(header::CONTENT_RANGE, format!("bytes */{len}")),
                Vec::new(),
            )
        }
    };
    let body = if method == Method::HEAD {
        Vec::new()
    } else {
        match model::read_span(&path, start, count) {
            Ok(body) if body.len() as u64 == count => body,
            Ok(_) => {
                return refuse(
                    StatusCode::CONFLICT,
                    "the file changed while it was read".into(),
                )
            }
            Err(err) => return refuse(StatusCode::FORBIDDEN, format!("unreadable: {err}")),
        }
    };
    let mut builder = base(&policy(&kit.id))
        .status(status)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_LENGTH, count)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(
            header::ACCESS_CONTROL_EXPOSE_HEADERS,
            "Content-Range, Accept-Ranges",
        );
    if status == StatusCode::PARTIAL_CONTENT {
        builder = builder.header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{}/{len}", start + count - 1),
        );
    }
    model::finish(builder, body)
}

/// The installed kits that can be used now, by canonical folder. Read at
/// each request, so a kit removed or broken stops being served at once.
async fn usable_kits() -> Vec<KitRoot> {
    crate::kits::kits_list()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|kit| kit.error.is_none() && kit.manifest.is_some())
        .filter_map(|kit| {
            Some(KitRoot {
                root: dunce::canonicalize(&kit.dir).ok()?,
                id: kit.id,
            })
        })
        .collect()
}

/// The `hangar-kit` handler registered in lib.rs: the work leaves the
/// webview's thread, which Tauri calls it on.
pub fn serve<R: Runtime>(
    _ctx: UriSchemeContext<'_, R>,
    request: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    tauri::async_runtime::spawn(async move {
        let kits = if request.uri().path() == SDK_PATH {
            Vec::new()
        } else {
            usable_kits().await
        };
        let response = tauri::async_runtime::spawn_blocking(move || answer(&request, &kits))
            .await
            .unwrap_or_else(|_| {
                refuse(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "the kit file could not be served".into(),
                )
            });
        responder.respond(response);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::{header_of, link, temp_dir, write};

    fn get(path: &str, origin: Option<&str>) -> Request<Vec<u8>> {
        let mut request = Request::builder().uri(format!("http://hangar-kit.localhost{path}"));
        if let Some(origin) = origin {
            request = request.header(header::ORIGIN, origin);
        }
        request.body(Vec::new()).expect("request")
    }

    fn kit(tag: &str, id: &str) -> KitRoot {
        let root = dunce::canonicalize(temp_dir(tag)).expect("canonical");
        write(&root, "hangar-kit.json", b"{}");
        write(&root, "tabs/status/index.html", b"<!doctype html><p>hi</p>");
        KitRoot {
            id: id.to_string(),
            root,
        }
    }

    fn status(response: &Response<Vec<u8>>) -> StatusCode {
        response.status()
    }

    #[test]
    fn a_kit_s_page_is_served_as_html_under_its_policy() {
        let hello = kit("page", "hello-kit");
        let response = answer(
            &get("/hello-kit/tabs/status/index.html", None),
            std::slice::from_ref(&hello),
        );
        assert_eq!(status(&response), StatusCode::OK);
        assert_eq!(response.body(), b"<!doctype html><p>hi</p>");
        assert_eq!(
            header_of(&response, header::CONTENT_TYPE),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            header_of(&response, header::X_CONTENT_TYPE_OPTIONS),
            "nosniff"
        );
        assert_eq!(
            header_of(&response, header::CONTENT_SECURITY_POLICY),
            policy("hello-kit")
        );
        let _ = std::fs::remove_dir_all(&hello.root);
    }

    #[test]
    fn the_policy_isolates_the_page_and_allows_its_own_files_only() {
        let csp = policy("hello-kit");
        let directives: Vec<&str> = csp.split("; ").collect();
        // An opaque origin: scripts run, nothing else the sandbox guards.
        assert_eq!(directives[0], "sandbox allow-scripts");
        assert!(!csp.contains("allow-same-origin"));
        assert!(!csp.contains("allow-top-navigation"));
        assert!(!csp.contains("allow-popups"));
        assert!(!csp.contains("allow-forms"));
        assert!(directives.contains(&"default-src 'none'"));
        for blocked in [
            "frame-src 'none'",
            "worker-src 'none'",
            "object-src 'none'",
            "form-action 'none'",
            "base-uri 'none'",
        ] {
            assert!(directives.contains(&blocked), "{blocked}");
        }
        // Every source named is the kit's own folder, the SDK, or local data:
        // no host of the network, no wildcard, no other kit.
        for directive in &directives[1..] {
            for source in directive.split(' ').skip(1) {
                let local = source.starts_with('\'')
                    || source == "data:"
                    || source == "blob:"
                    || BASES
                        .iter()
                        .any(|base| source == format!("{base}/hello-kit/"))
                    || BASES
                        .iter()
                        .any(|base| source == format!("{base}{SDK_PATH}"));
                assert!(local, "{source} in {directive}");
            }
        }
        let connect = directives
            .iter()
            .find(|d| d.starts_with("connect-src"))
            .unwrap();
        assert!(connect.contains("http://hangar-kit.localhost/hello-kit/"));
        assert!(connect.contains("hangar-kit://localhost/hello-kit/"));
        assert!(!connect.contains("_hangar"));
        let script = directives
            .iter()
            .find(|d| d.starts_with("script-src"))
            .unwrap();
        assert!(script.contains("http://hangar-kit.localhost/_hangar/kit.js"));
        assert!(!script.contains("'unsafe-eval'"));
    }

    #[test]
    fn every_type_goes_out_as_what_it_is() {
        let hello = kit("mime", "hello-kit");
        let cases = [
            ("app.js", "text/javascript; charset=utf-8"),
            ("app.mjs", "text/javascript; charset=utf-8"),
            ("style.css", "text/css; charset=utf-8"),
            ("data.json", "application/json; charset=utf-8"),
            ("icon.svg", "image/svg+xml"),
            ("icon.png", "image/png"),
            ("photo.JPG", "image/jpeg"),
            ("font.woff2", "font/woff2"),
            ("lib.wasm", "application/wasm"),
            ("notes.txt", "text/plain; charset=utf-8"),
        ];
        for (name, mime) in cases {
            write(&hello.root, &format!("assets/{name}"), b"x");
            let response = answer(
                &get(&format!("/hello-kit/assets/{name}"), None),
                std::slice::from_ref(&hello),
            );
            assert_eq!(status(&response), StatusCode::OK, "{name}");
            assert_eq!(header_of(&response, header::CONTENT_TYPE), mime, "{name}");
            assert_eq!(
                header_of(&response, header::ACCESS_CONTROL_ALLOW_ORIGIN),
                "*"
            );
        }
        // What is not a web file stays on disk.
        for name in ["run.exe", "script.ps1", "Makefile", "archive.zip"] {
            write(&hello.root, &format!("assets/{name}"), b"x");
            let response = answer(
                &get(&format!("/hello-kit/assets/{name}"), None),
                std::slice::from_ref(&hello),
            );
            assert_eq!(status(&response), StatusCode::FORBIDDEN, "{name}");
            assert!(response.body().starts_with(b"a kit serves only web files"));
        }
        let _ = std::fs::remove_dir_all(&hello.root);
    }

    #[test]
    fn the_sdk_is_served_to_every_tab() {
        let response = answer(&get(SDK_PATH, None), &[]);
        assert_eq!(status(&response), StatusCode::OK);
        assert_eq!(
            header_of(&response, header::CONTENT_TYPE),
            "text/javascript; charset=utf-8"
        );
        let body = String::from_utf8(response.body().clone()).unwrap();
        assert!(body.contains("window.hangar") || body.contains("\"hangar\""));
        assert!(body.contains("postMessage"));
        // Nothing else lives under /_hangar.
        for path in ["/_hangar/other.js", "/_hangar/", "/_hangar"] {
            assert_eq!(
                status(&answer(&get(path, None), &[])),
                StatusCode::NOT_FOUND,
                "{path}"
            );
        }
    }

    #[test]
    fn an_unknown_kit_and_a_missing_file_are_not_found() {
        let hello = kit("unknown", "hello-kit");
        for path in [
            "/other-kit/tabs/status/index.html",
            "/Hello-Kit/tabs/status/index.html",
            "/x/index.html",
            "/hello-kit/tabs/status/missing.html",
            "/hello-kit/tabs/status",
            "/hello-kit/tabs/status/",
            "/hello-kit",
            "/hello-kit/",
            "/",
        ] {
            let response = answer(&get(path, None), std::slice::from_ref(&hello));
            assert_eq!(status(&response), StatusCode::NOT_FOUND, "{path}");
        }
        let _ = std::fs::remove_dir_all(&hello.root);
    }

    #[test]
    fn nothing_outside_the_kit_folder_is_served() {
        let hello = kit("traversal", "hello-kit");
        let outside = temp_dir("traversal-outside");
        write(&outside, "secret.txt", b"secret");
        let sibling = hello.root.parent().unwrap().join("secret.txt");
        std::fs::write(&sibling, b"secret").unwrap();
        for path in [
            "/hello-kit/%2E%2E/secret.txt",
            "/hello-kit/..%2Fsecret.txt",
            "/hello-kit/tabs%2F..%2F..%2Fsecret.txt",
            "/hello-kit/tabs%5C..%5C..%5Csecret.txt",
            "/hello-kit/./tabs/status/index.html",
            "/hello-kit/%2E/tabs/status/index.html",
            "/hello-kit/C%3A/Windows/win.ini",
            "/hello-kit/tabs//status/index.html",
            "/hello-kit/tabs/status/index.html%00.png",
            "/hello-kit/tabs/status/index.html.",
            "/hello-kit/hangar-kit.json:stream",
        ] {
            let response = answer(&get(path, None), std::slice::from_ref(&hello));
            assert_eq!(status(&response), StatusCode::FORBIDDEN, "{path}");
        }
        // Hidden files and folders, whatever they hold.
        write(&hello.root, ".env", b"TOKEN=1");
        write(&hello.root, ".git/config", b"[core]");
        for path in [
            "/hello-kit/.env",
            "/hello-kit/.git/config",
            "/hello-kit/%2Eenv",
        ] {
            let response = answer(&get(path, None), std::slice::from_ref(&hello));
            assert_eq!(status(&response), StatusCode::NOT_FOUND, "{path}");
        }
        // A link inside the kit that leads out of it.
        if link(&outside.join("secret.txt"), &hello.root.join("leak.txt")) {
            let response = answer(
                &get("/hello-kit/leak.txt", None),
                std::slice::from_ref(&hello),
            );
            assert_eq!(status(&response), StatusCode::FORBIDDEN);
        }
        if link(&outside, &hello.root.join("away")) {
            let response = answer(
                &get("/hello-kit/away/secret.txt", None),
                std::slice::from_ref(&hello),
            );
            assert_eq!(status(&response), StatusCode::FORBIDDEN);
        }
        // One kit's folder is not another's: each is served under its own id.
        let other = kit("traversal-other", "other-kit");
        write(&other.root, "page.html", b"other");
        let response = answer(
            &get("/hello-kit/page.html", None),
            &[hello.clone(), other.clone()],
        );
        assert_eq!(status(&response), StatusCode::NOT_FOUND);
        let response = answer(
            &get("/other-kit/page.html", None),
            &[hello.clone(), other.clone()],
        );
        assert_eq!(status(&response), StatusCode::OK);
        assert_eq!(
            header_of(&response, header::CONTENT_SECURITY_POLICY),
            policy("other-kit")
        );
        for dir in [&hello.root, &other.root, &outside] {
            let _ = std::fs::remove_dir_all(dir);
        }
        let _ = std::fs::remove_file(sibling);
    }

    #[test]
    fn only_kit_pages_and_hangar_may_read_with_an_origin() {
        let hello = kit("origin", "hello-kit");
        let path = "/hello-kit/tabs/status/index.html";
        for origin in ["null", "http://tauri.localhost", "tauri://localhost"] {
            let response = answer(&get(path, Some(origin)), std::slice::from_ref(&hello));
            assert_eq!(status(&response), StatusCode::OK, "{origin}");
        }
        for origin in [
            "https://example.com",
            "http://localhost:1420",
            "http://hangar-model.localhost",
        ] {
            let response = answer(&get(path, Some(origin)), std::slice::from_ref(&hello));
            assert_eq!(status(&response), StatusCode::FORBIDDEN, "{origin}");
        }
        let _ = std::fs::remove_dir_all(&hello.root);
    }

    #[test]
    fn errors_and_methods_are_answered_without_the_kit_s_policy() {
        let hello = kit("errors", "hello-kit");
        let post = Request::builder()
            .method(Method::POST)
            .uri("http://hangar-kit.localhost/hello-kit/tabs/status/index.html")
            .body(Vec::new())
            .unwrap();
        let response = answer(&post, std::slice::from_ref(&hello));
        assert_eq!(status(&response), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(
            header_of(&response, header::CONTENT_SECURITY_POLICY),
            BARE_POLICY
        );
        let head = Request::builder()
            .method(Method::HEAD)
            .uri("http://hangar-kit.localhost/hello-kit/tabs/status/index.html")
            .body(Vec::new())
            .unwrap();
        let response = answer(&head, std::slice::from_ref(&hello));
        assert_eq!(status(&response), StatusCode::OK);
        assert!(response.body().is_empty());
        let ranged = Request::builder()
            .uri("http://hangar-kit.localhost/hello-kit/tabs/status/index.html")
            .header(header::RANGE, "bytes=2-5")
            .body(Vec::new())
            .unwrap();
        let response = answer(&ranged, std::slice::from_ref(&hello));
        assert_eq!(status(&response), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.body(), b"doct");
        let _ = std::fs::remove_dir_all(&hello.root);
    }
}
