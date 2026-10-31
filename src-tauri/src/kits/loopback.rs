//! `loopback.fetch`: a kit's tab talking to a local server, by way of Hangar.
//!
//! The tab's page keeps no network of its own (kit_scheme.rs): the window checks
//! the tab holds `loopback:<port>` for that very port (src/lib/kitApi.ts), and
//! this makes the call. 127.0.0.1 only, whatever the system's proxy says, GET or
//! POST, no redirect followed, and no header or cookie of the page's. Nothing
//! that answers is an ordinary state for a tab — the editor is closed — so it
//! comes back as the error code `unreachable`, and nothing is logged.

use std::sync::OnceLock;
use std::time::Duration;

use serde::Serialize;

use super::{off_thread, TabError};

/// Below this, a port belongs to the system: `loopback:<port>` cannot name it.
pub const MIN_PORT: u16 = 1024;
/// `KIT_LOOPBACK_MAX_RESPONSE_BYTES` in src/lib/kits.ts.
pub const MAX_RESPONSE_BYTES: u64 = 2 * 1024 * 1024;
/// `KIT_LOOPBACK_MAX_BODY_BYTES` in src/lib/kits.ts.
pub const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
const MAX_PATH_BYTES: usize = 2048;
pub const DEFAULT_TIMEOUT_MS: u64 = 5_000;
pub const MAX_TIMEOUT_MS: u64 = 15_000;

/// A server on this machine takes the connection at once, however busy it is.
/// Without this, Windows retries a refused one for some two seconds, too slow
/// for a tab asking once a second.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Answer {
    pub status: u16,
    pub text: String,
}

/// One call, as the window passes it on.
#[derive(Debug, Clone, Default)]
pub struct Call {
    pub port: u16,
    pub path: String,
    /// `GET`, or absent for it, or `POST`.
    pub method: Option<String>,
    pub body: Option<String>,
    pub timeout_ms: Option<u64>,
}

fn invalid(what: impl Into<String>) -> TabError {
    TabError::new(TabError::INVALID, what)
}

/// The call's URL, whose host no path can change. A path may hold a query; it
/// has to be encoded already, and holds nothing a URL parser could read as the
/// start of another authority.
fn url_of(port: u16, path: &str) -> Result<String, TabError> {
    if port < MIN_PORT {
        return Err(invalid(format!("port must be {MIN_PORT} to 65535")));
    }
    if !path.starts_with('/') || path.starts_with("//") {
        return Err(invalid("path must start with one /"));
    }
    if path.len() > MAX_PATH_BYTES {
        return Err(invalid(format!(
            "path must be at most {MAX_PATH_BYTES} characters"
        )));
    }
    if !path
        .bytes()
        .all(|byte| byte.is_ascii_graphic() && byte != b'\\' && byte != b'#')
    {
        return Err(invalid(
            "path must be URL-encoded: no space, backslash, # or character beyond ASCII",
        ));
    }
    let url = format!("http://127.0.0.1:{port}{path}");
    let uri: ureq::http::Uri = url
        .parse()
        .map_err(|_| invalid("path is not a URL's path"))?;
    if uri.host() != Some("127.0.0.1") || uri.port_u16() != Some(port) {
        return Err(invalid("path must not change the host"));
    }
    Ok(url)
}

fn timeout_of(timeout_ms: Option<u64>) -> Result<Duration, TabError> {
    let ms = timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);
    if ms == 0 || ms > MAX_TIMEOUT_MS {
        return Err(invalid(format!("timeoutMs must be 1 to {MAX_TIMEOUT_MS}")));
    }
    Ok(Duration::from_millis(ms))
}

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            // Any status is the server's answer, for the tab to read.
            .http_status_as_error(false)
            .max_redirects(0)
            // A system proxy has no business with 127.0.0.1.
            .proxy(None)
            .user_agent("Hangar")
            .timeout_connect(Some(CONNECT_TIMEOUT))
            // A connection each: nothing stays open on a server between polls.
            .max_idle_connections(0)
            .build()
            .new_agent()
    })
}

