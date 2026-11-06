//! A pane's terminal and the way to its process: what `lib/ptyBus.ts` and
//! the `pty_*` calls of TerminalPane.tsx were, without the bus.
//!
//! # The output path, which is the one latency matters on
//!
//! Core calls a pane's [`PtySink`] on the pane's reader thread for every
//! chunk the shell prints. The sink feeds the chunk to the pane's
//! [`Terminal`] right there, under its lock, and only then tells the window
//! that something changed. So the parsing of every pane's output happens on
//! that pane's own thread, sixteen busy panes in parallel, and the UI thread
//! only draws: it never queues bytes behind a message nor parses them between
//! two frames. What the window is told is a wake-up, and only one is ever on
//! its way for all the panes together ([`Waker`]): the first chunk after a
//! quiet spell sends it, and from then on the window looks at every pane
//! itself, once a frame ([`PaneLink::take_output`]), until a frame finds them
//! all quiet. A message makes iced build the whole interface again (every
//! view, then the layout), so sixteen panes printing a build log cost that
//! once a frame, not once for every chunk that lands between two frames.
//!
//! A pane off screen (another workspace, the chat view, a pane zoomed over
//! it) has nothing to draw: its output wakes the window on a flag of its own
//! ([`Waker::off_screen`]), and from then on the panes off screen are looked
//! at with the pane logic's tick, four times a second, and never once a
//! frame. What they printed is in their terminal all the same, and the look
//! hands the pane logic when they first and last printed
//! ([`Printed`]), measured here on the reader thread, so the activity it
//! reads from them is the one a look every frame would have read. Which
//! side a pane is on is the window's to say ([`PaneLink::set_shown`]).
//!
//! The terminal's answers to the program's queries (cursor position, device
//! attributes, colours) come out of the same feed and go straight to the
//! pane's writer, in order with the typing; the other things the terminal
//! has to say (a bell, a notification) go to the window as [`HubEvent`]s.
//!
//! # The input path
//!
//! Keys, pastes and resizes go to a writer thread of the pane's own, which
//! calls `pty_write` and `pty_resize`. Core's write blocks while the process
//! is not reading (a paste bigger than the PTY's buffer into a program that
//! is busy) and holds core's registry lock while it does, so a write on the
//! UI thread could freeze the whole window, every pane with it. The thread
//! hop costs a few microseconds.
//!
//! # Local and remote panes
//!
//! A pane talks to a [`PtyBackend`]: core's PTY manager for a workspace of
//! this machine, or the node a remote workspace lives on (docs/native/
//! nodes.md §5). A remote pane's output comes over the node's stream
//! WebSocket, read by a tokio task ([`PaneLink::follow_remote`]) that feeds
//! the terminal through the very [`Feeder`] the local sink is made of, so
//! the terminal, the hub and the frames do not know the difference; its
//! writer thread hands what is typed to that task, which sends it on the
//! same socket.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::time::Instant;

use hangar_core::pty::{PtyEvent, PtyManager, PtySink};
use hangar_term::{Event, Terminal};
use tokio::sync::mpsc as tokio_mpsc;

use super::launch::Echo;
use crate::nodes::NodeClient;
use crate::nodes::remote::{self, Backoff, Outgoing, StreamEvent};
use crate::state::PaneId;

/// What a pane's process is reached through.
#[derive(Clone)]
pub enum PtyBackend {
    /// Core's PTY manager: a pane of this machine.
    Local(Arc<PtyManager>),
    /// The node of id `node`: a pane of a remote workspace.
    Remote { node: String },
}

impl std::fmt::Debug for PtyBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PtyBackend::Local(_) => f.write_str("Local"),
            PtyBackend::Remote { node } => write!(f, "Remote({node})"),
        }
    }
}

