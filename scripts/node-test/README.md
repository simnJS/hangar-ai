# Nodes test rig

A Docker container with sshd, reachable on `127.0.0.1`, to test hangar-node end
to end without touching `~/.ssh` or a real server. Everything it needs lives here:
a dedicated key pair and known_hosts in `.keys/` (gitignored).

| Script | What it does |
| --- | --- |
| `up.sh [--arm64] [--with-claude]` | generates the key once, builds the image, runs container `hangar-node-test`, waits for ssh, prints the target and options |
| `down.sh` | removes the container (keeps the keys) |
| `ssh.sh <cmd>` | runs a command on the node with the right options |
| `check.sh [--arm64]` | smoke test: up, `uname -m`, `whoami`, scp round trip, password/root refused, down |
| `e2e.sh [--no-build] [--keep]` | the whole node flow, end to end, against an isolated debug Hangar (below) |

- Image: pinned Debian (dated tag + digest), openssh-server, git, bash, curl,
  ca-certificates, procps; user `hangar`; key auth only, no root login, port 22.
- Port: `HANGAR_NODE_TEST_PORT`, else the first free one from 2222. Container
  name: `HANGAR_NODE_TEST_CONTAINER` (default `hangar-node-test`).
- `--with-claude` adds Node.js and `@anthropic-ai/claude-code` (off by default;
  no Claude login happens in tests).
- `--arm64` runs a `linux/arm64` container through qemu-user-binfmt. If this
  machine cannot run arm64 containers it says so and exits 0 without starting anything.
- One container per name: `up.sh` replaces a running one of the same name.
- `HANGAR_NODE_TEST_STATE_DIR`: where `port`, `known_hosts` and `ssh_config`
  go instead of `.keys/` and this folder (the key stays in `.keys/`), so a run
  with its own container never clobbers the files of a rig started by hand.

## Pointing code at it

`up.sh` writes `scripts/node-test/ssh_config` (gitignored, absolute paths, current port):

```
ssh -F scripts/node-test/ssh_config hangar-node-test
```

or the plain form `up.sh` prints:

```
ssh -i scripts/node-test/.keys/id_ed25519 -p <port> -o StrictHostKeyChecking=no \
    -o UserKnownHostsFile=scripts/node-test/.keys/known_hosts hangar@127.0.0.1
```

The port is also in `scripts/node-test/.keys/port` while the rig is up.
`down.sh` removes `ssh_config` and `port`.

## End-to-end test: `e2e.sh`

```
bash scripts/node-test/e2e.sh            # builds, runs, cleans up; exit 1 on any failure
bash scripts/node-test/e2e.sh --no-build # binaries already built
bash scripts/node-test/e2e.sh --keep     # keep the run's folder (app.log, state, repos)
```

Needs Docker, a display (the desktop app opens a window: `DISPLAY` or
`WAYLAND_DISPLAY`), `python3`, `git`, `cargo`. A run takes under a minute
once built. Not in CI (Docker, a display, minutes).

What it does, printing `PASS`/`FAIL` per step:

1. `cargo build -p hangar-node --release` on its own (built together with
   hangar-app, the features would unify), then a debug `hangar-app`.
2. Starts its own container (`HANGAR_NODE_TEST_CONTAINER`, default
   `hangar-node-e2e-<pid>`) on its own port (`HANGAR_NODE_TEST_PORT`, else the
   first free one of `HANGAR_PORT_BASE`.. when set, else from 22200). Seeds the
   node with a git repo (`/home/hangar/demo`) and a handcrafted Claude Code
   transcript, and an isolated state with node `n1` and two workspaces: `E2E
   remote` on the node (a `Shell` pane and an orchestrator shell `Rorch`) and
   `E2E local`, a git repo in the run's folder (orchestrator shell `Lorch`,
   check `grep -q done work.sh`, automatic landing). Starts the debug app with
   `HANGAR_DEV_NODES_CONNECT=1` and `HANGAR_NODE_BINARY`.
3. Checks:
   - the remote `Shell` pane echoes a command, with the node's hostname
     (`pane_send` / `pane_read` on the control API);
   - reconnect: the app is restarted, the pane shows the replayed history and
     a variable exported before the restart is still set (same shell);
   - memory: an entry written through the node's `/api/memory` is read whole
     by `/node/memory`;
   - transcripts: `/node/chat`, `/node/context` and `/node/sessions` read the
     handcrafted transcript;
   - board + orchestration in the remote workspace: a task made through the
     node's API; `worker_spawn` called as `Rorch` (its MCP proxy, `hangar-node
     --mcp`, run on the node with that pane's environment) — the desktop
     claims the node's task (doing, worker `Rwork` on the node's board), the
     worktree exists on the node, the desktop lists the worker pane, its shell
     runs on the node; `worker_stop` removes the worktree and gives the task
     back (todo);
   - a local workspace sends a worker to the node: `Lorch` (through
     `hangar-ai --mcp` with that pane's environment and folder) creates a task
     and calls `worker_spawn node=n1`; the worker's shell runs on the node in
     its worktree and nowhere under the app; a commit made there, then
     `delivery_submit` as that worker (on the node), comes back: the commit is
     on the local `main`, the task is done, the worktree on the node is gone.
4. Cleans up: the app's process group, the container, the run's folder (all
   on exit, failure or Ctrl-C included).

Isolation, so a run can never reach the person's Hangar:

- every `HANGAR_*` variable of the caller is dropped first (`HANGAR_BRIDGE_*`,
  `HANGAR_API_*`, a pane's id and name…);
- `HANGAR_DATA_ROOT` and `HANGAR_ENDPOINT_FILE` point into a `mktemp -d`
  folder, and the script refuses to run if the endpoint file is not under it;
  the helper refuses any endpoint, pane environment or pane folder outside it;
- `XDG_DATA_HOME`/`XDG_CACHE_HOME`/`XDG_STATE_HOME` point there too (the
  delivery's check worktree lands there, not in `~/.local/share/hangar/c`);
- the agents' MCP proxies run in their pane's folder with `HANGAR_WORKSPACE`
  set to it: run from a checkout, a proxy would take that checkout's board;
- the script works from the run's folder, never from the checkout.

The review of the delivery is the one part not exercised for real: `claude`
on the app's PATH is a script that answers a fixed approval to `claude -p`
(so no Claude account is spent and the run is repeatable) and refuses any
other `claude` on this computer. Everything else — push to the node, worktree
and pane there, board relay, fetch back, check, landing — is the real code.

The app's own servers (its API, the bridge, the SSH tunnel's local end) bind
loopback ports the app picks itself; the rig's port is the only one chosen
here.

Files: `e2e.sh` (the steps), `e2e_helper.py` (desktop side, stdlib, run with
`python3 -I`: control API, MCP calls as a pane, a pane's environment from
`/proc`), `e2e_node.sh` (copied to the node: its API with `node.json`'s token,
MCP calls as a pane there).
