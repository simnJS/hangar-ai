//! P5: the pane grid and the terminal panes, the heart of Hangar: every pane
//! is a real process in a real terminal.
//!
//! Ports WorkspaceTerminals.tsx, PaneGrid.tsx, TerminalPane.tsx,
//! TerminalSearch.tsx, PaneZoomBar.tsx, PaneGauge.tsx, PaneStatus.ts and
//! lib/{ptyBus,terminalRegistry}.ts (inventory §4, §2.1/12).
//!
//! | File | What |
//! | --- | --- |
//! | `pty_link` | A pane's terminal, its writer thread, and the sink core feeds it through |
//! | `hub` | Every terminal by pane id, for the engines (`TerminalAccess`), and the channel the reader threads wake the window on |
//! | `controller` | A pane's life: mount, spawn, launch of its agent, exit, unmount |
//! | `agent_watch` | Where its agent stands: the mod's reports, the output watcher, the trust question, hand-backs |
//! | `term_widget` | hangar-term's widget over the locked terminal |
//! | `grid` | The split grid: slots, boundaries, moves, zoom, file drops |
//! | `workspace_terminals`, `terminal_pane`, `header`, `search`, `zoom_bar`, `gauge` | The views |
//! | `status`, `geometry`, `look`, `launch`, `drop` | Pure logic, with the TS tests |
//!
//! Terminals live in this package's state, never in a window: a hidden
//! workspace draws nothing, and its panes keep consuming their output, on
//! their own reader threads, all the same. A detached workspace is only
//! drawn by another window.

mod agent_watch;
mod controller;
mod drop;
mod gauge;
mod geometry;
mod grid;
mod header;
mod hub;
mod launch;
mod look;
mod press;
mod pty_link;
pub mod search;
mod status;
mod term_widget;
mod terminal_pane;
mod workspace_terminals;
mod zoom_bar;

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hangar_term::{Action, ClipboardTarget, Event as TermEvent, KeyPress, ScrollTo};
use iced::futures::stream;
use iced::widget::Space;
use iced::{Element, Size, Subscription, Task, window};

use crate::commands::CommandId;
use crate::ctx::Ctx;
use crate::keymap::KeymapIndex;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services, TerminalAccess};
use crate::shell::{Focus, Region, View, WindowKind};
use crate::state::live::{ContextGauge, QueuedPrompt};
use crate::state::types::{AgentId, DropZone, Pane, SplitDir};
use crate::state::{
    AgentActivity, BridgeEvent, GlyphState, Model, PaneId, Patch, StoreAction, Workspace, WsId,
};

use controller::{Context, PaneRt, SpawnReport};
use hub::{Hub, HubChannel};
use pty_link::{HubEvent, HubKind};

pub use geometry::Turn;
pub use header::Folds;
pub use search::SearchMsg;
pub use status::PaneStatus;

/// The package's UI state; lives in `App.pkg.panes`. The terminals and their
/// PTY links live here, not in a window.
#[derive(Debug, Default)]
pub struct State {
    hub: Arc<Hub>,
    channel: HubChannel,
    panes: HashMap<PaneId, PaneRt>,
    /// Each grid's size as it last measured itself, for the headers'
    /// breakpoints and the direction of a split.
    grid_sizes: HashMap<WsId, Size>,
    /// The panes each opened workspace had at the last sync: a change in the
    /// count ends the zoom.
    pane_counts: HashMap<WsId, usize>,
    /// The windows known not to have the system's focus.
    unfocused: HashSet<window::Id>,
    /// Files dropped in the batch of events being handled, joined into one
    /// paste per pane.
    drops: Vec<(PaneId, PathBuf)>,
    /// Prompts still queued for a pane that restarted, for its successor
    /// (`stashQueue`, `CARRY_MS`).
    carried: HashMap<(WsId, String), (Instant, Vec<QueuedPrompt>)>,
    /// The header under the pointer, which shows its actions.
    hovered: Option<PaneId>,
    /// An agent or shell picker open in a header.
    picker: Option<(PaneId, Picker)>,
    /// The signature of the model the last sync brought the panes in line
    /// with.
    synced: u64,
}

/// How long the prompts of a restarted pane wait for its successor.
const CARRY: Duration = Duration::from_secs(10);

/// The pickers of a header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Picker {
    Agent,
    Shell,
}

/// What other packages read of the panes; lives in `Model.live.panes`.
#[derive(Debug, Default)]
pub struct Live {
    reports: HashMap<PaneId, PaneReport>,
}

/// One mounted pane, as the rest of the window sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct PaneReport {
    pub workspace: WsId,
    /// The shell's own life (starting, running, exited).
    pub status: PaneStatus,
    /// Where its agent stands. A plain shell reports nothing (`None`) unless
    /// a Claude Code in it reports through the mod.
    pub activity: Option<AgentActivity>,
    pub bridged: bool,
    /// It handed back while the person was looking elsewhere.
    pub attention: bool,
    pub gauge: Option<ContextGauge>,
    pub queued: usize,
    /// The Claude account its Claude Code was started on.
    pub account: Option<String>,
    /// The header buttons its width folded into the "…" menu.
    pub folds: Folds,
}

