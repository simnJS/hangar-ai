//! remoteApps.test.ts, case for case.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::control::port::Port;
use crate::state::types::PaneRole;
use crate::state::{AppState, Pane, Workspace};

use super::super::apps::{candidates_of, remote_apps};
use super::super::contract::value;
use super::super::engine::{Mobile, locked, run};
use super::fake::{App, Fakes, assert_equal, claude, workspace};

fn worker(id: &str, name: &str, port_base: Option<u32>, task: Option<&str>) -> Pane {
    Pane {
        port_base,
        task_id: task.map(str::to_string),
        ..claude(id, name, Some(PaneRole::Worker))
    }
}

fn with_ports(mut ws: Workspace, ports: &[u16]) -> Workspace {
    ws.mobile_ports = Some(ports.to_vec());
    ws
}

fn shop() -> Workspace {
    with_ports(
        workspace(
            "w-shop",
            "Shop",
            vec![
                claude("o1", "Jack", Some(PaneRole::Orchestrator)),
                worker("p-ada", "Ada", Some(21000), Some("t-cart")),
                worker("p-ben", "Ben", Some(21020), Some("t-pay")),
            ],
        ),
        &[5173],
    )
}

fn blog() -> Workspace {
    with_ports(
        workspace("w-blog", "Blog", vec![claude("p-sh", "shell", None)]),
        &[3000, 4321],
    )
}

fn hq() -> Workspace {
    let mut hq = with_ports(
        workspace(
            "hq",
            "HQ",
            vec![
                claude("pm", "Max", Some(PaneRole::Manager)),
                worker("p-hq", "Odd", Some(21100), None),
            ],
        ),
        &[8080],
    );
    hq.hq = Some(true);
    hq
}

struct Rig {
    fakes: Arc<Fakes>,
    mobile: Arc<Mobile>,
    port: Port,
}

type Boards = Vec<(
    &'static str,
    Result<Vec<(&'static str, &'static str)>, &'static str>,
)>;

/// The ports that listen, and each folder's board.
fn rig(workspaces: Vec<Workspace>, listening: &[u16], boards: Boards) -> Rig {
    let mut state = AppState {
        workspaces,
        ..AppState::default()
    };
    state.settings.agent_pane_control = false;
    let app = App::new(state);
    let fakes = Fakes::new(app.clone());
    let open = listening.to_vec();
    *locked(&fakes.listening) = Box::new(move |ports| {
        Ok(ports
            .into_iter()
            .filter(|port| open.contains(port))
            .collect())
    });
    for (cwd, board) in boards {
        let board = board
            .map(|tasks| {
                tasks
                    .into_iter()
                    .map(|(id, title)| (id.to_string(), title.to_string()))
                    .collect()
            })
            .map_err(str::to_string);
        locked(&fakes.tasks).insert(cwd.to_string(), board);
    }
    let mobile = fakes.mobile();
    Rig {
        fakes,
        mobile,
        port: app.port(),
    }
}

fn shop_board() -> Boards {
    vec![(
        "C:\\dev\\w-shop",
        Ok(vec![
            ("t-cart", "The cart keeps its items"),
            ("t-pay", "Pay by card"),
        ]),
    )]
}

impl Rig {
    async fn apps(&self, args: Value) -> Result<Value, String> {
        remote_apps(&self.port, &args, self.mobile.sources())
            .await
            .and_then(|answer| value(&answer))
    }

    fn probed(&self) -> Vec<Vec<u16>> {
        locked(&self.fakes.probed).clone()
    }

    fn boards(&self) -> Vec<String> {
        locked(&self.fakes.boards).clone()
    }
}