/// What a pane's reader thread tells the window.
#[derive(Debug, Clone)]
pub struct HubEvent {
    pub pane: PaneId,
    pub kind: HubKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HubKind {
    /// A pane on screen printed something since the window last looked.
    Output,
    /// A pane off screen did: the window looks at those with the pane
    /// logic's tick, not every frame.
    OffScreen,
    /// The shell ended by itself (never after `pty_kill`).
    Exit,
    Bell,
    /// `OSC 9` or `OSC 777`, the escape codes agents raise a notification
    /// with.
    Notification {
        title: Option<String>,
        body: String,
    },
}

/// The sending half of the window's channel, cloned into every sink.
pub type HubSender = tokio_mpsc::UnboundedSender<HubEvent>;

/// What a sink wakes the window with: the channel, and the flag every pane
/// on screen shares that says the window already knows output is coming,
/// because a wake-up is on its way or it is looking at the panes every frame.
/// Set by the sink that sends the wake-up, cleared by the window once a frame
/// finds every pane on screen quiet (`HubChannel::rest`). `off_screen` is the
/// same for the panes off screen, which the window looks at with the pane
/// logic's tick (`HubChannel::rest_off_screen`).
#[derive(Debug, Clone)]
pub struct Waker {
    pub sender: HubSender,
    pub awake: Arc<AtomicBool>,
    pub off_screen: Arc<AtomicBool>,
}

/// When a pane printed since the window last looked: its first chunk and its
/// last, in milliseconds since the Unix epoch, read on the thread the output
/// arrived on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Printed {
    pub first: u64,
    pub last: u64,
}

/// What the writer thread does, in order.
enum Job {
    Write(Vec<u8>),
    Resize(u16, u16),
}

/// Where the writer thread sends what it is given.
enum Sink {
    Local(Arc<PtyManager>),
    Remote(tokio_mpsc::UnboundedSender<Outgoing>),
}

/// One pane's terminal, its writer, when it printed since the window last
/// looked, and whether it is on screen.
pub struct PaneLink {
    pub id: PaneId,
    terminal: Arc<Mutex<Terminal>>,
    jobs: mpsc::Sender<Job>,
    woken: Arc<Mutex<Option<Printed>>>,
    /// The window draws the pane: its output is looked at every frame.
    shown: Arc<AtomicBool>,
    launch: Arc<Mutex<LaunchWatch>>,
    /// The remote side, for a pane of a remote workspace.
    remote: Option<RemoteSide>,
}

/// A remote pane's stream: what the writer thread sends it, until the task
/// that follows the stream takes it, and that task, stopped with the link.
struct RemoteSide {
    node: String,
    inbox: Mutex<Option<tokio_mpsc::UnboundedReceiver<Outgoing>>>,
    stream: Mutex<Option<tokio::task::AbortHandle>>,
    alive: Arc<AtomicBool>,
}

impl Drop for PaneLink {
    fn drop(&mut self) {
        if let Some(side) = &self.remote {
            if let Some(stream) = side
                .stream
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .take()
            {
                stream.abort();
            }
            remote::unregister(&self.id);
        }
    }
}

/// What the launch of the pane's agent reads of its output: when the shell
/// last printed, and whether the command it typed came back.
#[derive(Debug, Default)]
struct LaunchWatch {
    last_output: Option<Instant>,
    echo: Option<Echo>,
}

impl std::fmt::Debug for PaneLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PaneLink").field("id", &self.id).finish()
    }
}

