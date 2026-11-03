//! Every call of a tab passes here before the app sees it, in the order
//! src/lib/kitApi.ts holds an HTML tab's: no more than 16 waiting, a method
//! that exists, a permission held (read afresh), params of the right shape,
//! what only the app can refuse (`KitApi::check`), a rate not exceeded. What
//! fails answers the same codes and the same words as an HTML tab is
//! answered.
//!
//! It is public so that the app holds the calls of an HTML tab, relayed from
//! its page, with this very gate: the two kinds of tab are one set of rules.
//! A param a page leaves out and one it sends as `null` are not the same, as
//! they were not in kitApi.ts: only the methods that take no params, and
//! `files.list`, read a `null` as nothing.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use hangar_kit_ui::abi::{Call, CallId};
use hangar_kit_ui::api::{self, BadgeState, KitError, Needs, limits};

use crate::api::{HostCall, LoopbackCall};

const MINUTE: Duration = Duration::from_secs(60);

fn error(code: &str, message: impl Into<String>) -> KitError {
    KitError::new(code, message)
}

fn invalid(message: impl Into<String>) -> KitError {
    error(KitError::INVALID, message)
}

/// At most `max` takes in any `window`.
struct Rate {
    max: usize,
    window: Duration,
    taken: VecDeque<Instant>,
}

impl Rate {
    fn new(max: usize) -> Self {
        Rate {
            max,
            window: MINUTE,
            taken: VecDeque::new(),
        }
    }

    fn take(&mut self, now: Instant) -> bool {
        while self
            .taken
            .front()
            .is_some_and(|at| now.duration_since(*at) >= self.window)
        {
            self.taken.pop_front();
        }
        if self.taken.len() >= self.max {
            return false;
        }
        self.taken.push_back(now);
        true
    }
}

/// The calls of one tab: those waiting for their answer, and its rates.
pub struct Gate {
    /// The calls let through and not answered yet, by id, with their method.
    waiting: HashMap<CallId, &'static str>,
    send: Rate,
    notify: Rate,
    data_write: Rate,
    files_write: Rate,
    title: Rate,
    badge: Rate,
    toast: Rate,
}

impl Default for Gate {
    fn default() -> Self {
        Gate::new()
    }
}

impl Gate {
    pub fn new() -> Self {
        Gate {
            waiting: HashMap::new(),
            send: Rate::new(limits::WRITES_PER_MINUTE),
            notify: Rate::new(limits::NOTIFY_PER_MINUTE),
            data_write: Rate::new(limits::WRITES_PER_MINUTE),
            files_write: Rate::new(limits::WRITES_PER_MINUTE),
            title: Rate::new(limits::TAB_CHANGES_PER_MINUTE),
            badge: Rate::new(limits::TAB_CHANGES_PER_MINUTE),
            toast: Rate::new(limits::TOASTS_PER_MINUTE),
        }
    }

    /// A call is answered: it no longer counts among those waiting. False
    /// for an id that was not let through, or was answered already.
    pub fn settle(&mut self, id: CallId) -> bool {
        self.waiting.remove(&id).is_some()
    }

    /// Forgets every call waiting: the instance they were made by is gone.
    pub fn clear(&mut self) {
        self.waiting.clear();
    }

