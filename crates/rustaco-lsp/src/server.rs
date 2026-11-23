//! [`LspServer`]: a [`LanguageService`] behind the Language Server Protocol.
//!
//! The server does no I/O itself: messages go in with [`LspServer::receive`] and come out of
//! [`LspServer::process`], so the same code serves a browser worker (strings through
//! `postMessage`) and a program on stdio ([`serve`], [`serve_stdio`]). It keeps the open
//! documents as [`TextModel`]s, applies `didChange` to them, and hands the service char
//! offsets and edit events.

use std::collections::{HashMap, VecDeque};
use std::io::{self, BufReader, Read, Write};
use std::ops::Range;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};

use rustaco_core::{ChangeSet, Edit, EditEvent, Rope, TextModel, line_end};
use serde_json::{Map, Value, json};

use crate::convert::{PositionEncoding, char_range, offset, position, range};
use crate::rpc::{read_message, write_message};
use crate::service::{Cancel, Context, Diagnostic, Features, LanguageService};
use crate::{CompletionItem, Location, Symbol, TextEdit, WorkspaceEdit};

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const NOT_INITIALIZED: i64 = -32002;
const REQUEST_CANCELLED: i64 = -32800;

struct Open {
    language: String,
    model: TextModel,
    version: Value,
}

/// The request being computed and its cancel flag, shared with a reader thread.
type InFlight = Arc<Mutex<Option<(Value, Cancel)>>>;

/// A language server around `service`.
pub struct LspServer<S> {
    service: S,
    features: Features,
    docs: HashMap<String, Open>,
    encoding: PositionEncoding,
    queue: VecDeque<Value>,
    out: Vec<Value>,
    in_flight: InFlight,
    initialized: bool,
    shut_down: bool,
    exited: bool,
}

impl<S: LanguageService> LspServer<S> {
    pub fn new(service: S) -> Self {
        Self {
            features: service.features(),
            service,
            docs: HashMap::new(),
            encoding: PositionEncoding::Utf16,
            queue: VecDeque::new(),
            out: Vec::new(),
            in_flight: InFlight::default(),
            initialized: false,
            shut_down: false,
            exited: false,
        }
    }

    pub fn service(&self) -> &S {
        &self.service
    }

    pub fn service_mut(&mut self) -> &mut S {
        &mut self.service
    }

    /// The text of an open document.
    pub fn document(&self, uri: &str) -> Option<&TextModel> {
        self.docs.get(uri).map(|d| &d.model)
    }

    /// Calls `f` with the service and each open document, for work between messages (a
    /// parse continued in slices).
    pub fn each_document(&mut self, mut f: impl FnMut(&mut S, &Context)) {
        let cancel = Cancel::default();
        for (uri, open) in &self.docs {
            let cx = Context { uri, language: &open.language, model: &open.model, cancel: &cancel };
            f(&mut self.service, &cx);
        }
    }

    /// True after the client sent `exit`.
    pub fn exited(&self) -> bool {
        self.exited
    }

    /// Queues a message. `$/cancelRequest` takes effect at once: a queued request is dropped
    /// and answered as cancelled, the one being computed gets its [`Cancel`] set.
    pub fn receive(&mut self, message: Value) {
        if message["method"] == "$/cancelRequest" {
            let id = &message["params"]["id"];
            if let Some(i) = self.queue.iter().position(|m| m.get("id") == Some(id) && m.get("method").is_some()) {
                self.queue.remove(i);
                self.error(id.clone(), REQUEST_CANCELLED, "cancelled");
            } else if let Ok(guard) = self.in_flight.lock()
                && let Some((current, cancel)) = &*guard
                && current == id
            {
                cancel.cancel();
            }
            return;
        }
        self.queue.push_back(message);
    }

    /// Handles the oldest queued message; false when the queue is empty.
    pub fn process_one(&mut self) -> bool {
        let Some(message) = self.queue.pop_front() else { return false };
        self.dispatch(message);
        true
    }

    /// Handles every queued message and returns what to send to the client.
    pub fn process(&mut self) -> Vec<Value> {
        while self.process_one() {}
        self.take_output()
    }

