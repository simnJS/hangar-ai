//! Standard base64, with padding: how a request or response body travels in a
//! JSON line.
//!
//! The protocol is the only reason this exists. A crate for it would be the
//! second dependency of the library, which is meant to carry serde and nothing
//! else, and the format is small enough to be written and tested here.

use std::fmt;

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const PAD: u8 = b'=';

/// What `decode` refused, and where.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// The text is not a whole number of four-character groups.
    Length,
    /// A character outside the alphabet, at this byte offset.
    Character(usize),
    /// Padding anywhere but the last one or two characters.
    Padding,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length => f.write_str("base64 text must be a multiple of four characters"),
            Self::Character(at) => write!(f, "not a base64 character at byte {at}"),
            Self::Padding => f.write_str("misplaced base64 padding"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// `bytes` as base64 text.
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let [a, b, c] = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let group = (u32::from(a) << 16) | (u32::from(b) << 8) | u32::from(c);
        out.push(ALPHABET[(group >> 18) as usize & 63] as char);
        out.push(ALPHABET[(group >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { ALPHABET[(group >> 6) as usize & 63] as char } else { PAD as char });
        out.push(if chunk.len() > 2 { ALPHABET[group as usize & 63] as char } else { PAD as char });
    }
    out
}

/// The value of a base64 character, or `None` for one outside the alphabet.
fn value(byte: u8) -> Option<u32> {
    match byte {
        b'A'..=b'Z' => Some(u32::from(byte - b'A')),
        b'a'..=b'z' => Some(u32::from(byte - b'a') + 26),
        b'0'..=b'9' => Some(u32::from(byte - b'0') + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// The bytes `text` stands for. Strict on purpose: no whitespace, no URL-safe
/// alphabet and no missing padding, since both ends of the protocol are ours.
pub fn decode(text: &str) -> Result<Vec<u8>, DecodeError> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return Err(DecodeError::Length);
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    // The length is a multiple of four, so there is no remainder to drop.
    let (quads, _) = bytes.as_chunks::<4>();
    for (index, group) in quads.iter().enumerate() {
        let last = index + 1 == quads.len();
        let mut sextets = [0u32; 4];
        let mut padding = 0;
        for (position, &byte) in group.iter().enumerate() {
            if byte == PAD {
                // Padding closes the text: only the last group has it, in its
                // last one or two places, and nothing follows it.
                if !last || position < 2 {
                    return Err(DecodeError::Padding);
                }
                padding += 1;
            } else if padding > 0 {
                return Err(DecodeError::Padding);
            } else {
                sextets[position] = value(byte).ok_or(DecodeError::Character(index * 4 + position))?;
            }
        }
        let word = (sextets[0] << 18) | (sextets[1] << 12) | (sextets[2] << 6) | sextets[3];
        out.push((word >> 16) as u8);
        if padding < 2 {
            out.push((word >> 8) as u8);
        }
        if padding < 1 {
            out.push(word as u8);
        }
    }
    Ok(out)
}

/// `#[serde(with = "…")]` for a `Vec<u8>` that is a base64 string in JSON.
pub mod serde_bytes {
    use std::fmt;

    use serde::de::{self, Visitor};
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&super::encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        struct Base64;

        impl Visitor<'_> for Base64 {
            type Value = Vec<u8>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a base64 string")
            }

            // Borrowed or not, the text is decoded where it stands: a body can
            // be tens of megabytes, and one copy of it is plenty.
            fn visit_str<E: de::Error>(self, text: &str) -> Result<Vec<u8>, E> {
                super::decode(text).map_err(E::custom)
            }
        }

        deserializer.deserialize_str(Base64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The vectors of RFC 4648, section 10.
    const RFC_4648: [(&str, &str); 7] = [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ];

    #[test]
    fn encodes_and_decodes_the_rfc_vectors() {
        for (plain, coded) in RFC_4648 {
            assert_eq!(encode(plain.as_bytes()), coded);
            assert_eq!(decode(coded).unwrap(), plain.as_bytes());
        }
    }

    #[test]
    fn every_byte_value_survives_a_round_trip() {
        let all: Vec<u8> = (0..=255).collect();
        for length in [0, 1, 2, 3, 4, 255, 256] {
            let slice = &all[..length.min(all.len())];
            assert_eq!(decode(&encode(slice)).unwrap(), slice);
        }
    }

    #[test]
    fn refuses_what_is_not_canonical_base64() {
        assert_eq!(decode("Zm9"), Err(DecodeError::Length));
        assert_eq!(decode("Zm9v\n"), Err(DecodeError::Length));
        assert_eq!(decode("Zm-v"), Err(DecodeError::Character(2)));
        assert_eq!(decode("Zm9 "), Err(DecodeError::Character(3)));
        assert_eq!(decode("=m9v"), Err(DecodeError::Padding));
        assert_eq!(decode("Z=9v"), Err(DecodeError::Padding));
        assert_eq!(decode("Zg==Zg=="), Err(DecodeError::Padding));
        assert_eq!(decode("Zg=A"), Err(DecodeError::Padding));
    }
}
