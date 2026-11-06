//! The nodes' connection layer, without SSH: the command lines of each step,
//! what is read back, the version decision, and the state machine run
//! against a fake runner that answers like a machine would.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::commands::{self, Cmd};
use super::connect::{self, Setup, Timing, TunnelSlot, locked};
use super::model::*;
use super::runner::{CmdOutput, Runner, Tunnel, parse_response};
use super::{AddForm, Live, NodeClient, State, apply_status, node_from_form};
use crate::state::types::NodeConfig;
use crate::state::{Model, Settings, read_state};

fn node(config: Option<&str>) -> NodeConfig {
    NodeConfig {
        id: "n1".into(),
        label: "Box".into(),
        ssh_target: "me@box".into(),
        ssh_config: config.map(str::to_string),
        extra: Default::default(),
    }
}

fn line(cmd: &Cmd) -> String {
    std::iter::once(cmd.program.as_str())
        .chain(cmd.args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" | ")
}

// ── command lines ─────────────────────────────────────────────────────

#[test]
fn every_ssh_step_is_batch_mode_with_a_bounded_wait() {
    let n = node(None);
    for cmd in [
        commands::probe(&n),
        commands::make_bin_dir(&n),
        commands::put_in_place(&n),
        commands::serve(&n),
        commands::read_node_json(&n),
        commands::stop(&n, 42),
        commands::tunnel(&n, 21080, 7000),
    ] {
        assert_eq!(cmd.program, "ssh");
        assert_eq!(
            cmd.args[..4],
            ["-o", "BatchMode=yes", "-o", "ConnectTimeout=10"],
            "{}",
            line(&cmd)
        );
        assert!(!cmd.args.contains(&"-F".to_string()), "{}", line(&cmd));
    }
}

#[test]
fn the_probe_asks_the_architecture_and_a_version_that_may_be_missing() {
    assert_eq!(
        line(&commands::probe(&node(None))),
        "ssh | -o | BatchMode=yes | -o | ConnectTimeout=10 | me@box | \
         uname -m; ~/.local/share/hangar-node/bin/hangar-node version 2>/dev/null || true"
    );
}

#[test]
fn a_config_file_is_passed_as_dash_f_before_the_target() {
    let n = node(Some("/tmp/rig/ssh_config"));
    let cmd = commands::serve(&n);
    assert_eq!(
        line(&cmd),
        "ssh | -o | BatchMode=yes | -o | ConnectTimeout=10 | -F | /tmp/rig/ssh_config | me@box | \
         ~/.local/share/hangar-node/bin/hangar-node serve --detach"
    );
    let upload = commands::upload(&n, Path::new("/opt/hangar/hangar-node-x86_64"));
    assert_eq!(
        line(&upload),
        "scp | -o | BatchMode=yes | -o | ConnectTimeout=10 | -F | /tmp/rig/ssh_config | -q | \
         /opt/hangar/hangar-node-x86_64 | me@box:.local/share/hangar-node/bin/hangar-node.new"
    );
}

#[test]
fn a_blank_config_file_is_no_config_file() {
    let cmd = commands::serve(&node(Some("  ")));
    assert!(!cmd.args.contains(&"-F".to_string()));
}

#[test]
fn the_upload_lands_beside_the_binary_and_is_renamed_over_it() {
    let n = node(None);
    assert_eq!(
        line(&commands::make_bin_dir(&n)),
        "ssh | -o | BatchMode=yes | -o | ConnectTimeout=10 | me@box | mkdir -p ~/.local/share/hangar-node/bin"
    );
    assert_eq!(
        commands::put_in_place(&n).args.last().unwrap(),
        "chmod +x ~/.local/share/hangar-node/bin/hangar-node.new && \
         mv -f ~/.local/share/hangar-node/bin/hangar-node.new ~/.local/share/hangar-node/bin/hangar-node"
    );
}

