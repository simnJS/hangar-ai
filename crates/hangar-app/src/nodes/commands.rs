//! The command lines of each step, as values: the runner runs them, the tests
//! read them.
//!
//! Every remote command is one argument of `ssh`, which the remote login
//! shell runs. They stick to what sh, bash, zsh and fish all read (`~/`,
//! `;`, `&&`, `||`, `2>/dev/null`), since the node's shell is the person's.
//! Paths are relative to the remote home: `~` for ssh, nothing for scp, whose
//! SFTP mode does not expand `~` everywhere.

use std::path::Path;

use crate::state::types::NodeConfig;

use super::model::{REMOTE_BIN, REMOTE_BIN_DIR, REMOTE_BIN_UPLOAD, REMOTE_NODE_JSON};

/// A program and its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cmd {
    pub program: String,
    pub args: Vec<String>,
}

impl Cmd {
    fn new(program: &str) -> Cmd {
        Cmd {
            program: program.to_string(),
            args: Vec::new(),
        }
    }

    fn arg(mut self, arg: impl Into<String>) -> Cmd {
        self.args.push(arg.into());
        self
    }

    fn args<I: IntoIterator<Item = S>, S: Into<String>>(mut self, args: I) -> Cmd {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }
}

/// How the commands of one node share an SSH connection.
///
/// Every command over its own connection pays the whole handshake (TCP,
/// key exchange, authentication: several round trips, and the CPU of both
/// ends) each time. With `Own`, the connection's tunnel is an OpenSSH
/// master (`ControlMaster`), and the other commands (the install, git's
/// push and fetch) run as sessions of it: one round trip or so each.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Mux {
    /// Each command its own connection: Windows (its OpenSSH has no
    /// `ControlMaster`), or an ssh that could not say what its config sets.
    #[default]
    Off,
    /// The person's ssh config names a `ControlPath`: their sharing, which
    /// the commands are left to.
    Theirs,
    /// The tunnel is the master, at this `ControlPath` (a socket of the
    /// app's, its folder private).
    Own(String),
}

/// What the commands share given what `ssh -G` printed for the node
/// (`None`: it could not say), on a system that has `ControlMaster`
/// (`!windows`), with `control_path` the socket to use when it is ours.
pub fn mux_for(windows: bool, ssh_g: Option<&str>, control_path: Option<String>) -> Mux {
    if windows {
        return Mux::Off;
    }
    let Some(config) = ssh_g else {
        return Mux::Off;
    };
    let theirs = config.lines().any(|line| {
        let mut words = line.split_whitespace();
        words
            .next()
            .is_some_and(|key| key.eq_ignore_ascii_case("controlpath"))
            && words.next().is_some_and(|value| value != "none")
    });
    if theirs {
        return Mux::Theirs;
    }
    control_path.map_or(Mux::Off, Mux::Own)
}

/// What both ssh and scp are given: never a prompt (a password, a host key
/// to accept) the app could not answer, a bounded wait, and the person's
/// config file when the node names one.
fn base(node: &NodeConfig) -> Vec<String> {
    let mut args = vec![
        "-o".to_string(),
        "BatchMode=yes".to_string(),
        "-o".to_string(),
        "ConnectTimeout=10".to_string(),
    ];
    if let Some(config) = node.ssh_config.as_deref().map(str::trim)
        && !config.is_empty()
    {
        args.push("-F".into());
        args.push(config.to_string());
    }
    args
}

/// [`base`], and a session of the master when it is ours. `ControlMaster=no`:
/// a command never becomes a master itself, and goes over a connection of
/// its own while the master is not up (or no more).
fn common(node: &NodeConfig, mux: &Mux) -> Vec<String> {
    let mut args = base(node);
    if let Mux::Own(path) = mux {
        args.extend([
            "-o".to_string(),
            "ControlMaster=no".to_string(),
            "-o".to_string(),
            format!("ControlPath={path}"),
        ]);
    }
    args
}

