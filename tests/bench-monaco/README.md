# Rustaco against Monaco

The same synthetic Rust files (5,000 and 100,000 lines) in both editors, in one Chromium.
`run.mjs` reports load time (text in, two frames drawn), main-thread work per keystroke
(Chromium's task time over 60 keys, idle time subtracted), keydown-to-frame latency, and
scroll latency. `profile.mjs <rustaco|monaco> <lines>` prints a CPU profile of typing.

Setup in this directory (downloads Monaco and Chromium from npm, kept out of the repository):

```
npm init -y && npm pkg set type=module
npm install monaco-editor@0 playwright@1 && npx playwright install chromium
cp -r ../../web rustaco        # after ./scripts/build-web.sh
node run.mjs
```

Results are in `docs/performance.md`.
