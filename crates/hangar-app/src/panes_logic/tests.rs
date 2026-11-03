//! The package against a `Model`: the reducer of core's events, who is
//! looking, the workspace on screen, a sleep on the live environment. The
//! state machines have their tests beside them; these check that the model is
//! read the way they are fed.

use hangar_core::bridge::Queued;
use serde_json::json;

use super::hibernate::PaneRest;
use super::*;
use crate::state::live::PaneEventDetail;

const NOW: EpochMs = 5_000_000;

fn pane(id: &str, name: &str, agent: AgentId) -> Pane {
    Pane {
        id: id.into(),
        name: name.into(),
        agent,
        session_id: (agent != AgentId::Shell).then(|| format!("s-{id}")),
        ..Pane::default()
    }
}

fn workspace(id: &str, panes: Vec<Pane>) -> Workspace {
    Workspace {
        id: id.into(),
        name: format!("ws {id}"),
        cwd: format!("/work/{id}"),
        panes,
        ..Workspace::default()
    }
}

/// A model with these workspaces, the first one active, and a main window;
/// every pane of every workspace mounted.
fn model(workspaces: Vec<Workspace>) -> (Model, window::Id) {
    let mut m = Model::default();
    let active = workspaces.first().map(|ws| ws.id.clone());
    let panes: Vec<(String, String, String)> = workspaces
        .iter()
        .flat_map(|ws| {
            ws.panes
                .iter()
                .map(move |p| (p.id.clone(), ws.id.clone(), p.name.clone()))
        })
        .collect();
    m.hydrate(AppState {
        workspaces,
        active_workspace_id: active,
        ..AppState::default()
    });
    let main = window::Id::unique();
    m.live.windows.insert(main, WindowKind::Main);
    for (pane, ws, name) in panes {
        m.live.panes_logic.mount_pane(&pane, &ws, &name, NOW);
    }
    (m, main)
}

fn bridge(m: &mut Model, pane: &str, event: serde_json::Value, now: EpochMs) {
    let event = CoreEvent::Bridge {
        pane: pane.into(),
        event,
    };
    assert!(reduce_core_event(m, &event, now).is_some());
}

fn one_claude() -> (Model, window::Id) {
    model(vec![workspace(
        "w1",
        vec![pane("p1", "Ava", AgentId::Claude)],
    )])
}

#[test]
fn the_glyph_is_the_shells_life_until_it_runs_then_the_agents_activity() {
    let (mut m, _) = one_claude();
    let p1 = "p1".to_string();
    // Mounted, the shell is still starting.
    assert_eq!(state_glyph_state(&m, &p1), GlyphState::Starting);
    m.live
        .panes_logic
        .status
        .insert(p1.clone(), PaneStatus::Running);
    assert_eq!(state_glyph_state(&m, &p1), GlyphState::Idle);

    bridge(&mut m, "p1", json!({"kind": "turn", "phase": "start"}), NOW);
    assert_eq!(state_glyph_state(&m, &p1), GlyphState::Working);
    bridge(
        &mut m,
        "p1",
        json!({"kind": "permission", "waiting": true, "tool": "Bash"}),
        NOW + 1,
    );
    assert_eq!(state_glyph_state(&m, &p1), GlyphState::Waiting);

    // The shell's own life wins over what the agent last said.
    m.live
        .panes_logic
        .status
        .insert(p1.clone(), PaneStatus::Exited);
    assert_eq!(state_glyph_state(&m, &p1), GlyphState::Exited);
    m.live
        .panes_logic
        .status
        .insert(p1.clone(), PaneStatus::Starting);
    assert_eq!(state_glyph_state(&m, &p1), GlyphState::Starting);

    // A pane nobody mounted rests as idle.
    assert_eq!(
        state_glyph_state(&m, &"never".to_string()),
        GlyphState::Idle
    );
}

#[test]
fn a_plain_shell_rests_as_idle_whatever_it_does() {
    let (mut m, _) = model(vec![workspace(
        "w1",
        vec![pane("sh", "Leo", AgentId::Shell)],
    )]);
    m.live
        .panes_logic
        .status
        .insert("sh".into(), PaneStatus::Running);
    let (info, amb) = inputs(&m, "sh", NOW).unwrap();
    m.live.panes_logic.bell(&info, &amb);
    assert_eq!(state_glyph_state(&m, &"sh".to_string()), GlyphState::Idle);
    // It badges, for the bell on a shell's header, but is not an agent.
    assert!(m.live.panes_logic.needs_attention("sh"));
    assert!(m.live.panes_logic.pane_activity("sh").is_none());
}

