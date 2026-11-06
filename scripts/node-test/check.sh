#!/usr/bin/env bash
# Smoke test: up, uname -m, whoami, scp a file both ways, down. Non-zero on failure.
#   check.sh [--arm64]
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
. "$here/lib.sh"

work="$(mktemp -d)"
cleanup() { rm -rf "$work"; "$here/down.sh" >/dev/null 2>&1 || true; }
trap cleanup EXIT

"$here/up.sh" "$@"
[ -r "$STATE_PORT" ] || { echo "SKIPPED (no rig started)"; exit 0; }

fail() { echo "FAIL: $*" >&2; exit 1; }

arch="$("$here/ssh.sh" uname -m)"
echo "uname -m: $arch"
case "$*" in
  *--arm64*) [ "$arch" = aarch64 ] || fail "expected aarch64, got $arch" ;;
  *) [ -n "$arch" ] || fail "empty uname -m" ;;
esac

who="$("$here/ssh.sh" whoami)"
echo "whoami: $who"
[ "$who" = hangar ] || fail "expected hangar, got $who"

port="$(rig_port)"
mapfile -t opts < <(rig_ssh_opts)
echo "hello from the host $$" > "$work/in.txt"
scp "${opts[@]}" -P "$port" "$work/in.txt" hangar@127.0.0.1:/home/hangar/in.txt >/dev/null
scp "${opts[@]}" -P "$port" hangar@127.0.0.1:/home/hangar/in.txt "$work/out.txt" >/dev/null
cmp "$work/in.txt" "$work/out.txt" || fail "scp round trip differs"
echo "scp: ok"

# Password and root logins must be refused.
if ssh -o BatchMode=yes -o PreferredAuthentications=password -o PubkeyAuthentication=no \
     -o StrictHostKeyChecking=no -o "UserKnownHostsFile=$KNOWN_HOSTS" -o LogLevel=ERROR \
     -p "$port" hangar@127.0.0.1 true 2>/dev/null; then fail "password login accepted"; fi
if ssh "${opts[@]}" -p "$port" root@127.0.0.1 true 2>/dev/null; then fail "root login accepted"; fi
echo "password/root refused: ok"

# The generated ssh_config works too.
[ "$(ssh -F "$SSH_CONFIG" "$HOST_ALIAS" whoami)" = hangar ] || fail "ssh_config does not work"
echo "ssh -F ssh_config: ok"
echo "OK"
