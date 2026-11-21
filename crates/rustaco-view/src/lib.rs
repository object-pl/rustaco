//! Renderer-agnostic layout for the Rustaco editor.
//!
//! [`View`] turns a [`TextModel`], one editor's selections and a viewport into a [`Frame`]:
//! positioned text runs, selection rectangles, carets and line numbers. A frontend only
//! draws the frame; it never measures text or walks the rope itself. Only visible lines and
//! visible columns are laid out, so frame cost does not grow with file size.
//!
//! All coordinates are CSS pixels relative to the viewport's top-left corner, gutter
//! included.
//!
//! A model line shows as one or more rows (word wrap) or not at all (inside a fold).
//! Vertical geometry goes through [`View::line_top`] and [`View::line_at_y`]: a
//! [`HeightMap`] holds every line's own height, rows times the line height, and folds are
//! taken out on top of it.

mod folding;
mod heights;
mod wrap;

pub use folding::{FoldMarker, indent_fold_ranges};
pub use heights::HeightMap;
pub use wrap::{LineRows, wrap_line, wrap_slice};

use std::ops::Range;

use folding::{Fold, Hidden, shift_foldable, shift_lines};
use rustaco_core::{Assoc, Rope, Selections, TextModel, char_to_position, line_end, line_len};
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

/// One row on screen: a whole line, or one row of a wrapped line.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameLine {
    pub line: usize,
    /// Row within the line; 0 for its first row, which carries the line number.
    pub row: usize,
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
    /// Fold markers for the gutter, on shown lines that head a foldable or folded region.
    pub fold_markers: Vec<FoldMarker>,
    /// Placeholders drawn after the text of folded header lines.
    pub fold_ellipses: Vec<Rect>,
}

/// Width of the fold marker column at the right of the gutter, in cells.
const FOLD_MARKER_CELLS: f32 = 2.0;

/// Space kept free for the vertical scrollbar when wrapping to the viewport.
const SCROLLBAR_WIDTH: f32 = 14.0;

/// Narrowest wrapping width, in cells.
const MIN_WRAP_CELLS: usize = 10;

#[derive(Clone, Debug)]
pub struct View {
    pub metrics: Metrics,
    pub width: f32,
    pub height: f32,
    scroll_top: f64,
    scroll_left: f32,
    /// Widest line laid out so far, in cells. Grows as the user scrolls; full scans are avoided.
    widest_cells: usize,
    /// Tab size of the model at the last sync; used by hit testing and reveal.
    tab_size: usize,
    /// Word wrap at the viewport's width.
    wrap: bool,
    heights: HeightMap,
    /// What `heights` was computed for: model version, line height, wrap width, tab size.
    synced: Option<Synced>,
    folds: Vec<Fold>,
    hidden: Hidden,
    /// Regions that can be folded, as (header line, last line), sorted by header.
    foldable: Vec<(usize, usize)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Synced {
    version: u64,
    line_height: f32,
    wrap_cells: Option<usize>,
    tab_size: usize,
}

impl View {
    pub fn new(metrics: Metrics, width: f32, height: f32) -> Self {
        Self {
            metrics,
            width,
            height,
            scroll_top: 0.0,
            scroll_left: 0.0,
            widest_cells: 0,
            tab_size: 4,
            wrap: false,
            heights: HeightMap::new(1, metrics.line_height),
            synced: None,
            folds: Vec::new(),
            hidden: Hidden::default(),
            foldable: Vec::new(),
        }
    }

    pub fn scroll_top(&self) -> f64 {
        self.scroll_top
    }

    pub fn scroll_left(&self) -> f32 {
        self.scroll_left
    }

    pub fn resize(&mut self, width: f32, height: f32) {
        self.width = width;
        self.height = height;
    }

    /// Turns word wrap at the viewport's width on or off. Takes effect at the next sync.
    pub fn set_wrap(&mut self, wrap: bool) {
        self.wrap = wrap;
        if wrap {
            self.scroll_left = 0.0;
        }
    }

    pub fn wraps(&self) -> bool {
        self.wrap
    }

    pub fn gutter_width(&self, rope: &Rope) -> f32 {
        let digits = rope.len_lines().to_string().len().max(2);
        (digits as f32 + 3.0 + FOLD_MARKER_CELLS) * self.metrics.char_width
    }

    /// Wrapping width in cells, when wrapping.
    fn wrap_cells(&self, rope: &Rope) -> Option<usize> {
        self.wrap.then(|| {
            let text = self.width - self.gutter_width(rope) - SCROLLBAR_WIDTH;
            ((text / self.metrics.char_width).floor().max(0.0) as usize).max(MIN_WRAP_CELLS)
        })
    }

    /// The rows `line` shows as.
    pub fn line_rows(&self, rope: &Rope, line: usize) -> LineRows {
        match self.synced.and_then(|s| s.wrap_cells) {
            Some(w) => wrap_line(rope, line, self.tab_size, w),
            None => LineRows::single(),
        }
    }

    // ---- model sync ----

