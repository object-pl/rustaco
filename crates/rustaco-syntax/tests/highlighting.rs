use proptest::prelude::*;
use rustaco_core::{Document, Edit, Selection, Selections};
use rustaco_syntax::{Highlights, Syntax, by_id, name_of_style};

fn highlight_all(doc: &Document, syntax: &mut Syntax) -> Highlights {
    syntax.update(doc.model());
    syntax.highlight(doc.rope(), 0..doc.rope().len_lines())
}

/// The highlight name over each char of `line`, `None` for plain text.
fn names_on(h: &Highlights, line: usize, text: &str) -> Vec<(String, Option<&'static str>)> {
    let line_text: Vec<char> = text.lines().nth(line).unwrap().chars().collect();
    let mut out: Vec<(String, Option<&'static str>)> = Vec::new();
    let mut col = 0;
    for span in h.line(line) {
        if span.start > col {
            out.push((line_text[col..span.start].iter().collect(), None));
        }
        out.push((line_text[span.start..span.end].iter().collect(), name_of_style(span.style)));
        col = span.end;
    }
    out
}

#[test]
fn rust_keywords_functions_and_strings() {
    let src = "fn main() {\n    let s = \"hi\"; // note\n}\n";
    let doc = Document::new(src);
    let mut syntax = Syntax::new(by_id("rust").unwrap()).unwrap();
    let h = highlight_all(&doc, &mut syntax);
    let line0 = names_on(&h, 0, src);
    assert!(line0.contains(&("fn".into(), Some("keyword"))), "{line0:?}");
    assert!(line0.contains(&("main".into(), Some("function"))), "{line0:?}");
    let line1 = names_on(&h, 1, src);
    assert!(line1.contains(&("let".into(), Some("keyword"))), "{line1:?}");
    assert!(line1.iter().any(|(t, n)| t.contains("\"hi\"") && *n == Some("string")), "{line1:?}");
    assert!(line1.contains(&("// note".into(), Some("comment"))), "{line1:?}");
}

#[test]
fn block_comment_spans_lines() {
    let src = "/* one\ntwo\nthree */ fn x() {}";
    let doc = Document::new(src);
    let mut syntax = Syntax::new(by_id("rust").unwrap()).unwrap();
    let h = highlight_all(&doc, &mut syntax);
    assert_eq!(names_on(&h, 1, src), vec![("two".to_string(), Some("comment"))]);
    assert_eq!(names_on(&h, 2, src)[0], ("three */".to_string(), Some("comment")));
}

#[test]
fn highlights_only_requested_lines() {
    let src: String = (0..1000).map(|i| format!("let v{i} = {i};\n")).collect();
    let doc = Document::new(&src);
    let mut syntax = Syntax::new(by_id("javascript").unwrap()).unwrap();
    syntax.update(doc.model());
    let h = syntax.highlight(doc.rope(), 500..510);
    assert!(h.line(499).is_empty());
    assert!(!h.line(505).is_empty());
    assert!(h.line(510).is_empty());
}

#[test]
fn json_keys_and_values() {
    let src = "{\"name\": \"rustaco\", \"n\": 3, \"ok\": true}";
    let doc = Document::new(src);
    let mut syntax = Syntax::new(by_id("json").unwrap()).unwrap();
    let h = highlight_all(&doc, &mut syntax);
    let names = names_on(&h, 0, src);
    assert!(names.iter().any(|(t, n)| t == "3" && *n == Some("number")), "{names:?}");
    assert!(names.iter().any(|(t, n)| t == "true" && n.is_some()), "{names:?}");
}

const RUST_SAMPLE: &str = r#"use std::collections::HashMap;

/// Counts words.
fn count(text: &str) -> HashMap<&str, usize> {
    let mut map = HashMap::new();
    for w in text.split_whitespace() {
        *map.entry(w).or_insert(0) += 1; /* inline */
    }
    map
}

struct Point { x: f64, y: f64 }

impl Point {
    fn len(&self) -> f64 { (self.x * self.x + self.y * self.y).sqrt() }
}
"#;

fn apply_random_edits(edits: &[(usize, usize, &'static str)]) -> (Document, Syntax) {
    let mut doc = Document::new(RUST_SAMPLE);
    let mut incremental = Syntax::new(by_id("rust").unwrap()).unwrap();
    incremental.update(doc.model());
    for &(pos, del, text) in edits {
        let len = doc.len_chars();
        let start = pos % (len + 1);
        let end = (start + del).min(len);
        doc.apply_edits(vec![Edit::new(start, end, text)]);
        if pos % 3 == 0 {
            incremental.update(doc.model());
        }
    }
    incremental.update(doc.model());
    (doc, incremental)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 300, ..ProptestConfig::default() })]

    /// Without syntax errors, highlighting after incremental reparses equals highlighting
    /// after a fresh parse. With errors the trees may legitimately differ: tree-sitter's
    /// error recovery depends on which nodes it could reuse. The edits here keep the code
    /// valid: whitespace and comments inserted next to existing whitespace, and removal of
    /// whitespace that has whitespace beside it.
    #[test]
    fn incremental_equals_fresh_without_errors(edits in prop::collection::vec(
        (any::<usize>(), any::<bool>(), prop::sample::select(vec![" ", "\n", "  ", "/* c */", "\n// note\n", "\n\n"])),
        1..30,
    )) {
        let mut doc = Document::new(RUST_SAMPLE);
        let mut incremental = Syntax::new(by_id("rust").unwrap()).unwrap();
        incremental.update(doc.model());
        for (i, &(pick, delete, text)) in edits.iter().enumerate() {
            let chars: Vec<char> = doc.text().chars().collect();
            let ws = |i: usize| chars.get(i).is_some_and(|c| *c == ' ' || *c == '\n');
            let edit = if delete {
                // A whitespace char with whitespace on at least one side can go without
                // joining two tokens.
                // Only spaces: removing a line break after `//` would comment out the next line.
                let spots: Vec<usize> =
                    (0..chars.len()).filter(|&i| chars[i] == ' ' && (ws(i + 1) || (i > 0 && ws(i - 1)))).collect();
                if spots.is_empty() { continue; }
                let at = spots[pick % spots.len()];
                Edit::delete(at, at + 1)
            } else {
                // Not after `//` on the same line: a line break there would end the comment.
                let in_line_comment = |i: usize| {
                    let line_start = chars[..i].iter().rposition(|c| *c == '\n').map_or(0, |p| p + 1);
                    chars[line_start..i].windows(2).any(|w| w == ['/', '/'])
                };
                let spots: Vec<usize> =
                    (1..chars.len()).filter(|&i| (ws(i) || ws(i - 1)) && !in_line_comment(i)).collect();
                if spots.is_empty() { continue; }
                Edit::insert(spots[pick % spots.len()], text)
            };
            doc.apply_edits(vec![edit]);
            if i % 3 == 0 {
                incremental.update(doc.model());
            }
        }
        incremental.update(doc.model());
        let mut fresh = Syntax::new(by_id("rust").unwrap()).unwrap();
        fresh.update(doc.model());
        let fresh_tree = fresh.tree().unwrap().root_node();
        prop_assert!(!fresh_tree.has_error(), "the generator produced invalid code:\n{}", doc.text());
        prop_assert_eq!(incremental.tree().unwrap().root_node().to_sexp(), fresh_tree.to_sexp());
        let lines = 0..doc.rope().len_lines();
        prop_assert_eq!(incremental.highlight(doc.rope(), lines.clone()), fresh.highlight(doc.rope(), lines));
    }

    /// Any edits, including unbalanced quotes and comments: highlighting never panics and
    /// every span stays inside its line.
    #[test]
    fn highlights_stay_inside_lines(edits in prop::collection::vec(
        (0usize..400, 0usize..6, prop::sample::select(vec![
            "", "x", "\"", "/*", "*/", "{", "}", "fn ", "let ", "\n", "// ", "é", "😀", "'", "r#\"", "\r\n", "\r",
        ])),
        1..25,
    )) {
        let (doc, incremental) = apply_random_edits(&edits);
        let rope = doc.rope();
        let h = incremental.highlight(rope, 0..rope.len_lines());
        for line in 0..rope.len_lines() {
            let len = rustaco_core::line_len(rope, line);
            for span in h.line(line) {
                prop_assert!(span.start < span.end && span.end <= len, "line {} span {:?} len {}", line, span, len);
            }
        }
    }
}

