# API review before 1.0

The public surface as of the end of phase 13, with what should change before it is frozen. Version 0.2.0 is published to crates.io and npm; the API may still change before 1.0.

## JavaScript (npm package `rustaco`)

| Area | Surface | Before freezing |
|---|---|---|
| Entry points | `createEditor`, `createModel`, `setTheme`, `detectLanguage`, `knownLanguages`, `defineDecorationType`, `setWorkersEnabled`, `setLanguageServicesEnabled`, `languages`, `fuzzyScore`; `rustaco/lsp`; `rustaco/diff` | Decide whether `fuzzyScore` stays public. |
| `RustacoEditor` | values, selections, options, keymaps, find, folding, snippets, view zones and widgets, scroll, events | `core`, `features`, `suggest`, `model.core` and `model.syntax` are reachable from outside but internal; `rustaco.d.ts` says so and declares none of them (decided 2026-11-22; the browser tests use them, so they stay reachable). Events are `onDid...(listener)` methods returning a remover that is also a `Disposable` (`onDidChangeContent`, `onDidScrollChange`, `onDidRefusePaste`, the diff editor's `onDidUpdateDiff`); the `onPasteTooLarge` property stays as a deprecated alias. |
| Positions | char offsets (Unicode scalar values) | Monaco uses 1-based line and column. Offsets are simpler and survive edits better; keep them, and keep `positionAt` and `offsetAt` as the bridge. |
| Providers | Monaco's function names, offsets instead of ranges; a `CancellationToken` comes last in every call, and the LSP client turns it into `$/cancelRequest` | Done for 1.0. |
| LSP client (`rustaco/lsp`) | `connectLanguageServer`, `LspClient` (`attach`, `request`, `ask`, `registerProviders`, `onNotification`), `webSocketTransport`; the protocol itself runs in the editor's wasm | Done for 1.0. The bundled languages use the same client on a worker transport. |
| Diff editor | `createDiffEditor`, `RustacoDiffEditor`; Alt+F5 and Shift+Alt+F5 go to the next and previous change (`diffEditor.nextChange`, `diffEditor.previousChange`) | Done for 1.0. |

## Rust crates

| Crate | Stable enough | Before freezing |
|---|---|---|
| `rustaco-core` | `TextModel`, `Cursors`, `Editor` commands, `ChangeSet`, `EditEvent`, `Decorations`, `Keymap`, `Snippet`, `diff` | `Editor::run` takes command ids as strings; a typed enum alongside would catch typos at compile time. `Options` and `LanguageEditing` are `#[non_exhaustive]` (start from `default()`); `rustaco_syntax::language_editing(id)` builds the latter from the catalog. |
| `rustaco-view` | `View`, `Frame`, `Metrics`, `Styler` | `Frame` is `#[non_exhaustive]`. Text measurement is a fixed cell grid; a `TextMeasurer` trait for proportional fonts would change `Metrics`. |
| `rustaco-syntax` | `Syntax`, `BackgroundSyntax`, `Theme`, catalog, `Engine`, `SyntaxService` (feature `service`), `mermaid` | Language packs are cargo features of this crate; publishing it pulls every grammar crate as an optional dependency, which is fine, but the browser build also depends on a patched `tree-sitter-language` (`vendor/`), which crates.io cannot carry. The patch has to land upstream, or the browser packs build from the repository only. |
| `rustaco-lsp` | `LspClient`, request methods returning plain structs; `Session`, the client's protocol without I/O; `LanguageService` and `LspServer` | `LspClient`'s requests block with a timeout. A UI thread that must not wait drives `Session` over its own I/O instead, as the browser does (settled 2026-11-22 with phase 13). `LanguageService` gets new methods with defaults as LSP grows, so implementing it stays source-compatible; `Features` is `#[non_exhaustive]`. |
| `rustaco-desktop` | `DesktopEditor` | Young: no gutter UI beyond line numbers, no widgets. Publish as 0.x after the other crates reach 1.0. |
| `rustaco-web`, `rustaco-syntax-wasm` | built into the npm package | Not published to crates.io; internal to the npm package. |

## Release checklist

1. Settle the items above.
2. Add `homepage`, `keywords` and `categories` to the crate manifests (`repository` points at github.com/theObjectCo/rustaco).
3. `cargo publish --dry-run` for each crate in dependency order: core, view, syntax, lsp, desktop.
4. `scripts/pack-web.sh`, then `npm publish target/npm/rustaco-<version>.tgz`.
5. Tag the release and attach `docs/performance.md` with the Monaco comparison.
