//! A TextMate grammar tokenizer, for languages without a usable tree-sitter grammar.
//!
//! It reads VS Code grammars (`*.tmLanguage.json`) and follows `vscode-textmate`'s model:
//! a line is tokenized left to right against the rules of the innermost open `begin`/`end`
//! (or `begin`/`while`) block; the earliest match wins, ties go to the rule listed first,
//! and a block's `end` is tried before its patterns unless `applyEndPatternLast` is set.
//!
//! Supported: `match`, `begin`/`end` (with backreferences from `begin` in `end`),
//! `begin`/`while`, `captures`, `beginCaptures`, `endCaptures`, `name`, `contentName`,
//! `patterns`, `include` (`#name`, `$self`, `$base`). Not supported: injections, includes
//! of other grammars, and `patterns` nested inside captures. Regexes are Oniguruma syntax
//! translated for `fancy-regex`; a regex that still fails to compile disables its rule.
//!
//! Every line stores the tokenizer state it starts with. After an edit, tokenization resumes
//! at the edited line and stops early once a line's start state equals the old one again.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use fancy_regex::Regex;

type Captures<'t> = fancy_regex::Captures<'t, str>;
use rustaco_core::{EditEvent, Rope};
use serde_json::Value;

use crate::names::HIGHLIGHT_NAMES;
use crate::spans::{Highlights, MAX_HIGHLIGHT_COLUMNS};

type RuleId = usize;
type ScopeId = u32;

/// Stands in for `\G` in translated regex sources until a search position is known.
const G_MARK: char = '\u{E000}';

/// A regex in TextMate form. `\G` (the position where the previous match on this line ended)
/// has no `fancy-regex` equivalent: a leading `\G` becomes the `anchored` flag (the match must
/// start there), any other `\G` stays as [`G_MARK`] in `template` and is replaced per search.
/// `never` is the variant where `\G` cannot match, compiled up front.
struct Pattern {
    template: Option<Arc<str>>,
    never: Option<Regex>,
    has_g: bool,
    anchored: bool,
}

/// Replaces [`G_MARK`] with a construct matching only `at` chars into the line, or never.
fn instantiate(template: &str, at: Option<usize>) -> String {
    let g = match at {
        None => "(?!)".to_owned(),
        Some(0) => "\\A".to_owned(),
        Some(n) => format!("(?<=\\A[\\s\\S]{{{n}}})"),
    };
    template.replace(G_MARK, &g)
}

#[derive(Default)]
struct CaptureScopes(Vec<(usize, ScopeId)>);

enum Kind {
    Match(Pattern, CaptureScopes),
    Block {
        begin: Pattern,
        /// Source of `end`/`while`, which may hold backreferences into `begin`'s captures.
        close: String,
        is_while: bool,
        begin_captures: CaptureScopes,
        close_captures: CaptureScopes,
        content_name: Option<ScopeId>,
        patterns: Vec<RuleId>,
        end_last: bool,
    },
    /// A rule with only `patterns`, or an `include` resolved to the rule it names (an empty
    /// group for includes of other grammars or unknown names).
    Group(Vec<RuleId>),
}

struct Rule {
    name: Option<ScopeId>,
    kind: Kind,
}

/// A loaded grammar. Immutable and shareable between documents.
pub struct Grammar {
    pub scope_name: String,
    rules: Vec<Rule>,
    root: RuleId,
    scopes: Vec<String>,
    /// Style id per scope, from the innermost matching highlight name.
    scope_styles: Vec<u32>,
    /// Flattened alternatives (Match and Block rules) per Group.
    flattened: HashMap<RuleId, Vec<RuleId>>,
    /// Regexes in the grammar that failed to compile; their rules never match.
    pub failed_regexes: usize,
}

