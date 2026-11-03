//! Keystrokes as the dispatcher compares them.
//!
//! The chord algebra itself (reading `"Ctrl+Shift+Enter"`, spelling a chord
//! for the eye, the recorder of the Shortcuts settings) is
//! `hangar_ui::shortcut`, the port of `src/lib/keys.ts` with its tests. What
//! this module adds is what the dispatcher needs on top of it: a chord packed
//! into one integer, a [`Stroke`], and the way from iced's key press to that
//! integer without building a string.
//!
//! The second part is on the typing path. Every key pressed in a terminal is
//! asked about twice before the terminal sees it (its key filter, then the
//! dispatcher's subscription), so the keys people type all day (letters,
//! digits, punctuation, Enter, the arrows, the function keys) are named with
//! a table and a few comparisons. Only a key nobody would type in a terminal
//! (a media key, Print Screen), and only when some binding names one, goes
//! through `hangar_ui`'s spelling, which allocates.
//!
//! The rules are `keyFromEvent` and `chordFromEvent` of keys.ts, as
//! `hangar_ui::shortcut::chord_from_press` ports them: the tests below hold
//! the two to the same answer for every case keys.test.ts lists.

use iced::keyboard::key::{Code, Named, Physical};
use iced::keyboard::{Key, Modifiers};

use hangar_ui::shortcut::{Chord, Platform, canonical_key};

/// A chord in one integer: the key in the low 32 bits, the modifiers above.
///
/// The key is a character's code point (`'A'`, `'1'`, `','`), one of the
/// named keys of [`NAMED`] above the last code point, a name an index learnt
/// from its bindings above those ([`Names`]), or [`UNKNOWN`], which no binding
/// can hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Stroke(u64);

const CTRL: u64 = 1 << 32;
const ALT: u64 = 1 << 33;
const SHIFT: u64 = 1 << 34;
const META: u64 = 1 << 35;
const KEY: u64 = 0xFFFF_FFFF;

/// The first id past the last Unicode code point.
const NAMED_BASE: u32 = 0x11_0000;
/// Where the names an index learnt from its bindings start.
const LEARNT_BASE: u32 = 0x12_0000;
/// A key no binding names: it completes no sequence and starts none.
const UNKNOWN: u32 = 0x1F_FFFF;

/// The named keys, by the name the settings write them with. Their order is
/// their id; the function keys are the last 35, from F1.
const NAMED: [(&str, Named); 50] = [
    ("Escape", Named::Escape),
    ("Enter", Named::Enter),
    ("Space", Named::Space),
    ("Tab", Named::Tab),
    ("Backspace", Named::Backspace),
    ("Delete", Named::Delete),
    ("Insert", Named::Insert),
    ("Home", Named::Home),
    ("End", Named::End),
    ("PageUp", Named::PageUp),
    ("PageDown", Named::PageDown),
    ("Up", Named::ArrowUp),
    ("Down", Named::ArrowDown),
    ("Left", Named::ArrowLeft),
    ("Right", Named::ArrowRight),
    ("F1", Named::F1),
    ("F2", Named::F2),
    ("F3", Named::F3),
    ("F4", Named::F4),
    ("F5", Named::F5),
    ("F6", Named::F6),
    ("F7", Named::F7),
    ("F8", Named::F8),
    ("F9", Named::F9),
    ("F10", Named::F10),
    ("F11", Named::F11),
    ("F12", Named::F12),
    ("F13", Named::F13),
    ("F14", Named::F14),
    ("F15", Named::F15),
    ("F16", Named::F16),
    ("F17", Named::F17),
    ("F18", Named::F18),
    ("F19", Named::F19),
    ("F20", Named::F20),
    ("F21", Named::F21),
    ("F22", Named::F22),
    ("F23", Named::F23),
    ("F24", Named::F24),
    ("F25", Named::F25),
    ("F26", Named::F26),
    ("F27", Named::F27),
    ("F28", Named::F28),
    ("F29", Named::F29),
    ("F30", Named::F30),
    ("F31", Named::F31),
    ("F32", Named::F32),
    ("F33", Named::F33),
    ("F34", Named::F34),
    ("F35", Named::F35),
];

