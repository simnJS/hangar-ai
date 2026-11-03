//! `latest.json`, the release feed the updater polls.
//!
//! The shape is the one `tauri-plugin-updater` reads and the release workflow
//! writes (`.github/workflows/release.yml`, job `publish`):
//!
//! ```json
//! {
//!   "version": "1.12.1",
//!   "notes": "…release body…",
//!   "pub_date": "2026-11-03T09:41:07Z",
//!   "platforms": {
//!     "linux-x86_64": { "signature": "<base64 of the .sig file>", "url": "https://…" }
//!   }
//! }
//! ```
//!
//! The plugin also accepted the "dynamic" form, where a server answers for one
//! platform with `url` and `signature` at the top level; that is kept so the
//! feed can move to such a server without a client release.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use semver::Version;
use serde::Deserialize;

use crate::error::{Error, Result};

/// One downloadable build.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Platform {
    pub url: String,
    /// The `.sig` file's content, base64-encoded (that is what the release
    /// workflow stores, and what the plugin expected).
    pub signature: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Builds {
    /// `platforms`: one entry per key.
    PerPlatform(HashMap<String, Platform>),
    /// A single build, whatever the platform asked for.
    Single(Platform),
}

/// A parsed `latest.json`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub version: Version,
    pub notes: Option<String>,
    pub pub_date: Option<DateTime<Utc>>,
    builds: Builds,
}

#[derive(Deserialize)]
struct Raw {
    // `name` is what GitHub's own release JSON calls it; the plugin accepted it.
    #[serde(alias = "name")]
    version: String,
    notes: Option<String>,
    pub_date: Option<String>,
    platforms: Option<HashMap<String, Platform>>,
    url: Option<String>,
    signature: Option<String>,
}

impl Manifest {
    pub fn parse(json: &str) -> Result<Self> {
        let raw: Raw = serde_json::from_str(json).map_err(|e| Error::Manifest(e.to_string()))?;

        let version = parse_version(&raw.version)?;

        let pub_date = match raw.pub_date {
            Some(text) => Some(
                DateTime::parse_from_rfc3339(&text)
                    .map_err(|e| Error::Manifest(format!("invalid `pub_date` {text:?}: {e}")))?
                    .with_timezone(&Utc),
            ),
            None => None,
        };

        let builds = match (raw.platforms, raw.url, raw.signature) {
            (Some(platforms), _, _) => Builds::PerPlatform(platforms),
            (None, Some(url), Some(signature)) => Builds::Single(Platform { url, signature }),
            (None, Some(_), None) => {
                return Err(Error::Manifest("`signature` is missing".into()));
            }
            (None, None, _) => {
                return Err(Error::Manifest(
                    "neither `platforms` nor `url` is present".into(),
                ));
            }
        };

        Ok(Self {
            version,
            notes: raw.notes,
            pub_date,
            builds,
        })
    }

    /// Whether this release is newer than the running version.
    ///
    /// Build metadata (`1.2.3+abc`) is not part of a version's precedence in
    /// semver, so it is left out: comparing it would offer `1.2.3+abc` to
    /// whoever runs `1.2.3`, forever. A feed that lists an older version than
    /// the running one is never an update either.
    pub fn is_newer_than(&self, current: &Version) -> bool {
        self.version.cmp_precedence(current).is_gt()
    }

    /// The build for the first of `keys` the release has, with the key that
    /// matched. The keys come from [`crate::platform::Target::keys`], most
    /// specific first.
    pub fn platform(&self, keys: &[String]) -> Result<(&str, &Platform)> {
        match &self.builds {
            Builds::PerPlatform(platforms) => keys
                .iter()
                .find_map(|key| platforms.get_key_value(key.as_str()))
                .map(|(key, platform)| (key.as_str(), platform))
                .ok_or_else(|| Error::NoPlatformEntry(keys.to_vec())),
            Builds::Single(platform) => Ok(("", platform)),
        }
    }

    /// Every key the release lists, sorted. For diagnostics and tests.
    pub fn platform_keys(&self) -> Vec<&str> {
        let mut keys: Vec<&str> = match &self.builds {
            Builds::PerPlatform(platforms) => platforms.keys().map(String::as_str).collect(),
            Builds::Single(_) => Vec::new(),
        };
        keys.sort_unstable();
        keys
    }
}

