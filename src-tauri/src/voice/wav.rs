//! A WAV writer, and nothing more.
//!
//! Both consumers want the same thing — 16-bit PCM, mono, 16 kHz — and the
//! header for that is forty-four fixed bytes with three numbers in it. A crate
//! would bring a reader, a format matrix and a set of traits for a file this
//! module already knows the shape of.

/// Encodes samples as a mono 16-bit PCM WAV at `rate`.
///
/// Input is clamped, not wrapped: a sample above 1.0 — which a hot microphone
/// with gain applied does produce — would otherwise fold over into a loud click
/// at the opposite polarity, and a click is exactly the kind of thing an ASR
/// model transcribes as a word.
pub fn encode(samples: &[f32], rate: u32) -> Vec<u8> {
    let data_len = samples.len() * 2;
    let mut out = Vec::with_capacity(44 + data_len);

    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");

    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // chunk size, PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // format: PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // channels: mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes()); // bytes per second
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample

    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_len as u32).to_le_bytes());
    for sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        out.extend_from_slice(&((clamped * i16::MAX as f32) as i16).to_le_bytes());
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_describes_the_data() {
        let wav = encode(&[0.0; 8], 16_000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(wav.len(), 44 + 16);

        // Both length fields are relative to different points, and getting
        // either wrong gives a file that plays as noise or not at all.
        assert_eq!(u32::from_le_bytes(wav[4..8].try_into().unwrap()), 36 + 16);
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 16);
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 16_000);
    }

    #[test]
    fn samples_past_full_scale_clamp_rather_than_wrap() {
        let wav = encode(&[2.0, -2.0], 16_000);
        let first = i16::from_le_bytes(wav[44..46].try_into().unwrap());
        let second = i16::from_le_bytes(wav[46..48].try_into().unwrap());
        assert_eq!(first, i16::MAX);
        assert_eq!(second, -i16::MAX);
    }
}
