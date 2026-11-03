//! A slot's listener (apps.rs): what Tailscale Serve sends to one of the
//! ports 8443–8448, relayed to the dev server the slot shows.
//!
//! - Nothing goes through without a paired phone's cookie: anything else
//!   gets a page saying to open the app from Hangar Mobile.
//! - A request reaches the dev server as one made on the PC: to
//!   127.0.0.1:<port> (or [::1]), with that for its Host, which Vite and
//!   Next check. The phone's cookie and `X-Hangar` are taken out of it: the
//!   dev server never sees the device's token.
//! - Bodies stream both ways, and a WebSocket — hot reload — is carried
//!   through once both sides have upgraded.
//! - What comes back is the dev server's own, headers and all, with nothing
//!   added. Only a cookie by the device's name stays out of it: set on the
//!   phone, it would replace its own and unpair it.

use std::future::IntoFuture;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, Version};
use axum::response::{IntoResponse, Response};
use axum::Router;
use hyper::upgrade::OnUpgrade;
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpSocket, TcpStream};
use tokio::sync::oneshot;

use super::apps::{Route, SWEEP_EVERY};
use super::server::COOKIE;
use super::{now_ms, Remote};

#[derive(Clone)]
struct Slot {
    remote: Arc<Remote>,
    index: usize,
}

pub fn router(remote: Arc<Remote>, index: usize) -> Router {
    Router::new()
        .fallback(relay)
        .with_state(Slot { remote, index })
}

/// The listener while it runs. Stopping it closes the port; what is in
/// flight finishes.
pub struct Running {
    port: u16,
    stop: oneshot::Sender<()>,
}

impl Running {
    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn stop(self) {
        let _ = self.stop.send(());
    }
}

/// The slot's listener, on 127.0.0.1 and nowhere else, as the mobile
/// server's is. While it runs it also frees, every minute, the slots gone
/// unused.
pub fn listen(remote: Arc<Remote>, index: usize, port: u16) -> io::Result<Running> {
    let socket = TcpSocket::new_v4()?;
    // As tokio's own bind does: a port just let go of is free again at once.
    #[cfg(unix)]
    socket.set_reuseaddr(true)?;
    socket.bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))?;
    let listener = socket.listen(1024)?;
    let port = listener.local_addr()?.port();
    let (stop, stopped) = oneshot::channel::<()>();
    let app = router(remote.clone(), index);
    tokio::spawn(async move {
        let serving = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .into_future();
        let sweeping = async {
            loop {
                tokio::time::sleep(SWEEP_EVERY).await;
                remote.apps.sweep(now_ms());
            }
        };
        tokio::select! {
            _ = serving => {}
            _ = sweeping => {}
        }
    });
    Ok(Running { port, stop })
}

async fn relay(State(slot): State<Slot>, request: Request) -> Response {
    if slot.remote.device(request.headers()).is_none() {
        return page(
            StatusCode::UNAUTHORIZED,
            "Open it from Hangar Mobile",
            "This is an app an agent is building on this PC. It opens from Hangar Mobile, \
             on a phone paired with the PC: tap the app there.",
        );
    }
    let Some(route) = slot.remote.apps.route(slot.index, now_ms()) else {
        return page(
            StatusCode::NOT_FOUND,
            "This app is closed",
            "It went unused for a while, or another app took its place. \
             Open it again from Hangar Mobile.",
        );
    };
    forward(route, request).await
}