impl PaneLink {
    /// The terminal of pane `id`, and its writer thread. The thread ends when
    /// the last handle on the link goes.
    pub fn new(id: PaneId, terminal: Terminal, backend: PtyBackend) -> Arc<PaneLink> {
        let (jobs, inbox) = mpsc::channel();
        let writer_id = id.clone();
        let (sink, remote) = match backend {
            PtyBackend::Local(pty) => (Sink::Local(pty), None),
            PtyBackend::Remote { node } => {
                let (outgoing, stream_inbox) = tokio_mpsc::unbounded_channel();
                let alive = Arc::new(AtomicBool::new(false));
                remote::register(&id, &node, outgoing.clone(), alive.clone());
                let side = RemoteSide {
                    node,
                    inbox: Mutex::new(Some(stream_inbox)),
                    stream: Mutex::new(None),
                    alive,
                };
                (Sink::Remote(outgoing), Some(side))
            }
        };
        let spawned = std::thread::Builder::new()
            .name(format!("pane-writer-{id}"))
            .spawn(move || write_loop(&sink, &writer_id, &inbox));
        if let Err(error) = spawned {
            // Typing into this pane will go nowhere; the rest of the window
            // goes on. A machine out of threads has bigger problems.
            eprintln!("pane {id}: no writer thread: {error}");
        }
        Arc::new(PaneLink {
            id,
            terminal: Arc::new(Mutex::new(terminal)),
            jobs,
            woken: Arc::default(),
            // On screen until the window says otherwise: looked at every
            // frame, as before it knew.
            shown: Arc::new(AtomicBool::new(true)),
            launch: Arc::default(),
            remote,
        })
    }

    /// The node of a remote pane; `None` for a pane of this machine.
    pub fn node(&self) -> Option<&str> {
        self.remote.as_ref().map(|side| side.node.as_str())
    }

    /// A remote pane's shell runs (it started or was found on its node), or
    /// ended.
    pub fn set_remote_alive(&self, alive: bool) {
        if let Some(side) = &self.remote {
            side.alive.store(alive, Ordering::Release);
        }
    }

    /// Follows a remote pane's stream from now on, its node reached through
    /// the directory of connected nodes. `replay_until`: the bytes it had
    /// printed before (a shell found running on the node), drawn without
    /// being answered. Nothing for a local pane, or one already followed.
    pub fn follow_remote(&self, waker: Waker, replay_until: u64) {
        self.follow_remote_with(waker, replay_until, Backoff::default(), remote::directory);
    }

    /// [`Self::follow_remote`], with the node found by `resolve`.
    pub fn follow_remote_with(
        &self,
        waker: Waker,
        replay_until: u64,
        backoff: Backoff,
        resolve: impl Fn(&str) -> Option<NodeClient> + Send + 'static,
    ) {
        let Some(side) = &self.remote else {
            return;
        };
        let Some(outgoing) = side
            .inbox
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take()
        else {
            return;
        };
        let mut feeder = self.feeder(waker);
        let alive = side.alive.clone();
        let emit = move |event| match event {
            StreamEvent::Output { data, replay, .. } => feeder.output(&data, replay),
            StreamEvent::Reset => feeder.reset(),
            StreamEvent::Exit => {
                alive.store(false, Ordering::Release);
                feeder.exit();
            }
        };
        let task = tokio::spawn(remote::follow(
            side.node.clone(),
            self.id.clone(),
            replay_until,
            outgoing,
            emit,
            backoff,
            resolve,
        ));
        *side
            .stream
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = Some(task.abort_handle());
    }

    /// The terminal, for as long as the guard lives. Held briefly: the
    /// pane's reader thread waits on it to feed the next chunk.
    pub fn lock(&self) -> MutexGuard<'_, Terminal> {
        self.terminal
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// Bytes for the process: typing, a paste, a report.
    pub fn write(&self, bytes: Vec<u8>) {
        if !bytes.is_empty() {
            let _ = self.jobs.send(Job::Write(bytes));
        }
    }

    /// When the shell last printed, if it did.
    pub fn last_output(&self) -> Option<Instant> {
        self.watch().last_output
    }

    /// Types an agent's command (`text`, its line ending included) and
    /// starts looking for its echo (`command`). One write, so nothing the
    /// terminal answers lands in the middle of it.
    pub fn type_command(&self, command: &str, text: String) {
        self.watch().echo = Some(Echo::new(command));
        self.write(text.into_bytes());
    }

    /// Whether the command last typed with [`Self::type_command`] was
    /// echoed.
    pub fn echoed(&self) -> bool {
        self.watch().echo.as_ref().is_some_and(Echo::found)
    }

