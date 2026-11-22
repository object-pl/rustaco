//! Mermaid: a hand-written tokenizer for the diagram language and the word lists its
//! completion offers.
//!
//! Mermaid is line-oriented: a line's colours never depend on the lines before it, apart
//! from which diagram the document is. So there is no parse step; highlighting tokenizes the
//! visible lines and nothing else.

use std::ops::Range;

use rustaco_core::Rope;

use crate::names::style_for_capture;
use crate::spans::{Highlights, MAX_HIGHLIGHT_COLUMNS};

/// Diagram types, the first word of a diagram.
pub const DIAGRAMS: &[&str] = &[
    "flowchart",
    "graph",
    "sequenceDiagram",
    "classDiagram",
    "stateDiagram",
    "stateDiagram-v2",
    "erDiagram",
    "journey",
    "gantt",
    "pie",
    "quadrantChart",
    "requirementDiagram",
    "gitGraph",
    "mindmap",
    "timeline",
    "sankey-beta",
    "xychart-beta",
    "block-beta",
    "packet-beta",
    "kanban",
    "architecture-beta",
    "C4Context",
    "C4Container",
    "C4Component",
    "C4Dynamic",
    "C4Deployment",
];

/// Flowchart directions, after `graph`, `flowchart` and `direction`.
pub const DIRECTIONS: &[&str] = &["TB", "TD", "BT", "RL", "LR"];

/// Keywords inside diagrams, all diagram types together.
pub const KEYWORDS: &[&str] = &[
    "subgraph",
    "end",
    "direction",
    "classDef",
    "class",
    "style",
    "linkStyle",
    "click",
    "call",
    "href",
    "participant",
    "actor",
    "as",
    "loop",
    "alt",
    "else",
    "opt",
    "par",
    "and",
    "rect",
    "critical",
    "break",
    "note",
    "Note",
    "over",
    "left",
    "right",
    "of",
    "activate",
    "deactivate",
    "autonumber",
    "box",
    "title",
    "section",
    "dateFormat",
    "axisFormat",
    "excludes",
    "todayMarker",
    "state",
    "accTitle",
    "accDescr",
    "commit",
    "branch",
    "checkout",
    "merge",
    "showData",
];

/// Node shapes in flowcharts: the opening and closing text around a label, and what the
/// shape is called.
pub const SHAPES: &[(&str, &str, &str)] = &[
    ("[", "]", "rectangle"),
    ("(", ")", "rounded rectangle"),
    ("([", "])", "stadium"),
    ("[[", "]]", "subroutine"),
    ("[(", ")]", "cylinder"),
    ("((", "))", "circle"),
    ("(((", ")))", "double circle"),
    (">", "]", "flag"),
    ("{", "}", "rhombus"),
    ("{{", "}}", "hexagon"),
    ("[/", "/]", "parallelogram"),
    ("[\\", "\\]", "parallelogram, leaning left"),
    ("[/", "\\]", "trapezoid"),
    ("[\\", "/]", "trapezoid, upside down"),
];

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-'
}

fn is_link(c: char) -> bool {
    matches!(c, '-' | '=' | '.' | '<' | '>' | '~')
}