/// One request, relayed over a connection of its own to the dev server, its
/// answer handed back as it comes.
async fn forward(route: Route, mut request: Request) -> Response {
    if request.method() == Method::CONNECT {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let upgrade = upgrade_asked(request.headers()).then(|| hyper::upgrade::on(&mut request));
    let (mut parts, body) = request.into_parts();
    // Its path only: the dev server is the one place it goes.
    parts.uri = parts
        .uri
        .path_and_query()
        .map_or_else(|| Uri::from_static("/"), |path| Uri::from(path.clone()));
    parts.version = Version::HTTP_11;
    as_made_on_the_pc(&mut parts.headers, route.target, upgrade.is_some());
    let request = Request::from_parts(parts, body);

    let Ok(stream) = TcpStream::connect(route.target).await else {
        return not_answering();
    };
    let _ = stream.set_nodelay(true);
    let Ok((mut sender, connection)) =
        hyper::client::conn::http1::handshake(TokioIo::new(stream)).await
    else {
        return not_answering();
    };
    tokio::spawn(async move {
        let _ = connection.with_upgrades().await;
    });
    let Ok(mut response) = sender.send_request(request).await else {
        return not_answering();
    };

    if response.status() == StatusCode::SWITCHING_PROTOCOLS {
        // A switch the phone did not ask for has nowhere to go.
        let Some(phone) = upgrade else {
            return not_answering();
        };
        let server = hyper::upgrade::on(&mut response);
        tokio::spawn(tunnel(phone, server, route));
        let (mut parts, _) = response.into_parts();
        keep_the_device_cookie(&mut parts.headers);
        return Response::from_parts(parts, Body::empty());
    }
    let (mut parts, body) = response.into_parts();
    drop_hop_by_hop(&mut parts.headers);
    keep_the_device_cookie(&mut parts.headers);
    Response::from_parts(parts, Body::new(body))
}

/// A socket carried through both ways until either side closes it, or the
/// slot closes under it.
async fn tunnel(phone: OnUpgrade, server: OnUpgrade, route: Route) {
    let (Ok(phone), Ok(server)) = tokio::join!(phone, server) else {
        return;
    };
    let mut phone = Touching {
        io: TokioIo::new(phone),
        last_used: route.last_used.clone(),
    };
    let mut server = Touching {
        io: TokioIo::new(server),
        last_used: route.last_used,
    };
    let mut closing = route.closing;
    tokio::select! {
        _ = tokio::io::copy_bidirectional(&mut phone, &mut server) => {}
        _ = closing.changed() => {}
    }
}

/// A socket's side whose every read counts as a use of the slot: hot reload
/// can go quiet for long, and is the app being used all the same.
struct Touching<T> {
    io: T,
    last_used: Arc<AtomicU64>,
}

impl<T: AsyncRead + Unpin> AsyncRead for Touching<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        let polled = Pin::new(&mut this.io).poll_read(cx, buf);
        if buf.filled().len() > before {
            this.last_used.fetch_max(now_ms(), Ordering::Relaxed);
        }
        polled
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for Touching<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().io).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().io).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().io).poll_shutdown(cx)
    }
}

/// The tokens of a header's values, `Connection: keep-alive, Upgrade`'s.
fn tokens(headers: &HeaderMap, name: HeaderName) -> impl Iterator<Item = &str> {
    headers
        .get_all(name)
        .into_iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
}

fn upgrade_asked(headers: &HeaderMap) -> bool {
    headers.contains_key(header::UPGRADE)
        && tokens(headers, header::CONNECTION).any(|token| token.eq_ignore_ascii_case("upgrade"))
}

/// Each connection's own (RFC 9110, 7.6.1), never passed along.
fn drop_hop_by_hop(headers: &mut HeaderMap) {
    let named: Vec<HeaderName> = tokens(headers, header::CONNECTION)
        .filter_map(|name| HeaderName::from_bytes(name.as_bytes()).ok())
        .collect();
    for name in named {
        headers.remove(name);
    }
    for name in [
        header::CONNECTION,
        HeaderName::from_static("keep-alive"),
        HeaderName::from_static("proxy-connection"),
        header::TE,
        header::TRAILER,
        header::TRANSFER_ENCODING,
        header::UPGRADE,
    ] {
        headers.remove(name);
    }
}

/// The request as the dev server expects one made on the PC: for its own
/// address, its connection's headers its own, and nothing of the phone's
/// credentials. The rest is left as the phone sent it.
fn as_made_on_the_pc(headers: &mut HeaderMap, target: SocketAddr, upgrade: bool) {
    let protocol = headers.get(header::UPGRADE).cloned();
    drop_hop_by_hop(headers);
    if let (true, Some(protocol)) = (upgrade, protocol) {
        headers.insert(header::CONNECTION, HeaderValue::from_static("upgrade"));
        headers.insert(header::UPGRADE, protocol);
    }
    headers.remove("x-hangar");
    without_the_device(headers);
    if let Ok(host) = HeaderValue::from_str(&target.to_string()) {
        headers.insert(header::HOST, host);
    }
}

