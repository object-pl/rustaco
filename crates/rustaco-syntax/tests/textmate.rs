use proptest::prelude::*;
use rustaco_core::{Document, Edit};
use rustaco_syntax::{Highlights, TextMateSyntax, name_of_style, textmate_grammar, textmate_languages};

fn tokenize(doc: &Document, syntax: &mut TextMateSyntax) -> Highlights {
    assert!(syntax.parse_with_budget(doc.rope(), || true));
    syntax.highlight(doc.rope(), 0..doc.rope().len_lines())
}

fn names_on(h: &Highlights, line: usize, text: &str) -> Vec<(String, Option<&'static str>)> {
    let chars: Vec<char> = text.lines().nth(line).unwrap().chars().collect();
    h.line(line).iter().map(|s| (chars[s.start..s.end].iter().collect(), name_of_style(s.style))).collect()
}

fn highlighted(language: &str, text: &str) -> (Document, Highlights) {
    let doc = Document::new(text);
    let mut syntax = TextMateSyntax::new(textmate_grammar(language).unwrap());
    let h = tokenize(&doc, &mut syntax);
    (doc, h)
}

#[test]
fn bundled_grammars_load_with_few_unsupported_regexes() {
    for id in textmate_languages() {
        let g = textmate_grammar(id).unwrap_or_else(|| panic!("{id} failed to load"));
        println!("{id}: {} regexes not supported by fancy-regex", g.failed_regexes);
        assert!(g.failed_regexes <= 3, "{id}: {} regexes failed", g.failed_regexes);
    }
}

#[test]
fn sql_keywords_strings_comments() {
    let src = "SELECT id, name FROM users WHERE id = 'a' AND n > 10; -- note\n";
    let (_, h) = highlighted("sql", src);
    let names = names_on(&h, 0, src);
    assert!(names.iter().any(|(t, n)| t.starts_with("SELECT") && *n == Some("keyword")), "{names:?}");
    assert!(names.iter().any(|(t, n)| t.contains("'a'") && *n == Some("string")), "{names:?}");
    assert!(names.iter().any(|(t, n)| t.contains("10") && *n == Some("number")), "{names:?}");
    assert!(names.iter().any(|(t, n)| t.contains("note") && *n == Some("comment")), "{names:?}");
}

#[test]
fn sql_block_comment_spans_lines() {
    let src = "/* one\ntwo\nthree */ SELECT 1;\n";
    let (_, h) = highlighted("sql", src);
    assert_eq!(names_on(&h, 1, src), vec![("two".to_string(), Some("comment"))]);
    assert!(names_on(&h, 2, src).iter().any(|(t, n)| t.contains("SELECT") && *n == Some("keyword")));
}

#[test]
fn kotlin_and_dockerfile() {
    let src = "fun main() {\n    val greeting = \"hi\"\n}\n";
    let (_, h) = highlighted("kotlin", src);
    assert!(
        names_on(&h, 0, src).iter().any(|(t, n)| t == "fun" && n.is_some_and(|n| n.starts_with("keyword"))),
        "{:?}",
        names_on(&h, 0, src)
    );
    assert!(
        names_on(&h, 1, src).iter().any(|(t, n)| t.contains("hi") && *n == Some("string")),
        "{:?}",
        names_on(&h, 1, src)
    );

    let src = "FROM rust:1 AS build\nRUN cargo build --release # compile\n";
    let (_, h) = highlighted("dockerfile", src);
    assert!(names_on(&h, 0, src).iter().any(|(t, n)| t.contains("FROM") && n.is_some()), "{:?}", names_on(&h, 0, src));
    assert!(names_on(&h, 1, src).iter().any(|(t, n)| t.contains("RUN") && n.is_some()), "{:?}", names_on(&h, 1, src));
}

const SQL_SAMPLE: &str = "-- report\nSELECT a.id, b.name\nFROM a\nJOIN b ON a.id = b.a_id\n/* filter\n   by date */\nWHERE a.created > '2026-01-01'\nORDER BY 2 DESC;\n";

proptest! {
    #![proptest_config(ProptestConfig { cases: 300, ..ProptestConfig::default() })]

    /// Tokens after incremental retokenization (with early stop at a matching line state)
    /// equal tokens from tokenizing the final text from scratch.
    #[test]
    fn incremental_equals_fresh(edits in prop::collection::vec(
        (0usize..200, 0usize..5, prop::sample::select(vec!["", "x", "'", "/*", "*/", "--", "\n", " SELECT ", "é"])),
        1..20,
    )) {
        let mut doc = Document::new(SQL_SAMPLE);
        let grammar = textmate_grammar("sql").unwrap();
        let mut incremental = TextMateSyntax::new(grammar.clone());
        incremental.parse_with_budget(doc.rope(), || true);
        let _ = doc.take_events();
        for (i, &(pos, del, text)) in edits.iter().enumerate() {
            let len = doc.len_chars();
            let start = pos % (len + 1);
            doc.apply_edits(vec![Edit::new(start, (start + del).min(len), text)]);
            for ev in doc.take_events() {
                incremental.apply_edit(&ev);
            }
            if i % 2 == 0 {
                let mut calls = 0;
                incremental.parse_with_budget(doc.rope(), || { calls += 1; calls < 2 });
            }
        }
        let lines = 0..doc.rope().len_lines();
        incremental.parse_with_budget(doc.rope(), || true);
        let mut fresh = TextMateSyntax::new(grammar);
        fresh.parse_with_budget(doc.rope(), || true);
        prop_assert_eq!(incremental.highlight(doc.rope(), lines.clone()), fresh.highlight(doc.rope(), lines));
    }
}
