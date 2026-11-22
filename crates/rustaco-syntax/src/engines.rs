//! [`Engine`]: one document's parser and highlighter, whichever kind its language uses
//! (tree-sitter, TextMate or the Mermaid tokenizer), behind one interface in char offsets.
//! The browser's language modules and [`crate::SyntaxService`] both build on it.

use std::ops::Range;

use rustaco_core::{EditEvent, Rope};

use crate::spans::{Highlights, TextContext};

/// A document's engine. The text lives with the caller and is passed in on every call.
pub enum Engine {
    #[cfg(feature = "engine")]
    TreeSitter(crate::Syntax),
    #[cfg(feature = "textmate")]
    TextMate(crate::TextMateSyntax),
    /// Mermaid needs no parse: every line is tokenized on its own.
    Mermaid,
}

/// A declaration from [`Engine::symbols`], in char offsets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CharSymbol {
    pub range: Range<usize>,
    pub name: Range<usize>,
    /// LSP's `SymbolKind` minus one, as [`crate::Symbol::kind`].
    pub kind: u32,
    pub depth: usize,
}

impl Engine {
    /// The engine for a language id, from those compiled in.
    pub fn new(language: &str) -> Result<Engine, String> {
        #[cfg(feature = "engine")]
        if let Some(def) = crate::by_id(language) {
            return crate::Syntax::new(def).map(Engine::TreeSitter);
        }
        #[cfg(feature = "textmate")]
        if let Some(grammar) = crate::textmate_grammar(language) {
            return Ok(Engine::TextMate(crate::TextMateSyntax::new(grammar)));
        }
        if language == "mermaid" {
            return Ok(Engine::Mermaid);
        }
        Err(format!("no engine for {language}"))
    }

    /// Ids of the languages [`Engine::new`] accepts.
    pub fn languages() -> Vec<String> {
        #[allow(unused_mut)]
        let mut ids = vec!["mermaid".to_owned()];
        #[cfg(feature = "engine")]
        ids.extend(crate::languages().iter().map(|l| l.id.to_owned()));
        #[cfg(feature = "textmate")]
        ids.extend(crate::textmate_languages().map(str::to_owned));
        ids
    }

    /// True for a tree-sitter engine: brackets, folds, symbols and selection ranges come from
    /// its tree.
    pub fn has_tree(&self) -> bool {
        #[cfg(feature = "engine")]
        if let Engine::TreeSitter(_) = self {
            return true;
        }
        false
    }

