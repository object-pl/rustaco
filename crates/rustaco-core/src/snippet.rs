//! Snippets in VS Code's syntax: tab stops (`$1`, `${1}`), placeholders (`${1:default}`,
//! nested), choices (`${1|one,two|}`), variables (`$TM_FILENAME`, `${CLIPBOARD:default}`) and
//! the final position `$0`.
//!
//! [`Snippet::parse`] turns the source into plain text plus tab stops; [`Editor::insert_snippet`]
//! inserts it at every cursor and starts a session: the cursors select the first tab stop
//! (every place it appears, so mirrors are edited together), Tab and Shift+Tab move between
//! stops, and reaching `$0` or leaving the stops ends the session. Tab stops are decorations,
//! so they follow edits.

// Tab stops hold lists of ranges; a list of one range is the common case, not a mistake.
#![allow(clippy::single_range_in_vec_init)]

use std::collections::HashMap;
use std::ops::Range;

use crate::decorations::{DecorationId, Stickiness};
use crate::editor::Editor;
use crate::history::EditKind;
use crate::selection::{Selection, Selections};

/// A parsed snippet: its text with variables resolved and the tab stops in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snippet {
    pub text: String,
    /// Tab stops by index, ascending, with `$0` last. Ranges are char offsets into `text`.
    pub stops: Vec<TabStop>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabStop {
    pub index: u32,
    pub ranges: Vec<Range<usize>>,
    /// The options of a choice stop.
    pub choices: Option<Vec<String>>,
}

/// The snippet as a tree, before mirrors get their placeholders' text.
#[derive(Clone, Debug)]
enum Node {
    Text(String),
    Stop { index: u32, children: Vec<Node>, choices: Option<Vec<String>> },
}

impl Snippet {
    /// Parses snippet source. `variable` resolves names such as `TM_FILENAME`; an unknown
    /// variable becomes a placeholder holding its name, as in VS Code.
    pub fn parse(source: &str, variable: &dyn Fn(&str) -> Option<String>) -> Snippet {
        let chars: Vec<char> = source.chars().collect();
        let mut p = Parser { chars: &chars, pos: 0, variable, next_auto: 1000 };
        let nodes = p.parse_until(&[]);
        // A stop without content shows its placeholder's text, wherever that is defined.
        let mut defaults: HashMap<u32, String> = HashMap::new();
        collect_defaults(&nodes, &mut defaults);
        let mut out = Render { text: String::new(), len: 0, stops: Vec::new() };
        out.render(&nodes, &defaults);
        let mut stops: Vec<TabStop> = Vec::new();
        let mut order: Vec<(u32, Range<usize>, Option<Vec<String>>)> = out.stops;
        order.sort_by_key(|s| (s.0 == 0, s.0, s.1.start));
        for (index, range, choices) in order {
            match stops.last_mut() {
                Some(last) if last.index == index => {
                    last.ranges.push(range);
                    if last.choices.is_none() {
                        last.choices = choices;
                    }
                }
                _ => stops.push(TabStop { index, ranges: vec![range], choices }),
            }
        }
        if !stops.iter().any(|s| s.index == 0) {
            let end = out.len;
            stops.push(TabStop { index: 0, ranges: vec![end..end], choices: None });
        }
        Snippet { text: out.text, stops }
    }

    /// The snippet as typed into a document: lines after the first get `indent` in front,
    /// and tabs become `unit` (the editor's indentation).
    pub fn indented(&self, indent: &str, unit: &str) -> Snippet {
        let mut text = String::new();
        // Old char offset -> new char offset.
        let mut map: Vec<usize> = Vec::with_capacity(self.text.chars().count() + 1);
        let mut at_line_start = false;
        let mut new_len = 0;
        for c in self.text.chars() {
            if at_line_start && c != '\n' {
                text.push_str(indent);
                new_len += indent.chars().count();
                at_line_start = false;
            }
            map.push(new_len);
            if c == '\t' && unit != "\t" {
                text.push_str(unit);
                new_len += unit.chars().count();
            } else {
                text.push(c);
                new_len += 1;
            }
            if c == '\n' {
                at_line_start = true;
            }
        }
        map.push(new_len);
        let stops = self
            .stops
            .iter()
            .map(|s| TabStop {
                index: s.index,
                ranges: s.ranges.iter().map(|r| map[r.start]..map[r.end]).collect(),
                choices: s.choices.clone(),
            })
            .collect();
        Snippet { text, stops }
    }
}

fn plain_text(nodes: &[Node]) -> String {
    nodes
        .iter()
        .map(|n| match n {
            Node::Text(t) => t.clone(),
            Node::Stop { children, .. } => plain_text(children),
        })
        .collect()
}

