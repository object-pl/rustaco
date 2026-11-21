//! Times a full parse and one incremental keystroke on a large file:
//! `cargo run --release -p rustaco-syntax --example parse_timing`.
use std::time::Instant;

use rustaco_core::{Document, Edit};
use rustaco_syntax::{Syntax, by_id};

fn time(label: &str, text: String) {
    let mut doc = Document::new(&text);
    let mut syntax = Syntax::new(by_id("rust").unwrap()).unwrap();
    let t = Instant::now();
    syntax.update(doc.model());
    let full = t.elapsed();
    let middle = doc.rope().line_to_char(doc.rope().len_lines() / 2) + 4;
    let (mut parse, mut highlight) = (std::time::Duration::ZERO, std::time::Duration::ZERO);
    for _ in 0..20 {
        doc.apply_edits(vec![Edit::insert(middle, "x")]);
        let t = Instant::now();
        syntax.update(doc.model());
        parse += t.elapsed();
        let first = doc.rope().len_lines() / 2;
        let t = Instant::now();
        syntax.highlight(doc.rope(), first..first + 50);
        highlight += t.elapsed();
    }
    let errors = syntax.tree().unwrap().root_node().has_error();
    println!(
        "{label}: full parse {full:?}, incremental parse {:?}, highlight {:?}, errors: {errors}",
        parse / 20,
        highlight / 20
    );
}

fn main() {
    let n = 100_000;
    time("top-level let (invalid)", (0..n).map(|i| format!("let v{i} = compute({i}, \"text\"); // c\n")).collect());
    let body: String = (0..n).map(|i| format!("    let v{i} = compute({i}, \"text\"); // c\n")).collect();
    time("inside fn (valid)", format!("fn main() {{\n{body}}}\n"));
    time(
        "many small fns (valid)",
        (0..n / 4).map(|i| format!("fn f{i}() -> u32 {{\n    let v = {i};\n    v + 1\n}}\n")).collect(),
    );
}