    /// The tree-sitter state, for the structural features.
    #[cfg(feature = "engine")]
    pub fn syntax(&self) -> Option<&crate::Syntax> {
        match self {
            Engine::TreeSitter(s) => Some(s),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }

    /// Forgets the parse; the next [`Engine::parse_with_budget`] starts from scratch.
    pub fn reset(&mut self) {
        match self {
            #[cfg(feature = "engine")]
            Engine::TreeSitter(s) => s.reset(),
            #[cfg(feature = "textmate")]
            Engine::TextMate(s) => s.reset(),
            Engine::Mermaid => {}
        }
    }

    pub fn apply_edit(&mut self, event: &EditEvent) {
        match self {
            #[cfg(feature = "engine")]
            Engine::TreeSitter(s) => s.apply_edit(event),
            #[cfg(feature = "textmate")]
            Engine::TextMate(s) => s.apply_edit(event),
            Engine::Mermaid => {
                let _ = event;
            }
        }
    }

    /// Parses while `keep_going` says so; true when the result matches `rope`.
    pub fn parse_with_budget(&mut self, rope: &Rope, keep_going: impl FnMut() -> bool) -> bool {
        match self {
            #[cfg(feature = "engine")]
            Engine::TreeSitter(s) => s.parse_with_budget(rope, keep_going),
            #[cfg(feature = "textmate")]
            Engine::TextMate(s) => s.parse_with_budget(rope, keep_going),
            Engine::Mermaid => {
                let _ = (rope, keep_going);
                true
            }
        }
    }

    /// True when edits wait for a parse.
    pub fn needs_parse(&self) -> bool {
        match self {
            #[cfg(feature = "engine")]
            Engine::TreeSitter(s) => s.needs_parse(),
            #[cfg(feature = "textmate")]
            Engine::TextMate(s) => s.needs_parse(),
            Engine::Mermaid => false,
        }
    }

    pub fn highlight(&self, rope: &Rope, lines: Range<usize>) -> Highlights {
        match self {
            #[cfg(feature = "engine")]
            Engine::TreeSitter(s) => s.highlight(rope, lines),
            #[cfg(feature = "textmate")]
            Engine::TextMate(s) => s.highlight(rope, lines),
            Engine::Mermaid => crate::mermaid::highlight(rope, lines),
        }
    }

    /// Lines `first..end` as quads: line, start column, end column (chars), style id. With
    /// `bracket_colors`, bracket tokens get depth colours on top (tree-sitter only).
    pub fn highlight_quads(&self, rope: &Rope, first: usize, end: usize, bracket_colors: bool) -> Vec<u32> {
        let highlights = self.highlight(rope, first..end);
        let mut out = Vec::new();
        for line in first..end {
            for span in highlights.line(line) {
                out.extend([line as u32, span.start as u32, span.end as u32, span.style]);
            }
        }
        #[cfg(feature = "engine")]
        if bracket_colors && let Engine::TreeSitter(s) = self {
            let lines = first.min(rope.len_lines())..end.min(rope.len_lines());
            let bytes = rope.line_to_byte(lines.start)..rope.line_to_byte(lines.end);
            for (byte, depth) in s.bracket_depths(bytes) {
                let c = rope.byte_to_char(byte);
                let line = rope.char_to_line(c);
                let col = c - rope.line_to_char(line);
                let style = crate::BRACKET_STYLE_BASE + depth as u32 % crate::BRACKET_COLORS;
                out.extend([line as u32, col as u32, col as u32 + 1, style]);
            }
        }
        let _ = bracket_colors;
        out
    }

    /// The bracket pair at char `pos` (the bracket before the cursor or at it), as char
    /// positions, opening first.
    pub fn matching_bracket(&self, rope: &Rope, pos: usize) -> Option<(usize, usize)> {
        #[cfg(feature = "engine")]
        if let Engine::TreeSitter(s) = self {
            let byte = rope.char_to_byte(pos.min(rope.len_chars()));
            let (a, b) = s.matching_bracket(byte)?;
            return Some((rope.byte_to_char(a), rope.byte_to_char(b)));
        }
        let _ = (rope, pos);
        None
    }

    /// Folding regions as (header line, last hidden line).
    pub fn folding_ranges(&self) -> Vec<(usize, usize)> {
        #[cfg(feature = "engine")]
        if let Engine::TreeSitter(s) = self {
            return s.fold_ranges();
        }
        Vec::new()
    }

    /// Declarations for an outline, in document order with their nesting depth.
    pub fn symbols(&self, rope: &Rope) -> Vec<CharSymbol> {
        #[cfg(feature = "engine")]
        if let Engine::TreeSitter(s) = self {
            let c = |b: usize| rope.byte_to_char(b.min(rope.len_bytes()));
            return s
                .symbols()
                .into_iter()
                .map(|sym| CharSymbol {
                    range: c(sym.bytes.start)..c(sym.bytes.end),
                    name: c(sym.name_bytes.start)..c(sym.name_bytes.end),
                    kind: sym.kind,
                    depth: sym.depth,
                })
                .collect();
        }
        let _ = rope;
        Vec::new()
    }

    /// Char ranges of the syntax nodes around `range`, innermost first.
    pub fn enclosing_ranges(&self, rope: &Rope, range: Range<usize>) -> Vec<Range<usize>> {
        #[cfg(feature = "engine")]
        if let Engine::TreeSitter(s) = self {
            let len = rope.len_chars();
            let bytes = rope.char_to_byte(range.start.min(len))..rope.char_to_byte(range.end.min(len));
            return s
                .enclosing_ranges(bytes)
                .into_iter()
                .map(|r| rope.byte_to_char(r.start)..rope.byte_to_char(r.end))
                .collect();
        }
        let _ = (rope, range);
        Vec::new()
    }

    /// Code, string or comment at char `pos`.
    pub fn context_at(&self, rope: &Rope, pos: usize) -> TextContext {
        let pos = pos.min(rope.len_chars());
        match self {
            #[cfg(feature = "engine")]
            Engine::TreeSitter(s) => s.context_at(rope.char_to_byte(pos)),
            #[cfg(feature = "textmate")]
            Engine::TextMate(s) => s.context_at(rope, pos),
            Engine::Mermaid => {
                let line = rope.char_to_line(pos);
                let col = pos - rope.line_to_char(line);
                let text = rope.line(line).to_string();
                let token = crate::mermaid::tokenize(&text).into_iter().find(|(r, _)| r.start < col && col < r.end);
                match token.map(|(_, s)| s) {
                    Some("comment") => TextContext::Comment,
                    Some("string") => TextContext::String,
                    _ => TextContext::Code,
                }
            }
        }
    }
}
