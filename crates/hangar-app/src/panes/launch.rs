//! When to type an agent's command into a pane's new shell, and whether it
//! ran: the decisions alone, with no clock and no terminal, so they are
//! tested as they are (the timing lives in controller.rs, the bytes in
//! pty_link.rs).
//!
//! A shell that starts reads its rc files, draws its prompt and only then
//! reads what was typed. What arrives before it may be read by the rc (a
//! `read`, an instant prompt that flushes typeahead) or thrown away as the
//! line editor sets the terminal up, and at a relaunch eight shells start
//! together on a busy disk. So the command waits for the shell to have
//! printed something and gone quiet, within bounds: never before
//! `launchDelayMs`, never later than [`MAX_EXTRA`] after it, so a shell that
//! prints nothing still gets its command.
//!
//! Typed, the command has to show: the shell echoes what it reads. When no
//! echo comes and the shell runs nothing either, it is typed once more; when
//! that is lost too, the pane says so in the log instead of sitting on a
//! bare prompt.

use std::time::Duration;

/// How long the shell's output must have paused before the command is typed.
pub const QUIET: Duration = Duration::from_millis(150);

/// How much longer than `launchDelayMs` a launch waits for a shell that has
/// printed nothing, or keeps printing.
pub const MAX_EXTRA: Duration = Duration::from_secs(3);

/// How long the echo of a typed command is waited for before the shell is
/// asked whether it runs something.
pub const ECHO_WAIT: Duration = Duration::from_secs(3);

/// How often a wait looks again.
pub const POLL: Duration = Duration::from_millis(25);

/// How many characters of the command its echo is recognised by: enough to
/// tell it from the prompt, few enough to be on the first line of the echo
/// in most panes (a wrapped echo is followed anyway, [`Echo`]).
const NEEDLE_CHARS: usize = 48;

/// What the launch does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settle {
    /// Look again after this long.
    Wait(Duration),
    /// The shell is ready: type the command.
    Type,
}

/// Whether to type now. `since_spawn` is the time since the shell started,
/// `since_output` the time since it last printed (`None`: nothing yet),
/// `min` the setting's `launchDelayMs`.
pub fn settle(since_spawn: Duration, since_output: Option<Duration>, min: Duration) -> Settle {
    let max = min + MAX_EXTRA;
    if since_spawn >= max {
        return Settle::Type;
    }
    if since_spawn < min {
        return Settle::Wait(min - since_spawn);
    }
    match since_output {
        Some(quiet) if quiet >= QUIET => Settle::Type,
        Some(quiet) => Settle::Wait((QUIET - quiet).min(max - since_spawn)),
        None => Settle::Wait(POLL.min(max - since_spawn)),
    }
}

/// What a launch's check found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// The command was echoed, or the shell runs something: it was read.
    Ran,
    /// Neither, on the first attempt: type it again.
    Retry,
    /// Neither, twice: say so.
    Lost,
}

/// What to do once the echo was waited for. `attempt` counts from 0.
pub fn check(echoed: bool, running: bool, attempt: u8) -> Check {
    if echoed || running {
        Check::Ran
    } else if attempt == 0 {
        Check::Retry
    } else {
        Check::Lost
    }
}

/// Looks for the echo of a typed command in what the shell prints, through
/// what a terminal does not show as text: escape sequences, carriage returns
/// and line feeds (a long command wraps), backspaces (a line editor that
/// redraws steps back over what it printed).
#[derive(Debug, Clone)]
pub struct Echo {
    needle: Vec<char>,
    seen: Vec<char>,
    escape: Escape,
    found: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Escape {
    None,
    /// After `ESC`.
    Start,
    /// In `ESC [ …`, until its final byte.
    Csi,
    /// In `ESC ] …`, until `BEL` or `ESC \`.
    Osc,
    /// In an OSC, after an `ESC`.
    OscEnd,
}

impl Echo {
    /// Waits for the start of `command`.
    pub fn new(command: &str) -> Echo {
        let needle: Vec<char> = command.chars().take(NEEDLE_CHARS).collect();
        Echo {
            seen: Vec::with_capacity(needle.len() * 2),
            needle,
            escape: Escape::None,
            found: false,
        }
    }

    /// Whether the echo went by.
    pub fn found(&self) -> bool {
        self.found || self.needle.is_empty()
    }

    /// Reads what the shell printed next.
    pub fn feed(&mut self, text: &str) {
        if self.found() {
            return;
        }
        for c in text.chars() {
            self.escape = match (self.escape, c) {
                (Escape::None, '\x1b') => Escape::Start,
                (Escape::None, '\x08') => {
                    self.seen.pop();
                    Escape::None
                }
                (Escape::None, c) if c.is_control() => Escape::None,
                (Escape::None, c) => {
                    self.push(c);
                    if self.found {
                        return;
                    }
                    Escape::None
                }
                (Escape::Start, '[') => Escape::Csi,
                (Escape::Start, ']') => Escape::Osc,
                (Escape::Start, _) => Escape::None,
                (Escape::Csi, c) if ('\x40'..='\x7e').contains(&c) => Escape::None,
                (Escape::Csi, _) => Escape::Csi,
                (Escape::Osc, '\x07') => Escape::None,
                (Escape::Osc, '\x1b') => Escape::OscEnd,
                (Escape::Osc, _) => Escape::Osc,
                (Escape::OscEnd, _) => Escape::None,
            };
        }
    }

