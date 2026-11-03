# Hangar, native — packaging and releases

How the native Hangar is turned into the installers people download, and into
the update that installs of the old Tauri app and of this one both pick up. The
plan is in [PLAN.md](PLAN.md) (section 3.3 lists what must stay compatible;
this file is where each item is kept), the log in [STATUS.md](STATUS.md).

The workflow is `.github/workflows/release.yml`; everything it does to a file is
a script under `crates/hangar-app/packaging/`, so any step can be run on a
developer's machine with the same commands.

## 1. What a release produces

One draft release, filled by three build jobs and published by a fourth step
once its feed has been checked. The names are the Tauri build's, because the
download page and `latest.json` link to them.

| Asset | Made by | Updater keys it answers for | Replaced in place by |
| --- | --- | --- | --- |
| `Hangar.AI_<v>_amd64.AppImage` | cargo-packager | `linux-x86_64`, `linux-x86_64-appimage` | a rename over `$APPIMAGE` |
| `Hangar.AI_<v>_amd64.deb` | cargo-packager | `linux-x86_64-deb` | `pkexec dpkg -i` |
| `Hangar.AI-<v>-1.x86_64.rpm` | cargo-generate-rpm | `linux-x86_64-rpm` | `pkexec rpm -U` |
| `Hangar.AI_<v>_x64-setup.exe` | `makensis` | `windows-x86_64`, `windows-x86_64-nsis` | run with `/P /R /UPDATE /ARGS …` |
| `Hangar.AI_universal.app.tar.gz` | tar, from the cargo-packager `.app` | `darwin-{aarch64,x86_64}`, and the same with `-app` | the bundle swapped for the new one |
| `Hangar.AI_<v>_universal.dmg` | cargo-packager | none: a download only | |
| `<each of the above but the .dmg>.sig` | `cargo packager signer sign` | | |
| `latest.json` | `packaging/latest-json.sh` | | |

`latest.json` is `{ version, notes, pub_date, platforms: { key: { signature, url } } }`,
the shape `hangar-update` parses (`crates/hangar-update/src/manifest.rs`) and
`tauri-plugin-updater` did. `signature` is the `.sig` file's text as it is (a
minisign signature of the file's BLAKE2b hash, base64-encoded). Both the plain
key and the per-installer one are written, as before: an install of the old app
asks for the plain one, the native app for the specific one first.

The `.app.tar.gz` holds one top-level `Hangar.AI.app`, which `hangar-update`
unpacks by dropping that first path component (as the plugin did).

## 2. The tools, and why

| Format | Tool |
| --- | --- |
| AppImage, `.deb`, macOS `.app` and `.dmg`, signing | **cargo-packager** 0.11.8, pinned |
| `.rpm` | **cargo-generate-rpm** 0.21.1, pinned |
| Windows installer | **`makensis`** on the vendored Tauri NSIS script, no bundler |
| Icon and version of `hangar-ai.exe` | `winresource`, from `crates/hangar-app/build.rs` |

**cargo-packager** is the Tauri bundler, extracted from Tauri and kept by
CrabNebula: the same `.deb` and `.app` code, the same create-dmg, the same
codesign-and-notarise sequence (it reads the secrets' `APPLE_*` names the old
workflow already used), the same minisign signer and key format (a key made by
`tauri signer generate` is read as it is, and its `.sig` files are what
`hangar-update` verifies; checked, see section 5). Its configuration is a
`[package.metadata.packager]` table in `crates/hangar-app/Cargo.toml`, so the
version, description, authors and executable names come from the package and a
release bumps one number. What the alternatives would have cost:

- *Hand-written scripts for all of it* is not much for a `.deb` and an AppImage
  and a lot for macOS: lipo, bundle layout, signing every Mach-O inside out,
  notarising, stapling, the disk image's layout, and keeping all of that
  working across macOS runner updates. cargo-packager has been through that.
- *The `tauri-bundler` crate as a library* would give exactly the old output for
  every format, including the `.rpm`, but its API is internal to the Tauri CLI
  and it bundles a Tauri app in mind (AppImage with GTK and WebKit, the
  `tauri.conf.json` model). The point of this branch is to stop carrying that.
- *cargo-packager for the NSIS installer too* does not work. Hangar's installer
  is the bundler's 2.9 script with its own pages (`theme.nsh`), the migration of
  installs made under earlier product names (`Hangar.IA`, `IaBench`) and, above
  all, the `/UPDATE` and `/ARGS` handling that `hangar-update` needs. cargo-packager
  carries an older fork of the script: no `/UPDATE`, no `/ARGS`, an older plugin
  (v0.2.1, which lacks `RunAsUser`, `FindProcessCurrentUser`,
  `KillProcessCurrentUser` and `StrReplace`, all called by the script and its
  `utils.nsh`), no `utils.nsh`. So the
  Windows installer is built by `packaging/windows/build.sh`, which runs
  `makensis` on the vendored script and the vendored support files, with the
  plugin the Tauri bundler pinned. That is the same installer as before, minus
  the handlebars step.

