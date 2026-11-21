//! Property tests: random edit sequences checked against invariants.

use proptest::prelude::*;
use rustaco_core::{
    Assoc, ChangeSet, Document, Edit, EditEvent, Rope, Selection, Selections, Utf16Position, utf16_to_char,
};

/// Alphabet chosen to hit multi-byte chars, surrogate pairs and every line terminator.
fn text() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(vec!["a", "b", " ", "\t", "é", "😀", "\n", "\r\n", "\r"]), 0..40)
        .prop_map(|parts| parts.concat())
}

#[derive(Clone, Debug)]
enum Op {
    Cursors(Vec<(usize, usize)>),
    Insert(String),
    Paste(String),
    Newline,
    Indent,
    Backspace,
    Delete,
    WordBackspace,
    WordDelete,
    Cut,
    Left(bool),
    Right(bool),
    Up(bool),
    Down(bool),
    WordLeft(bool),
    WordRight(bool),
    Home(bool),
    End(bool),
    AddCursor(bool),
    Undo,
    Redo,
    External(Vec<(usize, usize, String)>),
}

fn op() -> impl Strategy<Value = Op> {
    let pos = 0usize..200;
    prop_oneof![
        prop::collection::vec((pos.clone(), pos.clone()), 1..4).prop_map(Op::Cursors),
        text().prop_map(Op::Insert),
        prop::sample::select(vec!["x", "é", "😀"]).prop_map(|s| Op::Insert(s.to_owned())),
        text().prop_map(Op::Paste),
        Just(Op::Newline),
        Just(Op::Indent),
        Just(Op::Backspace),
        Just(Op::Delete),
        Just(Op::WordBackspace),
        Just(Op::WordDelete),
        Just(Op::Cut),
        any::<bool>().prop_map(Op::Left),
        any::<bool>().prop_map(Op::Right),
        any::<bool>().prop_map(Op::Up),
        any::<bool>().prop_map(Op::Down),
        any::<bool>().prop_map(Op::WordLeft),
        any::<bool>().prop_map(Op::WordRight),
        any::<bool>().prop_map(Op::Home),
        any::<bool>().prop_map(Op::End),
        any::<bool>().prop_map(Op::AddCursor),
        Just(Op::Undo),
        Just(Op::Redo),
        prop::collection::vec((pos.clone(), pos, text()), 1..3).prop_map(Op::External),
    ]
}

fn run(doc: &mut Document, op: &Op) {
    match op {
        Op::Cursors(c) => {
            let len = doc.len_chars() + 1;
            let sels = c.iter().map(|&(a, h)| Selection::new(a % len, h % len)).collect();
            doc.set_selections(Selections::new(sels, 0));
        }
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
        Op::Left(e) => doc.move_left(*e),
        Op::Right(e) => doc.move_right(*e),
        Op::Up(e) => doc.move_up(*e),
        Op::Down(e) => doc.move_down(*e),
        Op::WordLeft(e) => doc.move_word_left(*e),
        Op::WordRight(e) => doc.move_word_right(*e),
        Op::Home(e) => doc.move_line_start(*e),
        Op::End(e) => doc.move_line_end(*e),
        Op::AddCursor(below) => doc.add_cursor_vertical(*below),
        Op::Undo => {
            doc.undo();
        }
        Op::Redo => {
            doc.redo();
        }
        Op::External(edits) => {
            let len = doc.len_chars() + 1;
            let edits = edits
                .iter()
                .map(|(a, b, t)| {
                    let (a, b) = (a % len, b % len);
                    Edit::new(a.min(b), a.max(b), t.clone())
                })
                .collect();
            doc.apply_edits(edits);
        }
    }
}

fn assert_selections_valid(doc: &Document) {
    let len = doc.len_chars();
    let sels = doc.selections().as_slice();
    for s in sels {
        assert!(s.anchor <= len && s.head <= len, "selection {s:?} outside document of {len} chars");
    }
    for w in sels.windows(2) {
        assert!(w[0].end() <= w[1].start(), "selections overlap or are unsorted: {sels:?}");
    }
    assert!(doc.selections().primary_index() < sels.len());
}

/// Replays events on three shadow copies, addressing them by chars, bytes and UTF-16.
struct Shadows {
    chars: Rope,
    bytes: String,
    utf16: Rope,
}

