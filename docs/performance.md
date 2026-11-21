# Performance

Measured on the development machine (Windows 11, release builds; browser numbers in Playwright's Chromium, Firefox and WebKit). Every figure comes from a benchmark or test in the repository, named in the last column.

## Editing and drawing

| What | Result | Where |
|---|---|---|
| Load a 1,000,000-line file (54 MB) into the model | 820 ms in Chromium | ARCHITECTURE.md, measured in the browser; native: `cargo bench -p rustaco-view` ("load 1M lines") |
| Keystroke with a full frame, 1,000,000-line file, no highlighting | 0.46 ms in Chromium | ARCHITECTURE.md, measured in the browser |
| Same, native frontend (1400x900 software frame) | 0.47 ms; the frame alone 0.23 ms | `cargo run -p rustaco-desktop --example frame_timing --release` |
| Layout of a frame at the middle of 1,000,000 lines | 53 µs; with word wrap 0.16 ms | `cargo bench -p rustaco-view` |
| Turning word wrap on for 1,000,000 lines | 0.3 to 0.45 s (computed for all lines) | `cargo bench -p rustaco-view` |
| Keystroke on a 10 MB Rust file, parsing and an active search in the worker | median 0.7 to 1.0 ms Chromium, 0.8 ms WebKit, 1.2 to 1.4 ms Firefox | `tests/e2e/specs/worker.spec.js` (@perf) |
| Keystroke on a 100,000-line Rust file, parsing on the main thread (no worker) | 7 to 8 ms | `tests/e2e/specs/syntax.spec.js` (@perf) |

Nearly all of the browser keystroke cost with the worker is drawing the frame on the canvas; Firefox draws canvas text more slowly than the other engines.

## Syntax, search, diff

| What | Result | Where |
|---|---|---|
| Incremental reparse after one keystroke, 100,000-line Rust file | about 4 ms (tree-sitter walks the top-level nodes) | ARCHITECTURE.md |
| TextMate highlighting of 2,000 SQL lines | 125 ms | `crates/rustaco-syntax/tests/textmate.rs` |
| Diff of two 100,000-line files with 1,000 changed lines | 124 ms native; 126 to 147 ms in the worker | `cargo bench -p rustaco-view` ("diff two 100k-line files"), `tests/e2e/specs/diff.spec.js` (@perf) |

## Size

| What | Gzipped |
|---|---|
| Editor module (`rustaco_web_bg.wasm`) | about 230 KB |
| Language modules | 290 to 690 KB each, loaded on first use |

`scripts/check-size.sh` fails the build above 1 MB for the editor module and 700 KB for a language module.

## Against Monaco

Measured on a second machine (DESKTOP-MZ1, Windows) with `tests/bench-monaco/run.mjs`: Monaco 0.x from npm and Rustaco, each in its own page in the same headless Chromium 159, on the same synthetic Rust files, with the caret in the middle and 60 keystrokes 40 ms apart. Rustaco parses in its worker; Monaco tokenizes with its Monarch grammar on the main thread. Two runs:

| File | Editor | Load (text in, two frames) | Main-thread work per keystroke |
|---|---|---|---|
| 5,000 lines (280 KB) | Rustaco | 34 to 65 ms | 9.5 to 10.5 ms |
| | Monaco | 36 to 39 ms | 5.6 to 8.7 ms |
| 100,000 lines (6 MB) | Rustaco | 103 to 136 ms | 9.8 to 13.6 ms |
| | Monaco | 121 to 153 ms | 6.0 to 9.6 ms |

"Main-thread work per keystroke" is Chromium's own task time (scripts, style, layout, painting, input handling) over the 60 keys with idle time subtracted, so it includes the browser's work around each key, not only the editor's. Scrolling takes one frame (16.5 ms at 60 Hz) in both.

Rustaco loads as fast as Monaco and does not slow down with file size, but it spends about one and a half times Monaco's main-thread time per keystroke. A CPU profile (`tests/bench-monaco/profile.mjs`) puts the difference in the frame drawing (canvas `fillText`, about 1.5 ms per key, plus decoding the frame's text from WASM, about 1 ms, since a keystroke draws two or three frames: the edit, the worker's highlights, the caret), garbage collection (1.7 ms) and the word-based completion list opening on every word char (about 1.2 ms). Two fixes came out of this run: the screen reader mirror now updates after a 60 ms pause instead of on every key, and the word list for completion is cached for a few seconds. The next ones would be drawing once per keystroke instead of again when highlights arrive, and passing frame text to JS without a string copy.
