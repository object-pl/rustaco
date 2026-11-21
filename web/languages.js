// Language feature providers: a host registers them per language id, the editor asks them.
// Positions are char offsets into the model's text; ranges are `{ start, end }` offsets.

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

/** Providers of `kind` for `languageId`, those registered for `"*"` last. */
export function providersFor(kind, languageId) {
  const byLanguage = registries.get(kind);
  if (!byLanguage) return [];
  return [...(byLanguage.get(languageId) ?? []), ...(languageId === '*' ? [] : byLanguage.get('*') ?? [])];
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

/**
 * Fuzzy match of `pattern` in `word`, case-insensitive, as completion filters: every pattern
 * char must appear in order. Matches at the word start, after `_`, `-`, `.` or at a lower to
 * upper case step score higher, and so do runs of consecutive chars. Returns
 * `{ score, matches }` (indexes of the matched chars) or null.
 */
export function fuzzyScore(pattern, word) {
  if (!pattern) return { score: 0, matches: [] };
  const p = pattern.toLowerCase();
  const w = word.toLowerCase();
  const boundary = (i) => i === 0 || '_-. /:'.includes(word[i - 1])
    || (word[i] !== word[i].toLowerCase() && word[i - 1] === word[i - 1].toLowerCase());
  // The first char must match at the start or at a boundary, as VS Code requires.
  let best = null;
  for (let first = 0; first < w.length; first++) {
    if (w[first] !== p[0] || !boundary(first)) continue;
    const matches = [first];
    let score = (first === 0 ? 8 : 5) + (word[first] === pattern[0] ? 1 : 0);
    let wi = first + 1;
    for (let pi = 1; pi < p.length; pi++) {
      while (wi < w.length && w[wi] !== p[pi]) wi++;
      if (wi >= w.length) { score = -1; break; }
      const consecutive = matches[matches.length - 1] === wi - 1;
      score += 1 + (consecutive ? 3 : 0) + (boundary(wi) ? 4 : 0) + (word[wi] === pattern[pi] ? 1 : 0);
      matches.push(wi);
      wi++;
    }
    if (score >= 0 && (!best || score > best.score)) best = { score: score - first * 0.1, matches };
    if (first === 0 && best) break;
  }
  return best;
}
