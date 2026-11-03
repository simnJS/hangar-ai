//! Test data: a key pair made for these tests, signatures it produced, and a
//! `latest.json` shaped like the one the release workflow publishes.
//!
//! The private keys were thrown away after signing, so nothing here can sign
//! anything new, and none of it is the key Hangar ships with. The signatures
//! are of [`payload`] (`SIG_PREHASHED`, `SIG_LEGACY`, `SIG_OTHER_KEY`) and of
//! the short strings `mac`, `deb` and `nsis`, in the minisign text format,
//! base64-encoded as they are in the feed.

/// The public key, as `plugins.updater.pubkey` holds it.
pub const PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDg3QTMxNUE3MEU5RDczMzIKUldReWM1ME9weFdqaDNESGk4WlBSaDJ5OWRNcGo5MEF0ZGxwOTJaK2h0NFdka3dFZ0JwRDNpbEkK";
/// A second key, which signed `SIG_OTHER_KEY`.
pub const OTHER_PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEIwNTYyMEU5MUE4OEVDOUUKUldTZTdJZ2E2U0JXc0x1a2ZHaFV0NXFqbnhzSnFZejZvNXRGM2FVOWJWRmp5S0lPVHNzUkg1NzcK";
/// Signature of [`payload`] by [`PUBKEY`]'s key, over its BLAKE2b hash: what
/// current minisign (and `tauri signer`) writes.
pub const SIG_PREHASHED: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVReWM1ME9weFdqaC9ncmxCa2pxL3BDWFhHMnUwZXNva1RyUDZoNjdIQ1l2Q1RuNW1GRFpKSjFUWmdZQ21ZRjFiTEVtenhzcHh2WnNzY0IvdmphOUd4cUFteGp4cU53ZmdZPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzYyMTcxMjAwCWZpbGU6aGFuZ2FyLkFwcEltYWdlClVxN2xxVzQxeFVQSE9xcmFjMDlSZDN4MTVvQzZwMmF4bGovcHRZTHVBMWVuaDAzbERXVC9lamM4ZXVTdmpLTXZlbExUSW1oNUhyZnRFNXBSZTNxb0FBPT0K";
/// The same, over the bytes themselves: the old signature format.
pub const SIG_LEGACY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUldReWM1ME9weFdqaHhSYmVzMU9YeDlmQ0NLNjRXRmRwQWtuWGtGbWMweVB2VjVBMDRQc2VoNUY0OCs3WEo5UHk0VVpKdTdxdmJld2hMZnZNcU54N3hESDFkMEkxZENXMUFnPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzYyMTcxMjAwCWZpbGU6aGFuZ2FyLkFwcEltYWdlClgwUzA2S0lSbk1QcW1aVVlKdGNPRUdhMExiODBST3QwSTBIUWoyVlFBTVYvVGcwanNtTHFuZmtkejV5V2lQcUJxWU1LaWlsQ2VMT1RvL0dnK2ZqNUJRPT0K";
/// Signature of [`payload`] by [`OTHER_PUBKEY`]'s key.
pub const SIG_OTHER_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVTZTdJZ2E2U0JXc0F2RG13b3Y5cDA1bmIxdWJmQnIxeW5aU09FUE9xNHBhcUc4NjB0S3dUd3Zyd09pSUlOOTJTMndaTHRMaVpRc09hZCtvUDM3R3IxdldWUGtEL3luVndNPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzYyMTcxMjAwCWZpbGU6aGFuZ2FyLkFwcEltYWdlCm4zSFBLa1R6dm5nT3VXN3hqQldmOWF0dlBEY2szdkZBRCtvMVY5Z0FSYkVTay9IYVBrNXNZS1FUWEQ1WS8rQndHM2ZCZ095OTlLZGRLM2VSQ1Rid0NRPT0K";
pub const SIG_MAC: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVReWM1ME9weFdqaHcxZ2FiWHlyNDNESS9tb2pZY1RrTHVpYmRiUXRhNWp6SXVkUzBKNm94dks4MlBnaUJ2SjViZlV4d1JUT0Z4RDJ1MmVjSzNwQndFanRjcFVZNFZjT2dFPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzYyMTcxMjAwCWZpbGU6SGFuZ2FyLkFJLmFwcC50YXIuZ3oKdUdPU3FlME45QUhUNlBMSlBmaDIzbTNRQWdaY092MzBldzJKQzVodFQxMHlQazFKNDNjOTQrMjlaNHJGOEF5NUtzb1RPYTRUYlFZR2xpYUQ4V2drRFE9PQo=";
pub const SIG_DEB: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVReWM1ME9weFdqaC9JUWZQbGwvb1lqSjA0WmFkV3FtK3ZCY0FIUnBoNVVVdkFEcEN4aHNEbGl3NlpCWUdaQWxadTFQRTM5UEJhQUI1QkNkdk93VldzU3hRTmdrbkswelFBPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzYyMTcxMjAwCWZpbGU6aGFuZ2FyLWFpLmRlYgpJQ3Myc0RHUmhyRDg4WmZ2ZEZXSU9ySDY0SVpubVI5MW1BZ0Vwa1JPWVVRQjNnbS9hdnBYSEZlY2N1UkJQZTdoWmhqUlVKK1ZSUVBFVXZ2d1ViNVFCZz09Cg==";
pub const SIG_NSIS: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVReWM1ME9weFdqaDBpeHNmdHMwSExJaXJpRWcxTVU0Q0crUkFQZzgvaVBRbGRMRXJGU1VPdFlXQW5vTjU4TGx5eVpQU0tKUVkzOXVtQTA2Mk1hMHh2NE9rNERuc0haTFFVPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzYyMTcxMjAwCWZpbGU6SGFuZ2FyLkFJX3NldHVwLmV4ZQoxRFZ2aHBhTEtjNSsydVEvUTR4VGJkZ3pMWnUyeFpuRnovcFRLOFlDWHVKZ2F6NWUzcWVhZHU3RGMvK2JDUCt0ZHhXejg1ZWZGK3JvR2xKWUxDcGFBZz09Cg==";

