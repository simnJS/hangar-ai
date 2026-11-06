//! `hangar-node`: the headless Hangar a node runs (docs/native/nodes.md,
//! sections 2 and 3).
//!
//! - `hangar-node version` prints `<version> <target triple>`;
//! - `hangar-node serve` runs the node in the foreground (for systemd), and
//!   `serve --detach` starts it in a session of its own, its output in
//!   `~/.local/state/hangar-node/node.log`, and returns once it answers;
//! - `hangar-node --mcp` is the MCP stdio proxy the agents of the node's
//!   panes start, as `hangar-ai --mcp` is on the desktop.
//!
//! Linux only: built for another system, the binary says so and exits.

mod api;
mod daemon;
mod events;
mod files;
mod panes;
mod proxy;
mod state;
mod transcripts;

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("hangar-node runs on Linux only");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run(&args));
}

/// The version and target `version` prints.
#[cfg(target_os = "linux")]
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
#[cfg(target_os = "linux")]
pub const TARGET: &str = env!("HANGAR_NODE_TARGET");

#[cfg(target_os = "linux")]
const USAGE: &str = "usage: hangar-node version | serve [--detach] | --mcp";

#[cfg(target_os = "linux")]
fn run(args: &[String]) -> i32 {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["version"] | ["--version"] | ["-V"] => {
            println!("{VERSION} {TARGET}");
            0
        }
        ["--mcp", ..] => {
            mcp();
            0
        }
        ["serve"] => report(daemon::serve()),
        ["serve", "--detach"] | ["--detach", "serve"] => report(daemon::detach()),
        ["help"] | ["--help"] | ["-h"] => {
            println!("{USAGE}");
            0
        }
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}

#[cfg(target_os = "linux")]
fn report(outcome: Result<(), String>) -> i32 {
    match outcome {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("hangar-node: {err}");
            1
        }
    }
}

/// The node publishes its agents' API to a file of its own
/// (`~/.local/state/hangar-node/endpoint.json`), never the desktop's
/// `~/.hangar/endpoint.json`, so a desktop Hangar on the same account keeps
/// its agents. The proxy finds it through `HANGAR_ENDPOINT_FILE`, unless the
/// caller already names one.
#[cfg(target_os = "linux")]
fn mcp() {
    let named = std::env::var_os("HANGAR_ENDPOINT_FILE").is_some_and(|v| !v.is_empty());
    if !named {
        if let Ok(dir) = state::dir() {
            // Still single-threaded: nothing else reads the environment yet.
            std::env::set_var("HANGAR_ENDPOINT_FILE", state::endpoint_file(&dir));
        }
    }
    hangar_core::run_mcp();
}
