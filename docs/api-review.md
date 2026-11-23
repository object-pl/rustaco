# API review for 1.0

The public surface frozen in 1.0.0, with the decisions taken in the review of 2026-11-23. Changes that break it wait for 2.0.

## JavaScript (npm package `rustaco`)

| Area | Surface | Before freezing |
|---|---|---|
| Entry points | `createEditor`, `createModel`, `setTheme`, `detectLanguage`, `knownLanguages`, `defineDecorationType`, `setWorkersEnabled`, `setLanguageServicesEnabled`, `languages`; `rustaco/lsp`; `rustaco/diff` | Done for 1.0. `fuzzyScore` was removed on 2026-11-23: the editor ranks completion in Rust, and Rust hosts have `rustaco_core::fuzzy_score`. |
| `RustacoEditor` | values, selections, options, keymaps, find, folding, snippets, view zones and widgets, scroll, events | `core`, `features`, `suggest`, `model.core` and `model.syntax` are reachable from outside but internal; `rustaco.d.ts` says so and declares none of them (decided 2026-11-22; the browser tests use them, so they stay reachable). Events are `onDid...(listener)` methods returning a remover that is also a `Disposable` (`onDidChangeContent`, `onDidScrollChange`, `onDidRefusePaste`, the diff editor's `onDidUpdateDiff`); the deprecated `onPasteTooLarge` property was removed for 1.0. |
| Positions | char offsets (Unicode scalar values) | Monaco uses 1-based line and column. Offsets are simpler and survive edits better; keep them, and keep `positionAt` and `offsetAt` as the bridge. |
| Providers | Monaco's function names, offsets instead of ranges; a `CancellationToken` comes last in every call, and the LSP client turns it into `$/cancelRequest` | Done for 1.0. |
| LSP client (`rustaco/lsp`) | `connectLanguageServer`, `LspClient` (`attach`, `request`, `ask`, `registerProviders`, `onNotification`), `webSocketTransport`; the protocol itself runs in the editor's wasm | Done for 1.0. The bundled languages use the same client on a worker transport. |
| Diff editor | `createDiffEditor`, `RustacoDiffEditor`; Alt+F5 and Shift+Alt+F5 go to the next and previous change (`diffEditor.nextChange`, `diffEditor.previousChange`) | Done for 1.0. |

## Rust crates

| Crate | Stable enough | Before freezing |
|---|---|---|
| `rustaco-core` | `TextModel`, `Cursors`, `Editor` commands, `ChangeSet`, `EditEvent`, `Decorations`, `Keymap`, `Snippet`, `diff` | `Editor::run` takes command ids as strings (VS Code's ids, which keymaps and JS use). A typed enum alongside can come in any 1.x, since it adds and breaks nothing. `Options` and `LanguageEditing` are `#[non_exhaustive]` (start from `default()`); `rustaco_syntax::language_editing(id)` builds the latter from the catalog. |
| `rustaco-view` | `View`, `Frame`, `Metrics`, `Styler` | `Frame` and `Metrics` are `#[non_exhaustive]` (`Metrics::new`), so measuring proportional text can be added in 1.x. Text is still a fixed cell grid. |
| `rustaco-syntax` | `Syntax`, `BackgroundSyntax`, `Theme` (`from_vscode`), catalog, `Engine`, `SyntaxService` (feature `service`); the Mermaid tokenizer is internal, reached through `Engine` | Language packs are cargo features of this crate; publishing it pulls every grammar crate as an optional dependency, which is fine, but the browser build also depends on a patched `tree-sitter-language` (`vendor/`), which crates.io cannot carry. The patch has to land upstream, or the browser packs build from the repository only. |
| `rustaco-lsp` | `LspClient`, request methods returning plain structs; `Session`, the client's protocol without I/O; `LanguageService` and `LspServer` | `LspClient`'s requests block with a timeout. A UI thread that must not wait drives `Session` over its own I/O instead, as the browser does (settled 2026-11-22 with phase 13). `LanguageService` gets new methods with defaults as LSP grows, so implementing it stays source-compatible; `Features` is `#[non_exhaustive]`, and so are the answer types (`CompletionItem`, `Hover`, `Location`, `Diagnostic`, ...), which services build with their constructors (`CompletionItem::new(label)`, `Location::local(uri, range)`) and then set fields on. |
| `rustaco-desktop` | `DesktopEditor` | Young: no gutter UI beyond line numbers, no widgets. Stays 0.x (0.4.0 next to the others' 1.0.0) while the native frontend is on hold. |
| `rustaco-web`, `rustaco-syntax-wasm` | built into the npm package | Not published to crates.io; internal to the npm package. |

## Release checklist

1. Settle the items above.
2. `keywords` and `categories` in the crate manifests (`repository` points at github.com/object-pl/rustaco; `homepage` would repeat it). Done.
3. `cargo publish --dry-run` for each crate in dependency order: core, view, lsp, syntax, desktop.
4. `scripts/pack-web.sh`, then `npm publish target/npm/rustaco-<version>.tgz`.
5. Tag the release and attach `docs/performance.md` with the Monaco comparison.

## Decisions of 2026-11-23

1. 1.0 covers the npm package and `rustaco-core`, `rustaco-view`, `rustaco-syntax`, `rustaco-lsp`; `rustaco-desktop` stays 0.x.
2. `onPasteTooLarge` removed.
3. No typed command enum before 1.0; it can be added later.
4. `Metrics` is `#[non_exhaustive]` with `Metrics::new`.
5. The LSP answer types are `#[non_exhaustive]` with constructors.
6. Public in `rustaco-syntax`: `Engine` and `SyntaxService`; the `mermaid` module is internal.
7. The JS API is what the three `.d.ts` files declare; everything else is internal.
8. Versions start together at 1.0.0 and then move per package; `rustaco-desktop` is 0.4.0.
