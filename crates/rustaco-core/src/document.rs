//! The editable document: buffer, selections, history and the editing commands.

use ropey::Rope;

use crate::change::{Assoc, ChangeSet, Edit, EditEvent, inside_crlf};
use crate::history::{EditKind, History, Transaction};
use crate::position::{Position, char_to_position, line_end, line_len, position_to_char};
use crate::selection::{Selection, Selections};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineEnding {
    #[default]
    Lf,
    CrLf,
    Cr,
}

impl LineEnding {
    pub fn as_str(self) -> &'static str {
        match self {
            LineEnding::Lf => "\n",
            LineEnding::CrLf => "\r\n",
            LineEnding::Cr => "\r",
        }
    }

    /// The first line terminator in the text decides; text without one gets `Lf`.
    pub fn detect(rope: &Rope) -> Self {
        let mut chars = rope.chars();
        while let Some(c) = chars.next() {
            match c {
                '\n' => return LineEnding::Lf,
                '\r' => return if chars.next() == Some('\n') { LineEnding::CrLf } else { LineEnding::Cr },
                _ => {}
            }
        }
        LineEnding::Lf
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub tab_size: usize,
    pub insert_spaces: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { tab_size: 4, insert_spaces: true }
    }
}

#[derive(Clone, Debug)]
pub struct Document {
    rope: Rope,
    selections: Selections,
    history: History,
    events: Vec<EditEvent>,
    version: u64,
    line_ending: LineEnding,
    pub options: Options,
}

impl Document {
    pub fn new(text: &str) -> Self {
        let rope = Rope::from_str(text);
        Self {
            line_ending: LineEnding::detect(&rope),
            rope,
            selections: Selections::single(Selection::cursor(0)),
            history: History::default(),
            events: Vec::new(),
            version: 0,
            options: Options::default(),
        }
    }

    pub fn rope(&self) -> &Rope {
        &self.rope
    }

    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    /// Increases by one with every change to the text, including undo and redo.
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn line_ending(&self) -> LineEnding {
        self.line_ending
    }

    pub fn selections(&self) -> &Selections {
        &self.selections
    }

    pub fn set_selections(&mut self, selections: Selections) {
        self.selections = snap_selections(&self.rope, &selections);
        self.history.break_group();
    }

    /// Returns and clears the edits applied since the last call, in application order.
    pub fn take_events(&mut self) -> Vec<EditEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Ends the current undo group explicitly.
    pub fn checkpoint(&mut self) {
        self.history.break_group();
    }

    // ---- core plumbing ----

    /// The only place the rope changes. Undo inverses can land inside a CRLF pair that a
    /// deletion created, so every change set is snapped here, not only new edits.
    fn apply_raw(&mut self, changes: &ChangeSet) -> ChangeSet {
        if changes.needs_crlf_snap(&self.rope) {
            let snapped = changes.clone().snap_crlf(&self.rope);
            return self.apply_raw(&snapped);
        }
        self.events.extend(changes.describe(&self.rope));
        self.version += 1;
        changes.apply(&mut self.rope)
    }

    fn commit(&mut self, changes: ChangeSet, after: Selections, kind: EditKind) {
        if changes.is_empty() {
            self.selections = snap_selections(&self.rope, &after);
            return;
        }
        let before = self.selections.clone();
        let inverse = self.apply_raw(&changes);
        self.selections = snap_selections(&self.rope, &after);
        let tx = Transaction { changes, inverse, before, after: self.selections.clone() };
        self.history.push(tx, kind);
    }

    /// Replaces every selection with the text `f` returns for it; carets land after the text.
    fn replace_each(&mut self, kind: EditKind, mut f: impl FnMut(&Rope, usize, &Selection) -> String) {
        let mut edits = Vec::with_capacity(self.selections.len());
        let mut carets = Vec::with_capacity(self.selections.len());
        let mut delta = 0isize;
        for (i, sel) in self.selections.iter().enumerate() {
            let text = f(&self.rope, i, sel);
            let inserted = text.chars().count();
            carets.push(Selection::cursor(sel.start().wrapping_add_signed(delta) + inserted));
            delta += inserted as isize - sel.len() as isize;
            edits.push(Edit::new(sel.start(), sel.end(), text));
        }
        let after = Selections::new(carets, self.selections.primary_index());
        self.commit(ChangeSet::new(edits).snap_crlf(&self.rope), after, kind);
    }

