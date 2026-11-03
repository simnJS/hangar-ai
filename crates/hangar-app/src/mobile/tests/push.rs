//! remotePush.test.ts, case for case, on the fake clock of P19's testkit.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use hangar_core::board::Task;
use hangar_core::remote::{Share, ShareKind};

use crate::orchestration::events::EngineIo;
use crate::orchestration::events::timing::MAX_WAIT_MS;
use crate::orchestration::manager_feed::{ManagerEvent, ManagerFeed};
use crate::orchestration::testkit::{START, Timed, advance, strings, task};
use crate::state::types::PaneRole;
use crate::state::{AgentActivity, AppState, Pane, PaneActivity, Workspace};

use super::super::engine::locked;
use super::super::push::{
    GAP_MS, GATHER_MS, Localized, Push, PushItem, PushMessage, PushSink, push_message, settle_push,
    workspace_of_share,
};
use super::fake::claude;

const NNBSP: char = '\u{202f}';

fn ws(id: &str, name: &str, cwd: &str, panes: Vec<Pane>, hq: bool) -> Workspace {
    Workspace {
        id: id.into(),
        name: name.into(),
        cwd: cwd.into(),
        panes,
        hq: hq.then_some(true),
        ..Workspace::default()
    }
}

fn state() -> AppState {
    AppState {
        workspaces: vec![
            ws(
                "hq",
                "HQ",
                "C:\\Users\\me\\AppData\\Roaming\\hangar\\hq",
                vec![claude("m", "Max", Some(PaneRole::Manager))],
                true,
            ),
            ws(
                "a",
                "Shop",
                "C:\\dev\\shop",
                vec![
                    claude("o", "Jack", Some(PaneRole::Orchestrator)),
                    Pane {
                        cwd: Some("C:\\dev\\shop-worktrees\\ada".into()),
                        ..claude("w1", "Ada", Some(PaneRole::Worker))
                    },
                    claude("w2", "Ben", Some(PaneRole::Worker)),
                ],
                false,
            ),
            ws(
                "b",
                "Blog",
                "/home/me/blog",
                vec![claude("p", "Cora", None)],
                false,
            ),
        ],
        active_workspace_id: Some("a".into()),
        ..AppState::default()
    }
}

/// The push on a clock the test moves, and what it sent, when.
struct Rig {
    t: u64,
    push: Push,
    world: AppState,
    waiting: HashSet<String>,
    sent: Vec<(u64, PushMessage)>,
    subscribers: Result<usize, String>,
}

impl Timed for Rig {
    fn next_wake(&self) -> Option<u64> {
        self.push.next_wake()
    }

    fn fire_due(&mut self, now: u64) {
        let waiting = self.waiting.clone();
        if let Some(message) = self.push.fire_due(now, &|pane| waiting.contains(pane)) {
            self.sent.push((now, message));
        }
    }
}

impl Rig {
    fn new(subscribers: usize) -> Rig {
        Rig {
            t: START,
            push: Push::default(),
            world: state(),
            waiting: HashSet::new(),
            sent: Vec::new(),
            subscribers: Ok(subscribers),
        }
    }

    /// Asks the PC again how many phones listen.
    fn refresh(&mut self) {
        self.push.refreshed(self.subscribers.clone());
    }

    fn post(&mut self, event: ManagerEvent) {
        self.push.add(PushItem::Manager(event), &self.world, self.t);
    }

    fn shared(&mut self, share: Share) {
        self.push.shared(&share, &self.world, self.t);
    }

    fn advance(&mut self, ms: u64) {
        let mut t = self.t;
        advance(&mut t, ms, self);
        self.t = t;
    }

    fn titles(&self) -> Vec<String> {
        self.sent
            .iter()
            .map(|(_, message)| message.title.en.clone())
            .collect()
    }
}

fn waits(pane: &str, name: &str, workspace_id: &str, workspace: &str) -> ManagerEvent {
    ManagerEvent::PaneWaiting {
        workspace_id: workspace_id.into(),
        workspace: workspace.into(),
        pane_id: pane.into(),
        pane: name.into(),
        for_ms: 120_000,
    }
}

fn question(task: &str, title: &str, workspace_id: &str, workspace: &str) -> ManagerEvent {
    ManagerEvent::TaskNeedsYou {
        workspace_id: workspace_id.into(),
        workspace: workspace.into(),
        task_id: task.into(),
        title: title.into(),
        reason: "Jack: which db?".into(),
    }
}

