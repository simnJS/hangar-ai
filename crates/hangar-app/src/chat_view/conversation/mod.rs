//! The conversation of the chat view, the left, main region: the person's
//! chat with the workspace's orchestrator, a Claude Code pane that goes on
//! running hidden behind the view.
//!
//! Everything goes through Hangar Mobile's typed calls (`mobile::desk`),
//! the phone's own path with its guards: `chat` reads a page of the pane's
//! transcript merged with what its mod reports (the question or permission it
//! waits on, the call it runs), `send_text` types a message, `answer` and
//! `permit` answer its dialogs.
//!
//! - `thread`: what is held between reads — the newest page laid over the
//!   end, older pages in front, the messages sent shown at once until the
//!   transcript has them;
//! - `titles`: Hangar's own calls in words ("Created task “…”");
//! - `rich`: Claude's Markdown, its code coloured as the preview colours it;
//! - `view`: the drawing.
//!
//! When it reads: as soon as the region is shown, whenever the pane's mod
//! reports (`CoreEvent::Bridge`) or its queue changes, and on a slow tick
//! that reads again only when the pane's activity changed, while it works
//! (it prints then), or after `FALLBACK` otherwise. The core's chat reads
//! only what was appended to the transcript since, nothing when it did not
//! change, and one read is out at a time (another asked meanwhile follows
//! it), so this is never a read per frame.
//!
//! What it costs between reads: the window is laid out again after every
//! message the app handles, many a second while the panes print behind the
//! view. The messages keep their layout until what they show changes
//! (`settled`, keyed by `Chat::revision`), and an unchanged message keeps
//! the Markdown it was parsed into (`Chat::prose`).

mod calls;
pub mod rich;
mod settled;
mod thread;
mod titles;
mod view;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use iced::widget::markdown;
use iced::widget::scrollable::Viewport;
use iced::widget::text_editor;
use iced::{Subscription, Task};

use hangar_core::chat::{Answer, Body, Question};

use crate::control::port::{BoxFuture, Port};
use crate::message::{Intent, Message};
use crate::mobile::contract::{ChatPending, RemoteAnswered, RemoteSent};
use crate::mobile::desk::{self, ChatView};
use crate::mobile::engine::Mobile;
use crate::preview::highlight::Lines;
use crate::preview::markdown::highlight_blocks;
use crate::services::{CoreEvent, Services};
use crate::shell::Notice;
use crate::state::roles::orchestrator_of;
use crate::state::{AgentActivity, AgentId, Model, Pane, PaneId, Workspace, WsId};

pub use thread::{Sent, Thread};
pub use view::view;

/// How often the tick looks whether to read again.
const TICK: Duration = Duration::from_secs(2);
/// How often it looks until a first read answered: Hangar Mobile's runtime
/// starts a moment after the window, and the region says « Loading » until
/// then.
const FIRST_TICK: Duration = Duration::from_millis(300);
/// A read at least this often while the region is shown, whatever else.
const FALLBACK: Duration = Duration::from_secs(10);
/// How far from the top a scroll asks for the page before.
const OLDER_MARGIN: f32 = 120.0;
/// How far from the bottom still counts as at the bottom.
const BOTTOM_MARGIN: f32 = 4.0;

/// The region's message, as the app routes it.
pub(super) fn msg(inner: Msg) -> Message {
    Message::ChatView(super::Msg::Conversation(inner))
}

/// What the region keeps between frames: a chat per workspace shown.
#[derive(Debug, Default)]
pub struct State {
    chats: HashMap<WsId, Chat>,
    /// Every desk call asked, for the tests to read.
    #[cfg(test)]
    calls: Vec<(WsId, Call)>,
}

/// A call to `mobile::desk`, as the region asks it.
#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    Read {
        before: Option<String>,
    },
    Send {
        text: String,
    },
    Answer {
        tool_use_id: String,
        answers: Vec<Answer>,
    },
    Permit {
        allow: bool,
        prompt: Option<String>,
    },
}

/// What Hangar Mobile's state says of the pane's input, looked at on each
/// read and tick rather than in the view (it sits behind a lock).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeskState {
    /// The runtime runs: the calls can be made.
    pub ready: bool,
    /// An answer is typed into the pane (the phone's, or this window's).
    pub answering: bool,
    /// The pane was woken and its Claude Code has not said hello yet.
    pub resuming: bool,
}