impl Live {
    pub fn report(&self, pane: &str) -> Option<&PaneReport> {
        self.reports.get(pane)
    }

    /// The glyph a pane shows (`paneGlyphState`), for whatever draws one.
    pub fn glyph(&self, pane: &str) -> GlyphState {
        self.report(pane).map_or(GlyphState::Starting, |report| {
            status::pane_glyph_state(report.status, report.activity)
        })
    }

    pub fn reports(&self) -> impl Iterator<Item = (&PaneId, &PaneReport)> {
        self.reports.iter()
    }
}

/// The package's messages, routed by the shell as `Message::Panes(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    /// What a pane's terminal widget did.
    Term(PaneId, Action),
    /// What the panes' reader threads said since the last batch.
    Hub(Vec<HubEvent>),
    /// The model changed under the panes: bring them in line.
    Sync,
    /// The output watchers' and the trust looks' clock.
    Tick(Instant),
    Spawned(PaneId, Result<SpawnReport, String>),
    /// The launch delay is over, and whether the session to resume exists.
    LaunchReady(PaneId, Option<bool>),
    LoginReady(PaneId),
    WindowFocus(window::Id, bool),
    FlushDrops,
    Paste(PaneId, Option<String>),
    /// A press in a pane.
    Focus(WsId, PaneId),
    Hover(Option<PaneId>),
    OpenPicker(PaneId, Picker),
    ClosePicker,
    SetAgent(WsId, PaneId, AgentId),
    SetShell(WsId, PaneId, Option<String>),
    Restart(WsId, PaneId),
    Close(WsId, PaneId),
    /// Without a direction, the pane's longer side decides.
    Split(WsId, PaneId, Option<SplitDir>),
    ToggleZoom(WsId, PaneId),
    /// Zooms onto a pane, or with `None` puts every pane back (`zoomTo`).
    Zoom(WsId, Option<PaneId>),
    Sessions(WsId, PaneId),
    Diff(WsId, PaneId),
    CancelQueue(PaneId),
    DismissNote(PaneId, String),
    GridSized(WsId, Size),
    SetRatio(WsId, Vec<Turn>, f64),
    Move {
        ws: WsId,
        drag: PaneId,
        target: PaneId,
        zone: DropZone,
    },
    Search(PaneId, SearchMsg),
    /// A `terminal.*` command for the focused pane of the focused window:
    /// what the shell's command table (`commands::run`) and the keymap send
    /// for clear, copy, paste, select all, scroll to the top or the bottom,
    /// and find.
    Command(CommandId),
}

fn done(message: Message) -> Task<Message> {
    Task::done(message)
}

fn intent(intent: Intent) -> Task<Message> {
    done(Message::Intent(intent))
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    let task = handle(st, m, sv, msg);
    let synced = sync_if_needed(st, m, sv);
    publish(st, m);
    Task::batch([task, synced])
}

