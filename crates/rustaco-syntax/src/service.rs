//! [`SyntaxService`]: the bundled languages as a [`LanguageService`], so the editor gets
//! them through the same protocol as any language server.
//!
//! Standard requests: folding and selection ranges, document symbols, completion (keywords
//! and the names of the file), definition, references and rename within the file, all from
//! the tree-sitter tree; completion of keywords, directions, shapes and node ids for Mermaid.
//! Two methods outside the protocol serve the editor's frame:
//!
//! - `rustaco/highlight` takes `{ textDocument, lines: [first, end, ...], colorize, caret,
//!   full }` and answers `{ segments: [[first, end, quads]], bracket, parsed }`, where quads
//!   are (line, start column, end column, style id) in chars and `bracket` is the pair at
//!   char offset `caret`. Without `full` a pending parse gets a few milliseconds and the
//!   answer comes from the last tree moved by the edits since.
//! - `rustaco/status` answers `{ ok, tree }`: whether the document's language has an
//!   engine here and whether it has a syntax tree.
//!
//! Parsing between requests is the host's job: [`SyntaxService::idle`] continues it for a
//! time budget, and a host notifies the client (`rustaco/parsed`) when it catches up.

use std::collections::HashMap;
use std::ops::Range;

use rustaco_core::{EditEvent, Rope};
use rustaco_lsp::{
    CompletionItem, Context, Features, JsonValue as Value, LanguageService, Location, Symbol, TextEdit, WorkspaceEdit,
};
use serde_json::json;

use crate::engines::{CharSymbol, Engine};

/// How long a highlight request may parse before answering from the old tree.
const FRAME_PARSE_MS: f64 = 4.0;

/// The service. `clock` returns milliseconds from any fixed point; it times parse slices.
pub struct SyntaxService {
    docs: HashMap<String, Option<Engine>>,
    clock: fn() -> f64,
    #[cfg(feature = "engine")]
    keywords: HashMap<&'static str, Vec<String>>,
}

impl SyntaxService {
    pub fn new(clock: fn() -> f64) -> Self {
        Self {
            docs: HashMap::new(),
            clock,
            #[cfg(feature = "engine")]
            keywords: HashMap::new(),
        }
    }

    /// Parses `cx`'s document until `deadline` (on the service's clock). Returns true when
    /// this call finished a parse that was pending.
    pub fn idle(&mut self, cx: &Context, deadline: f64) -> bool {
        let clock = self.clock;
        let Some(Some(engine)) = self.docs.get_mut(cx.uri) else { return false };
        engine.needs_parse() && engine.parse_with_budget(cx.model.rope(), || clock() < deadline)
    }

    /// True while some document waits for a parse.
    pub fn busy(&self) -> bool {
        self.docs.values().flatten().any(Engine::needs_parse)
    }

    /// The document's engine with its parse finished.
    fn parsed(&mut self, cx: &Context) -> Option<&Engine> {
        let engine = self.docs.get_mut(cx.uri)?.as_mut()?;
        // A cancelled parse leaves a tree that no longer matches the text; nothing may read
        // it against the new text (the answer is dropped anyway).
        engine.parse_with_budget(cx.model.rope(), || !cx.cancel.is_cancelled()).then_some(&*engine)
    }

    fn highlight(&mut self, cx: &Context, params: &Value) -> Value {
        let clock = self.clock;
        let Some(Some(engine)) = self.docs.get_mut(cx.uri) else {
            return json!({ "segments": [], "bracket": [], "parsed": true });
        };
        let rope = cx.model.rope();
        if params["full"].as_bool() == Some(true) {
            engine.parse_with_budget(rope, || true);
        } else if engine.needs_parse() {
            let deadline = clock() + FRAME_PARSE_MS;
            engine.parse_with_budget(rope, || clock() < deadline);
        }
        let colorize = params["colorize"].as_bool() == Some(true);
        let lines: Vec<usize> = params["lines"]
            .as_array()
            .map(|a| a.iter().filter_map(|n| n.as_u64()).map(|n| n as usize).collect())
            .unwrap_or_default();
        let segments: Vec<Value> = lines
            .chunks_exact(2)
            .map(|r| json!([r[0], r[1], engine.highlight_quads(rope, r[0], r[1], colorize)]))
            .collect();
        let bracket = params["caret"]
            .as_u64()
            .and_then(|c| engine.matching_bracket(rope, c as usize))
            .map_or_else(Vec::new, |(a, b)| vec![a, b]);
        json!({ "segments": segments, "bracket": bracket, "parsed": !engine.needs_parse() })
    }
}

