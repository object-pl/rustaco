//! Language Server Protocol for the Rustaco editor: the client, and the server side.
//!
//! The server side is [`LanguageService`], one trait for what a language service answers
//! (completion, hover, definitions, symbols...) in char offsets, and [`LspServer`], which
//! serves any implementation over the protocol: in a browser worker, or as a program on
//! stdio ([`serve_stdio`]). The editor's own client talks to it like to any other server.
//!
//! The client:
//!
//! [`LspClient`] speaks JSON-RPC over any pair of byte streams: a server's stdio
//! ([`LspClient::spawn`]) on the desktop, or a bridge the host provides. It negotiates the
//! position encoding (UTF-8 when the server offers it, UTF-16 otherwise), keeps documents in
//! sync from the model's edit events with incremental `didChange`, and maps the answers of
//! each request to the editor's char offsets. Edits for other documents
//! (`workspace/applyEdit`, renames across files) come back in LSP's own coordinates for the
//! host to apply, since the editor holds one model at a time.

mod convert;
mod rpc;
mod server;
mod service;

use std::io;
use std::ops::Range;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

pub use convert::{PositionEncoding, char_range, offset, position, range};
pub use rpc::{Connection, RpcError, ServerMessage, read_message, write_message};
use rustaco_core::TextModel;
/// JSON values as the protocol carries them (serde_json's).
pub use serde_json::Value as JsonValue;
use serde_json::{Value, json};
pub use server::{LspServer, serve, serve_stdio};
pub use service::{Cancel, Context, Diagnostic, Features, LanguageService, Severity};

