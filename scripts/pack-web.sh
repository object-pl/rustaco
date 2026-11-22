#!/usr/bin/env bash
# Builds the browser editor and packs it as an npm package in target/npm.
# The package holds rustaco.js, its TypeScript types, the editor module and one module per
# language pack (loaded on demand), plus the license files.
set -euo pipefail
cd "$(dirname "$0")/.."

./scripts/build-web.sh
stage=target/npm/package
rm -rf "$stage"
mkdir -p "$stage"
cp web/package.json web/*.js web/*.d.ts web/favicon.ico LICENSE "$stage/"
rm -f "$stage/csp.js" "$stage/demo-language.js"
cp README.md "$stage/README.md"
cp -r web/pkg "$stage/pkg"
(cd target/npm && npm pack ./package --pack-destination . >/dev/null)
ls -l target/npm/*.tgz
