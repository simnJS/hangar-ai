//! Minisign verification of a downloaded artifact.
//!
//! The scheme is the plugin's. `tauri signer` signs the artifact with minisign
//! and leaves a `.sig` file beside it; the release workflow puts that file's
//! text, base64-encoded, in `latest.json`. The public key in the configuration
//! is the `minisign.pub` text, base64-encoded the same way. Both are decoded
//! twice over: base64 first, then the minisign text format.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use minisign_verify::{PublicKey, Signature};

use crate::error::{Error, Result};

const READ_BUFFER: usize = 64 * 1024;

/// Reads a base64-wrapped minisign text, which is what both the key and the
/// signature are stored as.
fn unwrap_base64(what: &str, wrapped: &str) -> Result<String> {
    let bytes = STANDARD
        .decode(wrapped.trim())
        .map_err(|e| Error::Signature(format!("the {what} is not valid base64: {e}")))?;
    String::from_utf8(bytes)
        .map_err(|_| Error::Signature(format!("the {what} is not minisign text")))
}

pub(crate) fn parse_public_key(pubkey: &str) -> Result<PublicKey> {
    PublicKey::decode(&unwrap_base64("public key", pubkey)?)
        .map_err(|e| Error::Signature(format!("the public key cannot be read: {e}")))
}

fn parse_signature(signature: &str) -> Result<Signature> {
    Signature::decode(&unwrap_base64("signature", signature)?)
        .map_err(|e| Error::Signature(format!("the signature cannot be read: {e}")))
}

/// Checks `file` against the release's signature.
///
/// Signatures made by current minisign hash the file first (BLAKE2b) and sign
/// the hash, which lets the file be read in chunks. An old-style signature
/// signs the bytes themselves and needs them all at once; the plugin accepted
/// those (`allow_legacy`), so the file is read whole in that case rather than
/// refusing a release the old app would have installed.
pub fn verify_file(path: &Path, pubkey: &str, signature: &str) -> Result<()> {
    let key = parse_public_key(pubkey)?;
    let signature = parse_signature(signature)?;
    let io = |e| Error::io(format!("reading {}", path.display()), e);
    let mut file = File::open(path).map_err(io)?;

    match key.verify_stream(&signature) {
        Ok(mut verifier) => {
            let mut buffer = vec![0u8; READ_BUFFER];
            loop {
                let read = file.read(&mut buffer).map_err(io)?;
                if read == 0 {
                    break;
                }
                verifier.update(&buffer[..read]);
            }
            verifier.finalize().map_err(describe)
        }
        Err(minisign_verify::Error::UnsupportedLegacyMode) => {
            file.seek(SeekFrom::Start(0)).map_err(io)?;
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).map_err(io)?;
            key.verify(&bytes, &signature, true).map_err(describe)
        }
        Err(e) => Err(describe(e)),
    }
}

/// Checks `bytes` against the release's signature. [`verify_file`] without the
/// file, for callers that already hold the artifact.
pub fn verify_bytes(bytes: &[u8], pubkey: &str, signature: &str) -> Result<()> {
    let key = parse_public_key(pubkey)?;
    let signature = parse_signature(signature)?;
    key.verify(bytes, &signature, true).map_err(describe)
}

fn describe(error: minisign_verify::Error) -> Error {
    use minisign_verify::Error as E;
    Error::Signature(match error {
        E::InvalidSignature => "the download does not match its signature".into(),
        E::UnexpectedKeyId => "the download was signed with a different key".into(),
        other => other.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{
        OTHER_PUBKEY, PUBKEY, SIG_LEGACY, SIG_OTHER_KEY, SIG_PREHASHED, payload,
    };
    use std::io::Write;

    fn file_with(bytes: &[u8]) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(bytes).unwrap();
        file
    }

    fn rejected(result: Result<()>) -> String {
        match result {
            Err(Error::Signature(why)) => why,
            other => panic!("expected a signature error, got {other:?}"),
        }
    }

    #[test]
    fn a_good_signature_verifies_in_chunks() {
        // Larger than the read buffer, so the hash really is fed in pieces.
        let data = payload();
        assert!(data.len() > READ_BUFFER);
        let file = file_with(&data);
        verify_file(file.path(), PUBKEY, SIG_PREHASHED).unwrap();
        verify_bytes(&data, PUBKEY, SIG_PREHASHED).unwrap();
    }

    #[test]
    fn a_legacy_signature_still_verifies() {
        let file = file_with(&payload());
        verify_file(file.path(), PUBKEY, SIG_LEGACY).unwrap();
        verify_bytes(&payload(), PUBKEY, SIG_LEGACY).unwrap();
    }

    #[test]
    fn a_changed_byte_is_refused() {
        let mut data = payload();
        let last = data.len() - 1;
        data[last] ^= 1;
        let file = file_with(&data);
        for signature in [SIG_PREHASHED, SIG_LEGACY] {
            let why = rejected(verify_file(file.path(), PUBKEY, signature));
            assert!(why.contains("does not match"), "{why}");
        }
        rejected(verify_bytes(&data, PUBKEY, SIG_PREHASHED));
    }

    #[test]
    fn a_truncated_download_is_refused() {
        let data = payload();
        let file = file_with(&data[..data.len() / 2]);
        rejected(verify_file(file.path(), PUBKEY, SIG_PREHASHED));
    }

    #[test]
    fn another_keys_signature_is_refused() {
        let file = file_with(&payload());
        // Signed correctly, but not by the key this app trusts.
        let why = rejected(verify_file(file.path(), PUBKEY, SIG_OTHER_KEY));
        assert!(why.contains("different key"), "{why}");
        // And the reverse: the right signature checked against the wrong key.
        rejected(verify_file(file.path(), OTHER_PUBKEY, SIG_PREHASHED));
    }

    #[test]
    fn unreadable_keys_and_signatures_are_errors_not_panics() {
        let file = file_with(&payload());
        for (key, signature) in [
            ("not base64!", SIG_PREHASHED),
            (PUBKEY, "not base64!"),
            // Valid base64, but not minisign text.
            ("aGVsbG8=", SIG_PREHASHED),
            (PUBKEY, "aGVsbG8="),
            ("", SIG_PREHASHED),
            (PUBKEY, ""),
        ] {
            rejected(verify_file(file.path(), key, signature));
        }
    }

    #[test]
    fn the_embedded_release_key_is_readable() {
        parse_public_key(crate::PUBKEY).expect("the key shipped in the binary must decode");
    }
}
