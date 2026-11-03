//! The client against a fake helper: another end of the protocol that the test
//! plays by hand, so that what the client does with each line is what is tested,
//! and no browser engine is needed (or a display).

use std::future::Future;
use std::io::{BufRead, BufReader, Write, pipe};
use std::pin::pin;
use std::sync::Arc;
use std::sync::mpsc::{RecvTimeoutError, TryRecvError};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};
use std::time::{Duration, Instant};

use hangar_webview::{
    Client, Closed, Command, Content, ErrorCode, Event, Events, OpenWindow, PROTOCOL_VERSION,
    SchemeResponse,
};

/// Long enough for anything that is going to happen, short enough that a test
/// that waits for something that never does fails before the suite is killed.
const PATIENCE: Duration = Duration::from_secs(10);

/// The helper's end of the two pipes.
struct Fake {
    commands: BufReader<std::io::PipeReader>,
    output: std::io::PipeWriter,
}

fn fake_helper() -> (Client, Events, Fake) {
    let (events_in, events_out) = pipe().unwrap();
    let (commands_in, commands_out) = pipe().unwrap();
    let (client, events) = Client::from_io(events_in, commands_out);
    (
        client,
        events,
        Fake {
            commands: BufReader::new(commands_in),
            output: events_out,
        },
    )
}

impl Fake {
    fn say(&mut self, event: &Event) {
        self.say_raw(&event.to_line());
    }

    fn say_raw(&mut self, line: &str) {
        writeln!(self.output, "{line}").unwrap();
        self.output.flush().unwrap();
    }

    fn ready(&mut self) {
        self.say(&Event::Ready {
            protocol: PROTOCOL_VERSION,
            version: "test".into(),
            engine: "fake".into(),
        });
    }

    /// The next command the client wrote, `None` once it closed its end.
    fn command(&mut self) -> Option<Command> {
        let mut line = String::new();
        match self.commands.read_line(&mut line).unwrap() {
            0 => None,
            _ => Some(
                Command::from_line(line.trim_end())
                    .unwrap_or_else(|error| panic!("{error}: {line}")),
            ),
        }
    }
}

fn next(events: &Events) -> Event {
    events.recv_timeout(PATIENCE).expect("an event comes")
}

/// What an app needs of the types: one client to share between its threads and
/// tasks, and the events to move into the one that reads them (an executor's
/// task included, which has to be `Send`).
#[test]
fn the_client_is_shared_and_the_events_are_moved() {
    fn shared<T: Send + Sync>() {}
    fn moved<T: Send>() {}
    shared::<Client>();
    moved::<Events>();
    shared::<Command>();
    shared::<Event>();
}

#[test]
fn events_arrive_in_order_and_the_queue_ends_with_the_helper() {
    let (_client, events, mut helper) = fake_helper();
    helper.ready();
    helper.say(&Event::Opened { window: "w".into() });
    helper.say(&Event::TitleChanged {
        window: "w".into(),
        title: "Hello".into(),
    });
    drop(helper);

    assert!(matches!(
        next(&events),
        Event::Ready {
            protocol: PROTOCOL_VERSION,
            ..
        }
    ));
    assert_eq!(next(&events), Event::Opened { window: "w".into() });
    assert_eq!(
        next(&events),
        Event::TitleChanged {
            window: "w".into(),
            title: "Hello".into()
        }
    );
    // What was written before the helper went is read, and then it is over.
    assert_eq!(events.recv(), None);
    assert_eq!(events.try_recv(), Err(TryRecvError::Disconnected));
    assert_eq!(
        events.recv_timeout(Duration::from_millis(10)),
        Err(RecvTimeoutError::Disconnected)
    );
}

#[test]
fn try_recv_and_recv_timeout_tell_nothing_yet_from_nothing_ever() {
    let (client, events, mut helper) = fake_helper();
    assert_eq!(events.try_recv(), Err(TryRecvError::Empty));
    assert_eq!(
        events.recv_timeout(Duration::from_millis(20)),
        Err(RecvTimeoutError::Timeout)
    );
    assert!(!client.is_closed());
    helper.ready();
    assert!(matches!(next(&events), Event::Ready { .. }));
}

#[test]
fn recv_blocks_until_an_event_comes_from_another_thread() {
    let (_client, events, mut helper) = fake_helper();
    let later = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        helper.say(&Event::Opened {
            window: "late".into(),
        });
        helper
    });
    assert_eq!(
        events.recv(),
        Some(Event::Opened {
            window: "late".into()
        })
    );
    drop(later.join().unwrap());
}

/// Runs a future on this thread with nothing but `std`, as any executor would.
fn block_on<F: Future>(future: F) -> F::Output {
    struct Unpark(Thread);
    impl Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = pin!(future);
    let deadline = Instant::now() + PATIENCE;
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        assert!(Instant::now() < deadline, "the future was never woken");
        thread::park_timeout(PATIENCE);
    }
}

