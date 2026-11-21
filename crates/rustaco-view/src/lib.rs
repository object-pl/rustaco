//! Renderer-agnostic layout for the Rustaco editor.
//!
//! [`View`] turns a [`TextModel`], one editor's selections and a viewport into a [`Frame`]: positioned text
//! runs, selection rectangles, carets and line numbers. A frontend only draws the
//! frame; it never measures text or walks the rope itself. Only visible lines and
//! visible columns are laid out, so frame cost does not grow with file size.
//!
//! All coordinates are CSS pixels relative to the viewport's top-left corner,
//! gutter included.
//!
//! Vertical geometry goes through [`View::line_top`] and [`View::line_at_y`] only.
//! Lines are uniform today; per-line heights (rendered markdown headings, inline
//! widgets) will plug in behind these two functions.

use std::ops::Range;

use rustaco_core::{Rope, Selections, TextModel, char_to_position, line_len};
use unicode_width::UnicodeWidthChar;

/// Font measurements supplied by the frontend.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    /// Advance of one monospace cell.
    pub char_width: f32,
    pub line_height: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A styled span on one line, in char columns. Spans are sorted and do not overlap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StyleSpan {
    pub start: usize,
    pub end: usize,
    pub style: u32,
}

/// Supplies syntax or semantic styling. Text outside every span gets style 0.
pub trait Styler {
    fn spans(&self, line: usize) -> Vec<StyleSpan>;
}

/// A styler that leaves everything at style 0.
pub struct Plain;

impl Styler for Plain {
    fn spans(&self, _line: usize) -> Vec<StyleSpan> {
        Vec::new()
    }
}