#[test]
fn the_reducer_publishes_a_reports_activity_insight_and_version() {
    let (mut m, _) = one_claude();
    let version = m.live.version;
    bridge(
        &mut m,
        "p1",
        json!({"kind": "hello", "sessionId": "s-p1", "model": "claude-opus-5", "version": "2.1"}),
        NOW,
    );
    assert!(m.live.version > version);
    let live = &m.live.panes_logic;
    let published = live.pane_activity("p1").unwrap();
    assert_eq!(
        (published.workspace_id.as_str(), published.bridged),
        ("w1", true)
    );
    assert_eq!(live.insight("p1").model.as_deref(), Some("claude-opus-5"));
    assert_eq!(live.bridged_count(&["p1".to_string()]), 1);
    assert_eq!(live.workspace_activity("w1"), WorkspaceActivity::default());
    // The session was the stored one: nothing to capture, and it reads as
    // resumed.
    assert_eq!(
        live.insight("p1").events[0].detail,
        PaneEventDetail::Session {
            id: "s-p1".into(),
            resumed: true
        }
    );

    bridge(
        &mut m,
        "p1",
        json!({"kind": "turn", "phase": "start"}),
        NOW + 1,
    );
    let live = &m.live.panes_logic;
    assert_eq!(live.workspace_activity("w1").working, 1);
    assert_eq!(live.activity_map().len(), 1);
    assert_eq!(live.agents_in(&["w1".to_string()]).working, ["p1"]);
    assert!(live.hibernation.pane_rest("p1").unwrap().busy);
}

#[test]
fn a_report_of_a_kind_this_version_does_not_know_is_ignored() {
    let (mut m, _) = one_claude();
    let event = CoreEvent::Bridge {
        pane: "p1".into(),
        event: json!({"kind": "from-the-future", "x": 1}),
    };
    assert!(reduce_core_event(&mut m, &event, NOW).is_none());
    assert!(m.live.panes_logic.pane_activity("p1").is_none());
}

#[test]
fn the_events_of_a_pane_that_is_not_in_the_state_are_nobodys() {
    let (mut m, _) = one_claude();
    let event = CoreEvent::Bridge {
        pane: "ghost".into(),
        event: json!({"kind": "turn", "phase": "start"}),
    };
    // Handled (it is a bridge event), and changed nothing.
    reduce_core_event(&mut m, &event, NOW);
    assert!(m.live.panes_logic.pane_activity("ghost").is_none());
    assert!(!m.live.panes_logic.insights.contains("ghost"));
}

#[test]
fn the_queue_and_the_shells_end_come_through_their_own_events() {
    let (mut m, _) = one_claude();
    bridge(&mut m, "p1", json!({"kind": "turn", "phase": "start"}), NOW);
    let queued = CoreEvent::BridgeQueue {
        pane: "p1".into(),
        pending: vec![Queued {
            id: "q1".into(),
            text: "run the tests".into(),
            from: None,
        }],
    };
    assert!(reduce_core_event(&mut m, &queued, NOW + 1).is_some());
    assert_eq!(m.live.panes_logic.insight("p1").queue.len(), 1);

    let exit = CoreEvent::PtyExit { pane: "p1".into() };
    assert!(reduce_core_event(&mut m, &exit, NOW + 2).is_some());
    assert_eq!(
        m.live.panes_logic.pane_status("p1"),
        Some(PaneStatus::Exited)
    );
    assert_eq!(
        m.live.panes_logic.pane_activity("p1").map(|a| a.activity),
        Some(AgentActivity::Idle)
    );
    assert_eq!(state_glyph_state(&m, &"p1".to_string()), GlyphState::Exited);
    // Other events are not this package's.
    assert!(reduce_core_event(&mut m, &CoreEvent::MemoryChanged, NOW).is_none());
}

// ----- who is looking

fn end_turn(m: &mut Model, now: EpochMs) {
    bridge(m, "p1", json!({"kind": "turn", "phase": "start"}), now);
    bridge(
        m,
        "p1",
        json!({"kind": "turn", "phase": "end", "reason": "answer", "durationMs": 9, "answer": "Done"}),
        now + 1,
    );
}

