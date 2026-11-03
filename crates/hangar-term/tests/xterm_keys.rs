//! The keys an agent reads, against what xterm.js 6.0 sent for them.
//!
//! The old Hangar terminal was xterm.js 6.0.0, which has no kitty keyboard
//! protocol: every program, Claude Code and the other agent CLIs included,
//! was typed to in the classic encoding below. The table is xterm.js's own
//! `evaluateKeyboardEvent` (`src/common/input/Keyboard.ts`) run over every
//! combination of Shift, Ctrl and Alt for the named keys and the letters, on
//! a keyboard without a Meta key and with `macOptionIsMeta` on, as the old
//! terminal set it up. Each line is `key|modifiers|cursor keys|bytes in hex`
//! (modifiers: S Shift, C Ctrl, A Alt; cursor keys: `norm`al or `app`lication
//! mode).
//!
//! These are the combinations where the two agree. The ones that do not are
//! not typed by anything an agent reads, and are listed in `known_differences`
//! with the reason, so a change of the encoder has to look at both.

use hangar_term::{KeyEventKind, KeyOptions, KeyPress, TermMode, encode_key};
use iced::keyboard::key::{Named, NativeCode, Physical};
use iced::keyboard::{Key, Location, Modifiers};

const XTERM_JS: &str = "\
Enter||norm|0d\n\
Enter|S|norm|0d\n\
Enter|C|norm|0d\n\
Enter|A|norm|1b0d\n\
Enter|CS|norm|0d\n\
Enter|CA|norm|1b0d\n\
Enter|AS|norm|1b0d\n\
Enter|CAS|norm|1b0d\n\
Tab||norm|09\n\
Tab|S|norm|1b5b5a\n\
Tab|C|norm|09\n\
Tab|CS|norm|1b5b5a\n\
Backspace||norm|7f\n\
Backspace|S|norm|7f\n\
Backspace|C|norm|08\n\
Backspace|A|norm|1b7f\n\
Backspace|CS|norm|08\n\
Backspace|CA|norm|1b08\n\
Backspace|AS|norm|1b7f\n\
Backspace|CAS|norm|1b08\n\
Escape||norm|1b\n\
Escape|S|norm|1b\n\
Escape|C|norm|1b\n\
Escape|A|norm|1b1b\n\
Escape|CS|norm|1b\n\
Escape|CA|norm|1b1b\n\
Escape|AS|norm|1b1b\n\
Escape|CAS|norm|1b1b\n\
ArrowLeft||norm|1b5b44\n\
ArrowLeft||app|1b4f44\n\
ArrowLeft|S|norm|1b5b313b3244\n\
ArrowLeft|S|app|1b5b313b3244\n\
ArrowLeft|C|norm|1b5b313b3544\n\
ArrowLeft|C|app|1b5b313b3544\n\
ArrowLeft|A|norm|1b5b313b3344\n\
ArrowLeft|A|app|1b5b313b3344\n\
ArrowLeft|CS|norm|1b5b313b3644\n\
ArrowLeft|CS|app|1b5b313b3644\n\
ArrowLeft|CA|norm|1b5b313b3744\n\
ArrowLeft|CA|app|1b5b313b3744\n\
ArrowLeft|AS|norm|1b5b313b3444\n\
ArrowLeft|AS|app|1b5b313b3444\n\
ArrowLeft|CAS|norm|1b5b313b3844\n\
ArrowLeft|CAS|app|1b5b313b3844\n\
ArrowUp||norm|1b5b41\n\
ArrowUp||app|1b4f41\n\
ArrowUp|S|norm|1b5b313b3241\n\
ArrowUp|S|app|1b5b313b3241\n\
ArrowUp|C|norm|1b5b313b3541\n\
ArrowUp|C|app|1b5b313b3541\n\
ArrowUp|A|norm|1b5b313b3341\n\
ArrowUp|A|app|1b5b313b3341\n\
ArrowUp|CS|norm|1b5b313b3641\n\
ArrowUp|CS|app|1b5b313b3641\n\
ArrowUp|CA|norm|1b5b313b3741\n\
ArrowUp|CA|app|1b5b313b3741\n\
ArrowUp|AS|norm|1b5b313b3441\n\
ArrowUp|AS|app|1b5b313b3441\n\
ArrowUp|CAS|norm|1b5b313b3841\n\
ArrowUp|CAS|app|1b5b313b3841\n\
ArrowRight||norm|1b5b43\n\
ArrowRight||app|1b4f43\n\
ArrowRight|S|norm|1b5b313b3243\n\
ArrowRight|S|app|1b5b313b3243\n\
ArrowRight|C|norm|1b5b313b3543\n\
ArrowRight|C|app|1b5b313b3543\n\
ArrowRight|A|norm|1b5b313b3343\n\
ArrowRight|A|app|1b5b313b3343\n\
ArrowRight|CS|norm|1b5b313b3643\n\
ArrowRight|CS|app|1b5b313b3643\n\
ArrowRight|CA|norm|1b5b313b3743\n\
ArrowRight|CA|app|1b5b313b3743\n\
ArrowRight|AS|norm|1b5b313b3443\n\
ArrowRight|AS|app|1b5b313b3443\n\
ArrowRight|CAS|norm|1b5b313b3843\n\
ArrowRight|CAS|app|1b5b313b3843\n\
ArrowDown||norm|1b5b42\n\
ArrowDown||app|1b4f42\n\
ArrowDown|S|norm|1b5b313b3242\n\
ArrowDown|S|app|1b5b313b3242\n\
ArrowDown|C|norm|1b5b313b3542\n\
ArrowDown|C|app|1b5b313b3542\n\
ArrowDown|A|norm|1b5b313b3342\n\
ArrowDown|A|app|1b5b313b3342\n\
ArrowDown|CS|norm|1b5b313b3642\n\
ArrowDown|CS|app|1b5b313b3642\n\
ArrowDown|CA|norm|1b5b313b3742\n\
ArrowDown|CA|app|1b5b313b3742\n\
ArrowDown|AS|norm|1b5b313b3442\n\
ArrowDown|AS|app|1b5b313b3442\n\
ArrowDown|CAS|norm|1b5b313b3842\n\
ArrowDown|CAS|app|1b5b313b3842\n\
Home||norm|1b5b48\n\
Home||app|1b4f48\n\
Home|S|norm|1b5b313b3248\n\
Home|S|app|1b5b313b3248\n\
Home|C|norm|1b5b313b3548\n\
Home|C|app|1b5b313b3548\n\
Home|A|norm|1b5b313b3348\n\
Home|A|app|1b5b313b3348\n\
Home|CS|norm|1b5b313b3648\n\
Home|CS|app|1b5b313b3648\n\
Home|CA|norm|1b5b313b3748\n\
Home|CA|app|1b5b313b3748\n\
Home|AS|norm|1b5b313b3448\n\
Home|AS|app|1b5b313b3448\n\
Home|CAS|norm|1b5b313b3848\n\
Home|CAS|app|1b5b313b3848\n\
End||norm|1b5b46\n\
End||app|1b4f46\n\
End|S|norm|1b5b313b3246\n\
End|S|app|1b5b313b3246\n\
End|C|norm|1b5b313b3546\n\
End|C|app|1b5b313b3546\n\
End|A|norm|1b5b313b3346\n\
End|A|app|1b5b313b3346\n\
End|CS|norm|1b5b313b3646\n\
End|CS|app|1b5b313b3646\n\
End|CA|norm|1b5b313b3746\n\
End|CA|app|1b5b313b3746\n\
End|AS|norm|1b5b313b3446\n\
End|AS|app|1b5b313b3446\n\
End|CAS|norm|1b5b313b3846\n\
End|CAS|app|1b5b313b3846\n\
PageUp||norm|1b5b357e\n\
PageUp|C|norm|1b5b353b357e\n\
PageUp|CA|norm|1b5b353b377e\n\
PageDown||norm|1b5b367e\n\
PageDown|C|norm|1b5b363b357e\n\
PageDown|CA|norm|1b5b363b377e\n\
Insert||norm|1b5b327e\n\
Delete||norm|1b5b337e\n\
Delete|S|norm|1b5b333b327e\n\
Delete|C|norm|1b5b333b357e\n\
Delete|A|norm|1b5b333b337e\n\
Delete|CS|norm|1b5b333b367e\n\
Delete|CA|norm|1b5b333b377e\n\
Delete|AS|norm|1b5b333b347e\n\
Delete|CAS|norm|1b5b333b387e\n\
F1||norm|1b4f50\n\
F1|S|norm|1b5b313b3250\n\
F1|C|norm|1b5b313b3550\n\
F1|A|norm|1b5b313b3350\n\
F1|CS|norm|1b5b313b3650\n\
F1|CA|norm|1b5b313b3750\n\
F1|AS|norm|1b5b313b3450\n\
F1|CAS|norm|1b5b313b3850\n\
F2||norm|1b4f51\n\
F2|S|norm|1b5b313b3251\n\
F2|C|norm|1b5b313b3551\n\
F2|A|norm|1b5b313b3351\n\
F2|CS|norm|1b5b313b3651\n\
F2|CA|norm|1b5b313b3751\n\
F2|AS|norm|1b5b313b3451\n\
F2|CAS|norm|1b5b313b3851\n\
F3||norm|1b4f52\n\
F3|S|norm|1b5b313b3252\n\
F3|C|norm|1b5b313b3552\n\
F3|A|norm|1b5b313b3352\n\
F3|CS|norm|1b5b313b3652\n\
F3|CA|norm|1b5b313b3752\n\
F3|AS|norm|1b5b313b3452\n\
F3|CAS|norm|1b5b313b3852\n\
F4||norm|1b4f53\n\
F4|S|norm|1b5b313b3253\n\
F4|C|norm|1b5b313b3553\n\
F4|A|norm|1b5b313b3353\n\
F4|CS|norm|1b5b313b3653\n\
F4|CA|norm|1b5b313b3753\n\
F4|AS|norm|1b5b313b3453\n\
F4|CAS|norm|1b5b313b3853\n\
F5||norm|1b5b31357e\n\
F5|S|norm|1b5b31353b327e\n\
F5|C|norm|1b5b31353b357e\n\
F5|A|norm|1b5b31353b337e\n\
F5|CS|norm|1b5b31353b367e\n\
F5|CA|norm|1b5b31353b377e\n\
F5|AS|norm|1b5b31353b347e\n\
F5|CAS|norm|1b5b31353b387e\n\
F6||norm|1b5b31377e\n\
F6|S|norm|1b5b31373b327e\n\
F6|C|norm|1b5b31373b357e\n\
F6|A|norm|1b5b31373b337e\n\
F6|CS|norm|1b5b31373b367e\n\
F6|CA|norm|1b5b31373b377e\n\
F6|AS|norm|1b5b31373b347e\n\
F6|CAS|norm|1b5b31373b387e\n\
F7||norm|1b5b31387e\n\
F7|S|norm|1b5b31383b327e\n\
F7|C|norm|1b5b31383b357e\n\
F7|A|norm|1b5b31383b337e\n\
F7|CS|norm|1b5b31383b367e\n\
F7|CA|norm|1b5b31383b377e\n\
F7|AS|norm|1b5b31383b347e\n\
F7|CAS|norm|1b5b31383b387e\n\
F8||norm|1b5b31397e\n\
F8|S|norm|1b5b31393b327e\n\
F8|C|norm|1b5b31393b357e\n\
F8|A|norm|1b5b31393b337e\n\
F8|CS|norm|1b5b31393b367e\n\
F8|CA|norm|1b5b31393b377e\n\
F8|AS|norm|1b5b31393b347e\n\
F8|CAS|norm|1b5b31393b387e\n\
F9||norm|1b5b32307e\n\
F9|S|norm|1b5b32303b327e\n\
F9|C|norm|1b5b32303b357e\n\
F9|A|norm|1b5b32303b337e\n\
F9|CS|norm|1b5b32303b367e\n\
F9|CA|norm|1b5b32303b377e\n\
F9|AS|norm|1b5b32303b347e\n\
F9|CAS|norm|1b5b32303b387e\n\
F10||norm|1b5b32317e\n\
F10|S|norm|1b5b32313b327e\n\
F10|C|norm|1b5b32313b357e\n\
F10|A|norm|1b5b32313b337e\n\
F10|CS|norm|1b5b32313b367e\n\
F10|CA|norm|1b5b32313b377e\n\
F10|AS|norm|1b5b32313b347e\n\
F10|CAS|norm|1b5b32313b387e\n\
F11||norm|1b5b32337e\n\
F11|S|norm|1b5b32333b327e\n\
F11|C|norm|1b5b32333b357e\n\
F11|A|norm|1b5b32333b337e\n\
F11|CS|norm|1b5b32333b367e\n\
F11|CA|norm|1b5b32333b377e\n\
F11|AS|norm|1b5b32333b347e\n\
F11|CAS|norm|1b5b32333b387e\n\
F12||norm|1b5b32347e\n\
F12|S|norm|1b5b32343b327e\n\
F12|C|norm|1b5b32343b357e\n\
F12|A|norm|1b5b32343b337e\n\
F12|CS|norm|1b5b32343b367e\n\
F12|CA|norm|1b5b32343b377e\n\
F12|AS|norm|1b5b32343b347e\n\
F12|CAS|norm|1b5b32343b387e\n\
a|C|norm|01\n\
a|A|norm|1b61\n\
b|C|norm|02\n\
b|A|norm|1b62\n\
c|C|norm|03\n\
c|A|norm|1b63\n\
d|C|norm|04\n\
d|A|norm|1b64\n\
e|C|norm|05\n\
e|A|norm|1b65\n\
f|C|norm|06\n\
f|A|norm|1b66\n\
g|C|norm|07\n\
g|A|norm|1b67\n\
h|C|norm|08\n\
h|A|norm|1b68\n\
i|C|norm|09\n\
i|A|norm|1b69\n\
j|C|norm|0a\n\
j|A|norm|1b6a\n\
k|C|norm|0b\n\
k|A|norm|1b6b\n\
l|C|norm|0c\n\
l|A|norm|1b6c\n\
m|C|norm|0d\n\
m|A|norm|1b6d\n\
n|C|norm|0e\n\
n|A|norm|1b6e\n\
o|C|norm|0f\n\
o|A|norm|1b6f\n\
p|C|norm|10\n\
p|A|norm|1b70\n\
q|C|norm|11\n\
q|A|norm|1b71\n\
r|C|norm|12\n\
r|A|norm|1b72\n\
s|C|norm|13\n\
s|A|norm|1b73\n\
t|C|norm|14\n\
t|A|norm|1b74\n\
u|C|norm|15\n\
u|A|norm|1b75\n\
v|C|norm|16\n\
v|A|norm|1b76\n\
w|C|norm|17\n\
w|A|norm|1b77\n\
x|C|norm|18\n\
x|A|norm|1b78\n\
y|C|norm|19\n\
y|A|norm|1b79\n\
z|C|norm|1a\n\
z|A|norm|1b7a";

