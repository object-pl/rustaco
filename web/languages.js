// Language feature providers: a host registers them per language id, the editor asks them.
// Positions are char offsets into the model's text; ranges are `{ start, end }` offsets.

/**
 * Cancellation for provider requests, as Monaco has it: every provider gets a token as its
 * last argument; the editor cancels it when the answer is no longer wanted (the caret
 * moved, a newer request of the same kind started). `token.isCancellationRequested` and
 * `token.onCancellationRequested(fn)` (returns `{ dispose }`).
 */
export class CancellationTokenSource {
  constructor() {
    const listeners = new Set();
    this.listeners = listeners;
    this.token = {
      isCancellationRequested: false,
      onCancellationRequested(fn) {
        if (this.isCancellationRequested) { fn(); return { dispose() {} }; }
        listeners.add(fn);
        return { dispose: () => listeners.delete(fn) };
      },
    };
  }

  cancel() {
    if (this.token.isCancellationRequested) return;
    this.token.isCancellationRequested = true;
    for (const fn of this.listeners) {
      try { fn(); } catch { /* a listener's failure is its own */ }
    }
    this.listeners.clear();
  }
}

const registries = new Map();

function register(kind, languageId, provider) {
  if (!registries.has(kind)) registries.set(kind, new Map());
  const byLanguage = registries.get(kind);
  if (!byLanguage.has(languageId)) byLanguage.set(languageId, []);
  const list = byLanguage.get(languageId);
  list.push(provider);
  for (const fn of listeners) fn(kind, languageId);
  return {
    dispose() {
      const i = list.indexOf(provider);
      if (i >= 0) list.splice(i, 1);
      for (const fn of listeners) fn(kind, languageId);
    },
  };
}

const listeners = new Set();

/** Calls `fn(kind, languageId)` whenever a provider is registered or disposed. */
export function onDidChangeProviders(fn) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

/** Providers of `kind` for `languageId`, those registered for `"*"` last; fallbacks only when nothing else serves it. */
export function providersFor(kind, languageId) {
  const byLanguage = registries.get(kind);
  if (!byLanguage) return [];
  const all = [...(byLanguage.get(languageId) ?? []), ...(languageId === '*' ? [] : byLanguage.get('*') ?? [])];
  // Fallbacks (the bundled language modules' guesses) step aside for any other provider.
  const primary = all.filter((p) => !p.fallback);
  return primary.length ? primary : all;
}

/** Kinds of completion items, as Monaco numbers them; they pick the icon. */
export const CompletionItemKind = Object.freeze({
  Method: 0, Function: 1, Constructor: 2, Field: 3, Variable: 4, Class: 5, Struct: 6, Interface: 7, Module: 8,
  Property: 9, Event: 10, Operator: 11, Unit: 12, Value: 13, Constant: 14, Enum: 15, EnumMember: 16, Keyword: 17,
  Text: 18, Color: 19, File: 20, Reference: 21, Customcolor: 22, Folder: 23, TypeParameter: 24, User: 25, Issue: 26,
  Snippet: 27,
});

/** Kinds of document symbols (LSP numbering). */
export const SymbolKind = Object.freeze({
  File: 0, Module: 1, Namespace: 2, Package: 3, Class: 4, Method: 5, Property: 6, Field: 7, Constructor: 8,
  Enum: 9, Interface: 10, Function: 11, Variable: 12, Constant: 13, String: 14, Number: 15, Boolean: 16,
  Array: 17, Object: 18, Key: 19, Null: 20, EnumMember: 21, Struct: 22, Event: 23, Operator: 24, TypeParameter: 25,
});

/**
 * Registration functions, named as in Monaco. Each takes a language id (`"*"` for every
 * language) and a provider object, and returns `{ dispose() }`.
 *
 * - completion: `{ triggerCharacters?, provideCompletionItems(model, offset, context) }`
 *   returning items `{ label, kind?, detail?, documentation?, insertText?, isSnippet?,
 *   sortText?, filterText?, range? }` (or `{ suggestions: [...] }`).
 * - hover: `{ provideHover(model, offset) }` returning `{ contents: string | string[], range? }`.
 * - signature help: `{ signatureHelpTriggerCharacters?, provideSignatureHelp(model, offset) }`
 *   returning `{ signatures: [{ label, documentation?, parameters: [{ label }] }],
 *   activeSignature, activeParameter }`.
 * - definition / references: `{ provideDefinition(model, offset) }`,
 *   `{ provideReferences(model, offset) }` returning ranges (or `{ range }` objects).
 * - rename: `{ provideRenameEdits(model, offset, newName) }` returning edits
 *   `[{ range, text }]`, optionally `resolveRenameLocation(model, offset)` returning
 *   `{ range, text }` or `{ rejectReason }`.
 * - formatting: `{ provideDocumentFormattingEdits(model, options) }` returning edits.
 * - code actions: `{ provideCodeActions(model, range) }` returning
 *   `[{ title, edits?, command? }]`.
 * - document highlights: `{ provideDocumentHighlights(model, offset) }` returning ranges.
 * - folding ranges: `{ provideFoldingRanges(model) }` returning `[{ start, end }]` lines.
 * - selection ranges: `{ provideSelectionRanges(model, offset) }` returning ranges from
 *   the innermost out.
 * - document symbols: `{ provideDocumentSymbols(model) }` returning
 *   `[{ name, kind, range, children? }]`.
 * - code lens: `{ provideCodeLenses(model) }` returning `[{ line, title, command? }]`.
 * - semantic tokens: `{ provideSemanticTokens(model) }` returning
 *   `[{ start, end, style }]` with highlight names (`"variable.parameter"`, ...).
 * - inlay hints: `{ provideInlayHints(model, range) }` returning `[{ offset, label }]`.
 */
export const languages = Object.freeze({
  registerCompletionItemProvider: (id, p) => register('completion', id, p),
  registerHoverProvider: (id, p) => register('hover', id, p),
  registerSignatureHelpProvider: (id, p) => register('signatureHelp', id, p),
  registerDefinitionProvider: (id, p) => register('definition', id, p),
  registerReferenceProvider: (id, p) => register('references', id, p),
  registerRenameProvider: (id, p) => register('rename', id, p),
  registerDocumentFormattingEditProvider: (id, p) => register('formatting', id, p),
  registerCodeActionProvider: (id, p) => register('codeAction', id, p),
  registerDocumentHighlightProvider: (id, p) => register('documentHighlight', id, p),
  registerFoldingRangeProvider: (id, p) => register('foldingRange', id, p),
  registerSelectionRangeProvider: (id, p) => register('selectionRange', id, p),
  registerDocumentSymbolProvider: (id, p) => register('documentSymbol', id, p),
  registerCodeLensProvider: (id, p) => register('codeLens', id, p),
  registerDocumentSemanticTokensProvider: (id, p) => register('semanticTokens', id, p),
  registerInlayHintsProvider: (id, p) => register('inlayHints', id, p),
  CompletionItemKind,
  SymbolKind,
});