fn activity_of(m: &Model) -> AgentActivity {
    m.live.panes_logic.pane_activity("p1").unwrap().activity
}

#[test]
fn a_turn_ends_idle_when_the_window_is_in_front_on_the_focused_pane() {
    let (mut m, main) = one_claude();
    m.live.panes_logic.focused_window = Some(main);
    m.ui.focus_by_workspace.insert("w1".into(), "p1".into());
    end_turn(&mut m, NOW);
    assert_eq!(activity_of(&m), AgentActivity::Idle);
    assert!(!m.live.panes_logic.needs_attention("p1"));
}

#[test]
fn a_turn_ends_yours_when_the_window_is_behind() {
    let (mut m, _) = one_claude();
    m.ui.focus_by_workspace.insert("w1".into(), "p1".into());
    end_turn(&mut m, NOW);
    assert_eq!(activity_of(&m), AgentActivity::Yours);
    assert!(m.live.panes_logic.needs_attention("p1"));
}

#[test]
fn a_turn_ends_yours_on_a_pane_the_person_is_not_on_or_a_workspace_off_screen() {
    // Another pane of the workspace has the focus.
    let (mut m, main) = model(vec![workspace(
        "w1",
        vec![
            pane("p1", "Ava", AgentId::Claude),
            pane("p2", "Bob", AgentId::Claude),
        ],
    )]);
    m.live.panes_logic.focused_window = Some(main);
    m.ui.focus_by_workspace.insert("w1".into(), "p2".into());
    end_turn(&mut m, NOW);
    assert_eq!(activity_of(&m), AgentActivity::Yours);

    // The focused pane of a workspace that is not the one shown.
    let (mut m, main) = model(vec![
        workspace("w0", vec![pane("p0", "Zed", AgentId::Claude)]),
        workspace("w1", vec![pane("p1", "Ava", AgentId::Claude)]),
    ]);
    m.live.panes_logic.focused_window = Some(main);
    m.ui.focus_by_workspace.insert("w1".into(), "p1".into());
    end_turn(&mut m, NOW);
    assert_eq!(activity_of(&m), AgentActivity::Yours);

    // The board is on screen, not the terminals.
    let (mut m, main) = one_claude();
    m.live.panes_logic.focused_window = Some(main);
    m.ui.focus_by_workspace.insert("w1".into(), "p1".into());
    m.ui.view = View::Board;
    end_turn(&mut m, NOW);
    assert_eq!(activity_of(&m), AgentActivity::Yours);
}

#[test]
fn a_workspace_in_a_window_of_its_own_is_looked_at_there() {
    let mut ws = workspace("w1", vec![pane("p1", "Ava", AgentId::Claude)]);
    ws.detached = Some(true);
    let (mut m, main) = model(vec![ws]);
    let own = window::Id::unique();
    m.live
        .windows
        .insert(own, WindowKind::Workspace("w1".into()));
    m.ui.focus_by_workspace.insert("w1".into(), "p1".into());
    // The main window has the focus, and does not draw this workspace.
    m.live.panes_logic.focused_window = Some(main);
    end_turn(&mut m, NOW);
    assert_eq!(activity_of(&m), AgentActivity::Yours);
    // Its own window in front: seen at once.
    m.live.panes_logic.focused_window = Some(own);
    end_turn(&mut m, NOW + 10);
    assert_eq!(activity_of(&m), AgentActivity::Idle);
}

#[test]
fn the_window_coming_back_to_the_front_takes_the_hand_back_of_the_pane_it_shows() {
    let (mut m, main) = one_claude();
    m.ui.focus_by_workspace.insert("w1".into(), "p1".into());
    refresh_attention(&mut m, None, NOW);
    end_turn(&mut m, NOW + 1);
    assert_eq!(activity_of(&m), AgentActivity::Yours);
    // Still behind: the look changes nothing.
    refresh_attention(&mut m, None, NOW + 2);
    assert_eq!(activity_of(&m), AgentActivity::Yours);
    // The window comes back with the pane already the one looked at.
    m.live.panes_logic.focused_window = Some(main);
    refresh_attention(&mut m, Some(main), NOW + 3);
    assert_eq!(activity_of(&m), AgentActivity::Idle);
    assert!(!m.live.panes_logic.needs_attention("p1"));
}

