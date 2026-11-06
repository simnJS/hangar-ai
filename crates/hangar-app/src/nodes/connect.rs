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

use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::state::types::NodeConfig;

use super::commands;
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
            handshake_tries: 20,
            handshake_wait: Duration::from_millis(250),
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

/// Steps 1 to 6, once. On success the tunnel is in `slot`.
pub async fn connect_once<R: Runner>(
    runner: &R,
    setup: &Setup,
    slot: &TunnelSlot<R::Tunnel>,
    report: &(impl Fn(NodeStatus) + Send + Sync),
) -> Result<(NodeInfo, u16, String), Failure> {
    let node = &setup.node;
    let mut laps = Laps::new(&node.id);

    report(NodeStatus::Connecting(Step::Probe));
    let output = run_ok(runner, commands::probe(node), Step::Probe).await?;
    laps.lap("probe");
    let probe = parse_probe(&output.stdout).map_err(|detail| Failure::Ssh {
        step: Step::Probe,
        detail,
    })?;

    let decision = decide(probe.installed.as_deref(), &setup.version);
    if decision != Decision::Keep {
        report(NodeStatus::Connecting(Step::Install));
        let binary = setup.source.binary_for(&probe.arch)?;
        run_ok(runner, commands::make_bin_dir(node), Step::Install).await?;
        run_ok(runner, commands::upload(node, &binary), Step::Install).await?;
        run_ok(runner, commands::put_in_place(node), Step::Install).await?;
        if let Decision::Update { .. } = decision {
            stop_older_daemon(runner, setup).await;
        }
        laps.lap("install");
    }

    report(NodeStatus::Connecting(Step::Start));
    run_ok(runner, commands::serve(node), Step::Start).await?;
    laps.lap("start");

    report(NodeStatus::Connecting(Step::Config));
    let output = run_ok(runner, commands::read_node_json(node), Step::Config).await?;
    let json = parse_node_json(&output.stdout).map_err(Failure::NodeJson)?;
    laps.lap("config");

    report(NodeStatus::Connecting(Step::Tunnel));
    let local = runner.free_port().map_err(Failure::Local)?;
    let tunnel = runner
        .spawn_tunnel(commands::tunnel(node, local, json.port))
        .map_err(Failure::Local)?;
    if let Some(mut old) = locked(slot).replace(tunnel) {
        old.kill();
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

/// The daemon `node.json` names, stopped when it is of another version than
/// the one just installed. Best effort: a node.json left by a daemon that
/// has since died names nothing that runs.
async fn stop_older_daemon<R: Runner>(runner: &R, setup: &Setup) {
    let Ok(output) = runner.run(commands::read_node_json(&setup.node)).await else {
        return;
    };
    if !output.success {
        return;
    }
    let Ok(json) = parse_node_json(&output.stdout) else {
        return;
    };
    if let (Some(pid), true) = (json.pid, json.version != setup.version) {
        let _ = runner.run(commands::stop(&setup.node, pid)).await;
    }
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
    loop {
        let failure = match connect_once(&runner, &setup, &slot, &report).await {
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
