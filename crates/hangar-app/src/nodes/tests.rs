//! The nodes' connection layer, without SSH: the command lines of each step,
//! what is read back, the version decision, and the state machine run
//! against a fake runner that answers like a machine would.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::commands::{self, Cmd, Mux};
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

const OWN: &str = "/run/user/1000/hangar-ssh/42-ab";

fn own() -> Mux {
    Mux::Own(OWN.into())
}

#[test]
fn every_ssh_step_is_batch_mode_with_a_bounded_wait() {
    let n = node(None);
    for mux in [Mux::Off, Mux::Theirs, own()] {
        for cmd in [
            commands::probe(&n, &mux, "1.0.0"),
            commands::install_and_start(&n, &mux, Some(42)),
            commands::tunnel(&n, &mux, 21080, 7000),
            commands::link(&n, OWN, "1.0.0"),
            commands::forward(&n, OWN, 21080, 7000),
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
}

#[test]
fn the_probe_asks_everything_in_one_command() {
    assert_eq!(
        line(&commands::probe(&node(None), &Mux::Off, "1.2.3")),
        "ssh | -o | BatchMode=yes | -o | ConnectTimeout=10 | me@box | \
         mkdir -p ~/.local/share/hangar-node/bin; uname -m; echo @@hangar; \
         ~/.local/share/hangar-node/bin/hangar-node version 2>/dev/null; echo @@hangar; \
         ~/.local/share/hangar-node/bin/hangar-node serve --detach --if-version 1.2.3 2>&1 \
         && echo @@hangar-started; echo @@hangar; cat ~/.local/state/hangar-node/node.json 2>/dev/null"
    );
}

#[test]
fn a_config_file_is_passed_as_dash_f_before_the_target() {
    let n = node(Some("/tmp/rig/ssh_config"));
    let cmd = commands::ssh(&n, &Mux::Off, "true");
    assert_eq!(
        line(&cmd),
        "ssh | -o | BatchMode=yes | -o | ConnectTimeout=10 | -F | /tmp/rig/ssh_config | me@box | true"
    );
    let upload = commands::upload(&n, &Mux::Off, Path::new("/opt/hangar/hangar-node-x86_64"));
    assert_eq!(
        line(&upload),
        "scp | -o | BatchMode=yes | -o | ConnectTimeout=10 | -F | /tmp/rig/ssh_config | -q | \
         /opt/hangar/hangar-node-x86_64 | me@box:.local/share/hangar-node/bin/hangar-node.new"
    );
    assert_eq!(
        line(&commands::config_query(&n)),
        "ssh | -G | -o | BatchMode=yes | -o | ConnectTimeout=10 | -F | /tmp/rig/ssh_config | me@box"
    );
}

#[test]
fn a_blank_config_file_is_no_config_file() {
    let cmd = commands::ssh(&node(Some("  ")), &Mux::Off, "true");
    assert!(!cmd.args.contains(&"-F".to_string()));
}

#[test]
fn the_install_stops_an_older_daemon_then_puts_the_upload_in_place_and_starts() {
    let n = node(None);
    assert_eq!(
        commands::install_and_start(&n, &Mux::Off, Some(4242))
            .args
            .last()
            .unwrap(),
        "kill 4242 2>/dev/null; chmod +x ~/.local/share/hangar-node/bin/hangar-node.new && \
         mv -f ~/.local/share/hangar-node/bin/hangar-node.new ~/.local/share/hangar-node/bin/hangar-node && \
         ~/.local/share/hangar-node/bin/hangar-node serve --detach && cat ~/.local/state/hangar-node/node.json"
    );
    assert!(
        commands::install_and_start(&n, &Mux::Off, None)
            .args
            .last()
            .unwrap()
            .starts_with("chmod +x")
    );
}

#[test]
fn the_tunnel_forwards_loopback_to_loopback_and_runs_no_command() {
    assert_eq!(
        line(&commands::tunnel(
            &node(Some("cfg")),
            &Mux::Off,
            21085,
            40123
        )),
        "ssh | -o | BatchMode=yes | -o | ConnectTimeout=10 | -F | cfg | \
         -o | ExitOnForwardFailure=yes | -o | ServerAliveInterval=15 | -o | ServerAliveCountMax=3 | \
         -o | IPQoS=af21 | -o | Compression=no | -N | -L | 127.0.0.1:21085:127.0.0.1:40123 | me@box"
    );
}

#[test]
fn the_target_is_trimmed() {
    let mut n = node(None);
    n.ssh_target = "  me@box ".into();
    assert_eq!(commands::ssh(&n, &Mux::Off, "true").args[4], "me@box");
}

#[test]
fn unix_shares_the_connection_and_windows_does_not() {
    let config = "user me\nhostname box\ncontrolmaster false\ncontrolpersist no\n";
    assert_eq!(
        commands::mux_for(false, Some(config), Some(OWN.into())),
        own()
    );
    // OpenSSH for Windows has no ControlMaster: the commands as they were.
    assert_eq!(
        commands::mux_for(true, Some(config), Some(OWN.into())),
        Mux::Off
    );
    // An ssh that could not say (too old for -G, a broken config), or no
    // folder for the socket: nothing shared.
    assert_eq!(commands::mux_for(false, None, Some(OWN.into())), Mux::Off);
    assert_eq!(commands::mux_for(false, Some(config), None), Mux::Off);
    assert_eq!(
        commands::mux_for(false, Some("controlpath none\n"), Some(OWN.into())),
        own()
    );
}

#[test]
fn a_person_s_control_path_is_left_to_them() {
    let config = "controlmaster auto\ncontrolpath /home/me/.ssh/cm-%C\ncontrolpersist 600\n";
    let mux = commands::mux_for(false, Some(config), Some(OWN.into()));
    assert_eq!(mux, Mux::Theirs);
    let n = node(None);
    // Not overridden: none of the app's sharing options on their commands.
    for cmd in [
        commands::probe(&n, &mux, "1.0.0"),
        commands::upload(&n, &mux, Path::new("/bin/x")),
        commands::install_and_start(&n, &mux, None),
    ] {
        assert!(
            !cmd.args.iter().any(|arg| arg.starts_with("Control")),
            "{}",
            line(&cmd)
        );
    }
    // The tunnel alone keeps off their master: a forward asked through one
    // would live in it, and the ssh asking would end at once.
    let tunnel = commands::tunnel(&n, &mux, 21085, 40123);
    assert!(line(&tunnel).contains("-o | ControlMaster=no | -o | ControlPath=none"));
    // Off: nothing at all, as before.
    let off = commands::tunnel(&n, &Mux::Off, 21085, 40123);
    assert!(!off.args.iter().any(|arg| arg.starts_with("Control")));
}

#[test]
fn with_sharing_the_tunnel_is_the_master_and_the_rest_its_sessions() {
    let n = node(None);
    let link = commands::link(&n, OWN, "1.2.3");
    let words = line(&link);
    assert!(
        words.contains(&format!(
            "-o | ControlMaster=yes | -o | ControlPath={OWN} | -o | ControlPersist=no | \
             -o | ExitOnForwardFailure=yes | -o | ServerAliveInterval=15 | -o | ServerAliveCountMax=3 | \
             -o | IPQoS=af21 | -o | Compression=no | me@box"
        )),
        "{words}"
    );
    let session = link.args.last().unwrap();
    assert!(session.starts_with(&commands::probe_script("1.2.3")));
    assert!(
        session.ends_with("; echo; echo @@hangar-end; exec cat"),
        "{session}"
    );

    assert_eq!(
        line(&commands::forward(&n, OWN, 21085, 40123)),
        format!(
            "ssh | -o | BatchMode=yes | -o | ConnectTimeout=10 | -o | ControlPath={OWN} | -O | forward | \
             -L | 127.0.0.1:21085:127.0.0.1:40123 | me@box"
        )
    );
    // The install's commands ride the master, never becoming one.
    for cmd in [
        commands::upload(&n, &own(), Path::new("/bin/x")),
        commands::install_and_start(&n, &own(), None),
    ] {
        assert!(
            line(&cmd).contains(&format!("-o | ControlMaster=no | -o | ControlPath={OWN}")),
            "{}",
            line(&cmd)
        );
    }
}

#[test]
fn the_control_socket_is_this_process_s_and_short() {
    let dir = Path::new("/run/user/1000/hangar-ssh");
    let path = connect::control_path(dir, "5f0c3f5e-9c7a-4a43-a2b5-3e8f9f1e2d11");
    assert_eq!(path.parent(), Some(dir));
    let name = path.file_name().unwrap().to_str().unwrap();
    assert!(
        name.starts_with(&format!("{}-", std::process::id())),
        "{name}"
    );
    assert!(name.len() <= 27, "{name}");
    assert_ne!(path, connect::control_path(dir, "another-node"));
}

#[test]
fn the_probe_reads_each_answer() {
    let json = r#"{"version":"0.4.2","port":40123,"token":"abc","pid":77}"#;
    assert_eq!(
        parse_probe(&format!(
            "x86_64\n@@hangar\n0.4.2 x86_64-unknown-linux-gnu\n@@hangar\n@@hangar-started\n@@hangar\n{json}\n"
        )),
        Ok(Probe {
            arch: "x86_64".into(),
            installed: Some("0.4.2".into()),
            started: true,
            said: String::new(),
            node_json: json.into(),
        })
    );
    // Nothing installed: the start failed, and says so; no node.json.
    assert_eq!(
        parse_probe(
            "aarch64\n@@hangar\n@@hangar\nsh: 1: hangar-node: not found\n@@hangar\n@@hangar-end\nlater\n"
        ),
        Ok(Probe {
            arch: "aarch64".into(),
            installed: None,
            started: false,
            said: "sh: 1: hangar-node: not found".into(),
            node_json: String::new(),
        })
    );
    assert!(parse_probe("\n  \n").is_err());
    // node.json has no newline of its own: the end mark may follow it.
    assert_eq!(
        parse_probe(
            "x86_64\n@@hangar\n1 t\n@@hangar\n@@hangar-started\n@@hangar\n{\"a\":1}@@hangar-end\n"
        )
        .map(|p| p.node_json),
        Ok("{\"a\":1}".into())
    );
    // What a shell's startup files print comes first.
    assert_eq!(
        parse_probe("Welcome to box\nx86_64\n@@hangar\n").map(|p| p.arch),
        Ok("x86_64".into())
    );
}

#[test]
fn the_daemon_to_stop_is_an_older_one() {
    let old = r#"{"version":"9.0.0","port":40000,"token":"old","pid":4242}"#;
    assert_eq!(connect::older_daemon(old, "9.9.9"), Some(4242));
    assert_eq!(connect::older_daemon(old, "9.0.0"), None);
    assert_eq!(connect::older_daemon("", "9.9.9"), None);
    assert_eq!(
        connect::older_daemon(r#"{"version":"1","port":5,"token":"t"}"#, "9"),
        None
    );
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
fn the_binary_is_found_in_a_package_s_resources() {
    let dir = tempfile::tempdir().unwrap();
    // macOS: Hangar.AI.app/Contents/{MacOS,Resources}.
    let contents = dir.path().join("Contents");
    std::fs::create_dir_all(contents.join("MacOS")).unwrap();
    std::fs::create_dir_all(contents.join("Resources")).unwrap();
    let in_resources = contents.join("Resources/hangar-node-aarch64");
    std::fs::write(&in_resources, b"bin").unwrap();
    let mac = BinarySource {
        override_path: None,
        exe_dir: Some(contents.join("MacOS")),
    };
    let found = mac.binary_for("arm64").unwrap();
    assert_eq!(
        found.canonicalize().unwrap(),
        in_resources.canonicalize().unwrap()
    );
    // Linux packages: usr/bin beside usr/lib/hangar-ai.
    let usr = dir.path().join("usr");
    std::fs::create_dir_all(usr.join("bin")).unwrap();
    std::fs::create_dir_all(usr.join("lib/hangar-ai")).unwrap();
    let in_lib = usr.join("lib/hangar-ai/hangar-node-x86_64");
    std::fs::write(&in_lib, b"bin").unwrap();
    let linux = BinarySource {
        override_path: None,
        exe_dir: Some(usr.join("bin")),
    };
    let found = linux.binary_for("x86_64").unwrap();
    assert_eq!(
        found.canonicalize().unwrap(),
        in_lib.canonicalize().unwrap()
    );
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

/// A machine: the answers to the commands, in order (the link's session
/// takes one too), and a tunnel the test can end.
#[derive(Clone, Default)]
struct Fake {
    answers: Arc<Mutex<VecDeque<CmdOutput>>>,
    ran: Arc<Mutex<Vec<Cmd>>>,
    tunnels: Arc<Mutex<Vec<Cmd>>>,
    links: Arc<Mutex<Vec<Cmd>>>,
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

    fn next(&self) -> CmdOutput {
        locked(&self.answers)
            .pop_front()
            .unwrap_or_else(|| failed("no more answers"))
    }

    fn tunnel(&self) -> FakeTunnel {
        FakeTunnel {
            dies: self.tunnel_dies.clone(),
            killed: false,
        }
    }
}

impl Runner for Fake {
    type Tunnel = FakeTunnel;

    async fn run(&self, cmd: Cmd) -> Result<CmdOutput, String> {
        locked(&self.ran).push(cmd);
        Ok(self.next())
    }

    fn spawn_tunnel(&self, cmd: Cmd) -> Result<FakeTunnel, String> {
        if self.tunnel_fails {
            return Err("ssh: not found".into());
        }
        locked(&self.tunnels).push(cmd);
        Ok(self.tunnel())
    }

    async fn open_link(
        &self,
        cmd: Cmd,
        _control_path: PathBuf,
    ) -> Result<(FakeTunnel, CmdOutput), String> {
        if self.tunnel_fails {
            return Err("ssh: not found".into());
        }
        locked(&self.links).push(cmd);
        Ok((self.tunnel(), self.next()))
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

/// What the probe prints: the architecture, the version installed (if
/// any), whether its daemon started, node.json.
fn probed(arch: &str, installed: Option<&str>, started: bool, json: &str) -> CmdOutput {
    let version = installed.map_or(String::new(), |v| format!("{v} x86_64-unknown-linux-gnu\n"));
    let start = if started {
        "@@hangar-started\n"
    } else {
        "hangar-node: this is another version: not started\n"
    };
    ok(&format!(
        "{arch}\n@@hangar\n{version}@@hangar\n{start}@@hangar\n{json}\n"
    ))
}

fn keep() -> CmdOutput {
    probed("x86_64", Some("9.9.9"), true, NODE_JSON)
}

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
        control_dir: None,
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

#[test]
fn the_install_is_skipped_only_for_this_version_started() {
    // This version there: nothing uploaded, the probe started it.
    assert_eq!(decide(Some("9.9.9"), "9.9.9"), Decision::Keep);
    let probe = parse_probe(&keep().stdout).unwrap();
    assert!(probe.started);
    // Another version, or none: the upload, whatever the probe said.
    assert!(matches!(
        decide(Some("9.0.0"), "9.9.9"),
        Decision::Update { .. }
    ));
    assert_eq!(decide(None, "9.9.9"), Decision::Install);
}

#[tokio::test]
async fn a_node_with_this_version_is_probed_started_and_tunnelled_in_one_command() {
    let fake = Fake::answering(vec![keep()]);
    *locked(&fake.info_refusals) = 2;
    let slot: TunnelSlot<FakeTunnel> = Arc::default();
    let (reports, report) = reporter();
    let result = connect::connect_once(&fake, &setup(None), &Mux::Off, &slot, &report).await;
    assert_eq!(result, Ok((info(), 21099, "tok".into())));
    assert_eq!(
        fake.ran(),
        [format!("ssh {}", commands::probe_script("9.9.9"))]
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
            NodeStatus::Connecting(Step::Config),
            NodeStatus::Connecting(Step::Tunnel),
            NodeStatus::Connecting(Step::Handshake),
        ]
    );
}

#[tokio::test]
async fn shared_the_probe_runs_in_the_master_and_the_forward_is_added_to_it() {
    let fake = Fake::answering(vec![keep(), ok("")]);
    let slot: TunnelSlot<FakeTunnel> = Arc::default();
    let (_, report) = reporter();
    let result = connect::connect_once(&fake, &setup(None), &own(), &slot, &report).await;
    assert_eq!(result, Ok((info(), 21099, "tok".into())));
    // One connection: the master's session probed and started the node.
    assert_eq!(locked(&fake.links).len(), 1);
    assert_eq!(
        locked(&fake.links)[0],
        commands::link(&node(None), OWN, "9.9.9")
    );
    // Then only the forward, asked of the master; no tunnel of its own.
    assert_eq!(
        *locked(&fake.ran),
        [commands::forward(&node(None), OWN, 21099, 40123)]
    );
    assert!(locked(&fake.tunnels).is_empty());
    assert!(locked(&slot).is_some());
}

#[tokio::test]
async fn shared_a_master_that_ends_before_the_probe_is_a_probe_failure() {
    let fake = Fake::answering(vec![failed(
        "ssh: connect to host box port 22: Connection refused",
    )]);
    let slot: TunnelSlot<FakeTunnel> = Arc::default();
    let (_, report) = reporter();
    let result = connect::connect_once(&fake, &setup(None), &own(), &slot, &report).await;
    assert_eq!(
        result,
        Err(Failure::Ssh {
            step: Step::Probe,
            detail: "ssh: connect to host box port 22: Connection refused".into()
        })
    );
}

#[tokio::test]
async fn a_node_without_hangar_node_gets_the_binary_for_its_architecture() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("hangar-node-aarch64"), b"bin").unwrap();
    let fake = Fake::answering(vec![
        probed("aarch64", None, false, ""),
        ok(""),
        ok(NODE_JSON),
    ]);
    let slot: TunnelSlot<FakeTunnel> = Arc::default();
    let (reports, report) = reporter();
    let result =
        connect::connect_once(&fake, &setup(Some(dir.path())), &Mux::Off, &slot, &report).await;
    assert!(result.is_ok(), "{result:?}");
    let ran = locked(&fake.ran).clone();
    assert_eq!(ran.len(), 3, "{ran:?}");
    assert_eq!(ran[1].program, "scp");
    assert_eq!(
        ran[1].args[ran[1].args.len() - 2],
        dir.path().join("hangar-node-aarch64").display().to_string()
    );
    assert_eq!(
        ran[2],
        commands::install_and_start(&node(None), &Mux::Off, None)
    );
    assert!(locked(&reports).contains(&NodeStatus::Connecting(Step::Install)));
    assert!(locked(&reports).contains(&NodeStatus::Connecting(Step::Start)));
}

#[tokio::test]
async fn an_update_stops_the_older_daemon_before_starting_the_new_one() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("hangar-node-x86_64"), b"bin").unwrap();
    let old = r#"{"version":"9.0.0","port":40000,"token":"old","pid":4242}"#;
    let fake = Fake::answering(vec![
        probed("x86_64", Some("9.0.0"), false, old),
        ok(""),
        ok(NODE_JSON),
        // The forward.
        ok(""),
    ]);
    let slot: TunnelSlot<FakeTunnel> = Arc::default();
    let (_, report) = reporter();
    let result =
        connect::connect_once(&fake, &setup(Some(dir.path())), &own(), &slot, &report).await;
    assert!(result.is_ok(), "{result:?}");
    let ran = locked(&fake.ran).clone();
    assert_eq!(ran[0].program, "scp");
    assert_eq!(
        ran[1],
        commands::install_and_start(&node(None), &own(), Some(4242))
    );
}

#[tokio::test]
async fn this_version_that_does_not_start_is_a_start_failure() {
    let fake = Fake::answering(vec![ok(
        "x86_64\n@@hangar\n9.9.9 t\n@@hangar\nhangar-node: no daemon answered within 30s\n@@hangar\n",
    )]);
    let slot: TunnelSlot<FakeTunnel> = Arc::default();
    let (_, report) = reporter();
    let result = connect::connect_once(&fake, &setup(None), &Mux::Off, &slot, &report).await;
    assert_eq!(
        result,
        Err(Failure::Ssh {
            step: Step::Start,
            detail: "hangar-node: no daemon answered within 30s".into()
        })
    );
}

#[tokio::test]
async fn no_binary_for_the_machine_is_a_clear_failure_before_anything_is_sent() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::answering(vec![probed("x86_64", None, false, "")]);
    let slot: TunnelSlot<FakeTunnel> = Arc::default();
    let (_, report) = reporter();
    let result =
        connect::connect_once(&fake, &setup(Some(dir.path())), &Mux::Off, &slot, &report).await;
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
    let fake = Fake::answering(vec![probed("x86_64", Some("9.9.9"), true, "{}")]);
    let slot: TunnelSlot<FakeTunnel> = Arc::default();
    let (_, report) = reporter();
    let result = connect::connect_once(&fake, &setup(None), &Mux::Off, &slot, &report).await;
    assert!(matches!(result, Err(Failure::NodeJson(_))), "{result:?}");
}

#[tokio::test]
async fn a_tunnel_that_cannot_start_is_a_local_failure() {
    let mut fake = Fake::answering(vec![keep()]);
    fake.tunnel_fails = true;
    let slot: TunnelSlot<FakeTunnel> = Arc::default();
    let (_, report) = reporter();
    let result = connect::connect_once(&fake, &setup(None), &Mux::Off, &slot, &report).await;
    assert_eq!(result, Err(Failure::Local("ssh: not found".into())));
}

#[tokio::test]
async fn an_api_that_never_answers_closes_the_tunnel() {
    let fake = Fake::answering(vec![keep()]);
    *locked(&fake.info_refusals) = 100;
    let slot: TunnelSlot<FakeTunnel> = Arc::default();
    let (_, report) = reporter();
    let result = connect::connect_once(&fake, &setup(None), &Mux::Off, &slot, &report).await;
    assert_eq!(result, Err(Failure::Api("Connection refused".into())));
    assert!(locked(&slot).is_none());
}

#[tokio::test]
async fn a_tunnel_that_dies_is_opened_again_after_a_wait() {
    let fake = Fake::answering(vec![keep(), keep()]);
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

// Windows' OpenSSH has no ControlMaster: there the run never asks (Mux::Off).
#[cfg(unix)]
#[tokio::test]
async fn the_run_asks_ssh_its_config_once_when_it_may_share() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::answering(vec![
        // `ssh -G`: the person's config shares already.
        ok("controlmaster auto\ncontrolpath ~/.ssh/cm-%C\n"),
        keep(),
    ]);
    let mut setup = setup(None);
    setup.control_dir = Some(dir.path().to_path_buf());
    setup.node.id = format!("theirs-{}", std::process::id());
    let slot: TunnelSlot<FakeTunnel> = Arc::default();
    let (reports, report) = reporter();
    let task = tokio::spawn(connect::run(
        fake.clone(),
        setup.clone(),
        slot.clone(),
        report,
    ));
    for _ in 0..400 {
        if locked(&reports).contains(&connected()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(locked(&reports).contains(&connected()));
    assert_eq!(locked(&fake.ran)[0], commands::config_query(&setup.node));
    assert_eq!(connect::mux_of(&setup.node.id), Mux::Theirs);
    // Theirs: a tunnel of its own, kept off their master.
    assert!(line(&locked(&fake.tunnels)[0]).contains("ControlPath=none"));
    if let Some(mut tunnel) = locked(&slot).take() {
        tunnel.kill();
    }
    let _ = tokio::time::timeout(Duration::from_secs(2), task).await;
}

/// The real link's reader, a shell standing in for ssh: what its session
/// printed up to the end mark (which may follow node.json on its line),
/// and it keeps running; killed, its socket goes. One that ends before the
/// mark says why on stderr.
#[cfg(unix)]
#[tokio::test]
async fn a_link_is_read_up_to_its_end_mark_and_held_open() {
    use super::runner::System;
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("sock");
    std::fs::write(&socket, b"").unwrap();
    let sh = |script: &str| Cmd {
        program: "sh".into(),
        args: vec!["-c".into(), script.into()],
    };
    let (mut tunnel, output) = System
        .open_link(
            sh(r#"printf 'x86_64\n{"a":1}@@hangar-end\nlater\n'; exec cat"#),
            socket.clone(),
        )
        .await
        .unwrap();
    assert!(output.success);
    assert_eq!(output.stdout, "x86_64\n{\"a\":1}");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(tunnel.exited(), None, "held open by its stdin");
    tunnel.kill();
    assert!(!socket.exists());

    let (_, output) = System
        .open_link(
            sh("echo 'Permission denied (publickey).' >&2; exit 255"),
            socket,
        )
        .await
        .unwrap();
    assert!(!output.success);
    assert_eq!(output.stderr.trim(), "Permission denied (publickey).");
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
        control_dir: None,
    };
    let slot: TunnelSlot<super::runner::SystemTunnel> = Arc::default();
    let (reports, report) = reporter();
    let result =
        connect::connect_once(&super::runner::System, &setup, &Mux::Off, &slot, &report).await;
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