    /// How many calls are waiting for their answer.
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    /// Lets `call` through, as a [`HostCall`], or says why not. `gesture`:
    /// the update that made it answers a person's click or keystroke.
    /// `check` is the app's own refusal of a call whose params are right
    /// ([`KitApi::check`](crate::KitApi::check)), asked before its rate.
    pub fn admit(
        &mut self,
        call: &Call,
        granted: &[String],
        gesture: bool,
        now: Instant,
        check: impl FnOnce(&HostCall) -> Result<(), KitError>,
    ) -> Result<HostCall, KitError> {
        if self.waiting.len() >= limits::MAX_PENDING {
            return Err(error(
                KitError::RATE_LIMITED,
                format!("at most {} calls at once", limits::MAX_PENDING),
            ));
        }
        if self.waiting.contains_key(&call.id) {
            return Err(invalid(format!(
                "call {} is already waiting for its answer",
                call.id
            )));
        }
        let Some(needs) = api::needs(&call.method) else {
            return Err(error(
                KitError::UNKNOWN,
                format!("no method {:?}", call.method),
            ));
        };
        if !api::may_call(&call.method, &call.params, granted) {
            return Err(refusal(needs, &call.params, granted));
        }
        let host_call = parse(&call.method, &call.params, granted, gesture)?;
        check(&host_call)?;
        if let HostCall::Confirm(_) = host_call
            && self
                .waiting
                .values()
                .any(|method| *method == api::method::UI_CONFIRM)
        {
            return Err(error(
                KitError::BUSY,
                "a question is already asked; wait for its answer",
            ));
        }
        self.rate(&host_call, now)?;
        self.waiting.insert(call.id, host_call.method());
        Ok(host_call)
    }

    fn rate(&mut self, call: &HostCall, now: Instant) -> Result<(), KitError> {
        let (rate, max, what) = match call {
            HostCall::PanesSend { .. } => (&mut self.send, limits::WRITES_PER_MINUTE, "panes.send"),
            HostCall::Notify { .. } => (&mut self.notify, limits::NOTIFY_PER_MINUTE, "notify"),
            HostCall::DataWrite { .. } => (
                &mut self.data_write,
                limits::WRITES_PER_MINUTE,
                "data.write",
            ),
            HostCall::FilesWrite { .. } | HostCall::FilesWriteBinary { .. } => (
                &mut self.files_write,
                limits::WRITES_PER_MINUTE,
                "files.write",
            ),
            HostCall::TabSetTitle { .. } => (
                &mut self.title,
                limits::TAB_CHANGES_PER_MINUTE,
                "tab.setTitle",
            ),
            HostCall::TabSetBadge { .. } => (
                &mut self.badge,
                limits::TAB_CHANGES_PER_MINUTE,
                "tab.setBadge",
            ),
            HostCall::Toast(_) => (&mut self.toast, limits::TOASTS_PER_MINUTE, "ui.toast"),
            _ => return Ok(()),
        };
        if rate.take(now) {
            Ok(())
        } else {
            Err(error(
                KitError::RATE_LIMITED,
                format!("at most {max} {what} a minute; try again later"),
            ))
        }
    }
}

/// Why a call was not let through: `refusal` of src/lib/kitApi.ts, word for
/// word. A tab holding no port, or no command, is told only that.
fn refusal(needs: Needs, params: &Value, granted: &[String]) -> KitError {
    let denied = |what: &str| error(KitError::DENIED, format!("this tab was not granted {what}"));
    match needs {
        Needs::Nothing => denied("this"),
        Needs::Permission(permission) => denied(permission),
        Needs::Loopback => match params.get("port").and_then(Value::as_u64) {
            Some(port) if api::is_loopback_port(port) => denied(&format!("loopback:{port}")),
            _ if granted
                .iter()
                .any(|held| api::loopback_port(held).is_some()) =>
            {
                invalid("port must be a whole number from 1024 to 65535")
            }
            _ => denied("loopback:<port>"),
        },
        Needs::Command => match params.get("name").and_then(Value::as_str) {
            Some(name) if api::is_command_name(name) => denied(&format!("command:{name}")),
            _ if granted.iter().any(|held| api::command_name(held).is_some()) => {
                invalid("name must be the name of a command of this kit")
            }
            _ => denied("command:<name>"),
        },
    }
}

fn object(params: &Value) -> Result<&Map<String, Value>, KitError> {
    params
        .as_object()
        .ok_or_else(|| invalid("params must be an object"))
}

/// The params as `T`, the shape refused as `invalid` with serde's words.
fn typed<T: DeserializeOwned>(params: &Value) -> Result<T, KitError> {
    object(params)?;
    serde_json::from_value(params.clone()).map_err(|why| invalid(why.to_string()))
}

