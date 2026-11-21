//! Change sets: a batch of non-overlapping edits applied to a rope in one step.

use ropey::Rope;

use crate::position::{Point, Utf16Position, char_to_point, char_to_utf16};

/// Replace chars `start..end` with `text`. Offsets refer to the document before the change set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

impl Edit {
    pub fn new(start: usize, end: usize, text: impl Into<String>) -> Self {
        assert!(start <= end, "edit range is reversed");
        Self { start, end, text: text.into() }
    }

    pub fn insert(at: usize, text: impl Into<String>) -> Self {
        Self::new(at, at, text)
    }

    pub fn delete(start: usize, end: usize) -> Self {
        Self::new(start, end, String::new())
    }

    fn is_noop(&self) -> bool {
        self.start == self.end && self.text.is_empty()
    }
}

/// Which side of an insertion a mapped position lands on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Assoc {
    Before,
    After,
}

/// Edits sorted by position. Overlapping or touching edits are merged on construction,
/// so every position belongs to at most one edit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangeSet {
    edits: Vec<Edit>,
}

impl ChangeSet {
    /// Sorts and merges `edits`. When two edits overlap, the merged edit replaces the
    /// union of both ranges with their texts concatenated in position order.
    pub fn new(mut edits: Vec<Edit>) -> Self {
        edits.retain(|e| !e.is_noop());
        edits.sort_by_key(|e| (e.start, e.end));
        let mut merged: Vec<Edit> = Vec::with_capacity(edits.len());
        for e in edits {
            match merged.last_mut() {
                Some(last) if e.start <= last.end => {
                    last.end = last.end.max(e.end);
                    last.text.push_str(&e.text);
                }
                _ => merged.push(e),
            }
        }
        Self { edits: merged }
    }

    pub fn edits(&self) -> &[Edit] {
        &self.edits
    }

    pub fn is_empty(&self) -> bool {
        self.edits.is_empty()
    }

    /// Applies the change set and returns its inverse.
    pub fn apply(&self, rope: &mut Rope) -> ChangeSet {
        let mut inverse = Vec::with_capacity(self.edits.len());
        let mut delta = 0isize;
        for e in &self.edits {
            let removed = rope.slice(e.start..e.end).to_string();
            let start = e.start.wrapping_add_signed(delta);
            let inserted = e.text.chars().count();
            inverse.push(Edit::new(start, start + inserted, removed));
            delta += inserted as isize - (e.end - e.start) as isize;
        }
        // Back to front, so earlier offsets stay valid.
        for e in self.edits.iter().rev() {
            rope.remove(e.start..e.end);
            rope.insert(e.start, &e.text);
        }
        ChangeSet { edits: inverse }
    }

    /// Maps a char offset from before the change to after it.
    /// A position inside a replaced range moves to its start (`Before`) or end (`After`).
    pub fn map(&self, pos: usize, assoc: Assoc) -> usize {
        let mut delta = 0isize;
        for e in &self.edits {
            if pos < e.start {
                break;
            }
            let inserted = e.text.chars().count();
            if pos > e.end || (pos == e.end && e.start < e.end) {
                delta += inserted as isize - (e.end - e.start) as isize;
                continue;
            }
            let start = e.start.wrapping_add_signed(delta);
            return match assoc {
                Assoc::Before => start,
                Assoc::After => start + inserted,
            };
        }
        pos.wrapping_add_signed(delta)
    }

