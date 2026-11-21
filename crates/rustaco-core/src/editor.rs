//! Editing commands, applied to a shared [`TextModel`] on behalf of one editor's [`Cursors`].

use ropey::Rope;

use crate::change::{Assoc, ChangeSet, Edit, inside_crlf};
use crate::history::{EditKind, Transaction};
use crate::model::{Options, TextModel};
use crate::position::{Position, char_to_position, line_end, line_len, position_to_char};
use crate::selection::{Selection, Selections};

/// Identifies an editor among those showing one model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct EditorId(pub u32);

/// One editor's selections, plus the model version they are valid for.
#[derive(Clone, Debug)]
pub struct Cursors {
    id: EditorId,
    pub(crate) selections: Selections,
    synced: u64,
}

impl Cursors {
    pub fn new(id: EditorId, model: &TextModel) -> Self {
        Self { id, selections: Selections::single(Selection::cursor(0)), synced: model.version() }
    }

    pub fn id(&self) -> EditorId {
        self.id
    }

    /// The selections as of the last [`Cursors::sync`]. Call `sync` first when another
    /// editor may have changed the model.
    pub fn selections(&self) -> &Selections {
        &self.selections
    }

    /// Moves the selections through every change made since they were last valid.
    /// A cursor at an insertion point stays before the inserted text.
    pub fn sync(&mut self, model: &TextModel) {
        if self.synced == model.version() {
            return;
        }
        if let Ok(changes) = model.changes_since(self.synced) {
            for cs in changes {
                self.selections = self.selections.map(|s| Selection {
                    anchor: cs.map(s.anchor, Assoc::Before),
                    head: cs.map(s.head, Assoc::Before),
                    goal_column: None,
                });
            }
        }
        self.selections = snap_selections(model.rope(), &self.selections);
        self.synced = model.version();
    }
}

/// Short-lived access to a model for one editor. Creating it syncs the cursors.
pub struct Editor<'a> {
    model: &'a mut TextModel,
    cursors: &'a mut Cursors,
}

impl<'a> Editor<'a> {
    pub fn new(model: &'a mut TextModel, cursors: &'a mut Cursors) -> Self {
        cursors.sync(model);
        Self { model, cursors }
    }

    pub fn selections(&self) -> &Selections {
        &self.cursors.selections
    }

    pub fn set_selections(&mut self, selections: Selections) {
        self.cursors.selections = snap_selections(&self.model.rope, &selections);
        self.model.history.break_group();
    }

    // ---- core plumbing ----

    fn commit(&mut self, changes: ChangeSet, after: Selections, kind: EditKind) {
        if changes.is_empty() {
            self.cursors.selections = snap_selections(&self.model.rope, &after);
            return;
        }
        let before = self.cursors.selections.clone();
        let inverse = self.model.apply(&changes);
        self.cursors.selections = snap_selections(&self.model.rope, &after);
        self.cursors.synced = self.model.version();
        let tx =
            Transaction { changes, inverse, before, after: self.cursors.selections.clone(), origin: self.cursors.id };
        self.model.history.push(tx, kind);
    }

    /// Replaces every selection with the text `f` returns for it; carets land after the text.
    fn replace_each(&mut self, kind: EditKind, mut f: impl FnMut(&Rope, usize, &Selection) -> String) {
        let mut edits = Vec::with_capacity(self.cursors.selections.len());
        let mut carets = Vec::with_capacity(self.cursors.selections.len());
        let mut delta = 0isize;
        for (i, sel) in self.cursors.selections.iter().enumerate() {
            let text = f(&self.model.rope, i, sel);
            let inserted = text.chars().count();
            carets.push(Selection::cursor(sel.start().wrapping_add_signed(delta) + inserted));
            delta += inserted as isize - sel.len() as isize;
            edits.push(Edit::new(sel.start(), sel.end(), text));
        }
        let after = Selections::new(carets, self.cursors.selections.primary_index());
        self.commit(ChangeSet::new(edits).snap_crlf(&self.model.rope), after, kind);
    }

    /// Deletes the range `f` returns for each selection; carets collapse to the range start.
    fn delete_each(&mut self, kind: EditKind, mut f: impl FnMut(&Rope, &Selection) -> (usize, usize)) {
        let edits = self
            .cursors
            .selections
            .iter()
            .map(|s| {
                let (start, end) = f(&self.model.rope, s);
                Edit::delete(start, end)
            })
            .collect();
        let changes = ChangeSet::new(edits).snap_crlf(&self.model.rope);
        let after = self.cursors.selections.map(|s| Selection::cursor(changes.map(s.start(), Assoc::Before)));
        self.commit(changes, after, kind);
    }

