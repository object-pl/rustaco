# Vendored crates

## tree-sitter-language 0.1.9 (MIT)

Upstream: https://crates.io/crates/tree-sitter-language, license in `tree-sitter-language/LICENSE`.

Change: `wasm/unsupported/tree-sitter-0.26/{stdio,stdlib,string}.c` are empty. Upstream they contain an `#error`, which stops the grammars for C, C#, CSS and SQL from building for `wasm32-unknown-unknown`: their build scripts still compile these files, although tree-sitter 0.27 already provides the functions. Drop this copy once those grammar crates stop compiling the files.

Also: `wasm/include/assert.h` defines `static_assert`, and `wasm/include/wctype.h` includes `<wchar.h>` (for `wchar_t`), and `wasm/include/ctype.h` adds inline ASCII classifiers (`isdigit`, `isspace`, `isalpha`, ...) that the Bash scanner calls. C libraries provide all of these through the same headers.

## regex-lite-shim (MIT OR Apache-2.0, written here)

A crate named `regex` that the workspace patches in for the real one. tree-sitter uses `regex` only to evaluate `#match?` and `#not-match?` predicates in queries (`bytes::Regex::new` and `is_match`); the shim provides those two on top of `regex-lite`. The full crate's Unicode tables made up about 160 KB gzipped of every browser language module, which drops to 157 KB (JSON) to 550 KB (C#). `regex-lite` has no Unicode classes (`\p{..}`) and its `\w`, `\d`, `\b` are ASCII; the queries of the bundled grammars use neither, and `every_highlight_query_compiles` and the golden files in `rustaco-syntax` check that they still compile and highlight the same. If a grammar ever needs Unicode classes, drop the patch line in the root `Cargo.toml`.