    /// Brings line heights and folds up to date with `model`. Layout calls it; call it too
    /// before hit testing or revealing right after an edit.
    pub fn sync(&mut self, model: &TextModel) {
        let rope = model.rope();
        let target = Synced {
            version: model.version(),
            line_height: self.metrics.line_height,
            wrap_cells: self.wrap_cells(rope),
            tab_size: model.options.tab_size.max(1),
        };
        if self.synced == Some(target) && self.heights.len() == rope.len_lines() {
            return;
        }
        self.tab_size = target.tab_size;
        let lh = target.line_height;
        let layout_same = self.synced.is_some_and(|s| Synced { version: target.version, ..s } == target);
        let mut incremental = false;
        if let Some(s) = self.synced.filter(|_| layout_same)
            && let (Ok(events), Ok(changes)) = (model.events_since(s.version), model.changes_since(s.version))
        {
            // Lines whose rows must be measured again, in final coordinates.
            let mut touched: Vec<Range<usize>> = Vec::new();
            for e in events {
                let a = e.start_point.row;
                let old = e.old_end_point.row - a;
                let new = e.new_end_point.row - a;
                if old != 0 || new != 0 {
                    self.heights.splice(a + 1, old, std::iter::repeat_n(lh, new));
                    shift_foldable(&mut self.foldable, a, old, new);
                    for r in &mut touched {
                        shift_lines(r, a, old, new);
                    }
                }
                touched.push(a..a + new + 1);
            }
            for c in changes {
                for f in &mut self.folds {
                    f.start = c.map(f.start, Assoc::After);
                    f.end = c.map(f.end, Assoc::After);
                }
            }
            incremental = self.heights.len() == rope.len_lines();
            if incremental && target.wrap_cells.is_some() {
                self.synced = Some(target);
                for r in touched {
                    for line in r.start..r.end.min(rope.len_lines()) {
                        let rows = self.line_rows(rope, line).count();
                        self.heights.set(line, rows as f32 * lh);
                    }
                }
            }
        }
        if !incremental {
            let stale = self.synced.is_some() && layout_same;
            self.synced = Some(target);
            self.heights = match target.wrap_cells {
                None => HeightMap::new(rope.len_lines(), lh),
                Some(width) => {
                    let mut map = HeightMap::new(0, lh);
                    let tab = self.tab_size;
                    // Sequential line access; looking each line up costs more than wrapping it.
                    let rows = rope.lines().map(|line| wrap_slice(line, tab, width).count() as f32 * lh);
                    map.splice(0, 0, rows);
                    map
                }
            };
            if stale {
                // The change log no longer reaches back; positions are unknown.
                self.folds.clear();
                self.foldable.clear();
            }
        }
        self.synced = Some(target);
        self.apply_folds(rope);
    }

    /// Recomputes the hidden lines from `folds`.
    fn apply_folds(&mut self, rope: &Rope) {
        let len = rope.len_chars();
        let mut ranges: Vec<Range<usize>> = Vec::new();
        self.folds.retain(|f| {
            let (header, last) = (rope.char_to_line(f.start.min(len)), rope.char_to_line(f.end.min(len)));
            if last <= header {
                return false;
            }
            ranges.push(header + 1..last + 1);
            true
        });
        self.folds.sort_by_key(|f| f.start);
        self.folds.dedup_by_key(|f| rope.char_to_line(f.start.min(len)));
        self.hidden = Hidden::new(ranges, &self.heights);
    }

    // ---- folding ----

    /// Replaces the regions that can be folded: (header line, last line) pairs, for example
    /// from [`indent_fold_ranges`] or a syntax tree.
    pub fn set_foldable(&mut self, mut ranges: Vec<(usize, usize)>) {
        ranges.retain(|&(s, e)| e > s);
        ranges.sort_unstable();
        ranges.dedup();
        self.foldable = ranges;
    }

    pub fn is_hidden(&self, line: usize) -> bool {
        self.hidden.run_of(line).is_some()
    }

    /// `line`, or the first shown line after the hidden run holding it.
    fn next_shown(&self, line: usize) -> usize {
        self.hidden.run_of(line).map_or(line, |i| self.hidden.ranges[i].end)
    }

    /// `line`, or the header above the hidden run holding it.
    fn prev_shown(&self, line: usize) -> usize {
        self.hidden.run_of(line).map_or(line, |i| self.hidden.ranges[i].start.saturating_sub(1))
    }

    /// Shown lines in `lines`, skipping folded ones without walking them.
    fn shown(&self, lines: Range<usize>) -> Vec<usize> {
        let mut out = Vec::new();
        let mut line = self.next_shown(lines.start);
        while line < lines.end {
            out.push(line);
            line = self.next_shown(line + 1);
        }
        out
    }

    /// Runs of shown lines in `lines`, as ranges.
    pub fn shown_segments(&self, lines: Range<usize>) -> Vec<Range<usize>> {
        let mut out: Vec<Range<usize>> = Vec::new();
        for l in self.shown(lines) {
            match out.last_mut() {
                Some(r) if r.end == l => r.end += 1,
                _ => out.push(l..l + 1),
            }
        }
        out
    }

    /// Whether `line` heads a collapsed fold.
    pub fn is_folded(&self, rope: &Rope, line: usize) -> bool {
        let len = rope.len_chars();
        self.folds.iter().any(|f| rope.char_to_line(f.start.min(len)) == line)
    }

    /// Collapses the innermost open region that holds `line`. Returns false if none does.
    pub fn fold(&mut self, model: &TextModel, line: usize) -> bool {
        self.sync(model);
        let rope = model.rope();
        let candidate = self
            .foldable
            .iter()
            .filter(|&&(s, e)| s <= line && line <= e && !self.is_folded(rope, s))
            .min_by_key(|&&(s, e)| e - s)
            .copied();
        let Some((s, e)) = candidate else { return false };
        self.folds.push(Fold { start: line_end(rope, s), end: line_end(rope, e) });
        self.apply_folds(rope);
        true
    }

    /// Opens the folds headed at `line` or hiding it. Returns false if there were none.
    pub fn unfold(&mut self, model: &TextModel, line: usize) -> bool {
        self.sync(model);
        let rope = model.rope();
        let len = rope.len_chars();
        let before = self.folds.len();
        self.folds.retain(|f| {
            let (h, l) = (rope.char_to_line(f.start.min(len)), rope.char_to_line(f.end.min(len)));
            !(h <= line && line <= l)
        });
        let changed = self.folds.len() != before;
        if changed {
            self.apply_folds(rope);
        }
        changed
    }

    /// Folds or unfolds at `line`.
    pub fn toggle_fold(&mut self, model: &TextModel, line: usize) -> bool {
        self.sync(model);
        if self.is_folded(model.rope(), line) { self.unfold(model, line) } else { self.fold(model, line) }
    }

    /// Collapses every foldable region.
    pub fn fold_all(&mut self, model: &TextModel) {
        self.sync(model);
        let rope = model.rope();
        self.folds =
            self.foldable.iter().map(|&(s, e)| Fold { start: line_end(rope, s), end: line_end(rope, e) }).collect();
        self.apply_folds(rope);
    }