fn string<'a>(
    params: &'a Map<String, Value>,
    key: &str,
    max_chars: usize,
    empty: bool,
) -> Result<&'a str, KitError> {
    let Some(Value::String(value)) = params.get(key) else {
        return Err(invalid(format!("{key} must be a string")));
    };
    if !empty && value.trim().is_empty() {
        return Err(invalid(format!("{key} must not be empty")));
    }
    if value.chars().count() > max_chars {
        return Err(invalid(format!(
            "{key} must be at most {max_chars} characters"
        )));
    }
    Ok(value)
}

/// A param that may be left out; given, a string of at most `max_chars`, a
/// `null` refused as any other value that is not one (`stringParam`).
fn optional_string<'a>(
    params: &'a Map<String, Value>,
    key: &str,
    max_chars: usize,
    empty: bool,
) -> Result<Option<&'a str>, KitError> {
    match params.get(key) {
        None => Ok(None),
        Some(_) => string(params, key, max_chars, empty).map(Some),
    }
}

fn over_bytes(text: &str, max: usize) -> bool {
    text.len() > max
}

/// A path a tab gave, relative to `folder`, with `/`: either slash, no
/// `..`, no drive or stream, no hidden part unless `hidden`, no part ending
/// in a dot or a space. `relativePath` in src/lib/kitApi.ts; the app holds it
/// again on disk, where links can lead out.
fn relative_path(raw: Option<&Value>, folder: &str, hidden: bool) -> Result<String, KitError> {
    let bad = |why: String| invalid(format!("path {why}"));
    let Some(Value::String(raw)) = raw else {
        return Err(bad("must be a non-empty string".into()));
    };
    if raw.trim().is_empty() {
        return Err(bad("must be a non-empty string".into()));
    }
    if raw.chars().count() > limits::PATH_MAX_CHARS {
        return Err(bad(format!(
            "must be at most {} characters",
            limits::PATH_MAX_CHARS
        )));
    }
    if raw.contains('\0') {
        return Err(bad("must not hold a NUL character".into()));
    }
    if raw.starts_with(['/', '\\']) {
        return Err(bad(format!("must be relative to {folder}")));
    }
    let mut parts = Vec::new();
    for part in raw.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(bad("must not climb out with ..".into()));
        }
        if part.contains(':') {
            return Err(bad(format!("must be relative to {folder}")));
        }
        if !hidden && part.starts_with('.') {
            return Err(bad("must not name a hidden file or folder".into()));
        }
        if part.ends_with(['.', ' ']) {
            return Err(bad("parts must not end with a dot or a space".into()));
        }
        parts.push(part);
    }
    if parts.is_empty() {
        return Err(bad(format!("must name a file inside {folder}")));
    }
    Ok(parts.join("/"))
}

fn workspace_path(raw: Option<&Value>) -> Result<String, KitError> {
    relative_path(raw, "the workspace folder", false)
}

fn data_path(raw: Option<&Value>) -> Result<String, KitError> {
    relative_path(raw, "the kit's data folder", true)
}

/// What `files.write` reaches, and a tab holding it reads: hidden names
/// allowed, `.git` never, at any depth.
fn project_path(raw: Option<&Value>) -> Result<String, KitError> {
    let path = relative_path(raw, "the workspace folder", true)?;
    if path
        .split('/')
        .any(|part| part.eq_ignore_ascii_case(".git"))
    {
        return Err(invalid("path must not name anything under .git"));
    }
    Ok(path)
}

fn names_its_folder(raw: &Value) -> bool {
    raw.as_str().is_some_and(|raw| {
        !raw.trim().is_empty()
            && raw
                .split(['/', '\\'])
                .all(|part| part.is_empty() || part == ".")
    })
}

/// Characters that would not show, or would turn the line around: control
/// and format characters, line and paragraph separators.
fn hides_or_turns(c: char) -> bool {
    c.is_control()
        || matches!(c as u32,
            0x00AD | 0x0600..=0x0605 | 0x061C | 0x06DD | 0x070F | 0x180E
            | 0x200B..=0x200F | 0x2028..=0x202E | 0x2060..=0x2064 | 0x2066..=0x206F
            | 0xFEFF | 0xFFF9..=0xFFFB)
}