    /// Deletes the range `f` returns for each selection; carets collapse to the range start.
    fn delete_each(&mut self, kind: EditKind, mut f: impl FnMut(&Rope, &Selection) -> (usize, usize)) {
        let edits = self
            .selections
            .iter()
            .map(|s| {
                let (start, end) = f(&self.rope, s);
                Edit::delete(start, end)
            })
            .collect();
        let changes = ChangeSet::new(edits).snap_crlf(&self.rope);
        let after = self.selections.map(|s| Selection::cursor(changes.map(s.start(), Assoc::Before)));
        self.commit(changes, after, kind);
    }

    fn move_each(&mut self, extend: bool, mut f: impl FnMut(&Rope, &Selection) -> (usize, Option<usize>)) {
        self.selections = self.selections.map(|s| {
            let (head, goal_column) = f(&self.rope, s);
            let anchor = if extend { s.anchor } else { head };
            Selection { anchor, head, goal_column }
        });
        self.selections = snap_selections(&self.rope, &self.selections);
        self.history.break_group();
    }

    // ---- editing ----

    /// Types `text` at every selection, replacing selected text.
    pub fn insert(&mut self, text: &str) {
        let mut chars = text.chars();
        let single_char = matches!((chars.next(), chars.next()), (Some(c), None) if c != '\n' && c != '\r');
        let kind = if single_char { EditKind::Typing } else { EditKind::Other };
        self.replace_each(kind, |_, _, _| text.to_owned());
    }

    /// Inserts a line break and copies the current line's indentation up to the caret.
    pub fn newline(&mut self) {
        let eol = self.line_ending.as_str();
        self.replace_each(EditKind::Other, |rope, _, sel| {
            let line = rope.char_to_line(sel.start());
            let line_start = rope.line_to_char(line);
            let indent: String =
                rope.slice(line_start..sel.start()).chars().take_while(|c| *c == ' ' || *c == '\t').collect();
            format!("{eol}{indent}")
        });
    }

    /// Inserts indentation up to the next tab stop at every cursor.
    pub fn indent(&mut self) {
        let Options { tab_size, insert_spaces } = self.options;
        self.replace_each(EditKind::Other, |rope, _, sel| {
            if !insert_spaces {
                return "\t".to_owned();
            }
            let column = char_to_position(rope, sel.start()).column;
            " ".repeat(tab_size - column % tab_size)
        });
    }

    /// Backspace: deletes the selection, or the char before each cursor.
    pub fn delete_backward(&mut self) {
        self.delete_each(EditKind::Deleting, |rope, s| {
            if s.is_empty() { (prev_boundary(rope, s.head), s.head) } else { (s.start(), s.end()) }
        });
    }

    /// Delete: deletes the selection, or the char after each cursor.
    pub fn delete_forward(&mut self) {
        self.delete_each(EditKind::Deleting, |rope, s| {
            if s.is_empty() { (s.head, next_boundary(rope, s.head)) } else { (s.start(), s.end()) }
        });
    }

    /// Ctrl+Backspace: deletes the selection, or back to the previous word start.
    pub fn delete_word_backward(&mut self) {
        self.delete_each(EditKind::Other, |rope, s| {
            if s.is_empty() { (word_left(rope, s.head), s.head) } else { (s.start(), s.end()) }
        });
    }

    /// Ctrl+Delete: deletes the selection, or forward to the next word end.
    pub fn delete_word_forward(&mut self) {
        self.delete_each(EditKind::Other, |rope, s| {
            if s.is_empty() { (s.head, word_right(rope, s.head)) } else { (s.start(), s.end()) }
        });
    }

    /// Applies edits that did not come from typing, such as LSP text edits or formatting.
    /// Selections follow the text around them.
    pub fn apply_edits(&mut self, edits: Vec<Edit>) {
        let len = self.rope.len_chars();
        let edits = edits.into_iter().map(|e| Edit::new(e.start.min(len), e.end.min(len), e.text)).collect();
        let changes = ChangeSet::new(edits).snap_crlf(&self.rope);
        let after = self.selections.map(|s| Selection {
            anchor: changes.map(s.anchor, Assoc::After),
            head: changes.map(s.head, Assoc::After),
            goal_column: None,
        });
        self.commit(changes, after, EditKind::Other);
        self.history.break_group();
    }

    // ---- clipboard ----

