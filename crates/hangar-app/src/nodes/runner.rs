//! What the connection asks of the outside world, behind a trait so that the
//! steps run against a fake in the tests: a command run to its end, a tunnel
//! kept running, a free port, the node's `/node/info`.

use std::future::Future;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

use super::commands::{Cmd, PROBE_END};
use super::model::NodeInfo;

/// How a command ended.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CmdOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// A running tunnel.
pub trait Tunnel: Send + 'static {
    /// Why it ended, once it has; `None` while it runs.
    fn exited(&mut self) -> Option<String>;
    /// Ends it now, without waiting: what quitting the app and Disconnect do.
    fn kill(&mut self);
}

pub trait Runner: Send + Sync + 'static {
    type Tunnel: Tunnel;

    fn run(&self, cmd: Cmd) -> impl Future<Output = Result<CmdOutput, String>> + Send;
    fn spawn_tunnel(&self, cmd: Cmd) -> Result<Self::Tunnel, String>;
    /// The tunnel as an SSH master ([`super::commands::link`]), kept
    /// running, and what its session printed up to [`PROBE_END`] (`success`
    /// once that line came; otherwise it ended first, and `stderr` says
    /// why). `control_path` is its socket, removed when it is killed.
    fn open_link(
        &self,
        cmd: Cmd,
        control_path: PathBuf,
    ) -> impl Future<Output = Result<(Self::Tunnel, CmdOutput), String>> + Send;
    /// A port of this machine nothing listens on.
    fn free_port(&self) -> Result<u16, String>;
    fn node_info(
        &self,
        port: u16,
        token: String,
    ) -> impl Future<Output = Result<NodeInfo, String>> + Send;
}

/// The real one: `ssh` and `scp` of the system, HTTP over the tunnel.
#[derive(Debug, Clone, Copy, Default)]
pub struct System;

fn command(cmd: &Cmd) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(&cmd.program);
    command
        .args(&cmd.args)
        .stdin(Stdio::null())
        // A Disconnect drops the connection's task, and whatever it runs
        // goes with it.
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        // No console window flashing up for each command.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

impl Runner for System {
    type Tunnel = SystemTunnel;

    async fn run(&self, cmd: Cmd) -> Result<CmdOutput, String> {
        let output = command(&cmd)
            .output()
            .await
            .map_err(|err| format!("{}: {err}", cmd.program))?;
        Ok(CmdOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    fn spawn_tunnel(&self, cmd: Cmd) -> Result<SystemTunnel, String> {
        let mut child = command(&cmd)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| format!("{}: {err}", cmd.program))?;
        let (said, _) = follow_stderr(&mut child);
        Ok(SystemTunnel {
            child,
            said,
            held: None,
        })
    }

    async fn open_link(
        &self,
        cmd: Cmd,
        control_path: PathBuf,
    ) -> Result<(SystemTunnel, CmdOutput), String> {
        let mut child = command(&cmd)
            // Held open for as long as the link runs: the session's `cat`
            // ends with it.
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| format!("{}: {err}", cmd.program))?;
        let (said, mut closed) = follow_stderr(&mut child);
        let stdin = child.stdin.take();
        let mut stdout = child.stdout.take().map(BufReader::new);
        let mut text = String::new();
        let mut ended = false;
        if let Some(reader) = stdout.as_mut() {
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if let Some(before) = line.trim_end().strip_suffix(PROBE_END) {
                            text.push_str(before);
                            ended = true;
                            break;
                        }
                        text.push_str(&line);
                    }
                }
            }
        }
        if !ended {
            // It ended: what it said on its way out is the reason.
            let _ =
                tokio::time::timeout(Duration::from_secs(2), closed.wait_for(|done| *done)).await;
        }
        let stderr = said.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let tunnel = SystemTunnel {
            child,
            said,
            held: Some(Held {
                _stdin: stdin,
                _stdout: stdout,
                control_path,
            }),
        };
        Ok((
            tunnel,
            CmdOutput {
                success: ended,
                stdout: text,
                stderr,
            },
        ))
    }

    fn free_port(&self) -> Result<u16, String> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|err| err.to_string())?;
        listener
            .local_addr()
            .map(|addr| addr.port())
            .map_err(|err| err.to_string())
    }

    async fn node_info(&self, port: u16, token: String) -> Result<NodeInfo, String> {
        tokio::task::spawn_blocking(move || {
            let body = http_get(port, "/node/info", &token, Duration::from_secs(5))?;
            serde_json::from_str(&body).map_err(|err| format!("/node/info: {err}"))
        })
        .await
        .map_err(|err| err.to_string())?
    }
}

