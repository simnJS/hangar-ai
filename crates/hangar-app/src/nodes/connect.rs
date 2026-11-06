//! One node's connection, on tokio: the steps of docs/native/nodes.md §4 in
//! order, then the tunnel watched and opened again when it dies.
//!
//! A node that never connected stops at its first failure and waits for the
//! person (a missing binary or a refused key does not mend itself). One that
//! was connected and dropped is retried with a growing delay: a laptop that
//! sleeps or changes network gets its node back on its own.
//!
//! The task owns everything it starts; the tunnel sits in a slot the
//! package also holds, so quitting can kill it synchronously, without
//! waiting for the task to be dropped.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, OnceLock, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::state::types::NodeConfig;

use super::commands::{self, Mux};
use super::model::{
    BinarySource, Decision, Failure, NodeInfo, NodeStatus, Step, decide, parse_node_json,
    parse_probe,
};
use super::runner::{CmdOutput, Runner, Tunnel};

/// Where a connection keeps its tunnel while it runs.
pub type TunnelSlot<T> = Arc<Mutex<Option<T>>>;

pub fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// `HANGAR_NODES_TIMING=1`: each step of a connection timed on stderr, one
/// line each (`nodes timing: node=<id> step=<step> ms=<its time>
/// at=<epoch ms>`), which `scripts/node-test/bench.sh` reads.
pub fn timing_on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("HANGAR_NODES_TIMING").is_ok_and(|value| value == "1"))
}

/// One timing line, when they are asked for.
pub fn timing_line(fields: &str) {
    if timing_on() {
        let at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_millis());
        eprintln!("nodes timing: {fields} at={at}");
    }
}

/// The steps of one connection, timed.
struct Laps<'a> {
    node: &'a str,
    last: Instant,
}

impl<'a> Laps<'a> {
    fn new(node: &'a str) -> Self {
        timing_line(&format!("node={node} step=begin ms=0"));
        Laps {
            node,
            last: Instant::now(),
        }
    }

    /// `step` is over: its time since the last one.
    fn lap(&mut self, step: &str) {
        let now = Instant::now();
        timing_line(&format!(
            "node={} step={step} ms={}",
            self.node,
            (now - self.last).as_millis()
        ));
        self.last = now;
    }
}

/// The waits of a connection; the tests shorten them.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// How often the tunnel is looked at.
    pub poll: Duration,
    /// The first wait before a reconnection, doubled each time up to
    /// `max_backoff`.
    pub first_backoff: Duration,
    pub max_backoff: Duration,
    /// `/node/info` is asked this many times, `handshake_wait` apart, while
    /// the tunnel comes up.
    pub handshake_tries: u32,
    pub handshake_wait: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            poll: Duration::from_millis(500),
            first_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(30),
            // A tunnel of its own (no master) listens a moment after its
            // ssh starts: asked often, it answers as soon as it can.
            handshake_tries: 250,
            handshake_wait: Duration::from_millis(20),
        }
    }
}

/// What a connection needs besides its runner.
#[derive(Debug, Clone)]
pub struct Setup {
    pub node: NodeConfig,
    pub source: BinarySource,
    /// The version to have on the node.
    pub version: String,
    pub timing: Timing,
    /// Where the connection's master socket goes ([`Mux::Own`]); `None`
    /// shares nothing (Windows, no private folder for it).
    pub control_dir: Option<PathBuf>,
}

/// The folder of the app's SSH master sockets, made private: under
/// `HANGAR_NODES_SSH_DIR` when set (the rig's runs), else the runtime
/// folder (`$XDG_RUNTIME_DIR`), else the cache. `None` on Windows, or when
/// it cannot be made, or is too deep for a socket's path (about 100 bytes,
/// ssh's temporary name included).
pub fn control_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        return None;
    }
    let dir = std::env::var_os("HANGAR_NODES_SSH_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::runtime_dir().map(|dir| dir.join("hangar-ssh")))
        .or_else(|| dirs::cache_dir().map(|dir| dir.join("hangar-ssh")))?;
    if dir.as_os_str().len() > 60 {
        return None;
    }
    std::fs::create_dir_all(&dir).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).ok()?;
    }
    Some(dir)
}