    fn watch(&self) -> MutexGuard<'_, LaunchWatch> {
        self.launch
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// Tells the process its window changed size (`pty_resize`).
    pub fn resize(&self, columns: u16, rows: u16) {
        let _ = self.jobs.send(Job::Resize(columns, rows));
    }

    /// When the pane printed since the last look, which this look is.
    pub fn take_output(&self) -> Option<Printed> {
        self.woken
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take()
    }

    /// Whether the window draws the pane (`Waker`).
    pub fn is_shown(&self) -> bool {
        self.shown.load(Ordering::Acquire)
    }

    /// The window draws the pane, or stopped drawing it. Whether that
    /// changed: the window then looks at what it printed (`take_output`),
    /// which a chunk printed before the change may have woken the other
    /// side for.
    pub fn set_shown(&self, shown: bool) -> bool {
        self.shown.swap(shown, Ordering::AcqRel) != shown
    }

    /// The sink `pty_spawn` is handed: it feeds the terminal on the reader
    /// thread and wakes the window, unless it is awake already.
    pub fn sink(&self, waker: Waker) -> PtySink {
        let mut feeder = self.feeder(waker);
        Box::new(move |event| match event {
            PtyEvent::Output(output) => feeder.output(&output.data, false),
            PtyEvent::Exit(_) => feeder.exit(),
        })
    }

    /// What feeds the terminal, for the sink of a local pane or the stream
    /// of a remote one.
    fn feeder(&self, waker: Waker) -> Feeder {
        Feeder {
            terminal: self.terminal.clone(),
            jobs: self.jobs.clone(),
            woken: self.woken.clone(),
            shown: self.shown.clone(),
            launch: self.launch.clone(),
            id: self.id.clone(),
            waker,
        }
    }
}

/// Feeds a pane's terminal on the thread its output arrives on, and wakes
/// the window, unless it is awake already.
struct Feeder {
    terminal: Arc<Mutex<Terminal>>,
    jobs: mpsc::Sender<Job>,
    woken: Arc<Mutex<Option<Printed>>>,
    shown: Arc<AtomicBool>,
    launch: Arc<Mutex<LaunchWatch>>,
    id: PaneId,
    waker: Waker,
}

impl Feeder {
    /// A chunk of output. `replay`: printed before the pane was followed (a
    /// remote shell's history), drawn without answering its queries nor
    /// ringing its bells again.
    fn output(&mut self, data: &str, replay: bool) {
        let at = crate::panes_logic::wall_clock_ms();
        {
            let mut watch = self
                .launch
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            watch.last_output = Some(Instant::now());
            if let Some(echo) = &mut watch.echo {
                echo.feed(data);
            }
        }
        let events = {
            let mut terminal = self
                .terminal
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if replay {
                terminal.replay(data.as_bytes())
            } else {
                terminal.feed(data.as_bytes())
            }
        };
        let hub = &self.waker.sender;
        for event in events {
            let kind = match event {
                Event::Write(bytes) => {
                    let _ = self.jobs.send(Job::Write(bytes));
                    continue;
                }
                Event::Bell => HubKind::Bell,
                Event::Notification { title, body } => HubKind::Notification { title, body },
                // Titles stay unused, as in the web app (inventory §4/1.12).
                // The clipboard is not offered to programs (`OSC 52`, off in
                // `look::config`), as xterm.js without its clipboard addon
                // never did.
                _ => continue,
            };
            let _ = hub.send(HubEvent {
                pane: self.id.clone(),
                kind,
            });
        }
        // After the feed, so a window that looks draws at least this chunk.
        // The pane's own record first: the window clears the shared flag
        // before it looks at the panes' (`Msg::Frame`), so a chunk it did not
        // see either finds the shared one clear and wakes it, or is seen by
        // its last look. Which flag is read after the record, so a pane the
        // window moves on or off screen meanwhile is either looked at by that
        // move (`PaneLink::set_shown`) or wakes the side it is now on.
        let fresh = {
            let mut woken = self
                .woken
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let first = woken.map_or(at, |printed| printed.first);
            let fresh = woken.is_none();
            *woken = Some(Printed { first, last: at });
            fresh
        };
        if !fresh {
            return;
        }
        let (flag, kind) = if self.shown.load(Ordering::Acquire) {
            (&self.waker.awake, HubKind::Output)
        } else {
            (&self.waker.off_screen, HubKind::OffScreen)
        };
        if !flag.swap(true, Ordering::AcqRel) {
            let _ = hub.send(HubEvent {
                pane: self.id.clone(),
                kind,
            });
        }
    }

