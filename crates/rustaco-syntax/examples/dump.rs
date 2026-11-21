//! Prints highlight names per span: `cargo run -p rustaco-syntax --example dump -- <lang> <file>`.
use rustaco_core::Document;
use rustaco_syntax::{Syntax, by_id, name_of_style};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let text = std::fs::read_to_string(&args[2]).expect("read file");
    let doc = Document::new(&text);
    let mut syntax = Syntax::new(by_id(&args[1]).expect("language")).expect("syntax");
    syntax.update(doc.model());
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
