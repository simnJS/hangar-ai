//! The tab message protocol of docs/kits.md as a kit's page in a helper
//! window speaks it, through the [kit bridge](crate::KIT_BRIDGE): what the
//! page's `window.hangar.call` posts arrives as an [`Event::Ipc`]
//! (`crate::Event::Ipc`), read here the way the frame's host read a
//! `message` event (`receive` in src/lib/kitApi.ts), and what Hangar answers
//! or tells it is built here, for [`Client::post_message`]
//! (`crate::Client::post_message`).
//!
//! This is the envelope only. Which methods there are, what they need and
//! what they answer is the app's, which holds a page's calls to the same gate
//! as a native tab's (hangar-kit-host), so an HTML tab and a native one are
//! one set of rules.
//!
//! What a page posts is the page's word: a message that is not a request of
//! the protocol is dropped without a word, since answering it would tell a
//! stranger something, and a document that is not the kit's own page is not
//! listened to at all ([`is_kit_document`]).

use serde_json::{Value, json};

use crate::scheme::scheme_base_for;

/// The protocol's version, the `hangar` of every message.
pub const VERSION: u64 = 1;

/// The longest id a page may give a call, as a string.
pub const ID_MAX_CHARS: usize = 128;

/// The scheme a kit's pages are served by (`hangar-core`'s `kit_scheme`).
pub const SCHEME: &str = "hangar-kit";

/// What a page posted, once read.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// A call: the id as the page gave it, to answer with, the method, and
    /// the params, `null` when the page left them out.
    Call {
        id: Value,
        method: String,
        params: Value,
    },
    /// A request whose method is not a string: answered `invalid`, as the
    /// frame's host answered it.
    NoMethod { id: Value },
}

/// Whether `id` is one a call may have: a string of 1 to 128 characters, or
/// a number (JSON has no number that is not finite).
pub fn valid_id(id: &Value) -> bool {
    match id {
        Value::String(id) => (1..=ID_MAX_CHARS).contains(&id.chars().count()),
        Value::Number(_) => true,
        _ => false,
    }
}

/// Reads what a page posted through the bridge: JSON of an object with
/// `hangar: 1` and a valid `id`. Anything else is not the protocol's, and is
/// `None`.
pub fn read(body: &str) -> Option<Incoming> {
    let message: Value = serde_json::from_str(body).ok()?;
    let object = message.as_object()?;
    if object.get("hangar").and_then(Value::as_u64) != Some(VERSION) {
        return None;
    }
    let id = object.get("id").filter(|id| valid_id(id))?.clone();
    match object.get("method") {
        Some(Value::String(method)) => Some(Incoming::Call {
            id,
            method: method.clone(),
            params: object.get("params").cloned().unwrap_or(Value::Null),
        }),
        _ => Some(Incoming::NoMethod { id }),
    }
}

/// The answer to call `id`: `{ hangar: 1, id, result }`, a call that
/// answers nothing answering `null`.
pub fn result(id: &Value, result: Value) -> Value {
    json!({ "hangar": VERSION, "id": id, "result": result })
}

/// A call refused or failed: `{ hangar: 1, id, error: { code, message } }`.
pub fn error(id: &Value, code: &str, message: &str) -> Value {
    json!({ "hangar": VERSION, "id": id, "error": { "code": code, "message": message } })
}

/// One of Hangar's events: `{ hangar: 1, event, data }`.
pub fn event(name: &str, data: Value) -> Value {
    json!({ "hangar": VERSION, "event": name, "data": data })
}