fn lands(task: &str, title: &str) -> ManagerEvent {
    ManagerEvent::ReadyToLand {
        workspace_id: "a".into(),
        workspace: "Shop".into(),
        task_id: task.into(),
        title: title.into(),
    }
}

fn done(tasks: usize, workspace_id: &str, workspace: &str) -> ManagerEvent {
    ManagerEvent::BoardDone {
        workspace_id: workspace_id.into(),
        workspace: workspace.into(),
        tasks,
    }
}

fn share(id: &str) -> Share {
    Share {
        id: id.into(),
        at: 1,
        workspace: "C:\\dev\\shop".into(),
        pane: "Ada".into(),
        name: format!("{id}.png"),
        mime: "image/png".into(),
        kind: ShareKind::default(),
        size: 10,
        caption: None,
    }
}

/// The French template's high punctuation, as the dictionary sets it.
fn fr(text: &str) -> String {
    text.replace(" :", &format!("{NNBSP}:"))
}

fn words(en: &str, fr: &str) -> Localized {
    Localized {
        en: en.into(),
        fr: fr.into(),
    }
}

fn message(title: Localized, body: Localized, url: &str) -> PushMessage {
    PushMessage {
        title,
        body,
        url: url.into(),
    }
}

/* ── What triggers it ─────────────────────────────────────────── */

#[test]
fn a_pane_waiting_opens_that_pane() {
    let mut r = Rig::new(1);
    r.refresh();
    r.waiting.insert("w1".into());
    r.post(waits("w1", "Ada", "a", "Shop"));
    r.advance(GATHER_MS);
    assert_eq!(r.sent.len(), 1);
    assert_eq!(
        r.sent[0].1,
        message(
            words("Ada waits for you", "Ada vous attend"),
            words(
                "Shop: a permission or a question",
                &fr("Shop : une permission ou une question")
            ),
            "./#/pane/w1",
        )
    );
}

#[test]
fn needs_you_and_ready_to_land_open_for_you() {
    let mut r = Rig::new(1);
    r.refresh();
    r.post(question("t1", "Which database?", "a", "Shop"));
    r.post(lands("t2", "Mobile settings"));
    r.advance(GATHER_MS);
    let sent: Vec<PushMessage> = r.sent.iter().map(|(_, message)| message.clone()).collect();
    assert_eq!(
        sent,
        [
            message(
                words("A question for you", "Une question pour vous"),
                // The task's own words are left as written.
                words(
                    "Shop: Which database?",
                    &format!("Shop{NNBSP}: Which database?")
                ),
                "./#/",
            ),
            message(
                words("Ready to merge", "Prête à fusionner"),
                words(
                    "Shop: Mobile settings",
                    &format!("Shop{NNBSP}: Mobile settings")
                ),
                "./#/",
            ),
        ]
    );
}

#[test]
fn a_board_all_done_opens_its_project() {
    let mut r = Rig::new(1);
    r.refresh();
    r.post(done(12, "b", "Blog"));
    r.advance(GATHER_MS);
    assert_eq!(
        r.sent[0].1,
        message(
            words("Every task is done", "Toutes les tâches sont faites"),
            words(
                "Blog: 12 tasks on the board",
                &fr("Blog : 12 tâches au tableau")
            ),
            "./#/projects/b",
        )
    );
}

#[test]
fn a_share_opens_the_shares_with_its_caption_or_its_name() {
    let mut r = Rig::new(1);
    r.refresh();
    r.shared(Share {
        caption: Some("The For you screen".into()),
        ..share("s1")
    });
    r.shared(Share {
        workspace: "/home/me/blog".into(),
        pane: "Cora".into(),
        name: "notes.pdf".into(),
        mime: "application/pdf".into(),
        ..share("s2")
    });
    r.advance(GATHER_MS);
    let sent: Vec<PushMessage> = r.sent.iter().map(|(_, message)| message.clone()).collect();
    assert_eq!(
        sent,
        [
            message(
                words("Ada sent you an image", "Ada vous a envoyé une image"),
                words(
                    "Shop: The For you screen",
                    &format!("Shop{NNBSP}: The For you screen")
                ),
                "./#/shares",
            ),
            message(
                words("Cora sent you a file", "Cora vous a envoyé un fichier"),
                words("Blog: notes.pdf", &format!("Blog{NNBSP}: notes.pdf")),
                "./#/shares",
            ),
        ]
    );
}