/// Nothing there, or nothing in time, is `unreachable`; the rest went wrong.
fn explain(port: u16, err: ureq::Error) -> TabError {
    match err {
        ureq::Error::Io(_)
        | ureq::Error::Timeout(_)
        | ureq::Error::ConnectionFailed
        | ureq::Error::HostNotFound
        | ureq::Error::BodyStalled => TabError::new(
            TabError::UNREACHABLE,
            format!("nothing answers on port {port}"),
        ),
        ureq::Error::BodyExceedsLimit(_) => {
            TabError::new(TabError::FAILED, "the answer is larger than 2 MB")
        }
        other => TabError::new(TabError::FAILED, other.to_string()),
    }
}

pub fn fetch(call: Call) -> Result<Answer, TabError> {
    let url = url_of(call.port, &call.path)?;
    let timeout = timeout_of(call.timeout_ms)?;
    if call
        .body
        .as_deref()
        .is_some_and(|body| body.len() > MAX_BODY_BYTES)
    {
        return Err(invalid("body is larger than 2 MB"));
    }
    let sent = match call.method.as_deref() {
        None | Some("GET") => {
            if call.body.is_some() {
                return Err(invalid("a GET carries no body"));
            }
            agent()
                .get(&url)
                .config()
                .timeout_global(Some(timeout))
                .build()
                .call()
        }
        Some("POST") => agent()
            .post(&url)
            .config()
            .timeout_global(Some(timeout))
            .build()
            .header("Content-Type", "application/json")
            .send(call.body.as_deref().unwrap_or("")),
        Some(_) => return Err(invalid("method must be GET or POST")),
    };
    let mut response = sent.map_err(|err| explain(call.port, err))?;
    let status = response.status().as_u16();
    // One byte more than allowed: ureq also refuses a body that ends right on
    // its limit, and an answer of exactly the maximum is a good one.
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BYTES + 1)
        .read_to_vec()
        .map_err(|err| explain(call.port, err))?;
    if bytes.len() as u64 > MAX_RESPONSE_BYTES {
        return Err(TabError::new(
            TabError::FAILED,
            "the answer is larger than 2 MB",
        ));
    }
    Ok(Answer {
        status,
        text: String::from_utf8_lossy(&bytes).into_owned(),
    })
}

