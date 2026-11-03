//! A release server for the tests: a feed and artifacts over a real socket, so
//! the HTTP client, redirects, progress and cancellation are exercised for
//! real rather than through a stand-in.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Clone)]
pub struct Reply {
    status: u16,
    location: Option<String>,
    body: Vec<u8>,
    /// Write the body this many bytes at a time, pausing between writes.
    slowly: Option<(usize, Duration)>,
    /// Promise the whole body, send only this much, and hang up.
    truncated_to: Option<usize>,
}

impl Reply {
    pub fn ok(body: Vec<u8>) -> Self {
        Self {
            status: 200,
            location: None,
            body,
            slowly: None,
            truncated_to: None,
        }
    }

    pub fn status(status: u16) -> Self {
        Self {
            status,
            ..Self::ok(Vec::new())
        }
    }

    pub fn redirect(location: &str) -> Self {
        Self {
            status: 302,
            location: Some(location.to_string()),
            ..Self::ok(Vec::new())
        }
    }

    pub fn slowly(mut self, chunk: usize, pause: Duration) -> Self {
        self.slowly = Some((chunk, pause));
        self
    }

    pub fn truncated_to(mut self, bytes: usize) -> Self {
        self.truncated_to = Some(bytes);
        self
    }
}

pub struct Server {
    address: SocketAddr,
    routes: Arc<Mutex<HashMap<String, Reply>>>,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
}

impl Server {
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let routes: Arc<Mutex<HashMap<String, Reply>>> = Arc::default();
        let requests: Arc<Mutex<Vec<String>>> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));

        let (accept_routes, accept_requests, accept_stop) =
            (routes.clone(), requests.clone(), stop.clone());
        thread::spawn(move || {
            for stream in listener.incoming() {
                if accept_stop.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let (routes, requests) = (accept_routes.clone(), accept_requests.clone());
                thread::spawn(move || serve(stream, &routes, &requests));
            }
        });

        Self {
            address,
            routes,
            requests,
            stop,
        }
    }

    /// `http://127.0.0.1:port`, without a trailing slash.
    pub fn base(&self) -> String {
        format!("http://{}", self.address)
    }

    pub fn reply(&self, path: &str, reply: Reply) {
        self.routes.lock().unwrap().insert(path.to_string(), reply);
    }

    /// The paths requested so far, in order.
    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Wakes the accept loop so it sees the flag.
        let _ = TcpStream::connect(self.address);
    }
}

fn serve(
    mut stream: TcpStream,
    routes: &Mutex<HashMap<String, Reply>>,
    requests: &Mutex<Vec<String>>,
) {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(1) => head.push(byte[0]),
            _ => return,
        }
    }
    let head = String::from_utf8_lossy(&head);
    let path = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/")
        .to_string();
    requests.lock().unwrap().push(path.clone());

    let reply = routes.lock().unwrap().get(&path).cloned();
    let reply = reply.unwrap_or_else(|| Reply::status(404));

    let reason = match reply.status {
        200 => "OK",
        204 => "No Content",
        302 => "Found",
        404 => "Not Found",
        _ => "Error",
    };
    let mut response = format!(
        "HTTP/1.1 {} {reason}\r\nConnection: close\r\n",
        reply.status
    );
    if let Some(location) = &reply.location {
        response.push_str(&format!("Location: {location}\r\n"));
    }
    if reply.status != 204 {
        response.push_str(&format!("Content-Length: {}\r\n", reply.body.len()));
    }
    response.push_str("\r\n");
    if stream.write_all(response.as_bytes()).is_err() {
        return;
    }

    let body = match reply.truncated_to {
        Some(n) => &reply.body[..n.min(reply.body.len())],
        None => &reply.body[..],
    };
    match reply.slowly {
        Some((chunk, pause)) => {
            for piece in body.chunks(chunk) {
                if stream.write_all(piece).is_err() || stream.flush().is_err() {
                    return; // the client hung up, which is what a cancel does
                }
                thread::sleep(pause);
            }
        }
        None => {
            let _ = stream.write_all(body);
        }
    }
}
