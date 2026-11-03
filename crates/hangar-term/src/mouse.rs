//! Mouse reports: what a program that asked for the mouse is told.
//!
//! A program turns the mouse on with a mode (X10 clicks, button tracking,
//! any motion) and picks an encoding for the coordinates (the original one
//! byte per number, UTF-8, or SGR, which has no limit). This module is the
//! encoding half; deciding when a report is due is the terminal's.

use alacritty_terminal::term::TermMode;
use iced::keyboard::Modifiers;

/// What happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Press,
    Release,
    Motion,
}

/// A button, as the protocol numbers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Code {
    Left,
    Middle,
    Right,
    /// Motion with no button down.
    None,
    WheelUp,
    WheelDown,
}

impl Code {
    fn number(self) -> u8 {
        match self {
            Code::Left => 0,
            Code::Middle => 1,
            Code::Right => 2,
            Code::None => 3,
            Code::WheelUp => 64,
            Code::WheelDown => 65,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Report {
    pub code: Code,
    pub kind: Kind,
    /// Zero-based cell.
    pub column: usize,
    pub row: usize,
    pub modifiers: Modifiers,
}

/// The escape sequence for a report in the encoding the terminal is set to,
/// or `None` when the cell cannot be expressed in it: the classic encoding
/// stops at column 222.
pub(crate) fn encode(mode: TermMode, report: Report) -> Option<Vec<u8>> {
    let mut code = report.code.number();
    if report.kind == Kind::Motion {
        code += 32;
    }
    if report.modifiers.shift() {
        code += 4;
    }
    if report.modifiers.alt() {
        code += 8;
    }
    if report.modifiers.control() {
        code += 16;
    }
    let (x, y) = (report.column + 1, report.row + 1);

    if mode.contains(TermMode::SGR_MOUSE) {
        let end = if report.kind == Kind::Release { 'm' } else { 'M' };
        return Some(format!("\x1b[<{code};{x};{y}{end}").into_bytes());
    }

    // The older encodings cannot say which button was released, only that one
    // was.
    if report.kind == Kind::Release && !matches!(report.code, Code::WheelUp | Code::WheelDown) {
        code = (code & !0b11) | 3;
    }

    let mut out = b"\x1b[M".to_vec();
    out.push(32 + code);
    if mode.contains(TermMode::UTF8_MOUSE) {
        for value in [x, y] {
            let c = char::from_u32((32 + value) as u32).filter(|_| value <= 2015)?;
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
    } else {
        for value in [x, y] {
            if 32 + value > 255 {
                return None;
            }
            out.push((32 + value) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(code: Code, kind: Kind, column: usize, row: usize) -> Report {
        Report { code, kind, column, row, modifiers: Modifiers::empty() }
    }

    fn text(bytes: Option<Vec<u8>>) -> String {
        String::from_utf8_lossy(&bytes.unwrap()).into_owned()
    }

    #[test]
    fn sgr_has_a_final_that_tells_press_from_release() {
        let mode = TermMode::SGR_MOUSE | TermMode::MOUSE_REPORT_CLICK;
        assert_eq!(text(encode(mode, report(Code::Left, Kind::Press, 0, 0))), "\x1b[<0;1;1M");
        assert_eq!(text(encode(mode, report(Code::Left, Kind::Release, 0, 0))), "\x1b[<0;1;1m");
        assert_eq!(text(encode(mode, report(Code::Right, Kind::Press, 199, 59))), "\x1b[<2;200;60M");
    }

    #[test]
    fn sgr_has_no_column_limit() {
        let mode = TermMode::SGR_MOUSE;
        assert_eq!(text(encode(mode, report(Code::Left, Kind::Press, 999, 0))), "\x1b[<0;1000;1M");
    }

    #[test]
    fn modifiers_are_added_to_the_button() {
        let mode = TermMode::SGR_MOUSE;
        let mut r = report(Code::Left, Kind::Press, 0, 0);
        r.modifiers = Modifiers::SHIFT | Modifiers::CTRL;
        assert_eq!(text(encode(mode, r)), "\x1b[<20;1;1M");
        r.modifiers = Modifiers::ALT;
        assert_eq!(text(encode(mode, r)), "\x1b[<8;1;1M");
    }

    #[test]
    fn motion_adds_32_and_no_button_is_3() {
        let mode = TermMode::SGR_MOUSE;
        assert_eq!(text(encode(mode, report(Code::Left, Kind::Motion, 4, 2))), "\x1b[<32;5;3M");
        assert_eq!(text(encode(mode, report(Code::None, Kind::Motion, 4, 2))), "\x1b[<35;5;3M");
    }

    #[test]
    fn wheel_is_buttons_64_and_65() {
        let mode = TermMode::SGR_MOUSE;
        assert_eq!(text(encode(mode, report(Code::WheelUp, Kind::Press, 1, 1))), "\x1b[<64;2;2M");
        assert_eq!(text(encode(mode, report(Code::WheelDown, Kind::Press, 1, 1))), "\x1b[<65;2;2M");
    }

    #[test]
    fn the_classic_encoding_is_one_byte_per_number() {
        let mode = TermMode::MOUSE_REPORT_CLICK;
        // Button 0, column 2 and row 3 are 32, 35 and 36 once offset.
        assert_eq!(
            encode(mode, report(Code::Left, Kind::Press, 2, 3)),
            Some(vec![0x1b, b'[', b'M', 32, 35, 36])
        );
        // Releases do not say which button.
        assert_eq!(
            encode(mode, report(Code::Right, Kind::Release, 2, 3)),
            Some(vec![0x1b, b'[', b'M', 35, 35, 36])
        );
    }

    #[test]
    fn the_classic_encoding_gives_up_past_column_222() {
        let mode = TermMode::MOUSE_REPORT_CLICK;
        assert!(encode(mode, report(Code::Left, Kind::Press, 222, 0)).is_some());
        assert!(encode(mode, report(Code::Left, Kind::Press, 223, 0)).is_none());
    }

    #[test]
    fn utf8_encoding_reaches_further() {
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::UTF8_MOUSE;
        let bytes = encode(mode, report(Code::Left, Kind::Press, 299, 0)).unwrap();
        assert_eq!(&bytes[..4], b"\x1b[M ");
        assert_eq!(String::from_utf8(bytes[4..].to_vec()).unwrap(), "\u{14c}!");
    }
}