#[test]
fn events_can_be_awaited_without_a_runtime() {
    let (_client, events, mut helper) = fake_helper();
    let later = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        helper.say(&Event::Opened { window: "w".into() });
        thread::sleep(Duration::from_millis(100));
        // Dropping the helper's end is the helper going away.
    });
    assert_eq!(
        block_on(events.recv_async()),
        Some(Event::Opened { window: "w".into() })
    );
    // A waiting task is woken by the end of the helper as well, not left hanging.
    assert_eq!(block_on(events.recv_async()), None);
    later.join().unwrap();
}

#[test]
fn commands_reach_the_helper_one_line_each_in_order() {
    let (client, _events, mut helper) = fake_helper();
    let open = OpenWindow::new("w", Content::url("hangar-kit://localhost/k/index.html"))
        .scheme("hangar-kit")
        .kit_bridge();
    client.open(open.clone()).unwrap();
    client.navigate("w", "https://example.com").unwrap();
    client.eval("w", "1 + 1").unwrap();
    client.eval_with_token("w", "2 + 2", "t").unwrap();
    client
        .post_message("w", serde_json::json!({ "hangar": 1, "event": "theme" }))
        .unwrap();
    client.set_title("w", "T").unwrap();
    client.focus("w").unwrap();
    client
        .respond(SchemeResponse::ok(5, "text/plain", b"hi".to_vec()))
        .unwrap();
    client.close("w").unwrap();
    client.quit().unwrap();

    assert_eq!(helper.command(), Some(Command::Open(open)));
    assert_eq!(
        helper.command(),
        Some(Command::Navigate {
            window: "w".into(),
            url: "https://example.com".into()
        })
    );
    assert_eq!(
        helper.command(),
        Some(Command::Eval {
            window: "w".into(),
            script: "1 + 1".into(),
            token: None
        })
    );
    assert_eq!(
        helper.command(),
        Some(Command::Eval {
            window: "w".into(),
            script: "2 + 2".into(),
            token: Some("t".into())
        })
    );
    assert!(matches!(
        helper.command(),
        Some(Command::PostMessage { .. })
    ));
    assert_eq!(
        helper.command(),
        Some(Command::SetTitle {
            window: "w".into(),
            title: "T".into()
        })
    );
    assert_eq!(
        helper.command(),
        Some(Command::Focus { window: "w".into() })
    );
    assert_eq!(
        helper.command(),
        Some(Command::SchemeResponse(SchemeResponse::ok(
            5,
            "text/plain",
            b"hi".to_vec()
        )))
    );
    assert_eq!(
        helper.command(),
        Some(Command::Close { window: "w".into() })
    );
    assert_eq!(helper.command(), Some(Command::Quit));
}

#[test]
fn many_threads_sending_at_once_never_mix_their_lines() {
    let (client, _events, mut helper) = fake_helper();
    let client = Arc::new(client);
    let senders: Vec<_> = (0..8)
        .map(|thread| {
            let client = client.clone();
            std::thread::spawn(move || {
                for n in 0..50 {
                    client
                        .set_title(
                            &format!("w{thread}"),
                            &format!("title {n} {}", "é".repeat(200)),
                        )
                        .unwrap();
                }
            })
        })
        .collect();
    let mut seen = 0;
    while seen < 8 * 50 {
        // Every line must parse whole: a torn write would not.
        let Some(Command::SetTitle { title, .. }) = helper.command() else {
            panic!("not a title")
        };
        assert!(title.ends_with(&"é".repeat(200)));
        seen += 1;
    }
    for sender in senders {
        sender.join().unwrap();
    }
}

#[test]
fn a_body_of_megabytes_passes_without_blocking_the_sender() {
    let (client, _events, mut helper) = fake_helper();
    let body: Vec<u8> = (0..8_000_000u32).map(|n| (n % 251) as u8).collect();
    let started = Instant::now();
    // The helper is not reading yet: `send` queues and comes back.
    client
        .respond(SchemeResponse::ok(
            1,
            "application/octet-stream",
            body.clone(),
        ))
        .unwrap();
    assert!(started.elapsed() < PATIENCE);
    let Some(Command::SchemeResponse(response)) = helper.command() else {
        panic!("not a response")
    };
    assert_eq!(response.body.len(), body.len());
    assert!(response.body == body, "the body changed on the way");
}