    fn move_each(&mut self, extend: bool, mut f: impl FnMut(&Rope, &Selection) -> (usize, Option<usize>)) {
        self.cursors.selections = self.cursors.selections.map(|s| {
            let (head, goal_column) = f(&self.model.rope, s);
            let anchor = if extend { s.anchor } else { head };
            Selection { anchor, head, goal_column }
        });
        self.cursors.selections = snap_selections(&self.model.rope, &self.cursors.selections);
        self.model.history.break_group();
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
        let eol = self.model.line_ending.as_str();
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
        let Options { tab_size, insert_spaces } = self.model.options;
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
        let len = self.model.rope.len_chars();
        let edits = edits.into_iter().map(|e| Edit::new(e.start.min(len), e.end.min(len), e.text)).collect();
        let changes = ChangeSet::new(edits).snap_crlf(&self.model.rope);
        let after = self.cursors.selections.map(|s| Selection {
            anchor: changes.map(s.anchor, Assoc::After),
            head: changes.map(s.head, Assoc::After),
            goal_column: None,
        });
        self.commit(changes, after, EditKind::Other);
        self.model.history.break_group();
    }

    // ---- clipboard ----

    /// Text for the clipboard. With only empty cursors, copies their whole lines.
    pub fn copy(&self) -> String {
        if self.cursors.selections.iter().all(Selection::is_empty) {
            let mut out = String::new();
            let mut last_line = None;
            for s in self.cursors.selections.iter() {
                let line = self.model.rope.char_to_line(s.head);
                if last_line == Some(line) {
                    continue;
                }
                last_line = Some(line);
                out.push_str(&self.model.rope.line(line).to_string());
                if line + 1 == self.model.rope.len_lines() {
                    out.push_str(self.model.line_ending.as_str());
                }
            }
            return out;
        }
        let parts: Vec<String> = self
            .cursors
            .selections
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| self.model.rope.slice(s.start()..s.end()).to_string())
            .collect();
        parts.join(self.model.line_ending.as_str())
    }

    /// Copies, then deletes what was copied.
    pub fn cut(&mut self) -> String {
        let text = self.copy();
        if self.cursors.selections.iter().all(Selection::is_empty) {
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
        let spread = self.cursors.selections.len() > 1 && lines.len() == self.cursors.selections.len();
        self.replace_each(EditKind::Other, |_, i, _| if spread { lines[i].to_owned() } else { text.to_owned() });
        self.model.history.break_group();
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
        self.set_selections(Selections::single(Selection::new(0, self.model.rope.len_chars())));
    }

    /// Adds a cursor one line below the last cursor (or above the first one) and makes it primary.
    pub fn add_cursor_vertical(&mut self, below: bool) {
        let sels = self.cursors.selections.as_slice();
        let source = if below { sels[sels.len() - 1] } else { sels[0] };
        let (head, goal_column) =
            vertical_target(&self.model.rope, &Selection::cursor(source.head), if below { 1 } else { -1 });
        let line = self.model.rope.char_to_line(source.head);
        let at_edge = if below { line + 1 >= self.model.rope.len_lines() } else { line == 0 };
        if at_edge {
            return;
        }
        let mut ranges = sels.to_vec();
        ranges.push(Selection { anchor: head, head, goal_column });
        let primary = ranges.len() - 1;
        self.set_selections(Selections::new(ranges, primary));
    }

    // ---- history ----

    /// Undoes the last step in the model, whichever editor made it. The editor that made it
    /// gets its selections back; any other editor's cursors follow the text.
    pub fn undo(&mut self) -> bool {
        let Some(step) = self.model.history.pop_undo() else { return false };
        for tx in step.transactions.iter().rev() {
            self.model.apply(&tx.inverse);
        }
        let first = &step.transactions[0];
        if first.origin == self.cursors.id {
            self.cursors.selections = snap_selections(&self.model.rope, &first.before);
            self.cursors.synced = self.model.version();
        } else {
            self.cursors.sync(self.model);
        }
        self.model.history.push_redo(step);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(step) = self.model.history.pop_redo() else { return false };
        for tx in &step.transactions {
            self.model.apply(&tx.changes);
        }
        let last = &step.transactions[step.transactions.len() - 1];
        if last.origin == self.cursors.id {
            self.cursors.selections = snap_selections(&self.model.rope, &last.after);
            self.cursors.synced = self.model.version();
        } else {
            self.cursors.sync(self.model);
        }
        self.model.history.push_undo(step);
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
