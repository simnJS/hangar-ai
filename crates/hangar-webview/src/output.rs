//! The helper's standard output, which is the protocol and nothing else.
//!
//! Handlers of the engine run on its main thread, in the middle of a frame.
//! They must not wait for a parent that is slow to read, so they queue each
//! event and a thread of its own writes it. Nothing else in the process may
//! print to standard output: a stray line would be read as a broken event.

use std::io::Write;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use hangar_webview::{ErrorCode, Event};

enum Out {
    Event(Event),
    /// Answered once everything queued before it is written.
    Flush(Sender<()>),
}

#[derive(Clone)]
pub struct Emitter {
    queue: Sender<Out>,
}

impl Emitter {
    pub fn start() -> Self {
        let (queue, lines) = mpsc::channel::<Out>();
        thread::Builder::new()
            .name("output".into())
            .spawn(move || {
                let mut stdout = std::io::stdout().lock();
                for out in lines {
                    match out {
                        Out::Event(event) => {
                            let mut line = event.to_line();
                            line.push('\n');
                            if stdout.write_all(line.as_bytes()).and_then(|()| stdout.flush()).is_err() {
                                // The parent closed its end: there is nobody to
                                // run for, and its closing our input would have
                                // ended us as well.
                                std::process::exit(0);
                            }
                        }
                        Out::Flush(done) => {
                            let _ = done.send(());
                        }
                    }
                }
            })
            .expect("a thread starts");
        Self { queue }
    }

    pub fn emit(&self, event: Event) {
        // The writer only ends with the process.
        let _ = self.queue.send(Out::Event(event));
    }

    pub fn error(&self, window: Option<&str>, code: ErrorCode, message: impl Into<String>) {
        self.emit(Event::Error { window: window.map(str::to_string), code, message: message.into() });
    }

    /// Wait until what was emitted has been written, for the last moments of the
    /// process. Bounded: a parent that stopped reading should not keep us alive.
    pub fn flush(&self) {
        let (done, written) = mpsc::channel();
        if self.queue.send(Out::Flush(done)).is_ok() {
            let _ = written.recv_timeout(Duration::from_secs(2));
        }
    }
}