#[test]
fn typing_through_the_editor_keeps_the_tree_in_step() {
    let mut doc = Document::new("fn a() {}\n");
    let mut syntax = Syntax::new(by_id("rust").unwrap()).unwrap();
    syntax.update(doc.model());
    doc.set_selections(Selections::single(Selection::cursor(10)));
    for c in "fn b() { let s = \"x\"; }".chars() {
        doc.insert(&c.to_string());
        syntax.update(doc.model());
    }
    let src = doc.text();
    let h = syntax.highlight(doc.rope(), 0..2);
    let line1 = names_on(&h, 1, &src);
    assert!(line1.contains(&("b".into(), Some("function"))), "{line1:?}");
    assert!(line1.contains(&("let".into(), Some("keyword"))), "{line1:?}");
}

/// A parse halted by its budget again and again ends with the same tree as one full parse,
/// also when an edit arrives while a halted parse is pending.
#[test]
fn budgeted_parse_resumes_to_the_same_tree() {
    let text: String = (0..3000).map(|i| format!("fn f{i}() -> u32 {{ let v = {i}; v + 1 }}\n")).collect();
    let mut doc = Document::new(&text);
    let mut sliced = Syntax::new(by_id("rust").unwrap()).unwrap();
    let mut steps = 0;
    let tiny_budget = |sliced: &mut Syntax, doc: &Document, steps: &mut usize| loop {
        let mut calls = 0;
        *steps += 1;
        if sliced.parse_with_budget(doc.rope(), || {
            calls += 1;
            calls < 3
        }) {
            break;
        }
    };
    tiny_budget(&mut sliced, &doc, &mut steps);
    assert!(steps > 1, "the budget never halted the parse");

    // Edit, start a sliced reparse, edit again before it finishes, then finish.
    let mid = doc.rope().line_to_char(1500) + 3;
    doc.apply_edits(vec![Edit::insert(mid, "x")]);
    for ev in doc.take_events() {
        sliced.apply_edit(&ev);
    }
    let mut calls = 0;
    assert!(!sliced.parse_with_budget(doc.rope(), || {
        calls += 1;
        calls < 2
    }));
    doc.apply_edits(vec![Edit::insert(doc.rope().line_to_char(10), "// added\n")]);
    for ev in doc.take_events() {
        sliced.apply_edit(&ev);
    }
    tiny_budget(&mut sliced, &doc, &mut steps);
    assert!(!sliced.needs_parse());

    let mut fresh = Syntax::new(by_id("rust").unwrap()).unwrap();
    fresh.update(doc.model());
    assert_eq!(sliced.tree().unwrap().root_node().to_sexp(), fresh.tree().unwrap().root_node().to_sexp());
}

