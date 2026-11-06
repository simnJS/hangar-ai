//! What the panes' output asks of the window: a pane on screen one wake-up,
//! then one look a frame, which the redraw after it draws; a pane off screen
//! no frame at all, its output in its terminal and told to the pane logic at
//! the looks of the tick, with the moments it was printed at.

use std::time::Duration;

use hangar_core::pty::{PtyEvent, PtyOutput, PtySink};
use tokio::sync::mpsc::UnboundedReceiver;

use super::controller::mounted;
use super::pty_link::HubKind;
use super::*;
use crate::navigator::testing::{model_of, pane, workspace};
use crate::panes_logic::{mount_for_test, output_seen, wall_clock_ms};
use crate::state::types::{AgentId, AppState};

/// Workspace "on", active, with Ava ("a"); workspace "off" with Bea ("b"):
/// both mounted, in the pane logic too, and placed.
fn two_workspaces() -> (Model, State, UnboundedReceiver<HubEvent>) {
    let mut m = model_of(AppState {
        workspaces: vec![
            workspace("on", "On", None, vec![pane("a", "Ava", AgentId::Claude)]),
            workspace("off", "Off", None, vec![pane("b", "Bea", AgentId::Claude)]),
        ],
        active_workspace_id: Some("on".into()),
        ..AppState::default()
    });
    let mut st = State::default();
    let wakes = st.channel.take_receiver().expect("the receiver");
    for id in ["on", "off"] {
        let ws = m.workspace(id).expect("the workspace").clone();
        for p in &ws.panes {
            st.panes.insert(p.id.clone(), mounted(&ws, p));
            mount_for_test(&mut m, &p.id, &ws.id, &p.name, wall_clock_ms());
        }
    }
    place_panes(&st, &mut m);
    (m, st, wakes)
}

fn sink(st: &State, pane: &str) -> PtySink {
    st.panes[pane].link.sink(st.channel.waker())
}

fn print(sink: &mut PtySink, text: &str) {
    sink(PtyEvent::Output(PtyOutput {
        id: String::new(),
        data: text.into(),
        seq: 0,
    }));
}

/// The wake-ups sent since the last call, as (pane, kind).
fn wakes_sent(wakes: &mut UnboundedReceiver<HubEvent>) -> Vec<(PaneId, HubKind)> {
    let mut sent = Vec::new();
    while let Ok(event) = wakes.try_recv() {
        sent.push((event.pane, event.kind));
    }
    sent
}

fn shows(st: &State, pane: &str, text: &str) -> bool {
    st.panes[pane]
        .link
        .lock()
        .read_lines(50)
        .iter()
        .any(|line| line.contains(text))
}

/// The window's own handling of the wake-ups (`hub_events`).
fn handle_wakes(st: &mut State, m: &mut Model, sent: &[(PaneId, HubKind)]) {
    for (_, kind) in sent {
        assert!(wake(st, m, kind), "{kind:?} is a wake-up for output");
    }
    woken(st, m);
}

#[test]
fn the_layout_says_which_panes_are_on_screen() {
    let (_m, st, _wakes) = two_workspaces();
    assert!(st.panes["a"].link.is_shown());
    assert!(!st.panes["b"].link.is_shown(), "another workspace's");
}

/// A pane off screen: its output is in its terminal and reaches the activity
/// and hibernation with the moments it was printed at, and it asks for no
/// frame: one wake-up when it starts after a quiet spell, then nothing until
/// the looks of the tick find it quiet.
#[test]
fn output_off_screen_reaches_the_terminal_and_the_activity_without_a_frame() {
    let (mut m, mut st, mut wakes) = two_workspaces();
    let mut bea = sink(&st, "b");
    let before = wall_clock_ms();
    print(&mut bea, "one\r\n");
    std::thread::sleep(Duration::from_millis(30));
    print(&mut bea, "two\r\n");
    print(&mut bea, "three\r\n");
    let after = wall_clock_ms();
    assert!(shows(&st, "b", "three"), "the terminal has it all");
    let sent = wakes_sent(&mut wakes);
    assert_eq!(
        sent,
        [("b".to_string(), HubKind::OffScreen)],
        "one wake-up for three chunks, and not the frames'"
    );

    handle_wakes(&mut st, &mut m, &sent);
    assert!(!st.ticking, "no frame is asked for");
    assert!(st.looking && m.live.panes.looking_off_screen());
    let (burst, rested) = output_seen(&m, "b");
    let (first, last) = burst.expect("the activity follows the burst");
    assert!(before <= first && first < last && last <= after);
    assert!(last - first >= 30, "measured from the chunks, not the look");
    assert_eq!(rested, Some(last), "hibernation saw it print");

    // While the looks go on, more output sends nothing; a frame for the
    // panes on screen does not take it, and ends at once.
    print(&mut bea, "four\r\n");
    assert!(wakes_sent(&mut wakes).is_empty());
    st.ticking = true;
    assert!(frame(&mut st).is_empty());
    assert!(!st.ticking);
    // The tick's look takes it.
    look_off_screen(&mut st, &mut m);
    let (burst, _) = output_seen(&m, "b");
    assert!(burst.expect("still followed").1 >= after);
    assert!(st.looking, "it printed since the last look");
    // A look that finds every pane off screen quiet ends the looks, and the
    // next chunk wakes the window again.
    look_off_screen(&mut st, &mut m);
    assert!(!st.looking && !m.live.panes.looking_off_screen());
    print(&mut bea, "five\r\n");
    assert_eq!(
        wakes_sent(&mut wakes),
        [("b".to_string(), HubKind::OffScreen)]
    );
}

