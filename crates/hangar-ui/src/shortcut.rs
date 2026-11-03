//! Keystroke algebra and the recorder behind the Shortcuts settings: a port of
//! `src/lib/keys.ts` and of the `Recorder` of `ShortcutsSettings.tsx`, with the
//! tests of the former.
//!
//! One shape, [`Chord`], for both what the user just pressed and what the
//! settings hold, so matching a keystroke against a binding is a comparison
//! and not a parsing job. A [`Binding`] is one chord or two typed in a row,
//! written the way the settings store them: `"Ctrl+Shift+Enter"`, or
//! `"Ctrl+K Ctrl+S"` for a two-step sequence. The format is the web app's, so
//! the `keybindings` of an existing `state.json` read as they are.
//!
//! ```
//! use hangar_ui::shortcut::{Platform, format_binding_on, parse_binding};
//!
//! let chords = parse_binding("ctrl+k ctrl+s").unwrap();
//! assert_eq!(chords.len(), 2);
//! assert_eq!(format_binding_on(Platform::Other, "ctrl+k ctrl+s"), "Ctrl+K Ctrl+S");
//! assert_eq!(format_binding_on(Platform::Mac, "Ctrl+Shift+Up"), "⌃⇧↑");
//! ```
//!
//! The recorder is a state machine fed with [`Input`]s and answering with an
//! [`Outcome`]; the field that shows it (`shortcut_recorder` in the `form`
//! module) only turns iced's keyboard events into inputs.

use iced::keyboard::{self, Key, Modifiers, key::Code, key::Named, key::Physical};

/// One or more chords, written as the settings store them.
pub type Binding = String;

/// Which family of keyboards the chords are spelled for: a Mac writes
/// `⌃⌥⇧⌘` and the others `Ctrl+Alt+Shift+Win`, and `Mod` means Cmd on the
/// one and Ctrl on the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Mac,
    Other,
}

impl Platform {
    pub const fn current() -> Platform {
        if cfg!(target_os = "macos") {
            Platform::Mac
        } else {
            Platform::Other
        }
    }
}

/// A keystroke: a key and the modifiers held with it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Chord {
    /// Canonical key name: `"A"`, `"1"`, `"Enter"`, `"Up"`, `","`. Never a
    /// modifier.
    pub key: String,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

impl Chord {
    pub fn new(key: impl Into<String>) -> Chord {
        Chord {
            key: key.into(),
            ..Chord::default()
        }
    }

    pub fn ctrl(mut self) -> Chord {
        self.ctrl = true;
        self
    }

    pub fn shift(mut self) -> Chord {
        self.shift = true;
        self
    }

    pub fn alt(mut self) -> Chord {
        self.alt = true;
        self
    }

    pub fn meta(mut self) -> Chord {
        self.meta = true;
        self
    }
}

/// The modifiers held, one flag each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Held {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

impl Held {
    pub fn any(self) -> bool {
        self.ctrl || self.shift || self.alt || self.meta
    }

    /// The modifiers spelled the way the rest of the app spells them, in the
    /// order this platform writes them: what the recorder shows while a chord
    /// is being held matches what it shows once the chord is recorded.
    pub fn segments_on(self, platform: Platform) -> Vec<&'static str> {
        let (ctrl, alt, shift, meta) = match platform {
            Platform::Mac => ("⌃", "⌥", "⇧", "⌘"),
            Platform::Other => ("Ctrl", "Alt", "Shift", "Win"),
        };
        [
            (self.ctrl, ctrl),
            (self.alt, alt),
            (self.shift, shift),
            (self.meta, meta),
        ]
        .into_iter()
        .filter_map(|(held, name)| held.then_some(name))
        .collect()
    }

    pub fn segments(self) -> Vec<&'static str> {
        self.segments_on(Platform::current())
    }
}

impl From<Modifiers> for Held {
    fn from(modifiers: Modifiers) -> Held {
        Held {
            ctrl: modifiers.control(),
            shift: modifiers.shift(),
            alt: modifiers.alt(),
            meta: modifiers.logo(),
        }
    }
}

/// Modifier spellings accepted in a written binding.
fn modifier(name: &str) -> Option<fn(&mut Chord)> {
    Some(match name {
        "ctrl" | "control" | "ctl" => |chord| chord.ctrl = true,
        "shift" => |chord| chord.shift = true,
        "alt" | "option" | "opt" => |chord| chord.alt = true,
        "meta" | "cmd" | "command" | "super" | "win" => |chord| chord.meta = true,
        _ => return None,
    })
}

