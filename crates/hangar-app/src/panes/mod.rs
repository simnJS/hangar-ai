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
//! | `controller` | A pane's life: mount, spawn (with the MCP scope), launch of its agent, exit, unmount, the restart a role asks for |
//! | `term_widget` | hangar-term's widget over the locked terminal |
//! | `grid` | The split grid: slots, boundaries, moves, zoom, file drops |
//! | `workspace_terminals`, `terminal_pane`, `header`, `search`, `zoom_bar`, `gauge` | The views |
//! | `status`, `look` | Pure logic, with the TS tests (the layout tree is `state::layout`) |
//!
//! Terminals live in this package's state, never in a window: a hidden
//! workspace draws nothing, and its panes keep consuming their output, on
//! their own reader threads, all the same. A detached workspace is only
//! drawn by another window.
//!
//! Where each agent stands is P4's (`panes_logic`): this package tells it
//! what happens to a pane (`panes_logic::Msg::{Mounted, Unmounted, Status,
//! Output, Bell, Notification, Key, Typed, WatchSession}`) and draws what it
//! publishes.

mod controller;
mod gauge;

mod grid;
mod header;
mod hub;
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

use hangar_term::{Action, ClipboardTarget, Event as TermEvent, ScrollTo};
use iced::futures::stream;
use iced::widget::Space;
use iced::{Element, Size, Subscription, Task};

use crate::ctx::Ctx;
use crate::message::{Intent, Message, TerminalAction};
use crate::pane_menu::Folded;
use crate::panes_logic::Msg as Logic;
use crate::services::{CoreEvent, Services, TerminalAccess};
use crate::shell::{Focus, Region, View, WindowKind};
use crate::state::layout;
use crate::state::live::{PaneMcp, PaneStatus};
use crate::state::types::{DropZone, Pane, SplitDir};
use crate::state::{Model, PaneId, Patch, StoreAction, Workspace, WsId};

use controller::{PaneRt, SpawnReport};
use hub::{Hub, HubChannel};
use pty_link::{HubEvent, HubKind};

pub use search::SearchMsg;

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
    /// Files dropped in the batch of events being handled, joined into one
    /// paste per pane.
    drops: Vec<(PaneId, PathBuf)>,
    /// The header under the pointer, which shows its actions.
    hovered: Option<PaneId>,
    /// An agent or shell picker open in a header.
    picker: Option<(PaneId, Picker)>,
    /// The signature of the model the last sync brought the panes in line
    /// with.
    synced: u64,
}

/// The pickers of a header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Picker {
    Agent,
    Shell,
}

/// What other packages read of the panes; lives in `Model.live.panes`. The
/// agent's side of a pane (activity, insight, attention, status) is P4's
/// `panes_logic::Live`.
#[derive(Debug, Default)]
pub struct Live {
    reports: HashMap<PaneId, PaneReport>,
}

/// One mounted pane, as the rest of the window sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct PaneReport {
    pub workspace: WsId,
    /// The header buttons its width folded into the "…" menu.
    pub folds: Folded,
    /// The Claude account its Claude Code was started on.
    pub account: Option<String>,
    /// The MCP servers its Claude Code was started with (`PaneInsight.mcp`
    /// of the TS, written by the launch).
    pub mcp: Option<PaneMcp>,
}

