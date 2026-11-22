//! [`Session`]: the client's side of the protocol without I/O. It builds the params of every
//! request and notification and decodes every answer into the editor's terms (char offsets,
//! the result types of this crate), with the capabilities and position encoding the server
//! announced. [`crate::LspClient`] runs it over a blocking connection; the browser runs it
//! inside the editor's wasm with JS carrying the strings.

use std::ops::Range;

use rustaco_core::TextModel;
use serde_json::{Value, json};

use crate::convert::{PositionEncoding, char_range, offset, position, range};
use crate::service::{Diagnostic, Severity};
use crate::{
    CodeAction, CodeLens, CompletionItem, Hover, InlayHint, Location, SemanticToken, SignatureHelp, Symbol, TextEdit,
    WorkspaceEdit,
};

/// A document open on the server.
#[derive(Clone, Debug)]
pub struct Document {
    pub uri: String,
    pub(crate) version: i32,
    /// Model version the server has.
    pub(crate) synced: u64,
}

/// What the client knows about the server after `initialize`.
#[derive(Clone, Debug, Default)]
pub struct Session {
    capabilities: Value,
    server_info: Value,
    encoding: PositionEncoding,
    token_types: Vec<String>,
    token_modifiers: Vec<String>,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    /// Params of `initialize`.
    pub fn initialize_params(root_uri: Option<&str>, process_id: Option<u32>) -> Value {
        json!({
            "processId": process_id,
            "rootUri": root_uri,
            "workspaceFolders": root_uri.map(|u| json!([{ "uri": u, "name": "workspace" }])),
            "capabilities": client_capabilities(),
            "clientInfo": { "name": "rustaco" },
        })
    }

    /// Takes in the answer to `initialize`; `initialized` goes out next.
    pub fn initialized(&mut self, result: &Value) {
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
    }

    pub fn capabilities(&self) -> &Value {
        &self.capabilities
    }

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

    /// Characters that open completion (or signature help, for `"signatureHelpProvider"`).
    pub fn trigger_characters(&self, provider: &str) -> Vec<String> {
        self.capabilities[provider]["triggerCharacters"]
            .as_array()
            .map(|a| a.iter().filter_map(|c| c.as_str().map(str::to_owned)).collect())
            .unwrap_or_default()
    }

    fn sync_kind(&self) -> u64 {
        let sync = &self.capabilities["textDocumentSync"];
        sync.as_u64().or_else(|| sync["change"].as_u64()).unwrap_or(1)
    }

    /// A document for `model` and the params of its `didOpen`.
    pub fn open(&self, uri: &str, language_id: &str, model: &TextModel) -> (Document, Value) {
        let params =
            json!({ "textDocument": { "uri": uri, "languageId": language_id, "version": 1, "text": model.text() } });
        (Document { uri: uri.to_owned(), version: 1, synced: model.version() }, params)
    }

