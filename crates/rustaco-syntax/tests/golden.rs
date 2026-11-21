//! Golden tests: one sample per language in `tests/golden/`, highlighted with the engine the
//! browser uses for it, compared with `<sample>.expected`. Run with `UPDATE_GOLDEN=1` to
//! rewrite the expected files after a deliberate change, then review the diff.

use std::fs;
use std::path::Path;

use rustaco_core::Document;
use rustaco_syntax::{Highlights, Syntax, TextMateSyntax, by_id, detect, name_of_style, textmate_grammar};

fn highlight(language: &str, pack: &str, doc: &Document) -> Highlights {
    let lines = 0..doc.rope().len_lines();
    if pack == "textmate" {
        let mut s = TextMateSyntax::new(textmate_grammar(language).expect("TextMate grammar"));
        s.parse_with_budget(doc.rope(), || true);
        s.highlight(doc.rope(), lines)
    } else {
        let mut s = Syntax::new(by_id(language).expect("tree-sitter grammar")).expect("syntax");
        s.update(doc.model());
        s.highlight(doc.rope(), lines)
    }
}

fn render(text: &str, h: &Highlights) -> String {
    let mut out = String::new();
    for (i, line) in text.lines().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let spans: Vec<String> = h
            .line(i)
            .iter()
            .map(|s| {
                format!(
                    "{}={}",
                    chars[s.start..s.end].iter().collect::<String>(),
                    name_of_style(s.style).unwrap_or("?")
                )
            })
            .collect();
        out.push_str(&format!("{:>3}: {}\n", i + 1, spans.join("  ")));
    }
    out
}

#[test]
fn every_language_matches_its_golden_file() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    let mut checked = Vec::new();
    let mut failures = Vec::new();
    for entry in fs::read_dir(&dir).expect("golden dir") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|e| e == "expected") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let text = fs::read_to_string(&path).expect("sample");
        let info = detect(&name, text.lines().next().unwrap_or("")).unwrap_or_else(|| panic!("{name}: no language"));
        let doc = Document::new(&text);
        let h = highlight(info.id, info.pack, &doc);
        let actual = render(&text, &h);

        let distinct: std::collections::BTreeSet<&str> =
            (0..doc.rope().len_lines()).flat_map(|l| h.line(l).iter().filter_map(|s| name_of_style(s.style))).collect();
        assert!(distinct.len() >= 3, "{name} ({}): only {distinct:?} highlighted", info.id);

        let expected_path = dir.join(format!("{name}.expected"));
        if update {
            fs::write(&expected_path, &actual).expect("write expected");
        } else {
            let expected = fs::read_to_string(&expected_path).unwrap_or_default();
            if expected.replace("\r\n", "\n") != actual {
                failures.push(format!("{name} ({}):\n--- expected\n{expected}--- actual\n{actual}", info.id));
            }
        }
        checked.push(info.id);
    }
    checked.sort();
    assert_eq!(checked.len(), 20, "languages checked: {checked:?}");
    assert!(failures.is_empty(), "{} golden files differ:\n{}", failures.len(), failures.join("\n"));
}
