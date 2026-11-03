//! The operating system commands the emulator does not pass on.
//!
//! alacritty_terminal handles the escape sequences a terminal draws with, and
//! drops the ones that are about the desktop: `OSC 9` and `OSC 777` ask for a
//! notification, `OSC 7` reports the shell's directory. Agents use the first
//! two to say that a turn is over, which Hangar turns into a badge and a
//! notification, so they cannot be dropped.
//!
//! The scanner watches the byte stream next to the emulator. It is a small
//! state machine that remembers where it was between chunks, because a
//! sequence is routinely split across two reads of the PTY.

/// What the scanner found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Osc {
    Notification { title: Option<String>, body: String },
    WorkingDirectory(String),
}

/// The longest payload kept. Notifications are a sentence; anything longer is
/// not one, and the cap keeps a stream that never terminates its sequence from
/// growing a buffer without bound.
const MAX_PAYLOAD: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum State {
    #[default]
    Ground,
    Escape,
    /// Reading the number that names the command.
    Number(u32),
    /// Collecting the payload of a command we act on.
    Payload(u32),
    /// An escape inside a payload: `ESC \` ends it.
    PayloadEscape(u32),
    /// A command we do not act on, skipped to its end.
    Ignore,
    IgnoreEscape,
}

#[derive(Debug, Default)]
pub(crate) struct OscScanner {
    state: State,
    payload: Vec<u8>,
}

impl OscScanner {
    /// Looks at the next chunk of output and appends what it completes to
    /// `found`.
    pub(crate) fn scan(&mut self, bytes: &[u8], found: &mut Vec<Osc>) {
        let mut i = 0;
        while i < bytes.len() {
            // Most output is plain text. Skipping to the next escape in one
            // go keeps the scanner free for it.
            if self.state == State::Ground {
                match bytes[i..].iter().position(|&b| b == 0x1b) {
                    Some(offset) => i += offset,
                    None => return,
                }
            }
            self.step(bytes[i], found);
            i += 1;
        }
    }

    fn step(&mut self, byte: u8, found: &mut Vec<Osc>) {
        // CAN and SUB abort whatever sequence was being read.
        if byte == 0x18 || byte == 0x1a {
            self.state = State::Ground;
            return;
        }
        self.state = match self.state {
            State::Ground => {
                if byte == 0x1b {
                    State::Escape
                } else {
                    State::Ground
                }
            }
            State::Escape => match byte {
                b']' => State::Number(0),
                0x1b => State::Escape,
                _ => State::Ground,
            },
            State::Number(n) => match byte {
                b'0'..=b'9' => State::Number(n.saturating_mul(10).saturating_add((byte - b'0') as u32)),
                b';' if matches!(n, 7 | 9 | 777) => {
                    self.payload.clear();
                    State::Payload(n)
                }
                b';' => State::Ignore,
                0x07 => State::Ground,
                0x1b => State::IgnoreEscape,
                _ => State::Ignore,
            },
            State::Payload(n) => match byte {
                0x07 => {
                    self.finish(n, found);
                    State::Ground
                }
                0x1b => State::PayloadEscape(n),
                _ => {
                    if self.payload.len() < MAX_PAYLOAD {
                        self.payload.push(byte);
                        State::Payload(n)
                    } else {
                        State::Ignore
                    }
                }
            },
            State::PayloadEscape(n) => match byte {
                b'\\' => {
                    self.finish(n, found);
                    State::Ground
                }
                // An escape that is not the terminator starts another
                // sequence; the one in progress was cut short.
                b']' => State::Number(0),
                _ => State::Ground,
            },
            State::Ignore => match byte {
                0x07 => State::Ground,
                0x1b => State::IgnoreEscape,
                _ => State::Ignore,
            },
            State::IgnoreEscape => match byte {
                b'\\' => State::Ground,
                b']' => State::Number(0),
                _ => State::Ground,
            },
        };
    }