impl Grammar {
    pub fn from_json(source: &str) -> Result<Self, String> {
        let json: Value = serde_json::from_str(source).map_err(|e| e.to_string())?;
        let mut b = Builder::default();
        let scope_name = json.get("scopeName").and_then(Value::as_str).unwrap_or("source").to_owned();
        let root = b.reserve();
        b.root = root;
        b.open_repository(json.get("repository"));
        let patterns = b.pattern_list(json.get("patterns"));
        b.repositories.pop();
        b.rules[root] = Some(Rule { name: Some(b.scope(&scope_name)), kind: Kind::Group(patterns) });

        let rules: Vec<Rule> =
            b.rules.into_iter().map(|r| r.unwrap_or(Rule { name: None, kind: Kind::Group(vec![]) })).collect();
        // Quotes and comment markers (`punctuation.definition.string.begin`) take the colour of
        // the string or comment around them, as in VS Code themes, which rarely style them.
        let scope_styles = b
            .scopes
            .iter()
            .map(|s| {
                let inherits = s.starts_with("punctuation.definition.") || s.starts_with("punctuation.whitespace.");
                if inherits { 0 } else { style_for_scope(s) }
            })
            .collect();
        let mut grammar = Grammar {
            scope_name,
            rules,
            root,
            scopes: b.scopes,
            scope_styles,
            flattened: HashMap::new(),
            failed_regexes: b.failed,
        };
        let groups: Vec<RuleId> = (0..grammar.rules.len())
            .filter(|&id| matches!(grammar.rules[id].kind, Kind::Group(_) | Kind::Block { .. }))
            .collect();
        for id in groups {
            let flat = grammar.flatten(id);
            grammar.flattened.insert(id, flat);
        }
        Ok(grammar)
    }

    /// The Match and Block rules reachable from a Group's or Block's patterns, in order.
    fn flatten(&self, id: RuleId) -> Vec<RuleId> {
        // `seen` holds the groups already expanded into this list; a group included twice, or
        // including itself, is expanded once. `depth` stops chains of includes of includes.
        fn walk(g: &Grammar, ids: &[RuleId], out: &mut Vec<RuleId>, seen: &mut Vec<RuleId>, depth: usize) {
            if depth > 64 {
                return;
            }
            for &id in ids {
                match &g.rules[id].kind {
                    Kind::Match(..) | Kind::Block { .. } => out.push(id),
                    Kind::Group(inner) => {
                        if !seen.contains(&id) {
                            seen.push(id);
                            walk(g, inner, out, seen, depth + 1);
                        }
                    }
                }
            }
        }
        let start: &[RuleId] = match &self.rules[id].kind {
            Kind::Group(p) => p,
            Kind::Block { patterns, .. } => patterns,
            _ => &[],
        };
        let mut out = Vec::new();
        walk(self, start, &mut out, &mut vec![id], 0);
        out
    }

    /// The rules tried at the top level, for diagnosing grammars: kind, scope and regex.
    #[doc(hidden)]
    pub fn describe_top_level(&self) -> Vec<String> {
        let flat = self.flattened.get(&self.root).cloned().unwrap_or_default();
        flat.iter()
            .map(|&id| {
                let rule = &self.rules[id];
                let scope = rule.name.map_or("-", |s| self.scopes[s as usize].as_str());
                let (kind, p) = match &rule.kind {
                    Kind::Match(p, _) => ("match", p),
                    Kind::Block { begin, .. } => ("block", begin),
                    _ => return format!("{id}: other"),
                };
                let src: String = p.template.as_deref().unwrap_or("<untranslatable>").chars().take(50).collect();
                format!("{id}: {kind} {scope} compiled={} {src:?}", p.never.is_some())
            })
            .collect()
    }

    fn style_of(&self, frames: &[Frame], extra: Option<ScopeId>) -> u32 {
        let scopes = frames.iter().flat_map(|f| [f.name, f.content]).flatten().chain(extra);
        let mut style = 0;
        for scope in scopes {
            let s = self.scope_styles[scope as usize];
            if s != 0 {
                style = s;
            }
        }
        style
    }
}

#[derive(Default)]
struct Builder {
    rules: Vec<Option<Rule>>,
    root: RuleId,
    /// Repositories visible at the rule being compiled, innermost last. A rule's own
    /// `repository` shadows its parents' entries, as in TextMate.
    repositories: Vec<HashMap<String, RuleId>>,
    top_level: HashMap<String, RuleId>,
    scopes: Vec<String>,
    scope_ids: HashMap<String, ScopeId>,
    failed: usize,
}

