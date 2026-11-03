//! The Groq API key, kept in the operating system's keychain: Credential
//! Manager on Windows, the login Keychain on macOS, the Secret Service (GNOME
//! Keyring, KWallet) on Linux.
//!
//! The window can save the key, clear it and ask whether one is there, but
//! never reads it back: dictation and the cleanup pass fetch it here, at the
//! moment they need it. When no keychain answers, every call says so and the
//! key goes nowhere else. A fallback to a file would put it back exactly where
//! this module exists to take it from.

use keyring::{Entry, Error};
use serde_json::Value;

const SERVICE: &str = "hangar-ai";
const ACCOUNT: &str = "groq-api-key";

/// Where the key lived, in plain text, before it moved here.
const LEGACY_FIELD: &str = "voiceApiKey";

fn entry() -> Result<Entry, String> {
    Entry::new(SERVICE, ACCOUNT).map_err(unavailable)
}

/// The keychain's own error, plus what to do about it where that is knowable.
fn unavailable(err: Error) -> String {
    if cfg!(all(unix, not(target_os = "macos"))) {
        format!(
            "no Secret Service answered ({err}). Start GNOME Keyring, KWallet or another \
             Secret Service provider, then try again"
        )
    } else {
        format!("the system keychain refused the request: {err}")
    }
}

/// `Ok(None)` when no key was saved.
pub fn read() -> Result<Option<String>, String> {
    match entry()?.get_password() {
        Ok(key) if key.trim().is_empty() => Ok(None),
        Ok(key) => Ok(Some(key.trim().to_string())),
        Err(Error::NoEntry) => Ok(None),
        Err(err) => Err(unavailable(err)),
    }
}

/// The key, or the reason dictation through Groq cannot start.
pub fn require() -> Result<String, String> {
    read()?.ok_or_else(|| "no Groq API key — Settings → Voice".to_string())
}

fn write(key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("the key is empty".into());
    }
    entry()?.set_password(key).map_err(unavailable)
}

fn clear() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) | Err(Error::NoEntry) => Ok(()),
        Err(err) => Err(unavailable(err)),
    }
}

/// Moves a key still sitting in a loaded state.json into the keychain, and
/// returns whether `state` changed and should be written back.
///
/// The field is only dropped once the keychain has the key. When it refuses,
/// the key stays where it was, unused, rather than being lost: the settings say
/// it is there and offer to delete it, and the next launch tries again.
pub fn migrate(state: &mut Value) -> bool {
    migrate_with(state, write)
}

fn migrate_with(state: &mut Value, store: impl FnOnce(&str) -> Result<(), String>) -> bool {
    let Some(settings) = state.get_mut("settings").and_then(Value::as_object_mut) else {
        return false;
    };
    let Some(legacy) = settings.get(LEGACY_FIELD) else {
        return false;
    };
    let key = legacy.as_str().map(str::trim).unwrap_or_default();
    if !key.is_empty() {
        if let Err(err) = store(key) {
            eprintln!("[voice] the Groq key stays in state.json: {err}");
            return false;
        }
    }
    settings.remove(LEGACY_FIELD);
    true
}

/// `true` when a key is saved. An error means the keychain itself could not be
/// asked, which the settings show as such rather than as "no key".
#[tauri::command]
pub async fn voice_key_status() -> Result<bool, String> {
    crate::git::off_thread(|| read().map(|key| key.is_some())).await
}

#[tauri::command]
pub async fn voice_key_set(key: String) -> Result<(), String> {
    crate::git::off_thread(move || write(&key)).await
}

#[tauri::command]
pub async fn voice_key_clear() -> Result<(), String> {
    crate::git::off_thread(clear).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_stored_key_leaves_the_state() {
        let mut state = json!({ "settings": { "voiceApiKey": " gsk_abc ", "fontSize": 13 } });
        let mut stored = None;
        assert!(migrate_with(&mut state, |key| {
            stored = Some(key.to_string());
            Ok(())
        }));
        assert_eq!(stored.as_deref(), Some("gsk_abc"));
        assert_eq!(state, json!({ "settings": { "fontSize": 13 } }));
    }

    #[test]
    fn a_refusing_keychain_keeps_the_key_where_it_was() {
        let mut state = json!({ "settings": { "voiceApiKey": "gsk_abc" } });
        let before = state.clone();
        let changed = migrate_with(&mut state, |_| Err("no Secret Service".into()));
        assert!(!changed);
        assert_eq!(state, before);
    }

    #[test]
    fn an_empty_field_is_dropped_without_asking_the_keychain() {
        let mut state = json!({ "settings": { "voiceApiKey": "  " } });
        assert!(migrate_with(&mut state, |_| panic!("nothing to store")));
        assert_eq!(state, json!({ "settings": {} }));
    }

    /// Talks to the real keychain, under a service of its own, so it stays out
    /// of CI: the Linux runner has no Secret Service. On a desktop:
    /// `cargo test real_keychain -- --ignored`.
    #[test]
    #[ignore]
    fn round_trip_through_the_real_keychain() {
        let entry = Entry::new("hangar-ai-test", ACCOUNT).expect("keychain");
        entry.set_password("gsk_test").expect("set");
        assert_eq!(entry.get_password().expect("get"), "gsk_test");
        entry.delete_credential().expect("delete");
        assert!(matches!(entry.get_password(), Err(Error::NoEntry)));
    }

    #[test]
    fn a_state_without_the_field_is_left_alone() {
        let mut state = json!({ "settings": { "fontSize": 13 } });
        assert!(!migrate_with(&mut state, |_| panic!("nothing to store")));
        let mut empty = json!({});
        assert!(!migrate_with(&mut empty, |_| panic!("nothing to store")));
    }
}