/// Written in defaults to mean "Cmd on a Mac, Ctrl everywhere else".
const PLATFORM_MODIFIER: &str = "mod";

/// Folds any spelling of a key onto the one name used everywhere else.
pub fn canonical_key(raw: &str) -> Option<String> {
    let text = raw.trim();
    if text.is_empty() {
        return None;
    }
    let lower = text.to_lowercase();
    let alias = match lower.as_str() {
        "esc" | "escape" => Some("Escape"),
        "enter" | "return" => Some("Enter"),
        "space" | "spacebar" => Some("Space"),
        "tab" => Some("Tab"),
        "backspace" => Some("Backspace"),
        "del" | "delete" => Some("Delete"),
        "ins" | "insert" => Some("Insert"),
        "home" => Some("Home"),
        "end" => Some("End"),
        "pgup" | "pageup" => Some("PageUp"),
        "pgdn" | "pagedown" => Some("PageDown"),
        "up" | "arrowup" => Some("Up"),
        "down" | "arrowdown" => Some("Down"),
        "left" | "arrowleft" => Some("Left"),
        "right" | "arrowright" => Some("Right"),
        "plus" => Some("+"),
        "minus" => Some("-"),
        "comma" => Some(","),
        "period" => Some("."),
        "slash" => Some("/"),
        "backslash" => Some("\\"),
        _ => None,
    };
    if let Some(alias) = alias {
        return Some(alias.to_string());
    }
    if is_function_key_name(&lower) {
        return Some(lower.to_uppercase());
    }
    let mut chars = text.chars();
    let first = chars.next()?;
    let rest = chars.as_str();
    if rest.is_empty() {
        // A single character: a letter in capitals, anything else as it is.
        return Some(if first.is_ascii_alphabetic() {
            first.to_ascii_uppercase().to_string()
        } else {
            first.to_string()
        });
    }
    // Anything else keeps its spelling, capitalised ("Home", "F13"...).
    Some(first.to_uppercase().chain(rest.chars()).collect())
}

/// `f1` to `f99` in lower case: the shape `canonical_key` upper-cases.
fn is_function_key_name(lower: &str) -> bool {
    let Some(digits) = lower.strip_prefix('f') else {
        return false;
    };
    (1..=2).contains(&digits.len()) && digits.chars().all(|c| c.is_ascii_digit())
}

/// Splits on `+`, except for a `+` that is the key itself: `"Ctrl++"` is three
/// plus signs' worth of ambiguity that a plain split gets wrong.
fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut buffer = String::new();
    for c in text.chars() {
        if c == '+' && !buffer.trim().is_empty() {
            tokens.push(buffer.trim().to_string());
            buffer.clear();
        } else {
            buffer.push(c);
        }
    }
    if !buffer.trim().is_empty() {
        tokens.push(buffer.trim().to_string());
    }
    tokens
}

/// A chord from its written form on this platform. `None` for anything that is
/// not a whole chord, a lone modifier included.
pub fn parse_chord_on(platform: Platform, text: &str) -> Option<Chord> {
    let tokens = tokenize(text.trim());
    let (key, modifiers) = tokens.split_last()?;
    let mut chord = Chord::default();

    for token in modifiers {
        let name = token.to_lowercase();
        if name == PLATFORM_MODIFIER {
            match platform {
                Platform::Mac => chord.meta = true,
                Platform::Other => chord.ctrl = true,
            }
            continue;
        }
        // An unknown word where a modifier belongs makes the whole thing
        // unusable: guessing would silently bind a key nobody asked for.
        modifier(&name)?(&mut chord);
    }

    let key = canonical_key(key)?;
    let lower = key.to_lowercase();
    if modifier(&lower).is_some() || lower == PLATFORM_MODIFIER {
        return None;
    }
    chord.key = key;
    Some(chord)
}

pub fn parse_chord(text: &str) -> Option<Chord> {
    parse_chord_on(Platform::current(), text)
}

/// The chords of a binding, or `None` as soon as one of them does not parse.
pub fn parse_binding_on(platform: Platform, binding: &str) -> Option<Vec<Chord>> {
    let parts: Vec<&str> = binding.split_whitespace().collect();
    if parts.is_empty() {
        return None;
    }
    parts
        .into_iter()
        .map(|part| parse_chord_on(platform, part))
        .collect()
}

pub fn parse_binding(binding: &str) -> Option<Vec<Chord>> {
    parse_binding_on(Platform::current(), binding)
}