fn collect_defaults(nodes: &[Node], out: &mut HashMap<u32, String>) {
    for n in nodes {
        if let Node::Stop { index, children, choices } = n {
            let text = match choices {
                Some(c) => c.first().cloned().unwrap_or_default(),
                None => plain_text(children),
            };
            if !text.is_empty() {
                out.entry(*index).or_insert(text);
            }
            collect_defaults(children, out);
        }
    }
}

struct Render {
    text: String,
    len: usize,
    stops: Vec<(u32, Range<usize>, Option<Vec<String>>)>,
}

impl Render {
    fn push(&mut self, s: &str) {
        self.text.push_str(s);
        self.len += s.chars().count();
    }

    fn render(&mut self, nodes: &[Node], defaults: &HashMap<u32, String>) {
        for n in nodes {
            match n {
                Node::Text(t) => self.push(t),
                Node::Stop { index, children, choices } => {
                    let from = self.len;
                    if let Some(c) = choices {
                        self.push(c.first().map_or("", String::as_str));
                    } else if children.is_empty() {
                        if let Some(d) = defaults.get(index) {
                            self.push(d);
                        }
                    } else {
                        self.render(children, defaults);
                    }
                    self.stops.push((*index, from..self.len, choices.clone()));
                }
            }
        }
    }
}

struct Parser<'a> {
    chars: &'a [char],
    pos: usize,
    variable: &'a dyn Fn(&str) -> Option<String>,
    /// Indexes for unknown variables turned into placeholders, after the snippet's own.
    next_auto: u32,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    /// Parses text and elements until one of `stop` (unescaped) or the end.
    fn parse_until(&mut self, stop: &[char]) -> Vec<Node> {
        let mut nodes = Vec::new();
        let mut text = String::new();
        while let Some(c) = self.peek() {
            if stop.contains(&c) {
                break;
            }
            match c {
                '\\' => {
                    let next = self.chars.get(self.pos + 1).copied();
                    if matches!(next, Some('$' | '}' | '\\')) || next.is_some_and(|n| stop.contains(&n)) {
                        text.push(next.expect("checked"));
                        self.pos += 2;
                    } else {
                        text.push('\\');
                        self.pos += 1;
                    }
                }
                '$' => match self.element() {
                    Some(node) => {
                        if !text.is_empty() {
                            nodes.push(Node::Text(std::mem::take(&mut text)));
                        }
                        nodes.extend(node);
                    }
                    None => {
                        text.push('$');
                        self.pos += 1;
                    }
                },
                _ => {
                    text.push(c);
                    self.pos += 1;
                }
            }
        }
        if !text.is_empty() {
            nodes.push(Node::Text(text));
        }
        nodes
    }

    fn number(&mut self) -> Option<u32> {
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
        }
        if self.pos == start {
            return None;
        }
        self.chars[start..self.pos].iter().collect::<String>().parse().ok()
    }

    fn name(&mut self) -> Option<String> {
        let start = self.pos;
        if !self.peek().is_some_and(|c| c == '_' || c.is_ascii_alphabetic()) {
            return None;
        }
        while self.peek().is_some_and(|c| c == '_' || c.is_ascii_alphanumeric()) {
            self.pos += 1;
        }
        Some(self.chars[start..self.pos].iter().collect())
    }

    fn close(&mut self) {
        if self.peek() == Some('}') {
            self.pos += 1;
        }
    }

    /// Parses one `$...` element at `pos` into nodes; None (and `pos` unchanged) when it is
    /// not one.
    fn element(&mut self) -> Option<Vec<Node>> {
        let start = self.pos;
        self.pos += 1; // '$'
        let stop = |index| Node::Stop { index, children: Vec::new(), choices: None };
        if let Some(index) = self.number() {
            return Some(vec![stop(index)]);
        }
        if let Some(name) = self.name() {
            return Some(self.variable_value(&name, None));
        }
        if self.peek() != Some('{') {
            self.pos = start;
            return None;
        }
        self.pos += 1;
        if let Some(index) = self.number() {
            match self.peek() {
                Some('}') => {
                    self.pos += 1;
                    return Some(vec![stop(index)]);
                }
                Some(':') => {
                    self.pos += 1;
                    let children = self.parse_until(&['}']);
                    self.close();
                    return Some(vec![Node::Stop { index, children, choices: None }]);
                }
                Some('|') => {
                    self.pos += 1;
                    let mut options = Vec::new();
                    let mut current = String::new();
                    while let Some(c) = self.peek() {
                        self.pos += 1;
                        match c {
                            '\\' if matches!(self.peek(), Some(',' | '|' | '\\')) => {
                                current.push(self.peek().expect("checked"));
                                self.pos += 1;
                            }
                            ',' => options.push(std::mem::take(&mut current)),
                            '|' => break,
                            _ => current.push(c),
                        }
                    }
                    options.push(current);
                    self.close();
                    return Some(vec![Node::Stop { index, children: Vec::new(), choices: Some(options) }]);
                }
                Some('/') => {
                    // A transform: its result depends on text typed later; keep the stop.
                    self.skip_to_close();
                    return Some(vec![stop(index)]);
                }
                _ => {
                    self.pos = start;
                    return None;
                }
            }
        }
        if let Some(name) = self.name() {
            match self.peek() {
                Some('}') => {
                    self.pos += 1;
                    return Some(self.variable_value(&name, None));
                }
                Some(':') => {
                    self.pos += 1;
                    let default = self.parse_until(&['}']);
                    self.close();
                    return Some(self.variable_value(&name, Some(default)));
                }
                Some('/') => {
                    self.skip_to_close();
                    return Some(self.variable_value(&name, None));
                }
                _ => {}
            }
        }
        self.pos = start;
        None
    }

    fn skip_to_close(&mut self) {
        let mut depth = 0;
        while let Some(c) = self.peek() {
            self.pos += 1;
            match c {
                '\\' => self.pos += 1,
                '{' => depth += 1,
                '}' if depth == 0 => return,
                '}' => depth -= 1,
                _ => {}
            }
        }
    }

    fn variable_value(&mut self, name: &str, default: Option<Vec<Node>>) -> Vec<Node> {
        if let Some(value) = (self.variable)(name) {
            return vec![Node::Text(value)];
        }
        match default {
            Some(nodes) => nodes,
            None => {
                // Unknown: a placeholder with the name, so it is easy to fill in.
                let index = self.next_auto;
                self.next_auto += 1;
                vec![Node::Stop { index, children: vec![Node::Text(name.to_owned())], choices: None }]
            }
        }
    }
}

