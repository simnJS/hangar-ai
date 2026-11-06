//! A local workspace's workers on a node: the command lines git reaches the
//! node with, the names and paths, and the way there and back against a
//! fake node that answers with core's own git helpers, as hangar-node does,
//! over real repositories.

// The way there and back runs on unix only (see below): its helpers rest
// unused on Windows.
#![cfg_attr(not(unix), allow(dead_code, unused_imports))]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::*;
use crate::nodes::commands::{Cmd, Mux};
use crate::nodes::model::NodeInfo;
use crate::nodes::runner::{CmdOutput, Runner, Tunnel};
use crate::nodes::testing::{answer, listen, read_request};
use crate::state::types::NodeConfig;

fn node(config: Option<&str>) -> NodeConfig {
    NodeConfig {
        id: "n1".into(),
        label: "GPU box".into(),
        ssh_target: "me@gpu".into(),
        ssh_config: config.map(str::to_string),
        ..NodeConfig::default()
    }
}

#[test]
fn git_reaches_the_node_with_the_connection_s_ssh() {
    let push = commands::push_branch(
        &node(Some("/home/me/my ssh/config")),
        &Mux::Off,
        "/home/me/site",
        "/home/me/hangar-repos/site-1a2b3c4d",
        "main",
    );
    assert_eq!(push.program, "git");
    assert_eq!(
        push.args,
        [
            "-C",
            "/home/me/site",
            "-c",
            "core.sshCommand=ssh -o BatchMode=yes -o ConnectTimeout=10 -F '/home/me/my ssh/config'",
            "push",
            "--quiet",
            "--force",
            "--no-verify",
            "me@gpu:/home/me/hangar-repos/site-1a2b3c4d",
            "refs/heads/main:refs/heads/main",
        ]
    );

    let fetch = commands::fetch_branch(
        &node(None),
        &Mux::Off,
        "/home/me/site",
        "/r",
        "hangar/x-1a2b3c",
    );
    assert_eq!(
        fetch.args,
        [
            "-C",
            "/home/me/site",
            "-c",
            "core.sshCommand=ssh -o BatchMode=yes -o ConnectTimeout=10",
            "fetch",
            "--quiet",
            "--no-tags",
            "me@gpu:/r",
            "+refs/heads/hangar/x-1a2b3c:refs/heads/hangar/x-1a2b3c",
        ]
    );
    // A quote in a config's path stays one word for the shell git uses.
    assert!(commands::git_ssh_command(&node(Some("/it's")), &Mux::Off).ends_with(r"-F '/it'\''s'"));
}

#[test]
fn git_goes_through_the_connection_s_master_when_it_has_one() {
    let own = Mux::Own("/run/user/1000/hangar-ssh/42-ab".into());
    assert_eq!(
        commands::git_ssh_command(&node(None), &own),
        "ssh -o BatchMode=yes -o ConnectTimeout=10 -o ControlMaster=no \
         -o ControlPath=/run/user/1000/hangar-ssh/42-ab"
    );
    // The person's own sharing: their config applies, nothing is added.
    assert_eq!(
        commands::git_ssh_command(&node(None), &Mux::Theirs),
        "ssh -o BatchMode=yes -o ConnectTimeout=10"
    );
}

#[test]
fn the_node_s_repository_is_named_after_the_checkout_and_its_path() {
    let name = repo_name("/home/me/My Site/");
    assert!(name.starts_with("My-Site-"), "{name}");
    assert_eq!(name.len(), "My-Site-".len() + 8);
    assert_eq!(name, repo_name("/home/me/My Site"), "the same each time");
    assert_ne!(name, repo_name("/srv/My Site"), "two checkouts named alike");
    assert_eq!(
        repo_name("C:\\Users\\me\\site"),
        repo_name("c:/users/me/site"),
        "Windows spells one folder many ways"
    );
    assert!(repo_name("/").starts_with("repo-"));
    assert!(repo_name("/x/.hidden").starts_with("hidden-"));
}

