//! Prints TextMate highlight names per span:
//! `cargo run -p rustaco-syntax --example tm_dump -- <language> <file>`.
use rustaco_core::Document;
use rustaco_syntax::{TextMateSyntax, name_of_style, textmate_grammar};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let text = std::fs::read_to_string(&args[2]).expect("read file");
    let doc = Document::new(&text);
    let mut syntax = TextMateSyntax::new(textmate_grammar(&args[1]).expect("grammar"));
    let t = std::time::Instant::now();
    syntax.parse_with_budget(doc.rope(), || true);
    eprintln!("tokenized in {:?}", t.elapsed());
    let h = syntax.highlight(doc.rope(), 0..doc.rope().len_lines());
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
        println!("{:>3}: {}", i + 1, spans.join("  "));
    }
}
