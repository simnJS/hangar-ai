//! Your `$SHELL` in a window: `cargo run -p hangar-term --example shell`.
//!
//! It is also the reference for wiring a terminal to a process, which is the
//! app's job: this file owns the PTY (through portable-pty), reads what the
//! process prints on a thread and feeds it to the [`Terminal`], and writes
//! back the bytes the terminal reports.
//!
//! Try: dead keys (`'` then `e` should give `é`, `'` then space an
//! apostrophe), Ctrl+Shift+C and Ctrl+Shift+V, selecting with the mouse
//! (double and triple click, Shift+click, Alt+drag), the wheel and
//! Shift+PageUp, Ctrl+click on a URL, Ctrl+Shift+F for the search bar, and
//! full-screen programs such as `htop`, `vim` or `less`.
//!
//! `HANGAR_TERM_FONT` sets the font stack and `HANGAR_TERM_SIZE` the size.

use std::io::{Read, Write};
use std::sync::{Mutex, OnceLock};

use hangar_term::{
    Action, Config, Event, Font, Palette, SearchDirection, SearchOptions, Terminal, terminal_view,
};
use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, StreamExt};
use iced::keyboard::{self, Key, key::Named};
use iced::widget::{button, column, container, row, text, text_input};
use iced::{Element, Length, Size, Subscription, Task, Theme};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};

fn main() -> iced::Result {
    iced::application(Shell::boot, Shell::update, Shell::view)
        .title(Shell::title)
        .subscription(Shell::subscription)
        .theme(|_: &Shell| Theme::Dark)
        .window_size(Size::new(1000.0, 640.0))
        .run()
}

#[derive(Debug, Clone)]
enum Message {
    /// Something happened to the terminal widget.
    Terminal(Action),
    /// The process printed something.
    Output(Vec<u8>),
    /// The process ended.
    Exited,
    ToggleSearch,
    SearchInput(String),
    SearchStep(SearchDirection),
    CloseSearch,
    /// Escape was pressed somewhere in the window.
    Escape,
}

struct Pty {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
}

struct Shell {
    terminal: Terminal,
    pty: Pty,
    title: String,
    /// The search bar, if open.
    search: Option<String>,
}

/// What the reader thread sends the subscription. A subscription is built
/// from a plain function, so the receiver waits here to be picked up.
static OUTPUT: OnceLock<Mutex<Option<Output>>> = OnceLock::new();

/// `None` is the end of the output: the process exited.
type Output = mpsc::UnboundedReceiver<Option<Vec<u8>>>;

