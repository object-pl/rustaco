//! Several editors on one model: each keeps its own cursors, and they follow each other's edits.

use proptest::prelude::*;
use rustaco_core::{Cursors, Editor, EditorId, Selection, Selections, TextModel};

fn cursor_at(model: &mut TextModel, cursors: &mut Cursors, pos: usize) {
    Editor::new(model, cursors).set_selections(Selections::single(Selection::cursor(pos)));
}

fn head(model: &TextModel, cursors: &mut Cursors) -> usize {
    cursors.sync(model);
    cursors.selections().primary().head
}

#[test]
fn edits_in_one_editor_move_the_other_editors_cursors() {
    let mut model = TextModel::new("first\nsecond\nthird");
    let (mut a, mut b) = (Cursors::new(EditorId(1), &model), Cursors::new(EditorId(2), &model));
    cursor_at(&mut model, &mut b, 13); // "third"
    cursor_at(&mut model, &mut a, 0);

    Editor::new(&mut model, &mut a).insert(">> ");
    assert_eq!(head(&model, &mut b), 16, "B stays on the same char of \"third\"");

    // Deleting text that contains B's cursor moves B to the start of the deletion.
    Editor::new(&mut model, &mut a).set_selections(Selections::single(Selection::new(9, 20)));
    Editor::new(&mut model, &mut a).delete_backward();
    assert_eq!(model.text(), ">> first\nd");
    assert_eq!(head(&model, &mut b), 9);
}

#[test]
fn a_cursor_at_another_editors_insertion_point_stays_before_the_text() {
    let mut model = TextModel::new("ab");
    let (mut a, mut b) = (Cursors::new(EditorId(1), &model), Cursors::new(EditorId(2), &model));
    cursor_at(&mut model, &mut a, 1);
    cursor_at(&mut model, &mut b, 1);
    Editor::new(&mut model, &mut a).insert("xyz");
    assert_eq!(head(&model, &mut a), 4);
    assert_eq!(head(&model, &mut b), 1);
}

#[test]
fn undo_restores_selections_only_for_the_editor_that_made_the_change() {
    let mut model = TextModel::new("hello world");
    let (mut a, mut b) = (Cursors::new(EditorId(1), &model), Cursors::new(EditorId(2), &model));
    cursor_at(&mut model, &mut b, 11);
    Editor::new(&mut model, &mut a).set_selections(Selections::single(Selection::new(0, 5)));
    Editor::new(&mut model, &mut a).insert("goodbye");
    assert_eq!(head(&model, &mut b), 13);

    // B undoes A's change: the text reverts, B's cursor follows the text, A's is untouched by B.
    assert!(Editor::new(&mut model, &mut b).undo());
    assert_eq!(model.text(), "hello world");
    assert_eq!(head(&model, &mut b), 11);

    // A redoes and gets its own post-edit selection back.
    assert!(Editor::new(&mut model, &mut a).redo());
    assert_eq!(model.text(), "goodbye world");
    assert_eq!(a.selections().primary(), Selection::cursor(7));

    // A undoes and gets its pre-edit selection back.
    assert!(Editor::new(&mut model, &mut a).undo());
    assert_eq!(a.selections().primary(), Selection::new(0, 5));
}

#[test]
fn typing_in_two_editors_does_not_merge_into_one_undo_step() {
    let mut model = TextModel::new("");
    let (mut a, mut b) = (Cursors::new(EditorId(1), &model), Cursors::new(EditorId(2), &model));
    Editor::new(&mut model, &mut a).insert("a");
    Editor::new(&mut model, &mut b).insert("b");
    Editor::new(&mut model, &mut a).undo();
    assert_eq!(model.text(), "a");
}

#[test]
fn set_text_moves_every_editor_to_valid_positions() {
    let mut model = TextModel::new("a long line of text");
    let mut a = Cursors::new(EditorId(1), &model);
    cursor_at(&mut model, &mut a, 15);
    model.set_text("short");
    assert!(head(&model, &mut a) <= 5);
    assert!(!model.can_undo());
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 500, ..ProptestConfig::default() })]

    /// Random interleaved edits from three editors: every editor's cursors stay valid and
    /// every editor sees the same text.
    #[test]
    fn interleaved_editors_keep_valid_cursors(
        initial in "[ab\n ]{0,20}",
        ops in prop::collection::vec((0usize..3, 0usize..8, "[xy\n]{0,3}", 0usize..40), 1..40),
    ) {
        let mut model = TextModel::new(&initial);
        let mut editors: Vec<Cursors> = (0..3).map(|i| Cursors::new(EditorId(i), &model)).collect();
        for (who, kind, text, pos) in ops {
            let cursors = &mut editors[who];
            let mut ed = Editor::new(&mut model, cursors);
            let len = ed.selections().primary().head.max(1);
            match kind {
                0 => ed.insert(&text),
                1 => ed.delete_backward(),
                2 => ed.newline(),
                3 => { ed.undo(); }
                4 => { ed.redo(); }
                5 => ed.set_selections(Selections::single(Selection::cursor(pos % (len + 5)))),
                6 => ed.add_cursor_vertical(true),
                _ => ed.move_word_left(true),
            }
            for c in editors.iter_mut() {
                c.sync(&model);
                let n = model.len_chars();
                let sels = c.selections().as_slice();
                prop_assert!(sels.iter().all(|s| s.anchor <= n && s.head <= n));
                prop_assert!(sels.windows(2).all(|w| w[0].end() <= w[1].start()));
            }
        }
        let mut probe = Cursors::new(EditorId(9), &model);
        while Editor::new(&mut model, &mut probe).undo() {}
        prop_assert_eq!(model.text(), initial);
    }
}
