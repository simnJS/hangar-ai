//! HTTP/1.1 and WebSocket clients for a node's API, by hand: the app has no
//! HTTP nor WebSocket client, and what it says to a node goes to a loopback
//! port (the SSH tunnel's end) and back, with no TLS, no proxy and no
//! redirect to follow. `runner::http_get` is the blocking one the connection
//! asks `/node/info` with; these are the async ones the remote workspaces
//! use (remote.rs, the board).
//!
//! The WebSocket half is RFC 6455's client side, what the node's axum server
//! speaks: frames from the client masked, text and binary messages,
//! fragments joined, pings answered. The handshake's `Sec-WebSocket-Accept`
//! is not checked: the app has no SHA-1, and the peer is the node behind its
//! token and the tunnel, not a cache that could answer for it.

use std::time::Duration;

use base64::Engine as _;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::model::NodeClient;
use super::runner::dechunk;

/// How long a request, or the WebSocket handshake, may take.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// The largest message taken from a node: a pane's history comes in chunks
/// far below it.
const MAX_MESSAGE: usize = 16 * 1024 * 1024;

/// Room made in a socket's buffer before each read: a flood comes in frames
/// of up to 64 KiB (hangar-node's `panes::BATCH_BYTES`).
const READ_ROOM: usize = 128 * 1024;

/// A connection to the node: a key's echo, a few bytes, is sent at once
/// rather than held back by Nagle's algorithm until the last segment is
/// acknowledged.
async fn connect(host: &str) -> std::io::Result<TcpStream> {
    let stream = TcpStream::connect(host).await?;
    stream.set_nodelay(true)?;
    Ok(stream)
}

/// An answer of the node, whatever its status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub status: u16,
    pub body: String,
}

impl Answer {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// The body as JSON, when the status is a success; the node's
    /// `{"error"}` (or the status) otherwise.
    pub fn json(&self) -> Result<Value, String> {
        if !self.ok() {
            return Err(self.error());
        }
        serde_json::from_str(&self.body).map_err(|err| err.to_string())
    }

    /// What a failed answer says.
    pub fn error(&self) -> String {
        serde_json::from_str::<Value>(&self.body)
            .ok()
            .and_then(|body| body.get("error")?.as_str().map(str::to_owned))
            .unwrap_or_else(|| format!("HTTP {}", self.status))
    }
}

/// `host:port` of the client's `http://host:port`.
fn authority(client: &NodeClient) -> Result<&str, String> {
    let rest = client
        .base_url
        .strip_prefix("http://")
        .ok_or_else(|| format!("not an http address: {}", client.base_url))?;
    Ok(rest.trim_end_matches('/'))
}

/// A path segment or query value, percent-encoded but for the characters
/// RFC 3986 leaves unreserved.
pub fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// `method path` on the node, with its token and, when given, a JSON body.
pub async fn request(
    client: &NodeClient,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> Result<Answer, String> {
    request_within(client, method, path, body, REQUEST_TIMEOUT).await
}

/// [`request`], given `limit` to answer: what the node does at length (a
/// worktree's checkout, a delivery's check and review).
pub async fn request_within(
    client: &NodeClient,
    method: &str,
    path: &str,
    body: Option<&Value>,
    limit: Duration,
) -> Result<Answer, String> {
    tokio::time::timeout(limit, exchange(client, method, path, body))
        .await
        .map_err(|_| format!("{method} {path}: no answer"))?
}

async fn exchange(
    client: &NodeClient,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> Result<Answer, String> {
    let host = authority(client)?;
    let mut stream = connect(host).await.map_err(|err| err.to_string())?;
    let payload = body.map(Value::to_string).unwrap_or_default();
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {}\r\n\
         Accept: application/json\r\nConnection: close\r\n",
        client.token
    );
    if body.is_some() {
        head.push_str("Content-Type: application/json\r\n");
    }
    head.push_str(&format!("Content-Length: {}\r\n\r\n", payload.len()));
    // One write: the head and the body in one segment.
    head.push_str(&payload);
    stream
        .write_all(head.as_bytes())
        .await
        .map_err(|err| err.to_string())?;
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .await
        .map_err(|err| err.to_string())?;
    parse_answer(&raw)
}

/// The status and body of an HTTP/1.1 answer read to its end.
pub fn parse_answer(raw: &[u8]) -> Result<Answer, String> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| "no HTTP answer".to_string())?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let mut lines = head.lines();
    let status = status_of(lines.next().unwrap_or_default())?;
    let chunked = lines.any(|line| {
        let lower = line.to_ascii_lowercase();
        lower.starts_with("transfer-encoding:") && lower.contains("chunked")
    });
    let body = &raw[split + 4..];
    let body = if chunked {
        dechunk(body)?
    } else {
        body.to_vec()
    };
    Ok(Answer {
        status,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

fn status_of(line: &str) -> Result<u16, String> {
    line.split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| format!("not an HTTP answer: {line}"))
}

/// What a WebSocket could not do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsError {
    /// The node answered the upgrade with this status (404: no such pane).
    Refused(u16, String),
    /// Anything else: the tunnel down, a broken frame.
    Io(String),
}

