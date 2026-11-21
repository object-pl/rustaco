#!/usr/bin/env sh
# Builds the browser editor into web/pkg: the editor module, plus one language module per
# pack (web/pkg/syntax-<pack>/), loaded by the page the first time a language needs it.
# Requires wasm-bindgen-cli matching the crate version, and clang for the C grammars.
# `PACKS="rust json" ./scripts/build-web.sh` limits the language modules built.
set -e
cd "$(dirname "$0")/.."
. ./scripts/wasm-env.sh

PROFILE=wasm-release
OUT=target/wasm32-unknown-unknown/$PROFILE

cargo build --profile $PROFILE -p rustaco-web --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --out-dir web/pkg "$OUT/rustaco_web.wasm"

# SQL's tree-sitter grammar compiles to 1.2 MB gzipped; the browser gets SQL colours from
# the TextMate fallback instead, and the desktop build keeps the grammar.
SKIP_PACKS=${SKIP_PACKS:-sql}
ALL_PACKS=$(sed -n 's/^\([a-z-]*\) = \["rustaco-syntax\/.*/\1/p' crates/rustaco-syntax-wasm/Cargo.toml \
  | grep -vxF "$(printf '%s\n' $SKIP_PACKS)")
for pack in ${PACKS:-$ALL_PACKS}; do
  cargo build --profile $PROFILE -p rustaco-syntax-wasm --target wasm32-unknown-unknown --features "$pack"
  wasm-bindgen --target web --no-typescript --out-dir "web/pkg/syntax-$pack" "$OUT/rustaco_syntax_wasm.wasm"
done

mkdir -p web/pkg/licenses && cp licenses/*.LICENSE LICENSE web/pkg/licenses/ && cp THIRD_PARTY.md web/pkg/
ls -l web/pkg/rustaco_web_bg.wasm web/pkg/syntax-*/rustaco_syntax_wasm_bg.wasm