/// One workspace's conversation with its orchestrator.
#[derive(Debug)]
pub struct Chat {
    /// The orchestrator pane it is of: another pane starts it over.
    pane: PaneId,
    thread: Thread,
    /// A first read answered.
    loaded: bool,
    /// The last read was refused.
    error: Option<String>,
    reading: bool,
    /// A read was asked while one was out.
    again: bool,
    reading_older: bool,
    read_at: Option<Instant>,
    /// The pane's activity at the last tick.
    seen: Option<AgentActivity>,
    desk: DeskState,
    /// Claude's messages parsed, by item id, with the words they were parsed
    /// from.
    prose: HashMap<String, (String, markdown::Content)>,
    /// The colours of the code blocks, by `block_key`.
    code: HashMap<u64, Arc<Lines>>,
    /// The calls shown with their detail, and Hangar's messages shown whole.
    expanded: HashSet<String>,
    draft: text_editor::Content,
    form: Form,
    /// An answer or a permission is on its way.
    deciding: bool,
    /// Why the last answer or permission was refused.
    refusal: Option<String>,
    /// The view is at its bottom: new items keep it there.
    at_bottom: bool,
    /// Changes whenever what the messages show does: the key their layout
    /// is kept under (`settled`). Unique across chats, so a chat started
    /// over never takes another's layout.
    revision: u64,
}

/// A revision no chat had yet.
fn next_revision() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl Chat {
    fn new(pane: PaneId) -> Chat {
        Chat {
            pane,
            thread: Thread::default(),
            loaded: false,
            error: None,
            reading: false,
            again: false,
            reading_older: false,
            read_at: None,
            seen: None,
            desk: DeskState::default(),
            prose: HashMap::new(),
            code: HashMap::new(),
            expanded: HashSet::new(),
            draft: text_editor::Content::new(),
            form: Form::default(),
            deciding: false,
            refusal: None,
            at_bottom: true,
            revision: next_revision(),
        }
    }

    /// What the messages show changed: their layout is made again.
    fn touch(&mut self) {
        self.revision = next_revision();
    }
}

/// The person's choices on the question the pane waits on, one question at
/// a time as Claude Code's own dialog asks them: `step` is the question
/// shown, the last one with the recap and Send.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Form {
    tool_use_id: String,
    /// By question: the options picked, by index.
    picked: Vec<Vec<usize>>,
    /// By question: "Other" is chosen, and its words.
    other_on: Vec<bool>,
    other: Vec<String>,
    /// The question shown.
    step: usize,
}

impl Form {
    fn new(tool_use_id: &str, questions: usize) -> Form {
        Form {
            tool_use_id: tool_use_id.to_string(),
            picked: vec![Vec::new(); questions],
            other_on: vec![false; questions],
            other: vec![String::new(); questions],
            step: 0,
        }
    }

    /// The question shown, within the questions asked.
    pub fn step(&self) -> usize {
        self.step.min(self.picked.len().saturating_sub(1))
    }

    fn pick(&mut self, question: &Question, at: usize, option: usize) {
        let Some(picked) = self.picked.get_mut(at) else {
            return;
        };
        if question.multi_select {
            match picked.iter().position(|&o| o == option) {
                Some(found) => {
                    picked.remove(found);
                }
                None => {
                    picked.push(option);
                    picked.sort_unstable();
                }
            }
        } else {
            *picked = vec![option];
            self.other_on[at] = false;
        }
    }

    fn other(&mut self, question: &Question, at: usize) {
        let Some(on) = self.other_on.get_mut(at) else {
            return;
        };
        if question.multi_select {
            *on = !*on;
        } else {
            *on = true;
            self.picked[at].clear();
        }
    }

    /// Question `at`'s answer, once it has one: the labels picked and the
    /// person's own words.
    fn answer_at(&self, question: &Question, at: usize) -> Option<Answer> {
        let selected: Vec<String> = self
            .picked
            .get(at)?
            .iter()
            .filter_map(|&o| question.options.get(o))
            .map(|option| option.label.clone())
            .collect();
        let other = Some(self.other.get(at)?.trim().to_string())
            .filter(|words| self.other_on[at] && !words.is_empty());
        (!selected.is_empty() || other.is_some()).then_some(Answer { selected, other })
    }

