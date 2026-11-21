#!/usr/bin/env sh
# Fails when the gzipped WASM module exceeds the budget from ARCHITECTURE.md.
set -e
cd "$(dirname "$0")/.."
BUDGET=${WASM_GZIP_BUDGET:-1048576}
WASM=web/pkg/rustaco_web_bg.wasm
SIZE=$(gzip -9 -c "$WASM" | wc -c | tr -d ' ')
echo "rustaco_web_bg.wasm: $SIZE bytes gzipped, budget $BUDGET"
if [ "$SIZE" -gt "$BUDGET" ]; then
  echo "over budget" >&2
  exit 1
fi
