//! A client with no helper behind it, for the tests of whoever sends the
//! helper commands: what it was sent is read back.

use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hangar_webview::Client;

/// A helper's output that never ends until the test lets go of its sender.
pub(crate) struct Silent(Receiver<Vec<u8>>);

impl Read for Silent {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.0.recv() {
            Ok(bytes) => {
                let n = bytes.len().min(buf.len());
                buf[..n].copy_from_slice(&bytes[..n]);
                Ok(n)
            }
            Err(_) => Ok(0),
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct Recorder(Arc<Mutex<Vec<u8>>>);

impl Write for Recorder {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(crate) struct Fake {
    pub(crate) client: Client,
    pub(crate) sent: Recorder,
    _keep: Sender<Vec<u8>>,
}

impl Fake {
    pub(crate) fn new() -> Fake {
        let (keep, reader) = mpsc::channel();
        let sent = Recorder::default();
        let (client, _events) = Client::from_io(Silent(reader), sent.clone());
        Fake {
            client,
            sent,
            _keep: keep,
        }
    }

    /// The commands the client wrote, once there are `count` of them.
    pub(crate) fn commands(&self, count: usize) -> Vec<serde_json::Value> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let text = String::from_utf8(self.sent.0.lock().unwrap().clone()).unwrap();
            let lines: Vec<serde_json::Value> = text
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            if lines.len() >= count || Instant::now() > deadline {
                return lines;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
