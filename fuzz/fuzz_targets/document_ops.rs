//! Drives a document with arbitrary operations and checks the invariants the property
//! tests check, without their size limits. Run with `cargo +nightly fuzz run document_ops`.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use rustaco_core::{Document, Edit, Rope, Selection, Selections, utf16_to_char};

#[derive(Arbitrary, Debug)]
enum Op {
    Cursors(Vec<(u16, u16)>),
    Insert(String),
    Paste(String),
    Newline,
    Indent,
    Backspace,
    Delete,
    WordBackspace,
    WordDelete,
    Cut,
    Move { kind: u8, extend: bool },
    AddCursor(bool),
    Undo,
    Redo,
    External(Vec<(u16, u16, String)>),
}

#[derive(Arbitrary, Debug)]
struct Input {
    initial: String,
    ops: Vec<Op>,
}

fuzz_target!(|input: Input| {
    let mut doc = Document::new(&input.initial);
    let mut by_char = Rope::from_str(&input.initial);
    let mut by_utf16 = Rope::from_str(&input.initial);

    for op in input.ops.iter().take(64) {
        apply(&mut doc, op);
        for ev in doc.take_events() {
            by_char.remove(ev.start..ev.old_end);
            by_char.insert(ev.start, &ev.text);
            let start = utf16_to_char(&by_utf16, ev.start_utf16);
            let end = utf16_to_char(&by_utf16, ev.old_end_utf16);
            by_utf16.remove(start..end);
            by_utf16.insert(start, &ev.text);
        }
        assert_eq!(by_char, *doc.rope());
        assert_eq!(by_utf16, *doc.rope());
        let len = doc.len_chars();
        let sels = doc.selections().as_slice();
        assert!(sels.iter().all(|s| s.anchor <= len && s.head <= len));
        assert!(sels.windows(2).all(|w| w[0].end() <= w[1].start()));
    }

    while doc.redo() {}
    while doc.undo() {}
    assert_eq!(doc.text(), input.initial);
});

fn apply(doc: &mut Document, op: &Op) {
    let len = doc.len_chars() + 1;
    match op {
        Op::Cursors(c) if !c.is_empty() => {
            let sels = c.iter().take(8).map(|&(a, h)| Selection::new(a as usize % len, h as usize % len)).collect();
            doc.set_selections(Selections::new(sels, 0));
        }
        Op::Cursors(_) => {}
        Op::Insert(t) => doc.insert(t),
        Op::Paste(t) => doc.paste(t),
        Op::Newline => doc.newline(),
        Op::Indent => doc.indent(),
        Op::Backspace => doc.delete_backward(),
        Op::Delete => doc.delete_forward(),
        Op::WordBackspace => doc.delete_word_backward(),
        Op::WordDelete => doc.delete_word_forward(),
        Op::Cut => {
            doc.cut();
        }
        Op::Move { kind, extend } => match kind % 10 {
            0 => doc.move_left(*extend),
            1 => doc.move_right(*extend),
            2 => doc.move_up(*extend),
            3 => doc.move_down(*extend),
            4 => doc.move_word_left(*extend),
            5 => doc.move_word_right(*extend),
            6 => doc.move_line_start(*extend),
            7 => doc.move_line_end(*extend),
            8 => doc.move_document_start(*extend),
            _ => doc.move_document_end(*extend),
        },
        Op::AddCursor(below) => doc.add_cursor_vertical(*below),
        Op::Undo => {
            doc.undo();
        }
        Op::Redo => {
            doc.redo();
        }
        Op::External(edits) => {
            let edits = edits
                .iter()
                .take(4)
                .map(|(a, b, t)| {
                    let (a, b) = (*a as usize % len, *b as usize % len);
                    Edit::new(a.min(b), a.max(b), t.clone())
                })
                .collect();
            doc.apply_edits(edits);
        }
    }
}