#[test]
fn injections_colour_embedded_languages() {
    let html = "<script>\nconst x = 42;\n</script>\n<style>.a { color: red; }</style>\n";
    let doc = Document::new(html);
    let mut syntax = Syntax::new(by_id("html").unwrap()).unwrap();
    let h = highlight_all(&doc, &mut syntax);
    let script = names_on(&h, 1, html);
    assert!(script.contains(&("const".into(), Some("keyword"))), "{script:?}");
    assert!(script.contains(&("42".into(), Some("number"))), "{script:?}");
    let style = names_on(&h, 3, html);
    assert!(style.contains(&("color".into(), Some("property"))), "{style:?}");

    let md = "# Title\n\nSome *em*.\n\n```python\ndef f(): return None\n```\n";
    let doc = Document::new(md);
    let mut syntax = Syntax::new(by_id("markdown").unwrap()).unwrap();
    let h = highlight_all(&doc, &mut syntax);
    assert!(names_on(&h, 2, md).contains(&("em".into(), Some("text.emphasis"))));
    let code = names_on(&h, 5, md);
    assert!(code.contains(&("def".into(), Some("keyword"))), "{code:?}");
    assert!(code.contains(&("None".into(), Some("constant.builtin"))), "{code:?}");
}

#[test]
fn unknown_fence_languages_keep_the_code_block_style() {
    let md = "```klingon\nqapla'\n```\n";
    let doc = Document::new(md);
    let mut syntax = Syntax::new(by_id("markdown").unwrap()).unwrap();
    let h = highlight_all(&doc, &mut syntax);
    assert_eq!(names_on(&h, 1, md), vec![("qapla'".to_string(), Some("text.literal"))]);
}
