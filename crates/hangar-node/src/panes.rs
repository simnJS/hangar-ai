//! The node's panes: each spawned through core's `PtyManager`, its output
//! kept in a bounded history numbered by byte (`seq`, as core numbers it:
//! the bytes printed so far, the chunk included) and broadcast to the
//! streams watching it (api.rs). A desktop that reconnects asks for what came
//! after the last `seq` it saw.
//!
//! Core hands over what each read of the PTY gave, a line or less while a
//! command floods it (50 bytes on average for `seq`): the history joins
//! small chunks, and a stream sends the live frames already waiting as one
//! ([`gather`]), so a flood is a few large frames rather than one per line.
#![cfg(target_os = "linux")]

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use hangar_core::pty::{self, PtyEvent, PtyManager};
use serde::Serialize;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::{RecvError, TryRecvError};

/// How much of a pane's output is kept for a desktop catching up: a full
/// screen redrawn many times over, and not so much that a dozen panes weigh.
pub const HISTORY_BYTES: usize = 4 * 1024 * 1024;

/// How many frames a slow stream may fall behind before it catches up from
/// the history instead (api.rs).
const LIVE_CAPACITY: usize = 1024;

/// A chunk of the history smaller than this takes the next one in: fewer,
/// larger chunks to keep and to replay.
pub const HISTORY_JOIN: usize = 16 * 1024;

/// The most output a stream joins into one frame ([`gather`]).
pub const BATCH_BYTES: usize = 64 * 1024;

/// What a pane's stream is sent live.
#[derive(Debug, Clone)]
pub enum Frame {
    Output {
        seq: u64,
        data: Arc<str>,
    },
    /// The shell ended, or the pane was killed: the last frame.
    Exit {
        code: Option<u32>,
    },
}

/// A pane's output, oldest chunk first, whole chunks only, at most
/// `capacity` bytes of them (and always the latest chunk, however large).
/// A chunk under `join` bytes takes the next one in.
#[derive(Debug)]
pub struct History {
    chunks: VecDeque<(u64, String)>,
    bytes: usize,
    capacity: usize,
    join: usize,
    /// The `seq` the oldest chunk kept starts at.
    start: u64,
    /// The `seq` of the last chunk: every byte the pane printed.
    end: u64,
}

/// What a stream starting at `from` is sent first.
#[derive(Debug, PartialEq, Eq)]
pub struct Replay {
    /// `Some(start)` when `from` fell out of the history: what is sent then
    /// starts at `start`, and the desktop clears its screen first.
    pub reset: Option<u64>,
    /// `(seq, data)`, each chunk's `seq` being the end of it.
    pub chunks: Vec<(u64, String)>,
}

impl History {
    pub fn new(capacity: usize, join: usize) -> Self {
        Self {
            chunks: VecDeque::new(),
            bytes: 0,
            capacity,
            join,
            start: 0,
            end: 0,
        }
    }

    /// Core's chunk, whose `seq` counts it in already.
    pub fn push(&mut self, seq: u64, data: &str) {
        self.end = seq;
        self.bytes += data.len();
        match self.chunks.back_mut() {
            Some((end, last)) if last.len() < self.join => {
                last.push_str(data);
                *end = seq;
            }
            _ => self.chunks.push_back((seq, data.to_owned())),
        }
        while self.bytes > self.capacity && self.chunks.len() > 1 {
            if let Some((seq, data)) = self.chunks.pop_front() {
                self.bytes -= data.len();
                self.start = seq;
            }
        }
    }

    /// Everything after byte `from`. A `from` inside a chunk (a stream's
    /// last byte, in a chunk joined since) starts at the next character.
    pub fn after(&self, from: u64) -> Replay {
        let reset = (from < self.start || from > self.end).then_some(self.start);
        let from = if reset.is_some() { self.start } else { from };
        let mut chunks = Vec::new();
        for (seq, data) in &self.chunks {
            if *seq <= from {
                continue;
            }
            let begins = seq - data.len() as u64;
            if begins >= from {
                chunks.push((*seq, data.clone()));
            } else {
                let mut cut = (from - begins) as usize;
                while !data.is_char_boundary(cut) {
                    cut += 1;
                }
                chunks.push((*seq, data[cut..].to_string()));
            }
        }
        Replay { reset, chunks }
    }
}