#[test]
fn a_worker_s_folder_on_the_node_names_its_repository() {
    assert_eq!(
        repo_of("/home/h/hangar-repos/site-1a-worktrees/hangar-x-1a2b3c"),
        Some((
            "/home/h/hangar-repos/site-1a".to_string(),
            "/home/h/hangar-repos/site-1a-worktrees/hangar-x-1a2b3c".to_string()
        ))
    );
    // A workspace on a subfolder of its repository: the same subfolder.
    assert_eq!(
        repo_of("/r/site-worktrees/w/app/web").map(|(_, root)| root),
        Some("/r/site-worktrees/w".to_string())
    );
    for not_ours in [
        "/home/h/site",
        "/home/h/-worktrees/w",
        "/r/site-worktrees/",
        "",
    ] {
        assert_eq!(repo_of(not_ours), None, "{not_ours}");
    }
}

#[test]
fn a_path_in_a_node_s_repository_is_that_node_s() {
    remember("n7", "/home/h/hangar-repos/known-1a");
    let state = AppState::default();
    for path in [
        "/home/h/hangar-repos/known-1a",
        "/home/h/hangar-repos/known-1a-worktrees/hangar-x",
        "/home/h/hangar-repos/known-1a-worktrees/hangar-x/sub",
    ] {
        assert_eq!(node_of_path(&state, path).as_deref(), Some("n7"), "{path}");
    }
    assert_eq!(node_of_path(&state, "/home/h/hangar-repos/known-1a2"), None);
    assert_eq!(node_of_path(&state, "/home/me/site"), None);

    // After a restart: the panes say where they run.
    let mut pane = Pane {
        cwd: Some("/srv/hangar-repos/later-2b-worktrees/hangar-y".into()),
        ..Pane::default()
    };
    set_pane_node(&mut pane, "n8");
    let state = AppState {
        workspaces: vec![crate::state::types::Workspace {
            panes: vec![pane],
            ..Default::default()
        }],
        ..AppState::default()
    };
    assert_eq!(
        node_of_path(&state, "/srv/hangar-repos/later-2b").as_deref(),
        Some("n8")
    );
}

#[test]
fn a_pane_names_its_node_in_its_json() {
    let mut pane = Pane::default();
    assert_eq!(pane_node(&pane), None);
    set_pane_node(&mut pane, "n1");
    assert_eq!(pane_node(&pane).as_deref(), Some("n1"));
    let written = serde_json::to_value(&pane).unwrap();
    assert_eq!(written["node"], "n1", "the field the pane model will read");
    let read: Pane = serde_json::from_value(written).unwrap();
    assert_eq!(pane_node(&read).as_deref(), Some("n1"));
}

#[test]
fn a_node_without_a_name_goes_by_its_target() {
    let unnamed = NodeConfig {
        id: "n2".into(),
        ssh_target: " build@ci ".into(),
        ..NodeConfig::default()
    };
    assert_eq!(label_of(&unnamed), "build@ci");
    assert_eq!(label_of(&node(None)), "GPU box");
}

/* ── There and back, against a fake node ───────────────────────── */

/// `git` in `dir` as the test's author; its output.
fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// ssh to the node, played by this machine: the command runs as given,
/// with `<target>:<path>` read as the path, and is recorded.
#[derive(Clone, Default)]
struct LocalSsh {
    ran: Arc<Mutex<Vec<Cmd>>>,
}

struct NoTunnel;

impl Tunnel for NoTunnel {
    fn exited(&mut self) -> Option<String> {
        Some("no tunnel".into())
    }
    fn kill(&mut self) {}
}

impl Runner for LocalSsh {
    type Tunnel = NoTunnel;

