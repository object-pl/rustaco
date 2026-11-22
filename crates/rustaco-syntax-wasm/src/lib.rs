//! A language module for the browser editor.
//!
//! Each build enables one language pack through cargo features and becomes its own WASM
//! file, loaded the first time a model uses one of its languages. A pack uses tree-sitter
//! grammars, TextMate grammars (the `textmate` pack) or the Mermaid tokenizer (the `mermaid`
//! pack); the interface is the same.
//!
//! The module offers two doors to the same engines:
//!
//! - [`LanguageServer`]: a language server without I/O. The worker hands it JSON-RPC
//!   messages and posts back what it answers; documents arrive with `didOpen` and
//!   `didChange`, highlights through `rustaco/highlight`, the rest through the standard
//!   requests (see `rustaco_syntax::SyntaxService`). [`LanguageServer::idle`] continues
//!   parsing between messages.
//! - [`Highlighter`]: the same engine called directly, for pages without workers, where the
//!   editor parses on the main thread in time slices.

use rustaco_core::{EditEvent, Point, Rope, Utf16Position};
use rustaco_lsp::LspServer;
use rustaco_syntax::{Engine, SyntaxService};
use serde_json::json;
use wasm_bindgen::prelude::*;

/// Numbers per edit in [`Highlighter::edit`]: start, old end and new end in bytes, then
/// row and byte column of the start, old end and new end.
pub const EDIT_STRIDE: usize = 9;

fn now() -> f64 {
    js_sys::Date::now()
}

/// The pack's languages as a language server, fed with JSON-RPC text.
#[wasm_bindgen]
pub struct LanguageServer {
    server: LspServer<SyntaxService>,
}

#[wasm_bindgen]
impl LanguageServer {
    #[wasm_bindgen(constructor)]
    pub fn new() -> LanguageServer {
        LanguageServer { server: LspServer::new(SyntaxService::new(now)) }
    }

    /// Takes one message and returns the messages to send back.
    pub fn handle(&mut self, message: &str) -> Vec<String> {
        self.server.handle_json(message)
    }

    /// Parses pending documents for at most `budget_ms` milliseconds. Returns a
    /// `rustaco/parsed` notification for each document whose parse caught up.
    pub fn idle(&mut self, budget_ms: f64) -> Vec<String> {
        let deadline = now() + budget_ms;
        let mut done = Vec::new();
        self.server.each_document(|service, cx| {
            if now() < deadline && service.idle(cx, deadline) {
                done.push(cx.uri.to_owned());
            }
        });
        for uri in done {
            self.server.notify("rustaco/parsed", json!({ "textDocument": { "uri": uri } }));
        }
        self.server.take_output().iter().map(ToString::to_string).collect()
    }

    /// True while a document waits for a parse.
    pub fn busy(&self) -> bool {
        self.server.service().busy()
    }
}

impl Default for LanguageServer {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
pub struct Highlighter {
    rope: Rope,
    engine: Engine,
}

#[wasm_bindgen]
impl Highlighter {
    #[wasm_bindgen(constructor)]
    pub fn new(language: &str, text: &str) -> Result<Highlighter, JsValue> {
        let engine = Engine::new(language).map_err(|e| JsValue::from_str(&e))?;
        Ok(Highlighter { rope: Rope::from_str(text), engine })
    }

    /// Ids of the languages this module parses.
    pub fn languages() -> Vec<String> {
        Engine::languages()
    }

    /// Replaces the text; the next [`Highlighter::parse`] starts from scratch.
    pub fn reset(&mut self, text: &str) {
        self.rope = Rope::from_str(text);
        self.engine.reset();
    }

    /// Parses for at most `budget_ms` milliseconds. Returns true when the result matches the
    /// text; false means call again (on a later frame) to continue.
    pub fn parse(&mut self, budget_ms: f64) -> bool {
        let deadline = now() + budget_ms;
        self.engine.parse_with_budget(&self.rope, || now() < deadline)
    }

