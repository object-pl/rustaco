//! Incremental parsing of a [`TextModel`] and highlighting of visible lines.

use std::cell::RefCell;
use std::ops::{ControlFlow, Range};

use rustaco_core::{EditEvent, Rope, TextModel};
use tree_sitter::{InputEdit, Node, ParseOptions, ParseState, Parser, Point, StreamingIterator, TextProvider, Tree};

use crate::injection::{InjectionCache, find_regions};
use crate::language::{CompiledQuery, LanguageDef};
use crate::spans::{Highlights, MAX_HIGHLIGHT_COLUMNS, TextContext};

/// Parse state for one model in one language. Shared by every editor showing the model.
pub struct Syntax {
    language: &'static LanguageDef,
    parser: Parser,
    /// The last parsed tree, adjusted by edits recorded since. Highlighting uses it while a
    /// reparse is pending, so colours stay in place instead of disappearing after a keystroke.
    tree: Option<Tree>,
    /// Edits were recorded (or no tree exists yet) and the tree does not match the text.
    stale: bool,
    version: u64,
    injections: RefCell<InjectionCache>,
}

impl Syntax {
    pub fn new(language: &'static LanguageDef) -> Result<Self, String> {
        let mut parser = Parser::new();
        parser.set_language(&language.grammar()).map_err(|e| format!("{}: {e}", language.id))?;
        language.highlight_query().map_err(str::to_owned)?;
        Ok(Self { language, parser, tree: None, stale: true, version: 0, injections: RefCell::default() })
    }

    pub fn language(&self) -> &'static LanguageDef {
        self.language
    }

    pub fn tree(&self) -> Option<&Tree> {
        self.tree.as_ref()
    }

    /// Brings the tree up to date with the model: edits the old tree with the model's edit
    /// events and reparses incrementally, or parses from scratch when the log no longer
    /// reaches back to the tree's version.
    pub fn update(&mut self, model: &TextModel) {
        if self.tree.is_some() && self.version == model.version() {
            return;
        }
        match model.events_since(self.version) {
            Ok(events) => {
                for ev in events {
                    self.apply_edit(ev);
                }
            }
            Err(_) => self.tree = None,
        }
        self.parse(model.rope());
        self.version = model.version();
    }

    /// Records one edit in the tree. Call [`Syntax::parse`] with the edited text afterwards;
    /// several edits can be recorded before one parse. A parse halted by its budget is
    /// abandoned, since its input changed.
    pub fn apply_edit(&mut self, ev: &EditEvent) {
        self.parser.reset();
        self.stale = true;
        if let Some(tree) = self.tree.as_mut() {
            tree.edit(&InputEdit {
                start_byte: ev.start_byte,
                old_end_byte: ev.old_end_byte,
                new_end_byte: ev.new_end_byte,
                start_position: point(ev.start_point),
                old_end_position: point(ev.old_end_point),
                new_end_position: point(ev.new_end_point),
            });
        }
    }

    /// Parses `rope`, reusing the unchanged parts of the previous tree.
    pub fn parse(&mut self, rope: &Rope) {
        self.parse_with_budget(rope, || true);
    }

    /// Parses `rope` while `keep_going` returns true. Returns true when the tree matches the
    /// text. When the budget runs out the parse halts; calling again with the same text
    /// resumes it where it stopped. tree-sitter's reparse cost grows with the number of
    /// top-level nodes (about 30 ms for 25,000 functions), so a frontend gives each frame a
    /// few milliseconds and keeps typing responsive on any file size.
    pub fn parse_with_budget(&mut self, rope: &Rope, mut keep_going: impl FnMut() -> bool) -> bool {
        if !self.stale {
            return true;
        }
        let len = rope.len_bytes();
        let mut read = |byte: usize, _: Point| -> &[u8] {
            if byte >= len {
                return &[];
            }
            let (chunk, chunk_start, _, _) = rope.chunk_at_byte(byte);
            &chunk.as_bytes()[byte - chunk_start..]
        };
        let mut progress =
            |_: &ParseState| if keep_going() { ControlFlow::Continue(()) } else { ControlFlow::Break(()) };
        let options = ParseOptions::new().progress_callback(&mut progress);
        match self.parser.parse_with_options(&mut read, self.tree.as_ref(), Some(options)) {
            Some(tree) => {
                self.tree = Some(tree);
                self.stale = false;
                true
            }
            None => false,
        }
    }

    /// True when edits are recorded that no parse has caught up with yet.
    pub fn needs_parse(&self) -> bool {
        self.stale
    }

    /// Byte ranges of the syntax nodes around `bytes`, from the smallest node that contains it
    /// out to the root, without repeats. Expand selection walks this list outward.
    pub fn enclosing_ranges(&self, bytes: Range<usize>) -> Vec<Range<usize>> {
        let Some(tree) = self.tree.as_ref() else { return Vec::new() };
        let mut node = tree.root_node().descendant_for_byte_range(bytes.start, bytes.end);
        let mut out: Vec<Range<usize>> = Vec::new();
        while let Some(n) = node {
            let r = n.byte_range();
            if out.last() != Some(&r) {
                out.push(r);
            }
            node = n.parent();
        }
        out
    }

    /// What kind of text is at `byte`: code, a string or a comment. Read from node kinds, which
    /// grammars name `string_literal`, `line_comment`, `template_string` and the like.
    pub fn context_at(&self, byte: usize) -> TextContext {
        let Some(tree) = self.tree.as_ref() else { return TextContext::Code };
        let mut node = tree.root_node().descendant_for_byte_range(byte, byte);
        while let Some(n) = node {
            let kind = n.kind();
            if kind.contains("comment") {
                return TextContext::Comment;
            }
            if kind.contains("string") || kind.contains("char_literal") || kind == "heredoc_body" {
                // The quotes themselves belong to the string node; between them is string text.
                if n.start_byte() < byte && byte < n.end_byte() {
                    return TextContext::String;
                }
            }
            node = n.parent();
        }
        TextContext::Code
    }

    /// Drops the tree, so the next parse starts from scratch.
    pub fn reset(&mut self) {
        self.parser.reset();
        self.tree = None;
        self.stale = true;
    }

    /// Highlights `lines` of the model the tree was last updated from.
    pub fn highlight(&self, rope: &Rope, lines: Range<usize>) -> Highlights {
        let lines = lines.start.min(rope.len_lines())..lines.end.min(rope.len_lines());
        let mut paint: Vec<Vec<u32>> =
            lines.clone().map(|l| vec![0; rustaco_core::line_len(rope, l).min(MAX_HIGHLIGHT_COLUMNS)]).collect();
        let (Some(tree), Ok(compiled)) = (self.tree.as_ref(), self.language.highlight_query()) else {
            return Highlights::from_paint(lines.start, paint);
        };
        if lines.is_empty() {
            return Highlights::from_paint(lines.start, paint);
        }

        let byte_range = rope.line_to_byte(lines.start)..rope.line_to_byte(lines.end);
        let mut spans = collect_captures(compiled, tree, byte_range.clone(), RopeText(rope));

        // Injected languages paint over the host's colours inside their regions.
        let regions = find_regions(self.language, tree, byte_range, RopeText(rope), |n| {
            rope.byte_slice(n.start_byte()..n.end_byte().min(rope.len_bytes())).to_string()
        });
        if !regions.is_empty() {
            let mut cache = self.injections.borrow_mut();
            for region in regions {
                let end = region.bytes.end.min(rope.len_bytes());
                let text = rope.byte_slice(region.bytes.start.min(end)..end).to_string();
                let inner = cache.spans(region.language, &text, 1);
                spans
                    .extend(inner.iter().map(|&(s, e, style)| (s + region.bytes.start, e + region.bytes.start, style)));
            }
        }

        for (start_byte, end_byte, style) in spans {
            let start = rope.byte_to_char(start_byte.min(rope.len_bytes()));
            let end = rope.byte_to_char(end_byte.min(rope.len_bytes()));
            let (first, last) = (rope.char_to_line(start), rope.char_to_line(end));
            for line in first.max(lines.start)..=last.min(lines.end - 1) {
                let row = &mut paint[line - lines.start];
                let line_start = rope.line_to_char(line);
                let from = if line == first { start - line_start } else { 0 };
                let to = if line == last { end - line_start } else { row.len() };
                let (from, to) = (from.min(row.len()), to.min(row.len()));
                row[from..to].fill(style);
            }
        }
        Highlights::from_paint(lines.start, paint)
    }
}

