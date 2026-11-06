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

/// What both ssh and scp are given: never a prompt (a password, a host key
/// to accept) the app could not answer, a bounded wait, and the person's
/// config file when the node names one.
fn common(node: &NodeConfig) -> Vec<String> {
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

/// `ssh <options> <target> <remote>`.
pub fn ssh(node: &NodeConfig, remote: &str) -> Cmd {
    Cmd::new("ssh")
        .args(common(node))
        .arg(node.ssh_target.trim())
        .arg(remote)
}

/// The binary's path on the node, for the remote shell.
fn remote_bin() -> String {
    format!("~/{REMOTE_BIN}")
}

/// Step 1: the machine's architecture, then the installed version, or
/// nothing when hangar-node is not there (which is not a failure).
pub fn probe(node: &NodeConfig) -> Cmd {
    ssh(
        node,
        &format!("uname -m; {} version 2>/dev/null || true", remote_bin()),
    )
}

/// Step 2a: the folder the binary goes in.
pub fn make_bin_dir(node: &NodeConfig) -> Cmd {
    ssh(node, &format!("mkdir -p ~/{REMOTE_BIN_DIR}"))
}

/// Step 2b: the upload, beside the binary.
pub fn upload(node: &NodeConfig, local: &Path) -> Cmd {
    Cmd::new("scp")
        .args(common(node))
        .arg("-q")
        .arg(local.display().to_string())
        .arg(format!("{}:{REMOTE_BIN_UPLOAD}", node.ssh_target.trim()))
}

/// Step 2c: the upload made executable and put in the binary's place.
pub fn put_in_place(node: &NodeConfig) -> Cmd {
    ssh(
        node,
        &format!(
            "chmod +x ~/{REMOTE_BIN_UPLOAD} && mv -f ~/{REMOTE_BIN_UPLOAD} {}",
            remote_bin()
        ),
    )
}

/// The daemon of an older version, stopped before the new one starts: it
/// would otherwise keep the port and answer with its old API.
pub fn stop(node: &NodeConfig, pid: u32) -> Cmd {
    ssh(node, &format!("kill {pid} 2>/dev/null || true"))
}

/// Step 3.
pub fn serve(node: &NodeConfig) -> Cmd {
    ssh(node, &format!("{} serve --detach", remote_bin()))
}

/// Step 4.
pub fn read_node_json(node: &NodeConfig) -> Cmd {
    ssh(node, &format!("cat ~/{REMOTE_NODE_JSON}"))
}

/// Step 5: the node's API on `local` here, kept open by the runner. It exits
/// if the forward cannot be set up rather than lingering without it, and
/// notices a dead link within a minute.
pub fn tunnel(node: &NodeConfig, local: u16, remote: u16) -> Cmd {
    Cmd::new("ssh")
        .args(common(node))
        .args([
            "-o",
            "ExitOnForwardFailure=yes",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=3",
            "-N",
            "-L",
        ])
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
pub fn git_ssh_command(node: &NodeConfig) -> String {
    std::iter::once("ssh".to_string())
        .chain(common(node).iter().map(|word| sh_quoted(word)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The node's repository as git names a remote over SSH: `<target>:<path>`.
fn remote_url(node: &NodeConfig, repo: &str) -> String {
    format!("{}:{repo}", node.ssh_target.trim())
}

/// `git -C <local> -c core.sshCommand=<ssh> <args…>`.
fn git_over_ssh(node: &NodeConfig, local: &str) -> Cmd {
    Cmd::new("git")
        .args(["-C", local, "-c"])
        .arg(format!("core.sshCommand={}", git_ssh_command(node)))
}

/// The branch a worker starts from, as it is in this machine's repository,
/// put in the node's (`repo`, an absolute path there): forced, since the
/// node's copy only ever follows this one.
pub fn push_branch(node: &NodeConfig, local: &str, repo: &str, branch: &str) -> Cmd {
    git_over_ssh(node, local)
        .args(["push", "--quiet", "--force", "--no-verify"])
        .arg(remote_url(node, repo))
        .arg(format!("refs/heads/{branch}:refs/heads/{branch}"))
}

/// A worker's branch fetched back from the node's repository into this
/// machine's, under the same name: forced, as the worker may have rebased.
pub fn fetch_branch(node: &NodeConfig, local: &str, repo: &str, branch: &str) -> Cmd {
    git_over_ssh(node, local)
        .args(["fetch", "--quiet", "--no-tags"])
        .arg(remote_url(node, repo))
        .arg(format!("+refs/heads/{branch}:refs/heads/{branch}"))
}
