//! What the window answers Hangar Mobile about the apps the agents build
//! (lib/remoteApps.ts): the dev servers the phone may open, each through a
//! slot of the mobile server's own (core's `remote/apps.rs`).
//!
//! A worker's are found by themselves, in the port block its shell was given
//! (`HANGAR_PORT_BASE` and the 19 after it); the person adds their own to a
//! workspace in Settings › Mobile (`mobilePorts`). Only the ports a server
//! listens on right now are listed: the phone shows what it can open, and
//! the mobile server, which asks this before it opens anything, opens nothing
//! else.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::control::port::Port;
use crate::orchestration::worker::PORT_COUNT;
use crate::state::Workspace;
use crate::state::roles::workers_of;

use super::contract::{RemoteApp, RemoteApps};
use super::engine::Sources;

/// `remote_ports_listening` asks about this many ports at most at once
/// (core's apps.rs).
pub const PROBE_BATCH: usize = 256;

/// The lowest port the person may add: below it, a system's (`APP_PORT_MIN`).
const APP_PORT_MIN: u16 = 1024;

/// A port the phone could open, listening or not; a worker's names its task
/// by id.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub app: RemoteApp,
    pub task_id: Option<String>,
}

/// The ports the person added to a workspace, as saved: whole ports in
/// range, each once, in order (`appPortsOf`). The model keeps them as `u16`,
/// so a fraction, a string or a number past 65535 never reaches here.
pub fn app_ports_of(ws: &Workspace) -> Vec<u16> {
    let mut ports: Vec<u16> = ws
        .mobile_ports
        .iter()
        .flatten()
        .copied()
        .filter(|port| *port >= APP_PORT_MIN)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    ports.sort_unstable();
    ports
}

/// Every port of these workspaces the phone could open: each worker's block,
/// in the order of the panes, then the ports the person added, but one
/// already in a worker's block. The HQ is not a project: it has none
/// (`candidatesOf`).
pub fn candidates_of(workspaces: &[&Workspace]) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    for ws in workspaces {
        if ws.hq == Some(true) {
            continue;
        }
        let mut taken = HashSet::new();
        for pane in workers_of(ws) {
            let Some(base) = pane.port_base else {
                continue;
            };
            for port in base..base + PORT_COUNT {
                let Ok(port) = u16::try_from(port) else {
                    continue;
                };
                taken.insert(port);
                candidates.push(Candidate {
                    app: RemoteApp {
                        workspace_id: ws.id.clone(),
                        workspace: ws.name.clone(),
                        port,
                        source: "worker",
                        pane: Some(pane.name.clone()),
                        task: None,
                    },
                    task_id: pane.task_id.clone(),
                });
            }
        }
        for port in app_ports_of(ws) {
            if taken.contains(&port) {
                continue;
            }
            candidates.push(Candidate {
                app: RemoteApp {
                    workspace_id: ws.id.clone(),
                    workspace: ws.name.clone(),
                    port,
                    source: "custom",
                    pane: None,
                    task: None,
                },
                task_id: None,
            });
        }
    }
    candidates
}

/// A project by id or by name; the HQ is not one.
fn find_project<'a>(workspaces: &'a [Workspace], wanted: &Value) -> Result<&'a Workspace, String> {
    let wanted = wanted.as_str().map(str::trim).unwrap_or("");
    if wanted.is_empty() {
        return Err("Name the project, by id or by name.".into());
    }
    let projects = || workspaces.iter().filter(|ws| ws.hq != Some(true));
    projects()
        .find(|ws| ws.id == wanted)
        .or_else(|| projects().find(|ws| ws.name.trim().to_lowercase() == wanted.to_lowercase()))
        .ok_or_else(|| format!("No project \"{wanted}\" in Hangar."))
}

/// The titles of these tasks, from the boards of the workspaces that hold
/// them; a board that cannot be read names none.
async fn titles_of(
    workspaces: &[&Workspace],
    listed: &[&Candidate],
    sources: &dyn Sources,
) -> HashMap<String, String> {
    let wanted: HashSet<&str> = listed
        .iter()
        .filter_map(|candidate| candidate.task_id.as_deref())
        .collect();
    let holders = workspaces.iter().filter(|ws| {
        listed
            .iter()
            .any(|candidate| candidate.task_id.is_some() && candidate.app.workspace_id == ws.id)
    });
    let boards = iced::futures::future::join_all(holders.map(|ws| sources.tasks(&ws.cwd))).await;
    let mut titles = HashMap::new();
    for tasks in boards.into_iter().filter_map(Result::ok) {
        for (id, title) in tasks {
            if wanted.contains(id.as_str()) {
                titles.insert(id, title);
            }
        }
    }
    titles
}

/// Which of these ports listen, asked a batch at a time.
pub async fn listening(
    ports: &[u16],
    ask: impl Fn(Vec<u16>) -> crate::control::port::BoxFuture<Vec<u16>>,
) -> Vec<u16> {
    let batches = ports.chunks(PROBE_BATCH).map(|batch| ask(batch.to_vec()));
    iced::futures::future::join_all(batches)
        .await
        .into_iter()
        .flatten()
        .collect()
}

/// `remote_apps {workspace?}`: every project's apps a server listens on, or
/// one project's (by id or name). A worker's carries its pane, and its
/// task's title when its board has it.
pub async fn remote_apps(
    port: &Port,
    args: &Value,
    sources: &dyn Sources,
) -> Result<RemoteApps, String> {
    let snapshot = port.snapshot();
    let workspaces = &snapshot.state.workspaces;
    let asked: Vec<&Workspace> = match args.get("workspace") {
        None => workspaces.iter().collect(),
        Some(wanted) => vec![find_project(workspaces, wanted)?],
    };
    let candidates = candidates_of(&asked);
    if candidates.is_empty() {
        return Ok(RemoteApps { apps: Vec::new() });
    }
    let mut ports = Vec::new();
    for candidate in &candidates {
        if !ports.contains(&candidate.app.port) {
            ports.push(candidate.app.port);
        }
    }
    let open: HashSet<u16> = sources.listening(ports).await?.into_iter().collect();
    let listed: Vec<&Candidate> = candidates
        .iter()
        .filter(|candidate| open.contains(&candidate.app.port))
        .collect();
    let titles = titles_of(&asked, &listed, sources).await;
    Ok(RemoteApps {
        apps: listed
            .into_iter()
            .map(|candidate| {
                let mut app = candidate.app.clone();
                app.task = candidate
                    .task_id
                    .as_ref()
                    .and_then(|id| titles.get(id).cloned());
                app
            })
            .collect(),
    })
}
