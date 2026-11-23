// A Language Server Protocol client for the browser. The protocol runs in the editor's wasm
// (`LspSession`, built on rustaco-lsp's `Session`): it builds every message, keeps attached
// models in sync with incremental `didChange`, converts positions in the encoding the server
// chose, and decodes every answer into char offsets. This file carries the strings over a
// transport (a WebSocket to a server bridge, or any object with `send` and `onMessage`),
// keeps the promises, shows diagnostics as squiggles and registers the editor's providers.

import { defineDecorationType, languages, lspSession, lspSessionNow } from './rustaco.js';

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

/** A session that never talked to a server: positions in UTF-16, LSP's default. */
let utf16 = null;

/** `{ line, character }` (UTF-16) of a char offset. */
export function toPosition(model, offset) {
  utf16 ??= lspSessionNow();
  return JSON.parse(utf16.toPosition(model.core, offset));
}

/** The char offset of an LSP position (UTF-16), clamped into the text. */
export function toOffset(model, position) {
  utf16 ??= lspSessionNow();
  return utf16.toOffset(model.core, JSON.stringify(position));
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
  const client = new LspClient(transport, languageId, onApplyEdit, await lspSession());
  await client.initialize(rootUri);
  client.registerProviders();
  return client;
}

export class LspClient {
  constructor(transport, languageId, onApplyEdit, session) {
    this.transport = transport;
    this.languageId = languageId;
    this.onApplyEdit = onApplyEdit;
    this.session = session;
    /** Requests waiting for an answer: id -> { resolve, reject, model }. */
    this.pending = new Map();
    /** Attached models: model -> { uri, handle, diagnostics, collection, off }. */
    this.docs = new Map();
    this.byUri = new Map();
    this.disposables = [];
    /** Raw requests (`request`), answered here without the session: id -> { resolve, reject }. */
    this.raw = new Map();
    this.nextRaw = 1;
    /** Called with (method, params) for notifications the client does not handle itself. */
    this.onNotification = null;
    transport.onMessage((text) => this.#receive(text));
  }

  /** Sends what the session queued. */
  #flush() {
    for (const m of this.session.take()) this.transport.send(m);
  }

