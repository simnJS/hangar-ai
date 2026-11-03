//! Keys to bytes: what a terminal sends the process when a key is pressed.
//!
//! The encoding is xterm's, which every shell and TUI is written against,
//! with the kitty keyboard protocol on top for the programs that ask for it.
//! The functions here are pure: they take the key as `iced` reports it and the
//! terminal's modes, and return bytes, so the whole table is tested without a
//! window.
//!
//! Text is taken from what the platform composed, never rebuilt from the key.
//! That is how dead keys work: pressing `'` on a US-International layout
//! reports a key with no text at all, and the `e` that follows reports the
//! text `é`. A terminal that derived the character from the key instead
//! would print the accent and the letter separately, or print the accent
//! late; this one prints nothing for the first press and `é` for the second.

use alacritty_terminal::term::TermMode;
use iced::keyboard::key::{Named, Physical};
use iced::keyboard::{self, Key, Location, Modifiers};

/// Whether a key went down, repeated, or came up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEventKind {
    Press,
    Repeat,
    Release,
}

/// A key event, in the terms the encoder needs.
///
/// It carries what `iced::keyboard::Event` carries, except that the text is
/// an owned `String` so that tests and apps can build one without naming
/// `iced`'s string type.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyPress {
    /// The key as the layout names it without any modifier applied. `a`, not
    /// `A`, even with Shift held.
    pub key: Key,
    /// The key with the modifiers applied. `Unidentified` for a dead key.
    pub modified_key: Key,
    pub physical_key: Physical,
    pub location: Location,
    pub modifiers: Modifiers,
    /// The text the platform composed for this press, if any.
    pub text: Option<String>,
    pub kind: KeyEventKind,
}

impl KeyPress {
    /// The key event behind an `iced` keyboard event; `None` for modifier
    /// changes, which carry no key.
    pub fn from_event(event: &keyboard::Event) -> Option<Self> {
        match event {
            keyboard::Event::KeyPressed {
                key,
                modified_key,
                physical_key,
                location,
                modifiers,
                text,
                repeat,
            } => Some(Self {
                key: key.clone(),
                modified_key: modified_key.clone(),
                physical_key: *physical_key,
                location: *location,
                modifiers: *modifiers,
                text: text.as_ref().map(|t| t.to_string()),
                kind: if *repeat {
                    KeyEventKind::Repeat
                } else {
                    KeyEventKind::Press
                },
            }),
            keyboard::Event::KeyReleased {
                key,
                modified_key,
                physical_key,
                location,
                modifiers,
            } => Some(Self {
                key: key.clone(),
                modified_key: modified_key.clone(),
                physical_key: *physical_key,
                location: *location,
                modifiers: *modifiers,
                text: None,
                kind: KeyEventKind::Release,
            }),
            keyboard::Event::ModifiersChanged(_) => None,
        }
    }
}

/// The options of the encoding that belong to the app rather than to the
/// program's modes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyOptions {
    /// Alt sends an escape before the key.
    pub alt_sends_escape: bool,
    /// What Shift+Enter sends in place of a carriage return.
    pub shift_enter: Option<Vec<u8>>,
}

impl Default for KeyOptions {
    fn default() -> Self {
        Self {
            alt_sends_escape: true,
            shift_enter: None,
        }
    }
}

/// The bytes for a key event, or `None` when it sends nothing: a modifier on
/// its own, a dead key, a release the program did not ask to hear about.
pub fn encode_key(press: &KeyPress, mode: TermMode, options: &KeyOptions) -> Option<Vec<u8>> {
    if mode.intersects(TermMode::KITTY_KEYBOARD_PROTOCOL) {
        return kitty(press, mode, options);
    }
    if press.kind == KeyEventKind::Release {
        return None;
    }
    legacy(press, mode, options)
}

/// The modifier parameter of a function key: 1 plus a bit for each of Shift
/// (1), Alt (2), Ctrl (4) and Super (8).
fn modifier_param(modifiers: Modifiers) -> u8 {
    1 + modifiers.shift() as u8
        + 2 * modifiers.alt() as u8
        + 4 * modifiers.control() as u8
        + 8 * modifiers.logo() as u8
}

