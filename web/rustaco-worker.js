// Work off the main thread: the language servers of the language modules, and search and
// diff on text replicas.
//
// Each language module (pack) runs as one language server here. The page talks to it in
// JSON-RPC (`{ type: 'lsp', pack, messages }` both ways): documents open and change with
// `didOpen` and `didChange`, a frame's highlights come from `rustaco/highlight`, the rest
// through the standard requests. Between messages the servers parse in slices, so a request
// never waits for a long parse; until a parse finishes, highlights come from the previous
// tree shifted by the edits, and `rustaco/parsed` tells the page when it caught up.
//
// Search and diff work on text replicas that follow the page's edits (`openText`, `edit`);
// every reply carries the model version it was computed for.

const SLICE_MS = 20;

/** Language servers by pack: a promise of the server, or null when the module is missing. */
const servers = new Map();
/** Servers that loaded, for the parse loop. */
const live = new Map();

function loadServer(pack) {
  if (!servers.has(pack)) {
    const url = new URL(`./pkg/syntax-${pack}/rustaco_syntax_wasm.js`, import.meta.url);
    servers.set(pack, import(url.href).then(async (m) => {
      await m.default();
      const server = new m.LanguageServer();
      live.set(pack, server);
      return server;
    }).catch(() => null));
  }
  return servers.get(pack);
}

let editorModule = null;
/** The editor module, for its text replicas (search). */
function loadEditor() {
  editorModule ??= import(new URL('./pkg/rustaco_web.js', import.meta.url).href).then(async (m) => { await m.default(); return m; });
  return editorModule;
}

/** Text replicas by model id, with the model version each is at. */
const docs = new Map();
const queue = [];
let pumping = false;
let parseTimer = 0;

function doc(id) {
  if (!docs.has(id)) docs.set(id, { text: null, version: 0 });
  return docs.get(id);
}

self.onmessage = (e) => {
  queue.push(e.data);
  pump();
};

async function pump() {
  if (pumping) return;
  pumping = true;
  while (queue.length) {
    const msg = queue.shift();
    try {
      await handle(msg);
    } catch (err) {
      if (msg.req !== undefined) self.postMessage({ type: 'reply', req: msg.req, error: String(err) });
    }
  }
  pumping = false;
  scheduleParse();
}

/** An error answer for each request among `messages`. */
function failed(messages, reason) {
  const out = [];
  for (const text of messages) {
    let m = null;
    try { m = JSON.parse(text); } catch { /* not JSON: nothing to answer */ }
    if (m && m.id !== undefined && m.method) {
      out.push(JSON.stringify({ jsonrpc: '2.0', id: m.id, error: { code: -32603, message: reason } }));
    }
  }
  return out;
}

async function handle(msg) {
  if (msg.type === 'lsp') {
    const server = await loadServer(msg.pack);
    let out = [];
    if (!server) {
      out = failed(msg.messages, `no language module for ${msg.pack}`);
    } else {
      for (const text of msg.messages) {
        try {
          out.push(...server.handle(text));
        } catch (err) {
          out.push(...failed([text], String(err)));
        }
      }
    }
    if (out.length) self.postMessage({ type: 'lsp', pack: msg.pack, messages: out });
    return;
  }
  const d = doc(msg.id);
  switch (msg.type) {
    case 'openText': {
      const m = await loadEditor();
      d.text?.free();
      d.text = new m.TextReplica(msg.text);
      d.version = msg.version;
      break;
    }
    case 'close':
      d.text?.free();
      docs.delete(msg.id);
      break;
    case 'edit':
      try {
        if (msg.reset !== undefined) d.text?.reset(msg.reset);
        else d.text?.edit(msg.numbers, msg.text);
      } catch {
        // Out of step with the page: ask for the whole text.
        self.postMessage({ type: 'resync', id: msg.id });
      }
      d.version = msg.version;
      break;
    case 'diff': {
      // The original's text against the modified model's, both kept here.
      const other = doc(msg.other);
      const result = d.text && other.text ? d.text.diff(other.text, msg.ignoreWhitespace, msg.budget) : null;
      self.postMessage({ type: 'reply', req: msg.req, version: d.version, otherVersion: other.version, result });
      break;
    }
    case 'find': {
      const result = d.text ? d.text.findAll(msg.pattern, msg.regex, msg.caseSensitive, msg.wholeWord) : [];
      self.postMessage({ type: 'reply', req: msg.req, version: d.version, result: result ?? null });
      break;
    }
    default:
      break;
  }
}

/** Parses pending documents a slice at a time, yielding to messages in between. */
function scheduleParse() {
  if (parseTimer || ![...live.values()].some((s) => s.busy())) return;
  parseTimer = setTimeout(() => {
    parseTimer = 0;
    if (pumping) return;
    for (const [pack, server] of live) {
      if (!server.busy()) continue;
      const messages = server.idle(SLICE_MS);
      if (messages.length) self.postMessage({ type: 'lsp', pack, messages });
    }
    scheduleParse();
  }, 0);
}
