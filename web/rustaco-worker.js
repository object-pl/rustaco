// Work off the main thread: parsing, highlighting, bracket and fold queries, search.
//
// The page sends each model's edits as they happen and asks for what a frame needs; every
// reply carries the model version it was computed for, so the page can drop stale ones.
// Parsing runs in slices between messages, so a request never waits for a long parse:
// until the parse finishes, highlights come from the previous tree shifted by the edits.

const SLICE_MS = 20;

const packs = new Map();
/** Loads the language module for `pack` once; resolves to the module or null. */
function loadPack(pack) {
  if (!packs.has(pack)) {
    const url = new URL(`./pkg/syntax-${pack}/rustaco_syntax_wasm.js`, import.meta.url);
    packs.set(pack, import(url.href).then(async (m) => { await m.default(); return m; }).catch(() => null));
  }
  return packs.get(pack);
}

let editorModule = null;
/** The editor module, for its text replicas (search). */
function loadEditor() {
  editorModule ??= import(new URL('./pkg/rustaco_web.js', import.meta.url).href).then(async (m) => { await m.default(); return m; });
  return editorModule;
}

/**
 * Models by id: the language module (`h`) and a text replica for search (`text`), each
 * present once asked for, the model version both are at, and whether `h` has parsed it.
 */
const docs = new Map();
const queue = [];
let pumping = false;
let parseTimer = 0;

function doc(id) {
  if (!docs.has(id)) docs.set(id, { h: null, text: null, version: 0, parsed: true });
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

async function handle(msg) {
  const d = doc(msg.id);
  switch (msg.type) {
    case 'open': {
      d.h?.free();
      d.h = null;
      const module = await loadPack(msg.pack);
      let ok = false;
      if (module) {
        try {
          d.h = new module.Highlighter(msg.language, msg.text);
          d.version = msg.version;
          d.parsed = false;
          ok = true;
        } catch { ok = false; }
      }
      self.postMessage({ type: 'reply', req: msg.req, ok, hasTree: ok && d.h.hasTree() });
      break;
    }
    case 'closeSyntax':
      d.h?.free();
      d.h = null;
      d.parsed = true;
      break;
    case 'openText': {
      const m = await loadEditor();
      d.text?.free();
      d.text = new m.TextReplica(msg.text);
      d.version = msg.version;
      break;
    }
    case 'close':
      d.h?.free();
      d.text?.free();
      docs.delete(msg.id);
      break;
    case 'edit':
      try {
        if (msg.reset !== undefined) {
          d.h?.reset(msg.reset);
          d.text?.reset(msg.reset);
        } else {
          d.h?.edit(msg.numbers, msg.text);
          d.text?.edit(msg.numbers, msg.text);
        }
      } catch {
        // Out of step with the page: ask for the whole text.
        self.postMessage({ type: 'resync', id: msg.id });
      }
      d.version = msg.version;
      if (d.h) d.parsed = false;
      break;
    case 'frame': {
      if (!d.h) break;
      // A short parse first, so small edits show with their final colours at once.
      if (!d.parsed) d.parsed = d.h.parse(4);
      const segments = [];
      for (let i = 0; i < msg.runs.length; i += 2) {
        segments.push([msg.runs[i], msg.runs[i + 1], d.h.highlight(msg.runs[i], msg.runs[i + 1], msg.colorize)]);
      }
      const bracket = d.h.matchingBracket(msg.caret);
      self.postMessage({ type: 'frame', id: msg.id, editor: msg.editor, version: d.version, segments, bracket, parsed: d.parsed });
      break;
    }
    case 'enclosing':
    case 'folding':
    case 'symbols': {
      if (!d.h) { self.postMessage({ type: 'reply', req: msg.req, version: d.version, result: [] }); break; }
      // These need the tree for the current text: finish the parse.
      while (!d.parsed) d.parsed = d.h.parse(1000);
      const result = msg.type === 'enclosing' ? d.h.enclosingRanges(msg.start, msg.end)
        : msg.type === 'folding' ? d.h.foldingRanges() : d.h.documentSymbols();
      self.postMessage({ type: 'reply', req: msg.req, version: d.version, result });
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

/** Parses unparsed models a slice at a time, yielding to messages in between. */
function scheduleParse() {
  if (parseTimer || ![...docs.values()].some((d) => d.h && !d.parsed)) return;
  parseTimer = setTimeout(() => {
    parseTimer = 0;
    if (pumping) return;
    for (const [id, d] of docs) {
      if (!d.h || d.parsed) continue;
      d.parsed = d.h.parse(SLICE_MS);
      if (d.parsed) self.postMessage({ type: 'parsed', id, version: d.version });
    }
    scheduleParse();
  }, 0);
}
