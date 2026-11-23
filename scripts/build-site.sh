#!/usr/bin/env sh
# Builds the project page (site/) with a copy of the browser editor into target/site, ready
# for GitHub Pages. Run scripts/build-web.sh first; this only copies.
set -e
cd "$(dirname "$0")/.."
OUT=target/site
rm -rf "$OUT"
mkdir -p "$OUT/editor"
cp site/index.html site/site.css site/site.js "$OUT/"
cp web/rustaco.js web/rustaco-worker.js web/languages.js web/suggest.js web/features.js web/lsp.js web/diff.js \
  web/webgl-text.js web/favicon.ico "$OUT/editor/"
cp -r web/pkg "$OUT/editor/pkg"
rm -f "$OUT/editor/pkg/.gitignore" "$OUT/editor/pkg"/*/.gitignore
# GitHub Pages: serve files as they are (no Jekyll).
touch "$OUT/.nojekyll"
du -sh "$OUT"
