//! Reading a pane's output in pieces as big as what is already there.
//!
//! A program flooding its terminal (a build log, `seq 1 1000000`) writes
//! it a line at a time, and the reader thread, woken by each, used to hand
//! out ~50 bytes per read: one event, one parse, one wake-up and one stream
//! frame per line. After the read that waits, [`read_some`] now takes, without
//! waiting, whatever else the PTY already holds, up to [`MOST`], and only then
//! hands the lot over. It never waits for more: a keystroke's echo, alone in
//! the PTY, still comes out with the read that found it.

use std::io::{self, Read};

/// The most one read hands over: what a flood joins into one event.
pub(super) const MOST: usize = 64 * 1024;

/// A pane's output, and whether more of it can be read without waiting.
pub(super) trait Source: Read {
    /// True when a read would return at once: bytes, or the end, are there.
    /// Never blocks.
    fn ready(&mut self) -> bool;
}

/// Reads into `out` what the source has: waits for the first bytes, then
/// adds those already there, without waiting, up to [`MOST`] in all. False
/// once the source has ended (its end, or an error, as the reader thread
/// always took it); what was read before that is in `out` all the same.
pub(super) fn read_some(source: &mut dyn Source, buf: &mut [u8], out: &mut Vec<u8>) -> bool {
    let mut filled = 0;
    let open = loop {
        match source.read(&mut buf[filled..]) {
            Ok(0) => break false,
            Ok(n) => filled += n,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break false,
        }
        if filled == buf.len() || !source.ready() {
            break true;
        }
    };
    out.extend_from_slice(&buf[..filled]);
    open
}

/// The reader of a pane's PTY: a descriptor of its own on the master, which
/// `poll` can be asked about without blocking.
#[cfg(unix)]
struct Fd(std::fs::File);

#[cfg(unix)]
impl Read for Fd {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

#[cfg(unix)]
impl Source for Fd {
    fn ready(&mut self) -> bool {
        use std::os::fd::AsRawFd;
        let mut fd = libc::pollfd {
            fd: self.0.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // A zero timeout: answers what is there now. POLLHUP and POLLERR
        // count as ready too, for the read to see the end.
        let polled = unsafe { libc::poll(&mut fd, 1, 0) };
        polled > 0 && fd.revents != 0
    }
}

/// Where nothing says whether a read would wait (Windows' ConPTY pipe): one
/// read at a time, as before.
#[cfg(not(unix))]
struct Blocking(Box<dyn Read + Send>);

#[cfg(not(unix))]
impl Read for Blocking {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

#[cfg(not(unix))]
impl Source for Blocking {
    fn ready(&mut self) -> bool {
        false
    }
}

/// The reader of the PTY whose master is `master`.
#[cfg(unix)]
pub(super) fn reader(
    master: &dyn portable_pty::MasterPty,
) -> Result<Box<dyn Source + Send>, String> {
    use std::os::fd::BorrowedFd;
    let fd = master
        .as_raw_fd()
        .ok_or("the pty's master has no descriptor")?;
    // A descriptor of the reader's own (close-on-exec, as portable-pty's
    // would be), so it stays valid for as long as the reader thread reads,
    // whatever becomes of the master.
    let owned = unsafe { BorrowedFd::borrow_raw(fd) }
        .try_clone_to_owned()
        .map_err(|e| e.to_string())?;
    Ok(Box::new(Fd(std::fs::File::from(owned))))
}

#[cfg(not(unix))]
pub(super) fn reader(
    master: &dyn portable_pty::MasterPty,
) -> Result<Box<dyn Source + Send>, String> {
    let reader = master.try_clone_reader().map_err(|e| e.to_string())?;
    Ok(Box::new(Blocking(reader)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// What a PTY does to a flood: the bytes in pieces of the size the
    /// program wrote them, one piece per read however big the buffer.
    struct Pieces(VecDeque<Vec<u8>>);

    impl Read for Pieces {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let Some(mut piece) = self.0.pop_front() else {
                return Ok(0);
            };
            let n = piece.len().min(buf.len());
            if n < piece.len() {
                self.0.push_front(piece.split_off(n));
            }
            buf[..n].copy_from_slice(&piece);
            Ok(n)
        }
    }

    impl Source for Pieces {
        fn ready(&mut self) -> bool {
            true
        }
    }

    fn events(source: &mut dyn Source) -> Vec<Vec<u8>> {
        let mut buf = vec![0; MOST];
        let mut events = Vec::new();
        loop {
            let mut out = Vec::new();
            let open = read_some(source, &mut buf, &mut out);
            if !out.is_empty() {
                events.push(out);
            }
            if !open {
                return events;
            }
        }
    }

    #[test]
    fn a_flood_of_small_writes_comes_out_whole_in_order_in_few_events() {
        let lines: Vec<Vec<u8>> = (0..5000)
            .map(|i| format!("line {i:05} of the flood\n").into_bytes())
            .collect();
        let written: Vec<u8> = lines.concat();
        let events = events(&mut Pieces(lines.into()));
        assert_eq!(events.concat(), written);
        // 5000 writes, ~120 KiB: two events of at most 64 KiB, not 5000.
        assert_eq!(events.len(), written.len().div_ceil(MOST));
        assert!(events.iter().all(|event| event.len() <= MOST));
    }

    /// A real pipe, as the PTY is read: the second write is still to come
    /// when the first is handed over.
    #[cfg(unix)]
    #[test]
    fn a_single_small_write_comes_out_at_once() {
        use std::io::Write;
        use std::os::fd::OwnedFd;
        use std::time::{Duration, Instant};

        let (read_end, mut write_end) = std::io::pipe().unwrap();
        let mut source = Fd(std::fs::File::from(OwnedFd::from(read_end)));
        let (sent, received) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut buf = vec![0; MOST];
            loop {
                let mut out = Vec::new();
                let open = read_some(&mut source, &mut buf, &mut out);
                if !out.is_empty() {
                    sent.send((Instant::now(), out)).unwrap();
                }
                if !open {
                    return;
                }
            }
        });

        let typed = Instant::now();
        write_end.write_all(b"a").unwrap();
        let (at, out) = received.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(out, b"a");
        assert!(at - typed < Duration::from_millis(500), "{:?}", at - typed);

        // Bytes already waiting are taken with the read that finds them.
        write_end.write_all(b"bc").unwrap();
        write_end.write_all(b"de").unwrap();
        drop(write_end);
        let mut rest = Vec::new();
        while let Ok((_, out)) = received.recv_timeout(Duration::from_secs(5)) {
            rest.extend(out);
        }
        assert_eq!(rest, b"bcde");
        reader.join().unwrap();
    }

    #[test]
    fn what_was_read_before_the_end_is_handed_over() {
        let mut source = Pieces(vec![b"last".to_vec()].into());
        let mut buf = vec![0; MOST];
        let mut out = Vec::new();
        assert!(!read_some(&mut source, &mut buf, &mut out));
        assert_eq!(out, b"last");
    }
}
