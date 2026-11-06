#!/usr/bin/env bash
# Shared by the packaging scripts: source it, do not run it.
#
# Sets `repo` (the workspace root), `version` (hangar-app's, as cargo has it:
# the one number a release bumps) and defines `absolute_dir`.

packaging="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$packaging/../../.." && pwd)"

pkgid="$(cd "$repo" && cargo pkgid -p hangar-app)"
version="${pkgid##*[#@]}" # path+file:///.../hangar-app#1.2.3, or #hangar-app@1.2.3
if [ -z "$version" ] || [ "$version" = "$pkgid" ]; then
  echo "could not read hangar-app's version from cargo (cargo pkgid said: $pkgid)" >&2
  exit 1
fi

# The headless node daemon, built for Linux x86_64 and arm64 by the release
# workflow's `node` job and downloaded into `$target_dir/node` as
# `hangar-node-x86_64` and `hangar-node-aarch64`. Every desktop package carries
# both, beside the executable, because that is where the app looks for the file
# to upload to a node (hangar_app::nodes::model::BinarySource) and a Mac or a
# Windows desktop drives Linux nodes too. This checks they are there, are ELF
# files of the right machine, and copies them to each folder given.
#
#   stage_node_binaries <target-dir> <dest-dir>...
stage_node_binaries() {
  local target_dir="$1" arch dest header machine
  shift
  for arch in x86_64 aarch64; do
    local src="$target_dir/node/hangar-node-$arch"
    if [ ! -f "$src" ]; then
      echo "$src is missing: download the release workflow's hangar-node artifacts there (docs/native/PACKAGING.md, \"hangar-node\")" >&2
      return 1
    fi
    # ELF magic, then e_machine (byte 18): 0x3e is x86-64, 0xb7 is AArch64.
    header="$(head -c 20 "$src" | od -An -tx1 | tr -d ' \n')"
    case "$arch:$header" in
      x86_64:7f454c46*3e00 | aarch64:7f454c46*b700) ;;
      *)
        echo "$src is not a Linux $arch ELF file" >&2
        return 1
        ;;
    esac
    for dest in "$@"; do
      mkdir -p "$dest"
      install -m 755 "$src" "$dest/hangar-node-$arch"
    done
  done
}

# The folder, created if need be, as an absolute path. cargo-packager wants its
# --out-dir absolute: it changes directory to the package's folder before it
# uses a relative one.
absolute_dir() {
  mkdir -p "$1"
  (cd "$1" && pwd)
}
