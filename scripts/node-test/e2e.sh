#!/usr/bin/env bash
# End-to-end test of the whole node flow on the Docker rig: builds hangar-node
# and a debug hangar-app, starts a container of its own, seeds an isolated
# Hangar with a node and two workspaces (one on the node, one local), and
# drives it through the control API and the agents' MCP proxies.
#   e2e.sh [--no-build] [--keep]
#     --no-build  use the binaries already in the target folder
#     --keep      keep the run's temporary folder (logs, state) for a look
# Container: HANGAR_NODE_TEST_CONTAINER (default hangar-node-e2e-<pid>).
# Port: HANGAR_NODE_TEST_PORT, else the first free one of HANGAR_PORT_BASE ..
# +HANGAR_PORT_COUNT-1 when set, else the first free one from 22200.
# Prints PASS/FAIL per step, cleans up after itself, exits 1 on any failure.
set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"

build=1
keep=0
for a in "$@"; do
  case "$a" in
    --no-build) build=0 ;;
    --keep) keep=1 ;;
    -h|--help) sed -n '2,12p' "$0"; exit 0 ;;
    *) echo "unknown argument: $a" >&2; exit 2 ;;
  esac
done

# --- Isolation ---------------------------------------------------------------
# What is read from the caller's environment before every HANGAR_* variable
# is dropped: nothing this run starts may find the person's Hangar (its
# bridge, its API, its endpoint file).
port_base="${HANGAR_PORT_BASE:-}"
port_count="${HANGAR_PORT_COUNT:-20}"
rig_port="${HANGAR_NODE_TEST_PORT:-}"
container="${HANGAR_NODE_TEST_CONTAINER:-hangar-node-e2e-$$}"
outer_bridge="${HANGAR_BRIDGE_URL:-}"
for name in $(compgen -e); do
  case "$name" in HANGAR_*) unset "$name" ;; esac
done