/// An outline tree from symbols in document order with their depths.
fn nest(rope: &Rope, flat: Vec<CharSymbol>) -> Vec<Symbol> {
    fn take(rope: &Rope, flat: &[CharSymbol], i: &mut usize, depth: usize) -> Vec<Symbol> {
        let mut out = Vec::new();
        while *i < flat.len() && flat[*i].depth >= depth {
            let s = &flat[*i];
            *i += 1;
            let children = take(rope, flat, i, s.depth + 1);
            let mut symbol =
                Symbol::new(rope.slice(s.name.clone()).to_string(), s.kind as u64 + 1, s.range.clone(), s.name.clone());
            symbol.children = children;
            out.push(symbol);
        }
        out
    }
    take(rope, &flat, &mut 0, 0)
}

fn item(label: &str, kind: u64, sort: String) -> CompletionItem {
    let mut item = CompletionItem::new(label);
    item.kind = Some(kind);
    item.sort_text = Some(sort);
    item
}

/// LSP's `CompletionItemKind`s used here.
const KIND_VARIABLE: u64 = 6;
const KIND_KEYWORD: u64 = 14;
const KIND_SNIPPET: u64 = 15;
const KIND_ENUM_MEMBER: u64 = 20;

/// `text` as literal snippet text: `\`, `$` and `}` escaped.
fn snippet_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '$' | '}') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Completion in a Mermaid document.
fn mermaid_completion(rope: &Rope, at: usize) -> Vec<CompletionItem> {
    use crate::mermaid::{DIAGRAMS, DIRECTIONS, KEYWORDS, SHAPES, diagram, node_ids};
    let line = rope.char_to_line(at);
    let line_start = rope.line_to_char(line);
    let before: String = rope.slice(line_start..at).to_string();
    // Right after a shape's opening bracket: the shapes, each replacing that bracket.
    if let Some(open) = before.chars().last().filter(|c| matches!(c, '[' | '(' | '{' | '>')) {
        let id_before = before.chars().rev().nth(1).is_some_and(|c| c.is_alphanumeric() || c == '_');
        if id_before {
            return SHAPES
                .iter()
                .filter(|(o, _, _)| o.starts_with(open))
                .enumerate()
                .map(|(i, (o, c, name))| {
                    let mut item = CompletionItem::new(format!("{o}text{c}"));
                    item.kind = Some(KIND_SNIPPET);
                    item.detail = Some((*name).into());
                    item.insert_text = format!("{}${{1:text}}{}$0", snippet_text(o), snippet_text(c));
                    item.is_snippet = true;
                    item.range = Some(at - 1..at);
                    item.sort_text = Some(format!("{i:02}"));
                    item.filter_text = Some(open.to_string());
                    item
                })
                .collect();
        }
        return Vec::new();
    }
    let words: Vec<&str> = before.split_whitespace().collect();
    let typing_first = words.len() <= 1 && !before.ends_with(char::is_whitespace);
    let kind = diagram(rope);
    if kind.is_none() && (words.is_empty() || typing_first) {
        return DIAGRAMS.iter().enumerate().map(|(i, d)| item(d, KIND_KEYWORD, format!("{i:02}"))).collect();
    }
    let previous = if before.ends_with(char::is_whitespace) { words.last() } else { words.iter().rev().nth(1) };
    if previous.is_some_and(|w| matches!(*w, "graph" | "flowchart" | "direction")) {
        return DIRECTIONS.iter().enumerate().map(|(i, d)| item(d, KIND_ENUM_MEMBER, format!("{i:02}"))).collect();
    }
    let mut out: Vec<CompletionItem> = Vec::new();
    let current: String = before.chars().rev().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    let current: String = current.chars().rev().collect();
    for (i, id) in node_ids(rope).iter().enumerate() {
        if *id != current {
            out.push(item(id, KIND_VARIABLE, format!("0{i:04}")));
        }
    }
    for k in KEYWORDS {
        out.push(item(k, KIND_KEYWORD, format!("1{k}")));
    }
    out
}

