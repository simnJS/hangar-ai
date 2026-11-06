//! Hangar Mobile's push notifications, the window's side
//! (lib/remotePush.ts): what waits for the person in a project — the four
//! things the project manager is told (P19's manager feed) — and each file an
//! agent shares with the phone (`CoreEvent::RemoteShared`), pushed to every
//! subscribed phone by the PC itself (core's `remote_push`), app closed or
//! not.
//!
//! At most one notification per kind and workspace every 30 s: the first
//! waits a few seconds for what comes with it, the next ones wait out the
//! 30 s and go as one ("2 panes wait for you"). A wait that ended before its
//! turn is dropped. Nothing from the HQ. A few words in English and in
//! French, each phone getting its own language; a tap opens the screen it is
//! about: the pane, the shares, the project, For you.
//!
//! A state machine on the clock (`next_wake`, `fire_due`), as P19's engines
//! are: live.rs runs it on a tokio task and sends what it says.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use hangar_core::remote::Share;
use hangar_i18n::{Arg, Key, Locale, t, t_with};

use crate::orchestration::manager_feed::{ManagerEvent, ManagerSink};
use crate::state::{AppState, Workspace};

use super::engine::locked;

/// The least time between two notifications of a kind in a workspace.
pub const GAP_MS: u64 = 30_000;
/// What comes together goes together.
pub const GATHER_MS: u64 = 3_000;
/// What a notification lists by name before it says "…".
const NAMED: usize = 3;
const MAX_PENDING: usize = 50;

/// What is pushed: what the manager is told, or a share (`PushItem`).
#[derive(Debug, Clone, PartialEq)]
pub enum PushItem {
    Manager(ManagerEvent),
    Shared {
        workspace_id: String,
        workspace: String,
        share: Share,
    },
}

impl PushItem {
    fn kind(&self) -> &'static str {
        match self {
            PushItem::Manager(ManagerEvent::PaneWaiting { .. }) => "paneWaiting",
            PushItem::Manager(ManagerEvent::TaskNeedsYou { .. }) => "taskNeedsYou",
            PushItem::Manager(ManagerEvent::ReadyToLand { .. }) => "readyToLand",
            PushItem::Manager(ManagerEvent::BoardDone { .. }) => "boardDone",
            PushItem::Shared { .. } => "shared",
        }
    }

    fn workspace_id(&self) -> &str {
        match self {
            PushItem::Manager(
                ManagerEvent::PaneWaiting { workspace_id, .. }
                | ManagerEvent::TaskNeedsYou { workspace_id, .. }
                | ManagerEvent::ReadyToLand { workspace_id, .. }
                | ManagerEvent::BoardDone { workspace_id, .. },
            )
            | PushItem::Shared { workspace_id, .. } => workspace_id,
        }
    }

    fn workspace(&self) -> &str {
        match self {
            PushItem::Manager(
                ManagerEvent::PaneWaiting { workspace, .. }
                | ManagerEvent::TaskNeedsYou { workspace, .. }
                | ManagerEvent::ReadyToLand { workspace, .. }
                | ManagerEvent::BoardDone { workspace, .. },
            )
            | PushItem::Shared { workspace, .. } => workspace,
        }
    }

    /// Two items with the same key are one (`itemKey`).
    fn key(&self) -> &str {
        match self {
            PushItem::Manager(ManagerEvent::PaneWaiting { pane_id, .. }) => pane_id,
            PushItem::Manager(
                ManagerEvent::TaskNeedsYou { task_id, .. }
                | ManagerEvent::ReadyToLand { task_id, .. },
            ) => task_id,
            PushItem::Manager(ManagerEvent::BoardDone { .. }) => "",
            PushItem::Shared { share, .. } => &share.id,
        }
    }
}

/// One text in each language a phone may have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Localized {
    pub en: String,
    pub fr: String,
}

/// What `remote_push` takes: each phone gets the words of its language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushMessage {
    pub title: Localized,
    pub body: Localized,
    /// A screen of the phone app.
    pub url: String,
}