/// The token styles of one line, as (char range, highlight name).
pub fn tokenize(line: &str) -> Vec<(Range<usize>, &'static str)> {
    let chars: Vec<char> = line.chars().take(MAX_HIGHLIGHT_COLUMNS).collect();
    let mut out = Vec::new();
    let mut i = 0;
    // Index of the first word on the line, to tell a diagram type from a node id.
    let mut first_word = true;
    let mut after_direction_word = false;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '%' && chars.get(i + 1) == Some(&'%') {
            let style = if chars.get(i + 2) == Some(&'{') { "attribute" } else { "comment" };
            out.push((i..chars.len(), style));
            break;
        }
        if c == '"' {
            let end = chars[i + 1..].iter().position(|&d| d == '"').map_or(chars.len(), |p| i + 2 + p);
            out.push((i..end, "string"));
            i = end;
            continue;
        }
        if c == '|' {
            // An edge label: `-->|text|`.
            let end = chars[i + 1..].iter().position(|&d| d == '|').map_or(chars.len(), |p| i + 2 + p);
            out.push((i..i + 1, "punctuation.bracket"));
            if end - 1 > i + 1 {
                out.push((i + 1..end - 1, "string"));
            }
            if end <= chars.len() && end > i + 1 && chars[end - 1] == '|' {
                out.push((end - 1..end, "punctuation.bracket"));
            }
            i = end;
            continue;
        }
        if c == ':' {
            if chars.get(i + 1) == Some(&':') && chars.get(i + 2) == Some(&':') {
                // `A:::className`
                out.push((i..i + 3, "operator"));
                let start = i + 3;
                let mut end = start;
                while end < chars.len() && is_word(chars[end]) {
                    end += 1;
                }
                if end > start {
                    out.push((start..end, "type"));
                }
                i = end;
                continue;
            }
            // The rest is text: a message in a sequence diagram, a task in a gantt chart.
            out.push((i..i + 1, "punctuation.delimiter"));
            let start = (i + 1..chars.len()).find(|&k| !chars[k].is_whitespace()).unwrap_or(chars.len());
            let end = chars.iter().rposition(|d| !d.is_whitespace()).map_or(start, |p| p + 1).max(start);
            if end > start {
                out.push((start..end, "string"));
            }
            break;
        }
        if is_link(c) {
            let start = i;
            while i < chars.len() && is_link(chars[i]) {
                i += 1;
            }
            // `--x` and `--o` end a link with a cross or a circle.
            let shaft = chars[start..i].iter().any(|&d| d == '-' || d == '=');
            if shaft
                && i < chars.len()
                && matches!(chars[i], 'x' | 'o')
                && chars.get(i + 1).is_none_or(|&d| !is_word(d))
            {
                i += 1;
            }
            if i - start >= 2 {
                out.push((start..i, "operator"));
            } else if c == '>' {
                // `id>label]`: the flag shape.
                i = shape(&chars, start, &mut out);
            }
            continue;
        }
        if matches!(c, '[' | '(' | '{') {
            i = shape(&chars, i, &mut out);
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            if i < chars.len() && is_word(chars[i]) {
                while i < chars.len() && is_word(chars[i]) {
                    i += 1;
                }
                continue;
            }
            out.push((start..i, "number"));
            continue;
        }
        if is_word(c) {
            let start = i;
            while i < chars.len() && is_word(chars[i]) {
                // A link starts inside the run: `A-->B`.
                if chars[i] == '-' && chars.get(i + 1).is_some_and(|&d| is_link(d)) {
                    break;
                }
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let style = if first_word && DIAGRAMS.contains(&word.as_str()) {
                after_direction_word = matches!(word.as_str(), "graph" | "flowchart");
                Some("keyword")
            } else if after_direction_word && DIRECTIONS.contains(&word.as_str()) {
                after_direction_word = false;
                Some("constant.builtin")
            } else if KEYWORDS.contains(&word.as_str()) {
                after_direction_word = word == "direction";
                Some("keyword")
            } else if chars.get(i).is_some_and(|&d| matches!(d, '[' | '(' | '{' | '>')) {
                // A node id with its shape.
                Some("variable")
            } else {
                None
            };
            if let Some(s) = style {
                out.push((start..i, s));
            }
            first_word = false;
            continue;
        }
        if matches!(c, ';' | ',') {
            out.push((i..i + 1, "punctuation.delimiter"));
        } else if matches!(c, ']' | ')' | '}') {
            out.push((i..i + 1, "punctuation.bracket"));
        }
        i += 1;
    }
    out
}

/// A node shape starting at `start`: the opening brackets, the label as a string, the
/// closing brackets. Returns the index after it.
fn shape(chars: &[char], start: usize, out: &mut Vec<(Range<usize>, &'static str)>) -> usize {
    let mut i = start;
    while i < chars.len() && matches!(chars[i], '[' | '(' | '{' | '>' | '/' | '\\') && i - start < 3 {
        i += 1;
    }
    out.push((start..i, "punctuation.bracket"));
    let closers = |c: char| matches!(c, ']' | ')' | '}' | '/' | '\\');
    let label_start = i;
    // The label runs to the first closing bracket outside quotes.
    let mut quoted = false;
    while i < chars.len() {
        match chars[i] {
            '"' => quoted = !quoted,
            c if !quoted && matches!(c, ']' | ')' | '}') => break,
            c if !quoted && matches!(c, '/' | '\\') && chars.get(i + 1).is_some_and(|&d| matches!(d, ']' | ')')) => {
                break;
            }
            _ => {}
        }
        i += 1;
    }
    if i > label_start {
        out.push((label_start..i, "string"));
    }
    let close = i;
    while i < chars.len() && closers(chars[i]) && i - close < 3 {
        i += 1;
    }
    if i > close {
        out.push((close..i, "punctuation.bracket"));
    }
    i
}

/// Highlights for `lines` of a Mermaid document.
pub fn highlight(rope: &Rope, lines: Range<usize>) -> Highlights {
    let end = lines.end.min(rope.len_lines());
    let first = lines.start.min(end);
    let paint = (first..end)
        .map(|n| {
            let line: String = rope.line(n).chars().filter(|&c| c != '\n' && c != '\r').collect();
            let mut row = vec![0u32; line.chars().count().min(MAX_HIGHLIGHT_COLUMNS)];
            for (range, name) in tokenize(&line) {
                let style = style_for_capture(name);
                for cell in row.iter_mut().take(range.end).skip(range.start) {
                    *cell = style;
                }
            }
            row
        })
        .collect();
    Highlights::from_paint(first, paint)
}

/// The node ids a flowchart document names: words followed by a shape, or on either side of
/// a link. Each once, in order of first appearance.
pub fn node_ids(rope: &Rope) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for line in rope.lines() {
        let line: String = line.chars().filter(|&c| c != '\n' && c != '\r').collect();
        let tokens = tokenize(&line);
        let chars: Vec<char> = line.chars().collect();
        let mut words = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            if is_word(chars[i]) && (i == 0 || !is_word(chars[i - 1])) {
                let start = i;
                while i < chars.len() && is_word(chars[i]) {
                    if chars[i] == '-' && chars.get(i + 1).is_some_and(|&d| is_link(d)) {
                        break;
                    }
                    i += 1;
                }
                if i == start {
                    // A link starts here, not a word.
                    i += 1;
                    continue;
                }
                words.push(start..i);
                continue;
            }
            i += 1;
        }
        for w in words {
            let covered = tokens.iter().any(|(r, s)| r.start <= w.start && w.end <= r.end && *s != "variable");
            if covered {
                continue;
            }
            let text: String = chars[w.clone()].iter().collect();
            if text.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let next = chars[w.end..].iter().find(|c| !c.is_whitespace());
            let prev = chars[..w.start].iter().rev().find(|c| !c.is_whitespace());
            let linked = |c: Option<&char>| c.is_some_and(|&c| is_link(c) || c == '|' || matches!(c, 'x' | 'o'));
            let shaped = chars.get(w.end).is_some_and(|&c| matches!(c, '[' | '(' | '{' | '>'));
            if (shaped || linked(next) || linked(prev) || next == Some(&'&') || prev == Some(&'&'))
                && !ids.contains(&text)
            {
                ids.push(text);
            }
        }
    }
    ids
}

