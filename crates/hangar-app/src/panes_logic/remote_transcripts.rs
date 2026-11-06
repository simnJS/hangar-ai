//! The transcripts of a Claude Code pane of a remote workspace, which are on
//! its node: the session hunt's listing and the context gauge's reading,
//! asked of the node (`nodes::remote::{sessions, context}`) where a local
//! pane reads this machine's disk. Same cadence, same answers: a hunt round
//! that fails finds nothing, as a local listing that fails does.
//!
//! The panes' controller follows the sessions of local panes only (it lists
//! the known ones before the launch from this machine's disk): a remote
//! pane's hunt starts here instead, when its agent's command is typed
//! (`Launched`), with the sessions its node already holds as the known ones.

use std::collections::HashSet;

use hangar_core::context::ContextUsage;
use hangar_core::sessions::AgentSession;

use crate::nodes::remote;

/// A round of the session hunt for a pane in `cwd` on node `node`: the
/// directory's listing, no label read. `None` when the node could not say.
pub async fn hunt_listing(node: String, cwd: String) -> Option<Vec<AgentSession>> {
    let client = remote::client_of(&node).ok()?;
    remote::sessions(&client, &cwd, false).await.ok()
}

/// The sessions node `node` already holds for `cwd`, so that a brand new one
/// stands out to the hunt. Empty when it cannot be listed, as locally.
pub async fn known_session_ids(node: String, cwd: String) -> HashSet<String> {
    hunt_listing(node, cwd)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|session| session.id)
        .collect()
}

/// The context of session `session` in `cwd`, read on node `node`. `Err`
/// when the node did not answer: the gauge keeps its last reading rather
/// than drop to nothing while the tunnel comes back.
pub async fn context_usage(
    node: String,
    session: String,
    cwd: String,
) -> Result<Option<ContextUsage>, String> {
    let client = remote::client_of(&node)?;
    remote::context(&client, &session, &cwd).await
}
