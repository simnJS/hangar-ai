//! Nodes (experiment `nodes`, docs/native/nodes.md §4): Linux machines the
//! desktop reaches over SSH, where it installs, starts and keeps a tunnel to
//! `hangar-node`.
//!
//! The machines are `settings.nodes` (how to reach each). Their state is
//! this package's: one connection per node runs on tokio (`connect.rs`) and
//! reports through the inbox; what it reported is `Model.live.nodes`, which
//! the Settings › Nodes page draws and later packages read with [`client`].
//! Nothing connects on its own: the person presses Connect, and the
//! connection then holds until Disconnect, the node's removal, the
//! experiment turned off, or the app quitting, which kills the tunnels.

pub mod commands;
pub mod connect;
pub mod model;
mod page;
pub mod runner;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use iced::{Subscription, Task};

use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::settings::experiments;
use crate::state::types::NodeConfig;
use crate::state::{Model, Patch, Settings, StoreAction};

pub use model::{NodeClient, NodeStatus};
pub use page::settings_page;

use connect::{Setup, Timing, TunnelSlot, locked};
use model::{BinarySource, NODE_VERSION, target_problem};
use runner::{System, SystemTunnel, Tunnel};

/// The running connections, and the page's add form.
#[derive(Debug, Default)]
pub struct State {
    conns: HashMap<String, Conn>,
    /// Bumped for each connection started, so that what an old one still
    /// had in flight is told from the new one's.
    generation: u64,
    pub(crate) form: AddForm,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct AddForm {
    pub label: String,
    pub target: String,
    pub config: String,
    /// Sent once: its problem shows from then on.
    pub touched: bool,
}

/// One running connection.
struct Conn {
    generation: u64,
    task: tokio::task::JoinHandle<()>,
    tunnel: TunnelSlot<SystemTunnel>,
}

impl std::fmt::Debug for Conn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Conn")
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

impl Conn {
    /// The task stopped and its tunnel killed now, not when the task is
    /// next polled.
    fn stop(self) {
        self.task.abort();
        if let Some(mut tunnel) = locked(&self.tunnel).take() {
            tunnel.kill();
        }
    }
}

/// What other packages read: each node's status, by id. A node with no entry
/// is disconnected.
#[derive(Debug, Default)]
pub struct Live {
    pub statuses: HashMap<String, NodeStatus>,
}

impl Live {
    pub fn status(&self, id: &str) -> NodeStatus {
        self.statuses.get(id).cloned().unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub enum Msg {
    /// What a connection reported; dropped unless `generation` is the
    /// node's current one.
    Status {
        id: String,
        generation: u64,
        status: NodeStatus,
    },
    Connect(String),
    Disconnect(String),
    Remove(String),
    AddLabel(String),
    AddTarget(String),
    AddConfig(String),
    AddSubmit,
    /// Connections whose node left the settings, or whose experiment was
    /// turned off, are stopped (every second while any runs).
    Reconcile,
}

/// The experiment is on.
pub fn available(m: &Model) -> bool {
    experiments::enabled(m.settings(), experiments::NODES)
}

/// How to talk to node `node_id`, while it is connected: its API through the
/// tunnel and the token it asks for.
#[allow(dead_code)] // Read by the remote workspaces, which come next.
pub fn client(m: &Model, node_id: &str) -> Option<NodeClient> {
    match m.live.nodes.statuses.get(node_id)? {
        NodeStatus::Connected {
            local_port, token, ..
        } => Some(NodeClient {
            base_url: format!("http://127.0.0.1:{local_port}"),
            token: token.clone(),
        }),
        _ => None,
    }
}

/// Every tunnel killed: the app is quitting.
pub fn shutdown(st: &mut State) {
    for (_, conn) in st.conns.drain() {
        conn.stop();
    }
}

/// A status a connection reported, kept when it is the current one's.
fn apply_status(st: &State, live: &mut Live, id: String, generation: u64, status: NodeStatus) {
    if st.conns.get(&id).map(|conn| conn.generation) == Some(generation) {
        live.statuses.insert(id, status);
    }
}

fn disconnect(st: &mut State, live: &mut Live, id: &str) {
    if let Some(conn) = st.conns.remove(id) {
        conn.stop();
    }
    live.statuses.remove(id);
}

fn connect(st: &mut State, m: &mut Model, sv: &Services, id: &str) {
    let Some(node) = m.settings().nodes.iter().find(|n| n.id == id).cloned() else {
        return;
    };
    if !available(m) {
        return;
    }
    disconnect(st, &mut m.live.nodes, id);
    st.generation += 1;
    let generation = st.generation;
    let setup = Setup {
        node,
        source: BinarySource::from_env(),
        version: NODE_VERSION.to_string(),
        timing: Timing::default(),
    };
    let tunnel: TunnelSlot<SystemTunnel> = Arc::new(Mutex::new(None));
    let sender = sv.clone();
    let node_id = id.to_string();
    let report = move |status: NodeStatus| {
        sender.send(Message::Nodes(Msg::Status {
            id: node_id.clone(),
            generation,
            status,
        }));
    };
    let task = tokio::spawn(connect::run(System, setup, tunnel.clone(), report));
    st.conns.insert(
        id.to_string(),
        Conn {
            generation,
            task,
            tunnel,
        },
    );
    m.live
        .nodes
        .statuses
        .insert(id.to_string(), NodeStatus::Connecting(model::Step::Probe));
}

/// The node the add form describes, or `None` when its target does not do.
pub(crate) fn node_from_form(form: &AddForm, id: String) -> Option<NodeConfig> {
    if target_problem(&form.target).is_some() {
        return None;
    }
    let config = form.config.trim();
    Some(NodeConfig {
        id,
        label: form.label.trim().to_string(),
        ssh_target: form.target.trim().to_string(),
        ssh_config: (!config.is_empty()).then(|| config.to_string()),
        extra: Default::default(),
    })
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    match msg {
        Msg::Status {
            id,
            generation,
            status,
        } => apply_status(st, &mut m.live.nodes, id, generation, status),
        Msg::Connect(id) => connect(st, m, sv, &id),
        Msg::Disconnect(id) => disconnect(st, &mut m.live.nodes, &id),
        Msg::Remove(id) => {
            disconnect(st, &mut m.live.nodes, &id);
            m.apply(StoreAction::UpdateSettings(Patch::new(
                move |s: &mut Settings| s.nodes.retain(|node| node.id != id),
            )));
        }
        Msg::AddLabel(value) => st.form.label = value,
        Msg::AddTarget(value) => st.form.target = value,
        Msg::AddConfig(value) => st.form.config = value,
        Msg::AddSubmit => {
            st.form.touched = true;
            if let Some(node) = node_from_form(&st.form, crate::state::store::new_id()) {
                m.apply(StoreAction::UpdateSettings(Patch::new(
                    move |s: &mut Settings| s.nodes.push(node.clone()),
                )));
                st.form = AddForm::default();
            }
        }
        Msg::Reconcile => reconcile(st, m),
    }
    Task::none()
}

fn reconcile(st: &mut State, m: &mut Model) {
    let on = available(m);
    let gone: Vec<String> = st
        .conns
        .keys()
        .filter(|id| !on || !m.settings().nodes.iter().any(|node| &node.id == *id))
        .cloned()
        .collect();
    for id in gone {
        disconnect(st, &mut m.live.nodes, &id);
    }
}

pub fn subscription(st: &State, _m: &Model) -> Subscription<Message> {
    if st.conns.is_empty() {
        return Subscription::none();
    }
    iced::time::every(Duration::from_secs(1)).map(|_| Message::Nodes(Msg::Reconcile))
}

pub fn on_intent(
    _st: &mut State,
    _m: &mut Model,
    _sv: &Services,
    _intent: &Intent,
) -> Option<Task<Message>> {
    None
}

pub fn on_event(
    _st: &mut State,
    _m: &mut Model,
    _sv: &Services,
    _event: &CoreEvent,
) -> Option<Task<Message>> {
    None
}
