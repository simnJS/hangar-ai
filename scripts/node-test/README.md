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
| `bench.sh [--runs N] [--samples N] [--lines N] [--connect-only] [--latency "0 20"] …` | what a remote pane costs: key echo, throughput, the node's memory, the time to connect (below) |

- Image: pinned Debian (dated tag + digest), openssh-server, git, bash, curl,
  ca-certificates, procps, iproute2; user `hangar`; key auth only, no root login, port 22.
- `HANGAR_NODE_TEST_NET_ADMIN=1`: the container gets `NET_ADMIN`, so that
  `tc qdisc add dev eth0 root netem delay 20ms` (as root, `docker exec`) can
  add latency to its link (`bench.sh` does).
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

## Benchmark: `bench.sh`

```
bash scripts/node-test/bench.sh                    # builds, 3 runs of each measure, medians
bash scripts/node-test/bench.sh --no-build --runs 5
bash scripts/node-test/bench.sh --node-bin old/hangar-node --app-bin old/hangar-ai   # a baseline
HANGAR_BENCH_OUT=after.json bash scripts/node-test/bench.sh   # the medians as JSON too
bash scripts/node-test/bench.sh --connect-only --latency "0 20"   # only the connect measures
```

Same needs and the same isolation as `e2e.sh` (every `HANGAR_*` dropped, its
own `mktemp -d` data root and endpoint file, its own container and port, the
app's process group and the container removed on exit). It builds
`hangar-node` in release, and `hangar-app` in release with debug assertions
(`target/bench-app/`, a profile of its own): optimized, so what is measured
is not a debug build's terminal, and with the assertions
`HANGAR_DEV_NODES_CONNECT` needs. The isolated app has a remote workspace
(pane `Remote` on node `n1`) and a local one (pane `Local`), both turned into
a plain `bash --norc` with a fixed prompt. The bench reaches the node's API
through an SSH tunnel of its own, opened with the app's options.

Measures (each `--runs` times, 3 by default; the report gives the median of
each number over the runs):

| Measure | What |
| --- | --- |
| `local-pty-echo` | a key into a bash on a local PTY (python's `pty`), until it echoes: the floor |
| `node-echo-ws` | a key sent as an `{"input"}` frame on the pane's stream (what the desktop does), until its echo comes back on it |
| `node-echo-post` | the same key through `POST /node/panes/{id}/input` |
| `desktop-echo-local-pane` / `-remote-pane` | `pane_send` of one key (no Enter), `pane_read` polled until the desktop's terminal shows it |
| `node-flood` | `seq 1 N` on the node's stream alone: seconds to the line after it, data and wire bytes, frames, bytes per frame, resets (output skipped by a stream that fell behind), hangar-node's CPU |
| `desktop-flood-local-pane` / `-remote-pane` | `seq 1 N` with `pane_send`, `pane_read` polled until the line after it shows; CPU of the app, of hangar-node and of the app's ssh tunnel over it |
| `node-rest-1-pane` / `-8-panes` | 10 s of nothing: hangar-node's RSS, threads and CPU (the desktop attached, its one pane, then 7 more spawned through the API), and the app's |
| `connect-cold-<L>ms` | the app started on its remote workspace, hangar-node never seen on the node (binary, state and daemon removed): `prompt_ms` until the remote pane shows text |
| `connect-warm-<L>ms` | the same with the binary there and its daemon running, the pane not (the app makes it) |
| `connect-reconnect-<L>ms` | the app quit and started again, everything left as it was (the pane's screen replayed) |
| `connect-local-<L>ms` | the app started on its local workspace instead: the floor |

The connect measures run once per `--latency` (milliseconds added with
`tc netem` on the container's `eth0`: what the node sends is delayed, so
each round trip grows by that much; default `0 20`), cold, warm, reconnect
and local `--runs` times each. Times are from the app's launch:
`endpoint_ms` (its control API up), `prompt_ms`, and from its
`HANGAR_NODES_TIMING=1` log (nodes/connect.rs) `begin_at_ms` (the
connection starts), each step's time (`probe_ms`, `upload_ms`, `start_ms`,
`tunnel_ms`, `handshake_ms`; older builds: `install_ms`, `config_ms`),
`connected_at_ms` and `stream_at_ms` (the pane's stream opened).
The app's SSH master sockets go to `HANGAR_NODES_SSH_DIR`, in the run's
folder (e2e.sh too).

Echo samples: `--samples` keys (200), median, p95, min and max per run; a
line is cleared with ctrl+c every 30 keys. `--lines` (2000000) for the
floods. CPU is `/proc/<pid>/stat`'s user + system time over the measure's
wall time (100 % is one core). Docker's loopback has no network latency:
what is measured is what Hangar adds, not the link.

Files: `bench.sh` (the run), `bench_helper.py` (stdlib, `python3 -I`: the
node's API and the control API, a WebSocket client, a local PTY, the report).