    pub fn unfold_all(&mut self, model: &TextModel) {
        self.sync(model);
        self.folds.clear();
        self.apply_folds(model.rope());
    }

    /// Opens folds hiding any of `positions` (char offsets), as when a caret lands inside
    /// one. Returns whether anything changed.
    pub fn reveal_positions(&mut self, model: &TextModel, positions: impl IntoIterator<Item = usize>) -> bool {
        self.sync(model);
        let rope = model.rope();
        let lines: Vec<usize> = positions
            .into_iter()
            .map(|p| rope.char_to_line(p.min(rope.len_chars())))
            .filter(|&l| self.is_hidden(l))
            .collect();
        if lines.is_empty() {
            return false;
        }
        let len = rope.len_chars();
        self.folds.retain(|f| {
            let (h, l) = (rope.char_to_line(f.start.min(len)), rope.char_to_line(f.end.min(len)));
            !lines.iter().any(|&x| h < x && x <= l)
        });
        self.apply_folds(rope);
        true
    }

    /// The header line of the fold hiding `line`, or `line` when it is shown.
    pub fn shown_line_for(&self, line: usize) -> usize {
        self.prev_shown(line)
    }

    // ---- vertical geometry ----

    /// Document y of the top of `line`. A hidden line sits at the top of the next shown line.
    pub fn line_top(&self, line: usize) -> f64 {
        if line < self.heights.len() {
            self.hidden.top(&self.heights, line)
        } else {
            // Not synced yet: extrapolate past the known lines.
            self.hidden.total(&self.heights) + (line - self.heights.len()) as f64 * self.metrics.line_height as f64
        }
    }

    /// Height of `line`: all its rows, or 0 when it is folded away.
    pub fn line_height(&self, line: usize) -> f32 {
        if line >= self.heights.len() {
            self.metrics.line_height
        } else if self.is_hidden(line) {
            0.0
        } else {
            self.heights.height(line)
        }
    }

    /// The shown line at document y, clamped into the document.
    pub fn line_at_y(&self, rope: &Rope, y: f64) -> usize {
        let total = self.hidden.total(&self.heights);
        let line = if y >= total && self.heights.len() < rope.len_lines() {
            // Not synced yet: extrapolate past the known lines.
            self.heights.len() + ((y - total) / self.metrics.line_height as f64) as usize
        } else {
            self.hidden.line_at(&self.heights, y)
        };
        line.min(rope.len_lines() - 1)
    }

    /// The line and row at document y.
    fn row_at_y(&self, rope: &Rope, y: f64) -> (usize, usize) {
        let line = self.line_at_y(rope, y);
        let row = ((y - self.line_top(line)) / self.metrics.line_height as f64).max(0.0) as usize;
        let rows = (self.line_height(line) / self.metrics.line_height).round().max(1.0) as usize;
        (line, row.min(rows - 1))
    }

    /// Height of the scrollable content. The last line can scroll up to the top of the viewport.
    pub fn content_height(&self, rope: &Rope) -> f64 {
        let last = self.prev_shown(rope.len_lines() - 1);
        self.line_top(last) + self.height as f64
    }

    pub fn content_width(&self, rope: &Rope) -> f32 {
        if self.wrap {
            return self.width;
        }
        self.gutter_width(rope) + (self.widest_cells + 2) as f32 * self.metrics.char_width
    }

    /// Lines from the first to the last on screen. Folded lines inside are included; layout
    /// skips them.
    pub fn visible_lines(&self, rope: &Rope) -> Range<usize> {
        let first = self.line_at_y(rope, self.scroll_top);
        let last = self.line_at_y(rope, self.scroll_top + self.height as f64);
        first..last + 1
    }

    /// Viewport y of the top of `line`.
    fn y_of(&self, line: usize) -> f32 {
        (self.line_top(line) - self.scroll_top) as f32
    }

    /// Row and x (cells from the text area's left edge, before scrolling) of char `column`
    /// on `line`.
    fn locate(&self, rope: &Rope, rows: &LineRows, line: usize, column: usize) -> (usize, usize) {
        let row = rows.row_of(column);
        let tab = self.tab_size;
        let start_cells = if row == 0 { 0 } else { visual_column(rope, line, rows.starts[row], tab) };
        (row, rows.indent_of(row) + visual_column(rope, line, column, tab) - start_cells)
    }

    /// Char column on `row` of `line` nearest to x `cells` (as [`View::locate`] measures).
    /// Past the end of a row that wraps, the column stays before the row's last char.
    fn column_in_row(&self, rope: &Rope, rows: &LineRows, line: usize, row: usize, cells: f32) -> usize {
        let tab = self.tab_size;
        let start = rows.starts[row];
        let start_cells = if row == 0 { 0 } else { visual_column(rope, line, start, tab) };
        let target = (cells - rows.indent_of(row) as f32).max(0.0) + start_cells as f32;
        let column = column_at_cells(rope, line, target, tab).max(start);
        match rows.starts.get(row + 1) {
            Some(&next) => column.min(next - 1),
            None => column,
        }
    }

    /// The caret target `lines` rows above (negative) or below `head`, skipping folded
    /// lines and keeping `goal` (x in cells within the row) when given: (offset, goal).
    pub fn vertical_target(
        &self,
        rope: &Rope,
        head: usize,
        goal: Option<usize>,
        lines: isize,
    ) -> (usize, Option<usize>) {
        let p = char_to_position(rope, head);
        let mut line = self.prev_shown(p.line);
        let mut rows = self.line_rows(rope, line);
        let (mut row, x) = self.locate(rope, &rows, line, p.column.min(line_len(rope, line)));
        let goal = goal.unwrap_or(x);
        for _ in 0..lines.unsigned_abs() {
            if lines < 0 {
                if row > 0 {
                    row -= 1;
                    continue;
                }
                if line == 0 {
                    return (0, None);
                }
                line = self.prev_shown(line - 1);
                rows = self.line_rows(rope, line);
                row = rows.count() - 1;
            } else {
                if row + 1 < rows.count() {
                    row += 1;
                    continue;
                }
                let next = self.next_shown(line + 1);
                if next >= rope.len_lines() {
                    return (rope.len_chars(), None);
                }
                line = next;
                rows = self.line_rows(rope, line);
                row = 0;
            }
        }
        let column = self.column_in_row(rope, &rows, line, row, goal as f32);
        (rope.line_to_char(line) + column, Some(goal))
    }