/// A character's control code on a keyboard with a Ctrl key.
///
/// The letters are the obvious part. The punctuation is what xterm maps onto
/// the rest of the C0 range, and the digits 2 to 8 repeat it for keyboards
/// where the punctuation needs Shift.
fn control_code(c: char) -> Option<u8> {
    Some(match c {
        'a'..='z' => c as u8 - b'a' + 1,
        'A'..='Z' => c as u8 - b'A' + 1,
        '@' | ' ' | '2' | '`' => 0x00,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' | '~' => 0x1e,
        '_' | '-' | '7' | '/' => 0x1f,
        '?' | '8' => 0x7f,
        _ => return None,
    })
}

/// The single character a key stands for, in Latin where the layout is not.
///
/// A shortcut like Ctrl+C has to work on a Russian layout, where the key
/// names `с`: `iced` can map it back to the Latin letter through the physical
/// key.
fn latin_char(press: &KeyPress) -> Option<char> {
    match &press.key {
        Key::Character(c) => {
            let mut chars = c.chars();
            let first = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            if first.is_ascii() {
                Some(first)
            } else {
                press.key.to_latin(press.physical_key).or(Some(first))
            }
        }
        Key::Named(Named::Space) => Some(' '),
        _ => None,
    }
}

/// The text of a press when it is something to type: not empty, and not a
/// control character, which the platform also reports for Ctrl+letter and
/// which the encoder builds itself.
fn typed_text(press: &KeyPress) -> Option<&str> {
    press
        .text
        .as_deref()
        .filter(|t| !t.is_empty() && !t.chars().any(char::is_control))
}

fn with_escape(mut bytes: Vec<u8>, escape: bool) -> Vec<u8> {
    if escape {
        bytes.insert(0, 0x1b);
    }
    bytes
}

/// The `CSI`/`SS3` sequence of a key that has one.
///
/// Without modifiers a key sends `CSI final` (or `SS3 final` for the cursor
/// keys in application mode); with modifiers, `CSI 1;m final`. Keys whose
/// sequence ends in `~` carry their number first: `CSI n ~`, `CSI n;m ~`.
enum Sequence {
    /// `final` byte, `SS3` in application cursor mode (arrows, Home, End).
    Cursor(u8),
    /// `final` byte, always `SS3` unmodified (F1 to F4).
    Function(u8),
    /// A number and `~`.
    Tilde(u8),
}

fn sequence_of(named: Named) -> Option<Sequence> {
    use Named::*;
    Some(match named {
        ArrowUp => Sequence::Cursor(b'A'),
        ArrowDown => Sequence::Cursor(b'B'),
        ArrowRight => Sequence::Cursor(b'C'),
        ArrowLeft => Sequence::Cursor(b'D'),
        Home => Sequence::Cursor(b'H'),
        End => Sequence::Cursor(b'F'),
        F1 => Sequence::Function(b'P'),
        F2 => Sequence::Function(b'Q'),
        F3 => Sequence::Function(b'R'),
        F4 => Sequence::Function(b'S'),
        Insert => Sequence::Tilde(2),
        Delete => Sequence::Tilde(3),
        PageUp => Sequence::Tilde(5),
        PageDown => Sequence::Tilde(6),
        F5 => Sequence::Tilde(15),
        F6 => Sequence::Tilde(17),
        F7 => Sequence::Tilde(18),
        F8 => Sequence::Tilde(19),
        F9 => Sequence::Tilde(20),
        F10 => Sequence::Tilde(21),
        F11 => Sequence::Tilde(23),
        F12 => Sequence::Tilde(24),
        F13 => Sequence::Tilde(25),
        F14 => Sequence::Tilde(26),
        F15 => Sequence::Tilde(28),
        F16 => Sequence::Tilde(29),
        F17 => Sequence::Tilde(31),
        F18 => Sequence::Tilde(32),
        F19 => Sequence::Tilde(33),
        F20 => Sequence::Tilde(34),
        _ => return None,
    })
}

