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
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use wasmtime::{Caller, Extern, Linker, Memory, ResourceLimiter, Store, Trap, TypedFunc};

use hangar_kit_ui::abi::{
    self, ABI_VERSION, CallId, HangarEvent, Input, KitInfo, LogLevel, MAX_LOG_LINE_BYTES,
    MAX_MESSAGE_BYTES, MAX_TIMERS, MIN_TIMER_MS, Output, Reply, Start, TabInfo, Timer, TimerFired,
    View, export, import,
};
use hangar_kit_ui::api::{self, Caller as TabCaller, KitError, Theme};
use hangar_kit_ui::{EventValue, UiEvent};

use crate::api::{CallFuture, KitApi, ready};
use crate::gate::Gate;
use crate::message::{KitMessage, Local};
use crate::render::WidgetState;
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
}

impl Effects {
    pub fn is_empty(&self) -> bool {
        self.calls.is_empty()
    }

    /// The calls as iced tasks, each answering [`KitMessage::Reply`].
    pub fn task(self) -> iced::Task<KitMessage> {
        iced::Task::batch(self.calls.into_iter().map(|Dispatch { id, future }| {
            iced::Task::perform(future, move |answer| KitMessage::Reply(id, answer))
        }))
    }
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
        };
        let effects = tab.boot();
        (tab, effects)
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
            KitMessage::Ui(event) => self.send(&Input::Ui(event), true),
            KitMessage::Local(local) => self.local(local),
            KitMessage::Reply(id, answer) => self.reply(id, answer),
            KitMessage::Timer(id) => self.timer(&id),
            KitMessage::Reload => self.reload(),
        }
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
                let future = match self.gate.admit(&call, &granted, gesture, now) {
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
            self.widgets.sync(&view, &*self.api, &granted);
            self.view = Some(view);
        }
        if self.failure.is_some() {
            // The calls of an instance that is gone are answered to no one.
            effects.calls.clear();
        }
        effects
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
