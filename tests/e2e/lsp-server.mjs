// A small language server behind a WebSocket, for the browser LSP client's tests. It keeps
// each document's text from didOpen and incremental didChange, reports every `TODO` as a
// warning, and answers hover, completion, definition, references and rename by word.
import { createServer } from 'node:http';
import { createHash } from 'node:crypto';

const port = Number(process.env.LSP_PORT || 47817);
const docs = new Map();

function offsetOf(text, { line, character }) {
  let i = 0;
  for (let l = 0; l < line; l++) {
    const nl = text.indexOf('\n', i);
    if (nl < 0) return text.length;
    i = nl + 1;
  }
  const end = text.indexOf('\n', i);
  return Math.min(i + character, end < 0 ? text.length : end);
}

function positionOf(text, offset) {
  const before = text.slice(0, offset);
  const line = before.split('\n').length - 1;
  return { line, character: offset - (before.lastIndexOf('\n') + 1) };
}

function wordAt(text, offset) {
  let a = offset;
  let b = offset;
  while (a > 0 && /\w/.test(text[a - 1])) a--;
  while (b < text.length && /\w/.test(text[b])) b++;
  return a < b ? { word: text.slice(a, b), start: a, end: b } : null;
}

function occurrences(text, word) {
  return [...text.matchAll(new RegExp(`\\b${word}\\b`, 'g'))].map((m) => ({ start: m.index, end: m.index + word.length }));
}

const range = (text, r) => ({ start: positionOf(text, r.start), end: positionOf(text, r.end) });

function handle(msg, send) {
  const p = msg.params ?? {};
  const doc = p.textDocument && docs.get(p.textDocument.uri);
  const at = doc && p.position ? offsetOf(doc, p.position) : 0;
  const reply = (result) => send({ jsonrpc: '2.0', id: msg.id, result });
  switch (msg.method) {
    case 'initialize':
      return reply({
        capabilities: {
          textDocumentSync: 2,
          hoverProvider: true,
          completionProvider: { triggerCharacters: ['.'] },
          definitionProvider: true,
          referencesProvider: true,
          renameProvider: true,
        },
        serverInfo: { name: 'test-lsp' },
      });
    case 'textDocument/didOpen':
      docs.set(p.textDocument.uri, p.textDocument.text);
      return diagnose(p.textDocument.uri, send);
    case 'textDocument/didChange': {
      let text = docs.get(p.textDocument.uri) ?? '';
      for (const c of p.contentChanges) {
        if (!c.range) { text = c.text; continue; }
        text = text.slice(0, offsetOf(text, c.range.start)) + c.text + text.slice(offsetOf(text, c.range.end));
      }
      docs.set(p.textDocument.uri, text);
      return diagnose(p.textDocument.uri, send);
    }
    case 'textDocument/hover': {
      const w = wordAt(doc, at);
      return reply(w ? { contents: { kind: 'markdown', value: `**${w.word}** has ${w.word.length} letters` }, range: range(doc, w) } : null);
    }
    case 'textDocument/completion': {
      // The words of the document, except the one being typed.
      const typing = wordAt(doc, at)?.word;
      const words = [...new Set(doc.match(/\w+/g) ?? [])].filter((w) => w !== typing);
      return reply({
        isIncomplete: false,
        items: [
          ...words.map((w) => ({ label: w, kind: 6 })),
          { label: 'serverItem', kind: 3, documentation: { kind: 'markdown', value: 'From the server.' }, insertText: 'serverItem($1)', insertTextFormat: 2 },
        ],
      });
    }
    case 'textDocument/definition': {
      const w = wordAt(doc, at);
      const first = w && occurrences(doc, w.word)[0];
      return reply(first ? { uri: p.textDocument.uri, range: range(doc, first) } : null);
    }
    case 'textDocument/references': {
      const w = wordAt(doc, at);
      return reply(w ? occurrences(doc, w.word).map((o) => ({ uri: p.textDocument.uri, range: range(doc, o) })) : []);
    }
    case 'textDocument/rename': {
      const w = wordAt(doc, at);
      const edits = w ? occurrences(doc, w.word).map((o) => ({ range: range(doc, o), newText: p.newName })) : [];
      return reply({ changes: { [p.textDocument.uri]: edits } });
    }
    case 'shutdown':
      return reply(null);
    default:
      if (msg.id !== undefined) reply(null);
  }
}

function diagnose(uri, send) {
  const text = docs.get(uri);
  const diagnostics = occurrences(text, 'TODO').map((o) => ({ range: range(text, o), severity: 2, message: 'Unresolved TODO' }));
  send({ jsonrpc: '2.0', method: 'textDocument/publishDiagnostics', params: { uri, diagnostics } });
}

/** A server frame holding `obj` as JSON text. */
function frame(obj) {
  const payload = Buffer.from(JSON.stringify(obj));
  const n = payload.length;
  let head;
  if (n < 126) {
    head = Buffer.from([0x81, n]);
  } else if (n < 65536) {
    head = Buffer.from([0x81, 126, n >> 8, n & 255]);
  } else {
    head = Buffer.alloc(10);
    head[0] = 0x81;
    head[1] = 127;
    head.writeBigUInt64BE(BigInt(n), 2);
  }
  return Buffer.concat([head, payload]);
}

// WebSocket: the upgrade handshake and text frames, enough for one JSON message per frame.
const server = createServer((req, res) => res.writeHead(426).end());
server.on('upgrade', (req, socket) => {
  const key = req.headers['sec-websocket-key'];
  const accept = createHash('sha1').update(`${key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11`).digest('base64');
  socket.write(`HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${accept}\r\n\r\n`);
  const send = (obj) => socket.write(frame(obj));
  let buffer = Buffer.alloc(0);
  socket.on('data', (chunk) => {
    buffer = Buffer.concat([buffer, chunk]);
    for (;;) {
      if (buffer.length < 2) return;
      const opcode = buffer[0] & 15;
      let len = buffer[1] & 127;
      let offset = 2;
      if (len === 126) {
        if (buffer.length < 4) return;
        len = buffer.readUInt16BE(2);
        offset = 4;
      } else if (len === 127) {
        if (buffer.length < 10) return;
        len = Number(buffer.readBigUInt64BE(2));
        offset = 10;
      }
      const masked = buffer[1] & 128;
      const total = offset + (masked ? 4 : 0) + len;
      if (buffer.length < total) return;
      const mask = masked ? buffer.subarray(offset, offset + 4) : null;
      const data = Buffer.from(buffer.subarray(offset + (masked ? 4 : 0), total));
      if (mask) for (let i = 0; i < data.length; i++) data[i] ^= mask[i % 4];
      buffer = buffer.subarray(total);
      if (opcode === 8) { socket.end(); return; }
      if (opcode === 1) handle(JSON.parse(data.toString('utf8')), send);
    }
  });
  socket.on('error', () => {});
});
server.listen(port, '127.0.0.1');