fn handle(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    match msg {
        Msg::Term(pane, action) => term_action(st, pane, action),
        Msg::Hub(events) => hub_events(st, m, sv, events),
        Msg::Sync => sync(st, m, sv),
        Msg::Tick(now) => tick(st, m, now),
        Msg::Spawned(pane, result) => {
            let tr = hangar_i18n::Translator::new(m.locale());
            match st.panes.get_mut(&pane) {
                Some(rt) => controller::spawned(rt, m, tr, result),
                // The pane went while its shell was starting: the shell
                // would outlive it.
                None if result.is_ok() => {
                    let pty = sv.core().pty().clone();
                    Task::perform(
                        tokio::task::spawn_blocking(move || {
                            hangar_core::pty::pty_kill(&pty, pane, None)
                        }),
                        |_| Message::Noop,
                    )
                }
                None => Task::none(),
            }
        }
        Msg::LaunchReady(pane, exists) => match st.panes.get_mut(&pane) {
            Some(rt) => controller::launch_ready(rt, m, exists),
            None => Task::none(),
        },
        Msg::LoginReady(pane) => {
            if let Some(rt) = st.panes.get_mut(&pane) {
                controller::login_ready(rt);
            }
            Task::none()
        }
        Msg::WindowFocus(window, focused) => {
            if focused {
                st.unfocused.remove(&window);
            } else {
                st.unfocused.insert(window);
            }
            Task::none()
        }
        Msg::FlushDrops => flush_drops(st, m),
        Msg::Paste(pane, text) => {
            if let (Some(rt), Some(text)) = (st.panes.get(&pane), text) {
                let bytes = rt.link.lock().paste_bytes(&text);
                rt.link.write(bytes);
            }
            Task::none()
        }
        Msg::Focus(ws, pane) => intent(Intent::FocusPane {
            ws,
            pane,
            terminal: true,
        }),
        Msg::Hover(pane) => {
            st.hovered = pane;
            Task::none()
        }
        Msg::OpenPicker(pane, picker) => {
            st.picker = match &st.picker {
                Some((open, kind)) if *open == pane && *kind == picker => None,
                _ => Some((pane, picker)),
            };
            Task::none()
        }
        Msg::ClosePicker => {
            st.picker = None;
            Task::none()
        }
        Msg::SetAgent(ws, pane, agent) => {
            st.picker = None;
            intent(Intent::ReplacePane {
                ws,
                pane,
                patch: Patch::new(move |pane: &mut Pane| {
                    pane.agent = agent;
                    pane.session_id = None;
                }),
            })
        }
        Msg::SetShell(ws, pane, shell) => {
            st.picker = None;
            intent(Intent::ReplacePane {
                ws,
                pane,
                patch: Patch::new(move |pane: &mut Pane| pane.shell_id = shell.clone()),
            })
        }
        Msg::Restart(ws, pane) => intent(Intent::ReplacePane {
            ws,
            pane,
            patch: Patch::none(),
        }),
        Msg::Close(ws, pane) => {
            m.apply(StoreAction::ClosePane { ws, pane });
            Task::none()
        }
        Msg::Split(ws, pane, dir) => {
            let dir = dir.unwrap_or_else(|| preferred_dir(st, m, &ws, &pane));
            intent(Intent::SplitPane {
                ws,
                near: Some(pane),
                dir: Some(dir),
            })
        }
        Msg::ToggleZoom(ws, pane) => {
            let target = (zoomed_pane(m, m.workspace(&ws)) != Some(pane.clone())).then_some(pane);
            zoom_to(ws, target)
        }
        Msg::Zoom(ws, target) => zoom_to(ws, target),
        Msg::Sessions(ws, pane) => intent(Intent::OpenSessions { ws, pane }),
        Msg::Diff(ws, pane) => intent(Intent::OpenDiff {
            ws,
            pane: Some(pane),
        }),
        Msg::CancelQueue(pane) => {
            let host = sv.core().host().clone();
            Task::perform(
                tokio::task::spawn_blocking(move || {
                    hangar_core::bridge::bridge_cancel(&host, pane, None)
                }),
                |_| Message::Noop,
            )
        }
        Msg::DismissNote(pane, note) => {
            if let Some(rt) = st.panes.get_mut(&pane) {
                rt.dismissed.insert(note);
            }
            Task::none()
        }
        Msg::GridSized(ws, size) => {
            st.grid_sizes.insert(ws, size);
            Task::none()
        }
        Msg::SetRatio(ws, path, ratio) => {
            if let Some(workspace) = m.workspace(&ws) {
                let tree = geometry::normalize_tree(&workspace.panes, workspace.tree.as_ref());
                let next = geometry::with_ratio(&tree, &path, ratio);
                m.apply(StoreAction::SetTree(ws, next));
            }
            Task::none()
        }
        Msg::Move {
            ws,
            drag,
            target,
            zone,
        } => {
            m.apply(StoreAction::MovePane {
                ws,
                drag,
                target,
                zone,
            });
            Task::none()
        }
        Msg::Search(pane, search_msg) => {
            let Some(rt) = st.panes.get_mut(&pane) else {
                return Task::none();
            };
            let mut terminal = rt.link.lock();
            let (task, closed) = search::update(&mut rt.search, &mut terminal, search_msg);
            drop(terminal);
            if let Some(query) = closed {
                rt.last_query = query;
            }
            task
        }
        Msg::Command(command) => run_command(st, m, command),
    }
}

/// Zooms onto `target`, which takes the focus and the keyboard, or with
/// `None` puts every pane back.
fn zoom_to(ws: WsId, target: Option<PaneId>) -> Task<Message> {
    let on = target.is_some();
    let focus = match target {
        Some(pane) => intent(Intent::FocusPane {
            ws: ws.clone(),
            pane,
            terminal: true,
        }),
        None => Task::none(),
    };
    Task::batch([focus, intent(Intent::SetZoom { ws, on })])
}

/// What a terminal widget did, applied, and what the terminal said back.
fn term_action(st: &mut State, pane: PaneId, action: Action) -> Task<Message> {
    let Some(rt) = st.panes.get_mut(&pane) else {
        return Task::none();
    };
    let events = rt.link.lock().perform(action);
    let mut tasks = Vec::new();
    for event in events {
        match event {
            TermEvent::Write(bytes) => rt.link.write(bytes),
            TermEvent::Resize(size) => rt.link.resize(size.columns, size.rows),
            TermEvent::SetClipboard { target, text } => tasks.push(match target {
                ClipboardTarget::Clipboard => iced::clipboard::write(text),
                ClipboardTarget::Primary => iced::clipboard::write_primary(text),
            }),
            TermEvent::OpenLink(link) => {
                // Web links open in the system browser (inventory §9.2).
                let _ = open::that_detached(&link.uri);
            }
            TermEvent::Pressed => {
                if let Some(search) = &mut rt.search {
                    search.typing = false;
                }
                tasks.push(intent(Intent::FocusPane {
                    ws: rt.ws.clone(),
                    pane: pane.clone(),
                    terminal: true,
                }));
            }
            TermEvent::UserInput => {
                rt.watch.key();
                rt.attention = false;
            }
            TermEvent::FileDropped(path) => {
                if st.drops.is_empty() {
                    tasks.push(done(Message::Panes(Msg::FlushDrops)));
                }
                st.drops.push((pane.clone(), path));
                break;
            }
            _ => {}
        }
    }
    Task::batch(tasks)
}