    /// Text for the clipboard. With only empty cursors, copies their whole lines.
    pub fn copy(&self) -> String {
        if self.selections.iter().all(Selection::is_empty) {
            let mut out = String::new();
            let mut last_line = None;
            for s in self.selections.iter() {
                let line = self.rope.char_to_line(s.head);
                if last_line == Some(line) {
                    continue;
                }
                last_line = Some(line);
                out.push_str(&self.rope.line(line).to_string());
                if line + 1 == self.rope.len_lines() {
                    out.push_str(self.line_ending.as_str());
                }
            }
            return out;
        }
        let parts: Vec<String> = self
            .selections
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| self.rope.slice(s.start()..s.end()).to_string())
            .collect();
        parts.join(self.line_ending.as_str())
    }

    /// Copies, then deletes what was copied.
    pub fn cut(&mut self) -> String {
        let text = self.copy();
        if self.selections.iter().all(Selection::is_empty) {
            self.delete_each(EditKind::Other, |rope, s| {
                let line = rope.char_to_line(s.head);
                (rope.line_to_char(line), rope.line_to_char(line + 1))
            });
        } else {
            self.delete_each(EditKind::Other, |_, s| (s.start(), s.end()));
        }
        text
    }

    /// Pastes `text`. When it has exactly one line per cursor, each cursor gets its own line.
    pub fn paste(&mut self, text: &str) {
        let lines: Vec<&str> = text.lines().collect();
        let spread = self.selections.len() > 1 && lines.len() == self.selections.len();
        self.replace_each(EditKind::Other, |_, i, _| if spread { lines[i].to_owned() } else { text.to_owned() });
        self.history.break_group();
    }

    // ---- movement ----

    pub fn move_left(&mut self, extend: bool) {
        self.move_each(extend, |rope, s| {
            if !extend && !s.is_empty() { (s.start(), None) } else { (prev_boundary(rope, s.head), None) }
        });
    }

    pub fn move_right(&mut self, extend: bool) {
        self.move_each(extend, |rope, s| {
            if !extend && !s.is_empty() { (s.end(), None) } else { (next_boundary(rope, s.head), None) }
        });
    }

    pub fn move_word_left(&mut self, extend: bool) {
        self.move_each(extend, |rope, s| (word_left(rope, s.head), None));
    }

    pub fn move_word_right(&mut self, extend: bool) {
        self.move_each(extend, |rope, s| (word_right(rope, s.head), None));
    }

    pub fn move_up(&mut self, extend: bool) {
        self.move_vertical(-1, extend);
    }

    pub fn move_down(&mut self, extend: bool) {
        self.move_vertical(1, extend);
    }

    /// Moves every caret `lines` lines up (negative) or down, keeping the goal column.
    pub fn move_vertical(&mut self, lines: isize, extend: bool) {
        self.move_each(extend, |rope, s| vertical_target(rope, s, lines));
    }

    /// Home: toggles between the first non-whitespace char and column 0.
    pub fn move_line_start(&mut self, extend: bool) {
        self.move_each(extend, |rope, s| {
            let line = rope.char_to_line(s.head);
            let start = rope.line_to_char(line);
            let indent = rope.line(line).chars().take(line_len(rope, line)).take_while(|c| c.is_whitespace()).count();
            let target = if s.head == start + indent { start } else { start + indent };
            (target, None)
        });
    }

    pub fn move_line_end(&mut self, extend: bool) {
        self.move_each(extend, |rope, s| (line_end(rope, rope.char_to_line(s.head)), None));
    }

    pub fn move_document_start(&mut self, extend: bool) {
        self.move_each(extend, |_, _| (0, None));
    }

    pub fn move_document_end(&mut self, extend: bool) {
        self.move_each(extend, |rope, _| (rope.len_chars(), None));
    }

    pub fn select_all(&mut self) {
        self.set_selections(Selections::single(Selection::new(0, self.rope.len_chars())));
    }

    /// Adds a cursor one line below the last cursor (or above the first one) and makes it primary.
    pub fn add_cursor_vertical(&mut self, below: bool) {
        let sels = self.selections.as_slice();
        let source = if below { sels[sels.len() - 1] } else { sels[0] };
        let (head, goal_column) =
            vertical_target(&self.rope, &Selection::cursor(source.head), if below { 1 } else { -1 });
        let line = self.rope.char_to_line(source.head);
        let at_edge = if below { line + 1 >= self.rope.len_lines() } else { line == 0 };
        if at_edge {
            return;
        }
        let mut ranges = sels.to_vec();
        ranges.push(Selection { anchor: head, head, goal_column });
        let primary = ranges.len() - 1;
        self.set_selections(Selections::new(ranges, primary));
    }

    // ---- history ----

    pub fn undo(&mut self) -> bool {
        let Some(step) = self.history.pop_undo() else { return false };
        for tx in step.transactions.iter().rev() {
            self.apply_raw(&tx.inverse);
        }
        self.selections = step.transactions[0].before.clone();
        self.history.push_redo(step);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(step) = self.history.pop_redo() else { return false };
        for tx in &step.transactions {
            self.apply_raw(&tx.changes);
        }
        self.selections = step.transactions[step.transactions.len() - 1].after.clone();
        self.history.push_undo(step);
        true
    }
}