impl Builder {
    /// Makes a `repository` object's entries visible to includes, and compiles them. Entries
    /// get ids before any is compiled, so they can refer to each other in any order.
    fn open_repository(&mut self, repo: Option<&Value>) {
        let mut map = HashMap::new();
        if let Some(obj) = repo.and_then(Value::as_object) {
            for name in obj.keys() {
                map.insert(name.clone(), self.reserve());
            }
        }
        if self.repositories.is_empty() {
            self.top_level = map.clone();
        }
        self.repositories.push(map.clone());
        if let Some(obj) = repo.and_then(Value::as_object) {
            for (name, value) in obj {
                self.fill(map[name], value);
            }
        }
    }

    fn resolve_include(&self, target: &str) -> Vec<RuleId> {
        match target {
            "$self" | "$base" => vec![self.root],
            t => t
                .strip_prefix('#')
                .and_then(|name| self.repositories.iter().rev().find_map(|r| r.get(name).copied()))
                .into_iter()
                .collect(),
        }
    }

    fn reserve(&mut self) -> RuleId {
        self.rules.push(None);
        self.rules.len() - 1
    }

    fn scope(&mut self, name: &str) -> ScopeId {
        // A name may list several scopes ("keyword.other.sql storage.type"); the last one is
        // the most specific for styling purposes.
        let name = name.split_whitespace().last().unwrap_or(name);
        if let Some(&id) = self.scope_ids.get(name) {
            return id;
        }
        self.scopes.push(name.to_owned());
        let id = (self.scopes.len() - 1) as ScopeId;
        self.scope_ids.insert(name.to_owned(), id);
        id
    }

    fn name(&mut self, v: &Value, key: &str) -> Option<ScopeId> {
        // Names with `$1`-style placeholders depend on captures; they are styled by their
        // static part only.
        v.get(key).and_then(Value::as_str).map(|s| {
            let s = s.split('$').next().unwrap_or(s).trim_end_matches('.');
            self.scope(s)
        })
    }

    fn pattern(&mut self, src: &str) -> Pattern {
        let (src, anchored) = match src.strip_prefix("\\G") {
            Some(rest) => (rest, true),
            None => (src, false),
        };
        let template: Option<Arc<str>> = translate(src).map(Into::into);
        let has_g = template.as_deref().is_some_and(|t| t.contains(G_MARK));
        let never = template.as_deref().and_then(|t| Regex::new(&instantiate(t, None)).ok());
        if never.is_none() {
            self.failed += 1;
        }
        Pattern { template, never, has_g, anchored }
    }

    fn captures(&mut self, v: Option<&Value>) -> CaptureScopes {
        let mut out = Vec::new();
        if let Some(map) = v.and_then(Value::as_object) {
            for (k, c) in map {
                if let (Ok(i), Some(name)) = (k.parse::<usize>(), self.name(c, "name")) {
                    out.push((i, name));
                }
            }
        }
        out.sort_by_key(|&(i, _)| i);
        CaptureScopes(out)
    }

    fn pattern_list(&mut self, v: Option<&Value>) -> Vec<RuleId> {
        v.and_then(Value::as_array).map(|a| a.iter().map(|p| self.compile(p)).collect()).unwrap_or_default()
    }

    fn compile(&mut self, v: &Value) -> RuleId {
        let id = self.reserve();
        self.fill(id, v);
        id
    }

    fn fill(&mut self, id: RuleId, v: &Value) {
        let name = self.name(v, "name");
        let own_repository = v.get("repository").is_some();
        if own_repository {
            self.open_repository(v.get("repository"));
        }
        let kind = if let Some(include) = v.get("include").and_then(Value::as_str) {
            Kind::Group(self.resolve_include(include))
        } else if let Some(m) = v.get("match").and_then(Value::as_str) {
            let pattern = self.pattern(m);
            Kind::Match(pattern, self.captures(v.get("captures")))
        } else if let Some(begin) = v.get("begin").and_then(Value::as_str) {
            let (close, is_while) = match (v.get("end").and_then(Value::as_str), v.get("while").and_then(Value::as_str))
            {
                (Some(e), _) => (e.to_owned(), false),
                (None, Some(w)) => (w.to_owned(), true),
                (None, None) => ("(?!)".to_owned(), false),
            };
            let begin = self.pattern(begin);
            let shared = v.get("captures");
            let begin_captures = self.captures(v.get("beginCaptures").or(shared));
            let close_captures =
                self.captures(v.get(if is_while { "whileCaptures" } else { "endCaptures" }).or(shared));
            let content_name = self.name(v, "contentName");
            let patterns = self.pattern_list(v.get("patterns"));
            let end_last =
                v.get("applyEndPatternLast").is_some_and(|b| b.as_bool() == Some(true) || b.as_i64() == Some(1));
            Kind::Block { begin, close, is_while, begin_captures, close_captures, content_name, patterns, end_last }
        } else {
            Kind::Group(self.pattern_list(v.get("patterns")))
        };
        if own_repository {
            self.repositories.pop();
        }
        self.rules[id] = Some(Rule { name, kind });
    }
}

