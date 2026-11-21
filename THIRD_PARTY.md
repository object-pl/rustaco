# Third-party components shipped with Rustaco

Rust crate dependencies are checked by `cargo deny check` against the license allowlist in ARCHITECTURE.md. This file lists the components whose code or data is compiled into the shipped WASM modules and native builds beyond ordinary crate dependencies: the tree-sitter runtime, the grammars, and the highlight queries that come with them. License texts are in [licenses/](licenses/); `scripts/build-web.sh` copies them next to the browser modules.

Every highlight query used comes from the grammar's own repository and carries the grammar's license. No queries from Helix (MPL-2.0) or nvim-treesitter are bundled.

| Component | Version | License | Source | In browser build |
|---|---|---|---|---|
| tree-sitter (runtime) | 0.27 | MIT | https://github.com/tree-sitter/tree-sitter | every language module |
| tree-sitter-language (patched copy, see vendor/README.md) | 0.1.9 | MIT | https://github.com/tree-sitter/tree-sitter | every language module |
| tree-sitter-rust | 0.24.2 | MIT | https://github.com/tree-sitter/tree-sitter-rust | syntax-rust |
| tree-sitter-javascript | 0.25.0 | MIT | https://github.com/tree-sitter/tree-sitter-javascript | syntax-javascript, syntax-typescript (query only) |
| tree-sitter-typescript | 0.23.2 | MIT | https://github.com/tree-sitter/tree-sitter-typescript | syntax-typescript |
| tree-sitter-json | 0.24.8 | MIT | https://github.com/tree-sitter/tree-sitter-json | syntax-json |
| tree-sitter-python | 0.25.0 | MIT | https://github.com/tree-sitter/tree-sitter-python | syntax-python |
| tree-sitter-c | 0.24.2 | MIT | https://github.com/tree-sitter/tree-sitter-c | syntax-c, syntax-cpp (query only) |
| tree-sitter-cpp | 0.23.4 | MIT | https://github.com/tree-sitter/tree-sitter-cpp | syntax-cpp |
| tree-sitter-go | 0.25.0 | MIT | https://github.com/tree-sitter/tree-sitter-go | syntax-go |
| tree-sitter-java | 0.23.5 | MIT | https://github.com/tree-sitter/tree-sitter-java | syntax-java |
| tree-sitter-c-sharp | 0.24.0 | MIT | https://github.com/tree-sitter/tree-sitter-c-sharp | syntax-csharp |
| tree-sitter-html | 0.23.2 | MIT | https://github.com/tree-sitter/tree-sitter-html | syntax-html |
| tree-sitter-css | 0.25.0 | MIT | https://github.com/tree-sitter/tree-sitter-css | syntax-css |
| tree-sitter-bash | 0.25.1 | MIT | https://github.com/tree-sitter/tree-sitter-bash | syntax-bash |
| tree-sitter-toml-ng | 0.7.0 | MIT | https://github.com/tree-sitter-grammars/tree-sitter-toml | syntax-toml |
| tree-sitter-yaml | 0.7.2 | MIT | https://github.com/tree-sitter-grammars/tree-sitter-yaml | syntax-yaml |
| tree-sitter-md | 0.5.4 | MIT | https://github.com/tree-sitter-grammars/tree-sitter-markdown | syntax-markdown |
| tree-sitter-sequel | 0.3.13 | MIT | https://github.com/derekstride/tree-sitter-sql | no (native builds only) |

The bundled colour themes (`crates/rustaco-syntax/src/default_theme.rs`) are written for this project and fall under its MIT license.