/// What stopped [`gather`] short of the frames waiting.
#[derive(Debug, PartialEq, Eq)]
pub enum Then {
    /// The pane ended: the last frame.
    Exit(Option<u32>),
    /// The stream fell behind the live frames: it catches up from the
    /// history.
    Lagged,
    /// The pane is gone.
    Closed,
}

/// The live frames a stream has waiting, `first` (what it was woken with)
/// and those already behind it, as one output: their data joined, up to
/// about `max` bytes, ending at the last `seq`; those up to `last` (sent
/// already) left out. Never waits: a key's echo goes alone, at once, and a
/// flood goes in large frames. `None` for the output when there was none;
/// the end, a lag or a closing that came in between after it.
pub fn gather(
    first: Result<Frame, RecvError>,
    live: &mut broadcast::Receiver<Frame>,
    last: u64,
    max: usize,
) -> (Option<(u64, String)>, Option<Then>) {
    let mut joined: Option<(u64, String)> = None;
    let mut next = first;
    loop {
        match next {
            Ok(Frame::Output { seq, data }) => {
                let upto = joined.as_ref().map_or(last, |(end, _)| *end);
                if seq > upto {
                    match &mut joined {
                        Some((end, text)) => {
                            text.push_str(&data);
                            *end = seq;
                        }
                        None => joined = Some((seq, data.to_string())),
                    }
                }
                if joined.as_ref().is_some_and(|(_, text)| text.len() >= max) {
                    return (joined, None);
                }
            }
            Ok(Frame::Exit { code }) => return (joined, Some(Then::Exit(code))),
            Err(RecvError::Lagged(_)) => return (joined, Some(Then::Lagged)),
            Err(RecvError::Closed) => return (joined, Some(Then::Closed)),
        }
        next = match live.try_recv() {
            Ok(frame) => Ok(frame),
            Err(TryRecvError::Empty) => return (joined, None),
            Err(TryRecvError::Lagged(missed)) => Err(RecvError::Lagged(missed)),
            Err(TryRecvError::Closed) => Err(RecvError::Closed),
        };
    }
}

/// An output frame as a stream that asked for binary frames is sent it
/// (the `hangar-binary` subprotocol, api.rs): the `seq` in 8 bytes,
/// big-endian, then the data, as is.
/// A JSON frame escapes every control character of a terminal's output
/// into six bytes, and both sides parse it.
pub fn binary_frame(seq: u64, data: &str) -> Vec<u8> {
    let mut frame = Vec::with_capacity(8 + data.len());
    frame.extend_from_slice(&seq.to_be_bytes());
    frame.extend_from_slice(data.as_bytes());
    frame
}

/// What `GET /node/panes` lists of a pane.
#[derive(Debug, Clone, Serialize)]
pub struct Info {
    pub id: String,
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    pub cols: u16,
    pub rows: u16,
    /// Every byte the pane printed so far.
    pub seq: u64,
    /// False once the shell ended on its own: the pane stays listed, with
    /// its history, until it is deleted.
    pub alive: bool,
}

struct State {
    info: Info,
    history: History,
    /// `Some` once ended: the code, which core never knows (`PtyExit`).
    exit: Option<Option<u32>>,
}

pub struct Pane {
    state: Mutex<State>,
    live: broadcast::Sender<Frame>,
}

/// What a stream gets on attaching: the history it missed, whether the pane
/// ended already, and the live frames from there on, with nothing lost or
/// sent twice in between (taken under the one lock the sink writes under).
pub struct Attached {
    pub replay: Replay,
    pub exit: Option<Option<u32>>,
    pub live: broadcast::Receiver<Frame>,
}

