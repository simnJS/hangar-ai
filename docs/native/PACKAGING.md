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
| `hangar-node-<v>-linux-x86_64`, `hangar-node-<v>-linux-aarch64` | `cargo build -p hangar-node`, stripped | none: a download only (see "hangar-node" below) | |
| `<each of the above but the .dmg>.sig` | `cargo packager signer sign` | | |
| `latest.json` | `packaging/latest-json.sh` | | |

### hangar-node

Every desktop package also carries the headless node daemon for Linux x86_64 and
arm64, as two files: `hangar-node-x86_64` and `hangar-node-aarch64`. They are
packager **resources**, not binaries (`resources` in
`crates/hangar-app/Cargo.toml`). The desktop uploads the one that matches a
node's `uname -m` over SSH (`crates/hangar-app/src/nodes/model.rs`,
`BinarySource`: `HANGAR_NODE_BINARY`, else `hangar-node-<arch>` looked for in
three places from the running `hangar-ai`'s folder: beside it, in
`../Resources` and in `../lib/hangar-ai`), and a Mac or a Windows desktop
drives Linux nodes too, so all three platforms carry both:

| Package | Where |
| --- | --- |
| `.deb`, `.rpm` | `/usr/lib/hangar-ai/hangar-node-{x86_64,aarch64}` |
| AppImage | `usr/lib/hangar-ai/` in the image (the executable is in `usr/bin/`) |
| macOS `.app` | `Contents/Resources/` |
| Windows installer | the install folder, beside the executable, removed by the uninstaller |

They are not programs the desktop's own system runs. On macOS they are not in
`Contents/MacOS`: everything there is signed as nested code, and codesign
refuses these ELF files ("code object is not signed at all"), whereas
`Contents/Resources` takes them as data. The same two files are on
the release as `hangar-node-<v>-linux-x86_64` and `-linux-aarch64`, for a node
set up by hand (`chmod +x`, then `hangar-node serve`); not in `latest.json`, not
signed.

The release workflow's `node` job builds them, before the build jobs, which
download them into `target/node` (`actions/download-artifact`) and whose
packaging scripts refuse to run without them: `common.sh`'s
`stage_node_binaries` checks that each is an ELF file of the right machine and
leaves it in `target/node`, where the packager reads resources (the
`resources` and the `generate-rpm` assets of `crates/hangar-app/Cargo.toml`
list them there); the Windows script hands them to `installer.nsi`. The job:

- builds on `ubuntu-22.04`, so the binary wants glibc 2.35 at most (checked, and
  the job fails otherwise), like the AppImage;
- builds arm64 by **cross-compiling** on that same x86_64 runner
  (`gcc-aarch64-linux-gnu`, `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER`)
  and not on `ubuntu-22.04-arm`: arm runners are not offered to every
  repository, a job waiting for one would hold the release, and hangar-node has
  no native dependency beyond C that a cross compiler cannot do. No linker is
  set in `.cargo/config.toml`, because one named there would break a build on a
  real arm64 machine, where the compiler is called `gcc`;
- strips both and uploads them as artifacts, and to the draft by id.

`ci.yml` cross-builds the arm64 one on every push, so a break shows before a tag.

By hand (the two files into `target/node`, then the platform's script as in
section 3):

```bash
rustup target add aarch64-unknown-linux-gnu           # and apt install gcc-aarch64-linux-gnu
mkdir -p target/node
cargo build --profile dist --locked -p hangar-node
strip -o target/node/hangar-node-x86_64 target/dist/hangar-node
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
  cargo build --profile dist --locked -p hangar-node --target aarch64-unknown-linux-gnu
aarch64-linux-gnu-strip -o target/node/hangar-node-aarch64 target/aarch64-unknown-linux-gnu/dist/hangar-node
```

(Run on the host's own glibc, the result wants that glibc: it is the workflow's
22.04 that makes it 2.35.) `qemu-user-binfmt` and Docker run the arm64 one:
`docker run --rm --platform linux/arm64 -v target/node:/n:ro debian:trixie-slim /n/hangar-node-aarch64 version`.

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

### The dist profile

