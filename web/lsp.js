// A Language Server Protocol client for the browser. It talks JSON-RPC over a transport
// (a WebSocket to a server bridge, or any object with `send` and `onMessage`), keeps attached
// models in sync with incremental `didChange`, shows the server's diagnostics as squiggles,
// and registers the editor's providers (languages.js) backed by LSP requests.
//
// Positions: the editor counts chars (code points), LSP counts UTF-16 units by default,
// which is what JS strings count, so the conversion goes through each line's text.

import { defineDecorationType, languages } from './rustaco.js';

/** A transport over a WebSocket carrying one JSON-RPC message per text frame. */
export function webSocketTransport(url) {
  const socket = new WebSocket(url);
  const listeners = [];
  const queue = [];
  socket.addEventListener('message', (e) => { for (const fn of listeners) fn(e.data); });
  socket.addEventListener('open', () => { for (const m of queue.splice(0)) socket.send(m); });
  return {
    send(text) { if (socket.readyState === WebSocket.OPEN) socket.send(text); else queue.push(text); },
    onMessage(fn) { listeners.push(fn); },
    close() { socket.close(); },
  };
}

/** `{ line, character }` (UTF-16) of a char offset. */
export function toPosition(model, offset) {
  const { line, column } = model.positionAt(offset);
  const text = model.getLineContent(line);
  let character = 0;
  let i = 0;
  for (const c of text) {
    if (i++ >= column) break;
    character += c.length;
  }
  return { line, character };
}

/** The char offset of an LSP position (UTF-16), clamped into the text. */
export function toOffset(model, position) {
  const line = Math.min(position.line, model.getLineCount() - 1);
  const text = model.getLineContent(line);
  let column = 0;
  let units = 0;
  for (const c of text) {
    if (units >= position.character) break;
    units += c.length;
    column++;
  }
  return model.offsetAt({ line, column });
}

const toRange = (model, r) => ({ start: toOffset(model, r.start), end: toOffset(model, r.end) });
const fromRange = (model, r) => ({ start: toPosition(model, r.start), end: toPosition(model, r.end) });

/** Text of a MarkedString or MarkupContent: code in a ``` fence, markdown as is. */
function markup(v) {
  if (v == null) return '';
  if (typeof v === 'string') return v;
  if (v.language) return `\`\`\`${v.language}\n${v.value}\n\`\`\``;
  return v.value ?? '';
}

let diagnosticTypes = null;
async function diagnosticType(severity) {
  diagnosticTypes ??= Promise.all([
    defineDecorationType({ underline: 'wavy', underlineColor: '#f14c4c', overviewRuler: '#f14c4c', gutter: '#f14c4c' }),
    defineDecorationType({ underline: 'wavy', underlineColor: '#cca700', overviewRuler: '#cca700' }),
    defineDecorationType({ underline: 'dotted', underlineColor: '#3794ff' }),
    defineDecorationType({ underline: 'dotted', underlineColor: '#888888' }),
  ]);
  return (await diagnosticTypes)[Math.min(Math.max((severity ?? 1) - 1, 0), 3)];
}

/**
 * Starts a client: `initialize` over `transport`, then providers for `languageId`
 * registered for the capabilities the server announced. Returns the client; `attach(model,
 * uri)` opens a model on the server.
 */
export async function connectLanguageServer({ transport, languageId, rootUri = null, onApplyEdit = null }) {
  const client = new LspClient(transport, languageId, onApplyEdit);
  await client.initialize(rootUri);
  client.registerProviders();
  return client;
}

