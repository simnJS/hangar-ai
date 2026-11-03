//! Push notifications to the paired phones, sent by the PC itself.
//!
//! Web Push as browsers take it: the phone subscribes with its browser's push
//! service (Apple's, Google's, Mozilla's…) and hands the PC the address it
//! got; the PC posts a message there, which the service carries to the phone,
//! app closed or not. No server of Hangar's is involved:
//!
//! - the message is encrypted for the phone alone (RFC 8291, aes128gcm): the
//!   push service carries it without being able to read it;
//! - each request is signed with the PC's own VAPID key (RFC 8292), a P-256
//!   pair generated once and kept in `<app data>/remote/vapid.json`. A
//!   subscription is bound to the key it was made with, so the service
//!   refuses anyone else's messages to it.
//!
//! The encryption is web-push-native's, the signature p256's ECDSA, both on
//! the RustCrypto crates; this module keeps the keys and the subscriptions,
//! writes the VAPID token around the signature, and sends.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// The `http` crate axum and ureq are both built on, at the version
// web-push-native builds its requests with.
use axum::http;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use p256::PublicKey;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use web_push_native::{Auth, WebPushBuilder};

/// In `<app data>/remote/`, beside the shares.
pub const KEY_FILE: &str = "vapid.json";

/// Who sends, as RFC 8292 asks of every signature: the project's site, where
/// a push service that has something to say about these messages finds who
/// makes Hangar.
const CONTACT: &str = "https://hangar.simnjs.fr";

/// How long a signature holds (RFC 8292 allows a day at most), and how long
/// a push service keeps a message for a phone that is off.
const VALID_FOR: Duration = Duration::from_secs(12 * 60 * 60);

/// A push service that has not answered by then is not waited on.
const SEND_TIMEOUT: Duration = Duration::from_secs(15);

/// A notification is a few words: these keep a runaway text from reaching
/// the 4 KB a push service carries.
const TITLE_LIMIT: usize = 80;
const BODY_LIMIT: usize = 240;
const URL_LIMIT: usize = 300;
/// Push services hand out addresses well under this.
const ENDPOINT_LIMIT: usize = 2048;

/// The PC's VAPID key pair, and its public half as the phone subscribes
/// with it: the uncompressed point, in base64url.
pub struct PushKey {
    signing: SigningKey,
    public: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SavedKey {
    /// The private scalar, 32 bytes, in base64url.
    private_key: String,
}

impl PushKey {
    /// The key kept in `file`, or a new one, saved there before it is used:
    /// a key that changed would leave every phone subscribed to the old one.
    /// An unreadable file is moved aside and replaced, and the phones
    /// subscribe again when they next open the app.
    pub fn open(file: &Path) -> Result<Self, String> {
        if let Ok(raw) = fs::read_to_string(file) {
            match Self::parse(&raw) {
                Ok(key) => return Ok(key),
                Err(err) => match crate::store::quarantine(file) {
                    Some(kept) => eprintln!(
                        "{} is unreadable ({err}); kept as {}",
                        file.display(),
                        kept.display()
                    ),
                    None => eprintln!("{} is unreadable ({err})", file.display()),
                },
            }
        }
        let signing = SigningKey::random(&mut rand_core::OsRng);
        let saved = SavedKey {
            private_key: URL_SAFE_NO_PAD.encode(signing.to_bytes()),
        };
        write(file, &saved)?;
        Ok(Self::of(signing))
    }

    fn parse(raw: &str) -> Result<Self, String> {
        let saved: SavedKey = serde_json::from_str(raw).map_err(|err| err.to_string())?;
        let bytes = URL_SAFE_NO_PAD
            .decode(saved.private_key.trim())
            .map_err(|err| err.to_string())?;
        let signing = SigningKey::from_slice(&bytes).map_err(|err| err.to_string())?;
        Ok(Self::of(signing))
    }

    fn of(signing: SigningKey) -> Self {
        let public =
            URL_SAFE_NO_PAD.encode(signing.verifying_key().to_encoded_point(false).as_bytes());
        Self { signing, public }
    }