impl std::fmt::Display for WsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WsError::Refused(status, why) => write!(f, "HTTP {status}: {why}"),
            WsError::Io(why) => f.write_str(why),
        }
    }
}

fn io(err: impl std::fmt::Display) -> WsError {
    WsError::Io(err.to_string())
}

/// A whole message the node sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incoming {
    Text(String),
    Binary(Vec<u8>),
    Close,
}

/// A WebSocket to the node.
#[derive(Debug)]
pub struct Socket {
    stream: TcpStream,
    /// What was read; the frames before `at` are parsed already (dropped at
    /// the next read, so a buffer of many frames is not shifted once each).
    buf: Vec<u8>,
    at: usize,
    /// A message whose fragments are still coming: its opcode and bytes.
    partial: Option<(u8, Vec<u8>)>,
    /// Pongs owed, sent by [`Socket::flush`].
    pongs: Vec<Vec<u8>>,
}

impl Socket {
    /// Opens `path` on the node as a WebSocket.
    pub async fn connect(client: &NodeClient, path: &str) -> Result<Socket, WsError> {
        Self::connect_offering(client, path, None).await
    }

    /// [`Socket::connect`], offering subprotocol `protocol`: a node that
    /// does not know it picks none, and the socket opens all the same.
    pub async fn connect_offering(
        client: &NodeClient,
        path: &str,
        protocol: Option<&str>,
    ) -> Result<Socket, WsError> {
        tokio::time::timeout(REQUEST_TIMEOUT, Self::handshake(client, path, protocol))
            .await
            .map_err(|_| WsError::Io(format!("{path}: no answer")))?
    }

    async fn handshake(
        client: &NodeClient,
        path: &str,
        protocol: Option<&str>,
    ) -> Result<Socket, WsError> {
        let host = authority(client).map_err(WsError::Io)?;
        let mut stream = connect(host).await.map_err(io)?;
        let key = base64::engine::general_purpose::STANDARD.encode(uuid::Uuid::new_v4().as_bytes());
        let offer = protocol
            .map(|protocol| format!("Sec-WebSocket-Protocol: {protocol}\r\n"))
            .unwrap_or_default();
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {}\r\n\
             Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\n\
             Sec-WebSocket-Version: 13\r\n{offer}\r\n",
            client.token
        );
        stream.write_all(request.as_bytes()).await.map_err(io)?;
        let mut buf = Vec::new();
        let end = loop {
            if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break end;
            }
            if buf.len() > 64 * 1024 {
                return Err(WsError::Io("an answer with no end to its head".into()));
            }
            let read = stream.read_buf(&mut buf).await.map_err(io)?;
            if read == 0 {
                return Err(WsError::Io("closed during the handshake".into()));
            }
        };
        let head = String::from_utf8_lossy(&buf[..end]).into_owned();
        let status = status_of(head.lines().next().unwrap_or_default()).map_err(WsError::Io)?;
        if status != 101 {
            // The rest of a refusal is its body: read what came, to say why.
            let mut rest = buf[end + 4..].to_vec();
            let _ = tokio::time::timeout(Duration::from_millis(500), stream.read_to_end(&mut rest))
                .await;
            let mut raw = buf[..end + 4].to_vec();
            raw.extend(rest);
            let why = parse_answer(&raw)
                .map(|answer| answer.error())
                .unwrap_or_else(|_| format!("HTTP {status}"));
            return Err(WsError::Refused(status, why));
        }
        Ok(Socket {
            stream,
            buf: buf[end + 4..].to_vec(),
            at: 0,
            partial: None,
            pongs: Vec::new(),
        })
    }

    /// Reads what the node sent next into the buffer: safe to cancel (it is
    /// one `read_buf`), so it goes into a `select!` beside the sending side.
    /// `false` once the node closed the connection.
    pub async fn fill(&mut self) -> Result<bool, WsError> {
        // Dropping what was parsed and making room cancels nothing: the
        // bytes not parsed stay, at the front.
        self.buf.drain(..self.at);
        self.at = 0;
        self.buf.reserve(READ_ROOM);
        let read = self.stream.read_buf(&mut self.buf).await.map_err(io)?;
        Ok(read > 0)
    }

    /// The next whole message already read, if there is one. Pings are
    /// answered at the next [`Socket::flush`].
    pub fn next(&mut self) -> Result<Option<Incoming>, WsError> {
        loop {
            let Some(Frame {
                fin,
                opcode,
                payload,
                used,
            }) = frame_at(&self.buf[self.at..])?
            else {
                return Ok(None);
            };
            self.at += used;
            match opcode {
                0x8 => return Ok(Some(Incoming::Close)),
                0x9 => self.pongs.push(payload),
                0xA => {}
                0x0 => {
                    let Some((first, mut bytes)) = self.partial.take() else {
                        return Err(WsError::Io("a continuation of nothing".into()));
                    };
                    bytes.extend(payload);
                    if bytes.len() > MAX_MESSAGE {
                        return Err(WsError::Io("a message too large".into()));
                    }
                    if fin {
                        return message(first, bytes).map(Some);
                    }
                    self.partial = Some((first, bytes));
                }
                0x1 | 0x2 if !fin => self.partial = Some((opcode, payload)),
                0x1 | 0x2 => return message(opcode, payload).map(Some),
                other => return Err(WsError::Io(format!("an unknown opcode {other}"))),
            }
        }
    }

    /// Sends the pongs owed.
    pub async fn flush(&mut self) -> Result<(), WsError> {
        for payload in std::mem::take(&mut self.pongs) {
            self.send(0xA, &payload).await?;
        }
        Ok(())
    }

    pub async fn send_text(&mut self, text: &str) -> Result<(), WsError> {
        self.send(0x1, text.as_bytes()).await
    }

    /// Says goodbye, when the node still listens.
    pub async fn close(&mut self) {
        let _ = self.send(0x8, &[]).await;
    }

    async fn send(&mut self, opcode: u8, payload: &[u8]) -> Result<(), WsError> {
        let frame = client_frame(opcode, payload, mask());
        self.stream.write_all(&frame).await.map_err(io)
    }
}

