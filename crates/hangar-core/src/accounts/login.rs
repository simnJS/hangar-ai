//! Who an account is logged in as, read from its own files on this machine
//! and never sent anywhere — and, on macOS, whether each login really is its
//! own.
//!
//! The tokens in `.credentials.json` are never kept: the file is read for
//! whether it holds a login and which subscription, nothing else.

use std::fs;
use std::path::Path;

use serde::de::IgnoredAny;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::ClaudeHome;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Login {
    pub logged_in: bool,
    pub email: Option<String>,
    /// Claude Code's `subscriptionType`: `pro`, `max`, `team`…
    pub plan: Option<String>,
}

#[derive(Deserialize)]
struct Config {
    #[serde(rename = "oauthAccount")]
    account: Option<OauthAccount>,
}

#[derive(Deserialize)]
struct OauthAccount {
    #[serde(rename = "emailAddress")]
    email: Option<String>,
    #[serde(rename = "accountUuid")]
    uuid: Option<IgnoredAny>,
}

#[derive(Deserialize)]
struct Credentials {
    #[serde(rename = "claudeAiOauth")]
    oauth: Option<Oauth>,
}

#[derive(Deserialize)]
struct Oauth {
    #[serde(rename = "accessToken")]
    access: Option<IgnoredAny>,
    #[serde(rename = "refreshToken")]
    refresh: Option<IgnoredAny>,
    #[serde(rename = "subscriptionType")]
    plan: Option<String>,
}

fn read<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(raw.trim_start_matches('\u{feff}')).ok()
}

/// The login of the account whose files are at `home`. `keychain` is
/// whether Claude Code keeps the tokens in the system keychain rather than in
/// `.credentials.json` (macOS): the account it logged in as, which it writes
/// to `.claude.json` at login and drops at logout, then says it.
pub fn read_login(home: &ClaudeHome, keychain: bool) -> Login {
    let account = read::<Config>(&home.config).and_then(|config| config.account);
    let email = account
        .as_ref()
        .and_then(|account| account.email.clone())
        .map(|email| email.trim().to_string())
        .filter(|email| !email.is_empty());
    if keychain {
        let logged_in = account.is_some_and(|account| account.uuid.is_some() || email.is_some());
        return Login {
            logged_in,
            email,
            plan: None,
        };
    }
    let oauth = read::<Credentials>(&home.credentials()).and_then(|file| file.oauth);
    let logged_in = oauth
        .as_ref()
        .is_some_and(|oauth| oauth.access.is_some() || oauth.refresh.is_some());
    let plan = oauth
        .and_then(|oauth| oauth.plan)
        .map(|plan| plan.trim().to_string())
        .filter(|plan| !plan.is_empty());
    Login {
        logged_in,
        email: if logged_in { email } else { None },
        plan: if logged_in { plan } else { None },
    }
}

/// The macOS Keychain entry Claude Code keeps the login of `dir` in, when it
/// is started with `CLAUDE_CONFIG_DIR=<dir>`: `Claude Code-credentials-`
/// and the first eight hex digits of the SHA-256 of the folder's path. Read
/// in Claude Code 2.1.316's bundle; the main account, started without the
/// variable, keeps the unsuffixed `Claude Code-credentials`.
pub fn keychain_service(dir: &Path) -> String {
    let digest = Sha256::digest(dir.to_string_lossy().as_bytes());
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("Claude Code-credentials-{}", &hex[..8])
}

/// Whether logins are kept apart, from the profiles that say they are logged
/// in: each must have a Keychain entry of its own. A Claude Code that keeps
/// one entry for every folder (older versions did) logs a profile in over
/// the main account instead, and leaves its own entry missing.
///
/// `probe` answers whether an entry exists, `None` when it cannot tell. A
/// folder whose path is not plain ASCII is not judged: Claude Code hashes it
/// in Unicode NFC, which is not checked here.
pub fn logins_isolated(profiles: &[ClaudeHome], probe: impl Fn(&str) -> Option<bool>) -> bool {
    !profiles.iter().any(|profile| {
        let dir = profile.dir.to_string_lossy();
        dir.is_ascii()
            && read_login(profile, true).logged_in
            && probe(&keychain_service(&profile.dir)) == Some(false)
    })
}