    async fn run(&self, cmd: Cmd) -> Result<CmdOutput, String> {
        self.ran.lock().unwrap().push(cmd.clone());
        let args: Vec<String> = cmd
            .args
            .iter()
            .map(|arg| arg.strip_prefix("me@gpu:").unwrap_or(arg).to_string())
            .collect();
        let output = tokio::process::Command::new(&cmd.program)
            .args(&args)
            .output()
            .await
            .map_err(|err| err.to_string())?;
        Ok(CmdOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    fn spawn_tunnel(&self, _cmd: Cmd) -> Result<NoTunnel, String> {
        Err("no tunnel".into())
    }

    async fn open_link(
        &self,
        _cmd: Cmd,
        _control_path: PathBuf,
    ) -> Result<(NoTunnel, CmdOutput), String> {
        Err("no tunnel".into())
    }

    fn free_port(&self) -> Result<u16, String> {
        Err("no port".into())
    }

    async fn node_info(&self, _port: u16, _token: String) -> Result<NodeInfo, String> {
        Err("no info".into())
    }
}

fn value(value: impl serde::Serialize) -> String {
    json!({ "value": value }).to_string()
}

/// What hangar-node answers for the routes these use, with core's helpers.
async fn node_answer(home: &Path, line: &str, body: &Value) -> (u16, String) {
    let text = |key: &str| body[key].as_str().unwrap_or_default().to_string();
    match line.split(' ').take(2).collect::<Vec<_>>().as_slice() {
        ["POST", "/node/repo"] => {
            let path = home.join("hangar-repos").join(text("name"));
            if !path.join("HEAD").exists() {
                std::fs::create_dir_all(&path).unwrap();
                git(&path, &["init", "--bare", "-q"]);
            }
            (200, value(json!({ "path": path, "created": true })))
        }
        ["POST", "/node/git"] => match body["op"].as_str() {
            Some("repo_info") => (
                200,
                value(hangar_core::git::git_repo_info(text("path")).await),
            ),
            Some("changes") => (
                200,
                value(
                    hangar_core::git::git_changes(text("path"), None)
                        .await
                        .unwrap(),
                ),
            ),
            other => (400, json!({ "error": format!("{other:?}") }).to_string()),
        },
        ["POST", "/node/worktree"] => {
            hangar_core::git::git_worktree_add(
                text("repo"),
                text("path"),
                text("branch"),
                true,
                body["base"].as_str().map(str::to_string),
            )
            .await
            .unwrap();
            (200, value(Value::Null))
        }
        ["DELETE", "/node/worktree"] => (
            200,
            value(
                hangar_core::git::git_worktree_remove(text("repo"), text("path"), true, None)
                    .await
                    .unwrap(),
            ),
        ),
        ["DELETE", "/node/branch"] => (
            200,
            value(
                hangar_core::git::git_branch_delete(text("repo"), text("branch"), true)
                    .await
                    .unwrap(),
            ),
        ),
        _ => (404, json!({ "error": line }).to_string()),
    }
}

/// A fake node whose disk is `home`: its client.
async fn fake_node(home: PathBuf) -> NodeClient {
    let (listener, client) = listen().await;
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let request = read_request(&mut stream).await;
            let body: Value = serde_json::from_str(&request.body).unwrap_or(Value::Null);
            let (status, said) = node_answer(&home, request.line(), &body).await;
            answer(&mut stream, status, &said).await;
        }
    });
    client
}

