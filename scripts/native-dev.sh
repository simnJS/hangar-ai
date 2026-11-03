#!/usr/bin/env bash
# Run the native Hangar (or any of its examples) without touching the
# installed app: every folder it would share with the released Hangar —
# state.json and the app data, the endpoint file, the Claude Code mods, kits,
# accounts — goes under target/dev-home/ instead (HANGAR_DATA_ROOT and
# HANGAR_ENDPOINT_FILE, see crates/hangar-core/src/paths.rs and endpoint.rs).
#
#   scripts/native-dev.sh                    cargo run -p hangar-app
#   scripts/native-dev.sh --release          cargo run -p hangar-app --release
#   scripts/native-dev.sh -- --mcp           arguments after -- go to the app
#
# Set HANGAR_DEV_HOME to use another folder, e.g. one holding a copy of a real
# state.json to compare against the old app.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
home="${HANGAR_DEV_HOME:-$root/target/dev-home}"
mkdir -p "$home/hangar"

export HANGAR_DATA_ROOT="$home"
export HANGAR_ENDPOINT_FILE="$home/hangar/endpoint.json"

exec cargo run --manifest-path "$root/Cargo.toml" -p hangar-app "$@"