impl Live {
    pub fn report(&self, pane: &str) -> Option<&PaneReport> {
        self.reports.get(pane)
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
    /// The terminal had its time to measure itself: start the shell.
    SpawnDue(PaneId),
    Spawned(PaneId, Result<SpawnReport, String>),
    /// The launch delay is over: whether the session to resume exists, and
    /// the transcripts known before the launch.
    LaunchReady(PaneId, Option<bool>, HashSet<String>),
    LoginReady(PaneId),
    /// Whether the session a role's restart resumes exists.
    RoleRestart(WsId, PaneId, String, bool),
    FlushDrops,
    Paste(PaneId, Option<String>),
    /// A press in a pane.
    Focus(WsId, PaneId),
    Hover(Option<PaneId>),
    OpenPicker(PaneId, Picker),
    ClosePicker,
    SetAgent(WsId, PaneId, crate::state::AgentId),
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
    SetRatio(WsId, layout::Path, f64),
    Move {
        ws: WsId,
        drag: PaneId,
        target: PaneId,
        zone: DropZone,
    },
    Search(PaneId, SearchMsg),
}

fn done(message: Message) -> Task<Message> {
    Task::done(message)
}

fn intent(intent: Intent) -> Task<Message> {
    done(Message::Intent(intent))
}

fn logic(msg: Logic) -> Task<Message> {
    done(Message::PanesLogic(msg))
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    // Keys, pointer moves, scrolls and ticks of a terminal come many times a
    // second, and only a measure or a press can change what the sync and the
    // reports depend on (`term_settles`).
    if let Msg::Term(pane, action) = msg {
        let (task, settles) = term_action(st, m, sv, pane, action);
        return if settles {
            settle(st, m, sv, task)
        } else {
            task
        };
    }
    // Output is the hot path, many batches a second while agents stream:
    // nothing other packages read changes with it.
    let quiet =
        matches!(&msg, Msg::Hub(events) if events.iter().all(|e| e.kind == HubKind::Output));
    let task = handle(st, m, sv, msg);
    if quiet {
        return task;
    }
    settle(st, m, sv, task)
}

/// `task`, once the panes are brought in line with the model and what other
/// packages read of them is republished.
fn settle(st: &mut State, m: &mut Model, sv: &Services, task: Task<Message>) -> Task<Message> {
    let synced = sync_if_needed(st, m, sv);
    publish(st, m);
    Task::batch([task, synced])
}

fn handle(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    match msg {
        // `update` takes these itself; here only for the match to be whole.
        Msg::Term(pane, action) => term_action(st, m, sv, pane, action).0,
        Msg::Hub(events) => hub_events(st, m, sv, events),
        Msg::Sync => sync(st, m, sv),
        Msg::SpawnDue(pane) => match st.panes.get_mut(&pane) {
            Some(rt) => controller::start(rt, sv, &st.channel),
            None => Task::none(),
        },
        Msg::Spawned(pane, result) => match st.panes.get_mut(&pane) {
            Some(rt) => controller::spawned(rt, m, result),
            // The pane went while its shell was starting: the shell would
            // outlive it.
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
        },
        Msg::LaunchReady(pane, exists, known) => match st.panes.get_mut(&pane) {
            Some(rt) => controller::launch_ready(rt, m, exists, known),
            None => Task::none(),
        },
        Msg::LoginReady(pane) => {
            if let Some(rt) = st.panes.get_mut(&pane) {
                controller::login_ready(rt);
            }
            Task::none()
        }
        Msg::RoleRestart(ws, pane, session, found) => match st.panes.get(&pane) {
            Some(rt) => controller::role_restart_answered(rt, ws, session, found, m.locale()),
            None => Task::none(),
        },
        Msg::FlushDrops => flush_drops(st, m),
        Msg::Paste(pane, text) => {
            if let (Some(rt), Some(text)) = (st.panes.get(&pane), text) {
                let bytes = rt.link.lock().paste_bytes(&text);
                rt.link.write(bytes);
            }
            Task::none()
        }
        // Clicking in a pane is using its workspace.
        Msg::Focus(ws, pane) => Task::batch([
            logic(Logic::Typed {
                workspace: ws.clone(),
            }),
            intent(Intent::FocusPane {
                ws,
                pane,
                terminal: true,
            }),
        ]),
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
        // Without a direction the shell splits along the pane's longer side.
        Msg::Split(ws, pane, dir) => intent(Intent::SplitPane {
            ws,
            near: Some(pane),
            dir,
        }),
        Msg::ToggleZoom(ws, pane) => {
            let zoomed = m.workspace(&ws).and_then(|w| zoomed_pane(m, w));
            let target = (zoomed.as_deref() != Some(pane.as_str())).then_some(pane);
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
                let tree = layout::normalize_tree(&workspace.panes, workspace.tree.as_ref());
                let next = layout::with_ratio(&tree, &path, ratio);
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

/// Whether what a terminal widget did can change what the sync and the
/// reports depend on: a measure (`measured`, an `Action::Resize`; the first
/// one starts the shell) or a press among the terminal's `events` (which
/// gives the pane the focus, and the keyboard back from its search bar).
/// Typing, pointer moves, scrolls and ticks cannot.
fn term_settles(measured: bool, events: &[TermEvent]) -> bool {
    measured
        || events
            .iter()
            .any(|event| matches!(event, TermEvent::Pressed))
}

/// What a terminal widget did, applied, and what the terminal said back;
/// with whether the panes have to be brought in line after it
/// ([`term_settles`]).
fn term_action(
    st: &mut State,
    m: &Model,
    sv: &Services,
    pane: PaneId,
    action: Action,
) -> (Task<Message>, bool) {
    let Some(rt) = st.panes.get_mut(&pane) else {
        return (Task::none(), false);
    };
    // The first measure of the terminal is the size its shell starts at.
    let measured = matches!(action, Action::Resize(_));
    let events = rt.link.lock().perform(action);
    let settles = term_settles(measured, &events);
    let mut tasks = Vec::new();
    if measured {
        tasks.push(controller::start(rt, sv, &st.channel));
    }
    // What the person copies (Ctrl+Shift+C, the selection on Linux) comes as
    // `SetClipboard` here; what a program asks to copy (`OSC 52`) never
    // reaches the app (`look::config`).
    for event in events {
        match event {
            TermEvent::Write(bytes) => rt.link.write(bytes),
            TermEvent::Resize(size) => rt.link.resize(size.columns, size.rows),
            TermEvent::SetClipboard { target, text } => tasks.push(match target {
                ClipboardTarget::Clipboard => iced::clipboard::write(text),
                ClipboardTarget::Primary => iced::clipboard::write_primary(text),
            }),
            // Web links open in the system browser (inventory §9.2), as
            // xterm.js's web-links addon was the only one the TS opened. The
            // terminal reports no other (a `file:`, `ssh:` or `git:` URL, or
            // a hyperlink elsewhere, would hand the system's handler for its
            // scheme anything a program printed); this checks it again
            // right before the opener.
            TermEvent::OpenLink(link) if hangar_term::is_web_link(&link.uri) => {
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
            // A key pressed in the pane, not the terminal answering its
            // program: you taking it back, or answering what it waited on.
            // On screen, it is using the workspace too.
            TermEvent::UserInput => {
                tasks.push(logic(Logic::Key { pane: pane.clone() }));
                if m.workspace(&rt.ws).is_some_and(|ws| visible(m, ws, &pane)) {
                    tasks.push(logic(Logic::Typed {
                        workspace: rt.ws.clone(),
                    }));
                }
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
    (Task::batch(tasks), settles)
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
        let text = crate::panes_logic::file_drop::drop_text(&paths, model_pane.agent, shell);
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

/// What the reader threads said: output (one wake-up per pane per batch),
/// an exit, a bell, a notification.
fn hub_events(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    events: Vec<HubEvent>,
) -> Task<Message> {
    let mut tasks = Vec::new();
    for event in events {
        let Some(rt) = st.panes.get_mut(&event.pane) else {
            continue;
        };
        let pane = event.pane;
        match event.kind {
            HubKind::Output => {
                rt.link.take_wake();
                tasks.push(logic(Logic::Output { pane }));
            }
            HubKind::Exit => controller::exited(rt, m, sv),
            HubKind::Bell => tasks.push(logic(Logic::Bell { pane })),
            // OSC 9 and OSC 777, already read by hangar-term (ConEmu's
            // numeric sub-commands left out): the message, or the title when
            // there is no body (`osc777_notification`).
            HubKind::Notification { title, body } => {
                let message = [body.trim(), title.as_deref().unwrap_or_default().trim()]
                    .into_iter()
                    .find(|text| !text.is_empty())
                    .unwrap_or_default()
                    .to_owned();
                tasks.push(logic(Logic::Notification { pane, message }));
            }
        }
    }
    Task::batch(tasks)
}

/// The `terminal.*` commands on a pane's terminal (App.tsx:1451, through
/// `terminalRegistry.getTerminal(focused)`), as the command table sends
/// them (`Intent::TerminalAction`).
fn terminal_action(st: &mut State, pane: &PaneId, action: TerminalAction) -> Task<Message> {
    let Some(rt) = st.panes.get_mut(pane) else {
        return Task::none();
    };
    match action {
        TerminalAction::Clear => rt.link.lock().clear(),
        TerminalAction::Copy => {
            if let Some(text) = rt.link.lock().selection_text() {
                return iced::clipboard::write(text);
            }
        }
        TerminalAction::Paste => {
            let pane = pane.clone();
            return iced::clipboard::read()
                .map(move |text| Message::Panes(Msg::Paste(pane.clone(), text)));
        }
        TerminalAction::SelectAll => rt.link.lock().select_all(),
        TerminalAction::ScrollTop => {
            rt.link.lock().perform(Action::Scroll(ScrollTo::Top));
        }
        TerminalAction::ScrollBottom => rt.link.lock().scroll_to_bottom(),
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
    let mut subscriptions = vec![st.channel.subscription()];
    // Whatever changed the model outside this package's own messages (a store
    // action of the shell, an engine's command, a hydration, a sleep) is
    // caught here: the subscription is rebuilt after every update, and a
    // signature that differs from the last sync's fires one `Sync`.
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
    // The focus and the zoom are the shell's to write (`FocusPane`,
    // `SetZoom`); the sync below gives the keyboard to the terminal they now
    // name. A respawn (`PaneRespawned`) is a pane id gone and another come,
    // which the same sync unmounts and mounts.
    let task = match intent {
        Intent::TerminalAction { pane, action } => Some(terminal_action(st, pane, *action)),
        Intent::OpenFinder(pane) => Some(open_finder(st, pane)),
        Intent::Broadcast {
            ws,
            text,
            when_free,
        } => Some(broadcast(st, m, sv, ws, text, *when_free)),
        Intent::InsertTranscript(text) => {
            // For the terminal that has the keyboard; a text field with the
            // focus is its own package's to fill.
            match keyboard_pane(m)
                .filter(|_| keyboard_free(m))
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
    Some(match task {
        Some(task) => Task::batch([task, synced]),
        None => synced,
    })
}

/// Types `text` into every pane of the workspace; with `when_free`, an agent
/// whose mod reports gets it queued for when it is free (App.tsx's
/// broadcast).
fn broadcast(
    st: &State,
    m: &Model,
    sv: &Services,
    ws: &str,
    text: &str,
    when_free: bool,
) -> Task<Message> {
    let mut queued = Vec::new();
    for (id, rt) in st.panes.iter().filter(|(_, rt)| rt.ws == ws) {
        let bridged = m
            .live
            .panes_logic
            .pane_activity(id)
            .is_some_and(|activity| activity.bridged);
        if when_free && bridged {
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

/// The mod's reports and the queue are P4's; a report may end a turn, which
/// a restart waiting for its role then takes (the next sync).
pub fn on_event(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    event: &CoreEvent,
) -> Option<Task<Message>> {
    match event {
        CoreEvent::Bridge { .. } | CoreEvent::BridgeQueue { .. } | CoreEvent::PtyExit { .. } => {
            Some(sync_if_needed(st, m, sv))
        }
        _ => None,
    }
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

/// A grid's size before it has measured itself.
const UNMEASURED: Size = Size::new(1000.0, 700.0);

// ---- Where the panes stand --------------------------------------------------

/// The pane of `ws` that has the focus: the one the shell remembers, else
/// the first in reading order.
pub(crate) fn focused_pane(m: &Model, ws: &Workspace) -> Option<PaneId> {
    if let Some(pane) = m.ui.focus_by_workspace.get(&ws.id)
        && ws.panes.iter().any(|p| &p.id == pane)
    {
        return Some(pane.clone());
    }
    let tree = layout::normalize_tree(&ws.panes, ws.tree.as_ref());
    crate::state::layout::leaf_ids(&tree)
        .into_iter()
        .find(|id| !id.is_empty())
}

/// The pane drawn over the whole grid: the focused one, while the
/// workspace's zoom is on and it has more than one pane. The zoom follows
/// the focus around.
pub(crate) fn zoomed_pane(m: &Model, ws: &Workspace) -> Option<PaneId> {
    let on = m.ui.zoom_by_workspace.get(&ws.id).copied().unwrap_or(false);
    (on && ws.panes.len() > 1)
        .then(|| focused_pane(m, ws))
        .flatten()
}

/// Workspace `ws` is on screen, its grid shown: in its own window, or the
/// active one of the main window.
fn on_screen(m: &Model, ws: &Workspace) -> bool {
    if !m.live.hydrated || m.ui.view != View::Terminals {
        return false;
    }
    m.live.windows.values().any(|kind| match kind {
        WindowKind::Workspace(id) => *id == ws.id,
        WindowKind::Main => {
            ws.detached != Some(true)
                && m.state().active_workspace_id.as_deref() == Some(ws.id.as_str())
        }
    })
}

/// The pane is on screen: its workspace is, and no other pane is zoomed over
/// it.
fn visible(m: &Model, ws: &Workspace, pane: &str) -> bool {
    on_screen(m, ws) && zoomed_pane(m, ws).is_none_or(|zoomed| zoomed == pane)
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

/// What the panes depend on in the model, hashed: which panes exist and with
/// what role, which has the focus and which is zoomed, the theme and the
/// look, what is on screen, and whether a pane waiting to restart for its
/// role is still in a turn.
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
            if pane.role.is_some() {
                format!("{:?}", pane.role).hash(&mut hash);
                pane.session_id.hash(&mut hash);
                let activity = m.live.panes_logic.activity_or_idle(&pane.id);
                crate::orchestration_ui::actions::is_mid_turn(activity).hash(&mut hash);
            }
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
    let mut running: Vec<&PaneId> = st
        .panes
        .iter()
        .filter(|(_, rt)| rt.status == PaneStatus::Running)
        .map(|(id, _)| id)
        .collect();
    running.sort();
    running.hash(&mut hash);
    hash.finish()
}

fn sync_if_needed(st: &mut State, m: &mut Model, sv: &Services) -> Task<Message> {
    if signature(st, m) == st.synced {
        Task::none()
    } else {
        sync(st, m, sv)
    }
}

/// Brings the panes in line with the model: starts the panes that appeared
/// in an opened workspace and stops those that went (a workspace that leaves
/// `opened_ids`, which is how a sleep unmounts it, takes its terminals with
/// it), ends a zoom whose pane count changed, restyles, gives the keyboard
/// to the one terminal that should have it, and restarts a pane a role
/// asks it to.
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
    // sleep.
    let gone: Vec<PaneId> = st
        .panes
        .keys()
        .filter(|id| !wanted_ids.contains(id))
        .cloned()
        .collect();
    for id in gone {
        let Some(rt) = st.panes.remove(&id) else {
            continue;
        };
        if st.hovered.as_ref() == Some(&id) {
            st.hovered = None;
        }
        if st.picker.as_ref().is_some_and(|(open, _)| *open == id) {
            st.picker = None;
        }
        tasks.push(controller::unmount(&st.hub, sv, m, rt));
    }

    for (ws_id, pane_id) in &wanted {
        if st.panes.contains_key(pane_id) {
            continue;
        }
        let Some((ws, pane)) = controller::find(m, ws_id, pane_id) else {
            continue;
        };
        let (rt, task) = controller::mount(&st.hub, sv, m, ws, pane);
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
    let locale = m.locale();
    for (id, rt) in st.panes.iter_mut() {
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
            && visible(m, ws, id)
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
        tasks.push(controller::role_check(rt, m, locale));
    }
    // The menus are placed from their button on screen, and a hidden
    // workspace has none.
    if let Some((open, _)) = &st.picker {
        let shown = st
            .panes
            .get(open)
            .and_then(|rt| m.workspace(&rt.ws))
            .is_some_and(|ws| visible(m, ws, open));
        if !shown {
            st.picker = None;
        }
    }

    st.synced = signature(st, m);
    Task::batch(tasks)
}

/// Republishes what other packages read of the panes.
fn publish(st: &State, m: &mut Model) {
    let mut reports = HashMap::with_capacity(st.panes.len());
    let mut layouts: HashMap<&str, layout::GridLayout> = HashMap::new();
    for (id, rt) in &st.panes {
        let Some(ws) = m.workspace(&rt.ws) else {
            continue;
        };
        let layout = layouts.entry(ws.id.as_str()).or_insert_with(|| {
            layout::compute_layout(&layout::normalize_tree(&ws.panes, ws.tree.as_ref()))
        });
        let width = pane_width(st, m, ws, layout, id);
        reports.insert(
            id.clone(),
            PaneReport {
                workspace: rt.ws.clone(),
                folds: header::Fit::of(width - header::PADDING_X).folded(),
                account: rt.account.clone(),
                mcp: rt.mcp.clone(),
            },
        );
    }
    if m.live.panes.reports != reports {
        m.live.panes.reports = reports;
        m.live.version += 1;
    }
}

/// A pane's width on screen, from its grid's last measured size.
fn pane_width(
    st: &State,
    m: &Model,
    ws: &Workspace,
    layout: &layout::GridLayout,
    pane: &str,
) -> f32 {
    let size = st.grid_sizes.get(&ws.id).copied().unwrap_or(UNMEASURED);
    if zoomed_pane(m, ws).as_deref() == Some(pane) {
        return size.width;
    }
    layout
        .rects
        .get(pane)
        .map_or(size.width, |rect| rect.width as f32 * size.width)
}

#[cfg(test)]
mod tests {
    use hangar_term::{Link, LinkSource};

    use super::*;

    /// The link a Ctrl+click at `column` of the first row reports, in a
    /// pane's terminal that printed `text`.
    fn ctrl_click(text: &str, column: i32) -> Option<String> {
        use crate::state::types::{AgentId, Settings};
        use hangar_term::{Button, CellPos, Pointer, Side, Terminal};
        let config = look::config(&Settings::default(), AgentId::Claude);
        let mut terminal = Terminal::new(config, 60, 3);
        terminal.feed(text.as_bytes());
        let events = terminal.perform(Action::Pointer(Pointer::Down {
            button: Button::Left,
            at: CellPos {
                column,
                row: 0,
                side: Side::Left,
            },
            modifiers: iced::keyboard::Modifiers::CTRL,
        }));
        events.into_iter().find_map(|event| match event {
            TermEvent::OpenLink(link) => Some(link.uri),
            _ => None,
        })
    }

    #[test]
    fn a_ctrl_click_on_a_file_link_or_a_hyperlink_elsewhere_opens_nothing() {
        // A pane's terminal reports neither as a link (nor underlines it).
        assert_eq!(ctrl_click("see file:///etc/passwd now\r\n", 8), None);
        assert_eq!(
            ctrl_click(
                "\x1b]8;;ssh://attacker.example/x\x1b\\click me\x1b]8;;\x1b\\\r\n",
                2,
            ),
            None
        );
        // A web link still opens.
        let web = ctrl_click("see https://hangar.ai/docs now\r\n", 8);
        assert!(hangar_term::is_web_link(&web.expect("a link")));
    }

    #[test]
    fn only_a_measure_or_a_press_settles_the_panes() {
        let link = || {
            TermEvent::OpenLink(Link {
                uri: "https://example.com".into(),
                source: LinkSource::Detected,
            })
        };
        assert!(term_settles(true, &[]));
        assert!(term_settles(
            false,
            &[TermEvent::UserInput, TermEvent::Pressed]
        ));
        // Typing, a pointer move over a link, a scroll, a tick.
        assert!(!term_settles(false, &[]));
        assert!(!term_settles(false, &[TermEvent::UserInput]));
        assert!(!term_settles(false, &[link()]));
        assert!(!term_settles(
            false,
            &[TermEvent::Write(b"\x1b[A".to_vec()), TermEvent::UserInput]
        ));
    }
}