    /// Question `at`'s answer, by index into `questions`.
    pub fn answers_at(&self, questions: &[Question], at: usize) -> Option<Answer> {
        self.answer_at(questions.get(at)?, at)
    }

    /// Question `at` has an answer: Next goes on.
    pub fn answered(&self, questions: &[Question], at: usize) -> bool {
        self.answers_at(questions, at).is_some()
    }

    /// Goes to question `to`: back to any, on only past questions answered.
    fn go(&mut self, questions: &[Question], to: usize) {
        if to >= questions.len() {
            return;
        }
        if (self.step()..to).all(|at| self.answered(questions, at)) {
            self.step = to;
        }
    }

    /// The answers, one per question, once each has one.
    fn answers(&self, questions: &[Question]) -> Option<Vec<Answer>> {
        if self.picked.len() != questions.len() {
            return None;
        }
        questions
            .iter()
            .enumerate()
            .map(|(at, question)| self.answer_at(question, at))
            .collect()
    }
}

/// What the region answers.
#[derive(Debug, Clone)]
pub enum Msg {
    Read {
        ws: WsId,
        pane: PaneId,
        older: bool,
        result: Result<Box<ChatView>, String>,
    },
    Tick,
    LoadOlder(WsId),
    Scrolled(WsId, Viewport),
    Edit(WsId, text_editor::Action),
    Send(WsId),
    Sent {
        ws: WsId,
        at: i64,
        result: Result<RemoteSent, String>,
    },
    /// A call's detail, or a message of Hangar's, shown or folded.
    Toggle(WsId, String),
    Pick {
        ws: WsId,
        question: usize,
        option: usize,
    },
    Other {
        ws: WsId,
        question: usize,
    },
    OtherText {
        ws: WsId,
        question: usize,
        text: String,
    },
    /// Back or Next on the question card: the question shown.
    Step {
        ws: WsId,
        to: usize,
    },
    SubmitAnswer(WsId),
    Permit(WsId, bool),
    Decided {
        ws: WsId,
        result: Result<RemoteAnswered, String>,
    },
    Coloured(WsId, Vec<(u64, Arc<Lines>)>),
    /// « Start the orchestrator » of the empty state.
    Start(WsId),
    Link(String),
}

/// Why the input takes nothing now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocked {
    /// Hangar Mobile's runtime is not up yet.
    Loading,
    NotClaude,
    /// An answer is being typed into the pane.
    Answering,
    /// The pane's Claude Code is resuming.
    Resuming,
    /// A question waits: what is typed would land in its dialog.
    Question,
    /// A permission waits, likewise.
    Permission,
}

/// Why the chat's input is disabled, if it is.
pub fn blocked(chat: &Chat, pane: &Pane) -> Option<Blocked> {
    if pane.agent != AgentId::Claude {
        return Some(Blocked::NotClaude);
    }
    if !chat.desk.ready {
        return Some(Blocked::Loading);
    }
    if chat.deciding || chat.desk.answering {
        return Some(Blocked::Answering);
    }
    if chat.desk.resuming {
        return Some(Blocked::Resuming);
    }
    match chat.thread.pending {
        Some(ChatPending::Question { .. }) => Some(Blocked::Question),
        Some(ChatPending::Permission { .. }) => Some(Blocked::Permission),
        None => None,
    }
}

fn now_ms() -> i64 {
    i64::try_from(crate::control::types::now_ms()).unwrap_or(i64::MAX)
}

/// The port and the runtime the desk calls take, once both are up.
fn desk_of(m: &Model) -> Option<(Port, Arc<Mobile>)> {
    Some((m.live.control.port()?, m.live.mobile.mobile()?))
}

fn desk_state(m: &Model, pane: &str) -> DeskState {
    let Some((_, mobile)) = desk_of(m) else {
        return DeskState::default();
    };
    let book = mobile.book();
    let now = crate::control::types::now_ms();
    DeskState {
        ready: true,
        answering: book.answering.contains(pane),
        resuming: book
            .resuming
            .get(pane)
            .is_some_and(|since| now.saturating_sub(*since) < crate::mobile::ops::RESUME_MS),
    }
}

/// The workspaces shown as the chat view, with their orchestrator.
fn shown(m: &Model) -> impl Iterator<Item = (&Workspace, &Pane)> {
    m.state()
        .workspaces
        .iter()
        .filter(|ws| super::shown(m, ws))
        .filter_map(|ws| Some((ws, orchestrator_of(ws)?)))
}

