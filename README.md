# Rustaco

A code editor component written in Rust, MIT-licensed. It runs in the browser (WASM) and in native applications on one shared text core. Its feature scope matches Monaco: multiple cursors, syntax highlighting, LSP.

Status: early. The text core, the layout layer, a browser frontend and tree-sitter highlighting for 18 languages work. The plan is in ROADMAP.md.

```
cargo test
```

The crate layout is in [ARCHITECTURE.md](ARCHITECTURE.md), the plan in [ROADMAP.md](ROADMAP.md).

Browser demo (requires `wasm-bindgen-cli` 0.2.130, the `wasm32-unknown-unknown` target and clang; on Windows a portable LLVM unpacked into `~/tools` is found automatically):

```
./scripts/build-web.sh
cd web && python -m http.server 8765
```

Checks (the same script CI runs; `--e2e` adds the Playwright tests in Chromium, Firefox and WebKit):

```
./scripts/ci.sh
./scripts/ci.sh --e2e
cargo bench -p rustaco-view
cd fuzz && cargo +nightly fuzz run document_ops
```

On Windows the fuzzer needs `clang_rt.asan_dynamic-x86_64.dll` from Visual Studio (`VC/Tools/MSVC/<version>/bin/Hostx64/x64`) on `PATH`.