impl Pane {
    fn new(info: Info, capacity: usize, join: usize) -> Self {
        Self {
            state: Mutex::new(State {
                info,
                history: History::new(capacity, join),
                exit: None,
            }),
            live: broadcast::channel(LIVE_CAPACITY).0,
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn attach(&self, from: u64) -> Attached {
        let state = self.lock();
        Attached {
            replay: state.history.after(from),
            exit: state.exit,
            live: self.live.subscribe(),
        }
    }

    pub fn info(&self) -> Info {
        self.lock().info.clone()
    }

    fn output(&self, seq: u64, data: String) {
        let mut state = self.lock();
        state.history.push(seq, &data);
        let frame = Frame::Output {
            seq,
            data: Arc::from(data),
        };
        state.info.seq = seq;
        let _ = self.live.send(frame);
    }

    /// The first end only: a pane killed after its shell ended stays ended.
    fn ended(&self, code: Option<u32>) {
        let mut state = self.lock();
        if state.exit.is_none() {
            state.exit = Some(code);
            state.info.alive = false;
            let _ = self.live.send(Frame::Exit { code });
        }
    }

    fn resized(&self, cols: u16, rows: u16) {
        let mut state = self.lock();
        state.info.cols = cols;
        state.info.rows = rows;
    }
}

/// What spawning a pane takes (POST /node/panes, api.rs).
pub struct Spawn {
    pub id: String,
    pub cwd: String,
    pub shell: Option<hangar_core::shells::ShellInfo>,
    pub cols: u16,
    pub rows: u16,
    pub name: Option<String>,
    pub agent: Option<String>,
    pub bridge: Option<bool>,
    pub port_base: Option<u16>,
    pub build_env: Vec<(String, String)>,
    pub mcp_config: Option<String>,
    pub kits: Option<Vec<String>>,
    pub claude_account: Option<String>,
}

pub struct Panes {
    pty: Arc<PtyManager>,
    panes: Mutex<HashMap<String, Arc<Pane>>>,
}

impl Panes {
    pub fn new(pty: Arc<PtyManager>) -> Self {
        Self {
            pty,
            panes: Mutex::new(HashMap::new()),
        }
    }

    fn map(&self) -> MutexGuard<'_, HashMap<String, Arc<Pane>>> {
        self.panes.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn get(&self, id: &str) -> Option<Arc<Pane>> {
        self.map().get(id).cloned()
    }

    pub fn list(&self) -> Vec<Info> {
        let mut list: Vec<Info> = self.map().values().map(|pane| pane.info()).collect();
        list.sort_by(|a, b| a.id.cmp(&b.id));
        list
    }

    /// Blocks while the shell starts. An id whose shell is still running
    /// fails (core refuses it); one that ended takes the new shell, with a
    /// new history.
    pub fn spawn(&self, spawn: Spawn) -> Result<pty::Spawned, String> {
        let pane = Arc::new(Pane::new(
            Info {
                id: spawn.id.clone(),
                cwd: spawn.cwd.clone(),
                name: spawn.name.clone(),
                agent: spawn.agent.clone(),
                cols: spawn.cols,
                rows: spawn.rows,
                seq: 0,
                alive: true,
            },
            HISTORY_BYTES,
            HISTORY_JOIN,
        ));
        let sink_pane = pane.clone();
        let sink: pty::PtySink = Box::new(move |event| match event {
            PtyEvent::Output(output) => sink_pane.output(output.seq, output.data),
            PtyEvent::Exit(exit) => sink_pane.ended(exit.code),
        });
        // Booked before the shell starts, so its first bytes have a pane to
        // land in; put back as it was when the spawn fails.
        let previous = self.map().insert(spawn.id.clone(), pane);
        let spawned = pty::pty_spawn_with(
            &self.pty,
            sink,
            spawn.id.clone(),
            spawn.cwd,
            spawn.shell,
            spawn.cols,
            spawn.rows,
            spawn.name,
            spawn.bridge,
            spawn.port_base,
            spawn.build_env,
            spawn.mcp_config,
            spawn.kits,
            spawn.agent,
            spawn.claude_account,
            false,
        );
        if spawned.is_err() {
            let mut map = self.map();
            match previous {
                Some(previous) => map.insert(spawn.id, previous),
                None => map.remove(&spawn.id),
            };
        }
        spawned
    }

    pub fn write(&self, id: &str, data: String) -> Result<(), String> {
        pty::pty_write(&self.pty, id.to_string(), data)
    }

    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<(), String> {
        pty::pty_resize(&self.pty, id.to_string(), cols, rows)?;
        if let Some(pane) = self.get(id) {
            pane.resized(cols, rows);
        }
        Ok(())
    }

    /// The pane's whole process tree, and the pane forgotten: its streams
    /// are sent an exit and end. `false` when there was no such pane.
    pub fn kill(&self, id: &str) -> bool {
        let pane = self.map().remove(id);
        // A shell core runs that this map lost (none should) is killed too.
        let running = pty::pty_alive(&self.pty, id.to_string());
        if pane.is_none() && !running {
            return false;
        }
        let _ = pty::pty_kill(&self.pty, id.to_string(), Some(true));
        if let Some(pane) = &pane {
            pane.ended(None);
        }
        true
    }

    pub fn kill_all(&self) {
        let panes: Vec<Arc<Pane>> = self.map().drain().map(|(_, pane)| pane).collect();
        pty::kill_all(&self.pty);
        for pane in panes {
            pane.ended(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Chunks as core numbers them: each `seq` the bytes so far.
    fn history(capacity: usize, chunks: &[&str]) -> History {
        joined(capacity, 0, chunks)
    }

    fn joined(capacity: usize, join: usize, chunks: &[&str]) -> History {
        let mut history = History::new(capacity, join);
        let mut seq = 0;
        for chunk in chunks {
            seq += chunk.len() as u64;
            history.push(seq, chunk);
        }
        history
    }

    #[test]
    fn replays_what_came_after_a_seq() {
        let history = history(100, &["one", "two", "three"]);
        assert_eq!(history.end, 11);
        assert_eq!(
            history.after(3),
            Replay {
                reset: None,
                chunks: vec![(6, "two".into()), (11, "three".into())]
            }
        );
        assert_eq!(history.after(11).chunks, vec![]);
        assert_eq!(history.after(0).chunks.len(), 3);
    }

    #[test]
    fn a_seq_inside_a_chunk_starts_at_the_next_character() {
        let history = history(100, &["héllo"]);
        // Byte 2 is inside the é.
        assert_eq!(history.after(2).chunks, vec![(6, "llo".into())]);
        assert_eq!(history.after(1).chunks, vec![(6, "éllo".into())]);
    }

    #[test]
    fn a_seq_that_fell_out_is_a_reset_and_the_whole_history() {
        let history = history(6, &["aaa", "bbb", "ccc"]);
        assert_eq!(history.start, 3);
        let replay = history.after(1);
        assert_eq!(replay.reset, Some(3));
        assert_eq!(replay.chunks, vec![(6, "bbb".into()), (9, "ccc".into())]);
        // One the pane never reached (a node restarted under the desktop).
        assert_eq!(history.after(50).reset, Some(3));
        assert_eq!(history.after(3).reset, None);
    }

    #[test]
    fn the_latest_chunk_stays_however_large() {
        let history = history(4, &["ab", "cdefgh"]);
        assert_eq!(history.after(0).chunks, vec![(8, "cdefgh".into())]);
        assert_eq!(history.after(0).reset, Some(2));
    }

    #[test]
    fn small_chunks_are_joined_and_replayed_from_any_seq() {
        let history = joined(100, 4, &["ab", "cd", "ef", "gh", "é", "z"]);
        // "ab" takes "cd" (2 < 4), then is full; "ef" takes "gh"; "é" takes "z".
        assert_eq!(history.chunks.len(), 3);
        assert_eq!(
            history.after(0).chunks,
            vec![(4, "abcd".into()), (8, "efgh".into()), (11, "éz".into())]
        );
        // A stream's last byte, now inside a joined chunk.
        assert_eq!(
            history.after(6).chunks,
            vec![(8, "gh".into()), (11, "éz".into())]
        );
        assert_eq!(history.after(10).chunks, vec![(11, "z".into())]);
        assert_eq!(history.after(11).chunks, vec![]);
        assert_eq!(history.end, 11);
        assert_eq!(history.bytes, 11);
    }

    #[test]
    fn joined_chunks_still_fall_out_whole() {
        let history = joined(6, 4, &["ab", "cd", "ef", "gh", "ij"]);
        // "abcd" went: 10 bytes > 6; "efgh" + "ij" (4 is not < 4) stay.
        assert_eq!(history.start, 4);
        assert_eq!(history.after(0).reset, Some(4));
        assert_eq!(
            history.after(4).chunks,
            vec![(8, "efgh".into()), (10, "ij".into())]
        );
    }

    fn output(seq: u64, data: &str) -> Frame {
        Frame::Output {
            seq,
            data: Arc::from(data),
        }
    }

    #[test]
    fn gather_joins_the_frames_waiting_and_never_waits() {
        let (sender, mut live) = broadcast::channel(16);
        sender.send(output(4, "cd")).unwrap();
        sender.send(output(6, "ef")).unwrap();
        let first = Ok(output(2, "ab"));
        assert_eq!(
            gather(first, &mut live, 0, 1024),
            (Some((6, "abcdef".into())), None)
        );
        // Nothing waiting: a key's echo goes alone.
        assert_eq!(
            gather(Ok(output(7, "g")), &mut live, 6, 1024),
            (Some((7, "g".into())), None)
        );
    }

    #[test]
    fn gather_leaves_out_what_was_sent_and_stops_at_the_size() {
        let (sender, mut live) = broadcast::channel(16);
        sender.send(output(4, "cd")).unwrap();
        sender.send(output(6, "ef")).unwrap();
        sender.send(output(8, "gh")).unwrap();
        // Up to 2 sent already (a catch-up); 4 bytes is the most.
        assert_eq!(
            gather(Ok(output(2, "ab")), &mut live, 2, 4),
            (Some((6, "cdef".into())), None)
        );
        // The rest is the next frame's.
        let next = Ok(live.try_recv().unwrap());
        assert_eq!(
            gather(next, &mut live, 6, 4),
            (Some((8, "gh".into())), None)
        );
    }

    #[test]
    fn gather_stops_at_the_end_a_lag_or_a_closing() {
        let (sender, mut live) = broadcast::channel(16);
        sender.send(output(4, "cd")).unwrap();
        sender.send(Frame::Exit { code: Some(3) }).unwrap();
        assert_eq!(
            gather(Ok(output(2, "ab")), &mut live, 0, 1024),
            (Some((4, "abcd".into())), Some(Then::Exit(Some(3))))
        );
        assert_eq!(
            gather(Err(RecvError::Lagged(5)), &mut live, 4, 1024),
            (None, Some(Then::Lagged))
        );

        let (sender, mut live) = broadcast::channel(2);
        for seq in 2..=5 {
            sender.send(output(seq, "x")).unwrap();
        }
        // Behind by more than the channel holds, after the first frame.
        assert_eq!(
            gather(Ok(output(1, "x")), &mut live, 0, 1024),
            (Some((1, "x".into())), Some(Then::Lagged))
        );
        drop(sender);
        let (_, mut gone) = broadcast::channel::<Frame>(2);
        assert_eq!(
            gather(Err(RecvError::Closed), &mut gone, 0, 1024),
            (None, Some(Then::Closed))
        );
    }

    #[test]
    fn a_binary_frame_is_the_seq_then_the_data() {
        let frame = binary_frame(0x0102_0304_0506_0708, "é\x1b[0m");
        assert_eq!(&frame[..8], &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(&frame[8..], "é\x1b[0m".as_bytes());
    }
}