/// The chat of `ws`, made or started over for its orchestrator as it is now.
fn chat_of<'a>(st: &'a mut State, ws: &str, pane: &str) -> &'a mut Chat {
    let chat = st
        .chats
        .entry(ws.to_string())
        .or_insert_with(|| Chat::new(pane.to_string()));
    if chat.pane != pane {
        *chat = Chat::new(pane.to_string());
    }
    chat
}

/// The orchestrator of `ws` and its chat.
fn found<'a>(st: &'a mut State, m: &Model, ws: &str) -> Option<&'a mut Chat> {
    let pane = orchestrator_of(m.workspace(ws)?)?.id.clone();
    Some(chat_of(st, ws, &pane))
}

impl State {
    /// The chat of `ws` as last read, for the view.
    pub fn chat(&self, ws: &str) -> Option<&Chat> {
        self.chats.get(ws)
    }

    #[cfg(test)]
    fn chat_mut(&mut self, ws: &str, pane: &str) -> &mut Chat {
        chat_of(self, ws, pane)
    }

    /// The desk calls asked so far, oldest first.
    #[cfg(test)]
    fn calls(&self) -> &[(WsId, Call)] {
        &self.calls
    }
}

/// Makes a desk call for `ws`'s pane `pane`, answering with `done`.
fn call<T: Send + 'static>(
    st: &mut State,
    m: &Model,
    ws: &str,
    pane: &str,
    call: Call,
    done: impl FnOnce(Result<T, String>) -> Message + Send + 'static,
    run: impl FnOnce(Port, Arc<Mobile>, String, String, Call) -> BoxFuture<Result<T, String>>,
) -> Task<Message> {
    #[cfg(test)]
    st.calls.push((ws.to_string(), call.clone()));
    #[cfg(not(test))]
    let _ = st;
    let Some((port, mobile)) = desk_of(m) else {
        let refused = hangar_i18n::t(m.locale(), hangar_i18n::Key::ChatConvDisabledLoading);
        return Task::done(done(Err(refused.into_owned())));
    };
    Task::perform(
        run(port, mobile, ws.to_string(), pane.to_string(), call),
        done,
    )
}

/// Reads the newest page of `ws`'s chat, or asks for it again after the read
/// that is out.
pub fn refresh(st: &mut State, m: &Model, ws: &str) -> Task<Message> {
    let Some(chat) = found(st, m, ws) else {
        return Task::none();
    };
    if chat.reading {
        chat.again = true;
        return Task::none();
    }
    chat.reading = true;
    chat.read_at = Some(Instant::now());
    let pane = chat.pane.clone();
    read(st, m, ws, &pane, None)
}

fn read(st: &mut State, m: &Model, ws: &str, pane: &str, before: Option<String>) -> Task<Message> {
    let older = before.is_some();
    let (ws_id, pane_id) = (ws.to_string(), pane.to_string());
    call(
        st,
        m,
        ws,
        pane,
        Call::Read { before },
        move |result| {
            msg(Msg::Read {
                ws: ws_id,
                pane: pane_id,
                older,
                result,
            })
        },
        |port, mobile, ws, pane, call| {
            let Call::Read { before } = call else {
                unreachable!("a read")
            };
            Box::pin(async move {
                desk::chat(&port, &mobile, &ws, &pane, before, None)
                    .await
                    .map(Box::new)
            })
        },
    )
}

/// Parses Claude's new messages, forgets those no longer held, and colours
/// the code blocks not coloured yet, off the UI thread.
fn sync_prose(chat: &mut Chat, ws: &str) -> Task<Message> {
    let mut held = HashSet::new();
    let mut blocks = Vec::new();
    for item in &chat.thread.items {
        let Body::Assistant { text } = &item.body else {
            continue;
        };
        held.insert(item.id.clone());
        let fresh = chat
            .prose
            .get(&item.id)
            .is_none_or(|(source, _)| source != text);
        if fresh {
            let content = rich::parse(text);
            blocks.extend(
                rich::code_blocks(&content)
                    .into_iter()
                    .filter(|block| !chat.code.contains_key(&block.key)),
            );
            chat.prose.insert(item.id.clone(), (text.clone(), content));
        }
    }
    chat.prose.retain(|id, _| held.contains(id));
    if blocks.is_empty() {
        return Task::none();
    }
    let ws = ws.to_string();
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || highlight_blocks(blocks))
                .await
                .unwrap_or_default()
        },
        move |found| msg(Msg::Coloured(ws, found)),
    )
}