// A node is a Linux machine: its fake here lays POSIX paths out on this
// disk, which a Windows host cannot stand in for.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_worker_s_branch_goes_to_the_node_and_comes_back() {
    let dir = tempfile::tempdir().unwrap();
    // macOS: the temp folder is a symlink (/var → /private/var) git resolves.
    let base = dir.path().canonicalize().unwrap();
    let here = base.join("site");
    let node_home = base.join("node");
    std::fs::create_dir_all(&here).unwrap();
    std::fs::create_dir_all(&node_home).unwrap();
    git(&here, &["init", "-q"]);
    std::fs::write(here.join("README"), "hi\n").unwrap();
    git(&here, &["add", "README"]);
    git(&here, &["commit", "-q", "-m", "first"]);
    let here_s = here.to_string_lossy().into_owned();
    let client = fake_node(node_home.clone()).await;
    let ssh = LocalSsh::default();
    let config = node(None);

    // The start: the branch it starts from, sent to the node's repository.
    let repo = send_branch(&ssh, &config, &client, &here_s, "main")
        .await
        .unwrap();
    assert!(
        repo.starts_with(
            &node_home
                .join("hangar-repos")
                .to_string_lossy()
                .into_owned()
        )
    );
    assert_eq!(
        git(Path::new(&repo), &["rev-parse", "main"]),
        git(&here, &["rev-parse", "main"]),
        "the node starts from this machine's main"
    );
    {
        let ran = ssh.ran.lock().unwrap();
        assert_eq!(ran.len(), 1);
        assert!(ran[0].args.contains(&format!("me@gpu:{repo}")));
        assert!(
            ran[0]
                .args
                .iter()
                .any(|arg| arg.starts_with("core.sshCommand=ssh -o BatchMode=yes"))
        );
    }

    // The worker's worktree on the node, and its work there.
    let branch = "hangar/do-it-1a2b3c";
    let tree = format!("{repo}-worktrees/hangar-do-it-1a2b3c");
    engine::worktree_add(&client, &repo, &tree, branch, true, Some("main"))
        .await
        .unwrap();
    std::fs::write(Path::new(&tree).join("work.txt"), "done\n").unwrap();

    // Uncommitted: refused, in the words core's preflight uses.
    let refused = bring_back(&ssh, &config, &client, &here_s, &tree, false)
        .await
        .unwrap_err();
    assert!(
        refused.contains("uncommitted changes: work.txt"),
        "{refused}"
    );
    // The person landing what waited does not ask.
    git(Path::new(&tree), &["add", "work.txt"]);
    git(Path::new(&tree), &["commit", "-q", "-m", "work"]);

    let back = bring_back(&ssh, &config, &client, &here_s, &tree, false)
        .await
        .unwrap();
    assert_eq!(back.branch, branch);
    assert_eq!(
        back.worktree,
        crate::orchestration::worker::worktree_folder(&here_s, branch),
        "where a local worker's worktree would be"
    );
    assert_eq!(
        git(&here, &["rev-parse", branch]),
        git(Path::new(&tree), &["rev-parse", "HEAD"])
    );
    assert!(Path::new(&back.worktree).join("work.txt").is_file());
    assert_eq!(
        git(Path::new(&back.worktree), &["status", "--porcelain"]),
        ""
    );

    // Fixed after a review and delivered again: brought back again.
    std::fs::write(Path::new(&tree).join("work.txt"), "fixed\n").unwrap();
    git(Path::new(&tree), &["commit", "-qam", "fix"]);
    let again = bring_back(&ssh, &config, &client, &here_s, &tree, false)
        .await
        .unwrap();
    assert_eq!(again, back);
    assert_eq!(
        std::fs::read_to_string(Path::new(&back.worktree).join("work.txt")).unwrap(),
        "fixed\n"
    );

    // Landed: the node's worktree and branch go.
    let cleanup = clean_node(&client, &tree, branch).await;
    assert!(cleanup.worktree_removed, "{cleanup:?}");
    assert!(cleanup.branch_deleted, "{cleanup:?}");
    assert!(!Path::new(&tree).exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_hangar_did_not_lay_out_is_not_brought_back() {
    let (_listener, client) = listen().await;
    let refused = bring_back(
        &LocalSsh::default(),
        &node(None),
        &client,
        "/home/me/site",
        "/home/h/somewhere",
        false,
    )
    .await
    .unwrap_err();
    assert!(refused.contains("not a worktree Hangar made"), "{refused}");
}
