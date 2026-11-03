#!/usr/bin/env bash
# Hangar Mobile has to be in the release.
#
#   packaging/check-mobile.sh              before cargo builds: dist-mobile/ is complete
#   packaging/check-mobile.sh <binary>...  after: those executables embed it
#
# hangar-core compiles dist-mobile/ into the executable (remote/assets.rs).
# Without it the build still succeeds, and every phone is served a page saying
# the app is missing, so a release that has no phone app must fail here rather
# than reach /releases/latest/. (`pnpm mobile:build` makes the folder.)
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
dist="$repo/dist-mobile"

if ! grep -q '<div id="root"></div>' "$dist/index.html" 2> /dev/null || [ ! -f "$dist/sw.js" ]; then
  echo "::error::dist-mobile/ is missing or incomplete: the installers would serve the placeholder instead of Hangar Mobile (run: pnpm install && pnpm mobile:build)"
  exit 1
fi

# The phone app's script has a hash in its name, which is nowhere else: finding
# it in the executable means the folder as it is now was compiled in, and not an
# older one or none.
script="$(cd "$dist/assets" && ls mobile-*.js 2> /dev/null | head -n 1)"
if [ -z "$script" ]; then
  echo "::error::dist-mobile/assets has no mobile-*.js"
  exit 1
fi

for binary in "$@"; do
  if ! grep -aqF "$script" "$binary"; then
    echo "::error::$binary does not contain Hangar Mobile ($script): it was built before dist-mobile/ or without it"
    exit 1
  fi
  echo "$binary embeds Hangar Mobile ($script)"
done
