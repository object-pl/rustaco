/* Rustaco patch: tree-sitter >= 0.27 provides the libc subset on wasm32-unknown-unknown, so
   grammars that still compile these files get an empty unit instead of the upstream #error.
   See vendor/README.md. */