/// Where F1 sits in [`NAMED`].
const F1_AT: u32 = 15;
const SPACE: u32 = NAMED_BASE + 2;
const ENTER: u32 = NAMED_BASE + 1;

fn named_id(named: Named) -> Option<u32> {
    NAMED
        .iter()
        .position(|(_, entry)| *entry == named)
        .map(|at| NAMED_BASE + at as u32)
}

fn name_id(name: &str) -> Option<u32> {
    NAMED
        .iter()
        .position(|(entry, _)| *entry == name)
        .map(|at| NAMED_BASE + at as u32)
}

/// The names of keys outside [`NAMED`] that an index's bindings use: a media
/// key, `PrintScreen`, `ContextMenu`. Learnt while the index is built, so that
/// a press of one of them can be named without a table of every key iced
/// knows. Almost always empty.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Names(Vec<Box<str>>);

impl Names {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn learn(&mut self, name: &str) -> u32 {
        match self.find(name) {
            Some(id) => id,
            None => {
                self.0.push(name.into());
                LEARNT_BASE + (self.0.len() - 1) as u32
            }
        }
    }

    fn find(&self, name: &str) -> Option<u32> {
        self.0
            .iter()
            .position(|entry| &**entry == name)
            .map(|at| LEARNT_BASE + at as u32)
    }

    fn name(&self, id: u32) -> Option<&str> {
        let at = id.checked_sub(LEARNT_BASE)? as usize;
        self.0.get(at).map(|name| &**name)
    }
}

/// The id of a canonical key name (`"A"`, `"Enter"`, `"F6"`, `","`), or
/// `None` when it is a name `names` has not learnt.
fn key_id(key: &str, names: &Names) -> Option<u32> {
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Some(c as u32),
        _ => name_id(key).or_else(|| names.find(key)),
    }
}

impl Stroke {
    fn new(key: u32, ctrl: bool, alt: bool, shift: bool, meta: bool) -> Stroke {
        let mut bits = u64::from(key);
        if ctrl {
            bits |= CTRL;
        }
        if alt {
            bits |= ALT;
        }
        if shift {
            bits |= SHIFT;
        }
        if meta {
            bits |= META;
        }
        Stroke(bits)
    }

    /// The stroke of a chord read from a binding, learning its key's name
    /// into `names` if it is one the table does not have.
    pub fn learn(chord: &Chord, names: &mut Names) -> Stroke {
        let key = match key_id(&chord.key, names) {
            Some(id) => id,
            None => names.learn(&chord.key),
        };
        Stroke::new(key, chord.ctrl, chord.alt, chord.shift, chord.meta)
    }

    /// The stroke of a chord, for looking it up: a key `names` does not know
    /// is [`UNKNOWN`], which matches nothing.
    pub fn of(chord: &Chord, names: &Names) -> Stroke {
        let key = key_id(&chord.key, names).unwrap_or(UNKNOWN);
        Stroke::new(key, chord.ctrl, chord.alt, chord.shift, chord.meta)
    }

    fn key(self) -> u32 {
        (self.0 & KEY) as u32
    }

    pub fn ctrl(self) -> bool {
        self.0 & CTRL != 0
    }

    pub fn alt(self) -> bool {
        self.0 & ALT != 0
    }

    pub fn shift(self) -> bool {
        self.0 & SHIFT != 0
    }

    pub fn meta(self) -> bool {
        self.0 & META != 0
    }

    /// No modifier that would keep it out of typing (`isBareChord`).
    pub fn is_bare(self) -> bool {
        !self.ctrl() && !self.alt() && !self.meta()
    }