/// The Cookie header less the phone's own: the host's other cookies are
/// the dev server's to read.
fn without_the_device(headers: &mut HeaderMap) {
    let kept: Vec<Vec<u8>> = headers
        .get_all(header::COOKIE)
        .iter()
        .flat_map(|value| value.as_bytes().split(|&byte| byte == b';'))
        .map(<[u8]>::trim_ascii)
        .filter(|pair| !pair.is_empty() && !names_the_device(pair))
        .map(<[u8]>::to_vec)
        .collect();
    headers.remove(header::COOKIE);
    if kept.is_empty() {
        return;
    }
    if let Ok(value) = HeaderValue::from_bytes(&kept.join(&b"; "[..])) {
        headers.insert(header::COOKIE, value);
    }
}

/// A cookie's `name=value`, or a Set-Cookie line, by the device's name.
fn names_the_device(cookie: &[u8]) -> bool {
    cookie
        .split(|&byte| byte == b'=')
        .next()
        .is_some_and(|name| name.trim_ascii() == COOKIE.as_bytes())
}

/// The dev server's cookies all reach the phone but one by the device's
/// name, which would replace the phone's own — the host's, whatever the
/// port — and unpair it.
fn keep_the_device_cookie(headers: &mut HeaderMap) {
    let cookies: Vec<HeaderValue> = headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter(|cookie| !names_the_device(cookie.as_bytes()))
        .cloned()
        .collect();
    headers.remove(header::SET_COOKIE);
    for cookie in cookies {
        headers.append(header::SET_COOKIE, cookie);
    }
}

fn not_answering() -> Response {
    page(
        StatusCode::BAD_GATEWAY,
        "The app is not answering",
        "Its dev server on this PC did not answer. It may have stopped: \
         once it runs again, reload this page.",
    )
}

