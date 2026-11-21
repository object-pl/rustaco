//! Single-editor convenience: one [`TextModel`] with one set of [`Cursors`].

use crate::change::{Edit, EditEvent};
use crate::editor::{Cursors, Editor, EditorId};
use crate::model::{LineEnding, Options, TextModel};
use crate::selection::Selections;
use ropey::Rope;

/// A model and the cursors of its only editor. Editors sharing a model use
/// [`TextModel`], [`Cursors`] and [`Editor`] directly.
#[derive(Clone, Debug)]
pub struct Document {
    model: TextModel,
    cursors: Cursors,
    events_seen: u64,
}

macro_rules! delegate {
    ($($(#[$doc:meta])* $name:ident($($arg:ident: $ty:ty),*) $(-> $ret:ty)?;)*) => {
        $($(#[$doc])* pub fn $name(&mut self, $($arg: $ty),*) $(-> $ret)? {
            self.editor().$name($($arg),*)
        })*
    };
}

impl Document {
    pub fn new(text: &str) -> Self {
        let model = TextModel::new(text);
        let cursors = Cursors::new(EditorId::default(), &model);
        Self { model, cursors, events_seen: 0 }
    }

    pub fn model(&self) -> &TextModel {
        &self.model
    }

    pub fn editor(&mut self) -> Editor<'_> {
        Editor::new(&mut self.model, &mut self.cursors)
    }

    pub fn rope(&self) -> &Rope {
        self.model.rope()
    }

    pub fn text(&self) -> String {
        self.model.text()
    }

    pub fn len_chars(&self) -> usize {
        self.model.len_chars()
    }

    pub fn version(&self) -> u64 {
        self.model.version()
    }

    pub fn line_ending(&self) -> LineEnding {
        self.model.line_ending()
    }

    pub fn options(&self) -> Options {
        self.model.options
    }

    pub fn set_options(&mut self, options: Options) {
        self.model.options = options;
    }

    pub fn selections(&self) -> &Selections {
        self.cursors.selections()
    }

    pub fn can_undo(&self) -> bool {
        self.model.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.model.can_redo()
    }

    pub fn checkpoint(&mut self) {
        self.model.checkpoint();
    }

    /// Replaces the whole text and clears the undo history.
    pub fn set_text(&mut self, text: &str) {
        self.model.set_text(text);
        self.cursors.sync(&self.model);
    }

    /// Returns the edits applied since the last call, in application order.
    /// Calls must come at least every 1024 change sets, or older events are lost.
    pub fn take_events(&mut self) -> Vec<EditEvent> {
        let events = self.model.events_since(self.events_seen).map(|e| e.cloned().collect()).unwrap_or_default();
        self.events_seen = self.model.version();
        events
    }

    pub fn copy(&mut self) -> String {
        self.editor().copy()
    }

    delegate! {
        set_selections(selections: Selections);
        insert(text: &str);
        newline();
        indent();
        delete_backward();
        delete_forward();
        delete_word_backward();
        delete_word_forward();
        apply_edits(edits: Vec<Edit>);
        cut() -> String;
        paste(text: &str);
        move_left(extend: bool);
        move_right(extend: bool);
        move_word_left(extend: bool);
        move_word_right(extend: bool);
        move_up(extend: bool);
        move_down(extend: bool);
        move_vertical(lines: isize, extend: bool);
        move_line_start(extend: bool);
        move_line_end(extend: bool);
        move_document_start(extend: bool);
        move_document_end(extend: bool);
        select_all();
        add_cursor_vertical(below: bool);
        undo() -> bool;
        redo() -> bool;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::word_at;
    use crate::selection::Selection;

    fn doc_with_cursors(text: &str, cursors: &[usize]) -> Document {
        let mut d = Document::new(text);
        d.set_selections(Selections::new(cursors.iter().map(|&c| Selection::cursor(c)).collect(), 0));
        d
    }

    fn carets(d: &Document) -> Vec<usize> {
        d.selections().iter().map(|s| s.head).collect()
    }

    #[test]
    fn multi_cursor_typing_and_grouped_undo() {
        let mut d = doc_with_cursors("ab\ncd", &[1, 4]);
        d.insert("x");
        d.insert("y");
        assert_eq!(d.text(), "axyb\ncxyd");
        assert_eq!(carets(&d), [3, 8]);
        assert!(d.undo());
        assert_eq!(d.text(), "ab\ncd");
        assert_eq!(carets(&d), [1, 4]);
        assert!(d.redo());
        assert_eq!(d.text(), "axyb\ncxyd");
        assert_eq!(d.version(), 6, "one version per applied change set");
    }

    #[test]
    fn movement_breaks_undo_group() {
        let mut d = Document::new("");
        d.insert("a");
        d.move_left(false);
        d.insert("b");
        d.undo();
        assert_eq!(d.text(), "a");
    }

    #[test]
    fn backspace_merges_adjacent_cursors() {
        let mut d = doc_with_cursors("abcd", &[2, 3]);
        d.delete_backward();
        assert_eq!(d.text(), "ad");
        assert_eq!(carets(&d), [1]);
    }

    #[test]
    fn backspace_removes_crlf_as_one() {
        let mut d = doc_with_cursors("a\r\nb", &[3]);
        d.delete_backward();
        assert_eq!(d.text(), "ab");
    }

    #[test]
    fn newline_keeps_indent_and_line_ending() {
        let mut d = doc_with_cursors("    foo\r\n", &[7]);
        d.newline();
        assert_eq!(d.text(), "    foo\r\n    \r\n");
    }

    #[test]
    fn vertical_movement_keeps_goal_column() {
        let mut d = doc_with_cursors("abcdef\nab\nabcdef", &[5]);
        d.move_down(false);
        assert_eq!(carets(&d), [9]);
        d.move_down(false);
        assert_eq!(carets(&d), [15]);
    }

    #[test]
    fn smart_home_toggles() {
        let mut d = doc_with_cursors("  foo", &[4]);
        d.move_line_start(false);
        assert_eq!(carets(&d), [2]);
        d.move_line_start(false);
        assert_eq!(carets(&d), [0]);
    }

    #[test]
    fn paste_spreads_lines_over_cursors() {
        let mut d = doc_with_cursors("a\nb", &[1, 3]);
        d.paste("1\n2");
        assert_eq!(d.text(), "a1\nb2");
    }

    #[test]
    fn cut_empty_cursor_takes_line() {
        let mut d = doc_with_cursors("one\ntwo\n", &[5]);
        assert_eq!(d.cut(), "two\n");
        assert_eq!(d.text(), "one\n");
    }

    #[test]
    fn events_describe_applied_edits() {
        let mut d = doc_with_cursors("ab\ncd", &[1, 4]);
        d.insert("Z");
        let ev = d.take_events();
        assert_eq!(ev.len(), 2);
        assert_eq!((ev[0].start, ev[0].text.as_str()), (4, "Z"));
        assert_eq!((ev[1].start, ev[1].text.as_str()), (1, "Z"));
        assert!(d.take_events().is_empty());
    }

    #[test]
    fn add_cursor_below_clamps_column() {
        let mut d = doc_with_cursors("abcd\nx", &[3]);
        d.add_cursor_vertical(true);
        assert_eq!(carets(&d), [3, 6]);
        assert_eq!(d.selections().primary_index(), 1);
    }

    #[test]
    fn word_movement_and_deletion() {
        let mut d = doc_with_cursors("foo.bar  baz\nqux", &[0]);
        let mut stops = Vec::new();
        for _ in 0..6 {
            d.move_word_right(false);
            stops.push(carets(&d)[0]);
        }
        assert_eq!(stops, [3, 4, 7, 12, 13, 16]);
        d.move_word_left(false);
        assert_eq!(carets(&d), [13]);
        let mut d = doc_with_cursors("let value = 1", &[7]);
        d.delete_word_backward();
        assert_eq!(d.text(), "let ue = 1");
        assert_eq!(word_at(d.rope(), 5), (4, 6));
        assert_eq!(word_at(d.rope(), 3), (0, 3));
    }

    #[test]
    fn indent_to_next_tab_stop() {
        let mut d = doc_with_cursors("ab", &[2]);
        d.indent();
        assert_eq!(d.text(), "ab  ");
    }
}
