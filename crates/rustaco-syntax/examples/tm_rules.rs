//! Lists the top-level rules of a bundled TextMate grammar:
//! `cargo run -p rustaco-syntax --example tm_rules -- sql`.
fn main() {
    let id = std::env::args().nth(1).expect("language id");
    let g = rustaco_syntax::textmate_grammar(&id).expect("grammar");
    for line in g.describe_top_level().iter().take(30) {
        println!("{line}");
    }
}