export class LspClient {
  constructor(transport, languageId, onApplyEdit) {
    this.transport = transport;
    this.languageId = languageId;
    this.onApplyEdit = onApplyEdit;
    this.nextId = 1;
    this.pending = new Map();
    /** Attached models: model -> { uri, version, sent, diagnostics, collection, off }. */
    this.docs = new Map();
    this.byUri = new Map();
    this.disposables = [];
    transport.onMessage((text) => this.#receive(JSON.parse(text)));
  }

  request(method, params) {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.transport.send(JSON.stringify({ jsonrpc: '2.0', id, method, params }));
    });
  }

  notify(method, params) {
    this.transport.send(JSON.stringify({ jsonrpc: '2.0', method, params }));
  }

  #receive(msg) {
    if (msg.id !== undefined && msg.method === undefined) {
      const p = this.pending.get(msg.id);
      this.pending.delete(msg.id);
      if (msg.error) p?.reject(new Error(msg.error.message));
      else p?.resolve(msg.result ?? null);
      return;
    }
    if (msg.method === 'textDocument/publishDiagnostics') {
      this.#diagnostics(msg.params);
    } else if (msg.method === 'workspace/applyEdit') {
      const applied = this.applyWorkspaceEdit(msg.params.edit);
      this.transport.send(JSON.stringify({ jsonrpc: '2.0', id: msg.id, result: { applied } }));
    } else if (msg.id !== undefined) {
      // Other requests (configuration, progress): an empty answer keeps the server going.
      this.transport.send(JSON.stringify({ jsonrpc: '2.0', id: msg.id, result: null }));
    }
  }

  async initialize(rootUri) {
    const result = await this.request('initialize', {
      processId: null,
      rootUri,
      clientInfo: { name: 'rustaco' },
      capabilities: {
        general: { positionEncodings: ['utf-16'] },
        textDocument: {
          synchronization: { didSave: false },
          hover: { contentFormat: ['markdown', 'plaintext'] },
          completion: { completionItem: { snippetSupport: true, documentationFormat: ['markdown', 'plaintext'] }, contextSupport: true },
          signatureHelp: { signatureInformation: { parameterInformation: { labelOffsetSupport: true } } },
          definition: { linkSupport: true },
          references: {}, documentHighlight: {}, formatting: {}, rename: {}, foldingRange: { lineFoldingOnly: true },
          selectionRange: {}, codeLens: {}, inlayHint: {},
          documentSymbol: { hierarchicalDocumentSymbolSupport: true },
          codeAction: { codeActionLiteralSupport: { codeActionKind: { valueSet: ['', 'quickfix', 'refactor', 'source'] } } },
          semanticTokens: { requests: { full: true }, tokenTypes: [], tokenModifiers: [], formats: ['relative'] },
          publishDiagnostics: {},
        },
        workspace: { applyEdit: true, workspaceEdit: { documentChanges: true } },
      },
    });
    this.capabilities = result?.capabilities ?? {};
    this.notify('initialized', {});
    return this.capabilities;
  }

  /** Opens `model` on the server as `uri` and keeps it in sync. */
  attach(model, uri) {
    const doc = { uri, version: 1, sent: model.core.version(), collection: model.createDecorationsCollection() };
    this.docs.set(model, doc);
    this.byUri.set(uri, model);
    this.notify('textDocument/didOpen', { textDocument: { uri, languageId: this.languageId, version: 1, text: model.getValue() } });
    doc.off = model.onDidChangeContent(() => this.sync(model));
    return () => this.detach(model);
  }

  detach(model) {
    const doc = this.docs.get(model);
    if (!doc) return;
    doc.off?.();
    doc.collection.clear();
    this.notify('textDocument/didClose', { textDocument: { uri: doc.uri } });
    this.docs.delete(model);
    this.byUri.delete(doc.uri);
  }

  /** Sends the model's edits since the last sync: incremental when the server takes them. */
  sync(model) {
    const doc = this.docs.get(model);
    if (!doc) return null;
    const v = model.core.version();
    if (v === doc.sent) return doc;
    const kind = typeof this.capabilities.textDocumentSync === 'number'
      ? this.capabilities.textDocumentSync : this.capabilities.textDocumentSync?.change ?? 1;
    const batch = model.core.editsSince(doc.sent);
    let changes;
    if (kind === 2 && !batch.stale()) {
      const n = batch.utf16();
      const texts = batch.texts();
      changes = texts.map((text, i) => ({
        range: { start: { line: n[i * 4], character: n[i * 4 + 1] }, end: { line: n[i * 4 + 2], character: n[i * 4 + 3] } },
        text,
      }));
    } else {
      changes = [{ text: model.getValue() }];
    }
    batch.free();
    doc.sent = v;
    doc.version++;
    this.notify('textDocument/didChange', { textDocument: { uri: doc.uri, version: doc.version }, contentChanges: changes });
    return doc;
  }

  async #diagnostics({ uri, diagnostics }) {
    const model = this.byUri.get(uri);
    const doc = model && this.docs.get(model);
    if (!doc) return;
    doc.diagnostics = diagnostics.map((d) => ({ ...d, offsets: toRange(model, d.range) }));
    const list = [];
    for (const d of doc.diagnostics) {
      const r = d.offsets.start === d.offsets.end ? { start: d.offsets.start, end: d.offsets.start + 1 } : d.offsets;
      list.push({ start: r.start, end: r.end, type: await diagnosticType(d.severity) });
    }
    doc.collection.set(list);
  }

  /** Applies a WorkspaceEdit to attached models; returns whether every document was attached. */
  applyWorkspaceEdit(edit) {
    const perUri = new Map();
    for (const [uri, edits] of Object.entries(edit?.changes ?? {})) perUri.set(uri, edits);
    for (const c of edit?.documentChanges ?? []) if (c.textDocument) perUri.set(c.textDocument.uri, c.edits);
    let all = true;
    for (const [uri, edits] of perUri) {
      const model = this.byUri.get(uri);
      const editor = model && [...model.editors][0];
      if (!editor) {
        all = false;
        if (this.onApplyEdit) this.onApplyEdit(uri, edits);
        continue;
      }
      editor.features.applyEdits(edits.map((e) => ({ range: toRange(model, e.range), text: e.newText })));
    }
    return all;
  }

  /** `{ textDocument, position }` for a request about `offset`, after syncing. */
  #at(model, offset) {
    const doc = this.sync(model);
    if (!doc) return null;
    return { textDocument: { uri: doc.uri }, position: toPosition(model, offset) };
  }

  #doc(model) {
    const doc = this.sync(model);
    return doc ? { textDocument: { uri: doc.uri } } : null;
  }

  /** Registers the editor providers the server's capabilities allow. */
  registerProviders() {
    const c = this.capabilities;
    const id = this.languageId;
    const reg = (cap, register) => { if (c[cap]) this.disposables.push(register()); };
    const locations = (model, result) => {
      const list = Array.isArray(result) ? result : result ? [result] : [];
      return list.map((l) => {
        const uri = l.uri ?? l.targetUri;
        const r = l.range ?? l.targetSelectionRange;
        const target = this.byUri.get(uri);
        return target === model ? toRange(model, r) : { model: target ?? null, uri, lspRange: r, range: target ? toRange(target, r) : null };
      });
    };

    reg('hoverProvider', () => languages.registerHoverProvider(id, {
      provideHover: async (model, offset) => {
        const params = this.#at(model, offset);
        const diagnostics = (this.docs.get(model)?.diagnostics ?? []).filter((d) => d.offsets.start <= offset && offset <= d.offsets.end);
        const result = params ? await this.request('textDocument/hover', params) : null;
        const contents = [...diagnostics.map((d) => d.message)];
        if (result) contents.push(...(Array.isArray(result.contents) ? result.contents : [result.contents]).map(markup));
        if (!contents.length) return null;
        return { contents, range: result?.range ? toRange(model, result.range) : undefined };
      },
    }));
    reg('completionProvider', () => languages.registerCompletionItemProvider(id, {
      triggerCharacters: c.completionProvider.triggerCharacters ?? [],
      provideCompletionItems: async (model, offset, context) => {
        const params = this.#at(model, offset);
        if (!params) return [];
        params.context = context.triggerCharacter ? { triggerKind: 2, triggerCharacter: context.triggerCharacter } : { triggerKind: 1 };
        const result = await this.request('textDocument/completion', params);
        const items = Array.isArray(result) ? result : result?.items ?? [];
        return items.map((item) => {
          const edit = item.textEdit;
          const range = edit ? toRange(model, edit.range ?? edit.replace) : undefined;
          return {
            label: item.label,
            kind: item.kind ? item.kind - 1 : undefined,
            detail: item.detail,
            documentation: markup(item.documentation),
            insertText: edit?.newText ?? item.insertText ?? item.label,
            isSnippet: item.insertTextFormat === 2,
            sortText: item.sortText,
            filterText: item.filterText,
            range,
          };
        });
      },
    }));
    reg('signatureHelpProvider', () => languages.registerSignatureHelpProvider(id, {
      signatureHelpTriggerCharacters: c.signatureHelpProvider.triggerCharacters ?? ['(', ','],
      provideSignatureHelp: async (model, offset) => {
        const params = this.#at(model, offset);
        const result = params ? await this.request('textDocument/signatureHelp', params) : null;
        if (!result?.signatures?.length) return null;
        return {
          signatures: result.signatures.map((s) => ({
            label: s.label,
            documentation: markup(s.documentation),
            parameters: (s.parameters ?? []).map((p) => ({ label: Array.isArray(p.label) ? s.label.slice(p.label[0], p.label[1]) : p.label })),
          })),
          activeSignature: result.activeSignature ?? 0,
          activeParameter: result.activeParameter ?? 0,
        };
      },
    }));
    reg('definitionProvider', () => languages.registerDefinitionProvider(id, {
      provideDefinition: async (model, offset) => locations(model, await this.request('textDocument/definition', this.#at(model, offset))),
    }));
    reg('referencesProvider', () => languages.registerReferenceProvider(id, {
      provideReferences: async (model, offset) => {
        const params = { ...this.#at(model, offset), context: { includeDeclaration: true } };
        return locations(model, await this.request('textDocument/references', params)).filter((r) => 'start' in r);
      },
    }));
    reg('documentHighlightProvider', () => languages.registerDocumentHighlightProvider(id, {
      provideDocumentHighlights: async (model, offset) => {
        const result = await this.request('textDocument/documentHighlight', this.#at(model, offset));
        return (result ?? []).map((h) => toRange(model, h.range));
      },
    }));
    reg('renameProvider', () => languages.registerRenameProvider(id, {
      provideRenameEdits: async (model, offset, newName) => {
        const result = await this.request('textDocument/rename', { ...this.#at(model, offset), newName });
        // Edits to this model come back for the editor; other documents go to the host.
        const own = [];
        const others = { changes: {} };
        const add = (uri, edits) => {
          if (this.byUri.get(uri) === model) own.push(...edits.map((e) => ({ range: toRange(model, e.range), text: e.newText })));
          else others.changes[uri] = edits;
        };
        for (const [uri, edits] of Object.entries(result?.changes ?? {})) add(uri, edits);
        for (const ch of result?.documentChanges ?? []) if (ch.textDocument) add(ch.textDocument.uri, ch.edits);
        if (Object.keys(others.changes).length) this.applyWorkspaceEdit(others);
        return own;
      },
    }));
    reg('documentFormattingProvider', () => languages.registerDocumentFormattingEditProvider(id, {
      provideDocumentFormattingEdits: async (model, options) => {
        const result = await this.request('textDocument/formatting', { ...this.#doc(model), options });
        return (result ?? []).map((e) => ({ range: toRange(model, e.range), text: e.newText }));
      },
    }));
    reg('codeActionProvider', () => languages.registerCodeActionProvider(id, {
      provideCodeActions: async (model, range) => {
        const diagnostics = (this.docs.get(model)?.diagnostics ?? []).filter((d) => d.offsets.start <= range.end && range.start <= d.offsets.end);
        const result = await this.request('textDocument/codeAction', {
          ...this.#doc(model), range: fromRange(model, range), context: { diagnostics: diagnostics.map(({ offsets, ...d }) => d) },
        });
        return (result ?? []).map((a) => ({
          title: a.title,
          edits: undefined,
          command: () => {
            if (a.edit) this.applyWorkspaceEdit(a.edit);
            const cmd = typeof a.command === 'string' ? a : a.command;
            if (cmd) this.request('workspace/executeCommand', { command: cmd.command, arguments: cmd.arguments ?? [] });
          },
        }));
      },
    }));
    reg('foldingRangeProvider', () => languages.registerFoldingRangeProvider(id, {
      provideFoldingRanges: async (model) => {
        const result = await this.request('textDocument/foldingRange', this.#doc(model));
        return (result ?? []).map((f) => ({ start: f.startLine, end: f.endLine }));
      },
    }));
    reg('selectionRangeProvider', () => languages.registerSelectionRangeProvider(id, {
      provideSelectionRanges: async (model, offset) => {
        const result = await this.request('textDocument/selectionRange', { ...this.#doc(model), positions: [toPosition(model, offset)] });
        const out = [];
        for (let n = result?.[0]; n; n = n.parent) out.push(toRange(model, n.range));
        return out;
      },
    }));
    reg('documentSymbolProvider', () => languages.registerDocumentSymbolProvider(id, {
      provideDocumentSymbols: async (model) => {
        const result = await this.request('textDocument/documentSymbol', this.#doc(model));
        const convert = (s) => ({
          name: s.name,
          kind: (s.kind ?? 13) - 1,
          detail: s.detail,
          range: toRange(model, s.range ?? s.location.range),
          selectionRange: toRange(model, s.selectionRange ?? s.range ?? s.location.range),
          children: (s.children ?? []).map(convert),
        });
        return (result ?? []).map(convert);
      },
    }));
    reg('codeLensProvider', () => languages.registerCodeLensProvider(id, {
      provideCodeLenses: async (model) => {
        const result = await this.request('textDocument/codeLens', this.#doc(model));
        return (result ?? []).filter((l) => l.command).map((l) => ({
          line: l.range.start.line,
          title: l.command.title,
          command: () => this.request('workspace/executeCommand', { command: l.command.command, arguments: l.command.arguments ?? [] }),
        }));
      },
    }));
    reg('inlayHintProvider', () => languages.registerInlayHintsProvider(id, {
      provideInlayHints: async (model, range) => {
        const result = await this.request('textDocument/inlayHint', { ...this.#doc(model), range: fromRange(model, range) });
        return (result ?? []).map((h) => ({
          offset: toOffset(model, h.position),
          label: typeof h.label === 'string' ? h.label : h.label.map((p) => p.value).join(''),
        }));
      },
    }));
    reg('semanticTokensProvider', () => {
      const legend = c.semanticTokensProvider.legend ?? { tokenTypes: [] };
      return languages.registerDocumentSemanticTokensProvider(id, {
        provideSemanticTokens: async (model) => {
          const result = await this.request('textDocument/semanticTokens/full', this.#doc(model));
          const data = result?.data ?? [];
          const out = [];
          let line = 0;
          let start = 0;
          for (let i = 0; i + 4 < data.length; i += 5) {
            if (data[i] > 0) { line += data[i]; start = data[i + 1]; } else start += data[i + 1];
            const s = toOffset(model, { line, character: start });
            const e = toOffset(model, { line, character: start + data[i + 2] });
            out.push({ start: s, end: e, style: SEMANTIC_STYLES[legend.tokenTypes[data[i + 3]]] ?? 'variable' });
          }
          return out;
        },
      });
    });
  }

  /** `shutdown`, `exit`, and the providers go away. */
  async dispose() {
    for (const d of this.disposables) d.dispose();
    for (const model of [...this.docs.keys()]) this.detach(model);
    try { await this.request('shutdown', null); } catch { /* the server may be gone */ }
    this.notify('exit', null);
  }
}

/** LSP semantic token types to the editor's highlight names. */
const SEMANTIC_STYLES = {
  namespace: 'module', type: 'type', class: 'type', enum: 'type', interface: 'type', struct: 'type',
  typeParameter: 'type', parameter: 'variable.parameter', variable: 'variable', property: 'property',
  enumMember: 'constant', event: 'property', function: 'function', method: 'function.method', macro: 'function.macro',
  keyword: 'keyword', modifier: 'keyword', comment: 'comment', string: 'string', number: 'number',
  regexp: 'string.special', operator: 'operator', decorator: 'attribute', label: 'label',
};
