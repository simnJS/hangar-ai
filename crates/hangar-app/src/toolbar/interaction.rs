//! The toolbar driven with real pointer events: what its controls and its
//! menus say to the app.
#![allow(clippy::field_reassign_with_default)]

use hangar_i18n::{Key, Locale, Translator, args};
use hangar_ui::menu as ui_menu;

use super::*;
use crate::app::Packages;
use crate::navigator::testing::{Drive, ctx, model_of, pane, simulate, workspace};
use crate::state::types::{AgentId, AppState, PaneRole};

fn model() -> Model {
    let mut ws = workspace(
        "w",
        "hangar",
        None,
        vec![
            pane("p1", "Ava", AgentId::Claude),
            pane("p2", "Ben", AgentId::Codex),
            pane("p3", "Dan", AgentId::Shell),
        ],
    );
    ws.panes[0].role = Some(PaneRole::Orchestrator);
    ws.panes[1].role = Some(PaneRole::Worker);
    let mut model = model_of(AppState {
        workspaces: vec![ws],
        active_workspace_id: Some("w".into()),
        ..AppState::default()
    });
    model.ui.view = View::Terminals;
    model
}

fn run(
    st: &State,
    model: &Model,
    drive: impl FnOnce(&mut iced_test::Simulator<'_, Message>),
) -> Vec<Message> {
    let pkg = Packages::default();
    let ctx = ctx(model, &pkg);
    let mut ui = simulate(iced::widget::container(view(st, ctx)).height(44));
    drive(&mut ui);
    ui.into_messages().collect()
}

fn sent(messages: &[Message], wanted: impl Fn(&Intent) -> bool) -> bool {
    messages.iter().any(|message| match message {
        Message::Intent(intent) => wanted(intent),
        _ => false,
    })
}

fn toolbar(messages: &[Message], wanted: impl Fn(&Msg) -> bool) -> bool {
    messages.iter().any(|message| match message {
        Message::Toolbar(msg) => wanted(msg),
        _ => false,
    })
}

const EN: Translator = Translator::new(Locale::En);

#[test]
fn a_view_button_switches_the_view() {
    let messages = run(&State::default(), &model(), |ui| {
        ui.click("Board").expect("the view is offered");
    });
    assert!(
        sent(&messages, |i| matches!(i, Intent::SetView(View::Board))),
        "{messages:?}"
    );
}

#[test]
fn each_of_the_four_views_is_offered_by_name() {
    let model = model();
    let pkg = Packages::default();
    let st = State::default();
    let mut ui = simulate(view(&st, ctx(&model, &pkg)));
    for name in ["Terminals", "Board", "Memory", "Statistics"] {
        assert!(ui.find(name).is_ok(), "{name} is missing");
    }
}

#[test]
fn the_toolbar_names_the_workspace() {
    let model = model();
    let pkg = Packages::default();
    let st = State::default();
    let mut ui = simulate(view(&st, ctx(&model, &pkg)));
    assert!(ui.find("hangar").is_ok());
}

#[test]
fn the_capsule_names_the_focused_pane_and_its_state() {
    let mut model = model();
    model
        .ui
        .focus_by_workspace
        .insert("w".into(), "p2".to_string());
    let pkg = Packages::default();
    let st = State::default();
    let mut ui = simulate(view(&st, ctx(&model, &pkg)));
    assert!(ui.find("Ben").is_ok());
    assert!(ui.find("Idle").is_ok());
}

#[test]
fn with_no_pane_in_front_the_capsule_says_the_workspace_is_quiet() {
    let model = model();
    let pkg = Packages::default();
    let st = State::default();
    let mut ui = simulate(view(&st, ctx(&model, &pkg)));
    assert!(ui.find("No agent at work").is_ok());
}

/// The « … » menu on its own at the window's corner, which a click picks a
/// line of by its height: five pixels of padding, then each line 26 pixels
/// tall and each separator 9.
fn pick_line(model: &Model, y: f32) -> Vec<Message> {
    let pkg = Packages::default();
    let ctx = ctx(model, &pkg);
    let ws = model.active_workspace().unwrap();
    let entries = menus::more_entries(ctx, ws, true, false, true, false);
    let mut ui = simulate(ui_menu::menu(ctx.theme, entries));
    ui.move_to(40.0, y);
    ui.press(iced::mouse::Button::Left);
    ui.release(iced::mouse::Button::Left);
    ui.into_messages().collect()
}

/// The intents a pick of a line of the menu hands to the toolbar.
fn picked(messages: &[Message]) -> Vec<String> {
    messages
        .iter()
        .filter_map(|message| match message {
            Message::Toolbar(Msg::Pick(list)) => Some(format!("{list:?}")),
            _ => None,
        })
        .collect()
}

// Open in a window 5..31, —, Command palette 40..66, —, Orchestration 75..101,
// Stop the workers 101..127, —, Reset agents… 136..162.

#[test]
fn the_more_menu_opens_the_workspace_in_a_window() {
    let picked = picked(&pick_line(&model(), 18.0));
    assert_eq!(picked, ["[Detach(\"w\")]"]);
}

#[test]
fn the_more_menu_opens_the_command_palette() {
    let picked = picked(&pick_line(&model(), 53.0));
    assert_eq!(picked, ["[TogglePalette]"]);
}

#[test]
fn an_orchestrated_workspace_has_its_settings_in_the_more_menu() {
    let picked = picked(&pick_line(&model(), 88.0));
    assert_eq!(picked, ["[OpenOrchestration(\"w\")]"]);
}

#[test]
fn stopping_the_workers_takes_over_from_a_reset_that_was_asking() {
    let picked = picked(&pick_line(&model(), 114.0));
    assert_eq!(picked, ["[ArmResetAgents(false), ArmStopWorkers(true)]"]);
}

#[test]
fn resetting_the_agents_takes_over_from_a_stop_that_was_asking() {
    let picked = picked(&pick_line(&model(), 149.0));
    assert_eq!(picked, ["[ArmStopWorkers(false), ArmResetAgents(true)]"]);
}

#[test]
fn a_menu_line_that_is_not_there_picks_nothing() {
    // Below the last line, in the padding.
    assert!(picked(&pick_line(&model(), 166.0)).is_empty());
}

#[test]
fn an_armed_reset_asks_how_many_agents_and_cancels_or_confirms() {
    let mut model = model();
    model.ui.confirm_reset = true;
    // Two agents run in the workspace; the third pane is a shell.
    let title = EN
        .t_with(Key::ToolbarResetTitle, &args!("n" => 2))
        .into_owned();
    let confirm = EN
        .t_with(Key::TopbarResetAllConfirm, &args!("n" => 2))
        .into_owned();

    let pkg = Packages::default();
    let st = State::default();
    let mut ui = simulate(view(&st, ctx(&model, &pkg)));
    assert!(ui.find(title.as_str()).is_ok(), "{title}");
    drop(ui);

    let cancelled = run(&State::default(), &model, |ui| {
        ui.click("Cancel").expect("the safe answer is shown");
    });
    assert!(
        sent(&cancelled, |i| matches!(i, Intent::ArmResetAgents(false))),
        "{cancelled:?}"
    );

    let confirmed = run(&State::default(), &model, |ui| {
        ui.click(confirm.as_str())
            .expect("the confirmation is shown");
    });
    assert!(
        sent(
            &confirmed,
            |i| matches!(i, Intent::ResetAgents(ws) if ws == "w")
        ),
        "{confirmed:?}"
    );
}

#[test]
fn an_armed_stop_asks_about_the_workers() {
    let mut model = model();
    model.ui.confirm_stop_workers = true;
    let confirm = EN
        .t_with(Key::OrchStopConfirm, &args!("n" => 1))
        .into_owned();
    let confirmed = run(&State::default(), &model, |ui| {
        ui.click(confirm.as_str())
            .expect("the confirmation is shown");
    });
    assert!(
        sent(
            &confirmed,
            |i| matches!(i, Intent::StopWorkers(ws) if ws == "w")
        ),
        "{confirmed:?}"
    );
}

#[test]
fn a_preset_that_would_stop_a_busy_agent_names_it_and_asks() {
    let mut st = State::default();
    st.preset = Some(PresetAsk {
        ws: "w".into(),
        view: View::Terminals,
        size: 2,
        closing: vec![("p3".into(), "Dan".into())],
        busy: vec![("Dan".into(), AgentActivity::Working)],
    });
    // Two panes of three stay at a preset of 2: it is the 2 button that asks.
    let model = model();
    let title = EN
        .t_with(Key::ToolbarPresetTitle, &args!("names" => "Dan"))
        .into_owned();
    let pkg = Packages::default();
    let mut ui = simulate(view(&st, ctx(&model, &pkg)));
    assert!(ui.find(title.as_str()).is_ok(), "{title}");
    assert!(ui.find("Working").is_ok());
    drop(ui);

    let cancelled = run(&st, &model, |ui| {
        ui.click("Cancel").unwrap();
    });
    assert!(
        toolbar(&cancelled, |msg| matches!(msg, Msg::PresetCancel)),
        "{cancelled:?}"
    );
    let confirmed = run(&st, &model, |ui| {
        ui.click("Close 1 pane").unwrap();
    });
    assert!(
        toolbar(&confirmed, |msg| matches!(msg, Msg::PresetConfirm)),
        "{confirmed:?}"
    );
}

#[test]
fn a_question_about_another_workspace_or_view_is_not_asked_here() {
    let mut st = State::default();
    st.preset = Some(PresetAsk {
        ws: "elsewhere".into(),
        view: View::Terminals,
        size: 2,
        closing: vec![("p3".into(), "Dan".into())],
        busy: vec![("Dan".into(), AgentActivity::Working)],
    });
    let model = model();
    let pkg = Packages::default();
    let mut ui = simulate(view(&st, ctx(&model, &pkg)));
    assert!(ui.find("Close Dan?").is_err());
}
