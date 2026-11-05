//! The parent's side of the protocol: start the helper, send it commands, read
//! its events.
//!
//! It is made to be called from anywhere, so it takes nothing from an async
//! runtime and blocks nothing it should not:
//!
//! - [`Client::send`] only queues a line. A thread of the client writes it, so
//!   a task on an executor never waits for a helper that is slow to read (a
//!   response body can be tens of megabytes), and a helper that has gone away
//!   shows as [`Closed`] instead of a hang.
//! - [`Events`] is a queue with a blocking [`recv`](Events::recv), a
//!   [`recv_timeout`](Events::recv_timeout), a [`try_recv`](Events::try_recv),
//!   and an async [`recv_async`](Events::recv_async) built from `std::task`
//!   alone, so it works under tokio, under iced's executor or under a plain
//!   `block_on`. A `Stream` is a `poll_recv` away.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::fmt;
use std::future::Future;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command as Process, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, Waker};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::protocol::{
    Command, ErrorCode, Event, MAX_EVENT_LINE_BYTES, OpenWindow, PROTOCOL_VERSION, SchemeResponse,
};

/// How long a helper gets to leave after its client is dropped before it is
/// killed.
const LEAVE_WITHIN: Duration = Duration::from_secs(3);

/// A lock that a panicking thread did not take with it: the queue and the child
/// are plain data, valid at any point a panic could leave them.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The helper is gone, or its input is closed: nothing more can be sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Closed;

impl fmt::Display for Closed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the webview helper is not running")
    }
}

impl std::error::Error for Closed {}

/// What happens to the helper's standard error, where it writes its logs and
/// the engine its complaints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Stderr {
    /// The parent's own, so the logs show where the parent's do.
    #[default]
    Inherit,
    Null,
}

/// How to start the helper.
#[derive(Debug, Clone)]
pub struct SpawnOptions {
    program: PathBuf,
    args: Vec<OsString>,
    envs: Vec<(OsString, OsString)>,
    stderr: Stderr,
}

impl SpawnOptions {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            envs: Vec::new(),
            stderr: Stderr::default(),
        }
    }

    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn env(mut self, name: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.envs.push((name.into(), value.into()));
        self
    }

    pub fn stderr(mut self, stderr: Stderr) -> Self {
        self.stderr = stderr;
        self
    }
}

/// Where the helper is, if it is installed next to the program running: the
/// `HANGAR_WEBVIEW` variable if set, then the folder of the current executable,
/// then its parent (for a program run from cargo's `examples/` or `deps/`).
pub fn find_helper() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("HANGAR_WEBVIEW") {
        return Some(PathBuf::from(path));
    }
    let name = format!("hangar-webview{}", std::env::consts::EXE_SUFFIX);
    let exe = std::env::current_exe().ok()?;
    exe.ancestors()
        .skip(1)
        .take(2)
        .map(|folder| folder.join(&name))
        .find(|path| path.is_file())
}

/// The events waiting from a helper, to be taken by one reader.
///
/// It ends (`None`, [`RecvTimeoutError::Disconnected`]) once the helper's output
/// closes and what it wrote before has been read, which is how a crash looks:
/// ask [`Client::wait`] how the helper left.
pub struct Events {
    queue: Arc<Queue>,
}

struct Queue {
    state: Mutex<QueueState>,
    arrived: Condvar,
}

/// The most events the queue holds before it drops a page's messages.
const MAX_QUEUED_IPC: usize = 1024;

struct QueueState {
    events: VecDeque<Event>,
    closed: bool,
    /// Of the task waiting in `poll_recv`; one reader, so one.
    waker: Option<Waker>,
}