/// What the tunnel's connection is set up with: it exits if a forward
/// cannot be set up rather than lingering without it, and notices a dead
/// link within a minute.
///
/// What it carries is the panes' keys and echoes: marked interactive
/// (`IPQoS=af21`) whereas ssh marks a session with no terminal as bulk
/// (`cs1`, which Wi-Fi queues as background), and not compressed whatever
/// the person's config says for their own sessions (zlib on every few-byte
/// frame, at both ends).
const LINK: [&str; 10] = [
    "-o",
    "ExitOnForwardFailure=yes",
    "-o",
    "ServerAliveInterval=15",
    "-o",
    "ServerAliveCountMax=3",
    "-o",
    "IPQoS=af21",
    "-o",
    "Compression=no",
];

/// `ssh <options> <target> <remote>`.
pub fn ssh(node: &NodeConfig, mux: &Mux, remote: &str) -> Cmd {
    Cmd::new("ssh")
        .args(common(node, mux))
        .arg(node.ssh_target.trim())
        .arg(remote)
}

/// `ssh -G`: the configuration ssh would use for the node, read for its
/// `ControlPath` (no connection is made).
pub fn config_query(node: &NodeConfig) -> Cmd {
    Cmd::new("ssh")
        .arg("-G")
        .args(base(node))
        .arg(node.ssh_target.trim())
}

/// The binary's path on the node, for the remote shell.
fn remote_bin() -> String {
    format!("~/{REMOTE_BIN}")
}

/// What separates the probe's answers ([`super::model::parse_probe`]).
pub const PROBE_SEP: &str = "@@hangar";
/// The probe's last line, when it runs in the tunnel's session.
pub const PROBE_END: &str = "@@hangar-end";
/// Printed by the probe once the daemon answers.
pub const PROBE_STARTED: &str = "@@hangar-started";

/// Everything one connection asks before its tunnel, in one remote command:
/// the binary's folder made (for an install), the machine's architecture,
/// the installed version (nothing when hangar-node is not there, which is
/// not a failure), the daemon started when the installed version is
/// `version` (`--if-version`: an older binary refuses the flag, another
/// version exits 3, and neither starts anything), and `node.json`: the
/// started daemon's, or the one an older daemon left (its pid, to stop it).
pub fn probe_script(version: &str) -> String {
    let bin = remote_bin();
    let version = sh_quoted(version);
    format!(
        "mkdir -p ~/{REMOTE_BIN_DIR}; uname -m; echo {PROBE_SEP}; {bin} version 2>/dev/null; \
         echo {PROBE_SEP}; {bin} serve --detach --if-version {version} 2>&1 && echo {PROBE_STARTED}; \
         echo {PROBE_SEP}; cat ~/{REMOTE_NODE_JSON} 2>/dev/null"
    )
}

/// The probe over a connection of its own ([`Mux::Off`], [`Mux::Theirs`]).
pub fn probe(node: &NodeConfig, mux: &Mux, version: &str) -> Cmd {
    ssh(node, mux, &probe_script(version))
}

/// [`Mux::Own`]: the tunnel's connection, a master at `control_path`, whose
/// one session runs the probe, prints [`PROBE_END`] and then holds (`cat` on
/// a stdin the app keeps open, which ends with it): the probe's answer is
/// also the sign that the master is up, and the forward is added to it
/// once `node.json` names the port ([`forward`]). It is the app's own
/// process, so killing it closes the tunnel and every session of it.
pub fn link(node: &NodeConfig, control_path: &str, version: &str) -> Cmd {
    Cmd::new("ssh")
        .args(base(node))
        .args([
            "-o",
            "ControlMaster=yes",
            "-o",
            &format!("ControlPath={control_path}"),
            "-o",
            "ControlPersist=no",
        ])
        .args(LINK)
        .arg(node.ssh_target.trim())
        .arg(format!(
            // node.json ends without a newline: the mark on a line of its own.
            "{}; echo; echo {PROBE_END}; exec cat",
            probe_script(version)
        ))
}

/// [`Mux::Own`]: the node's API on `local` here, a forward of the master
/// (`-O forward` returns once the master listens).
pub fn forward(node: &NodeConfig, control_path: &str, local: u16, remote: u16) -> Cmd {
    Cmd::new("ssh")
        .args(base(node))
        .args([
            "-o",
            &format!("ControlPath={control_path}"),
            "-O",
            "forward",
            "-L",
        ])
        .arg(format!("127.0.0.1:{local}:127.0.0.1:{remote}"))
        .arg(node.ssh_target.trim())
}

