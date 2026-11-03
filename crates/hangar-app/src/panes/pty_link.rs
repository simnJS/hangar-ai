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
//! two frames. What the window is told is a wake-up, sent once until the
//! window has taken it ([`PaneLink::take_wake`]), so a pane printing a build
//! log costs one message per frame at most, not one per read.
//!
//! The terminal's answers to the program's queries (cursor position, device
//! attributes, colours) come out of the same feed and go straight to the
//! pane's writer, in order with the typing; the other things the terminal
//! has to say (a bell, a notification, a copy) go to the window as
//! [`HubEvent`]s.
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

use hangar_core::pty::{PtyEvent, PtyManager, PtySink};
use hangar_term::{ClipboardTarget, Event, Terminal};
use tokio::sync::mpsc as tokio_mpsc;

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
    /// A program asked to copy (`OSC 52`).
    Copy(String),
}

/// The sending half of the window's channel, cloned into every sink.
pub type HubSender = tokio_mpsc::UnboundedSender<HubEvent>;

/// What the writer thread does, in order.
enum Job {
    Write(Vec<u8>),
    Resize(u16, u16),
}

/// One pane's terminal, its writer, and the flag that keeps its wake-ups to
/// one in flight.
pub struct PaneLink {
    pub id: PaneId,
    terminal: Arc<Mutex<Terminal>>,
    jobs: mpsc::Sender<Job>,
    woken: Arc<AtomicBool>,
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

    /// Tells the process its window changed size (`pty_resize`).
    pub fn resize(&self, columns: u16, rows: u16) {
        let _ = self.jobs.send(Job::Resize(columns, rows));
    }

    /// Takes the wake-up: the next chunk the pane prints sends another.
    pub fn take_wake(&self) {
        self.woken.store(false, Ordering::Release);
    }

    /// The sink `pty_spawn` is handed: it feeds the terminal on the reader
    /// thread and wakes the window.
    pub fn sink(&self, hub: HubSender) -> PtySink {
        let terminal = self.terminal.clone();
        let jobs = self.jobs.clone();
        let woken = self.woken.clone();
        let id = self.id.clone();
        Box::new(move |event| match event {
            PtyEvent::Output(output) => {
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
                        Event::SetClipboard {
                            target: ClipboardTarget::Clipboard,
                            text,
                        } => HubKind::Copy(text),
                        // Titles stay unused, as in the web app (inventory
                        // §4/1.12); the primary selection and reading the
                        // clipboard are not offered to programs.
                        _ => continue,
                    };
                    let _ = hub.send(HubEvent {
                        pane: id.clone(),
                        kind,
                    });
                }
                // After the feed, so a window that takes the wake-up draws
                // at least this chunk.
                if !woken.swap(true, Ordering::AcqRel) {
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
