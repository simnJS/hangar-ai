//! The page's keyboard and pointer logic: which key a chart takes, how the
//! readout moves, and who has the readout, the pointer or the keys.

use hangar_core::stats::{ScanInfo, UsageStats};
use iced::keyboard::{Key as KeyboardKey, Modifiers, key::Named};

use super::charts::Event;
use super::*;

fn named(key: Named) -> KeyboardKey {
    KeyboardKey::Named(key)
}

/// A page whose last summary is a week with no data in it: seven columns.
fn week() -> State {
    let data = UsageStats {
        generated_at: 0,
        scan: ScanInfo::default(),
        sources: Vec::new(),
        active_gap_cap_ms: 0,
        sessions: Vec::new(),
        models: Vec::new(),
        buckets: Vec::new(),
    };
    let projects: Vec<Project> = Vec::new();
    let summary = model::summarize(
        &data,
        &SummaryOptions {
            period: Period::D7,
            project: None,
            measure: Measure::Tokens,
            now: time::now_ms(),
            projects: &projects,
            agent_label: &|_| String::new(),
            top_sessions: TOP_SESSIONS,
        },
    );
    let st = State::default();
    *st.computed.lock().unwrap() = Some((
        ComputedFor {
            generation: 0,
            period: Period::D7,
            measure: Measure::Tokens,
            project: None,
            roots: 0,
            day: 0,
        },
        Arc::new(Computed {
            summary: Arc::new(summary),
            projects,
        }),
    ));
    st
}

#[test]
fn only_the_focused_chart_takes_keys() {
    let none = Modifiers::empty();
    assert_eq!(chart_key(None, true, &named(Named::ArrowLeft), none), None);
    assert_eq!(
        chart_key(Some(ChartId::Columns), false, &named(Named::ArrowLeft), none),
        Some(ChartKey::Left)
    );
    assert_eq!(
        chart_key(Some(ChartId::Columns), false, &named(Named::End), none),
        Some(ChartKey::End)
    );
}

#[test]
fn up_and_down_belong_to_the_heatmap() {
    let none = Modifiers::empty();
    assert_eq!(
        chart_key(Some(ChartId::Columns), true, &named(Named::ArrowUp), none),
        None
    );
    assert_eq!(
        chart_key(Some(ChartId::Heat), true, &named(Named::ArrowUp), none),
        Some(ChartKey::Up)
    );
    assert_eq!(
        chart_key(Some(ChartId::Heat), true, &named(Named::ArrowDown), none),
        Some(ChartKey::Down)
    );
}

#[test]
fn escape_is_the_charts_only_while_it_shows_a_readout() {
    let none = Modifiers::empty();
    assert_eq!(
        chart_key(Some(ChartId::Heat), false, &named(Named::Escape), none),
        None
    );
    assert_eq!(
        chart_key(Some(ChartId::Heat), true, &named(Named::Escape), none),
        Some(ChartKey::Escape)
    );
}

#[test]
fn a_shortcut_is_not_a_chart_key() {
    assert_eq!(
        chart_key(
            Some(ChartId::Columns),
            true,
            &named(Named::ArrowLeft),
            Modifiers::CTRL
        ),
        None
    );
    assert_eq!(
        chart_key(
            Some(ChartId::Columns),
            true,
            &KeyboardKey::Character("a".into()),
            Modifiers::empty()
        ),
        None
    );
}

#[test]
fn the_keys_walk_the_columns_from_the_last_and_stop_at_the_ends() {
    let mut st = week();
    st.focus = Some(ChartId::Columns);
    // The first key starts at the last column (today).
    walk(&mut st, ChartKey::Left);
    assert_eq!(st.columns, Some(5));
    assert!(st.keyboard);
    walk(&mut st, ChartKey::Home);
    assert_eq!(st.columns, Some(0));
    walk(&mut st, ChartKey::Left);
    assert_eq!(st.columns, Some(0));
    walk(&mut st, ChartKey::End);
    assert_eq!(st.columns, Some(6));
    walk(&mut st, ChartKey::Right);
    assert_eq!(st.columns, Some(6));
    walk(&mut st, ChartKey::Escape);
    assert_eq!(st.columns, None);
}

#[test]
fn the_keys_walk_the_heatmap_in_both_directions() {
    let mut st = State {
        focus: Some(ChartId::Heat),
        heat: Some((0, 0)),
        ..State::default()
    };
    walk(&mut st, ChartKey::Up);
    assert_eq!(st.heat, Some((0, 0)));
    walk(&mut st, ChartKey::Down);
    walk(&mut st, ChartKey::Right);
    assert_eq!(st.heat, Some((1, 1)));
    walk(&mut st, ChartKey::End);
    assert_eq!(st.heat, Some((1, 23)));
    walk(&mut st, ChartKey::Right);
    assert_eq!(st.heat, Some((1, 23)));
    walk(&mut st, ChartKey::Home);
    assert_eq!(st.heat, Some((1, 0)));
    for _ in 0..10 {
        walk(&mut st, ChartKey::Down);
    }
    assert_eq!(st.heat, Some((6, 0)));
}

#[test]
fn a_heatmap_walk_with_no_cell_starts_today() {
    let mut st = State {
        focus: Some(ChartId::Heat),
        ..State::default()
    };
    walk(&mut st, ChartKey::Right);
    let (day, hour) = today_cell().unwrap();
    assert_eq!(st.heat, Some((day, (hour + 1).min(23))));
}

#[test]
fn keys_do_nothing_when_no_chart_has_them() {
    let mut st = week();
    walk(&mut st, ChartKey::Left);
    assert_eq!(st.columns, None);
    assert!(!st.keyboard);
}

#[test]
fn the_pointer_takes_the_readout_back_from_the_keys() {
    let mut st = week();
    st.focus = Some(ChartId::Columns);
    walk(&mut st, ChartKey::Home);
    assert!(st.keyboard);
    // Leaving the chart does not clear what the keys are reading.
    chart_event(&mut st, ChartId::Columns, Event::Hover(None));
    assert_eq!(st.columns, Some(0));
    chart_event(&mut st, ChartId::Columns, Event::Hover(Some((3, 0))));
    assert_eq!(st.columns, Some(3));
    assert!(!st.keyboard);
    // Now it is the pointer's, and leaving clears it.
    chart_event(&mut st, ChartId::Columns, Event::Hover(None));
    assert_eq!(st.columns, None);
}

#[test]
fn a_click_elsewhere_lets_the_keys_go_and_the_readout_with_them() {
    let mut st = week();
    chart_event(&mut st, ChartId::Columns, Event::Focus);
    assert_eq!(st.focus, Some(ChartId::Columns));
    walk(&mut st, ChartKey::Home);
    chart_event(&mut st, ChartId::Heat, Event::Blur);
    // Another chart's blur is not ours.
    assert_eq!(st.focus, Some(ChartId::Columns));
    chart_event(&mut st, ChartId::Columns, Event::Blur);
    assert_eq!(st.focus, None);
    assert_eq!(st.columns, None);
    assert!(!st.keyboard);
}
