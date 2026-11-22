# Architecture

## Crates

| Crate | Status | Contents |
|---|---|---|
| `rustaco-core` | v0 | `TextModel` (rope buffer, version, undo history, change log) shared by several editors, per-editor `Cursors`, editing commands through `Editor`, `Document` for the single-editor case. Multi-cursor selections, change sets, undo grouping, word movement, clipboard, edit events. No UI, no I/O, builds for `wasm32-unknown-unknown`. |
| `rustaco-view` | v0 | Renderer-agnostic layout: viewport, scrolling, visual columns with tabs and wide chars, hit testing, display list (text runs, carets, selection rects). Lays out visible lines and visible columns only. |
| `rustaco-web` | v0 | `wasm-bindgen` frontend, without tree-sitter. Text is drawn with WebGL2 (glyph atlas, instances laid out in Rust and read by JS from WASM memory, one draw call), between a 2D canvas for what lies under the text and a 2D overlay canvas for carets, gutter and scrollbars; without WebGL2 the text goes on the base canvas. `web/rustaco.js` forwards DOM events (keyboard through a hidden `textarea`, so IME and dead keys work), loads language modules and passes edits and highlights between them and the editor. |
| `rustaco-syntax` | v0 | Language catalog (detection, comment and bracket configuration) and themes in the TextMate scope model, always available. Behind the `engine` feature: incremental tree-sitter parsing driven by `EditEvent`, resumable parsing within a time budget, highlighting of requested lines, 18 grammars. |
| `rustaco-syntax-wasm` | v0 | Browser language module: one WASM file per language pack, loaded on first use. Keeps its own copy of the text, takes edits and returns highlights as plain data. |
| `rustaco-lsp` | v0 | LSP client over any byte streams (a server's stdio): handshake with position encoding negotiation, incremental sync from edit events, every editor feature's request mapped to char offsets. The browser has its own client in `web/lsp.js` over WebSocket. |
| diff | v0 | `rustaco_core::diff` (Myers line diff with a char-level pass, moves, a time budget) and `web/diff.js` (side-by-side and inline views). |
| `rustaco-markdown` | later | Markdown rendering, postponed until after the first release. |
| `rustaco-desktop` | v0 | Native frontend: `winit` events, the same `Frame` drawn in software into a pixel buffer with glyphs from a system monospace font (`fontdb`, `fontdue`), clipboard through `arboard`, IME through winit, highlighting on a background thread. `examples/editor.rs` opens and saves files and talks to an LSP server. |

## Rules for the core

- A `TextModel` holds the text and the undo history; each editor holds its own `Cursors`. The model keeps no references to editors or other consumers. They read its change log by version (`changes_since`, `events_since`) and catch up when they next need the text; a consumer more than 1024 change sets behind gets `Stale` and resyncs from the full text.
- Undo and redo act on the model, whichever editor triggers them. The editor that made the change gets its selections back; other editors' cursors follow the text.
- A cursor at another editor's insertion point stays before the inserted text.
- Text is addressed by char offset. UTF-16 positions (LSP, JS) and bytes/points (tree-sitter) are converted in `position.rs` and nowhere else.
- Every text change goes through `ChangeSet` and `TextModel::apply`. Each applied change set logs one `EditEvent` per edit, last edit first, in char, byte, point and UTF-16 units. LSP `didChange` and `Tree::edit` consume them without seeing the old text.
- `version` increases with every applied change set, including undo and redo.
- Recognised line terminators are `\n`, `\r\n` and `\r`. `\r\n` is a single step for cursor movement and deletion. No cursor or edit boundary stays between `\r` and `\n`: such a position has no line/column form, so it could not be reported to LSP servers. Edits there are widened to the whole pair with the text adjusted, so the result is the same.

## Rules for the view

- Vertical geometry goes through `View::line_top` and `View::line_at_y` only. Lines have one height today. Markdown headings, diff filler blocks and inline widgets need per-line heights and blocks between lines; they will be added behind these two functions as a height map with prefix sums, without touching frontends.
- Frontends draw a `Frame` and never measure text or walk the rope.

## Syntax highlighting in the browser

The editor module contains no parser. Each language pack is a separate WASM module with the tree-sitter runtime and its grammars (`web/pkg/syntax-<pack>/`), loaded the first time a model uses one of its languages; a page that edits only Rust never downloads the C++ grammar. Measured gzipped sizes: editor 230 KB, language modules 290 to 690 KB (JSON smallest, C# largest). Twenty grammars in one module would be about 2 MB.

The language module receives edits as data (`Model.editsSince`: nine numbers per edit plus the inserted text) and returns highlights as data (quads of line, start column, end column, style id). No shared memory and no callbacks.

Where workers are available the language modules run in one Web Worker (`web/rustaco-worker.js`) shared by all models on the page. The page sends each model's edits as they happen; each frame asks for the highlights of its visible lines and the bracket pair at the caret, and the reply carries the model version it was computed for, so a reply that arrives after the next edit is dropped. Meanwhile the editor draws the previous highlights, moved with the edits. Search runs there too, on a text copy kept by the editor module (`TextReplica`), so re-running a search after each typing pause costs the page nothing. Questions that need an answer at once (is the caret inside a string, for auto-closing pairs) are answered from the highlight styles already on screen. Without workers (or with `setWorkersEnabled(false)`) the modules run on the main thread in time slices, as described next. Native frontends get the same protocol on a thread: `rustaco_syntax::BackgroundSyntax`.

tree-sitter's incremental reparse walks the top-level nodes of the file, so its cost grows with their number: about 30 ms for 25,000 Rust functions, 500 ms for 100,000 top-level statements. Parsing therefore runs in slices: each frame gives the module 4 ms, a halted parse resumes on the next frame, and until it finishes the editor highlights from the previous tree shifted by the edits. Measured per keystroke in a 100,000-line Rust file, Chromium: parse 3.8 ms (the budget), highlight 2.3 ms, drawing 2.0 ms, the rest under 0.5 ms; Firefox is about 30% slower.

Pattern priority: when several patterns capture the same node, the more specific pattern wins, counted as the nodes and predicates the pattern requires; between equally specific patterns the later one wins. Neither plain order fits the bundled queries: Python lists a method pattern before a property pattern for the same node but its generic `(identifier) @variable` first, and Go puts its generic captures after the specific ones.

## Performance

Measured in Chromium with the release WASM build on a 1,000,000-line file (54 MB):

- loading the text: 820 ms;
- a keystroke including a full frame render: 0.46 ms;
- with highlighting in the worker, a keystroke in a 10 MB Rust file while a search for a word with 20,000 matches stays active: 0.7 ms in Chromium, 0.8 ms in WebKit, 1.3 ms in Firefox (median of batches; nearly all of it is drawing the frame);
- a scroll frame: under 0.1 ms.

Budgets (checked by `scripts/check-size.sh`): the editor module under 1 MB gzipped (now 230 KB), each language module under 700 KB gzipped. SQL's tree-sitter grammar compiles to 1.2 MB and is left out of the browser build.

LSP will run outside the input path.

## Known gaps

- Cursor movement steps by Unicode scalar, not grapheme cluster. Emoji with ZWJ and combining marks need `unicode-segmentation`.
- Goal columns for vertical movement count chars, not visual cells.
- Wide chars are positioned at two cells; a glyph wider or narrower than its cells overlaps or leaves a gap, but the text after it stays on the grid.
- WebGL glyphs are rasterised white and tinted, so dark text on a light theme comes out slightly bolder than the canvas draws it (browsers tune text masks per colour).
- Missing editing features: line operations (move, duplicate, comment), find/replace, auto-closing brackets, outdent (Shift+Tab), word wrap, folding.
- The web frontend has no draggable scrollbar, no mouse auto-scroll outside the canvas and no accessibility layer for screen readers yet.

## Licensing rules

Rustaco is MIT-licensed and everything it depends on or ships must allow that without conditions on the user's code.

- Allowed for dependencies and bundled files (grammars, queries, themes, data): MIT, MIT-0, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, Unicode-3.0, Unicode-DFS-2016, 0BSD, CC0-1.0, BSL-1.0 (Boost; the Windows clipboard crates under `arboard` in the desktop frontend).
- Not allowed: GPL, LGPL, AGPL, MPL, EPL, CDDL, SSPL, any license with copyleft or field-of-use restrictions, and files without a license.
- `deny.toml` enforces the list for Rust crates; CI fails on a violation.
- Bundled non-crate files are listed in `THIRD_PARTY.md` with source URL, commit and license, and their license texts ship with the package.
- Apache-2.0 components require keeping their NOTICE files; the build copies them into the package.
