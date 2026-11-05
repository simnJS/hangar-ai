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

# The real key, made by `tauri signer generate` with an empty password, is
# refused by cargo-packager's signer ("Wrong password for that key", 2.0.0's
# first publish), while the Tauri signer that signed every 1.x release reads
# it. So when cargo-packager refuses, the same Tauri CLI as 1.12.1 signs: its
# .sig is the same text, and the publish job verifies every one of them
# against the public key the installs carry before anything is published.
# The key goes through the environment, never the command line; an empty
# password has to be given as an argument, since an empty variable reads as
# no password at all and the CLI would then ask for one.
TAURI_CLI="@tauri-apps/cli@2.11.4"

for file in "$@"; do
  if ! CARGO_PACKAGER_SIGN_PRIVATE_KEY="$key" \
    CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD="$password" \
    cargo packager signer sign "$file"; then
    echo "cargo-packager refused the key: signing $file with $TAURI_CLI" >&2
    rm -f "$file.sig"
    if [ -z "$password" ]; then
      TAURI_SIGNING_PRIVATE_KEY="$key" \
        npx --yes "$TAURI_CLI" signer sign --password "" "$file"
    else
      TAURI_SIGNING_PRIVATE_KEY="$key" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$password" \
        npx --yes "$TAURI_CLI" signer sign "$file"
    fi
  fi
  test -s "$file.sig"
done