    /// Messages for the client produced so far.
    pub fn take_output(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.out)
    }

    /// [`LspServer::receive`] then [`LspServer::process`].
    pub fn handle(&mut self, message: Value) -> Vec<Value> {
        self.receive(message);
        self.process()
    }

    /// [`LspServer::handle`] on JSON text, for transports that carry strings.
    pub fn handle_json(&mut self, message: &str) -> Vec<String> {
        match serde_json::from_str(message) {
            Ok(m) => self.receive(m),
            Err(e) => self.error(Value::Null, PARSE_ERROR, &e.to_string()),
        }
        self.process().iter().map(Value::to_string).collect()
    }

    fn error(&mut self, id: Value, code: i64, message: &str) {
        self.out.push(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }));
    }

    fn dispatch(&mut self, message: Value) {
        let Some(method) = message.get("method").and_then(Value::as_str).map(str::to_owned) else { return };
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let Some(id) = message.get("id").cloned() else {
            self.notification(&method, &params);
            return;
        };
        if method != "initialize" && !self.initialized {
            return self.error(id, NOT_INITIALIZED, "initialize first");
        }
        if self.shut_down {
            return self.error(id, INVALID_REQUEST, "the server is shut down");
        }
        let cancel = Cancel::default();
        if let Ok(mut f) = self.in_flight.lock() {
            *f = Some((id.clone(), cancel.clone()));
        }
        let answer = self.request(&method, &params, &cancel);
        if let Ok(mut f) = self.in_flight.lock() {
            *f = None;
        }
        match answer {
            _ if cancel.is_cancelled() => self.error(id, REQUEST_CANCELLED, "cancelled"),
            Ok(result) => self.out.push(json!({ "jsonrpc": "2.0", "id": id, "result": result })),
            Err((code, text)) => self.error(id, code, &text),
        }
    }

    fn notification(&mut self, method: &str, params: &Value) {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or("").to_owned();
        match method {
            "exit" => self.exited = true,
            "textDocument/didOpen" => {
                let doc = &params["textDocument"];
                let open = Open {
                    language: doc["languageId"].as_str().unwrap_or("").to_owned(),
                    model: TextModel::new(doc["text"].as_str().unwrap_or("")),
                    version: doc["version"].clone(),
                };
                self.docs.insert(uri.clone(), open);
                self.with_doc(&uri, |service, cx| service.open(cx));
                self.publish(&uri);
            }
            "textDocument/didChange" => {
                let encoding = self.encoding;
                let Some(open) = self.docs.get_mut(&uri) else { return };
                let before = open.model.version();
                for change in params["contentChanges"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
                    let text = change["text"].as_str().unwrap_or("");
                    match change.get("range") {
                        Some(r) => {
                            let r = char_range(open.model.rope(), r, encoding);
                            open.model.apply_changes(&ChangeSet::new(vec![Edit::new(r.start, r.end, text)]));
                        }
                        None => open.model.set_text(text),
                    }
                }
                open.version = params["textDocument"]["version"].clone();
                let events: Option<Vec<EditEvent>> = open.model.events_since(before).ok().map(|e| e.cloned().collect());
                match events {
                    Some(events) => self.with_doc(&uri, |service, cx| service.change(cx, &events)),
                    // More edits than the log keeps: the service starts over from the text.
                    None => self.with_doc(&uri, |service, cx| service.open(cx)),
                };
                self.publish(&uri);
            }
            "textDocument/didClose" if self.docs.remove(&uri).is_some() => {
                self.service.close(&uri);
                if self.features.diagnostics {
                    self.notify("textDocument/publishDiagnostics", json!({ "uri": uri, "diagnostics": [] }));
                }
            }
            _ => {}
        }
    }

    /// Queues a notification for the client, for a service that has news between requests.
    pub fn notify(&mut self, method: &str, params: Value) {
        self.out.push(json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    /// Calls `f` with the service and a context for `uri`, if it is open.
    fn with_doc<R>(&mut self, uri: &str, f: impl FnOnce(&mut S, &Context) -> R) -> Option<R> {
        let open = self.docs.get(uri)?;
        let cancel = Cancel::default();
        let cx = Context { uri, language: &open.language, model: &open.model, cancel: &cancel };
        Some(f(&mut self.service, &cx))
    }

    fn publish(&mut self, uri: &str) {
        if !self.features.diagnostics {
            return;
        }
        let encoding = self.encoding;
        let Some(diagnostics) = self.with_doc(uri, |service, cx| service.diagnostics(cx)) else { return };
        let Some(open) = self.docs.get(uri) else { return };
        let rope = open.model.rope();
        let list: Vec<Value> = diagnostics.iter().map(|d| diagnostic_json(rope, encoding, d)).collect();
        let version = open.version.clone();
        self.notify("textDocument/publishDiagnostics", json!({ "uri": uri, "version": version, "diagnostics": list }));
    }

    fn capabilities(&self) -> Value {
        let f = &self.features;
        let chars = |cs: &[char]| cs.iter().map(char::to_string).collect::<Vec<_>>();
        let mut c = Map::new();
        c.insert("positionEncoding".into(), json!(self.encoding.name()));
        c.insert("textDocumentSync".into(), json!({ "openClose": true, "change": 2 }));
        if let Some(t) = &f.completion {
            c.insert("completionProvider".into(), json!({ "triggerCharacters": chars(t) }));
        }
        if let Some(t) = &f.signature_help {
            c.insert("signatureHelpProvider".into(), json!({ "triggerCharacters": chars(t) }));
        }
        if let Some((types, modifiers)) = &f.semantic_tokens {
            c.insert(
                "semanticTokensProvider".into(),
                json!({ "legend": { "tokenTypes": types, "tokenModifiers": modifiers }, "full": true }),
            );
        }
        if !f.commands.is_empty() {
            c.insert("executeCommandProvider".into(), json!({ "commands": f.commands }));
        }
        for (on, name) in [
            (f.hover, "hoverProvider"),
            (f.definition, "definitionProvider"),
            (f.references, "referencesProvider"),
            (f.document_highlights, "documentHighlightProvider"),
            (f.rename, "renameProvider"),
            (f.formatting, "documentFormattingProvider"),
            (f.code_actions, "codeActionProvider"),
            (f.inlay_hints, "inlayHintProvider"),
            (f.folding_ranges, "foldingRangeProvider"),
            (f.selection_ranges, "selectionRangeProvider"),
            (f.document_symbols, "documentSymbolProvider"),
        ] {
            if on {
                c.insert(name.into(), json!(true));
            }
        }
        if f.code_lens {
            c.insert("codeLensProvider".into(), json!({}));
        }
        if !f.methods.is_empty() {
            let methods: Map<String, Value> = f.methods.iter().map(|m| (m.clone(), json!(true))).collect();
            c.insert("experimental".into(), Value::Object(methods));
        }
        Value::Object(c)
    }

    fn request(&mut self, method: &str, params: &Value, cancel: &Cancel) -> Result<Value, (i64, String)> {
        match method {
            "initialize" => {
                let offered = params["capabilities"]["general"]["positionEncodings"].as_array();
                self.encoding = match offered {
                    Some(list) if list.iter().any(|e| e == "utf-8") => PositionEncoding::Utf8,
                    _ => PositionEncoding::Utf16,
                };
                self.initialized = true;
                return Ok(
                    json!({ "capabilities": self.capabilities(), "serverInfo": { "name": self.features.name } }),
                );
            }
            "shutdown" => {
                self.shut_down = true;
                return Ok(Value::Null);
            }
            "workspace/executeCommand" => {
                let args = params["arguments"].as_array().map(Vec::as_slice).unwrap_or(&[]);
                return Ok(self.service.execute_command(params["command"].as_str().unwrap_or(""), args));
            }
            _ => {}
        }
        let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
        let enc = self.encoding;
        let legend = self.features.semantic_tokens.clone().unwrap_or_default();
        let Some(open) = self.docs.get(uri) else {
            return Err((INVALID_PARAMS, format!("{method}: {uri} is not open")));
        };
        let service = &mut self.service;
        let cx = Context { uri, language: &open.language, model: &open.model, cancel };
        let rope = open.model.rope();
        let at = || offset(rope, &params["position"], enc);
        let r = |x: Range<usize>| range(rope, x, enc);
        Ok(match method {
            "textDocument/completion" => {
                let trigger = match params["context"]["triggerKind"].as_u64() {
                    Some(2) => params["context"]["triggerCharacter"].as_str().and_then(|s| s.chars().next()),
                    _ => None,
                };
                let items = service.completion(&cx, at(), trigger);
                json!({ "isIncomplete": false, "items": items.iter().map(|i| completion_json(i, &r)).collect::<Vec<_>>() })
            }
            "textDocument/hover" => match service.hover(&cx, at()) {
                None => Value::Null,
                Some(h) => {
                    let contents = match h.contents.as_slice() {
                        [one] => json!({ "kind": "markdown", "value": one }),
                        many => json!(many),
                    };
                    let mut v = json!({ "contents": contents });
                    if let Some(x) = h.range {
                        v["range"] = r(x);
                    }
                    v
                }
            },
            "textDocument/signatureHelp" => match service.signature_help(&cx, at()) {
                None => Value::Null,
                Some(s) => json!({
                    "signatures": s.signatures.iter().map(|(label, doc, params)| {
                        let mut v = json!({ "label": label, "parameters": params.iter().map(|p| json!({ "label": p })).collect::<Vec<_>>() });
                        if let Some(d) = doc {
                            v["documentation"] = json!({ "kind": "markdown", "value": d });
                        }
                        v
                    }).collect::<Vec<_>>(),
                    "activeSignature": s.active_signature,
                    "activeParameter": s.active_parameter,
                }),
            },
            "textDocument/definition" => locations_json(&service.definition(&cx, at()), uri, &r),
            "textDocument/references" => locations_json(&service.references(&cx, at()), uri, &r),
            "textDocument/documentHighlight" => {
                json!(
                    service
                        .document_highlights(&cx, at())
                        .into_iter()
                        .map(|x| json!({ "range": r(x) }))
                        .collect::<Vec<_>>()
                )
            }
            "textDocument/rename" => match service.rename(&cx, at(), params["newName"].as_str().unwrap_or("")) {
                None => Value::Null,
                Some(edit) => workspace_edit_json(&edit, uri, &r),
            },
            "textDocument/formatting" => {
                let options = &params["options"];
                let tab = options["tabSize"].as_u64().unwrap_or(4) as usize;
                let spaces = options["insertSpaces"].as_bool().unwrap_or(true);
                edits_json(&service.formatting(&cx, tab, spaces), &r)
            }
            "textDocument/codeAction" => {
                let actions = service.code_actions(&cx, char_range(rope, &params["range"], enc));
                json!(
                    actions
                        .iter()
                        .map(|a| {
                            let mut v = json!({ "title": a.title });
                            if let Some(e) = &a.edit {
                                v["edit"] = workspace_edit_json(e, uri, &r);
                            }
                            if let Some(c) = &a.command {
                                v["command"] = c.clone();
                            }
                            v
                        })
                        .collect::<Vec<_>>()
                )
            }
            "textDocument/codeLens" => json!(
                service
                    .code_lens(&cx)
                    .into_iter()
                    .map(|l| {
                        let mut v = json!({ "range": r(l.range) });
                        if let Some(c) = l.command {
                            v["command"] = c;
                        }
                        v
                    })
                    .collect::<Vec<_>>()
            ),
            "textDocument/semanticTokens/full" => {
                json!({ "data": encode_tokens(rope, enc, &legend, service.semantic_tokens(&cx)) })
            }
            "textDocument/inlayHint" => {
                let hints = service.inlay_hints(&cx, char_range(rope, &params["range"], enc));
                json!(
                    hints
                        .iter()
                        .map(|h| json!({ "position": position(rope, h.offset, enc), "label": h.label }))
                        .collect::<Vec<_>>()
                )
            }
            "textDocument/foldingRange" => json!(
                service
                    .folding_ranges(&cx)
                    .into_iter()
                    .map(|(a, b)| json!({ "startLine": a, "endLine": b }))
                    .collect::<Vec<_>>()
            ),
            "textDocument/selectionRange" => {
                let positions = params["positions"].as_array().map(Vec::as_slice).unwrap_or(&[]);
                json!(
                    positions
                        .iter()
                        .map(|p| {
                            let ranges = service.selection_ranges(&cx, offset(rope, p, enc));
                            let mut node = Value::Null;
                            for x in ranges.into_iter().rev() {
                                node = if node.is_null() {
                                    json!({ "range": r(x) })
                                } else {
                                    json!({ "range": r(x), "parent": node })
                                };
                            }
                            node
                        })
                        .collect::<Vec<_>>()
                )
            }
            "textDocument/documentSymbol" => {
                json!(service.document_symbols(&cx).iter().map(|s| symbol_json(s, &r)).collect::<Vec<_>>())
            }
            _ => match service.request(&cx, method, params) {
                Some(v) => v,
                None => return Err((METHOD_NOT_FOUND, format!("{method} is not supported"))),
            },
        })
    }
}

fn completion_json(i: &CompletionItem, r: &impl Fn(Range<usize>) -> Value) -> Value {
    let mut v = json!({ "label": i.label });
    if let Some(k) = i.kind {
        v["kind"] = json!(k);
    }
    if let Some(d) = &i.detail {
        v["detail"] = json!(d);
    }
    if let Some(d) = &i.documentation {
        v["documentation"] = json!({ "kind": "markdown", "value": d });
    }
    if i.is_snippet {
        v["insertTextFormat"] = json!(2);
    }
    match &i.range {
        Some(x) => v["textEdit"] = json!({ "range": r(x.clone()), "newText": i.insert_text }),
        None => v["insertText"] = json!(i.insert_text),
    }
    if let Some(s) = &i.sort_text {
        v["sortText"] = json!(s);
    }
    if let Some(s) = &i.filter_text {
        v["filterText"] = json!(s);
    }
    v
}

fn locations_json(list: &[Location], uri: &str, r: &impl Fn(Range<usize>) -> Value) -> Value {
    json!(
        list.iter()
            .map(|l| {
                let range = match &l.range {
                    Some(x) if l.uri == uri => r(x.clone()),
                    _ => l.lsp_range.clone(),
                };
                json!({ "uri": l.uri, "range": range })
            })
            .collect::<Vec<_>>()
    )
}

fn edits_json(edits: &[TextEdit], r: &impl Fn(Range<usize>) -> Value) -> Value {
    json!(edits.iter().map(|e| json!({ "range": r(e.range.clone()), "newText": e.text })).collect::<Vec<_>>())
}

fn workspace_edit_json(edit: &WorkspaceEdit, uri: &str, r: &impl Fn(Range<usize>) -> Value) -> Value {
    let mut changes = Map::new();
    if !edit.local.is_empty() {
        changes.insert(uri.to_owned(), edits_json(&edit.local, r));
    }
    for (other, edits) in &edit.other {
        let entry = changes.entry(other.clone()).or_insert_with(|| json!([]));
        if let Some(list) = entry.as_array_mut() {
            list.extend(edits.iter().cloned());
        }
    }
    json!({ "changes": changes })
}

fn symbol_json(s: &Symbol, r: &impl Fn(Range<usize>) -> Value) -> Value {
    let mut v = json!({
        "name": s.name,
        "kind": s.kind,
        "range": r(s.range.clone()),
        "selectionRange": r(s.selection_range.clone()),
        "children": s.children.iter().map(|c| symbol_json(c, r)).collect::<Vec<_>>(),
    });
    if let Some(d) = &s.detail {
        v["detail"] = json!(d);
    }
    v
}

fn diagnostic_json(rope: &Rope, enc: PositionEncoding, d: &Diagnostic) -> Value {
    let mut v =
        json!({ "range": range(rope, d.range.clone(), enc), "severity": d.severity as u8, "message": d.message });
    if let Some(s) = &d.source {
        v["source"] = json!(s);
    }
    if let Some(c) = &d.code {
        v["code"] = json!(c);
    }
    v
}

/// (line, character) of a char offset.
fn line_char(rope: &Rope, at: usize, enc: PositionEncoding) -> (u64, u64) {
    let p = position(rope, at, enc);
    (p["line"].as_u64().unwrap_or(0), p["character"].as_u64().unwrap_or(0))
}

/// Semantic tokens in LSP's relative encoding. Tokens spanning lines are split per line;
/// types missing from the legend are left out.
fn encode_tokens(
    rope: &Rope,
    enc: PositionEncoding,
    (types, modifiers): &(Vec<String>, Vec<String>),
    tokens: Vec<crate::SemanticToken>,
) -> Vec<u64> {
    let len = rope.len_chars();
    // Split into one segment per line first, then sort: a token inside a multi-line one
    // starts on an earlier line than that one's last segment, and deltas must not go back.
    let mut segments: Vec<(u64, u64, u64, u64, u64)> = Vec::with_capacity(tokens.len());
    for t in tokens {
        let Some(ty) = types.iter().position(|x| *x == t.token_type) else { continue };
        let bits = t
            .modifiers
            .iter()
            .filter_map(|m| modifiers.iter().position(|x| x == m))
            .fold(0u64, |acc, i| acc | (1 << i));
        let (start, end) = (t.range.start.min(len), t.range.end.min(len));
        let mut a = start;
        while a < end {
            let b = end.min(line_end(rope, rope.char_to_line(a)));
            if b > a {
                let (line, ch) = line_char(rope, a, enc);
                let (_, ch_end) = line_char(rope, b, enc);
                segments.push((line, ch, ch_end - ch, ty as u64, bits));
            }
            // Past the line break to the next line's start.
            let next = rope.line_to_char((rope.char_to_line(a) + 1).min(rope.len_lines()));
            if next <= a {
                break;
            }
            a = next;
        }
    }
    segments.sort_by_key(|&(line, ch, ..)| (line, ch));
    let mut data = Vec::with_capacity(segments.len() * 5);
    let (mut prev_line, mut prev_char) = (0u64, 0u64);
    for (line, ch, length, ty, bits) in segments {
        let delta_char = if line == prev_line { ch - prev_char } else { ch };
        data.extend([line - prev_line, delta_char, length, ty, bits]);
        (prev_line, prev_char) = (line, ch);
    }
    data
}

/// Serves `service` on a byte stream with LSP's framing until the client sends `exit` or
/// the input ends. A thread reads the input, so `$/cancelRequest` reaches the request being
/// computed.
pub fn serve<S: LanguageService>(
    service: S,
    input: impl Read + Send + 'static,
    mut output: impl Write,
) -> io::Result<()> {
    let mut server = LspServer::new(service);
    let in_flight = server.in_flight.clone();
    let (tx, rx) = channel();
    std::thread::Builder::new().name("rustaco-lsp-server-reader".into()).spawn(move || {
        let mut input = BufReader::new(input);
        while let Ok(Some(message)) = read_message(&mut input) {
            if message["method"] == "$/cancelRequest"
                && let Ok(guard) = in_flight.lock()
                && let Some((id, cancel)) = &*guard
                && *id == message["params"]["id"]
            {
                cancel.cancel();
            }
            if tx.send(message).is_err() {
                break;
            }
        }
    })?;
    while let Ok(first) = rx.recv() {
        server.receive(first);
        loop {
            // Take in what arrived meanwhile, so cancellations reach queued requests.
            while let Ok(m) = rx.try_recv() {
                server.receive(m);
            }
            let more = server.process_one();
            for m in server.take_output() {
                write_message(&mut output, &m)?;
            }
            if server.exited() {
                return Ok(());
            }
            if !more {
                break;
            }
        }
    }
    Ok(())
}

/// [`serve`] on the process's stdin and stdout.
pub fn serve_stdio<S: LanguageService>(service: S) -> io::Result<()> {
    serve(service, io::stdin(), io::stdout().lock())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SemanticToken;

    #[test]
    fn tokens_inside_a_multi_line_token_keep_deltas_forward() {
        let rope = Rope::from_str("/* a\nb\nc */ x\n");
        let legend = (vec!["comment".to_owned(), "variable".to_owned()], Vec::new());
        let token = |range: std::ops::Range<usize>, t: &str| SemanticToken {
            range,
            token_type: t.into(),
            modifiers: Vec::new(),
        };
        // A comment over three lines and a token on its middle line.
        let data = encode_tokens(
            &rope,
            PositionEncoding::Utf16,
            &legend,
            vec![token(0..11, "comment"), token(5..6, "variable")],
        );
        let rows: Vec<&[u64]> = data.chunks(5).collect();
        assert_eq!(rows, vec![&[0, 0, 4, 0, 0][..], &[1, 0, 1, 0, 0], &[0, 0, 1, 1, 0], &[1, 0, 4, 0, 0]]);
    }
}
