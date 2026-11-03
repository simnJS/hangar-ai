//! Keyboard shortcuts a tab subscribes to: `mod+s`, `mod+shift+z`,
//! `escape`. Written as text on the wire, as an HTML page names a key, so
//! that a kit and a Hangar of different ages read them the same way.
//!
//! `mod` is the platform's command key, Ctrl on Linux and Windows and Cmd on
//! macOS, what a kit means by "Ctrl+S" nine times out of ten. `ctrl`, `alt`
//! and `shift` are those keys themselves. The key is one character (`s`,
//! `+`, `0`), case not counting, or a name: `enter`, `escape`, `tab`,
//! `space`, `backspace`, `delete`, `up`, `down`, `left`, `right`, `pageup`,
//! `pagedown`, `home`, `end`, `f1` to `f12`.

use std::fmt;

/// A key of a shortcut, as [`Shortcut::parse`] reads it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    /// A character, lowercase.
    Char(char),
    Named(NamedKey),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NamedKey {
    Enter,
    Escape,
    Tab,
    Space,
    Backspace,
    Delete,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
    /// F1 to F12.
    F(u8),
}

const NAMES: &[(&str, NamedKey)] = &[
    ("enter", NamedKey::Enter),
    ("escape", NamedKey::Escape),
    ("tab", NamedKey::Tab),
    ("space", NamedKey::Space),
    ("backspace", NamedKey::Backspace),
    ("delete", NamedKey::Delete),
    ("up", NamedKey::Up),
    ("down", NamedKey::Down),
    ("left", NamedKey::Left),
    ("right", NamedKey::Right),
    ("pageup", NamedKey::PageUp),
    ("pagedown", NamedKey::PageDown),
    ("home", NamedKey::Home),
    ("end", NamedKey::End),
];

/// A shortcut, read.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Shortcut {
    pub key: Key,
    /// The platform's command key: Ctrl, or Cmd on macOS.
    pub command: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Shortcut {
    /// Reads `mod+shift+z`; none for what is not a shortcut. The parts are
    /// joined by `+`, the modifiers first, in any order and any case; the
    /// key `+` itself is written last (`mod++`).
    pub fn parse(spec: &str) -> Option<Shortcut> {
        let spec = spec.trim();
        let (mods, key) = match spec.strip_suffix("++") {
            _ if spec == "+" => ("", "+"),
            Some(mods) => (mods, "+"),
            None => match spec.rsplit_once('+') {
                Some((mods, key)) => (mods, key),
                None => ("", spec),
            },
        };
        let mut shortcut = Shortcut {
            key: read_key(key)?,
            command: false,
            ctrl: false,
            alt: false,
            shift: false,
        };
        for part in mods.split('+').filter(|part| !part.is_empty()) {
            let held = match part.to_ascii_lowercase().as_str() {
                "mod" | "cmd" | "command" => &mut shortcut.command,
                "ctrl" | "control" => &mut shortcut.ctrl,
                "alt" | "option" => &mut shortcut.alt,
                "shift" => &mut shortcut.shift,
                _ => return None,
            };
            if *held {
                return None;
            }
            *held = true;
        }
        Some(shortcut)
    }
}

fn read_key(key: &str) -> Option<Key> {
    let lower = key.to_lowercase();
    let mut chars = lower.chars();
    if let (Some(only), None) = (chars.next(), chars.next())
        && !only.is_whitespace()
    {
        return Some(Key::Char(only));
    }
    if let Some((_, named)) = NAMES.iter().find(|(name, _)| *name == lower) {
        return Some(Key::Named(*named));
    }
    let number: u8 = lower.strip_prefix('f')?.parse().ok()?;
    (1..=12)
        .contains(&number)
        .then_some(Key::Named(NamedKey::F(number)))
}

impl fmt::Display for Shortcut {
    /// The shortcut as [`Shortcut::parse`] reads it, its modifiers in one
    /// order: two specs of the same shortcut write the same.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (held, name) in [
            (self.command, "mod"),
            (self.ctrl, "ctrl"),
            (self.alt, "alt"),
            (self.shift, "shift"),
        ] {
            if held {
                write!(f, "{name}+")?;
            }
        }
        match &self.key {
            Key::Char(c) => write!(f, "{c}"),
            Key::Named(NamedKey::F(number)) => write!(f, "f{number}"),
            Key::Named(named) => {
                let name = NAMES
                    .iter()
                    .find(|(_, key)| key == named)
                    .map(|(name, _)| *name)
                    .unwrap_or_default();
                f.write_str(name)
            }
        }
    }
}
