use rustaco_core::{Document, LanguageEditing, Selection, Selections};

fn doc(text: &str, sels: &[(usize, usize)]) -> Document {
    let mut d = Document::new(text);
    d.set_language_editing(LanguageEditing {
        line_comment: Some("//".into()),
        block_comment: Some(("/*".into(), "*/".into())),
        auto_closing: vec![("(".into(), ")".into()), ("[".into(), "]".into()), ("\"".into(), "\"".into())],
    });
    d.set_selections(Selections::new(sels.iter().map(|&(a, h)| Selection::new(a, h)).collect(), 0));
    d
}

fn sels(d: &Document) -> Vec<(usize, usize)> {
    d.selections().iter().map(|s| (s.anchor, s.head)).collect()
}

#[test]
fn indent_and_outdent_selected_lines() {
    let mut d = doc("a\n\nb\nc", &[(0, 4)]);
    d.tab();
    assert_eq!(d.text(), "    a\n\n    b\nc");
    assert_eq!(sels(&d), [(0, 12)], "a selection from a line start keeps covering the whole lines");
    d.outdent_lines();
    assert_eq!(d.text(), "a\n\nb\nc");
    let mut d = doc("\tx\n  y", &[(0, 6)]);
    d.outdent_lines();
    assert_eq!(d.text(), "x\ny");
}

#[test]
fn tab_in_one_line_inserts_indentation() {
    let mut d = doc("ab", &[(1, 1)]);
    d.tab();
    assert_eq!(d.text(), "a   b");
}

#[test]
fn move_lines_up_and_down() {
    let mut d = doc("one\ntwo\nthree", &[(5, 5)]);
    d.move_lines(true);
    assert_eq!(d.text(), "two\none\nthree");
    assert_eq!(sels(&d), [(1, 1)]);
    d.move_lines(false);
    d.move_lines(false);
    assert_eq!(d.text(), "one\nthree\ntwo");
    assert_eq!(sels(&d), [(11, 11)]);
    d.move_lines(false);
    assert_eq!(d.text(), "one\nthree\ntwo", "the last line cannot move down");
}

#[test]
fn copy_and_delete_lines() {
    let mut d = doc("a\nb", &[(0, 0)]);
    d.copy_lines(false);
    assert_eq!(d.text(), "a\na\nb");
    assert_eq!(sels(&d), [(2, 2)], "the cursor moves to the copy");
    d.copy_lines(true);
    assert_eq!(d.text(), "a\na\na\nb");
    let mut d = doc("one\ntwo\nthree", &[(6, 6)]);
    d.delete_lines();
    assert_eq!(d.text(), "one\nthree");
    assert_eq!(sels(&d), [(6, 6)], "the column is kept on the next line");
    let mut d = doc("one\ntwo", &[(5, 5)]);
    d.delete_lines();
    assert_eq!(d.text(), "one");
}

#[test]
fn insert_line_above_and_below_keeps_indent() {
    let mut d = doc("    x", &[(2, 2)]);
    d.insert_line(true);
    assert_eq!(d.text(), "    x\n    ");
    assert_eq!(sels(&d), [(10, 10)]);
    let mut d = doc("    x", &[(5, 5)]);
    d.insert_line(false);
    assert_eq!(d.text(), "    \n    x");
    assert_eq!(sels(&d), [(4, 4)]);
}

#[test]
fn toggle_line_comment() {
    let mut d = doc("  a\n\n    b", &[(0, 10)]);
    d.toggle_line_comment();
    assert_eq!(d.text(), "  // a\n\n  //   b");
    d.toggle_line_comment();
    assert_eq!(d.text(), "  a\n\n    b");
    let mut d = doc("a\n// b", &[(0, 6)]);
    d.toggle_line_comment();
    assert_eq!(d.text(), "// a\n// // b", "mixed lines are all commented");
}

#[test]
fn toggle_block_comment() {
    let mut d = doc("let x = 1;", &[(4, 9)]);
    d.toggle_block_comment();
    assert_eq!(d.text(), "let /* x = 1 */;");
    assert_eq!(sels(&d), [(7, 12)]);
    d.toggle_block_comment();
    assert_eq!(d.text(), "let x = 1;", "tokens around the selection are removed");
    assert_eq!(sels(&d), [(4, 9)]);
    d.set_selections(Selections::single(Selection::new(0, 0)));
    d.toggle_block_comment();
    assert_eq!(d.text(), "/*  */let x = 1;");
    assert_eq!(sels(&d), [(3, 3)], "an empty selection gets an empty comment with the cursor inside");
}