impl LanguageService for SyntaxService {
    fn features(&self) -> Features {
        let mut f = Features::new("rustaco-syntax");
        f.methods = vec!["rustaco/highlight".into(), "rustaco/status".into()];
        if cfg!(feature = "engine") {
            f.completion = Some(Vec::new());
            f.definition = true;
            f.references = true;
            f.rename = true;
            f.folding_ranges = true;
            f.selection_ranges = true;
            f.document_symbols = true;
        } else if !cfg!(feature = "textmate") {
            // Only Mermaid: its shapes open after a node id.
            f.completion = Some(vec!['[', '(', '{']);
        }
        f
    }

    fn open(&mut self, cx: &Context) {
        self.docs.insert(cx.uri.to_owned(), Engine::new(cx.language).ok());
    }

    fn change(&mut self, cx: &Context, edits: &[EditEvent]) {
        if let Some(Some(engine)) = self.docs.get_mut(cx.uri) {
            for e in edits {
                engine.apply_edit(e);
            }
        }
    }

    fn close(&mut self, uri: &str) {
        self.docs.remove(uri);
    }

    fn folding_ranges(&mut self, cx: &Context) -> Vec<(usize, usize)> {
        self.parsed(cx).map(Engine::folding_ranges).unwrap_or_default()
    }

    fn selection_ranges(&mut self, cx: &Context, at: usize) -> Vec<Range<usize>> {
        let rope = cx.model.rope();
        self.parsed(cx).map(|e| e.enclosing_ranges(rope, at..at)).unwrap_or_default()
    }

    fn document_symbols(&mut self, cx: &Context) -> Vec<Symbol> {
        let rope = cx.model.rope();
        let flat = self.parsed(cx).map(|e| e.symbols(rope)).unwrap_or_default();
        nest(rope, flat)
    }

    fn completion(&mut self, cx: &Context, at: usize, _trigger: Option<char>) -> Vec<CompletionItem> {
        let rope = cx.model.rope();
        if let Some(Some(Engine::Mermaid)) = self.docs.get(cx.uri) {
            return mermaid_completion(rope, at);
        }
        #[cfg(feature = "engine")]
        {
            let byte = rope.char_to_byte(at.min(rope.len_chars()));
            let (def, names) = {
                let Some(syntax) = self.parsed(cx).and_then(Engine::syntax) else { return Vec::new() };
                if syntax.context_at(byte) != crate::TextContext::Code {
                    return Vec::new();
                }
                let names = syntax.tree().map(|t| crate::locals::completion_names(t, rope, byte)).unwrap_or_default();
                (syntax.language(), names)
            };
            let mut out: Vec<CompletionItem> = names
                .iter()
                .take(5000)
                .enumerate()
                .map(|(i, (n, _))| item(n, KIND_VARIABLE, format!("0{i:05}")))
                .collect();
            let keywords = self.keywords.entry(def.id).or_insert_with(|| crate::locals::keywords(&def.grammar()));
            for k in keywords.iter() {
                if !names.iter().any(|(n, _)| n == k) {
                    out.push(item(k, KIND_KEYWORD, format!("1{k}")));
                }
            }
            out
        }
        #[cfg(not(feature = "engine"))]
        Vec::new()
    }

    fn definition(&mut self, cx: &Context, at: usize) -> Vec<Location> {
        #[cfg(feature = "engine")]
        {
            let rope = cx.model.rope();
            let Some(syntax) = self.parsed(cx).and_then(Engine::syntax) else { return Vec::new() };
            let Some(tree) = syntax.tree() else { return Vec::new() };
            let byte = rope.char_to_byte(at.min(rope.len_chars()));
            crate::locals::definition(tree, rope, byte)
                .map(|r| Location::local(cx.uri, rope.byte_to_char(r.start)..rope.byte_to_char(r.end)))
                .into_iter()
                .collect()
        }
        #[cfg(not(feature = "engine"))]
        {
            let _ = (cx, at);
            Vec::new()
        }
    }