/// `v1.2.3` is accepted as `1.2.3`: release tags carry the `v`, and the
/// plugin trimmed it.
pub fn parse_version(text: &str) -> Result<Version> {
    Version::parse(text.trim().trim_start_matches('v'))
        .map_err(|e| Error::Manifest(format!("invalid version {text:?}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{SIG_DEB, latest_json};

    #[test]
    fn parses_the_feed_the_release_workflow_writes() {
        let manifest = Manifest::parse(&latest_json()).unwrap();
        assert_eq!(manifest.version, Version::new(1, 13, 0));
        assert!(manifest.notes.as_deref().unwrap().contains("**Linux**"));
        assert_eq!(
            manifest.pub_date.unwrap().to_rfc3339(),
            "2026-11-03T09:41:07+00:00"
        );
        assert_eq!(
            manifest.platform_keys(),
            [
                "darwin-aarch64",
                "darwin-aarch64-app",
                "darwin-x86_64",
                "darwin-x86_64-app",
                "linux-x86_64",
                "linux-x86_64-appimage",
                "linux-x86_64-deb",
                "linux-x86_64-rpm",
                "windows-x86_64",
                "windows-x86_64-nsis",
            ]
        );
    }

    #[test]
    fn picks_the_first_key_the_release_has() {
        let manifest = Manifest::parse(&latest_json()).unwrap();
        let keys = |list: &[&str]| list.iter().map(|k| k.to_string()).collect::<Vec<_>>();

        let (key, platform) = manifest
            .platform(&keys(&["linux-x86_64-deb", "linux-x86_64"]))
            .unwrap();
        assert_eq!(key, "linux-x86_64-deb");
        assert!(platform.url.ends_with("_amd64.deb"));
        assert_eq!(platform.signature, SIG_DEB);

        // No per-installer entry: the plain key answers.
        let (key, platform) = manifest
            .platform(&keys(&["windows-x86_64-msi", "windows-x86_64"]))
            .unwrap();
        assert_eq!(key, "windows-x86_64");
        assert!(platform.url.ends_with("-setup.exe"));
    }

    #[test]
    fn a_missing_platform_names_what_was_tried() {
        let manifest = Manifest::parse(&latest_json()).unwrap();
        let tried = vec!["linux-aarch64-deb".to_string(), "linux-aarch64".to_string()];
        match manifest.platform(&tried) {
            Err(Error::NoPlatformEntry(keys)) => assert_eq!(keys, tried),
            other => panic!("expected NoPlatformEntry, got {other:?}"),
        }
    }

    #[test]
    fn accepts_a_leading_v_and_the_name_alias() {
        let json = r#"{ "name": "v2.0.0-rc.1", "url": "https://x/y", "signature": "c2ln" }"#;
        let manifest = Manifest::parse(json).unwrap();
        assert_eq!(manifest.version, Version::parse("2.0.0-rc.1").unwrap());
        // The single-build form answers for any platform.
        let (_, platform) = manifest.platform(&["anything".to_string()]).unwrap();
        assert_eq!(platform.url, "https://x/y");
    }

    #[test]
    fn rejects_feeds_it_cannot_use() {
        for (json, needle) in [
            ("not json", "expected"),
            (
                r#"{ "version": "one", "platforms": {} }"#,
                "invalid version",
            ),
            (r#"{ "version": "1.0.0" }"#, "neither"),
            (r#"{ "version": "1.0.0", "url": "https://x" }"#, "signature"),
            (
                r#"{ "version": "1.0.0", "pub_date": "yesterday", "platforms": {} }"#,
                "pub_date",
            ),
            (
                r#"{ "version": "1.0.0", "platforms": { "linux-x86_64": { "url": "https://x" } } }"#,
                "signature",
            ),
        ] {
            match Manifest::parse(json) {
                Err(Error::Manifest(why)) => {
                    assert!(
                        why.contains(needle),
                        "{json:?} gave {why:?}, wanted {needle:?}"
                    )
                }
                other => panic!("{json:?} should be rejected, got {other:?}"),
            }
        }
    }

    #[test]
    fn newer_means_higher_precedence() {
        let release = |v: &str| {
            Manifest::parse(&format!(r#"{{ "version": "{v}", "platforms": {{}} }}"#)).unwrap()
        };
        let running = Version::new(1, 12, 1);

        assert!(release("1.12.2").is_newer_than(&running));
        assert!(release("1.13.0").is_newer_than(&running));
        assert!(release("2.0.0").is_newer_than(&running));
        assert!(
            release("1.100.0").is_newer_than(&Version::new(1, 9, 0)),
            "numbers, not text"
        );

        assert!(
            !release("1.12.1").is_newer_than(&running),
            "the same version is not an update"
        );
        assert!(
            !release("1.12.0").is_newer_than(&running),
            "never downgrade"
        );
        assert!(!release("0.99.0").is_newer_than(&running));

        // A pre-release sorts below its release, and above the one before.
        assert!(!release("1.12.1-beta.2").is_newer_than(&running));
        assert!(release("1.13.0-beta.1").is_newer_than(&running));
        assert!(release("1.12.1").is_newer_than(&Version::parse("1.12.1-beta.2").unwrap()));
        assert!(release("1.12.1-beta.2").is_newer_than(&Version::parse("1.12.1-beta.1").unwrap()));

        // Build metadata does not make a version newer.
        assert!(!release("1.12.1+abc").is_newer_than(&running));
    }

    /// 2.0.0, the native Hangar, is offered to every 1.x install through the
    /// same feed, and nothing of 1.x is ever offered back to it: a feed that
    /// still said 1.12.1, the last 1.x release, is not an update.
    #[test]
    fn the_native_release_replaces_1_x_and_is_never_downgraded() {
        let release = |v: &str| {
            Manifest::parse(&format!(r#"{{ "version": "{v}", "platforms": {{}} }}"#)).unwrap()
        };
        let native = Version::new(2, 0, 0);

        for old in ["1.0.0", "1.9.1", "1.12.0", "1.12.1"] {
            let old = Version::parse(old).unwrap();
            assert!(release("2.0.0").is_newer_than(&old), "{old} updates to 2.0.0");
        }
        for old in ["1.12.1", "1.99.0", "2.0.0-beta.1", "2.0.0"] {
            assert!(!release(old).is_newer_than(&native), "2.0.0 is not offered {old}");
        }
        assert!(release("2.0.1").is_newer_than(&native));
        assert!(release("2.1.0").is_newer_than(&native));
    }
}