work="$(mktemp -d "${TMPDIR:-/tmp}/hangar-node-e2e.XXXXXX")"
work="$(cd "$work" && pwd -P)"
export HANGAR_DATA_ROOT="$work/home"
export HANGAR_ENDPOINT_FILE="$work/home/hangar/endpoint.json"
# Delivery check worktrees and other per-user folders of the app, kept in the run.
export XDG_DATA_HOME="$work/xdg/data" XDG_CACHE_HOME="$work/xdg/cache" XDG_STATE_HOME="$work/xdg/state"
export HANGAR_NODE_TEST_CONTAINER="$container"
export HANGAR_NODE_TEST_STATE_DIR="$work/rig"
export GIT_AUTHOR_NAME="Hangar E2E" GIT_AUTHOR_EMAIL="e2e@hangar.invalid"
export GIT_COMMITTER_NAME="Hangar E2E" GIT_COMMITTER_EMAIL="e2e@hangar.invalid"
mkdir -p "$HANGAR_DATA_ROOT/data" "$HANGAR_DATA_ROOT/hangar" "$XDG_DATA_HOME" "$XDG_CACHE_HOME" "$XDG_STATE_HOME" "$work/bin"
case "$(realpath -m "$HANGAR_ENDPOINT_FILE")" in
  "$work"/*) ;;
  *) echo "refusing to run: HANGAR_ENDPOINT_FILE ($HANGAR_ENDPOINT_FILE) is not under $work" >&2; rm -rf "$work"; exit 2 ;;
esac

# Nothing runs from the caller's checkout: a tool that takes its folder as
# its workspace would find the person's board there.
cd "$work" || exit 2

# shellcheck source=lib.sh
. "$here/lib.sh"

# --- Bookkeeping -------------------------------------------------------------
results=()
failed=0
app_pid=""

note() { printf '      %s\n' "$@"; }
pass() { printf 'PASS  %s\n' "$1"; results+=("PASS  $1"); }
fail() {
  printf 'FAIL  %s\n' "$1"; results+=("FAIL  $1"); failed=1
  if [ -s "$work/step.log" ]; then tail -n 25 "$work/step.log" | sed 's/^/      | /'; fi
}
# step <name> <function...>: runs it in this shell (its variables stay),
# output to step.log, shown only on failure.
step() {
  local name="$1"; shift
  : > "$work/step.log"
  if "$@" > "$work/step.log" 2>&1; then pass "$name"; return 0; fi
  fail "$name"; return 1
}
# The steps that cannot run once something they need failed.
skip() { printf 'FAIL  %s (not run: %s)\n' "$1" "$2"; results+=("FAIL  $1 (not run)"); failed=1; }

stop_app() {
  [ -n "$app_pid" ] || return 0
  kill -TERM -- "-$app_pid" 2>/dev/null || true
  for _ in $(seq 1 40); do kill -0 "$app_pid" 2>/dev/null || break; sleep 0.25; done
  kill -KILL -- "-$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=""
}

cleanup() {
  local code=$?
  stop_app
  "$here/down.sh" >/dev/null 2>&1 || true
  if [ "$keep" = 1 ]; then echo "kept: $work"; else rm -rf "$work"; fi
  exit "$code"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

# --- Helpers -----------------------------------------------------------------
helper() { python3 -I "$here/e2e_helper.py" "$@"; }
ctl() { helper control --root "$work" --endpoint "$HANGAR_ENDPOINT_FILE" "$@"; }
wait_pane() { helper wait-pane --root "$work" --endpoint "$HANGAR_ENDPOINT_FILE" "$@"; }
mcp_as() { helper mcp --root "$work" --endpoint "$HANGAR_ENDPOINT_FILE" --app "$app_bin" "$@"; }
jget() { helper get "$1"; }
# A command on the node, its arguments quoted for the remote shell.
on_node() { "$here/ssh.sh" "$(printf '%q ' "$@")"; }
node_helper() { on_node bash /home/hangar/e2e_node.sh "$@"; }
# retry <secs> <command...>: until it succeeds.
retry() {
  local secs="$1"; shift
  local deadline=$((SECONDS + secs))
  until "$@"; do
    [ "$SECONDS" -lt "$deadline" ] || return 1
    sleep 1
  done
}

remote_ws=w-remote
remote_cwd=/home/hangar/demo
local_ws=w-local
session_id=7d0c1e2f-3a4b-4c5d-8e9f-0a1b2c3d4e5f
mark="e2e$$x$RANDOM"

# --- 1. Builds ---------------------------------------------------------------
target_dir() {
  cargo metadata --manifest-path "$root/Cargo.toml" --format-version 1 --no-deps 2>/dev/null \
    | jget target_directory
}
build_node() { cargo build --manifest-path "$root/Cargo.toml" -p hangar-node --release --locked; }
build_app() { cargo build --manifest-path "$root/Cargo.toml" -p hangar-app --locked; }

target="$(target_dir)"
node_bin="$target/release/hangar-node"
app_bin="$target/debug/hangar-ai"
if [ "$build" = 1 ]; then
  # Apart: built together, hangar-app's features would unify into hangar-node.
  step "build hangar-node (release, on its own)" build_node || exit 1
  step "build hangar-app (debug)" build_app || exit 1
fi
for bin in "$node_bin" "$app_bin"; do
  [ -x "$bin" ] || { printf 'FAIL  %s is missing (build it, or drop --no-build)\n' "$bin"; exit 1; }
done
if [ -z "${DISPLAY:-}" ] && [ -z "${WAYLAND_DISPLAY:-}" ]; then
  echo "FAIL  no display: the desktop app needs DISPLAY or WAYLAND_DISPLAY"; exit 1
fi

# --- 2. Rig, seeds, app ------------------------------------------------------
pick_port() {
  [ -n "$rig_port" ] && return 0
  local from=22200 count=100 p
  if [ -n "$port_base" ]; then from="$port_base"; count="$port_count"; fi
  for p in $(seq "$from" $((from + count - 1))); do
    if port_is_free "$p"; then rig_port="$p"; return 0; fi
  done
  echo "no free port in $from..$((from + count - 1))" >&2; return 1
}
rig_up() {
  pick_port || return 1
  HANGAR_NODE_TEST_PORT="$rig_port" "$here/up.sh" || return 1
  [ -r "$STATE_PORT" ] || { echo "the rig did not start"; return 1; }
  node_host="$(docker exec "$CONTAINER" hostname)"
  echo "container $CONTAINER ($node_host) on port $rig_port"
}
step "rig up (container $container)" rig_up || exit 1

seed_node() {
  "$here/ssh.sh" 'cat > /home/hangar/e2e_node.sh' < "$here/e2e_node.sh" || return 1
  "$here/ssh.sh" "set -e
    git init -q -b main $remote_cwd
    cd $remote_cwd
    echo 'Remote workspace of the nodes e2e test.' > README
    git add README
    git -c user.name=E2E -c user.email=e2e@node commit -qm 'Initial commit'
    folder=\$(printf %s $remote_cwd | sed 's/[^A-Za-z0-9]/-/g')
    mkdir -p ~/.claude/projects/\$folder
    cat > ~/.claude/projects/\$folder/$session_id.jsonl" <<JSONL
{"parentUuid":null,"isSidechain":false,"type":"user","uuid":"u1","timestamp":"2026-11-06T05:00:00.000Z","cwd":"$remote_cwd","sessionId":"$session_id","version":"2.9.0","message":{"role":"user","content":"what is on this node?"},"origin":{"kind":"human"},"promptSource":"typed"}
{"parentUuid":"u1","isSidechain":false,"type":"assistant","uuid":"a1","timestamp":"2026-11-06T05:00:05.000Z","cwd":"$remote_cwd","sessionId":"$session_id","version":"2.9.0","message":{"id":"msg_a1","model":"claude-opus-5-5","role":"assistant","content":[{"type":"text","text":"A demo repository, $mark."}],"usage":{"input_tokens":3,"cache_creation_input_tokens":2000,"cache_read_input_tokens":40000,"output_tokens":120}},"requestId":"req_1"}
JSONL
}

seed_local() {
  git init -q -b main "$work/repo" || return 1
  echo 'Local workspace of the nodes e2e test.' > "$work/repo/README"
  git -C "$work/repo" add README && git -C "$work/repo" commit -qm 'Initial commit' || return 1
  # The review of a delivery is a `claude -p` run. This one answers a fixed
  # approval so no Claude account is spent and the run is repeatable; any
  # other claude started on this computer (an interactive one) is refused:
  # the workers of this test run on the node.
  cat > "$work/bin/claude" <<'SH'
#!/bin/sh
for a in "$@"; do
  case "$a" in
    -p|--print)
      cat > /dev/null
      echo '{"type":"result","is_error":false,"result":"","structured_output":{"approve":true,"blocking":[],"notes":"nodes e2e: fixed approval, no review run"}}'
      exit 0 ;;
  esac
done
echo "nodes e2e: no interactive claude on this computer" >&2
exit 1
SH
  chmod +x "$work/bin/claude"
  cat > "$HANGAR_DATA_ROOT/data/state.json" <<JSON
{
  "activeWorkspaceId": "$remote_ws",
  "workspaces": [
    {
      "id": "$remote_ws", "name": "E2E remote", "cwd": "$remote_cwd", "node": "n1",
      "panes": [
        {"id": "p-rshell", "name": "Shell", "agent": "shell"},
        {"id": "p-rorch", "name": "Rorch", "agent": "shell", "role": "orchestrator"}
      ],
      "orchestration": {"autoMerge": true, "checkCommand": "true", "workerModel": "haiku", "reviewerModel": "haiku"},
      "savedCommands": []
    },
    {
      "id": "$local_ws", "name": "E2E local", "cwd": "$work/repo",
      "panes": [{"id": "p-lorch", "name": "Lorch", "agent": "shell", "role": "orchestrator"}],
      "orchestration": {"autoMerge": true, "checkCommand": "grep -q done work.sh", "workerModel": "haiku", "reviewerModel": "haiku"},
      "savedCommands": []
    }
  ],
  "settings": {
    "experiments": ["nodes"],
    "agentPaneControl": true,
    "claudeBridge": true,
    "nodes": [{"id": "n1", "label": "E2E rig", "sshTarget": "$HOST_ALIAS", "sshConfig": "$SSH_CONFIG"}]
  }
}
JSON
}

start_app() {
  rm -f "$HANGAR_ENDPOINT_FILE"
  HANGAR_NODE_BINARY="$node_bin" HANGAR_DEV_NODES_CONNECT=1 PATH="$work/bin:$PATH" \
    setsid "$app_bin" >> "$work/app.log" 2>&1 < /dev/null &
  app_pid=$!
  retry 90 test -s "$HANGAR_ENDPOINT_FILE" || { echo "no endpoint after 90 s"; tail -n 20 "$work/app.log"; return 1; }
  local bridge
  bridge="$(helper get port < "$HANGAR_ENDPOINT_FILE")"
  echo "isolated instance: pid $app_pid, API port $bridge"
}

step "seed the node (git repo, transcript)" seed_node || exit 1
step "seed the isolated state (node n1, remote + local workspace)" seed_local || exit 1
step "start the isolated app (HANGAR_DEV_NODES_CONNECT=1)" start_app || exit 1

# --- 3. Checks ---------------------------------------------------------------
# A remote shell pane echoes a command (the node's hostname proves where).
send() { ctl pane_send "{\"workspace\":\"$1\",\"pane\":\"$2\",\"text\":$(printf '%s' "$3" | python3 -I -c 'import json,sys;print(json.dumps(sys.stdin.read()))')}"; }
remote_echo() {
  local tries
  for tries in 1 2 3 4 5 6 7 8 9 10 11 12; do
    send "$remote_ws" Shell 'echo e2e-shell-$((6*7))-$(hostname)' >/dev/null 2>&1 &&
      wait_pane "$remote_ws" Shell "e2e-shell-42-$node_host" 10 >/dev/null 2>&1 && {
        echo "the Shell pane printed e2e-shell-42-$node_host (try $tries)"; return 0; }
  done
  wait_pane "$remote_ws" Shell "e2e-shell-42-$node_host" 1
}
step "remote shell pane echoes a command on the node" remote_echo && shell_ok=1 || shell_ok=0

# Reconnect: the app restarts, the shell is still the same one on the node and
# its history is replayed into the pane.
reconnect() {
  send "$remote_ws" Shell "export E2E_MARK=$mark" || return 1
  sleep 1
  stop_app
  start_app || return 1
  wait_pane "$remote_ws" Shell "e2e-shell-42-$node_host" 90 > /dev/null || { echo "history not replayed"; return 1; }
  echo "history replayed after the restart"
  send "$remote_ws" Shell 'echo same-shell-$E2E_MARK' || return 1
  wait_pane "$remote_ws" Shell "same-shell-$mark" 30 > /dev/null || { echo "not the same shell"; return 1; }
  echo "same shell: E2E_MARK kept ($mark)"
}
if [ "$shell_ok" = 1 ]; then step "reconnect replays the history (same shell on the node)" reconnect
else skip "reconnect replays the history" "no remote shell"; fi

# The memory route: an entry written through the node's /api/memory, read
# whole by /node/memory.
memory() {
  node_helper api POST "/api/memory?cwd=$remote_cwd" \
    "{\"title\":\"E2E note\",\"content\":\"memory-$mark\",\"tags\":[\"e2e\"]}" || return 1
  echo
  node_helper api GET /node/memory | tee "$work/memory.json" | grep -q "memory-$mark" || return 1
  echo "/node/memory holds memory-$mark"
}
step "memory route (/api/memory write, /node/memory read)" memory

# A handcrafted transcript read by /node/chat (and /node/context, /node/sessions).
chat() {
  local page
  page="$(node_helper api GET "/node/chat?session=$session_id&cwd=$remote_cwd")" || return 1
  echo "$page" | grep -q "A demo repository, $mark." || { echo "chat: $page"; return 1; }
  echo "/node/chat: the assistant's line is there"
  node_helper api GET "/node/context?session=$session_id&cwd=$remote_cwd" | grep -q '"usedTokens"' || return 1
  echo "/node/context: usage read"
  node_helper api GET "/node/sessions?cwd=$remote_cwd&labels=1" | grep -q "what is on this node?" || return 1
  echo "/node/sessions: the session and its label"
}
step "transcript read by /node/chat" chat

# The board of the remote workspace: a task created on the node through its
# API is found by the desktop's engine, which claims it for a worker.
remote_task=""
remote_worktree=""
make_remote_task() {
  local made
  made="$(node_helper api POST "/api/tasks?cwd=$remote_cwd" \
    '{"title":"E2E remote job","description":"Made on the node by e2e.sh.","owns":["notes/"]}')" || return 1
  remote_task="$(echo "$made" | jget task.id)"
  [ -n "$remote_task" ] && [ "$remote_task" != null ] || { echo "no id in $made"; return 1; }
  echo "task $remote_task made on the node"
}
remote_spawn() {
  retry 60 node_helper pane-pid HANGAR_PANE_ID=p-rorch >/dev/null || { echo "no orchestrator shell on the node"; return 1; }
  node_helper pane-mcp HANGAR_PANE_ID=p-rorch worker_spawn "{\"task\":\"$remote_task\",\"name\":\"Rwork\"}" 150 \
    | tee "$work/rspawn.json" || return 1
}
remote_board() {
  local board
  board="$(node_helper api GET "/api/board?cwd=$remote_cwd")" || return 1
  echo "$board" | python3 -I -c '
import json, sys
board = json.load(sys.stdin)
task = next(t for t in board["tasks"] if t["id"] == sys.argv[1])
print("on the node:", task["column"], task.get("assignee"))
sys.exit(0 if task["column"] == sys.argv[2] and (sys.argv[3] == "-" or task.get("assignee") == sys.argv[3]) else 1)
' "$remote_task" "$1" "$2"
}
remote_worktree_made() {
  remote_worktree="$(on_node git -C "$remote_cwd" worktree list --porcelain | sed -n 's/^worktree //p' | grep -vx "$remote_cwd" | head -1)"
  [ -n "$remote_worktree" ] || { echo "no worktree on the node"; return 1; }
  on_node test -d "$remote_worktree" || return 1
  echo "worktree on the node: $remote_worktree"
  ctl pane_list "{\"workspace\":\"$remote_ws\"}" | tee "$work/rpanes.json" | grep -q '"Rwork"' || { echo "Rwork not in the desktop's panes"; return 1; }
  echo "the desktop lists the worker pane Rwork"
  node_helper pane-pid HANGAR_PANE_NAME=Rwork > /dev/null || { echo "Rwork has no shell on the node"; return 1; }
  echo "Rwork's shell runs on the node"
}
remote_stop() {
  node_helper pane-mcp HANGAR_PANE_ID=p-rorch worker_stop '{"worker":"Rwork"}' 120 || return 1
  echo
  retry 30 on_node test ! -e "$remote_worktree" || { echo "$remote_worktree still there"; return 1; }
  echo "worktree removed on the node"
  remote_board todo - || return 1
}
if step "board: a task made on the node (its API)" make_remote_task && \
   step "orchestration: worker_spawn in the remote workspace (orchestrator on the node)" remote_spawn; then
  step "board: the desktop read the node's task and claimed it (doing, Rwork)" remote_board doing Rwork
  step "orchestration: the worktree is on the node, the worker pane on the desktop" remote_worktree_made
  if [ -n "$remote_worktree" ]; then
    step "orchestration: worker_stop removes the worktree on the node" remote_stop
  else skip "orchestration: worker_stop" "no worktree"; fi
else
  skip "board: the desktop read the node's task" "no worker_spawn"
  skip "orchestration: worktree on the node" "no worker_spawn"
  skip "orchestration: worker_stop" "no worker_spawn"
fi

# A local workspace sends a worker to the node; a commit made there comes back
# and lands here.
local_task=""
node_worktree=""
node_commit=""
local_orchestrator() {
  # Opens the local workspace (the first send says it is starting).
  retry 30 send "$local_ws" Lorch 'true' > /dev/null 2>&1 || { send "$local_ws" Lorch 'true'; return 1; }
  retry 60 helper pane-env --root "$work" "$app_pid" p-lorch > "$work/lorch.env" 2>/dev/null || {
    helper pane-env --root "$work" "$app_pid" p-lorch; return 1; }
  local url; url="$(sed -n 's/^HANGAR_BRIDGE_URL=//p' "$work/lorch.env")"
  if [ -n "$outer_bridge" ] && [ "$url" = "$outer_bridge" ]; then
    echo "the orchestrator's bridge is the caller's own Hangar ($url): refusing"; return 1
  fi
  echo "local orchestrator Lorch: bridge $url (the isolated instance's)"
  local made
  made="$(mcp_as "$work/lorch.env" board_create_task \
    '{"title":"E2E node job","description":"Add work.sh on the node.","owns":["work.sh"]}')" || { echo "$made"; return 1; }
  local_task="$(echo "$made" | jget task.id 2>/dev/null)"
  if [ -z "$local_task" ] || [ "$local_task" = null ]; then
    local_task="$(echo "$made" | grep -oE '[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}' | head -1)"
  fi
  [ -n "$local_task" ] || { echo "no task id in: $made"; return 1; }
  echo "local task $local_task"
}
local_spawn() {
  mcp_as "$work/lorch.env" worker_spawn "{\"task\":\"$local_task\",\"name\":\"Nwork\",\"node\":\"n1\"}" 240 \
    | tee "$work/nspawn.txt" || return 1
  echo
  retry 60 node_helper pane-pid HANGAR_PANE_NAME=Nwork > /dev/null || { echo "Nwork has no shell on the node"; return 1; }
  node_worktree="$(node_helper pane-cwd HANGAR_PANE_NAME=Nwork)" || return 1
  [ "$node_worktree" != /home/hangar ] || { echo "Nwork's shell is not in a worktree"; return 1; }
  on_node git -C "$node_worktree" rev-parse --abbrev-ref HEAD || return 1
  echo "Nwork runs on the node in $node_worktree"
  if helper app-runs "$app_pid" HANGAR_PANE_NAME=Nwork; then
    echo "a process of Nwork runs on this computer, under the app"; return 1
  fi
  echo "no process of Nwork on this computer"
}
node_commit_and_deliver() {
  on_node bash -c "cd $(printf '%q' "$node_worktree") && printf '#!/bin/sh\necho done\n' > work.sh && git add work.sh && git -c user.name=Nwork -c user.email=nwork@node commit -qm 'Add work.sh (made on the node)'" || return 1
  node_commit="$(on_node git -C "$node_worktree" rev-parse HEAD)" || return 1
  echo "commit $node_commit made on the node"
  node_helper pane-mcp HANGAR_PANE_NAME=Nwork delivery_submit '{"summary":"Added work.sh on the node (nodes e2e)."}' 120 || return 1
}
local_task_done() {
  mcp_as "$work/lorch.env" board_list_tasks '{}' | python3 -I -c '
import json, sys
board = json.loads(sys.stdin.read())
task = next(t for t in board["tasks"] if t["id"] == sys.argv[1])
print("local task:", task["column"])
sys.exit(0 if task["column"] == "done" else 1)
' "$local_task"
}
landed() { git -C "$work/repo" merge-base --is-ancestor "$node_commit" main 2>/dev/null; }
local_landed() {
  retry 240 landed || { echo "$node_commit did not land on the local main"; git -C "$work/repo" log --oneline -5 main; return 1; }
  git -C "$work/repo" log --oneline -3 main
  [ "$(git -C "$work/repo" show main:work.sh)" = "$(printf '#!/bin/sh\necho done')" ] || return 1
  echo "the node's commit is on the local main"
  retry 60 local_task_done || { local_task_done; return 1; }
  retry 60 on_node test ! -e "$node_worktree" || { echo "$node_worktree still on the node"; return 1; }
  echo "the worktree on the node is gone"
}
if step "local workspace: orchestrator and a task on the local board" local_orchestrator && \
   step "local workspace: worker_spawn node=n1 runs the worker on the node" local_spawn && \
   step "local workspace: a commit on the node, delivery_submit from there" node_commit_and_deliver; then
  step "local workspace: the node's commit lands on the local main (task done, node cleaned)" local_landed
else
  skip "local workspace: the node's commit lands locally" "an earlier local-workspace step failed"
fi

# --- 4. Summary --------------------------------------------------------------
echo
echo "Summary:"
printf '  %s\n' "${results[@]}"
if [ "$failed" = 0 ]; then echo "e2e: PASS"; exit 0; fi
echo "e2e: FAIL (rerun with --keep to look at $work/app.log)"
exit 1
