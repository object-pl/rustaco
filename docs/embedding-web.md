# Embedding the editor in a web page

The package is plain ES modules plus WASM files: `rustaco.js` (the API), `rustaco-worker.js` (parsing and search off the main thread), `languages.js`, `suggest.js`, `features.js`, `lsp.js`, `diff.js`, and `pkg/` with the editor module and one module per language pack. Types are in `rustaco.d.ts` and `lsp.d.ts`. `scripts/pack-web.sh` builds the npm package into `target/npm/`.

## An editor

```js
import { createEditor, setTheme } from 'rustaco';

await setTheme('dark'); // 'light', 'high-contrast', or a VS Code theme object
const editor = await createEditor(document.getElementById('editor'), {
  value: 'fn main() {}\n',
  language: 'rust',
  wordWrap: 'on',
  minimap: { enabled: true },
  stickyScroll: { enabled: true },
});
editor.onDidChangeContent(() => save(editor.getValue()));
```

The container needs a size; the editor fills it and follows its resizes. Positions in the API are char offsets (Unicode scalar values) from the start of the text; `model.positionAt(offset)` and `model.offsetAt({ line, column })` convert to 0-based lines and columns.

Several editors can show one model (a split view): create the model with `createModel(text, language)` and pass it as `model` to each editor. Every editor keeps its own cursors, scroll position and folds.

## Options

`setOptions` takes Monaco's names: `fontFamily` and `fontSize` (the text is measured again once the font has loaded; only monospace fonts line up, since text sits on a grid of equal cells), `rulers`, `renderWhitespace` (`'none'` or `'all'`), `guides`, `occurrencesHighlight`, `bracketPairColorization`, `wordWrap`, `minimap`, `stickyScroll`, `quickSuggestions`, `maxPasteLength`. `setKeymap('jetbrains')` switches the preset; `addKeybindings([{ key: 'ctrl+k ctrl+c', command: 'editor.action.commentLine', when: 'editorHasSelection' }])` adds or removes bindings (a command starting with `-` removes). `docs/keybindings.md` lists the default bindings and their status.

## Languages, the worker and CSP

A language module downloads the first time a model uses one of its languages; `detectLanguage(fileName, firstLine)` picks an id. Parsing, highlighting, bracket and fold queries, search re-runs and diffs run in a Web Worker started on first use. Where workers cannot start (a CSP without `worker-src` for the page's origin, or `setWorkersEnabled(false)` before creating models), the same work runs on the main thread in 4 ms slices per frame.

In the worker each language module is a language server. Besides highlighting it answers completion (the grammar's keywords and the file's names), definition, references and rename within the file, and the editor registers those as providers for the module's languages; `setLanguageServicesEnabled(false)` before opening a language keeps them out. Providers the page registers for the same language are asked too, and their completion items are merged with these. Mermaid (`mermaid`, `.mmd`) has its own small module with colours and completion of diagram types, directions, shapes and node ids.

Under a Content Security Policy the page needs `'wasm-unsafe-eval'` in `script-src`. The editor uses no inline scripts and sets styles only through the CSSOM, so `style-src 'self'` is enough. The editor also works inside Shadow DOM.

## Language features from providers

Providers are registered per language id with Monaco's function names and receive the model and char offsets:

```js
import { languages } from 'rustaco';

languages.registerCompletionItemProvider('mylang', {
  triggerCharacters: ['.'],
  provideCompletionItems: (model, offset) => [
    { label: 'print', kind: languages.CompletionItemKind.Function, insertText: 'print(${1:value})', isSnippet: true },
  ],
});
languages.registerHoverProvider('mylang', {
  provideHover: (model, offset) => ({ contents: ['**print** writes a value'] }),
});
```

Each registration returns `{ dispose() }`. The full list (hover, signature help, definition, references, rename, formatting, code actions, highlights, folding and selection ranges, symbols, code lens, semantic tokens, inlay hints) and each provider's shape are documented at the top of `languages.js` and typed in `rustaco.d.ts`. `web/demo-language.js` implements all of them for a toy language; `web/providers.html` shows it running. Documentation text from providers renders code blocks, inline code, bold and italic, always as text: provider HTML is never parsed.

Without providers the editor still completes words of the file, builds the outline from the syntax tree, and folds by brackets or indentation.

## A language server over WebSocket

```js
import { connectLanguageServer, webSocketTransport } from 'rustaco/lsp';

const lsp = await connectLanguageServer({
  transport: webSocketTransport('wss://example.test/lsp'),
  languageId: 'rust',
  rootUri: 'file:///project',
  onApplyEdit: (uri, edits) => openAndApply(uri, edits), // edits for files not open here
});
lsp.attach(editor.getModel(), 'file:///project/src/main.rs');
```

The transport carries one JSON-RPC message per WebSocket text frame, which is what common stdio-to-WebSocket bridges produce. Any object with `send(string)` and `onMessage(listener)` works as a transport. The client registers providers for the capabilities the server announces, keeps attached models in sync with incremental changes, and shows diagnostics as squiggles with their messages in the hover.

## Decorations and widgets

```js
const error = await defineDecorationType({ underline: 'wavy', underlineColor: '#f14c4c', overviewRuler: '#f14c4c' });
const marks = editor.getModel().createDecorationsCollection([{ start: 10, end: 14, type: error }]);
```

Decorations move with edits and show in every editor of the model. View zones (`addViewZone({ afterLine, height, domNode })`) open space between lines for DOM content; content widgets (`addContentWidget({ domNode, position })`) follow a char offset; overlay widgets sit at a fixed corner.

## Diff editor

```js
import { createDiffEditor } from 'rustaco/diff';

const diff = await createDiffEditor(container, { original: oldText, modified: newText, language: 'rust', inline: false });
diff.goToChange(true);
diff.revertChange(0);
diff.collapseUnchanged(3);
```

Both sides stay editable and the diff recomputes 200 ms after an edit. `diff.changes` lists line ranges on both sides with the char ranges that differ inside them.

## Size and limits

The editor module is about 370 KB gzipped; language modules are 235 to 630 KB each. Pastes longer than `maxPasteLength` (50,000,000 UTF-16 units by default) are refused and reported through `editor.onDidRefusePaste(fn)`.
