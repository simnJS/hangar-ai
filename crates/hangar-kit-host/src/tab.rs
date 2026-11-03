//! One open tab: its own instance of the kit's module, its own memory, its
//! own calls waiting, and the last view it drew.
//!
//! A tab is driven from the app's `update`: each input goes to the module
//! (`hangar_update`), the calls it answers with go through the gate and to
//! the app's [`KitApi`], and the page is asked for again (`hangar_view`).
//! Every call into the module has a deadline. Past it, or past its memory,
//! or on a panic, the instance is dropped and the tab shows why, with a
//! button to start it again: Hangar never restarts a tab by itself, so a
//! kit that fails at once does not fail in a loop.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use wasmtime::{Caller, Extern, Linker, Memory, ResourceLimiter, Store, Trap, TypedFunc};

use hangar_kit_ui::abi::{
    self, ABI_VERSION, CallId, HangarEvent, Input, KitInfo, LogLevel, MAX_LOG_LINE_BYTES,
    MAX_MESSAGE_BYTES, MAX_TIMERS, MIN_TIMER_MS, Operation, Output, Reply, ShortcutPressed, Start,
    TabInfo, Timer, TimerFired, View, export, import,
};
use hangar_kit_ui::api::{self, Caller as TabCaller, KitError, Theme, limits};
use hangar_kit_ui::{EventValue, ImageSource, Range, UiEvent};

use crate::api::{CallFuture, HostCall, KitApi, ready};
use crate::gate::Gate;
use crate::message::{KitMessage, Local};
use crate::render::{self, WidgetState};
use crate::runtime::{KitModule, KitRuntime, LoadError, TICK};

/// Who the tab is, from the app's record: never from the kit.
#[derive(Debug, Clone, PartialEq)]
pub struct TabStart {
    /// Hangar's version.
    pub hangar: String,
    pub kit: KitInfo,
    pub tab: TabInfo,
    pub theme: Theme,
    pub visible: bool,
}

/// Why a tab stopped, in words for the person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailureKind {
    /// The module did not load, or speaks another ABI.
    Load,
    /// A call took longer than its deadline.
    Timeout,
    /// It wanted more memory than a tab may have.
    Memory,
    /// The kit's code panicked; its message is the reason.
    Panic,
    /// Any other trap.
    Trap,
    /// It answered something Hangar cannot read.
    Message,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub kind: FailureKind,
    /// What happened, in the interface's voice.
    pub reason: String,
}

impl Failure {
    fn new(kind: FailureKind, reason: impl Into<String>) -> Self {
        Failure {
            kind,
            reason: reason.into(),
        }
    }

    /// The title the tab shows in its place.
    pub fn title(&self) -> &'static str {
        match self.kind {
            FailureKind::Load => "This tab could not start",
            _ => "This tab stopped",
        }
    }
}

impl From<LoadError> for Failure {
    fn from(error: LoadError) -> Self {
        Failure::new(FailureKind::Load, error.to_string())
    }
}

/// A line the kit logged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub level: LogLevel,
    pub text: String,
    /// Milliseconds since the Unix epoch.
    pub at: i64,
}

/// A call let through, or refused, for the app to answer: run `future`,
/// then give its result to [`KitTab::reply`] with `id`. A refusal is a
/// future already answered, so every call comes back the same way.
pub struct Dispatch {
    pub id: CallId,
    pub future: CallFuture,
}

/// What an update leaves for the app to do.
#[derive(Default)]
pub struct Effects {
    pub calls: Vec<Dispatch>,
    /// What the kit asked to be done to its page once drawn: a field
    /// focused, a part scrolled. Done by [`Effects::task`].
    pub(crate) operations: Vec<PageOperation>,
}

/// An [`Operation`] of the kit, as iced does it: on a widget of this tab's
/// own ids ([`render::widget_id`]).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PageOperation {
    Focus(String),
    ScrollTo(String, f32),
    ScrollToEnd(String),
}

impl Effects {
    pub fn is_empty(&self) -> bool {
        self.calls.is_empty() && self.operations.is_empty()
    }

    /// What [`Effects::task`] will do to the page, in words: `focus <id>`,
    /// `scroll <id> <y>`, `end <id>`, by the ids iced knows the widgets by.
    /// For a log, and a test.
    pub fn operations(&self) -> Vec<String> {
        self.operations
            .iter()
            .map(|done| match done {
                PageOperation::Focus(id) => format!("focus {id}"),
                PageOperation::ScrollTo(id, y) => format!("scroll {id} {y}"),
                PageOperation::ScrollToEnd(id) => format!("end {id}"),
            })
            .collect()
    }

