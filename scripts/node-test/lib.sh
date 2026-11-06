# Shared by the rig scripts (sourced, not run).
RIG_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
KEYS_DIR="$RIG_DIR/.keys"
KEY="$KEYS_DIR/id_ed25519"
KNOWN_HOSTS="$KEYS_DIR/known_hosts"
SSH_CONFIG="$RIG_DIR/ssh_config"
STATE_PORT="$KEYS_DIR/port"
CONTAINER="${HANGAR_NODE_TEST_CONTAINER:-hangar-node-test}"
HOST_ALIAS="hangar-node-test"

# Options for a plain `ssh`/`scp` invocation (port is added by the caller).
rig_ssh_opts() {
  printf '%s\n' -i "$KEY" -o IdentitiesOnly=yes -o StrictHostKeyChecking=no \
    -o "UserKnownHostsFile=$KNOWN_HOSTS" -o LogLevel=ERROR -o BatchMode=yes \
    -o ConnectTimeout=5
}

rig_port() {
  if [ -r "$STATE_PORT" ]; then cat "$STATE_PORT"; else
    echo "no rig running (run scripts/node-test/up.sh)" >&2; return 1
  fi
}

port_is_free() {
  ! (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null
}
