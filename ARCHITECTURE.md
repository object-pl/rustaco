# Architecture

## Crates

| Crate | Status | Contents |
|---|---|---|
| `rustaco-core` | v0 | Rope buffer (ropey), multi-cursor selections, change sets, undo/redo with typing grouping, editing commands, word movement, clipboard, edit events. No UI, no I/O, builds for `wasm32-unknown-unknown`. |
| `rustaco-view` | v0 | Renderer-agnostic layout: viewport, scrolling, visual columns with tabs and wide chars, hit testing, display list (text runs, carets, selection rects). Lays out visible lines and visible columns only. |
| `rustaco-web` | v0 | `wasm-bindgen` frontend drawing on a 2D canvas. `web/rustaco.js` forwards DOM events; keyboard input goes through a hidden `textarea`, so IME and dead keys work. |
| `rustaco-syntax` | planned | Incremental tree-sitter parsing driven by `EditEvent`, highlight queries feeding `rustaco_view::Styler`, bracket matching, indentation hints. |
| `rustaco-lsp` | planned | LSP client over an abstract transport: stdio on desktop, WebSocket or a JS callback in the browser. Completion, diagnostics, hover, go-to-definition, formatting. |
| `rustaco-diff` | planned | Line diff (histogram) with a char-level pass inside changed lines. Side-by-side and inline diff views, both sides editable, the diff recomputed incrementally after edits. |
| `rustaco-markdown` | later | Markdown rendering, postponed until after the first release. |
| `rustaco-desktop` | planned | Native frontend: `winit` plus GPU text rendering (`wgpu` with a glyph atlas), drawing the same `Frame`. |

## Rules for the core

- Text is addressed by char offset. UTF-16 positions (LSP, JS) and bytes/points (tree-sitter) are converted in `position.rs` and nowhere else.
- Every text change goes through `ChangeSet`. The document emits one `EditEvent` per edit, last edit first, in char, byte, point and UTF-16 units. LSP `didChange` and `Tree::edit` consume them without seeing the old text.
- `version` increases with every applied change set, including undo and redo.
- Recognised line terminators are `\n`, `\r\n` and `\r`. `\r\n` is a single step for cursor movement and deletion.

## Rules for the view

- Vertical geometry goes through `View::line_top` and `View::line_at_y` only. Lines have one height today. Markdown headings, diff filler blocks and inline widgets need per-line heights and blocks between lines; they will be added behind these two functions as a height map with prefix sums, without touching frontends.
- Frontends draw a `Frame` and never measure text or walk the rope.

## Performance

Measured in Chromium with the release WASM build on a 1,000,000-line file (54 MB):

- loading the text: 820 ms;
- a keystroke including a full frame render: 0.46 ms;
- a scroll frame: under 0.1 ms.

The WASM module is 304 KB, 98 KB after gzip. Budget: under 1 MB gzip without grammars; grammars load on demand.

Syntax parsing and LSP will run outside the input path. Highlighting from the previous tree is drawn until the new tree is ready.

## Known gaps

- Cursor movement steps by Unicode scalar, not grapheme cluster. Emoji with ZWJ and combining marks need `unicode-segmentation`.
- Goal columns for vertical movement count chars, not visual cells.
- Wide chars are positioned at two cells, but the canvas draws them with the font's own advance, so a run containing CJK text can drift.
- Missing editing features: line operations (move, duplicate, comment), find/replace, auto-closing brackets, outdent (Shift+Tab), word wrap, folding.
- The web frontend has no draggable scrollbar, no mouse auto-scroll outside the canvas and no accessibility layer for screen readers yet.

## Licensing rules

Rustaco is MIT-licensed and everything it depends on or ships must allow that without conditions on the user's code.

- Allowed for dependencies and bundled files (grammars, queries, themes, data): MIT, MIT-0, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, Unicode-3.0, Unicode-DFS-2016, 0BSD, CC0-1.0.
- Not allowed: GPL, LGPL, AGPL, MPL, EPL, CDDL, SSPL, any license with copyleft or field-of-use restrictions, and files without a license.
- `deny.toml` enforces the list for Rust crates; CI fails on a violation.
- Bundled non-crate files are listed in `THIRD_PARTY.md` with source URL, commit and license, and their license texts ship with the package.
- Apache-2.0 components require keeping their NOTICE files; the build copies them into the package.
