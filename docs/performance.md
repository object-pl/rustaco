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

Not measured yet. A fair comparison runs both editors on the same files in the same browser with the same keystroke script (Monaco from its npm package, served locally), and reports keystroke-to-frame time, load time for a 10 MB file, and memory. The harness is the next piece of work for the release.