    /// What the phone hands its push service as `applicationServerKey`.
    pub fn public(&self) -> &str {
        &self.public
    }

    /// The `Authorization` of a message to `endpoint` (RFC 8292): a JWT for
    /// the push service's origin, signed ES256, and the key that checks it.
    pub fn vapid(&self, endpoint: &http::Uri, now: SystemTime) -> Result<String, String> {
        let scheme = endpoint.scheme_str().ok_or("the endpoint has no scheme")?;
        let host = endpoint.host().ok_or("the endpoint has no host")?;
        let audience = match endpoint.port_u16() {
            Some(port) => format!("{scheme}://{host}:{port}"),
            None => format!("{scheme}://{host}"),
        };
        let expires = now
            .duration_since(UNIX_EPOCH)
            .map_err(|err| err.to_string())?
            .saturating_add(VALID_FOR)
            .as_secs();
        let header = URL_SAFE_NO_PAD.encode(r#"{"typ":"JWT","alg":"ES256"}"#);
        let claims = URL_SAFE_NO_PAD
            .encode(json!({ "aud": audience, "exp": expires, "sub": CONTACT }).to_string());
        let signed = format!("{header}.{claims}");
        // ES256: ECDSA on P-256 over SHA-256, the signature as r ‖ s.
        let signature: Signature = self.signing.sign(signed.as_bytes());
        let token = format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature.to_bytes()));
        Ok(format!("vapid t={token}, k={}", self.public))
    }
}

fn write(file: &Path, saved: &SavedKey) -> Result<(), String> {
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)
            .map_err(|err| format!("could not create {}: {err}", dir.display()))?;
    }
    let body = serde_json::to_string_pretty(saved).map_err(|err| err.to_string())?;
    let tmp = file.with_extension("json.tmp");
    fs::write(&tmp, body).map_err(|err| format!("could not save the push key: {err}"))?;
    fs::rename(&tmp, file).map_err(|err| format!("could not save the push key: {err}"))
}

/// The language a phone's notifications are written in, as it said when it
/// subscribed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    #[default]
    En,
    Fr,
}

impl Lang {
    /// French for any `fr` tag, English for everything else.
    pub fn of(tag: &str) -> Self {
        let tag = tag.trim().to_ascii_lowercase();
        if tag == "fr" || tag.starts_with("fr-") || tag.starts_with("fr_") {
            Lang::Fr
        } else {
            Lang::En
        }
    }
}

/// Where a phone's browser takes its notifications: what
/// `PushSubscription.toJSON()` gives, and the language to write in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    pub endpoint: String,
    /// The browser's P-256 public key, base64url: what the message is
    /// encrypted for.
    pub p256dh: String,
    /// Its 16-byte secret, base64url.
    pub auth: String,
    #[serde(default)]
    pub lang: Lang,
    pub created: u64,
}

fn decode(value: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(value.trim().trim_end_matches('='))
        .ok()
}

/// The subscription a phone posts, `{ subscription, lang }`, checked: an
/// https address, and keys that make a P-256 point and a 16-byte secret.
pub fn subscription(body: &Value, now: u64) -> Result<Subscription, String> {
    let given = &body["subscription"];
    let endpoint = given["endpoint"]
        .as_str()
        .ok_or("the subscription has no endpoint")?
        .trim();
    if endpoint.len() > ENDPOINT_LIMIT {
        return Err("the endpoint is too long".to_string());
    }
    // The PC posts to this address: only a push service's, over https.
    let uri: http::Uri = endpoint
        .parse()
        .map_err(|_| "the endpoint is not an address".to_string())?;
    if uri.scheme_str() != Some("https") || uri.host().is_none_or(str::is_empty) {
        return Err("the endpoint is not an https address".to_string());
    }
    let p256dh = given["keys"]["p256dh"]
        .as_str()
        .ok_or("the subscription has no p256dh key")?;
    let auth = given["keys"]["auth"]
        .as_str()
        .ok_or("the subscription has no auth secret")?;
    let point = decode(p256dh).ok_or("the p256dh key is not base64url")?;
    PublicKey::from_sec1_bytes(&point).map_err(|_| "the p256dh key is not a P-256 key")?;
    if decode(auth).is_none_or(|secret| secret.len() != 16) {
        return Err("the auth secret is not 16 bytes".to_string());
    }
    Ok(Subscription {
        endpoint: endpoint.to_string(),
        p256dh: URL_SAFE_NO_PAD.encode(point),
        auth: auth.trim().trim_end_matches('=').to_string(),
        lang: Lang::of(body["lang"].as_str().unwrap_or_default()),
        created: now,
    })
}