    /// Rows a page holds.
    pub fn page_rows(&self) -> usize {
        ((self.height / self.metrics.line_height) as usize).saturating_sub(1).max(1)
    }

    // ---- scrolling ----

    pub fn scroll_to(&mut self, rope: &Rope, top: f64, left: f32) {
        let max_top = (self.content_height(rope) - self.height as f64).max(0.0);
        let max_left = (self.content_width(rope) - self.width).max(0.0);
        self.scroll_top = top.clamp(0.0, max_top);
        self.scroll_left = if self.wrap { 0.0 } else { left.clamp(0.0, max_left) };
    }

    pub fn scroll_by(&mut self, rope: &Rope, dx: f32, dy: f64) {
        self.scroll_to(rope, self.scroll_top + dy, self.scroll_left + dx);
    }

    /// Scrolls the minimum distance that makes the char at `pos` fully visible.
    pub fn reveal(&mut self, rope: &Rope, pos: usize) {
        let p = char_to_position(rope, pos);
        let line = self.prev_shown(p.line);
        let rows = self.line_rows(rope, line);
        let (row, x_cells) = if line == p.line { self.locate(rope, &rows, line, p.column) } else { (0, 0) };
        let lh = self.metrics.line_height as f64;
        let top = self.line_top(line) + row as f64 * lh;
        let bottom = top + lh;
        let mut scroll_top = self.scroll_top;
        if top < scroll_top {
            scroll_top = top;
        } else if bottom > scroll_top + self.height as f64 {
            scroll_top = bottom - self.height as f64;
        }

        let cw = self.metrics.char_width;
        self.widest_cells = self.widest_cells.max(x_cells + 1);
        let text_width = self.width - self.gutter_width(rope);
        let margin = 4.0 * cw;
        let x = x_cells as f32 * cw;
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
        let (line, row) = self.row_at_y(rope, y as f64 + self.scroll_top);
        let rows = self.line_rows(rope, line);
        let cells = ((x - self.gutter_width(rope) + self.scroll_left) / self.metrics.char_width).max(0.0);
        rope.line_to_char(line) + self.column_in_row(rope, &rows, line, row.min(rows.count() - 1), cells)
    }

    /// The line and visual column (fractional cells from the line start, may lie past the
    /// line's end) at a point in viewport coordinates. Column selection uses cells, so it
    /// stays rectangular across lines with tabs.
    pub fn line_and_cells_at(&self, rope: &Rope, x: f32, y: f32) -> (usize, f32) {
        let (line, row) = self.row_at_y(rope, y as f64 + self.scroll_top);
        let cells = ((x - self.gutter_width(rope) + self.scroll_left) / self.metrics.char_width).max(0.0);
        if row == 0 {
            return (line, cells);
        }
        let rows = self.line_rows(rope, line);
        let row = row.min(rows.count() - 1);
        let start_cells = visual_column(rope, line, rows.starts[row], self.tab_size) as f32;
        (line, (cells - rows.indent_of(row) as f32).max(0.0) + start_cells)
    }

    /// Whether viewport x falls on the fold marker column of the gutter.
    pub fn in_fold_gutter(&self, rope: &Rope, x: f32) -> bool {
        let g = self.gutter_width(rope);
        x >= g - (FOLD_MARKER_CELLS + 0.5) * self.metrics.char_width && x < g - 0.5 * self.metrics.char_width
    }

    /// Char offset of the column nearest to `cells` on `line`.
    pub fn offset_at_cells(&self, rope: &Rope, line: usize, cells: f32) -> usize {
        rope.line_to_char(line) + column_at_cells(rope, line, cells, self.tab_size)
    }

    // ---- layout ----

    pub fn layout(&mut self, model: &TextModel, selections: &Selections, styler: &dyn Styler) -> Frame {
        self.sync(model);
        let tab = self.tab_size;
        let rope = model.rope();
        let Metrics { char_width: cw, line_height: lh } = self.metrics;
        let gutter = self.gutter_width(rope);
        let visible = self.visible_lines(rope);
        let first_cell = (self.scroll_left / cw).floor() as usize;
        let last_cell = ((self.scroll_left + self.width - gutter) / cw).ceil() as usize + 1;

        let mut frame = Frame { gutter_width: gutter, ..Frame::default() };
        let mut widest = self.widest_cells;
        let len = rope.len_chars();
        let folded_headers: Vec<usize> = self.folds.iter().map(|f| rope.char_to_line(f.start.min(len))).collect();
        let mut foldable_at = self.foldable.partition_point(|&(s, _)| s < visible.start);
        let (top, bottom) = (-lh, self.height + lh);

        for line in self.shown(visible.clone()) {
            let spans = styler.spans(line);
            let rows = self.line_rows(rope, line);
            let y = self.y_of(line);
            let line_len = line_len(rope, line);
            for row in 0..rows.count() {
                let row_y = y + row as f32 * lh;
                if row_y < top || row_y > bottom {
                    continue;
                }
                let cols = rows.starts[row]..rows.starts.get(row + 1).copied().unwrap_or(line_len);
                let start_cells = if row == 0 { 0 } else { visual_column(rope, line, cols.start, tab) };
                let shift = rows.indent_of(row) as f32 - start_cells as f32;
                let x_of = |cells: usize| gutter + (cells as f32 + shift) * cw - self.scroll_left;
                let window = first_cell + start_cells..last_cell + start_cells;
                let (runs, cells) = layout_line(rope, line, cols, &spans, window, tab, &x_of);
                widest = widest.max(cells);
                frame.lines.push(FrameLine { line, row, y: row_y, height: lh, runs });
            }
            while foldable_at < self.foldable.len() && self.foldable[foldable_at].0 < line {
                foldable_at += 1;
            }
            let folded = folded_headers.contains(&line);
            if folded || self.foldable.get(foldable_at).is_some_and(|&(s, _)| s == line) {
                frame.fold_markers.push(FoldMarker { line, y, height: lh, folded });
            }
            if folded {
                let (row, x) = self.locate(rope, &rows, line, line_len);
                let end = gutter + x as f32 * cw - self.scroll_left;
                frame.fold_ellipses.push(Rect { x: end + cw, y: y + row as f32 * lh, width: 3.0 * cw, height: lh });
            }
        }

        let primary = selections.primary();
        let primary_line = rope.char_to_line(primary.head);
        if visible.contains(&primary_line) && primary.is_empty() && !self.is_hidden(primary_line) {
            frame.current_line = Some(Rect {
                x: 0.0,
                y: self.y_of(primary_line),
                width: self.width,
                height: self.line_height(primary_line),
            });
        }

        for sel in selections.iter() {
            if let Some(r) = self.caret_rect(rope, sel.head, gutter) {
                frame.carets.push(r);
            }
            if sel.is_empty() {
                continue;
            }
            frame.selections.extend(self.range_rects(rope, sel.start()..sel.end(), gutter));
        }
        self.widest_cells = widest;
        frame
    }

