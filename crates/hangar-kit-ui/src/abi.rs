//! The envelope between Hangar and a kit's WebAssembly module: what crosses
//! the boundary, in which order, and who owns the bytes.
//!
//! docs/native/kits-native.md, "The WebAssembly ABI", is the prose of this
//! module. In short:
//!
//! - the module exports [`export::ABI`], [`export::ALLOC`],
//!   [`export::INIT`], [`export::UPDATE`] and [`export::VIEW`], and imports
//!   nothing but [`import::LOG`] and [`import::NOW`] from
//!   [`import::MODULE`];
//! - every message is UTF-8 JSON: [`Start`] and [`Input`] in, [`Output`] and
//!   [`View`] out;
//! - Hangar writes a message into a buffer the module allocated with
//!   `hangar_alloc`, and the module owns it from then on; the module answers
//!   with a pointer and a length packed in a `u64` ([`pack`]), a buffer that
//!   stays the module's and that Hangar copies before calling it again.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api::{Caller, KitError, Theme};
use crate::element::Element;
use crate::event::UiEvent;

/// The ABI this crate describes. Hangar refuses a module that answers
/// another one it does not read, saying a newer Hangar is needed.
///
/// What changes it: another export or import signature, another meaning
/// for one, another envelope. What does not: a widget, a field, a method, an
/// event or an input kind added, all of which an older side ignores.
pub const ABI_VERSION: u32 = 1;

/// The functions a kit's module exports.
pub mod export {
    /// `() -> i32`: the [`ABI_VERSION`](super::ABI_VERSION) it was built
    /// against. Called first, before anything is allocated.
    pub const ABI: &str = "hangar_abi_version";
    /// `(len: i32) -> i32`: a buffer of `len` bytes for Hangar to write a
    /// message into. The module owns it, and frees it once read.
    pub const ALLOC: &str = "hangar_alloc";
    /// `(ptr: i32, len: i32) -> i64`: a [`Start`](super::Start) in, an
    /// [`Output`](super::Output) out. Called once per instance.
    pub const INIT: &str = "hangar_init";
    /// `(ptr: i32, len: i32) -> i64`: an [`Input`](super::Input) in, an
    /// [`Output`](super::Output) out.
    pub const UPDATE: &str = "hangar_update";
    /// `() -> i64`: the [`View`](super::View) of the state as it is now.
    pub const VIEW: &str = "hangar_view";
    /// Its linear memory, as a Rust module exports it by itself.
    pub const MEMORY: &str = "memory";
}

/// What Hangar gives a module to import, and nothing else: no WASI, no
/// clock or file of the system. Anything a kit needs from the machine is a
/// call ([`Call`](super::Call)), held to its tab's permissions.
pub mod import {
    pub const MODULE: &str = "hangar";
    /// `(level: i32, ptr: i32, len: i32)`: a line for the tab's log, UTF-8,
    /// [`LogLevel`](super::LogLevel) as a number. A `panic` line is what
    /// Hangar shows as the reason when the module traps right after.
    pub const LOG: &str = "log";
    /// `() -> i64`: milliseconds since the Unix epoch, UTC.
    pub const NOW: &str = "now";
}

/// A pointer and a length in the module's memory, as an export answers them:
/// the pointer in the high 32 bits.
pub const fn pack(ptr: u32, len: u32) -> u64 {
    ((ptr as u64) << 32) | len as u64
}

/// [`pack`]'s pointer and length.
pub const fn unpack(packed: u64) -> (u32, u32) {
    ((packed >> 32) as u32, packed as u32)
}

/// The largest message either side sends: a view, an output, an input.
/// Hangar stops a module that answers more.
pub const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;

/// The most timers a tab keeps at once; the rest are ignored, and logged.
pub const MAX_TIMERS: usize = 16;

/// The shortest period of a timer: a shorter one runs at this.
pub const MIN_TIMER_MS: u32 = 100;

/// The longest log line Hangar keeps, in bytes; the rest is cut.
pub const MAX_LOG_LINE_BYTES: usize = 4096;

/// The number a kit gives a call, unique among its calls still waiting.
pub type CallId = u64;

/// What `hangar_init` is given: who the tab is, and where it is shown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Start {
    /// The ABI the host speaks to it, [`ABI_VERSION`] or one it reads too.
    pub abi: u32,
    /// Hangar's version, `1.2.3`.
    pub hangar: String,
    pub kit: KitInfo,
    pub tab: TabInfo,
    pub theme: Theme,
    /// What this tab may call when it starts: its manifest's permissions the
    /// person granted. Read again at each call, so this may change: a page
    /// draws its denied state from it, and still handles a `denied` answer.
    #[serde(default)]
    pub permissions: Vec<String>,
    /// Whether the tab is in front.
    #[serde(default = "visible_by_default")]
    pub visible: bool,
    /// The person's time zone when the tab started, as minutes east of UTC
    /// (`120` in Paris in summer): what `format::since`, `clock` and `date`
    /// write local times with, wasm32 having no time zone of its own.
    #[serde(default)]
    pub utc_offset: i32,
}

fn visible_by_default() -> bool {
    true
}

