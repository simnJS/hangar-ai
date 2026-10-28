//! Pairing a phone. The person starts it from Hangar's settings, which show a
//! six-digit code and a QR code of `https://<pc>.<tailnet>.ts.net/?code=…`;
//! the phone opens that address, posts the code to /m/api/pair (server.rs)
//! and gets its cookie in return.
//!
//! One code at a time, good for five minutes and for one phone. Five wrong
//! guesses kill it: a million codes against five tries leaves anyone without
//! the QR code one chance in two hundred thousand, and only while the person
//! is pairing.

use subtle::ConstantTimeEq;

pub const LIFETIME_MS: u64 = 5 * 60 * 1000;
pub const MAX_WRONG: u32 = 5;

/// What a phone is called when it gives no name of its own.
const UNNAMED: &str = "Phone";
const MAX_NAME_CHARS: usize = 60;

pub struct Pairing {
    pub(super) code: String,
    pub(super) expires_at: u64,
    pub(super) wrong: u32,
}

impl Pairing {
    pub fn new(now: u64) -> Self {
        Self {
            code: new_code(),
            expires_at: now + LIFETIME_MS,
            wrong: 0,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum PairError {
    /// No code to pair with: none started, used, out of time or guessed at
    /// too often. Only a new one from the settings helps.
    Expired,
    /// Not the code; `left` more tries before it dies.
    Wrong { left: u32 },
    /// The code was right but the phone could not be saved.
    Storage(String),
}

/// Spends `slot`'s code on `code`: emptied when it matches, when it has run
/// out of time, and on the last wrong guess.
pub fn redeem(slot: &mut Option<Pairing>, code: &str, now: u64) -> Result<(), PairError> {
    let Some(pairing) = slot.as_mut() else {
        return Err(PairError::Expired);
    };
    if now >= pairing.expires_at || pairing.wrong >= MAX_WRONG {
        *slot = None;
        return Err(PairError::Expired);
    }
    if bool::from(code.trim().as_bytes().ct_eq(pairing.code.as_bytes())) {
        *slot = None;
        return Ok(());
    }
    pairing.wrong += 1;
    let left = MAX_WRONG - pairing.wrong;
    if left == 0 {
        *slot = None;
    }
    Err(PairError::Wrong { left })
}

/// Six digits from six bytes of a v4 UUID — all of them random, drawn from the
/// OS — folded onto a million values: 2^48 of them, so no code comes up
/// measurably more often than another.
fn new_code() -> String {
    let bytes = uuid::Uuid::new_v4().into_bytes();
    let random = bytes[10..]
        .iter()
        .fold(0u64, |number, byte| number << 8 | u64::from(*byte));
    format!("{:06}", random % 1_000_000)
}

/// The name the phone gave itself, as the settings will list it.
pub fn device_name(given: &str) -> String {
    let name: String = given
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME_CHARS)
        .collect();
    let name = name.trim();
    if name.is_empty() {
        UNNAMED.to_string()
    } else {
        name.to_string()
    }
}

/// `text` as a QR code, in SVG the settings can show as is.
pub fn qr_svg(text: &str) -> Result<String, String> {
    let code = qrcode::QrCode::new(text.as_bytes())
        .map_err(|err| format!("could not draw the QR code: {err}"))?;
    Ok(code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(240, 240)
        .quiet_zone(true)
        .build())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairing(code: &str) -> Option<Pairing> {
        Some(Pairing {
            code: code.into(),
            expires_at: 10_000,
            wrong: 0,
        })
    }

    #[test]
    fn a_code_is_six_digits_and_lasts_five_minutes() {
        for _ in 0..200 {
            let pairing = Pairing::new(1_000);
            assert_eq!(pairing.code.len(), 6, "{}", pairing.code);
            assert!(pairing.code.chars().all(|c| c.is_ascii_digit()));
            assert_eq!(pairing.expires_at, 1_000 + 300_000);
        }
        let codes: std::collections::HashSet<_> = (0..50).map(|_| Pairing::new(0).code).collect();
        assert!(codes.len() > 40, "codes repeat: {codes:?}");
    }

    #[test]
    fn the_right_code_pairs_once() {
        let mut slot = pairing("123456");
        assert_eq!(redeem(&mut slot, "123456", 5_000), Ok(()));
        assert!(slot.is_none());
        assert_eq!(redeem(&mut slot, "123456", 5_001), Err(PairError::Expired));
    }

    #[test]
    fn a_code_out_of_time_is_refused_even_when_right() {
        let mut slot = pairing("123456");
        assert_eq!(redeem(&mut slot, "123456", 10_000), Err(PairError::Expired));
        assert!(slot.is_none());
    }

    #[test]
    fn five_wrong_tries_kill_the_code() {
        let mut slot = pairing("123456");
        for left in (0..5).rev() {
            assert_eq!(
                redeem(&mut slot, "654321", 5_000),
                Err(PairError::Wrong { left })
            );
        }
        assert!(slot.is_none());
        assert_eq!(redeem(&mut slot, "123456", 5_000), Err(PairError::Expired));
    }

    #[test]
    fn a_right_code_after_wrong_ones_still_pairs() {
        let mut slot = pairing("123456");
        assert!(redeem(&mut slot, "", 5_000).is_err());
        assert!(redeem(&mut slot, "12345", 5_000).is_err());
        assert!(redeem(&mut slot, "1234567", 5_000).is_err());
        assert_eq!(redeem(&mut slot, " 123456 ", 5_000), Ok(()));
    }

    #[test]
    fn without_a_pairing_nothing_pairs() {
        assert_eq!(redeem(&mut None, "000000", 0), Err(PairError::Expired));
    }

    #[test]
    fn a_phone_s_name_is_cleaned_and_capped() {
        assert_eq!(device_name("  iPhone de Simon \n"), "iPhone de Simon");
        assert_eq!(device_name("a\u{0}b\u{7}c"), "abc");
        assert_eq!(device_name("   "), "Phone");
        assert_eq!(device_name(&"é".repeat(100)).chars().count(), 60);
    }

    #[test]
    fn the_qr_code_is_an_svg() {
        let svg = qr_svg("https://pc.tailnet.ts.net/?code=123456").unwrap();
        assert!(svg.contains("<svg"), "{svg}");
        assert!(svg.trim_end().ends_with("</svg>"));
    }
}
