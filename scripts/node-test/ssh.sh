#!/usr/bin/env bash
# Run a command on the test node: ssh.sh <cmd...>  (no command: interactive shell)
set -euo pipefail
# shellcheck source=lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
port="$(rig_port)"
mapfile -t opts < <(rig_ssh_opts)
exec ssh "${opts[@]}" -p "$port" hangar@127.0.0.1 "$@"
