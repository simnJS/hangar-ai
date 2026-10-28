//! The mobile server: its listener and its routes, each kind behind its own
//! door.
//!
//! - The web app's files, at / and below, are open: a phone that is not
//!   paired yet has to load the page that pairs it.
//! - /m/api/health and /m/api/pair are open for the same reason.
//! - Everything else under /m/ takes a paired phone's cookie, or 401.
//!
//! Every request that is not a GET or a HEAD also carries `X-Hangar: 1`, or
//! 403 (`from_the_app`), and no answer carries a CORS header: a page on
//! another site can neither send these requests nor read their answers.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use axum::body::{to_bytes, Body};
use axum::extract::{Path, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use super::pairing::PairError;
use super::{assets, Remote};
use crate::control::ControlError;

/// What a phone may ask the window, each relayed as the internal operation
/// `remote_<op>`, which the window answers.
pub const CALLS: &[&str] = &[
    "state",
    "screen",
    "send",
    "permission",
    "manager",
    "history",
];

/// The largest call, `send`, carries a prompt; 64 KB is far past any typed
/// on a phone, and keeps a runaway body from reaching the window at all.
const CALL_LIMIT: usize = 64 * 1024;
/// A pairing is a code and a phone's name.
const PAIR_LIMIT: usize = 4 * 1024;

const COOKIE: &str = "hangar_device";
const COOKIE_MAX_AGE: u64 = 365 * 24 * 60 * 60;

fn refuse(status: StatusCode, error: &str) -> Response {
    (status, Json(json!({ "error": error }))).into_response()
}

pub fn router(remote: Arc<Remote>) -> Router {
    let paired = Router::new()
        .route("/m/api/call/{op}", post(call))
        // /m/api/shares, /m/files/{id} and /m/api/upload, then /m/api/push/*,
        // join here, behind the same cookie.
        .route("/m/{*rest}", any(unknown))
        .route_layer(middleware::from_fn_with_state(remote.clone(), paired_only));
    Router::new()
        .route("/m/api/health", get(health))
        .route("/m/api/pair", post(pair))
        .merge(paired)
        .fallback(page)
        .layer(middleware::from_fn(from_the_app))
        .layer(middleware::map_response(harden))
        .with_state(remote)
}

/// The token a phone's cookie carries, if it sent one.
pub fn cookie_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .find_map(|pair| pair.trim().strip_prefix(COOKIE)?.strip_prefix('='))
        .filter(|token| !token.is_empty())
}

/// A page on another site can send a form or a plain POST to this server
/// with the phone's cookie riding along — but not with a header of its own,
/// which takes a CORS preflight this server never answers. A request with
/// `X-Hangar: 1` came from the web app itself.
async fn from_the_app(request: Request, next: Next) -> Response {
    let reads = matches!(*request.method(), Method::GET | Method::HEAD);
    let marked = request
        .headers()
        .get("x-hangar")
        .is_some_and(|value| value == "1");
    if !reads && !marked {
        return refuse(StatusCode::FORBIDDEN, "forbidden");
    }
    next.run(request).await
}

/// The phone whose cookie opens the route, handed to it as an extension.
async fn paired_only(
    State(remote): State<Arc<Remote>>,
    mut request: Request,
    next: Next,
) -> Response {
    let Some(device) = remote.device(request.headers()) else {
        return refuse(StatusCode::UNAUTHORIZED, "unpaired");
    };
    request.extensions_mut().insert(device);
    next.run(request).await
}