/// A notification's words in both languages: the templates are typeset by
/// the dictionary, what fills them is shown as written (`words`).
fn words(key: Key, vars: &[(&str, &str)]) -> Localized {
    let say = |locale| {
        if vars.is_empty() {
            t(locale, key).into_owned()
        } else {
            let args: Vec<(&str, Arg)> = vars
                .iter()
                .map(|(name, value)| (*name, Arg::from(*value)))
                .collect();
            t_with(locale, key, &args).into_owned()
        }
    };
    Localized {
        en: say(Locale::En),
        fr: say(Locale::Fr),
    }
}

/// `encodeURIComponent`: what a route's segment carries.
fn encode(segment: &str) -> String {
    let mut out = String::new();
    for byte in segment.bytes() {
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
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// The phone app's screens (`src/mobile/route.ts`), relative to where it is
/// served.
const FOR_YOU: &str = "./#/";
const SHARES: &str = "./#/shares";

fn pane_screen(id: &str) -> String {
    format!("./#/pane/{}", encode(id))
}

fn project_screen(id: &str) -> String {
    format!("./#/projects/{}", encode(id))
}

/// The first few, each once, and "…" past them.
fn names<'a>(list: impl Iterator<Item = &'a str>) -> String {
    let mut unique: Vec<&str> = Vec::new();
    for name in list {
        if !unique.contains(&name) {
            unique.push(name);
        }
    }
    let shown = unique
        .iter()
        .take(NAMED)
        .copied()
        .collect::<Vec<_>>()
        .join(", ");
    if unique.len() > NAMED {
        format!("{shown}…")
    } else {
        shown
    }
}

/// The notification for a batch of one kind in one workspace
/// (`pushMessage`).
pub fn push_message(items: &[PushItem]) -> Option<PushMessage> {
    let first = items.first()?;
    let place = first.workspace();
    let count = items.len().to_string();
    let n = items.len();
    Some(match first {
        PushItem::Manager(ManagerEvent::PaneWaiting { pane_id, pane, .. }) => {
            if n == 1 {
                PushMessage {
                    title: words(Key::PushPaneWaits, &[("pane", pane)]),
                    body: words(Key::PushPaneWaitsBody, &[("where", place)]),
                    url: pane_screen(pane_id),
                }
            } else {
                let list = names(items.iter().map(|item| match item {
                    PushItem::Manager(ManagerEvent::PaneWaiting { pane, .. }) => pane.as_str(),
                    _ => "",
                }));
                PushMessage {
                    title: words(Key::PushPanesWait, &[("n", &count)]),
                    body: words(Key::PushList, &[("where", place), ("list", &list)]),
                    url: FOR_YOU.into(),
                }
            }
        }
        PushItem::Manager(ManagerEvent::TaskNeedsYou { .. } | ManagerEvent::ReadyToLand { .. }) => {
            let list = names(items.iter().map(|item| match item {
                PushItem::Manager(
                    ManagerEvent::TaskNeedsYou { title, .. }
                    | ManagerEvent::ReadyToLand { title, .. },
                ) => title.as_str(),
                _ => "",
            }));
            let questions = matches!(first, PushItem::Manager(ManagerEvent::TaskNeedsYou { .. }));
            let title = match (n == 1, questions) {
                (true, true) => words(Key::PushQuestion, &[]),
                (true, false) => words(Key::PushReadyToLand, &[]),
                (false, true) => words(Key::PushQuestions, &[("n", &count)]),
                (false, false) => words(Key::PushReadyToLandMany, &[("n", &count)]),
            };
            PushMessage {
                title,
                body: words(Key::PushList, &[("where", place), ("list", &list)]),
                url: FOR_YOU.into(),
            }
        }
        PushItem::Manager(ManagerEvent::BoardDone { workspace_id, .. }) => {
            // The latest count is the board's.
            let tasks = match items.last() {
                Some(PushItem::Manager(ManagerEvent::BoardDone { tasks, .. })) => *tasks,
                _ => 0,
            };
            let tasks_text = tasks.to_string();
            PushMessage {
                title: words(Key::PushBoardDone, &[]),
                body: if tasks == 1 {
                    words(Key::PushBoardDoneOne, &[("where", place)])
                } else {
                    words(
                        Key::PushBoardDoneMany,
                        &[("where", place), ("n", &tasks_text)],
                    )
                },
                url: project_screen(workspace_id),
            }
        }
        PushItem::Shared { .. } => {
            let shares: Vec<&Share> = items
                .iter()
                .filter_map(|item| match item {
                    PushItem::Shared { share, .. } => Some(share),
                    _ => None,
                })
                .collect();
            if n == 1 {
                let share = shares[0];
                let caption = share.caption.as_deref().map(str::trim).unwrap_or("");
                let what = if caption.is_empty() {
                    share.name.as_str()
                } else {
                    caption
                };
                let image = share.mime.starts_with("image/");
                PushMessage {
                    title: words(
                        if image {
                            Key::PushSharedImage
                        } else {
                            Key::PushSharedFile
                        },
                        &[("pane", &share.pane)],
                    ),
                    body: words(Key::PushSharedWhat, &[("where", place), ("what", what)]),
                    url: SHARES.into(),
                }
            } else {
                let list = names(shares.iter().map(|share| share.name.as_str()));
                PushMessage {
                    title: words(Key::PushFiles, &[("n", &count)]),
                    body: words(Key::PushList, &[("where", place), ("list", &list)]),
                    url: SHARES.into(),
                }
            }
        }
    })
}

/// A batch as it goes: repeats folded, the latest kept, waits that ended
/// dropped (`settlePush`).
pub fn settle_push(batch: Vec<PushItem>, is_waiting: &dyn Fn(&str) -> bool) -> Vec<PushItem> {
    let mut seen: Vec<PushItem> = Vec::new();
    for item in batch {
        if let PushItem::Manager(ManagerEvent::PaneWaiting { pane_id, .. }) = &item
            && !is_waiting(pane_id)
        {
            continue;
        }
        // A board done twice says its latest count.
        seen.retain(|known| known.key() != item.key());
        seen.push(item);
    }
    seen
}

fn path_key(path: &str) -> String {
    let slashed = path.replace('\\', "/");
    let slashed = slashed.strip_prefix("//?/").unwrap_or(&slashed);
    slashed.trim_end_matches('/').to_lowercase()
}

/// The workspace a share came from. A share names its folder, which is a
/// workspace's own, or a pane's (a worktree); failing both, the pane's name,
/// when one workspace only has a pane by that name (`workspaceOfShare`).
pub fn workspace_of_share<'a>(state: &'a AppState, share: &Share) -> Option<&'a Workspace> {
    let folder = path_key(&share.workspace);
    let by_folder = state
        .workspaces
        .iter()
        .find(|ws| path_key(&ws.cwd) == folder)
        .or_else(|| {
            state.workspaces.iter().find(|ws| {
                ws.panes.iter().any(|pane| {
                    pane.cwd
                        .as_deref()
                        .is_some_and(|cwd| !cwd.is_empty() && path_key(cwd) == folder)
                })
            })
        });
    if by_folder.is_some() {
        return by_folder;
    }
    let by_pane: Vec<&Workspace> = state
        .workspaces
        .iter()
        .filter(|ws| ws.panes.iter().any(|pane| pane.name == share.pane))
        .collect();
    match by_pane.as_slice() {
        [only] => Some(only),
        _ => None,
    }
}

