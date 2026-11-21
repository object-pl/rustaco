# Vendored crates

## tree-sitter-language 0.1.9 (MIT)

Upstream: https://crates.io/crates/tree-sitter-language, license in `tree-sitter-language/LICENSE`.

Change: `wasm/unsupported/tree-sitter-0.26/{stdio,stdlib,string}.c` are empty. Upstream they contain an `#error`, which stops the grammars for C, C#, CSS and SQL from building for `wasm32-unknown-unknown`: their build scripts still compile these files, although tree-sitter 0.27 already provides the functions. Drop this copy once those grammar crates stop compiling the files.

Also: `wasm/include/assert.h` defines `static_assert`, and `wasm/include/wctype.h` includes `<wchar.h>` (for `wchar_t`), and `wasm/include/ctype.h` adds inline ASCII classifiers (`isdigit`, `isspace`, `isalpha`, ...) that the Bash scanner calls. C libraries provide all of these through the same headers.
