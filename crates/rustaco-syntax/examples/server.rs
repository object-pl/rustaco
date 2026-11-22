//! The bundled languages as a language server on stdio: folding, outline, and completion,
//! definition, references and rename within a file, for any LSP client.
//! `cargo run -p rustaco-syntax --example server`
//!
//! Documents are parsed when a request needs the tree; nothing parses between messages.

use std::sync::OnceLock;
use std::time::Instant;

use rustaco_syntax::SyntaxService;

/// Milliseconds since the first call.
fn clock() -> f64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
}

fn main() -> std::io::Result<()> {
    rustaco_lsp::serve_stdio(SyntaxService::new(clock))
}
