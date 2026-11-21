//! Talks to a language server through the client and prints what each feature returns for
//! one position, so a server can be checked end to end:
//!
//! ```text
//! cargo run -p rustaco-lsp --example probe -- rust-analyzer crates/rustaco-core/src/model.rs 120:12
//! cargo run -p rustaco-lsp --example probe -- typescript-language-server,--stdio app.ts 3:5
//! ```
//!
//! The server starts in the current directory, which is also the workspace root. Commas in
//! the first argument separate the server's own arguments.

use std::path::Path;
use std::time::{Duration, Instant};

use rustaco_core::{Position, TextModel, position_to_char};
use rustaco_lsp::LspClient;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: probe <server[,arg...]> <file> <line:column (1-based)>");
        std::process::exit(2);
    }
    let mut server = args[0].split(',');
    let program = server.next().unwrap_or_default();
    let server_args: Vec<&str> = server.collect();
    let path = Path::new(&args[1]).canonicalize().expect("the file exists");
    let (line, column) = args[2]
        .split_once(':')
        .map(|(l, c)| (l.parse::<usize>().unwrap(), c.parse::<usize>().unwrap()))
        .expect("line:column");
    let text = std::fs::read_to_string(&path).expect("readable file");
    let model = TextModel::new(&text);
    let at = position_to_char(model.rope(), Position::new(line - 1, column - 1));

    let root = std::env::current_dir().unwrap();
    let uri = |p: &Path| format!("file:///{}", p.display().to_string().trim_start_matches(r"\\?\").replace('\\', "/"));
    let (mut client, mut child) = LspClient::spawn(program, &server_args, Some(&root)).expect("start the server");
    client.timeout = Duration::from_secs(30);
    client.initialize(Some(&uri(&root))).expect("initialize");
    println!("server: {}", client.server_info());
    println!("position encoding: {}", client.encoding().name());
    let language = match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => "rust",
        Some("ts") => "typescript",
        Some("js") => "javascript",
        Some("py") => "python",
        Some(other) => other,
        None => "plaintext",
    };
    let mut doc = client.open(&uri(&path), language, &model).expect("didOpen");

    // Servers index first; ask for hover until an answer comes.
    let start = Instant::now();
    let hover = loop {
        match client.hover(&mut doc, &model, at) {
            Ok(Some(h)) => break Some(h),
            _ if start.elapsed() > Duration::from_secs(120) => break None,
            _ => std::thread::sleep(Duration::from_millis(500)),
        }
    };
    println!(
        "hover after {:.1} s: {:?}",
        start.elapsed().as_secs_f32(),
        hover.map(|h| h.contents.join(" | ").chars().take(160).collect::<String>())
    );
    let show = |name: &str, n: Result<usize, rustaco_lsp::RpcError>| {
        println!("{name}: {}", n.map_or_else(|e| format!("error: {e}"), |n| n.to_string()))
    };
    show("completion items", client.completion(&mut doc, &model, at, None).map(|v| v.len()));
    match client.definition(&mut doc, &model, at) {
        Ok(defs) => println!(
            "definition: {:?}",
            defs.iter()
                .map(|d| (d.uri.rsplit('/').next().unwrap_or("").to_owned(), d.lsp_range["start"].clone()))
                .collect::<Vec<_>>()
        ),
        Err(e) => println!("definition: error: {e}"),
    }
    show("references", client.references(&mut doc, &model, at).map(|v| v.len()));
    show("document highlights", client.document_highlights(&mut doc, &model, at).map(|v| v.len()));
    show("document symbols (top level)", client.document_symbols(&mut doc, &model).map(|v| v.len()));
    show("folding ranges", client.folding_ranges(&mut doc, &model).map(|v| v.len()));
    show("selection ranges", client.selection_ranges(&mut doc, &model, at).map(|v| v.len()));
    show("semantic tokens", client.semantic_tokens(&mut doc, &model).map(|v| v.len()));
    show("inlay hints", client.inlay_hints(&mut doc, &model, 0..model.len_chars()).map(|v| v.len()));
    show("code actions", client.code_actions(&mut doc, &model, at..at).map(|v| v.len()));
    show(
        "signature help signatures",
        client.signature_help(&mut doc, &model, at).map(|v| v.map_or(0, |s| s.signatures.len())),
    );
    client.shutdown();
    let _ = child.wait();
}
