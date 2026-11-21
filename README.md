# Rustaco

A code editor component written in Rust, MIT-licensed. It runs in the browser (WASM) and in native applications on one shared text core. Its feature scope matches Monaco: multiple cursors, syntax highlighting, LSP.

Status: early. The text core, the layout layer and a browser frontend work; syntax highlighting and LSP are next.

```
cargo test
```

The crate layout is in [ARCHITECTURE.md](ARCHITECTURE.md), the plan in [ROADMAP.md](ROADMAP.md).

Browser demo (requires `wasm-bindgen-cli` 0.2.130 and the `wasm32-unknown-unknown` target):

```
./scripts/build-web.sh
cd web && python -m http.server 8765
```