fn named(name: &str) -> Option<Named> {
    Some(match name {
        "Enter" => Named::Enter,
        "Tab" => Named::Tab,
        "Backspace" => Named::Backspace,
        "Escape" => Named::Escape,
        "ArrowLeft" => Named::ArrowLeft,
        "ArrowUp" => Named::ArrowUp,
        "ArrowRight" => Named::ArrowRight,
        "ArrowDown" => Named::ArrowDown,
        "Home" => Named::Home,
        "End" => Named::End,
        "PageUp" => Named::PageUp,
        "PageDown" => Named::PageDown,
        "Insert" => Named::Insert,
        "Delete" => Named::Delete,
        "F1" => Named::F1,
        "F2" => Named::F2,
        "F3" => Named::F3,
        "F4" => Named::F4,
        "F5" => Named::F5,
        "F6" => Named::F6,
        "F7" => Named::F7,
        "F8" => Named::F8,
        "F9" => Named::F9,
        "F10" => Named::F10,
        "F11" => Named::F11,
        "F12" => Named::F12,
        _ => return None,
    })
}

/// The press of `name` with `modifiers`, as a platform reports it: the text
/// it composed is the character, or the control code under Ctrl.
fn press(name: &str, modifiers: &str) -> KeyPress {
    let mut mods = Modifiers::empty();
    for (letter, modifier) in [
        ('C', Modifiers::CTRL),
        ('A', Modifiers::ALT),
        ('S', Modifiers::SHIFT),
    ] {
        if modifiers.contains(letter) {
            mods |= modifier;
        }
    }
    let (key, text) = match named(name) {
        Some(Named::Enter) => (Key::Named(Named::Enter), Some("\r".to_string())),
        Some(Named::Tab) => (Key::Named(Named::Tab), Some("\t".to_string())),
        Some(named) => (Key::Named(named), None),
        None => {
            let c = name.chars().next().expect("a key");
            let shown = if mods.shift() {
                c.to_ascii_uppercase()
            } else {
                c
            };
            let text = if mods.control() && c.is_ascii_lowercase() {
                ((c as u8 - b'a' + 1) as char).to_string()
            } else {
                shown.to_string()
            };
            (Key::Character(c.to_string().into()), Some(text))
        }
    };
    KeyPress {
        modified_key: key.clone(),
        key,
        physical_key: Physical::Unidentified(NativeCode::Unidentified),
        location: Location::Standard,
        modifiers: mods,
        text,
        kind: KeyEventKind::Press,
    }
}