/// Styled captures of `query` in `range`, as (start byte, end byte, style) in paint order:
/// outer nodes before inner ones, and for one node the more specific pattern last, so it
/// wins (see [`CompiledQuery::specificity`]); between equally specific patterns the later
/// one wins. Neither order alone fits the bundled queries: Python lists `@function.method`
/// before `@property` for the same attribute node, but its generic `(identifier) @variable`
/// first.
pub(crate) fn collect_captures<I: AsRef<[u8]>, T: TextProvider<I>>(
    compiled: &CompiledQuery,
    tree: &Tree,
    range: Range<usize>,
    provider: T,
) -> Vec<(usize, usize, u32)> {
    let mut cursor = tree_sitter::QueryCursor::new();
    cursor.set_byte_range(range);
    let mut found: Vec<(usize, usize, (u32, usize), u32)> = Vec::new();
    let mut captures = cursor.captures(&compiled.query, tree.root_node(), provider);
    while let Some((m, index)) = captures.next() {
        let capture = m.captures()[*index];
        let style = compiled.capture_styles[capture.index as usize];
        if style != 0 {
            let rank = (compiled.specificity[m.pattern_index], m.pattern_index);
            found.push((capture.node.start_byte(), capture.node.end_byte(), rank, style));
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
    found.into_iter().map(|(s, e, _, style)| (s, e, style)).collect()
}

fn point(p: rustaco_core::Point) -> Point {
    Point { row: p.row, column: p.column }
}

/// Feeds node text to query predicates (`#eq?`, `#match?`) straight from the rope.
struct RopeText<'a>(&'a Rope);

impl<'a> TextProvider<&'a [u8]> for RopeText<'a> {
    type I = std::iter::Map<ropey::iter::Chunks<'a>, fn(&'a str) -> &'a [u8]>;

    fn text(&mut self, node: Node) -> Self::I {
        let end = node.end_byte().min(self.0.len_bytes());
        let start = node.start_byte().min(end);
        self.0.byte_slice(start..end).chunks().map(str::as_bytes as fn(&'a str) -> &'a [u8])
    }
}