/// The master socket of node `id` in `dir`: this process's own (two apps
/// on one account each have theirs), short.
pub fn control_path(dir: &Path, id: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    dir.join(format!("{}-{:x}", std::process::id(), hasher.finish()))
}

/// How each node's commands share its connection, as its connection found
/// out: what git's push and fetch (`away.rs`) go through.
static MUXES: LazyLock<RwLock<HashMap<String, Mux>>> = LazyLock::new(Default::default);

pub fn mux_of(node: &str) -> Mux {
    MUXES
        .read()
        .unwrap_or_else(|poison| poison.into_inner())
        .get(node)
        .cloned()
        .unwrap_or_default()
}

/// What node `setup.node` shares: `ssh -G` asked once what the person's
/// config sets.
async fn find_mux<R: Runner>(runner: &R, setup: &Setup) -> Mux {
    let Some(dir) = &setup.control_dir else {
        return Mux::Off;
    };
    let said = runner
        .run(commands::config_query(&setup.node))
        .await
        .ok()
        .filter(|output| output.success)
        .map(|output| output.stdout);
    let path = control_path(dir, &setup.node.id);
    commands::mux_for(
        cfg!(windows),
        said.as_deref(),
        path.to_str().map(str::to_string),
    )
}

/// The last line a tool wrote on stderr, which is where ssh says why.
fn reason(output: &CmdOutput) -> String {
    let said = output.stderr.trim();
    said.lines()
        .last()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "failed without a word".to_string())
}

async fn run_ok<R: Runner>(
    runner: &R,
    cmd: commands::Cmd,
    step: Step,
) -> Result<CmdOutput, Failure> {
    let output = runner
        .run(cmd)
        .await
        .map_err(|detail| Failure::Ssh { step, detail })?;
    if output.success {
        Ok(output)
    } else {
        Err(Failure::Ssh {
            step,
            detail: reason(&output),
        })
    }
}

/// The connection's steps, once. On success the tunnel is in `slot`.
///
/// One remote command probes the node and starts its daemon when the
/// version installed is this one ([`commands::probe_script`]); the install,
/// when one is needed, is an upload then one command; then the tunnel and
/// `/node/info` through it. With [`Mux::Own`] the probe runs in the
/// tunnel's own connection, which the install shares and the forward is
/// added to: one SSH handshake for the whole connection.
pub async fn connect_once<R: Runner>(
    runner: &R,
    setup: &Setup,
    mux: &Mux,
    slot: &TunnelSlot<R::Tunnel>,
    report: &(impl Fn(NodeStatus) + Send + Sync),
) -> Result<(NodeInfo, u16, String), Failure> {
    let node = &setup.node;
    let mut laps = Laps::new(&node.id);

    report(NodeStatus::Connecting(Step::Probe));
    if let Some(mut old) = locked(slot).take() {
        old.kill();
    }
    let output = match mux {
        Mux::Own(path) => {
            // What a killed master left would keep this one from listening.
            let _ = std::fs::remove_file(path);
            let (tunnel, output) = runner
                .open_link(
                    commands::link(node, path, &setup.version),
                    PathBuf::from(path),
                )
                .await
                .map_err(Failure::Local)?;
            *locked(slot) = Some(tunnel);
            if !output.success {
                return Err(Failure::Ssh {
                    step: Step::Probe,
                    detail: reason(&output),
                });
            }
            output
        }
        Mux::Off | Mux::Theirs => {
            run_ok(
                runner,
                commands::probe(node, mux, &setup.version),
                Step::Probe,
            )
            .await?
        }
    };
    let probe = parse_probe(&output.stdout).map_err(|detail| Failure::Ssh {
        step: Step::Probe,
        detail,
    })?;
    laps.lap("probe");

    let decision = decide(probe.installed.as_deref(), &setup.version);
    let node_json = if decision == Decision::Keep {
        if !probe.started {
            return Err(Failure::Ssh {
                step: Step::Start,
                detail: probe
                    .said
                    .lines()
                    .last()
                    .unwrap_or("failed without a word")
                    .to_string(),
            });
        }
        probe.node_json
    } else {
        report(NodeStatus::Connecting(Step::Install));
        let binary = setup.source.binary_for(&probe.arch)?;
        run_ok(runner, commands::upload(node, mux, &binary), Step::Install).await?;
        laps.lap("upload");
        let stop = match decision {
            Decision::Update { .. } => older_daemon(&probe.node_json, &setup.version),
            _ => None,
        };
        report(NodeStatus::Connecting(Step::Start));
        let output = run_ok(
            runner,
            commands::install_and_start(node, mux, stop),
            Step::Start,
        )
        .await?;
        laps.lap("start");
        output.stdout
    };

    report(NodeStatus::Connecting(Step::Config));
    let json = parse_node_json(&node_json).map_err(Failure::NodeJson)?;

    report(NodeStatus::Connecting(Step::Tunnel));
    let local = runner.free_port().map_err(Failure::Local)?;
    match mux {
        Mux::Own(path) => {
            run_ok(
                runner,
                commands::forward(node, path, local, json.port),
                Step::Tunnel,
            )
            .await?;
        }
        Mux::Off | Mux::Theirs => {
            let tunnel = runner
                .spawn_tunnel(commands::tunnel(node, mux, local, json.port))
                .map_err(Failure::Local)?;
            *locked(slot) = Some(tunnel);
        }
    }
    laps.lap("tunnel");

    report(NodeStatus::Connecting(Step::Handshake));
    let mut last = String::new();
    for attempt in 0..setup.timing.handshake_tries.max(1) {
        if attempt > 0 {
            tokio::time::sleep(setup.timing.handshake_wait).await;
        }
        let ended = match locked(slot).as_mut() {
            Some(tunnel) => tunnel.exited(),
            None => Some("closed".to_string()),
        };
        if let Some(why) = ended {
            locked(slot).take();
            return Err(Failure::Tunnel(why));
        }
        match runner.node_info(local, json.token.clone()).await {
            Ok(info) => {
                laps.lap("handshake");
                return Ok((info, local, json.token));
            }
            Err(err) => last = err,
        }
    }
    if let Some(mut tunnel) = locked(slot).take() {
        tunnel.kill();
    }
    Err(Failure::Api(last))
}

