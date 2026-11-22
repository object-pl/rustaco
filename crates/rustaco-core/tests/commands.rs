use rustaco_core::{Document, LanguageEditing, Selection, Selections};

fn doc(text: &str, sels: &[(usize, usize)]) -> Document {
    let mut d = Document::new(text);
    let mut editing = LanguageEditing::default();
    editing.line_comment = Some("//".into());
    editing.block_comment = Some(("/*".into(), "*/".into()));
    editing.auto_closing = vec![("(".into(), ")".into()), ("[".into(), "]".into()), ("\"".into(), "\"".into())];
    editing.brackets = vec![("{".into(), "}".into()), ("(".into(), ")".into())];
    editing.indent_after = vec![":".into()];
    d.set_language_editing(editing);
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
fn smart_enter_indents_after_brackets_and_colons() {
    let mut d = doc("fn f() {}", &[(8, 8)]);
    d.newline();
    assert_eq!(d.text(), "fn f() {\n    \n}");
    assert_eq!(sels(&d), [(13, 13)], "the caret is on the indented middle line");
    let mut d = doc("    if x {", &[(10, 10)]);
    d.newline();
    assert_eq!(d.text(), "    if x {\n        ");
    let mut d = doc("def f():", &[(8, 8)]);
    d.newline();
    assert_eq!(d.text(), "def f():\n    ");
    let mut d = doc("    x = 1", &[(9, 9)]);
    d.newline();
    assert_eq!(d.text(), "    x = 1\n    ");
}

#[test]
fn expand_and_shrink_selection_follow_given_ranges() {
    let mut d = doc("f(a + b)", &[(2, 2)]);
    let ranges = [2..3, 2..7, 1..8, 0..8];
    assert!(d.expand_selection(&ranges));
    assert_eq!(sels(&d), [(2, 3)]);
    assert!(d.expand_selection(&ranges));
    assert_eq!(sels(&d), [(2, 7)]);
    assert!(d.shrink_selection());
    assert_eq!(sels(&d), [(2, 3)]);
    assert!(d.shrink_selection());
    assert_eq!(sels(&d), [(2, 2)]);
    assert!(!d.shrink_selection());
}

#[test]
fn no_auto_closing_inside_literals() {
    let mut d = doc("\"ab\"", &[(2, 2)]);
    d.type_char_in('(', true);
    assert_eq!(d.text(), "\"a(b\"", "no pair opened inside a string");
    let mut d = doc("\"a\"", &[(2, 2)]);
    d.type_char_in('"', true);
    assert_eq!(sels(&d), [(3, 3)], "typing the closing quote steps over it");
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
fn brackets_match_by_text() {
    use rustaco_core::{Rope, matching_bracket_text};
    let pairs = vec![("(".to_string(), ")".to_string()), ("[".to_string(), "]".to_string())];
    let rope = Rope::from_str("f(a[1], (b))");
    assert_eq!(matching_bracket_text(&rope, 1, &pairs), Some((1, 11)));
    assert_eq!(matching_bracket_text(&rope, 12, &pairs), Some((1, 11)), "the bracket before the cursor");
    assert_eq!(matching_bracket_text(&rope, 4, &pairs), Some((3, 5)));
    assert_eq!(matching_bracket_text(&rope, 7, &pairs), None, "no bracket next to the cursor");
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
        // Expanding by syntax needs the language module.
        "editor.action.smartSelect.expand",
        "editor.action.smartSelect.shrink",
        // The bracket pair comes from the frontend's syntax data.
        "editor.action.jumpToBracket",
        // Find navigation needs the frontend's current query.
        "editor.action.nextMatchFindAction",
        "editor.action.previousMatchFindAction",
        // Snippet navigation needs an active snippet.
        "jumpToNextSnippetPlaceholder",
        "jumpToPrevSnippetPlaceholder",
        "leaveSnippet",
        // Language features come from the frontend's providers.
        "editor.action.triggerSuggest",
        "editor.action.triggerParameterHints",
        "editor.action.revealDefinition",
        "editor.action.referenceSearch.trigger",
        "editor.action.rename",
        "editor.action.formatDocument",
        "editor.action.showHover",
        "editor.action.quickFix",
        "editor.action.quickOutline",
        // The find widget is the frontend's.
        "actions.find",
        "editor.action.startFindReplaceAction",
        // Folds belong to the view.
        "editor.fold",
        "editor.unfold",
        "editor.toggleFold",
        "editor.foldAll",
        "editor.unfoldAll",
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

#[test]
fn composition_steps_replace_each_other_and_undo_as_one() {
    let mut d = doc("ab", &[(1, 1)]);
    d.editor().compose(0, "k");
    d.editor().compose(1, "ka");
    d.editor().compose(2, "\u{304B}");
    assert_eq!(d.text(), "a\u{304B}b");
    assert_eq!(d.selections().primary().head, 2);
    d.editor().undo();
    assert_eq!(d.text(), "ab");
}

#[test]
fn pasting_pieces_gives_each_cursor_its_own() {
    let mut d = doc("x\ny\nz", &[(1, 1), (3, 3), (5, 5)]);
    d.editor().paste_each(&["1".into(), "2".into(), "3".into()]);
    assert_eq!(d.text(), "x1\ny2\nz3");
    let mut d = doc("x\ny", &[(1, 1)]);
    d.editor().paste_each(&["1".into(), "2".into()]);
    assert_eq!(d.text(), "x1\n2\ny", "one cursor takes the pieces joined");
}

#[test]
fn snippet_session_walks_its_stops() {
    use rustaco_core::Snippet;
    let mut d = doc("    x", &[(5, 5)]);
    let snippet = Snippet::parse("for ${1:i} in ${2:items} {\n\t$1;$0\n}", &|_| None);
    d.editor().insert_snippet(&snippet);
    assert_eq!(d.text(), "    xfor i in items {\n        i;\n    }");
    // Stop 1 selects both places `i` appears.
    let sels: Vec<(usize, usize)> = d.selections().iter().map(|s| (s.start(), s.end())).collect();
    assert_eq!(sels.len(), 2);
    assert_eq!(&d.text()[sels[0].0..sels[0].1], "i");
    d.insert("k");
    assert_eq!(d.text(), "    xfor k in items {\n        k;\n    }");
    assert!(d.editor().next_snippet_stop());
    let s = d.selections().primary();
    assert_eq!(&d.text()[s.start()..s.end()], "items");
    assert!(d.editor().prev_snippet_stop());
    assert_eq!(d.selections().len(), 2, "back on the mirrored stop");
    d.editor().next_snippet_stop();
    d.editor().next_snippet_stop();
    assert!(d.editor().snippet().is_none(), "reaching $0 ends the session");
    assert_eq!(d.selections().primary().head, "    xfor k in items {\n        k;".len());
}