fn legacy(press: &KeyPress, mode: TermMode, options: &KeyOptions) -> Option<Vec<u8>> {
    let m = press.modifiers;
    let (shift, ctrl, alt, logo) = (m.shift(), m.control(), m.alt(), m.logo());
    let escape = alt && options.alt_sends_escape;

    // The keypad in application mode sends its own sequences, and only then:
    // otherwise its keys are the digits and operators they show.
    if press.location == Location::Numpad && mode.contains(TermMode::APP_KEYPAD) && !logo {
        if let Some(byte) = keypad_application(press) {
            return Some(vec![0x1b, b'O', byte]);
        }
    }

    if let Key::Named(named) = &press.key {
        if let Some(sequence) = sequence_of(*named) {
            return Some(encode_sequence(sequence, m, mode));
        }
        match named {
            Named::Enter => {
                if shift && !ctrl && !alt && !logo {
                    return Some(options.shift_enter.clone().unwrap_or_else(|| vec![b'\r']));
                }
                return Some(with_escape(vec![b'\r'], escape));
            }
            Named::Backspace => {
                let byte = if ctrl { 0x08 } else { 0x7f };
                return Some(with_escape(vec![byte], escape));
            }
            Named::Tab => {
                if shift {
                    return Some(with_escape(b"\x1b[Z".to_vec(), escape));
                }
                return Some(with_escape(vec![b'\t'], escape));
            }
            Named::Escape => return Some(with_escape(vec![0x1b], escape)),
            Named::Space => {}
            // Modifiers, locks, media keys and the like type nothing.
            _ => return None,
        }
    }

    // Super combinations belong to the desktop; a terminal that typed their
    // text would insert a stray letter for every window shortcut.
    if logo {
        return None;
    }

    // Ctrl+Alt with text is AltGr on keyboards that report it that way: the
    // text is the point, and neither modifier is meant for the program.
    let alt_gr = ctrl && alt && typed_text(press).is_some();

    if ctrl && !alt_gr {
        if let Some(code) = latin_char(press).and_then(control_code) {
            return Some(with_escape(vec![code], escape));
        }
    }

    if let Some(text) = typed_text(press) {
        return Some(with_escape(text.as_bytes().to_vec(), escape && !alt_gr));
    }

    // No text came with the press. Alt as Meta still needs the character the
    // layout makes of the key, since some platforms (macOS) turn Option+key
    // into a different symbol or into nothing. A press with no text and no
    // Alt is a dead key or an unmapped one, and sends nothing.
    if escape && !ctrl {
        let shifted = match &press.modified_key {
            Key::Character(c) if c.chars().count() == 1 => Some(c.to_string()),
            _ => latin_char(press).map(String::from),
        };
        if let Some(c) = shifted {
            return Some(with_escape(c.into_bytes(), true));
        }
    }
    None
}

/// The final byte of a keypad key in application keypad mode (`SS3 byte`).
fn keypad_application(press: &KeyPress) -> Option<u8> {
    let c = match &press.key {
        Key::Named(Named::Enter) => return Some(b'M'),
        Key::Character(c) => c.chars().next()?,
        _ => return None,
    };
    Some(match c {
        '0' => b'p',
        '1' => b'q',
        '2' => b'r',
        '3' => b's',
        '4' => b't',
        '5' => b'u',
        '6' => b'v',
        '7' => b'w',
        '8' => b'x',
        '9' => b'y',
        '*' => b'j',
        '+' => b'k',
        ',' => b'l',
        '-' => b'm',
        '.' => b'n',
        '/' => b'o',
        '=' => b'X',
        _ => return None,
    })
}

fn encode_sequence(sequence: Sequence, modifiers: Modifiers, mode: TermMode) -> Vec<u8> {
    let param = modifier_param(modifiers);
    let modified = param > 1;
    match sequence {
        Sequence::Cursor(final_byte) => {
            if modified {
                format!("\x1b[1;{param}{}", final_byte as char).into_bytes()
            } else if mode.contains(TermMode::APP_CURSOR) {
                vec![0x1b, b'O', final_byte]
            } else {
                vec![0x1b, b'[', final_byte]
            }
        }
        Sequence::Function(final_byte) => {
            if modified {
                format!("\x1b[1;{param}{}", final_byte as char).into_bytes()
            } else {
                vec![0x1b, b'O', final_byte]
            }
        }
        Sequence::Tilde(number) => {
            if modified {
                format!("\x1b[{number};{param}~").into_bytes()
            } else {
                format!("\x1b[{number}~").into_bytes()
            }
        }
    }
}

