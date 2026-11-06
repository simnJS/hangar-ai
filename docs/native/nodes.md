# Hangar nodes (experimental)

A *node* is a headless Hangar running on another Linux machine (a server, a VPS,
a GPU box, a Raspberry Pi), driven from the desktop Hangar over SSH. Everything
here sits behind the `nodes` experiment.

The person's decisions: first, whole workspaces that live on a node; then,
workers sent from a local workspace to a node, on the same base. The desktop
reaches a node over SSH (its keys, nothing to open on the server). Nodes run on
Linux x86_64 and arm64.

## What runs where

| | Desktop | Node |
|---|---|---|
| Window, panes' terminals (`hangar-term`), chat view | yes | no |
| PTYs: shells, Claude Code, builds | local workspaces | workspaces on the node |
| Board, memory, MCP server, deliveries | local workspaces | workspaces on the node |
| Transcripts (`~/.claude`) | local | on the node |

A workspace on a node is drawn by the desktop exactly like a local one. Its
terminals are fed by a byte stream from the node instead of a local PTY; its
board and memory are read from the node's API instead of the local disk. Close
the laptop: the node keeps running, and the desktop catches up from the PTY
history when it reconnects.

## Pieces

1. **`hangar-core`, headless.** The desktop-only parts of core (voice with
   whisper/cpal, Discord, notifications, autostart, the phone server) move
   behind a default `desktop` feature. `headless::start` runs what a node
   needs: the PTY manager, the board and memory stores, the HTTP API on
   loopback with its tokens, the bridge and deliveries.
2. **`hangar-node`** (new crate, binary `hangar-node`). `serve` starts the
   headless core plus the *node API* on 127.0.0.1 and writes
   `~/.local/state/hangar-node/node.json` (port, token, version, pid); `--mcp`
   is the MCP stdio proxy agents on the node run, as `hangar-ai --mcp` does on
   the desktop; `version` prints the version and target. Linux only.
3. **Node API** (HTTP + WebSocket, Bearer token, loopback only, reached through
   the SSH tunnel):
   - `GET /node/info`: version, target, hostname, home, whether `claude` and
     `git` are on PATH;
   - `POST /node/panes` spawn (id, cwd, shell, size, agent, env bits),
     `POST /node/panes/{id}/input`, `/resize`, `DELETE /node/panes/{id}`,
     `GET /node/panes` (live panes);
   - `GET /node/panes/{id}/stream?from=<seq>` (WebSocket): the history after
     `seq`, then live output frames `{seq, data}` and `{exit}`;
   - `GET /node/events` (WebSocket): the node's core events (board changed,
     memory changed, bridge events) forwarded as JSON;
   - `GET /node/fs?path=` list a folder (to pick a workspace's folder);
   - the existing `/api/board`, `/api/tasks/...`, `/api/memory/...` routes of
     core's server, with the App token.
4. **Desktop: connection** (`hangar-app/src/nodes/`). A node is
   `{id, label, ssh target}` in the settings. Connecting runs, with
   `ssh -o BatchMode=yes`: `uname -m` → picks the binary for the target;
   installs or updates `hangar-node` under `~/.local/share/hangar-node/bin`
   when the version differs (the binary comes from beside `hangar-ai`, or
   `HANGAR_NODE_BINARY` for development); starts `hangar-node serve` detached
   (a `systemd-run --user` unit when available, else `setsid nohup`); reads
   `node.json`; opens `ssh -N -L 127.0.0.1:<local>:127.0.0.1:<port>` and keeps
   it up, reconnecting with backoff. Status per node: connecting, connected,
   unreachable (with the reason).
5. **Desktop: remote workspaces.** `Workspace.node: Option<NodeId>`; its `cwd`
   is a path on the node. `PaneLink` and the pane controller talk to a
   `PtyBackend` (local: today's `PtyManager`; remote: the node API), so the
   terminal, the hub and the frame logic stay as they are. The board and
   memory views read and write through the node API for such a workspace and
   refresh on the node's forwarded events.
6. **Test rig.** A Docker image (Debian, openssh-server, git, a test user with
   a key made for the tests) and an end-to-end test: build `hangar-node`,
   start the container, connect over SSH, start the node, open the tunnel,
   spawn a shell, type `echo`, read it back, kill it, reconnect and replay
   the history.

## Later (phase 2 and on)

- Orchestration on a node (workers in worktrees there), the chat view of a
  remote workspace, previews and kits of remote files.
- Workers sent from a local workspace to a node: the worker clones the branch
  on the node and delivers it back.
- arm64 builds published with releases; Tailscale as a second transport.