/// Answers kept out of every cache, the API's as much as the placeholder's,
/// unless the file says otherwise; and a page no other site can frame, no
/// browser second-guesses the type of, and no link out of tells where it was.
async fn harden(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    if !headers.contains_key(header::CACHE_CONTROL) {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    response
}

async fn page(State(remote): State<Arc<Remote>>, method: Method, uri: Uri) -> Response {
    if !matches!(method, Method::GET | Method::HEAD) {
        return refuse(StatusCode::METHOD_NOT_ALLOWED, "method-not-allowed");
    }
    assets::respond(remote.assets, uri.path())
}

async fn unknown() -> Response {
    refuse(StatusCode::NOT_FOUND, "not-found")
}

/// Whether the PC answers, what it is, and whether this phone is paired with
/// it — the first thing the web app asks, cookie or not.
async fn health(State(remote): State<Arc<Remote>>, headers: HeaderMap) -> Response {
    Json(json!({
        "ok": true,
        "pc": remote.pc,
        "version": env!("CARGO_PKG_VERSION"),
        "paired": remote.device(&headers).is_some(),
    }))
    .into_response()
}

/// The request's JSON, read to `limit` bytes at most, or the refusal to
/// answer with. An empty body is `{}`: a call with nothing to say need not
/// say it.
async fn read_json(body: Body, limit: usize) -> Result<Value, (StatusCode, &'static str)> {
    let bytes = to_bytes(body, limit)
        .await
        .map_err(|_| (StatusCode::PAYLOAD_TOO_LARGE, "too-large"))?;
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(json!({}));
    }
    serde_json::from_slice(&bytes).map_err(|_| (StatusCode::BAD_REQUEST, "bad-json"))
}

/// `{ code, device }`, `device` being the name the phone gives itself.
async fn pair(State(remote): State<Arc<Remote>>, body: Body) -> Response {
    let body = match read_json(body, PAIR_LIMIT).await {
        Ok(body) => body,
        Err((status, error)) => return refuse(status, error),
    };
    let Some(code) = body["code"].as_str() else {
        return refuse(StatusCode::BAD_REQUEST, "bad-request");
    };
    let name = match &body["device"] {
        Value::String(name) => name.as_str(),
        device => device["name"].as_str().unwrap_or_default(),
    };
    match remote.pair(code, name) {
        Ok((device, token)) => {
            let cookie = format!(
                "{COOKIE}={token}; Path=/; Max-Age={COOKIE_MAX_AGE}; HttpOnly; Secure; SameSite=Strict"
            );
            (
                [(header::SET_COOKIE, cookie)],
                Json(json!({ "device": { "id": device.id, "name": device.name } })),
            )
                .into_response()
        }
        Err(PairError::Wrong { left }) => (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "wrong-code", "left": left })),
        )
            .into_response(),
        Err(PairError::Expired) => refuse(StatusCode::GONE, "expired"),
        Err(PairError::Storage(message)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "storage", "message": message })),
        )
            .into_response(),
    }
}

/// One of the phone's calls, relayed to the window and its answer handed
/// back as is.
async fn call(State(remote): State<Arc<Remote>>, Path(op): Path<String>, body: Body) -> Response {
    if !CALLS.contains(&op.as_str()) {
        return refuse(StatusCode::NOT_FOUND, "unknown-op");
    }
    let args = match read_json(body, CALL_LIMIT).await {
        Ok(args) => args,
        Err((status, error)) => return refuse(status, error),
    };
    match remote
        .window
        .ask(&format!("remote_{op}"), args, remote.call_timeout)
        .await
    {
        Ok(answer) => Json(answer).into_response(),
        Err(ControlError::Refused(message)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "refused", "message": message })),
        )
            .into_response(),
        Err(ControlError::Unavailable(_)) => refuse(StatusCode::SERVICE_UNAVAILABLE, "pc-busy"),
    }
}

/// The listener while it runs. Stopping it — or dropping it — closes the
/// port at once; a call already in flight still gets its answer.
pub struct Running {
    addr: SocketAddr,
    stop: oneshot::Sender<()>,
}

impl Running {
    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    pub fn stop(self) {
        let _ = self.stop.send(());
    }
}

pub async fn listen(remote: Arc<Remote>, port: u16) -> Result<Running, String> {
    let listener = bind(port)
        .await
        .map_err(|err| format!("could not listen on 127.0.0.1:{port}: {err}"))?;
    let addr = listener.local_addr().map_err(|err| err.to_string())?;
    let (stop, stopped) = oneshot::channel::<()>();
    let app = router(remote);
    tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await;
    });
    Ok(Running { addr, stop })
}