/// What the manager feed reads: no manager at all, a board the test edits.
struct FeedIo {
    state: Arc<AppState>,
    boards: Mutex<HashMap<String, Vec<Task>>>,
    activities: HashMap<String, AgentActivity>,
    enqueued: Mutex<Vec<String>>,
}

impl EngineIo for FeedIo {
    fn state(&self) -> Arc<AppState> {
        self.state.clone()
    }

    fn activity(&self, pane: &str) -> Option<PaneActivity> {
        self.activities.get(pane).map(|activity| PaneActivity {
            workspace_id: "x".into(),
            activity: *activity,
            bridged: true,
        })
    }

    fn load_board(&self, cwd: &str) -> Result<Vec<Task>, String> {
        Ok(locked(&self.boards).get(cwd).cloned().unwrap_or_default())
    }

    fn enqueue(&self, _pane: &str, text: &str, _from: &str) -> Result<(), String> {
        locked(&self.enqueued).push(text.into());
        Ok(())
    }
}

/// The feed and the push on one clock.
struct Both<'a> {
    feed: &'a mut ManagerFeed,
    io: &'a FeedIo,
    push: Arc<Mutex<Push>>,
    sent: &'a mut Vec<PushMessage>,
    clock: Arc<AtomicU64>,
}

impl Timed for Both<'_> {
    fn next_wake(&self) -> Option<u64> {
        [self.feed.next_wake(), locked(&self.push).next_wake()]
            .into_iter()
            .flatten()
            .min()
    }

    fn fire_due(&mut self, now: u64) {
        self.clock.store(now, Ordering::SeqCst);
        if locked(&self.push).due(now) {
            if let Some(message) = locked(&self.push).fire_due(now, &|_| false) {
                self.sent.push(message);
            }
        } else {
            self.feed.fire_due(self.io, now);
        }
    }
}

#[test]
fn feeds_on_the_manager_feed_with_no_manager_at_all() {
    let push = Arc::new(Mutex::new(Push::default()));
    locked(&push).refreshed(Ok(1));
    let mut world = state();
    for ws in &mut world.workspaces {
        if ws.hq == Some(true) {
            ws.panes.clear();
        }
    }
    let world = Arc::new(world);
    let clock = Arc::new(AtomicU64::new(START));
    let sink = {
        let (world, clock) = (world.clone(), clock.clone());
        PushSink {
            push: push.clone(),
            state: Box::new(move || world.clone()),
            now: Box::new(move || clock.load(Ordering::SeqCst)),
            wake: Box::new(|| {}),
        }
    };
    let board = |labels: &[&str]| {
        vec![Task {
            title: "Which database?".into(),
            labels: strings(labels),
            ..task("t1")
        }]
    };
    let io = FeedIo {
        state: world,
        boards: Mutex::new(HashMap::from([("C:\\dev\\shop".to_string(), board(&[]))])),
        activities: HashMap::new(),
        enqueued: Mutex::new(Vec::new()),
    };
    let mut feed = ManagerFeed::new(Some(Box::new(sink)));
    feed.tick(&io, START);
    locked(&io.boards).insert("C:\\dev\\shop".into(), board(&["needs-you"]));
    feed.tick(&io, START);
    let mut sent = Vec::new();
    let mut t = START;
    advance(
        &mut t,
        MAX_WAIT_MS,
        &mut Both {
            feed: &mut feed,
            io: &io,
            push,
            sent: &mut sent,
            clock,
        },
    );
    assert!(locked(&io.enqueued).is_empty());
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].title.en, "A question for you");
    feed.stop();
}

/* ── Its pace ─────────────────────────────────────────────────── */

#[test]
fn gathers_what_comes_together_then_waits_out_30_s_per_kind_and_workspace() {
    let mut r = Rig::new(1);
    r.refresh();
    r.post(question("t1", "One", "a", "Shop"));
    r.post(question("t2", "Two", "a", "Shop"));
    r.advance(GATHER_MS);
    assert_eq!(r.sent.len(), 1);
    assert_eq!(r.sent[0].1.title.en, "2 questions for you");
    assert_eq!(r.sent[0].1.body.en, "Shop: One, Two");
    let first = r.sent[0].0;

    // Within the 30 s: held, then sent as one.
    r.advance(5_000);
    r.post(question("t3", "Three", "a", "Shop"));
    r.advance(10_000);
    r.post(question("t4", "Four", "a", "Shop"));
    r.post(question("t5", "Five", "a", "Shop"));
    r.post(question("t6", "Six", "a", "Shop"));
    r.advance(GAP_MS);
    assert_eq!(r.sent.len(), 2);
    assert_eq!(r.sent[1].0 - first, GAP_MS);
    assert_eq!(r.sent[1].1.title.en, "4 questions for you");
    assert_eq!(r.sent[1].1.title.fr, "4 questions pour vous");
    assert_eq!(r.sent[1].1.body.en, "Shop: Three, Four, Five…");

    // Long after: the next goes after the gathering only.
    r.advance(60_000);
    r.post(question("t7", "Seven", "a", "Shop"));
    r.advance(GATHER_MS);
    assert_eq!(r.sent.len(), 3);
}