  /**
   * Waits for request `id`. When `token` (from the editor's provider call) is cancelled
   * first, the server gets `$/cancelRequest` and the promise resolves to null.
   */
  #wait(id, model, token) {
    if (id < 0) return Promise.resolve(null);
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject, model });
      this.#flush();
      token?.onCancellationRequested(() => {
        if (!this.pending.has(id)) return;
        this.pending.delete(id);
        this.session.cancel(id);
        this.#flush();
        resolve(null);
      });
    });
  }

  /** Asks the server about an attached model: `kind` is the editor feature (see LspSession.request). */
  #ask(kind, model, token, a = 0, b = 0, text = undefined) {
    const doc = this.docs.get(model);
    if (!doc) return Promise.resolve(null);
    return this.#wait(this.session.request(kind, doc.handle, model.core, a, b, text), model, token);
  }

  /**
   * Asks about an attached model by editor feature (`'foldingRange'`, `'documentSymbol'`,
   * ...; see `LspSession.request`); resolves to the decoded answer.
   */
  ask(kind, model, a = 0, b = 0, text = undefined) {
    return this.#ask(kind, model, null, a, b, text);
  }

  /** Rejects every waiting request, for a transport that is gone. */
  abort(reason = 'the connection closed') {
    for (const p of [...this.pending.values(), ...this.raw.values()]) p.reject(new Error(reason));
    this.pending.clear();
    this.raw.clear();
  }

  /**
   * Sends any request; resolves to the result as the server sent it. Its answer skips the
   * session: JS parses it once, which matters for the editor's frame highlights.
   */
  request(method, params, token) {
    const id = `r${this.nextRaw++}`;
    return new Promise((resolve, reject) => {
      this.raw.set(id, { resolve, reject });
      // Queued edits go first, so the server answers about the current text.
      this.#flush();
      this.transport.send(JSON.stringify({ jsonrpc: '2.0', id, method, params: params ?? null }));
      token?.onCancellationRequested(() => {
        if (!this.raw.delete(id)) return;
        this.notify('$/cancelRequest', { id });
        resolve(null);
      });
    });
  }

  notify(method, params) {
    this.session.notify(method, JSON.stringify(params ?? null));
    this.#flush();
  }

  #receive(text) {
    if (this.raw.size) {
      const m = JSON.parse(text);
      const p = typeof m.id === 'string' && !m.method ? this.raw.get(m.id) : undefined;
      if (p) {
        this.raw.delete(m.id);
        if (m.error) p.reject(new Error(m.error.message ?? 'error'));
        else p.resolve(m.result ?? null);
        return;
      }
    }
    const r = JSON.parse(this.session.receive(text));
    if (r.type === 'response') {
      const p = this.pending.get(r.id);
      this.pending.delete(r.id);
      if (r.error) {
        this.session.resultRaw(r.id);
        p?.reject(new Error(r.error));
      } else if (p) {
        p.resolve(JSON.parse(p.model ? this.session.result(r.id, p.model.core) : this.session.resultRaw(r.id)));
      }
    } else if (r.type === 'diagnostics') {
      this.#diagnostics(r.uri);
    } else if (r.type === 'notification') {
      this.onNotification?.(r.method, r.params);
    } else if (r.type === 'applyEdit') {
      const applied = this.applyWorkspaceEdit(r.edit);
      this.session.respond(JSON.stringify(r.id), JSON.stringify({ applied }));
    }
    this.#flush();
  }

  async initialize(rootUri) {
    this.capabilities = (await this.#wait(this.session.initialize(rootUri), null)) ?? {};
    return this.capabilities;
  }

  /** Opens `model` on the server as `uri` (in `languageId`) and keeps it in sync. */
  attach(model, uri, languageId = this.languageId) {
    const handle = this.session.open(model.core, uri, languageId);
    const doc = { uri, handle, collection: model.createDecorationsCollection() };
    this.docs.set(model, doc);
    this.byUri.set(uri, model);
    doc.off = model.onDidChangeContent(() => this.sync(model));
    this.#flush();
    return () => this.detach(model);
  }

  detach(model) {
    const doc = this.docs.get(model);
    if (!doc) return;
    doc.off?.();
    doc.collection.clear();
    this.session.close(doc.handle);
    this.#flush();
    this.docs.delete(model);
    this.byUri.delete(doc.uri);
  }

  /** Sends the model's edits since the last sync. */
  sync(model) {
    const doc = this.docs.get(model);
    if (!doc) return null;
    this.session.sync(doc.handle, model.core);
    this.#flush();
    return doc;
  }

  async #diagnostics(uri) {
    const model = this.byUri.get(uri);
    const doc = model && this.docs.get(model);
    if (!doc) return;
    doc.diagnostics = JSON.parse(this.session.diagnostics(uri, model.core))
      .map((d) => ({ message: d.message, severity: d.severity, offsets: { start: d.start, end: d.end } }));
    const list = [];
    for (const d of doc.diagnostics) {
      const r = d.offsets.start === d.offsets.end ? { start: d.offsets.start, end: d.offsets.start + 1 } : d.offsets;
      list.push({ start: r.start, end: r.end, type: await diagnosticType(d.severity) });
    }
    doc.collection.set(list);
  }

  /** The char range of an LSP range in `model`. */
  #range(model, lspRange) {
    const [start, end] = this.session.toRange(JSON.stringify(lspRange), model.core);
    return { start, end };
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
      editor.features.applyEdits(edits.map((e) => ({ range: this.#range(model, e.range), text: e.newText })));
    }
    return all;
  }

  /** Locations: ranges in `model` as they are, the rest with their model when attached. */
  #locations(model, list) {
    return (list ?? []).map((l) => {
      if (!('uri' in l)) return l;
      const target = this.byUri.get(l.uri);
      if (target === model) return this.#range(model, l.lspRange);
      return { model: target ?? null, uri: l.uri, lspRange: l.lspRange, range: target ? this.#range(target, l.lspRange) : null };
    });
  }

  #execute(command) {
    if (command) this.request('workspace/executeCommand', { command: command.command, arguments: command.arguments ?? [] });
  }

  /**
   * Registers the editor providers the server's capabilities allow, for `languageId`;
   * `only` (a set of capability names such as `'renameProvider'`) limits them. With
   * `fallback`, each is asked only while no other provider of its kind serves the language
   * (the bundled language modules register this way, so a real language server wins).
   */
  registerProviders(languageId = this.languageId, only = null, fallback = false) {
    const c = this.capabilities;
    const id = languageId;
    const mark = (provider) => (fallback ? Object.assign(provider, { fallback: true }) : provider);
    const reg = (cap, register) => { if (c[cap] && (!only || only.has(cap))) this.disposables.push(register()); };
    const diagnosticsAt = (model, start, end) => (this.docs.get(model)?.diagnostics ?? [])
      .filter((d) => d.offsets.start <= end && start <= d.offsets.end);

    reg('hoverProvider', () => languages.registerHoverProvider(id, mark({
      provideHover: async (model, offset, token) => {
        const result = await this.#ask('hover', model, token, offset);
        const contents = [...diagnosticsAt(model, offset, offset).map((d) => d.message), ...(result?.contents ?? [])];
        return contents.length ? { contents, range: result?.range } : null;
      },
    })));
    reg('completionProvider', () => languages.registerCompletionItemProvider(id, mark({
      triggerCharacters: c.completionProvider.triggerCharacters ?? [],
      provideCompletionItems: async (model, offset, context, token) =>
        (await this.#ask('completion', model, token, offset, 0, context.triggerCharacter)) ?? [],
    })));
    reg('signatureHelpProvider', () => languages.registerSignatureHelpProvider(id, mark({
      signatureHelpTriggerCharacters: c.signatureHelpProvider.triggerCharacters ?? ['(', ','],
      provideSignatureHelp: (model, offset, context, token) => this.#ask('signatureHelp', model, token, offset),
    })));
    reg('definitionProvider', () => languages.registerDefinitionProvider(id, mark({
      provideDefinition: async (model, offset, token) => this.#locations(model, await this.#ask('definition', model, token, offset)),
    })));
    reg('referencesProvider', () => languages.registerReferenceProvider(id, mark({
      provideReferences: async (model, offset, context, token) =>
        this.#locations(model, await this.#ask('references', model, token, offset)).filter((r) => 'start' in r),
    })));
    reg('documentHighlightProvider', () => languages.registerDocumentHighlightProvider(id, mark({
      provideDocumentHighlights: async (model, offset, token) => (await this.#ask('documentHighlight', model, token, offset)) ?? [],
    })));
    reg('renameProvider', () => languages.registerRenameProvider(id, mark({
      provideRenameEdits: async (model, offset, newName, token) => {
        const result = await this.#ask('rename', model, token, offset, 0, newName);
        // Edits to this model come back for the editor; other documents go to the host.
        if (result?.others && Object.keys(result.others).length) this.applyWorkspaceEdit({ changes: result.others });
        return result?.own ?? [];
      },
    })));
    reg('documentFormattingProvider', () => languages.registerDocumentFormattingEditProvider(id, mark({
      provideDocumentFormattingEdits: async (model, options, token) =>
        (await this.#ask('formatting', model, token, options?.tabSize ?? 4, options?.insertSpaces === false ? 0 : 1)) ?? [],
    })));
    reg('codeActionProvider', () => languages.registerCodeActionProvider(id, mark({
      provideCodeActions: async (model, range, token) => {
        const result = await this.#ask('codeAction', model, token, range.start, range.end);
        return (result ?? []).map((a) => ({
          title: a.title,
          edits: undefined,
          command: () => {
            if (a.edit) this.applyWorkspaceEdit(a.edit);
            this.#execute(a.command);
          },
        }));
      },
    })));
    reg('foldingRangeProvider', () => languages.registerFoldingRangeProvider(id, mark({
      provideFoldingRanges: async (model) => (await this.#ask('foldingRange', model)) ?? [],
    })));
    reg('selectionRangeProvider', () => languages.registerSelectionRangeProvider(id, mark({
      provideSelectionRanges: async (model, offset) => (await this.#ask('selectionRange', model, null, offset)) ?? [],
    })));
    reg('documentSymbolProvider', () => languages.registerDocumentSymbolProvider(id, mark({
      provideDocumentSymbols: async (model) => (await this.#ask('documentSymbol', model)) ?? [],
    })));
    reg('codeLensProvider', () => languages.registerCodeLensProvider(id, mark({
      provideCodeLenses: async (model, token) => ((await this.#ask('codeLens', model, token)) ?? []).map((l) => ({
        line: l.line,
        title: l.title,
        command: () => this.#execute(l.command),
      })),
    })));
    reg('inlayHintProvider', () => languages.registerInlayHintsProvider(id, mark({
      provideInlayHints: async (model, range, token) => (await this.#ask('inlayHint', model, token, range.start, range.end)) ?? [],
    })));
    reg('semanticTokensProvider', () => languages.registerDocumentSemanticTokensProvider(id, mark({
      provideSemanticTokens: async (model, token) => (await this.#ask('semanticTokens', model, token)) ?? [],
    })));
  }

  /** `shutdown`, `exit`, and the providers go away. */
  async dispose() {
    for (const d of this.disposables) d.dispose();
    for (const model of [...this.docs.keys()]) this.detach(model);
    try { await this.request('shutdown', null); } catch { /* the server may be gone */ }
    this.notify('exit', null);
  }
}