    fn references(&mut self, cx: &Context, at: usize) -> Vec<Location> {
        self.uses(cx, at).into_iter().map(|r| Location::local(cx.uri, r)).collect()
    }

    fn rename(&mut self, cx: &Context, at: usize, new_name: &str) -> Option<WorkspaceEdit> {
        #[cfg(feature = "engine")]
        {
            if !crate::locals::valid_name(new_name) {
                return None;
            }
            // A keyword would not parse as a name; a shorthand field (`C { total }`) would
            // rename the field along with the local.
            let syntax = self.parsed(cx).and_then(Engine::syntax)?;
            let def = syntax.language();
            let rope = cx.model.rope();
            let byte = rope.char_to_byte(at.min(rope.len_chars()));
            if syntax.tree().is_some_and(|t| crate::locals::in_shorthand(t, rope, byte)) {
                return None;
            }
            let keywords = self.keywords.entry(def.id).or_insert_with(|| crate::locals::keywords(&def.grammar()));
            if keywords.iter().any(|k| k == new_name) {
                return None;
            }
        }
        let uses = self.uses(cx, at);
        if uses.is_empty() {
            return None;
        }
        Some(WorkspaceEdit::local(uses.into_iter().map(|range| TextEdit::new(range, new_name)).collect()))
    }

    fn request(&mut self, cx: &Context, method: &str, params: &Value) -> Option<Value> {
        match method {
            "rustaco/highlight" => Some(self.highlight(cx, params)),
            "rustaco/status" => {
                let engine = self.docs.get(cx.uri).and_then(Option::as_ref);
                Some(json!({ "ok": engine.is_some(), "tree": engine.is_some_and(Engine::has_tree) }))
            }
            _ => None,
        }
    }
}

impl SyntaxService {
    /// The uses of the name at `at`, in char ranges.
    fn uses(&mut self, cx: &Context, at: usize) -> Vec<Range<usize>> {
        #[cfg(feature = "engine")]
        {
            let rope = cx.model.rope();
            let Some(syntax) = self.parsed(cx).and_then(Engine::syntax) else { return Vec::new() };
            let Some(tree) = syntax.tree() else { return Vec::new() };
            let byte = rope.char_to_byte(at.min(rope.len_chars()));
            crate::locals::references(tree, rope, byte)
                .into_iter()
                .map(|r| rope.byte_to_char(r.start)..rope.byte_to_char(r.end))
                .collect()
        }
        #[cfg(not(feature = "engine"))]
        {
            let _ = (cx, at);
            Vec::new()
        }
    }
}

#[cfg(all(test, feature = "rust"))]
mod tests {
    use rustaco_lsp::LspServer;
    use serde_json::json;

    use super::*;

    fn clock() -> f64 {
        0.0
    }