/// Translates Oniguruma syntax that `fancy-regex` lacks. `\G` becomes [`G_MARK`].
fn translate(src: &str) -> Option<String> {
    let mut out = String::with_capacity(src.len() + 8);
    let mut chars = src.chars().peekable();
    let mut in_class = false;
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                let Some(n) = chars.next() else { break };
                match n {
                    'h' if !in_class => out.push_str("[0-9a-fA-F]"),
                    'h' => out.push_str("0-9a-fA-F"),
                    'H' if !in_class => out.push_str("[^0-9a-fA-F]"),
                    'Z' => out.push_str("(?=\\n?\\z)"),
                    'G' => out.push(G_MARK),
                    _ => {
                        out.push('\\');
                        out.push(n);
                    }
                }
            }
            '[' if !in_class => {
                in_class = true;
                out.push(c);
                if chars.peek() == Some(&'^') {
                    out.push(chars.next().unwrap_or('^'));
                }
                if chars.peek() == Some(&']') {
                    out.push_str("\\]");
                    chars.next();
                }
            }
            ']' if in_class => {
                in_class = false;
                out.push(c);
            }
            // Possessive quantifiers become greedy: the match set is the same for the
            // patterns grammars use them in, only backtracking differs.
            '+' if !in_class && out.ends_with(['*', '+', '?', '}']) && !out.ends_with("\\+") => {}
            _ => out.push(c),
        }
    }
    Some(out)
}

/// Style id for a TextMate scope: the highlight name with the longest scope that is a
/// dot-separated prefix of it.
fn style_for_scope(scope: &str) -> u32 {
    // Ranked by selector length, then by the selector being the name's primary scope, then by
    // the name being spelled like the selector: `keyword.other.sql` goes to `keyword`, not to
    // `charset`, whose fallback scope is also `keyword`.
    let mut best: Option<(usize, (usize, bool, bool))> = None;
    for (i, (name, scopes)) in HIGHLIGHT_NAMES.iter().enumerate() {
        for (k, s) in scopes.iter().enumerate() {
            let covers = scope == *s || (scope.starts_with(s) && scope.as_bytes().get(s.len()) == Some(&b'.'));
            let rank = (s.len(), k == 0, name == s);
            if covers && best.is_none_or(|(_, r)| rank > r) {
                best = Some((i, rank));
            }
        }
    }
    best.map_or(0, |(i, _)| i as u32 + 1)
}

/// One open block on the tokenizer stack.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Frame {
    rule: RuleId,
    name: Option<ScopeId>,
    content: Option<ScopeId>,
    /// The block's `end`/`while` with backreferences substituted.
    close: Option<Arc<str>>,
}

type State = Vec<Frame>;

struct LineInfo {
    state: State,
    /// (start byte, end byte, style) in paint order; None until tokenized.
    tokens: Option<Vec<(usize, usize, u32)>>,
}

/// Lines tokenized before an edit, kept so tokenization can stop early.
struct Tail {
    lines: Vec<LineInfo>,
    first_new_line: usize,
}

/// TextMate tokenization state for one document.
pub struct TextMateSyntax {
    grammar: Arc<Grammar>,
    lines: Vec<LineInfo>,
    tail: Option<Tail>,
    /// Compiled `end`/`while` patterns and `\G` variants, by source and `\G` position.
    dynamic: HashMap<(Arc<str>, Option<usize>), Option<Regex>>,
}