    /// The caret at char `pos`, if its line is visible.
    fn caret_rect(&self, rope: &Rope, pos: usize, gutter: f32) -> Option<Rect> {
        let p = char_to_position(rope, pos);
        if !self.visible_lines(rope).contains(&p.line) || self.is_hidden(p.line) {
            return None;
        }
        let rows = self.line_rows(rope, p.line);
        let (row, x) = self.locate(rope, &rows, p.line, p.column);
        let lh = self.metrics.line_height;
        Some(Rect {
            x: gutter + x as f32 * self.metrics.char_width - self.scroll_left,
            y: self.y_of(p.line) + row as f32 * lh,
            width: 2.0,
            height: lh,
        })
    }

    /// Rectangles covering `range` on the visible lines, one per row; a line break inside
    /// the range adds half a cell at the end of its line, as for selections.
    fn range_rects(&self, rope: &Rope, range: std::ops::Range<usize>, gutter: f32) -> Vec<Rect> {
        let cw = self.metrics.char_width;
        let lh = self.metrics.line_height;
        let visible = self.visible_lines(rope);
        let x_at = |cells: usize| gutter + cells as f32 * cw - self.scroll_left;
        let (start, end) = (char_to_position(rope, range.start), char_to_position(rope, range.end));
        let mut out = Vec::new();
        let lines = start.line.max(visible.start)..(end.line + 1).min(visible.end);
        for line in self.shown(lines) {
            let from = if line == start.line { start.column } else { 0 };
            let (to, newline) = if line == end.line { (end.column, false) } else { (line_len(rope, line), true) };
            let rows = self.line_rows(rope, line);
            let y = self.y_of(line);
            let (first_row, x_from) = self.locate(rope, &rows, line, from);
            let (last_row, x_to) = self.locate(rope, &rows, line, to);
            for row in first_row..=last_row {
                let x0 = if row == first_row { x_from } else { rows.indent_of(row) };
                let (x1, extra) = if row == last_row {
                    (x_to, if newline { cw * 0.5 } else { 0.0 })
                } else {
                    // Up to the end of the row, where the next row's first char starts.
                    let (_, end_cells) = self.locate_row_end(rope, &rows, line, row);
                    (end_cells, 0.0)
                };
                let (x0, x1) = (x_at(x0), x_at(x1) + extra);
                out.push(Rect { x: x0, y: y + row as f32 * lh, width: x1 - x0, height: lh });
            }
        }
        out
    }

    /// Row and x of the end of `row`, a row that is followed by another.
    fn locate_row_end(&self, rope: &Rope, rows: &LineRows, line: usize, row: usize) -> (usize, usize) {
        let tab = self.tab_size;
        let start_cells = if row == 0 { 0 } else { visual_column(rope, line, rows.starts[row], tab) };
        let end_cells = visual_column(rope, line, rows.starts[row + 1], tab);
        (row, rows.indent_of(row) + end_cells - start_cells)
    }

    /// Rectangles for decorations on the visible lines, with each decoration's kind. Call
    /// after [`View::layout`] for the same frame.
    pub fn decoration_rects(&self, model: &TextModel) -> Vec<(Rect, u32)> {
        let rope = model.rope();
        let visible = self.visible_lines(rope);
        let range = rope.line_to_char(visible.start)..rope.line_to_char(visible.end.min(rope.len_lines()));
        let gutter = self.gutter_width(rope);
        model
            .decorations
            .in_range(range)
            .into_iter()
            .flat_map(|d| self.range_rects(rope, d.range.clone(), gutter).into_iter().map(move |r| (r, d.kind)))
            .collect()
    }

    /// Bands across the text area for whole-line decorations (kinds `whole_line` accepts) on
    /// the visible lines: one band per shown line the decoration touches.
    pub fn line_bands(&self, model: &TextModel, whole_line: impl Fn(u32) -> bool) -> Vec<(Rect, u32)> {
        let gutter = self.gutter_width(model.rope());
        self.decorated_lines(model, whole_line)
            .into_iter()
            .map(|(line, kind)| {
                let r =
                    Rect { x: gutter, y: self.y_of(line), width: self.width - gutter, height: self.line_height(line) };
                (r, kind)
            })
            .collect()
    }

    /// Marks for the gutter: for each visible shown line a decoration (of a kind `accept`
    /// takes) touches, the line's y and height with the kind.
    pub fn gutter_marks(&self, model: &TextModel, accept: impl Fn(u32) -> bool) -> Vec<(f32, f32, u32)> {
        self.decorated_lines(model, accept)
            .into_iter()
            .map(|(line, kind)| (self.y_of(line), self.line_height(line), kind))
            .collect()
    }