/// Hangar's own few words in place of the app, kept out of every cache.
fn page(status: StatusCode, title: &str, text: &str) -> Response {
    let body = format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
body {{ margin: 0; min-height: 100vh; display: grid; place-items: center; background: #0f1115; color: #e6e6e6; font: 16px/1.5 system-ui, sans-serif; }}
main {{ max-width: 28rem; padding: 24px; }}
h1 {{ font-size: 1.25rem; }}
</style>
</head>
<body>
<main>
<h1>{title}</h1>
<p>{text}</p>
</main>
</body>
</html>
"#
    );
    (
        status,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (header::REFERRER_POLICY, "no-referrer"),
            (
                header::CONTENT_SECURITY_POLICY,
                "default-src 'none'; style-src 'unsafe-inline'",
            ),
        ],
        body,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::body::to_bytes;
    use axum::http::Request as HttpRequest;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tower::ServiceExt;

    use super::super::apps::tests::{
        dev_server, on_ports, pair, showing, window_listing, FakeTailnet, PC,
    };
    use super::super::apps::{self, IDLE_MS};
    use super::super::assets::tests::no_app;
    use super::super::tests::{remote_on, test_port};
    use super::*;

    /// Hangar Mobile with a phone paired and the stand-in dev server open on
    /// the slot: the Remote, the phone's token, the dev server's port and
    /// what it receives.
    async fn opened() -> (Arc<Remote>, String, u16, apps::tests::DevServer) {
        let (port, dev) = dev_server(test_port(9)).await;
        let remote = remote_on(window_listing(&[port]), no_app, FakeTailnet::connected());
        let token = pair(&remote);
        apps::open(&remote, "w1", port).await.unwrap();
        (remote, token, port, dev)
    }

    /// The slot's listener, as Tailscale Serve reaches it.
    async fn connect(remote: &Arc<Remote>) -> TcpStream {
        let port = remote.apps.slots().await[0].local_port;
        TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap()
    }

    /// Reads into `got` until `until` shows in it, or fails after 5 s.
    async fn read_until(stream: &mut TcpStream, got: &mut Vec<u8>, until: &str) {
        let reading = async {
            let mut buffer = [0u8; 16 * 1024];
            while !String::from_utf8_lossy(got).contains(until) {
                let read = stream.read(&mut buffer).await.unwrap();
                assert!(read > 0, "closed before {until:?}");
                got.extend_from_slice(&buffer[..read]);
            }
        };
        if tokio::time::timeout(Duration::from_secs(5), reading)
            .await
            .is_err()
        {
            panic!("{until:?} never came");
        }
    }

    #[test]
    fn a_request_reaches_the_dev_server_as_one_made_on_the_pc() {
        on_ports(|| async {
            let (remote, token, port, dev) = opened().await;
            let mut stream = connect(&remote).await;
            let request = format!(
                "POST /api/echo?x=1 HTTP/1.1\r\n\
                 Host: {PC}:8443\r\n\
                 Cookie: theme=dark; {COOKIE}={token}; lang=fr\r\n\
                 X-Hangar: 1\r\n\
                 Origin: https://{PC}:8443\r\n\
                 Content-Type: text/plain\r\n\
                 Content-Length: 5\r\n\
                 Connection: close\r\n\r\nhello"
            );
            stream.write_all(request.as_bytes()).await.unwrap();
            let mut reply = String::new();
            stream.read_to_string(&mut reply).await.unwrap();
            let (head, body) = reply.split_once("\r\n\r\n").unwrap();
            assert!(head.starts_with("HTTP/1.1 200"), "{head}");
            assert_eq!(body, "hello");

            // The dev server's headers as it sent them, nothing added…
            let head = head.to_ascii_lowercase();
            for kept in [
                "content-security-policy: default-src 'self'",
                "x-frame-options: sameorigin",
                "set-cookie: theme=dark; path=/",
            ] {
                assert!(head.contains(kept), "{kept} missing from {head}");
            }
            for added in [
                "x-content-type-options",
                "cache-control",
                "referrer-policy",
                "access-control-",
            ] {
                assert!(!head.contains(added), "{added} in {head}");
            }
            // …but for the cookie that would unpair the phone.
            assert!(!head.contains(COOKIE), "{head}");

            let seen = dev.seen.lock().unwrap().clone();
            assert_eq!(seen.len(), 1);
            let seen = &seen[0];
            assert_eq!(
                (seen.method.as_str(), seen.uri.as_str()),
                ("POST", "/api/echo?x=1")
            );
            assert_eq!(seen.body, b"hello");
            assert_eq!(
                seen.headers[header::HOST],
                format!("127.0.0.1:{port}").as_str()
            );
            assert_eq!(seen.headers[header::COOKIE], "theme=dark; lang=fr");
            assert!(seen.headers.get("x-hangar").is_none());
            assert!(!format!("{:?}", seen.headers).contains(&token));
            // The rest as the phone sent it.
            assert_eq!(
                seen.headers[header::ORIGIN],
                format!("https://{PC}:8443").as_str()
            );
            assert_eq!(seen.headers[header::CONTENT_TYPE], "text/plain");
        });
    }

    #[test]
    fn bodies_stream_through_as_they_are_sent() {
        on_ports(|| async {
            let (remote, token, _, dev) = opened().await;
            let mut stream = connect(&remote).await;
            let request = format!(
                "GET /stream HTTP/1.1\r\nHost: {PC}:8443\r\nCookie: {COOKIE}={token}\r\n\r\n"
            );
            stream.write_all(request.as_bytes()).await.unwrap();
            let mut trickle = None;
            for _ in 0..500 {
                trickle = dev.trickle.lock().unwrap().take();
                if trickle.is_some() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let trickle = trickle.expect("the dev server never got the request");

            // The first chunk reaches the phone while the second is not
            // even written: nothing waits for the whole body.
            let mut got = Vec::new();
            trickle.send("first chunk").await.unwrap();
            read_until(&mut stream, &mut got, "first chunk").await;
            trickle.send("second chunk").await.unwrap();
            drop(trickle);
            read_until(&mut stream, &mut got, "second chunk").await;

            // An upload past any buffer, and any limit, goes through whole.
            let big = vec![b'x'; 3 * 1024 * 1024];
            let mut stream = connect(&remote).await;
            let head = format!(
                "PUT /upload HTTP/1.1\r\nHost: {PC}:8443\r\nCookie: {COOKIE}={token}\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n",
                big.len()
            );
            stream.write_all(head.as_bytes()).await.unwrap();
            stream.write_all(&big).await.unwrap();
            let mut reply = Vec::new();
            stream.read_to_end(&mut reply).await.unwrap();
            assert!(reply.starts_with(b"HTTP/1.1 200"));
            let seen = dev.seen.lock().unwrap();
            assert_eq!(seen.last().unwrap().body.len(), big.len());
        });
    }

    #[test]
    fn a_websocket_goes_through_both_ways_until_the_slot_closes() {
        on_ports(|| async {
            let (remote, token, port, dev) = opened().await;
            let mut socket = connect(&remote).await;
            let request = format!(
                "GET /hmr?token=vite HTTP/1.1\r\n\
                 Host: {PC}:8443\r\n\
                 Connection: Upgrade\r\n\
                 Upgrade: websocket\r\n\
                 Sec-WebSocket-Version: 13\r\n\
                 Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
                 Sec-WebSocket-Protocol: vite-hmr\r\n\
                 Cookie: {COOKIE}={token}\r\n\
                 X-Hangar: 1\r\n\r\n"
            );
            socket.write_all(request.as_bytes()).await.unwrap();
            let mut head = Vec::new();
            read_until(&mut socket, &mut head, "\r\n\r\n").await;
            let head = String::from_utf8_lossy(&head).to_ascii_lowercase();
            assert!(head.starts_with("http/1.1 101"), "{head}");
            assert!(head.contains("upgrade: websocket"), "{head}");
            assert!(
                head.contains("sec-websocket-accept: s3pplmbitxaq9kygzzhzrbk+xoo="),
                "{head}"
            );

            let seen = dev.seen.lock().unwrap()[0].clone();
            assert_eq!(seen.uri, "/hmr?token=vite");
            assert_eq!(
                seen.headers[header::HOST],
                format!("127.0.0.1:{port}").as_str()
            );
            assert_eq!(seen.headers[header::CONNECTION], "upgrade");
            assert_eq!(seen.headers[header::UPGRADE], "websocket");
            assert_eq!(seen.headers["sec-websocket-protocol"], "vite-hmr");
            assert!(seen.headers.get(header::COOKIE).is_none());
            assert!(seen.headers.get("x-hangar").is_none());

            // Byte for byte both ways, each byte a use of the slot.
            tokio::time::sleep(Duration::from_millis(20)).await;
            let before = now_ms();
            socket.write_all(b"ping from the phone").await.unwrap();
            let mut echoed = Vec::new();
            read_until(&mut socket, &mut echoed, "ping from the phone").await;
            let used = remote.apps.slots().await[0].app.clone().unwrap().last_used;
            assert!(used >= before, "{used} < {before}");

            // The slot freed, its socket closes.
            remote.apps.sweep(used + IDLE_MS);
            let mut rest = Vec::new();
            let closed =
                tokio::time::timeout(Duration::from_secs(5), socket.read_to_end(&mut rest)).await;
            assert!(closed.is_ok(), "still open after the slot was freed");
        });
    }

    fn request(method: Method, cookie: Option<&str>, upgrade: bool) -> HttpRequest<Body> {
        let mut request = HttpRequest::builder()
            .method(method)
            .uri("/")
            .header(header::HOST, format!("{PC}:8443"));
        if let Some(token) = cookie {
            request = request.header(header::COOKIE, format!("{COOKIE}={token}"));
        }
        if upgrade {
            request = request
                .header(header::CONNECTION, "upgrade")
                .header(header::UPGRADE, "websocket");
        }
        request.body(Body::empty()).unwrap()
    }

    async fn text(response: Response) -> String {
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    #[test]
    fn nothing_goes_through_without_a_paired_phone_s_cookie() {
        on_ports(|| async {
            let (remote, token, _, dev) = opened().await;
            let revoked = pair(&remote);
            let id = remote.store.find(&revoked, 0).unwrap().id;
            remote.revoke(&id).unwrap();

            for cookie in [None, Some(""), Some("forged"), Some(revoked.as_str())] {
                for (method, upgrade) in [
                    (Method::GET, false),
                    (Method::GET, true),
                    (Method::POST, false),
                ] {
                    let response = router(remote.clone(), 0)
                        .oneshot(request(method.clone(), cookie, upgrade))
                        .await
                        .unwrap();
                    assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{cookie:?}");
                    assert_eq!(
                        response.headers()[header::CONTENT_TYPE],
                        "text/html; charset=utf-8"
                    );
                    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
                    assert!(text(response).await.contains("Hangar Mobile"));
                }
            }
            assert!(dev.seen.lock().unwrap().is_empty());

            // The paired phone's goes through.
            let response = router(remote.clone(), 0)
                .oneshot(request(Method::GET, Some(&token), false))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(dev.seen.lock().unwrap().len(), 1);
        });
    }

    #[tokio::test]
    async fn a_slot_that_shows_nothing_relays_nothing() {
        let remote = remote_on(window_listing(&[]), no_app, FakeTailnet::connected());
        let token = pair(&remote);
        for slot in [0, 1, 9] {
            let response = router(remote.clone(), slot)
                .oneshot(request(Method::GET, Some(&token), false))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            assert!(text(response)
                .await
                .contains("Open it again from Hangar Mobile"));
        }
    }

    #[test]
    fn a_dev_server_gone_is_a_page_saying_so() {
        on_ports(|| async {
            let remote = remote_on(window_listing(&[]), no_app, FakeTailnet::connected());
            let token = pair(&remote);
            // Nothing listens on the share's port 9 outside these tests.
            showing(&remote.apps, test_port(9));
            let response = router(remote.clone(), 0)
                .oneshot(request(Method::GET, Some(&token), false))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
            assert!(text(response).await.contains("not answering"));
            let response = router(remote, 0)
                .oneshot(request(Method::CONNECT, Some(&token), false))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        });
    }

    #[test]
    fn only_the_device_s_cookie_is_taken_out() {
        let mut headers = HeaderMap::new();
        headers.append(
            header::COOKIE,
            HeaderValue::from_static("a=1; hangar_device=tok;hangar_device_old=2"),
        );
        headers.append(
            header::COOKIE,
            HeaderValue::from_static(" hangar_device = x "),
        );
        without_the_device(&mut headers);
        assert_eq!(headers[header::COOKIE], "a=1; hangar_device_old=2");

        let mut alone = HeaderMap::new();
        alone.insert(
            header::COOKIE,
            HeaderValue::from_static("hangar_device=tok"),
        );
        without_the_device(&mut alone);
        assert!(alone.get(header::COOKIE).is_none());

        let mut response = HeaderMap::new();
        for cookie in [
            "hangar_device=x; Path=/m/",
            "session=1; HttpOnly",
            "hangar_devices=2",
        ] {
            response.append(header::SET_COOKIE, HeaderValue::from_static(cookie));
        }
        keep_the_device_cookie(&mut response);
        let set: Vec<&str> = response
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|cookie| cookie.to_str().unwrap())
            .collect();
        assert_eq!(set, ["session=1; HttpOnly", "hangar_devices=2"]);
    }

    #[test]
    fn a_connection_s_own_headers_stay_with_it() {
        let mut headers = HeaderMap::new();
        for (name, value) in [
            ("connection", "keep-alive, x-trace"),
            ("keep-alive", "timeout=5"),
            ("x-trace", "1"),
            ("transfer-encoding", "chunked"),
            ("upgrade", "websocket"),
            ("accept", "text/html"),
        ] {
            headers.insert(name, HeaderValue::from_static(value));
        }
        let target = SocketAddr::from((Ipv4Addr::LOCALHOST, 5173));
        let mut plain = headers.clone();
        as_made_on_the_pc(&mut plain, target, false);
        let mut names: Vec<&str> = plain.keys().map(|name| name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["accept", "host"]);
        assert_eq!(plain[header::HOST], "127.0.0.1:5173");

        // An upgrade keeps what makes it one.
        headers.insert("connection", HeaderValue::from_static("Upgrade"));
        let mut upgrade = headers;
        as_made_on_the_pc(&mut upgrade, "[::1]:5173".parse().unwrap(), true);
        assert_eq!(upgrade[header::CONNECTION], "upgrade");
        assert_eq!(upgrade[header::UPGRADE], "websocket");
        assert_eq!(upgrade[header::HOST], "[::1]:5173");
        assert!(upgrade.get("keep-alive").is_none());
    }
}