    /// F1 to F24, which type nothing anywhere (`isFunctionKey`).
    pub fn is_function_key(self) -> bool {
        let first = NAMED_BASE + F1_AT;
        (first..first + 24).contains(&self.key())
    }

    /// The chords a text field owns: Ctrl+A/C/V/X/Z/Y, Cmd on a Mac
    /// (`isTextEditingChord`).
    pub fn is_text_editing(self, platform: Platform) -> bool {
        const EDITING: [char; 6] = ['A', 'C', 'V', 'X', 'Z', 'Y'];
        let editing = char::from_u32(self.key()).is_some_and(|c| EDITING.contains(&c));
        if self.alt() || !editing {
            return false;
        }
        match platform {
            Platform::Mac => self.meta() && !self.ctrl(),
            Platform::Other => self.ctrl() && !self.meta(),
        }
    }

    /// The chord back, for showing it (the chord hint) or comparing it with a
    /// written binding.
    pub fn chord(self, names: &Names) -> Chord {
        let id = self.key();
        let key = if id < NAMED_BASE {
            char::from_u32(id).map(String::from).unwrap_or_default()
        } else if id < LEARNT_BASE {
            NAMED
                .get((id - NAMED_BASE) as usize)
                .map(|(name, _)| (*name).to_string())
                .unwrap_or_default()
        } else {
            names.name(id).map(str::to_string).unwrap_or_default()
        };
        Chord {
            key,
            ctrl: self.ctrl(),
            shift: self.shift(),
            alt: self.alt(),
            meta: self.meta(),
        }
    }
}