    /// The calls as iced tasks, each answering [`KitMessage::Reply`], and
    /// what is done to the page.
    pub fn task(self) -> iced::Task<KitMessage> {
        use iced::widget::operation;
        let calls = self.calls.into_iter().map(|Dispatch { id, future }| {
            iced::Task::perform(future, move |answer| KitMessage::Reply(id, answer))
        });
        let operations = self.operations.into_iter().map(|done| match done {
            PageOperation::Focus(id) => operation::focus(iced::widget::Id::from(id)),
            PageOperation::ScrollTo(id, y) => operation::scroll_to(
                iced::widget::Id::from(id),
                operation::AbsoluteOffset {
                    x: None,
                    y: Some(y.max(0.0)),
                },
            ),
            PageOperation::ScrollToEnd(id) => operation::snap_to_end(iced::widget::Id::from(id)),
        });
        iced::Task::batch(calls.chain(operations))
    }
}

/// The pictures a tab holds: what the person handed it from outside the
/// workspace (`files.pick`) or from the clipboard (`ui.pasteImage`), by the
/// ids it was told, for as long as its instance runs. The oldest goes for a
/// new one past [`limits::HELD_IMAGES`].
#[derive(Default)]
pub(crate) struct Held {
    pictures: VecDeque<(String, Arc<Vec<u8>>)>,
    next: u64,
}

impl Held {
    pub(crate) fn hold(&mut self, bytes: Vec<u8>) -> String {
        self.next += 1;
        let id = format!("held-{}", self.next);
        self.pictures.push_back((id.clone(), Arc::new(bytes)));
        while self.pictures.len() > limits::HELD_IMAGES {
            self.pictures.pop_front();
        }
        id
    }

    pub(crate) fn get(&self, id: &str) -> Option<Arc<Vec<u8>>> {
        self.pictures
            .iter()
            .find(|(held, _)| held == id)
            .map(|(_, bytes)| bytes.clone())
    }
}

/// Each tab's own namespace for the ids of its widgets, so that two tabs of
/// a window, of one kit or not, never name the same field.
static NEXT_TAB: AtomicU64 = AtomicU64::new(1);

/// The person's time zone now, as minutes east of UTC.
fn utc_offset_now() -> i32 {
    chrono::Local::now().offset().local_minus_utc() / 60
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis() as i64)
        .unwrap_or(0)
}

/// Holds an instance's memory to the tab's, and says when it was refused,
/// which a Rust module answers with a trap.
struct Limiter {
    memory_bytes: usize,
    refused: Option<usize>,
}

impl ResourceLimiter for Limiter {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        if desired > self.memory_bytes {
            self.refused = Some(desired);
            return Ok(false);
        }
        Ok(true)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        Ok(desired <= 100_000)
    }

    fn instances(&self) -> usize {
        1
    }

    fn tables(&self) -> usize {
        4
    }

    fn memories(&self) -> usize {
        1
    }
}

struct GuestState {
    limiter: Limiter,
    logs: Vec<LogEntry>,
    /// The last `panic` line, the reason of the trap that follows it.
    panic: Option<String>,
}

struct Guest {
    store: Store<GuestState>,
    memory: Memory,
    alloc: TypedFunc<u32, u32>,
    init: TypedFunc<(u32, u32), u64>,
    update: TypedFunc<(u32, u32), u64>,
    view: TypedFunc<(), u64>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Entry {
    Init,
    Update,
    View,
}

impl Entry {
    fn during(self) -> &'static str {
        match self {
            Entry::Init => "while starting",
            Entry::Update => "answering an event",
            Entry::View => "drawing its page",
        }
    }
}

fn ticks(deadline: Duration) -> u64 {
    (deadline.as_millis() as u64)
        .div_ceil(TICK.as_millis() as u64)
        .max(1)
        + 1
}

pub struct KitTab {
    runtime: KitRuntime,
    module: KitModule,
    start: TabStart,
    api: Arc<dyn KitApi>,
    guest: Option<Guest>,
    failure: Option<Failure>,
    view: Option<View>,
    subscriptions: abi::Subscriptions,
    gate: Gate,
    pub(crate) widgets: WidgetState,
    logs: VecDeque<LogEntry>,
    /// What its widgets' ids are made of.
    pub(crate) ns: u64,
    pub(crate) held: Arc<Mutex<Held>>,
}

