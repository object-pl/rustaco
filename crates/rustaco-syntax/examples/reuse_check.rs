//! Compares incremental reparse cost with tree-sitter fed from a String and from our rope path.
use std::time::Instant;

use tree_sitter::{InputEdit, Parser, Point};

fn main() {
    let n = 25_000;
    let mut text: String = (0..n).map(|i| format!("fn f{i}() -> u32 {{\n    let v = {i};\n    v + 1\n}}\n")).collect();
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_rust::LANGUAGE.into()).unwrap();
    let t = Instant::now();
    let mut tree = parser.parse(&text, None).unwrap();
    println!("full parse from String: {:?}", t.elapsed());

    let line = n * 2; // somewhere in the middle
    let byte = text.match_indices('\n').nth(line).unwrap().0 + 5;
    let row = line + 1;
    let total = std::time::Duration::ZERO;
    let mut total = total;
    for _ in 0..20 {
        text.insert(byte, 'x');
        tree.edit(&InputEdit {
            start_byte: byte,
            old_end_byte: byte,
            new_end_byte: byte + 1,
            start_position: Point { row, column: 4 },
            old_end_position: Point { row, column: 4 },
            new_end_position: Point { row, column: 5 },
        });
        let t = Instant::now();
        tree = parser.parse(&text, Some(&tree)).unwrap();
        total += t.elapsed();
    }
    println!("incremental parse from String: {:?} per keystroke", total / 20);
}