fn message(opcode: u8, bytes: Vec<u8>) -> Result<Incoming, WsError> {
    if opcode == 0x1 {
        String::from_utf8(bytes)
            .map(Incoming::Text)
            .map_err(|_| WsError::Io("a text message that is not UTF-8".into()))
    } else {
        Ok(Incoming::Binary(bytes))
    }
}

fn mask() -> [u8; 4] {
    let bytes = *uuid::Uuid::new_v4().as_bytes();
    [bytes[0], bytes[1], bytes[2], bytes[3]]
}

/// A frame as a client sends it: final, masked with `key`.
pub fn client_frame(opcode: u8, payload: &[u8], key: [u8; 4]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(payload.len() + 14);
    frame.push(0x80 | opcode);
    let len = payload.len();
    if len < 126 {
        frame.push(0x80 | len as u8);
    } else if len <= usize::from(u16::MAX) {
        frame.push(0x80 | 126);
        frame.extend((len as u16).to_be_bytes());
    } else {
        frame.push(0x80 | 127);
        frame.extend((len as u64).to_be_bytes());
    }
    frame.extend(key);
    frame.extend(
        payload
            .iter()
            .enumerate()
            .map(|(i, byte)| byte ^ key[i % 4]),
    );
    frame
}

/// One frame off the front of `buf`, when it is all there: whether it is
/// final, its opcode and its payload, unmasked. (The tests' fake node reads
/// the desktop's frames with it.)
#[cfg(test)]
pub fn parse_frame(buf: &mut Vec<u8>) -> Result<Option<(bool, u8, Vec<u8>)>, WsError> {
    let Some(frame) = frame_at(buf)? else {
        return Ok(None);
    };
    buf.drain(..frame.used);
    Ok(Some((frame.fin, frame.opcode, frame.payload)))
}

/// A frame read off a buffer.
struct Frame {
    fin: bool,
    opcode: u8,
    /// Unmasked.
    payload: Vec<u8>,
    /// The bytes it took in the buffer.
    used: usize,
}

/// The frame at the front of `buf`, when it is all there.
fn frame_at(buf: &[u8]) -> Result<Option<Frame>, WsError> {
    if buf.len() < 2 {
        return Ok(None);
    }
    let fin = buf[0] & 0x80 != 0;
    let opcode = buf[0] & 0x0F;
    let masked = buf[1] & 0x80 != 0;
    let (len, mut at) = match buf[1] & 0x7F {
        126 => {
            if buf.len() < 4 {
                return Ok(None);
            }
            (usize::from(u16::from_be_bytes([buf[2], buf[3]])), 4)
        }
        127 => {
            if buf.len() < 10 {
                return Ok(None);
            }
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&buf[2..10]);
            let len = u64::from_be_bytes(bytes);
            (usize::try_from(len).unwrap_or(usize::MAX), 10)
        }
        len => (usize::from(len), 2),
    };
    if len > MAX_MESSAGE {
        return Err(WsError::Io("a frame too large".into()));
    }
    let key = if masked {
        if buf.len() < at + 4 {
            return Ok(None);
        }
        let key = [buf[at], buf[at + 1], buf[at + 2], buf[at + 3]];
        at += 4;
        Some(key)
    } else {
        None
    };
    if buf.len() < at + len {
        return Ok(None);
    }
    let mut payload: Vec<u8> = buf[at..at + len].to_vec();
    if let Some(key) = key {
        for (i, byte) in payload.iter_mut().enumerate() {
            *byte ^= key[i % 4];
        }
    }
    Ok(Some(Frame {
        fin,
        opcode,
        payload,
        used: at + len,
    }))
}