    fn finish(&mut self, number: u32, found: &mut Vec<Osc>) {
        let text = String::from_utf8_lossy(&self.payload).into_owned();
        self.payload.clear();
        match number {
            9 => {
                // Only iTerm2's `OSC 9 ; message` is a notification. ConEmu
                // and Windows Terminal use the same number for sub-commands
                // (`4;state;percent` is the taskbar progress, `9;cwd` the
                // working directory) that have nothing to say to a person.
                // A message written for one does not start with a number.
                let numeric = text
                    .split(';')
                    .next()
                    .is_some_and(|head| !head.is_empty() && head.bytes().all(|b| b.is_ascii_digit()));
                if numeric {
                    return;
                }
                found.push(Osc::Notification {
                    title: None,
                    body: text.trim().to_owned(),
                });
            }
            777 => {
                let mut parts = text.splitn(3, ';');
                if parts.next() != Some("notify") {
                    return;
                }
                let title = parts.next().map(str::trim).filter(|t| !t.is_empty());
                let body = parts.next().map(str::trim).unwrap_or("");
                found.push(Osc::Notification {
                    title: title.map(str::to_owned),
                    body: body.to_owned(),
                });
            }
            7 => {
                if let Some(path) = file_uri_path(&text) {
                    found.push(Osc::WorkingDirectory(path));
                }
            }
            _ => {}
        }
    }
}

/// The path of a `file://host/path` URI, percent-decoded.
fn file_uri_path(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    // The host is whatever comes before the first slash; it is usually the
    // machine's name or empty, and the path is the same either way.
    let path = &rest[rest.find('/')?..];
    let mut out = Vec::with_capacity(path.len());
    let bytes = path.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = path.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(hex);
                i += 3;
                continue;
            }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_all(chunks: &[&[u8]]) -> Vec<Osc> {
        let mut scanner = OscScanner::default();
        let mut found = Vec::new();
        for chunk in chunks {
            scanner.scan(chunk, &mut found);
        }
        found
    }

    fn note(body: &str) -> Osc {
        Osc::Notification { title: None, body: body.to_owned() }
    }

    #[test]
    fn osc_9_with_bell_or_string_terminator() {
        assert_eq!(scan_all(&[b"\x1b]9;build done\x07"]), [note("build done")]);
        assert_eq!(scan_all(&[b"\x1b]9;build done\x1b\\"]), [note("build done")]);
    }

    #[test]
    fn a_sequence_split_between_reads_is_still_found() {
        assert_eq!(
            scan_all(&[b"text \x1b", b"]9;fin", b"ished\x1b", b"\\ more"]),
            [note("finished")]
        );
    }

    #[test]
    fn conemu_sub_commands_are_not_notifications() {
        assert!(scan_all(&[b"\x1b]9;4;3;0\x07\x1b]9;9;/home/me\x07"]).is_empty());
    }

    #[test]
    fn osc_777_notify_has_a_title_and_a_body() {
        assert_eq!(
            scan_all(&[b"\x1b]777;notify;Claude;Your turn\x07"]),
            [Osc::Notification {
                title: Some("Claude".into()),
                body: "Your turn".into()
            }]
        );
        assert!(scan_all(&[b"\x1b]777;precmd\x07"]).is_empty());
    }

    #[test]
    fn working_directory_is_decoded() {
        assert_eq!(
            scan_all(&[b"\x1b]7;file://host/home/me/my%20dir\x07"]),
            [Osc::WorkingDirectory("/home/me/my dir".into())]
        );
    }

    #[test]
    fn other_commands_and_plain_escapes_are_skipped() {
        let found = scan_all(&[b"\x1b[31mred\x1b[0m\x1b]0;title\x07\x1b]8;;http://x\x1b\\link\x1b]9;hi\x07"]);
        assert_eq!(found, [note("hi")]);
    }

    #[test]
    fn an_unterminated_payload_does_not_grow_without_bound() {
        let mut scanner = OscScanner::default();
        let mut found = Vec::new();
        scanner.scan(b"\x1b]9;", &mut found);
        for _ in 0..10 {
            scanner.scan(&[b'a'; 1024], &mut found);
        }
        assert!(scanner.payload.len() <= MAX_PAYLOAD);
        assert!(found.is_empty());
    }
}