const MAX_LINE_BYTES: usize = 20_000;
const MAX_DEPTH: usize = 100;

impl TextMateSyntax {
    pub fn new(grammar: Arc<Grammar>) -> Self {
        let root = Frame { rule: grammar.root, name: grammar.rules[grammar.root].name, content: None, close: None };
        Self { grammar, lines: vec![LineInfo { state: vec![root], tokens: None }], tail: None, dynamic: HashMap::new() }
    }

    pub fn reset(&mut self) {
        let grammar = self.grammar.clone();
        *self = Self::new(grammar);
    }

    pub fn needs_parse(&self) -> bool {
        self.lines.last().is_none_or(|l| l.tokens.is_none())
    }

    /// Invalidates tokens from the edited line on. Rows are rope lines before the edit.
    pub fn apply_edit(&mut self, ev: &EditEvent) {
        let (start, old_end, new_end) = (ev.start_point.row, ev.old_end_point.row, ev.new_end_point.row);
        let keep = (start + 1).min(self.lines.len());
        let mut removed = self.lines.split_off(keep);
        if let Some(line) = self.lines.get_mut(start) {
            line.tokens = None;
        }
        let first_old_tail = old_end - start;
        self.tail = if self.tail.is_none() && removed.len() > first_old_tail {
            Some(Tail { lines: removed.split_off(first_old_tail), first_new_line: new_end + 1 })
        } else {
            None
        };
    }

    /// Tokenizes lines while `keep_going` returns true. Returns true when every line is done.
    pub fn parse_with_budget(&mut self, rope: &Rope, mut keep_going: impl FnMut() -> bool) -> bool {
        let total = rope.len_lines();
        self.lines.truncate(total);
        let mut count = 0;
        while let Some(i) = self.next_untokenized(total) {
            let text = line_text(rope, i);
            let mut state = self.lines[i].state.clone();
            let tokens = self.tokenize_line(&text, &mut state);
            self.lines[i].tokens = Some(tokens);
            let next = i + 1;
            if next < total {
                if !self.splice_tail(next, &state) {
                    self.lines.push(LineInfo { state, tokens: None });
                }
            } else {
                self.tail = None;
            }
            count += 1;
            if count % 16 == 0 && !keep_going() {
                return self.next_untokenized(total).is_none();
            }
        }
        true
    }

    /// The line to tokenize next. Only the last stored line can lack tokens: edits truncate
    /// the list at the edited line, and tokenizing a line stores its successor's state.
    fn next_untokenized(&mut self, total: usize) -> Option<usize> {
        let last = self.lines.len().checked_sub(1)?;
        if self.lines[last].tokens.is_none() {
            return Some(last);
        }
        if self.lines.len() < total {
            // The successor's state is missing; retokenize the last line to produce it.
            self.lines[last].tokens = None;
            return Some(last);
        }
        None
    }

    /// Reuses the lines tokenized before the edit when line `next` starts in the state they
    /// recorded for it.
    fn splice_tail(&mut self, next: usize, state: &State) -> bool {
        let Some(tail) = self.tail.as_mut() else { return false };
        if next < tail.first_new_line {
            return false;
        }
        let k = next - tail.first_new_line;
        match tail.lines.get(k) {
            Some(old) if old.state == *state => {
                let rest = tail.lines.split_off(k);
                self.tail = None;
                self.lines.extend(rest);
                true
            }
            Some(_) => false,
            None => {
                self.tail = None;
                false
            }
        }
    }

    pub fn highlight(&self, rope: &Rope, lines: Range<usize>) -> Highlights {
        let lines = lines.start.min(rope.len_lines())..lines.end.min(rope.len_lines());
        let rows = lines
            .clone()
            .map(|l| {
                let len = rustaco_core::line_len(rope, l).min(MAX_HIGHLIGHT_COLUMNS);
                let mut row = vec![0u32; len];
                if let Some(tokens) = self.lines.get(l).and_then(|i| i.tokens.as_ref()) {
                    let slice = rope.line(l);
                    for &(s, e, style) in tokens {
                        let from = slice.byte_to_char(s.min(slice.len_bytes())).min(len);
                        let to = slice.byte_to_char(e.min(slice.len_bytes())).min(len);
                        row[from..to].fill(style);
                    }
                }
                row
            })
            .collect();
        Highlights::from_paint(lines.start, rows)
    }