/// The files of one drop, joined into one paste for the pane they landed on:
/// as a paste, so an agent that brackets pastes sees one, which is how
/// Claude Code tells a dropped image path from typed text.
fn flush_drops(st: &mut State, m: &Model) -> Task<Message> {
    let drops = std::mem::take(&mut st.drops);
    let mut by_pane: Vec<(PaneId, Vec<String>)> = Vec::new();
    for (pane, path) in drops {
        let path = path.to_string_lossy().into_owned();
        match by_pane.iter_mut().find(|(id, _)| *id == pane) {
            Some((_, paths)) => paths.push(path),
            None => by_pane.push((pane, vec![path])),
        }
    }
    let mut tasks = Vec::new();
    for (pane, paths) in by_pane {
        let Some(rt) = st.panes.get(&pane) else {
            continue;
        };
        let Some((ws, model_pane)) = controller::find(m, &rt.ws, &pane) else {
            continue;
        };
        let shell = controller::shell_of(m, ws, model_pane);
        let text = drop::drop_text(&paths, model_pane.agent, shell);
        let bytes = rt.link.lock().paste_bytes(&text);
        rt.link.write(bytes);
        tasks.push(intent(Intent::FocusPane {
            ws: ws.id.clone(),
            pane,
            terminal: true,
        }));
    }
    Task::batch(tasks)
}

fn hub_events(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    events: Vec<HubEvent>,
) -> Task<Message> {
    let now = Instant::now();
    let mut tasks = Vec::new();
    for event in events {
        let looking = looking(st, m, &event.pane);
        let window_focused = window_focused(st, m, &event.pane);
        let Some(rt) = st.panes.get_mut(&event.pane) else {
            continue;
        };
        let said = match event.kind {
            HubKind::Output => {
                controller::output(rt, now);
                continue;
            }
            HubKind::Exit => {
                controller::exited(rt, sv);
                continue;
            }
            HubKind::Copy(text) => {
                tasks.push(iced::clipboard::write(text));
                continue;
            }
            // A bell is the agent asking for you outright.
            HubKind::Bell => rt
                .watcher
                .ring(now)
                .and_then(|verdict| rt.watch.watched(verdict)),
            HubKind::Notification { title, body } => rt.watch.notification(title.as_deref(), &body),
        };
        if let Some(said) = said {
            let tr = hangar_i18n::Translator::new(m.locale());
            let settings = m.settings().clone();
            let cx = Context {
                tr,
                settings: &settings,
                window_focused,
                looking,
            };
            tasks.push(controller::said(rt, m, &cx, vec![said]));
        }
    }
    Task::batch(tasks)
}

/// The output watchers' silence checks, and the trust looks that are due.
fn tick(st: &mut State, m: &mut Model, now: Instant) -> Task<Message> {
    let idle = Duration::from_millis(m.settings().notify_idle_ms);
    let ids: Vec<PaneId> = st
        .panes
        .iter()
        .filter(|(_, rt)| rt.watcher.ticking() || rt.trust_due.is_some())
        .map(|(id, _)| id.clone())
        .collect();
    let mut tasks = Vec::new();
    for id in ids {
        let looking = looking(st, m, &id);
        let window_focused = window_focused(st, m, &id);
        let Some(rt) = st.panes.get_mut(&id) else {
            continue;
        };
        let mut said = Vec::new();
        for verdict in rt.watcher.tick(now, idle) {
            said.extend(rt.watch.watched(verdict));
        }
        if rt.trust_due.is_some_and(|due| due <= now) {
            said.extend(controller::trust_look(rt));
        }
        if !said.is_empty() {
            let tr = hangar_i18n::Translator::new(m.locale());
            let settings = m.settings().clone();
            let cx = Context {
                tr,
                settings: &settings,
                window_focused,
                looking,
            };
            tasks.push(controller::said(rt, m, &cx, said));
        }
    }
    Task::batch(tasks)
}

/// The `terminal.*` commands, for the focused pane of the focused window
/// (App.tsx:1451, through `terminalRegistry.getTerminal(focused)`).
fn run_command(st: &mut State, m: &Model, command: CommandId) -> Task<Message> {
    let Some(pane) = keyboard_pane(m) else {
        return Task::none();
    };
    let Some(rt) = st.panes.get_mut(&pane) else {
        return Task::none();
    };
    match command {
        CommandId::TerminalClear => rt.link.lock().clear(),
        CommandId::TerminalCopy => {
            if let Some(text) = rt.link.lock().selection_text() {
                return iced::clipboard::write(text);
            }
        }
        CommandId::TerminalPaste => {
            return iced::clipboard::read()
                .map(move |text| Message::Panes(Msg::Paste(pane.clone(), text)));
        }
        CommandId::TerminalSelectAll => rt.link.lock().select_all(),
        CommandId::TerminalScrollTop => {
            rt.link.lock().perform(Action::Scroll(ScrollTo::Top));
        }
        CommandId::TerminalScrollBottom => rt.link.lock().scroll_to_bottom(),
        CommandId::TerminalFind => return open_finder(st, &pane),
        _ => {}
    }
    Task::none()
}