What is released is built with `cargo build --profile dist` (the workspace
`Cargo.toml`): `release` plus `lto = "fat"` and `codegen-units = 1`, written to
`target/dist`, which is where the three packaging scripts, `cargo packager
--profile dist` and `cargo generate-rpm --profile dist` read it from. `cargo
build --release` is left as cargo has it, for working on the app: a one-line
change in hangar-app rebuilds in about 30 seconds with it and in 5 minutes with
dist, because fat LTO redoes the whole program's optimisation on every link.

Measured on a 32-core Linux machine shared with other builds (Rust 1.99.1, the
load average noted with each run), each variant built from a clean
`target/release` with `CARGO_PROFILE_RELEASE_*` overrides, one build at a time.
The runs are `bench/harness/resume.py open4` (time to all four prompts, memory)
and `busy8` (app CPU from 10 to 40 s with eight busy agents), three of each per
variant in alternating order, and `cargo bench -p hangar-term --bench feed`
(`--sample claude`; `--pty --ls`, five runs per round, three rounds). The
values are medians; the one-minute load was between 0.4 and 6.4 during these runs.

| Variant | Cold build | hangar-ai (stripped) | hangar-node (stripped) | open4 all prompts | open4 PSS | busy8 PSS | busy8 CPU | feed claude | feed ls | pty-ls reader |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| release as it was (no LTO, 16 units) | 80 s | 118.9 MB (85.3) | 17.0 MB (11.9) | 0.393 s | 181.4 MB | 207.1 MB | 6.0 % | 236.9 MB/s | 141.3 MB/s | 77.5 MB/CPU-s |
| `lto = "thin"` | 110 s | 118.2 MB (86.6) | 17.0 MB (12.1) | 0.383 s | 183.6 MB | 209.5 MB | 5.8 % | 234.7 | 138.1 | 75.2 |
| `lto = "fat"` | 429 s | 84.2 MB (70.0) | 10.6 MB (8.7) | 0.401 s | 179.8 MB | 205.3 MB | 5.5 % | 245.2 | 157.3 | 82.2 |
| thin, `codegen-units = 1` | 224 s | 88.8 MB (71.4) | 13.1 MB (10.3) | 0.414 s | 177.8 MB | 203.0 MB | 6.9 % | 228.3 | 130.4 | 70.7 |
| **fat, `codegen-units = 1` (dist)** | 416 s | **78.8 MB (65.7)** | **10.0 MB (8.3)** | 0.396 s | **176.4 MB** | **202.3 MB** | 6.1 % | **247.9** | 147.5 | 80.5 |

What the table says:

- **Start-up and CPU at rest do not move.** The time to all prompts spreads
  over ±30 ms from run to run in every variant, and busy8's CPU over a point or
  two; none of the variants is outside that.
- **Size** is where LTO pays: fat LTO with one unit makes `hangar-ai` a third
  smaller and hangar-node 41% smaller (8.3 MB stripped instead of 11.9 MB: the
  file the desktop uploads to each node over SSH). Thin LTO with sixteen units
  saves nothing.
- **Parsing a pane's output** (hangar-term) is about 4% faster with fat LTO
  (claude +4.6%, ls +4.4%, the PTY reader thread +3.9% MB per CPU second), and
  the app holds about 5 MB less. Thin LTO with one unit was no faster than no
  LTO at all.
- Fat LTO with sixteen units is as fast as with one (ls a little faster, claude
  a little slower) and 5 MB bigger, for the same build time; one unit is kept
  for the size.
- A second cold build of the chosen profile took 395 s (load 4.8), the same as
  the variant's 416 s. A cold build of the release workflow's runners will take
  a few minutes longer than it did; the workflow sets no timeout.

`strip` stays at `"debuginfo"`, which is what `release` already does when it
has no debug info: the symbol table remains (13 MB of the dist `hangar-ai`, a
sixth of it), and it is what a panic's backtrace is made from. The same panic in
a program built both ways, with `RUST_BACKTRACE=1`:

```
strip = "debuginfo"                          strip = "symbols"
stack backtrace:                             stack backtrace:
   0: __rustc::rust_begin_unwind             note: Some details are omitted, ...
   1: core::panicking::panic_fmt
   2: core::panicking::panic_bounds_check
   3: bt::parse_pane_output
   4: bt::main
```