/// The pid of the daemon an older `node.json` names, when it is of another
/// version than the one being installed: stopped before the new one starts.
/// A node.json left by a daemon that has since died names nothing that
/// runs, which `kill` is told without harm.
pub fn older_daemon(node_json: &str, version: &str) -> Option<u32> {
    let json = parse_node_json(node_json).ok()?;
    (json.version != version).then_some(json.pid).flatten()
}

/// The connection for as long as the task runs: connected, watched, opened
/// again when it drops.
pub async fn run<R: Runner>(
    runner: R,
    setup: Setup,
    slot: TunnelSlot<R::Tunnel>,
    report: impl Fn(NodeStatus) + Send + Sync,
) {
    let timing = setup.timing;
    let mut backoff = timing.first_backoff;
    let mut was_connected = false;
    let mux = find_mux(&runner, &setup).await;
    MUXES
        .write()
        .unwrap_or_else(|poison| poison.into_inner())
        .insert(setup.node.id.clone(), mux.clone());
    loop {
        let failure = match connect_once(&runner, &setup, &mux, &slot, &report).await {
            Ok((info, local_port, token)) => {
                report(NodeStatus::Connected {
                    info,
                    local_port,
                    token,
                });
                was_connected = true;
                backoff = timing.first_backoff;
                match watch(&slot, timing.poll).await {
                    Some(why) => Failure::Tunnel(why),
                    // Killed from outside: the app is quitting.
                    None => return,
                }
            }
            Err(failure) => {
                if let Some(mut tunnel) = locked(&slot).take() {
                    tunnel.kill();
                }
                if !was_connected {
                    report(NodeStatus::Unreachable {
                        failure,
                        retry_in: None,
                    });
                    return;
                }
                failure
            }
        };
        report(NodeStatus::Unreachable {
            failure,
            retry_in: Some(backoff.as_secs().max(1)),
        });
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(timing.max_backoff);
    }
}

/// Waits for the tunnel to end: why it did, or `None` when it was taken out
/// of its slot (killed on quit).
async fn watch<T: Tunnel>(slot: &TunnelSlot<T>, poll: Duration) -> Option<String> {
    loop {
        tokio::time::sleep(poll).await;
        let mut guard = locked(slot);
        let tunnel = guard.as_mut()?;
        if let Some(why) = tunnel.exited() {
            guard.take();
            return Some(why);
        }
    }
}