/// `terminal.find` on a pane (`openFinder`).
fn open_finder(st: &mut State, pane: &PaneId) -> Task<Message> {
    let Some(rt) = st.panes.get_mut(pane) else {
        return Task::none();
    };
    let mut terminal = rt.link.lock();
    let task = search::open(&mut rt.search, &rt.last_query, pane, &mut terminal);
    drop(terminal);
    task
}

pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    let mut subscriptions = vec![
        st.channel.subscription(),
        iced::event::listen_with(|event, _status, window| match event {
            iced::Event::Window(window::Event::Focused) => {
                Some(Message::Panes(Msg::WindowFocus(window, true)))
            }
            iced::Event::Window(window::Event::Unfocused) => {
                Some(Message::Panes(Msg::WindowFocus(window, false)))
            }
            _ => None,
        }),
    ];
    if st
        .panes
        .values()
        .any(|rt| rt.watcher.ticking() || rt.trust_due.is_some())
    {
        subscriptions
            .push(iced::time::every(agent_watch::TICK).map(|at| Message::Panes(Msg::Tick(at))));
    }
    // Whatever changed the model outside this package's own messages (a store
    // action of the shell, an engine's command, a hydration) is caught here:
    // the subscription is rebuilt after every update, and a signature that
    // differs from the last sync's fires one `Sync`.
    let signature = signature(st, m);
    if signature != st.synced {
        subscriptions.push(Subscription::run_with(signature, |_| {
            stream::once(async { Message::Panes(Msg::Sync) })
        }));
    }
    Subscription::batch(subscriptions)
}

/// Region `main` takes the focus: the focused pane's terminal takes the
/// keyboard, which the next sync gives it.
pub fn focus_entry(st: &State, m: &Model, region: Region) -> Option<Task<Message>> {
    let _ = (st, m);
    (region == Region::Main).then(|| done(Message::Panes(Msg::Sync)))
}

pub fn on_intent(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    let task = match intent {
        // The focus and the zoom are the shell's transient state; written
        // here too, idempotently, so the panes work before the shell's own
        // handling of these intents lands (P0b, `activateRef`).
        Intent::FocusPane { ws, pane, terminal } => {
            m.ui.focus_by_workspace.insert(ws.clone(), pane.clone());
            if *terminal {
                m.ui.focus = Some(Focus::Terminal(pane.clone()));
            }
            Some(Task::none())
        }
        Intent::SetZoom { ws, on } => {
            m.ui.zoom_by_workspace.insert(ws.clone(), *on);
            Some(Task::none())
        }
        Intent::OpenFinder(pane) => Some(open_finder(st, pane)),
        Intent::Broadcast {
            ws,
            text,
            when_free,
        } => Some(broadcast(st, sv, ws, text, *when_free)),
        Intent::InsertTranscript(text) => {
            // For the terminal that has the keyboard; a text field with the
            // focus is its own package's to fill.
            let focus_is_terminal = matches!(
                m.ui.focus,
                None | Some(Focus::Terminal(_)) | Some(Focus::Region(Region::Main))
            );
            match keyboard_pane(m)
                .filter(|_| focus_is_terminal)
                .and_then(|pane| st.panes.get(&pane))
            {
                Some(rt) => {
                    let submit = if m.settings().voice_submit { "\r" } else { "" };
                    rt.link.write(format!("{text}{submit}").into_bytes());
                    Some(Task::none())
                }
                None => None,
            }
        }
        _ => None,
    };
    let synced = sync_if_needed(st, m, sv);
    publish(st, m);
    match task {
        Some(task) => Some(Task::batch([task, synced])),
        None => Some(synced),
    }
}

/// Types `text` into every pane of the workspace; with `when_free`, an agent
/// whose mod reports gets it queued for when it is free (App.tsx's broadcast).
fn broadcast(st: &State, sv: &Services, ws: &str, text: &str, when_free: bool) -> Task<Message> {
    let mut queued = Vec::new();
    for (id, rt) in st.panes.iter().filter(|(_, rt)| rt.ws == ws) {
        if when_free && rt.watch.bridged {
            queued.push(id.clone());
        } else {
            rt.link.write(format!("{text}\r").into_bytes());
        }
    }
    if queued.is_empty() {
        return Task::none();
    }
    let host = sv.core().host().clone();
    let text = text.to_owned();
    Task::perform(
        tokio::task::spawn_blocking(move || {
            for pane in queued {
                hangar_core::bridge::bridge_enqueue(&host, pane, text.clone(), None);
            }
        }),
        |_| Message::Noop,
    )
}