// ---- The kitty keyboard protocol ------------------------------------------

/// The unicode key number of a key that is sent as `CSI number u`.
fn kitty_code(press: &KeyPress) -> Option<u32> {
    match &press.key {
        Key::Named(Named::Escape) => Some(27),
        Key::Named(Named::Enter) => Some(13),
        Key::Named(Named::Tab) => Some(9),
        Key::Named(Named::Backspace) => Some(127),
        Key::Named(Named::Space) => Some(32),
        Key::Character(_) => {
            let c = latin_char(press).or_else(|| match &press.key {
                Key::Character(c) => c.chars().next(),
                _ => None,
            })?;
            // The protocol names a key by its unshifted, lowercase codepoint.
            Some(c.to_lowercase().next().unwrap_or(c) as u32)
        }
        _ => None,
    }
}

fn kitty(press: &KeyPress, mode: TermMode, options: &KeyOptions) -> Option<Vec<u8>> {
    let report_events = mode.contains(TermMode::REPORT_EVENT_TYPES);
    let report_all = mode.contains(TermMode::REPORT_ALL_KEYS_AS_ESC);
    let report_text = mode.contains(TermMode::REPORT_ASSOCIATED_TEXT);
    let report_alternates = mode.contains(TermMode::REPORT_ALTERNATE_KEYS);

    let m = press.modifiers;
    let param = modifier_param(m);
    // Releases and repeats are only reported when the program asked.
    let event = match press.kind {
        KeyEventKind::Press => 1,
        KeyEventKind::Repeat => {
            if report_events {
                2
            } else {
                1
            }
        }
        KeyEventKind::Release => {
            if !report_events {
                return None;
            }
            3
        }
    };
    let released = event == 3;
    // The `;modifiers:event` field, left out while it holds its defaults.
    let modifiers_field = |trailing: bool| -> String {
        if event == 1 && param == 1 && !trailing {
            String::new()
        } else if event == 1 {
            format!(";{param}")
        } else {
            format!(";{param}:{event}")
        }
    };

    // Keys that have a classic sequence keep its shape.
    if let Key::Named(named) = &press.key {
        if let Some(sequence) = sequence_of(*named) {
            if param == 1 && event == 1 {
                return Some(encode_sequence(sequence, m, mode));
            }
            let field = modifiers_field(true);
            return Some(match sequence {
                Sequence::Cursor(f) | Sequence::Function(f) => {
                    format!("\x1b[1{field}{}", f as char).into_bytes()
                }
                Sequence::Tilde(n) => format!("\x1b[{n}{field}~").into_bytes(),
            });
        }
    }

    let plain_modifiers = !(m.control() || m.alt() || m.logo());
    let code = kitty_code(press);

    // Enter, Tab and Backspace stay what they always were until the program
    // asks for everything, or a modifier makes them ambiguous.
    let legacy_named = matches!(
        press.key,
        Key::Named(Named::Enter | Named::Tab | Named::Backspace)
    );
    if legacy_named && !report_all && param == 1 {
        return if released { None } else { legacy(press, mode, options) };
    }

    // Text keys with nothing but Shift are typed as text, as they always were,
    // unless the program wants every key as an escape code.
    if matches!(press.key, Key::Character(_) | Key::Named(Named::Space)) && plain_modifiers && !report_all {
        if released {
            return None;
        }
        return legacy(press, mode, options);
    }

    // Ctrl+Alt with composed text is AltGr, which is text.
    if !report_all && m.control() && m.alt() && typed_text(press).is_some() && !released {
        return legacy(press, mode, options);
    }

    let code = code?;
    // A press that produced no text and carries no modifier, in report-all
    // mode, is e.g. a dead key: nothing to report yet.
    if report_all
        && matches!(press.key, Key::Character(_))
        && press.text.is_none()
        && param == 1
        && !released
        && matches!(press.modified_key, Key::Unidentified)
    {
        return None;
    }

    let mut number = code.to_string();
    if report_alternates && m.shift() {
        if let Key::Character(c) = &press.modified_key {
            if let Some(shifted) = c.chars().next() {
                if shifted as u32 != code {
                    number = format!("{code}:{}", shifted as u32);
                }
            }
        }
    }

    let mut out = format!("\x1b[{number}");
    let text_field = if report_all && report_text && !released {
        typed_text(press).map(|t| {
            t.chars()
                .map(|c| (c as u32).to_string())
                .collect::<Vec<_>>()
                .join(":")
        })
    } else {
        None
    };
    match &text_field {
        Some(text) => {
            out.push_str(&format!(";{param}"));
            if event != 1 {
                out.push_str(&format!(":{event}"));
            }
            out.push(';');
            out.push_str(text);
        }
        None => out.push_str(&modifiers_field(false)),
    }
    out.push('u');
    Some(out.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::key::Code;

    fn ch(c: &str) -> Key {
        Key::Character(c.into())
    }

    fn press(key: Key, modifiers: Modifiers, text: Option<&str>) -> KeyPress {
        KeyPress {
            modified_key: key.clone(),
            key,
            physical_key: Physical::Unidentified(
                iced::keyboard::key::NativeCode::Unidentified,
            ),
            location: Location::Standard,
            modifiers,
            text: text.map(str::to_owned),
            kind: KeyEventKind::Press,
        }
    }

    fn enc(key: Key, modifiers: Modifiers, text: Option<&str>) -> Option<String> {
        enc_in(key, modifiers, text, TermMode::NONE)
    }

    fn enc_in(key: Key, modifiers: Modifiers, text: Option<&str>, mode: TermMode) -> Option<String> {
        encode_key(&press(key, modifiers, text), mode, &KeyOptions::default())
            .map(|b| String::from_utf8(b).unwrap())
    }

    const NONE: Modifiers = Modifiers::empty();
    const SHIFT: Modifiers = Modifiers::SHIFT;
    const CTRL: Modifiers = Modifiers::CTRL;
    const ALT: Modifiers = Modifiers::ALT;

    #[test]
    fn text_is_typed_as_the_platform_composed_it() {
        assert_eq!(enc(ch("a"), NONE, Some("a")).as_deref(), Some("a"));
        assert_eq!(enc(ch("a"), SHIFT, Some("A")).as_deref(), Some("A"));
        assert_eq!(enc(ch("é"), NONE, Some("é")).as_deref(), Some("é"));
        assert_eq!(enc(Key::Named(Named::Space), NONE, Some(" ")).as_deref(), Some(" "));
    }

    #[test]
    fn a_dead_key_sends_nothing_and_the_next_press_sends_the_composed_letter() {
        // US-International: `'` is a dead key. The platform reports it with no
        // text and no identified key.
        let dead = KeyPress {
            modified_key: Key::Unidentified,
            ..press(ch("'"), NONE, None)
        };
        assert_eq!(encode_key(&dead, TermMode::NONE, &KeyOptions::default()), None);
        // `e` after it arrives with the composed text.
        assert_eq!(enc(ch("e"), NONE, Some("é")).as_deref(), Some("é"));
        // `'` then space gives the apostrophe itself, on the space key.
        assert_eq!(enc(Key::Named(Named::Space), NONE, Some("'")).as_deref(), Some("'"));
        // And a key that does not combine produces both: the platform reports
        // the accent and the letter in one text.
        assert_eq!(enc(ch("x"), NONE, Some("´x")).as_deref(), Some("´x"));
    }

    #[test]
    fn control_letters_become_c0_codes() {
        assert_eq!(enc(ch("a"), CTRL, Some("\u{1}")).as_deref(), Some("\u{1}"));
        assert_eq!(enc(ch("c"), CTRL, Some("\u{3}")).as_deref(), Some("\u{3}"));
        // Even when the platform reports no text at all.
        assert_eq!(enc(ch("d"), CTRL, None).as_deref(), Some("\u{4}"));
        assert_eq!(enc(ch("["), CTRL, None).as_deref(), Some("\u{1b}"));
        assert_eq!(enc(ch("\\"), CTRL, None).as_deref(), Some("\u{1c}"));
        assert_eq!(enc(ch(" "), CTRL, None).as_deref(), Some("\0"));
        assert_eq!(enc(Key::Named(Named::Space), CTRL, Some(" ")).as_deref(), Some("\0"));
        assert_eq!(enc(ch("2"), CTRL, None).as_deref(), Some("\0"));
        assert_eq!(enc(ch("/"), CTRL, None).as_deref(), Some("\u{1f}"));
    }

    #[test]
    fn ctrl_shortcuts_follow_the_latin_letter_on_other_layouts() {
        let mut russian_c = press(ch("с"), CTRL, Some("\u{3}"));
        russian_c.physical_key = Physical::Code(Code::KeyC);
        assert_eq!(
            encode_key(&russian_c, TermMode::NONE, &KeyOptions::default()),
            Some(vec![3])
        );
    }

    #[test]
    fn alt_is_an_escape_prefix() {
        assert_eq!(enc(ch("b"), ALT, Some("b")).as_deref(), Some("\u{1b}b"));
        assert_eq!(enc(ch("b"), ALT | SHIFT, Some("B")).as_deref(), Some("\u{1b}B"));
        assert_eq!(enc(ch("f"), ALT | CTRL, Some("\u{6}")).as_deref(), Some("\u{1b}\u{6}"));
        // No text came with it (a platform that eats Option+key): the key is
        // enough.
        assert_eq!(enc(ch("d"), ALT, None).as_deref(), Some("\u{1b}d"));
        assert_eq!(enc(Key::Named(Named::Backspace), ALT, None).as_deref(), Some("\u{1b}\u{7f}"));
        assert_eq!(enc(Key::Named(Named::Enter), ALT, None).as_deref(), Some("\u{1b}\r"));
    }

    #[test]
    fn alt_gr_text_is_not_prefixed() {
        assert_eq!(enc(ch("e"), CTRL | ALT, Some("€")).as_deref(), Some("€"));
    }

    #[test]
    fn alt_can_be_left_to_the_layout() {
        let options = KeyOptions { alt_sends_escape: false, ..KeyOptions::default() };
        let p = press(ch("b"), ALT, Some("∫"));
        assert_eq!(encode_key(&p, TermMode::NONE, &options), Some("∫".as_bytes().to_vec()));
    }

    #[test]
    fn super_combinations_type_nothing() {
        assert_eq!(enc(ch("e"), Modifiers::LOGO, Some("e")), None);
    }

    #[test]
    fn editing_keys() {
        let named = |n| Key::Named(n);
        assert_eq!(enc(named(Named::Enter), NONE, Some("\r")).as_deref(), Some("\r"));
        assert_eq!(enc(named(Named::Enter), SHIFT, Some("\r")).as_deref(), Some("\r"));
        assert_eq!(enc(named(Named::Backspace), NONE, Some("\u{8}")).as_deref(), Some("\u{7f}"));
        assert_eq!(enc(named(Named::Backspace), CTRL, None).as_deref(), Some("\u{8}"));
        assert_eq!(enc(named(Named::Tab), NONE, Some("\t")).as_deref(), Some("\t"));
        assert_eq!(enc(named(Named::Tab), SHIFT, None).as_deref(), Some("\u{1b}[Z"));
        assert_eq!(enc(named(Named::Escape), NONE, None).as_deref(), Some("\u{1b}"));
        assert_eq!(enc(named(Named::Delete), NONE, None).as_deref(), Some("\u{1b}[3~"));
        assert_eq!(enc(named(Named::Insert), NONE, None).as_deref(), Some("\u{1b}[2~"));
        assert_eq!(enc(named(Named::PageUp), NONE, None).as_deref(), Some("\u{1b}[5~"));
        assert_eq!(enc(named(Named::PageDown), NONE, None).as_deref(), Some("\u{1b}[6~"));
    }

    #[test]
    fn shift_enter_can_be_made_a_newline_for_agent_panes() {
        let options = KeyOptions { shift_enter: Some(b"\x1b\r".to_vec()), ..KeyOptions::default() };
        let shift_enter = press(Key::Named(Named::Enter), SHIFT, Some("\r"));
        assert_eq!(encode_key(&shift_enter, TermMode::NONE, &options), Some(b"\x1b\r".to_vec()));
        let enter = press(Key::Named(Named::Enter), NONE, Some("\r"));
        assert_eq!(encode_key(&enter, TermMode::NONE, &options), Some(b"\r".to_vec()));
    }

    #[test]
    fn cursor_keys_follow_application_mode_and_modifiers() {
        let up = Key::Named(Named::ArrowUp);
        assert_eq!(enc(up.clone(), NONE, None).as_deref(), Some("\u{1b}[A"));
        assert_eq!(
            enc_in(up.clone(), NONE, None, TermMode::APP_CURSOR).as_deref(),
            Some("\u{1b}OA")
        );
        // Modifiers force the CSI form even in application mode.
        assert_eq!(
            enc_in(up.clone(), SHIFT, None, TermMode::APP_CURSOR).as_deref(),
            Some("\u{1b}[1;2A")
        );
        assert_eq!(enc(Key::Named(Named::ArrowRight), CTRL, None).as_deref(), Some("\u{1b}[1;5C"));
        assert_eq!(enc(Key::Named(Named::ArrowLeft), ALT, None).as_deref(), Some("\u{1b}[1;3D"));
        assert_eq!(enc(Key::Named(Named::ArrowDown), CTRL | SHIFT, None).as_deref(), Some("\u{1b}[1;6B"));
        assert_eq!(enc(Key::Named(Named::Home), NONE, None).as_deref(), Some("\u{1b}[H"));
        assert_eq!(
            enc_in(Key::Named(Named::End), NONE, None, TermMode::APP_CURSOR).as_deref(),
            Some("\u{1b}OF")
        );
        assert_eq!(enc(Key::Named(Named::Home), CTRL, None).as_deref(), Some("\u{1b}[1;5H"));
        assert_eq!(enc(Key::Named(Named::Delete), CTRL, None).as_deref(), Some("\u{1b}[3;5~"));
    }

    #[test]
    fn function_keys() {
        let f = |n| Key::Named(n);
        assert_eq!(enc(f(Named::F1), NONE, None).as_deref(), Some("\u{1b}OP"));
        assert_eq!(enc(f(Named::F4), NONE, None).as_deref(), Some("\u{1b}OS"));
        assert_eq!(enc(f(Named::F1), SHIFT, None).as_deref(), Some("\u{1b}[1;2P"));
        assert_eq!(enc(f(Named::F5), NONE, None).as_deref(), Some("\u{1b}[15~"));
        assert_eq!(enc(f(Named::F12), NONE, None).as_deref(), Some("\u{1b}[24~"));
        assert_eq!(enc(f(Named::F6), CTRL, None).as_deref(), Some("\u{1b}[17;5~"));
        assert_eq!(enc(f(Named::F20), NONE, None).as_deref(), Some("\u{1b}[34~"));
    }

    #[test]
    fn modifier_keys_alone_send_nothing() {
        assert_eq!(enc(Key::Named(Named::Shift), SHIFT, None), None);
        assert_eq!(enc(Key::Named(Named::Control), CTRL, None), None);
        assert_eq!(enc(Key::Named(Named::CapsLock), NONE, None), None);
    }

    #[test]
    fn the_keypad_in_application_mode() {
        let mut p = press(ch("5"), NONE, Some("5"));
        p.location = Location::Numpad;
        assert_eq!(encode_key(&p, TermMode::NONE, &KeyOptions::default()), Some(b"5".to_vec()));
        assert_eq!(
            encode_key(&p, TermMode::APP_KEYPAD, &KeyOptions::default()),
            Some(b"\x1bOu".to_vec())
        );
        let mut enter = press(Key::Named(Named::Enter), NONE, Some("\r"));
        enter.location = Location::Numpad;
        assert_eq!(
            encode_key(&enter, TermMode::APP_KEYPAD, &KeyOptions::default()),
            Some(b"\x1bOM".to_vec())
        );
    }

    #[test]
    fn releases_are_ignored_without_the_kitty_protocol() {
        let mut p = press(ch("a"), NONE, None);
        p.kind = KeyEventKind::Release;
        assert_eq!(encode_key(&p, TermMode::NONE, &KeyOptions::default()), None);
    }

    // ---- kitty ----

    const KITTY_1: TermMode = TermMode::DISAMBIGUATE_ESC_CODES;

    #[test]
    fn kitty_disambiguates_escape_and_modified_enter() {
        assert_eq!(enc_in(Key::Named(Named::Escape), NONE, None, KITTY_1).as_deref(), Some("\u{1b}[27u"));
        assert_eq!(enc_in(Key::Named(Named::Enter), SHIFT, None, KITTY_1).as_deref(), Some("\u{1b}[13;2u"));
        assert_eq!(enc_in(Key::Named(Named::Enter), NONE, Some("\r"), KITTY_1).as_deref(), Some("\r"));
        assert_eq!(enc_in(Key::Named(Named::Tab), NONE, Some("\t"), KITTY_1).as_deref(), Some("\t"));
        assert_eq!(enc_in(Key::Named(Named::Tab), SHIFT, None, KITTY_1).as_deref(), Some("\u{1b}[9;2u"));
        assert_eq!(enc_in(Key::Named(Named::Backspace), CTRL, None, KITTY_1).as_deref(), Some("\u{1b}[127;5u"));
    }

    #[test]
    fn kitty_sends_modified_letters_as_csi_u_and_plain_ones_as_text() {
        assert_eq!(enc_in(ch("c"), CTRL, Some("\u{3}"), KITTY_1).as_deref(), Some("\u{1b}[99;5u"));
        assert_eq!(enc_in(ch("a"), ALT, Some("a"), KITTY_1).as_deref(), Some("\u{1b}[97;3u"));
        assert_eq!(enc_in(ch("a"), NONE, Some("a"), KITTY_1).as_deref(), Some("a"));
        assert_eq!(enc_in(ch("a"), SHIFT, Some("A"), KITTY_1).as_deref(), Some("A"));
        assert_eq!(enc_in(ch("e"), NONE, Some("é"), KITTY_1).as_deref(), Some("é"));
        // A dead key still sends nothing.
        let dead = KeyPress { modified_key: Key::Unidentified, ..press(ch("'"), NONE, None) };
        assert_eq!(encode_key(&dead, KITTY_1, &KeyOptions::default()), None);
    }

    #[test]
    fn kitty_keeps_the_classic_shape_of_cursor_keys() {
        assert_eq!(enc_in(Key::Named(Named::ArrowUp), NONE, None, KITTY_1).as_deref(), Some("\u{1b}[A"));
        assert_eq!(enc_in(Key::Named(Named::ArrowUp), SHIFT, None, KITTY_1).as_deref(), Some("\u{1b}[1;2A"));
        assert_eq!(enc_in(Key::Named(Named::Delete), CTRL, None, KITTY_1).as_deref(), Some("\u{1b}[3;5~"));
    }

    #[test]
    fn kitty_reports_repeats_and_releases_when_asked() {
        let mode = KITTY_1 | TermMode::REPORT_EVENT_TYPES;
        let mut up = press(Key::Named(Named::ArrowUp), NONE, None);
        up.kind = KeyEventKind::Release;
        assert_eq!(encode_key(&up, mode, &KeyOptions::default()), Some(b"\x1b[1;1:3A".to_vec()));
        up.kind = KeyEventKind::Repeat;
        assert_eq!(encode_key(&up, mode, &KeyOptions::default()), Some(b"\x1b[1;1:2A".to_vec()));
        up.kind = KeyEventKind::Release;
        // Text keys are typed, and their release is not reported.
        let mut a = press(ch("a"), NONE, None);
        a.kind = KeyEventKind::Release;
        assert_eq!(encode_key(&a, mode, &KeyOptions::default()), None);
        // Without the flag, releases are silent.
        assert_eq!(encode_key(&up, KITTY_1, &KeyOptions::default()), None);
    }

    #[test]
    fn kitty_report_all_keys_sends_everything_as_escape_codes() {
        let mode = KITTY_1 | TermMode::REPORT_ALL_KEYS_AS_ESC;
        assert_eq!(enc_in(ch("a"), NONE, Some("a"), mode).as_deref(), Some("\u{1b}[97u"));
        assert_eq!(enc_in(Key::Named(Named::Enter), NONE, Some("\r"), mode).as_deref(), Some("\u{1b}[13u"));
        let with_text = mode | TermMode::REPORT_ASSOCIATED_TEXT;
        assert_eq!(enc_in(ch("a"), SHIFT, Some("A"), with_text).as_deref(), Some("\u{1b}[97;2;65u"));
    }
}
