//! Positions between the editor's char offsets and LSP's line and character, in the
//! encoding the server and client agreed on.

use rustaco_core::{Point, Rope, Utf16Position, char_to_point, char_to_utf16, line_end, utf16_to_char};
use serde_json::{Value, json};

/// How LSP counts the `character` of a position.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PositionEncoding {
    /// UTF-16 code units, LSP's default.
    #[default]
    Utf16,
    /// Bytes, when the server offers it (saves conversions).
    Utf8,
}

impl PositionEncoding {
    pub fn name(self) -> &'static str {
        match self {
            Self::Utf16 => "utf-16",
            Self::Utf8 => "utf-8",
        }
    }
}

/// `{ line, character }` of char offset `offset`.
pub fn position(rope: &Rope, offset: usize, encoding: PositionEncoding) -> Value {
    let (line, character) = match encoding {
        PositionEncoding::Utf16 => {
            let p = char_to_utf16(rope, offset);
            (p.line, p.character)
        }
        PositionEncoding::Utf8 => {
            let p = char_to_point(rope, offset);
            (p.row, p.column)
        }
    };
    json!({ "line": line, "character": character })
}

/// The char offset of an LSP position, clamped into the text.
pub fn offset(rope: &Rope, position: &Value, encoding: PositionEncoding) -> usize {
    let line = position.get("line").and_then(Value::as_u64).unwrap_or(0) as usize;
    let character = position.get("character").and_then(Value::as_u64).unwrap_or(0) as usize;
    if line >= rope.len_lines() {
        return rope.len_chars();
    }
    match encoding {
        PositionEncoding::Utf16 => utf16_to_char(rope, Utf16Position { line, character }),
        PositionEncoding::Utf8 => point_to_char(rope, Point { row: line, column: character }),
    }
}

fn point_to_char(rope: &Rope, p: Point) -> usize {
    let start = rope.line_to_byte(p.row);
    let end = rope.char_to_byte(line_end(rope, p.row));
    let byte = (start + p.column).min(end);
    // A column inside a multi-byte char rounds down to its start.
    rope.byte_to_char(byte)
}

/// `{ start, end }` of a char range.
pub fn range(rope: &Rope, r: std::ops::Range<usize>, encoding: PositionEncoding) -> Value {
    json!({ "start": position(rope, r.start, encoding), "end": position(rope, r.end, encoding) })
}

/// The char range of an LSP range.
pub fn char_range(rope: &Rope, range: &Value, encoding: PositionEncoding) -> std::ops::Range<usize> {
    let start = range.get("start").map_or(0, |p| offset(rope, p, encoding));
    let end = range.get("end").map_or(start, |p| offset(rope, p, encoding));
    start..end.max(start)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_and_utf8_positions_round_trip() {
        let rope = Rope::from_str("a\u{1F600}b\n\u{e9}x");
        for enc in [PositionEncoding::Utf16, PositionEncoding::Utf8] {
            for o in 0..=rope.len_chars() {
                assert_eq!(offset(&rope, &position(&rope, o, enc), enc), o, "{enc:?} at {o}");
            }
        }
        assert_eq!(position(&rope, 2, PositionEncoding::Utf16), json!({"line": 0, "character": 3}));
        assert_eq!(position(&rope, 2, PositionEncoding::Utf8), json!({"line": 0, "character": 5}));
        assert_eq!(position(&rope, 5, PositionEncoding::Utf8), json!({"line": 1, "character": 2}));
    }
}