/// A folder no workspace has: named after its last part.
pub fn folder_name(folder: &str) -> String {
    let trimmed = folder.trim_end_matches(['\\', '/']);
    trimmed
        .rsplit(['\\', '/'])
        .next()
        .filter(|last| !last.is_empty())
        .unwrap_or(folder)
        .to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Timer {
    at: u64,
    seq: u64,
}

#[derive(Debug, Default)]
struct Slot {
    last_sent_at: Option<u64>,
    pending: Vec<PushItem>,
    timer: Option<Timer>,
}

/// The push (`createRemotePush`).
#[derive(Debug, Default)]
pub struct Push {
    slots: HashMap<(String, String), Slot>,
    subscribers: usize,
    stopped: bool,
    seq: u64,
}

impl Push {
    /// Phones take notifications, and the push has not been stopped.
    pub fn active(&self) -> bool {
        !self.stopped && self.subscribers > 0
    }

    pub fn add(&mut self, item: PushItem, state: &AppState, now: u64) {
        let hq = state
            .workspaces
            .iter()
            .any(|ws| ws.id == item.workspace_id() && ws.hq == Some(true));
        if !self.active() || hq {
            return;
        }
        self.seq += 1;
        let seq = self.seq;
        let key = (item.kind().to_string(), item.workspace_id().to_string());
        let slot = self.slots.entry(key).or_default();
        slot.pending.push(item);
        if slot.pending.len() > MAX_PENDING {
            let over = slot.pending.len() - MAX_PENDING;
            slot.pending.drain(..over);
        }
        if slot.timer.is_some() {
            return;
        }
        let gap = slot
            .last_sent_at
            .map_or(0, |at| (at + GAP_MS).saturating_sub(now));
        slot.timer = Some(Timer {
            at: now + GATHER_MS.max(gap),
            seq,
        });
    }

    /// An agent shared a file with the phones.
    pub fn shared(&mut self, share: &Share, state: &AppState, now: u64) {
        if !self.active() {
            return;
        }
        let ws = workspace_of_share(state, share);
        if ws.is_some_and(|ws| ws.hq == Some(true)) {
            return;
        }
        let item = PushItem::Shared {
            workspace_id: ws.map_or_else(|| path_key(&share.workspace), |ws| ws.id.clone()),
            workspace: ws.map_or_else(|| folder_name(&share.workspace), |ws| ws.name.clone()),
            share: share.clone(),
        };
        self.add(item, state, now);
    }

    /// How many phones listen, as the PC just said; none drops what waited
    /// to go (`refresh`).
    pub fn set_subscribers(&mut self, count: usize) {
        if self.stopped {
            return;
        }
        self.subscribers = count;
        if count == 0 {
            self.slots.clear();
        }
    }

    /// What the PC answered when asked how many phones listen: one that
    /// cannot say turns the push off, as none listening does.
    pub fn refreshed(&mut self, answer: Result<usize, String>) {
        self.set_subscribers(answer.unwrap_or(0));
    }

    #[cfg(test)]
    pub fn stop(&mut self) {
        self.stopped = true;
        self.slots.clear();
    }

    /// Timers waiting to fire.
    #[cfg(test)]
    pub fn pending(&self) -> usize {
        self.slots
            .values()
            .filter(|slot| slot.timer.is_some())
            .count()
    }

    pub fn next_wake(&self) -> Option<u64> {
        self.slots
            .values()
            .filter_map(|slot| slot.timer)
            .min_by_key(|timer| (timer.at, timer.seq))
            .map(|timer| timer.at)
    }

    /// Fires the earliest timer due by `now`, and answers the notification
    /// it makes, if any; `None` too when nothing was due.
    pub fn fire_due(&mut self, now: u64, is_waiting: &dyn Fn(&str) -> bool) -> Option<PushMessage> {
        let slot = self
            .slots
            .values_mut()
            .filter(|slot| slot.timer.is_some_and(|timer| timer.at <= now))
            .min_by_key(|slot| slot.timer.map(|timer| (timer.at, timer.seq)))?;
        slot.timer = None;
        let batch = std::mem::take(&mut slot.pending);
        if self.stopped || self.subscribers == 0 {
            return None;
        }
        let message = push_message(&settle_push(batch, is_waiting))?;
        slot.last_sent_at = Some(now);
        Some(message)
    }

    /// Whether a timer is due by `now`.
    pub fn due(&self, now: u64) -> bool {
        self.next_wake().is_some_and(|at| at <= now)
    }
}

/// The manager feed's second sink: what it is told is queued for the push.
/// P19 calls it with its engines locked, so it only queues and returns.
pub struct PushSink {
    pub push: Arc<Mutex<Push>>,
    pub state: Box<dyn Fn() -> Arc<AppState> + Send>,
    pub now: Box<dyn Fn() -> u64 + Send>,
    /// Rung when a timer may have come closer.
    pub wake: Box<dyn Fn() + Send>,
}

impl ManagerSink for PushSink {
    fn active(&self) -> bool {
        locked(&self.push).active()
    }

    fn post(&mut self, event: &ManagerEvent) -> Result<(), String> {
        let state = (self.state)();
        locked(&self.push).add(PushItem::Manager(event.clone()), &state, (self.now)());
        (self.wake)();
        Ok(())
    }
}
