//! `board_relay`: the board calls of a worker a local workspace sent to a
//! node (nodes/away.rs). Its MCP proxy runs on the node, whose board is not
//! its task's: with `HANGAR_BOARD_RELAY` set in its shell, the proxy sends
//! each board call as this operation instead (hangar-core's mcp.rs), the
//! node relays it here like any control request, and it is made to this
//! machine's own API as the worker's pane, on its workspace's board — the
//! signatures, claims and checks of the board's routes, unchanged.
//!
//! Only the board's routes, and only for such a worker.

use serde_json::Value;

use crate::state::Workspace;

use super::port::Port;
use super::types::Caller;

/// The routes a relayed call may name.
const ROUTES: [&str; 3] = ["/api/board", "/api/next", "/api/tasks"];

/// `path` is one of the board's routes, and names no board of its own.
fn allowed(path: &str) -> bool {
    let route = path.split('?').next().unwrap_or_default();
    ROUTES
        .iter()
        .any(|prefix| route == *prefix || route.starts_with(&format!("{prefix}/")))
        && !path.contains("cwd=")
        && !path.contains("..")
}

pub async fn control(
    ws: &Workspace,
    args: &Value,
    caller: &Caller,
    port: &Port,
) -> Result<Value, String> {
    let pane = caller
        .pane_id
        .as_deref()
        .and_then(|id| ws.panes.iter().find(|pane| pane.id == id))
        .filter(|pane| crate::nodes::away::pane_node(pane).is_some());
    let Some(pane) = pane else {
        return Err(
            "board_relay carries the board calls of a worker sent to a node from a workspace of this computer, and you are not one. Use the board tools."
                .into(),
        );
    };
    let request = args.get("request").cloned().unwrap_or(Value::Null);
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .filter(|method| matches!(*method, "GET" | "POST" | "PATCH" | "DELETE"));
    let path = request
        .get("path")
        .and_then(Value::as_str)
        .filter(|path| allowed(path));
    let (Some(method), Some(path)) = (method, path) else {
        return Err("board_relay takes a board call: GET, POST, PATCH or DELETE on /api/board, /api/next or /api/tasks.".into());
    };
    let body = request.get("body").cloned().filter(|body| !body.is_null());
    port.board_request(&pane.id, &pane.name, &ws.cwd, method, path, body)
        .await
}

#[cfg(test)]
mod tests {
    use super::allowed;

    #[test]
    fn only_the_board_s_routes_are_relayed() {
        for route in [
            "/api/board",
            "/api/next",
            "/api/tasks",
            "/api/tasks/t1/comment",
        ] {
            assert!(allowed(route), "{route}");
        }
        for refused in [
            "/api/memory",
            "/api/control/worker_spawn",
            "/api/boardx",
            "/api/board?cwd=/etc",
            "/api/tasks/../memory",
        ] {
            assert!(!allowed(refused), "{refused}");
        }
    }
}
