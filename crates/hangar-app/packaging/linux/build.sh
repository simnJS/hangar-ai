#!/usr/bin/env bash
# Builds the Linux packages from the release binaries cargo already built:
#
#   Hangar.AI_<version>_amd64.AppImage
#   Hangar.AI_<version>_amd64.deb
#   Hangar.AI-<version>-1.x86_64.rpm
#
#   packaging/linux/build.sh <out-dir>
#
# The names are the Tauri build's, because the download page links to them and
# the feed's URLs are made from them. Needs cargo-packager (the AppImage and the
# .deb), cargo-generate-rpm (the .rpm), jq, and `cargo build --release --locked
# -p hangar-app -p hangar-webview --features hangar-webview/engine` done first,
# and hangar-node-x86_64 and hangar-node-aarch64 in <target dir>/node.
# docs/native/PACKAGING.md explains each choice.
set -euo pipefail

if [ "$#" -ne 1 ]; then
  echo "usage: $0 <out-dir>" >&2
  exit 2
fi

source "$(dirname "${BASH_SOURCE[0]}")/../common.sh"
out="$(absolute_dir "$1")"
target_dir="$(cd "$repo" && cargo metadata --no-deps --format-version 1 | jq -r .target_directory)"

for binary in hangar-ai hangar-webview; do
  if [ ! -x "$target_dir/release/$binary" ]; then
    echo "$target_dir/release/$binary is missing: build it first (see docs/native/PACKAGING.md)" >&2
    exit 1
  fi
done

# hangar-node for x86_64 and arm64 beside the executables, where the packager
# (the `binaries` of the manifest, and the .rpm's assets) takes them from.
stage_node_binaries "$target_dir" "$target_dir/release"

# The packager's own AppRun, which it downloads, exports PYTHONHOME, PYTHONPATH
# and friends pointing into the image, and so breaks every Python that a Hangar
# pane starts. It copies the file with the name below from its tool cache
# whenever one is there, so ours is put in its place. The cache is a folder of
# its own so a developer's real one is left as it was.
cache="$target_dir/packager-cache"
mkdir -p "$cache/.cargo-packager/AppImage"
install -m 755 "$packaging/linux/AppRun" "$cache/.cargo-packager/AppImage/AppRun-x86_64"

rm -rf "$out/.cargo-packager"
(
  cd "$repo"
  XDG_CACHE_HOME="$cache" cargo packager --release --formats deb,appimage --out-dir "$out"
)

# cargo-packager's names (hangar-ai_1.2.3_x86_64.AppImage, hangar-ai_1.2.3_amd64.deb)
# to the ones the Tauri build published.
mv "$out/hangar-ai_${version}_x86_64.AppImage" "$out/Hangar.AI_${version}_amd64.AppImage"
mv "$out/hangar-ai_${version}_amd64.deb" "$out/Hangar.AI_${version}_amd64.deb"
rm -rf "$out/.cargo-packager"

# cargo-generate-rpm writes the package itself, without rpmbuild. It names it
# <name>-<version>-<release>.<arch>.rpm from the manifest, which is already the
# Tauri build's Hangar.AI-<version>-1.x86_64.rpm.
(
  cd "$repo"
  cargo generate-rpm -p crates/hangar-app -o "$out/"
)

ls -la "$out"
