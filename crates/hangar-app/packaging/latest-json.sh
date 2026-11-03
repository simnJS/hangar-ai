#!/usr/bin/env bash
# Writes latest.json, the feed every install polls, from the signatures beside
# the artifacts they sign.
#
#   TAG=v1.13.0 REPO=hangar-ai/hangar [NOTES=...] packaging/latest-json.sh <dir> [latest.json]
#
# <dir> holds each updater artifact with its .sig (packaging/sign.sh). The shape
# is the one hangar-update parses (crates/hangar-update/src/manifest.rs) and
# tauri-plugin-updater read: version, notes, pub_date and a `platforms` map of
# key -> { signature, url }, the signature being the .sig file's text as it is.
#
# A key is `{os}-{arch}` or `{os}-{arch}-{installer}`; the updater asks for the
# installer's first and the plain one second (hangar_update::Target::keys), so
# an app installed by an old release, which only knows the plain ones, and the
# native app, which knows both, find their build. The extension says which keys
# an artifact answers for.
set -euo pipefail

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  echo "usage: TAG=v1.2.3 REPO=owner/name $0 <dir> [latest.json]" >&2
  exit 2
fi

dir="$1"
output="${2:-latest.json}"
: "${TAG:?TAG is not set (the release tag, v1.2.3)}"
: "${REPO:?REPO is not set (owner/name)}"
notes="${NOTES:-}"

platforms='{}'
count=0
for sig in "$dir"/*.sig; do
  [ -e "$sig" ] || continue
  bundle="$(basename "${sig%.sig}")"
  case "$bundle" in
    *.app.tar.gz) keys="darwin-aarch64 darwin-x86_64 darwin-aarch64-app darwin-x86_64-app" ;;
    # The AppImage takes the plain Linux key: it is the one bundle that can
    # replace itself in place, which is what the updater does. The other two
    # are listed under their own name for an install that came from a package
    # manager.
    *.AppImage) keys="linux-x86_64 linux-x86_64-appimage" ;;
    *.deb) keys="linux-x86_64-deb" ;;
    *.rpm) keys="linux-x86_64-rpm" ;;
    # The plain Windows key belongs to the NSIS installer, which is the only one
    # shipped: it installs per user, so an update can replace an install
    # without asking for rights the first one never needed. The msi line is kept
    # for the day that target comes back, on its own key so it cannot take the
    # default by existing.
    *-setup.exe) keys="windows-x86_64 windows-x86_64-nsis" ;;
    *.msi) keys="windows-x86_64-msi" ;;
    *)
      echo "::warning::no platform key for $bundle" >&2
      continue
      ;;
  esac
  signature="$(cat "$sig")"
  url="https://github.com/$REPO/releases/download/$TAG/$bundle"
  for key in $keys; do
    platforms="$(jq -c --arg k "$key" --arg s "$signature" --arg u "$url" \
      '.[$k] = { signature: $s, url: $u }' <<< "$platforms")"
  done
  count=$((count + 1))
done

# A feed missing a platform is exactly the silent failure this exists to
# prevent, and one missing all of them means the signing key never got here.
# Neither is written.
for required in darwin-aarch64 linux-x86_64 windows-x86_64; do
  if [ "$(jq --arg k "$required" 'has($k)' <<< "$platforms")" != "true" ]; then
    echo "latest.json would ship without $required (found $count signed artifact(s) in $dir)" >&2
    exit 1
  fi
done

jq -n --arg v "${TAG#v}" \
      --arg n "$notes" \
      --arg d "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
      --argjson p "$platforms" \
  '{ version: $v, notes: $n, pub_date: $d, platforms: $p }' > "$output"
jq -r '.platforms | keys[]' "$output" | sed 's/^/  /'
