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
fn the_pane_that_has_the_keyboard_counts_as_the_focused_one() {
    let (mut m, main) = one_claude();
    m.live.panes_logic.focused_window = Some(main);
    // The workspace remembers no focused pane, but the terminal has the keys.
    m.ui.focus = Some(Focus::Terminal("p1".into()));
    end_turn(&mut m, NOW);
    assert_eq!(activity_of(&m), AgentActivity::Idle);
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
fn a_pane_selected_by_the_keyboard_asks_for_its_branch_as_a_landing_does() {
    let (mut m, _) = model(vec![workspace(
        "w1",
        vec![
            pane("p1", "Ava", AgentId::Claude),
            Pane {
                cwd: Some("/work/w1/site".into()),
                ..pane("p2", "Ben", AgentId::Shell)
            },
        ],
    )]);
    let asked = |m: &mut Model, folder: &str| {
        m.live
            .panes_logic
            .branches
            .lookup(folder, NOW, PANE_BRANCH_FOCUS_MS)
            == Lookup::InFlight
    };
    let select = Intent::SelectPane {
        ws: "w1".into(),
        pane: "p2".into(),
    };
    assert!(focused_branch(&mut m, &select, NOW).is_some());
    assert!(asked(&mut m, "/work/w1/site"), "the pane's own folder");
    let land = Intent::FocusPane {
        ws: "w1".into(),
        pane: "p1".into(),
        terminal: true,
    };
    assert!(focused_branch(&mut m, &land, NOW).is_some());
    assert!(asked(&mut m, "/work/w1"));
    // A pane that is gone, and any other intent, ask nothing.
    let gone = Intent::SelectPane {
        ws: "w1".into(),
        pane: "p9".into(),
    };
    assert!(focused_branch(&mut m, &gone, NOW).is_none());
    assert!(focused_branch(&mut m, &Intent::CloseOverlay, NOW).is_none());
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

// ----- the servers a launch started Claude Code with

fn custom_launch() -> PaneMcp {
    PaneMcp {
        mode: crate::state::live::PaneMcpMode::Custom {
            servers: vec!["hangar".into(), "github".into()],
        },
        hangar: Some(true),
    }
}

#[test]
fn a_launch_records_the_servers_for_whoever_reads_the_insight() {
    let (mut m, _) = one_claude();
    assert_eq!(m.live.panes_logic.insight("p1").mcp, None);
    assert!(
        m.live
            .panes_logic
            .set_launch_mcp("p1", Some(custom_launch()))
    );
    assert_eq!(m.live.panes_logic.insight("p1").mcp, Some(custom_launch()));
    // The same launch twice changes nothing, so nobody is told twice.
    assert!(
        !m.live
            .panes_logic
            .set_launch_mcp("p1", Some(custom_launch()))
    );
    // A launch that could not say forgets the last one's servers.
    assert!(m.live.panes_logic.set_launch_mcp("p1", None));
    assert_eq!(m.live.panes_logic.insight("p1").mcp, None);
}

#[test]
fn the_servers_go_with_the_pane_unless_another_window_takes_it_over() {
    let (mut m, _) = one_claude();
    m.live
        .panes_logic
        .set_launch_mcp("p1", Some(custom_launch()));
    // Moving to another window keeps what the pane reported.
    m.live.panes_logic.unmount_pane("p1", true);
    assert_eq!(m.live.panes_logic.insight("p1").mcp, Some(custom_launch()));
    // Closed, or restarted under a new id: the new one launches afresh.
    m.live.panes_logic.unmount_pane("p1", false);
    assert_eq!(m.live.panes_logic.insight("p1").mcp, None);
}

// ----- inside a turn

#[test]
fn a_pane_is_inside_a_turn_while_it_works_or_waits_for_an_answer() {
    let (mut m, _) = one_claude();
    let live = &mut m.live.panes_logic;
    assert!(!live.is_mid_turn("p1"), "a pane that reports nothing");
    assert!(!live.is_mid_turn("never"));
    for (activity, inside) in [
        (AgentActivity::Working, true),
        (AgentActivity::Waiting, true),
        (AgentActivity::Yours, false),
        (AgentActivity::Idle, false),
    ] {
        live.seed_activity("p1", "w1", activity);
        assert_eq!(live.is_mid_turn("p1"), inside, "{activity:?}");
    }
}

// ----- a launch of an agent without the mod

fn session_line(m: &Model, pane: &str) -> Option<PaneEventDetail> {
    m.live
        .panes_logic
        .insight(pane)
        .events
        .first()
        .map(|event| event.detail.clone())
}

fn without_the_mod() -> Model {
    let (m, _) = model(vec![workspace(
        "w1",
        vec![
            pane("cx", "Cy", AgentId::Claude),
            pane("cl", "Ava", AgentId::Claude),
            pane("sh", "Leo", AgentId::Shell),
        ],
    )]);
    m
}

/// The hangar-bridge mod switched off: nothing but the launch says when a
/// Claude Code session began.
fn bridge_off(m: &mut Model) {
    let mut state = m.state().clone();
    state.settings.claude_bridge = false;
    m.hydrate(state);
}

#[test]
fn an_agent_without_the_mod_dates_its_session_and_words_it_when_launched() {
    let mut m = without_the_mod();
    bridge_off(&mut m);
    let version = m.live.version;
    launched(&mut m, "cx", None, NOW + 7);
    let insight = m.live.panes_logic.insight("cx");
    assert_eq!(insight.session_since, Some(NOW + 7));
    assert_eq!(
        session_line(&m, "cx"),
        Some(PaneEventDetail::Session {
            id: String::new(),
            resumed: false
        })
    );
    assert!(m.live.version > version, "readers are told");

    // Resuming one names the session, short.
    launched(&mut m, "cx", Some("3f2a9c1e-0b7d"), NOW + 9);
    assert_eq!(
        m.live.panes_logic.insight("cx").session_since,
        Some(NOW + 9)
    );
    assert_eq!(
        session_line(&m, "cx"),
        Some(PaneEventDetail::Session {
            id: "3f2a9c".into(),
            resumed: true
        })
    );
    assert_eq!(m.live.panes_logic.insight("cx").events.len(), 2);
}

#[test]
fn a_claude_code_with_the_mod_leaves_it_to_its_hello() {
    let mut m = without_the_mod();
    assert!(m.settings().claude_bridge);
    launched(&mut m, "cl", Some("abc"), NOW + 1);
    let insight = m.live.panes_logic.insight("cl");
    assert_eq!((insight.session_since, insight.events.len()), (None, 0));

    // With the mod switched off nothing will say it: the launch does.
    bridge_off(&mut m);
    launched(&mut m, "cl", Some("abc"), NOW + 2);
    assert_eq!(
        m.live.panes_logic.insight("cl").session_since,
        Some(NOW + 2)
    );
    assert!(matches!(
        session_line(&m, "cl"),
        Some(PaneEventDetail::Session { resumed: true, .. })
    ));
}

#[test]
fn a_shell_and_a_pane_nobody_mounted_launch_nothing() {
    let mut m = without_the_mod();
    bridge_off(&mut m);
    let version = m.live.version;
    launched(&mut m, "sh", None, NOW + 1);
    launched(&mut m, "never", None, NOW + 1);
    assert!(session_line(&m, "sh").is_none());
    assert!(session_line(&m, "never").is_none());
    assert_eq!(m.live.version, version);

    // In the state but without a terminal: nothing to date.
    m.live.panes_logic.unmount_pane("cx", false);
    launched(&mut m, "cx", None, NOW + 1);
    assert!(session_line(&m, "cx").is_none());
}

// ----- the prompts a restart carried over

fn queue_host() -> hangar_core::Host {
    let dir = std::env::temp_dir().join(format!("hangar-panes-logic-{}", std::process::id()));
    hangar_core::Host::with_paths(hangar_core::paths::Paths::rooted(&dir))
}

fn prompt(text: &str, from: Option<&str>) -> QueuedPrompt {
    QueuedPrompt {
        id: String::new(),
        text: text.into(),
        from: from.map(str::to_string),
    }
}

#[test]
fn mounting_hands_the_carried_prompts_back_off_the_ui_thread() {
    // The queue is core's, global to the process: a pane id of this test's own.
    let pane = format!("requeue-{}", std::process::id());
    let carried = vec![prompt("first", None), prompt("second", Some("hangar"))];

    // Building the task does nothing yet; it only runs on the blocking pool.
    let _task = requeue(queue_host(), pane.clone(), carried.clone());
    assert!(hangar_core::bridge::bridge_queue(pane.clone()).is_empty());

    // What it runs: the prompts, in the order they were held, from whom.
    enqueue_all(&queue_host(), &pane, carried);
    let queue = hangar_core::bridge::bridge_queue(pane.clone());
    let held: Vec<(&str, Option<&str>)> = queue
        .iter()
        .map(|queued| (queued.text.as_str(), queued.from.as_deref()))
        .collect();
    assert_eq!(held, [("first", None), ("second", Some("hangar"))]);
    hangar_core::bridge::bridge_cancel(&queue_host(), pane, None);
}

// ----- the sleep verdict a menu asks for

/// `env_model`, with the workspace `other` the one a menu is opened on.
fn verdict_of(m: &Model, probed: Option<&HashMap<PaneId, bool>>) -> Option<Verdict> {
    sleep_verdict(m, "other", probed)
}

#[test]
fn a_menu_asks_the_shells_before_it_can_say_a_workspace_may_sleep() {
    let m = env_model();
    // Ava has rested, Leo's shell may be running something: ask the system.
    assert_eq!(
        verdict_of(&m, None),
        Some(Verdict::Probe {
            pane_ids: vec!["leo".into()]
        })
    );
    assert_eq!(
        sleep_line(&m, "other", None),
        Some(SleepLine::Ask(vec!["leo".into()]))
    );

    let quiet: HashMap<PaneId, bool> = [("leo".to_string(), false)].into();
    assert_eq!(
        verdict_of(&m, Some(&quiet)),
        Some(Verdict::Sleep { agents: 1 })
    );
    assert_eq!(
        sleep_line(&m, "other", Some(&quiet)),
        Some(SleepLine::Ready)
    );
}

#[test]
fn a_menu_is_told_why_a_workspace_stays_awake_in_the_persons_language() {
    let mut m = env_model();
    let busy: HashMap<PaneId, bool> = [("leo".to_string(), true)].into();
    let Some(SleepLine::Refused(words)) = sleep_line(&m, "other", Some(&busy)) else {
        panic!("a shell running a program keeps the workspace awake");
    };
    let leo = pane("leo", "Leo", AgentId::Shell);
    let awake = Awake {
        reason: Blocker::ShellChildren,
        pane_id: Some("leo".into()),
    };
    let expected = sleep_refusal(Translator::new(m.locale()), &awake, &[leo], &|_| false);
    assert_eq!(words, expected);

    // Prompts waiting for Ava keep it awake before the shells are asked at all.
    m.live.panes_logic.queue_changed(
        "ava",
        vec![Queued {
            id: "q".into(),
            text: "later".into(),
            from: None,
        }],
        NOW,
    );
    assert!(matches!(
        sleep_verdict(&m, "other", None),
        Some(Verdict::Awake(Awake {
            reason: Blocker::Queued,
            ..
        }))
    ));
    assert!(matches!(
        sleep_line(&m, "other", None),
        Some(SleepLine::Refused(_))
    ));
}

#[test]
fn a_menu_sees_what_the_click_will_decide_on_and_changes_nothing() {
    let mut m = env_model();
    // A workspace already asleep, or one that is gone.
    let version = m.live.version;
    let before = m.live.panes_logic.hibernation.last_touch("other");
    let seen = sleep_verdict(&m, "other", None);
    assert_eq!(m.live.version, version);
    assert_eq!(m.live.panes_logic.hibernation.last_touch("other"), before);

    // The click reads the same rules through the live environment.
    let (step, ..) = env_run(&mut m, Arc::new(Snapshot::default()), |env| {
        hibernation::begin(env, "other", SleepMode::Forced, NOW)
    });
    assert_eq!(
        seen,
        Some(Verdict::Probe {
            pane_ids: vec!["leo".into()]
        })
    );
    assert!(matches!(step, Step::Probe { ref pane_ids, .. } if pane_ids == &["leo".to_string()]));

    assert_eq!(sleep_verdict(&m, "gone", None), None);
    assert_eq!(sleep_line(&m, "gone", None), None);

    m.live
        .panes_logic
        .hibernation
        .mark_asleep("other", Asleep { at: NOW, agents: 1 });
    assert!(matches!(
        sleep_line(&m, "other", None),
        Some(SleepLine::Refused(_))
    ));
}

// ---- The trust question -----------------------------------------------------

/// A terminal whose history still holds Claude Code's question and whose
/// screen holds `screen`.
struct History {
    screen: Vec<String>,
}

const QUESTION: &str = "Quick safety check: Is this a project you trust? Yes, I trust this folder";

impl crate::services::TerminalAccess for History {
    fn read_lines(&self, _pane: &PaneId, _n: usize) -> Option<Vec<String>> {
        Some(vec![QUESTION.into(), "welcome back".into()])
    }

    fn screen_text(&self, _pane: &PaneId) -> Option<Vec<String>> {
        Some(self.screen.clone())
    }
}

#[test]
fn the_trust_question_is_looked_for_on_the_screen_and_not_in_the_history() {
    let pane: PaneId = "p1".into();
    // Answered, and scrolled off: the history holds it, the screen does not.
    let answered = History {
        screen: vec!["welcome back".into()],
    };
    assert!(!super::agents::is_trust_prompt(&screen_of(
        &answered, &pane
    )));
    // On the screen, it is asked, wherever on it.
    let asking = History {
        screen: vec![String::new(), QUESTION.into(), String::new()],
    };
    assert!(super::agents::is_trust_prompt(&screen_of(&asking, &pane)));
    // A pane with no terminal is not asking.
    struct Nothing;
    impl crate::services::TerminalAccess for Nothing {}
    assert_eq!(screen_of(&Nothing, &pane), "");
}

#[test]
fn a_pane_is_found_through_the_index_even_one_it_has_not_caught_up_with() {
    let (mut m, _) = model(vec![
        workspace("w1", vec![pane("ix-a", "Ava", AgentId::Claude)]),
        workspace(
            "w2",
            vec![
                pane("ix-b", "Bob", AgentId::Claude),
                pane("ix-c", "Cid", AgentId::Shell),
            ],
        ),
    ]);
    index_panes(&mut m);
    let (ws, found) = m.find_pane("ix-c").expect("indexed");
    assert_eq!((ws.id.as_str(), found.name.as_str()), ("w2", "Cid"));
    assert_eq!(m.find_workspace("w2").map(|ws| ws.panes.len()), Some(2));
    assert!(m.find_pane("nobody").is_none());

    // A store action moves the panes under the index: what it finds is
    // still what the state says, before and after it catches up.
    m.apply(StoreAction::ClosePane {
        ws: "w2".into(),
        pane: "ix-b".into(),
    });
    for _ in 0..2 {
        assert!(m.find_pane("ix-b").is_none());
        let (ws, found) = m.find_pane("ix-c").expect("still there");
        assert_eq!((ws.id.as_str(), found.id.as_str()), ("w2", "ix-c"));
        index_panes(&mut m);
    }

    // The machines get a `PaneInfo` that says what the state says, the
    // session a capture just stored included.
    m.apply(StoreAction::UpdatePane {
        ws: "w1".into(),
        pane: "ix-a".into(),
        patch: Patch::new(|pane: &mut Pane| pane.session_id = Some("s-new".into())),
    });
    let (info, _) = inputs(&m, "ix-a", NOW).unwrap();
    assert_eq!(info.session_id.as_deref(), Some("s-new"));
    index_panes(&mut m);
    let (info, _) = inputs(&m, "ix-a", NOW).unwrap();
    assert_eq!(info.session_id.as_deref(), Some("s-new"));

    // A state hydrated anew, at the same revision, is indexed anew.
    m.hydrate(AppState {
        workspaces: vec![workspace("w9", vec![pane("ix-z", "Zed", AgentId::Shell)])],
        ..AppState::default()
    });
    index_panes(&mut m);
    assert_eq!(
        m.find_pane("ix-z").map(|(ws, _)| ws.id.as_str()),
        Some("w9")
    );
    assert!(m.find_pane("ix-a").is_none());
}

#[test]
fn a_pane_closed_for_good_loses_its_api_token_and_one_still_in_the_state_keeps_it() {
    let tokens = hangar_core::server::pane_tokens();
    let (mut m, _) = model(vec![workspace(
        "w1",
        vec![
            pane("tok-kept", "Ava", AgentId::Claude),
            pane("tok-closed", "Bob", AgentId::Claude),
        ],
    )]);
    index_panes(&mut m);
    // What their spawn put in their environment.
    let kept = tokens.issue("tok-kept", Some("Ava"));
    let closed = tokens.issue("tok-closed", Some("Bob"));

    m.apply(StoreAction::ClosePane {
        ws: "w1".into(),
        pane: "tok-closed".into(),
    });
    // What the package's next message (`update`, `on_intent`, `on_event`)
    // and the terminals' run first: the pane is seen gone.
    index_panes(&mut m);

    // Revoked: the token issued again for that id is a new one. The other
    // pane, still in the state (as a pane asleep or restarted on its id is),
    // keeps its own.
    assert_ne!(tokens.issue("tok-closed", None), closed);
    assert_eq!(tokens.issue("tok-kept", Some("Ava")), kept);
    tokens.revoke("tok-closed");
    tokens.revoke("tok-kept");
}

#[test]
fn a_session_no_pane_holds_any_more_is_released_and_one_still_held_is_not() {
    let (mut m, _) = model(vec![workspace(
        "w1",
        vec![
            pane("p1", "Ava", AgentId::Claude),
            pane("p2", "Bob", AgentId::Claude),
        ],
    )]);
    m.live.panes_logic.claim_session("s-p1");
    m.live.panes_logic.claim_session("s-p2");
    m.live.panes_logic.claim_session("s-gone");

    release_unheld_sessions(&mut m);

    let claims = &m.live.panes_logic.claims;
    assert!(claims.contains("s-p1"));
    assert!(claims.contains("s-p2"));
    assert!(!claims.contains("s-gone"));
}

// ---- The timers sleep until a job is due -------------------------------------

/// The polls just done, so that what is due next is what the test sets up.
fn polled_at(now: EpochMs) -> State {
    State {
        last_context_poll: now,
        last_branch_poll: now,
        ..State::default()
    }
}

/// A shell and a Claude Code with the mod, both running and quiet.
fn at_rest() -> Model {
    let (mut m, _) = model(vec![workspace(
        "w1",
        vec![
            pane("sh", "sh", AgentId::Shell),
            pane("p1", "Ava", AgentId::Claude),
        ],
    )]);
    for id in ["sh", "p1"] {
        m.live
            .panes_logic
            .status
            .insert(id.into(), PaneStatus::Running);
    }
    bridge(
        &mut m,
        "p1",
        json!({"kind": "hello", "sessionId": "s-p1", "model": "claude-opus-5", "version": "2.1"}),
        NOW,
    );
    m
}

#[test]
fn panes_at_rest_wake_the_window_for_their_branch_alone() {
    let m = at_rest();
    let st = polled_at(NOW);
    // Nothing prints: no check of a burst, no trust look.
    assert_eq!(next_tick_at(&m), None);
    // No mark, no hunt after output, no gauge to read: the branches' refresh
    // is the one job left, 45 s away, where `Slow` used to tick every 1.5 s.
    assert_eq!(next_slow_at(&st, &m), Some(NOW + PANE_BRANCH_REFRESH_MS));
    // Without a pane mounted, nothing at all.
    let st = State::default();
    assert_eq!(next_slow_at(&st, &Model::default()), None);
    assert_eq!(next_tick_at(&Model::default()), None);
}

#[test]
fn output_arms_the_tick_for_the_bursts_next_check_and_rest_disarms_it() {
    let (mut m, _) = model(vec![workspace(
        "w1",
        vec![pane("sh", "sh", AgentId::Shell)],
    )]);
    let live = &mut m.live.panes_logic;
    live.note_printed("sh", NOW, NOW + 10);
    // The first check of the burst comes before the trust look.
    assert_eq!(next_tick_at(&m), Some(NOW + activity::TICK_MS));
    // A short burst falls silent: the watcher stops, the trust look is left,
    // then nothing once it is done.
    let later = NOW + 5_000;
    let (info, amb) = inputs(&m, "sh", later).expect("a mounted pane");
    let live = &mut m.live.panes_logic;
    live.poll_pane(&info, 1000, &amb);
    assert_eq!(next_tick_at(&m), Some(NOW + 10 + pane_agent::TRUST_LOOK_MS));
    let live = &mut m.live.panes_logic;
    assert!(live.trust_look_due("sh", later));
    live.look_for_trust_question(&info, "", &amb);
    assert_eq!(next_tick_at(&m), None);
}

#[test]
fn a_bridged_panes_trust_look_keeps_no_timer_running() {
    let mut m = at_rest();
    // Printed while it had no mod yet: the look is set, but never due.
    let machine = m.live.panes_logic.machines.get_mut("p1").unwrap();
    machine.trust_look_at = Some(NOW);
    assert_eq!(next_tick_at(&m), None);
}

#[test]
fn the_hunt_wakes_slow_only_after_output_and_on_its_cadence() {
    let mut m = at_rest();
    let st = polled_at(NOW);
    let watch = SessionWatch::new("/work/w1".into(), HashSet::new(), NOW);
    m.live.panes_logic.watches.insert("sh".into(), watch);
    // Its round is due at NOW + HUNT_MS, but would do nothing: not a wake.
    assert_eq!(next_slow_at(&st, &m), Some(NOW + PANE_BRANCH_REFRESH_MS));
    // Output long after: the round comes on the slot it would have had if
    // every round in between had been passed, not right away.
    let printed = NOW + session_watch::HUNT_MS * 4 + 100;
    m.live.panes_logic.note_printed("sh", printed, printed);
    assert_eq!(
        next_slow_at(&st, &m),
        Some(NOW + session_watch::HUNT_MS * 5)
    );
}

#[test]
fn an_overlap_mark_wakes_slow_when_it_wears_off() {
    // No pane mounted here, so that the mark is the one job.
    let mut m = Model::default();
    let mut st = polled_at(NOW);
    m.live.panes_logic.agents.flag_overlap("p1", "Bob", NOW);
    assert_eq!(next_slow_at(&st, &m), Some(NOW + OVERLAP_SHOWN_MS));
    // Due, it goes, and nothing is left to wake for.
    let _ = slow(&mut st, &mut m, NOW + OVERLAP_SHOWN_MS);
    assert_eq!(m.live.panes_logic.agents.conflict("p1"), None);
    assert_eq!(next_slow_at(&st, &m), None);
}

#[test]
fn a_gauge_to_read_or_to_clear_wakes_slow_for_the_context_poll() {
    let (mut m, _) = without_the_mod_running();
    let st = polled_at(NOW);
    // An agent without the mod, shown and running: its transcript is read.
    assert_eq!(next_slow_at(&st, &m), Some(NOW + CONTEXT_POLL_MS));
    // Its shell gone, there is nothing to read.
    m.live
        .panes_logic
        .status
        .insert("p1".into(), PaneStatus::Exited);
    assert_eq!(next_slow_at(&st, &m), Some(NOW + PANE_BRANCH_REFRESH_MS));
    // A pane with no session that still shows a gauge: one clear is due.
    let mut state = m.state().clone();
    state.workspaces[0].panes[0].session_id = None;
    m.hydrate(state);
    let gauge = compute_context_gauge(&hangar_core::context::ContextUsage {
        used_tokens: 50_000,
        model: Some("claude-opus-5-5".into()),
    });
    assert!(gauge.is_some());
    m.live.panes_logic.set_transcript_context("p1", gauge);
    assert_eq!(next_slow_at(&st, &m), Some(NOW + CONTEXT_POLL_MS));
    let mut st = st;
    let _ = slow(&mut st, &mut m, NOW + CONTEXT_POLL_MS);
    assert!(!m.live.panes_logic.has_transcript_context("p1"));
    assert_eq!(next_slow_at(&st, &m), Some(NOW + PANE_BRANCH_REFRESH_MS));
}

/// An agent pane whose Claude Code has no mod, running in the workspace on
/// screen.
fn without_the_mod_running() -> (Model, window::Id) {
    let (mut m, main) = one_claude();
    bridge_off(&mut m);
    m.live
        .panes_logic
        .status
        .insert("p1".into(), PaneStatus::Running);
    (m, main)
}

#[tokio::test]
async fn a_wake_fires_at_its_deadline() {
    use iced::futures::StreamExt;
    let start = std::time::Instant::now();
    let at = wall_clock_ms() + 120;
    let mut stream = Box::pin(wake_stream(at, Wake::Slow));
    let first = stream.next().await;
    let waited = start.elapsed();
    assert!(matches!(first, Some(Message::PanesLogic(Msg::Slow))));
    assert!(waited >= Duration::from_millis(110), "{waited:?}");
    assert!(waited < Duration::from_millis(1000), "{waited:?}");
    // A deadline already passed fires at once.
    let start = std::time::Instant::now();
    let mut late = Box::pin(wake_stream(wall_clock_ms() - 5_000, Wake::Tick));
    assert!(matches!(
        late.next().await,
        Some(Message::PanesLogic(Msg::Tick))
    ));
    assert!(start.elapsed() < Duration::from_millis(100));
}

#[test]
fn the_subscription_holds_a_timer_only_for_a_job_due() {
    let mut m = at_rest();
    let mut state = m.state().clone();
    state.settings.hibernate_idle = false;
    m.hydrate(state);
    let st = polled_at(NOW);
    // The focus listener and the branches' wake.
    assert_eq!(subscription(&st, &m).units(), 2);
    m.live.panes_logic.note_printed("sh", NOW, NOW);
    assert_eq!(subscription(&st, &m).units(), 3);
}
