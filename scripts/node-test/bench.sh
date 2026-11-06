#!/usr/bin/env bash
# Benchmark of a remote pane on the Docker rig, against an isolated Hangar
# started as e2e.sh starts it: keystroke -> echo latency (the node's API, the
# desktop's mirrored terminal, a local pane, a local PTY), output throughput
# (`seq 1 N`), hangar-node's memory, threads and idle CPU, and the time from
# the app's start to a live remote pane (cold, warm, reconnect; per step).
#   bench.sh [--no-build] [--keep] [--runs N] [--samples N] [--lines N]
#            [--node-bin PATH] [--app-bin PATH] [--connect-only] [--latency "0 20"]
#     --no-build    use the binaries already in the target folder
#     --keep        keep the run's temporary folder (logs, results)
#     --runs N      runs of each measure, the median reported (default 3)
#     --samples N   keystrokes per echo run (default 200)
#     --lines N     lines of `seq` per throughput run (default 2000000)
#     --node-bin/--app-bin  other binaries (a baseline build) than the target's
#     --connect-only  only the connect measures
#     --latency "MS…" the added latencies of the connect measures, each a run
#                   of its own (default "0 20"; `tc netem` in the container)
# Container, port and isolation: as e2e.sh (HANGAR_NODE_TEST_CONTAINER,
# HANGAR_NODE_TEST_PORT, HANGAR_PORT_BASE). Results: printed, and
# HANGAR_BENCH_OUT (a JSON file) when set.
set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"

build=1
keep=0
runs=3
samples=200
lines=2000000
node_bin=""
app_bin=""
connect_only=0
latencies="0 20"
while [ $# -gt 0 ]; do
  case "$1" in
    --no-build) build=0 ;;
    --keep) keep=1 ;;
    --runs) runs="$2"; shift ;;
    --samples) samples="$2"; shift ;;
    --lines) lines="$2"; shift ;;
    --node-bin) node_bin="$(realpath "$2")"; build=0; shift ;;
    --app-bin) app_bin="$(realpath "$2")"; build=0; shift ;;
    --connect-only) connect_only=1 ;;
    --latency) latencies="$2"; shift ;;
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done

# --- Isolation (as e2e.sh) ---------------------------------------------------
port_base="${HANGAR_PORT_BASE:-}"
port_count="${HANGAR_PORT_COUNT:-20}"
rig_port="${HANGAR_NODE_TEST_PORT:-}"
container="${HANGAR_NODE_TEST_CONTAINER:-hangar-node-bench-$$}"
out_file="${HANGAR_BENCH_OUT:-}"
for name in $(compgen -e); do
  case "$name" in HANGAR_*) unset "$name" ;; esac
done