    /// Params of the `didChange` that brings the server to `model`'s text, or None when it
    /// has it: incremental changes when the server takes them and the edit log reaches back,
    /// the whole text otherwise.
    pub fn change(&self, doc: &mut Document, model: &TextModel) -> Option<Value> {
        if doc.synced == model.version() {
            return None;
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
        Some(json!({ "textDocument": { "uri": doc.uri, "version": doc.version }, "contentChanges": changes }))
    }

    pub fn close_params(doc: &Document) -> Value {
        json!({ "textDocument": { "uri": doc.uri } })
    }

    /// `{ textDocument, position }` for `at`, with `extra` fields added.
    pub fn at_params(&self, doc: &Document, model: &TextModel, at: usize, extra: Value) -> Value {
        let mut params = json!({ "textDocument": { "uri": doc.uri }, "position": self.position(model, at) });
        merge(&mut params, extra);
        params
    }

    /// `{ textDocument }` with `extra` fields added.
    pub fn doc_params(&self, doc: &Document, extra: Value) -> Value {
        let mut params = json!({ "textDocument": { "uri": doc.uri } });
        merge(&mut params, extra);
        params
    }

    pub fn position(&self, model: &TextModel, at: usize) -> Value {
        position(model.rope(), at, self.encoding)
    }

    pub fn offset(&self, model: &TextModel, position: &Value) -> usize {
        offset(model.rope(), position, self.encoding)
    }

    pub fn range(&self, model: &TextModel, r: Range<usize>) -> Value {
        range(model.rope(), r, self.encoding)
    }

    pub fn char_range(&self, model: &TextModel, v: &Value) -> Range<usize> {
        char_range(model.rope(), v, self.encoding)
    }

    pub fn completion_context(trigger: Option<char>) -> Value {
        match trigger {
            Some(c) => json!({ "context": { "triggerKind": 2, "triggerCharacter": c.to_string() } }),
            None => json!({ "context": { "triggerKind": 1 } }),
        }
    }

    pub fn hover(&self, model: &TextModel, v: &Value) -> Option<Hover> {
        if v.is_null() {
            return None;
        }
        let contents = match &v["contents"] {
            Value::Array(items) => items.iter().map(marked_string).collect(),
            other => vec![marked_string(other)],
        };
        let range = v.get("range").filter(|r| !r.is_null()).map(|r| self.char_range(model, r));
        Some(Hover { contents, range })
    }

    pub fn completion(&self, model: &TextModel, v: &Value) -> Vec<CompletionItem> {
        let items = v.get("items").unwrap_or(v).as_array().map(Vec::as_slice).unwrap_or(&[]);
        items
            .iter()
            .map(|item| {
                let label = item["label"].as_str().unwrap_or("").to_owned();
                let edit = &item["textEdit"];
                let (insert, range) = if edit.is_object() {
                    let r = edit.get("range").or_else(|| edit.get("replace")).map(|r| self.char_range(model, r));
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
            .collect()
    }

    pub fn signature_help(&self, v: &Value) -> Option<SignatureHelp> {
        let sigs = v["signatures"].as_array().filter(|s| !s.is_empty())?;
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
                                    let units: Vec<u16> = label.encode_utf16().collect();
                                    let b = (r[1].as_u64().unwrap_or(0) as usize).min(units.len());
                                    let a = (r[0].as_u64().unwrap_or(0) as usize).min(b);
                                    String::from_utf16_lossy(&units[a..b])
                                }
                                _ => String::new(),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                (label, s.get("documentation").filter(|d| !d.is_null()).map(marked_string), params)
            })
            .collect();
        Some(SignatureHelp {
            signatures,
            active_signature: v["activeSignature"].as_u64().unwrap_or(0) as usize,
            active_parameter: v["activeParameter"].as_u64().unwrap_or(0) as usize,
        })
    }

    /// Locations or location links; char ranges for those in `doc`.
    pub fn locations(&self, doc: &Document, model: &TextModel, v: &Value) -> Vec<Location> {
        let list = match v {
            Value::Array(a) => a.clone(),
            Value::Null => Vec::new(),
            single => vec![single.clone()],
        };
        list.iter()
            .map(|l| {
                let uri = l["uri"].as_str().or_else(|| l["targetUri"].as_str()).unwrap_or("").to_owned();
                let lsp_range =
                    l.get("range").or_else(|| l.get("targetSelectionRange")).cloned().unwrap_or(Value::Null);
                let range = (uri == doc.uri).then(|| self.char_range(model, &lsp_range));
                Location { uri, range, lsp_range }
            })
            .collect()
    }

    pub fn ranges(&self, model: &TextModel, v: &Value) -> Vec<Range<usize>> {
        v.as_array().map(|a| a.iter().map(|h| self.char_range(model, &h["range"])).collect()).unwrap_or_default()
    }

    /// Edits from a `WorkspaceEdit`, those for `doc` in char offsets.
    pub fn workspace_edit(&self, doc: &Document, model: &TextModel, v: &Value) -> WorkspaceEdit {
        let mut out = WorkspaceEdit::default();
        let mut add = |uri: &str, edits: &[Value]| {
            if uri == doc.uri {
                out.local.extend(edits.iter().map(|e| TextEdit {
                    range: self.char_range(model, &e["range"]),
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

    pub fn text_edits(&self, model: &TextModel, v: &Value) -> Vec<TextEdit> {
        v.as_array()
            .map(|a| {
                a.iter()
                    .map(|e| TextEdit {
                        range: self.char_range(model, &e["range"]),
                        text: e["newText"].as_str().unwrap_or("").to_owned(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn code_actions(&self, doc: &Document, model: &TextModel, v: &Value) -> Vec<CodeAction> {
        v.as_array()
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
            .unwrap_or_default()
    }

    /// Params of `workspace/executeCommand` for a `Command`.
    pub fn execute_command_params(command: &Value) -> Value {
        json!({ "command": command["command"], "arguments": command.get("arguments").cloned().unwrap_or(json!([])) })
    }

    pub fn folding_ranges(&self, v: &Value) -> Vec<(usize, usize)> {
        v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|f| Some((f["startLine"].as_u64()? as usize, f["endLine"].as_u64()? as usize)))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The first position's ranges, innermost first.
    pub fn selection_ranges(&self, model: &TextModel, v: &Value) -> Vec<Range<usize>> {
        let mut out = Vec::new();
        let mut node = v.get(0);
        while let Some(n) = node.filter(|n| !n.is_null()) {
            out.push(self.char_range(model, &n["range"]));
            node = n.get("parent");
        }
        out
    }

    pub fn document_symbols(&self, model: &TextModel, v: &Value) -> Vec<Symbol> {
        fn convert(session: &Session, model: &TextModel, s: &Value) -> Symbol {
            // DocumentSymbol, or SymbolInformation with a location.
            let range = s.get("range").or_else(|| s["location"].get("range")).cloned().unwrap_or(Value::Null);
            let selection = s.get("selectionRange").cloned().unwrap_or_else(|| range.clone());
            Symbol {
                name: s["name"].as_str().unwrap_or("").to_owned(),
                kind: s["kind"].as_u64().unwrap_or(13),
                detail: s["detail"].as_str().map(str::to_owned),
                range: session.char_range(model, &range),
                selection_range: session.char_range(model, &selection),
                children: s["children"]
                    .as_array()
                    .map(|c| c.iter().map(|x| convert(session, model, x)).collect())
                    .unwrap_or_default(),
            }
        }
        v.as_array().map(|a| a.iter().map(|s| convert(self, model, s)).collect()).unwrap_or_default()
    }

    pub fn code_lens(&self, model: &TextModel, v: &Value) -> Vec<CodeLens> {
        v.as_array()
            .map(|a| {
                a.iter()
                    .map(|l| CodeLens {
                        range: self.char_range(model, &l["range"]),
                        title: l["command"]["title"].as_str().map(str::to_owned),
                        command: l.get("command").filter(|c| !c.is_null()).cloned(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Semantic tokens decoded with the server's legend.
    pub fn semantic_tokens(&self, model: &TextModel, v: &Value) -> Vec<SemanticToken> {
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
            let modifiers = (0..self.token_modifiers.len().min(64))
                .filter(|i| t[4] & (1 << i) != 0)
                .map(|i| self.token_modifiers[i].clone())
                .collect();
            out.push(SemanticToken { range, token_type, modifiers });
        }
        out
    }

    pub fn inlay_hints(&self, model: &TextModel, v: &Value) -> Vec<InlayHint> {
        v.as_array()
            .map(|a| {
                a.iter()
                    .map(|h| InlayHint {
                        offset: self.offset(model, &h["position"]),
                        label: match &h["label"] {
                            Value::String(s) => s.clone(),
                            Value::Array(parts) => parts.iter().filter_map(|p| p["value"].as_str()).collect(),
                            _ => String::new(),
                        },
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The diagnostics of a `publishDiagnostics` notification.
    pub fn diagnostics(&self, model: &TextModel, params: &Value) -> Vec<Diagnostic> {
        params["diagnostics"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|d| Diagnostic {
                        range: self.char_range(model, &d["range"]),
                        severity: match d["severity"].as_u64() {
                            Some(2) => Severity::Warning,
                            Some(3) => Severity::Information,
                            Some(4) => Severity::Hint,
                            _ => Severity::Error,
                        },
                        message: d["message"].as_str().unwrap_or("").to_owned(),
                        source: d["source"].as_str().map(str::to_owned),
                        code: match &d["code"] {
                            Value::String(s) => Some(s.clone()),
                            Value::Number(n) => Some(n.to_string()),
                            _ => None,
                        },
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn merge(params: &mut Value, extra: Value) {
    if let (Some(p), Value::Object(e)) = (params.as_object_mut(), extra) {
        p.extend(e);
    }
}

/// Text of a `MarkedString` or `MarkupContent`: code in a ``` fence, markdown as is.
pub(crate) fn marked_string(v: &Value) -> String {
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

/// What the client can do, as sent in `initialize`. Parsed from text: the `json!` form of
/// this object compiles to 17 KB of wasm.
fn client_capabilities() -> Value {
    serde_json::from_str(CLIENT_CAPABILITIES).unwrap_or(Value::Null)
}

const CLIENT_CAPABILITIES: &str = r#"{
  "general": { "positionEncodings": ["utf-8", "utf-16"] },
  "textDocument": {
    "synchronization": { "didSave": false, "dynamicRegistration": false },
    "hover": { "contentFormat": ["markdown", "plaintext"] },
    "completion": {
      "completionItem": { "snippetSupport": true, "documentationFormat": ["markdown", "plaintext"], "insertReplaceSupport": true },
      "contextSupport": true
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
      "formats": ["relative"]
    },
    "publishDiagnostics": {}
  },
  "workspace": { "applyEdit": true, "workspaceEdit": { "documentChanges": true }, "configuration": false },
  "window": { "workDoneProgress": false }
}"#;