cargo-packager has no `.rpm` format, and the only `.rpm` tool that needs no
`rpmbuild` is `cargo-generate-rpm`.

## 3. Building each bundle by hand

Common to all: `pnpm install && pnpm mobile:build` first (Hangar Mobile's
`dist-mobile/` is compiled into the executable by hangar-core), then

```bash
crates/hangar-app/packaging/check-mobile.sh                # dist-mobile/ is whole
cargo build --release --locked -p hangar-app
cargo build --release --locked -p hangar-webview --features engine
crates/hangar-app/packaging/check-mobile.sh target/release/hangar-ai   # and it is in the binary
```

The app and the helper are built separately so that `hangar-ai` is never built
with the helper's `engine` feature: the main executable links no browser engine
(`ldd` shows libc, libm, libgcc_s, libstdc++ and libasound only). The helper
(`hangar-webview`) is the only thing that links WebKitGTK, WebView2 or
WKWebView. Every bundle puts it next to `hangar-ai`, which is where
`hangar_webview::find_helper` looks.

**Linux** (`.AppImage`, `.deb`, `.rpm`). Needs `jq`, the build dependencies
listed in `.github/workflows/native.yml`, and
`cargo install cargo-packager --version 0.11.8 --locked`,
`cargo install cargo-generate-rpm --version 0.21.1 --locked`.

```bash
crates/hangar-app/packaging/linux/build.sh target/packager
```

The packager downloads its AppImage tools (linuxdeploy and its plugin, then
appimagetool and the type-2 runtime) into `target/packager-cache`. Nothing is
installed on the system; run the AppImage, or unpack it with `--appimage-extract`.
To try the app from a package without installing it, `dpkg-deb -x` the `.deb`
into a folder and run `usr/bin/hangar-ai` with `HANGAR_DATA_ROOT` and
`HANGAR_ENDPOINT_FILE` set to a throwaway folder (as `scripts/native-dev.sh`
does), never to the installed Hangar's.

**macOS.** Build both targets, then `lipo` and package:

```bash
export MACOSX_DEPLOYMENT_TARGET=10.15   # the oldest macOS it runs on, as the workflow sets it
for t in aarch64-apple-darwin x86_64-apple-darwin; do
  cargo build --release --locked --target $t -p hangar-app
  cargo build --release --locked --target $t -p hangar-webview --features engine
done
crates/hangar-app/packaging/macos/build.sh target/packager
```

Unsigned unless `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_ID`,
`APPLE_PASSWORD`, `APPLE_TEAM_ID` are set and the manifest has a
`signingIdentity` under `[package.metadata.packager.macos]` (the workflow
writes it in from its secret; cargo-packager takes it from there and not from
the environment).

**Windows.** On Windows with NSIS 3.11 (the workflow downloads the archive the
Tauri bundler used, `nsis-3.11.zip`, and checks its SHA-1):

```bash
MAKENSIS=/path/to/makensis.exe \
  crates/hangar-app/packaging/windows/build.sh target/release target/packager
```

On Linux the same script compiles the installer, which makes a quick check of
the script and of the plugin's function names (`makensis` reads the DLL's export
table), but the result is a Windows program that has not been run. A Debian or
Ubuntu `nsis` package unpacked with `dpkg-deb -x` into a folder with a **short
path** works (set `NSISDIR` to its `usr/share/nsis`): the Debian build of
makensis overflows a stack buffer on very long include paths.

**Signing and the feed**, with any key (the real one is only in the CI secrets):

```bash
cargo packager signer generate --ci --path /tmp/test-key --password ""
TAURI_SIGNING_PRIVATE_KEY="$(cat /tmp/test-key)" crates/hangar-app/packaging/sign.sh target/packager/Hangar.AI*
TAG=v1.13.0 REPO=hangar-ai/hangar crates/hangar-app/packaging/latest-json.sh target/packager latest.json
cargo run -p hangar-update --example verify_feed -- latest.json target/packager --version v1.13.0 --pubkey /tmp/test-key.pub
```

Without `--pubkey`, `verify_feed` checks against `hangar_update::PUBKEY`, the
key compiled into the app, which is what the workflow does.

## 4. The workflow

Triggers, matrix (`macos-latest` universal, `windows-latest`, `ubuntu-22.04`),
secrets and the draft-first design are unchanged. Linux stays on 22.04: an
AppImage runs on the glibc of the machine that built it or newer, so the
release notes' "glibc 2.35 or later" is that runner's.