/// Keys that only ever qualify another one; a chord is never made of them.
fn is_bare_modifier(named: Named) -> bool {
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
fn numpad_symbol(code: Code) -> Option<u32> {
    Some(match code {
        Code::NumpadAdd => '+' as u32,
        Code::NumpadSubtract => '-' as u32,
        Code::NumpadMultiply => '*' as u32,
        Code::NumpadDivide => '/' as u32,
        Code::NumpadDecimal => '.' as u32,
        Code::NumpadEnter => ENTER,
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
    Some(match code {
        Code::KeyA => 'A',
        Code::KeyB => 'B',
        Code::KeyC => 'C',
        Code::KeyD => 'D',
        Code::KeyE => 'E',
        Code::KeyF => 'F',
        Code::KeyG => 'G',
        Code::KeyH => 'H',
        Code::KeyI => 'I',
        Code::KeyJ => 'J',
        Code::KeyK => 'K',
        Code::KeyL => 'L',
        Code::KeyM => 'M',
        Code::KeyN => 'N',
        Code::KeyO => 'O',
        Code::KeyP => 'P',
        Code::KeyQ => 'Q',
        Code::KeyR => 'R',
        Code::KeyS => 'S',
        Code::KeyT => 'T',
        Code::KeyU => 'U',
        Code::KeyV => 'V',
        Code::KeyW => 'W',
        Code::KeyX => 'X',
        Code::KeyY => 'Y',
        Code::KeyZ => 'Z',
        _ => return None,
    })
}

/// The stroke of a key press, or `None` when the press is not a chord: a
/// modifier on its own, or a character composed with AltGr (which Windows
/// reports as Ctrl+Alt: without this, every `@` typed on an AZERTY board
/// would fire Ctrl+Alt+0).
///
/// `key` is the key as the layout names it without modifiers, so the printed
/// letter leads (AZERTY's A is `KeyQ` to the hardware); the physical code is
/// the fallback for a digit row that needs Shift (AZERTY's `&` key stays `1`)
/// and for a non-Latin layout, whose letters no binding could be written in.
pub fn stroke_of_press(
    key: &Key,
    physical: Physical,
    modifiers: Modifiers,
    text: Option<&str>,
    names: &Names,
) -> Option<Stroke> {
    let ctrl = modifiers.control();
    let alt = modifiers.alt();
    if ctrl && alt && text.is_some_and(|text| text.chars().any(|c| !c.is_control())) {
        return None;
    }
    let code = match physical {
        Physical::Code(code) => Some(code),
        Physical::Unidentified(_) => None,
    };
    let id = match code.and_then(numpad_symbol) {
        Some(id) => id,
        None => match key {
            Key::Unidentified => return None,
            Key::Named(named) if is_bare_modifier(*named) => return None,
            Key::Named(named) => named_id(*named).unwrap_or_else(|| learnt_named(*named, names)),
            Key::Character(text) => {
                let mut chars = text.chars();
                match (chars.next(), chars.next()) {
                    (None, _) => return None,
                    (Some(c), None) => {
                        if c.is_ascii_alphabetic() {
                            c.to_ascii_uppercase() as u32
                        } else if c.is_ascii_digit() {
                            c as u32
                        } else if let Some(digit) = code.and_then(digit_from_code) {
                            digit as u32
                        } else if let Some(letter) = code.and_then(letter_from_code) {
                            letter as u32
                        } else if c == ' ' {
                            SPACE
                        } else {
                            c as u32
                        }
                    }
                    // A key that types several characters at once: rare
                    // enough to be spelled the slow way.
                    _ => {
                        let name = canonical_key(text)?;
                        key_id(&name, names).unwrap_or(UNKNOWN)
                    }
                }
            }
        },
    };
    Some(Stroke::new(
        id,
        ctrl,
        alt,
        modifiers.shift(),
        modifiers.logo(),
    ))
}

/// A named key outside the table: only worth spelling when a binding names
/// such a key at all.
fn learnt_named(named: Named, names: &Names) -> u32 {
    if names.is_empty() {
        return UNKNOWN;
    }
    canonical_key(&format!("{named:?}"))
        .and_then(|name| names.find(&name))
        .unwrap_or(UNKNOWN)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hangar_ui::shortcut::{KeyPress, chord_from_press, parse_chord_on};

    fn press(
        key: Key,
        code: Code,
        modifiers: Modifiers,
        text: Option<&str>,
    ) -> (Key, Physical, Modifiers, Option<String>) {
        (
            key,
            Physical::Code(code),
            modifiers,
            text.map(str::to_string),
        )
    }

    fn character(c: &str) -> Key {
        Key::Character(c.into())
    }

    /// Both ways of naming a press agree: ours, and hangar-ui's, which the
    /// recorder of the Shortcuts settings uses to write bindings down.
    fn agree(
        names: &Names,
        (key, physical, modifiers, text): (Key, Physical, Modifiers, Option<String>),
    ) {
        let ours = stroke_of_press(&key, physical, modifiers, text.as_deref(), names);
        let theirs = chord_from_press(&KeyPress {
            key: &key,
            physical,
            modifiers,
            text: text.as_deref(),
        })
        .map(|chord| Stroke::of(&chord, names));
        assert_eq!(ours, theirs, "{key:?} {physical:?} {modifiers:?} {text:?}");
    }

    fn stroke(written: &str) -> Stroke {
        Stroke::of(
            &parse_chord_on(Platform::Other, written).unwrap(),
            &Names::default(),
        )
    }

    #[test]
    fn letters_are_upper_cased_and_keep_the_printed_letter() {
        let names = Names::default();
        let (key, physical, modifiers, text) =
            press(character("a"), Code::KeyA, Modifiers::empty(), Some("a"));
        assert_eq!(
            stroke_of_press(&key, physical, modifiers, text.as_deref(), &names),
            Some(stroke("A"))
        );
        // AZERTY: the key printed "A" sits where QWERTY has Q.
        let (key, physical, modifiers, text) =
            press(character("a"), Code::KeyQ, Modifiers::CTRL, None);
        assert_eq!(
            stroke_of_press(&key, physical, modifiers, text.as_deref(), &names),
            Some(stroke("Ctrl+A"))
        );
    }

    #[test]
    fn the_digit_row_falls_back_to_its_digit_and_a_foreign_letter_to_its_code() {
        let names = Names::default();
        // AZERTY: the "1" key types "&" without Shift.
        let (key, physical, modifiers, text) =
            press(character("&"), Code::Digit1, Modifiers::CTRL, None);
        assert_eq!(
            stroke_of_press(&key, physical, modifiers, text.as_deref(), &names),
            Some(stroke("Ctrl+1"))
        );
        let (key, physical, modifiers, text) =
            press(character("ф"), Code::KeyA, Modifiers::ALT, Some("ф"));
        assert_eq!(
            stroke_of_press(&key, physical, modifiers, text.as_deref(), &names),
            Some(stroke("Alt+A"))
        );
    }

    #[test]
    fn numpad_symbols_arrows_and_space_are_named() {
        let names = Names::default();
        let cases = [
            (
                press(character("+"), Code::NumpadAdd, Modifiers::CTRL, None),
                "Ctrl+Plus",
            ),
            (
                press(
                    Key::Named(Named::Enter),
                    Code::NumpadEnter,
                    Modifiers::empty(),
                    Some("\r"),
                ),
                "Enter",
            ),
            (
                press(
                    Key::Named(Named::ArrowLeft),
                    Code::ArrowLeft,
                    Modifiers::ALT,
                    None,
                ),
                "Alt+Left",
            ),
            (
                press(character(" "), Code::Space, Modifiers::empty(), Some(" ")),
                "Space",
            ),
            (
                press(
                    Key::Named(Named::Space),
                    Code::Space,
                    Modifiers::empty(),
                    Some(" "),
                ),
                "Space",
            ),
            (
                press(Key::Named(Named::F6), Code::F6, Modifiers::SHIFT, None),
                "Shift+F6",
            ),
        ];
        for ((key, physical, modifiers, text), written) in cases {
            assert_eq!(
                stroke_of_press(&key, physical, modifiers, text.as_deref(), &names),
                Some(stroke(written)),
                "{written}"
            );
        }
    }

    #[test]
    fn a_modifier_alone_is_no_chord() {
        let names = Names::default();
        for named in [
            Named::Shift,
            Named::Control,
            Named::Alt,
            Named::AltGraph,
            Named::Super,
        ] {
            assert_eq!(
                stroke_of_press(
                    &Key::Named(named),
                    Physical::Code(Code::ShiftLeft),
                    Modifiers::SHIFT,
                    None,
                    &names
                ),
                None
            );
        }
    }

    #[test]
    fn a_character_composed_with_altgr_is_no_chord() {
        let names = Names::default();
        // Windows reports AltGr as Ctrl+Alt; on AZERTY that is how "@" is typed.
        let (key, physical, modifiers, text) = press(
            character("à"),
            Code::Digit0,
            Modifiers::CTRL | Modifiers::ALT,
            Some("@"),
        );
        assert_eq!(
            stroke_of_press(&key, physical, modifiers, text.as_deref(), &names),
            None
        );
        let (key, physical, modifiers, text) = press(
            character("0"),
            Code::Digit0,
            Modifiers::CTRL | Modifiers::ALT,
            None,
        );
        assert_eq!(
            stroke_of_press(&key, physical, modifiers, text.as_deref(), &names),
            Some(stroke("Ctrl+Alt+0"))
        );
    }

    #[test]
    fn our_names_and_hangar_uis_agree_on_every_press() {
        let mut names = Names::default();
        let mods = [
            Modifiers::empty(),
            Modifiers::CTRL,
            Modifiers::SHIFT,
            Modifiers::ALT,
            Modifiers::LOGO,
            Modifiers::CTRL | Modifiers::SHIFT,
            Modifiers::CTRL | Modifiers::ALT,
            Modifiers::CTRL | Modifiers::ALT | Modifiers::SHIFT,
        ];
        let keys = [
            (character("a"), Code::KeyA, Some("a")),
            (character("a"), Code::KeyQ, Some("a")),
            (character("&"), Code::Digit1, Some("&")),
            (character("é"), Code::Digit2, Some("é")),
            (character("ф"), Code::KeyA, Some("ф")),
            (character("="), Code::Equal, Some("=")),
            (character("+"), Code::NumpadAdd, Some("+")),
            (character(","), Code::Comma, Some(",")),
            (character(" "), Code::Space, Some(" ")),
            (character("ß"), Code::Minus, Some("ß")),
            (character("ss"), Code::Minus, Some("ss")),
            (character("^"), Code::BracketLeft, None),
            (Key::Named(Named::Enter), Code::Enter, Some("\r")),
            (Key::Named(Named::Tab), Code::Tab, Some("\t")),
            (Key::Named(Named::Escape), Code::Escape, None),
            (Key::Named(Named::PageDown), Code::PageDown, None),
            (Key::Named(Named::ArrowUp), Code::ArrowUp, None),
            (Key::Named(Named::F11), Code::F11, None),
            (Key::Named(Named::F24), Code::F24, None),
            (Key::Named(Named::PrintScreen), Code::PrintScreen, None),
            (Key::Named(Named::Control), Code::ControlLeft, None),
            (Key::Unidentified, Code::KeyA, None),
        ];
        for round in 0..2 {
            for (key, code, text) in &keys {
                for modifiers in mods {
                    agree(&names, press(key.clone(), *code, modifiers, *text));
                }
            }
            // The second time round, an index knows Print Screen.
            if round == 0 {
                names.learn("PrintScreen");
            }
        }
    }

    #[test]
    fn a_learnt_name_is_found_again_and_shown_as_written() {
        let mut names = Names::default();
        let chord = parse_chord_on(Platform::Other, "Ctrl+PrintScreen").unwrap();
        let learnt = Stroke::learn(&chord, &mut names);
        assert_eq!(Stroke::of(&chord, &names), learnt);
        assert_eq!(learnt.chord(&names), chord);
        let pressed = stroke_of_press(
            &Key::Named(Named::PrintScreen),
            Physical::Code(Code::PrintScreen),
            Modifiers::CTRL,
            None,
            &names,
        );
        assert_eq!(pressed, Some(learnt));
        // Without the name learnt, the same press matches nothing.
        let unknown = Stroke::of(&chord, &Names::default());
        assert_ne!(unknown, learnt);
    }

    #[test]
    fn strokes_know_their_class() {
        assert!(stroke("Shift+A").is_bare());
        assert!(!stroke("Alt+A").is_bare());
        assert!(stroke("F6").is_function_key());
        assert!(stroke("F24").is_function_key());
        assert!(!stroke("F25").is_function_key());
        assert!(!stroke("Home").is_function_key());
        assert!(stroke("Ctrl+C").is_text_editing(Platform::Other));
        assert!(stroke("Meta+V").is_text_editing(Platform::Mac));
        assert!(!stroke("Ctrl+K").is_text_editing(Platform::Other));
        assert!(!stroke("Ctrl+Alt+C").is_text_editing(Platform::Other));
        assert!(!stroke("C").is_text_editing(Platform::Other));
        assert!(!stroke("Ctrl+C").is_text_editing(Platform::Mac));
    }

    #[test]
    fn a_stroke_reads_back_as_its_chord() {
        let names = Names::default();
        for written in [
            "Ctrl+Shift+Enter",
            "Alt+Up",
            "Meta+,",
            "F6",
            "Ctrl+Alt+Shift+Meta+Z",
        ] {
            let chord = parse_chord_on(Platform::Other, written).unwrap();
            assert_eq!(Stroke::of(&chord, &names).chord(&names), chord, "{written}");
        }
    }
}
