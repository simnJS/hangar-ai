//! A terminal wired to a real process through a pseudo-terminal, the way an app
//! wires it: output in, the bytes of the events out.
//!
//! These tests prove the loop that the example is made of, end to end: what is
//! typed reaches the process, what the process prints reaches the screen, a
//! query the process sends is answered, and a resize reaches the PTY.

#![cfg(unix)]

use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use hangar_term::{Action, Config, Event, Metrics, Terminal};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};

struct Session {
    terminal: Terminal,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    output: mpsc::Receiver<Vec<u8>>,
    notifications: Vec<String>,
}

impl Session {
    /// Runs `program args` in a PTY and feeds a terminal with what it prints.
    fn start(program: &str, args: &[&str]) -> Option<Self> {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .ok()?;
        let mut command = CommandBuilder::new(program);
        command.args(args);
        command.env("TERM", "xterm-256color");
        let mut child = pair.slave.spawn_command(command).ok()?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().ok()?;
        let writer = pair.master.take_writer().ok()?;
        let (sender, output) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            while let Ok(n) = reader.read(&mut buffer) {
                if n == 0 || sender.send(buffer[..n].to_vec()).is_err() {
                    break;
                }
            }
            let _ = child.wait();
        });
        Some(Self {
            terminal: Terminal::new(Config::default(), 80, 24),
            master: pair.master,
            writer,
            output,
            notifications: Vec::new(),
        })
    }

    /// What an app does with the events: bytes to the process, size to the PTY.
    fn handle(&mut self, events: Vec<Event>) {
        for event in events {
            match event {
                Event::Write(bytes) => {
                    self.writer.write_all(&bytes).unwrap();
                    self.writer.flush().unwrap();
                }
                Event::Resize(size) => {
                    self.master
                        .resize(PtySize {
                            rows: size.rows,
                            cols: size.columns,
                            pixel_width: size.pixel_width,
                            pixel_height: size.pixel_height,
                        })
                        .unwrap();
                }
                Event::Notification { body, .. } => self.notifications.push(body),
                _ => {}
            }
        }
    }

    fn type_text(&mut self, bytes: &[u8]) {
        let events = self.terminal.perform(Action::Input(bytes.to_vec()));
        self.handle(events);
    }

    /// Pumps output until `done` holds of the terminal, or five seconds have
    /// passed.
    fn wait_until(&mut self, done: impl Fn(&Terminal) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !done(&self.terminal) && Instant::now() < deadline {
            if let Ok(chunk) = self.output.recv_timeout(Duration::from_millis(50)) {
                let events = self.terminal.feed(&chunk);
                self.handle(events);
            }
        }
    }

    /// The same, for a condition on the screen's text, returning the text.
    fn wait_for(&mut self, done: impl Fn(&str) -> bool) -> String {
        self.wait_until(|terminal| done(&terminal.screen_text().join("\n")));
        self.terminal.screen_text().join("\n")
    }
}

#[test]
fn what_is_typed_reaches_the_process_and_its_output_the_screen() {
    let Some(mut session) = Session::start("sh", &["-c", "read line; echo \"you said: $line\""])
    else {
        return;
    };
    // The composed text of a dead-key sequence, then Enter.
    session.type_text("café".as_bytes());
    session.type_text(b"\r");
    let screen = session.wait_for(|s| s.contains("you said: café"));
    assert!(screen.contains("you said: café"), "{screen}");
}

#[test]
fn a_query_of_the_process_is_answered_through_the_pty() {
    // The process asks where the cursor is and reads the answer from its
    // input, which is what a TUI does at start-up.
    let script = r#"
        stty raw -echo
        printf '\033[5;10H\033[6n'
        answer=$(dd bs=1 count=7 2>/dev/null)
        stty sane
        printf 'answer=%s' "$(printf '%s' "$answer" | tr '\033' '^')"
    "#;
    let Some(mut session) = Session::start("sh", &["-c", script]) else {
        return;
    };
    let screen = session.wait_for(|s| s.contains("answer="));
    assert!(screen.contains("answer=^[5;10R"), "{screen}");
}

#[test]
fn a_resize_reaches_the_process() {
    let script = r#"
        trap 'printf "winch:%s\n" "$(stty size)"' WINCH
        printf 'ready\n'
        while true; do sleep 0.05; done
    "#;
    let Some(mut session) = Session::start("sh", &["-c", script]) else {
        return;
    };
    session.wait_for(|s| s.contains("ready"));
    let events = session.terminal.perform(Action::Resize(Metrics {
        columns: 100,
        rows: 30,
        cell_width: 8.0,
        cell_height: 16.0,
    }));
    session.handle(events);
    let screen = session.wait_for(|s| s.contains("winch:30 100"));
    assert!(screen.contains("winch:30 100"), "{screen}");
}

#[test]
fn notifications_and_colours_survive_a_real_process() {
    let Some(mut session) = Session::start(
        "sh",
        &[
            "-c",
            r"printf '\033[31mred\033[0m\n\033]9;turn finished\007done\n'",
        ],
    ) else {
        return;
    };
    session.wait_for(|s| s.contains("done"));
    assert_eq!(session.notifications, ["turn finished"]);
}

#[test]
fn bracketed_paste_reaches_a_program_that_asked_for_it() {
    // The program turns bracketed paste on, then reads what arrives.
    let script = r#"
        stty raw -echo
        printf '\033[?2004h'
        data=$(dd bs=1 count=14 2>/dev/null)
        stty sane
        printf 'got=%s' "$(printf '%s' "$data" | tr '\033' '^')"
    "#;
    let Some(mut session) = Session::start("sh", &["-c", script]) else {
        return;
    };
    // Wait until the mode is on before pasting, as a user could not paste
    // before the program has drawn.
    session.wait_until(|terminal| {
        terminal
            .mode()
            .contains(hangar_term::TermMode::BRACKETED_PASTE)
    });
    let events = session.terminal.perform(Action::Paste("hi".into()));
    session.handle(events);
    let screen = session.wait_for(|s| s.contains("got="));
    assert!(screen.contains("got=^[200~hi^[201~"), "{screen}");
}
