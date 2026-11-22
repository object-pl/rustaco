# Adding a language

A language needs three things: a catalog entry (detection and editing rules), a grammar with a highlight query, and, for the browser, a place in a language pack. Tree-sitter grammars are the default; a TextMate grammar is the fallback when no tree-sitter grammar exists or its WASM build is too large.

## 1. Check the license

Grammars, queries and TextMate files ship with the package, so their license must be on the list in `ARCHITECTURE.md` ("Licensing rules"): MIT, Apache-2.0, BSD, ISC, Zlib, 0BSD, CC0, Unicode or BSL-1.0. GPL, LGPL, MPL and files without a license are out. Record the grammar in `THIRD_PARTY.md` (source, version, license) and copy its license text to `licenses/`; an Apache-2.0 grammar with a NOTICE file needs the NOTICE copied as well.

## 2. Catalog entry

`crates/rustaco-syntax/src/catalog.rs` lists every language the editor knows, compiled in or not:

```rust
lang!("zig", "Zig", "zig", ext: &["zig"], files: &[], shebangs: &[],
    LanguageConfig { auto_closing: CLOSE_C, ..C_STYLE }),
```

The arguments are the id, the display name, the pack that parses it (the WASM module name in the browser), file extensions, exact file names (`Dockerfile`), interpreter names for `#!` lines, and the editing rules: line and block comment markers, auto-closing pairs, bracket pairs, and the line endings after which Enter indents (`:` for Python). `C_STYLE` covers C-like languages. The catalog test in the same file shows how detection is checked.

## 3. Tree-sitter grammar

In `crates/rustaco-syntax/Cargo.toml`, add the grammar crate as an optional dependency and a feature for it, then add the feature to `all-languages`:

```toml
tree-sitter-zig = { version = "1", optional = true }
zig = ["engine", "dep:tree-sitter-zig"]
```

In `crates/rustaco-syntax/src/language.rs`, add the grammar and its queries to `LANGUAGES`. Queries are joined in order, which is how TypeScript builds on the JavaScript query:

```rust
#[cfg(feature = "zig")]
language!("zig", tree_sitter_zig::LANGUAGE, [tree_sitter_zig::HIGHLIGHTS_QUERY]),
```

Capture names map to editor styles through `crates/rustaco-syntax/src/names.rs`. A capture such as `@function.builtin` falls back to the longest known prefix (`function`), so most grammars need no changes there. When several patterns capture one node, the more specific pattern wins (see "Pattern priority" in `ARCHITECTURE.md`).

Outline symbols, folding and selection ranges come from the tree without extra queries: declarations are found by node kind (`*_item`, `*_declaration`, `*_definition`) and named by their `name` field, fold regions follow bracket pairs and multi-line comments.

## 4. Golden file

`crates/rustaco-syntax/tests/golden/` holds one sample per language and, next to it, `<sample>.expected` with the highlight output it must produce. Add `sample.zig` with a representative sample, then generate its expected output:

```
UPDATE_GOLDEN=1 cargo test -p rustaco-syntax --features all-languages --test golden
```

Read the generated file before committing it: it is the specification of what the highlighting should look like.

## 5. Browser pack

Each pack is one WASM module built from `crates/rustaco-syntax-wasm` with one or more language features. A language with its own pack needs a feature there:

```toml
zig = ["tree-sitter", "rustaco-syntax/zig"]
```

`scripts/build-web.sh` builds every feature of that crate into `web/pkg/syntax-<pack>/`, and `scripts/check-size.sh` fails the build when a pack exceeds 700 KB gzipped. A language that injects others (HTML with CSS and JavaScript) lists them in its pack's feature, so one download covers the file. The editor loads the pack named in the catalog entry the first time a model uses the language.

When a tree-sitter grammar is too large for the browser (SQL's is 1.2 MB), keep it for native builds and add the language to `SKIP_PACKS` in `scripts/build-web.sh`; the browser then needs a TextMate grammar for it.

## TextMate grammar instead

Put the VS Code grammar JSON in `crates/rustaco-syntax/textmate/` (record it in that directory's README and in `THIRD_PARTY.md`) and add it to the grammar table in `crates/rustaco-syntax/src/textmate.rs`, next to the SQL, Kotlin and Dockerfile entries. Set the catalog entry's pack to `"textmate"`. TextMate languages highlight but have no syntax tree, so bracket matching falls back to a text scan and folding to indentation.

## Language services come with the grammar

A tree-sitter language gets more than colours. Its pack runs as a language server in the browser's worker (`rustaco_syntax::SyntaxService`), and the editor registers completion, definition, references and rename for it. These work from the grammar alone: keywords are the grammar's anonymous word tokens, names are the nodes whose kind ends in `identifier`, and declarations are found by the field they sit in (`name`, `pattern`, `declarator`). A grammar that names identifiers differently (`word`, `symbol`) gets keyword completion only, until `is_name` in `crates/rustaco-syntax/src/locals.rs` learns its kind.

The same service runs anywhere `rustaco-lsp` runs: `cargo run -p rustaco-syntax --example server` is a language server on stdio for any editor.

## A hand-written engine instead

A language without a grammar can be tokenized by hand, as Mermaid is (`crates/rustaco-syntax/src/mermaid.rs`): a function from a line to (char range, highlight name) pairs, a variant in `rustaco_syntax::Engine`, and completion in `SyntaxService::completion`. The `mermaid` feature of `rustaco-syntax-wasm` builds a pack with nothing else in it (121 KB gzipped).
