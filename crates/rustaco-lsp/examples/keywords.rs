//! A language server on stdio built from a `LanguageService`: completion of a fixed keyword
//! list and hover over them. Any LSP client can run it:
//! `cargo run -p rustaco-lsp --example keywords`.
//!
//! The same service, unchanged, runs in a browser worker through `LspServer::handle_json`.

use rustaco_lsp::{CompletionItem, Context, Features, Hover, LanguageService, serve_stdio};

const KEYWORDS: &[(&str, &str)] = &[
    ("graph", "Starts a flowchart; a direction follows (TD, LR, ...)."),
    ("subgraph", "Groups nodes until `end`."),
    ("end", "Closes a `subgraph`."),
    ("classDef", "Defines a style class for nodes."),
];

struct Keywords;

impl LanguageService for Keywords {
    fn features(&self) -> Features {
        let mut f = Features::new("keywords");
        f.completion = Some(Vec::new());
        f.hover = true;
        f
    }

    fn completion(&mut self, _: &Context, _: usize, _: Option<char>) -> Vec<CompletionItem> {
        KEYWORDS
            .iter()
            .map(|(k, doc)| {
                let mut item = CompletionItem::new(*k);
                item.kind = Some(14);
                item.documentation = Some((*doc).into());
                item
            })
            .collect()
    }

    fn hover(&mut self, cx: &Context, at: usize) -> Option<Hover> {
        let rope = cx.model.rope();
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        let mut start = at.min(rope.len_chars());
        while start > 0 && is_word(rope.char(start - 1)) {
            start -= 1;
        }
        let mut end = start;
        while end < rope.len_chars() && is_word(rope.char(end)) {
            end += 1;
        }
        let word = rope.slice(start..end).to_string();
        let (_, doc) = KEYWORDS.iter().find(|(k, _)| *k == word)?;
        let mut hover = Hover::new(vec![format!("`{word}`: {doc}")]);
        hover.range = Some(start..end);
        Some(hover)
    }
}

fn main() -> std::io::Result<()> {
    serve_stdio(Keywords)
}