    fn push(&mut self, c: char) {
        self.seen.push(c);
        if self.seen.ends_with(&self.needle) {
            self.found = true;
            self.seen = Vec::new();
            return;
        }
        // Only the tail can still begin a match.
        let keep = self.needle.len() * 2;
        if self.seen.len() > keep * 2 {
            self.seen.drain(..self.seen.len() - keep);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: Duration = Duration::from_millis(700);

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn never_types_before_the_launch_delay() {
        assert_eq!(settle(ms(0), None, MIN), Settle::Wait(MIN));
        assert_eq!(settle(ms(300), Some(ms(250)), MIN), Settle::Wait(ms(400)));
    }

    #[test]
    fn types_once_the_shell_printed_and_went_quiet() {
        // The prompt came long ago: at the delay, as before.
        assert_eq!(settle(MIN, Some(ms(600)), MIN), Settle::Type);
        // Still printing (an rc, a slow prompt): waits for the pause.
        assert_eq!(settle(ms(800), Some(ms(40)), MIN), Settle::Wait(ms(110)));
        assert_eq!(settle(ms(910), Some(QUIET), MIN), Settle::Type);
    }

    #[test]
    fn waits_for_a_shell_that_printed_nothing_yet() {
        assert_eq!(settle(ms(900), None, MIN), Settle::Wait(POLL));
    }

    #[test]
    fn a_silent_or_chatty_shell_still_gets_its_command() {
        let max = MIN + MAX_EXTRA;
        assert_eq!(settle(max, None, MIN), Settle::Type);
        assert_eq!(settle(max, Some(ms(1)), MIN), Settle::Type);
        // The waits never run past the bound.
        assert_eq!(settle(max - ms(10), None, MIN), Settle::Wait(ms(10)));
        assert_eq!(settle(max - ms(10), Some(ms(0)), MIN), Settle::Wait(ms(10)));
    }

    #[test]
    fn a_launch_delay_of_zero_still_waits_for_the_prompt() {
        assert_eq!(settle(ms(0), None, ms(0)), Settle::Wait(POLL));
        assert_eq!(settle(ms(50), Some(ms(200)), ms(0)), Settle::Type);
    }

    #[test]
    fn a_lost_command_is_typed_again_once_then_reported() {
        assert_eq!(check(true, false, 0), Check::Ran);
        assert_eq!(check(false, true, 0), Check::Ran);
        assert_eq!(check(false, false, 0), Check::Retry);
        assert_eq!(check(false, false, 1), Check::Lost);
        assert_eq!(check(true, false, 1), Check::Ran);
    }

    const COMMAND: &str =
        "'/opt/claude' --resume b82acf25-f5a7-407a-848f-22c6788a600e --name \"P6 (bench)\"";

    #[test]
    fn the_echo_is_found_as_the_shell_prints_it() {
        let mut echo = Echo::new(COMMAND);
        echo.feed("\x1b[?2004huser@host:/work$ ");
        assert!(!echo.found());
        // In pieces, as a line editor echoes.
        let (a, b) = COMMAND.split_at(20);
        echo.feed(a);
        assert!(!echo.found());
        echo.feed(b);
        assert!(echo.found());
    }

    #[test]
    fn the_echo_is_found_through_a_wrap_colours_and_a_redraw() {
        let mut echo = Echo::new(COMMAND);
        let (a, b) = COMMAND.split_at(15);
        // Wrapped at the margin, with a colour and a step back over a space.
        echo.feed(&format!(
            "$ {a} \x08\r\n\x1b[1;32m\x1b]0;title\x07{b}\x1b[0m"
        ));
        assert!(echo.found());
    }

    #[test]
    fn the_prompt_alone_is_no_echo() {
        let mut echo = Echo::new(COMMAND);
        echo.feed("'/opt/claude' --res\r\n$ ");
        echo.feed("ume nothing\r\n");
        assert!(!echo.found());
    }

    #[test]
    fn a_long_screen_before_the_echo_does_not_hide_it() {
        let mut echo = Echo::new(COMMAND);
        echo.feed(&"x".repeat(10_000));
        echo.feed(COMMAND);
        assert!(echo.found());
    }

    #[test]
    fn a_saved_session_is_forgotten_only_on_a_second_miss_in_a_row() {
        let mut live = crate::panes_logic::Live::default();
        // Found: kept.
        assert!(!live.session_looked_up("s", Some(true)));
        // One miss keeps it, a find in between starts the count again.
        assert!(!live.session_looked_up("s", Some(false)));
        assert!(!live.session_looked_up("s", Some(true)));
        assert!(!live.session_looked_up("s", Some(false)));
        // A failed lookup says nothing either way.
        assert!(!live.session_looked_up("s", None));
        // The second miss in a row forgets it.
        assert!(live.session_looked_up("s", Some(false)));
        // Per session.
        assert!(!live.session_looked_up("t", Some(false)));
    }
}