impl Start {
    /// Whether the tab held what `method` needs when it started.
    pub fn may_call(&self, method: &str, params: &Value) -> bool {
        crate::api::may_call(method, params, &self.permissions)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KitInfo {
    pub id: String,
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TabInfo {
    /// The tab's id in its kit's manifest.
    pub id: String,
    /// What it was opened with, `{}` for none.
    #[serde(default)]
    pub params: BTreeMap<String, String>,
    /// Who asked for it, when an agent or a hook did.
    #[serde(default)]
    pub caller: Option<Caller>,
}

/// What `hangar_update` is given. In JSON, an object whose `type` says
/// which: `ui`, `reply`, `timer` or `event`. A kit ignores a `type` it does
/// not know: a newer Hangar may send more.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Input {
    /// Someone used a widget.
    Ui(UiEvent),
    /// A call answered.
    Reply(Reply),
    /// A timer of the tab's subscriptions came round.
    Timer(TimerFired),
    /// Something happened in Hangar that the tab subscribed to.
    Event(HangarEvent),
    /// A keyboard shortcut of the tab's subscriptions was pressed: a
    /// person's keystroke, as a click is.
    Shortcut(ShortcutPressed),
}

/// A shortcut pressed, by the spec the tab subscribed with
/// ([`crate::keys`]): `{ "type": "shortcut", "keys": "mod+s" }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutPressed {
    pub keys: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reply {
    pub id: CallId,
    #[serde(flatten)]
    pub outcome: Outcome,
}

/// A call's answer: `{ "result": … }` or `{ "error": { "code", "message" } }`,
/// as in the tab message protocol of HTML tabs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    Result(Value),
    Error(KitError),
}

impl From<Result<Value, KitError>> for Outcome {
    fn from(result: Result<Value, KitError>) -> Self {
        match result {
            Ok(value) => Outcome::Result(value),
            Err(error) => Outcome::Error(error),
        }
    }
}

impl From<Outcome> for Result<Value, KitError> {
    fn from(outcome: Outcome) -> Self {
        match outcome {
            Outcome::Result(value) => Ok(value),
            Outcome::Error(error) => Err(error),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerFired {
    pub id: String,
}

/// An event of Hangar's: see [`crate::api::event`] for their names, what each
/// carries and what each needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HangarEvent {
    pub name: String,
    #[serde(default)]
    pub data: Value,
}

/// What `hangar_init` and `hangar_update` answer.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output {
    /// Calls to make, answered later by a [`Reply`] each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub calls: Vec<Call>,
    /// What the tab wants to hear from now on, all of it: what it leaves out
    /// stops. Absent, they stay as they were.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscriptions: Option<Subscriptions>,
    /// What to do to the page once it is drawn: a field focused, a part
    /// scrolled. Done after the view that follows this output.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operations: Vec<Operation>,
}

/// Something done to the page, naming a widget by its `id`; an id the view
/// does not have does nothing. In JSON, an object whose `type` says which.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Operation {
    /// The keyboard goes to this text input, search or text area.
    Focus { id: String },
    /// A `scroll` or a log with an id shows this: a line of a log, from 0,
    /// with a few lines above it; a point `y` from the top of a scroll; or
    /// its end.
    ScrollTo {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        line: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        y: Option<f32>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        end: bool,
    },
    /// A text area's cursor goes to the start of a line, from 1, at a
    /// column, from 1, and a text area with `lineNumbers` scrolls to it.
    CursorTo {
        id: String,
        line: u32,
        #[serde(default = "first_column")]
        column: u32,
    },
    /// What a newer SDK asks and this Hangar does not do.
    #[serde(other)]
    Unsupported,
}

fn first_column() -> u32 {
    1
}

/// A call to Hangar: a method of the host API, and its params, as an HTML
/// tab's `hangar.call(method, params)` sends them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Call {
    pub id: CallId,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscriptions {
    /// Names of Hangar's events. One the tab may not hear is left out,
    /// silently, as for an HTML tab.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub timers: Vec<Timer>,
    /// Keyboard shortcuts, as [`crate::keys`] writes them: pressed while the
    /// person is in the tab, each comes as an [`Input::Shortcut`]. A key a
    /// focused control uses itself (Ctrl+C in a field) is the control's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shortcuts: Vec<String>,
}

/// A timer: a [`TimerFired`] with this `id` every `everyMs`. One that keeps
/// its id and period keeps running across updates; another period starts it
/// again.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Timer {
    pub id: String,
    pub every_ms: u32,
}

/// What `hangar_view` answers: the whole page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    /// Numbers this view, for the events drawn from it ([`UiEvent::view`]).
    pub seq: u32,
    pub root: Element,
}

/// How much a log line matters. `panic` is the last word of a module before
/// it traps: Hangar shows it as the reason the tab stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[repr(u32)]
pub enum LogLevel {
    Debug = 0,
    Info = 1,
    Warn = 2,
    Error = 3,
    Panic = 4,
}

impl LogLevel {
    pub fn from_u32(level: u32) -> LogLevel {
        match level {
            0 => LogLevel::Debug,
            1 => LogLevel::Info,
            2 => LogLevel::Warn,
            3 => LogLevel::Error,
            _ => LogLevel::Panic,
        }
    }
}
