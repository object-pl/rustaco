//! A small LanguageService served by LspServer, driven by the crate's own client.

use std::io::{Read, Write};
use std::ops::Range;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use rustaco_core::{Document, Edit, EditEvent};
use rustaco_lsp::{
    CompletionItem, Connection, Context, Diagnostic, Features, Hover, LanguageService, Location, LspClient, LspServer,
    SemanticToken, ServerMessage, Severity, Symbol, TextEdit, WorkspaceEdit, serve,
};
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

/// Words are runs of letters, digits and `_`; `fn NAME` declares NAME; `TODO` is a warning.
#[derive(Default)]
struct Words {
    /// Edit events received by `change`, to check that edits arrive incrementally.
    events: usize,
}

fn words(text: &str) -> Vec<(Range<usize>, String)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in text.chars().chain(std::iter::once(' ')).enumerate() {
        match (c.is_alphanumeric() || c == '_', start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push((s..i, text.chars().skip(s).take(i - s).collect()));
                start = None;
            }
            _ => {}
        }
    }
    out
}

fn word_at(cx: &Context, at: usize) -> Option<(Range<usize>, String)> {
    words(&cx.model.text()).into_iter().find(|(r, _)| r.start <= at && at <= r.end)
}

fn occurrences(cx: &Context, at: usize) -> Vec<Range<usize>> {
    let Some((_, w)) = word_at(cx, at) else { return Vec::new() };
    words(&cx.model.text()).into_iter().filter(|(_, x)| *x == w).map(|(r, _)| r).collect()
}

impl LanguageService for Words {
    fn features(&self) -> Features {
        let mut f = Features::new("words");
        f.completion = Some(vec!['.']);
        f.hover = true;
        f.definition = true;
        f.references = true;
        f.rename = true;
        f.document_symbols = true;
        f.diagnostics = true;
        f.semantic_tokens = Some((vec!["keyword".into(), "function".into()], vec!["declaration".into()]));
        f
    }

    fn change(&mut self, _: &Context, edits: &[EditEvent]) {
        self.events += edits.len();
    }

    fn diagnostics(&mut self, cx: &Context) -> Vec<Diagnostic> {
        words(&cx.model.text())
            .into_iter()
            .filter(|(_, w)| w == "TODO")
            .map(|(range, _)| {
                let mut d = Diagnostic::new(range, Severity::Warning, "unfinished");
                d.source = Some("words".into());
                d
            })
            .collect()
    }

    fn completion(&mut self, cx: &Context, _: usize, _: Option<char>) -> Vec<CompletionItem> {
        let mut list: Vec<String> = words(&cx.model.text()).into_iter().map(|(_, w)| w).collect();
        list.sort();
        list.dedup();
        list.into_iter()
            .map(|w| {
                let mut item = CompletionItem::new(w);
                item.kind = Some(1);
                item
            })
            .collect()
    }

    fn hover(&mut self, cx: &Context, at: usize) -> Option<Hover> {
        let (range, w) = word_at(cx, at)?;
        let mut hover = Hover::new(vec![format!("**{w}**")]);
        hover.range = Some(range);
        Some(hover)
    }

    fn definition(&mut self, cx: &Context, at: usize) -> Vec<Location> {
        occurrences(cx, at).into_iter().take(1).map(|r| Location::local(cx.uri, r)).collect()
    }

    fn references(&mut self, cx: &Context, at: usize) -> Vec<Location> {
        occurrences(cx, at).into_iter().map(|r| Location::local(cx.uri, r)).collect()
    }

    fn rename(&mut self, cx: &Context, at: usize, new_name: &str) -> Option<WorkspaceEdit> {
        Some(WorkspaceEdit::local(
            occurrences(cx, at).into_iter().map(|range| TextEdit::new(range, new_name)).collect(),
        ))
    }

    fn semantic_tokens(&mut self, cx: &Context) -> Vec<SemanticToken> {
        let list = words(&cx.model.text());
        let mut out = Vec::new();
        for pair in list.windows(2) {
            if pair[0].1 == "fn" {
                out.push(SemanticToken::new(pair[0].0.clone(), "keyword"));
                let mut name = SemanticToken::new(pair[1].0.clone(), "function");
                name.modifiers = vec!["declaration".into()];
                out.push(name);
            }
        }
        out
    }

    fn document_symbols(&mut self, cx: &Context) -> Vec<Symbol> {
        let list = words(&cx.model.text());
        list.windows(2)
            .filter(|p| p[0].1 == "fn")
            .map(|p| Symbol::new(p[1].1.clone(), 12, p[0].0.start..p[1].0.end, p[1].0.clone()))
            .collect()
    }
}

fn start() -> LspClient {
    let (to_server, server_in) = pipe();
    let (server_out, from_server) = pipe();
    std::thread::spawn(move || serve(Words::default(), server_in, server_out).unwrap());
    let mut client = LspClient::new(Connection::new(from_server, to_server));
    client.initialize(Some("file:///")).unwrap();
    client
}

fn diagnostics(client: &LspClient) -> Value {
    loop {
        match client.connection().messages().recv_timeout(Duration::from_secs(5)).unwrap() {
            ServerMessage::Notification { method, params } if method == "textDocument/publishDiagnostics" => {
                return params;
            }
            _ => {}
        }
    }
}

const TEXT: &str = "fn alpha() {}\n// 😀 TODO ąę\nfn beta() { alpha(); }\n";

