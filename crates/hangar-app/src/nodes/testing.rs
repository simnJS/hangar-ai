//! A fake node for the tests of what talks to one (the remote panes, the
//! board of a remote workspace): a loopback listener, and the server side of
//! HTTP and WebSocket written as plainly as the client side is.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::model::NodeClient;
use super::wire::parse_frame;

/// The token the fake node asks for.
pub const TOKEN: &str = "test-token";

/// A listener on a port of this worker's range (`HANGAR_PORT_BASE`, the
/// first one free), or any when there is no range, and the client of it.
pub async fn listen() -> (TcpListener, NodeClient) {
    let base = std::env::var("HANGAR_PORT_BASE")
        .ok()
        .and_then(|base| base.parse::<u16>().ok());
    let mut listener = None;
    if let Some(base) = base {
        for port in base..base.saturating_add(20) {
            if let Ok(bound) = TcpListener::bind(("127.0.0.1", port)).await {
                listener = Some(bound);
                break;
            }
        }
    }
    let listener = match listener {
        Some(listener) => listener,
        None => TcpListener::bind(("127.0.0.1", 0)).await.expect("a port"),
    };
    let port = listener.local_addr().expect("an address").port();
    let client = NodeClient {
        base_url: format!("http://127.0.0.1:{port}"),
        token: TOKEN.into(),
    };
    (listener, client)
}

/// A request read: its head (request line and headers) and its body.
pub struct Request {
    pub head: String,
    pub body: String,
}

impl Request {
    /// `METHOD /path?query`.
    pub fn line(&self) -> &str {
        self.head.lines().next().unwrap_or_default()
    }
}

/// Reads one request off `stream`, its body as long as it says.
pub async fn read_request(stream: &mut TcpStream) -> Request {
    let mut raw = Vec::new();
    let end = loop {
        if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break end;
        }
        let mut buf = [0u8; 1024];
        let read = stream.read(&mut buf).await.expect("a request");
        assert!(read > 0, "the client closed before its request ended");
        raw.extend_from_slice(&buf[..read]);
    };
    let head = String::from_utf8_lossy(&raw[..end]).into_owned();
    let length = head
        .lines()
        .find_map(|line| {
            let lower = line.to_ascii_lowercase();
            lower
                .strip_prefix("content-length:")
                .and_then(|value| value.trim().parse::<usize>().ok())
        })
        .unwrap_or(0);
    let mut body = raw[end + 4..].to_vec();
    while body.len() < length {
        let mut buf = [0u8; 1024];
        let read = stream.read(&mut buf).await.expect("a body");
        assert!(read > 0, "the client closed before its body ended");
        body.extend_from_slice(&buf[..read]);
    }
    Request {
        head,
        body: String::from_utf8_lossy(&body).into_owned(),
    }
}

/// Answers `status` with a JSON `body`, and closes.
pub async fn answer(stream: &mut TcpStream, status: u16, body: &str) {
    let text = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(text.as_bytes()).await.expect("an answer");
    let _ = stream.shutdown().await;
}

/// Accepts a WebSocket: its upgrade request, answered `101`.
pub async fn accept_ws(listener: &TcpListener) -> (TcpStream, Request) {
    let (mut stream, _) = listener.accept().await.expect("a connection");
    let request = read_request(&mut stream).await;
    assert!(
        request
            .head
            .contains(&format!("Authorization: Bearer {TOKEN}")),
        "the token is sent"
    );
    stream
        .write_all(
            b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\
              Connection: Upgrade\r\nSec-WebSocket-Accept: x\r\n\r\n",
        )
        .await
        .expect("an upgrade");
    (stream, request)
}

/// A text frame as a server sends it: unmasked.
pub async fn send_text(stream: &mut TcpStream, text: &str) {
    let payload = text.as_bytes();
    let mut frame = vec![0x81];
    if payload.len() < 126 {
        frame.push(payload.len() as u8);
    } else {
        frame.push(126);
        frame.extend((payload.len() as u16).to_be_bytes());
    }
    frame.extend_from_slice(payload);
    stream.write_all(&frame).await.expect("a frame");
}

/// The next text frame the client sent (masked, as a client's must be).
pub async fn read_text(stream: &mut TcpStream, buf: &mut Vec<u8>) -> String {
    loop {
        if let Some((_, opcode, payload)) = parse_frame(buf).expect("a frame") {
            if opcode == 0x1 {
                return String::from_utf8(payload).expect("UTF-8");
            }
            continue;
        }
        let mut chunk = [0u8; 1024];
        let read = stream.read(&mut chunk).await.expect("a frame");
        assert!(read > 0, "the client closed");
        buf.extend_from_slice(&chunk[..read]);
    }
}