/// A pane on screen: a burst of chunks is one wake-up, then one look a frame,
/// each of which the redraw after it draws, however many chunks came between
/// two frames.
#[test]
fn a_burst_on_screen_is_one_look_a_frame() {
    let (mut m, mut st, mut wakes) = two_workspaces();
    let mut ava = sink(&st, "a");
    for chunk in 0..50 {
        print(&mut ava, &format!("line {chunk}\r\n"));
    }
    let sent = wakes_sent(&mut wakes);
    assert_eq!(sent, [("a".to_string(), HubKind::Output)]);
    handle_wakes(&mut st, &mut m, &sent);
    assert!(st.ticking && !st.looking);
    assert!(output_seen(&m, "a").0.is_some(), "told at once");

    for _ in 0..3 {
        for chunk in 0..20 {
            print(&mut ava, &format!("more {chunk}\r\n"));
        }
        assert!(
            wakes_sent(&mut wakes).is_empty(),
            "no message between frames"
        );
        let looked = frame(&mut st);
        assert_eq!(looked.len(), 1, "one look for twenty chunks");
        assert_eq!(looked[0].0, "a");
    }
    assert!(frame(&mut st).is_empty());
    assert!(!st.ticking, "a quiet frame ends the frames");
    print(&mut ava, "again\r\n");
    assert_eq!(wakes_sent(&mut wakes), [("a".to_string(), HubKind::Output)]);
}

/// Switching to a pane that printed off screen: the switch's redraw draws
/// its terminal, which has every chunk already; what it printed is told at
/// the switch, and its next chunk asks for frames.
#[test]
fn a_pane_coming_on_screen_shows_its_latest_output_at_once() {
    let (mut m, st, mut wakes) = two_workspaces();
    let mut bea = sink(&st, "b");
    print(&mut bea, "latest while hidden\r\n");
    let sent = wakes_sent(&mut wakes);
    assert_eq!(sent, [("b".to_string(), HubKind::OffScreen)]);

    m.apply(StoreAction::SetActiveWorkspace(Some("off".into())));
    place_panes(&st, &mut m);
    assert!(st.panes["b"].link.is_shown() && !st.panes["a"].link.is_shown());
    assert!(shows(&st, "b", "latest while hidden"));
    assert!(output_seen(&m, "b").0.is_some(), "told when it moved");
    assert!(st.panes["b"].link.take_output().is_none());

    print(&mut bea, "now on screen\r\n");
    assert_eq!(wakes_sent(&mut wakes), [("b".to_string(), HubKind::Output)]);
}

/// The chat view draws none of the workspace's terminals, and a zoom only
/// the zoomed one.
#[test]
fn the_chat_view_and_a_zoom_take_panes_off_screen() {
    let mut m = model_of(AppState {
        workspaces: vec![workspace(
            "w",
            "Shop",
            None,
            vec![
                pane("a", "Ava", AgentId::Claude),
                pane("b", "Bea", AgentId::Claude),
            ],
        )],
        active_workspace_id: Some("w".into()),
        ..AppState::default()
    });
    let mut st = State::default();
    let ws = m.workspace("w").unwrap().clone();
    for p in &ws.panes {
        st.panes.insert(p.id.clone(), mounted(&ws, p));
    }
    let shown = |st: &State| ["a", "b"].map(|id| st.panes[id].link.is_shown());
    place_panes(&st, &mut m);
    assert_eq!(shown(&st), [true, true]);

    m.ui.focus_by_workspace.insert("w".into(), "b".into());
    m.ui.zoom_by_workspace.insert("w".into(), true);
    place_panes(&st, &mut m);
    assert_eq!(shown(&st), [false, true], "the zoomed pane alone");
    m.ui.zoom_by_workspace.insert("w".into(), false);

    m.apply(StoreAction::UpdateSettings(crate::state::Patch::new(
        |s: &mut crate::state::Settings| {
            s.experiments = vec![crate::settings::experiments::CHAT_VIEW.into()]
        },
    )));
    m.apply(StoreAction::SetChatView {
        ws: "w".into(),
        on: true,
    });
    place_panes(&st, &mut m);
    assert_eq!(
        shown(&st),
        [false, false],
        "the chat view shows no terminal"
    );
}
