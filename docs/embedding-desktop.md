# Embedding the editor in a native application

The native side is a set of Rust crates:

| Crate | Role |
|---|---|
| `rustaco-core` | Text model (rope), cursors, commands, undo, keymaps, search, snippets, decorations, diff. No UI, no I/O. |
| `rustaco-view` | Layout: turns a model, selections and a viewport into a `Frame` of positioned runs, rectangles and carets. Folding, wrapping, view zones, minimap and sticky scroll data. |
| `rustaco-syntax` | Tree-sitter and TextMate highlighting, themes, the language catalog, `BackgroundSyntax` (parsing on a thread). |
| `rustaco-lsp` | LSP client over a server's stdio. |
| `rustaco-desktop` | `DesktopEditor`: the above wired to winit events, drawing frames into a pixel buffer. |

## With `rustaco-desktop`

`DesktopEditor` owns a document, its view, keymap and highlighting. The host owns the window and the event loop:

```rust
let font = Font::system_monospace((14.0 * scale).round())?;
let mut editor = DesktopEditor::new(&text, "rust", font, width, height, scale);

// winit events:
editor.key_down(key_press(&event.key_without_modifiers(), modifiers).unwrap(), event.text.as_deref());
editor.ime_preedit(&preedit);
editor.ime_commit(&text);
editor.mouse_down(x, y, Button::Left, shift, alt);
editor.wheel(dx, dy);
editor.resize(width, height, scale);

// Once per loop iteration, then draw when needed:
if editor.poll_highlights() || editor.dirty {
    editor.render(&mut buffer, physical_width, physical_height); // 0RGB pixels
}
```

Coordinates passed in are logical pixels; the buffer is in physical pixels. `examples/editor.rs` is a complete winit application presenting the buffer with `softbuffer`: it opens a file, saves with Ctrl+S, places the IME candidate window at the caret with `caret_rect`, and, started with `--lsp <server command>`, talks to a language server.

```
cargo run -p rustaco-desktop --example editor --release -- src/main.rs --lsp rust-analyzer
```

The renderer draws text with one monospace face from the system fonts (Cascadia Code, Consolas, SF Mono, Menlo, DejaVu Sans Mono and others are tried in that order). There is no shaping, no ligatures and no fallback font for chars the face lacks; bold and italic styles draw in the regular face.

## With your own renderer

A host that draws with its own toolkit (a GPU renderer, egui, iced) uses the lower crates directly:

1. Keep a `rustaco_core::Document` (or a `TextModel` with one `Cursors` per editor for split views). Run commands with `doc.editor().run("editor.action.commentLine")`, resolve keys with `default_keymap(platform).resolve(&press, &context)`.
2. Keep a `rustaco_view::View` per editor. Each frame call `view.layout(model, selections, &highlights)`; it returns runs with x positions, line tops, selection and caret rectangles, fold markers and zone rectangles, all in logical pixels relative to the editor's top-left. Draw them; the view never measures text, it assumes cells of `metrics.char_width`.
3. For hit testing call `view.hit_test(rope, x, y)`; for vertical caret moves `view.vertical_target(...)`, which steps over folds and wrapped rows.
4. For highlighting, `rustaco_syntax::BackgroundSyntax::spawn(by_id("rust").unwrap(), model)` parses on a thread. Each frame `request_highlights(model, visible_lines)` and `poll(model)`; replies for an older text version are dropped. The returned `Highlights` implement the view's `Styler`.

## Language servers

```rust
let (mut client, _child) = LspClient::spawn("rust-analyzer", &[], Some(&root))?;
client.initialize(Some("file:///path/to/root"))?;
let mut doc = client.open("file:///path/to/root/src/main.rs", "rust", model)?;
// after edits:
client.sync(&mut doc, model)?;
let hover = client.hover(&mut doc, model, caret_offset)?;
```

The client negotiates UTF-8 positions when the server offers them and UTF-16 otherwise, sends incremental changes from the model's edit log, and returns char offsets for the document asked about. Locations and edits in other files keep their LSP ranges for the host. Server notifications (diagnostics) and requests (`workspace/applyEdit`) arrive on `client.connection().messages()`. `cargo run -p rustaco-lsp --example probe -- <server> <file> <line:column>` prints what each request returns.
