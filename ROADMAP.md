# Roadmap

This plan covers the work from the current v0 to a release whose feature set matches Monaco as a code editor, plus a native frontend and a diff editor. Priority goes to syntax highlighting and language support; that work starts right after the engineering base. Phases are ordered by dependency, and each ends with a check that is run before the next one starts. Sizes: S is up to a week of one person's work, M is two to four weeks, L is more than a month.

Done: phase 0 (2026-11-21) and phase 1 (2026-11-21). In progress: phase 2.

Phase 2 status (2026-11-21):
- Done: the spike (tree-sitter and C grammars build for `wasm32-unknown-unknown`; four grammars needed a patched `tree-sitter-language`, see vendor/README.md), one language module per pack loaded on demand, incremental and time-sliced parsing, highlighting of visible lines, language catalog and detection, themes in the TextMate scope model with VS Code theme import, dark and light themes, license audit (THIRD_PARTY.md, licenses/).
- 18 of 20 languages have tree-sitter highlighting: Rust, JavaScript, TypeScript, TSX, JSON, Python, C, C++, Go, Java, C#, HTML, CSS, Bash, TOML, YAML, Markdown, and SQL on native builds only.
- Open: language injections, the TextMate fallback (for SQL in the browser, Kotlin, Dockerfile), the high-contrast theme, screenshot tests per language.
- Not met: "a keystroke in a 10 MB file under 1 ms on the main thread". With parsing time-sliced in the main thread a keystroke costs 7 to 13 ms (4 ms of it parsing) on a 100,000-line file. The language module interface is built for a worker; meeting the target moves to phase 7.

## Decisions

| # | Question | Proposal | Affects |
|---|---|---|---|
| D1 | Public API: compatible with Monaco's API, or modelled on its concepts only? | Concepts only (models, editors, decorations, commands, providers). A compatibility shim for the most used calls can come after the release. | Phases 6, 8 |
| D2 | Syntax highlighting engine | Decided: hybrid. Tree-sitter is the main engine and the source of every structural feature; TextMate grammars (through `syntect` with the pure-Rust `fancy-regex`) cover languages without a tree-sitter grammar and load on demand. Themes use TextMate scopes, so VS Code themes work with both engines. | Phase 2 |
| D3 | Background work in the browser: Web Workers with message passing, or WASM threads? | Workers with message passing. WASM threads need `SharedArrayBuffer`, so the host page must be cross-origin isolated, which many embedders cannot do. | Phases 2, 7, 10 |
| D4 | Right-to-left text and mobile (touch, virtual keyboards): in scope for the first release? | Out of scope. The text measurement layer from phase 5 is designed so bidi can be added later. | Phases 5, 6 |
| D5 | Desktop text stack | `winit` + `wgpu` + `parley` (shaping, font fallback, bidi) + glyph atlas, AccessKit for accessibility. | Phase 11 |

Markdown rendering is postponed; see "Later" at the end.

## Phase 0: engineering base (S)

- CI: `cargo test`, `clippy -D warnings`, `rustfmt --check`, a wasm32 build, a gzip size check of the WASM module (fails above budget), `cargo-deny` for licenses and advisories.
- Property tests (`proptest`): random edit sequences on `Document` compared with a plain `String` model; undo followed by redo returns the same text and selections; `ChangeSet::map` agrees with applying the change.
- `cargo-fuzz` targets for `ChangeSet` and the position conversions.
- Criterion benchmarks: load, keystroke, frame layout on 1M lines. CI records the numbers so regressions are visible.
- Playwright setup for end-to-end tests of the web frontend in Chromium, Firefox and WebKit.

Check: CI is green on every push, and the benchmarks run.

## Phase 1: split the model from the view state (M)

v0 keeps selections inside `Document`. The diff editor and split views show one text in two editors with separate cursors, so this has to change before anything is built on top.

- `TextModel`: rope, version, line ending, options, language id, undo history. Shared by every editor that shows it.
- `CursorState` per editor: selections, goal columns. Undo entries store the cursor state of the editor that made the change, as Monaco does.
- An edit in one editor moves the cursors of other editors showing the same model.
- Tab size and indentation options move from `rustaco-view` constants to the model.

Check: two views on one model, an edit in one keeps the other's cursors in place relative to the text; property tests pass.

## Phase 2: syntax highlighting and languages (L)