    /// Compiles `src` (TextMate syntax) for a search at byte `pos`, where `\G` holds only when
    /// `pos` is the current anchor.
    ///
    /// Regexes are borrowed, never cloned: a clone starts with empty search caches, and
    /// rebuilding them made every search several times slower.
    fn regex_for(&mut self, src: &Arc<str>, line: &str, pos: usize, anchor: Option<usize>) -> Option<&Regex> {
        let at = (src.contains("\\G") && anchor == Some(pos)).then(|| line[..pos].chars().count());
        self.dynamic
            .entry((src.clone(), at))
            .or_insert_with(|| translate(src).and_then(|t| Regex::new(&instantiate(&t, at)).ok()))
            .as_ref()
    }

    /// The regex of a grammar pattern for a search at byte `pos`.
    fn pattern_regex<'a>(
        &'a mut self,
        pattern: &'a Pattern,
        line: &str,
        pos: usize,
        anchor: Option<usize>,
    ) -> Option<&'a Regex> {
        if !pattern.has_g || anchor != Some(pos) {
            return pattern.never.as_ref();
        }
        let template = pattern.template.clone()?;
        let at = Some(line[..pos].chars().count());
        self.dynamic
            .entry((template.clone(), at))
            .or_insert_with(|| Regex::new(&instantiate(&template, at)).ok())
            .as_ref()
    }

    /// Tokenizes one line (without its terminator). As in `vscode-textmate`, the line is
    /// matched with a `\n` appended: grammars end line comments and similar blocks on it.
    fn tokenize_line(&mut self, line: &str, state: &mut State) -> Vec<(usize, usize, u32)> {
        let line = if line.len() > MAX_LINE_BYTES { truncate(line, MAX_LINE_BYTES) } else { line };
        let owned = format!("{line}\n");
        let line = owned.as_str();
        let grammar = self.grammar.clone();
        let g = &*grammar;
        let mut tokens = Vec::new();
        let mut pos = 0;
        // Where each open block's `begin` (or `while`) ended on this line; `\G` matches there.
        let mut anchors: Vec<Option<usize>> = vec![None; state.len()];

        // `while` blocks continue only while their pattern matches at the start of the line.
        let mut depth = 1;
        while depth < state.len() {
            let frame = state[depth].clone();
            if let (Kind::Block { is_while: true, close_captures, .. }, Some(src)) =
                (&g.rules[frame.rule].kind, &frame.close)
            {
                let matched = self
                    .regex_for(src, line, pos, Some(pos))
                    .and_then(|r| r.captures_from_pos(line, pos).ok().flatten());
                match matched {
                    Some(c) if c.get(0).is_some_and(|m| m.start() == pos) => {
                        let m = c.get(0).unwrap_or_else(|| unreachable!());
                        emit_captures(g, &state[..=depth], None, &c, close_captures, &mut tokens);
                        pos = m.end();
                        anchors[depth] = Some(pos);
                        depth += 1;
                    }
                    _ => {
                        state.truncate(depth);
                        anchors.truncate(depth);
                        break;
                    }
                }
            } else {
                depth += 1;
            }
        }

        let mut next_match: HashMap<RuleId, Option<usize>> = HashMap::new();
        let mut guard = 0;
        while pos <= line.len() && guard < 10_000 {
            guard += 1;
            let top = state.last().cloned().unwrap_or_else(|| unreachable!());
            let anchor = anchors.last().copied().flatten();
            let (close_src, end_last, close_captures_rule) = match &g.rules[top.rule].kind {
                Kind::Block { is_while: false, end_last, .. } => (top.close.clone(), *end_last, Some(top.rule)),
                _ => (None, false, None),
            };
            let candidates: &[RuleId] = g.flattened.get(&top.rule).map_or(&[], Vec::as_slice);

            // Find the earliest match; the end pattern first unless applyEndPatternLast. Only
            // match starts are searched here; captures are computed for the winner alone.
            let end_start = close_src
                .as_ref()
                .and_then(|src| self.regex_for(src, line, pos, anchor))
                .and_then(|re| re.find_from_pos(line, pos).ok().flatten())
                .map(|m| m.start());
            let mut best: Option<(usize, Option<RuleId>)> = None;
            if !end_last && let Some(s) = end_start {
                best = Some((s, None));
            }
            for &rule in candidates {
                let pattern = match &g.rules[rule].kind {
                    Kind::Match(p, _) => p,
                    Kind::Block { begin, .. } => begin,
                    _ => continue,
                };
                if pattern.anchored && anchor != Some(pos) {
                    continue;
                }
                // A match found earlier on this line at or after `pos` is still the next one:
                // nothing matched between the earlier search position and it.
                let cacheable = !pattern.has_g || anchor != Some(pos);
                let found = match next_match.get(&rule) {
                    Some(&cached) if cacheable && cached.is_none_or(|s| s >= pos) => cached,
                    _ => {
                        let Some(re) = self.pattern_regex(pattern, line, pos, anchor) else { continue };
                        let found = re.find_from_pos(line, pos).ok().flatten().map(|m| m.start());
                        if cacheable {
                            next_match.insert(rule, found);
                        }
                        found
                    }
                };
                let Some(start) = found else { continue };
                if pattern.anchored && start != pos {
                    continue;
                }
                if best.is_none_or(|b| start < b.0) {
                    best = Some((start, Some(rule)));
                    if start == pos {
                        break;
                    }
                }
            }
            if end_last
                && let Some(s) = end_start
                && best.is_none_or(|b| s < b.0)
            {
                best = Some((s, None));
            }
            let best = best.and_then(|(start, rule)| {
                let re = match rule {
                    None => self.regex_for(close_src.as_ref()?, line, pos, anchor),
                    Some(r) => match &g.rules[r].kind {
                        Kind::Match(p, _) | Kind::Block { begin: p, .. } => self.pattern_regex(p, line, pos, anchor),
                        _ => None,
                    },
                };
                let caps = re?.captures_from_pos(line, start).ok().flatten()?;
                Some((start, rule, caps))
            });

            let Some((start, rule, caps)) = best else {
                push_token(&mut tokens, pos, line.len(), g.style_of(state, None));
                break;
            };
            let end = caps.get(0).map_or(start, |m| m.end());
            push_token(&mut tokens, pos, start, g.style_of(state, None));

            match rule {
                None => {
                    // The block ends: its end captures are styled with the block's name scope
                    // but without its content scope.
                    let mut outer = state.clone();
                    if let Some(f) = outer.last_mut() {
                        f.content = None;
                    }
                    let captures = match close_captures_rule.map(|r| &g.rules[r].kind) {
                        Some(Kind::Block { close_captures, .. }) => close_captures,
                        _ => &CaptureScopes::default(),
                    };
                    emit_captures(g, &outer, None, &caps, captures, &mut tokens);
                    if state.len() > 1 {
                        state.pop();
                        anchors.pop();
                    }
                    pos = end;
                    if start == end && state.len() <= 1 {
                        break;
                    }
                }
                Some(rule) => match &g.rules[rule].kind {
                    Kind::Match(_, captures) => {
                        emit_captures(g, state, g.rules[rule].name, &caps, captures, &mut tokens);
                        if end == pos {
                            // An empty match would loop forever; step over one char.
                            let step = line[pos..].chars().next().map_or(1, char::len_utf8);
                            push_token(&mut tokens, pos, (pos + step).min(line.len()), g.style_of(state, None));
                            pos += step;
                        } else {
                            pos = end;
                        }
                    }
                    Kind::Block { close, begin_captures, content_name, .. } => {
                        let resolved: Arc<str> = substitute_backrefs(close, &caps).into();
                        let frame = Frame { rule, name: g.rules[rule].name, content: None, close: Some(resolved) };
                        state.push(frame);
                        anchors.push(Some(end));
                        emit_captures(g, state, None, &caps, begin_captures, &mut tokens);
                        if let Some(f) = state.last_mut() {
                            f.content = *content_name;
                        }
                        if state.len() > MAX_DEPTH || (end == pos && start == pos && guard > 1000) {
                            state.pop();
                            anchors.pop();
                            break;
                        }
                        pos = end;
                    }
                    _ => break,
                },
            }
        }
        tokens
    }
}