/// `encodeURIComponent`: everything but the letters, the digits and
/// `-_.!~*'()` written as UTF-8 `%XX`.
fn encode_component(part: &str) -> String {
    let mut out = String::with_capacity(part.len());
    for byte in part.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Where the scheme serves a file of a kit (`kitFileUrl`): Windows' webview
/// reaches custom schemes as `http://<scheme>.localhost`, the others by the
/// scheme itself. Segments are encoded one by one, so the page's relative
/// URLs resolve inside the kit.
pub fn kit_file_url_for(kit_id: &str, path: &str, windows: bool) -> String {
    let parts: Vec<String> = path
        .split(['/', '\\'])
        .filter(|part| !part.is_empty() && *part != ".")
        .map(encode_component)
        .collect();
    format!(
        "{}/{}/{}",
        scheme_base_for(SCHEME, windows),
        encode_component(kit_id),
        parts.join("/")
    )
}

/// [`kit_file_url_for`] for the platform this is built for.
pub fn kit_file_url(kit_id: &str, path: &str) -> String {
    kit_file_url_for(kit_id, path, cfg!(windows))
}

/// The start of every URL of a kit's files, in each spelling an engine may
/// give it: what a window of the kit may navigate to.
pub fn kit_prefixes(kit_id: &str) -> Vec<String> {
    let kit = encode_component(kit_id);
    [false, true]
        .into_iter()
        .map(|windows| format!("{}/{kit}/", scheme_base_for(SCHEME, windows)))
        .chain(std::iter::once(format!(
            "https://{SCHEME}.localhost/{kit}/"
        )))
        .collect()
}

/// Whether the document an ipc came from (`Event::Ipc`'s `uri`) is one of
/// the kit's own pages. The frame's host answered only its own frame, whose
/// origin the sandbox made opaque; a window of the helper holds one page,
/// and this is how the parent tells that page from anything it went to.
pub fn is_kit_document(uri: &str, kit_id: &str) -> bool {
    kit_prefixes(kit_id)
        .iter()
        .any(|prefix| uri.starts_with(prefix.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_call_with_its_own_id_and_params() {
        assert_eq!(
            read(r#"{"hangar":1,"id":"r1","method":"panes.list"}"#),
            Some(Incoming::Call {
                id: json!("r1"),
                method: "panes.list".into(),
                params: Value::Null,
            })
        );
        assert_eq!(
            read(r#"{"hangar":1,"id":12,"method":"files.read","params":{"path":"a"}}"#),
            Some(Incoming::Call {
                id: json!(12),
                method: "files.read".into(),
                params: json!({ "path": "a" }),
            })
        );
    }

    #[test]
    fn drops_what_is_not_a_request_of_the_protocol() {
        let long = format!(
            r#"{{"hangar":1,"id":"{}","method":"panes.list"}}"#,
            "x".repeat(200)
        );
        for body in [
            "not json",
            "null",
            r#""panes.list""#,
            "[1,2]",
            r#"{"id":"x","method":"panes.list"}"#,
            r#"{"hangar":2,"id":"x","method":"panes.list"}"#,
            r#"{"hangar":"1","id":"x","method":"panes.list"}"#,
            r#"{"hangar":1.5,"id":"x","method":"panes.list"}"#,
            r#"{"hangar":1,"method":"panes.list"}"#,
            r#"{"hangar":1,"id":"","method":"panes.list"}"#,
            &long,
            r#"{"hangar":1,"id":null,"method":"panes.list"}"#,
            r#"{"hangar":1,"id":{"nested":true},"method":"panes.list"}"#,
        ] {
            assert_eq!(read(body), None, "{body}");
        }
    }

    #[test]
    fn a_request_without_a_method_name_is_one_to_refuse() {
        assert_eq!(
            read(r#"{"hangar":1,"id":7,"method":42}"#),
            Some(Incoming::NoMethod { id: json!(7) })
        );
        assert_eq!(
            error(&json!(7), "invalid", "method must be a string"),
            json!({ "hangar": 1, "id": 7, "error": { "code": "invalid", "message": "method must be a string" } })
        );
    }

    #[test]
    fn answers_and_events_are_the_protocols_messages() {
        assert_eq!(
            result(&json!("a"), json!({ "ok": true })),
            json!({ "hangar": 1, "id": "a", "result": { "ok": true } })
        );
        assert_eq!(
            event("panes.changed", json!({})),
            json!({ "hangar": 1, "event": "panes.changed", "data": {} })
        );
    }

    #[test]
    fn reaches_the_scheme_the_way_each_platforms_webview_does() {
        assert_eq!(
            kit_file_url_for("hello-kit", "tabs/status/index.html", true),
            "http://hangar-kit.localhost/hello-kit/tabs/status/index.html"
        );
        assert_eq!(
            kit_file_url_for("hello-kit", "tabs/status/index.html", false),
            "hangar-kit://localhost/hello-kit/tabs/status/index.html"
        );
    }

    #[test]
    fn encodes_each_segment_and_keeps_the_separators() {
        assert_eq!(
            kit_file_url_for("hello-kit", "./tabs\\my tab/é#1.html", true),
            "http://hangar-kit.localhost/hello-kit/tabs/my%20tab/%C3%A9%231.html"
        );
    }

    #[test]
    fn listens_to_the_kits_own_pages_only() {
        let page = kit_file_url_for("hello-kit", "tabs/a.html", false);
        assert!(is_kit_document(&page, "hello-kit"));
        assert!(is_kit_document(
            "http://hangar-kit.localhost/hello-kit/tabs/b.html",
            "hello-kit"
        ));
        for other in [
            "hangar-kit://localhost/other-kit/tabs/a.html",
            "hangar-kit://localhost/hello-kit-2/a.html",
            "https://example.com/hello-kit/a.html",
            "about:blank",
            "",
        ] {
            assert!(!is_kit_document(other, "hello-kit"), "{other}");
        }
    }
}