#[test]
fn client_and_server_agree() {
    let client = start();
    assert_eq!(client.server_info()["name"], "words");
    assert!(client.supports("hoverProvider") && client.supports("renameProvider"));
    assert!(!client.supports("documentFormattingProvider"));
    assert_eq!(client.encoding().name(), "utf-8");

    let mut doc = Document::new(TEXT);
    let mut open = client.open("file:///a.txt", "plaintext", doc.model()).unwrap();
    let d = diagnostics(&client);
    assert_eq!(d["diagnostics"].as_array().unwrap().len(), 1);
    assert_eq!(d["diagnostics"][0]["severity"], 2);

    // The second `alpha`, after the emoji line.
    let second = TEXT.chars().count() - "alpha(); }\n".chars().count() + 1;
    let hover = client.hover(&mut open, doc.model(), second).unwrap().unwrap();
    assert_eq!(hover.contents, ["**alpha**"]);
    assert_eq!(hover.range, Some(second - 1..second + 4));
    let def = client.definition(&mut open, doc.model(), second).unwrap();
    assert_eq!(def[0].range, Some(3..8));
    assert_eq!(client.references(&mut open, doc.model(), second).unwrap().len(), 2);

    // Edits reach the server incrementally and answers follow them.
    doc.editor().apply_edits(vec![Edit::insert(0, "fn gamma() {}\n")]);
    doc.editor().apply_edits(vec![Edit::delete(0, 3), Edit::insert(6, "x")]);
    let items = client.completion(&mut open, doc.model(), 0, None).unwrap();
    assert!(items.iter().any(|i| i.label == "gamxma"), "{:?}", items.iter().map(|i| &i.label).collect::<Vec<_>>());
    let moved = second + "fn gamma() {}\n".len() - 3 + 1;
    assert_eq!(client.definition(&mut open, doc.model(), moved).unwrap()[0].range, Some(15..20));

    let edit = client.rename(&mut open, doc.model(), moved, "omega").unwrap();
    assert_eq!(edit.local.len(), 2);
    assert!(edit.local.iter().all(|e| e.text == "omega"));

    let symbols = client.document_symbols(&mut open, doc.model()).unwrap();
    assert_eq!(symbols.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["alpha", "beta"]);
    let tokens = client.semantic_tokens(&mut open, doc.model()).unwrap();
    let text = doc.text();
    let named: Vec<(String, String)> = tokens
        .iter()
        .map(|t| (text.chars().skip(t.range.start).take(t.range.len()).collect(), t.token_type.clone()))
        .collect();
    assert_eq!(named[0], ("fn".into(), "keyword".into()));
    assert_eq!(named[1], ("alpha".into(), "function".into()));
    assert_eq!(tokens[1].modifiers, ["declaration"]);
    assert_eq!(named.len(), 4);
    client.close(open).unwrap();
    client.shutdown();
}

fn request(id: u64, method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
}

#[test]
fn utf16_positions_and_cancellation() {
    let mut server = LspServer::new(Words::default());
    // No encodings offered: UTF-16.
    let out = server.handle(request(1, "initialize", json!({ "capabilities": {} })));
    assert_eq!(out[0]["result"]["capabilities"]["positionEncoding"], "utf-16");
    server.handle(json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
        "textDocument": { "uri": "u", "languageId": "x", "version": 1, "text": "😀 fn ab" } } }));
    // The emoji is two UTF-16 units: `ab` starts at character 6.
    let out = server.handle(request(
        2,
        "textDocument/hover",
        json!({
        "textDocument": { "uri": "u" }, "position": { "line": 0, "character": 7 } }),
    ));
    assert_eq!(
        out[0]["result"]["range"],
        json!({ "start": { "line": 0, "character": 6 }, "end": { "line": 0, "character": 8 } })
    );
    let out = server.handle(request(3, "textDocument/semanticTokens/full", json!({ "textDocument": { "uri": "u" } })));
    assert_eq!(out[0]["result"]["data"], json!([0, 3, 2, 0, 0, 0, 3, 2, 1, 1]));

    // A change splitting nothing, then a cancelled request still in the queue.
    server.receive(json!({ "jsonrpc": "2.0", "method": "textDocument/didChange", "params": {
        "textDocument": { "uri": "u", "version": 2 },
        "contentChanges": [{ "range": { "start": { "line": 0, "character": 8 }, "end": { "line": 0, "character": 8 } }, "text": "c" }] } }));
    server.receive(request(
        4,
        "textDocument/hover",
        json!({ "textDocument": { "uri": "u" }, "position": { "line": 0, "character": 7 } }),
    ));
    server.receive(json!({ "jsonrpc": "2.0", "method": "$/cancelRequest", "params": { "id": 4 } }));
    server.receive(request(
        5,
        "textDocument/hover",
        json!({ "textDocument": { "uri": "u" }, "position": { "line": 0, "character": 7 } }),
    ));
    let out = server.process();
    let answer = |id: u64| out.iter().find(|m| m["id"] == id).unwrap().clone();
    assert_eq!(answer(4)["error"]["code"], -32800);
    assert_eq!(answer(5)["result"]["contents"]["value"], "**abc**");
    assert_eq!(server.service().events, 1);
    assert_eq!(server.document("u").unwrap().text(), "😀 fn abc");

    let out =
        server.handle(request(6, "textDocument/formatting", json!({ "textDocument": { "uri": "u" }, "options": {} })));
    assert_eq!(out[0]["result"], json!([]));
    let out = server.handle(request(
        7,
        "textDocument/hover",
        json!({ "textDocument": { "uri": "nope" }, "position": { "line": 0, "character": 0 } }),
    ));
    assert_eq!(out[0]["error"]["code"], -32602);
    let out = server.handle(request(8, "custom/thing", json!({ "textDocument": { "uri": "u" } })));
    assert_eq!(out[0]["error"]["code"], -32601);
}