#[test]
fn node_json_is_read_with_cat() {
    assert_eq!(
        commands::read_node_json(&node(None)).args.last().unwrap(),
        "cat ~/.local/state/hangar-node/node.json"
    );
}

#[test]
fn the_tunnel_forwards_loopback_to_loopback_and_runs_no_command() {
    assert_eq!(
        line(&commands::tunnel(&node(Some("cfg")), 21085, 40123)),
        "ssh | -o | BatchMode=yes | -o | ConnectTimeout=10 | -F | cfg | \
         -o | ExitOnForwardFailure=yes | -o | ServerAliveInterval=15 | -o | ServerAliveCountMax=3 | \
         -N | -L | 127.0.0.1:21085:127.0.0.1:40123 | me@box"
    );
}

#[test]
fn the_target_is_trimmed() {
    let mut n = node(None);
    n.ssh_target = "  me@box ".into();
    assert_eq!(commands::serve(&n).args[4], "me@box");
}

// ── what is read back ─────────────────────────────────────────────────

#[test]
fn the_probe_reads_the_architecture_and_the_version() {
    assert_eq!(
        parse_probe("x86_64\n0.4.2 x86_64-unknown-linux-gnu\n"),
        Ok(Probe {
            arch: "x86_64".into(),
            installed: Some("0.4.2".into())
        })
    );
    assert_eq!(
        parse_probe("aarch64\n"),
        Ok(Probe {
            arch: "aarch64".into(),
            installed: None
        })
    );
    assert!(parse_probe("\n  \n").is_err());
}