/// A hover answer.
#[derive(Clone, Debug, PartialEq)]
pub struct Hover {
    /// Markdown or plain text blocks.
    pub contents: Vec<String>,
    pub range: Option<Range<usize>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompletionItem {
    pub label: String,
    /// LSP's `CompletionItemKind` (1-based).
    pub kind: Option<u64>,
    pub detail: Option<String>,
    pub documentation: Option<String>,
    pub insert_text: String,
    pub is_snippet: bool,
    /// What the item replaces, when the server says.
    pub range: Option<Range<usize>>,
    pub sort_text: Option<String>,
    pub filter_text: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SignatureHelp {
    /// (label, documentation, parameter labels)
    pub signatures: Vec<(String, Option<String>, Vec<String>)>,
    pub active_signature: usize,
    pub active_parameter: usize,
}

/// A place in a document: the char range when it is the document asked about, the raw LSP
/// range otherwise.
#[derive(Clone, Debug, PartialEq)]
pub struct Location {
    pub uri: String,
    pub range: Option<Range<usize>>,
    pub lsp_range: Value,
}

/// An edit to the document asked about.
#[derive(Clone, Debug, PartialEq)]
pub struct TextEdit {
    pub range: Range<usize>,
    pub text: String,
}

/// Edits across documents: those for the document asked about in char offsets, the rest as
/// LSP sent them (`uri`, raw `TextEdit` values) for the host.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkspaceEdit {
    pub local: Vec<TextEdit>,
    pub other: Vec<(String, Vec<Value>)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CodeAction {
    pub title: String,
    pub edit: Option<WorkspaceEdit>,
    /// A command to run on the server with [`LspClient::execute_command`].
    pub command: Option<Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Symbol {
    pub name: String,
    /// LSP's `SymbolKind` (1-based).
    pub kind: u64,
    pub detail: Option<String>,
    pub range: Range<usize>,
    pub selection_range: Range<usize>,
    pub children: Vec<Symbol>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SemanticToken {
    pub range: Range<usize>,
    /// The token type from the server's legend (`"function"`, `"parameter"`, ...).
    pub token_type: String,
    pub modifiers: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InlayHint {
    pub offset: usize,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CodeLens {
    pub range: Range<usize>,
    pub title: Option<String>,
    pub command: Option<Value>,
}

/// A document open on the server.
#[derive(Clone, Debug)]
pub struct Document {
    pub uri: String,
    version: i32,
    /// Model version the server has.
    synced: u64,
}

/// An initialized connection to one language server.
pub struct LspClient {
    conn: Connection,
    capabilities: Value,
    server_info: Value,
    encoding: PositionEncoding,
    /// How long a request may take.
    pub timeout: Duration,
    token_types: Vec<String>,
    token_modifiers: Vec<String>,
}

impl LspClient {
    /// A client on a connection; call [`LspClient::initialize`] next.
    pub fn new(conn: Connection) -> Self {
        Self {
            conn,
            capabilities: Value::Null,
            server_info: Value::Null,
            encoding: PositionEncoding::Utf16,
            timeout: Duration::from_secs(10),
            token_types: Vec::new(),
            token_modifiers: Vec::new(),
        }
    }

    /// Starts a server and talks to it over its stdin and stdout.
    pub fn spawn(program: &str, args: &[&str], cwd: Option<&Path>) -> io::Result<(Self, Child)> {
        let mut command = Command::new(program);
        command.args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        if let Some(dir) = cwd {
            command.current_dir(dir);
        }
        let mut child = command.spawn()?;
        let stdin = child.stdin.take().ok_or_else(|| io::Error::other("no stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| io::Error::other("no stdout"))?;
        Ok((Self::new(Connection::new(stdout, stdin)), child))
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn capabilities(&self) -> &Value {
        &self.capabilities
    }

    /// `{ name, version }` the server reported, if any.
    pub fn server_info(&self) -> &Value {
        &self.server_info
    }

    pub fn encoding(&self) -> PositionEncoding {
        self.encoding
    }

    /// Whether the server offers `provider` (`"hoverProvider"`, `"renameProvider"`, ...).
    pub fn supports(&self, provider: &str) -> bool {
        match self.capabilities.get(provider) {
            None | Some(Value::Null) | Some(Value::Bool(false)) => false,
            Some(_) => true,
        }
    }

    /// The handshake: `initialize` with the client's capabilities, then `initialized`.
    pub fn initialize(&mut self, root_uri: Option<&str>) -> Result<(), RpcError> {
        let params = json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "workspaceFolders": root_uri.map(|u| json!([{ "uri": u, "name": "workspace" }])),
            "capabilities": client_capabilities(),
            "clientInfo": { "name": "rustaco" },
        });
        let result = self.conn.request("initialize", params, Duration::from_secs(60))?;
        self.capabilities = result.get("capabilities").cloned().unwrap_or(Value::Null);
        self.server_info = result.get("serverInfo").cloned().unwrap_or(Value::Null);
        self.encoding = match self.capabilities.get("positionEncoding").and_then(Value::as_str) {
            Some("utf-8") => PositionEncoding::Utf8,
            _ => PositionEncoding::Utf16,
        };
        let legend = &self.capabilities["semanticTokensProvider"]["legend"];
        let names = |v: &Value| {
            v.as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_owned)).collect()).unwrap_or_default()
        };
        self.token_types = names(&legend["tokenTypes"]);
        self.token_modifiers = names(&legend["tokenModifiers"]);
        self.conn.notify("initialized", json!({}))
    }

    /// `shutdown`, then `exit`.
    pub fn shutdown(&self) {
        let _ = self.conn.request("shutdown", Value::Null, Duration::from_secs(5));
        let _ = self.conn.notify("exit", Value::Null);
    }

    fn sync_kind(&self) -> u64 {
        let sync = &self.capabilities["textDocumentSync"];
        sync.as_u64().or_else(|| sync["change"].as_u64()).unwrap_or(1)
    }

    /// Opens `model` on the server as `uri`.
    pub fn open(&self, uri: &str, language_id: &str, model: &TextModel) -> Result<Document, RpcError> {
        self.conn.notify(
            "textDocument/didOpen",
            json!({ "textDocument": { "uri": uri, "languageId": language_id, "version": 1, "text": model.text() } }),
        )?;
        Ok(Document { uri: uri.to_owned(), version: 1, synced: model.version() })
    }

    /// Sends the model's edits since the last sync: incremental changes when the server
    /// takes them and the edit log reaches back, the whole text otherwise.
    pub fn sync(&self, doc: &mut Document, model: &TextModel) -> Result<(), RpcError> {
        if doc.synced == model.version() {
            return Ok(());
        }
        let changes: Vec<Value> = match (self.sync_kind(), model.events_since(doc.synced)) {
            (2, Ok(events)) => events
                .map(|e| {
                    let (start, end) = match self.encoding {
                        PositionEncoding::Utf16 => (
                            json!({ "line": e.start_utf16.line, "character": e.start_utf16.character }),
                            json!({ "line": e.old_end_utf16.line, "character": e.old_end_utf16.character }),
                        ),
                        PositionEncoding::Utf8 => (
                            json!({ "line": e.start_point.row, "character": e.start_point.column }),
                            json!({ "line": e.old_end_point.row, "character": e.old_end_point.column }),
                        ),
                    };
                    json!({ "range": { "start": start, "end": end }, "text": e.text })
                })
                .collect(),
            _ => vec![json!({ "text": model.text() })],
        };
        doc.version += 1;
        doc.synced = model.version();
        self.conn.notify(
            "textDocument/didChange",
            json!({ "textDocument": { "uri": doc.uri, "version": doc.version }, "contentChanges": changes }),
        )
    }

    pub fn close(&self, doc: Document) -> Result<(), RpcError> {
        self.conn.notify("textDocument/didClose", json!({ "textDocument": { "uri": doc.uri } }))
    }

    /// Syncs, then sends a request about `doc` at `at` (char offset) with `extra` params.
    fn at(
        &self,
        method: &str,
        doc: &mut Document,
        model: &TextModel,
        at: usize,
        extra: Value,
    ) -> Result<Value, RpcError> {
        self.sync(doc, model)?;
        let mut params = json!({
            "textDocument": { "uri": doc.uri },
            "position": position(model.rope(), at, self.encoding),
        });
        if let (Some(p), Some(e)) = (params.as_object_mut(), extra.as_object()) {
            p.extend(e.clone());
        }
        self.conn.request(method, params, self.timeout)
    }

    fn whole(&self, method: &str, doc: &mut Document, model: &TextModel, extra: Value) -> Result<Value, RpcError> {
        self.sync(doc, model)?;
        let mut params = json!({ "textDocument": { "uri": doc.uri } });
        if let (Some(p), Some(e)) = (params.as_object_mut(), extra.as_object()) {
            p.extend(e.clone());
        }
        self.conn.request(method, params, self.timeout)
    }

    fn r(&self, model: &TextModel, v: &Value) -> Range<usize> {
        char_range(model.rope(), v, self.encoding)
    }

    pub fn hover(&self, doc: &mut Document, model: &TextModel, at: usize) -> Result<Option<Hover>, RpcError> {
        let v = self.at("textDocument/hover", doc, model, at, json!({}))?;
        if v.is_null() {
            return Ok(None);
        }
        let contents = match &v["contents"] {
            Value::Array(items) => items.iter().map(marked_string).collect(),
            other => vec![marked_string(other)],
        };
        let range = v.get("range").filter(|r| !r.is_null()).map(|r| self.r(model, r));
        Ok(Some(Hover { contents, range }))
    }

    pub fn completion(
        &self,
        doc: &mut Document,
        model: &TextModel,
        at: usize,
        trigger: Option<char>,
    ) -> Result<Vec<CompletionItem>, RpcError> {
        let context = match trigger {
            Some(c) => json!({ "triggerKind": 2, "triggerCharacter": c.to_string() }),
            None => json!({ "triggerKind": 1 }),
        };
        let v = self.at("textDocument/completion", doc, model, at, json!({ "context": context }))?;
        let items = v.get("items").unwrap_or(&v).as_array().cloned().unwrap_or_default();
        Ok(items
            .iter()
            .map(|item| {
                let label = item["label"].as_str().unwrap_or("").to_owned();
                let edit = &item["textEdit"];
                let (insert, range) = if edit.is_object() {
                    let r = edit.get("range").or_else(|| edit.get("replace")).map(|r| self.r(model, r));
                    (edit["newText"].as_str().unwrap_or(&label).to_owned(), r)
                } else {
                    (item["insertText"].as_str().unwrap_or(&label).to_owned(), None)
                };
                CompletionItem {
                    kind: item["kind"].as_u64(),
                    detail: item["detail"].as_str().map(str::to_owned),
                    documentation: item.get("documentation").filter(|d| !d.is_null()).map(marked_string),
                    is_snippet: item["insertTextFormat"].as_u64() == Some(2),
                    insert_text: insert,
                    range,
                    sort_text: item["sortText"].as_str().map(str::to_owned),
                    filter_text: item["filterText"].as_str().map(str::to_owned),
                    label,
                }
            })
            .collect())
    }

    pub fn signature_help(
        &self,
        doc: &mut Document,
        model: &TextModel,
        at: usize,
    ) -> Result<Option<SignatureHelp>, RpcError> {
        let v = self.at("textDocument/signatureHelp", doc, model, at, json!({}))?;
        let Some(sigs) = v["signatures"].as_array() else { return Ok(None) };
        let signatures = sigs
            .iter()
            .map(|s| {
                let label = s["label"].as_str().unwrap_or("").to_owned();
                let params = s["parameters"]
                    .as_array()
                    .map(|ps| {
                        ps.iter()
                            .map(|p| match &p["label"] {
                                Value::String(l) => l.clone(),
                                // [start, end] offsets into the signature label (UTF-16).
                                Value::Array(r) => {
                                    let (a, b) =
                                        (r[0].as_u64().unwrap_or(0) as usize, r[1].as_u64().unwrap_or(0) as usize);
                                    String::from_utf16_lossy(&label.encode_utf16().collect::<Vec<_>>()[a.min(b)..b])
                                }
                                _ => String::new(),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                (label, s.get("documentation").filter(|d| !d.is_null()).map(marked_string), params)
            })
            .collect();
        Ok(Some(SignatureHelp {
            signatures,
            active_signature: v["activeSignature"].as_u64().unwrap_or(0) as usize,
            active_parameter: v["activeParameter"].as_u64().unwrap_or(0) as usize,
        }))
    }

    fn locations(&self, doc: &Document, model: &TextModel, v: &Value) -> Vec<Location> {
        let list = match v {
            Value::Array(a) => a.clone(),
            Value::Null => Vec::new(),
            single => vec![single.clone()],
        };
        list.iter()
            .map(|l| {
                // Location or LocationLink.
                let uri = l["uri"].as_str().or_else(|| l["targetUri"].as_str()).unwrap_or("").to_owned();
                let lsp_range =
                    l.get("range").or_else(|| l.get("targetSelectionRange")).cloned().unwrap_or(Value::Null);
                let range = (uri == doc.uri).then(|| self.r(model, &lsp_range));
                Location { uri, range, lsp_range }
            })
            .collect()
    }

    pub fn definition(&self, doc: &mut Document, model: &TextModel, at: usize) -> Result<Vec<Location>, RpcError> {
        let v = self.at("textDocument/definition", doc, model, at, json!({}))?;
        Ok(self.locations(doc, model, &v))
    }

    pub fn references(&self, doc: &mut Document, model: &TextModel, at: usize) -> Result<Vec<Location>, RpcError> {
        let v =
            self.at("textDocument/references", doc, model, at, json!({ "context": { "includeDeclaration": true } }))?;
        Ok(self.locations(doc, model, &v))
    }

    pub fn document_highlights(
        &self,
        doc: &mut Document,
        model: &TextModel,
        at: usize,
    ) -> Result<Vec<Range<usize>>, RpcError> {
        let v = self.at("textDocument/documentHighlight", doc, model, at, json!({}))?;
        Ok(v.as_array().map(|a| a.iter().map(|h| self.r(model, &h["range"])).collect()).unwrap_or_default())
    }

    /// Edits from a `WorkspaceEdit`, those for `doc` in char offsets.
    pub fn workspace_edit(&self, doc: &Document, model: &TextModel, v: &Value) -> WorkspaceEdit {
        let mut out = WorkspaceEdit::default();
        let mut add = |uri: &str, edits: &[Value]| {
            if uri == doc.uri {
                out.local.extend(edits.iter().map(|e| TextEdit {
                    range: self.r(model, &e["range"]),
                    text: e["newText"].as_str().unwrap_or("").to_owned(),
                }));
            } else {
                out.other.push((uri.to_owned(), edits.to_vec()));
            }
        };
        if let Some(changes) = v["changes"].as_object() {
            for (uri, edits) in changes {
                add(uri, edits.as_array().map(Vec::as_slice).unwrap_or(&[]));
            }
        }
        if let Some(doc_changes) = v["documentChanges"].as_array() {
            for c in doc_changes {
                if let Some(uri) = c["textDocument"]["uri"].as_str() {
                    add(uri, c["edits"].as_array().map(Vec::as_slice).unwrap_or(&[]));
                }
            }
        }
        out
    }

    pub fn rename(
        &self,
        doc: &mut Document,
        model: &TextModel,
        at: usize,
        new_name: &str,
    ) -> Result<WorkspaceEdit, RpcError> {
        let v = self.at("textDocument/rename", doc, model, at, json!({ "newName": new_name }))?;
        Ok(self.workspace_edit(doc, model, &v))
    }

    pub fn formatting(
        &self,
        doc: &mut Document,
        model: &TextModel,
        tab_size: usize,
        insert_spaces: bool,
    ) -> Result<Vec<TextEdit>, RpcError> {
        let v = self.whole(
            "textDocument/formatting",
            doc,
            model,
            json!({ "options": { "tabSize": tab_size, "insertSpaces": insert_spaces } }),
        )?;
        Ok(v.as_array()
            .map(|a| {
                a.iter()
                    .map(|e| TextEdit {
                        range: self.r(model, &e["range"]),
                        text: e["newText"].as_str().unwrap_or("").to_owned(),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    pub fn code_actions(
        &self,
        doc: &mut Document,
        model: &TextModel,
        range: Range<usize>,
    ) -> Result<Vec<CodeAction>, RpcError> {
        self.sync(doc, model)?;
        let params = json!({
            "textDocument": { "uri": doc.uri },
            "range": convert::range(model.rope(), range, self.encoding),
            "context": { "diagnostics": [] },
        });
        let v = self.conn.request("textDocument/codeAction", params, self.timeout)?;
        Ok(v.as_array()
            .map(|a| {
                a.iter()
                    .map(|c| CodeAction {
                        title: c["title"].as_str().unwrap_or("").to_owned(),
                        edit: c.get("edit").filter(|e| !e.is_null()).map(|e| self.workspace_edit(doc, model, e)),
                        // A bare Command has `command` as a string; a CodeAction nests it.
                        command: if c["command"].is_string() {
                            Some(c.clone())
                        } else {
                            c.get("command").filter(|x| !x.is_null()).cloned()
                        },
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    pub fn execute_command(&self, command: &Value) -> Result<Value, RpcError> {
        self.conn.request(
            "workspace/executeCommand",
            json!({ "command": command["command"], "arguments": command.get("arguments").cloned().unwrap_or(json!([])) }),
            self.timeout,
        )
    }

    /// Folding regions as (first line, last line).
    pub fn folding_ranges(&self, doc: &mut Document, model: &TextModel) -> Result<Vec<(usize, usize)>, RpcError> {
        let v = self.whole("textDocument/foldingRange", doc, model, json!({}))?;
        Ok(v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|f| Some((f["startLine"].as_u64()? as usize, f["endLine"].as_u64()? as usize)))
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Ranges around `at`, innermost first.
    pub fn selection_ranges(
        &self,
        doc: &mut Document,
        model: &TextModel,
        at: usize,
    ) -> Result<Vec<Range<usize>>, RpcError> {
        self.sync(doc, model)?;
        let params =
            json!({ "textDocument": { "uri": doc.uri }, "positions": [position(model.rope(), at, self.encoding)] });
        let v = self.conn.request("textDocument/selectionRange", params, self.timeout)?;
        let mut out = Vec::new();
        let mut node = v.get(0);
        while let Some(n) = node.filter(|n| !n.is_null()) {
            out.push(self.r(model, &n["range"]));
            node = n.get("parent");
        }
        Ok(out)
    }

    pub fn document_symbols(&self, doc: &mut Document, model: &TextModel) -> Result<Vec<Symbol>, RpcError> {
        let v = self.whole("textDocument/documentSymbol", doc, model, json!({}))?;
        fn convert(client: &LspClient, model: &TextModel, s: &Value) -> Symbol {
            // DocumentSymbol, or SymbolInformation with a location.
            let range = s.get("range").or_else(|| s["location"].get("range")).cloned().unwrap_or(Value::Null);
            let selection = s.get("selectionRange").cloned().unwrap_or_else(|| range.clone());
            Symbol {
                name: s["name"].as_str().unwrap_or("").to_owned(),
                kind: s["kind"].as_u64().unwrap_or(13),
                detail: s["detail"].as_str().map(str::to_owned),
                range: client.r(model, &range),
                selection_range: client.r(model, &selection),
                children: s["children"]
                    .as_array()
                    .map(|c| c.iter().map(|x| convert(client, model, x)).collect())
                    .unwrap_or_default(),
            }
        }
        Ok(v.as_array().map(|a| a.iter().map(|s| convert(self, model, s)).collect()).unwrap_or_default())
    }

    pub fn code_lens(&self, doc: &mut Document, model: &TextModel) -> Result<Vec<CodeLens>, RpcError> {
        let v = self.whole("textDocument/codeLens", doc, model, json!({}))?;
        Ok(v.as_array()
            .map(|a| {
                a.iter()
                    .map(|l| CodeLens {
                        range: self.r(model, &l["range"]),
                        title: l["command"]["title"].as_str().map(str::to_owned),
                        command: l.get("command").filter(|c| !c.is_null()).cloned(),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// The whole document's semantic tokens, decoded with the server's legend.
    pub fn semantic_tokens(&self, doc: &mut Document, model: &TextModel) -> Result<Vec<SemanticToken>, RpcError> {
        let v = self.whole("textDocument/semanticTokens/full", doc, model, json!({}))?;
        let data: Vec<u64> =
            v["data"].as_array().map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
        let rope = model.rope();
        let mut out = Vec::with_capacity(data.len() / 5);
        let (mut line, mut start) = (0u64, 0u64);
        for t in data.chunks_exact(5) {
            if t[0] > 0 {
                line += t[0];
                start = t[1];
            } else {
                start += t[1];
            }
            let begin = json!({ "line": line, "character": start });
            let end = json!({ "line": line, "character": start + t[2] });
            let range = offset(rope, &begin, self.encoding)..offset(rope, &end, self.encoding);
            let token_type = self.token_types.get(t[3] as usize).cloned().unwrap_or_default();
            let modifiers = (0..self.token_modifiers.len())
                .filter(|i| t[4] & (1 << i) != 0)
                .map(|i| self.token_modifiers[i].clone())
                .collect();
            out.push(SemanticToken { range, token_type, modifiers });
        }
        Ok(out)
    }

    pub fn inlay_hints(
        &self,
        doc: &mut Document,
        model: &TextModel,
        range: Range<usize>,
    ) -> Result<Vec<InlayHint>, RpcError> {
        self.sync(doc, model)?;
        let params =
            json!({ "textDocument": { "uri": doc.uri }, "range": convert::range(model.rope(), range, self.encoding) });
        let v = self.conn.request("textDocument/inlayHint", params, self.timeout)?;
        Ok(v.as_array()
            .map(|a| {
                a.iter()
                    .map(|h| InlayHint {
                        offset: offset(model.rope(), &h["position"], self.encoding),
                        label: match &h["label"] {
                            Value::String(s) => s.clone(),
                            Value::Array(parts) => parts.iter().filter_map(|p| p["value"].as_str()).collect(),
                            _ => String::new(),
                        },
                    })
                    .collect()
            })
            .unwrap_or_default())
    }
}

/// Text of a `MarkedString` or `MarkupContent`: code in a ``` fence, markdown as is.
fn marked_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Object(o) => match (o.get("language"), o.get("value")) {
            (Some(lang), Some(value)) => {
                format!("```{}\n{}\n```", lang.as_str().unwrap_or(""), value.as_str().unwrap_or(""))
            }
            (None, Some(value)) => value.as_str().unwrap_or("").to_owned(),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

/// What the client can do, as sent in `initialize`.
fn client_capabilities() -> Value {
    json!({
        "general": { "positionEncodings": ["utf-8", "utf-16"] },
        "textDocument": {
            "synchronization": { "didSave": false, "dynamicRegistration": false },
            "hover": { "contentFormat": ["markdown", "plaintext"] },
            "completion": {
                "completionItem": { "snippetSupport": true, "documentationFormat": ["markdown", "plaintext"], "insertReplaceSupport": true },
                "contextSupport": true,
            },
            "signatureHelp": { "signatureInformation": { "parameterInformation": { "labelOffsetSupport": true } } },
            "definition": { "linkSupport": true },
            "references": {},
            "documentHighlight": {},
            "documentSymbol": { "hierarchicalDocumentSymbolSupport": true },
            "codeAction": { "codeActionLiteralSupport": { "codeActionKind": { "valueSet": ["", "quickfix", "refactor", "source"] } } },
            "formatting": {},
            "rename": { "prepareSupport": false },
            "foldingRange": { "lineFoldingOnly": true },
            "selectionRange": {},
            "codeLens": {},
            "inlayHint": {},
            "semanticTokens": {
                "requests": { "full": true },
                "tokenTypes": ["namespace", "type", "class", "enum", "interface", "struct", "typeParameter", "parameter", "variable", "property", "enumMember", "event", "function", "method", "macro", "keyword", "modifier", "comment", "string", "number", "regexp", "operator"],
                "tokenModifiers": ["declaration", "definition", "readonly", "static", "deprecated", "abstract", "async", "modification", "documentation", "defaultLibrary"],
                "formats": ["relative"],
            },
            "publishDiagnostics": {},
        },
        "workspace": { "applyEdit": true, "workspaceEdit": { "documentChanges": true }, "configuration": false },
        "window": { "workDoneProgress": false },
    })
}
