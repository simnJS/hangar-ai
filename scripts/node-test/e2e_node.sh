#!/usr/bin/env bash
# Node-side helper of e2e.sh, copied into the test container and run there
# over ssh (the image has bash, curl and procps, no python).
#   e2e_node.sh api <METHOD> <path> [json body]   the node API, with node.json's token
#   e2e_node.sh pane-pid <VAR=value>               the shell of the pane whose env has it
#   e2e_node.sh pane-cwd <VAR=value>               that shell's folder
#   e2e_node.sh pane-mcp <VAR=value> <tool> <json> [secs]
#                                                  one MCP tool call as that pane
set -euo pipefail

node_json="$HOME/.local/state/hangar-node/node.json"
node_bin="$HOME/.local/share/hangar-node/bin/hangar-node"

field() { grep -o "\"$1\": *[^,}]*" "$node_json" | head -1 | sed 's/^[^:]*: *//; s/^"//; s/"$//'; }

api() {
  local method="$1" path="$2" body="${3:-}"
  local port token
  port="$(field port)"; token="$(field token)"
  local args=(-sS --fail-with-body -X "$method" -H "Authorization: Bearer $token")
  [ -n "$body" ] && args+=(-H 'Content-Type: application/json' --data "$body")
  curl "${args[@]}" "http://127.0.0.1:$port$path"
}

# The oldest process whose environment holds VAR=value: the pane's shell
# (what runs in it inherits the same variables and comes later).
pane_pid() {
  local want="$1" pid
  for pid in $(ls /proc | grep -E '^[0-9]+$' | sort -n); do
    if tr '\0' '\n' < "/proc/$pid/environ" 2>/dev/null | grep -qxF "$want"; then
      echo "$pid"; return 0
    fi
  done
  echo "no process with $want" >&2; return 1
}

pane_mcp() {
  local want="$1" tool="$2" arguments="$3" secs="${4:-60}"
  local pid; pid="$(pane_pid "$want")"
  local -a env=()
  mapfile -t env < <(tr '\0' '\n' < "/proc/$pid/environ" | grep '^HANGAR_')
  cd "$(readlink "/proc/$pid/cwd")"
  coproc MCP { exec env "${env[@]}" "$node_bin" --mcp 2>/dev/null; }
  printf '%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"hangar-node-e2e","version":"1"}}}' \
    '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
    "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"$tool\",\"arguments\":$arguments}}" \
    >&"${MCP[1]}"
  local line found=""
  local deadline=$((SECONDS + secs))
  while [ "$SECONDS" -lt "$deadline" ] && IFS= read -r -t "$((deadline - SECONDS + 1))" line <&"${MCP[0]}"; do
    case "$line" in *'"id":2'*) found="$line"; break ;; esac
  done
  kill "$MCP_PID" 2>/dev/null || true
  [ -n "$found" ] || { echo "$tool: no answer within $secs s" >&2; return 1; }
  echo "$found"
  case "$found" in *'"isError":true'*|*'"error":{'*) return 1 ;; esac
}

case "${1:-}" in
  api) shift; api "$@" ;;
  pane-pid) pane_pid "$2" ;;
  pane-cwd) readlink "/proc/$(pane_pid "$2")/cwd" ;;
  pane-mcp) shift; pane_mcp "$@" ;;
  *) sed -n '2,9p' "$0" >&2; exit 2 ;;
esac