#[test]
fn node_json_is_read_and_checked() {
    let json = parse_node_json(r#"{"version":"0.4.2","port":40123,"token":"abc","pid":77}"#);
    assert_eq!(
        json,
        Ok(NodeJson {
            version: "0.4.2".into(),
            port: 40123,
            token: "abc".into(),
            pid: Some(77)
        })
    );
    // No pid is still a node.json; no port or token is not.
    assert!(parse_node_json(r#"{"version":"1","port":1,"token":"t"}"#).is_ok());
    assert!(parse_node_json(r#"{"version":"1","port":0,"token":"t"}"#).is_err());
    assert!(parse_node_json(r#"{"version":"1","port":5,"token":""}"#).is_err());
    assert!(parse_node_json(r#"{"version":"1","token":"t"}"#).is_err());
    assert!(parse_node_json("cat: no such file").is_err());
}

#[test]
fn the_version_decides_install_update_or_keep() {
    assert_eq!(decide(None, "0.5.0"), Decision::Install);
    assert_eq!(
        decide(Some("0.4.2"), "0.5.0"),
        Decision::Update {
            from: "0.4.2".into()
        }
    );
    assert_eq!(decide(Some("0.5.0"), "0.5.0"), Decision::Keep);
}

#[test]
fn the_architectures_go_by_their_build_names() {
    assert_eq!(normalize_arch("x86_64"), Some("x86_64"));
    assert_eq!(normalize_arch("aarch64"), Some("aarch64"));
    assert_eq!(normalize_arch("arm64"), Some("aarch64"));
    assert_eq!(normalize_arch("armv7l"), None);
}

#[test]
fn the_binary_comes_from_the_override_then_beside_the_executable() {
    let dir = tempfile::tempdir().unwrap();
    let beside = dir.path().join("hangar-node-x86_64");
    std::fs::write(&beside, b"bin").unwrap();
    let source = BinarySource {
        override_path: None,
        exe_dir: Some(dir.path().to_path_buf()),
    };
    assert_eq!(source.binary_for("x86_64"), Ok(beside.clone()));
    assert!(matches!(
        source.binary_for("aarch64"),
        Err(Failure::NoBinary { ref file, .. }) if file == "hangar-node-aarch64"
    ));
    assert_eq!(
        source.binary_for("mips"),
        Err(Failure::UnsupportedArch("mips".into()))
    );

    let dev = dir.path().join("dev-build");
    std::fs::write(&dev, b"bin").unwrap();
    let with_override = BinarySource {
        override_path: Some(dev.clone()),
        ..source.clone()
    };
    assert_eq!(with_override.binary_for("aarch64"), Ok(dev));
    let missing = BinarySource {
        override_path: Some(PathBuf::from("/nowhere/hangar-node")),
        ..source
    };
    assert!(matches!(
        missing.binary_for("x86_64"),
        Err(Failure::NoBinary { .. })
    ));
}

#[test]
fn a_target_is_one_argument_that_is_not_an_option() {
    assert_eq!(target_problem("me@box"), None);
    assert_eq!(target_problem("box"), None);
    assert_eq!(target_problem(""), Some(TargetProblem::Empty));
    assert_eq!(target_problem("  "), Some(TargetProblem::Empty));
    assert_eq!(
        target_problem("-oProxyCommand=sh"),
        Some(TargetProblem::Invalid)
    );
    assert_eq!(target_problem("me@box ls"), Some(TargetProblem::Invalid));
}

#[test]
fn the_add_form_makes_a_node_only_with_a_usable_target() {
    let form = AddForm {
        label: " GPU ".into(),
        target: " me@gpu ".into(),
        config: "".into(),
        touched: true,
    };
    assert_eq!(
        node_from_form(&form, "id".into()),
        Some(NodeConfig {
            id: "id".into(),
            label: "GPU".into(),
            ssh_target: "me@gpu".into(),
            ssh_config: None,
            extra: Default::default(),
        })
    );
    let with_config = AddForm {
        config: " ~/rig/ssh_config ".into(),
        ..form.clone()
    };
    assert_eq!(
        node_from_form(&with_config, "id".into()).and_then(|n| n.ssh_config),
        Some("~/rig/ssh_config".into())
    );
    let bad = AddForm {
        target: "-x".into(),
        ..form
    };
    assert_eq!(node_from_form(&bad, "id".into()), None);
}

#[test]
fn an_http_answer_gives_its_body_plain_or_chunked() {
    assert_eq!(
        parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}"),
        Ok("{}".into())
    );
    assert_eq!(
        parse_response(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\n{\"a\r\n4\r\n\":1}\r\n0\r\n\r\n"
        ),
        Ok("{\"a\":1}".into())
    );
    assert_eq!(
        parse_response(b"HTTP/1.1 401 Unauthorized\r\n\r\n"),
        Err("HTTP 401".into())
    );
    assert!(parse_response(b"garbage").is_err());
}

#[test]
fn http_get_sends_the_token_and_reads_the_answer() {
    use std::io::{Read, Write};
    // A port of this worker's range when there is one, else any.
    let port = std::env::var("HANGAR_PORT_BASE")
        .ok()
        .and_then(|base| base.parse::<u16>().ok())
        .map(|base| base + 17)
        .unwrap_or(0);
    let listener = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        let mut buf = [0u8; 512];
        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = stream.read(&mut buf).unwrap();
            request.extend_from_slice(&buf[..n]);
        }
        let body =
            r#"{"version":"1","target":"x","hostname":"h","home":"/h","claude":true,"git":false}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        String::from_utf8(request).unwrap()
    });
    let body = super::runner::http_get(port, "/node/info", "tok", Duration::from_secs(5)).unwrap();
    let request = server.join().unwrap();
    assert!(request.starts_with("GET /node/info HTTP/1.1\r\n"));
    assert!(request.contains("Authorization: Bearer tok\r\n"));
    let info: NodeInfo = serde_json::from_str(&body).unwrap();
    assert!(info.claude && !info.git);
    assert_eq!(info.hostname, "h");
}

// ── settings ──────────────────────────────────────────────────────────

#[test]
fn nodes_are_read_leniently_and_written_only_when_there_are_some() {
    let state = read_state(serde_json::json!({"settings": {"nodes": [
        {"id": "a", "label": "A", "sshTarget": "me@a", "sshConfig": "/c"},
        {"id": "b", "sshTarget": 12, "label": "B"},
        "not a node",
    ]}}));
    let nodes = &state.settings.nodes;
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].ssh_config.as_deref(), Some("/c"));
    assert_eq!(nodes[0].ssh_target, "me@a");
    // A bad field costs that field, not the node.
    assert_eq!(nodes[1].label, "B");
    assert_eq!(nodes[1].ssh_target, "");

    let written = serde_json::to_value(&state.settings).unwrap();
    assert_eq!(
        written["nodes"][0],
        serde_json::json!({"id": "a", "label": "A", "sshTarget": "me@a", "sshConfig": "/c"})
    );
    let none = serde_json::to_value(Settings::default()).unwrap();
    assert!(none.get("nodes").is_none());
}

#[test]
fn a_reset_of_the_settings_keeps_the_nodes() {
    use crate::state::{Patch, StoreAction};
    let mut m = Model::default();
    m.apply(StoreAction::UpdateSettings(Patch::new(
        |s: &mut Settings| s.nodes.push(node(None)),
    )));
    m.apply(StoreAction::ResetSettings);
    assert_eq!(m.settings().nodes, vec![node(None)]);
}

// ── status and client ─────────────────────────────────────────────────

#[test]
fn the_client_is_there_only_while_connected() {
    let mut m = Model::default();
    assert_eq!(super::client(&m, "n1"), None);
    m.live
        .nodes
        .statuses
        .insert("n1".into(), NodeStatus::Connecting(Step::Tunnel));
    assert_eq!(super::client(&m, "n1"), None);
    m.live.nodes.statuses.insert(
        "n1".into(),
        NodeStatus::Connected {
            info: NodeInfo::default(),
            local_port: 21090,
            token: "tok".into(),
        },
    );
    assert_eq!(
        super::client(&m, "n1"),
        Some(NodeClient {
            base_url: "http://127.0.0.1:21090".into(),
            token: "tok".into()
        })
    );
}

#[tokio::test]
async fn a_status_from_an_older_connection_is_dropped() {
    let mut st = State::default();
    st.conns.insert(
        "n1".into(),
        super::Conn {
            generation: 2,
            task: tokio::spawn(async {}),
            tunnel: Arc::new(Mutex::new(None)),
            events: None,
        },
    );
    let mut live = Live::default();
    apply_status(
        &mut st,
        &mut live,
        "n1".into(),
        1,
        NodeStatus::Connecting(Step::Probe),
    );
    assert_eq!(live.status("n1"), NodeStatus::Disconnected);
    apply_status(
        &mut st,
        &mut live,
        "n1".into(),
        2,
        NodeStatus::Connecting(Step::Start),
    );
    assert_eq!(live.status("n1"), NodeStatus::Connecting(Step::Start));
    apply_status(
        &mut st,
        &mut live,
        "other".into(),
        2,
        NodeStatus::Connecting(Step::Start),
    );
    assert_eq!(live.status("other"), NodeStatus::Disconnected);
}

// ── the state machine, against a fake machine ─────────────────────────

/// A machine: the answers to the commands, in order, and a tunnel the test
/// can end.
#[derive(Clone, Default)]
struct Fake {
    answers: Arc<Mutex<VecDeque<CmdOutput>>>,
    ran: Arc<Mutex<Vec<Cmd>>>,
    tunnels: Arc<Mutex<Vec<Cmd>>>,
    /// Set to end the current tunnel.
    tunnel_dies: Arc<AtomicBool>,
    /// The tunnel cannot start.
    tunnel_fails: bool,
    /// `/node/info` answers after this many refusals.
    info_refusals: Arc<Mutex<u32>>,
}

struct FakeTunnel {
    dies: Arc<AtomicBool>,
    killed: bool,
}

impl Tunnel for FakeTunnel {
    fn exited(&mut self) -> Option<String> {
        if self.killed {
            return Some("killed".into());
        }
        self.dies
            .swap(false, Ordering::SeqCst)
            .then(|| "Connection reset".to_string())
    }
    fn kill(&mut self) {
        self.killed = true;
    }
}

fn ok(stdout: &str) -> CmdOutput {
    CmdOutput {
        success: true,
        stdout: stdout.into(),
        stderr: String::new(),
    }
}

fn failed(stderr: &str) -> CmdOutput {
    CmdOutput {
        success: false,
        stdout: String::new(),
        stderr: stderr.into(),
    }
}

impl Fake {
    fn answering(answers: Vec<CmdOutput>) -> Fake {
        Fake {
            answers: Arc::new(Mutex::new(answers.into())),
            ..Fake::default()
        }
    }

    fn ran(&self) -> Vec<String> {
        locked(&self.ran)
            .iter()
            .map(|cmd| format!("{} {}", cmd.program, cmd.args.last().unwrap()))
            .collect()
    }
}

impl Runner for Fake {
    type Tunnel = FakeTunnel;

    async fn run(&self, cmd: Cmd) -> Result<CmdOutput, String> {
        locked(&self.ran).push(cmd);
        Ok(locked(&self.answers)
            .pop_front()
            .unwrap_or_else(|| failed("no more answers")))
    }

    fn spawn_tunnel(&self, cmd: Cmd) -> Result<FakeTunnel, String> {
        if self.tunnel_fails {
            return Err("ssh: not found".into());
        }
        locked(&self.tunnels).push(cmd);
        Ok(FakeTunnel {
            dies: self.tunnel_dies.clone(),
            killed: false,
        })
    }

    fn free_port(&self) -> Result<u16, String> {
        Ok(21099)
    }

    async fn node_info(&self, port: u16, token: String) -> Result<NodeInfo, String> {
        assert_eq!((port, token.as_str()), (21099, "tok"));
        let mut refusals = locked(&self.info_refusals);
        if *refusals > 0 {
            *refusals -= 1;
            return Err("Connection refused".into());
        }
        Ok(info())
    }
}

fn info() -> NodeInfo {
    NodeInfo {
        version: "9.9.9".into(),
        target: "x86_64-unknown-linux-gnu".into(),
        hostname: "box".into(),
        home: "/home/me".into(),
        claude: true,
        git: true,
    }
}

const NODE_JSON: &str = r#"{"version":"9.9.9","port":40123,"token":"tok","pid":10}"#;

fn setup(binary_dir: Option<&Path>) -> Setup {
    Setup {
        node: node(None),
        source: BinarySource {
            override_path: None,
            exe_dir: binary_dir.map(Path::to_path_buf),
        },
        version: "9.9.9".into(),
        timing: Timing {
            poll: Duration::from_millis(5),
            first_backoff: Duration::from_millis(5),
            max_backoff: Duration::from_millis(20),
            handshake_tries: 5,
            handshake_wait: Duration::from_millis(1),
        },
    }
}

type Reports = Arc<Mutex<Vec<NodeStatus>>>;

fn reporter() -> (Reports, impl Fn(NodeStatus) + Send + Sync + 'static) {
    let reports: Reports = Arc::default();
    let sink = reports.clone();
    (reports, move |status| locked(&sink).push(status))
}

fn connected() -> NodeStatus {
    NodeStatus::Connected {
        info: info(),
        local_port: 21099,
        token: "tok".into(),
    }
}

#[tokio::test]
async fn a_node_with_this_version_is_started_and_tunnelled_without_an_upload() {
    let fake = Fake::answering(vec![
        ok("x86_64\n9.9.9 x86_64-unknown-linux-gnu\n"),
        ok(""),
        ok(NODE_JSON),
    ]);
    *locked(&fake.info_refusals) = 2;
    let slot: TunnelSlot<FakeTunnel> = Arc::default();
    let (reports, report) = reporter();
    let result = connect::connect_once(&fake, &setup(None), &slot, &report).await;
    assert_eq!(result, Ok((info(), 21099, "tok".into())));
    assert_eq!(
        fake.ran(),
        [
            "ssh uname -m; ~/.local/share/hangar-node/bin/hangar-node version 2>/dev/null || true",
            "ssh ~/.local/share/hangar-node/bin/hangar-node serve --detach",
            "ssh cat ~/.local/state/hangar-node/node.json",
        ]
    );
    assert_eq!(
        locked(&fake.tunnels)[0].args.last().map(String::as_str),
        Some("me@box")
    );
    assert!(
        locked(&fake.tunnels)[0]
            .args
            .contains(&"127.0.0.1:21099:127.0.0.1:40123".to_string())
    );
    assert!(locked(&slot).is_some());
    assert_eq!(
        *locked(&reports),
        [
            NodeStatus::Connecting(Step::Probe),
            NodeStatus::Connecting(Step::Start),
            NodeStatus::Connecting(Step::Config),
            NodeStatus::Connecting(Step::Tunnel),
            NodeStatus::Connecting(Step::Handshake),
        ]
    );
}

#[tokio::test]
async fn a_node_without_hangar_node_gets_the_binary_for_its_architecture() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("hangar-node-aarch64"), b"bin").unwrap();
    let fake = Fake::answering(vec![
        ok("aarch64\n"),
        ok(""),
        ok(""),
        ok(""),
        ok(""),
        ok(NODE_JSON),
    ]);
    let slot: TunnelSlot<FakeTunnel> = Arc::default();
    let (reports, report) = reporter();
    let result = connect::connect_once(&fake, &setup(Some(dir.path())), &slot, &report).await;
    assert!(result.is_ok(), "{result:?}");
    let ran = locked(&fake.ran).clone();
    assert_eq!(
        ran[1].args.last().unwrap(),
        "mkdir -p ~/.local/share/hangar-node/bin"
    );
    assert_eq!(ran[2].program, "scp");
    assert_eq!(
        ran[2].args[ran[2].args.len() - 2],
        dir.path().join("hangar-node-aarch64").display().to_string()
    );
    assert!(ran[3].args.last().unwrap().starts_with("chmod +x"));
    assert!(ran[4].args.last().unwrap().ends_with("serve --detach"));
    assert!(locked(&reports).contains(&NodeStatus::Connecting(Step::Install)));
}

#[tokio::test]
async fn an_update_stops_the_older_daemon_before_starting_the_new_one() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("hangar-node-x86_64"), b"bin").unwrap();
    let fake = Fake::answering(vec![
        ok("x86_64\n9.0.0 x86_64-unknown-linux-gnu\n"),
        ok(""),
        ok(""),
        ok(""),
        ok(r#"{"version":"9.0.0","port":40000,"token":"old","pid":4242}"#),
        ok(""),
        ok(""),
        ok(NODE_JSON),
    ]);
    let slot: TunnelSlot<FakeTunnel> = Arc::default();
    let (_, report) = reporter();
    let result = connect::connect_once(&fake, &setup(Some(dir.path())), &slot, &report).await;
    assert!(result.is_ok(), "{result:?}");
    let ran = fake.ran();
    assert_eq!(ran[5], "ssh kill 4242 2>/dev/null || true");
    assert!(ran[6].ends_with("serve --detach"));
}

#[tokio::test]
async fn no_binary_for_the_machine_is_a_clear_failure_before_anything_is_sent() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::answering(vec![ok("x86_64\n")]);
    let slot: TunnelSlot<FakeTunnel> = Arc::default();
    let (_, report) = reporter();
    let result = connect::connect_once(&fake, &setup(Some(dir.path())), &slot, &report).await;
    assert!(matches!(
        result,
        Err(Failure::NoBinary { ref arch, ref file, .. }) if arch == "x86_64" && file == "hangar-node-x86_64"
    ));
    assert_eq!(locked(&fake.ran).len(), 1);
}