impl Shell {
    fn boot() -> Self {
        let font = std::env::var("HANGAR_TERM_FONT").unwrap_or_else(|_| {
            "\"JetBrains Mono\", \"Cascadia Code\", \"DejaVu Sans Mono\", monospace".into()
        });
        let size = std::env::var("HANGAR_TERM_SIZE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(14.0);

        let mut terminal = Terminal::new(Config::default(), 80, 24);
        terminal.set_font(Font::from_css_stack(&font, size, 1.25));
        terminal.set_palette(Palette::default());
        terminal.set_focused(true);

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("open a pty");
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        let mut command = CommandBuilder::new(shell);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("TERM_PROGRAM", "hangar-term");
        if let Ok(dir) = std::env::current_dir() {
            command.cwd(dir);
        }
        let mut child = pair.slave.spawn_command(command).expect("spawn the shell");
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .expect("clone the pty reader");
        let writer = pair.master.take_writer().expect("take the pty writer");

        // The reader thread: blocking reads, forwarded as they come. The
        // subscription joins whatever has piled up before it reaches the UI,
        // so a burst of output is one update, not hundreds.
        let (sender, receiver) = mpsc::unbounded();
        OUTPUT
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .replace(receiver);
        std::thread::spawn(move || {
            let mut buffer = vec![0u8; 64 * 1024];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if sender.unbounded_send(Some(buffer[..n].to_vec())).is_err() {
                            return;
                        }
                    }
                }
            }
            let _ = child.wait();
            let _ = sender.unbounded_send(None);
        });

        Self {
            terminal,
            pty: Pty {
                master: pair.master,
                writer,
            },
            title: "hangar-term".into(),
            search: None,
        }
    }

    fn title(&self) -> String {
        if self.title.is_empty() {
            "hangar-term".into()
        } else {
            self.title.clone()
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        // The search bar closes on Escape, wherever the keyboard is.
        let escape = iced::event::listen_with(|event, _status, _window| match event {
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key: Key::Named(Named::Escape),
                ..
            }) => Some(Message::Escape),
            _ => None,
        });
        let output = Subscription::run(|| {
            iced::stream::channel(16, async |mut output| {
                let receiver = OUTPUT.get().and_then(|slot| slot.lock().unwrap().take());
                let Some(mut receiver) = receiver else { return };
                while let Some(item) = receiver.next().await {
                    let Some(mut bytes) = item else {
                        let _ = output.send(Message::Exited).await;
                        return;
                    };
                    // Whatever else is already waiting goes in the same update.
                    while bytes.len() < 512 * 1024 {
                        match receiver.try_recv() {
                            Ok(Some(more)) => bytes.extend(more),
                            Ok(None) => {
                                let _ = output.send(Message::Output(bytes)).await;
                                let _ = output.send(Message::Exited).await;
                                return;
                            }
                            _ => break,
                        }
                    }
                    let _ = output.send(Message::Output(bytes)).await;
                }
            })
        });
        Subscription::batch([escape, output])
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Terminal(action) => {
                let events = self.terminal.perform(action);
                self.handle(events)
            }
            Message::Output(bytes) => {
                let events = self.terminal.feed(&bytes);
                self.handle(events)
            }
            Message::Exited => iced::exit(),
            Message::Escape => {
                if self.search.is_some() {
                    self.update(Message::CloseSearch)
                } else {
                    Task::none()
                }
            }
            Message::ToggleSearch => {
                if self.search.is_some() {
                    return self.update(Message::CloseSearch);
                }
                self.search = Some(
                    self.terminal
                        .search_query()
                        .map(|(q, _)| q.to_owned())
                        .unwrap_or_default(),
                );
                // The keyboard goes to the search field while it is open.
                self.terminal.set_focused(false);
                iced::widget::operation::focus("search")
            }
            Message::SearchInput(query) => {
                let _ = self.terminal.search(&query, SearchOptions::default());
                self.search = Some(query);
                Task::none()
            }
            Message::SearchStep(direction) => {
                self.terminal.search_step(direction);
                Task::none()
            }
            Message::CloseSearch => {
                self.search = None;
                self.terminal.clear_search();
                let events = self.terminal.set_focused(true);
                self.handle(events)
            }
        }
    }

    /// What a terminal reports, done: this is the whole of the wiring.
    fn handle(&mut self, events: Vec<Event>) -> Task<Message> {
        let mut tasks = Vec::new();
        for event in events {
            match event {
                // The bytes for the process: typing, pastes, mouse reports and
                // the answers to its queries. A real app writes them from a
                // writer thread, so that a process that stopped reading cannot
                // freeze the UI.
                Event::Write(bytes) => {
                    let _ = self.pty.writer.write_all(&bytes);
                    let _ = self.pty.writer.flush();
                }
                Event::Resize(size) => {
                    let _ = self.pty.master.resize(PtySize {
                        rows: size.rows,
                        cols: size.columns,
                        pixel_width: size.pixel_width,
                        pixel_height: size.pixel_height,
                    });
                }
                Event::Title(title) => self.title = title,
                Event::ResetTitle => self.title.clear(),
                Event::SetClipboard { target, text } => tasks.push(match target {
                    hangar_term::ClipboardTarget::Clipboard => iced::clipboard::write(text),
                    hangar_term::ClipboardTarget::Primary => iced::clipboard::write_primary(text),
                }),
                Event::OpenLink(link) => {
                    let _ = std::process::Command::new("xdg-open")
                        .arg(&link.uri)
                        .spawn();
                }
                Event::Notification { title, body } => {
                    eprintln!("notification: {} {body}", title.unwrap_or_default());
                }
                _ => {}
            }
        }
        Task::batch(tasks)
    }

    fn view(&self) -> Element<'_, Message> {
        let terminal = terminal_view(&self.terminal, Message::Terminal)
            .padding(6)
            // The app's own shortcuts are taken before the terminal sees them.
            .intercept(|press| {
                let ctrl_shift = press.modifiers.control() && press.modifiers.shift();
                match &press.key {
                    Key::Character(c) if ctrl_shift && c.eq_ignore_ascii_case("f") => {
                        Some(Message::ToggleSearch)
                    }
                    _ => None,
                }
            });

        let mut layout = column![];
        if let Some(query) = &self.search {
            let status = self.terminal.search_status();
            let counter = match status.current {
                Some(i) => format!(
                    "{}/{}{}",
                    i + 1,
                    status.count,
                    if status.truncated { "+" } else { "" }
                ),
                None if query.is_empty() => String::new(),
                None => "no match".into(),
            };
            layout = layout.push(
                container(
                    row![
                        text_input("search", query)
                            .id("search")
                            .on_input(Message::SearchInput)
                            .on_submit(Message::SearchStep(SearchDirection::Backward)),
                        text(counter).width(80),
                        button("up").on_press(Message::SearchStep(SearchDirection::Backward)),
                        button("down").on_press(Message::SearchStep(SearchDirection::Forward)),
                        button("close").on_press(Message::CloseSearch),
                    ]
                    .spacing(6)
                    .align_y(iced::Center),
                )
                .padding(6),
            );
        }
        layout
            .push(container(terminal).width(Length::Fill).height(Length::Fill))
            .into()
    }
}