work="$(mktemp -d "${TMPDIR:-/tmp}/hangar-node-bench.XXXXXX")"
work="$(cd "$work" && pwd -P)"
export HANGAR_DATA_ROOT="$work/home"
export HANGAR_ENDPOINT_FILE="$work/home/hangar/endpoint.json"
export XDG_DATA_HOME="$work/xdg/data" XDG_CACHE_HOME="$work/xdg/cache" XDG_STATE_HOME="$work/xdg/state"
export HANGAR_NODE_TEST_CONTAINER="$container"
export HANGAR_NODE_TEST_STATE_DIR="$work/rig"
mkdir -p "$HANGAR_DATA_ROOT/data" "$HANGAR_DATA_ROOT/hangar" "$XDG_DATA_HOME" "$XDG_CACHE_HOME" "$XDG_STATE_HOME" "$work/local"
case "$(realpath -m "$HANGAR_ENDPOINT_FILE")" in
  "$work"/*) ;;
  *) echo "refusing to run: HANGAR_ENDPOINT_FILE ($HANGAR_ENDPOINT_FILE) is not under $work" >&2; rm -rf "$work"; exit 2 ;;
esac
cd "$work" || exit 2

# shellcheck source=lib.sh
. "$here/lib.sh"

app_pid=""
tunnel_pid=""
results="$work/results.jsonl"
: > "$results"

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
  [ -n "$tunnel_pid" ] && kill "$tunnel_pid" 2>/dev/null
  stop_app
  "$here/down.sh" >/dev/null 2>&1 || true
  if [ "$keep" = 1 ]; then echo "kept: $work"; else rm -rf "$work"; fi
  exit "$code"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

die() { echo "bench: $*" >&2; exit 1; }
say() { printf '== %s\n' "$*"; }
helper() { python3 -I "$here/bench_helper.py" "$@"; }
retry() {
  local secs="$1"; shift
  local deadline=$((SECONDS + secs))
  until "$@"; do
    [ "$SECONDS" -lt "$deadline" ] || return 1
    sleep 1
  done
}
record() { tee -a "$results"; }

# --- Binaries ----------------------------------------------------------------
target="$(cargo metadata --manifest-path "$root/Cargo.toml" --format-version 1 --no-deps 2>/dev/null \
  | python3 -I -c 'import json,sys;print(json.load(sys.stdin)["target_directory"])')"
[ -n "$node_bin" ] || node_bin="$target/release/hangar-node"
[ -n "$app_bin" ] || app_bin="$target/bench-app/hangar-ai"
if [ "$build" = 1 ]; then
  # Apart: built together, hangar-app's features would unify into hangar-node.
  # The app: optimized (a debug build's terminal would be what is measured),
  # with debug assertions, which HANGAR_DEV_NODES_CONNECT needs (nodes/mod.rs),
  # in a profile of its own (target/bench-app) so the release build stays.
  say "build hangar-node (release) and hangar-app (release + debug assertions)"
  cargo build --manifest-path "$root/Cargo.toml" -p hangar-node --release --locked || die "hangar-node did not build"
  cargo build --manifest-path "$root/Cargo.toml" -p hangar-app --locked \
    --config 'profile.bench-app.inherits="release"' --config 'profile.bench-app.debug-assertions=true' \
    --profile bench-app || die "hangar-app did not build"
fi
for bin in "$node_bin" "$app_bin"; do [ -x "$bin" ] || die "$bin is missing"; done
[ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ] || die "no display: the desktop app needs DISPLAY or WAYLAND_DISPLAY"

# --- Rig, state, app ---------------------------------------------------------
free_port() {
  local from=22200 count=100 p
  if [ -n "$port_base" ]; then from="$port_base"; count="$port_count"; fi
  for p in $(seq "$from" $((from + count - 1))); do
    [ "$p" = "$rig_port" ] && continue
    if port_is_free "$p"; then echo "$p"; return 0; fi
  done
  return 1
}
[ -n "$rig_port" ] || rig_port="$(free_port)" || die "no free port"
say "rig: container $container on port $rig_port"
HANGAR_NODE_TEST_NET_ADMIN=1 HANGAR_NODE_TEST_PORT="$rig_port" "$here/up.sh" > "$work/rig.log" 2>&1 || { cat "$work/rig.log"; die "the rig did not start"; }

git init -q -b main "$work/local" && git -C "$work/local" -c user.name=B -c user.email=b@b commit -q --allow-empty -m init
cat > "$HANGAR_DATA_ROOT/data/state.json" <<JSON
{
  "activeWorkspaceId": "w-remote",
  "workspaces": [
    {"id": "w-remote", "name": "Bench remote", "cwd": "/home/hangar", "node": "n1",
     "panes": [{"id": "p-remote", "name": "Remote", "agent": "shell"}], "savedCommands": []},
    {"id": "w-local", "name": "Bench local", "cwd": "$work/local",
     "panes": [{"id": "p-local", "name": "Local", "agent": "shell"}], "savedCommands": []}
  ],
  "settings": {
    "experiments": ["nodes"],
    "agentPaneControl": true,
    "nodes": [{"id": "n1", "label": "Bench rig", "sshTarget": "$HOST_ALIAS", "sshConfig": "$SSH_CONFIG"}]
  }
}
JSON

say "app: $app_bin"
say "node: $node_bin"
start_app() { # [log]: the isolated app, connecting its node as it starts
  rm -f "$HANGAR_ENDPOINT_FILE"
  HANGAR_NODE_BINARY="$node_bin" HANGAR_DEV_NODES_CONNECT=1 HANGAR_NODES_TIMING=1 \
    setsid "$app_bin" >> "${1:-$work/app.log}" 2>&1 < /dev/null &
  app_pid=$!
}
start_app
retry 90 test -s "$HANGAR_ENDPOINT_FILE" || { tail -n 20 "$work/app.log"; die "no endpoint after 90 s"; }
ctl_args=(--root "$work" --endpoint "$HANGAR_ENDPOINT_FILE")
# The remote pane once its node is deployed, started and connected.
retry 120 helper desktop-prepare "${ctl_args[@]}" w-remote Remote > /dev/null 2>&1 \
  || { helper desktop-prepare "${ctl_args[@]}" w-remote Remote; tail -n 20 "$work/app.log"; die "the remote pane did not start"; }
retry 60 helper desktop-prepare "${ctl_args[@]}" w-local Local > /dev/null 2>&1 \
  || { helper desktop-prepare "${ctl_args[@]}" w-local Local; die "the local pane did not start"; }

# The node's API through a tunnel of the bench's own, opened as the app opens
# its own (commands.rs `tunnel`).
node_field() { "$here/ssh.sh" 'cat ~/.local/state/hangar-node/node.json' | python3 -I -c "import json,sys;print(json.load(sys.stdin)['$1'])"; }
node_port="$(node_field port)" && node_token="$(node_field token)" && node_pid="$(node_field pid)" || die "no node.json on the node"
tunnel_port="$(free_port)" || die "no free port for the tunnel"
ssh -F "$SSH_CONFIG" -o BatchMode=yes -o ConnectTimeout=10 -o ExitOnForwardFailure=yes \
  -o ServerAliveInterval=15 -o ServerAliveCountMax=3 -o IPQoS=af21 -o Compression=no -N \
  -L "127.0.0.1:$tunnel_port:127.0.0.1:$node_port" "$HOST_ALIAS" &
tunnel_pid=$!
retry 15 bash -c "exec 3<>/dev/tcp/127.0.0.1/$tunnel_port" 2>/dev/null || die "the bench's tunnel did not open"
node_args=(--url "http://127.0.0.1:$tunnel_port" --token "$node_token")
say "node pid $node_pid, API on the node's 127.0.0.1:$node_port, here on $tunnel_port"

# --- Processes ---------------------------------------------------------------
# `ticks rss_kb threads` of a process, from its /proc (here or on the node).
proc_of() { # <cat command...>
  "$@" | awk '
    FNR == 1 && !stat { sub(/.*\) /, ""); split($0, f, " "); ticks = f[12] + f[13]; stat = 1; next }
    /^VmRSS:/ { rss = $2 } /^Threads:/ { threads = $2 }
    END { print ticks, rss, threads }'
}
node_proc() { proc_of docker exec "$container" cat "/proc/$node_pid/stat" "/proc/$node_pid/status"; }
app_proc() { proc_of cat "/proc/$app_pid/stat" "/proc/$app_pid/status"; }
# The app's ssh tunnels (its process group): their CPU ticks together.
tunnel_ticks() {
  local total=0 pid t
  for pid in $(ps -eo pid=,pgid=,args= | awk -v g="$app_pid" '$2 == g && $3 == "ssh" && / -L / { print $1 }'); do
    t="$(sed 's/.*) //' "/proc/$pid/stat" 2>/dev/null | awk '{ print $12 + $13 }')"
    total=$((total + ${t:-0}))
  done
  echo "$total"
}
hz="$(getconf CLK_TCK)"
cpu_pct() { python3 -I -c "print(round(100 * $1 / $hz / $2, 2))"; }

# --- Measures ----------------------------------------------------------------
if [ "$connect_only" = 0 ]; then
say "keystroke -> echo ($samples keys, $runs runs each)"
for run in $(seq 1 "$runs"); do
  helper local-echo --samples "$samples" | record
  helper node-echo "${node_args[@]}" --pane bench-echo --via ws --samples "$samples" | record
  helper node-echo "${node_args[@]}" --pane bench-echo --via post --samples "$samples" | record
  helper desktop-echo "${ctl_args[@]}" w-local Local --samples "$samples" \
    | sed 's/"desktop-echo"/"desktop-echo-local-pane"/' | record
  helper desktop-echo "${ctl_args[@]}" w-remote Remote --samples "$samples" \
    | sed 's/"desktop-echo"/"desktop-echo-remote-pane"/' | record
done
helper node-kill "${node_args[@]}" --pane bench-echo > /dev/null

say "throughput: seq 1 $lines ($runs runs each)"
for run in $(seq 1 "$runs"); do
  # The node's stream alone (no desktop pane drawing it).
  read -r n0 _ _ < <(node_proc)
  out="$(helper node-flood "${node_args[@]}" --pane bench-flood --lines "$lines")" || die "node-flood failed"
  read -r n1 _ _ < <(node_proc)
  secs="$(echo "$out" | python3 -I -c 'import json,sys;print(json.load(sys.stdin)["seconds"])')"
  echo "$out" | python3 -I -c "import json,sys;v=json.load(sys.stdin);v['node_cpu_pct']=$(cpu_pct $((n1 - n0)) "$secs");print(json.dumps(v))" | record
  helper node-kill "${node_args[@]}" --pane bench-flood > /dev/null

  for side in local remote; do
    if [ "$side" = local ]; then ws=w-local; pane=Local; else ws=w-remote; pane=Remote; fi
    read -r n0 _ _ < <(node_proc); read -r a0 _ _ < <(app_proc); s0="$(tunnel_ticks)"
    out="$(helper desktop-flood "${ctl_args[@]}" "$ws" "$pane" --lines "$lines")" || die "desktop-flood failed ($side)"
    read -r n1 _ _ < <(node_proc); read -r a1 _ _ < <(app_proc); s1="$(tunnel_ticks)"
    secs="$(echo "$out" | python3 -I -c 'import json,sys;print(json.load(sys.stdin)["seconds"])')"
    echo "$out" | python3 -I -c "
import json, sys
v = json.load(sys.stdin)
v['measure'] = 'desktop-flood-$side-pane'
v['app_cpu_pct'] = $(cpu_pct $((a1 - a0)) "$secs")
v['node_cpu_pct'] = $( [ "$side" = remote ] && cpu_pct $((n1 - n0)) "$secs" || echo 0)
v['ssh_tunnel_cpu_pct'] = $(cpu_pct $((s1 - s0)) "$secs")
print(json.dumps(v))" | record
  done
done

idle() { # <label>: 10 s of nothing, node and app
  local n0 a0 n1 a1 rss threads arss athreads
  read -r n0 _ _ < <(node_proc); read -r a0 _ _ < <(app_proc)
  sleep 10
  read -r n1 rss threads < <(node_proc); read -r a1 arss athreads < <(app_proc)
  printf '{"measure":"%s","node_rss_kb":%s,"node_threads":%s,"node_idle_cpu_pct":%s,"app_rss_kb":%s,"app_threads":%s,"app_idle_cpu_pct":%s}\n' \
    "$1" "$rss" "$threads" "$(cpu_pct $((n1 - n0)) 10)" "$arss" "$athreads" "$(cpu_pct $((a1 - a0)) 10)"
}
say "node at rest (1 pane, the desktop attached) and with 8 panes ($runs runs each)"
for run in $(seq 1 "$runs"); do
  idle node-rest-1-pane | record
  for i in 1 2 3 4 5 6 7; do helper node-spawn "${node_args[@]}" --pane "bench-idle-$i" > /dev/null; done
  sleep 2
  idle node-rest-8-panes | record
  for i in 1 2 3 4 5 6 7; do helper node-kill "${node_args[@]}" --pane "bench-idle-$i" > /dev/null; done
  sleep 1
done
fi

# --- Connect -------------------------------------------------------------------
# The app quit and started again, until its remote pane shows text (its
# prompt; the replayed screen on a reconnect). Local: the local workspace
# shown instead, until its pane shows its prompt (the floor). Cold: no hangar-node on the node (binary, state and daemon gone).
# Warm: the binary there and its daemon running, the pane not (killed
# through the node's API): the app makes it. Reconnect: everything as the
# app left it. Each with each `--latency` added on the container's link
# (netem on its eth0: what it sends is delayed, so the round trip grows by
# that much).
docker cp "$here/e2e_node.sh" "$container:/tmp/e2e_node.sh" >/dev/null
node_sh() { docker exec -u hangar -e HOME=/home/hangar "$container" bash /tmp/e2e_node.sh "$@"; }
set_latency() { # <ms>
  docker exec "$container" tc qdisc del dev eth0 root 2>/dev/null
  [ "$1" = 0 ] && return 0
  docker exec "$container" tc qdisc add dev eth0 root netem delay "${1}ms" \
    || { echo "bench: tc netem refused in the container (no NET_ADMIN?): $1 ms not measured" >&2; return 1; }
}
forget_node() { # no hangar-node at all on the node
  docker exec "$container" pkill -u hangar -f 'hangar-node serve' 2>/dev/null
  for _ in $(seq 1 40); do docker exec "$container" pgrep -u hangar -f 'hangar-node serve' >/dev/null || break; sleep 0.1; done
  docker exec -u hangar "$container" rm -rf /home/hangar/.local/share/hangar-node /home/hangar/.local/state/hangar-node
}
shown_workspace() { # <id>: the workspace the app opens on
  python3 -I - "$HANGAR_DATA_ROOT/data/state.json" "$1" <<'PY'
import json, sys
path, ws = sys.argv[1], sys.argv[2]
with open(path, encoding="utf-8") as handle:
    state = json.load(handle)
state["activeWorkspaceId"] = ws
with open(path, "w", encoding="utf-8") as handle:
    json.dump(state, handle)
PY
}
connect_run() { # <kind> <latency>: not in a subshell (app_pid is the caller's)
  local log="$work/connect-$1-$2ms-$run.log" t0 ws=w-remote pane=Remote
  stop_app
  case "$1" in
    cold) forget_node ;;
    warm) node_sh api DELETE /node/panes/p-remote > /dev/null 2>&1 || true ;;
    local) ws=w-local; pane=Local ;;
  esac
  shown_workspace "$ws"
  t0="$(date +%s%N)"
  start_app "$log"
  helper connect "${ctl_args[@]}" --since "$t0" --log "$log" --name "connect-$1-$2ms" \
    "$ws" "$pane" > "$work/connect.json" || { tail -n 20 "$log"; die "connect-$1 ($2 ms) did not finish"; }
  record < "$work/connect.json"
}
say "connect: cold, warm, reconnect, local ($runs runs each, added latency: $latencies ms)"
for latency in $latencies; do
  set_latency "$latency" || continue
  for run in $(seq 1 "$runs"); do
    for kind in cold warm reconnect local; do connect_run "$kind" "$latency"; done
  done
done
set_latency 0

echo
say "medians of $runs runs"
helper report "$results" "$work/report.json"
[ -n "$out_file" ] && cp "$work/report.json" "$out_file"
exit 0