#[tokio::test]
async fn an_ssh_failure_names_its_step_and_what_ssh_said() {
    let fake = Fake::answering(vec![failed(
        "Warning: x\nme@box: Permission denied (publickey).\n",
    )]);
    let slot: TunnelSlot<FakeTunnel> = Arc::default();
    let (reports, report) = reporter();
    connect::run(fake, setup(None), slot, report).await;
    // Never connected: it stops and waits for the person.
    assert_eq!(
        locked(&reports).last(),
        Some(&NodeStatus::Unreachable {
            failure: Failure::Ssh {
                step: Step::Probe,
                detail: "me@box: Permission denied (publickey).".into()
            },
            retry_in: None
        })
    );
}

#[tokio::test]
async fn a_node_json_that_does_not_read_fails_the_connection() {
    let fake = Fake::answering(vec![ok("x86_64\n9.9.9 t\n"), ok(""), ok("{}")]);
    let slot: TunnelSlot<FakeTunnel> = Arc::default();
    let (_, report) = reporter();
    let result = connect::connect_once(&fake, &setup(None), &slot, &report).await;
    assert!(matches!(result, Err(Failure::NodeJson(_))), "{result:?}");
}

#[tokio::test]
async fn a_tunnel_that_cannot_start_is_a_local_failure() {
    let mut fake = Fake::answering(vec![ok("x86_64\n9.9.9 t\n"), ok(""), ok(NODE_JSON)]);
    fake.tunnel_fails = true;
    let slot: TunnelSlot<FakeTunnel> = Arc::default();
    let (_, report) = reporter();
    let result = connect::connect_once(&fake, &setup(None), &slot, &report).await;
    assert_eq!(result, Err(Failure::Local("ssh: not found".into())));
}