**Spike first (S).** Confirm that tree-sitter and its C grammars build for `wasm32-unknown-unknown` and that grammars can be loaded on demand, so the base bundle does not carry every language. If on-demand loading fails, the fallbacks are a fixed bundled set compiled in, or `web-tree-sitter` in the browser behind the same Rust interface. This is the largest technical risk in the plan, so it goes first.

**`rustaco-syntax` crate:**
- incremental parsing driven by `EditEvent` (the byte and point coordinates are already in it);
- highlight queries mapped to style ids through `rustaco_view::Styler`; only visible lines are queried;
- language injections: SQL in strings, CSS and JS inside HTML, code blocks inside markdown files;
- a time budget per parse: on a large file the first parse runs in slices between frames, and the previous tree's highlights stay on screen until the new tree is ready; above a configurable size, syntax is off;
- parsing moves to a worker in phase 7 behind the same interface.

**Language registry.** Detection by file extension, file name and first line (shebang, modelines). Each language is a package: grammar, queries (highlights, injections, folds, indents, locals), and a language configuration (comment tokens, bracket pairs, auto-closing pairs, indentation rules, word pattern). The browser loads a package on first use.

**TextMate fallback.** Languages without a usable tree-sitter grammar are tokenized by `syntect` (MIT) with `fancy-regex` (MIT), which builds for WASM without the C Oniguruma library. Tokenizing keeps a state per line and, after an edit, re-tokenizes from the edited line until the state matches the stored one. The engine and each grammar load only when a file needs them. VS Code ships grammars as JSON `.tmLanguage.json`; a build step converts them to the format `syntect` loads. Grammars that use Oniguruma features `fancy-regex` lacks are listed by the converter and fixed or dropped. A TextMate file gets colours; folding falls back to indentation, and other structural features are off.

**Themes.** Themes use TextMate scopes, the format of VS Code themes, so existing VS Code themes load directly. Tree-sitter capture names are mapped to scopes (`keyword` to `keyword.control`, `function.method` to `entity.name.function.member`, ...) by a table shipped with each language package, so one theme colours both engines. Light, dark and high-contrast themes are bundled. The canvas and desktop renderers read the same theme.

**Initial language set (20):** Rust, C, C++, C#, Go, Java, Kotlin, Python, JavaScript, TypeScript, TSX, JSON, HTML, CSS, SQL, Bash, YAML, TOML, Markdown (highlighting only), Dockerfile. More languages are added as packages without changes to the editor.

**License audit.** Every bundled grammar, query file, TextMate grammar and theme is listed in `THIRD_PARTY.md` with its source URL, commit and license, and only licenses from the allowlist in ARCHITECTURE.md are accepted. Tree-sitter grammars are mostly MIT. Helix queries (MPL-2.0) are excluded; nvim-treesitter queries (Apache-2.0) are allowed. VS Code's own grammars are MIT, but several are copied from other projects under their own licenses, so each one is checked separately. Where no permissively licensed source exists, the grammar or queries are written from scratch.

Check: all 20 languages highlight correctly on sample files (screenshot tests); a keystroke in a 10 MB file stays under 1 ms on the main thread; the base WASM bundle stays within budget with grammars loaded separately.

## Phase 3: core completeness (L)

**Decorations.** An interval tree of ranges that follow edits (sticky before, after or both). Diagnostics, search matches, diff hunks, folding ranges, snippet placeholders and inlay hints all store their ranges here. Lookup by visible line range costs O(log n + k).

**Commands and keybindings.** Every action from the frontends goes through a command registry with string ids; the frontends send key events, never call editing functions directly.
- Keybinding table: key combination, command id, arguments, `when` condition over context keys (`editorFocus`, `hasMultipleSelections`, `findWidgetVisible`, the language id, ...).
- Chords (`Ctrl+K Ctrl+C`), with the pending first key shown to the user.
- Keys identified by `KeyboardEvent.code` with a layout map, so shortcuts work on non-US layouts (Polish programmer, German, French AZERTY) and AltGr characters are not swallowed as shortcuts.
- Platform variants in one entry (Ctrl on Windows and Linux, Cmd on macOS).
- Keymaps are data files: a default keymap matching VS Code and Monaco, user overrides on top, a host can load its own keymap at runtime. A JetBrains-style keymap is the second bundled preset, to prove the format.
- The dispatcher supports modes (a mode is a set of context keys plus a keymap layer), so a modal keymap such as Vim can be added later without changes to the core.

