//! The client against a scripted server on in-memory pipes.

use std::io::{BufReader, Read, Write};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use rustaco_core::{Document, Selection, Selections};
use rustaco_lsp::{Connection, LspClient, read_message, write_message};
use serde_json::{Value, json};

struct Pipe {
    rx: Receiver<Vec<u8>>,
    buf: Vec<u8>,
    pos: usize,
}

impl Read for Pipe {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.pos == self.buf.len() {
            match self.rx.recv() {
                Ok(b) => {
                    self.buf = b;
                    self.pos = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let n = out.len().min(self.buf.len() - self.pos);
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

struct Sink(Sender<Vec<u8>>);

impl Write for Sink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        let _ = self.0.send(b.to_vec());
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn pipe() -> (Sink, Pipe) {
    let (tx, rx) = channel();
    (Sink(tx), Pipe { rx, buf: Vec::new(), pos: 0 })
}

/// The server's copy of the document, kept from didOpen and didChange (UTF-16 positions).
fn apply_change(text: &mut String, change: &Value) {
    let Some(range) = change.get("range") else {
        *text = change["text"].as_str().unwrap().to_owned();
        return;
    };
    let offset = |p: &Value| -> usize {
        let (line, ch) = (p["line"].as_u64().unwrap() as usize, p["character"].as_u64().unwrap() as usize);
        let mut byte = 0;
        for (i, l) in text.split_inclusive('\n').enumerate() {
            if i == line {
                let mut units = 0;
                for (b, c) in l.char_indices() {
                    if units >= ch {
                        return byte + b;
                    }
                    units += c.len_utf16();
                }
                return byte + l.trim_end_matches('\n').len();
            }
            byte += l.len();
        }
        byte
    };
    let (a, b) = (offset(&range["start"]), offset(&range["end"]));
    text.replace_range(a..b, change["text"].as_str().unwrap());
}

/// A server that answers a few requests and keeps the document text.
fn start_server() -> (LspClient, Arc<Mutex<String>>) {
    let (to_server, server_in) = pipe();
    let (server_out, from_server) = pipe();
    let text = Arc::new(Mutex::new(String::new()));
    let server_text = text.clone();
    std::thread::spawn(move || {
        let mut input = BufReader::new(server_in);
        let mut out = server_out;
        while let Ok(Some(msg)) = read_message(&mut input) {
            let method = msg["method"].as_str().unwrap_or("").to_owned();
            let params = &msg["params"];
            let result = match method.as_str() {
                "initialize" => json!({ "capabilities": {
                    "textDocumentSync": 2,
                    "hoverProvider": true,
                    "completionProvider": { "triggerCharacters": ["."] },
                    "definitionProvider": true,
                    "renameProvider": true,
                    "semanticTokensProvider": { "legend": { "tokenTypes": ["function", "variable"], "tokenModifiers": ["declaration"] }, "full": true },
                }}),
                "textDocument/didOpen" => {
                    *server_text.lock().unwrap() = params["textDocument"]["text"].as_str().unwrap().to_owned();
                    continue;
                }
                "textDocument/didChange" => {
                    let mut t = server_text.lock().unwrap();
                    for c in params["contentChanges"].as_array().unwrap() {
                        apply_change(&mut t, c);
                    }
                    continue;
                }
                "textDocument/hover" => json!({
                    "contents": { "kind": "markdown", "value": format!("at {}:{}", params["position"]["line"], params["position"]["character"]) },
                    "range": { "start": params["position"], "end": { "line": params["position"]["line"], "character": params["position"]["character"].as_u64().unwrap() + 1 } },
                }),
                "textDocument/completion" => json!({ "isIncomplete": false, "items": [
                    { "label": "len", "kind": 2, "insertTextFormat": 2, "insertText": "len($1)" },
                    { "label": "push", "textEdit": { "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } }, "newText": "push" } },
                ]}),
                "textDocument/definition" => {
                    json!([{ "uri": "file:///other.rs", "range": { "start": { "line": 3, "character": 0 }, "end": { "line": 3, "character": 2 } } },
                                                    { "uri": params["textDocument"]["uri"], "range": { "start": { "line": 0, "character": 1 }, "end": { "line": 0, "character": 3 } } }])
                }
                "textDocument/rename" => json!({ "changes": {
                    params["textDocument"]["uri"].as_str().unwrap(): [{ "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } }, "newText": params["newName"] }],
                    "file:///other.rs": [{ "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } }, "newText": params["newName"] }],
                }}),
                // Two tokens on line 0 (UTF-16 0..3 function, 3..4 variable), one on line 1.
                "textDocument/semanticTokens/full" => json!({ "data": [0, 0, 3, 0, 1, 0, 3, 1, 1, 0, 1, 1, 1, 1, 0] }),
                "shutdown" => Value::Null,
                "initialized" | "exit" => continue,
                _ => Value::Null,
            };
            if let Some(id) = msg.get("id") {
                write_message(&mut out, &json!({ "jsonrpc": "2.0", "id": id, "result": result })).unwrap();
            }
        }
    });
    let mut client = LspClient::new(Connection::new(from_server, to_server));
    client.initialize(Some("file:///")).unwrap();
    (client, text)
}

#[test]
fn incremental_sync_keeps_the_server_text_equal() {
    let (client, server_text) = start_server();
    let mut doc = Document::new("fn main() {\n    let s = \"\u{1F600}\";\n}\n");
    let mut lsp_doc = client.open("file:///a.rs", "rust", doc.model()).unwrap();
    let edits: [(usize, &str); 4] = [(23, "x"), (0, "// top\n"), (30, "\u{e9}\u{1F600}"), (5, "\n\n")];
    for (at, text) in edits {
        doc.set_selections(Selections::single(Selection::cursor(at)));
        doc.insert(text);
        client.sync(&mut lsp_doc, doc.model()).unwrap();
    }
    doc.set_selections(Selections::single(Selection::new(2, 12)));
    doc.insert("");
    client.sync(&mut lsp_doc, doc.model()).unwrap();
    // A request after the notifications sees them applied (messages are ordered).
    client.hover(&mut lsp_doc, doc.model(), 0).unwrap();
    assert_eq!(*server_text.lock().unwrap(), doc.text());
}

#[test]
fn answers_map_to_char_offsets() {
    let (client, _) = start_server();
    let doc = Document::new("a\u{1F600}bc\nxyz");
    let mut lsp_doc = client.open("file:///a.rs", "rust", doc.model()).unwrap();
    // Offset 2 is after the emoji: UTF-16 character 3.
    let hover = client.hover(&mut lsp_doc, doc.model(), 2).unwrap().unwrap();
    assert_eq!(hover.contents, vec!["at 0:3".to_owned()]);
    assert_eq!(hover.range, Some(2..3));

    let items = client.completion(&mut lsp_doc, doc.model(), 1, None).unwrap();
    assert!(items[0].is_snippet && items[0].insert_text == "len($1)");
    assert_eq!(items[1].range, Some(0..1));

    let defs = client.definition(&mut lsp_doc, doc.model(), 0).unwrap();
    assert_eq!(defs[0].range, None, "another document keeps its LSP range for the host");
    assert_eq!(defs[1].range, Some(1..2), "UTF-16 1..3 is the emoji");

    let edit = client.rename(&mut lsp_doc, doc.model(), 0, "z").unwrap();
    assert_eq!(edit.local.len(), 1);
    assert_eq!(edit.other[0].0, "file:///other.rs");

    let tokens = client.semantic_tokens(&mut lsp_doc, doc.model()).unwrap();
    let got: Vec<(std::ops::Range<usize>, &str)> =
        tokens.iter().map(|t| (t.range.clone(), t.token_type.as_str())).collect();
    assert_eq!(got, vec![(0..2, "function"), (2..3, "variable"), (6..7, "variable")]);
    assert_eq!(tokens[0].modifiers, vec!["declaration".to_owned()]);
    client.shutdown();
}