With `"symbols"` the backtrace is empty, and a bug report loses the one thing
that says where the panic came from. (hangar-node is still stripped by the
release workflow: nobody reads a node's backtrace from its file.) `panic` stays
`"unwind"`: the app catches panics with `catch_unwind` around the image and
AVIF decoders, the 3D loader, kit tabs, dictation and the git consent
callbacks, and `"abort"` would turn each of those into a crash.

## 3. Building each bundle by hand

Common to all: `pnpm install && pnpm mobile:build` first (Hangar Mobile's
`dist-mobile/` is compiled into the executable by hangar-core), then

```bash
crates/hangar-app/packaging/check-mobile.sh                # dist-mobile/ is whole
cargo build --profile dist --locked -p hangar-app
cargo build --profile dist --locked -p hangar-webview --features engine
crates/hangar-app/packaging/check-mobile.sh target/dist/hangar-ai      # and it is in the binary
```

The app and the helper are built separately so that `hangar-ai` is never built
with the helper's `engine` feature: the main executable links no browser engine
(`ldd` shows libc, libm, libgcc_s, libstdc++ and libasound only). The helper
(`hangar-webview`) is the only thing that links WebKitGTK, WebView2 or
WKWebView. Every bundle puts it next to `hangar-ai`, which is where
`hangar_webview::find_helper` looks.

**Linux** (`.AppImage`, `.deb`, `.rpm`). Needs `jq`, the build dependencies
listed in `.github/workflows/ci.yml`, and
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
  cargo build --profile dist --locked --target $t -p hangar-app
  cargo build --profile dist --locked --target $t -p hangar-webview --features engine
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
  crates/hangar-app/packaging/windows/build.sh target/dist target/packager
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
2. **node** (x86_64 and arm64, at once; see "hangar-node" in section 1) builds
   the node daemon the packages carry, and puts it on the draft.
3. **build** (three at once, after `node`): Defender exclusion on Windows; build dependencies
   on Linux; `pnpm mobile:build` and `check-mobile.sh`; the Apple secrets and
   whisper.cpp's portable-AVX2 pins as before; `MACOSX_DEPLOYMENT_TARGET=10.15`
   (what `minimumSystemVersion` did for the Tauri build); the two cargo builds;
   `check-mobile.sh` again on the executable; the `hangar-node` files downloaded; the packaging script of the
   platform; upload to the draft by id (`upload-assets.sh`, which replaces an
   asset of the same name, so a re-run is harmless).
4. **publish** downloads the updater artifacts from the draft, **signs** them
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
| `.deb` | Package `hangar-ai`, so `dpkg -i` upgrades the old one. It depends on `libwebkit2gtk-4.1-0` and GTK as the old one did, and now on `libasound2`, `libstdc++6`, `libc6`. The desktop file is `hangar-ai.desktop` (the old one was `Hangar.AI.desktop`; dpkg removes it), with `StartupWMClass=hangar-ai`. Icons go in the `hicolor` folders, from 16 to 512 px plus `scalable/apps/hangar-ai.svg`: a rounded tile with room around it, as GNOME and KDE draw app icons (`packaging/icons/linux/`, rendered from its SVG by `render.sh`). |
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
  `packaging/icons/linux/128x128.png` on Linux and `packaging/icons/128x128.png`
  elsewhere; the executable's own icon is handled by `build.rs`.
- **`hangar-webview` has no Windows icon or version resource**: a `build.rs` there
  would make the helper's taskbar entry match.

Wave 5 removed `src-tauri/`: the installer, its bitmaps, the icons and
`Info.plist` live under `crates/hangar-app/packaging/` only, and
`scripts/installer-art.ps1` writes the NSIS backdrops there (the WiX images went,
since no `.msi` ships). The Tauri-only installer preview
(`scripts/preview.nsi`, `scripts/preview-installer.ps1`) went with it:
`packaging/windows/build.sh` compiles the real script in seconds. Nothing checks
the packaging between releases: a Linux job in `ci.yml` that runs
`packaging/windows/build.sh` with `apt install nsis` and
`packaging/linux/build.sh` would catch a broken installer script or packager
configuration before a tag does.