impl Shadows {
    fn new(text: &str) -> Self {
        Self { chars: Rope::from_str(text), bytes: text.to_owned(), utf16: Rope::from_str(text) }
    }

    fn apply(&mut self, ev: &EditEvent) {
        self.chars.remove(ev.start..ev.old_end);
        self.chars.insert(ev.start, &ev.text);

        self.bytes.replace_range(ev.start_byte..ev.old_end_byte, &ev.text);

        let start = utf16_to_char(&self.utf16, ev.start_utf16);
        let end = utf16_to_char(&self.utf16, ev.old_end_utf16);
        self.utf16.remove(start..end);
        self.utf16.insert(start, &ev.text);
    }

    fn check(&self, doc: &Document, op: &str) {
        let text = doc.text();
        assert_eq!(self.chars.to_string(), text, "char-offset replay diverged after {op}");
        assert_eq!(self.bytes, text, "byte-offset replay diverged after {op}");
        assert_eq!(self.utf16.to_string(), text, "UTF-16 replay diverged after {op}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(), ..ProptestConfig::default() })]

    #[test]
    fn edit_sequences_keep_invariants(initial in text(), ops in prop::collection::vec(op(), 1..30)) {
        let mut doc = Document::new(&initial);
        let mut shadows = Shadows::new(&initial);
        for op in &ops {
            let version = doc.version();
            run(&mut doc, op);
            for ev in doc.take_events() {
                shadows.apply(&ev);
            }
            shadows.check(&doc, &format!("{op:?}"));
            assert_selections_valid(&doc);
            prop_assert!(doc.version() >= version);
        }

        // Undoing everything returns to the initial text, redoing everything to the final one.
        // Redo first: trailing Undo ops leave redo steps, and "redo all" goes back to the tip.
        while doc.redo() {}
        let last = doc.text();
        while doc.undo() {
            assert_selections_valid(&doc);
        }
        prop_assert_eq!(doc.text(), initial);
        while doc.redo() {}
        prop_assert_eq!(doc.text(), last);
        for ev in doc.take_events() {
            shadows.apply(&ev);
        }
        shadows.check(&doc, "undo all and redo all");
    }

    #[test]
    fn change_set_inverse_and_map(initial in text(), raw in prop::collection::vec((0usize..60, 0usize..60, text()), 0..5)) {
        let len = initial.chars().count() + 1;
        let edits: Vec<Edit> = raw
            .into_iter()
            .map(|(a, b, t)| {
                let (a, b) = (a % len, b % len);
                Edit::new(a.min(b), a.max(b), t)
            })
            .collect();
        let cs = ChangeSet::new(edits);
        let mut rope = Rope::from_str(&initial);
        let inverse = cs.apply(&mut rope);
        let new_len = rope.len_chars();

        let (mut last_before, mut last_after) = (0, 0);
        for pos in 0..len {
            let before = cs.map(pos, Assoc::Before);
            let after = cs.map(pos, Assoc::After);
            prop_assert!(before <= after && after <= new_len);
            prop_assert!(before >= last_before && after >= last_after, "map is not monotonic at {}", pos);
            (last_before, last_after) = (before, after);
        }
        prop_assert_eq!(cs.map(len - 1, Assoc::After), new_len);

        inverse.apply(&mut rope);
        prop_assert_eq!(rope.to_string(), initial);
    }

    #[test]
    fn single_cursor_typing_matches_string(initial in text(), at in 0usize..60, typed in text()) {
        let mut doc = Document::new(&initial);
        let at = snap_out_of_crlf(&initial, at % (initial.chars().count() + 1));
        doc.set_selections(Selections::single(Selection::cursor(at)));
        doc.insert(&typed);
        let mut expected: Vec<char> = initial.chars().collect();
        expected.splice(at..at, typed.chars());
        prop_assert_eq!(doc.text(), expected.into_iter().collect::<String>());
    }
}

/// 2000 cases by default; `PROPTEST_CASES` raises it for longer runs.
fn cases() -> u32 {
    std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(2000)
}

fn snap_out_of_crlf(text: &str, at: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    if at > 0 && at < chars.len() && chars[at - 1] == '\r' && chars[at] == '\n' { at - 1 } else { at }
}

#[test]
fn utf16_position_past_line_end_clamps() {
    let rope = Rope::from_str("ab\ncd");
    assert_eq!(utf16_to_char(&rope, Utf16Position { line: 0, character: 99 }), 2);
}