    /// The history the terminal holds is no longer the node's: wiped (a full
    /// reset, `ESC c`), for the history that follows to be drawn again.
    fn reset(&mut self) {
        self.output("\x1bc", true);
    }

    fn exit(&mut self) {
        let _ = self.waker.sender.send(HubEvent {
            pane: self.id.clone(),
            kind: HubKind::Exit,
        });
    }
}

/// The writer thread: jobs in order, consecutive writes joined into one call
/// so a burst of keys or a long paste costs one lock of core's registry.
fn write_loop(pty: &Sink, id: &str, inbox: &mpsc::Receiver<Job>) {
    while let Ok(job) = inbox.recv() {
        match job {
            Job::Write(mut bytes) => {
                let mut next = None;
                while let Ok(job) = inbox.try_recv() {
                    match job {
                        Job::Write(more) => bytes.extend(more),
                        other => {
                            next = Some(other);
                            break;
                        }
                    }
                }
                write(pty, id, bytes);
                if let Some(Job::Resize(columns, rows)) = next {
                    resize(pty, id, columns, rows);
                }
            }
            Job::Resize(columns, rows) => resize(pty, id, columns, rows),
        }
    }
}

fn resize(pty: &Sink, id: &str, columns: u16, rows: u16) {
    match pty {
        Sink::Local(pty) => {
            let _ = hangar_core::pty::pty_resize(pty, id.to_owned(), columns, rows);
        }
        Sink::Remote(stream) => {
            let _ = stream.send(Outgoing::Resize(columns, rows));
        }
    }
}

