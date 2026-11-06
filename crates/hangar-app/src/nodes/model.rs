//! What a node is while the app runs: its status, what it said about itself,
//! and the decisions taken from what the machine answered. Plain values, so
//! that the connection's steps can be tested without SSH.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The version this desktop installs on its nodes: the workspace's, which
/// `hangar-node` shares (both are built from the same release).
pub const NODE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where the binary lives on a node, under the remote home.
pub const REMOTE_BIN_DIR: &str = ".local/share/hangar-node/bin";
/// The binary itself, under the remote home.
pub const REMOTE_BIN: &str = ".local/share/hangar-node/bin/hangar-node";
/// Where an upload lands before it replaces the binary: a running daemon
/// keeps its file busy, and a rename is what replaces it under a process
/// that still runs.
pub const REMOTE_BIN_UPLOAD: &str = ".local/share/hangar-node/bin/hangar-node.new";
/// What `hangar-node serve` writes once it listens, under the remote home.
pub const REMOTE_NODE_JSON: &str = ".local/state/hangar-node/node.json";

/// The development override of the binary to upload.
pub const BINARY_ENV: &str = "HANGAR_NODE_BINARY";

/// A step of a connection, in the order they run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// `uname -m` and `hangar-node version`.
    Probe,
    /// The binary uploaded, when missing or of another version.
    Install,
    /// `hangar-node serve --detach`.
    Start,
    /// `node.json` read.
    Config,
    /// The SSH tunnel opened.
    Tunnel,
    /// `GET /node/info` through the tunnel.
    Handshake,
}

/// Why a node could not be reached. The page words it; the details are what
/// the tools said, which are not translated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// A command over SSH failed during `step`.
    Ssh { step: Step, detail: String },
    /// No binary to upload for this architecture.
    NoBinary {
        arch: String,
        file: String,
        dir: String,
    },
    /// The machine is not one hangar-node is built for.
    UnsupportedArch(String),
    /// `node.json` is missing or does not read.
    NodeJson(String),
    /// The tunnel ended.
    Tunnel(String),
    /// The node's API did not answer as it should.
    Api(String),
    /// Something on this machine (no free port, `ssh` not found…).
    Local(String),
}

/// What `GET /node/info` answers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct NodeInfo {
    pub version: String,
    pub target: String,
    pub hostname: String,
    pub home: String,
    pub claude: bool,
    pub git: bool,
}

/// Where a node is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum NodeStatus {
    #[default]
    Disconnected,
    Connecting(Step),
    Connected {
        info: NodeInfo,
        /// The tunnel's end on this machine.
        local_port: u16,
        token: String,
    },
    /// `retry_in`: seconds before the next attempt, when one is planned (a
    /// node that was connected and dropped); `None` waits for the person.
    Unreachable {
        failure: Failure,
        retry_in: Option<u64>,
    },
}

/// What a later task needs to talk to a connected node.
#[allow(dead_code)] // Made by `nodes::client`, for the remote workspaces to come.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeClient {
    /// `http://127.0.0.1:<port>`, the tunnel's end.
    pub base_url: String,
    pub token: String,
}

/// `~/.local/state/hangar-node/node.json`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct NodeJson {
    pub version: String,
    pub port: u16,
    pub token: String,
    #[serde(default)]
    pub pid: Option<u32>,
}

/// `node.json`'s text, or why it is not one.
pub fn parse_node_json(text: &str) -> Result<NodeJson, String> {
    let json: NodeJson = serde_json::from_str(text.trim()).map_err(|err| err.to_string())?;
    if json.port == 0 {
        return Err("port 0".into());
    }
    if json.token.is_empty() {
        return Err("empty token".into());
    }
    Ok(json)
}

/// What the probe found: the machine's architecture and the installed
/// version, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    /// `uname -m` as the machine said it.
    pub arch: String,
    /// The first word of `hangar-node version`; `None` when it is not there.
    pub installed: Option<String>,
}

/// The probe's output: `uname -m` on the first line, then whatever
/// `hangar-node version` printed (`<version> <target-triple>`), nothing when
/// it is missing.
pub fn parse_probe(output: &str) -> Result<Probe, String> {
    let mut lines = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let arch = lines
        .next()
        .ok_or_else(|| "uname -m printed nothing".to_string())?
        .to_string();
    let installed = lines
        .next()
        .and_then(|line| line.split_whitespace().next())
        .map(str::to_string);
    Ok(Probe { arch, installed })
}

/// What to do with the binary on the node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// None there.
    Install,
    /// Another version there.
    Update { from: String },
    /// This version there.
    Keep,
}

pub fn decide(installed: Option<&str>, wanted: &str) -> Decision {
    match installed {
        None => Decision::Install,
        Some(version) if version == wanted => Decision::Keep,
        Some(version) => Decision::Update {
            from: version.to_string(),
        },
    }
}

/// The name hangar-node's builds go by for `uname -m`'s answer, or `None`
/// for a machine it is not built for.
pub fn normalize_arch(uname: &str) -> Option<&'static str> {
    match uname.trim() {
        "x86_64" | "amd64" => Some("x86_64"),
        "aarch64" | "arm64" | "armv8l" => Some("aarch64"),
        _ => None,
    }
}

/// Where the binary to upload comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinarySource {
    /// `HANGAR_NODE_BINARY`, for development: used whatever the machine.
    pub override_path: Option<PathBuf>,
    /// The folder of the running `hangar-ai`, which holds
    /// `hangar-node-<arch>` in a release.
    pub exe_dir: Option<PathBuf>,
}

impl BinarySource {
    /// From the environment and the running executable.
    pub fn from_env() -> BinarySource {
        BinarySource {
            override_path: std::env::var_os(BINARY_ENV)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
            exe_dir: std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(Path::to_path_buf)),
        }
    }

    /// The file to upload for a machine whose `uname -m` is `uname`.
    pub fn binary_for(&self, uname: &str) -> Result<PathBuf, Failure> {
        let Some(arch) = normalize_arch(uname) else {
            return Err(Failure::UnsupportedArch(uname.to_string()));
        };
        let file = format!("hangar-node-{arch}");
        let dir = self.exe_dir.as_deref();
        if let Some(path) = &self.override_path {
            if path.is_file() {
                return Ok(path.clone());
            }
            return Err(Failure::NoBinary {
                arch: arch.to_string(),
                file: path.display().to_string(),
                dir: dir.map(|d| d.display().to_string()).unwrap_or_default(),
            });
        }
        if let Some(path) = dir.map(|dir| dir.join(&file)).filter(|p| p.is_file()) {
            return Ok(path);
        }
        Err(Failure::NoBinary {
            arch: arch.to_string(),
            file,
            dir: dir.map(|d| d.display().to_string()).unwrap_or_default(),
        })
    }
}

/// Why an SSH target cannot be used, or `None`. A target is one argument of
/// `ssh`: a leading dash would make it an option (`-oProxyCommand=…`), and a
/// space would split it.
pub fn target_problem(target: &str) -> Option<TargetProblem> {
    let target = target.trim();
    if target.is_empty() {
        return Some(TargetProblem::Empty);
    }
    if target.starts_with('-') || target.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Some(TargetProblem::Invalid);
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetProblem {
    Empty,
    Invalid,
}