impl KitTab {
    /// Instantiates the module for this tab and starts it: `hangar_init`,
    /// then `hangar_view`. A failure is a state of the tab, not an error:
    /// it draws itself saying why.
    pub fn start(
        runtime: &KitRuntime,
        module: &KitModule,
        start: TabStart,
        api: Arc<dyn KitApi>,
    ) -> (KitTab, Effects) {
        let mut tab = KitTab {
            runtime: runtime.clone(),
            module: module.clone(),
            start,
            api,
            guest: None,
            failure: None,
            view: None,
            subscriptions: abi::Subscriptions::default(),
            gate: Gate::new(),
            widgets: WidgetState::default(),
            logs: VecDeque::new(),
            ns: NEXT_TAB.fetch_add(1, Ordering::Relaxed),
            held: Arc::default(),
        };
        let effects = tab.boot();
        (tab, effects)
    }

    /// The id iced knows the kit's widget `id` by in this tab: what
    /// [`Effects::operations`] names, and what an app focuses it with.
    pub fn widget_id(&self, id: &str) -> String {
        render::widget_id(self.ns, id)
    }

    /// The shortcuts the kit listens for now, by their specs.
    pub fn shortcuts(&self) -> &[String] {
        if self.failure.is_some() {
            &[]
        } else {
            &self.subscriptions.shortcuts
        }
    }

    /// Why the tab stopped, when it did.
    pub fn failure(&self) -> Option<&Failure> {
        self.failure.as_ref()
    }

    /// The last page the kit drew. Kept after a failure, for nothing but
    /// what the app wants to do with it: the tab draws the failure.
    pub fn view(&self) -> Option<&View> {
        self.view.as_ref()
    }

    /// The timers it runs, for the app's subscription
    /// ([`KitTab::subscription`] makes it).
    pub fn timers(&self) -> &[Timer] {
        if self.failure.is_some() {
            &[]
        } else {
            &self.subscriptions.timers
        }
    }

    /// What the kit logged, oldest first.
    pub fn logs(&self) -> impl Iterator<Item = &LogEntry> {
        self.logs.iter()
    }

    pub fn start_info(&self) -> &TabStart {
        &self.start
    }

    /// Hands the tab what its drawing sent.
    pub fn update(&mut self, message: KitMessage) -> Effects {
        match message {
            KitMessage::Ui(event) => {
                // Whatever the person did in the tab, a line of the open menu
                // among others, the menu has done its part.
                self.widgets.menu = None;
                self.send(&Input::Ui(event), true)
            }
            KitMessage::Local(local) => self.local(local),
            KitMessage::Reply(id, answer) => self.reply(id, answer),
            KitMessage::Timer(id) => self.timer(&id),
            KitMessage::Reload => self.reload(),
            KitMessage::Shortcut(keys) => self.shortcut(keys),
        }
    }

    /// A shortcut pressed in the tab: the kit's when it listens for it, a
    /// keystroke of the person's.
    pub fn shortcut(&mut self, keys: String) -> Effects {
        if !self.subscriptions.shortcuts.contains(&keys) {
            return Effects::default();
        }
        self.widgets.menu = None;
        self.send(&Input::Shortcut(ShortcutPressed { keys }), true)
    }

    /// A call answered.
    pub fn reply(&mut self, id: CallId, answer: Result<Value, KitError>) -> Effects {
        self.gate.settle(id);
        self.send(
            &Input::Reply(Reply {
                id,
                outcome: answer.into(),
            }),
            false,
        )
    }

    /// A timer came round; one the kit no longer runs is dropped.
    pub fn timer(&mut self, id: &str) -> Effects {
        if !self.subscriptions.timers.iter().any(|timer| timer.id == id) {
            return Effects::default();
        }
        self.send(&Input::Timer(TimerFired { id: id.to_owned() }), false)
    }

    /// One of Hangar's events, delivered when the kit subscribed to it and
    /// holds what it needs (read afresh), dropped otherwise, silently.
    pub fn event(&mut self, name: &str, data: Value) -> Effects {
        if !self.subscriptions.events.iter().any(|event| event == name) {
            return Effects::default();
        }
        if !api::may_hear(name, &self.api.granted()) {
            return Effects::default();
        }
        self.send(
            &Input::Event(HangarEvent {
                name: name.to_owned(),
                data,
            }),
            false,
        )
    }

    /// The tab came to the front, or went behind another.
    pub fn set_visible(&mut self, visible: bool) -> Effects {
        if self.start.visible == visible {
            return Effects::default();
        }
        self.start.visible = visible;
        self.event(api::event::TAB_VISIBILITY, json!({ "visible": visible }))
    }