/// The question form follows the question the pane waits on.
fn sync_form(chat: &mut Chat) {
    match &chat.thread.pending {
        Some(ChatPending::Question {
            tool_use_id,
            questions,
        }) => {
            if chat.form.tool_use_id != *tool_use_id {
                chat.form = Form::new(tool_use_id, questions.len());
                chat.refusal = None;
            }
        }
        _ => chat.form = Form::default(),
    }
}

pub fn update(st: &mut State, m: &mut Model, _sv: &Services, msg: Msg) -> Task<Message> {
    handle(st, m, msg)
}

/// `update`, without the services: the region reaches the pane through the
/// desk calls alone.
fn handle(st: &mut State, m: &mut Model, msg_: Msg) -> Task<Message> {
    let touched = touches(&msg_).map(str::to_string);
    let task = route(st, m, msg_);
    if let Some(chat) = touched.and_then(|ws| st.chats.get_mut(&ws)) {
        chat.touch();
    }
    task
}

/// The chat whose messages `msg_` may change, if any: not the draft, the
/// form, a scroll (`load_older` touches its own) or a tick (a read
/// answering does).
fn touches(msg_: &Msg) -> Option<&str> {
    match msg_ {
        Msg::Read { ws, .. }
        | Msg::Sent { ws, .. }
        | Msg::Toggle(ws, _)
        | Msg::SubmitAnswer(ws)
        | Msg::Permit(ws, _)
        | Msg::Decided { ws, .. }
        | Msg::Coloured(ws, _)
        | Msg::Send(ws) => Some(ws),
        Msg::Tick
        | Msg::LoadOlder(_)
        | Msg::Scrolled(..)
        | Msg::Edit(..)
        | Msg::Pick { .. }
        | Msg::Other { .. }
        | Msg::OtherText { .. }
        | Msg::Step { .. }
        | Msg::Start(_)
        | Msg::Link(_) => None,
    }
}