/// `loopback.fetch`. The window checked the tab holds `loopback:<port>`.
#[tauri::command]
pub async fn kit_loopback_fetch(
    port: u16,
    path: String,
    method: Option<String>,
    body: Option<String>,
    timeout_ms: Option<u64>,
) -> Result<Answer, TabError> {
    off_thread(move || {
        fetch(Call {
            port,
            path,
            method,
            body,
            timeout_ms,
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    /// An OS-chosen free port: `test_port`'s twenty are all named already by the
    /// `remote` tests running in this same process, and a listener here on one
    /// of them would break whichever test binds it next.
    fn listener() -> (TcpListener, u16) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        (listener, port)
    }

    fn read_request(stream: &mut TcpStream) -> String {
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut seen = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            if let Some(end) = seen.windows(4).position(|window| window == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&seen[..end]).to_ascii_lowercase();
                let length = head
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if seen.len() >= end + 4 + length {
                    break;
                }
            }
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => seen.extend_from_slice(&chunk[..n]),
            }
        }
        String::from_utf8_lossy(&seen).into_owned()
    }

    /// Answers each connection with the next of `answers`, and keeps what it was
    /// asked. Connections past the last are only counted, as asked.
    fn server(answers: Vec<Vec<u8>>) -> (u16, Arc<Mutex<Vec<String>>>) {
        let (listener, port) = listener();
        let asked = Arc::new(Mutex::new(Vec::new()));
        let kept = asked.clone();
        std::thread::spawn(move || {
            let mut answers = answers.into_iter();
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                kept.lock().unwrap().push(read_request(&mut stream));
                if let Some(answer) = answers.next() {
                    let _ = stream.write_all(&answer);
                    let _ = stream.flush();
                }
            }
        });
        (port, asked)
    }

    fn http(status: &str, extra: &str, body: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 {status}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    fn get(port: u16, path: &str) -> Call {
        Call {
            port,
            path: path.to_string(),
            ..Call::default()
        }
    }

    #[test]
    fn a_get_comes_back_with_its_status_and_text() {
        let (port, asked) = server(vec![
            http("200 OK", "", "{\"queue\":[]}"),
            http("404 Not Found", "", "none"),
        ]);
        assert_eq!(
            fetch(get(port, "/queue?since=1")).unwrap(),
            Answer {
                status: 200,
                text: "{\"queue\":[]}".into()
            }
        );
        // Any status is the server's answer, not a failure.
        assert_eq!(fetch(get(port, "/none")).unwrap().status, 404);
        let asked = asked.lock().unwrap();
        assert!(
            asked[0].starts_with("GET /queue?since=1 HTTP/1.1\r\n"),
            "{}",
            asked[0]
        );
        assert!(
            asked[0]
                .to_ascii_lowercase()
                .contains(&format!("host: 127.0.0.1:{port}\r\n")),
            "{}",
            asked[0]
        );
    }

    #[test]
    fn a_post_carries_its_body_as_json_and_no_other_header_of_the_page() {
        let (port, asked) = server(vec![http("201 Created", "", "ok")]);
        let answer = fetch(Call {
            method: Some("POST".into()),
            body: Some("{\"run\":true}".into()),
            ..get(port, "/run")
        })
        .unwrap();
        assert_eq!((answer.status, answer.text.as_str()), (201, "ok"));
        let asked = asked.lock().unwrap();
        let lower = asked[0].to_ascii_lowercase();
        assert!(
            asked[0].starts_with("POST /run HTTP/1.1\r\n"),
            "{}",
            asked[0]
        );
        assert!(
            lower.contains("content-type: application/json\r\n"),
            "{lower}"
        );
        assert!(asked[0].ends_with("{\"run\":true}"), "{}", asked[0]);
        assert!(!lower.contains("cookie:"), "{lower}");
        assert!(!lower.contains("origin:"), "{lower}");
        assert!(!lower.contains("authorization:"), "{lower}");
    }

    #[test]
    fn a_refused_connection_is_unreachable_and_quick() {
        let (listener, port) = listener();
        drop(listener);
        let started = Instant::now();
        let err = fetch(get(port, "/queue")).unwrap_err();
        assert_eq!(err.code, TabError::UNREACHABLE, "{}", err.message);
        assert!(
            started.elapsed() < Duration::from_millis(2_500),
            "took {:?}: a tab asks once a second",
            started.elapsed()
        );
    }

    #[test]
    fn a_silent_server_times_out_as_unreachable() {
        let (listener, port) = listener();
        let holder = std::thread::spawn(move || {
            let accepted = listener.accept();
            std::thread::sleep(Duration::from_millis(800));
            drop(accepted);
        });
        let started = Instant::now();
        let err = fetch(Call {
            timeout_ms: Some(200),
            ..get(port, "/queue")
        })
        .unwrap_err();
        assert_eq!(err.code, TabError::UNREACHABLE, "{}", err.message);
        assert!(
            started.elapsed() < Duration::from_millis(790),
            "{:?}",
            started.elapsed()
        );
        holder.join().unwrap();
    }

    #[test]
    fn a_redirect_is_not_followed() {
        let (target, target_port) = listener();
        let target_hit = Arc::new(Mutex::new(false));
        {
            let target_hit = target_hit.clone();
            std::thread::spawn(move || {
                for stream in target.incoming().flatten() {
                    *target_hit.lock().unwrap() = true;
                    drop(stream);
                }
            });
        }
        let location = format!("Location: http://127.0.0.1:{target_port}/elsewhere\r\n");
        let (port, asked) = server(vec![http("302 Found", &location, "moved")]);
        let answer = fetch(get(port, "/queue")).unwrap();
        assert_eq!((answer.status, answer.text.as_str()), (302, "moved"));
        assert_eq!(asked.lock().unwrap().len(), 1, "asked once");
        assert!(
            !*target_hit.lock().unwrap(),
            "the other port was never reached"
        );
    }

    #[test]
    fn the_answer_is_held_to_two_megabytes() {
        let at_cap = "a".repeat(MAX_RESPONSE_BYTES as usize);
        let over = at_cap.clone() + "a";
        let (port, _) = server(vec![http("200 OK", "", &at_cap), http("200 OK", "", &over)]);
        assert_eq!(fetch(get(port, "/")).unwrap().text.len(), at_cap.len());
        let err = fetch(get(port, "/")).unwrap_err();
        assert_eq!(err.code, TabError::FAILED);
        assert!(err.message.contains("2 MB"), "{}", err.message);
    }

    #[test]
    fn what_cannot_be_read_as_text_is_still_an_answer() {
        let mut answer =
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n".to_vec();
        answer.extend_from_slice(&[0xff, 0xfe]);
        let (port, _) = server(vec![answer]);
        assert_eq!(fetch(get(port, "/")).unwrap().text, "\u{fffd}\u{fffd}");
    }

    #[test]
    fn something_that_does_not_speak_http_is_a_failure() {
        let (port, _) = server(vec![b"220 smtp ready\r\n".to_vec()]);
        assert_eq!(fetch(get(port, "/")).unwrap_err().code, TabError::FAILED);
    }

    #[test]
    fn bad_calls_are_refused_before_anything_is_sent() {
        let (port, asked) = server(vec![]);
        for path in [
            "",
            "queue",
            "//example.com/queue",
            "http://example.com/",
            "/a b",
            "/a\nHost: example.com",
            "/a\\b",
            "/\\example.com",
            "/é",
            "/a#b",
            &format!("/{}", "a".repeat(MAX_PATH_BYTES)),
        ] {
            let err = fetch(get(port, path)).unwrap_err();
            assert_eq!(err.code, TabError::INVALID, "{path:?}");
        }
        for method in ["PUT", "DELETE", "get", "", "POST\r\nX: y"] {
            let err = fetch(Call {
                method: Some(method.into()),
                ..get(port, "/")
            })
            .unwrap_err();
            assert_eq!(err.code, TabError::INVALID, "{method:?}");
        }
        for timeout in [0, MAX_TIMEOUT_MS + 1] {
            let err = fetch(Call {
                timeout_ms: Some(timeout),
                ..get(port, "/")
            })
            .unwrap_err();
            assert_eq!(err.code, TabError::INVALID, "{timeout}");
        }
        assert_eq!(fetch(get(80, "/")).unwrap_err().code, TabError::INVALID);
        let body_on_get = fetch(Call {
            body: Some("x".into()),
            ..get(port, "/")
        });
        assert_eq!(body_on_get.unwrap_err().code, TabError::INVALID);
        let big = fetch(Call {
            method: Some("POST".into()),
            body: Some("x".repeat(MAX_BODY_BYTES + 1)),
            ..get(port, "/")
        });
        assert_eq!(big.unwrap_err().code, TabError::INVALID);
        // Given a moment, a request that got through would have been kept.
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            asked.lock().unwrap().is_empty(),
            "nothing reached the server"
        );
    }

    #[test]
    fn the_host_stays_the_loopback_whatever_the_path() {
        for path in ["/", "/a/../b", "/@example.com/", "/a?b=http://example.com"] {
            let url = url_of(8765, path).unwrap();
            let uri: ureq::http::Uri = url.parse().unwrap();
            assert_eq!(uri.host(), Some("127.0.0.1"), "{path}");
            assert_eq!(uri.port_u16(), Some(8765), "{path}");
        }
    }
}