    /// Describes the edits for incremental consumers (LSP `didChange`, tree-sitter `Tree::edit`).
    ///
    /// `rope` must be the document before the change. Events come last edit first, so each
    /// event's coordinates stay valid after the events before it have been applied.
    pub fn describe(&self, rope: &Rope) -> Vec<EditEvent> {
        self.edits
            .iter()
            .rev()
            .map(|e| {
                let extent = TextExtent::of(&e.text);
                let start_point = char_to_point(rope, e.start);
                let start_byte = rope.char_to_byte(e.start);
                EditEvent {
                    start: e.start,
                    old_end: e.end,
                    new_end: e.start + extent.chars,
                    start_byte,
                    old_end_byte: rope.char_to_byte(e.end),
                    new_end_byte: start_byte + e.text.len(),
                    start_point,
                    old_end_point: char_to_point(rope, e.end),
                    new_end_point: if extent.line_breaks == 0 {
                        Point { row: start_point.row, column: start_point.column + extent.last_line_bytes }
                    } else {
                        Point { row: start_point.row + extent.line_breaks, column: extent.last_line_bytes }
                    },
                    start_utf16: char_to_utf16(rope, e.start),
                    old_end_utf16: char_to_utf16(rope, e.end),
                    text: e.text.clone(),
                }
            })
            .collect()
    }
}

/// One applied edit, in every unit a consumer might need.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditEvent {
    pub start: usize,
    pub old_end: usize,
    pub new_end: usize,
    pub start_byte: usize,
    pub old_end_byte: usize,
    pub new_end_byte: usize,
    pub start_point: Point,
    pub old_end_point: Point,
    pub new_end_point: Point,
    pub start_utf16: Utf16Position,
    pub old_end_utf16: Utf16Position,
    pub text: String,
}

struct TextExtent {
    chars: usize,
    line_breaks: usize,
    last_line_bytes: usize,
}

impl TextExtent {
    fn of(text: &str) -> Self {
        let mut ext = TextExtent { chars: 0, line_breaks: 0, last_line_bytes: 0 };
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            ext.chars += 1;
            match c {
                '\r' => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                        ext.chars += 1;
                    }
                    ext.line_breaks += 1;
                    ext.last_line_bytes = 0;
                }
                '\n' => {
                    ext.line_breaks += 1;
                    ext.last_line_bytes = 0;
                }
                _ => ext.last_line_bytes += c.len_utf8(),
            }
        }
        ext
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_and_invert() {
        let mut rope = Rope::from_str("hello world");
        let cs = ChangeSet::new(vec![Edit::new(6, 11, "rust"), Edit::insert(0, ">> ")]);
        let inverse = cs.apply(&mut rope);
        assert_eq!(rope.to_string(), ">> hello rust");
        inverse.apply(&mut rope);
        assert_eq!(rope.to_string(), "hello world");
    }

    #[test]
    fn merges_overlapping_and_touching() {
        let cs = ChangeSet::new(vec![Edit::delete(2, 3), Edit::delete(1, 2), Edit::delete(0, 2)]);
        assert_eq!(cs.edits(), [Edit::delete(0, 3)]);
    }

    #[test]
    fn map_positions() {
        let cs = ChangeSet::new(vec![Edit::insert(2, "xx"), Edit::new(5, 8, "y")]);
        assert_eq!(cs.map(1, Assoc::After), 1);
        assert_eq!(cs.map(2, Assoc::Before), 2);
        assert_eq!(cs.map(2, Assoc::After), 4);
        assert_eq!(cs.map(6, Assoc::Before), 7);
        assert_eq!(cs.map(8, Assoc::Before), 8);
        assert_eq!(cs.map(10, Assoc::Before), 10);
    }

    #[test]
    fn describe_reports_bytes_points_and_utf16() {
        let rope = Rope::from_str("é\nab");
        let cs = ChangeSet::new(vec![Edit::new(3, 4, "x\nyz"), Edit::insert(0, "\u{1F600}")]);
        let ev = cs.describe(&rope);
        assert_eq!(ev.len(), 2);
        // Last edit first.
        assert_eq!(ev[0].start_point, Point { row: 1, column: 1 });
        assert_eq!(ev[0].new_end_point, Point { row: 2, column: 2 });
        assert_eq!(ev[0].start_byte, 4);
        assert_eq!(ev[1].start_utf16, Utf16Position { line: 0, character: 0 });
        assert_eq!(ev[1].new_end_byte, 4);
    }
}
