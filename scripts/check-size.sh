#!/usr/bin/env sh
# Fails when a gzipped WASM module exceeds its budget from ARCHITECTURE.md: the editor
# module, and each language module separately.
set -e
cd "$(dirname "$0")/.."
EDITOR_BUDGET=${WASM_GZIP_BUDGET:-1048576}
PACK_BUDGET=${WASM_PACK_GZIP_BUDGET:-716800}
status=0

check() {
  size=$(gzip -9 -c "$1" | wc -c | tr -d ' ')
  printf '%-58s %8s bytes gzipped (budget %s)\n' "$1" "$size" "$2"
  if [ "$size" -gt "$2" ]; then
    echo "  over budget" >&2
    status=1
  fi
}

check web/pkg/rustaco_web_bg.wasm "$EDITOR_BUDGET"
for pack in web/pkg/syntax-*/rustaco_syntax_wasm_bg.wasm; do
  [ -f "$pack" ] && check "$pack" "$PACK_BUDGET"
done
exit $status