pub fn on_event(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    event: &CoreEvent,
) -> Option<Task<Message>> {
    let task = match event {
        CoreEvent::Bridge { pane, event } => {
            let parsed = BridgeEvent::parse(event)?;
            let looking = looking(st, m, pane);
            let window_focused = window_focused(st, m, pane);
            let rt = st.panes.get_mut(pane)?;
            let stored = controller::find(m, &rt.ws, pane).and_then(|(_, p)| p.session_id.clone());
            let said = rt.watch.bridge(&parsed, stored.as_deref());
            if !rt.watch.bridged {
                rt.trust_due = None;
            }
            let tr = hangar_i18n::Translator::new(m.locale());
            let settings = m.settings().clone();
            let cx = Context {
                tr,
                settings: &settings,
                window_focused,
                looking,
            };
            controller::said(rt, m, &cx, said)
        }
        CoreEvent::BridgeQueue { pane, pending } => {
            let rt = st.panes.get_mut(pane)?;
            rt.queue.clone_from(pending);
            Task::none()
        }
        _ => return None,
    };
    let synced = sync_if_needed(st, m, sv);
    publish(st, m);
    Some(Task::batch([task, synced]))
}

/// Region `main` in the terminals view: the grid of workspace `ws`. A hidden
/// workspace draws nothing; its terminals keep consuming output.
pub fn view_workspace<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    ws: &'a WsId,
    hidden: bool,
) -> Element<'a, Message> {
    if hidden {
        return Space::new().into();
    }
    match ctx.model.workspace(ws) {
        Some(workspace) => workspace_terminals::view(st, ctx, workspace),
        None => Space::new().into(),
    }
}

/// What the engines may do with the terminals (`Services::terminals`).
pub fn terminal_access(st: &State) -> Arc<dyn TerminalAccess> {
    st.hub.clone()
}

/// The header buttons the pane's width folded into its "…" menu, for the
/// pane menu (P7).
pub fn folds(st: &State, m: &Model, pane: &str) -> Folds {
    let _ = st;
    m.live
        .panes
        .report(pane)
        .map(|r| r.folds)
        .unwrap_or_default()
}

/// The direction a split without one takes: along the pane's longer side
/// (`preferredDir`), for the shell's `SplitPane` handling (P0b).
pub fn preferred_dir(st: &State, m: &Model, ws: &str, pane: &str) -> SplitDir {
    let Some(workspace) = m.workspace(ws) else {
        return SplitDir::Row;
    };
    let size = st
        .grid_sizes
        .get(ws)
        .copied()
        .unwrap_or(Size::new(1000.0, 700.0));
    let tree = geometry::normalize_tree(&workspace.panes, workspace.tree.as_ref());
    match geometry::compute_layout(&tree).rect(pane) {
        Some(rect) => geometry::preferred_dir(rect.width * size.width, rect.height * size.height),
        None => SplitDir::Row,
    }
}

/// Whether the app's keymap takes a key before the terminal sees it
/// (hangar-term's `key_filter`, inventory §1.10).
///
/// `KeymapIndex` is P7's to define, with the lookup this needs; until it
/// lands there is no dispatcher a key could go to, so every key stays the
/// terminal's, as it would in a terminal without Hangar around it.
pub(crate) fn keymap_takes(index: &KeymapIndex, press: &KeyPress) -> bool {
    let _ = (index, press);
    false
}

// ---- Where the panes stand --------------------------------------------------

/// The pane of `ws` that has the focus: the one the shell remembers, else
/// the first in reading order.
pub(crate) fn focused_pane(m: &Model, ws: &Workspace) -> Option<PaneId> {
    if let Some(pane) = m.ui.focus_by_workspace.get(&ws.id)
        && ws.panes.iter().any(|p| &p.id == pane)
    {
        return Some(pane.clone());
    }
    let tree = geometry::normalize_tree(&ws.panes, ws.tree.as_ref());
    crate::state::layout::leaf_ids(&tree)
        .into_iter()
        .find(|id| !id.is_empty())
}

/// The pane drawn over the whole grid: the focused one, while the
/// workspace's zoom is on and it has more than one pane. The zoom follows
/// the focus around.
pub(crate) fn zoomed_pane<'w>(m: &Model, ws: impl Into<Option<&'w Workspace>>) -> Option<PaneId> {
    let ws = ws.into()?;
    let on = m.ui.zoom_by_workspace.get(&ws.id).copied().unwrap_or(false);
    (on && ws.panes.len() > 1)
        .then(|| focused_pane(m, ws))
        .flatten()
}

/// The window that draws workspace `ws`: its own, or the main window while
/// it is the active one there.
fn window_of(m: &Model, ws: &str) -> Option<window::Id> {
    let own = m
        .live
        .windows
        .iter()
        .find(|(_, kind)| matches!(kind, WindowKind::Workspace(id) if id == ws))
        .map(|(id, _)| *id);
    own.or_else(|| {
        let active = m
            .active_workspace()
            .filter(|active| active.id == ws && active.detached != Some(true))?;
        let _ = active;
        m.live
            .windows
            .iter()
            .find(|(_, kind)| matches!(kind, WindowKind::Main))
            .map(|(id, _)| *id)
    })
}

/// Workspace `ws` is on screen, its grid shown.
fn on_screen(m: &Model, ws: &str) -> bool {
    m.live.hydrated && m.ui.view == View::Terminals && window_of(m, ws).is_some()
}

/// The pane is on screen: its workspace is, and no other pane is zoomed over
/// it.
fn visible(m: &Model, ws: &Workspace, pane: &str) -> bool {
    on_screen(m, &ws.id) && zoomed_pane(m, ws).is_none_or(|zoomed| zoomed == pane)
}