    /// Applies edits in order; they take effect in highlighting after the next
    /// [`Highlighter::parse`] completes. `numbers` holds [`EDIT_STRIDE`] values per
    /// edit; `text` is the inserted texts concatenated, each as long as its edit's
    /// `new_end - start` bytes.
    pub fn edit(&mut self, numbers: &[u32], text: &str) -> Result<(), JsValue> {
        let mut offset = 0;
        for e in numbers.chunks_exact(EDIT_STRIDE) {
            let [start_byte, old_end_byte, new_end_byte] = [e[0], e[1], e[2]].map(|n| n as usize);
            let len = self.rope.len_bytes();
            if start_byte > old_end_byte || old_end_byte > len || new_end_byte < start_byte {
                return Err(JsValue::from_str("edit outside the text; call reset"));
            }
            let inserted = text
                .get(offset..offset + (new_end_byte - start_byte))
                .ok_or_else(|| JsValue::from_str("edit text does not match its byte range"))?;
            offset += inserted.len();
            let start = self.rope.byte_to_char(start_byte);
            let old_end = self.rope.byte_to_char(old_end_byte);
            self.rope.remove(start..old_end);
            self.rope.insert(start, inserted);
            let point = |row: u32, column: u32| Point { row: row as usize, column: column as usize };
            self.engine.apply_edit(&EditEvent {
                start,
                old_end,
                new_end: start + inserted.chars().count(),
                start_byte,
                old_end_byte,
                new_end_byte,
                start_point: point(e[3], e[4]),
                old_end_point: point(e[5], e[6]),
                new_end_point: point(e[7], e[8]),
                start_utf16: Utf16Position::default(),
                old_end_utf16: Utf16Position::default(),
                text: String::new(),
            });
        }
        Ok(())
    }

    /// The bracket pair at char `pos` (the bracket before the cursor or at it), as
    /// [open, close] char positions; empty when there is none or the engine has no tree.
    #[wasm_bindgen(js_name = matchingBracket)]
    pub fn matching_bracket(&self, pos: u32) -> Vec<u32> {
        self.engine.matching_bracket(&self.rope, pos as usize).map_or_else(Vec::new, |(a, b)| vec![a as u32, b as u32])
    }

    /// Regions worth folding from the syntax tree, as pairs of header line and last hidden
    /// line; empty when the engine has no tree.
    #[wasm_bindgen(js_name = foldingRanges)]
    pub fn folding_ranges(&self) -> Vec<u32> {
        self.engine.folding_ranges().into_iter().flat_map(|(a, b)| [a as u32, b as u32]).collect()
    }

    /// Declarations for an outline, six numbers each: start and end of the declaration,
    /// start and end of its name (char offsets), kind (Monaco's `SymbolKind`) and nesting
    /// depth. Empty when the engine has no tree.
    #[wasm_bindgen(js_name = documentSymbols)]
    pub fn document_symbols(&self) -> Vec<u32> {
        self.engine
            .symbols(&self.rope)
            .into_iter()
            .flat_map(|s| {
                [
                    s.range.start as u32,
                    s.range.end as u32,
                    s.name.start as u32,
                    s.name.end as u32,
                    s.kind,
                    s.depth as u32,
                ]
            })
            .collect()
    }

    /// True when this module answers bracket questions from a syntax tree.
    #[wasm_bindgen(js_name = hasTree)]
    pub fn has_tree(&self) -> bool {
        self.engine.has_tree()
    }

    /// Highlights lines `first..end`, as quads: line, start column, end column (chars),
    /// style id. With `bracket_colors`, bracket tokens get depth colours on top.
    pub fn highlight(&self, first: u32, end: u32, bracket_colors: bool) -> Vec<u32> {
        self.engine.highlight_quads(&self.rope, first as usize, end as usize, bracket_colors)
    }

    /// Char ranges of the syntax nodes around `start..end`, innermost first, as pairs
    /// (start, end). Empty for engines without a tree (TextMate).
    #[wasm_bindgen(js_name = enclosingRanges)]
    pub fn enclosing_ranges(&self, start: u32, end: u32) -> Vec<u32> {
        self.engine
            .enclosing_ranges(&self.rope, start as usize..end as usize)
            .into_iter()
            .flat_map(|r| [r.start as u32, r.end as u32])
            .collect()
    }

    /// 0 for code, 1 inside a string, 2 inside a comment, at char `pos`.
    #[wasm_bindgen(js_name = contextAt)]
    pub fn context_at(&self, pos: u32) -> u8 {
        self.engine.context_at(&self.rope, pos as usize) as u8
    }

    /// The module's copy of the text; for tests that check it stays in step with the editor.
    pub fn text(&self) -> String {
        self.rope.to_string()
    }
}
