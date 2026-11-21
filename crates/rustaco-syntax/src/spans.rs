//! Highlight spans per line, shared by the tree-sitter and TextMate engines.

use rustaco_view::{StyleSpan, Styler};

/// Lines longer than this are highlighted only up to this column, as in Monaco.
pub const MAX_HIGHLIGHT_COLUMNS: usize = 20_000;

/// Style spans for a range of lines, ready to hand to the view as a [`Styler`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Highlights {
    first_line: usize,
    lines: Vec<Vec<StyleSpan>>,
}

impl Highlights {
    pub(crate) fn from_paint(first_line: usize, paint: Vec<Vec<u32>>) -> Self {
        let lines = paint
            .into_iter()
            .map(|row| {
                let mut spans: Vec<StyleSpan> = Vec::new();
                for (col, &style) in row.iter().enumerate() {
                    match spans.last_mut() {
                        Some(s) if s.style == style && s.end == col => s.end = col + 1,
                        _ if style != 0 => spans.push(StyleSpan { start: col, end: col + 1, style }),
                        _ => {}
                    }
                }
                spans
            })
            .collect();
        Self { first_line, lines }
    }

    pub fn line(&self, line: usize) -> &[StyleSpan] {
        line.checked_sub(self.first_line).and_then(|i| self.lines.get(i)).map_or(&[], Vec::as_slice)
    }
}

impl Styler for Highlights {
    fn spans(&self, line: usize) -> Vec<StyleSpan> {
        self.line(line).to_vec()
    }
}