fn hex(bytes: Option<Vec<u8>>) -> String {
    bytes
        .map(|bytes| bytes.iter().map(|b| format!("{b:02x}")).collect())
        .unwrap_or_else(|| "none".into())
}

#[test]
fn the_classic_encoding_is_what_xterm_js_sent() {
    let mut wrong = Vec::new();
    let mut checked = 0;
    for line in XTERM_JS.lines() {
        let [name, modifiers, mode, expected] = line.split('|').collect::<Vec<_>>()[..] else {
            panic!("{line}");
        };
        let mode = if mode == "app" {
            TermMode::APP_CURSOR
        } else {
            TermMode::empty()
        };
        let got = hex(encode_key(
            &press(name, modifiers),
            mode,
            &KeyOptions::default(),
        ));
        checked += 1;
        if got != expected {
            wrong.push(format!("{line}: got {got}"));
        }
    }
    assert!(checked > 200, "{checked}");
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn known_differences() {
    let encode = |name: &str, modifiers: &str| {
        hex(encode_key(
            &press(name, modifiers),
            TermMode::empty(),
            &KeyOptions::default(),
        ))
    };
    // Alt+Tab belongs to the window manager, and xterm.js left Alt out of it.
    assert_eq!(encode("Tab", "A"), "1b09");
    // Alt with Page Up, Page Down or Insert: xterm.js dropped the modifier,
    // which makes them indistinguishable from the plain keys; xterm proper
    // and the kitty protocol keep it, as this does. No agent binds them.
    assert_eq!(encode("PageUp", "A"), "1b5b353b337e");
    assert_eq!(encode("Insert", "A"), "1b5b323b337e");
    // Ctrl+Shift+letter sent nothing from xterm.js (the browser makes no
    // character of it); here it is the control code, as in xterm proper.
    // The app's own Ctrl+Shift shortcuts never get this far (`key_filter`).
    assert_eq!(encode("c", "CS"), "03");
}