    /// The scheme or the language changed.
    pub fn set_theme(&mut self, theme: Theme) -> Effects {
        if self.start.theme == theme {
            return Effects::default();
        }
        let data = serde_json::to_value(&theme).unwrap_or(Value::Null);
        self.start.theme = theme;
        self.event(api::event::THEME, data)
    }

    /// The tab was asked for again: a singleton's new params, or another
    /// caller. The kit hears `tab.params`; a restart sees these.
    pub fn set_params(
        &mut self,
        params: std::collections::BTreeMap<String, String>,
        caller: Option<TabCaller>,
    ) -> Effects {
        self.start.tab.params = params.clone();
        self.start.tab.caller = caller.clone();
        let data =
            serde_json::to_value(api::TabParamsAnswer { params, caller }).unwrap_or(Value::Null);
        self.event(api::event::TAB_PARAMS, data)
    }

    /// Starts the tab again, from a new instance: what the person asks for
    /// after a failure, or from the tab's bar.
    pub fn reload(&mut self) -> Effects {
        self.guest = None;
        self.boot()
    }

    /// The tab's timers as an iced subscription. Several tabs: give each its
    /// own identity, `tab.subscription().with(key)`, or two tabs with a timer
    /// of the same id and period would share one.
    pub fn subscription(&self) -> iced::Subscription<KitMessage> {
        iced::Subscription::batch(self.timers().iter().map(|timer| {
            iced::time::every(Duration::from_millis(timer.every_ms as u64))
                .with(timer.id.clone())
                .map(|(id, _)| KitMessage::Timer(id))
        }))
    }

    fn boot(&mut self) -> Effects {
        self.gate.clear();
        self.failure = None;
        self.view = None;
        self.subscriptions = abi::Subscriptions::default();
        *self
            .held
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Held::default();
        match self.instantiate() {
            Ok(guest) => self.guest = Some(guest),
            Err(failure) => {
                self.fail(failure);
                return Effects::default();
            }
        }
        let start = Start {
            abi: ABI_VERSION,
            hangar: self.start.hangar.clone(),
            kit: self.start.kit.clone(),
            tab: self.start.tab.clone(),
            theme: self.start.theme.clone(),
            permissions: self.api.granted(),
            visible: self.start.visible,
            utc_offset: utc_offset_now(),
        };
        match self.exchange::<_, Output>(Entry::Init, Some(&start)) {
            Some(output) => self.after(output, false),
            None => Effects::default(),
        }
    }

    fn instantiate(&mut self) -> Result<Guest, Failure> {
        let limits = &self.runtime.limits;
        let engine = &self.runtime.engine;
        let state = GuestState {
            limiter: Limiter {
                memory_bytes: limits.memory_bytes,
                refused: None,
            },
            logs: Vec::new(),
            panic: None,
        };
        let mut store = Store::new(engine, state);
        store.limiter(|state| &mut state.limiter);
        store.epoch_deadline_trap();
        store.set_epoch_deadline(ticks(limits.init_deadline));

        let mut linker: Linker<GuestState> = Linker::new(engine);
        linker
            .func_wrap(
                import::MODULE,
                import::LOG,
                |mut caller: Caller<'_, GuestState>, level: u32, ptr: u32, len: u32| {
                    let Some(Extern::Memory(memory)) = caller.get_export(export::MEMORY) else {
                        return;
                    };
                    let len = (len as usize).min(MAX_LOG_LINE_BYTES);
                    let mut bytes = vec![0; len];
                    if memory.read(&caller, ptr as usize, &mut bytes).is_err() {
                        return;
                    }
                    let level = LogLevel::from_u32(level);
                    let text = String::from_utf8_lossy(&bytes).into_owned();
                    let state = caller.data_mut();
                    if level == LogLevel::Panic {
                        state.panic = Some(text.clone());
                    }
                    state.logs.push(LogEntry {
                        level,
                        text,
                        at: now_ms(),
                    });
                },
            )
            .and_then(|linker| linker.func_wrap(import::MODULE, import::NOW, now_ms))
            .map_err(|why| Failure::new(FailureKind::Load, why.to_string()))?;