fn route(st: &mut State, m: &mut Model, msg_: Msg) -> Task<Message> {
    match msg_ {
        Msg::Read {
            ws,
            pane,
            older,
            result,
        } => {
            let desk = desk_state(m, &pane);
            let Some(chat) = st.chats.get_mut(&ws).filter(|chat| chat.pane == pane) else {
                return Task::none();
            };
            chat.desk = desk;
            if older {
                chat.reading_older = false;
            } else {
                chat.reading = false;
            }
            match result {
                Ok(view) => {
                    let view = *view;
                    chat.error = None;
                    if older {
                        chat.thread.take_older(view);
                    } else {
                        chat.loaded = true;
                        chat.thread.take_newest(view, now_ms());
                        sync_form(chat);
                    }
                }
                Err(error) => {
                    chat.loaded = true;
                    chat.error = Some(error);
                }
            }
            let colour = sync_prose(chat, &ws);
            let again = !older && std::mem::take(&mut chat.again);
            let next = if again {
                refresh(st, m, &ws)
            } else {
                Task::none()
            };
            Task::batch([colour, next])
        }
        Msg::Tick => {
            let due: Vec<WsId> = shown(m)
                .map(|(ws, pane)| {
                    let activity = m.live.panes_logic.activity_or_idle(&pane.id);
                    let desk = desk_state(m, &pane.id);
                    let chat = chat_of(st, &ws.id, &pane.id);
                    chat.desk = desk;
                    let changed = chat.seen.replace(activity) != Some(activity);
                    let stale = chat.read_at.is_none_or(|at| at.elapsed() >= FALLBACK);
                    let due = desk.ready
                        && (!chat.loaded || changed || stale || activity == AgentActivity::Working);
                    (ws.id.clone(), due)
                })
                .filter(|(_, due)| *due)
                .map(|(ws, _)| ws)
                .collect();
            Task::batch(due.iter().map(|ws| refresh(st, m, ws)))
        }
        Msg::LoadOlder(ws) => load_older(st, m, &ws),
        Msg::Scrolled(ws, viewport) => {
            let Some(chat) = st.chats.get_mut(&ws) else {
                return Task::none();
            };
            // Anchored to the bottom: the offset counts from there.
            chat.at_bottom = viewport.absolute_offset().y <= BOTTOM_MARGIN;
            if viewport.absolute_offset_reversed().y <= OLDER_MARGIN {
                return load_older(st, m, &ws);
            }
            Task::none()
        }
        Msg::Edit(ws, action) => {
            if let Some(chat) = st.chats.get_mut(&ws) {
                chat.draft.perform(action);
            }
            Task::none()
        }
        Msg::Send(ws) => send(st, m, &ws),
        Msg::Sent { ws, at, result } => {
            if let Some(chat) = st.chats.get_mut(&ws) {
                let state = match result {
                    Ok(sent) if sent.queued == Some(true) => Sent::Queued,
                    Ok(_) => Sent::Typed,
                    Err(error) => Sent::Failed(error),
                };
                chat.thread.sent(at, state);
            }
            refresh(st, m, &ws)
        }
        Msg::Toggle(ws, id) => {
            if let Some(chat) = st.chats.get_mut(&ws)
                && !chat.expanded.remove(&id)
            {
                chat.expanded.insert(id);
            }
            Task::none()
        }
        Msg::Pick {
            ws,
            question,
            option,
        } => {
            if let Some(chat) = st.chats.get_mut(&ws)
                && let Some(ChatPending::Question { questions, .. }) = &chat.thread.pending
                && let Some(asked) = questions.get(question)
            {
                chat.form.pick(asked, question, option);
            }
            Task::none()
        }
        Msg::Other { ws, question } => {
            if let Some(chat) = st.chats.get_mut(&ws)
                && let Some(ChatPending::Question { questions, .. }) = &chat.thread.pending
                && let Some(asked) = questions.get(question)
            {
                chat.form.other(asked, question);
            }
            Task::none()
        }
        Msg::OtherText { ws, question, text } => {
            if let Some(chat) = st.chats.get_mut(&ws)
                && let Some(words) = chat.form.other.get_mut(question)
            {
                *words = text;
            }
            Task::none()
        }
        Msg::Step { ws, to } => {
            if let Some(chat) = st.chats.get_mut(&ws)
                && let Some(ChatPending::Question { questions, .. }) = &chat.thread.pending
            {
                chat.form.go(questions, to);
            }
            Task::none()
        }
        Msg::SubmitAnswer(ws) => submit_answer(st, m, &ws),
        Msg::Permit(ws, allow) => permit(st, m, &ws, allow),
        Msg::Decided { ws, result } => {
            if let Some(chat) = st.chats.get_mut(&ws) {
                chat.deciding = false;
                chat.refusal = result.err();
            }
            // Answered, or refused because the prompt changed: either way the
            // card follows what the pane shows now.
            refresh(st, m, &ws)
        }
        Msg::Coloured(ws, found) => {
            if let Some(chat) = st.chats.get_mut(&ws) {
                chat.code.extend(found);
            }
            Task::none()
        }
        Msg::Start(ws) => Task::done(Message::Intent(Intent::CreateOrchestrator(ws))),
        Msg::Link(url) => {
            let web = ["http://", "https://", "mailto:"]
                .iter()
                .any(|scheme| url.starts_with(scheme));
            match web.then(|| hangar_core::opener::open_url(&url)) {
                Some(Err(error)) => {
                    let words = hangar_i18n::t_with(
                        m.locale(),
                        hangar_i18n::Key::PreviewOpenFailed,
                        &hangar_i18n::args!("error" => error.to_string()),
                    )
                    .into_owned();
                    Task::done(Message::Intent(Intent::Notice(Notice::Problem(words))))
                }
                _ => Task::none(),
            }
        }
    }
}

fn load_older(st: &mut State, m: &Model, ws: &str) -> Task<Message> {
    let Some(chat) = found(st, m, ws) else {
        return Task::none();
    };
    if chat.reading_older || !chat.thread.older {
        return Task::none();
    }
    let Some(cursor) = chat.thread.cursor.clone() else {
        return Task::none();
    };
    chat.reading_older = true;
    chat.touch();
    let pane = chat.pane.clone();
    read(st, m, ws, &pane, Some(cursor))
}