/// Grammars bundled with the TextMate engine, by language id. Sources and licenses are
/// listed in `textmate/README.md` and THIRD_PARTY.md.
const GRAMMARS: &[(&str, &str)] = &[
    ("sql", include_str!("../textmate/sql.tmlanguage.json")),
    ("kotlin", include_str!("../textmate/kotlin.tmlanguage.json")),
    ("dockerfile", include_str!("../textmate/docker.tmlanguage.json")),
];

/// The bundled grammar for `language`, loaded on first use.
pub fn grammar(language: &str) -> Option<Arc<Grammar>> {
    use std::sync::{Mutex, OnceLock};
    static LOADED: OnceLock<Mutex<HashMap<&'static str, Arc<Grammar>>>> = OnceLock::new();
    let (id, source) = GRAMMARS.iter().find(|(id, _)| *id == language)?;
    let mut loaded = LOADED.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if let Some(g) = loaded.get(id) {
        return Some(g.clone());
    }
    let g = Arc::new(Grammar::from_json(source).ok()?);
    loaded.insert(id, g.clone());
    Some(g)
}

/// Ids of the languages with a bundled TextMate grammar.
pub fn textmate_languages() -> impl Iterator<Item = &'static str> {
    GRAMMARS.iter().map(|(id, _)| *id)
}