#[test]
fn the_pane_that_takes_the_focus_takes_its_hand_back() {
    let (mut m, _) = model(vec![workspace(
        "w1",
        vec![
            pane("p1", "Ava", AgentId::Claude),
            pane("p2", "Bob", AgentId::Claude),
        ],
    )]);
    m.ui.focus_by_workspace.insert("w1".into(), "p2".into());
    refresh_attention(&mut m, None, NOW);
    end_turn(&mut m, NOW + 1);
    assert_eq!(activity_of(&m), AgentActivity::Yours);
    m.ui.focus_by_workspace.insert("w1".into(), "p1".into());
    refresh_attention(&mut m, None, NOW + 5);
    assert_eq!(activity_of(&m), AgentActivity::Idle);
}

// ----- the workspace on screen

fn activate(m: &mut Model, id: &str) {
    // The store of this branch does not write yet; set the state as it will.
    let mut state = m.state().clone();
    state.active_workspace_id = Some(id.into());
    m.hydrate(state);
}

#[test]
fn the_workspace_on_screen_is_touched_when_it_arrives_and_when_it_leaves() {
    let (mut m, _) = model(vec![
        workspace("a", vec![pane("pa", "Ava", AgentId::Claude)]),
        workspace("b", vec![pane("pb", "Bob", AgentId::Claude)]),
    ]);
    sync_active(&mut m, NOW);
    assert_eq!(m.live.panes_logic.hibernation.last_touch("a"), Some(NOW));
    assert_eq!(m.live.panes_logic.recent, ["a"]);

    activate(&mut m, "b");
    sync_active(&mut m, NOW + 100);
    assert_eq!(
        m.live.panes_logic.hibernation.last_touch("a"),
        Some(NOW + 100)
    );
    assert_eq!(
        m.live.panes_logic.hibernation.last_touch("b"),
        Some(NOW + 100)
    );
    assert_eq!(m.live.panes_logic.recent, ["b", "a"]);

    // Nothing moved: nothing is touched again.
    sync_active(&mut m, NOW + 900);
    assert_eq!(
        m.live.panes_logic.hibernation.last_touch("b"),
        Some(NOW + 100)
    );
}

#[test]
fn a_workspace_mounted_again_wakes_and_counts_as_used() {
    let (mut m, _) = model(vec![
        workspace("a", vec![pane("pa", "Ava", AgentId::Claude)]),
        workspace("b", vec![pane("pb", "Bob", AgentId::Claude)]),
    ]);
    m.ui.opened_ids = vec!["a".into()];
    m.live
        .panes_logic
        .hibernation
        .mark_asleep("b", Asleep { at: 1, agents: 1 });
    sync_active(&mut m, NOW);
    // Not mounted: still asleep.
    assert!(m.live.panes_logic.is_asleep("b"));
    m.ui.opened_ids.push("b".into());
    sync_active(&mut m, NOW + 7);
    assert!(!m.live.panes_logic.is_asleep("b"));
    assert_eq!(
        m.live.panes_logic.hibernation.last_touch("b"),
        Some(NOW + 7)
    );
}

#[test]
fn the_delay_is_at_least_a_minute_whatever_the_file_says() {
    let (mut m, _) = one_claude();
    let mut state = m.state().clone();
    for (minutes, expected) in [
        (10.0, 600_000),
        (0.0, 60_000),
        (-5.0, 60_000),
        (0.5, 60_000),
        (f64::NAN, 60_000),
    ] {
        state.settings.hibernate_after_min = minutes;
        m.hydrate(state.clone());
        assert_eq!(delay_ms(&m), expected, "{minutes}");
    }
}

// ----- a sleep, against the live model

fn env_model() -> Model {
    let (mut m, _) = model(vec![
        workspace("screen", vec![pane("gus", "Gus", AgentId::Claude)]),
        workspace(
            "other",
            vec![
                pane("ava", "Ava", AgentId::Claude),
                pane("leo", "Leo", AgentId::Shell),
            ],
        ),
    ]);
    m.ui.opened_ids = vec!["screen".into(), "other".into()];
    m
}

fn nothing_runs(_: &mut dyn SleepEnv, _: &[PaneId]) -> Result<Vec<PaneId>, String> {
    Ok(Vec::new())
}

