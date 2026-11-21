//! Conversions between char offsets and line/column positions.
//!
//! The core addresses text by char offset (what `ropey` indexes natively).
//! Frontends and protocols need other units: LSP and JavaScript count UTF-16
//! code units, tree-sitter counts bytes. Every conversion lives here.

use ropey::Rope;

/// Line and column, both zero-based. `column` counts chars from line start.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

impl Position {
    pub const fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

/// Line and column where `character` counts UTF-16 code units (LSP, JS).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Utf16Position {
    pub line: usize,
    pub character: usize,
}

/// Row and byte column, the coordinates tree-sitter uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Point {
    pub row: usize,
    pub column: usize,
}

/// Number of chars on `line`, excluding the line terminator (`\n`, `\r\n` or `\r`).
pub fn line_len(rope: &Rope, line: usize) -> usize {
    let slice = rope.line(line);
    let mut len = slice.len_chars();
    if len > 0 {
        match slice.char(len - 1) {
            '\n' => {
                len -= 1;
                if len > 0 && slice.char(len - 1) == '\r' {
                    len -= 1;
                }
            }
            '\r' => len -= 1,
            _ => {}
        }
    }
    len
}

/// Char offset of the end of `line`, before its terminator.
pub fn line_end(rope: &Rope, line: usize) -> usize {
    rope.line_to_char(line) + line_len(rope, line)
}

pub fn char_to_position(rope: &Rope, idx: usize) -> Position {
    let idx = idx.min(rope.len_chars());
    let line = rope.char_to_line(idx);
    Position::new(line, idx - rope.line_to_char(line))
}

/// Converts a position to a char offset, clamping line and column into the document.
pub fn position_to_char(rope: &Rope, pos: Position) -> usize {
    let line = pos.line.min(rope.len_lines() - 1);
    rope.line_to_char(line) + pos.column.min(line_len(rope, line))
}

pub fn char_to_utf16(rope: &Rope, idx: usize) -> Utf16Position {
    let idx = idx.min(rope.len_chars());
    let line = rope.char_to_line(idx);
    let line_start = rope.line_to_char(line);
    Utf16Position {
        line,
        character: rope.char_to_utf16_cu(idx) - rope.char_to_utf16_cu(line_start),
    }
}

/// Converts a UTF-16 position to a char offset, clamping into the document.
/// A `character` that falls inside a surrogate pair resolves to the char containing it.
pub fn utf16_to_char(rope: &Rope, pos: Utf16Position) -> usize {
    let line = pos.line.min(rope.len_lines() - 1);
    let line_start = rope.line_to_char(line);
    let end = line_end(rope, line);
    let cu = rope.char_to_utf16_cu(line_start) + pos.character;
    rope.utf16_cu_to_char(cu.min(rope.char_to_utf16_cu(end)))
}

pub fn char_to_point(rope: &Rope, idx: usize) -> Point {
    let idx = idx.min(rope.len_chars());
    let line = rope.char_to_line(idx);
    Point {
        row: line,
        column: rope.char_to_byte(idx) - rope.line_to_byte(line),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_len_strips_every_terminator() {
        let rope = Rope::from_str("ab\r\ncd\nef\rg");
        assert_eq!(rope.len_lines(), 4);
        assert_eq!((0..4).map(|l| line_len(&rope, l)).collect::<Vec<_>>(), [2, 2, 2, 1]);
    }

    #[test]
    fn position_roundtrip_and_clamp() {
        let rope = Rope::from_str("hello\nworld");
        assert_eq!(char_to_position(&rope, 8), Position::new(1, 2));
        assert_eq!(position_to_char(&rope, Position::new(1, 2)), 8);
        assert_eq!(position_to_char(&rope, Position::new(0, 99)), 5);
        assert_eq!(position_to_char(&rope, Position::new(9, 0)), 6);
    }

    #[test]
    fn utf16_counts_surrogate_pairs() {
        // U+1F600 takes two UTF-16 code units and four bytes.
        let rope = Rope::from_str("a\u{1F600}b");
        assert_eq!(char_to_utf16(&rope, 2).character, 3);
        assert_eq!(utf16_to_char(&rope, Utf16Position { line: 0, character: 3 }), 2);
        assert_eq!(char_to_point(&rope, 2).column, 5);
    }
}