/// Asks the Keychain for an entry's attributes only: no secret is read, so
/// nothing is prompted. `security` exits 44 when there is no such entry.
pub fn keychain_has(service: &str) -> Option<bool> {
    let status = std::process::Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", service])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;
    match status.code() {
        Some(0) => Some(true),
        Some(44) => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::testing::Scratch;
    use std::path::PathBuf;

    fn home(scratch: &Scratch, id: &str) -> ClaudeHome {
        ClaudeHome::profile(scratch.dir(&format!("accounts/{id}")))
    }

    #[test]
    fn a_login_is_read_from_the_profiles_own_files() {
        let scratch = Scratch::new("login");
        let work = home(&scratch, "work");
        assert_eq!(read_login(&work, false), Login::default());

        scratch.file(
            "accounts/work/.claude.json",
            r#"{"oauthAccount":{"emailAddress":" work@example.com ","accountUuid":"u"},"projects":{}}"#,
        );
        // An account without tokens is not logged in, whatever it remembers.
        assert_eq!(read_login(&work, false), Login::default());

        scratch.file(
            "accounts/work/.credentials.json",
            r#"{"claudeAiOauth":{"accessToken":"secret","refreshToken":"secret","subscriptionType":"max"}}"#,
        );
        assert_eq!(
            read_login(&work, false),
            Login {
                logged_in: true,
                email: Some("work@example.com".into()),
                plan: Some("max".into()),
            }
        );

        // Logged out: Claude Code empties the file.
        scratch.file("accounts/work/.credentials.json", "{}");
        assert!(!read_login(&work, false).logged_in);
        // Broken files are no login, and no panic.
        scratch.file("accounts/work/.credentials.json", "{ broken");
        assert!(!read_login(&work, false).logged_in);
    }

    #[test]
    fn with_the_keychain_the_account_in_the_config_is_the_login() {
        let scratch = Scratch::new("login-keychain");
        let work = home(&scratch, "work");
        assert!(!read_login(&work, true).logged_in);
        scratch.file(
            "accounts/work/.claude.json",
            r#"{"oauthAccount":{"accountUuid":"u","emailAddress":"w@example.com"}}"#,
        );
        let login = read_login(&work, true);
        assert!(login.logged_in);
        assert_eq!(login.email.as_deref(), Some("w@example.com"));
        assert_eq!(login.plan, None);
    }

    #[test]
    fn the_keychain_entry_is_named_after_the_folder() {
        // sha256("/Users/me/.hangar/accounts/work") starts with these digits.
        let dir = PathBuf::from("/Users/me/.hangar/accounts/work");
        let digest = Sha256::digest(b"/Users/me/.hangar/accounts/work");
        let expected = format!(
            "Claude Code-credentials-{:02x}{:02x}{:02x}{:02x}",
            digest[0], digest[1], digest[2], digest[3]
        );
        assert_eq!(keychain_service(&dir), expected);
        assert_eq!(
            keychain_service(&dir).len(),
            "Claude Code-credentials-".len() + 8
        );
        assert_ne!(
            keychain_service(&dir),
            keychain_service(Path::new("/Users/me/.hangar/accounts/perso"))
        );
    }

    #[test]
    fn a_logged_in_profile_without_its_own_entry_means_logins_are_shared() {
        let scratch = Scratch::new("login-isolated");
        let work = home(&scratch, "work");
        let perso = home(&scratch, "perso");
        scratch.file(
            "accounts/work/.claude.json",
            r#"{"oauthAccount":{"accountUuid":"u"}}"#,
        );
        let profiles = [work.clone(), perso];
        let work_entry = keychain_service(&work.dir);

        assert!(logins_isolated(&profiles, |service| Some(
            service == work_entry
        )));
        assert!(!logins_isolated(&profiles, |_| Some(false)));
        // Unknown is not proof of anything.
        assert!(logins_isolated(&profiles, |_| None));
        // Nobody logged in yet: nothing to judge.
        assert!(logins_isolated(&profiles[1..], |_| Some(false)));
    }
}