fn ports_of(answer: &Value) -> Vec<u64> {
    answer["apps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|app| app["port"].as_u64().unwrap())
        .collect()
}

/* ── remote_apps ──────────────────────────────────────────────── */

#[tokio::test]
async fn lists_every_worker_s_port_block_and_every_port_the_person_added_kept_to_the_ones_that_listen()
 {
    let r = rig(
        vec![shop(), blog()],
        &[21003, 21020, 5173, 4321, 9999],
        shop_board(),
    );
    assert_equal(
        &r.apps(json!({})).await.unwrap(),
        json!({"apps": [
            {"workspaceId": "w-shop", "workspace": "Shop", "port": 21003, "source": "worker", "pane": "Ada", "task": "The cart keeps its items"},
            {"workspaceId": "w-shop", "workspace": "Shop", "port": 21020, "source": "worker", "pane": "Ben", "task": "Pay by card"},
            {"workspaceId": "w-shop", "workspace": "Shop", "port": 5173, "source": "custom"},
            {"workspaceId": "w-blog", "workspace": "Blog", "port": 4321, "source": "custom"},
        ]}),
    );
    // Each worker's twenty ports, then the person's, asked once.
    let probed = r.probed();
    assert_eq!(probed.len(), 1);
    assert_eq!(probed[0].len(), 20 + 20 + 1 + 2);
    assert_eq!(probed[0][..2], [21000, 21001]);
    assert!(probed[0].contains(&21039));
    assert!(!probed[0].contains(&21040));
    assert_eq!(probed[0][probed[0].len() - 3..], [5173, 3000, 4321]);
}

#[tokio::test]
async fn lists_nothing_when_no_server_listens_and_reads_no_board_then() {
    let r = rig(vec![shop(), blog()], &[], shop_board());
    assert_equal(&r.apps(json!({})).await.unwrap(), json!({"apps": []}));
    assert!(r.boards().is_empty());
}

#[tokio::test]
async fn leaves_the_hq_out_its_manager_s_ports_and_any_it_was_given_are_no_project_s() {
    let r = rig(vec![hq(), shop()], &[21100, 8080, 5173], vec![]);
    let answer = r.apps(json!({})).await.unwrap();
    let ids: Vec<&str> = answer["apps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|app| app["workspaceId"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["w-shop"]);
    let probed: Vec<u16> = r.probed().concat();
    assert!(!probed.contains(&21100));
    assert!(!probed.contains(&8080));
}

#[tokio::test]
async fn asks_nothing_of_the_pc_when_no_project_has_a_port_to_offer() {
    let r = rig(
        vec![
            hq(),
            workspace("w", "Plain", vec![claude("p", "Sam", None)]),
        ],
        &[3000],
        vec![],
    );
    assert_equal(&r.apps(json!({})).await.unwrap(), json!({"apps": []}));
    assert!(r.probed().is_empty());
}

#[tokio::test]
async fn lists_one_project_s_when_asked_by_id_or_by_name() {
    let r = rig(vec![shop(), blog()], &[21003, 5173, 3000], shop_board());
    assert_equal(
        &r.apps(json!({"workspace": "w-blog"})).await.unwrap()["apps"],
        json!([{"workspaceId": "w-blog", "workspace": "Blog", "port": 3000, "source": "custom"}]),
    );
    assert_eq!(
        ports_of(&r.apps(json!({"workspace": " shop "})).await.unwrap()),
        [21003, 5173]
    );
}

#[tokio::test]
async fn refuses_a_project_it_does_not_know_the_hq_included() {
    let r = rig(vec![hq(), shop()], &[3000], vec![]);
    assert_eq!(
        r.apps(json!({"workspace": "w-nope"})).await,
        Err("No project \"w-nope\" in Hangar.".into())
    );
    assert_eq!(
        r.apps(json!({"workspace": "hq"})).await,
        Err("No project \"hq\" in Hangar.".into())
    );
    assert!(
        r.apps(json!({"workspace": ""}))
            .await
            .unwrap_err()
            .contains("Name the project")
    );
}

#[tokio::test]
async fn names_a_worker_s_task_only_when_its_board_has_it_and_lists_the_app_all_the_same() {
    let lost = workspace(
        "w-lost",
        "Lost",
        vec![
            worker("p-cy", "Cy", Some(21200), Some("t-gone")),
            worker("p-di", "Di", Some(21220), None),
        ],
    );
    let r = rig(
        vec![lost],
        &[21200, 21220],
        vec![("C:\\dev\\w-lost", Ok(vec![("t-other", "Another task")]))],
    );
    assert_equal(
        &r.apps(json!({})).await.unwrap()["apps"],
        json!([
            {"workspaceId": "w-lost", "workspace": "Lost", "port": 21200, "source": "worker", "pane": "Cy"},
            {"workspaceId": "w-lost", "workspace": "Lost", "port": 21220, "source": "worker", "pane": "Di"},
        ]),
    );
}

#[tokio::test]
async fn lists_a_worker_s_app_even_when_its_board_cannot_be_read() {
    let r = rig(
        vec![shop()],
        &[21000],
        vec![("C:\\dev\\w-shop", Err("board.json is not valid JSON"))],
    );
    assert_equal(
        &r.apps(json!({})).await.unwrap()["apps"],
        json!([{"workspaceId": "w-shop", "workspace": "Shop", "port": 21000, "source": "worker", "pane": "Ada"}]),
    );
}

#[tokio::test]
async fn reads_only_the_boards_of_the_projects_whose_workers_have_an_app_up() {
    let r = rig(vec![shop(), blog()], &[21003, 3000], shop_board());
    r.apps(json!({})).await.unwrap();
    assert_eq!(r.boards(), ["C:\\dev\\w-shop"]);
}

#[tokio::test]
async fn fails_when_the_pc_cannot_say_which_ports_listen_rather_than_list_them_all() {
    let r = rig(vec![shop()], &[], vec![]);
    *locked(&r.fakes.listening) =
        Box::new(|_| Err("remote_ports_listening is not a command".into()));
    assert!(
        r.apps(json!({}))
            .await
            .unwrap_err()
            .contains("not a command")
    );
}

/* ── The ports a project offers ───────────────────────────────── */

fn candidate_ports(ws: &Workspace) -> Vec<u16> {
    candidates_of(&[ws])
        .into_iter()
        .map(|candidate| candidate.app.port)
        .collect()
}

#[test]
fn offers_skip_a_worker_without_a_port_block_and_the_panes_that_are_not_workers() {
    let ws = workspace(
        "w",
        "W",
        vec![
            Pane {
                port_base: Some(21000),
                ..claude("o", "Jo", Some(PaneRole::Orchestrator))
            },
            worker("a", "Al", None, None),
            worker("b", "Bo", Some(21040), None),
        ],
    );
    let ports = candidate_ports(&ws);
    assert_eq!(ports.len(), 20);
    assert_eq!(ports[0], 21040);
}

#[test]
fn offers_keep_a_port_the_person_added_once_a_worker_s_when_it_falls_in_its_block() {
    let ws = with_ports(
        workspace("w", "W", vec![worker("b", "Bo", Some(21040), None)]),
        &[21045, 5173, 5173, 3000],
    );
    let candidates = candidates_of(&[&ws]);
    let custom: Vec<u16> = candidates
        .iter()
        .filter(|c| c.app.source == "custom")
        .map(|c| c.app.port)
        .collect();
    assert_eq!(custom, [3000, 5173]);
    assert_eq!(
        candidates
            .iter()
            .find(|c| c.app.port == 21045)
            .unwrap()
            .app
            .source,
        "worker"
    );
}

#[test]
fn offers_leave_out_what_the_file_holds_that_is_not_a_port_the_person_could_have_added() {
    // Read from the file as the store reads it, list entries one by one.
    let state = crate::state::read_state(json!({"workspaces": [{
        "id": "w", "name": "W", "cwd": "C:\\dev\\w", "panes": [],
        "mobilePorts": [80, 5173.5, 70000, "3000", null, 4000],
    }]}));
    assert_eq!(candidate_ports(&state.workspaces[0]), [4000]);
}

/* ── Through the phone's requests ─────────────────────────────── */

#[tokio::test]
async fn answers_remote_apps_whatever_the_agents_switch_says() {
    let r = rig(vec![shop()], &[5173], vec![]);
    let answer = run("remote_apps", &json!({}), &r.port, &r.mobile)
        .await
        .unwrap();
    assert_equal(
        &answer,
        json!({"apps": [{"workspaceId": "w-shop", "workspace": "Shop", "port": 5173, "source": "custom"}]}),
    );
}