/// Clamps selections into the document and moves endpoints out of CRLF pairs.
fn snap_selections(rope: &Rope, selections: &Selections) -> Selections {
    let len = rope.len_chars();
    let snap = |p: usize| {
        let p = p.min(len);
        if inside_crlf(rope, p) { p - 1 } else { p }
    };
    selections.map(|s| Selection { anchor: snap(s.anchor), head: snap(s.head), goal_column: s.goal_column })
}

/// Char offset one step left, treating `\r\n` as a single step.
fn prev_boundary(rope: &Rope, pos: usize) -> usize {
    if pos >= 2 && rope.char(pos - 1) == '\n' && rope.char(pos - 2) == '\r' { pos - 2 } else { pos.saturating_sub(1) }
}

/// Char offset one step right, treating `\r\n` as a single step.
fn next_boundary(rope: &Rope, pos: usize) -> usize {
    let len = rope.len_chars();
    if pos + 1 < len && rope.char(pos) == '\r' && rope.char(pos + 1) == '\n' { pos + 2 } else { (pos + 1).min(len) }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Word,
    Space,
    Newline,
    Punct,
}

fn char_class(c: char) -> CharClass {
    match c {
        '\n' | '\r' => CharClass::Newline,
        c if c.is_whitespace() => CharClass::Space,
        c if c.is_alphanumeric() || c == '_' => CharClass::Word,
        _ => CharClass::Punct,
    }
}

/// Ctrl+Right: skips spaces, then one run of word or punctuation chars. Stops at line ends.
pub(crate) fn word_right(rope: &Rope, mut pos: usize) -> usize {
    let len = rope.len_chars();
    if pos < len && char_class(rope.char(pos)) == CharClass::Newline {
        return next_boundary(rope, pos);
    }
    while pos < len && char_class(rope.char(pos)) == CharClass::Space {
        pos += 1;
    }
    if pos < len {
        let class = char_class(rope.char(pos));
        if class != CharClass::Newline {
            while pos < len && char_class(rope.char(pos)) == class {
                pos += 1;
            }
        }
    }
    pos
}

/// Ctrl+Left: mirror of [`word_right`].
pub(crate) fn word_left(rope: &Rope, mut pos: usize) -> usize {
    if pos > 0 && char_class(rope.char(pos - 1)) == CharClass::Newline {
        return prev_boundary(rope, pos);
    }
    while pos > 0 && char_class(rope.char(pos - 1)) == CharClass::Space {
        pos -= 1;
    }
    if pos > 0 {
        let class = char_class(rope.char(pos - 1));
        if class != CharClass::Newline {
            while pos > 0 && char_class(rope.char(pos - 1)) == class {
                pos -= 1;
            }
        }
    }
    pos
}

/// The run of same-class chars around `pos`, preferring a word on either side. Used by double-click.
pub fn word_at(rope: &Rope, pos: usize) -> (usize, usize) {
    let len = rope.len_chars();
    let class_at = |i: usize| (i < len).then(|| char_class(rope.char(i)));
    let class = match (class_at(pos), pos.checked_sub(1).and_then(class_at)) {
        (Some(CharClass::Word), _) => CharClass::Word,
        (_, Some(CharClass::Word)) => CharClass::Word,
        (Some(c), _) if c != CharClass::Newline => c,
        (_, Some(c)) if c != CharClass::Newline => c,
        _ => return (pos, pos),
    };
    let (mut start, mut end) = (pos, pos);
    while start > 0 && char_class(rope.char(start - 1)) == class {
        start -= 1;
    }
    while end < len && char_class(rope.char(end)) == class {
        end += 1;
    }
    (start, end)
}

fn vertical_target(rope: &Rope, s: &Selection, lines: isize) -> (usize, Option<usize>) {
    let pos = char_to_position(rope, s.head);
    let goal = s.goal_column.unwrap_or(pos.column);
    let target = pos.line as isize + lines;
    if target < 0 {
        return (0, None);
    }
    if target as usize >= rope.len_lines() {
        return (rope.len_chars(), None);
    }
    (position_to_char(rope, Position::new(target as usize, goal)), Some(goal))
}

#[cfg(test)]
mod tests {
    use super::*;

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
