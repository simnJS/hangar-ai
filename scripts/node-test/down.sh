#!/usr/bin/env bash
# Remove the test node container (keys are kept).
set -euo pipefail
# shellcheck source=lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
rm -f "$STATE_PORT" "$SSH_CONFIG"
echo "removed $CONTAINER"