/// A notification's words: one text for every phone, or one per language.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Text {
    Same(String),
    ByLang {
        en: String,
        #[serde(default)]
        fr: Option<String>,
    },
}

impl Text {
    pub fn get(&self, lang: Lang) -> &str {
        match (self, lang) {
            (Text::Same(text), _) => text,
            (Text::ByLang { fr: Some(fr), .. }, Lang::Fr) => fr,
            (Text::ByLang { en, .. }, _) => en,
        }
    }
}

/// What the window asks to be pushed (`remote_push`). `url` is a screen of
/// the phone app, as its address: `./#/pane/<id>`, `./#/shares`, `./#/`.
#[derive(Debug, Clone)]
pub struct Message {
    pub title: Text,
    pub body: Text,
    pub url: String,
}

fn cut(text: &str, limit: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut cut: String = text.chars().take(limit - 1).collect();
    cut.push('…');
    cut
}

/// What the phone's service worker reads: `{ title, body, url }`, in the
/// phone's language.
pub fn payload(message: &Message, lang: Lang) -> Vec<u8> {
    let url = if message.url.len() <= URL_LIMIT {
        message.url.as_str()
    } else {
        "./#/"
    };
    json!({
        "title": cut(message.title.get(lang), TITLE_LIMIT),
        "body": cut(message.body.get(lang), BODY_LIMIT),
        "url": url,
    })
    .to_string()
    .into_bytes()
}

/// How a push service took a message.
#[derive(Debug, PartialEq)]
pub enum Delivery {
    Delivered,
    /// 404 or 410: the subscription is gone for good (the app was removed,
    /// notifications were turned off), and is forgotten.
    Gone,
    /// Anything else — refused, unreachable — is tried again with the next
    /// message.
    Failed(String),
}

pub fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(SEND_TIMEOUT))
        .http_status_as_error(false)
        .build()
        .new_agent()
}

/// One message to one phone, encrypted for it and signed with the PC's key.
pub fn deliver(
    agent: &ureq::Agent,
    key: &PushKey,
    subscription: &Subscription,
    payload: Vec<u8>,
) -> Delivery {
    let request = (|| {
        let endpoint: http::Uri = subscription
            .endpoint
            .parse()
            .map_err(|_| "the endpoint is not an address".to_string())?;
        let point = decode(&subscription.p256dh).ok_or("the p256dh key is not base64url")?;
        let public =
            PublicKey::from_sec1_bytes(&point).map_err(|_| "the p256dh key is not a P-256 key")?;
        let secret = decode(&subscription.auth)
            .filter(|secret| secret.len() == 16)
            .ok_or("the auth secret is not 16 bytes")?;
        let authorization = key.vapid(&endpoint, SystemTime::now())?;
        let mut request = WebPushBuilder::new(endpoint, public, Auth::clone_from_slice(&secret))
            .with_valid_duration(VALID_FOR)
            .build(payload)
            .map_err(|err| err.to_string())?;
        let headers = request.headers_mut();
        headers.insert(
            http::header::AUTHORIZATION,
            http::HeaderValue::from_str(&authorization).map_err(|err| err.to_string())?,
        );
        // Something waits for the person: worth waking the phone for.
        headers.insert("urgency", http::HeaderValue::from_static("high"));
        Ok::<_, String>(request)
    })();
    let request = match request {
        Ok(request) => request,
        Err(err) => return Delivery::Failed(err),
    };
    match agent.run(request) {
        Ok(response) => match response.status().as_u16() {
            200..=299 => Delivery::Delivered,
            404 | 410 => Delivery::Gone,
            status => Delivery::Failed(format!("the push service answered {status}")),
        },
        Err(err) => Delivery::Failed(err.to_string()),
    }
}

