#!/bin/sh
# Builds the tab to WebAssembly and puts it where the manifest's entry says.
# Run from anywhere; needs `rustup target add wasm32-unknown-unknown` once.
set -eu
kit="$(cd "$(dirname "$0")" && pwd)"
cd "$kit/../../.."
cargo build -p hello-native-tab --release --target wasm32-unknown-unknown
mkdir -p "$kit/tabs"
cp target/wasm32-unknown-unknown/release/hello_native_tab.wasm "$kit/tabs/hello.wasm"
echo "$kit/tabs/hello.wasm"