/// What the signatures above cover: 80 KiB that starts like an ELF file, big
/// enough to be read and hashed in more than one chunk.
pub fn payload() -> Vec<u8> {
    let mut bytes = b"\x7fELF".to_vec();
    for _ in 0..4096 {
        bytes.extend_from_slice(b"hangar test payload\n");
    }
    bytes
}

/// A `latest.json` as `release.yml` writes it: every key it can publish, the
/// plain Linux and Windows keys beside the per-bundle ones, and the macOS
/// bundle under both architectures. The AppImage entry is signed over
/// [`payload`], so a test can serve that and have it verify.
pub fn latest_json() -> String {
    LATEST_JSON_TEMPLATE
        .replace("@SIG_APPIMAGE@", SIG_PREHASHED)
        .replace("@SIG_MAC@", SIG_MAC)
        .replace("@SIG_DEB@", SIG_DEB)
        .replace("@SIG_NSIS@", SIG_NSIS)
}

const LATEST_JSON_TEMPLATE: &str = r#"{
  "version": "1.13.0",
  "notes": "- **Windows** — the `.exe` under **Assets**. Installs for you alone,\n  with no administrator rights.\n- **macOS** — the `.dmg`, universal for Apple Silicon and Intel.\n- **Linux** — the `.AppImage` runs without installing; the `.deb` and\n  `.rpm` are there for a package manager.",
  "pub_date": "2026-11-03T09:41:07Z",
  "platforms": {
    "darwin-aarch64": { "signature": "@SIG_MAC@", "url": "https://github.com/hangar-ai/hangar/releases/download/v1.13.0/Hangar.AI_universal.app.tar.gz" },
    "darwin-x86_64": { "signature": "@SIG_MAC@", "url": "https://github.com/hangar-ai/hangar/releases/download/v1.13.0/Hangar.AI_universal.app.tar.gz" },
    "darwin-aarch64-app": { "signature": "@SIG_MAC@", "url": "https://github.com/hangar-ai/hangar/releases/download/v1.13.0/Hangar.AI_universal.app.tar.gz" },
    "darwin-x86_64-app": { "signature": "@SIG_MAC@", "url": "https://github.com/hangar-ai/hangar/releases/download/v1.13.0/Hangar.AI_universal.app.tar.gz" },
    "linux-x86_64": { "signature": "@SIG_APPIMAGE@", "url": "https://github.com/hangar-ai/hangar/releases/download/v1.13.0/Hangar.AI_1.13.0_amd64.AppImage" },
    "linux-x86_64-appimage": { "signature": "@SIG_APPIMAGE@", "url": "https://github.com/hangar-ai/hangar/releases/download/v1.13.0/Hangar.AI_1.13.0_amd64.AppImage" },
    "linux-x86_64-deb": { "signature": "@SIG_DEB@", "url": "https://github.com/hangar-ai/hangar/releases/download/v1.13.0/Hangar.AI_1.13.0_amd64.deb" },
    "linux-x86_64-rpm": { "signature": "@SIG_DEB@", "url": "https://github.com/hangar-ai/hangar/releases/download/v1.13.0/Hangar.AI-1.13.0-1.x86_64.rpm" },
    "windows-x86_64": { "signature": "@SIG_NSIS@", "url": "https://github.com/hangar-ai/hangar/releases/download/v1.13.0/Hangar.AI_1.13.0_x64-setup.exe" },
    "windows-x86_64-nsis": { "signature": "@SIG_NSIS@", "url": "https://github.com/hangar-ai/hangar/releases/download/v1.13.0/Hangar.AI_1.13.0_x64-setup.exe" }
  }
}"#;