/// `<app data>/remote/vapid.json`, from remote.json's path.
pub fn key_file(remote_json: &Path) -> PathBuf {
    remote_json
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join("remote")
        .join(KEY_FILE)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// RFC 8291, Appendix A: the example message, the keys on both sides and
    /// what it encrypts to.
    pub mod rfc8291 {
        pub const PLAINTEXT: &[u8] = b"When I grow up, I want to be a watermelon";
        pub const AS_PUBLIC: &str =
            "BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8";
        pub const UA_PRIVATE: &str = "q1dXpw3UpT5VOmu_cf_v6ih07Aems3njxI-JWgLcM94";
        pub const UA_PUBLIC: &str =
            "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4";
        pub const AUTH: &str = "BTBZMqHH6r4Tts7J_aSIgg";
        pub const SALT: &str = "DGv6ra1nlYgDCS1FRnbzlw";
        pub const IKM: &str = "S4lYMb_L0FxCeq0WhDx813KgSYqU26kOyzWUdsXYyrg";
        pub const CIPHERTEXT: &str = "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPTpK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN";
    }

    pub fn b64(value: &str) -> Vec<u8> {
        URL_SAFE_NO_PAD.decode(value).unwrap()
    }

    fn ua_secret() -> p256::SecretKey {
        p256::SecretKey::from_slice(&b64(rfc8291::UA_PRIVATE)).unwrap()
    }

    pub fn a_key() -> PushKey {
        PushKey::of(SigningKey::random(&mut rand_core::OsRng))
    }

    /// The claims of a VAPID `Authorization`, once its signature is checked
    /// with the key it carries.
    pub fn check_vapid(authorization: &str) -> Value {
        use p256::ecdsa::signature::Verifier;
        let rest = authorization.strip_prefix("vapid t=").unwrap();
        let (token, key) = rest.split_once(", k=").unwrap();
        let (signed, signature) = token.rsplit_once('.').unwrap();
        let verifying = p256::ecdsa::VerifyingKey::from_sec1_bytes(&b64(key)).unwrap();
        let signature = Signature::from_slice(&b64(signature)).unwrap();
        verifying.verify(signed.as_bytes(), &signature).unwrap();
        let (header, claims) = signed.split_once('.').unwrap();
        let header: Value = serde_json::from_slice(&b64(header)).unwrap();
        assert_eq!(header, json!({ "typ": "JWT", "alg": "ES256" }));
        serde_json::from_slice(&b64(claims)).unwrap()
    }

    /// The RFC's receiving browser, as a subscription.
    pub fn rfc_subscription(endpoint: &str, lang: Lang) -> Subscription {
        Subscription {
            endpoint: endpoint.to_string(),
            p256dh: rfc8291::UA_PUBLIC.to_string(),
            auth: rfc8291::AUTH.to_string(),
            lang,
            created: 1,
        }
    }

    /// What the RFC's receiving browser reads of a message sent to it.
    pub fn open_as_the_rfc_browser(body: Vec<u8>) -> Value {
        let auth = Auth::clone_from_slice(&b64(rfc8291::AUTH));
        let plain = web_push_native::decrypt(body, &ua_secret(), &auth).unwrap();
        serde_json::from_slice(&plain).unwrap()
    }

    #[test]
    fn the_rfc_8291_example_decrypts_to_its_message() {
        let auth = Auth::clone_from_slice(&b64(rfc8291::AUTH));
        let plain =
            web_push_native::decrypt(b64(rfc8291::CIPHERTEXT), &ua_secret(), &auth).unwrap();
        assert_eq!(plain, rfc8291::PLAINTEXT);
    }

    #[test]
    fn the_rfc_8291_example_encrypts_byte_for_byte() {
        let salt: [u8; 16] = b64(rfc8291::SALT).try_into().unwrap();
        let encrypted = ece_native::encrypt(
            b64(rfc8291::IKM),
            salt,
            b64(rfc8291::AS_PUBLIC),
            std::iter::once(rfc8291::PLAINTEXT.to_vec()),
            4096,
        )
        .unwrap();
        assert_eq!(encrypted, b64(rfc8291::CIPHERTEXT));
    }

    #[test]
    fn what_the_pc_encrypts_the_rfc_browser_reads() {
        let message = Message {
            title: Text::ByLang {
                en: "Ava waits for you".into(),
                fr: Some("Ava vous attend".into()),
            },
            body: Text::Same("Shop".into()),
            url: "./#/pane/p1".into(),
        };
        let subscription = rfc_subscription("https://push.example/abc", Lang::Fr);
        let point = PublicKey::from_sec1_bytes(&b64(&subscription.p256dh)).unwrap();
        let request = WebPushBuilder::new(
            subscription.endpoint.parse().unwrap(),
            point,
            Auth::clone_from_slice(&b64(&subscription.auth)),
        )
        .build(payload(&message, subscription.lang))
        .unwrap();
        assert_eq!(request.headers()["content-encoding"], "aes128gcm");
        let body = request.into_body();
        // A salt of its own, then RFC 8188's header: the record size and
        // the sender's one-time public key, an uncompressed P-256 point.
        assert_eq!(body[20], 65);
        assert_eq!(body[21], 0x04);
        assert_eq!(
            open_as_the_rfc_browser(body),
            json!({ "title": "Ava vous attend", "body": "Shop", "url": "./#/pane/p1" })
        );
    }

    #[test]
    fn the_vapid_token_is_signed_for_the_push_service_s_origin() {
        let key = a_key();
        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        for (endpoint, audience) in [
            (
                "https://web.push.apple.com/QGx1?x=1",
                "https://web.push.apple.com",
            ),
            ("http://127.0.0.1:21012/push/a", "http://127.0.0.1:21012"),
        ] {
            let authorization = key.vapid(&endpoint.parse().unwrap(), now).unwrap();
            assert!(authorization.ends_with(&format!(", k={}", key.public())));
            let claims = check_vapid(&authorization);
            assert_eq!(claims["aud"], audience);
            assert_eq!(claims["sub"], CONTACT);
            // Within the day RFC 8292 allows.
            let exp = claims["exp"].as_u64().unwrap();
            assert_eq!(exp, 1_800_000_000 + 12 * 60 * 60);
        }
    }

    fn scratch_key() -> PathBuf {
        super::super::store::tests::scratch_file("push-key")
            .parent()
            .unwrap()
            .join("remote")
            .join(KEY_FILE)
    }

    #[test]
    fn the_key_is_made_once_and_kept() {
        let file = scratch_key();
        assert!(!file.exists());
        let first = PushKey::open(&file).unwrap();
        assert!(file.exists());
        let saved = fs::read_to_string(&file).unwrap();

        let again = PushKey::open(&file).unwrap();
        assert_eq!(again.public(), first.public());
        assert_eq!(fs::read_to_string(&file).unwrap(), saved);
        // The public half is the uncompressed point a browser takes.
        let point = b64(first.public());
        assert_eq!(point.len(), 65);
        assert_eq!(point[0], 0x04);
        assert!(PublicKey::from_sec1_bytes(&point).is_ok());
        // The private half never leaves the file.
        assert!(!saved.contains(first.public()));
    }

    #[test]
    fn an_unreadable_key_is_kept_aside_and_replaced() {
        let file = scratch_key();
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "{ not a key").unwrap();
        let key = PushKey::open(&file).unwrap();
        assert_eq!(PushKey::open(&file).unwrap().public(), key.public());
        let kept = fs::read_dir(file.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name() != KEY_FILE)
            .count();
        assert_eq!(kept, 1);
    }

    fn posted(endpoint: &str, p256dh: &str, auth: &str, lang: &str) -> Value {
        json!({
            "subscription": {
                "endpoint": endpoint,
                "expirationTime": null,
                "keys": { "p256dh": p256dh, "auth": auth },
            },
            "lang": lang,
        })
    }

    #[test]
    fn a_browser_s_subscription_is_taken_as_it_posts_it() {
        let taken = subscription(
            &posted(
                "https://web.push.apple.com/QGx1",
                rfc8291::UA_PUBLIC,
                rfc8291::AUTH,
                "fr-FR",
            ),
            42,
        )
        .unwrap();
        assert_eq!(taken.endpoint, "https://web.push.apple.com/QGx1");
        assert_eq!(taken.p256dh, rfc8291::UA_PUBLIC);
        assert_eq!(taken.auth, rfc8291::AUTH);
        assert_eq!(taken.lang, Lang::Fr);
        assert_eq!(taken.created, 42);
        // Padded base64 is the same key.
        let padded = subscription(
            &posted(
                "https://fcm.googleapis.com/fcm/send/x",
                rfc8291::UA_PUBLIC,
                &format!("{}==", rfc8291::AUTH),
                "en",
            ),
            1,
        )
        .unwrap();
        assert_eq!(padded.auth, rfc8291::AUTH);
        assert_eq!(padded.lang, Lang::En);
    }

    #[test]
    fn a_subscription_that_is_not_a_push_service_s_is_refused() {
        for body in [
            posted(
                "http://push.example/x",
                rfc8291::UA_PUBLIC,
                rfc8291::AUTH,
                "en",
            ),
            posted(
                "file:///etc/passwd",
                rfc8291::UA_PUBLIC,
                rfc8291::AUTH,
                "en",
            ),
            posted("not an address", rfc8291::UA_PUBLIC, rfc8291::AUTH, "en"),
            posted("https://push.example/x", "AAAA", rfc8291::AUTH, "en"),
            posted(
                "https://push.example/x",
                rfc8291::UA_PUBLIC,
                "c2hvcnQ",
                "en",
            ),
            json!({ "lang": "en" }),
            json!({ "subscription": { "endpoint": "https://push.example/x" } }),
        ] {
            assert!(subscription(&body, 1).is_err(), "{body}");
        }
    }

    #[test]
    fn a_language_is_french_or_english() {
        assert_eq!(Lang::of("fr"), Lang::Fr);
        assert_eq!(Lang::of("FR-ca"), Lang::Fr);
        assert_eq!(Lang::of("en-GB"), Lang::En);
        assert_eq!(Lang::of("de"), Lang::En);
        assert_eq!(Lang::of(""), Lang::En);
    }

    #[test]
    fn a_notification_is_written_in_the_phone_s_language_and_kept_short() {
        let message = Message {
            title: Text::ByLang {
                en: "Ready to merge".into(),
                fr: Some("Prête à fusionner".into()),
            },
            body: Text::Same("x".repeat(1_000)),
            url: "./#/".into(),
        };
        let fr: Value = serde_json::from_slice(&payload(&message, Lang::Fr)).unwrap();
        let en: Value = serde_json::from_slice(&payload(&message, Lang::En)).unwrap();
        assert_eq!(fr["title"], "Prête à fusionner");
        assert_eq!(en["title"], "Ready to merge");
        assert_eq!(en["body"].as_str().unwrap().chars().count(), BODY_LIMIT);
        assert!(en["body"].as_str().unwrap().ends_with('…'));

        // Without French, English.
        let only = Text::ByLang {
            en: "Shared".into(),
            fr: None,
        };
        assert_eq!(only.get(Lang::Fr), "Shared");
        let parsed: Text = serde_json::from_value(json!({ "en": "a", "fr": "b" })).unwrap();
        assert_eq!(parsed.get(Lang::Fr), "b");
        let parsed: Text = serde_json::from_value(json!("same")).unwrap();
        assert_eq!(parsed.get(Lang::Fr), "same");
    }
}
