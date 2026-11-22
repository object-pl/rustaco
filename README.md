<p align="center"><img src="https://raw.githubusercontent.com/theObjectCo/rustaco/main/docs/logo.png" alt="Rustaco logo: a smiling taco" width="240"></p>

# Rustaco

A code editor component written in Rust, MIT-licensed. It runs in the browser (WASM) and in native applications on one shared text core. Its feature scope matches Monaco: multiple cursors, syntax highlighting, LSP.

Status: early. Version 0.2.0 is on crates.io (`rustaco-core`, `rustaco-view`, `rustaco-syntax`, `rustaco-lsp`, `rustaco-desktop`) and npm (`rustaco`). The browser editor draws with WebGL2 (2D canvas as fallback) and works: highlighting for 20 languages (17 through tree-sitter, 3 through TextMate grammars), multiple cursors, folding, word wrap, minimap, sticky scroll, find and replace, decorations, IME input, a screen reader mirror, parsing and search in a Web Worker, snippets, and language features from JS providers (completion, hover, parameter hints, go to definition, references, rename, formatting, code actions, code lens, semantic tokens, inlay hints; `web/providers.html` shows a toy language built only from them). An LSP client works through `rustaco-lsp` (Rust, stdio) and `web/lsp.js` (browser, WebSocket); a diff editor works side by side and inline (`web/diff.html`); a native frontend draws the same frames in a winit window (`cargo run -p rustaco-desktop --example editor --release -- <file>`); the plan is in ROADMAP.md.

In a web page, from npm (`npm install rustaco`) or after `./scripts/pack-web.sh` (the package lands in `target/npm/`):

```js
import { createEditor, setTheme } from 'rustaco';

await setTheme('dark');
const editor = await createEditor(document.getElementById('editor'), {
  value: 'fn main() {}\n',
  language: 'rust',
  wordWrap: 'on',
  minimap: { enabled: true },
});
editor.onDidChangeContent(() => console.log(editor.getValue()));
```

The editor module is about 230 KB gzipped; each language module (290 to 690 KB) loads the first time a model uses that language. The page needs `'wasm-unsafe-eval'` in `script-src` when it sets a Content Security Policy; inline scripts and styles are not needed. Types for the API are in `web/rustaco.d.ts`.

```
cargo test
```

The crate layout is in [ARCHITECTURE.md](ARCHITECTURE.md), the plan in [ROADMAP.md](ROADMAP.md). Guides: [embedding in a web page](docs/embedding-web.md), [embedding in a native application](docs/embedding-desktop.md), [adding a language](docs/adding-a-language.md); measurements in [docs/performance.md](docs/performance.md); the open API questions before 1.0 in [docs/api-review.md](docs/api-review.md).

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
