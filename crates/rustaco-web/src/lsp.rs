//! The LSP client's protocol inside the editor's wasm. [`LspSession`] builds every message
//! and decodes every answer against the model (positions, sync, results in char offsets);
//! JS only carries the strings and keeps the promises (`web/lsp.js`).

use std::collections::HashMap;
use std::ops::Range;

use rustaco_core::TextModel;
use rustaco_lsp::{Document, Session};
use serde_json::{Value, json};
use wasm_bindgen::prelude::*;

use crate::Model;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Initialize,
    Raw,
    Hover,
    Completion,
    SignatureHelp,
    Definition,
    References,
    DocumentHighlight,
    Rename,
    Formatting,
    CodeAction,
    FoldingRange,
    SelectionRange,
    DocumentSymbol,
    CodeLens,
    SemanticTokens,
    InlayHint,
}

struct Pending {
    kind: Kind,
    doc: u32,
    /// The answer, once it arrived.
    answer: Option<Result<Value, String>>,
    /// The model version the request was made for; an answer about another version would
    /// land on the wrong chars.
    version: Option<u64>,
}

/// One language server connection's protocol state.
#[wasm_bindgen]
pub struct LspSession {
    session: Session,
    docs: HashMap<u32, Document>,
    next_doc: u32,
    next_id: i64,
    pending: HashMap<i64, Pending>,
    /// The latest `publishDiagnostics` params per uri.
    diagnostics: HashMap<String, Value>,
    outbox: Vec<String>,
}

/// Drops null fields, so JS sees them as absent (`undefined`), as providers return them.
fn strip_nulls(v: &mut Value) {
    match v {
        Value::Object(o) => {
            o.retain(|_, x| !x.is_null());
            o.values_mut().for_each(strip_nulls);
        }
        Value::Array(a) => a.iter_mut().for_each(strip_nulls),
        _ => {}
    }
}

fn r(range: Range<usize>) -> Value {
    json!({ "start": range.start, "end": range.end })
}

/// LSP semantic token types to the editor's highlight names.
fn semantic_style(token_type: &str) -> &'static str {
    match token_type {
        "namespace" => "module",
        "type" | "class" | "enum" | "interface" | "struct" | "typeParameter" => "type",
        "parameter" => "variable.parameter",
        "property" | "event" => "property",
        "enumMember" => "constant",
        "function" => "function",
        "method" => "function.method",
        "macro" => "function.macro",
        "keyword" | "modifier" => "keyword",
        "comment" => "comment",
        "string" => "string",
        "number" => "number",
        "regexp" => "string.special",
        "operator" => "operator",
        "decorator" => "attribute",
        "label" => "label",
        _ => "variable",
    }
}

impl LspSession {
    fn send(&mut self, message: Value) {
        self.outbox.push(message.to_string());
    }

    fn send_request(&mut self, kind: Kind, doc: u32, method: &str, params: Value) -> f64 {
        let id = self.next_id;
        self.next_id += 1;
        self.pending.insert(id, Pending { kind, doc, answer: None, version: None });
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        id as f64
    }

