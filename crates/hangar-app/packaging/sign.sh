#!/usr/bin/env bash
# Signs the files the updater downloads, leaving <file>.sig beside each.
#
#   TAURI_SIGNING_PRIVATE_KEY=... packaging/sign.sh <file>...
#
# The private key is the one the Tauri build signed with, under the same
# secret names (TAURI_SIGNING_PRIVATE_KEY and, if it has one,
# TAURI_SIGNING_PRIVATE_KEY_PASSWORD): its public half is compiled into every
# install as hangar_update::PUBKEY, and an install only accepts what that key
# signed. CARGO_PACKAGER_SIGN_PRIVATE_KEY(_PASSWORD) name the same things for
# cargo-packager and win if set.
#
# `cargo packager signer sign` writes what the feed needs and the Tauri signer
# wrote: a minisign signature of the file's BLAKE2b hash, with the file's name
# in its trusted comment, the whole .sig text base64-encoded.
set -euo pipefail

if [ "$#" -lt 1 ]; then
  echo "usage: $0 <file>..." >&2
  exit 2
fi

key="${CARGO_PACKAGER_SIGN_PRIVATE_KEY:-${TAURI_SIGNING_PRIVATE_KEY:-}}"
if [ -z "$key" ]; then
  echo "no signing key: set TAURI_SIGNING_PRIVATE_KEY (without it the release would be refused by every install)" >&2
  exit 1
fi
password="${CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD:-${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}}"

for file in "$@"; do
  CARGO_PACKAGER_SIGN_PRIVATE_KEY="$key" \
  CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD="$password" \
    cargo packager signer sign "$file"
  test -s "$file.sig"
done
