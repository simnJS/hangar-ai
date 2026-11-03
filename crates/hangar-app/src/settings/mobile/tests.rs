//! The page's controller, for the parts that need nothing from the PC: a
//! phone arriving while a code is shown, the ports of a workspace.

use super::*;
use crate::state::types::{AppState, Workspace};

mod drawn;
mod guarded;

fn device(id: &str, name: &str, created: u64) -> RemoteDevice {
    RemoteDevice {
        id: id.into(),
        name: name.into(),
        created,
        last_seen: 0,
    }
}

fn showing_a_code(known: &[&str]) -> State {
    let start = PairStart {
        code: "123456".into(),
        url: "https://pc/?code=123456".into(),
        qr_svg: "<svg/>".into(),
        expires_at: 0,
    };
    State {
        pairing: Some(Pairing {
            qr: svg::Handle::from_memory(start.qr_svg.clone().into_bytes()),
            start,
        }),
        known: known.iter().map(|id| id.to_string()).collect(),
        ..State::default()
    }
}

#[test]
fn a_phone_that_was_not_there_closes_the_code_and_is_named() {
    let mut st = showing_a_code(&["a"]);
    let flash = st.devices_arrived(vec![device("a", "Pixel", 1), device("b", "iPhone", 2)]);
    assert_eq!(flash, Some(1));
    assert!(!st.pairing_open());
    assert_eq!(st.just_paired, Some(("iPhone".into(), 1)));
    // The list is the newest first, and is what the next refresh compares to.
    assert_eq!(st.devices[0].id, "b");
    assert!(st.known.contains("b"));
}

#[test]
fn a_refresh_that_finds_nobody_new_leaves_the_code_up() {
    let mut st = showing_a_code(&["a"]);
    assert_eq!(st.devices_arrived(vec![device("a", "Pixel", 1)]), None);
    assert!(st.pairing_open());
    assert_eq!(st.just_paired, None);
}

#[test]
fn without_a_code_a_new_phone_is_only_listed() {
    let mut st = State::default();
    assert_eq!(st.devices_arrived(vec![device("a", "Pixel", 1)]), None);
    assert_eq!(st.devices.len(), 1);
    assert_eq!(st.just_paired, None);
}

fn project(id: &str, ports: &[u16]) -> AppProject {
    AppProject {
        id: id.into(),
        name: id.to_uppercase(),
        ports: ports.to_vec(),
    }
}

fn written(action: StoreAction, before: Option<Vec<u16>>) -> Option<Vec<u16>> {
    let StoreAction::UpdateWorkspace(_, patch) = action else {
        panic!("not a workspace patch");
    };
    let mut ws = Workspace {
        mobile_ports: before,
        ..Workspace::default()
    };
    patch.apply(&mut ws);
    ws.mobile_ports
}

#[test]
fn a_typed_port_is_added_to_its_workspaces_list_and_the_field_empties() {
    let mut st = State::default();
    st.port_drafts.insert("w1".into(), " 5173 ".into());
    let action = st.add_port(&[project("w1", &[3000])], "w1").expect("added");
    assert_eq!(written(action, Some(vec![3000])), Some(vec![3000, 5173]));
    assert!(!st.port_drafts.contains_key("w1"));
    assert!(!st.port_problems.contains_key("w1"));
}

#[test]
fn a_port_that_is_refused_says_why_and_keeps_what_was_typed() {
    let mut st = State::default();
    st.port_drafts.insert("w1".into(), "80".into());
    assert!(st.add_port(&[project("w1", &[])], "w1").is_none());
    assert_eq!(st.port_problems["w1"], AppPortProblem::Range);
    assert_eq!(st.port_drafts["w1"], "80");
    // Hangar's own ports are refused from the first answer on, before the PC
    // has listed its slots.
    st.port_drafts.insert("w1".into(), "47822".into());
    assert!(st.add_port(&[project("w1", &[])], "w1").is_none());
    assert_eq!(st.port_problems["w1"], AppPortProblem::Hangar);
    st.port_drafts.insert("w1".into(), "3000".into());
    assert!(st.add_port(&[project("w1", &[3000])], "w1").is_none());
    assert_eq!(st.port_problems["w1"], AppPortProblem::Duplicate);
}

#[test]
fn a_workspace_that_is_gone_adds_nothing() {
    let mut st = State::default();
    st.port_drafts.insert("w9".into(), "3000".into());
    assert!(st.add_port(&[project("w1", &[])], "w9").is_none());
}

#[test]
fn an_emptied_list_leaves_the_file_rather_than_being_written_empty() {
    assert_eq!(written(ports_action("w1", vec![]), Some(vec![3000])), None);
    assert_eq!(
        written(ports_action("w1", vec![3000]), None),
        Some(vec![3000])
    );
}

#[test]
fn the_hq_is_not_offered_a_port() {
    let mut model = Model::default();
    model.hydrate(AppState {
        workspaces: vec![
            Workspace {
                id: "w1".into(),
                name: "Shop".into(),
                mobile_ports: Some(vec![5173, 3000]),
                ..Workspace::default()
            },
            Workspace {
                id: "hq".into(),
                hq: Some(true),
                ..Workspace::default()
            },
        ],
        ..AppState::default()
    });
    let listed = projects(&model);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].ports, [3000, 5173]);
}
