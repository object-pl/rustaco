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
| Editor module (`rustaco_web_bg.wasm`) | about 368 KB, of which about 55 KB is the LSP client (rustaco-lsp's `Session` and serde_json) and about 16 KB the theme, markdown and diff view code moved from JS on 2026-11-23 |
| JavaScript (`rustaco.js` and the modules it imports) | about 46 KB |
| Language modules | 155 to 550 KB each (Rust 270 KB), loaded on first use; about 150 KB of each is the tree-sitter runtime and the highlighter, repeated per module |

What a page loads to show a Rust file, measured with Playwright on the benchmark pages (`tests/bench-monaco`), gzipped with level 9:

| Editor | Files | Raw | Gzipped |
|---|---|---|---|
| Rustaco (editor, Rust language module, worker) | 12 | 2,978 KB | 768 KB (now about 620 KB, see below) |
| Monaco 0.57 (editor, CSS, editor worker, Monarch Rust grammar) | 21 | 4,831 KB | 1,267 KB |

The Rust language module was 435 KB of Rustaco's 768 KB. Most of the part every module repeats was the Unicode tables of the `regex` crate, which tree-sitter uses only for `#match?` predicates; a shim on `regex-lite` (`vendor/README.md`) took about 160 KB off each module, so the Rust module is now 270 KB. A second language adds its own module. Monaco's Monarch grammars are a few KB each, but its language services load workers when used: TypeScript 1,507 KB gzipped (6.9 MB raw), CSS 233 KB, HTML 184 KB, JSON 118 KB.

Since the language modules became language servers (roadmap, phase 13) each carries the LSP server and `serde_json`: 60 to 80 KB gzipped more per module, the Rust module 346 KB.

`scripts/check-size.sh` fails the build above 1 MB for the editor module and 700 KB for a language module.

## Text layer: WebGL, canvas, DOM

Measured on DESKTOP-MZ1 (GeForce GTX 1660) with `tests/bench-monaco/run.mjs`, three runs each, medians. Chromium ran with `--use-angle=d3d11 --enable-gpu`, because headless Chromium otherwise renders WebGL in software (SwiftShader), which made the WebGL layer look five times slower than it is. Two measures: Chromium's own task time per keystroke, and in every browser a probe that posts messages to itself back to back and sums the gaps over 2 ms (busy main thread), with an idle page subtracted. The two are not comparable with each other, only across columns.

| Main thread per keystroke | canvas | DOM rows | WebGL | Monaco |
|---|---|---|---|---|
| Chromium task time, 5,000 lines | 8.2 ms | 7.4 ms | 4.8 ms | 7.9 ms |
| Chromium task time, 100,000 lines | 8.1 ms | 10.1 ms | 6.4 ms | 5.6 ms |
| Firefox probe, 5,000 lines | 7.1 ms | 7.4 ms | 6.5 ms | 11.9 ms |
| Firefox probe, 100,000 lines | 5.4 ms | 6.5 ms | 5.5 ms | 11.0 ms |

WebGL was the fastest or level with the fastest in both browsers, and it became the default; the DOM layer (one `div` per row, rows matched by their HTML so a keystroke rewrites one) was no faster than the canvas and was dropped. Runs vary by about 1 ms (Monaco ranged from 4.9 to 9.2 ms in Chromium), so smaller differences mean nothing. Playwright's WebKit on Windows gave no usable numbers: its probe baseline was noisier than the typing, and its frame latency is quantised to whole frames. Safari needs a Mac.

After the text layer, the whole frame moved to WebGL (backgrounds, selections, gutter, minimap, scrollbars: one buffer, one draw call). Same machine and method, three runs, medians:

| Main thread per keystroke | Rustaco, WebGL frame | Rustaco, canvas | Monaco |
|---|---|---|---|
| Chromium task time, 5,000 lines | 5.3 ms | 8.0 ms | 6.2 ms |
| Chromium task time, 100,000 lines | 5.4 ms | 8.5 ms | 7.4 ms |
| Firefox probe, 5,000 lines | 5.8 ms | 7.1 ms | 12.2 ms |
| Firefox probe, 100,000 lines | 2.8 ms | 5.4 ms | 10.3 ms |
| Chromium, keydown to end of frame, 100,000 lines | 7.6 ms | 16.1 ms | 14.0 ms |

Then a keystroke went from two or three frames to one (highlights move with in-line edits, and a worker reply that changes nothing draws nothing), and completion ranking and word collection moved to Rust:

| Main thread per keystroke | Rustaco | Monaco |
|---|---|---|
| Chromium task time, 5,000 lines | 4.2 ms | 9.8 ms |
| Chromium task time, 100,000 lines | 3.8 ms | 6.0 ms |
| Firefox probe, 5,000 lines | 3.6 ms | 10.0 ms |
| Firefox probe, 100,000 lines | 2.5 ms | 13.8 ms |
| Chromium, keydown to end of frame, 100,000 lines | 2.8 ms | 15.3 ms |

The section below is the earlier comparison, with the canvas renderer.

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