/// The diagram type of a document: the first word of its first line that is not blank, a
/// comment or a directive.
pub fn diagram(rope: &Rope) -> Option<String> {
    let mut front_matter = false;
    for line in rope.lines().take(200) {
        let text = line.to_string();
        let t = text.trim();
        if t == "---" {
            front_matter = !front_matter;
            continue;
        }
        if front_matter || t.is_empty() || t.starts_with("%%") {
            continue;
        }
        let word: String = t.chars().take_while(|&c| is_word(c)).collect();
        return DIAGRAMS.contains(&word.as_str()).then_some(word);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn styles(line: &str) -> Vec<(String, &'static str)> {
        let chars: Vec<char> = line.chars().collect();
        tokenize(line).into_iter().map(|(r, s)| (chars[r].iter().collect(), s)).collect()
    }

    #[test]
    fn flowchart_line() {
        assert_eq!(styles("graph LR"), vec![("graph".into(), "keyword"), ("LR".into(), "constant.builtin")]);
        let s = styles("  A[Start] -->|yes| B{Is it?}:::warn");
        assert!(s.contains(&("A".into(), "variable")));
        assert!(s.contains(&("Start".into(), "string")));
        assert!(s.contains(&("-->".into(), "operator")));
        assert!(s.contains(&("yes".into(), "string")));
        assert!(s.contains(&("Is it?".into(), "string")));
        assert!(s.contains(&(":::".into(), "operator")));
        assert!(s.contains(&("warn".into(), "type")));
    }

    #[test]
    fn links_inside_words_and_comments() {
        let s = styles("A-->B --x C %% done");
        assert!(s.contains(&("-->".into(), "operator")));
        assert!(s.contains(&("--x".into(), "operator")));
        assert!(s.contains(&("%% done".into(), "comment")));
        let s = styles("Alice->>John: Hello John, how are you?");
        assert!(s.contains(&("->>".into(), "operator")));
        assert!(s.contains(&("Hello John, how are you?".into(), "string")));
    }

    #[test]
    fn shapes_and_ids() {
        let s = styles("db[(Database)] --> c((Circle))");
        assert!(s.contains(&("[(".into(), "punctuation.bracket")));
        assert!(s.contains(&("Database".into(), "string")));
        assert!(s.contains(&(")]".into(), "punctuation.bracket")));
        assert!(s.contains(&("Circle".into(), "string")));
        let rope =
            Rope::from_str("flowchart TD\n  start[Go] --> check{Ok?}\n  check -->|no| start\n  check --> done\n");
        assert_eq!(node_ids(&rope), vec!["start", "check", "done"]);
        assert_eq!(diagram(&rope).as_deref(), Some("flowchart"));
    }

    #[test]
    fn highlights_cover_the_tokens() {
        let rope = Rope::from_str("graph TD\nA --> B\n");
        let h = highlight(&rope, 0..3);
        assert_eq!(h.line(0).len(), 2);
        assert_eq!(h.line(1).len(), 1);
        assert_eq!(h.line(1)[0].start, 2);
        assert_eq!(h.line(1)[0].end, 5);
    }
}