/// The comparable form of a chord: same modifiers in the same order, always.
/// Used as a map key, never shown; [`chord_segments`] is what the eye reads.
pub fn serialize_chord(chord: &Chord) -> String {
    let mut out = String::new();
    for (held, name) in [
        (chord.ctrl, "ctrl+"),
        (chord.alt, "alt+"),
        (chord.shift, "shift+"),
        (chord.meta, "meta+"),
    ] {
        if held {
            out.push_str(name);
        }
    }
    out.push_str(&chord.key);
    out
}

pub fn serialize_chords(chords: &[Chord]) -> String {
    chords
        .iter()
        .map(serialize_chord)
        .collect::<Vec<_>>()
        .join(" ")
}

/// `None` when the binding does not parse, so callers can drop it.
pub fn serialize_binding(binding: &str) -> Option<String> {
    parse_binding(binding).map(|chords| serialize_chords(&chords))
}

fn arrow_symbol(key: &str) -> Option<&'static str> {
    Some(match key {
        "Up" => "↑",
        "Down" => "↓",
        "Left" => "←",
        "Right" => "→",
        _ => return None,
    })
}

/// The pieces of a chord, in the order this platform writes them.
pub fn chord_segments_on(platform: Platform, chord: &Chord) -> Vec<String> {
    let held = Held {
        ctrl: chord.ctrl,
        shift: chord.shift,
        alt: chord.alt,
        meta: chord.meta,
    };
    let mut segments: Vec<String> = held
        .segments_on(platform)
        .into_iter()
        .map(str::to_string)
        .collect();
    segments.push(arrow_symbol(&chord.key).map_or_else(|| chord.key.clone(), str::to_string));
    segments
}

pub fn chord_segments(chord: &Chord) -> Vec<String> {
    chord_segments_on(Platform::current(), chord)
}

pub fn format_chord_on(platform: Platform, chord: &Chord) -> String {
    let separator = match platform {
        Platform::Mac => "",
        Platform::Other => "+",
    };
    chord_segments_on(platform, chord).join(separator)
}

pub fn format_chord(chord: &Chord) -> String {
    format_chord_on(Platform::current(), chord)
}

/// Human form of a whole binding, sequences included: `"Ctrl+K Ctrl+S"`. A
/// binding that does not parse is shown as it is.
pub fn format_binding_on(platform: Platform, binding: &str) -> String {
    match parse_binding_on(platform, binding) {
        Some(chords) => chords
            .iter()
            .map(|chord| format_chord_on(platform, chord))
            .collect::<Vec<_>>()
            .join(" "),
        None => binding.to_string(),
    }
}

pub fn format_binding(binding: &str) -> String {
    format_binding_on(Platform::current(), binding)
}

/// Canonical written form of a chord, which is what gets stored once a key is
/// recorded.
pub fn write_chord(chord: &Chord) -> Binding {
    let mut parts: Vec<&str> = Vec::with_capacity(5);
    if chord.ctrl {
        parts.push("Ctrl");
    }
    if chord.alt {
        parts.push("Alt");
    }
    if chord.shift {
        parts.push("Shift");
    }
    if chord.meta {
        parts.push("Meta");
    }
    parts.push(&chord.key);
    parts.join("+")
}

pub fn write_binding(chords: &[Chord]) -> Binding {
    chords.iter().map(write_chord).collect::<Vec<_>>().join(" ")
}

/// True when a chord carries no modifier that would keep it out of typing.
pub fn is_bare_chord(chord: &Chord) -> bool {
    !chord.ctrl && !chord.alt && !chord.meta
}

/// F1 to F24: keys that never type a character, with a modifier or without.
pub fn is_function_key(key: &str) -> bool {
    let Some(number) = key.strip_prefix('F') else {
        return false;
    };
    !number.starts_with('0')
        && number.len() <= 2
        && number.parse::<u8>().is_ok_and(|n| (1..=24).contains(&n))
}

/// True for the chords a text field owns: Cmd+C on a Mac, Ctrl+C elsewhere.
///
/// Nothing is bound to them by default, but they are the obvious thing to
/// rebind copy and paste onto, and the dispatcher listens above every field in
/// the app. Without this, doing so would take copying away from the workspace
/// name, the broadcast box and the task board at the same time.
pub fn is_text_editing_chord_on(platform: Platform, chord: &Chord) -> bool {
    const EDITING_KEYS: [&str; 6] = ["A", "C", "V", "X", "Z", "Y"];
    if chord.alt || !EDITING_KEYS.contains(&chord.key.as_str()) {
        return false;
    }
    match platform {
        Platform::Mac => chord.meta && !chord.ctrl,
        Platform::Other => chord.ctrl && !chord.meta,
    }
}

