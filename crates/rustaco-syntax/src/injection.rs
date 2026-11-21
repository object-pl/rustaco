//! Language injections: code of one language embedded in another (scripts and styles in
//! HTML, fenced code blocks in Markdown, macro bodies in Rust, tagged templates in JS).
//!
//! Injected regions are found per highlight call, only inside the requested lines, and each
//! region is parsed on its own. Results are cached by language and region text, so typing
//! outside a code block does not reparse it. Regions longer than [`MAX_INJECTION_BYTES`] are
//! left to the host language's highlighting.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use tree_sitter::{Node, Parser, Query, QueryCursor, StreamingIterator, TextProvider, Tree};

use crate::language::{LanguageDef, by_id};

pub const MAX_INJECTION_BYTES: usize = 256 * 1024;
/// Injections inside injections are followed this deep (Markdown, then its inline grammar).
const MAX_DEPTH: usize = 3;
/// Total region text kept in the cache before it is cleared.
const CACHE_BYTES: usize = 4 * 1024 * 1024;

/// An injection query with the capture indices and fixed languages it uses.
pub(crate) struct CompiledInjections {
    query: Query,
    content: Option<u32>,
    language: Option<u32>,
    /// `#set! injection.language "x"` per pattern.
    fixed_language: Vec<Option<String>>,
}

impl CompiledInjections {
    pub(crate) fn new(query: Query) -> Self {
        let index = |name: &str| query.capture_names().iter().position(|n| *n == name).map(|i| i as u32);
        let fixed_language = (0..query.pattern_count())
            .map(|p| {
                query
                    .property_settings(p)
                    .iter()
                    .find(|prop| &*prop.key == "injection.language")
                    .and_then(|prop| prop.value.as_deref().map(str::to_owned))
            })
            .collect();
        Self { content: index("injection.content"), language: index("injection.language"), fixed_language, query }
    }
}

/// A region of the host text to highlight as another language.
pub(crate) struct Region {
    pub(crate) bytes: Range<usize>,
    pub(crate) language: &'static LanguageDef,
}

/// Finds injected regions intersecting `range` in `tree`. `text_of` returns the text of a
/// node (for `@injection.language` captures such as a Markdown fence's info string).
pub(crate) fn find_regions<I: AsRef<[u8]>, T: TextProvider<I>>(
    host: &LanguageDef,
    tree: &Tree,
    range: Range<usize>,
    provider: T,
    mut text_of: impl FnMut(Node) -> String,
) -> Vec<Region> {
    let Some(Ok(compiled)) = host.injection_query() else { return Vec::new() };
    let Some(content_index) = compiled.content else { return Vec::new() };
    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(range);
    let mut regions = Vec::new();
    let mut matches = cursor.matches(&compiled.query, tree.root_node(), provider);
    while let Some(m) = matches.next() {
        let named =
            compiled.language.and_then(|li| m.captures().iter().find(|c| c.index == li)).map(|c| text_of(c.node));
        let name = named.or_else(|| compiled.fixed_language[m.pattern_index].clone());
        let Some(language) = name.as_deref().and_then(resolve) else { continue };
        for c in m.captures().iter().filter(|c| c.index == content_index) {
            let bytes = c.node.start_byte()..c.node.end_byte();
            if !bytes.is_empty() && bytes.len() <= MAX_INJECTION_BYTES {
                regions.push(Region { bytes, language });
            }
        }
    }
    regions
}

/// Maps an injection language name (a fence info string, a template tag) to a compiled
/// language. Names are matched case-insensitively, with common aliases.
pub fn resolve(name: &str) -> Option<&'static LanguageDef> {
    let name = name.trim().to_ascii_lowercase();
    let id = match name.as_str() {
        "js" | "jsx" | "node" | "mjs" | "cjs" => "javascript",
        "ts" | "mts" | "cts" => "typescript",
        "py" | "python3" => "python",
        "sh" | "shell" | "zsh" | "console" => "bash",
        "yml" => "yaml",
        "c++" | "cxx" | "cc" | "hpp" => "cpp",
        "cs" | "c#" => "csharp",
        "md" => "markdown",
        "rs" => "rust",
        "golang" => "go",
        "htm" | "xhtml" => "html",
        "jsonc" | "json5" => "json",
        other => other,
    };
    by_id(id)
}

/// Spans of an injected region, relative to its start, in paint order.
pub(crate) type Spans = Arc<Vec<(usize, usize, u32)>>;

#[derive(Default)]
pub(crate) struct InjectionCache {
    entries: HashMap<(&'static str, String), Spans>,
    bytes: usize,
    parsers: HashMap<&'static str, Parser>,
}

impl InjectionCache {
    /// Highlight spans for `text` as `language`, from the cache or by parsing it now.
    pub(crate) fn spans(&mut self, language: &'static LanguageDef, text: &str, depth: usize) -> Spans {
        let key = (language.id, text.to_owned());
        if let Some(spans) = self.entries.get(&key) {
            return spans.clone();
        }
        let spans = Arc::new(self.highlight_text(language, text, depth));
        if self.bytes + text.len() > CACHE_BYTES {
            self.entries.clear();
            self.bytes = 0;
        }
        self.bytes += text.len();
        self.entries.insert(key, spans.clone());
        spans
    }

    fn highlight_text(&mut self, language: &'static LanguageDef, text: &str, depth: usize) -> Vec<(usize, usize, u32)> {
        let Ok(compiled) = language.highlight_query() else { return Vec::new() };
        let parser = self.parsers.entry(language.id).or_insert_with(|| {
            let mut p = Parser::new();
            let _ = p.set_language(&language.grammar());
            p
        });
        let Some(tree) = parser.parse(text, None) else { return Vec::new() };
        let mut spans = crate::highlight::collect_captures(compiled, &tree, 0..text.len(), text.as_bytes());
        if depth < MAX_DEPTH {
            let regions = find_regions(language, &tree, 0..text.len(), text.as_bytes(), |n| {
                text.get(n.byte_range()).unwrap_or_default().to_owned()
            });
            for region in regions {
                let inner_text = &text[region.bytes.clone()];
                let inner = self.spans(region.language, inner_text, depth + 1);
                spans
                    .extend(inner.iter().map(|&(s, e, style)| (s + region.bytes.start, e + region.bytes.start, style)));
            }
        }
        spans
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_resolve_to_compiled_languages() {
        assert_eq!(resolve("JS").map(|l| l.id), Some("javascript"));
        assert_eq!(resolve(" py ").map(|l| l.id), Some("python"));
        assert_eq!(resolve("c++").map(|l| l.id), Some("cpp"));
        assert!(resolve("brainfuck").is_none());
    }
}