    fn notify_value(&mut self, method: &str, params: Value) {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    fn sync_doc(&mut self, doc: u32, model: &TextModel) {
        let Some(d) = self.docs.get_mut(&doc) else { return };
        if let Some(params) = self.session.change(d, model) {
            self.notify_value("textDocument/didChange", params);
        }
    }

    /// Decodes an answer for the editor's providers (`web/languages.js` shapes).
    fn decode(&mut self, kind: Kind, doc: Option<&Document>, model: &TextModel, v: &Value) -> Value {
        let s = &self.session;
        let Some(doc) = doc else {
            return match kind {
                Kind::Initialize => {
                    self.session.initialized(v);
                    self.notify_value("initialized", json!({}));
                    self.session.capabilities().clone()
                }
                Kind::Raw => v.clone(),
                // The document was closed meanwhile: nothing to place the answer in.
                _ => Value::Null,
            };
        };
        match kind {
            Kind::Initialize | Kind::Raw => v.clone(),
            Kind::Hover => match s.hover(model, v) {
                None => Value::Null,
                Some(h) => json!({ "contents": h.contents, "range": h.range.map(r) }),
            },
            Kind::Completion => json!(
                s.completion(model, v)
                    .into_iter()
                    .map(|i| json!({
                        "label": i.label,
                        "kind": i.kind.map(|k| k.saturating_sub(1)),
                        "detail": i.detail,
                        "documentation": i.documentation,
                        "insertText": i.insert_text,
                        "isSnippet": i.is_snippet,
                        "sortText": i.sort_text,
                        "filterText": i.filter_text,
                        "range": i.range.map(r),
                    }))
                    .collect::<Vec<_>>()
            ),
            Kind::SignatureHelp => match s.signature_help(v) {
                None => Value::Null,
                Some(h) => json!({
                    "signatures": h.signatures.into_iter().map(|(label, documentation, params)| json!({
                        "label": label,
                        "documentation": documentation,
                        "parameters": params.into_iter().map(|p| json!({ "label": p })).collect::<Vec<_>>(),
                    })).collect::<Vec<_>>(),
                    "activeSignature": h.active_signature,
                    "activeParameter": h.active_parameter,
                }),
            },
            // Ranges in this document; the rest keep LSP's coordinates for JS to place.
            Kind::Definition | Kind::References => json!(
                s.locations(doc, model, v)
                    .into_iter()
                    .map(|l| match l.range {
                        Some(x) => r(x),
                        None => json!({ "uri": l.uri, "lspRange": l.lsp_range }),
                    })
                    .collect::<Vec<_>>()
            ),
            Kind::DocumentHighlight => json!(s.ranges(model, v).into_iter().map(r).collect::<Vec<_>>()),
            Kind::Rename => {
                let edit = s.workspace_edit(doc, model, v);
                let others: serde_json::Map<String, Value> =
                    edit.other.into_iter().map(|(uri, edits)| (uri, Value::Array(edits))).collect();
                json!({
                    "own": edit.local.into_iter().map(|e| json!({ "range": r(e.range), "text": e.text })).collect::<Vec<_>>(),
                    "others": others,
                })
            }
            Kind::Formatting => json!(
                s.text_edits(model, v)
                    .into_iter()
                    .map(|e| json!({ "range": r(e.range), "text": e.text }))
                    .collect::<Vec<_>>()
            ),
            // Titles with the raw edit and command: JS applies them when one is picked.
            Kind::CodeAction => json!(
                v.as_array()
                    .map(Vec::as_slice)
                    .unwrap_or(&[])
                    .iter()
                    .map(|a| {
                        let command = if a["command"].is_string() { a.clone() } else { a["command"].clone() };
                        json!({ "title": a["title"], "edit": a["edit"], "command": command })
                    })
                    .collect::<Vec<_>>()
            ),
            Kind::FoldingRange => {
                json!(s.folding_ranges(v).into_iter().map(|(a, b)| json!({ "start": a, "end": b })).collect::<Vec<_>>())
            }
            Kind::SelectionRange => json!(s.selection_ranges(model, v).into_iter().map(r).collect::<Vec<_>>()),
            Kind::DocumentSymbol => {
                fn symbol(x: rustaco_lsp::Symbol) -> Value {
                    json!({
                        "name": x.name,
                        "kind": x.kind.saturating_sub(1),
                        "detail": x.detail,
                        "range": r(x.range),
                        "selectionRange": r(x.selection_range),
                        "children": x.children.into_iter().map(symbol).collect::<Vec<_>>(),
                    })
                }
                json!(s.document_symbols(model, v).into_iter().map(symbol).collect::<Vec<_>>())
            }
            Kind::CodeLens => json!(
                s.code_lens(model, v)
                    .into_iter()
                    .filter(|l| l.command.is_some())
                    .map(|l| json!({
                        "line": model.rope().char_to_line(l.range.start.min(model.len_chars())),
                        "title": l.title,
                        "command": l.command,
                    }))
                    .collect::<Vec<_>>()
            ),
            Kind::SemanticTokens => json!(s
                .semantic_tokens(model, v)
                .into_iter()
                .map(|t| json!({ "start": t.range.start, "end": t.range.end, "style": semantic_style(&t.token_type) }))
                .collect::<Vec<_>>()),
            Kind::InlayHint => json!(
                s.inlay_hints(model, v)
                    .into_iter()
                    .map(|h| json!({ "offset": h.offset, "label": h.label }))
                    .collect::<Vec<_>>()
            ),
        }
    }
}

#[wasm_bindgen]
impl LspSession {
    #[wasm_bindgen(constructor)]
    pub fn new() -> LspSession {
        LspSession {
            session: Session::new(),
            docs: HashMap::new(),
            next_doc: 1,
            next_id: 1,
            pending: HashMap::new(),
            diagnostics: HashMap::new(),
            outbox: Vec::new(),
        }
    }