#[test]
fn a_sleep_unmounts_the_workspace_marks_it_and_owes_its_agents_their_sessions() {
    let mut m = env_model();
    let (outcome, kills, shown) = env_run(&mut m, Arc::new(Snapshot::default()), |env| {
        hibernation::force_sleep(env, "other", NOW, &mut nothing_runs)
    });
    assert!(matches!(
        outcome,
        Some(ForcedOutcome::Slept {
            agents: 1,
            shown: None,
            ..
        })
    ));
    assert_eq!(kills, ["ava", "leo"]);
    assert!(shown.is_empty());
    assert_eq!(m.ui.opened_ids, ["screen"]);
    let live = &mut m.live.panes_logic;
    assert!(live.is_asleep("other"));
    assert_eq!(live.asleep_ids().into_iter().collect::<Vec<_>>(), ["other"]);
    assert!(live.must_resume("ava", false));
    assert!(!live.must_resume("leo", false));
}

#[test]
fn a_delivery_the_snapshot_knows_of_keeps_the_workspace_awake() {
    let mut m = env_model();
    let mut snapshot = Snapshot::default();
    snapshot
        .delivering
        .insert("other".into(), ["task-1".to_string()].into());
    let (outcome, kills, _) = env_run(&mut m, Arc::new(snapshot), |env| {
        hibernation::force_sleep(env, "other", NOW, &mut nothing_runs)
    });
    let Some(ForcedOutcome::Refused { verdict, .. }) = outcome else {
        panic!("expected a refusal, got {outcome:?}");
    };
    assert_eq!(verdict.reason, Blocker::Delivering);
    assert!(kills.is_empty());
    assert_eq!(m.ui.opened_ids.len(), 2);
}

#[test]
fn prompts_queued_in_the_insight_keep_the_workspace_awake() {
    let mut m = env_model();
    m.live.panes_logic.queue_changed(
        "ava",
        vec![Queued {
            id: "q".into(),
            text: "later".into(),
            from: None,
        }],
        NOW,
    );
    let (outcome, ..) = env_run(&mut m, Arc::new(Snapshot::default()), |env| {
        hibernation::force_sleep(env, "other", NOW, &mut nothing_runs)
    });
    let Some(ForcedOutcome::Refused { verdict, .. }) = outcome else {
        panic!("expected a refusal, got {outcome:?}");
    };
    assert_eq!(verdict.reason, Blocker::Queued);
    assert_eq!(verdict.pane_id.as_deref(), Some("ava"));
}

#[test]
fn the_sweep_waits_for_the_delay_on_the_models_own_data() {
    let mut m = env_model();
    let delay = 10 * 60_000;
    // Mounted at NOW: nothing to do yet.
    let (swept, ..) = env_run(&mut m, Arc::new(Snapshot::default()), |env| {
        hibernation::sweep(env, delay, NOW, &mut nothing_runs)
    });
    assert!(swept.is_empty());
    let (swept, kills, _) = env_run(&mut m, Arc::new(Snapshot::default()), |env| {
        hibernation::sweep(env, delay, NOW + delay + 1, &mut nothing_runs)
    });
    assert_eq!(swept.len(), 1);
    assert_eq!(swept[0].workspace.id, "other");
    assert_eq!(kills, ["ava", "leo"]);
    // The workspace on screen is never touched.
    assert!(!m.live.panes_logic.is_asleep("screen"));
}

// ----- small readers

#[test]
fn a_panes_folder_is_its_own_or_its_workspaces() {
    let ws = workspace("w", vec![]);
    let mut p = pane("p", "Ava", AgentId::Claude);
    assert_eq!(folder_of(&ws, &p), "/work/w");
    p.cwd = Some(String::new());
    assert_eq!(folder_of(&ws, &p), "/work/w");
    p.cwd = Some("/elsewhere".into());
    assert_eq!(folder_of(&ws, &p), "/elsewhere");
}

#[test]
fn a_freshly_mounted_pane_has_done_nothing_yet() {
    let (m, _) = one_claude();
    let rest = m.live.panes_logic.hibernation.pane_rest("p1").unwrap();
    let as_rest = PaneRest {
        last_output_at: rest.last_output_at,
        busy: rest.busy,
        resting_since: rest.resting_since,
        ..PaneRest::default()
    };
    assert_eq!(
        (as_rest.last_output_at, as_rest.resting_since, as_rest.busy),
        (NOW, NOW, false)
    );
}