        let instance = linker
            .instantiate(&mut store, &self.module.module)
            .map_err(|error| classify(error, store.data(), limits, Entry::Init))?;
        let missing = |name: &str| {
            Failure::from(LoadError::Export {
                name: leak(name),
                why: "it is missing".into(),
            })
        };
        let memory = instance
            .get_memory(&mut store, export::MEMORY)
            .ok_or_else(|| missing(export::MEMORY))?;
        let typed = |store: &mut Store<GuestState>, name: &'static str| {
            instance
                .get_func(&mut *store, name)
                .ok_or_else(|| missing(name))
        };
        let abi: TypedFunc<(), u32> = typed(&mut store, export::ABI)?
            .typed(&store)
            .map_err(|why| load(export::ABI, why))?;
        let version = abi
            .call(&mut store, ())
            .map_err(|error| classify(error, store.data(), limits, Entry::Init))?;
        if version != ABI_VERSION {
            return Err(LoadError::Abi {
                module: version,
                host: ABI_VERSION,
            }
            .into());
        }
        Ok(Guest {
            alloc: typed(&mut store, export::ALLOC)?
                .typed(&store)
                .map_err(|why| load(export::ALLOC, why))?,
            init: typed(&mut store, export::INIT)?
                .typed(&store)
                .map_err(|why| load(export::INIT, why))?,
            update: typed(&mut store, export::UPDATE)?
                .typed(&store)
                .map_err(|why| load(export::UPDATE, why))?,
            view: typed(&mut store, export::VIEW)?
                .typed(&store)
                .map_err(|why| load(export::VIEW, why))?,
            memory,
            store,
        })
    }

    /// Writes `input`, calls `entry`, reads its answer as `T`. A failure of
    /// any of it stops the tab, and answers none.
    fn exchange<I: Serialize, T: DeserializeOwned>(
        &mut self,
        entry: Entry,
        input: Option<&I>,
    ) -> Option<T> {
        let bytes = match input.map(serde_json::to_vec) {
            Some(Ok(bytes)) => Some(bytes),
            Some(Err(why)) => {
                self.fail(Failure::new(
                    FailureKind::Message,
                    format!("Hangar could not write its message: {why}"),
                ));
                return None;
            }
            None => None,
        };
        let deadline = match entry {
            Entry::Init => self.runtime.limits.init_deadline,
            _ => self.runtime.limits.call_deadline,
        };
        let guest = self.guest.as_mut()?;
        let answer = invoke(guest, entry, bytes.as_deref(), deadline);
        let state = guest.store.data_mut();
        let logged = std::mem::take(&mut state.logs);
        for entry in logged {
            self.logs.push_back(entry);
        }
        while self.logs.len() > self.runtime.limits.log_lines {
            self.logs.pop_front();
        }
        let answer = match answer {
            Ok(answer) => answer,
            Err(Fault::Wasm(error)) => {
                let failure = classify(
                    error,
                    self.guest.as_ref()?.store.data(),
                    &self.runtime.limits,
                    entry,
                );
                self.fail(failure);
                return None;
            }
            Err(Fault::Message(why)) => {
                self.fail(Failure::new(FailureKind::Message, why));
                return None;
            }
        };
        match serde_json::from_slice(&answer) {
            Ok(value) => Some(value),
            Err(why) => {
                let what = if entry == Entry::View {
                    "a page"
                } else {
                    "an answer"
                };
                self.fail(Failure::new(
                    FailureKind::Message,
                    format!("it sent {what} Hangar cannot read: {why}"),
                ));
                None
            }
        }
    }

    fn send(&mut self, input: &Input, gesture: bool) -> Effects {
        if self.guest.is_none() {
            return Effects::default();
        }
        match self.exchange::<_, Output>(Entry::Update, Some(input)) {
            Some(output) => self.after(output, gesture),
            None => Effects::default(),
        }
    }

    /// The calls of an answer through the gate, its subscriptions kept, and
    /// the page asked for again.
    fn after(&mut self, output: Output, gesture: bool) -> Effects {
        if let Some(mut subscriptions) = output.subscriptions {
            subscriptions.timers.truncate(MAX_TIMERS);
            for timer in &mut subscriptions.timers {
                timer.every_ms = timer.every_ms.max(MIN_TIMER_MS);
            }
            self.subscriptions = subscriptions;
        }
        let mut effects = Effects::default();
        if !output.calls.is_empty() {
            let granted = self.api.granted();
            let now = Instant::now();
            for call in output.calls {
                let api = &self.api;
                let admitted = self
                    .gate
                    .admit(&call, &granted, gesture, now, |call| api.check(call));
                let future = match admitted {
                    Ok(HostCall::FilesPick(request)) => self.pick(request),
                    Ok(HostCall::PasteImage) => self.paste(),
                    Ok(HostCall::CanvasExport(export)) => self.export(*export),
                    Ok(HostCall::ImageSize(source)) => ready(self.image_size(&source)),
                    Ok(host_call) => self.api.call(host_call),
                    Err(refusal) => ready(Err(refusal)),
                };
                effects.calls.push(Dispatch {
                    id: call.id,
                    future,
                });
            }
        }
        if let Some(view) = self.exchange::<(), View>(Entry::View, None) {
            let granted = self.api.granted();
            let held = self
                .held
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            self.widgets.sync(&view, &*self.api, &granted, &held);
            drop(held);
            self.view = Some(view);
        }
        if self.failure.is_some() {
            // The calls of an instance that is gone are answered to no one.
            effects.calls.clear();
        } else {
            effects.operations = self.operations(output.operations);
        }
        effects
    }

    /// The kit's operations, on the page just drawn: each names a widget by
    /// its id, and does what that widget takes; an id the page does not have
    /// does nothing.
    fn operations(&mut self, asked: Vec<Operation>) -> Vec<PageOperation> {
        let Some(view) = &self.view else {
            return Vec::new();
        };
        let ns = self.ns;
        let mut done = Vec::new();
        for operation in asked {
            match operation {
                Operation::Focus { id } => {
                    if render::find_field(&view.root, &id) {
                        done.push(PageOperation::Focus(render::widget_id(ns, &id)));
                    }
                }
                Operation::ScrollTo { id, line, y, end } => {
                    let Some(target) = render::find_scroller(&view.root, &id) else {
                        continue;
                    };
                    let scroller = render::widget_id(ns, &id);
                    match (end, line, y, target) {
                        (true, ..) => done.push(PageOperation::ScrollToEnd(scroller)),
                        (false, Some(line), _, render::Scroller::Log) => done.push(
                            PageOperation::ScrollTo(scroller, render::log_line_top(line)),
                        ),
                        (false, _, Some(y), _) => done.push(PageOperation::ScrollTo(scroller, y)),
                        _ => {}
                    }
                }
                Operation::CursorTo { id, line, column } => {
                    let Some(numbered) = render::find_area(&view.root, &id) else {
                        continue;
                    };
                    if let Some(editor) = self.widgets.editors.get_mut(&id) {
                        let last = editor.content.line_count().saturating_sub(1);
                        let line = (line.max(1) as usize - 1).min(last);
                        editor.content.move_to(iced::widget::text_editor::Cursor {
                            position: iced::widget::text_editor::Position {
                                line,
                                column: column.max(1) as usize - 1,
                            },
                            selection: None,
                        });
                        done.push(PageOperation::Focus(render::widget_id(ns, &id)));
                        if numbered {
                            done.push(PageOperation::ScrollTo(
                                render::area_scroller_id(ns, &id),
                                render::editor_line_top(line as u32),
                            ));
                        }
                    }
                }
                Operation::Unsupported => {}
            }
        }
        done
    }

    /// `files.pick`: the app's chooser, then the file by its path in the
    /// workspace, or held as a picture when it is outside.
    fn pick(&self, request: api::FilesPick) -> CallFuture {
        let chosen = self.api.pick_file(&request);
        let held = self.held.clone();
        Box::pin(async move {
            let Some(file) = chosen.await? else {
                return Ok(Value::Null);
            };
            match (file.workspace_path, file.bytes) {
                (Some(path), _) => {
                    Ok(json!({ "name": file.name, "size": file.size, "path": path }))
                }
                (None, Some(bytes)) if bytes.len() > limits::BINARY_MAX_BYTES => {
                    Err(KitError::new(
                        KitError::INVALID,
                        format!("{} is larger than 10 MB", file.name),
                    ))
                }
                (None, Some(bytes)) => {
                    let id = held
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .hold(bytes);
                    Ok(json!({ "name": file.name, "size": file.size, "held": id }))
                }
                (None, None) => Err(KitError::new(
                    KitError::FAILED,
                    format!("{} could not be read", file.name),
                )),
            }
        })
    }

    /// `ui.pasteImage`: the clipboard's picture, held as a PNG.
    fn paste(&self) -> CallFuture {
        let pasted = self.api.clipboard_image();
        let held = self.held.clone();
        Box::pin(async move {
            let Some(image) = pasted.await? else {
                return Err(KitError::new(
                    KitError::NOT_FOUND,
                    "the clipboard holds no picture",
                ));
            };
            let png = crate::paint::encode_rgba(image.width, image.height, &image.rgba)
                .map_err(|why| KitError::new(KitError::FAILED, why))?;
            if png.len() > limits::BINARY_MAX_BYTES {
                return Err(KitError::new(
                    KitError::INVALID,
                    "the picture on the clipboard is larger than 10 MB",
                ));
            }
            let id = held
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .hold(png);
            Ok(json!({ "held": id, "width": image.width, "height": image.height }))
        })
    }

    /// `image.size`: a picture's size from its header, read as a view's
    /// image is read (a file read, on the window's thread).
    fn image_size(&self, source: &ImageSource) -> Result<Value, KitError> {
        let bytes = match source {
            ImageSource::Held(id) => self
                .held
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(id)
                .map(|bytes| (*bytes).clone())
                .ok_or_else(|| {
                    KitError::new(
                        KitError::NOT_FOUND,
                        format!("this tab holds no picture {id}"),
                    )
                })?,
            other => self.api.image(other)?,
        };
        let (width, height) = image::ImageReader::new(std::io::Cursor::new(&bytes))
            .with_guessed_format()
            .ok()
            .and_then(|reader| reader.into_dimensions().ok())
            .ok_or_else(|| {
                KitError::new(
                    KitError::UNSUPPORTED,
                    "not a raster image Hangar reads: PNG, JPEG, GIF, WebP or BMP",
                )
            })?;
        Ok(json!({ "width": width, "height": height }))
    }

    /// `canvas.export`: the picture drawn on a thread of its own (a large
    /// capture takes a moment), then written by the app as a new image of
    /// the workspace, as `files.writeBinary` writes one.
    fn export(&self, export: api::CanvasExport) -> CallFuture {
        let background = match &export.background {
            None => Ok(None),
            Some(ImageSource::Held(id)) => self
                .held
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(id)
                .map(|bytes| Some((*bytes).clone()))
                .ok_or_else(|| {
                    KitError::new(
                        KitError::NOT_FOUND,
                        format!("this tab holds no picture {id}: a restart lets go of them"),
                    )
                }),
            Some(source) => self.api.image(source).map(Some),
        };
        let api = self.api.clone();
        Box::pin(async move {
            let background = background?;
            let (sent, drawn) = iced::futures::channel::oneshot::channel();
            let api::CanvasExport {
                path,
                width,
                height,
                marks,
                stroke,
                ..
            } = export;
            std::thread::spawn(move || {
                let stroke = stroke.unwrap_or_else(|| hangar_kit_ui::default_stroke(width, height));
                let _ = sent.send(crate::paint::render_png(
                    width,
                    height,
                    background.as_deref(),
                    &marks,
                    stroke,
                    limits::BINARY_MAX_BYTES,
                ));
            });
            let rendered = drawn
                .await
                .map_err(|_| KitError::new(KitError::FAILED, "the picture was not drawn"))?
                .map_err(|why| KitError::new(KitError::FAILED, why))?;
            use base64::Engine;
            let base64 = base64::engine::general_purpose::STANDARD.encode(&rendered.png);
            api.call(HostCall::FilesWriteBinary {
                path: path.clone(),
                base64,
            })
            .await?;
            Ok(json!({
                "path": path,
                "width": rendered.width,
                "height": rendered.height,
                "bytes": rendered.png.len(),
                "scale": rendered.scale,
            }))
        })
    }

    /// A long log scrolled: kept, and when it is given in parts and the
    /// lines now in view are not all given, the kit is asked for them (once
    /// for a range).
    fn scrolled(&mut self, log: String, top: f32, height: f32) -> Effects {
        self.widgets.scrolls.insert(log.clone(), (top, height));
        let Some(view) = &self.view else {
            return Effects::default();
        };
        let Some((wanted, handler)) = render::range_wanted(&view.root, &log, top, height) else {
            return Effects::default();
        };
        if self.widgets.asked.get(&log) == Some(&wanted) {
            return Effects::default();
        }
        self.widgets.asked.insert(log, wanted);
        let event = UiEvent {
            view: view.seq,
            handler,
            value: Some(EventValue::Range(Range {
                start: wanted.start,
                end: wanted.end,
            })),
        };
        self.send(&Input::Ui(event), false)
    }

    fn local(&mut self, local: Local) -> Effects {
        match local {
            Local::Sort { table, sort } => {
                self.widgets.sorts.insert(table, sort);
                Effects::default()
            }
            Local::Toggle { tree, node, open } => {
                self.widgets.toggled.insert((tree, node), open);
                Effects::default()
            }
            Local::Menu(menu) => {
                self.widgets.menu = menu;
                Effects::default()
            }
            Local::MenuOpened { key, view, handler } => {
                let effects = self.send(
                    &Input::Ui(UiEvent {
                        view,
                        handler,
                        value: None,
                    }),
                    true,
                );
                self.widgets.menu = Some(key);
                effects
            }
            Local::Scrolled { log, top, height } => self.scrolled(log, top, height),
            Local::Edit {
                area,
                action,
                view,
                handler,
            } => {
                let edit = action.is_edit();
                let Some(editor) = self.widgets.editors.get_mut(&area) else {
                    return Effects::default();
                };
                match (edit, handler) {
                    // A text area the kit gave no handler is read-only: the
                    // cursor moves and selects, nothing is typed.
                    (true, None) => Effects::default(),
                    (true, Some(handler)) => {
                        editor.content.perform(action);
                        let text = editor.content.text();
                        let event = UiEvent {
                            view,
                            handler,
                            value: Some(EventValue::Text(text)),
                        };
                        self.send(&Input::Ui(event), true)
                    }
                    (false, _) => {
                        editor.content.perform(action);
                        Effects::default()
                    }
                }
            }
        }
    }

    fn fail(&mut self, failure: Failure) {
        self.guest = None;
        self.gate.clear();
        self.failure = Some(failure);
    }
}