fn window_focused(st: &State, m: &Model, pane: &str) -> bool {
    let Some(rt) = st.panes.get(pane) else {
        return false;
    };
    window_of(m, &rt.ws).is_some_and(|window| !st.unfocused.contains(&window))
}

/// The person is on the pane: its window in front, the pane focused and on
/// screen. A hand-back then is not news.
fn looking(st: &State, m: &Model, pane: &str) -> bool {
    let Some(rt) = st.panes.get(pane) else {
        return false;
    };
    let Some(ws) = m.workspace(&rt.ws) else {
        return false;
    };
    window_focused(st, m, pane)
        && focused_pane(m, ws).as_deref() == Some(pane)
        && visible(m, ws, pane)
}

/// The pane whose terminal has the keyboard in the focused window.
fn keyboard_pane(m: &Model) -> Option<PaneId> {
    let window = m.ui.focused_window;
    let ws = match window.and_then(|id| m.live.windows.get(&id)) {
        Some(WindowKind::Workspace(ws)) => m.workspace(ws),
        _ => m.active_workspace(),
    }?;
    focused_pane(m, ws)
}

/// Nothing else holds the keyboard: no dialog, no other region, no field.
fn keyboard_free(m: &Model) -> bool {
    m.ui.overlays.is_empty()
        && matches!(
            m.ui.focus,
            None | Some(Focus::Terminal(_)) | Some(Focus::Region(Region::Main))
        )
}

// ---- Keeping the panes in line with the model -------------------------------

/// What the panes depend on in the model, hashed: which panes exist, which
/// has the focus and which is zoomed, the theme and the look, what is on
/// screen.
fn signature(st: &State, m: &Model) -> u64 {
    let mut hash = std::hash::DefaultHasher::new();
    m.live.hydrated.hash(&mut hash);
    m.ui.view.hash(&mut hash);
    m.ui.overlays.len().hash(&mut hash);
    std::mem::discriminant(&m.ui.focus).hash(&mut hash);
    if let Some(Focus::Region(region)) = m.ui.focus {
        region.hash(&mut hash);
    }
    m.state().active_workspace_id.hash(&mut hash);
    let mut windows: Vec<String> = m
        .live
        .windows
        .iter()
        .map(|(id, kind)| format!("{id:?}{kind:?}"))
        .collect();
    windows.sort();
    windows.hash(&mut hash);
    let s = m.settings();
    (
        &s.theme_id,
        &s.font_family,
        s.font_size.to_bits(),
        s.line_height.to_bits(),
        s.cursor_blink,
        s.scrollback,
        s.padding.to_bits(),
    )
        .hash(&mut hash);
    format!("{:?}", s.cursor_style).hash(&mut hash);
    for id in &m.ui.opened_ids {
        let Some(ws) = m.workspace(id) else {
            continue;
        };
        (&ws.id, &ws.theme_id, ws.detached).hash(&mut hash);
        for pane in &ws.panes {
            pane.id.hash(&mut hash);
        }
        focused_pane(m, ws).hash(&mut hash);
        m.ui.zoom_by_workspace.get(&ws.id).hash(&mut hash);
    }
    let mut typing: Vec<&PaneId> = st
        .panes
        .iter()
        .filter(|(_, rt)| rt.search.as_ref().is_some_and(|s| s.typing))
        .map(|(id, _)| id)
        .collect();
    typing.sort();
    typing.hash(&mut hash);
    let mut unfocused: Vec<String> = st.unfocused.iter().map(|id| format!("{id:?}")).collect();
    unfocused.sort();
    unfocused.hash(&mut hash);
    hash.finish()
}

fn sync_if_needed(st: &mut State, m: &mut Model, sv: &Services) -> Task<Message> {
    if signature(st, m) == st.synced {
        Task::none()
    } else {
        sync(st, m, sv)
    }
}