**Editing operations:**
- grapheme cluster movement and deletion (`unicode-segmentation`);
- move, copy, delete, join and duplicate lines;
- indent and outdent selections (Shift+Tab);
- toggle line and block comments, using the language configuration from phase 2;
- auto-closing and auto-surrounding pairs, typing over a closing char, skipping pairs inside strings and comments (from the syntax tree);
- indentation on Enter from the indent queries, with the v0 copy-the-previous-line rule as the fallback;
- add the next occurrence (Ctrl+D), select all occurrences;
- column (box) selection with Alt+Shift+drag and keyboard;
- expand and shrink selection along the syntax tree;
- case changes, trim trailing whitespace, line ending conversion;
- cursor undo (Ctrl+U).

**Search.** Find and replace over the rope without copying it to a string (`regex-cursor`, the approach Helix uses), regular expressions with capture groups in replacement, case and whole-word options, search inside the selection. Matches are stored as decorations; search over a large file runs in chunks and does not block input.

Check: property tests cover every new operation; a table of Monaco keybindings is checked off command by command.

## Phase 4: syntax-aware view features (M)

Small features that build on phases 2 and 3 and make the editor feel complete for code:
- matching bracket highlight and jump to bracket, from the syntax tree;
- bracket pair colours and bracket pair guides;
- indentation guides, with the active scope highlighted;
- highlight of other occurrences of the word under the cursor, scoped by the locals query where available;
- whitespace rendering, rulers.

Check: screenshot tests for each feature in three languages.

## Phase 5: view model (L)

The largest piece of architecture after the core. Folding, wrapping, diff and inline widgets depend on it.

