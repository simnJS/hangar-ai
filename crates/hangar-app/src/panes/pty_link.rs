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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::time::Instant;

use hangar_core::pty::{PtyEvent, PtyManager, PtySink};
use hangar_term::{Event, Terminal};
use tokio::sync::mpsc as tokio_mpsc;

use super::launch::Echo;
use crate::state::PaneId;

/// What a pane's reader thread tells the window.
#[derive(Debug, Clone)]
pub struct HubEvent {
    pub pane: PaneId,
    pub kind: HubKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HubKind {
    /// The pane printed something since the window last looked.
    Output,
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
/// shares that says the window already knows output is coming, because a
/// wake-up is on its way or it is looking at the panes every frame. Set by
/// the sink that sends the wake-up, cleared by the window once a frame finds
/// every pane quiet (`HubChannel::rest`).
#[derive(Debug, Clone)]
pub struct Waker {
    pub sender: HubSender,
    pub awake: Arc<AtomicBool>,
}

/// What the writer thread does, in order.
enum Job {
    Write(Vec<u8>),
    Resize(u16, u16),
}

/// One pane's terminal, its writer, and the flag that says it printed since
/// the window last looked.
pub struct PaneLink {
    pub id: PaneId,
    terminal: Arc<Mutex<Terminal>>,
    jobs: mpsc::Sender<Job>,
    woken: Arc<AtomicBool>,
    launch: Arc<Mutex<LaunchWatch>>,
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
    pub fn new(id: PaneId, terminal: Terminal, pty: Arc<PtyManager>) -> Arc<PaneLink> {
        let (jobs, inbox) = mpsc::channel();
        let writer_id = id.clone();
        let spawned = std::thread::Builder::new()
            .name(format!("pane-writer-{id}"))
            .spawn(move || write_loop(&pty, &writer_id, &inbox));
        if let Err(error) = spawned {
            // Typing into this pane will go nowhere; the rest of the window
            // goes on. A machine out of threads has bigger problems.
            eprintln!("pane {id}: no writer thread: {error}");
        }
        Arc::new(PaneLink {
            id,
            terminal: Arc::new(Mutex::new(terminal)),
            jobs,
            woken: Arc::new(AtomicBool::new(false)),
            launch: Arc::default(),
        })
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

    /// Whether the pane printed since the last look, which this look is.
    pub fn take_output(&self) -> bool {
        self.woken.swap(false, Ordering::AcqRel)
    }

    /// The sink `pty_spawn` is handed: it feeds the terminal on the reader
    /// thread and wakes the window, unless it is awake already.
    pub fn sink(&self, waker: Waker) -> PtySink {
        let terminal = self.terminal.clone();
        let jobs = self.jobs.clone();
        let woken = self.woken.clone();
        let launch = self.launch.clone();
        let id = self.id.clone();
        let Waker { sender: hub, awake } = waker;
        Box::new(move |event| match event {
            PtyEvent::Output(output) => {
                {
                    let mut watch = launch.lock().unwrap_or_else(|poison| poison.into_inner());
                    watch.last_output = Some(Instant::now());
                    if let Some(echo) = &mut watch.echo {
                        echo.feed(&output.data);
                    }
                }
                let events = terminal
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .feed(output.data.as_bytes());
                for event in events {
                    let kind = match event {
                        Event::Write(bytes) => {
                            let _ = jobs.send(Job::Write(bytes));
                            continue;
                        }
                        Event::Bell => HubKind::Bell,
                        Event::Notification { title, body } => {
                            HubKind::Notification { title, body }
                        }
                        // Titles stay unused, as in the web app (inventory
                        // §4/1.12). The clipboard is not offered to
                        // programs (`OSC 52`, off in `look::config`), as
                        // xterm.js without its clipboard addon never did.
                        _ => continue,
                    };
                    let _ = hub.send(HubEvent {
                        pane: id.clone(),
                        kind,
                    });
                }
                // After the feed, so a window that looks draws at least this
                // chunk. The pane's own flag first: the window clears the
                // shared one before it looks at the panes' (`Msg::Frame`), so
                // a chunk it did not see either finds the shared one clear
                // and wakes it, or is seen by its last look.
                if !woken.swap(true, Ordering::AcqRel) && !awake.swap(true, Ordering::AcqRel) {
                    let _ = hub.send(HubEvent {
                        pane: id.clone(),
                        kind: HubKind::Output,
                    });
                }
            }
            PtyEvent::Exit(_) => {
                let _ = hub.send(HubEvent {
                    pane: id.clone(),
                    kind: HubKind::Exit,
                });
            }
        })
    }
}

/// The writer thread: jobs in order, consecutive writes joined into one call
/// so a burst of keys or a long paste costs one lock of core's registry.
fn write_loop(pty: &PtyManager, id: &str, inbox: &mpsc::Receiver<Job>) {
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
                    let _ = hangar_core::pty::pty_resize(pty, id.to_owned(), columns, rows);
                }
            }
            Job::Resize(columns, rows) => {
                let _ = hangar_core::pty::pty_resize(pty, id.to_owned(), columns, rows);
            }
        }
    }
}

/// `pty_write` takes text. What the terminal writes is UTF-8 but for the
/// legacy X10 mouse encoding, which puts raw bytes past 127 for columns past
/// 95; those are replaced, which only a program that asked for that encoding
/// (and not the SGR one every current TUI uses) would see.
fn write(pty: &PtyManager, id: &str, bytes: Vec<u8>) {
    let text = String::from_utf8(bytes)
        .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned());
    // A pane not spawned yet, or already gone, drops what was typed, as the
    // web app's `ptyWrite` did.
    let _ = hangar_core::pty::pty_write(pty, id.to_owned(), text);
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
            pty.clone(),
        );
        let (hub, mut wakes) = tokio_mpsc::unbounded_channel();
        let awake = Arc::new(AtomicBool::new(false));
        let waker = Waker {
            sender: hub,
            awake: awake.clone(),
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
        assert!(link.take_output(), "it printed");
        assert!(!link.take_output(), "and was looked at");
        // Still awake: more output is the window's to find, with no message.
        link.write(b"echo awake\r".to_vec());
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline && !link.take_output() {
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
        let command = "echo launched-$((6*7))";
        link.type_command(command, format!("{command}\r"));
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline && !link.echoed() {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(link.echoed(), "the command's echo was seen");
        let _ = hangar_core::pty::pty_kill(&pty, id, None);
    }
}
