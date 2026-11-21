# API review before 1.0

The public surface as of the end of phase 11, with what should change before it is frozen. Nothing here is published yet; the version is 0.1.0 everywhere.

## JavaScript (npm package `rustaco`)

| Area | Surface | Before freezing |
|---|---|---|
| Entry points | `createEditor`, `createModel`, `setTheme`, `detectLanguage`, `knownLanguages`, `defineDecorationType`, `setWorkersEnabled`, `languages`, `fuzzyScore`; `rustaco/lsp`; `rustaco/diff` | Decide whether `fuzzyScore` stays public. |
| `RustacoEditor` | values, selections, options, keymaps, find, folding, snippets, view zones and widgets, scroll, events | `core`, `features`, `suggest` and `model.core` are reachable from outside but internal; hide them behind private fields or document them as unstable. Event names follow Monaco only partly (`onDidChangeContent`, `onDidScrollChange`, `onPasteTooLarge`); settle one naming scheme. |
| Positions | char offsets (Unicode scalar values) | Monaco uses 1-based line and column. Offsets are simpler and survive edits better; keep them, and keep `positionAt` and `offsetAt` as the bridge. |
| Providers | Monaco's function names, offsets instead of ranges | Cancellation tokens are not passed; add them before 1.0 so long requests can be dropped. |
| Diff editor | `createDiffEditor`, `RustacoDiffEditor` | Add keyboard commands for next and previous change. |

## Rust crates

| Crate | Stable enough | Before freezing |
|---|---|---|
| `rustaco-core` | `TextModel`, `Cursors`, `Editor` commands, `ChangeSet`, `EditEvent`, `Decorations`, `Keymap`, `Snippet`, `diff` | `Editor::run` takes command ids as strings; a typed enum alongside would catch typos at compile time. Several fields are `pub` on structs that may grow (`Options`, `LanguageEditing`): mark them `#[non_exhaustive]`. |
| `rustaco-view` | `View`, `Frame`, `Metrics`, `Styler` | `Frame` grows with each feature (minimap, sticky, zones); mark it `#[non_exhaustive]`. Text measurement is a fixed cell grid; a `TextMeasurer` trait for proportional fonts would change `Metrics`. |
| `rustaco-syntax` | `Syntax`, `BackgroundSyntax`, `Theme`, catalog | Language packs are cargo features of this crate; publishing it pulls every grammar crate as an optional dependency, which is fine, but the browser build also depends on a patched `tree-sitter-language` (`vendor/`), which crates.io cannot carry. The patch has to land upstream, or the browser packs build from the repository only. |
| `rustaco-lsp` | `LspClient`, request methods returning plain structs | Requests are blocking with a timeout; an async or callback form is needed for UI threads that must not wait. |
| `rustaco-desktop` | `DesktopEditor` | Young: no gutter UI beyond line numbers, no widgets. Publish as 0.x after the other crates reach 1.0. |
| `rustaco-web`, `rustaco-syntax-wasm` | built into the npm package | Not published to crates.io; internal to the npm package. |

## Release checklist

1. Settle the items above.
2. Add `repository`, `homepage`, `keywords` and `categories` to the crate manifests and `web/package.json` (the repository URL is not set yet).
3. `cargo publish --dry-run` for each crate in dependency order: core, view, syntax, lsp, desktop.
4. `scripts/pack-web.sh`, then `npm publish target/npm/rustaco-<version>.tgz`.
5. Tag the release and attach `docs/performance.md` with the Monaco comparison.