- **Text measurement.** A `TextMeasurer` trait supplied by the frontend: the web uses `measureText` with a per-cluster cache, the desktop uses `parley`. This replaces the fixed `char_width` from v0, fixes the CJK drift and handles fonts with ligatures.
- **Projection from model lines to view lines.** Word wrapping, folded ranges and virtual text (inlay hints) are applied here. Coordinate conversion goes both ways: model position to view position and back.
- **Height map.** Prefix sums over view line heights plus blocks between lines (Monaco's view zones). Diff filler, code lens and inline widgets use it. Wrapping on a huge file is computed lazily in chunks, with estimated heights for lines not yet measured.
- **Folding.** Ranges from the folds queries (phase 2), indentation as the fallback, LSP ranges later; fold markers in the gutter.
- **Decoration rendering.** Inline styles (backgrounds, underlines, wavy underlines), line and gutter decorations, the overview ruler in the scrollbar.
- **Minimap** drawn from the same frame data at a reduced scale.
- **Sticky scroll:** the headers of enclosing scopes (function, class) stay pinned at the top, taken from the syntax tree.
- **Widgets.** Content widgets anchored to a text position (hover, completion, parameter hints), overlay widgets (find bar), with an API both frontends implement.

Check: a 1M-line file with wrapping on renders a frame in under 2 ms; property tests for the projection round trip; screenshot tests for each visual feature.

## Phase 6: web frontend, complete (L)

- Input through `beforeinput`; IME composition drawn inline with an underline; dead keys; Firefox and Safari differences covered by tests.
- Accessibility: the hidden textarea mirrors the current line and selection so screen readers can read them (the method VS Code uses), ARIA labels, a high-contrast theme, full keyboard operation.
- Clipboard: per-cursor metadata, so copy and paste with several cursors restores the split; copying as HTML with syntax colours.
- Mouse: draggable scrollbars, auto-scroll while dragging past the edge, text drag and drop, context menu, middle-click paste on Linux.
- Find widget with match count, regex, case and word toggles, replace.
- Packaging: an npm package with ESM, hand-written TypeScript types for the public API, language packages as separate files loaded on demand, several editors on one page sharing one WASM module, works inside Shadow DOM and under a strict CSP.

Check: Playwright suite on three browsers covering typing, IME (simulated composition events), clipboard, mouse; manual check with NVDA and VoiceOver.

## Phase 7: background processing (M)

- Workers in the browser, threads on the desktop, one interface for both.
- The model sends `EditEvent`s to the worker; results (syntax trees and highlights, diff, search matches) come back tagged with the model version they were computed for, and stale results are dropped.
- Parsing from phase 2 moves here; the time-sliced main-thread path stays as the fallback when workers are unavailable.

Check: typing in a 10 MB file while search and parsing run keeps keystrokes under 1 ms on the main thread.

## Phase 8: provider API (M)

Language features are defined as providers, independent of LSP: completion, hover, signature help, definition, references, rename, formatting, code actions, document highlights, semantic tokens, inlay hints, folding ranges, selection ranges, document symbols, code lens. In the browser, a host registers them from JS (like Monaco's `registerCompletionItemProvider`); LSP in phase 9 is one implementation.

Built-in providers that need no server, computed from the syntax tree: document symbols (outline), word-based completion from the open file, folding ranges, selection ranges.

The UI for each provider comes here: completion list with fuzzy filtering and a documentation pane, hover card, parameter hints, peek view for references, rename box, lightbulb for code actions.

**Snippets.** Tab stops, placeholders, choices, variables, nested placeholders; used by completion. Each language package can carry its own snippets.

Check: a demo language implemented entirely in JS providers exercises every feature.

## Phase 9: LSP client (M)

- JSON-RPC over a transport trait: stdio on the desktop, WebSocket or a JS callback in the browser.
- Lifecycle, capability negotiation, position encoding (UTF-16 by default, UTF-8 when the server offers it; both conversions exist in the core).
- Incremental `didChange` from `EditEvent`.
- Every phase 8 provider backed by its LSP request; semantic tokens layered over tree-sitter highlights; `workspace/applyEdit` and multi-file edits passed to the host, because the component edits one model at a time.
- Hover and completion documentation: plain text and code blocks with highlighting; full markdown waits for the postponed markdown work.

Check: rust-analyzer, typescript-language-server and pyright work through the desktop example; one server works in the browser over WebSocket.

## Phase 10: diff editor (L)

- Line diff (histogram algorithm, own implementation or `imara-diff`) with a char-level pass inside changed lines; ignore-whitespace option; detection of moved blocks.
- Side-by-side view: two editors on two models, scroll synchronised, filler blocks from the height map aligning the sides.
- Inline view: one editor, deleted lines drawn as read-only blocks between lines, with syntax highlighting.
- Both sides editable; the diff is recomputed in the worker after edits, with a timeout that falls back to a coarser diff on very large inputs.
- Navigation between changes, reverting a hunk, collapsing unchanged regions.

Check: diff of two 100k-line files completes in the worker under 1 s; screenshot tests for both views.

## Phase 11: desktop frontend (L)

- `winit` window, `wgpu` renderer drawing the same `Frame`, glyph atlas, `parley` measurement.
- IME through winit, clipboard through `arboard`, accessibility through AccessKit.
- Grammars compiled in natively, without the on-demand loading the browser needs.
- An example application that opens files and talks to LSP servers over stdio.
- Integration crates for `egui` and `iced` after the core frontend is stable.

Check: the example application edits a 1M-line file at the same speed as the browser frontend; IME works on Windows, macOS and Linux.

## Phase 12: release (M)

- API review and freeze; semver from 1.0.
- Documentation: API reference, embedding guide for the web and the desktop, a guide for adding a language package.
- Publishing to crates.io and npm.
- Security: sanitising hover HTML, limits on pasted and loaded input sizes.
- Final performance report against Monaco on the same files.

## Order and parallel work

```
0 → 1 → 2 → 3 → 4 → 5 → 6 → 8 → 9
                      │    └──→ 10
                      └→ 11 (in parallel with 6)
2 → 7 (any time after 2, before 10)
all → 12
```

With one person, the order is 0 to 12 as numbered. With two, the second person takes phase 7 after phase 2 and phase 11 after phase 5.

A public preview fits after phase 4: a browser editor with highlighting for 20 languages, search, multi-cursor editing, comment toggling and bracket features, without folding, wrapping, LSP and diff.

## Risks

- Tree-sitter on `wasm32-unknown-unknown` with on-demand grammars; the phase 2 spike settles it before any other syntax work.
- Grammar and query licenses in an MIT project; audited in phase 2.
- IME and input differences between browsers; they are found only by testing on real systems.
- Lazy wrapping and height estimation on huge files can make scrolling jump; this needs careful design in phase 5.
- Accessibility of a canvas renderer is worse than of the DOM by default and needs deliberate work in phase 6.
- Size budget: `regex`, tree-sitter and grammars grow the bundle; grammars load on demand and the size check in CI catches the rest.

## Later

- **Markdown rendering:** a preview pane and possibly in-place rendering (headings enlarged, markers hidden off the cursor line). The height map and text measurement from phase 5 are what it needs, so nothing in the plan blocks adding it after the release.
- Vim and Emacs emulation as keymap packages: modes, operators with motions and text objects, registers, dot repeat, macros for Vim; kill ring and marks for Emacs. Phase 3 prepares the dispatcher for it.
- Right-to-left text, mobile input.
- A Monaco API compatibility shim.
- Three-way merge editor on top of the diff editor.
