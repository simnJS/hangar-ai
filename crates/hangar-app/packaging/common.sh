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

# The folder, created if need be, as an absolute path. cargo-packager wants its
# --out-dir absolute: it changes directory to the package's folder before it
# uses a relative one.
absolute_dir() {
  mkdir -p "$1"
  (cd "$1" && pwd)
}
