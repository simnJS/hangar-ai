# Build from source

[← Hangar](../../README.md) · [Guide](README.md)

You need:

- **Rust** stable, and `rustup target add wasm32-unknown-unknown` for native kit tabs;
- **cmake** and **LLVM/clang**: dictation compiles whisper.cpp from source and generates
  its bindings with libclang;
- for your system:
  - **Windows:** the MSVC build tools, plus cmake and LLVM
    (`winget install Kitware.CMake LLVM.LLVM`), and WebView2 for the pages the webview
    helper shows (HTML kit tabs, PDFs);
  - **macOS:** the Xcode command line tools (`xcode-select --install`, which includes
    clang) and `brew install cmake`;
  - **Linux:** the ALSA, libclang and OpenSSL headers, and WebKitGTK for the webview
    helper, as installed by CI on Ubuntu 22.04:

    ```bash
    sudo apt install build-essential cmake libwebkit2gtk-4.1-dev libssl-dev libasound2-dev libclang-dev
    ```
- **Node.js** 22 and **pnpm** 10, only for Hangar Mobile, the phone app: a build without it
  serves the phone a page saying the app is missing.

Then:

```bash
scripts/native-dev.sh             # build and run Hangar, on data of its own under target/dev-home/
scripts/native-dev.sh --release   # the same, optimised
cargo build -p hangar-webview --features engine   # the helper, for HTML kit tabs and PDFs
cargo test --workspace            # the tests
pnpm install
pnpm mobile:build                 # Hangar Mobile into dist-mobile/, which the next build embeds
pnpm mobile:dev                   # Hangar Mobile against a fake PC, on http://localhost:4310
pnpm test                         # Hangar Mobile's tests
```

The installers are built by the scripts under `crates/hangar-app/packaging/`, as the
release workflow runs them: see [docs/native/PACKAGING.md](../native/PACKAGING.md).

> [!WARNING]
> If Hangar is already installed on your machine, a plain `cargo run -p hangar-app` shares
> its data and its MCP endpoint, and your installed agents would then talk to the dev
> build. `scripts/native-dev.sh` keeps them apart; see
> [CONTRIBUTING.md](../../CONTRIBUTING.md#running-a-dev-build-next-to-an-installed-hangar).

## Repository layout

```
crates/hangar-app/       the app, `hangar-ai`: windows, views and state; packaging/ holds the installers
crates/hangar-core/      everything that is not drawing: PTYs, board, memory, MCP server, git, deliveries,
                         stats, dictation, Hangar Mobile's server, the kits and their HTML SDK
crates/hangar-core/claude-mod/hangar-bridge/
                         the Claude Code mod that reports session state to Hangar
crates/hangar-term/      the terminal widget, on alacritty_terminal
crates/hangar-theme/     the 33 themes and the design tokens
crates/hangar-ui/        the widgets every view is built from: buttons, menus, dialogs, the palette…
crates/hangar-i18n/      the interface strings, English and French (dict/en.json, dict/fr.json)
crates/hangar-kit-ui/    the widgets and events of native kit tabs, as data: the public contract
crates/hangar-kit-sdk/   what a kit links to write a native tab
crates/hangar-kit-host/  runs native kit tabs in wasmtime and draws them
crates/hangar-webview/   the helper for HTML kit tabs, PDFs and web pages, and the app's client for it
crates/hangar-model3d/   the 3D preview
crates/hangar-update/    the updater
mobile/                  Hangar Mobile, the phone app, built into dist-mobile/ and embedded in the binary
examples/kits/           example kits, among them hello-native, a native tab
assets/                  the logo, the README's images and the sample 3D models
site/                    the website at hangar.simnjs.fr
scripts/                 the dev launcher, the kits' stylesheet generator, logo and installer art
docs/                    kits, Hangar UI, and the native rewrite's plan, status and contracts (docs/native/)
.github/workflows/       CI (checks on pushes and pull requests) and Release (builds and publishes on a tag)
```
