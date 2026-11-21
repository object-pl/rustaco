//! Times keystrokes on a 1,000,000-line file without a window: each keystroke edits the
//! document and draws a full 1400x900 frame into a buffer, as the editor example does.
//!
//! ```text
//! cargo run -p rustaco-desktop --example frame_timing --release
//! ```

use std::time::Instant;

use rustaco_core::{KeyPress, Selection, Selections};
use rustaco_desktop::{DesktopEditor, Font};

fn main() {
    let text: String = (0..1_000_000).map(|i| format!("let value_{i} = compute({i}, \"some text here\");\n")).collect();
    let font = Font::system_monospace(14.0).expect("a monospace font");
    let started = Instant::now();
    let mut editor = DesktopEditor::new(&text, "", font, 1400.0, 900.0, 1.0);
    println!("open 1M lines: {:.0} ms", started.elapsed().as_secs_f64() * 1000.0);
    let middle = editor.doc.rope().line_to_char(500_000);
    editor.doc.set_selections(Selections::single(Selection::cursor(middle)));
    let mut buffer = vec![0u32; 1400 * 900];
    editor.render(&mut buffer, 1400, 900);
    let press = KeyPress { ctrl: false, shift: false, alt: false, meta: false, key: "x".into() };
    let rounds = 300;
    let started = Instant::now();
    for _ in 0..rounds {
        editor.key_down(press.clone(), Some("x"));
        editor.render(&mut buffer, 1400, 900);
    }
    let per = started.elapsed().as_secs_f64() * 1000.0 / rounds as f64;
    println!("keystroke with a full frame: {per:.2} ms");
    let started = Instant::now();
    for _ in 0..rounds {
        editor.render(&mut buffer, 1400, 900);
    }
    println!("frame alone: {:.2} ms", started.elapsed().as_secs_f64() * 1000.0 / rounds as f64);
}
