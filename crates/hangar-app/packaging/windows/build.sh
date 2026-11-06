#!/usr/bin/env bash
# Builds the Windows installer: Hangar.AI_<version>_x64-setup.exe.
#
#   packaging/windows/build.sh <release-dir> <out-dir>
#
# <release-dir> holds the two executables cargo built (hangar-ai.exe and
# hangar-webview.exe, normally target/release) and, in its sibling `node` folder
# (target/node), the Linux hangar-node-x86_64 and hangar-node-aarch64 the
# installer carries for the app to upload to nodes; the installer lands in
# <out-dir>.
# It needs `makensis` (NSIS 3.x; set MAKENSIS to run another one), curl and
# sha1sum or shasum, and runs on Linux as well as under Git Bash on Windows, so
# the installer's script can be compiled, though not run, on a developer's
# machine (docs/native/PACKAGING.md, "The Windows installer").
#
# Why this is not cargo-packager's NSIS step: the installer is the Tauri
# bundler's script with Hangar's own pages and the update-in-place behaviour
# hangar-update relies on (/UPDATE, /ARGS, the migration of installs made under
# earlier product names). cargo-packager's copy of the script is an older fork
# of the bundler's, with an older plugin, and has none of that. Running makensis
# on the script as it was keeps the installer what it was.
set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: $0 <release-dir> <out-dir>" >&2
  exit 2
fi

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
icons="$(cd "$here/../icons" && pwd)"
repo="$(cd "$here/../../../.." && pwd)"
release_dir="$(cd "$1" && pwd)"
mkdir -p "$2"
out_dir="$(cd "$2" && pwd)"
makensis="${MAKENSIS:-makensis}"

# The plugin the bundler's script calls (nsis_tauri_utils::SemverCompare,
# RunAsUser, FindProcessCurrentUser...), at the version and with the checksum
# the Tauri bundler 2.9 pinned for it.
plugin_url="https://github.com/tauri-apps/nsis-tauri-utils/releases/download/nsis_tauri_utils-v0.5.3/nsis_tauri_utils.dll"
plugin_sha1="75197fee3c6a814fe035788d1c34ead39349b860"

for exe in hangar-ai.exe hangar-webview.exe; do
  if [ ! -f "$release_dir/$exe" ]; then
    echo "$release_dir/$exe is missing: build it first (see docs/native/PACKAGING.md)" >&2
    exit 1
  fi
done

# hangar-node for Linux, which the installed app uploads to nodes. Windows
# cannot run them; they travel in the installer and are not renamed, because
# that is the name the app looks for.
node_dir="$release_dir/../node"
for arch in x86_64 aarch64; do
  if [ ! -f "$node_dir/hangar-node-$arch" ]; then
    echo "$node_dir/hangar-node-$arch is missing: download the release workflow's hangar-node artifacts there (docs/native/PACKAGING.md)" >&2
    exit 1
  fi
done

# The version comes from cargo, so a release bumps one number.
pkgid="$(cd "$repo" && cargo pkgid -p hangar-app)"
version="${pkgid##*[#@]}" # path+file:///.../hangar-app#1.2.3, or #hangar-app@1.2.3
if [ -z "$version" ] || [ "$version" = "$pkgid" ]; then
  echo "could not read hangar-app's version from cargo (cargo pkgid said: $pkgid)" >&2
  exit 1
fi
# VIProductVersion takes four numbers: 1.13.0-beta.1 becomes 1.13.0.0.
numeric="${version%%[-+]*}"
version_with_build="$numeric.0"

sha1_of() {
  if command -v sha1sum > /dev/null; then sha1sum "$1" | cut -d' ' -f1; else shasum -a 1 "$1" | cut -d' ' -f1; fi
}

# Everything the script reads sits in one folder, which is makensis's working
# directory: relative paths in the script then mean the same thing on every
# system, and nothing has to be converted to a Windows path.
work="$out_dir/nsis-work"
rm -rf "$work"
mkdir -p "$work/plugins"

dll="$work/plugins/nsis_tauri_utils.dll"
cache="${NSIS_PLUGIN_CACHE:-}"
if [ -n "$cache" ] && [ -f "$cache" ] && [ "$(sha1_of "$cache")" = "$plugin_sha1" ]; then
  cp "$cache" "$dll"
else
  curl --fail --silent --show-error --location --output "$dll" "$plugin_url"
fi
if [ "$(sha1_of "$dll")" != "$plugin_sha1" ]; then
  echo "nsis_tauri_utils.dll does not have the checksum the Tauri bundler pinned" >&2
  exit 1
fi

cp "$here/installer.nsi" "$here/theme.nsh" "$here/utils.nsh" "$here/backdrop.bmp" "$here/backdrop@2x.bmp" "$work/"
cp "$icons/icon.ico" "$work/icon.ico"
cp "$release_dir/hangar-ai.exe" "$release_dir/hangar-webview.exe" "$work/"
cp "$node_dir/hangar-node-x86_64" "$node_dir/hangar-node-aarch64" "$work/"
# NSIS reads these as UTF-8 only with the byte order mark in front.
for language in English French; do
  { printf '\357\273\277'; cat "$here/languages/$language.nsh"; } > "$work/$language.nsh"
done

size=$(( $(wc -c < "$release_dir/hangar-ai.exe") + $(wc -c < "$release_dir/hangar-webview.exe") + $(wc -c < "$node_dir/hangar-node-x86_64") + $(wc -c < "$node_dir/hangar-node-aarch64") ))
estimated="$(printf '0x%06x' $(( size / 1000 )))"

installer="Hangar.AI_${version}_x64-setup.exe"
(
  cd "$work"
  "$makensis" -V3 \
    "-DVERSION=$version" \
    "-DVERSIONWITHBUILD=$version_with_build" \
    "-DESTIMATEDSIZE=$estimated" \
    "-DOUTFILE=$installer" \
    installer.nsi
)

mv "$work/$installer" "$out_dir/$installer"
rm -rf "$work"
echo "built $out_dir/$installer"