pub fn is_text_editing_chord(chord: &Chord) -> bool {
    is_text_editing_chord_on(Platform::current(), chord)
}

// ---- From iced's keyboard events -----------------------------------------------------------

/// What iced says of a key press, gathered, so that the translation to a
/// [`Chord`] can be tested without an event loop.
#[derive(Debug, Clone, Copy)]
pub struct KeyPress<'a> {
    pub key: &'a Key,
    pub physical: Physical,
    pub modifiers: Modifiers,
    /// The text the press produced, if any.
    pub text: Option<&'a str>,
}

/// Keys that only ever qualify another one; a chord can never be made of them.
fn is_bare_modifier_key(named: Named) -> bool {
    matches!(
        named,
        Named::Control
            | Named::Shift
            | Named::Alt
            | Named::AltGraph
            | Named::Meta
            | Named::Super
            | Named::Hyper
            | Named::CapsLock
            | Named::NumLock
            | Named::ScrollLock
            | Named::Fn
            | Named::FnLock
            | Named::Symbol
            | Named::SymbolLock
    )
}

/// Numpad keys that carry a symbol rather than a digit.
fn numpad_symbol(code: Code) -> Option<&'static str> {
    Some(match code {
        Code::NumpadAdd => "+",
        Code::NumpadSubtract => "-",
        Code::NumpadMultiply => "*",
        Code::NumpadDivide => "/",
        Code::NumpadDecimal => ".",
        Code::NumpadEnter => "Enter",
        _ => return None,
    })
}

fn digit_from_code(code: Code) -> Option<char> {
    Some(match code {
        Code::Digit0 | Code::Numpad0 => '0',
        Code::Digit1 | Code::Numpad1 => '1',
        Code::Digit2 | Code::Numpad2 => '2',
        Code::Digit3 | Code::Numpad3 => '3',
        Code::Digit4 | Code::Numpad4 => '4',
        Code::Digit5 | Code::Numpad5 => '5',
        Code::Digit6 | Code::Numpad6 => '6',
        Code::Digit7 | Code::Numpad7 => '7',
        Code::Digit8 | Code::Numpad8 => '8',
        Code::Digit9 | Code::Numpad9 => '9',
        _ => return None,
    })
}

fn letter_from_code(code: Code) -> Option<char> {
    let name = format!("{code:?}");
    let letter = name.strip_prefix("Key")?;
    let mut chars = letter.chars();
    let first = chars.next()?;
    (chars.next().is_none() && first.is_ascii_uppercase()).then_some(first)
}

/// The key of a press, named the way the settings name it.
///
/// The key iced reports without modifiers leads, because it is the character
/// printed on the key the user actually pressed: on an AZERTY board the
/// physical code of the "A" key reads `KeyQ`, and binding by code would put
/// the shortcut on the wrong key. Two cases fall back to the code: a digit row
/// that needs Shift to type digits (AZERTY again: Ctrl+1 has to stay Ctrl+1),
/// and a non-Latin layout, whose key is a character no binding could ever be
/// written with.
pub fn key_from_press(press: &KeyPress<'_>) -> Option<String> {
    let code = match press.physical {
        Physical::Code(code) => Some(code),
        Physical::Unidentified(_) => None,
    };
    if let Some(symbol) = code.and_then(numpad_symbol) {
        return Some(symbol.to_string());
    }

    match press.key {
        Key::Unidentified => None,
        Key::Named(named) if is_bare_modifier_key(*named) => None,
        Key::Named(named) => canonical_key(&format!("{named:?}")),
        Key::Character(text) => {
            let mut chars = text.chars();
            match (chars.next(), chars.next()) {
                (None, _) => None,
                (Some(c), None) => Some(
                    if c.is_ascii_alphabetic() {
                        c.to_ascii_uppercase()
                    } else if c.is_ascii_digit() {
                        c
                    } else if let Some(digit) = code.and_then(digit_from_code) {
                        digit
                    } else if let Some(letter) = code.and_then(letter_from_code) {
                        letter
                    } else if c == ' ' {
                        return Some("Space".to_string());
                    } else {
                        c
                    }
                    .to_string(),
                ),
                _ => canonical_key(text),
            }
        }
    }
}