fn leak(name: &str) -> &'static str {
    match name {
        export::ABI => export::ABI,
        export::ALLOC => export::ALLOC,
        export::INIT => export::INIT,
        export::UPDATE => export::UPDATE,
        export::VIEW => export::VIEW,
        _ => export::MEMORY,
    }
}

fn load(name: &'static str, why: wasmtime::Error) -> Failure {
    LoadError::Export {
        name,
        why: why.to_string(),
    }
    .into()
}

enum Fault {
    Wasm(wasmtime::Error),
    Message(String),
}

/// One call into the module, its answer copied out.
fn invoke(
    guest: &mut Guest,
    entry: Entry,
    input: Option<&[u8]>,
    deadline: Duration,
) -> Result<Vec<u8>, Fault> {
    let state = guest.store.data_mut();
    state.limiter.refused = None;
    state.panic = None;
    guest.store.set_epoch_deadline(ticks(deadline));
    let packed = match entry {
        Entry::View => guest.view.call(&mut guest.store, ()).map_err(Fault::Wasm)?,
        Entry::Init | Entry::Update => {
            let input = input.unwrap_or(b"{}");
            if input.len() > MAX_MESSAGE_BYTES {
                return Err(Fault::Message(
                    "Hangar's message to it was larger than 4 MB".into(),
                ));
            }
            let len = input.len() as u32;
            let ptr = guest
                .alloc
                .call(&mut guest.store, len)
                .map_err(Fault::Wasm)?;
            guest
                .memory
                .write(&mut guest.store, ptr as usize, input)
                .map_err(|_| Fault::Message("it gave Hangar a buffer outside its memory".into()))?;
            let function = if entry == Entry::Init {
                &guest.init
            } else {
                &guest.update
            };
            function
                .call(&mut guest.store, (ptr, len))
                .map_err(Fault::Wasm)?
        }
    };
    let (ptr, len) = abi::unpack(packed);
    if len as usize > MAX_MESSAGE_BYTES {
        return Err(Fault::Message(format!(
            "it answered {} MB, more than the 4 MB a message may hold",
            len >> 20
        )));
    }
    let mut answer = vec![0; len as usize];
    guest
        .memory
        .read(&guest.store, ptr as usize, &mut answer)
        .map_err(|_| Fault::Message("it answered with a buffer outside its memory".into()))?;
    Ok(answer)
}