fn send(st: &mut State, m: &Model, ws: &str) -> Task<Message> {
    let Some(pane) = m.workspace(ws).and_then(orchestrator_of).cloned() else {
        return Task::none();
    };
    let chat = chat_of(st, ws, &pane.id);
    if blocked(chat, &pane).is_some() {
        return Task::none();
    }
    let text = chat.draft.text().trim_end().to_string();
    if text.trim().is_empty() {
        return Task::none();
    }
    let at = now_ms();
    chat.thread.clear_failed();
    chat.thread.send(text.clone(), at);
    chat.draft = text_editor::Content::new();
    chat.at_bottom = true;
    let ws_id = ws.to_string();
    call(
        st,
        m,
        ws,
        &pane.id,
        Call::Send { text },
        move |result| {
            msg(Msg::Sent {
                ws: ws_id,
                at,
                result,
            })
        },
        |port, mobile, ws, pane, call| {
            let Call::Send { text } = call else {
                unreachable!("a send")
            };
            Box::pin(async move { desk::send_text(&port, &mobile, &ws, &pane, &text).await })
        },
    )
}

fn decided(ws: &str) -> impl FnOnce(Result<RemoteAnswered, String>) -> Message + Send + 'static {
    let ws = ws.to_string();
    move |result| msg(Msg::Decided { ws, result })
}

fn submit_answer(st: &mut State, m: &Model, ws: &str) -> Task<Message> {
    let Some(chat) = found(st, m, ws) else {
        return Task::none();
    };
    let Some(ChatPending::Question {
        tool_use_id,
        questions,
    }) = &chat.thread.pending
    else {
        return Task::none();
    };
    if chat.deciding || chat.form.tool_use_id != *tool_use_id {
        return Task::none();
    }
    let Some(answers) = chat.form.answers(questions) else {
        return Task::none();
    };
    let tool_use_id = tool_use_id.clone();
    chat.deciding = true;
    chat.refusal = None;
    let pane = chat.pane.clone();
    call(
        st,
        m,
        ws,
        &pane,
        Call::Answer {
            tool_use_id,
            answers,
        },
        decided(ws),
        |port, mobile, ws, pane, call| {
            let Call::Answer {
                tool_use_id,
                answers,
            } = call
            else {
                unreachable!("an answer")
            };
            Box::pin(async move {
                desk::answer(&port, &mobile, &ws, &pane, &tool_use_id, &answers).await
            })
        },
    )
}

fn permit(st: &mut State, m: &Model, ws: &str, allow: bool) -> Task<Message> {
    let Some(chat) = found(st, m, ws) else {
        return Task::none();
    };
    let Some(ChatPending::Permission { prompt, .. }) = &chat.thread.pending else {
        return Task::none();
    };
    if chat.deciding {
        return Task::none();
    }
    let prompt = Some(prompt.clone());
    chat.deciding = true;
    chat.refusal = None;
    let pane = chat.pane.clone();
    call(
        st,
        m,
        ws,
        &pane,
        Call::Permit { allow, prompt },
        decided(ws),
        |port, mobile, ws, pane, call| {
            let Call::Permit { allow, prompt } = call else {
                unreachable!("a permission")
            };
            Box::pin(async move {
                desk::permit(&port, &mobile, &ws, &pane, allow, prompt.as_deref()).await
            })
        },
    )
}

/// The tick, while a workspace is shown as the chat view with an
/// orchestrator.
pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    let mut shown = shown(m).peekable();
    if shown.peek().is_none() {
        return Subscription::none();
    }
    let waiting = shown.any(|(ws, pane)| {
        st.chat(&ws.id)
            .is_none_or(|chat| chat.pane != pane.id || !chat.loaded)
    });
    let every = if waiting { FIRST_TICK } else { TICK };
    iced::time::every(every).map(|_| msg(Msg::Tick))
}

/// The orchestrator's mod reported, or its queue changed: read again.
pub fn on_event(st: &mut State, m: &mut Model, event: &CoreEvent) -> Option<Task<Message>> {
    let pane = match event {
        CoreEvent::Bridge { pane, .. } | CoreEvent::BridgeQueue { pane, .. } => pane,
        _ => return None,
    };
    let ws = shown(m)
        .find(|(_, orchestrator)| orchestrator.id == *pane)
        .map(|(ws, _)| ws.id.clone())?;
    Some(refresh(st, m, &ws))
}

/// The region was just shown: read at once rather than at the next tick.
pub fn on_shown(st: &mut State, m: &Model, ws: &str) -> Task<Message> {
    if desk_of(m).is_none() {
        return Task::none();
    }
    refresh(st, m, ws)
}