/// What ssh says on its way out is the reason the page shows; it is read as
/// it comes so the pipe never fills. The receiver turns true once stderr
/// closed (ssh ended).
fn follow_stderr(
    child: &mut tokio::process::Child,
) -> (Arc<Mutex<String>>, tokio::sync::watch::Receiver<bool>) {
    let said = Arc::new(Mutex::new(String::new()));
    let (done, closed) = tokio::sync::watch::channel(false);
    if let Some(mut stderr) = child.stderr.take() {
        let said = said.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            while let Ok(n) = stderr.read(&mut buf).await {
                if n == 0 {
                    break;
                }
                let mut said = said.lock().unwrap_or_else(|p| p.into_inner());
                said.push_str(&String::from_utf8_lossy(&buf[..n]));
                // The last words are the ones that matter.
                if said.len() > 4096 {
                    let cut = said.len() - 2048;
                    let cut = (cut..said.len())
                        .find(|i| said.is_char_boundary(*i))
                        .unwrap_or(cut);
                    said.drain(..cut);
                }
            }
            let _ = done.send(true);
        });
    } else {
        let _ = done.send(true);
    }
    (said, closed)
}

pub struct SystemTunnel {
    child: tokio::process::Child,
    said: Arc<Mutex<String>>,
    /// A master's ([`Runner::open_link`]).
    held: Option<Held>,
}

/// What a master keeps: its session's pipes (stdin's end closed would end
/// it), and its socket, to remove once it is killed.
struct Held {
    _stdin: Option<tokio::process::ChildStdin>,
    _stdout: Option<BufReader<tokio::process::ChildStdout>>,
    control_path: PathBuf,
}

impl Tunnel for SystemTunnel {
    fn exited(&mut self) -> Option<String> {
        match self.child.try_wait() {
            Ok(None) => None,
            Ok(Some(status)) => {
                let said = self.said.lock().unwrap_or_else(|p| p.into_inner());
                let said = said.trim();
                Some(if said.is_empty() {
                    format!("ssh {status}")
                } else {
                    said.lines().last().unwrap_or(said).to_string()
                })
            }
            Err(err) => Some(err.to_string()),
        }
    }

    fn kill(&mut self) {
        let _ = self.child.start_kill();
        // Killed, it leaves its socket behind.
        if let Some(held) = &self.held {
            let _ = std::fs::remove_file(&held.control_path);
        }
    }
}

/// `GET path` on `127.0.0.1:port` with the node's token, and the body of a
/// 200. HTTP/1.1 by hand: the app has no HTTP client, and this one request
/// to a loopback port does not need one.
pub fn http_get(port: u16, path: &str, token: &str, timeout: Duration) -> Result<String, String> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&addr, timeout).map_err(|err| err.to_string())?;
    stream
        .set_read_timeout(Some(timeout))
        .and_then(|()| stream.set_write_timeout(Some(timeout)))
        .map_err(|err| err.to_string())?;
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\n\
         Accept: application/json\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|err| err.to_string())?;
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|err| err.to_string())?;
    parse_response(&raw)
}

/// The body of a 200 response, or what went wrong.
pub fn parse_response(raw: &[u8]) -> Result<String, String> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| "no HTTP answer".to_string())?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let body = &raw[split + 4..];
    let mut lines = head.lines();
    let status_line = lines.next().unwrap_or_default();
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| format!("not an HTTP answer: {status_line}"))?;
    if status != 200 {
        return Err(format!("HTTP {status}"));
    }
    let chunked = lines.any(|line| {
        let lower = line.to_ascii_lowercase();
        lower.starts_with("transfer-encoding:") && lower.contains("chunked")
    });
    let body = if chunked {
        dechunk(body)?
    } else {
        body.to_vec()
    };
    String::from_utf8(body).map_err(|err| err.to_string())
}

pub(crate) fn dechunk(mut body: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let end = body
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or_else(|| "a broken chunk".to_string())?;
        let size_text = String::from_utf8_lossy(&body[..end]);
        let size_text = size_text.split(';').next().unwrap_or_default().trim();
        let size = usize::from_str_radix(size_text, 16)
            .map_err(|_| format!("a broken chunk size: {size_text}"))?;
        body = &body[end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if body.len() < size {
            return Err("a truncated chunk".into());
        }
        out.extend_from_slice(&body[..size]);
        body = body.get(size + 2..).unwrap_or_default();
    }
}