#[tokio::test]
async fn an_api_that_never_answers_closes_the_tunnel() {
    let fake = Fake::answering(vec![ok("x86_64\n9.9.9 t\n"), ok(""), ok(NODE_JSON)]);
    *locked(&fake.info_refusals) = 100;
    let slot: TunnelSlot<FakeTunnel> = Arc::default();
    let (_, report) = reporter();
    let result = connect::connect_once(&fake, &setup(None), &slot, &report).await;
    assert_eq!(result, Err(Failure::Api("Connection refused".into())));
    assert!(locked(&slot).is_none());
}

#[tokio::test]
async fn a_tunnel_that_dies_is_opened_again_after_a_wait() {
    let once = [ok("x86_64\n9.9.9 t\n"), ok(""), ok(NODE_JSON)];
    let fake = Fake::answering(once.iter().chain(once.iter()).cloned().collect());
    let slot: TunnelSlot<FakeTunnel> = Arc::default();
    let (reports, report) = reporter();
    let task = tokio::spawn(connect::run(
        fake.clone(),
        setup(None),
        slot.clone(),
        report,
    ));

    let wait_for = |count: usize| {
        let reports = reports.clone();
        async move {
            for _ in 0..400 {
                let n = locked(&reports)
                    .iter()
                    .filter(|s| **s == connected())
                    .count();
                if n >= count {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            panic!("not connected {count} times: {:?}", locked(&reports));
        }
    };
    wait_for(1).await;
    fake.tunnel_dies.store(true, Ordering::SeqCst);
    wait_for(2).await;

    let seen = locked(&reports).clone();
    let dropped = seen
        .iter()
        .position(|s| matches!(s, NodeStatus::Unreachable { .. }))
        .expect("the drop is reported");
    assert_eq!(
        seen[dropped],
        NodeStatus::Unreachable {
            failure: Failure::Tunnel("Connection reset".into()),
            retry_in: Some(1)
        }
    );
    assert_eq!(seen[dropped + 1], NodeStatus::Connecting(Step::Probe));
    assert_eq!(locked(&fake.tunnels).len(), 2);

    // Quitting takes the tunnel out of its slot: the task ends on its own.
    if let Some(mut tunnel) = locked(&slot).take() {
        tunnel.kill();
    }
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("the connection ends once its tunnel is killed")
        .unwrap();
}

/// Against the Docker rig (`scripts/node-test/up.sh`), with a fake
/// `hangar-node` uploaded as the binary: a shell script that writes a
/// node.json and serves `/node/info` with Python. Run with
/// `cargo test -p hangar-app nodes::tests::rig -- --ignored` once the rig is
/// up.
#[tokio::test]
#[ignore = "needs the Docker rig of scripts/node-test/ up"]
async fn rig_connects_to_the_test_container_with_a_fake_hangar_node() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config = root.join("scripts/node-test/ssh_config");
    assert!(
        config.is_file(),
        "{} is missing: start the rig first",
        config.display()
    );
    let dir = tempfile::tempdir().unwrap();
    let fake_node = dir.path().join("hangar-node");
    std::fs::write(&fake_node, FAKE_HANGAR_NODE).unwrap();
    let node = NodeConfig {
        id: "rig".into(),
        label: "rig".into(),
        ssh_target: "hangar-node-test".into(),
        ssh_config: Some(config.display().to_string()),
        extra: Default::default(),
    };
    // A version no install has, so the script is uploaded every run.
    let setup = Setup {
        node: node.clone(),
        source: BinarySource {
            override_path: Some(fake_node),
            exe_dir: None,
        },
        version: format!("rig-{}", std::process::id()),
        timing: Timing::default(),
    };
    let slot: TunnelSlot<super::runner::SystemTunnel> = Arc::default();
    let (reports, report) = reporter();
    let result = connect::connect_once(&super::runner::System, &setup, &slot, &report).await;
    let (info, port, token) = result.unwrap_or_else(|f| panic!("{f:?}: {:?}", locked(&reports)));
    assert_eq!(info.version, "fake");
    assert!(!info.hostname.is_empty());
    assert!(!token.is_empty());
    let body = super::runner::http_get(port, "/node/info", "wrong", Duration::from_secs(5));
    assert_eq!(body, Err("HTTP 401".into()));
    if let Some(mut tunnel) = locked(&slot).take() {
        tunnel.kill();
    }
}

/// The fake `hangar-node` of the rig test: `version` prints a version that
/// is never the desktop's, `serve --detach` starts a Python server on a free
/// port that answers `/node/info` to the right token, and writes node.json.
const FAKE_HANGAR_NODE: &str = r#"#!/bin/sh
set -e
case "$1" in
version) echo "fake x86_64-unknown-linux-gnu" ;;
serve)
  state="$HOME/.local/state/hangar-node"
  mkdir -p "$state"
  token=$(head -c 16 /dev/urandom | od -An -tx1 | tr -d ' \n')
  port=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')
  cat > "$state/server.py" <<EOF
import http.server, json, os, shutil, socket
class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.headers.get("Authorization") != "Bearer $token":
            self.send_response(401); self.end_headers(); return
        body = json.dumps({"version": "fake", "target": "x86_64-unknown-linux-gnu",
            "hostname": socket.gethostname(), "home": os.path.expanduser("~"),
            "claude": shutil.which("claude") is not None,
            "git": shutil.which("git") is not None}).encode()
        self.send_response(200)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers(); self.wfile.write(body)
http.server.HTTPServer(("127.0.0.1", $port), H).serve_forever()
EOF
  nohup python3 "$state/server.py" >/dev/null 2>&1 &
  echo "{\"version\":\"fake\",\"port\":$port,\"token\":\"$token\",\"pid\":$!}" > "$state/node.json"
  sleep 0.3
  ;;
esac
"#;
