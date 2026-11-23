//! [`LanguageService`]: what a language service answers about documents, in the editor's
//! terms (char offsets into the document's text, the client's result types).
//!
//! One implementation runs three ways: inside the editor's process, as a language server in
//! a browser worker (compiled to wasm), or as a language server program over stdio. The last
//! two go through [`crate::LspServer`], which turns the protocol into calls of this trait.

use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rustaco_core::{EditEvent, TextModel};
use serde_json::Value;

use crate::{
    CodeAction, CodeLens, CompletionItem, Hover, InlayHint, Location, SemanticToken, SignatureHelp, Symbol, TextEdit,
    WorkspaceEdit,
};

/// Set when the client no longer wants the answer of the request being computed. Long
/// computations check it and return early; what they return then is dropped.
#[derive(Clone, Debug, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// The document a call is about.
pub struct Context<'a> {
    pub uri: &'a str,
    /// The language id the client opened the document with (`"rust"`, `"mermaid"`).
    pub language: &'a str,
    /// The document's text as of this call.
    pub model: &'a TextModel,
    pub cancel: &'a Cancel,
}

/// A problem in a document.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Diagnostic {
    pub range: Range<usize>,
    pub severity: Severity,
    pub message: String,
    /// Who reports it (`"mermaid"`, `"rustc"`).
    pub source: Option<String>,
    pub code: Option<String>,
}

impl Diagnostic {
    /// A diagnostic without source or code.
    pub fn new(range: Range<usize>, severity: Severity, message: impl Into<String>) -> Self {
        Self { range, severity, message: message.into(), source: None, code: None }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error = 1,
    Warning = 2,
    Information = 3,
    Hint = 4,
}

/// The requests a service answers. The server advertises exactly these; the editor asks
/// only for these.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct Features {
    /// `serverInfo.name`.
    pub name: String,
    /// Completion, opened automatically after these characters.
    pub completion: Option<Vec<char>>,
    pub hover: bool,
    /// Signature help, opened automatically after these characters.
    pub signature_help: Option<Vec<char>>,
    pub definition: bool,
    pub references: bool,
    pub document_highlights: bool,
    pub rename: bool,
    pub formatting: bool,
    pub code_actions: bool,
    /// Commands [`LanguageService::execute_command`] runs.
    pub commands: Vec<String>,
    pub code_lens: bool,
    /// Semantic tokens with this legend: token types, then modifiers.
    pub semantic_tokens: Option<(Vec<String>, Vec<String>)>,
    pub inlay_hints: bool,
    pub folding_ranges: bool,
    pub selection_ranges: bool,
    pub document_symbols: bool,
    /// [`LanguageService::diagnostics`] is asked after every change.
    pub diagnostics: bool,
    /// Methods outside the protocol that [`LanguageService::request`] answers, advertised
    /// under `experimental` in the capabilities (`"rustaco/highlight"`).
    pub methods: Vec<String>,
}

impl Features {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), ..Self::default() }
    }
}

/// A language service. Every request has a default that answers nothing, so a service
/// implements what it declares in [`LanguageService::features`] and nothing else.
///
/// Offsets are chars into `cx.model`'s text. Locations and edits in other documents carry
/// LSP's own coordinates ([`Location::lsp_range`], [`WorkspaceEdit::other`]), since the
/// service may not hold those documents.
#[allow(unused_variables)]
pub trait LanguageService {
    fn features(&self) -> Features;

    /// A document was opened; `cx.model` holds its text.
    fn open(&mut self, cx: &Context) {}

    /// `cx.model` changed by `edits`, in application order (char, byte, point and UTF-16
    /// positions of each, as incremental parsers want them).
    fn change(&mut self, cx: &Context, edits: &[EditEvent]) {}

    fn close(&mut self, uri: &str) {}

    fn diagnostics(&mut self, cx: &Context) -> Vec<Diagnostic> {
        Vec::new()
    }

    fn completion(&mut self, cx: &Context, at: usize, trigger: Option<char>) -> Vec<CompletionItem> {
        Vec::new()
    }

    fn hover(&mut self, cx: &Context, at: usize) -> Option<Hover> {
        None
    }

    fn signature_help(&mut self, cx: &Context, at: usize) -> Option<SignatureHelp> {
        None
    }

    fn definition(&mut self, cx: &Context, at: usize) -> Vec<Location> {
        Vec::new()
    }

    fn references(&mut self, cx: &Context, at: usize) -> Vec<Location> {
        Vec::new()
    }

    fn document_highlights(&mut self, cx: &Context, at: usize) -> Vec<Range<usize>> {
        Vec::new()
    }

    fn rename(&mut self, cx: &Context, at: usize, new_name: &str) -> Option<WorkspaceEdit> {
        None
    }

    fn formatting(&mut self, cx: &Context, tab_size: usize, insert_spaces: bool) -> Vec<TextEdit> {
        Vec::new()
    }

    fn code_actions(&mut self, cx: &Context, range: Range<usize>) -> Vec<CodeAction> {
        Vec::new()
    }

    /// Runs one of [`Features::commands`]; the result goes back to the client as is.
    fn execute_command(&mut self, command: &str, arguments: &[Value]) -> Value {
        Value::Null
    }

    fn code_lens(&mut self, cx: &Context) -> Vec<CodeLens> {
        Vec::new()
    }

    /// Tokens of the whole document, types and modifiers named from the legend.
    fn semantic_tokens(&mut self, cx: &Context) -> Vec<SemanticToken> {
        Vec::new()
    }

    fn inlay_hints(&mut self, cx: &Context, range: Range<usize>) -> Vec<InlayHint> {
        Vec::new()
    }

    /// Folding regions as (first line, last line).
    fn folding_ranges(&mut self, cx: &Context) -> Vec<(usize, usize)> {
        Vec::new()
    }

    /// Ranges around `at`, innermost first.
    fn selection_ranges(&mut self, cx: &Context, at: usize) -> Vec<Range<usize>> {
        Vec::new()
    }

    fn document_symbols(&mut self, cx: &Context) -> Vec<Symbol> {
        Vec::new()
    }

    /// A request outside the protocol (one of [`Features::methods`]) about the document in
    /// `params.textDocument`. The params and the answer are the service's own JSON; None
    /// answers "method not found".
    fn request(&mut self, cx: &Context, method: &str, params: &Value) -> Option<Value> {
        None
    }
}