/// An active snippet: its tab stops as decorations and the stop being edited.
#[derive(Clone, Debug)]
pub struct SnippetSession {
    /// Per stop (in order, `$0` last): its decorations and choices.
    stops: Vec<(Vec<DecorationId>, Option<Vec<String>>)>,
    current: usize,
    owner: u32,
}

/// Decoration kind of snippet tab stops.
pub const SNIPPET_STOP: u32 = 3;

/// Snippet sessions own decorations from this owner id up (plus the editor's id).
pub const SNIPPET_OWNER: u32 = 1 << 18;

impl SnippetSession {
    /// The options of the current stop, when it is a choice.
    pub fn choices(&self) -> Option<&[String]> {
        self.stops.get(self.current).and_then(|s| s.1.as_deref())
    }

    pub fn owner(&self) -> u32 {
        self.owner
    }
}

impl Editor<'_> {
    /// Inserts `snippet` at every selection, replacing selected text, indents its lines like
    /// the line it lands on, and selects its first tab stop.
    pub fn insert_snippet(&mut self, snippet: &Snippet) {
        self.end_snippet();
        let o = self.model.options;
        let unit = if o.insert_spaces { " ".repeat(o.tab_size) } else { "\t".to_owned() };
        let rope = &self.model.rope;
        // One indented copy per selection, placed at its start.
        let copies: Vec<(usize, Snippet)> = self
            .cursors
            .selections
            .iter()
            .map(|s| {
                let line = rope.char_to_line(s.start());
                let indent: String = rope.line(line).chars().take_while(|c| *c == ' ' || *c == '\t').collect();
                (s.start(), snippet.indented(&indent, &unit))
            })
            .collect();
        let texts: Vec<String> = copies.iter().map(|(_, s)| s.text.clone()).collect();
        let before: Vec<Selection> = self.cursors.selections.iter().cloned().collect();
        self.replace_each(EditKind::Other, |_, i, _| texts[i].clone());
        // Where each copy landed: selection starts shift by the text inserted before them.
        let mut shift = 0isize;
        let mut starts = Vec::with_capacity(copies.len());
        for (i, s) in before.iter().enumerate() {
            starts.push(s.start().wrapping_add_signed(shift));
            shift += copies[i].1.text.chars().count() as isize - s.len() as isize;
        }
        let owner = SNIPPET_OWNER + self.cursors.id().0;
        let mut stops = Vec::new();
        for stop in &snippet.stops {
            let ranges: Vec<Range<usize>> = copies
                .iter()
                .zip(&starts)
                .flat_map(|((_, copy), &start)| {
                    let s = copy.stops.iter().find(|t| t.index == stop.index).expect("same stops in every copy");
                    s.ranges.iter().map(move |r| start + r.start..start + r.end).collect::<Vec<_>>()
                })
                .collect();
            stops.push((ranges, stop.choices.clone()));
        }
        // All stops share one owner; each gets its own ids, kept per stop.
        let all: Vec<(Range<usize>, u32)> =
            stops.iter().flat_map(|(ranges, _)| ranges.iter().map(|r| (r.clone(), SNIPPET_STOP))).collect();
        let ids = self.model.decorations.set_each(owner, Stickiness::AlwaysGrows, all);
        let mut it = ids.into_iter();
        let stops =
            stops.into_iter().map(|(ranges, choices)| (it.by_ref().take(ranges.len()).collect(), choices)).collect();
        self.cursors.snippet = Some(SnippetSession { stops, current: 0, owner });
        self.select_stop(0);
        self.model.history.break_group();
    }

    /// Selects every range of tab stop `i` of the active session.
    fn select_stop(&mut self, i: usize) {
        let Some(session) = self.cursors.snippet.as_mut() else { return };
        session.current = i;
        let ranges: Vec<Range<usize>> = session.stops[i]
            .0
            .iter()
            .filter_map(|id| self.model.decorations.get(*id).map(|d| d.range.clone()))
            .collect();
        let last = i + 1 == session.stops.len();
        if ranges.is_empty() {
            return;
        }
        let sels: Vec<Selection> = ranges.iter().map(|r| Selection::new(r.start, r.end)).collect();
        self.set_selections(Selections::new(sels, 0));
        if last {
            self.end_snippet();
        }
    }

    /// Moves to the next tab stop; at `$0` the session ends. False without a session.
    pub fn next_snippet_stop(&mut self) -> bool {
        let Some(session) = self.cursors.snippet.as_ref() else { return false };
        let next = (session.current + 1).min(session.stops.len() - 1);
        self.select_stop(next);
        true
    }

    /// Moves to the previous tab stop. False without a session.
    pub fn prev_snippet_stop(&mut self) -> bool {
        let Some(session) = self.cursors.snippet.as_ref() else { return false };
        let prev = session.current.saturating_sub(1);
        self.select_stop(prev);
        true
    }

    /// Ends the active snippet session, if any.
    pub fn end_snippet(&mut self) {
        if let Some(session) = self.cursors.snippet.take() {
            self.model.decorations.clear(session.owner);
        }
    }

    /// Ends the session when the primary caret has left every tab stop.
    pub fn check_snippet(&mut self) {
        let Some(session) = self.cursors.snippet.as_ref() else { return };
        let head = self.cursors.selections.primary().head;
        let inside =
            session.stops.iter().flat_map(|s| s.0.iter()).any(|id| {
                self.model.decorations.get(*id).is_some_and(|d| d.range.start <= head && head <= d.range.end)
            });
        if !inside {
            self.end_snippet();
        }
    }

    /// The active snippet session.
    pub fn snippet(&self) -> Option<&SnippetSession> {
        self.cursors.snippet.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Snippet {
        Snippet::parse(s, &|name| (name == "TM_FILENAME").then(|| "main.rs".to_owned()))
    }

    #[test]
    fn stops_placeholders_and_final_position() {
        let s = parse("fn ${1:name}($2) {\n\t$0\n}");
        assert_eq!(s.text, "fn name() {\n\t\n}");
        assert_eq!(
            s.stops.iter().map(|t| (t.index, t.ranges.clone())).collect::<Vec<_>>(),
            vec![(1, vec![3..7]), (2, vec![8..8]), (0, vec![13..13]),]
        );
    }

    #[test]
    fn nested_placeholders_choices_variables_and_escapes() {
        let s = parse("${1:outer ${2:inner}} ${3|a,b\\,c|} $TM_FILENAME ${NOPE} \\$1 ${UNSET:dflt}");
        assert_eq!(s.text, "outer inner a main.rs NOPE $1 dflt");
        let stop = |i: u32| s.stops.iter().find(|t| t.index == i).unwrap();
        assert_eq!(stop(1).ranges, vec![0..11]);
        assert_eq!(stop(2).ranges, vec![6..11]);
        assert_eq!(stop(3).choices.as_deref(), Some(&["a".to_owned(), "b,c".to_owned()][..]));
        assert!(
            s.stops.iter().any(|t| t.index >= 1000 && t.ranges == vec![22..26]),
            "unknown variable is a placeholder"
        );
        assert_eq!(s.stops.last().unwrap().index, 0);
    }

    #[test]
    fn mirrors_share_an_index_and_the_default() {
        let s = parse("${1:x} = $1;");
        assert_eq!(s.text, "x = x;");
        assert_eq!(s.stops[0].ranges, vec![0..1, 4..5]);
    }

    #[test]
    fn indentation_follows_the_line() {
        let s = parse("if {\n\t$1\n}").indented("    ", "  ");
        assert_eq!(s.text, "if {\n      \n    }");
        assert_eq!(s.stops[0].ranges, vec![11..11]);
    }
}