/// Why a call into the module failed, in words: the memory refused comes
/// first, as the trap that follows it says nothing of it, then the panic's
/// own message, then the trap.
fn classify(
    error: wasmtime::Error,
    state: &GuestState,
    limits: &crate::Limits,
    entry: Entry,
) -> Failure {
    let mb = |bytes: usize| bytes.div_ceil(1024 * 1024);
    if let Some(desired) = state.limiter.refused {
        return Failure::new(
            FailureKind::Memory,
            format!(
                "it asked for {} MB of memory {}, more than the {} MB a tab may use",
                mb(desired),
                entry.during(),
                mb(limits.memory_bytes)
            ),
        );
    }
    let deadline = if entry == Entry::Init {
        limits.init_deadline
    } else {
        limits.call_deadline
    };
    match error.downcast_ref::<Trap>() {
        Some(Trap::Interrupt) => Failure::new(
            FailureKind::Timeout,
            format!(
                "it took longer than {} ms {}: a tab must answer at once",
                deadline.as_millis(),
                entry.during()
            ),
        ),
        _ => match &state.panic {
            Some(panic) => Failure::new(
                FailureKind::Panic,
                format!("it panicked {}: {panic}", entry.during()),
            ),
            None => Failure::new(
                FailureKind::Trap,
                format!("it failed {}: {error:#}", entry.during()),
            ),
        },
    }
}
