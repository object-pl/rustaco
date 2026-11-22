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
mod session;

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
pub use session::{Document, Session};

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

/// An initialized connection to one language server: a [`Session`] over a blocking
/// [`Connection`].
pub struct LspClient {
    conn: Connection,
    session: Session,
    /// How long a request may take.
    pub timeout: Duration,
}

impl LspClient {
    /// A client on a connection; call [`LspClient::initialize`] next.
    pub fn new(conn: Connection) -> Self {
        Self { conn, session: Session::new(), timeout: Duration::from_secs(10) }
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

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn capabilities(&self) -> &Value {
        self.session.capabilities()
    }

    /// `{ name, version }` the server reported, if any.
    pub fn server_info(&self) -> &Value {
        self.session.server_info()
    }

    pub fn encoding(&self) -> PositionEncoding {
        self.session.encoding()
    }

    /// Whether the server offers `provider` (`"hoverProvider"`, `"renameProvider"`, ...).
    pub fn supports(&self, provider: &str) -> bool {
        self.session.supports(provider)
    }

    /// The handshake: `initialize` with the client's capabilities, then `initialized`.
    pub fn initialize(&mut self, root_uri: Option<&str>) -> Result<(), RpcError> {
        let params = Session::initialize_params(root_uri, Some(std::process::id()));
        let result = self.conn.request("initialize", params, Duration::from_secs(60))?;
        self.session.initialized(&result);
        self.conn.notify("initialized", json!({}))
    }

    /// `shutdown`, then `exit`.
    pub fn shutdown(&self) {
        let _ = self.conn.request("shutdown", Value::Null, Duration::from_secs(5));
        let _ = self.conn.notify("exit", Value::Null);
    }

    /// Opens `model` on the server as `uri`.
    pub fn open(&self, uri: &str, language_id: &str, model: &TextModel) -> Result<Document, RpcError> {
        let (doc, params) = self.session.open(uri, language_id, model);
        self.conn.notify("textDocument/didOpen", params)?;
        Ok(doc)
    }

    /// Sends the model's edits since the last sync.
    pub fn sync(&self, doc: &mut Document, model: &TextModel) -> Result<(), RpcError> {
        match self.session.change(doc, model) {
            Some(params) => self.conn.notify("textDocument/didChange", params),
            None => Ok(()),
        }
    }

    pub fn close(&self, doc: Document) -> Result<(), RpcError> {
        self.conn.notify("textDocument/didClose", Session::close_params(&doc))
    }

    /// Syncs, then asks `method` about `at` with `extra` params.
    fn at(
        &self,
        method: &str,
        doc: &mut Document,
        model: &TextModel,
        at: usize,
        extra: Value,
    ) -> Result<Value, RpcError> {
        self.sync(doc, model)?;
        self.conn.request(method, self.session.at_params(doc, model, at, extra), self.timeout)
    }

    /// Syncs, then asks `method` about the whole document with `extra` params.
    fn whole(&self, method: &str, doc: &mut Document, model: &TextModel, extra: Value) -> Result<Value, RpcError> {
        self.sync(doc, model)?;
        self.conn.request(method, self.session.doc_params(doc, extra), self.timeout)
    }

    pub fn hover(&self, doc: &mut Document, model: &TextModel, at: usize) -> Result<Option<Hover>, RpcError> {
        let v = self.at("textDocument/hover", doc, model, at, json!({}))?;
        Ok(self.session.hover(model, &v))
    }

    pub fn completion(
        &self,
        doc: &mut Document,
        model: &TextModel,
        at: usize,
        trigger: Option<char>,
    ) -> Result<Vec<CompletionItem>, RpcError> {
        let v = self.at("textDocument/completion", doc, model, at, Session::completion_context(trigger))?;
        Ok(self.session.completion(model, &v))
    }

    pub fn signature_help(
        &self,
        doc: &mut Document,
        model: &TextModel,
        at: usize,
    ) -> Result<Option<SignatureHelp>, RpcError> {
        let v = self.at("textDocument/signatureHelp", doc, model, at, json!({}))?;
        Ok(self.session.signature_help(&v))
    }

    pub fn definition(&self, doc: &mut Document, model: &TextModel, at: usize) -> Result<Vec<Location>, RpcError> {
        let v = self.at("textDocument/definition", doc, model, at, json!({}))?;
        Ok(self.session.locations(doc, model, &v))
    }

    pub fn references(&self, doc: &mut Document, model: &TextModel, at: usize) -> Result<Vec<Location>, RpcError> {
        let extra = json!({ "context": { "includeDeclaration": true } });
        let v = self.at("textDocument/references", doc, model, at, extra)?;
        Ok(self.session.locations(doc, model, &v))
    }

    pub fn document_highlights(
        &self,
        doc: &mut Document,
        model: &TextModel,
        at: usize,
    ) -> Result<Vec<Range<usize>>, RpcError> {
        let v = self.at("textDocument/documentHighlight", doc, model, at, json!({}))?;
        Ok(self.session.ranges(model, &v))
    }

    /// Edits from a `WorkspaceEdit`, those for `doc` in char offsets.
    pub fn workspace_edit(&self, doc: &Document, model: &TextModel, v: &Value) -> WorkspaceEdit {
        self.session.workspace_edit(doc, model, v)
    }

    pub fn rename(
        &self,
        doc: &mut Document,
        model: &TextModel,
        at: usize,
        new_name: &str,
    ) -> Result<WorkspaceEdit, RpcError> {
        let v = self.at("textDocument/rename", doc, model, at, json!({ "newName": new_name }))?;
        Ok(self.session.workspace_edit(doc, model, &v))
    }

    pub fn formatting(
        &self,
        doc: &mut Document,
        model: &TextModel,
        tab_size: usize,
        insert_spaces: bool,
    ) -> Result<Vec<TextEdit>, RpcError> {
        let extra = json!({ "options": { "tabSize": tab_size, "insertSpaces": insert_spaces } });
        let v = self.whole("textDocument/formatting", doc, model, extra)?;
        Ok(self.session.text_edits(model, &v))
    }

    pub fn code_actions(
        &self,
        doc: &mut Document,
        model: &TextModel,
        range: Range<usize>,
    ) -> Result<Vec<CodeAction>, RpcError> {
        let extra = json!({ "range": self.session.range(model, range), "context": { "diagnostics": [] } });
        let v = self.whole("textDocument/codeAction", doc, model, extra)?;
        Ok(self.session.code_actions(doc, model, &v))
    }

    pub fn execute_command(&self, command: &Value) -> Result<Value, RpcError> {
        self.conn.request("workspace/executeCommand", Session::execute_command_params(command), self.timeout)
    }

    /// Folding regions as (first line, last line).
    pub fn folding_ranges(&self, doc: &mut Document, model: &TextModel) -> Result<Vec<(usize, usize)>, RpcError> {
        let v = self.whole("textDocument/foldingRange", doc, model, json!({}))?;
        Ok(self.session.folding_ranges(&v))
    }

    /// Ranges around `at`, innermost first.
    pub fn selection_ranges(
        &self,
        doc: &mut Document,
        model: &TextModel,
        at: usize,
    ) -> Result<Vec<Range<usize>>, RpcError> {
        let extra = json!({ "positions": [self.session.position(model, at)] });
        let v = self.whole("textDocument/selectionRange", doc, model, extra)?;
        Ok(self.session.selection_ranges(model, &v))
    }

    pub fn document_symbols(&self, doc: &mut Document, model: &TextModel) -> Result<Vec<Symbol>, RpcError> {
        let v = self.whole("textDocument/documentSymbol", doc, model, json!({}))?;
        Ok(self.session.document_symbols(model, &v))
    }

    pub fn code_lens(&self, doc: &mut Document, model: &TextModel) -> Result<Vec<CodeLens>, RpcError> {
        let v = self.whole("textDocument/codeLens", doc, model, json!({}))?;
        Ok(self.session.code_lens(model, &v))
    }

    /// The whole document's semantic tokens, decoded with the server's legend.
    pub fn semantic_tokens(&self, doc: &mut Document, model: &TextModel) -> Result<Vec<SemanticToken>, RpcError> {
        let v = self.whole("textDocument/semanticTokens/full", doc, model, json!({}))?;
        Ok(self.session.semantic_tokens(model, &v))
    }

    pub fn inlay_hints(
        &self,
        doc: &mut Document,
        model: &TextModel,
        range: Range<usize>,
    ) -> Result<Vec<InlayHint>, RpcError> {
        let extra = json!({ "range": self.session.range(model, range) });
        let v = self.whole("textDocument/inlayHint", doc, model, extra)?;
        Ok(self.session.inlay_hints(model, &v))
    }
}
