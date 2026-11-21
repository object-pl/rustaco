//! Prints the open TextMate blocks at the start of each line:
//! `cargo run -p rustaco-syntax --example tm_states -- <language> <file>`.
use rustaco_core::Document;
use rustaco_syntax::{TextMateSyntax, textmate_grammar};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let text = std::fs::read_to_string(&args[2]).expect("read file");
    let doc = Document::new(&text);
    let mut syntax = TextMateSyntax::new(textmate_grammar(&args[1]).expect("grammar"));
    syntax.parse_with_budget(doc.rope(), || true);
    for (i, state) in syntax.describe_line_states().iter().enumerate() {
        println!("{:>3}: {state}", i + 1);
    }
}