#[test]
fn a_helper_of_another_protocol_is_refused_and_not_heard_again() {
    let (client, events, mut helper) = fake_helper();
    helper.say_raw(r#"{"type":"ready","protocol":999,"version":"far future","engine":"x"}"#);
    helper.say(&Event::Opened { window: "w".into() });

    let Event::Error { code, message, .. } = next(&events) else {
        panic!("not an error")
    };
    assert_eq!(code, ErrorCode::ProtocolMismatch);
    assert!(
        message.contains("999") && message.contains(&PROTOCOL_VERSION.to_string()),
        "{message}"
    );
    // Nothing after it is passed on, and the queue ends.
    assert_eq!(events.recv(), None);
    assert!(client.is_closed());
    assert_eq!(client.quit(), Err(Closed));
}

#[test]
fn lines_that_are_not_events_are_reported_and_skipped() {
    let (_client, events, mut helper) = fake_helper();
    helper.ready();
    helper.say_raw("this is not json");
    helper.say_raw("");
    helper.say_raw(r#"{"type":"opened"}"#);
    helper.say_raw(r#"{"type":"from_the_future","window":"w"}"#);
    helper.say(&Event::Opened {
        window: "after".into(),
    });

    assert!(matches!(next(&events), Event::Ready { .. }));
    let Event::Error { code, message, .. } = next(&events) else {
        panic!("not an error")
    };
    assert_eq!(code, ErrorCode::BadEvent);
    assert!(message.contains("this is not json"), "{message}");
    // A known type missing what it needs is bad too; an unknown type is just not ours.
    assert!(matches!(
        next(&events),
        Event::Error {
            code: ErrorCode::BadEvent,
            ..
        }
    ));
    assert_eq!(next(&events), Event::Unknown);
    assert_eq!(
        next(&events),
        Event::Opened {
            window: "after".into()
        }
    );
}

#[test]
fn a_helper_that_stops_reading_shows_as_closed() {
    let (client, events, helper) = fake_helper();
    let Fake { commands, output } = helper;
    drop(commands);
    // The write fails in the client's thread, so it is the second send or so
    // that knows: wait for it rather than for a number of them.
    let deadline = Instant::now() + PATIENCE;
    while client.set_title("w", "t").is_ok() {
        assert!(
            Instant::now() < deadline,
            "a closed helper was never noticed"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert!(client.is_closed());
    drop(output);
    assert_eq!(events.recv(), None);
}

#[test]
fn dropping_the_client_asks_the_helper_to_quit_and_closes_its_input() {
    let (client, _events, mut helper) = fake_helper();
    drop(client);
    assert_eq!(helper.command(), Some(Command::Quit));
    assert_eq!(
        helper.command(),
        None,
        "the helper's input is closed, so it would leave on its own"
    );
}

/// A real child process, with `sh` standing in for the helper: it greets,
/// answers each command with an `opened` and leaves on `quit`.
#[cfg(unix)]
mod process {
    use std::path::PathBuf;

    use hangar_webview::{SpawnOptions, Stderr};

    use super::*;

    const SCRIPT: &str = r#"
printf '%s\n' '{"type":"ready","protocol":1,"version":"sh","engine":"sh"}'
while IFS= read -r line; do
  case "$line" in
    *'"quit"'*) exit 7 ;;
    *) printf '%s\n' '{"type":"opened","window":"w"}' ;;
  esac
done
"#;

    fn sh() -> SpawnOptions {
        SpawnOptions::new("sh")
            .arg("-c")
            .arg(SCRIPT)
            .stderr(Stderr::Null)
    }

    #[test]
    fn spawn_talks_to_a_child_and_reports_how_it_left() {
        let (client, events) = Client::spawn_with(sh()).unwrap();
        assert!(matches!(next(&events), Event::Ready { .. }));
        client
            .open(OpenWindow::new("w", Content::html("<p>")))
            .unwrap();
        assert_eq!(next(&events), Event::Opened { window: "w".into() });
        assert_eq!(
            client.wait(Duration::from_millis(50)),
            None,
            "it is still running"
        );

        client.quit().unwrap();
        assert_eq!(events.recv(), None);
        assert_eq!(
            client.wait(PATIENCE).and_then(|status| status.code()),
            Some(7)
        );
    }

    #[test]
    fn a_child_that_dies_ends_the_events_and_closes_the_client() {
        let (client, events) =
            Client::spawn_with(SpawnOptions::new("sh").arg("-c").arg("exit 3")).unwrap();
        assert_eq!(events.recv(), None);
        assert!(client.is_closed());
        assert_eq!(
            client.wait(PATIENCE).and_then(|status| status.code()),
            Some(3)
        );
    }

    #[test]
    fn kill_stops_a_child_that_ignores_its_input() {
        let (client, events) = Client::spawn_with(SpawnOptions::new("sleep").arg("30")).unwrap();
        client.kill();
        assert_eq!(events.recv(), None);
        assert!(client.wait(PATIENCE).is_some());
    }

    /// Whether a process still exists, a zombie that nobody waited for included.
    fn exists(pid: u32) -> bool {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success()
    }

    #[test]
    fn dropping_the_client_makes_the_child_leave_and_reaps_it() {
        // Leaves as soon as it has read a line, which `quit` is.
        let (client, _events) =
            Client::spawn_with(SpawnOptions::new("sh").arg("-c").arg("read line; exit 5")).unwrap();
        let pid = client.id().expect("a child of its own");
        assert!(exists(pid));
        drop(client);
        let deadline = Instant::now() + PATIENCE;
        while exists(pid) {
            assert!(
                Instant::now() < deadline,
                "the child was left running, or unreaped"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn a_program_that_is_not_there_is_an_error_not_a_hang() {
        let missing = PathBuf::from("/nonexistent/hangar-webview");
        let error = Client::spawn(&missing).err().expect("it does not start");
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    }
}