#[test]
fn never_sends_one_kind_in_one_workspace_twice_within_30_s() {
    let mut r = Rig::new(1);
    r.refresh();
    for i in 0..40 {
        r.shared(share(&format!("s{i}")));
        r.advance(2_000);
    }
    r.advance(GAP_MS);
    assert!(r.sent.len() > 1);
    for pair in r.sent.windows(2) {
        assert!(pair[1].0 - pair[0].0 >= GAP_MS);
    }
    let title = &r.sent[1].1.title.en;
    let count = title
        .strip_suffix(" files for you")
        .expect("a count of files");
    assert!(count.parse::<u32>().is_ok(), "{title}");
}

#[test]
fn does_not_hold_one_kind_or_workspace_for_another() {
    let mut r = Rig::new(1);
    r.refresh();
    r.waiting.extend(["w1".to_string(), "p".to_string()]);
    r.post(waits("w1", "Ada", "a", "Shop"));
    r.advance(GATHER_MS);
    r.post(question("t1", "One", "a", "Shop"));
    r.post(waits("p", "Cora", "b", "Blog"));
    r.shared(share("s1"));
    r.advance(GATHER_MS);
    assert_eq!(
        r.titles(),
        [
            "Ada waits for you",
            "A question for you",
            "Cora waits for you",
            "Ada sent you an image"
        ]
    );
}

#[test]
fn several_panes_waiting_open_for_you_and_a_wait_that_ended_is_dropped() {
    let mut r = Rig::new(1);
    r.refresh();
    r.waiting
        .extend(["w1".to_string(), "w2".to_string(), "o".to_string()]);
    r.post(waits("w1", "Ada", "a", "Shop"));
    r.post(waits("w2", "Ben", "a", "Shop"));
    r.post(waits("o", "Jack", "a", "Shop"));
    r.waiting.remove("o");
    r.advance(GATHER_MS);
    assert_eq!(r.sent.len(), 1);
    let (_, sent) = &r.sent[0];
    assert_eq!(
        sent.title,
        words("2 panes wait for you", "2 panneaux vous attendent")
    );
    assert_eq!(sent.body.en, "Shop: Ada, Ben");
    assert_eq!(sent.url, "./#/");

    // Every wait of the next batch ended: nothing goes, and the 30 s are not spent.
    r.post(waits("w1", "Ada", "a", "Shop"));
    r.waiting.clear();
    r.advance(GAP_MS);
    assert_eq!(r.sent.len(), 1);
}

/* ── Who it is for ────────────────────────────────────────────── */

#[test]
fn pushes_nothing_without_a_subscribed_phone() {
    let mut r = Rig::new(0);
    r.refresh();
    assert!(!r.push.active());
    r.waiting.insert("w1".into());
    r.post(waits("w1", "Ada", "a", "Shop"));
    r.shared(share("s1"));
    r.advance(GAP_MS);
    assert!(r.sent.is_empty());

    r.subscribers = Ok(1);
    r.refresh();
    assert!(r.push.active());
    r.shared(share("s2"));
    r.advance(GATHER_MS);
    assert_eq!(r.sent.len(), 1);
}

#[test]
fn drops_what_waited_to_go_when_the_last_phone_unsubscribes() {
    let mut r = Rig::new(1);
    r.refresh();
    r.shared(share("s1"));
    r.subscribers = Ok(0);
    r.refresh();
    r.advance(GAP_MS);
    assert!(r.sent.is_empty());
    assert_eq!(r.push.pending(), 0);
}

#[test]
fn is_off_when_the_pc_cannot_say_how_many_phones_listen() {
    let mut r = Rig::new(1);
    r.subscribers = Err("Hangar Mobile is unavailable".into());
    r.refresh();
    assert!(!r.push.active());
}