/// Consecutive chars with one style. Tabs are already expanded to spaces.
#[derive(Clone, Debug, PartialEq)]
pub struct TextRun {
    pub x: f32,
    pub text: String,
    pub style: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FrameLine {
    pub line: usize,
    pub y: f32,
    pub height: f32,
    pub runs: Vec<TextRun>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frame {
    pub gutter_width: f32,
    pub lines: Vec<FrameLine>,
    pub selections: Vec<Rect>,
    pub carets: Vec<Rect>,
    /// Background band of the primary caret's line, when visible.
    pub current_line: Option<Rect>,
}

#[derive(Clone, Debug)]
pub struct View {
    pub metrics: Metrics,
    pub width: f32,
    pub height: f32,
    scroll_top: f32,
    scroll_left: f32,
    /// Widest line laid out so far, in cells. Grows as the user scrolls; full scans are avoided.
    widest_cells: usize,
    /// Tab size of the model at the last layout; used by hit testing and reveal.
    tab_size: usize,
}

impl View {
    pub fn new(metrics: Metrics, width: f32, height: f32) -> Self {
        Self { metrics, width, height, scroll_top: 0.0, scroll_left: 0.0, widest_cells: 0, tab_size: 4 }
    }

    pub fn scroll_top(&self) -> f32 {
        self.scroll_top
    }

    pub fn scroll_left(&self) -> f32 {
        self.scroll_left
    }

    pub fn resize(&mut self, width: f32, height: f32) {
        self.width = width;
        self.height = height;
    }

    pub fn gutter_width(&self, rope: &Rope) -> f32 {
        let digits = rope.len_lines().to_string().len().max(2);
        (digits + 3) as f32 * self.metrics.char_width
    }

    // ---- vertical geometry ----

    pub fn line_top(&self, line: usize) -> f32 {
        line as f32 * self.metrics.line_height
    }

    pub fn line_height(&self, _line: usize) -> f32 {
        self.metrics.line_height
    }

    /// The line at document y, clamped into the document.
    pub fn line_at_y(&self, rope: &Rope, y: f32) -> usize {
        ((y.max(0.0) / self.metrics.line_height) as usize).min(rope.len_lines() - 1)
    }

    /// Height of the scrollable content. The last line can scroll up to the top of the viewport.
    pub fn content_height(&self, rope: &Rope) -> f32 {
        self.line_top(rope.len_lines() - 1) + self.height
    }

    pub fn content_width(&self, rope: &Rope) -> f32 {
        self.gutter_width(rope) + (self.widest_cells + 2) as f32 * self.metrics.char_width
    }

    pub fn visible_lines(&self, rope: &Rope) -> Range<usize> {
        let first = self.line_at_y(rope, self.scroll_top);
        let last = self.line_at_y(rope, self.scroll_top + self.height);
        first..last + 1
    }

    // ---- scrolling ----

    pub fn scroll_to(&mut self, rope: &Rope, top: f32, left: f32) {
        let max_top = (self.content_height(rope) - self.height).max(0.0);
        let max_left = (self.content_width(rope) - self.width).max(0.0);
        self.scroll_top = top.clamp(0.0, max_top);
        self.scroll_left = left.clamp(0.0, max_left);
    }

    pub fn scroll_by(&mut self, rope: &Rope, dx: f32, dy: f32) {
        self.scroll_to(rope, self.scroll_top + dy, self.scroll_left + dx);
    }

    /// Scrolls the minimum distance that makes the char at `pos` fully visible.
    pub fn reveal(&mut self, rope: &Rope, pos: usize) {
        let p = char_to_position(rope, pos);
        let top = self.line_top(p.line);
        let bottom = top + self.line_height(p.line);
        let mut scroll_top = self.scroll_top;
        if top < scroll_top {
            scroll_top = top;
        } else if bottom > scroll_top + self.height {
            scroll_top = bottom - self.height;
        }

        let cw = self.metrics.char_width;
        let cells = visual_column(rope, p.line, p.column, self.tab_size) as f32;
        self.widest_cells = self.widest_cells.max(cells as usize + 1);
        let text_width = self.width - self.gutter_width(rope);
        let margin = 4.0 * cw;
        let x = cells * cw;
        let mut scroll_left = self.scroll_left;
        if x - margin < scroll_left {
            scroll_left = (x - margin).max(0.0);
        } else if x + margin > scroll_left + text_width {
            scroll_left = x + margin - text_width;
        }
        self.scroll_to(rope, scroll_top, scroll_left);
    }

    // ---- hit testing ----

    /// Char offset nearest to a point in viewport coordinates.
    pub fn hit_test(&self, rope: &Rope, x: f32, y: f32) -> usize {
        let line = self.line_at_y(rope, y + self.scroll_top);
        let cells = (x - self.gutter_width(rope) + self.scroll_left) / self.metrics.char_width;
        rope.line_to_char(line) + column_at_cells(rope, line, cells, self.tab_size)
    }

    // ---- layout ----

    pub fn layout(&mut self, model: &TextModel, selections: &Selections, styler: &dyn Styler) -> Frame {
        self.tab_size = model.options.tab_size.max(1);
        let tab = self.tab_size;
        let rope = model.rope();
        let Metrics { char_width: cw, .. } = self.metrics;
        let gutter = self.gutter_width(rope);
        let visible = self.visible_lines(rope);
        let first_cell = (self.scroll_left / cw).floor() as usize;
        let last_cell = ((self.scroll_left + self.width - gutter) / cw).ceil() as usize + 1;
        let x_of = |cells: usize| gutter + cells as f32 * cw - self.scroll_left;
        let y_of = |line: usize| self.line_top(line) - self.scroll_top;

        let mut frame = Frame { gutter_width: gutter, ..Frame::default() };
        let mut widest = self.widest_cells;

        for line in visible.clone() {
            let spans = styler.spans(line);
            let (runs, cells) = layout_line(rope, line, &spans, first_cell..last_cell, tab, &x_of);
            widest = widest.max(cells);
            frame.lines.push(FrameLine { line, y: y_of(line), height: self.line_height(line), runs });
        }

        let primary = selections.primary();
        let primary_line = rope.char_to_line(primary.head);
        if visible.contains(&primary_line) && primary.is_empty() {
            frame.current_line =
                Some(Rect { x: 0.0, y: y_of(primary_line), width: self.width, height: self.line_height(primary_line) });
        }

        for sel in selections.iter() {
            let head = char_to_position(rope, sel.head);
            if visible.contains(&head.line) {
                frame.carets.push(Rect {
                    x: x_of(visual_column(rope, head.line, head.column, tab)),
                    y: y_of(head.line),
                    width: 2.0,
                    height: self.line_height(head.line),
                });
            }
            if sel.is_empty() {
                continue;
            }
            let (start, end) = (char_to_position(rope, sel.start()), char_to_position(rope, sel.end()));
            for line in start.line.max(visible.start)..=end.line.min(visible.end - 1) {
                let from = if line == start.line { start.column } else { 0 };
                let (to, newline) = if line == end.line { (end.column, false) } else { (line_len(rope, line), true) };
                let x0 = x_of(visual_column(rope, line, from, tab));
                let x1 = x_of(visual_column(rope, line, to, tab)) + if newline { cw * 0.5 } else { 0.0 };
                frame.selections.push(Rect { x: x0, y: y_of(line), width: x1 - x0, height: self.line_height(line) });
            }
        }
        self.widest_cells = widest;
        frame
    }
}

fn cell_width(c: char, at_cell: usize, tab: usize) -> usize {
    if c == '\t' { tab - at_cell % tab } else { c.width().unwrap_or(1) }
}

/// Cells from line start to char column `column`, with tabs and wide chars expanded.
pub fn visual_column(rope: &Rope, line: usize, column: usize, tab: usize) -> usize {
    rope.line(line).chars().take(column).fold(0, |cells, c| cells + cell_width(c, cells, tab))
}

/// Char column whose boundary is nearest to `cells` (fractional) on `line`.
pub fn column_at_cells(rope: &Rope, line: usize, cells: f32, tab: usize) -> usize {
    let len = line_len(rope, line);
    let mut acc = 0usize;
    for (i, c) in rope.line(line).chars().take(len).enumerate() {
        let w = cell_width(c, acc, tab);
        if cells < acc as f32 + w as f32 / 2.0 {
            return i;
        }
        acc += w;
    }
    len
}

/// Lays out the cells of `line` inside `window`. Returns the runs and the line's width in
/// cells, measured up to the end of the window.
fn layout_line(
    rope: &Rope,
    line: usize,
    spans: &[StyleSpan],
    window: Range<usize>,
    tab: usize,
    x_of: &dyn Fn(usize) -> f32,
) -> (Vec<TextRun>, usize) {
    let len = line_len(rope, line);
    let mut runs: Vec<TextRun> = Vec::new();
    let mut span_idx = 0;
    let mut acc = 0usize;
    for (col, c) in rope.line(line).chars().take(len).enumerate() {
        let w = cell_width(c, acc, tab);
        if acc >= window.end {
            // Past the visible area: the rest of the line only matters for its width,
            // which is approximated by the char count to keep long lines cheap.
            return (runs, acc + (len - col));
        }
        if acc + w > window.start {
            while span_idx < spans.len() && spans[span_idx].end <= col {
                span_idx += 1;
            }
            let style = spans.get(span_idx).filter(|s| s.start <= col).map_or(0, |s| s.style);
            let start_new = runs.last().is_none_or(|r| r.style != style);
            if start_new {
                runs.push(TextRun { x: x_of(acc), text: String::new(), style });
            }
            let run = runs.last_mut().expect("a run was just ensured");
            if c == '\t' {
                run.text.extend(std::iter::repeat_n(' ', w));
            } else {
                run.text.push(c);
            }
        }
        acc += w;
    }
    (runs, acc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustaco_core::{Document, Selection, Selections};

    fn view() -> View {
        View::new(Metrics { char_width: 10.0, line_height: 20.0 }, 400.0, 100.0)
    }

    #[test]
    fn tabs_and_wide_chars_take_cells() {
        let rope = Rope::from_str("\tab\u{4E2D}c");
        assert_eq!(visual_column(&rope, 0, 1, 4), 4);
        assert_eq!(visual_column(&rope, 0, 4, 4), 8);
        assert_eq!(visual_column(&rope, 0, 1, 2), 2);
        assert_eq!(column_at_cells(&rope, 0, 1.9, 4), 0);
        assert_eq!(column_at_cells(&rope, 0, 2.1, 4), 1);
        assert_eq!(column_at_cells(&rope, 0, 99.0, 4), 5);
    }

    #[test]
    fn layout_only_visible_lines() {
        let text: String = (0..100).map(|i| format!("line {i}\n")).collect();
        let doc = Document::new(&text);
        let mut v = view();
        v.scroll_to(doc.rope(), 400.0, 0.0);
        let frame = v.layout(doc.model(), doc.selections(), &Plain);
        assert_eq!(frame.lines.first().map(|l| l.line), Some(20));
        assert_eq!(frame.lines.len(), 6);
        assert_eq!(frame.lines[0].y, 0.0);
        assert_eq!(frame.lines[0].runs[0].text, "line 20");
    }

    #[test]
    fn runs_split_by_style_and_expand_tabs() {
        struct Kw;
        impl Styler for Kw {
            fn spans(&self, _: usize) -> Vec<StyleSpan> {
                vec![StyleSpan { start: 0, end: 2, style: 7 }]
            }
        }
        let doc = Document::new("fn\tx");
        let frame = view().layout(doc.model(), doc.selections(), &Kw);
        let runs = &frame.lines[0].runs;
        assert_eq!(runs.len(), 2);
        assert_eq!((runs[0].text.as_str(), runs[0].style), ("fn", 7));
        assert_eq!(runs[1].text, "  x");
        assert_eq!(runs[1].x, frame.gutter_width + 20.0);
    }

    #[test]
    fn hit_test_and_reveal() {
        let text: String = (0..50).map(|_| "abcdef\n").collect();
        let doc = Document::new(&text);
        let mut v = view();
        let g = v.gutter_width(doc.rope());
        assert_eq!(v.hit_test(doc.rope(), g + 26.0, 45.0), 2 * 7 + 3);
        v.reveal(doc.rope(), 30 * 7);
        assert_eq!(v.scroll_top(), 31.0 * 20.0 - 100.0);
    }

    #[test]
    fn multi_line_selection_rects() {
        let mut doc = Document::new("abc\nde\nf");
        doc.set_selections(Selections::single(Selection::new(1, 8)));
        let frame = view().layout(doc.model(), doc.selections(), &Plain);
        assert_eq!(frame.selections.len(), 3);
        assert_eq!(frame.carets.len(), 1);
        assert!(frame.current_line.is_none());
    }
}