impl Queue {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(QueueState {
                events: VecDeque::new(),
                closed: false,
                waker: None,
            }),
            arrived: Condvar::new(),
        })
    }

    fn push(&self, event: Event) {
        let waker = {
            let mut state = lock(&self.state);
            // A page that posts faster than the window reads is not kept up
            // with: its messages past the bound are dropped, not queued.
            if matches!(event, Event::Ipc { .. }) && state.events.len() >= MAX_QUEUED_IPC {
                return;
            }
            state.events.push_back(event);
            state.waker.take()
        };
        self.arrived.notify_one();
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    fn close(&self) {
        let waker = {
            let mut state = lock(&self.state);
            state.closed = true;
            state.waker.take()
        };
        self.arrived.notify_all();
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

impl Events {
    /// The next event if there is one now.
    pub fn try_recv(&self) -> Result<Event, TryRecvError> {
        let mut state = lock(&self.queue.state);
        match state.events.pop_front() {
            Some(event) => Ok(event),
            None if state.closed => Err(TryRecvError::Disconnected),
            None => Err(TryRecvError::Empty),
        }
    }

    /// The next event, waiting for it; `None` once there will be no more.
    pub fn recv(&self) -> Option<Event> {
        let mut state = lock(&self.queue.state);
        loop {
            if let Some(event) = state.events.pop_front() {
                return Some(event);
            }
            if state.closed {
                return None;
            }
            state = self
                .queue
                .arrived
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// The next event, waiting at most `timeout` for it.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<Event, RecvTimeoutError> {
        let deadline = Instant::now() + timeout;
        let mut state = lock(&self.queue.state);
        loop {
            if let Some(event) = state.events.pop_front() {
                return Ok(event);
            }
            if state.closed {
                return Err(RecvTimeoutError::Disconnected);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(RecvTimeoutError::Timeout);
            }
            state = self
                .queue
                .arrived
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// For a `Stream`, or any poll-based reader: the next event if there is one,
    /// else `Pending` with the task woken when one comes (or when the helper
    /// goes away, which is `Ready(None)`). Only the task that polled last is
    /// woken.
    pub fn poll_recv(&self, cx: &mut Context<'_>) -> Poll<Option<Event>> {
        let mut state = lock(&self.queue.state);
        if let Some(event) = state.events.pop_front() {
            return Poll::Ready(Some(event));
        }
        if state.closed {
            return Poll::Ready(None);
        }
        state.waker = Some(cx.waker().clone());
        Poll::Pending
    }

    /// [`recv`](Self::recv) for an async task.
    pub fn recv_async(&self) -> impl Future<Output = Option<Event>> + '_ {
        std::future::poll_fn(|cx| self.poll_recv(cx))
    }
}

/// Blocks on each event, so `for event in events` reads until the helper ends.
impl Iterator for Events {
    type Item = Event;

    fn next(&mut self) -> Option<Event> {
        self.recv()
    }
}

/// A running helper, or anything that speaks its protocol over two streams.
/// Dropping it quits the helper: it is asked to, its input is closed, and a
/// helper that has not left after a few seconds is killed.
pub struct Client {
    lines: Sender<String>,
    /// Set when either direction fails or ends, so `send` says so at once.
    closed: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
}

impl Client {
    /// Start the helper at `program`. It does not wait: the helper's first
    /// event, [`Event::Ready`], comes through the returned [`Events`] when its
    /// window system is up, and a helper of another protocol version is
    /// answered by an [`ErrorCode::ProtocolMismatch`] error and a closed queue.
    pub fn spawn(program: impl AsRef<Path>) -> io::Result<(Client, Events)> {
        Self::spawn_with(SpawnOptions::new(program.as_ref()))
    }

    pub fn spawn_with(options: SpawnOptions) -> io::Result<(Client, Events)> {
        let mut process = Process::new(&options.program);
        process
            .args(&options.args)
            .envs(options.envs.iter().map(|(name, value)| (name, value)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(match options.stderr {
                Stderr::Inherit => Stdio::inherit(),
                Stderr::Null => Stdio::null(),
            });
        #[cfg(windows)]
        {
            // The helper is a console program: without this, a parent that is
            // not one would open a console window for it.
            use std::os::windows::process::CommandExt;
            process.creation_flags(0x0800_0000);
        }
        let mut child = process.spawn()?;
        let stdin: ChildStdin = child.stdin.take().expect("stdin was piped");
        let stdout: ChildStdout = child.stdout.take().expect("stdout was piped");
        Ok(Self::start(stdout, stdin, Some(child)))
    }

    /// A client over streams of your own: the other end is a helper, or a fake
    /// of one. Whatever `reader` yields is read as the helper's output, and
    /// every command is written to `writer`, which is closed when the client
    /// is dropped.
    pub fn from_io(
        reader: impl Read + Send + 'static,
        writer: impl Write + Send + 'static,
    ) -> (Client, Events) {
        Self::start(reader, writer, None)
    }

    fn start(
        reader: impl Read + Send + 'static,
        writer: impl Write + Send + 'static,
        child: Option<Child>,
    ) -> (Client, Events) {
        let queue = Queue::new();
        let closed = Arc::new(AtomicBool::new(false));
        let child = Arc::new(Mutex::new(child));
        let (lines, outgoing) = mpsc::channel();

        let reading = (queue.clone(), closed.clone(), child.clone());
        thread::Builder::new()
            .name("hangar-webview-reader".into())
            .spawn(move || read_events(reader, reading.0, reading.1, reading.2))
            .expect("a thread starts");
        let closing = closed.clone();
        thread::Builder::new()
            .name("hangar-webview-writer".into())
            .spawn(move || write_lines(writer, outgoing, closing))
            .expect("a thread starts");

        (
            Client {
                lines,
                closed,
                child,
            },
            Events { queue },
        )
    }

    /// Queue a command for the helper. It returns at once; [`Closed`] means the
    /// helper has already gone, and a failure to write later shows as the end
    /// of the events.
    pub fn send(&self, command: Command) -> Result<(), Closed> {
        let mut line = command.to_line();
        line.push('\n');
        if self.closed.load(Ordering::Acquire) {
            return Err(Closed);
        }
        self.lines.send(line).map_err(|_| Closed)
    }

    /// Whether the helper's output has ended or its input broken.
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    pub fn open(&self, window: OpenWindow) -> Result<(), Closed> {
        self.send(Command::Open(window))
    }

    pub fn navigate(&self, window: &str, url: &str) -> Result<(), Closed> {
        self.send(Command::Navigate {
            window: window.into(),
            url: url.into(),
        })
    }

    /// Run a script, forgetting what it gives.
    pub fn eval(&self, window: &str, script: &str) -> Result<(), Closed> {
        self.send(Command::Eval {
            window: window.into(),
            script: script.into(),
            token: None,
        })
    }

    /// Run a script and have its value come back as an [`Event::EvalResult`]
    /// with this token.
    pub fn eval_with_token(&self, window: &str, script: &str, token: &str) -> Result<(), Closed> {
        self.send(Command::Eval {
            window: window.into(),
            script: script.into(),
            token: Some(token.into()),
        })
    }

    pub fn post_message(&self, window: &str, message: Value) -> Result<(), Closed> {
        self.send(Command::PostMessage {
            window: window.into(),
            message,
        })
    }

    pub fn set_title(&self, window: &str, title: &str) -> Result<(), Closed> {
        self.send(Command::SetTitle {
            window: window.into(),
            title: title.into(),
        })
    }

    pub fn focus(&self, window: &str) -> Result<(), Closed> {
        self.send(Command::Focus {
            window: window.into(),
        })
    }

    /// Show or hide a window, keeping its page (see [`Command::SetVisible`]).
    /// `focus` asks for the keyboard when it is shown.
    pub fn set_visible(&self, window: &str, visible: bool, focus: bool) -> Result<(), Closed> {
        self.send(Command::SetVisible {
            window: window.into(),
            visible,
            focus,
        })
    }

    pub fn close(&self, window: &str) -> Result<(), Closed> {
        self.send(Command::Close {
            window: window.into(),
        })
    }

    /// Answer a [`SchemeRequest`](crate::SchemeRequest).
    pub fn respond(&self, response: SchemeResponse) -> Result<(), Closed> {
        self.send(Command::SchemeResponse(response))
    }

    /// Ask the helper to close its windows and exit.
    pub fn quit(&self) -> Result<(), Closed> {
        self.send(Command::Quit)
    }

    /// How the helper left, once it has: waits up to `timeout` for it. `None`
    /// for a client with no process of its own, or a helper still running.
    /// It blocks the calling thread, so a task on an executor calls it from a
    /// thread meant for blocking (`spawn_blocking`), or with a timeout of zero.
    pub fn wait(&self, timeout: Duration) -> Option<ExitStatus> {
        let deadline = Instant::now() + timeout;
        loop {
            {
                let mut child = lock(&self.child);
                match child.as_mut()?.try_wait() {
                    Ok(Some(status)) => return Some(status),
                    Ok(None) => {}
                    Err(_) => return None,
                }
            }
            if Instant::now() >= deadline {
                return None;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// The helper's process id, for a client that started one.
    pub fn id(&self) -> Option<u32> {
        lock(&self.child).as_ref().map(Child::id)
    }

    /// Stop the helper now, without asking.
    pub fn kill(&self) {
        if let Some(child) = lock(&self.child).as_mut() {
            let _ = child.kill();
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        // Asked first, so it closes its windows itself; the writer thread
        // closes its input when `lines` goes, which says the same if the line
        // does not get through.
        let _ = self.send(Command::Quit);
        let Some(child) = lock(&self.child).take() else {
            return;
        };
        // The wait is not ours to spend: whoever dropped the client may be a UI
        // thread.
        let spawned = thread::Builder::new()
            .name("hangar-webview-reaper".into())
            .spawn({
                let mut child = child;
                move || leave(&mut child)
            });
        drop(spawned);
    }
}

/// Wait for a helper that was asked to quit, and kill one that does not.
fn leave(child: &mut Child) {
    let deadline = Instant::now() + LEAVE_WITHIN;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Ok(Some(_)) | Err(_) => return,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Writes each queued line to the helper. When the client is dropped the queue
/// ends, and so does this, which closes the helper's input.
fn write_lines(mut writer: impl Write, lines: Receiver<String>, closed: Arc<AtomicBool>) {
    for line in lines {
        if writer
            .write_all(line.as_bytes())
            .and_then(|()| writer.flush())
            .is_err()
        {
            closed.store(true, Ordering::Release);
            return;
        }
    }
}

enum Line {
    /// The stream ended with nothing more.
    End,
    /// `raw` holds a line (with its newline, unless the stream ended).
    Read,
    /// The line was longer than allowed: it was read to its end and dropped,
    /// without being held.
    TooLong,
}

/// `read_until(b'\n')` that never holds more than `max` bytes of a line.
fn read_line_bounded(
    reader: &mut impl BufRead,
    raw: &mut Vec<u8>,
    max: usize,
) -> std::io::Result<Line> {
    let mut over = false;
    let mut any = false;
    loop {
        let available = match reader.fill_buf() {
            Ok(available) => available,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if available.is_empty() {
            break;
        }
        any = true;
        let (used, done) = match available.iter().position(|byte| *byte == b'\n') {
            Some(end) => (end + 1, true),
            None => (available.len(), false),
        };
        if !over {
            if raw.len() + used > max {
                over = true;
                raw.clear();
            } else {
                raw.extend_from_slice(&available[..used]);
            }
        }
        reader.consume(used);
        if done {
            break;
        }
    }
    Ok(match (any, over) {
        (false, _) => Line::End,
        (_, true) => Line::TooLong,
        _ => Line::Read,
    })
}

/// Reads the helper's output into the queue until it ends.
fn read_events(
    reader: impl Read,
    queue: Arc<Queue>,
    closed: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
) {
    let mut reader = BufReader::new(reader);
    let mut raw = Vec::new();
    loop {
        raw.clear();
        match read_line_bounded(&mut reader, &mut raw, MAX_EVENT_LINE_BYTES) {
            Ok(Line::End) | Err(_) => break,
            Ok(Line::Read) => {}
            Ok(Line::TooLong) => {
                queue.push(Event::error(
                    ErrorCode::TooLarge,
                    format!("the helper wrote a line of more than {MAX_EVENT_LINE_BYTES} bytes"),
                ));
                continue;
            }
        }
        let line = match std::str::from_utf8(&raw) {
            Ok(line) => line.trim_end_matches(['\r', '\n']),
            Err(_) => {
                queue.push(Event::error(
                    ErrorCode::BadEvent,
                    "the helper wrote a line that is not UTF-8",
                ));
                continue;
            }
        };
        if line.is_empty() {
            continue;
        }
        match Event::from_line(line) {
            Ok(Event::Ready {
                protocol, version, ..
            }) if protocol != PROTOCOL_VERSION => {
                // A helper of another protocol cannot be trusted to mean what
                // its lines say, so it is not listened to any further.
                queue.push(Event::error(
                    ErrorCode::ProtocolMismatch,
                    format!("the helper (version {version}) speaks protocol {protocol}, this client {PROTOCOL_VERSION}"),
                ));
                if let Some(child) = lock(&child).as_mut() {
                    let _ = child.kill();
                }
                break;
            }
            Ok(event) => queue.push(event),
            Err(error) => {
                let shown: String = line.chars().take(200).collect();
                queue.push(Event::error(
                    ErrorCode::BadEvent,
                    format!("{error}: {shown}"),
                ));
            }
        }
    }
    closed.store(true, Ordering::Release);
    queue.close();
}

#[cfg(test)]
mod bounds {
    use super::*;

    #[test]
    fn a_line_over_the_bound_is_dropped_and_the_next_one_is_read() {
        let mut data = vec![b'x'; 100];
        data.push(b'\n');
        data.extend_from_slice(b"ok\n");
        let mut reader = BufReader::with_capacity(8, std::io::Cursor::new(data));
        let mut raw = Vec::new();
        assert!(matches!(
            read_line_bounded(&mut reader, &mut raw, 50).unwrap(),
            Line::TooLong
        ));
        assert!(raw.len() <= 50, "{}", raw.len());
        raw.clear();
        assert!(matches!(
            read_line_bounded(&mut reader, &mut raw, 50).unwrap(),
            Line::Read
        ));
        assert_eq!(raw, b"ok\n");
        raw.clear();
        assert!(matches!(
            read_line_bounded(&mut reader, &mut raw, 50).unwrap(),
            Line::End
        ));
    }

    #[test]
    fn a_page_that_floods_the_queue_is_dropped_past_the_bound() {
        let queue = Queue::new();
        for _ in 0..MAX_QUEUED_IPC + 500 {
            queue.push(Event::Ipc {
                window: "w".into(),
                uri: String::new(),
                body: "x".into(),
            });
        }
        assert_eq!(lock(&queue.state).events.len(), MAX_QUEUED_IPC);
        // Other events still get through.
        queue.push(Event::error(ErrorCode::BadEvent, "x"));
        assert_eq!(lock(&queue.state).events.len(), MAX_QUEUED_IPC + 1);
    }

    #[test]
    fn a_huge_event_line_reaches_the_queue_as_an_error_not_as_the_line() {
        let mut data = vec![b'a'; MAX_EVENT_LINE_BYTES + 10];
        data.push(b'\n');
        let queue = Queue::new();
        read_events(
            std::io::Cursor::new(data),
            queue.clone(),
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(None)),
        );
        let state = lock(&queue.state);
        assert!(matches!(
            state.events.front(),
            Some(Event::Error {
                code: ErrorCode::TooLarge,
                ..
            })
        ));
    }
}