#[test]
fn auto_closing_pairs() {
    let mut d = doc("", &[(0, 0)]);
    d.type_char('(');
    assert_eq!((d.text().as_str(), sels(&d)), ("()", vec![(1, 1)]));
    d.type_char(')');
    assert_eq!((d.text().as_str(), sels(&d)), ("()", vec![(2, 2)]), "typing over the closing char");
    let mut d = doc("()", &[(1, 1)]);
    d.delete_backward_pair();
    assert_eq!(d.text(), "", "backspace in an empty pair removes both");
    let mut d = doc("abc", &[(0, 3)]);
    d.type_char('[');
    assert_eq!((d.text().as_str(), sels(&d)), ("[abc]", vec![(1, 4)]), "a selection is wrapped");
    let mut d = doc("it", &[(2, 2)]);
    d.type_char('"');
    assert_eq!(d.text(), "it\"", "no closing quote right after a word");
    let mut d = doc("x", &[(0, 0)]);
    d.type_char('(');
    assert_eq!(d.text(), "(x", "no closing bracket before a word");
}

#[test]
fn add_next_occurrence_and_select_all() {
    let mut d = doc("foo bar foo baz foo", &[(1, 1)]);
    d.add_next_occurrence();
    assert_eq!(sels(&d), [(0, 3)], "first press selects the word");
    d.add_next_occurrence();
    d.add_next_occurrence();
    assert_eq!(sels(&d), [(0, 3), (8, 11), (16, 19)]);
    d.insert("x");
    assert_eq!(d.text(), "x bar x baz x");
    let mut d = doc("ab ab ab", &[(0, 0)]);
    d.select_all_occurrences();
    assert_eq!(d.selections().len(), 3);
}

#[test]
fn grapheme_clusters_move_and_delete_as_one() {
    // Family emoji: four people joined by zero-width joiners, seven chars.
    let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}";
    let text = format!("a{family}b");
    let mut d = doc(&text, &[(1, 1)]);
    d.move_right(false);
    assert_eq!(sels(&d), [(8, 8)]);
    d.delete_backward();
    assert_eq!(d.text(), "ab");
    let mut d = doc("e\u{301}x", &[(0, 0)]);
    d.delete_forward();
    assert_eq!(d.text(), "x", "a letter with a combining accent is one cluster");
}

#[test]
fn case_and_trailing_whitespace() {
    let mut d = doc("hello world  \nx\t", &[(0, 5)]);
    d.transform_case(true);
    assert_eq!(d.text(), "HELLO world  \nx\t");
    d.trim_trailing_whitespace();
    assert_eq!(d.text(), "HELLO world\nx");
}

#[test]
fn cursor_undo_steps_back_through_selections() {
    let mut d = doc("foo bar foo", &[(1, 1)]);
    d.add_next_occurrence();
    d.add_next_occurrence();
    assert_eq!(d.selections().len(), 2);
    assert!(d.cursor_undo());
    assert_eq!(sels(&d), [(0, 3)]);
    assert!(d.cursor_undo());
    assert_eq!(sels(&d), [(1, 1)]);
    assert!(d.cursor_undo(), "placing the cursor at 1 was a cursor change too");
    assert_eq!(sels(&d), [(0, 0)]);
    assert!(!d.cursor_undo());
    d.add_next_occurrence();
    d.insert("x");
    assert!(!d.cursor_undo(), "an edit clears cursor history");
}

#[test]
fn every_bound_command_runs() {
    use rustaco_core::{Platform, default_keymap, jetbrains_keymap};
    // Page movement needs the view's height; frontends run these.
    let frontend = [
        "cursorPageUp",
        "cursorPageUpSelect",
        "cursorPageDown",
        "cursorPageDownSelect",
        // Find navigation needs the frontend's current query.
        "editor.action.nextMatchFindAction",
        "editor.action.previousMatchFindAction",
    ];
    for platform in [Platform::Windows, Platform::Mac] {
        for keymap in [default_keymap(platform), jetbrains_keymap(platform)] {
            for b in keymap.bindings() {
                let mut d = doc("one two\nthree", &[(4, 7)]);
                let handled = d.editor().run(&b.command);
                assert!(handled || frontend.contains(&b.command.as_str()), "{} is bound but not handled", b.command);
            }
        }
    }
}