    /// Messages to send, oldest first; the queue empties.
    pub fn take(&mut self) -> Vec<String> {
        std::mem::take(&mut self.outbox)
    }

    /// Queues `initialize`; returns the request id. Its result is the server's capabilities.
    pub fn initialize(&mut self, root_uri: Option<String>) -> f64 {
        let params = Session::initialize_params(root_uri.as_deref(), None);
        self.send_request(Kind::Initialize, 0, "initialize", params)
    }

    /// The capabilities the server announced, as JSON.
    pub fn capabilities(&self) -> String {
        self.session.capabilities().to_string()
    }

    /// Opens `model` as `uri`; returns a document handle.
    pub fn open(&mut self, model: &Model, uri: &str, language_id: &str) -> u32 {
        let (doc, params) = self.session.open(uri, language_id, &model.inner.borrow());
        self.notify_value("textDocument/didOpen", params);
        let handle = self.next_doc;
        self.next_doc += 1;
        self.docs.insert(handle, doc);
        handle
    }

    /// Queues a `didChange` when the model moved on since the server last saw it.
    pub fn sync(&mut self, doc: u32, model: &Model) {
        self.sync_doc(doc, &model.inner.borrow());
    }

    pub fn close(&mut self, doc: u32) {
        if let Some(d) = self.docs.remove(&doc) {
            self.diagnostics.remove(&d.uri);
            self.notify_value("textDocument/didClose", Session::close_params(&d));
        }
    }