    /// Visible shown lines touched by decorations of accepted kinds, once per (line, kind).
    fn decorated_lines(&self, model: &TextModel, accept: impl Fn(u32) -> bool) -> Vec<(usize, u32)> {
        let rope = model.rope();
        let visible = self.visible_lines(rope);
        let range = rope.line_to_char(visible.start)..rope.line_to_char(visible.end.min(rope.len_lines()));
        let mut out = Vec::new();
        for d in model.decorations.in_range(range).into_iter().filter(|d| accept(d.kind)) {
            let first = rope.char_to_line(d.range.start).max(visible.start);
            // A range ending at a line start does not touch that line.
            let end_pos = if d.range.end > d.range.start { d.range.end - 1 } else { d.range.end };
            let last = rope.char_to_line(end_pos.min(rope.len_chars())).min(visible.end - 1);
            for line in self.shown(first..last + 1) {
                out.push((line, d.kind));
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Marks for the overview ruler, a strip `track` pixels tall standing for the whole
    /// document: for every decoration of an accepted kind, its top and height on the strip
    /// (at least 2 px) with its kind. Folded lines map to their fold's header.
    pub fn overview_marks(&self, model: &TextModel, track: f32, accept: impl Fn(u32) -> bool) -> Vec<(f32, f32, u32)> {
        let rope = model.rope();
        let total = (self.content_height(rope) - self.height as f64 + self.metrics.line_height as f64).max(1.0);
        let scale = track as f64 / total;
        let len = rope.len_chars();
        let mut out: Vec<(f32, f32, u32)> = model
            .decorations
            .iter()
            .filter(|d| accept(d.kind))
            .map(|d| {
                let a = self.prev_shown(rope.char_to_line(d.range.start.min(len)));
                let b = self.prev_shown(rope.char_to_line(d.range.end.min(len)));
                let top = self.line_top(a) * scale;
                let bottom = (self.line_top(b) + self.line_height(b) as f64) * scale;
                (top as f32, ((bottom - top) as f32).max(2.0), d.kind)
            })
            .collect();
        out.sort_by(|x, y| x.0.total_cmp(&y.0));
        out
    }

    /// Indentation guides for the visible lines: one vertical segment per indentation level
    /// and run of lines. A blank line takes the smaller level of the nearest non-blank lines
    /// around it, so guides continue through blank lines inside a block but not past its end.
    /// The guide of the block holding `cursor_line` is marked active. Folded lines are
    /// skipped: a guide runs on through a fold only if the lines on both sides are inside it.
    pub fn indent_guides(&self, model: &TextModel, cursor_line: usize) -> Vec<Guide> {
        let rope = model.rope();
        let tab = self.tab_size.max(1);
        let visible = self.visible_lines(rope);
        let total = rope.len_lines();
        let lines = self.shown(visible.start..visible.end.min(total));
        let level_at = |line: usize| guide_level(rope, line, tab);
        let levels: Vec<usize> = lines.iter().map(|&l| level_at(l)).collect();
        let active_level = level_at(cursor_line).max(1);

        let cw = self.metrics.char_width;
        let gutter = self.gutter_width(rope);
        let mut guides = Vec::new();
        let max_level = levels.iter().copied().max().unwrap_or(0);
        for level in 1..=max_level {
            let x = gutter + ((level - 1) * tab) as f32 * cw - self.scroll_left;
            let mut start: Option<usize> = None;
            for i in 0..=lines.len() {
                let inside = i < lines.len() && levels[i] >= level;
                match (inside, start) {
                    (true, None) => start = Some(i),
                    (false, Some(s)) => {
                        let (first, last) = (lines[s], lines[i - 1]);
                        let bottom_line = self.next_shown(last + 1);
                        let active = level == active_level && (first..bottom_line).contains(&cursor_line);
                        guides.push(Guide {
                            x,
                            top: self.y_of(first),
                            bottom: self.y_of(last) + self.line_height(last),
                            level,
                            active,
                        });
                        start = None;
                    }
                    _ => {}
                }
            }
        }
        guides
    }

    /// The rectangle of the char at `pos` (one cell wide for a line end), if visible.
    pub fn char_rects(&self, model: &TextModel, pos: usize) -> Vec<Rect> {
        let rope = model.rope();
        let end = (pos + 1).min(rope.len_chars());
        let mut rects = self.range_rects(rope, pos..end, self.gutter_width(rope));
        rects.truncate(1);
        rects
    }

    /// Rectangles covering `range` on the visible lines.
    pub fn char_range_rects(&self, model: &TextModel, range: std::ops::Range<usize>) -> Vec<Rect> {
        let rope = model.rope();
        self.range_rects(rope, range, self.gutter_width(rope))
    }

    /// X positions of rulers drawn at the given columns.
    pub fn ruler_positions(&self, model: &TextModel, columns: &[usize]) -> Vec<f32> {
        let gutter = self.gutter_width(model.rope());
        columns.iter().map(|&c| gutter + c as f32 * self.metrics.char_width - self.scroll_left).collect()
    }

    /// Positions of spaces and tabs on the visible lines, for drawing them as `·` and `→`:
    /// (x of the cell start, y of the row top, width in cells, is a tab).
    pub fn whitespace_marks(&self, model: &TextModel) -> Vec<(f32, f32, usize, bool)> {
        let rope = model.rope();
        let tab = self.tab_size.max(1);
        let cw = self.metrics.char_width;
        let lh = self.metrics.line_height;
        let gutter = self.gutter_width(rope);
        let mut out = Vec::new();
        for line in self.shown(self.visible_lines(rope)) {
            let y = self.y_of(line);
            let rows = self.line_rows(rope, line);
            let mut cells = 0;
            let mut row = 0;
            let mut row_shift = 0isize;
            for (col, c) in rope.line(line).chars().take(line_len(rope, line).min(MAX_MARKED_COLUMNS)).enumerate() {
                if rows.starts.get(row + 1) == Some(&col) {
                    row += 1;
                    row_shift = rows.indent as isize - cells as isize;
                }
                let w = cell_width(c, cells, tab);
                if c == ' ' || c == '\t' {
                    let x = (cells as isize + row_shift) as f32;
                    out.push((gutter + x * cw - self.scroll_left, y + row as f32 * lh, w, c == '\t'));
                }
                cells += w;
            }
        }
        out
    }
}

/// Nearest blank-line lookaround for guide levels.
const GUIDE_REACH: usize = 200;

/// Indentation level of `line` for guides; a blank line takes the smaller level of the
/// nearest non-blank lines around it.
fn guide_level(rope: &Rope, line: usize, tab: usize) -> usize {
    if let Some(l) = indent_level(rope, line, tab) {
        return l;
    }
    let total = rope.len_lines();
    let prev = (line.saturating_sub(GUIDE_REACH)..line).rev().find_map(|l| indent_level(rope, l, tab)).unwrap_or(0);
    let next = (line + 1..(line + GUIDE_REACH).min(total)).find_map(|l| indent_level(rope, l, tab)).unwrap_or(0);
    prev.min(next)
}

/// Indentation of `line` in levels, or None for a blank line.
fn indent_level(rope: &Rope, line: usize, tab: usize) -> Option<usize> {
    let len = line_len(rope, line);
    let mut cells = 0;
    for c in rope.line(line).chars().take(len) {
        match c {
            ' ' => cells += 1,
            '\t' => cells += tab - cells % tab,
            _ => return Some(cells / tab),
        }
    }
    None
}

/// Whitespace marks stop after this many chars on a line.
const MAX_MARKED_COLUMNS: usize = 2000;

/// A vertical indentation guide in viewport coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Guide {
    pub x: f32,
    pub top: f32,
    pub bottom: f32,
    pub level: usize,
    /// The guide of the block that holds the cursor.
    pub active: bool,
}

pub(crate) fn cell_width(c: char, at_cell: usize, tab: usize) -> usize {
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

/// Lays out the chars in `cols` of `line` that fall inside `window` (cells from the line
/// start). Returns the runs and the line's width in cells, measured up to the end of the
/// window.
fn layout_line(
    rope: &Rope,
    line: usize,
    cols: Range<usize>,
    spans: &[StyleSpan],
    window: Range<usize>,
    tab: usize,
    x_of: &dyn Fn(usize) -> f32,
) -> (Vec<TextRun>, usize) {
    let mut runs: Vec<TextRun> = Vec::new();
    let mut span_idx = 0;
    let mut acc = 0usize;
    for (col, c) in rope.line(line).chars().take(cols.end).enumerate() {
        let w = cell_width(c, acc, tab);
        if acc >= window.end {
            // Past the visible area: the rest of the line only matters for its width,
            // which is approximated by the char count to keep long lines cheap.
            return (runs, acc + (cols.end - col));
        }
        if col >= cols.start && acc + w > window.start {
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
    fn decoration_rects_follow_the_text() {
        use rustaco_core::Stickiness;
        let doc = Document::new("abc\ndef\nghi");
        let mut v = view();
        v.layout(doc.model(), doc.selections(), &Plain);
        let mut model = doc.model().clone();
        model.decorations.set(7, 3, Stickiness::NeverGrows, [1..2, 5..9]);
        let rects = v.decoration_rects(&model);
        assert_eq!(rects.len(), 3, "one rect for 1..2, two for the range across a line break");
        assert!(rects.iter().all(|(_, kind)| *kind == 3));
        assert_eq!(rects[0].0.width, 10.0);
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

#[cfg(test)]
mod guide_tests {
    use super::*;
    use rustaco_core::Document;

    #[test]
    fn guides_follow_blocks_and_mark_the_active_one() {
        let text = "fn a() {\n    x\n\n    y\n        z\n}\nq";
        let doc = Document::new(text);
        let mut v = View::new(Metrics { char_width: 10.0, line_height: 20.0 }, 400.0, 400.0);
        v.layout(doc.model(), doc.selections(), &Plain);
        let guides = v.indent_guides(doc.model(), 4);
        let level1: Vec<&Guide> = guides.iter().filter(|g| g.level == 1).collect();
        assert_eq!(level1.len(), 1, "the blank line inside the block keeps the guide whole");
        assert_eq!((level1[0].top, level1[0].bottom), (20.0, 100.0));
        let level2: Vec<&Guide> = guides.iter().filter(|g| g.level == 2).collect();
        assert_eq!(level2.len(), 1);
        assert!(level2[0].active, "the cursor on line 4 is in the level-2 block");
        assert!(!level1[0].active);
        assert_eq!(v.whitespace_marks(doc.model()).len(), 2 + 4 + 4 + 8, "two spaces in the fn line, then the indents");
    }
}

#[cfg(test)]
mod fold_tests {
    use super::*;
    use rustaco_core::{Document, Edit, Selection};

    const TEXT: &str = "fn a() {\n    one\n    two\n}\nfn b() {\n    three\n}\nend";

    fn setup() -> (Document, View) {
        let doc = Document::new(TEXT);
        let mut v = View::new(Metrics { char_width: 10.0, line_height: 20.0 }, 400.0, 400.0);
        v.sync(doc.model());
        v.set_foldable(indent_fold_ranges(doc.rope(), 4));
        (doc, v)
    }

    #[test]
    fn folded_lines_take_no_space() {
        let (doc, mut v) = setup();
        assert!(v.fold(doc.model(), 2), "line 2 lies in the region headed by line 0");
        let frame = v.layout(doc.model(), doc.selections(), &Plain);
        let lines: Vec<usize> = frame.lines.iter().map(|l| l.line).collect();
        assert_eq!(lines, vec![0, 3, 4, 5, 6, 7]);
        assert_eq!(frame.lines[1].y, 20.0);
        assert_eq!(
            frame.fold_markers.iter().map(|m| (m.line, m.folded)).collect::<Vec<_>>(),
            vec![(0, true), (4, false)]
        );
        assert_eq!(frame.fold_ellipses.len(), 1);
        assert_eq!(v.line_at_y(doc.rope(), 25.0), 3);
        assert_eq!(v.vertical_target(doc.rope(), 0, None, 1).0, doc.rope().line_to_char(3));
        assert_eq!(v.vertical_target(doc.rope(), doc.rope().line_to_char(3), None, -1).0, 0);
    }

    #[test]
    fn folds_follow_edits_and_open_for_carets() {
        let (mut doc, mut v) = setup();
        v.fold(doc.model(), 4);
        doc.apply_edits(vec![Edit::insert(0, "// top\n")]);
        v.sync(doc.model());
        assert!(v.is_folded(doc.rope(), 5), "the fold moved down with its header");
        assert!(v.is_hidden(6) && !v.is_hidden(7));
        let inside = doc.rope().line_to_char(6) + 2;
        doc.set_selections(rustaco_core::Selections::single(Selection::cursor(inside)));
        assert!(v.reveal_positions(doc.model(), [inside]));
        assert!(!v.is_hidden(6));
    }

    #[test]
    fn heights_match_a_fresh_view_after_random_edits() {
        let mut seed = 0x9e37_79b9_u64;
        let mut rand = move |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n.max(1) as u64) as usize
        };
        let text: String = (0..300).map(|i| format!("{}line {i}\n", "    ".repeat(i % 4))).collect();
        let mut doc = Document::new(&text);
        let mut v = View::new(Metrics { char_width: 10.0, line_height: 20.0 }, 400.0, 400.0);
        v.sync(doc.model());
        for step in 0..200 {
            if step % 10 == 0 {
                v.set_foldable(indent_fold_ranges(doc.rope(), 4));
                let line = rand(doc.rope().len_lines());
                v.fold(doc.model(), line);
            }
            let len = doc.rope().len_chars();
            let a = rand(len + 1);
            let b = (a + rand(40)).min(len);
            let insert = ["", "x", "\n", "\n    y\n", "{\n\n}"][rand(5)];
            doc.apply_edits(vec![Edit::new(a, b, insert)]);
            v.sync(doc.model());
            let lines = doc.rope().len_lines();
            assert_eq!(v.heights.len(), lines);
            // Shown tops are the sum of the heights of the shown lines above.
            let mut top = 0.0f64;
            for l in 0..lines {
                assert_eq!(v.line_top(l), top, "top of line {l} at step {step}");
                if !v.is_hidden(l) {
                    top += 20.0;
                }
            }
        }
    }
}

#[cfg(test)]
mod wrap_tests {
    use super::*;
    use rustaco_core::{Document, Edit, Selection};

    /// 20 cells of text: the gutter takes 7 cells and the scrollbar 14 px.
    fn wrapped_view() -> View {
        let mut v = View::new(Metrics { char_width: 10.0, line_height: 20.0 }, 284.0, 200.0);
        v.set_wrap(true);
        v
    }

    #[test]
    fn long_lines_take_several_rows() {
        let doc = Document::new("short\nthe quick brown fox jumps over the lazy dog\nend");
        let mut v = wrapped_view();
        let frame = v.layout(doc.model(), doc.selections(), &Plain);
        let rows: Vec<(usize, usize, f32)> = frame.lines.iter().map(|l| (l.line, l.row, l.y)).collect();
        assert_eq!(rows, vec![(0, 0, 0.0), (1, 0, 20.0), (1, 1, 40.0), (1, 2, 60.0), (2, 0, 80.0)]);
        assert_eq!(frame.lines[2].runs[0].text, "jumps over the lazy ");
        assert_eq!(v.line_top(2), 80.0);
        assert_eq!(v.line_at_y(doc.rope(), 70.0), 1);
    }

    #[test]
    fn carets_hit_tests_and_moves_follow_rows() {
        let text = "the quick brown fox jumps over the lazy dog";
        let mut doc = Document::new(text);
        let mut v = wrapped_view();
        let jumps = text.find("jumps").unwrap();
        doc.set_selections(rustaco_core::Selections::single(Selection::cursor(jumps + 2)));
        let frame = v.layout(doc.model(), doc.selections(), &Plain);
        let g = frame.gutter_width;
        assert_eq!((frame.carets[0].x, frame.carets[0].y), (g + 20.0, 20.0));
        assert_eq!(v.hit_test(doc.rope(), g + 21.0, 25.0), jumps + 2);
        assert_eq!(
            v.hit_test(doc.rope(), g + 500.0, 5.0),
            jumps - 1,
            "past a wrapping row's end, before its last char"
        );
        let (up, goal) = v.vertical_target(doc.rope(), jumps + 2, None, -1);
        assert_eq!((up, goal), (2, Some(2)));
        assert_eq!(v.vertical_target(doc.rope(), up, goal, 2).0, text.find("dog").unwrap() + 2);
        let rects = v.char_range_rects(doc.model(), 4..jumps + 2);
        assert_eq!(rects.len(), 2, "one rectangle per row");
    }

    #[test]
    fn every_column_round_trips_through_rows() {
        let text =
            "\tfn wrapped(aaa: u32, bbbbbbbbbbbbbbbbbbbbbbbbbbbbb: \u{4E2D}\u{4E2D}, c: [u8; 4]) -> bool { true }";
        let doc = Document::new(text);
        let mut v = wrapped_view();
        v.sync(doc.model());
        let rope = doc.rope();
        let rows = v.line_rows(rope, 0);
        assert!(rows.count() > 3);
        for column in 0..line_len(rope, 0) {
            let (row, x) = v.locate(rope, &rows, 0, column);
            assert_eq!(v.column_in_row(rope, &rows, 0, row, x as f32), column, "column {column}");
        }
    }

    #[test]
    fn heights_follow_edits_while_wrapping() {
        let mut seed = 0x51ed_270b_u64;
        let mut rand = move |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n.max(1) as u64) as usize
        };
        let text: String = (0..200).map(|i| format!("{} {}\n", "word ".repeat(i % 9), i)).collect();
        let mut doc = Document::new(&text);
        let mut v = wrapped_view();
        v.sync(doc.model());
        for step in 0..200 {
            let len = doc.rope().len_chars();
            let a = rand(len + 1);
            let b = (a + rand(30)).min(len);
            let insert = ["", "x", "\n", " long words here and there ", "\nnew line\n"][rand(5)];
            doc.apply_edits(vec![Edit::new(a, b, insert)]);
            v.sync(doc.model());
            let mut fresh = wrapped_view();
            fresh.sync(doc.model());
            for l in 0..doc.rope().len_lines() {
                assert_eq!(v.heights.height(l), fresh.heights.height(l), "line {l} at step {step}");
            }
        }
    }
}