/// Brings the panes in line with the model: starts the panes that appeared in
/// an opened workspace and stops those that went, ends a zoom whose pane
/// count changed, restyles, and gives the keyboard to the one terminal that
/// should have it.
fn sync(st: &mut State, m: &mut Model, sv: &Services) -> Task<Message> {
    let mut tasks = Vec::new();
    let wanted: Vec<(WsId, PaneId)> = if m.live.hydrated {
        m.ui.opened_ids
            .iter()
            .filter_map(|id| m.workspace(id))
            .flat_map(|ws| ws.panes.iter().map(|pane| (ws.id.clone(), pane.id.clone())))
            .collect()
    } else {
        Vec::new()
    };
    let wanted_ids: HashSet<&PaneId> = wanted.iter().map(|(_, pane)| pane).collect();

    // Gone: closed, restarted under a new id, its workspace closed or put to
    // sleep. Its prompts still queued go to its successor, if the restart
    // made one; closed for real, they go with it.
    let gone: Vec<PaneId> = st
        .panes
        .keys()
        .filter(|id| !wanted_ids.contains(id))
        .cloned()
        .collect();
    let now = Instant::now();
    st.carried
        .retain(|_, (at, _)| now.duration_since(*at) < CARRY);
    for id in gone {
        let Some(rt) = st.panes.remove(&id) else {
            continue;
        };
        let successor = m
            .workspace(&rt.ws)
            .is_some_and(|ws| ws.panes.iter().any(|p| p.name == rt.name && p.id != id));
        if successor && !rt.queue.is_empty() {
            st.carried
                .insert((rt.ws.clone(), rt.name.clone()), (now, rt.queue.clone()));
        }
        if st.hovered.as_ref() == Some(&id) {
            st.hovered = None;
        }
        tasks.push(controller::unmount(&st.hub, sv, rt));
    }

    for (ws_id, pane_id) in &wanted {
        if st.panes.contains_key(pane_id) {
            continue;
        }
        let Some((ws, pane)) = controller::find(m, ws_id, pane_id) else {
            continue;
        };
        let carried = st
            .carried
            .remove(&(ws_id.clone(), pane.name.clone()))
            .map(|(_, queue)| queue)
            .unwrap_or_default();
        let (rt, task) = controller::mount(&st.hub, &st.channel, sv, m, ws, pane, carried);
        st.panes.insert(pane_id.clone(), rt);
        tasks.push(task);
    }

    // A split, a close or a preset ends the zoom, as it does in tmux: the
    // person asked for a change of the arrangement and expects to see it.
    for id in m.ui.opened_ids.clone() {
        let Some(count) = m.workspace(&id).map(|ws| ws.panes.len()) else {
            continue;
        };
        if let Some(before) = st.pane_counts.insert(id.clone(), count)
            && before != count
            && m.ui.zoom_by_workspace.get(&id) == Some(&true)
        {
            m.ui.zoom_by_workspace.insert(id, false);
        }
    }

    let free = keyboard_free(m);
    let ids: Vec<PaneId> = st.panes.keys().cloned().collect();
    for id in ids {
        let is_looking = looking(st, m, &id);
        let Some(rt) = st.panes.get_mut(&id) else {
            continue;
        };
        let Some(ws) = m.workspace(&rt.ws) else {
            continue;
        };
        // A restyle never restarts the pane.
        let look = look::look(m.settings(), ws, rt.agent);
        if look != rt.look {
            let mut terminal = rt.link.lock();
            terminal.set_config(look.config.clone());
            terminal.set_palette(look.palette.clone());
            terminal.set_font(look.font.clone());
            drop(terminal);
            rt.look = look;
        }
        let typing = rt.search.as_ref().is_some_and(|search| search.typing);
        let wants = free
            && !typing
            && visible(m, ws, &id)
            && focused_pane(m, ws).as_deref() == Some(id.as_str());
        if wants != rt.focused {
            rt.focused = wants;
            let events = rt.link.lock().set_focused(wants);
            for event in events {
                if let TermEvent::Write(bytes) = event {
                    rt.link.write(bytes);
                }
            }
        }
        // Looking at the pane clears its badge, and a turn seen counts as
        // taken back; a dialog still waiting is still waiting.
        if is_looking {
            rt.attention = false;
            if rt.watch.activity == AgentActivity::Yours {
                rt.watch.activity = AgentActivity::Idle;
            }
        }
        // The menus are placed from their button on screen, and a hidden
        // workspace has none.
        if !visible(m, ws, &id) && st.picker.as_ref().is_some_and(|(open, _)| *open == id) {
            st.picker = None;
        }
    }

    st.synced = signature(st, m);
    Task::batch(tasks)
}

/// Republishes what other packages read of the panes.
fn publish(st: &State, m: &mut Model) {
    let mut reports = HashMap::with_capacity(st.panes.len());
    for (id, rt) in &st.panes {
        let Some(ws) = m.workspace(&rt.ws) else {
            continue;
        };
        let agent = ws
            .panes
            .iter()
            .find(|p| &p.id == id)
            .map_or(rt.agent, |p| p.agent);
        let reports_activity = agent != AgentId::Shell || rt.watch.bridged;
        let width = pane_width(st, m, ws, id);
        reports.insert(
            id.clone(),
            PaneReport {
                workspace: rt.ws.clone(),
                status: rt.status,
                activity: reports_activity.then_some(rt.watch.activity),
                bridged: rt.watch.bridged,
                attention: rt.attention,
                gauge: rt.watch.gauge.clone(),
                queued: rt.queue.len(),
                account: rt.account.clone(),
                folds: header::Fit::of(width - header::PADDING_X).into(),
            },
        );
    }
    if m.live.panes.reports != reports {
        m.live.panes.reports = reports;
        m.live.version += 1;
    }
}

/// A pane's width on screen, from its grid's last measured size.
fn pane_width(st: &State, m: &Model, ws: &Workspace, pane: &str) -> f32 {
    let size = st
        .grid_sizes
        .get(&ws.id)
        .copied()
        .unwrap_or(Size::new(1000.0, 700.0));
    if zoomed_pane(m, ws).as_deref() == Some(pane) {
        return size.width;
    }
    let tree = geometry::normalize_tree(&ws.panes, ws.tree.as_ref());
    geometry::compute_layout(&tree)
        .rect(pane)
        .map_or(size.width, |rect| rect.width * size.width)
}