    /// Syncs, then queues a request about `doc`; returns its id. `kind` names the editor
    /// feature; `a` and `b` are offsets (a position, or a range's ends) and `text` the extra
    /// argument (trigger character, new name). Formatting takes the tab size in `a` and
    /// spaces (1) or tabs (0) in `b`.
    pub fn request(&mut self, kind: &str, doc: u32, model: &Model, a: u32, b: u32, text: Option<String>) -> f64 {
        let m = model.inner.borrow();
        self.sync_doc(doc, &m);
        let Some(d) = self.docs.get(&doc) else { return -1.0 };
        let (a, b) = (a as usize, b as usize);
        let s = &self.session;
        let at = |extra| s.at_params(d, &m, a, extra);
        let whole = |extra| s.doc_params(d, extra);
        let (k, method, params) = match kind {
            "hover" => (Kind::Hover, "textDocument/hover", at(json!({}))),
            "completion" => (
                Kind::Completion,
                "textDocument/completion",
                at(Session::completion_context(text.and_then(|t| t.chars().next()))),
            ),
            "signatureHelp" => (Kind::SignatureHelp, "textDocument/signatureHelp", at(json!({}))),
            "definition" => (Kind::Definition, "textDocument/definition", at(json!({}))),
            "references" => {
                (Kind::References, "textDocument/references", at(json!({ "context": { "includeDeclaration": true } })))
            }
            "documentHighlight" => (Kind::DocumentHighlight, "textDocument/documentHighlight", at(json!({}))),
            "rename" => (Kind::Rename, "textDocument/rename", at(json!({ "newName": text.unwrap_or_default() }))),
            "formatting" => (
                Kind::Formatting,
                "textDocument/formatting",
                whole(json!({ "options": { "tabSize": a, "insertSpaces": b != 0 } })),
            ),
            "codeAction" => {
                // The diagnostics overlapping the range go along, as the server sent them.
                let diagnostics: Vec<Value> = self
                    .diagnostics
                    .get(&d.uri)
                    .and_then(|p| p["diagnostics"].as_array())
                    .map(|list| {
                        list.iter()
                            .filter(|x| {
                                let dr = s.char_range(&m, &x["range"]);
                                dr.start <= b && a <= dr.end
                            })
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default();
                let extra = json!({ "range": s.range(&m, a..b.max(a)), "context": { "diagnostics": diagnostics } });
                (Kind::CodeAction, "textDocument/codeAction", whole(extra))
            }
            "foldingRange" => (Kind::FoldingRange, "textDocument/foldingRange", whole(json!({}))),
            "selectionRange" => (
                Kind::SelectionRange,
                "textDocument/selectionRange",
                whole(json!({ "positions": [s.position(&m, a)] })),
            ),
            "documentSymbol" => (Kind::DocumentSymbol, "textDocument/documentSymbol", whole(json!({}))),
            "codeLens" => (Kind::CodeLens, "textDocument/codeLens", whole(json!({}))),
            "semanticTokens" => (Kind::SemanticTokens, "textDocument/semanticTokens/full", whole(json!({}))),
            "inlayHint" => {
                (Kind::InlayHint, "textDocument/inlayHint", whole(json!({ "range": s.range(&m, a..b.max(a)) })))
            }
            _ => return -1.0,
        };
        let version = m.version();
        drop(m);
        let id = self.send_request(k, doc, method, params);
        if let Some(p) = self.pending.get_mut(&(id as i64)) {
            p.version = Some(version);
        }
        id
    }

    /// Queues any request with JSON params; its result comes back as the server sent it.
    #[wasm_bindgen(js_name = requestRaw)]
    pub fn request_raw(&mut self, method: &str, params: &str) -> f64 {
        let params = serde_json::from_str(params).unwrap_or(Value::Null);
        self.send_request(Kind::Raw, 0, method, params)
    }

    /// Queues a notification with JSON params.
    pub fn notify(&mut self, method: &str, params: &str) {
        let params = serde_json::from_str(params).unwrap_or(Value::Null);
        self.notify_value(method, params);
    }

    /// Answers a request the server sent, with a JSON result.
    pub fn respond(&mut self, id: &str, result: &str) {
        let id: Value = serde_json::from_str(id).unwrap_or(Value::Null);
        let result: Value = serde_json::from_str(result).unwrap_or(Value::Null);
        self.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    }

    /// Drops a request and tells the server; its answer, if it comes, is ignored.
    pub fn cancel(&mut self, id: f64) {
        if self.pending.remove(&(id as i64)).is_some() {
            self.notify_value("$/cancelRequest", json!({ "id": id as i64 }));
        }
    }

    /// Takes in one message from the server and says what it was, as JSON:
    /// `{ type: "response", id, error? }` (the result waits for [`LspSession::result`]),
    /// `{ type: "diagnostics", uri }`, `{ type: "applyEdit", id, edit }`,
    /// `{ type: "notification", method, params }` for any other notification, or
    /// `{ type: "other" }`. Other server requests are answered with null here.
    pub fn receive(&mut self, text: &str) -> String {
        let Ok(msg) = serde_json::from_str::<Value>(text) else { return r#"{"type":"other"}"#.into() };
        let method = msg["method"].as_str().map(str::to_owned);
        let id = msg.get("id").cloned();
        let out = match (method.as_deref(), id) {
            (None, Some(id)) => {
                let Some(p) = id.as_i64().and_then(|n| self.pending.get_mut(&n)) else {
                    return r#"{"type":"other"}"#.into();
                };
                let answer = match msg.get("error") {
                    Some(e) => Err(e["message"].as_str().unwrap_or("error").to_owned()),
                    None => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
                };
                let error = answer.as_ref().err().cloned();
                p.answer = Some(answer);
                json!({ "type": "response", "id": id, "error": error })
            }
            (Some("textDocument/publishDiagnostics"), _) => {
                let uri = msg["params"]["uri"].as_str().unwrap_or("").to_owned();
                self.diagnostics.insert(uri.clone(), msg["params"].clone());
                json!({ "type": "diagnostics", "uri": uri })
            }
            (Some("workspace/applyEdit"), Some(id)) => {
                json!({ "type": "applyEdit", "id": id, "edit": msg["params"]["edit"] })
            }
            (Some(_), Some(id)) => {
                // Configuration, progress and the like: an empty answer keeps the server going.
                self.send(json!({ "jsonrpc": "2.0", "id": id, "result": null }));
                json!({ "type": "other" })
            }
            (Some(m), None) => json!({ "type": "notification", "method": m, "params": msg["params"] }),
            _ => json!({ "type": "other" }),
        };
        out.to_string()
    }

    /// The decoded result of a request about a document whose answer arrived, as JSON,
    /// against `model` (the document asked about); the request is forgotten. `null` when the
    /// model changed since the request, since the answer's positions are about the old text.
    pub fn result(&mut self, id: f64, model: &Model) -> String {
        let Some(p) = self.pending.remove(&(id as i64)) else { return "null".into() };
        let Some(Ok(v)) = p.answer else { return "null".into() };
        let doc = self.docs.get(&p.doc).cloned();
        let m = model.inner.borrow();
        // The text changed while the server worked: its offsets would point at other chars.
        if p.version.is_some_and(|v| v != m.version()) {
            return "null".into();
        }
        let mut out = self.decode(p.kind, doc.as_ref(), &m, &v);
        strip_nulls(&mut out);
        out.to_string()
    }

    /// The result of `initialize` or a raw request, as JSON; the request is forgotten.
    #[wasm_bindgen(js_name = resultRaw)]
    pub fn result_raw(&mut self, id: f64) -> String {
        let Some(p) = self.pending.remove(&(id as i64)) else { return "null".into() };
        let Some(Ok(v)) = p.answer else { return "null".into() };
        self.decode(p.kind, None, &TextModel::new(""), &v).to_string()
    }

    /// The latest diagnostics for `uri` against `model`, as JSON:
    /// `[{ start, end, severity, message }]`.
    pub fn diagnostics(&self, uri: &str, model: &Model) -> String {
        let Some(params) = self.diagnostics.get(uri) else { return "[]".into() };
        let m = model.inner.borrow();
        let list: Vec<Value> = self
            .session
            .diagnostics(&m, params)
            .into_iter()
            .map(|d| json!({ "start": d.range.start, "end": d.range.end, "severity": d.severity as u8, "message": d.message }))
            .collect();
        json!(list).to_string()
    }

    /// The char range `[start, end]` of an LSP range (JSON) in `model`.
    #[wasm_bindgen(js_name = toRange)]
    pub fn to_range(&self, range: &str, model: &Model) -> Vec<u32> {
        let v: Value = serde_json::from_str(range).unwrap_or(Value::Null);
        let x = self.session.char_range(&model.inner.borrow(), &v);
        vec![x.start as u32, x.end as u32]
    }

    /// The LSP position (JSON) of a char offset in `model`, in the negotiated encoding.
    #[wasm_bindgen(js_name = toPosition)]
    pub fn to_position(&self, model: &Model, offset: u32) -> String {
        self.session.position(&model.inner.borrow(), offset as usize).to_string()
    }

    /// The char offset of an LSP position (JSON) in `model`.
    #[wasm_bindgen(js_name = toOffset)]
    pub fn to_offset(&self, model: &Model, position: &str) -> u32 {
        let v: Value = serde_json::from_str(position).unwrap_or(Value::Null);
        self.session.offset(&model.inner.borrow(), &v) as u32
    }
}

impl Default for LspSession {
    fn default() -> Self {
        Self::new()
    }
}
