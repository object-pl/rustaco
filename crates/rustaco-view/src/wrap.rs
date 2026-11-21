//! Word wrapping: where a model line breaks into rows.

use rustaco_core::{Rope, RopeSlice};

use crate::cell_width;

/// The rows of one model line: the char column each row starts at (`starts[0]` is 0) and
/// the indentation, in cells, of every row after the first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineRows {
    pub starts: Vec<usize>,
    pub indent: usize,
}

impl LineRows {
    /// A line that is not wrapped.
    pub fn single() -> Self {
        Self { starts: vec![0], indent: 0 }
    }

    pub fn count(&self) -> usize {
        self.starts.len()
    }

    /// The row holding char `column`. A column where a row starts belongs to that row.
    pub fn row_of(&self, column: usize) -> usize {
        self.starts.partition_point(|&s| s <= column).saturating_sub(1)
    }

    /// Indentation of `row` in cells.
    pub fn indent_of(&self, row: usize) -> usize {
        if row == 0 { 0 } else { self.indent }
    }
}

/// Breaks `line` into rows at most `width` cells wide; see [`wrap_slice`].
pub fn wrap_line(rope: &Rope, line: usize, tab: usize, width: usize) -> LineRows {
    wrap_slice(rope.line(line), tab, width)
}

/// Breaks a line (with or without its line break) into rows at most `width` cells wide.
/// Rows break after a run of spaces when the row holds one, otherwise inside the word.
/// Spaces may hang past the edge, so a row never starts with the spaces that ended the
/// previous one. Continuation rows are indented like the line itself, unless that
/// indentation takes half the width or more.
pub fn wrap_slice(slice: RopeSlice, tab: usize, width: usize) -> LineRows {
    if width == 0 {
        return LineRows::single();
    }
    match wrap_ascii(slice, width) {
        Some(rows) => rows,
        None => wrap_chars(slice, tab, width),
    }
}

/// Feeds cells to the wrapping state machine.
struct Wrapper {
    width: usize,
    indent: usize,
    starts: Vec<usize>,
    row_cells: usize,
    break_at: Option<(usize, usize)>,
    prev_space: bool,
}

impl Wrapper {
    fn new(width: usize, lead: usize) -> Self {
        let indent = if lead * 2 < width { lead } else { 0 };
        Self { width, indent, starts: vec![0], row_cells: 0, break_at: None, prev_space: false }
    }

    #[inline]
    fn push(&mut self, col: usize, w: usize, space: bool) {
        if !space {
            if self.prev_space {
                self.break_at = Some((col, self.row_cells));
            }
            let row_start = *self.starts.last().expect("starts is never empty");
            if self.row_cells + w > self.width && col > row_start {
                match self.break_at.filter(|&(b, _)| b > row_start) {
                    Some((b, before)) => {
                        self.starts.push(b);
                        self.row_cells = self.indent + (self.row_cells - before);
                    }
                    None => {
                        self.starts.push(col);
                        self.row_cells = self.indent;
                    }
                }
                self.break_at = None;
            }
        }
        self.row_cells += w;
        self.prev_space = space;
    }

    fn finish(self) -> LineRows {
        LineRows { starts: self.starts, indent: self.indent }
    }
}

/// ASCII text without tabs, where every byte is one char and one cell, read straight from
/// the rope's chunks. None when the line has other chars.
fn wrap_ascii(slice: RopeSlice, width: usize) -> Option<LineRows> {
    if slice.len_bytes() <= width {
        // Cells never exceed bytes except for tabs, so a short line without tabs fits.
        return (!slice.chunks().any(|c| c.as_bytes().contains(&b'\t'))).then(LineRows::single);
    }
    let mut lead = 0;
    'lead: for chunk in slice.chunks() {
        for &b in chunk.as_bytes() {
            if b != b' ' {
                break 'lead;
            }
            lead += 1;
        }
    }
    let mut wrapper = Wrapper::new(width, lead);
    let mut col = 0;
    for chunk in slice.chunks() {
        for &b in chunk.as_bytes() {
            match b {
                b'\n' | b'\r' => return Some(wrapper.finish()),
                b'\t' | 0x80.. => return None,
                _ => wrapper.push(col, 1, b == b' '),
            }
            col += 1;
        }
    }
    Some(wrapper.finish())
}

/// Any text: tabs expand to the next stop and wide chars take two cells.
fn wrap_chars(slice: RopeSlice, tab: usize, width: usize) -> LineRows {
    let chars = || slice.chars().take_while(|&c| c != '\n' && c != '\r');
    let mut lead = 0;
    for c in chars() {
        match c {
            ' ' | '\t' => lead += cell_width(c, lead, tab),
            _ => break,
        }
    }
    let mut wrapper = Wrapper::new(width, lead);
    let mut line_cells = 0; // cells from the line start, for tab stops
    for (col, c) in chars().enumerate() {
        let w = cell_width(c, line_cells, tab);
        wrapper.push(col, w, c == ' ' || c == '\t');
        line_cells += w;
    }
    wrapper.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(text: &str, width: usize) -> Vec<String> {
        let rope = Rope::from_str(text);
        let r = wrap_line(&rope, 0, 4, width);
        let chars: Vec<char> = text.chars().collect();
        let mut out = Vec::new();
        for (i, &s) in r.starts.iter().enumerate() {
            let e = r.starts.get(i + 1).copied().unwrap_or(chars.len());
            out.push(chars[s..e].iter().collect());
        }
        out
    }

    #[test]
    fn breaks_after_spaces() {
        assert_eq!(rows("aaa bbb ccc", 7), vec!["aaa bbb ", "ccc"], "the space hangs past the edge");
        assert_eq!(rows("aaa bbb ccc", 6), vec!["aaa ", "bbb ", "ccc"]);
        assert_eq!(rows("short", 80), vec!["short"]);
    }

    #[test]
    fn long_words_break_inside() {
        assert_eq!(rows("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
    }

    #[test]
    fn continuation_rows_keep_the_indentation() {
        let rope = Rope::from_str("    one two three");
        let r = wrap_line(&rope, 0, 4, 12);
        assert_eq!(r.indent, 4);
        assert_eq!(r.starts, vec![0, 12]);
        assert_eq!(rows("    one two three", 12), vec!["    one two ", "three"]);
    }

    #[test]
    fn the_ascii_path_matches_the_general_one() {
        for text in [
            "aaa bbb ccc ddd",
            "    one two three four five",
            "x  y   z    wwwwwwwwwwwwwwwwww q",
            "  aaaaaaaaaaaaaaaaaaaaaa",
        ] {
            for width in 3..20 {
                let rope = Rope::from_str(text);
                // A trailing wide char forces the general path; compare the rows before it.
                let wide = Rope::from_str(&format!("{text} \u{4E2D}"));
                let mut general = wrap_line(&wide, 0, 4, width);
                let n = text.chars().count();
                general.starts.retain(|&s| s <= n);
                if general.starts.last() == Some(&n) {
                    general.starts.pop();
                }
                assert_eq!(wrap_line(&rope, 0, 4, width), general, "{text:?} at {width}");
            }
        }
    }

    #[test]
    fn wide_chars_take_two_cells() {
        assert_eq!(rows("\u{4E2D}\u{4E2D}\u{4E2D}", 4), vec!["\u{4E2D}\u{4E2D}", "\u{4E2D}"]);
    }
}