1. **draft** creates the release once, as a draft, and passes its id on.
2. **build** (three at once): Defender exclusion on Windows; build dependencies
   on Linux; `pnpm mobile:build` and `check-mobile.sh`; the Apple secrets and
   whisper.cpp's portable-AVX2 pins as before; `MACOSX_DEPLOYMENT_TARGET=10.15`
   (what `minimumSystemVersion` did for the Tauri build); the two cargo builds;
   `check-mobile.sh` again on the executable; the packaging script of the
   platform; upload to the draft by id (`upload-assets.sh`, which replaces an
   asset of the same name, so a re-run is harmless).
3. **publish** downloads the updater artifacts from the draft, **signs** them
   (the only job that sees `TAURI_SIGNING_PRIVATE_KEY`), writes `latest.json`,
   runs **`verify_feed`**, uploads the `.sig` files and the feed, and only then
   makes the release public. `verify_feed` is new: it fails the release if a
   platform the updater can ask for is missing, if an artifact is not what its
   name says (an ELF, an `ar`, an RPM, a PE; a `.app.tar.gz` with one `.app` that
   has the helper beside the executable), or if any signature does not verify
   against the key the installs carry. A signing secret that is not the old key
   used to be found by the first person who tried to update; now the release
   stays a draft.

Secrets, all as before: `TAURI_SIGNING_PRIVATE_KEY` and `_PASSWORD`,
`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`,
`APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`. The names are kept so nothing
has to change in the repository settings.

## 5. Compatibility with installs of the old app

