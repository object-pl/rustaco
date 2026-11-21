#!/usr/bin/env sh
# Every check CI runs, runnable locally. `--e2e` adds the browser tests.
set -e
cd "$(dirname "$0")/.."

echo "== format"
cargo fmt --all --check

echo "== clippy (native)"
cargo clippy --workspace --all-targets -- -D warnings

echo "== clippy (wasm)"
. ./scripts/wasm-env.sh
cargo clippy -p rustaco-web --target wasm32-unknown-unknown -- -D warnings
cargo clippy -p rustaco-syntax-wasm --features rust --target wasm32-unknown-unknown -- -D warnings

echo "== tests"
cargo test --workspace

echo "== licenses and advisories"
cargo deny check

echo "== wasm build and size budget"
./scripts/build-web.sh
./scripts/check-size.sh

if [ "$1" = "--e2e" ]; then
  echo "== browser tests"
  (cd tests/e2e && npm ci && npx playwright test)
fi