    fn server() -> LspServer<SyntaxService> {
        let mut s = LspServer::new(SyntaxService::new(clock));
        s.handle(json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": { "capabilities": {} } }));
        s
    }

    fn ask(s: &mut LspServer<SyntaxService>, method: &str, params: Value) -> Value {
        let out = s.handle(json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }));
        out.into_iter().find(|m| m["id"] == 1).map(|m| m["result"].clone()).unwrap_or(Value::Null)
    }

    #[test]
    fn highlight_status_and_names_through_the_protocol() {
        let mut s = server();
        let uri = "mem://a.rs";
        let text = "fn main() {\n    let x = 1;\n    x + 1;\n}\n";
        s.handle(json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen",
            "params": { "textDocument": { "uri": uri, "languageId": "rust", "version": 1, "text": text } } }));
        let doc = json!({ "uri": uri });
        assert_eq!(ask(&mut s, "rustaco/status", json!({ "textDocument": doc })), json!({ "ok": true, "tree": true }));
        let h = ask(
            &mut s,
            "rustaco/highlight",
            json!({ "textDocument": doc, "lines": [0, 4], "caret": 10, "full": true }),
        );
        assert_eq!(h["parsed"], true);
        assert_eq!(h["bracket"], json!([10, 38]));
        let quads = h["segments"][0][2].as_array().unwrap();
        assert!(quads.len() >= 8 && quads.len() % 4 == 0);
        // Rename `x` from its use on line 2.
        let edit = ask(
            &mut s,
            "textDocument/rename",
            json!({ "textDocument": doc, "position": { "line": 2, "character": 4 }, "newName": "count" }),
        );
        assert_eq!(edit["changes"][uri].as_array().unwrap().len(), 2);
        let bad = ask(
            &mut s,
            "textDocument/rename",
            json!({ "textDocument": doc, "position": { "line": 2, "character": 4 }, "newName": "no way" }),
        );
        assert_eq!(bad, Value::Null);
        let keyword = ask(
            &mut s,
            "textDocument/rename",
            json!({ "textDocument": doc, "position": { "line": 2, "character": 4 }, "newName": "match" }),
        );
        assert_eq!(keyword, Value::Null);
        let symbols = ask(&mut s, "textDocument/documentSymbol", json!({ "textDocument": doc }));
        assert_eq!(symbols[0]["name"], "main");
        assert_eq!(symbols[0]["kind"], 12);
        let items = ask(
            &mut s,
            "textDocument/completion",
            json!({ "textDocument": doc, "position": { "line": 2, "character": 5 } }),
        );
        let labels: Vec<&str> = items["items"].as_array().unwrap().iter().filter_map(|i| i["label"].as_str()).collect();
        assert!(labels.contains(&"main") && labels.contains(&"let"));
    }

    #[test]
    fn unknown_language_and_mermaid() {
        let mut s = server();
        for (uri, lang, text) in [("mem://x", "cobol", "x"), ("mem://m", "mermaid", "graph LR\n  A[Go] --> B\n  ")] {
            s.handle(json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen",
                "params": { "textDocument": { "uri": uri, "languageId": lang, "version": 1, "text": text } } }));
        }
        assert_eq!(ask(&mut s, "rustaco/status", json!({ "textDocument": { "uri": "mem://x" } }))["ok"], false);
        let m = json!({ "uri": "mem://m" });
        assert_eq!(ask(&mut s, "rustaco/status", json!({ "textDocument": m })), json!({ "ok": true, "tree": false }));
        let h = ask(&mut s, "rustaco/highlight", json!({ "textDocument": m, "lines": [0, 2] }));
        assert!(!h["segments"][0][2].as_array().unwrap().is_empty());
        let items = ask(
            &mut s,
            "textDocument/completion",
            json!({ "textDocument": m, "position": { "line": 2, "character": 2 } }),
        );
        let labels: Vec<&str> = items["items"].as_array().unwrap().iter().filter_map(|i| i["label"].as_str()).collect();
        assert_eq!(&labels[..2], &["A", "B"]);
        assert!(labels.contains(&"subgraph"));
        // Shapes after `id[`, with backslashes escaped so the snippet keeps its tab stop.
        s.handle(json!({ "jsonrpc": "2.0", "method": "textDocument/didChange", "params": {
            "textDocument": { "uri": "mem://m", "version": 2 },
            "contentChanges": [{ "range": { "start": { "line": 2, "character": 2 }, "end": { "line": 2, "character": 2 } }, "text": "X[" }] } }));
        let items = ask(
            &mut s,
            "textDocument/completion",
            json!({ "textDocument": m, "position": { "line": 2, "character": 4 } }),
        );
        let items = items["items"].as_array().unwrap();
        let lean = items.iter().find(|i| i["detail"] == "parallelogram, leaning left").unwrap();
        assert_eq!(lean["textEdit"]["newText"], "[\\\\${1:text}\\\\]$0");
        let snippet = rustaco_core::Snippet::parse(lean["textEdit"]["newText"].as_str().unwrap(), &|_| None);
        assert_eq!(snippet.text, "[\\text\\]");
        assert!(snippet.stops.len() >= 2);
    }
}