#[test]
fn pushes_nothing_from_the_hq() {
    let mut r = Rig::new(1);
    r.refresh();
    r.waiting.insert("m".into());
    r.post(waits("m", "Max", "hq", "HQ"));
    r.post(question("h1", "Anything?", "hq", "HQ"));
    r.post(done(3, "hq", "HQ"));
    r.shared(Share {
        workspace: "C:\\Users\\me\\AppData\\Roaming\\hangar\\hq".into(),
        pane: "Max".into(),
        ..share("s1")
    });
    r.advance(GAP_MS);
    assert!(r.sent.is_empty());
}

#[test]
fn stops_for_good() {
    let mut r = Rig::new(1);
    r.refresh();
    r.shared(share("s1"));
    r.push.stop();
    r.refresh();
    r.shared(share("s2"));
    r.advance(GAP_MS);
    assert!(r.sent.is_empty());
    assert!(!r.push.active());
}

/* ── workspaceOfShare ─────────────────────────────────────────── */

fn found(share: Share) -> Option<String> {
    workspace_of_share(&state(), &share).map(|ws| ws.id.clone())
}

#[test]
fn finds_a_workspace_by_its_folder_however_it_is_spelled() {
    assert_eq!(
        found(Share {
            workspace: "c:/dev/Shop/".into(),
            ..share("s")
        })
        .as_deref(),
        Some("a")
    );
    assert_eq!(
        found(Share {
            workspace: "\\\\?\\C:\\dev\\shop".into(),
            ..share("s")
        })
        .as_deref(),
        Some("a")
    );
    assert_eq!(
        found(Share {
            workspace: "/home/me/blog".into(),
            pane: "?".into(),
            ..share("s")
        })
        .as_deref(),
        Some("b")
    );
}

#[test]
fn by_a_pane_s_folder_then_by_a_pane_s_name() {
    assert_eq!(
        found(Share {
            workspace: "C:\\dev\\shop-worktrees\\ada".into(),
            pane: "?".into(),
            ..share("s")
        })
        .as_deref(),
        Some("a")
    );
    assert_eq!(
        found(Share {
            workspace: "D:\\elsewhere".into(),
            pane: "Cora".into(),
            ..share("s")
        })
        .as_deref(),
        Some("b")
    );
    assert_eq!(
        found(Share {
            workspace: "D:\\elsewhere".into(),
            pane: "Nobody".into(),
            ..share("s")
        }),
        None
    );
}

#[test]
fn names_a_folder_no_workspace_has_after_its_last_part() {
    let mut r = Rig::new(1);
    r.refresh();
    r.shared(Share {
        workspace: "D:\\elsewhere\\tools\\".into(),
        pane: "Nobody".into(),
        ..share("s")
    });
    r.advance(GATHER_MS);
    assert_eq!(r.sent[0].1.body.en, "tools: s.png");
}

/* ── settlePush and pushMessage ───────────────────────────────── */

#[test]
fn folds_repeats_the_latest_board_count_winning() {
    let batch = [
        done(3, "b", "Blog"),
        question("t1", "One", "a", "Shop"),
        question("t1", "One", "a", "Shop"),
        done(4, "b", "Blog"),
    ];
    let out = settle_push(batch.into_iter().map(PushItem::Manager).collect(), &|_| {
        true
    });
    assert_eq!(out.len(), 2);
    assert!(matches!(
        out[0],
        PushItem::Manager(ManagerEvent::TaskNeedsYou { .. })
    ));
    assert!(matches!(
        out[1],
        PushItem::Manager(ManagerEvent::BoardDone { tasks: 4, .. })
    ));
}

#[test]
fn says_one_task_in_the_singular() {
    assert_eq!(
        push_message(&[PushItem::Manager(done(1, "b", "Blog"))])
            .unwrap()
            .body,
        words(
            "Blog: 1 task on the board",
            &fr("Blog : 1 tâche au tableau")
        )
    );
}

#[test]
fn lists_several_shares_by_name() {
    let items: Vec<PushItem> = ["a", "b", "c", "d"]
        .into_iter()
        .map(|id| PushItem::Shared {
            workspace_id: "a".into(),
            workspace: "Shop".into(),
            share: share(id),
        })
        .collect();
    assert_eq!(
        push_message(&items),
        Some(message(
            words("4 files for you", "4 fichiers pour vous"),
            words(
                "Shop: a.png, b.png, c.png…",
                &format!("Shop{NNBSP}: a.png, b.png, c.png…")
            ),
            "./#/shares",
        ))
    );
    assert_eq!(push_message(&[]), None);
}