/// The install: the upload, beside the binary.
pub fn upload(node: &NodeConfig, mux: &Mux, local: &Path) -> Cmd {
    Cmd::new("scp")
        .args(common(node, mux))
        .arg("-q")
        .arg(local.display().to_string())
        .arg(format!("{}:{REMOTE_BIN_UPLOAD}", node.ssh_target.trim()))
}

/// The install, then: the daemon of an older version stopped (`stop`, the
/// pid its node.json names: it would otherwise keep the port and answer
/// with its old API), the upload made executable and put in the binary's
/// place, the new daemon started, its `node.json` printed.
pub fn install_and_start(node: &NodeConfig, mux: &Mux, stop: Option<u32>) -> Cmd {
    let stop = stop.map_or(String::new(), |pid| format!("kill {pid} 2>/dev/null; "));
    let bin = remote_bin();
    ssh(
        node,
        mux,
        &format!(
            "{stop}chmod +x ~/{REMOTE_BIN_UPLOAD} && mv -f ~/{REMOTE_BIN_UPLOAD} {bin} && \
             {bin} serve --detach && cat ~/{REMOTE_NODE_JSON}"
        ),
    )
}

/// [`Mux::Off`], [`Mux::Theirs`]: the node's API on `local` here, kept open by
/// the runner, over a connection of its own ([`LINK`]'s options). Never
/// through a master: a forward asked of one lives in the master, and the
/// `ssh` that asked returns at once (nothing would be left to watch or kill),
/// so the person's `ControlPath` is set aside for this command alone.
pub fn tunnel(node: &NodeConfig, mux: &Mux, local: u16, remote: u16) -> Cmd {
    let mut cmd = Cmd::new("ssh").args(base(node));
    if *mux == Mux::Theirs {
        cmd = cmd.args(["-o", "ControlMaster=no", "-o", "ControlPath=none"]);
    }
    cmd.args(LINK)
        .args(["-N", "-L"])
        .arg(format!("127.0.0.1:{local}:127.0.0.1:{remote}"))
        .arg(node.ssh_target.trim())
}

/* ── A local workspace's workers on a node (nodes/away.rs) ─────── */

/// `word` as one word for a POSIX shell: git runs `core.sshCommand` through
/// one (sh, Git for Windows' too).
fn sh_quoted(word: &str) -> String {
    if !word.is_empty()
        && word
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "-_./=:@,+".contains(ch))
    {
        return word.to_string();
    }
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// What git reaches the node with: the same `ssh` and options as the
/// connection, so a node that connects is a node git reaches.
pub fn git_ssh_command(node: &NodeConfig, mux: &Mux) -> String {
    std::iter::once("ssh".to_string())
        .chain(common(node, mux).iter().map(|word| sh_quoted(word)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The node's repository as git names a remote over SSH: `<target>:<path>`.
fn remote_url(node: &NodeConfig, repo: &str) -> String {
    format!("{}:{repo}", node.ssh_target.trim())
}

/// `git -C <local> -c core.sshCommand=<ssh> <args…>`.
fn git_over_ssh(node: &NodeConfig, mux: &Mux, local: &str) -> Cmd {
    Cmd::new("git")
        .args(["-C", local, "-c"])
        .arg(format!("core.sshCommand={}", git_ssh_command(node, mux)))
}

/// The branch a worker starts from, as it is in this machine's repository,
/// put in the node's (`repo`, an absolute path there): forced, since the
/// node's copy only ever follows this one.
pub fn push_branch(node: &NodeConfig, mux: &Mux, local: &str, repo: &str, branch: &str) -> Cmd {
    git_over_ssh(node, mux, local)
        .args(["push", "--quiet", "--force", "--no-verify"])
        .arg(remote_url(node, repo))
        .arg(format!("refs/heads/{branch}:refs/heads/{branch}"))
}

/// A worker's branch fetched back from the node's repository into this
/// machine's, under the same name: forced, as the worker may have rebased.
pub fn fetch_branch(node: &NodeConfig, mux: &Mux, local: &str, repo: &str, branch: &str) -> Cmd {
    git_over_ssh(node, mux, local)
        .args(["fetch", "--quiet", "--no-tags"])
        .arg(remote_url(node, repo))
        .arg(format!("+refs/heads/{branch}:refs/heads/{branch}"))
}