/// Text shown in the tab strip: trimmed, plain, on one line, `max`
/// characters at most. `shownText` of src/lib/kitApi.ts.
fn shown_text(value: Option<&Value>, key: &str, max: usize) -> Result<String, KitError> {
    let Some(Value::String(value)) = value else {
        return Err(invalid(format!("{key} must be a string")));
    };
    let text = value.trim();
    if text.is_empty() {
        return Err(invalid(format!("{key} must not be empty")));
    }
    if text.chars().count() > max {
        return Err(invalid(format!("{key} must be at most {max} characters")));
    }
    if text.chars().any(hides_or_turns) {
        return Err(invalid(format!("{key} must be plain text on one line")));
    }
    Ok(text.to_owned())
}

fn loopback(params: &Map<String, Value>) -> Result<LoopbackCall, KitError> {
    let port = params
        .get("port")
        .and_then(Value::as_u64)
        .filter(|port| api::is_loopback_port(*port));
    let Some(port) = port else {
        return Err(invalid("port must be a whole number from 1024 to 65535"));
    };
    let path = match params.get("path") {
        Some(Value::String(path)) if path.starts_with('/') && !path.starts_with("//") => path,
        _ => return Err(invalid("path must start with one /")),
    };
    if path.chars().count() > limits::LOOPBACK_PATH_MAX_CHARS {
        return Err(invalid(format!(
            "path must be at most {} characters",
            limits::LOOPBACK_PATH_MAX_CHARS
        )));
    }
    if !path.bytes().all(|b| (0x21..=0x7e).contains(&b)) || path.contains(['\\', '#']) {
        return Err(invalid(
            "path must be URL-encoded: no space, backslash, # or character beyond ASCII",
        ));
    }
    let method = match params.get("method") {
        None => api::HttpMethod::Get,
        Some(Value::String(method)) if method == "GET" => api::HttpMethod::Get,
        Some(Value::String(method)) if method == "POST" => api::HttpMethod::Post,
        _ => return Err(invalid("method must be GET or POST")),
    };
    let body = match params.get("body") {
        None => None,
        Some(Value::String(body)) => Some(body.clone()),
        Some(_) => return Err(invalid("body must be a string")),
    };
    if body.is_some() && method == api::HttpMethod::Get {
        return Err(invalid("a GET carries no body"));
    }
    if body
        .as_deref()
        .is_some_and(|body| over_bytes(body, limits::LOOPBACK_MAX_BODY_BYTES))
    {
        return Err(invalid("body is larger than 2 MB"));
    }
    let timeout_ms = match params.get("timeoutMs") {
        None => limits::LOOPBACK_TIMEOUT_MS,
        Some(value) => match value.as_u64() {
            Some(ms) if (1..=limits::LOOPBACK_MAX_TIMEOUT_MS as u64).contains(&ms) => ms as u32,
            _ => {
                return Err(invalid(format!(
                    "timeoutMs must be a whole number from 1 to {}",
                    limits::LOOPBACK_MAX_TIMEOUT_MS
                )));
            }
        },
    };
    Ok(LoopbackCall {
        port: port as u16,
        path: path.clone(),
        method,
        body,
        timeout_ms,
    })
}

/// The values of a `command.run`: strings each, or none at all. The app
/// holds them to the command's own rules.
fn command_args(params: &Map<String, Value>) -> Result<BTreeMap<String, String>, KitError> {
    match params.get("args") {
        None => Ok(BTreeMap::new()),
        Some(Value::Object(args)) => args
            .iter()
            .map(|(name, value)| match value {
                Value::String(value) => Ok((name.clone(), value.clone())),
                _ => Err(error(
                    KitError::BAD_ARGS,
                    format!("{name} must be a string"),
                )),
            })
            .collect(),
        Some(_) => Err(invalid("args must be an object")),
    }
}

