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

    /// The bracket pair at `byte`: when the char at `byte` (or just before it) is a bracket
    /// token, returns the start bytes of it and its partner, opening bracket first. Partners
    /// come from the tree (siblings under one node), so brackets in strings never match code.
    pub fn matching_bracket(&self, byte: usize) -> Option<(usize, usize)> {
        let tree = self.tree.as_ref()?;
        let root = tree.root_node();
        let candidates = [byte.checked_sub(1), Some(byte)];
        for at in candidates.into_iter().flatten() {
            let Some(node) = root.descendant_for_byte_range(at, at + 1) else { continue };
            let kind = node.kind();
            let Some(partner_kind) = bracket_partner(kind) else { continue };
            if node.is_named() || node.start_byte() != at {
                continue;
            }
            let parent = node.parent()?;
            let mut cursor = parent.walk();
            let siblings: Vec<_> = parent.children(&mut cursor).collect();
            let partner = if is_opening(kind) {
                siblings.iter().rev().find(|s| s.kind() == partner_kind && !s.is_named())
            } else {
                siblings.iter().find(|s| s.kind() == partner_kind && !s.is_named())
            };
            if let Some(p) = partner {
                let (a, b) = (node.start_byte(), p.start_byte());
                return Some(if a < b { (a, b) } else { (b, a) });
            }
        }
        None
    }

    /// Declarations for an outline: functions, methods, classes, structs, enums, traits and
    /// interfaces, modules, impl blocks, constants, in document order with their nesting
    /// depth. Found by node kind (`function_item`, `class_declaration`, `method_definition`,
    /// ...) and named by the node's `name` field (`type` for impl blocks), which the bundled
    /// grammars share.
    pub fn symbols(&self) -> Vec<Symbol> {
        let Some(tree) = self.tree.as_ref() else { return Vec::new() };
        let mut out = Vec::new();
        let mut cursor = tree.walk();
        let mut depth = 0usize;
        // Depths at which a symbol was entered, to know the nesting of the next ones.
        let mut open: Vec<usize> = Vec::new();
        loop {
            let node = cursor.node();
            if let Some(kind) = symbol_kind(node.kind()) {
                let name = node.child_by_field_name("name").or_else(|| node.child_by_field_name("type"));
                if let Some(name) = name {
                    out.push(Symbol {
                        kind,
                        bytes: node.start_byte()..node.end_byte(),
                        name_bytes: name.start_byte()..name.end_byte(),
                        depth: open.len(),
                    });
                    open.push(depth);
                }
            }
            if cursor.goto_first_child() {
                depth += 1;
                continue;
            }
            loop {
                while open.last() == Some(&depth) {
                    open.pop();
                }
                if cursor.goto_next_sibling() {
                    break;
                }
                if !cursor.goto_parent() {
                    return out;
                }
                depth -= 1;
            }
        }
    }

    /// Regions worth folding, as (header row, last hidden row): the inside of every bracket
    /// pair spanning lines (the closing bracket's line stays visible), multi-line comments,
    /// and markdown sections. One region per header row, the outermost. Walks only nodes
    /// that span lines, but still the whole tree, so call it once typing pauses.
    pub fn fold_ranges(&self) -> Vec<(usize, usize)> {
        let Some(tree) = self.tree.as_ref() else { return Vec::new() };
        let mut out: Vec<(usize, usize)> = Vec::new();
        let mut cursor = tree.walk();
        let last_row = |n: Node| {
            let end = n.end_position();
            if end.column == 0 && end.row > n.start_position().row { end.row - 1 } else { end.row }
        };
        loop {
            let node = cursor.node();
            let multi_line = node.end_position().row > node.start_position().row;
            if multi_line {
                let count = node.child_count();
                let open =
                    (0..count.min(3)).filter_map(|i| node.child(i)).find(|c| !c.is_named() && is_opening(c.kind()));
                let close = count.checked_sub(1).and_then(|i| node.child(i));
                if let (Some(open), Some(close)) = (open, close)
                    && bracket_partner(open.kind()) == Some(close.kind())
                {
                    let (a, b) = (open.start_position().row, close.start_position().row);
                    if b > a + 1 {
                        out.push((a, b - 1));
                    }
                } else if node.kind().contains("comment") || node.kind() == "section" {
                    let (a, b) = (node.start_position().row, last_row(node));
                    if b > a {
                        out.push((a, b));
                    }
                }
            }
            if multi_line && cursor.goto_first_child() {
                continue;
            }
            loop {
                if cursor.goto_next_sibling() {
                    break;
                }
                if !cursor.goto_parent() {
                    out.sort_unstable_by_key(|&(a, b)| (a, std::cmp::Reverse(b)));
                    out.dedup_by_key(|r| r.0);
                    return out;
                }
            }
        }
    }

    /// Bracket tokens starting in `bytes`, with their nesting depth (0 for the outermost
    /// pair): (start byte, depth). The walk descends only into nodes that reach the range, so
    /// its cost follows the visible text, not the file. Depth counts the enclosing nodes that
    /// open with a bracket among their first children, which is where grammars put pairs.
    pub fn bracket_depths(&self, bytes: Range<usize>) -> Vec<(usize, usize)> {
        fn opens_pair(n: Node) -> bool {
            (0..n.child_count().min(3)).any(|i| n.child(i).is_some_and(|c| !c.is_named() && is_opening(c.kind())))
        }
        fn visit(node: Node, bytes: &Range<usize>, out: &mut Vec<(usize, usize)>) {
            let mut cursor = node.walk();
            if cursor.goto_first_child_for_byte(bytes.start).is_none() {
                return;
            }
            loop {
                let child = cursor.node();
                if child.start_byte() >= bytes.end {
                    break;
                }
                if child.child_count() == 0 {
                    if !child.is_named()
                        && bracket_partner(child.kind()).is_some()
                        && bytes.contains(&child.start_byte())
                    {
                        let mut depth = 0;
                        let mut ancestor = child.parent().and_then(|p| p.parent());
                        while let Some(a) = ancestor {
                            if opens_pair(a) {
                                depth += 1;
                            }
                            ancestor = a.parent();
                        }
                        out.push((child.start_byte(), depth));
                    }
                } else {
                    visit(child, bytes, out);
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
        let Some(tree) = self.tree.as_ref() else { return Vec::new() };
        let mut out = Vec::new();
        visit(tree.root_node(), &bytes, &mut out);
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

fn bracket_partner(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "(" => ")",
        ")" => "(",
        "[" => "]",
        "]" => "[",
        "{" => "}",
        "}" => "{",
        _ => return None,
    })
}

fn is_opening(kind: &str) -> bool {
    matches!(kind, "(" | "[" | "{")
}

/// A declaration found by [`Syntax::symbols`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    /// Kind in LSP's numbering minus one (Monaco's `SymbolKind`): 4 class, 5 method,
    /// 11 function, 22 struct, ...
    pub kind: u32,
    pub bytes: Range<usize>,
    pub name_bytes: Range<usize>,
    /// Number of enclosing symbols.
    pub depth: usize,
}

/// The outline kind of a node kind, or None for nodes that are not declarations.
fn symbol_kind(kind: &str) -> Option<u32> {
    // Monaco's SymbolKind numbers.
    const MODULE: u32 = 1;
    const CLASS: u32 = 4;
    const METHOD: u32 = 5;
    const ENUM: u32 = 9;
    const INTERFACE: u32 = 10;
    const FUNCTION: u32 = 11;
    const CONSTANT: u32 = 13;
    const STRUCT: u32 = 22;
    let declaration = kind.ends_with("_item")
        || kind.ends_with("_declaration")
        || kind.ends_with("_definition")
        || kind.ends_with("_specifier")
        || kind == "type_spec";
    if !declaration {
        return None;
    }
    Some(if kind.contains("method") {
        METHOD
    } else if kind.contains("function") || kind.contains("constructor") {
        FUNCTION
    } else if kind.contains("class") {
        CLASS
    } else if kind.contains("struct") {
        STRUCT
    } else if kind.contains("enum") {
        ENUM
    } else if kind.contains("interface") || kind.contains("trait") || kind.contains("protocol") {
        INTERFACE
    } else if kind.contains("impl") {
        CLASS
    } else if kind.contains("mod") || kind.contains("namespace") {
        MODULE
    } else if kind.starts_with("const") || kind.starts_with("static") {
        CONSTANT
    } else if kind == "type_spec" || kind.starts_with("type_") {
        STRUCT
    } else {
        return None;
    })
}
