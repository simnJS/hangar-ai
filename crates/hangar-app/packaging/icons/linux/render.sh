#!/usr/bin/env bash
# Renders the Linux launcher icon, hangar-ai.svg, to the PNG sizes the
# packages install into the hicolor theme. Needs resvg (`cargo install resvg`).
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
for size in 16 24 32 48 64 128 256 512; do
  resvg -w "$size" -h "$size" hangar-ai.svg "${size}x${size}.png"
done