/// 127.0.0.1 and nowhere else: Tailscale Serve reaches the server there, and
/// nothing on the network can. A port still held by the listener just
/// stopped — off and on again in the settings — gets a second to come free.
async fn bind(port: u16) -> std::io::Result<TcpListener> {
    let mut tries = 0;
    loop {
        match TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await {
            Err(err) if err.kind() == std::io::ErrorKind::AddrInUse && tries < 10 => {
                tries += 1;
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            bound => return bound,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use axum::http::Request as HttpRequest;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tower::ServiceExt;

    use super::super::assets::tests::{no_app, small_app};
    use super::super::tests::{remote_with, test_port, Fake};
    use super::*;

    struct Answer {
        status: StatusCode,
        headers: HeaderMap,
        body: Value,
    }

    async fn ask(remote: &Arc<Remote>, request: HttpRequest<Body>) -> Answer {
        let response = router(remote.clone()).oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
        Answer {
            status,
            headers,
            body,
        }
    }

    fn get_from(path: &str, cookie: Option<&str>) -> HttpRequest<Body> {
        let mut request = HttpRequest::get(path);
        if let Some(token) = cookie {
            request = request.header(header::COOKIE, format!("theme=dark; {COOKIE}={token}"));
        }
        request.body(Body::empty()).unwrap()
    }

    fn post_to(
        path: &str,
        cookie: Option<&str>,
        marked: bool,
        body: impl Into<Body>,
    ) -> HttpRequest<Body> {
        let mut request = HttpRequest::post(path).header(header::CONTENT_TYPE, "application/json");
        if let Some(token) = cookie {
            request = request.header(header::COOKIE, format!("{COOKIE}={token}"));
        }
        if marked {
            request = request.header("x-hangar", "1");
        }
        request.body(body.into()).unwrap()
    }

    /// A phone paired the way the web app does it, and the token its cookie
    /// was given.
    async fn paired_phone(remote: &Arc<Remote>) -> String {
        let code = remote
            .begin_pairing("atelier.tail1234.ts.net")
            .unwrap()
            .code;
        let answer = ask(
            remote,
            post_to(
                "/m/api/pair",
                None,
                true,
                json!({ "code": code, "device": "Pixel 9" }).to_string(),
            ),
        )
        .await;
        assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
        let cookie = answer.headers[header::SET_COOKIE].to_str().unwrap();
        cookie
            .split(';')
            .next()
            .and_then(|pair| pair.strip_prefix("hangar_device="))
            .unwrap()
            .to_string()
    }

    #[tokio::test]
    async fn without_dist_mobile_the_root_is_a_placeholder_page() {
        let remote = remote_with(Fake::echo(), no_app);
        let answer = ask(&remote, get_from("/", None)).await;
        assert_eq!(answer.status, StatusCode::OK);
        assert!(answer.headers[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/html"));
        assert!(answer
            .body
            .as_str()
            .unwrap()
            .contains("Hangar Mobile is not in this build"));
    }

    #[tokio::test]
    async fn the_app_is_served_at_the_root_without_a_cookie() {
        let remote = remote_with(Fake::echo(), small_app);
        let answer = ask(&remote, get_from("/", None)).await;
        assert_eq!(answer.status, StatusCode::OK);
        assert!(answer
            .body
            .as_str()
            .unwrap()
            .contains("<title>Hangar</title>"));
        let script = ask(&remote, get_from("/assets/app-3f2a.js", None)).await;
        assert_eq!(script.status, StatusCode::OK);
    }

    #[tokio::test]
    async fn health_answers_anyone_and_says_whether_they_are_paired() {
        let remote = remote_with(Fake::echo(), no_app);
        let answer = ask(&remote, get_from("/m/api/health", None)).await;
        assert_eq!(answer.status, StatusCode::OK);
        assert_eq!(answer.body["ok"], true);
        assert_eq!(answer.body["paired"], false);
        assert_eq!(answer.body["version"], env!("CARGO_PKG_VERSION"));
        assert!(!answer.body["pc"].as_str().unwrap().is_empty());

        let token = paired_phone(&remote).await;
        let answer = ask(&remote, get_from("/m/api/health", Some(&token))).await;
        assert_eq!(answer.body["paired"], true);
        let answer = ask(&remote, get_from("/m/api/health", Some("forged"))).await;
        assert_eq!(answer.body["paired"], false);
    }

    #[tokio::test]
    async fn the_right_code_pairs_a_phone_with_a_strict_cookie() {
        let remote = remote_with(Fake::echo(), no_app);
        let code = remote
            .begin_pairing("atelier.tail1234.ts.net")
            .unwrap()
            .code;
        let answer = ask(
            &remote,
            post_to(
                "/m/api/pair",
                None,
                true,
                json!({ "code": code, "device": "  Pixel 9 " }).to_string(),
            ),
        )
        .await;
        assert_eq!(answer.status, StatusCode::OK);
        assert_eq!(answer.body["device"]["name"], "Pixel 9");
        let id = answer.body["device"]["id"].as_str().unwrap();

        let cookie = answer.headers[header::SET_COOKIE].to_str().unwrap();
        let mut parts = cookie.split("; ");
        let token = parts
            .next()
            .unwrap()
            .strip_prefix("hangar_device=")
            .unwrap();
        assert!(token.len() >= 64, "{cookie}");
        let flags: Vec<&str> = parts.collect();
        for flag in [
            "Path=/",
            "Max-Age=31536000",
            "HttpOnly",
            "Secure",
            "SameSite=Strict",
        ] {
            assert!(flags.contains(&flag), "{flag} missing from {cookie}");
        }

        let devices = remote.devices();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id, id);
        // Kept hashed: the token is nowhere in what is saved.
        let saved = std::fs::read_to_string(remote.store.file()).unwrap();
        assert!(!saved.contains(token));
    }

    #[tokio::test]
    async fn a_code_works_once() {
        let remote = remote_with(Fake::echo(), no_app);
        let code = remote
            .begin_pairing("atelier.tail1234.ts.net")
            .unwrap()
            .code;
        let body = json!({ "code": code, "device": "Pixel" }).to_string();
        let first = ask(&remote, post_to("/m/api/pair", None, true, body.clone())).await;
        assert_eq!(first.status, StatusCode::OK);
        let again = ask(&remote, post_to("/m/api/pair", None, true, body)).await;
        assert_eq!(again.status, StatusCode::GONE);
        assert_eq!(again.body, json!({ "error": "expired" }));
        assert!(again.headers.get(header::SET_COOKIE).is_none());
        assert_eq!(remote.devices().len(), 1);
    }

    #[tokio::test]
    async fn a_code_out_of_time_pairs_nothing() {
        let remote = remote_with(Fake::echo(), no_app);
        let code = remote
            .begin_pairing("atelier.tail1234.ts.net")
            .unwrap()
            .code;
        remote.pairing.lock().unwrap().as_mut().unwrap().expires_at = super::super::now_ms() - 1;
        let answer = ask(
            &remote,
            post_to(
                "/m/api/pair",
                None,
                true,
                json!({ "code": code }).to_string(),
            ),
        )
        .await;
        assert_eq!(answer.status, StatusCode::GONE);
        assert!(remote.devices().is_empty());
    }

    #[tokio::test]
    async fn five_wrong_codes_and_the_right_one_no_longer_works() {
        let remote = remote_with(Fake::echo(), no_app);
        let code = remote
            .begin_pairing("atelier.tail1234.ts.net")
            .unwrap()
            .code;
        let wrong = if code == "000000" { "111111" } else { "000000" };
        for left in (0..5).rev() {
            let answer = ask(
                &remote,
                post_to(
                    "/m/api/pair",
                    None,
                    true,
                    json!({ "code": wrong }).to_string(),
                ),
            )
            .await;
            assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
            assert_eq!(answer.body, json!({ "error": "wrong-code", "left": left }));
        }
        let answer = ask(
            &remote,
            post_to(
                "/m/api/pair",
                None,
                true,
                json!({ "code": code }).to_string(),
            ),
        )
        .await;
        assert_eq!(answer.status, StatusCode::GONE);
        assert!(remote.devices().is_empty());
    }

    #[tokio::test]
    async fn a_pairing_that_is_not_json_is_a_bad_request_not_a_try() {
        let remote = remote_with(Fake::echo(), no_app);
        let code = remote
            .begin_pairing("atelier.tail1234.ts.net")
            .unwrap()
            .code;
        for body in [
            "{ nope",
            r#"{ "device": "Pixel" }"#,
            r#"{ "code": 123456 }"#,
        ] {
            let answer = ask(&remote, post_to("/m/api/pair", None, true, body)).await;
            assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{body}");
        }
        assert_eq!(remote.pairing.lock().unwrap().as_ref().unwrap().wrong, 0);
        let answer = ask(
            &remote,
            post_to(
                "/m/api/pair",
                None,
                true,
                json!({ "code": code }).to_string(),
            ),
        )
        .await;
        assert_eq!(answer.status, StatusCode::OK);
        assert_eq!(answer.body["device"]["name"], "Phone");
    }

    #[tokio::test]
    async fn every_other_route_needs_the_cookie() {
        let remote = remote_with(Fake::echo(), no_app);
        paired_phone(&remote).await;
        for request in [
            post_to("/m/api/call/state", None, true, "{}"),
            post_to("/m/api/call/state", Some("forged"), true, "{}"),
            post_to("/m/api/call/nope", None, true, "{}"),
            get_from("/m/api/shares", None),
            get_from("/m/files/abc", Some("")),
        ] {
            let path = request.uri().to_string();
            let answer = ask(&remote, request).await;
            assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{path}");
            assert_eq!(answer.body, json!({ "error": "unpaired" }), "{path}");
        }
    }

    #[tokio::test]
    async fn a_post_without_x_hangar_is_forbidden() {
        let remote = remote_with(Fake::echo(), no_app);
        let token = paired_phone(&remote).await;
        let code = remote
            .begin_pairing("atelier.tail1234.ts.net")
            .unwrap()
            .code;
        for request in [
            post_to("/m/api/call/state", Some(&token), false, "{}"),
            post_to(
                "/m/api/pair",
                None,
                false,
                json!({ "code": code }).to_string(),
            ),
            HttpRequest::post("/m/api/call/state")
                .header(header::COOKIE, format!("{COOKIE}={token}"))
                .header("x-hangar", "0")
                .body(Body::from("{}"))
                .unwrap(),
            HttpRequest::delete("/m/api/call/state")
                .header(header::COOKIE, format!("{COOKIE}={token}"))
                .body(Body::empty())
                .unwrap(),
        ] {
            let path = format!("{} {}", request.method(), request.uri());
            let answer = ask(&remote, request).await;
            assert_eq!(answer.status, StatusCode::FORBIDDEN, "{path}");
        }
        // The refused pairing spent nothing: the code still pairs.
        let answer = ask(
            &remote,
            post_to(
                "/m/api/pair",
                None,
                true,
                json!({ "code": code }).to_string(),
            ),
        )
        .await;
        assert_eq!(answer.status, StatusCode::OK);
    }

    #[tokio::test]
    async fn nothing_answers_cors() {
        let remote = remote_with(Fake::echo(), no_app);
        let token = paired_phone(&remote).await;
        let preflight = HttpRequest::builder()
            .method(Method::OPTIONS)
            .uri("/m/api/call/state")
            .header(header::ORIGIN, "https://elsewhere.example")
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
            .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "x-hangar")
            .body(Body::empty())
            .unwrap();
        let mut call = post_to("/m/api/call/state", Some(&token), true, "{}");
        call.headers_mut().insert(
            header::ORIGIN,
            HeaderValue::from_static("https://elsewhere.example"),
        );
        for request in [preflight, call, get_from("/m/api/health", None)] {
            let answer = ask(&remote, request).await;
            for name in answer.headers.keys() {
                assert!(!name.as_str().starts_with("access-control-"), "{name}");
            }
        }
    }

    #[tokio::test]
    async fn each_call_reaches_the_window_as_its_remote_op() {
        let window = Fake::echo();
        let remote = remote_with(window.clone(), no_app);
        let token = paired_phone(&remote).await;
        for op in CALLS {
            let answer = ask(
                &remote,
                post_to(
                    &format!("/m/api/call/{op}"),
                    Some(&token),
                    true,
                    json!({ "pane": "Ava", "text": "hello" }).to_string(),
                ),
            )
            .await;
            assert_eq!(answer.status, StatusCode::OK, "{op}");
            assert_eq!(
                answer.body,
                json!({ "op": format!("remote_{op}"), "args": { "pane": "Ava", "text": "hello" } })
            );
        }
        let empty = ask(
            &remote,
            post_to("/m/api/call/state", Some(&token), true, ""),
        )
        .await;
        assert_eq!(empty.body["args"], json!({}));
        assert_eq!(
            *window.asked.lock().unwrap(),
            [
                "remote_state",
                "remote_screen",
                "remote_send",
                "remote_permission",
                "remote_manager",
                "remote_history",
                "remote_state",
            ]
        );
    }

    #[tokio::test]
    async fn an_op_outside_the_list_never_reaches_the_window() {
        let window = Fake::echo();
        let remote = remote_with(window.clone(), no_app);
        let token = paired_phone(&remote).await;
        for op in [
            "pane_create",
            "orchestrator_briefing",
            "remote_state",
            "STATE",
            "state%20",
        ] {
            let answer = ask(
                &remote,
                post_to(&format!("/m/api/call/{op}"), Some(&token), true, "{}"),
            )
            .await;
            assert_eq!(answer.status, StatusCode::NOT_FOUND, "{op}");
            assert_eq!(answer.body, json!({ "error": "unknown-op" }), "{op}");
        }
        let elsewhere = ask(
            &remote,
            post_to("/m/api/control/pane_list", Some(&token), true, "{}"),
        )
        .await;
        assert_eq!(elsewhere.status, StatusCode::NOT_FOUND);
        // The agents' API is not here at all: its paths are only pages of the
        // web app, which has none by those names.
        let agents = ask(&remote, get_from("/api/board?cwd=/", Some(&token))).await;
        assert!(agents.body.as_str().unwrap().contains("Hangar Mobile"));
        assert!(window.asked.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_window_that_does_not_answer_in_time_is_a_busy_pc() {
        let remote = remote_with(Fake::silent(), no_app);
        let token = paired_phone(&remote).await;
        let answer = ask(
            &remote,
            post_to("/m/api/call/state", Some(&token), true, "{}"),
        )
        .await;
        assert_eq!(answer.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(answer.body, json!({ "error": "pc-busy" }));
    }

    #[tokio::test]
    async fn a_window_that_refuses_says_why() {
        let remote = remote_with(Fake::refusing("no pane named Zed"), no_app);
        let token = paired_phone(&remote).await;
        let answer = ask(
            &remote,
            post_to("/m/api/call/send", Some(&token), true, "{}"),
        )
        .await;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST);
        assert_eq!(
            answer.body,
            json!({ "error": "refused", "message": "no pane named Zed" })
        );
    }

    #[tokio::test]
    async fn a_call_s_body_is_capped_at_64_kb_and_must_be_json() {
        let window = Fake::echo();
        let remote = remote_with(window.clone(), no_app);
        let token = paired_phone(&remote).await;
        let fits = json!({ "text": "a".repeat(60 * 1024) }).to_string();
        let answer = ask(
            &remote,
            post_to("/m/api/call/send", Some(&token), true, fits),
        )
        .await;
        assert_eq!(answer.status, StatusCode::OK);
        let over = json!({ "text": "a".repeat(64 * 1024) }).to_string();
        let answer = ask(
            &remote,
            post_to("/m/api/call/send", Some(&token), true, over),
        )
        .await;
        assert_eq!(answer.status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(answer.body, json!({ "error": "too-large" }));
        let answer = ask(
            &remote,
            post_to("/m/api/call/send", Some(&token), true, "{ text"),
        )
        .await;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST);
        assert_eq!(window.asked.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_revoked_phone_is_shut_out_at_once() {
        let remote = remote_with(Fake::echo(), no_app);
        let token = paired_phone(&remote).await;
        let other = paired_phone(&remote).await;
        let call = || post_to("/m/api/call/state", Some(&token), true, "{}");
        assert_eq!(ask(&remote, call()).await.status, StatusCode::OK);

        let id = remote.store.find(&token, 0).unwrap().id;
        remote.revoke(&id).unwrap();

        let answer = ask(&remote, call()).await;
        assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
        assert_eq!(answer.body, json!({ "error": "unpaired" }));
        let health = ask(&remote, get_from("/m/api/health", Some(&token))).await;
        assert_eq!(health.body["paired"], false);
        // The other phone is untouched.
        let answer = ask(
            &remote,
            post_to("/m/api/call/state", Some(&other), true, "{}"),
        )
        .await;
        assert_eq!(answer.status, StatusCode::OK);
        assert_eq!(remote.devices().len(), 1);
    }

    #[tokio::test]
    async fn a_call_records_the_phone_s_visit() {
        let remote = remote_with(Fake::echo(), no_app);
        let token = paired_phone(&remote).await;
        remote
            .store
            .update(|saved| saved.devices[0].last_seen = 0)
            .unwrap();
        ask(
            &remote,
            post_to("/m/api/call/state", Some(&token), true, "{}"),
        )
        .await;
        assert!(remote.devices()[0].last_seen > 0);
    }

    #[tokio::test]
    async fn every_answer_is_hardened() {
        let remote = remote_with(Fake::echo(), small_app);
        for path in ["/", "/m/api/health", "/m/api/call/state"] {
            let answer = ask(&remote, get_from(path, None)).await;
            assert_eq!(
                answer.headers[header::X_CONTENT_TYPE_OPTIONS],
                "nosniff",
                "{path}"
            );
            assert_eq!(answer.headers[header::X_FRAME_OPTIONS], "DENY", "{path}");
            assert!(answer.headers.contains_key(header::CACHE_CONTROL), "{path}");
        }
        let api = ask(&remote, get_from("/m/api/health", None)).await;
        assert_eq!(api.headers[header::CACHE_CONTROL], "no-store");
    }

    #[test]
    fn the_cookie_is_read_among_others() {
        let mut headers = HeaderMap::new();
        headers.append(
            header::COOKIE,
            HeaderValue::from_static("a=1; hangar_device_old=x"),
        );
        assert_eq!(cookie_token(&headers), None);
        headers.append(
            header::COOKIE,
            HeaderValue::from_static("b=2;hangar_device=tok; c=3"),
        );
        assert_eq!(cookie_token(&headers), Some("tok"));
        assert_eq!(cookie_token(&HeaderMap::new()), None);
    }

    #[tokio::test]
    async fn the_listener_is_on_127_0_0_1_only() {
        let remote = remote_with(Fake::echo(), no_app);
        let running = listen(remote, test_port(0)).await.unwrap();
        assert_eq!(running.addr.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));

        let mut stream = tokio::net::TcpStream::connect(running.addr).await.unwrap();
        stream
            .write_all(b"GET /m/api/health HTTP/1.1\r\nHost: atelier\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).await.unwrap();
        assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
        assert!(reply.contains("\"ok\":true"), "{reply}");

        let addr = running.addr;
        running.stop();
        let mut closed = false;
        for _ in 0..50 {
            if tokio::net::TcpStream::connect(addr).await.is_err() {
                closed = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(closed, "still listening on {addr} after stop");
    }
}
