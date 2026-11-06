//! `/api/*` of the node API, relayed to core's own server (server.rs: the
//! board, the tasks, the memory, the bridge). That server listens on a port
//! of its own; relaying keeps the node to one port for the desktop to tunnel.
//! The desktop speaks with the node's token, and the request reaches core
//! with Hangar's own (`Caller::App`) in its place.
#![cfg(target_os = "linux")]

use axum::body::Body;
use axum::http::{header, HeaderValue, Request, Response, StatusCode};
use http_body_util::{BodyExt, Full};
use hyper_util::rt::TokioIo;

/// The largest body relayed: a task's description, a memory entry.
const BODY_LIMIT: usize = 16 * 1024 * 1024;

/// Where core's server listens, and the token it knows Hangar by.
#[derive(Clone)]
pub struct Core {
    pub port: u16,
    pub token: String,
}

/// The headers that belong to one hop, never relayed (RFC 9110, 7.6.1).
const HOP: &[header::HeaderName] = &[
    header::CONNECTION,
    header::TRANSFER_ENCODING,
    header::UPGRADE,
    header::TE,
    header::TRAILER,
    header::PROXY_AUTHORIZATION,
    header::PROXY_AUTHENTICATE,
];

pub async fn forward(core: &Core, request: Request<Body>) -> Response<Body> {
    match relay(core, request).await {
        Ok(response) => response,
        Err(err) => crate::api::error(
            StatusCode::BAD_GATEWAY,
            &format!("core's API did not answer: {err}"),
        ),
    }
}

async fn relay(core: &Core, request: Request<Body>) -> Result<Response<Body>, String> {
    let (mut parts, body) = request.into_parts();
    let body = axum::body::to_bytes(body, BODY_LIMIT)
        .await
        .map_err(|err| err.to_string())?;
    for name in HOP {
        parts.headers.remove(name);
    }
    let bearer =
        HeaderValue::from_str(&format!("Bearer {}", core.token)).map_err(|err| err.to_string())?;
    parts.headers.insert(header::AUTHORIZATION, bearer);
    let authority = format!("127.0.0.1:{}", core.port);
    parts.headers.insert(
        header::HOST,
        HeaderValue::from_str(&authority).map_err(|err| err.to_string())?,
    );
    // Origin form: the path and query, as the node was asked them.
    parts.uri = parts
        .uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/")
        .parse()
        .map_err(|err: axum::http::uri::InvalidUri| err.to_string())?;

    let stream = tokio::net::TcpStream::connect(("127.0.0.1", core.port))
        .await
        .map_err(|err| err.to_string())?;
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .map_err(|err| err.to_string())?;
    tokio::spawn(connection);
    let answer = sender
        .send_request(Request::from_parts(parts, Full::new(body)))
        .await
        .map_err(|err| err.to_string())?;
    let (mut parts, body) = answer.into_parts();
    let body = body
        .collect()
        .await
        .map_err(|err| err.to_string())?
        .to_bytes();
    for name in HOP {
        parts.headers.remove(name);
    }
    Ok(Response::from_parts(parts, Body::from(body)))
}