| What an install knows | Kept how |
| --- | --- |
| The feed and the key | Same URL (`ENDPOINT`), same `latest.json` shape, signed by the same private key; `hangar-update` and the plugin verify the same `.sig` text. Checked with a key made by cargo-packager's `signer generate` (the same format as `tauri signer generate`) through `hangar-update`'s `verify_file`; the shipped `PUBKEY` is read by it too. The real key could not be tried here. |
| Asset names | The Tauri build's, in every row of section 1; the download page links to them. |
| Identifier, product name, executable | `com.simnjs.iabench`, `Hangar.AI`, `hangar-ai` (so `hangar-ai --mcp`, found by agents' MCP configs, stays at `/usr/bin/hangar-ai`, `…/Hangar.AI.app/Contents/MacOS/hangar-ai`, `%LOCALAPPDATA%\Hangar.AI\hangar-ai.exe`). |
| `.deb` | Package `hangar-ai`, so `dpkg -i` upgrades the old one. It depends on `libwebkit2gtk-4.1-0` and GTK as the old one did, and now on `libasound2`, `libstdc++6`, `libc6`. The desktop file is `hangar-ai.desktop` (the old one was `Hangar.AI.desktop`; dpkg removes it), with `StartupWMClass=hangar-ai`; icons in the same `hicolor` folders. |
| `.rpm` | Package `Hangar.AI`, as the Tauri build named it, so `rpm -U` upgrades. Requires are written by soname (`libasound.so.2`, `libstdc++.so.6`, `libgtk-3.so.0`, `libwebkit2gtk-4.1.so.0`) rather than the ldd scan cargo-generate-rpm does by default, which would list the build machine's own ICU and GStreamer sonames. |
| AppImage | A single file the updater renames over `$APPIMAGE`, as before. Its own `AppRun` (`packaging/linux/AppRun`) replaces the packager's, which exported `PYTHONHOME` and `PYTHONPATH` into the image and so broke every Python started in a pane. The image bundles no libraries; it is 20 MB, not 91. |
| Windows | Per-user (`currentUser`), `%LOCALAPPDATA%\Hangar.AI`, the same uninstall and `Software\simnjs\Hangar.AI` registry keys, so it finds the old install and replaces it in place. `/P /R /UPDATE /ARGS …` are read by the script as before. The migration of installs under the earlier names stays. The look is `theme.nsh` and the backdrops, copied. WebView2 is still fetched when missing (the helper needs it for HTML kit tabs and PDFs), not for the app. |
| macOS | `Hangar.AI.app`, `com.simnjs.iabench`, minimum macOS 10.15, the microphone usage string merged into `Info.plist` from `packaging/macos/Info.plist`, universal. |

What changes for people:

- The AppImage and the `.deb` no longer carry or require GTK for the app itself;
  HTML kit tabs and PDF previews (the helper) still need WebKitGTK 4.1 on the
  system. The release notes say so.
- No `.msi` (it was never shipped; the WiX banner and dialog images were not
  carried over).
- `hangar-ai.exe` takes its icon and version information from `build.rs`
  (`winresource`) instead of `tauri-build`.

## 6. Verified, and not

Done on Linux with the real release build (1.12.1, `x86_64`):

- The `.deb`: `dpkg-deb -I` and `-c` (package `hangar-ai`, the dependencies above,
  `usr/bin/hangar-ai` and `hangar-webview` side by side, the three icon sizes
  the old one had, the desktop file). The extracted `usr/bin/hangar-ai` ran on a
  throwaway data folder, and `hangar-webview` answered `ready` next to it.
- The AppImage: extracted with `--appimage-extract` (no bundled libraries, our
  `AppRun`, the desktop file and icon), run as the file itself and from the
  extracted tree on a throwaway folder; the process had loaded the host's
  Wayland, xkbcommon, Vulkan and EGL libraries. Under the real runtime, with a
  probe in place of the app, the only variables added to the caller's
  environment are the runtime's `APPIMAGE`, `APPDIR`, `ARGV0` and `OWD`: the
  first two are what `Install::detect` looks for. (With the packager's own
  `AppRun` there were also `PATH`, `LD_LIBRARY_PATH`, `XDG_DATA_DIRS`,
  `PYTHONHOME`, `PYTHONPATH` and `PERLLIB` pointing into the image.)
- The `.rpm`: header and payload read by hand (no `rpm` tools here): name,
  version, release, architecture, the four requires, the six files.
- The macOS `.app` bundle assembled on Linux (no signing, no dmg): layout,
  `Info.plist` with the merged microphone string, `--target universal-apple-darwin`
  finding `target/universal-apple-darwin/release`, the tarball's single `.app`.
- The Windows installer script compiled with makensis 3.10 on Linux, with a
  stand-in for each executable. `makensis` refuses a call to a plugin function
  the DLL does not export, so every `nsis_tauri_utils::` call is known to exist
  in v0.5.3; all the language strings and macros resolve.
- Signing and the feed with a throwaway key: five artifacts signed through the
  Tauri secret names, `latest.json` written with the ten keys, and
  `verify_feed` accepting it, then refusing it with the shipped key, with a
  changed byte, and with a missing Windows signature.

Not done, and so the first things to watch on the first release:

- Anything on Windows or macOS at run time: the installer itself, the NSIS
  archive download, `winresource` finding `rc.exe`, `lipo`, codesigning,
  notarisation and the disk image. They are the packager's and the bundler's
  own steps, called with this repository's values.
- Installing the `.deb` or `.rpm` (nothing was installed system-wide), and an
  update from a real old install through each of them.
- The real signing key against `PUBKEY`; the publish job will say.
- An unsigned macOS build (no `APPLE_CERTIFICATE`): the bundle carries only the
  linker's ad-hoc signature on the executables, and what the Tauri build did
  in that case was not compared.
- cargo-packager is pre-1.0 and its last release was in November 2025. Its
  AppImage step downloads linuxdeploy, `linuxdeploy-plugin-appimage`,
  appimagetool and the runtime from their `continuous` releases, which are not
  pinned or checked. If the packager is ever abandoned, what it does here is
  `deb::generate_data` plus `appimagetool`, and `tauri-bundler` remains the
  fallback.

## 7. Left for the app crate

Not packaging, but a package cannot do them (the brief keeps app sources out of
this work):

- **The window's application id.** The desktop files say
  `StartupWMClass=hangar-ai`; on Wayland the match is by the window's `app_id`.
  `crates/hangar-app` sets none today (no `application_id` in the iced window
  settings), so a launcher's icon would not attach to the window. Set
  `window::Settings::platform_specific.application_id` to `"hangar-ai"` for the
  main window on Linux.
- **The window icon** for X11 and Windows title bars, from
  `packaging/icons/icon.png`; the executable's own icon is handled by `build.rs`.
- **`hangar-webview` has no Windows icon or version resource**: a `build.rs` there
  would make the helper's taskbar entry match.

And for wave 5, when `src-tauri/` goes: the installer, its bitmaps, the icons and
`Info.plist` now have copies under `crates/hangar-app/packaging/` (git stores the
identical files once), and the release no longer reads `src-tauri/` at all.
`scripts/preview.nsi`, `scripts/preview-installer.ps1` and
`scripts/installer-art.ps1` still point at `src-tauri/installer` and
`src-tauri/icons`; they should be repointed at `packaging/windows` and
`packaging/icons` (the art script writes the backdrops, which would otherwise
drift from the copies the installer is built from). `ci.yml` still builds
`src-tauri`. Nothing checks the packaging between releases: a Linux job in
`native.yml` that runs `packaging/windows/build.sh` with `apt install nsis` and
`packaging/linux/build.sh` would catch a broken installer script or packager
configuration before a tag does.
