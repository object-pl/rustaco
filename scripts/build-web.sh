#!/usr/bin/env sh
# Builds the WASM frontend into web/pkg. Requires wasm-bindgen-cli matching the crate version.
set -e
cd "$(dirname "$0")/.."
cargo build --release -p rustaco-web --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --out-dir web/pkg target/wasm32-unknown-unknown/release/rustaco_web.wasm
ls -l web/pkg/rustaco_web_bg.wasm