fn push_token(tokens: &mut Vec<(usize, usize, u32)>, start: usize, end: usize, style: u32) {
    if start < end && style != 0 {
        tokens.push((start, end, style));
    }
}

fn emit_captures(
    g: &Grammar,
    frames: &[Frame],
    name: Option<ScopeId>,
    caps: &Captures<'_>,
    scopes: &CaptureScopes,
    tokens: &mut Vec<(usize, usize, u32)>,
) {
    let Some(whole) = caps.get(0) else { return };
    push_token(tokens, whole.start(), whole.end(), g.style_of(frames, name));
    for &(i, scope) in &scopes.0 {
        if let Some(m) = caps.get(i) {
            let base = g.style_of(frames, name);
            let own = g.scope_styles[scope as usize];
            push_token(tokens, m.start(), m.end(), if own != 0 { own } else { base });
        }
    }
}

/// Replaces `\1`..`\9` in an end pattern with the escaped text of `begin`'s captures.
fn substitute_backrefs(src: &str, caps: &Captures<'_>) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some(d @ '1'..='9') => {
                    let i = d.to_digit(10).unwrap_or(0) as usize;
                    chars.next();
                    if let Some(m) = caps.get(i) {
                        out.push_str(&fancy_regex::escape(m.as_str()));
                    }
                }
                Some(&n) => {
                    out.push('\\');
                    out.push(n);
                    chars.next();
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn line_text(rope: &Rope, line: usize) -> String {
    let slice = rope.line(line);
    let len = rustaco_core::line_len(rope, line);
    slice.slice(..len).to_string()
}

fn truncate(s: &str, max: usize) -> &str {
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translation() {
        assert_eq!(translate(r"\h+").as_deref(), Some("[0-9a-fA-F]+"));
        assert_eq!(translate(r"a*+b").as_deref(), Some("a*b"));
        assert_eq!(translate(r"[\h_]").as_deref(), Some("[0-9a-fA-F_]"));
        assert_eq!(translate(r"(?!\G)").map(|t| instantiate(&t, Some(3))).as_deref(), Some(r"(?!(?<=\A[\s\S]{3}))"));
        assert_eq!(translate(r"(?!\G)").map(|t| instantiate(&t, None)).as_deref(), Some("(?!(?!))"));
    }

    #[test]
    fn scopes_map_to_highlight_names() {
        let name = |s: &str| crate::names::name_of_style(style_for_scope(s));
        assert_eq!(name("keyword.other.DML.sql"), Some("keyword"));
        assert_eq!(name("comment.line.double-dash.sql"), Some("comment"));
        assert_eq!(name("string.quoted.single.sql"), Some("string"));
        assert_eq!(name("constant.numeric.sql"), Some("number"));
        assert_eq!(name("meta.something"), None);
    }
}