fn badge(params: &Map<String, Value>) -> Result<Option<(BadgeState, Option<String>)>, KitError> {
    let state = match params.get("state") {
        Some(Value::Null) => return Ok(None),
        Some(state) => serde_json::from_value::<BadgeState>(state.clone()).ok(),
        None => None,
    };
    let Some(state) = state else {
        return Err(invalid(
            "state must be null or one of running, success, failure, neutral",
        ));
    };
    let text = match params.get("text") {
        None | Some(Value::Null) => None,
        text => Some(shown_text(text, "text", limits::TAB_BADGE_TEXT_MAX_CHARS)?),
    };
    Ok(Some((state, text)))
}

/// The params of an admitted method, checked, as a [`HostCall`].
fn parse(
    method: &str,
    params: &Value,
    granted: &[String],
    gesture: bool,
) -> Result<HostCall, KitError> {
    use api::method::*;
    let hidden = api::reads_hidden_files(granted);
    Ok(match method {
        WORKSPACE_INFO => HostCall::WorkspaceInfo,
        PANES_LIST => HostCall::PanesList,
        TAB_PARAMS => HostCall::TabParams,
        PANES_SEND => {
            let params = object(params)?;
            let pane = string(params, "pane", 200, false)?.trim().to_owned();
            let text = string(params, "text", limits::SEND_MAX_CHARS, false)?.to_owned();
            HostCall::PanesSend { pane, text }
        }
        FILES_READ | FILES_READ_BINARY => {
            let given = object(params)?.get("path");
            let path = if hidden {
                project_path(given)?
            } else {
                workspace_path(given)?
            };
            if method == FILES_READ {
                HostCall::FilesRead { path, hidden }
            } else {
                HostCall::FilesReadBinary { path, hidden }
            }
        }
        FILES_LIST => {
            let empty = Map::new();
            let params = if params.is_null() {
                &empty
            } else {
                object(params)?
            };
            let path = match params.get("path") {
                None => None,
                Some(given) if names_its_folder(given) => None,
                given => Some(project_path(given)?),
            };
            let pattern =
                optional_string(params, "pattern", limits::LIST_PATTERN_MAX_CHARS, false)?
                    .map(str::to_owned);
            HostCall::FilesList { path, pattern }
        }
        FILES_WRITE => {
            let params = object(params)?;
            let path = project_path(params.get("path"))?;
            let Some(Value::String(text)) = params.get("text") else {
                return Err(invalid("text must be a string"));
            };
            if over_bytes(text, limits::FILE_MAX_BYTES) {
                return Err(invalid("text is larger than 1 MB"));
            }
            let expected = match params.get("expected") {
                None => None,
                Some(Value::String(expected)) if over_bytes(expected, limits::FILE_MAX_BYTES) => {
                    return Err(invalid("expected is larger than 1 MB"));
                }
                Some(Value::String(expected)) => Some(expected.clone()),
                Some(_) => {
                    return Err(invalid(
                        "expected must be a string: the text the page last read",
                    ));
                }
            };
            HostCall::FilesWrite {
                path,
                text: text.clone(),
                expected,
            }
        }
        FILES_WRITE_BINARY => {
            let params = object(params)?;
            let path = project_path(params.get("path"))?;
            let Some(Value::String(base64)) = params.get("base64") else {
                return Err(invalid("base64 must be a string"));
            };
            if base64.len() > limits::BINARY_MAX_BYTES.div_ceil(3) * 4 {
                return Err(invalid("the image is larger than 10 MB"));
            }
            HostCall::FilesWriteBinary {
                path,
                base64: base64.clone(),
            }
        }
        PREVIEW_SHOW => HostCall::PreviewShow {
            path: workspace_path(object(params)?.get("path"))?,
        },
        DATA_READ => HostCall::DataRead {
            path: data_path(object(params)?.get("path"))?,
        },
        DATA_WRITE => {
            let params = object(params)?;
            let path = data_path(params.get("path"))?;
            let Some(Value::String(text)) = params.get("text") else {
                return Err(invalid("text must be a string"));
            };
            if over_bytes(text, limits::DATA_MAX_BYTES) {
                return Err(invalid("text is larger than 1 MB"));
            }
            HostCall::DataWrite {
                path,
                text: text.clone(),
            }
        }
        LOOPBACK_FETCH => HostCall::LoopbackFetch(loopback(object(params)?)?),
        COMMAND_RUN => {
            let params = object(params)?;
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            HostCall::CommandRun {
                name,
                args: command_args(params)?,
            }
        }
        NOTIFY => {
            let params = object(params)?;
            let title = string(params, "title", limits::NOTIFY_TITLE_MAX_CHARS, false)?
                .trim()
                .to_owned();
            let body = optional_string(params, "body", limits::NOTIFY_BODY_MAX_CHARS, true)?
                .unwrap_or_default()
                .to_owned();
            HostCall::Notify { title, body }
        }
        TAB_SET_TITLE => HostCall::TabSetTitle {
            title: shown_text(
                object(params)?.get("title"),
                "title",
                limits::TAB_TITLE_MAX_CHARS,
            )?,
        },
        TAB_SET_BADGE => HostCall::TabSetBadge {
            badge: badge(object(params)?)?,
        },
        UI_TOAST => {
            let toast: api::Toast = typed(params)?;
            if toast.message.trim().is_empty()
                || toast.message.chars().count() > limits::UI_MESSAGE_MAX_CHARS
            {
                return Err(invalid(format!(
                    "message must be 1 to {} characters",
                    limits::UI_MESSAGE_MAX_CHARS
                )));
            }
            if toast
                .action
                .as_deref()
                .is_some_and(|action| action.trim().is_empty() || action.chars().count() > 40)
            {
                return Err(invalid("action must be 1 to 40 characters"));
            }
            HostCall::Toast(toast)
        }
        UI_CONFIRM => {
            let confirm: api::Confirm = typed(params)?;
            if confirm.message.trim().is_empty()
                || confirm.message.chars().count() > limits::UI_MESSAGE_MAX_CHARS
            {
                return Err(invalid(format!(
                    "message must be 1 to {} characters",
                    limits::UI_MESSAGE_MAX_CHARS
                )));
            }
            for label in [&confirm.confirm, &confirm.cancel].into_iter().flatten() {
                if label.trim().is_empty() || label.chars().count() > 40 {
                    return Err(invalid("a button's label must be 1 to 40 characters"));
                }
            }
            HostCall::Confirm(confirm)
        }
        UI_COPY => {
            if !gesture {
                return Err(error(
                    KitError::DENIED,
                    "ui.copy answers a click or a keystroke only: a tab may not fill the clipboard by itself",
                ));
            }
            let copy: api::CopyText = typed(params)?;
            if over_bytes(&copy.text, limits::COPY_MAX_BYTES) {
                return Err(invalid("text is larger than 1 MB"));
            }
            HostCall::Copy { text: copy.text }
        }
        other => return Err(error(KitError::UNKNOWN, format!("no method {other:?}"))),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn call(id: CallId, method: &str, params: Value) -> Call {
        Call {
            id,
            method: method.to_owned(),
            params,
        }
    }

    /// The app's own check, refusing nothing.
    fn ok(_: &HostCall) -> Result<(), KitError> {
        Ok(())
    }

    fn grant(permissions: &[&str]) -> Vec<String> {
        permissions
            .iter()
            .map(|permission| (*permission).to_owned())
            .collect()
    }

    #[test]
    fn a_permission_not_held_is_denied_in_kit_api_words() {
        let mut gate = Gate::new();
        let now = Instant::now();
        let refused = gate
            .admit(
                &call(1, "panes.send", json!({ "pane": "Ava", "text": "hi" })),
                &[],
                false,
                now,
                ok,
            )
            .unwrap_err();
        assert_eq!(
            refused,
            KitError::new("denied", "this tab was not granted panes.send")
        );
        let refused = gate
            .admit(
                &call(2, "loopback.fetch", json!({ "port": 8765, "path": "/" })),
                &[],
                false,
                now,
                ok,
            )
            .unwrap_err();
        assert_eq!(refused.message, "this tab was not granted loopback:8765");
        let refused = gate
            .admit(&call(3, "files.delete", json!({})), &[], false, now, ok)
            .unwrap_err();
        assert_eq!(refused.code, "unknown");
    }

    #[test]
    fn paths_follow_relative_path() {
        let mut gate = Gate::new();
        let now = Instant::now();
        let reader = grant(&["files.read"]);
        for bad in ["../x", "/etc/passwd", "C:x", ".env", "a/./b. ", ""] {
            let refused = gate
                .admit(
                    &call(1, "files.read", json!({ "path": bad })),
                    &reader,
                    false,
                    now,
                    ok,
                )
                .unwrap_err();
            assert_eq!(refused.code, "invalid", "{bad}");
        }
        let admitted = gate
            .admit(
                &call(2, "files.read", json!({ "path": "src\\./main.rs" })),
                &reader,
                false,
                now,
                ok,
            )
            .unwrap();
        assert_eq!(
            admitted,
            HostCall::FilesRead {
                path: "src/main.rs".into(),
                hidden: false
            }
        );
        let writer = grant(&["files.read", "files.write"]);
        let admitted = gate
            .admit(
                &call(3, "files.read", json!({ "path": ".env" })),
                &writer,
                false,
                now,
                ok,
            )
            .unwrap();
        assert_eq!(
            admitted,
            HostCall::FilesRead {
                path: ".env".into(),
                hidden: true
            }
        );
        let refused = gate
            .admit(
                &call(4, "files.read", json!({ "path": ".GIT/config" })),
                &writer,
                false,
                now,
                ok,
            )
            .unwrap_err();
        assert_eq!(refused.code, "invalid");
    }

    #[test]
    fn rates_and_the_pending_cap_hold() {
        let mut gate = Gate::new();
        let now = Instant::now();
        let notify = grant(&["notify"]);
        for id in 0..5 {
            gate.admit(
                &call(id, "notify", json!({ "title": "t" })),
                &notify,
                false,
                now,
                ok,
            )
            .unwrap();
            gate.settle(id);
        }
        let refused = gate
            .admit(
                &call(9, "notify", json!({ "title": "t" })),
                &notify,
                false,
                now,
                ok,
            )
            .unwrap_err();
        assert_eq!(refused.code, "rate_limited");
        assert!(
            gate.admit(
                &call(10, "notify", json!({ "title": "t" })),
                &notify,
                false,
                now + MINUTE,
                ok
            )
            .is_ok()
        );
        let mut gate = Gate::new();
        for id in 0..16 {
            gate.admit(&call(id, "tab.params", json!(null)), &[], false, now, ok)
                .unwrap();
        }
        assert_eq!(
            gate.admit(&call(99, "tab.params", json!(null)), &[], false, now, ok)
                .unwrap_err()
                .code,
            "rate_limited"
        );
        assert!(gate.settle(3));
        assert!(
            gate.admit(&call(99, "tab.params", json!(null)), &[], false, now, ok)
                .is_ok()
        );
    }

    #[test]
    fn the_apps_refusal_comes_before_the_rate() {
        let mut gate = Gate::new();
        let now = Instant::now();
        let sender = grant(&["panes.send"]);
        let not_claude = |call: &HostCall| match call {
            HostCall::PanesSend { pane, .. } if pane == "Ben" => {
                Err(KitError::new("denied", "Ben runs a shell"))
            }
            _ => Ok(()),
        };
        let send = |id, pane: &str| call(id, "panes.send", json!({ "pane": pane, "text": "hi" }));
        for id in 0..20 {
            let refused = gate
                .admit(&send(id, "Ben"), &sender, false, now, not_claude)
                .unwrap_err();
            assert_eq!(refused.code, "denied");
        }
        // Nothing of the refused calls waits, nor counted against the sends.
        assert_eq!(gate.waiting(), 0);
        assert!(
            gate.admit(&send(99, "Ava"), &sender, false, now, not_claude)
                .is_ok()
        );
    }

    #[test]
    fn a_null_is_not_a_param_left_out() {
        let mut gate = Gate::new();
        let now = Instant::now();
        let all = grant(&[
            "files.read",
            "files.write",
            "notify",
            "loopback:8765",
            "command:ci",
        ]);
        let mut id = 0;
        let mut admit = |method: &str, params: Value| {
            id += 1;
            let answer = gate.admit(&call(id, method, params), &all, false, now, ok);
            if let Ok(admitted) = &answer {
                gate.settle(id);
                let _ = admitted;
            }
            answer
        };
        for (method, params) in [
            (
                "files.write",
                json!({ "path": "a", "text": "x", "expected": null }),
            ),
            ("files.list", json!({ "path": null })),
            ("files.list", json!({ "pattern": null })),
            ("notify", json!({ "title": "t", "body": null })),
            (
                "loopback.fetch",
                json!({ "port": 8765, "path": "/", "method": null }),
            ),
            (
                "loopback.fetch",
                json!({ "port": 8765, "path": "/", "body": null }),
            ),
            (
                "loopback.fetch",
                json!({ "port": 8765, "path": "/", "timeoutMs": null }),
            ),
            ("command.run", json!({ "name": "ci", "args": null })),
        ] {
            let refused = admit(method, params.clone()).unwrap_err();
            assert_eq!(refused.code, "invalid", "{method} {params}");
        }
        for (method, params) in [
            ("files.write", json!({ "path": "a", "text": "x" })),
            ("files.list", json!({})),
            ("files.list", Value::Null),
            ("notify", json!({ "title": "t" })),
            ("loopback.fetch", json!({ "port": 8765, "path": "/" })),
            ("command.run", json!({ "name": "ci" })),
        ] {
            assert!(admit(method, params.clone()).is_ok(), "{method} {params}");
        }
    }

    #[test]
    fn the_clipboard_needs_a_gesture() {
        let mut gate = Gate::new();
        let now = Instant::now();
        assert_eq!(
            gate.admit(
                &call(1, "ui.copy", json!({ "text": "x" })),
                &[],
                false,
                now,
                ok
            )
            .unwrap_err()
            .code,
            "denied"
        );
        assert_eq!(
            gate.admit(
                &call(2, "ui.copy", json!({ "text": "x" })),
                &[],
                true,
                now,
                ok
            )
            .unwrap(),
            HostCall::Copy { text: "x".into() }
        );
    }

    #[test]
    fn tab_titles_and_loopback_are_checked() {
        let mut gate = Gate::new();
        let now = Instant::now();
        assert_eq!(
            gate.admit(
                &call(1, "tab.setTitle", json!({ "title": "  Run #4 " })),
                &[],
                false,
                now,
                ok
            )
            .unwrap(),
            HostCall::TabSetTitle {
                title: "Run #4".into()
            }
        );
        assert!(
            gate.admit(
                &call(2, "tab.setTitle", json!({ "title": "a\u{202e}b" })),
                &[],
                false,
                now,
                ok
            )
            .is_err()
        );
        let port = grant(&["loopback:8765"]);
        let fetch = gate
            .admit(
                &call(
                    3,
                    "loopback.fetch",
                    json!({ "port": 8765, "path": "/q?a=1" }),
                ),
                &port,
                false,
                now,
                ok,
            )
            .unwrap();
        assert!(matches!(
            fetch,
            HostCall::LoopbackFetch(LoopbackCall {
                timeout_ms: 5000,
                ..
            })
        ));
        let bad = gate
            .admit(
                &call(4, "loopback.fetch", json!({ "port": 8765, "path": "/a b" })),
                &port,
                false,
                now,
                ok,
            )
            .unwrap_err();
        assert_eq!(bad.code, "invalid");
    }
}