/// `pty_write` takes text. What the terminal writes is UTF-8 but for the
/// legacy X10 mouse encoding, which puts raw bytes past 127 for columns past
/// 95; those are replaced, which only a program that asked for that encoding
/// (and not the SGR one every current TUI uses) would see.
fn write(pty: &Sink, id: &str, bytes: Vec<u8>) {
    let text = String::from_utf8(bytes)
        .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned());
    // A pane not spawned yet, or already gone, drops what was typed, as the
    // web app's `ptyWrite` did.
    match pty {
        Sink::Local(pty) => {
            let _ = hangar_core::pty::pty_write(pty, id.to_owned(), text);
        }
        Sink::Remote(stream) => {
            let _ = stream.send(Outgoing::Input(text));
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::time::{Duration, Instant};

    use hangar_core::shells::ShellInfo;
    use hangar_term::Config;

    use super::*;

    /// The whole path of a pane against a real shell: typing goes out through
    /// the writer thread, the output is parsed on the reader thread into the
    /// terminal, and the window is woken once, not once per chunk.
    #[test]
    fn a_shells_output_lands_in_the_terminal_on_its_own_thread() {
        let pty = Arc::new(PtyManager::default());
        let id = format!("p5-test-{}", std::process::id());
        let link = PaneLink::new(
            id.clone(),
            Terminal::new(Config::default(), 80, 24),
            PtyBackend::Local(pty.clone()),
        );
        let (hub, mut wakes) = tokio_mpsc::unbounded_channel();
        let awake = Arc::new(AtomicBool::new(false));
        let waker = Waker {
            sender: hub,
            awake: awake.clone(),
            off_screen: Arc::default(),
        };
        let shell = ShellInfo {
            id: "sh".into(),
            label: "sh".into(),
            program: "/bin/sh".into(),
            args: vec![],
        };
        hangar_core::pty::pty_spawn(
            &pty,
            link.sink(waker),
            id.clone(),
            std::env::temp_dir().to_string_lossy().into_owned(),
            Some(shell),
            80,
            24,
            None,
            Some(false),
            None,
            None,
            None,
            Some("shell".into()),
            None,
        )
        .expect("a shell");
        link.write(b"echo hello-$((40+2))\r".to_vec());

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut seen = false;
        while Instant::now() < deadline {
            if link
                .lock()
                .read_lines(50)
                .iter()
                .any(|line| line.contains("hello-42"))
            {
                seen = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(seen, "the echo reached the terminal");

        // One wake-up, however many chunks came, until the window rests.
        let mut outputs = 0;
        while let Ok(event) = wakes.try_recv() {
            assert_eq!(event.pane, id);
            if event.kind == HubKind::Output {
                outputs += 1;
            }
        }
        assert_eq!(outputs, 1);
        assert!(link.take_output().is_some(), "it printed");
        assert!(link.take_output().is_none(), "and was looked at");
        // Still awake: more output is the window's to find, with no message.
        link.write(b"echo awake\r".to_vec());
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline && link.take_output().is_none() {
            std::thread::sleep(Duration::from_millis(10));
        }
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            !matches!(
                wakes.try_recv(),
                Ok(HubEvent {
                    kind: HubKind::Output,
                    ..
                })
            ),
            "no wake-up while the window is awake"
        );
        // At rest, the next chunk wakes it again.
        awake.store(false, Ordering::Release);
        let _ = link.take_output();
        link.write(b"echo again\r".to_vec());
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut woken = false;
        while Instant::now() < deadline && !woken {
            woken = matches!(
                wakes.try_recv(),
                Ok(HubEvent {
                    kind: HubKind::Output,
                    ..
                })
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(woken, "a taken wake-up is sent again");

        // A command typed for an agent is followed until its echo shows.
        assert!(link.last_output().is_some(), "the shell's output is dated");
        // Keys that reached the pane first are wiped off the line, and the
        // command runs as typed.
        link.write(b"etc".to_vec());
        let command = "echo launched-$((6*7))";
        link.type_command(
            command,
            format!("{}{command}\r", crate::panes::launch::CLEAR_LINE),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        let ran = || {
            link.lock()
                .read_lines(50)
                .iter()
                .any(|line| line.trim_end() == "launched-42")
        };
        while Instant::now() < deadline && !(link.echoed() && ran()) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(link.echoed(), "the command's echo was seen");
        assert!(ran(), "the command ran, without the keys typed before it");
        let _ = hangar_core::pty::pty_kill(&pty, id, None);
    }
}

#[cfg(test)]
mod remote_tests {
    use std::time::{Duration, Instant};

    use hangar_term::Config;
    use serde_json::json;

    use super::*;
    use crate::nodes::testing::{accept_ws, listen, read_text, send_text};
    use crate::panes::controller::backend_for;
    use crate::state::Workspace;

    #[test]
    fn a_workspace_on_a_node_gets_the_remote_backend() {
        let pty = Arc::new(PtyManager::default());
        let local = Workspace::default();
        let pane = crate::state::Pane::default();
        assert!(matches!(
            backend_for(&local, &pane, &pty),
            PtyBackend::Local(_)
        ));
        let remote = Workspace {
            node: Some("n1".into()),
            ..Workspace::default()
        };
        match backend_for(&remote, &pane, &pty) {
            PtyBackend::Remote { node } => assert_eq!(node, "n1"),
            other => panic!("a remote workspace got {other:?}"),
        }
        let id = format!("backend-test-{}", std::process::id());
        let link = PaneLink::new(
            id.clone(),
            Terminal::new(Config::default(), 80, 24),
            backend_for(&remote, &pane, &pty),
        );
        assert_eq!(link.node(), Some("n1"));
        assert_eq!(remote::node_of(&id).as_deref(), Some("n1"));
        drop(link);
        assert_eq!(remote::node_of(&id), None, "forgotten with its link");
    }

    /// A worker a local workspace sent to a node runs there; the others of
    /// that workspace stay on this machine.
    #[test]
    fn a_pane_on_a_node_of_a_local_workspace_gets_the_remote_backend() {
        let pty = Arc::new(PtyManager::default());
        let local = Workspace::default();
        let worker = crate::state::Pane {
            node: Some("n2".into()),
            ..crate::state::Pane::default()
        };
        match backend_for(&local, &worker, &pty) {
            PtyBackend::Remote { node } => assert_eq!(node, "n2"),
            other => panic!("a worker on a node got {other:?}"),
        }
        let shell = crate::state::Pane::default();
        assert!(matches!(
            backend_for(&local, &shell, &pty),
            PtyBackend::Local(_)
        ));
    }

    async fn until(what: &str, done: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting: {what}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// A remote pane end to end against a fake node: its output lands in the
    /// terminal through the sink's own feeding (the window woken once), what
    /// is typed and the size go out on the stream, a drop resumes from the
    /// last byte, a reset clears, and the shell's end reaches the window.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_remote_panes_stream_feeds_its_terminal() {
        let (listener, client) = listen().await;
        let id = format!("remote-link-{}", std::process::id());
        let link = PaneLink::new(
            id.clone(),
            Terminal::new(Config::default(), 80, 24),
            PtyBackend::Remote { node: "n".into() },
        );
        let (hub, mut wakes) = tokio_mpsc::unbounded_channel();
        let waker = Waker {
            sender: hub,
            awake: Arc::new(AtomicBool::new(false)),
            off_screen: Arc::default(),
        };
        let fast = Backoff {
            first: Duration::from_millis(10),
            max: Duration::from_millis(50),
        };
        link.follow_remote_with(waker, 0, fast, move |_| Some(client.clone()));

        let (mut first, request) = accept_ws(&listener).await;
        assert!(request.line().contains("/stream?from=0 "));
        send_text(
            &mut first,
            &json!({ "seq": 13, "data": "hello-remote\r\n" }).to_string(),
        )
        .await;
        until("the output in the terminal", || {
            link.lock()
                .read_lines(10)
                .iter()
                .any(|line| line.contains("hello-remote"))
        })
        .await;
        assert!(link.take_output().is_some(), "the pane printed");
        let woken = wakes.recv().await.unwrap();
        assert_eq!((woken.pane, woken.kind), (id.clone(), HubKind::Output));

        // Typing and resizing go out through the writer thread to the stream.
        link.write(b"uname -a\r".to_vec());
        link.resize(100, 30);
        let mut buf = Vec::new();
        assert_eq!(
            read_text(&mut first, &mut buf).await,
            json!({ "input": "uname -a\r" }).to_string()
        );
        assert_eq!(
            read_text(&mut first, &mut buf).await,
            json!({ "resize": { "cols": 100, "rows": 30 } }).to_string()
        );
        drop(first);

        // The drop: resumed from the last byte, the size said again.
        let (mut second, request) = accept_ws(&listener).await;
        assert!(
            request.line().contains("/stream?from=13 "),
            "{}",
            request.line()
        );
        let mut buf = Vec::new();
        assert!(read_text(&mut second, &mut buf).await.contains("resize"));
        send_text(&mut second, &json!({ "reset": { "start": 0 } }).to_string()).await;
        send_text(
            &mut second,
            &json!({ "seq": 30, "data": "after-reset\r\n" }).to_string(),
        )
        .await;
        until("the history after the reset", || {
            let lines = link.lock().read_lines(10);
            lines.iter().any(|line| line.contains("after-reset"))
                && !lines.iter().any(|line| line.contains("hello-remote"))
        })
        .await;
        link.set_remote_alive(true);
        send_text(&mut second, &json!({ "exit": { "code": 0 } }).to_string()).await;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let event = tokio::time::timeout_at(deadline.into(), wakes.recv())
                .await
                .expect("the exit reached the window")
                .unwrap();
            if event.kind == HubKind::Exit {
                break;
            }
        }
        assert_eq!(remote::alive(&id), Some(false), "its shell ended");
    }
}
