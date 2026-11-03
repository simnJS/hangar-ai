//! What a kit hands Hangar during `init` and `update`: calls, answered later
//! by a message of its own, as an iced `Task` would be.

use std::time::Duration;

use serde_json::Value;

use hangar_kit_ui::Notice;
use hangar_kit_ui::abi::{Call, CallId, LogLevel, MIN_TIMER_MS, Operation};
use hangar_kit_ui::api::{self, KitError, Request};

/// A call's answer, turned into the kit's message; none drops it.
pub(crate) type Pending<M> = Box<dyn FnOnce(Result<Value, KitError>) -> Option<M>>;

/// Given to `init` and `update`. Nothing it asks for happens while they
/// run: the calls go out when they return, and each answer comes back as an
/// update of its own.
pub struct Context<M> {
    next_id: CallId,
    pub(crate) calls: Vec<(Call, Pending<M>)>,
    gesture: bool,
    utc_offset: i32,
    pub(crate) operations: Vec<Operation>,
    /// One-shot timers asked for, by key: `(key, delay in ms, message)`.
    pub(crate) after: Vec<(String, u32, M)>,
    pub(crate) cancelled: Vec<String>,
}

impl<M: 'static> Context<M> {
    pub(crate) fn new(next_id: CallId, gesture: bool, utc_offset: i32) -> Self {
        Context {
            next_id,
            calls: Vec::new(),
            gesture,
            utc_offset,
            operations: Vec::new(),
            after: Vec::new(),
            cancelled: Vec::new(),
        }
    }

    pub(crate) fn next_id(&self) -> CallId {
        self.next_id
    }

    /// Calls Hangar: `on_reply` makes the message the answer comes back as.
    ///
    /// ```ignore
    /// cx.call(api::PanesList {}, Message::Panes);
    /// ```
    pub fn call<R: Request>(
        &mut self,
        request: R,
        on_reply: impl FnOnce(Result<R::Output, KitError>) -> M + 'static,
    ) {
        let params = serde_json::to_value(&request).unwrap_or(Value::Null);
        self.push(R::METHOD, params, move |answer| {
            Some(on_reply(decode::<R>(answer)))
        });
    }

    /// Calls Hangar and lets the answer go: a refusal is logged, nothing
    /// else comes back.
    pub fn call_quietly<R: Request>(&mut self, request: R) {
        let params = serde_json::to_value(&request).unwrap_or(Value::Null);
        self.push(R::METHOD, params, |answer| {
            if let Err(error) = answer {
                crate::sys::log(LogLevel::Warn, &format!("{}: {error}", R::METHOD));
            }
            None
        });
    }

    /// Calls a method by its name, with params and an answer as JSON: a
    /// method a newer Hangar has and this SDK does not type yet.
    pub fn call_raw(
        &mut self,
        method: impl Into<String>,
        params: Value,
        on_reply: impl FnOnce(Result<Value, KitError>) -> M + 'static,
    ) {
        let method = method.into();
        self.push(&method, params, move |answer| Some(on_reply(answer)));
    }

    /// A toast at the foot of the tab, for what just happened.
    pub fn toast(&mut self, kind: Notice, message: impl Into<String>) {
        self.call_quietly(api::Toast {
            message: message.into(),
            kind,
            action: None,
        });
    }

    /// Asks the person, in place of the page; `on_answer` is told whether
    /// they confirmed. A refusal of the call reads as a no.
    pub fn confirm(&mut self, question: api::Confirm, on_answer: impl FnOnce(bool) -> M + 'static) {
        self.call(question, move |answer| on_answer(answer.unwrap_or(false)));
    }

    /// Puts `text` on the clipboard. Only from an update that answers a
    /// person's click or keystroke: Hangar refuses it otherwise.
    pub fn copy(&mut self, text: impl Into<String>) {
        self.call_quietly(api::CopyText { text: text.into() });
    }

    /// Whether this update answers something a person did on the page, a
    /// click or a keystroke, rather than a timer, an answer or an event.
    pub fn is_gesture(&self) -> bool {
        self.gesture
    }

    /// Milliseconds since the Unix epoch, UTC, from Hangar's clock.
    pub fn now(&self) -> i64 {
        crate::sys::now_ms()
    }

    /// The person's time zone when the tab started, as minutes east of UTC:
    /// what [`format::since`](crate::format::since) and the others take.
    pub fn utc_offset(&self) -> i32 {
        self.utc_offset
    }

    /// `message` once, `delay` from now (100 ms at the shortest), under
    /// `key`: asking again under the same key starts the wait again, which
    /// is a debounce (save 1.5 s after the last keystroke). Gone with the
    /// tab, as a page's `setTimeout` is. It takes one of the tab's 16 timers
    /// while it waits.
    pub fn after(&mut self, key: impl Into<String>, delay: Duration, message: M) {
        let ms = delay
            .as_millis()
            .clamp(MIN_TIMER_MS as u128, u32::MAX as u128) as u32;
        let key = key.into();
        self.after.retain(|(waiting, _, _)| *waiting != key);
        self.after.push((key, ms, message));
    }

    /// The message [`Context::after`] waits to send under `key`, not sent.
    pub fn cancel(&mut self, key: impl Into<String>) {
        let key = key.into();
        self.after.retain(|(waiting, _, _)| *waiting != key);
        self.cancelled.push(key);
    }

    /// The keyboard goes to the text input, search or text area of this `id`
    /// once the page is drawn: the note just added, the field that was wrong.
    pub fn focus(&mut self, id: impl Into<String>) {
        self.operations.push(Operation::Focus { id: id.into() });
    }

    /// The long log of this `id` shows its line `line`, from 0, with a few
    /// lines above it: the first error, the next one.
    pub fn scroll_to_line(&mut self, id: impl Into<String>, line: u32) {
        self.operations.push(Operation::ScrollTo {
            id: id.into(),
            line: Some(line),
            y: None,
            end: false,
        });
    }

    /// The `scroll` of this `id` shows what is `y` logical pixels from its top.
    pub fn scroll_to(&mut self, id: impl Into<String>, y: f32) {
        self.operations.push(Operation::ScrollTo {
            id: id.into(),
            line: None,
            y: Some(y),
            end: false,
        });
    }

    /// The `scroll` or log of this `id` shows its end.
    pub fn scroll_to_end(&mut self, id: impl Into<String>) {
        self.operations.push(Operation::ScrollTo {
            id: id.into(),
            line: None,
            y: None,
            end: true,
        });
    }

    /// The cursor of the text area of this `id` goes to `line` and `column`,
    /// both from 1; one with line numbers scrolls to it.
    pub fn cursor_to(&mut self, id: impl Into<String>, line: u32, column: u32) {
        self.operations.push(Operation::CursorTo {
            id: id.into(),
            line: line.max(1),
            column: column.max(1),
        });
    }

    fn push(
        &mut self,
        method: &str,
        params: Value,
        pending: impl FnOnce(Result<Value, KitError>) -> Option<M> + 'static,
    ) {
        let id = self.next_id;
        self.next_id += 1;
        self.calls.push((
            Call {
                id,
                method: method.to_owned(),
                params,
            },
            Box::new(pending),
        ));
    }
}

fn decode<R: Request>(answer: Result<Value, KitError>) -> Result<R::Output, KitError> {
    let value = answer?;
    serde_json::from_value(value).map_err(|error| {
        KitError::new(
            KitError::FAILED,
            format!("Hangar's answer to {} did not read: {error}", R::METHOD),
        )
    })
}