/// The chord a key press is, or `None` when it is not one: a modifier pressed
/// on its own, or a character being composed with AltGr, which Windows reports
/// as Ctrl+Alt, so without this every `@` typed on an AZERTY board would fire
/// Ctrl+Alt+0.
pub fn chord_from_press(press: &KeyPress<'_>) -> Option<Chord> {
    let held = Held::from(press.modifiers);
    if held.ctrl
        && held.alt
        && press
            .text
            .is_some_and(|text| text.chars().any(|c| !c.is_control()))
    {
        return None;
    }
    let key = key_from_press(press)?;
    Some(Chord {
        key,
        ctrl: held.ctrl,
        shift: held.shift,
        alt: held.alt,
        meta: held.meta,
    })
}

/// What iced's keyboard events mean to the recorder.
pub fn input_from_event(event: &keyboard::Event) -> Option<Input> {
    match event {
        keyboard::Event::KeyPressed {
            key,
            physical_key,
            modifiers,
            text,
            ..
        } => Some(Input::Press {
            chord: chord_from_press(&KeyPress {
                key,
                physical: *physical_key,
                modifiers: *modifiers,
                text: text.as_deref(),
            }),
            held: Held::from(*modifiers),
        }),
        keyboard::Event::KeyReleased { .. } => Some(Input::Release),
        keyboard::Event::ModifiersChanged(modifiers) => Some(Input::Modifiers(Held::from(*modifiers))),
    }
}

// ---- The recorder ---------------------------------------------------------------------------

/// What the recorder is fed with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// A key went down. `chord` is `None` for a modifier on its own, which
    /// still tells which modifiers are held.
    Press { chord: Option<Chord>, held: Held },
    Modifiers(Held),
    Release,
}

/// What the recorder decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Still listening.
    Pending,
    /// Confirmed with Enter: these chords are the new binding.
    Done(Vec<Chord>),
    /// Abandoned with Escape.
    Cancelled,
}

/// Captures keystrokes until the user confirms.
///
/// Enter and Escape on their own steer the recorder rather than being
/// recorded, which is the one thing it cannot bind; held with any modifier
/// they are ordinary keys again, so Ctrl+Shift+Enter stays reachable. That is
/// what buys sequences: every other chord is appended, so Ctrl+K then Ctrl+S
/// records as a single binding.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Recorder {
    chords: Vec<Chord>,
    held: Option<Held>,
}

/// Two chords is as long as a sequence gets; a third starts over rather than
/// growing something nobody could type in time.
pub const MAX_SEQUENCE: usize = 2;

impl Recorder {
    pub fn new() -> Recorder {
        Recorder::default()
    }

    /// The chords captured so far.
    pub fn chords(&self) -> &[Chord] {
        &self.chords
    }

    /// The modifiers held while no chord is complete: shown so the recorder
    /// feels like it is listening.
    pub fn held(&self) -> Option<Held> {
        self.held
    }

    /// The binding as it would be stored, or `None` before the first chord.
    pub fn binding(&self) -> Option<Binding> {
        (!self.chords.is_empty()).then(|| write_binding(&self.chords))
    }

    /// Confirms what is captured, as the check button does. `None` when there
    /// is nothing to confirm.
    pub fn confirm(&self) -> Option<Vec<Chord>> {
        (!self.chords.is_empty()).then(|| self.chords.clone())
    }

    pub fn input(&mut self, input: Input) -> Outcome {
        match input {
            Input::Release => {
                self.held = None;
                Outcome::Pending
            }
            Input::Modifiers(held) => {
                self.held = held.any().then_some(held);
                Outcome::Pending
            }
            Input::Press { chord: None, held } => {
                // Modifiers alone are not a chord, but showing them while
                // they are held is what makes the recorder feel alive.
                self.held = held.any().then_some(held);
                Outcome::Pending
            }
            Input::Press {
                chord: Some(chord), ..
            } => self.chord(chord),
        }
    }

    fn chord(&mut self, chord: Chord) -> Outcome {
        if is_bare_chord(&chord) && !chord.shift {
            match chord.key.as_str() {
                "Escape" => return Outcome::Cancelled,
                "Enter" => {
                    return match self.confirm() {
                        Some(chords) => Outcome::Done(chords),
                        None => Outcome::Pending,
                    };
                }
                _ => {}
            }
        }
        self.held = None;
        if self.chords.len() >= MAX_SEQUENCE {
            self.chords.clear();
        }
        self.chords.push(chord);
        Outcome::Pending
    }
}

#[cfg(test)]
mod tests;
