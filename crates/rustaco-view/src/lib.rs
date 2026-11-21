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
//! Vertical geometry goes through [`View::line_top`] and [`View::line_at_y`], backed by a
//! [`HeightMap`]: folded lines have height 0, and wrapped lines and blocks between lines
//! will take more than one line height.

mod heights;

pub use heights::HeightMap;

use std::ops::Range;

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
    /// Fold markers for the gutter, on shown lines that head a foldable or folded region.
    pub fold_markers: Vec<FoldMarker>,
    /// Placeholders drawn after the text of folded header lines.
    pub fold_ellipses: Vec<Rect>,
}

/// A collapsed region: the lines after the header line down to the line holding `end` are
/// hidden. Both ends are char offsets (the end of the header line and the end of the last
/// hidden line), so the fold follows edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Fold {
    start: usize,
    end: usize,
}

#[derive(Clone, Debug)]
pub struct View {
    pub metrics: Metrics,
    pub width: f32,
    pub height: f32,
    scroll_top: f64,
    scroll_left: f32,
    /// Widest line laid out so far, in cells. Grows as the user scrolls; full scans are avoided.
    widest_cells: usize,
    /// Tab size of the model at the last layout; used by hit testing and reveal.
    tab_size: usize,
    heights: HeightMap,
    /// Model version `heights`, `folds` and `foldable` describe.
    synced: Option<u64>,
    /// Line height `heights` was built with.
    synced_line_height: f32,
    folds: Vec<Fold>,
    /// Union of the lines hidden by `folds`, sorted and disjoint.
    hidden: Vec<Range<usize>>,
    /// Regions that can be folded, as (header line, last line), sorted by header.
    foldable: Vec<(usize, usize)>,
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
            heights: HeightMap::new(1, metrics.line_height),
            synced: None,
            synced_line_height: metrics.line_height,
            folds: Vec::new(),
            hidden: Vec::new(),
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

    pub fn gutter_width(&self, rope: &Rope) -> f32 {
        let digits = rope.len_lines().to_string().len().max(2);
        (digits + 3) as f32 * self.metrics.char_width + FOLD_MARKER_CELLS * self.metrics.char_width
    }

    // ---- model sync ----

    /// Brings line heights and folds up to date with `model`. Layout calls it; call it too
    /// before hit testing or revealing right after an edit.
    pub fn sync(&mut self, model: &TextModel) {
        let rope = model.rope();
        let lh = self.metrics.line_height;
        let version = model.version();
        if self.synced == Some(version) && self.synced_line_height == lh && self.heights.len() == rope.len_lines() {
            return;
        }
        let mut incremental = false;
        if let Some(v) = self.synced.filter(|_| self.synced_line_height == lh)
            && let (Ok(events), Ok(changes)) = (model.events_since(v), model.changes_since(v))
        {
            for r in std::mem::take(&mut self.hidden) {
                self.heights.fill(r, lh);
            }
            for e in events {
                let a = e.start_point.row;
                let old = e.old_end_point.row - a;
                let new = e.new_end_point.row - a;
                if old != 0 || new != 0 {
                    self.heights.splice(a + 1, old, std::iter::repeat_n(lh, new));
                    shift_foldable(&mut self.foldable, a, old, new);
                }
            }
            for c in changes {
                for f in &mut self.folds {
                    f.start = c.map(f.start, Assoc::After);
                    f.end = c.map(f.end, Assoc::After);
                }
            }
            incremental = self.heights.len() == rope.len_lines();
        }
        if !incremental {
            self.heights = HeightMap::new(rope.len_lines(), lh);
            self.hidden.clear();
            if self.synced.is_some() && self.synced_line_height == lh {
                // The change log no longer reaches back; positions are unknown.
                self.folds.clear();
                self.foldable.clear();
            }
        }
        self.synced = Some(version);
        self.synced_line_height = lh;
        self.apply_folds(rope);
    }

    /// Recomputes `hidden` from `folds` and zeroes those lines' heights.
    fn apply_folds(&mut self, rope: &Rope) {
        let lh = self.metrics.line_height;
        for r in std::mem::take(&mut self.hidden) {
            self.heights.fill(r, lh);
        }
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
        ranges.sort_by_key(|r| r.start);
        for r in ranges {
            match self.hidden.last_mut() {
                Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
                _ => self.hidden.push(r),
            }
        }
        for r in self.hidden.clone() {
            self.heights.fill(r, 0.0);
        }
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
        self.hidden_range(line).is_some()
    }

    fn hidden_range(&self, line: usize) -> Option<Range<usize>> {
        let i = self.hidden.partition_point(|r| r.end <= line);
        self.hidden.get(i).filter(|r| r.start <= line).cloned()
    }

    /// `line`, or the first shown line after the hidden run holding it.
    fn next_shown(&self, line: usize) -> usize {
        self.hidden_range(line).map_or(line, |r| r.end)
    }

    /// `line`, or the header above the hidden run holding it.
    fn prev_shown(&self, line: usize) -> usize {
        self.hidden_range(line).map_or(line, |r| r.start.saturating_sub(1))
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

    // ---- vertical geometry ----

    /// Document y of the top of `line`. A hidden line sits at the top of the next shown line.
    pub fn line_top(&self, line: usize) -> f64 {
        if line < self.heights.len() {
            self.heights.top(line)
        } else {
            // Not synced yet: extrapolate past the known lines.
            self.heights.total() + (line - self.heights.len()) as f64 * self.metrics.line_height as f64
        }
    }

    pub fn line_height(&self, line: usize) -> f32 {
        if line < self.heights.len() { self.heights.height(line) } else { self.metrics.line_height }
    }

    /// The shown line at document y, clamped into the document.
    pub fn line_at_y(&self, rope: &Rope, y: f64) -> usize {
        let total = self.heights.total();
        let line = if y >= total && self.heights.len() < rope.len_lines() {
            // Not synced yet: extrapolate past the known lines.
            self.heights.len() + ((y - total) / self.metrics.line_height as f64) as usize
        } else {
            self.heights.line_at(y)
        };
        line.min(rope.len_lines() - 1)
    }

    /// Height of the scrollable content. The last line can scroll up to the top of the viewport.
    pub fn content_height(&self, rope: &Rope) -> f64 {
        let last = self.prev_shown(rope.len_lines() - 1);
        self.line_top(last) + self.height as f64
    }

    pub fn content_width(&self, rope: &Rope) -> f32 {
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

    /// The caret target `lines` shown lines above (negative) or below `head`, keeping
    /// `goal` (a char column) when given: (offset, goal column).
    pub fn vertical_target(
        &self,
        rope: &Rope,
        head: usize,
        goal: Option<usize>,
        lines: isize,
    ) -> (usize, Option<usize>) {
        let p = char_to_position(rope, head);
        let goal = goal.unwrap_or(p.column);
        let mut line = p.line;
        for _ in 0..lines.unsigned_abs() {
            if lines < 0 {
                if line == 0 {
                    return (0, None);
                }
                line = self.prev_shown(line - 1);
            } else {
                let next = self.next_shown(line + 1);
                if next >= rope.len_lines() {
                    return (rope.len_chars(), None);
                }
                line = next;
            }
        }
        (rope.line_to_char(line) + goal.min(line_len(rope, line)), Some(goal))
    }

    // ---- scrolling ----

    pub fn scroll_to(&mut self, rope: &Rope, top: f64, left: f32) {
        let max_top = (self.content_height(rope) - self.height as f64).max(0.0);
        let max_left = (self.content_width(rope) - self.width).max(0.0);
        self.scroll_top = top.clamp(0.0, max_top);
        self.scroll_left = left.clamp(0.0, max_left);
    }

    pub fn scroll_by(&mut self, rope: &Rope, dx: f32, dy: f64) {
        self.scroll_to(rope, self.scroll_top + dy, self.scroll_left + dx);
    }

    /// Scrolls the minimum distance that makes the char at `pos` fully visible.
    pub fn reveal(&mut self, rope: &Rope, pos: usize) {
        let p = char_to_position(rope, pos);
        let line = self.prev_shown(p.line);
        let top = self.line_top(line);
        let bottom = top + self.line_height(line) as f64;
        let mut scroll_top = self.scroll_top;
        if top < scroll_top {
            scroll_top = top;
        } else if bottom > scroll_top + self.height as f64 {
            scroll_top = bottom - self.height as f64;
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
        let (line, cells) = self.line_and_cells_at(rope, x, y);
        rope.line_to_char(line) + column_at_cells(rope, line, cells, self.tab_size)
    }

    /// The line and visual column (fractional cells, may lie past the line's end) at a point
    /// in viewport coordinates. Column selection uses cells, so it stays rectangular across
    /// lines with tabs.
    pub fn line_and_cells_at(&self, rope: &Rope, x: f32, y: f32) -> (usize, f32) {
        let line = self.line_at_y(rope, y as f64 + self.scroll_top);
        let cells = ((x - self.gutter_width(rope) + self.scroll_left) / self.metrics.char_width).max(0.0);
        (line, cells)
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
        self.tab_size = model.options.tab_size.max(1);
        let tab = self.tab_size;
        let rope = model.rope();
        let Metrics { char_width: cw, .. } = self.metrics;
        let gutter = self.gutter_width(rope);
        let visible = self.visible_lines(rope);
        let first_cell = (self.scroll_left / cw).floor() as usize;
        let last_cell = ((self.scroll_left + self.width - gutter) / cw).ceil() as usize + 1;
        let x_of = |cells: usize| gutter + cells as f32 * cw - self.scroll_left;

        let mut frame = Frame { gutter_width: gutter, ..Frame::default() };
        let mut widest = self.widest_cells;
        let len = rope.len_chars();
        let folded_headers: Vec<usize> = self.folds.iter().map(|f| rope.char_to_line(f.start.min(len))).collect();
        let mut foldable_at = self.foldable.partition_point(|&(s, _)| s < visible.start);

        for line in self.shown(visible.clone()) {
            let spans = styler.spans(line);
            let (runs, cells) = layout_line(rope, line, &spans, first_cell..last_cell, tab, &x_of);
            widest = widest.max(cells);
            let y = self.y_of(line);
            let height = self.line_height(line);
            while foldable_at < self.foldable.len() && self.foldable[foldable_at].0 < line {
                foldable_at += 1;
            }
            let folded = folded_headers.contains(&line);
            if folded || self.foldable.get(foldable_at).is_some_and(|&(s, _)| s == line) {
                frame.fold_markers.push(FoldMarker { line, y, height, folded });
            }
            if folded {
                let end = x_of(visual_column(rope, line, line_len(rope, line), tab));
                frame.fold_ellipses.push(Rect { x: end + cw, y, width: 3.0 * cw, height });
            }
            frame.lines.push(FrameLine { line, y, height, runs });
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
            let head = char_to_position(rope, sel.head);
            if visible.contains(&head.line) && !self.is_hidden(head.line) {
                frame.carets.push(Rect {
                    x: x_of(visual_column(rope, head.line, head.column, tab)),
                    y: self.y_of(head.line),
                    width: 2.0,
                    height: self.line_height(head.line),
                });
            }
            if sel.is_empty() {
                continue;
            }
            frame.selections.extend(self.range_rects(rope, sel.start()..sel.end(), gutter));
        }
        self.widest_cells = widest;
        frame
    }

    /// Rectangles covering `range` on the visible lines; a line break inside the range adds
    /// half a cell at the end of its line, as for selections.
    fn range_rects(&self, rope: &Rope, range: std::ops::Range<usize>, gutter: f32) -> Vec<Rect> {
        let tab = self.tab_size;
        let cw = self.metrics.char_width;
        let visible = self.visible_lines(rope);
        let x_of = |cells: usize| gutter + cells as f32 * cw - self.scroll_left;
        let (start, end) = (char_to_position(rope, range.start), char_to_position(rope, range.end));
        let mut out = Vec::new();
        let lines = start.line.max(visible.start)..(end.line + 1).min(visible.end);
        for line in self.shown(lines) {
            let from = if line == start.line { start.column } else { 0 };
            let (to, newline) = if line == end.line { (end.column, false) } else { (line_len(rope, line), true) };
            let x0 = x_of(visual_column(rope, line, from, tab));
            let x1 = x_of(visual_column(rope, line, to, tab)) + if newline { cw * 0.5 } else { 0.0 };
            out.push(Rect { x: x0, y: self.y_of(line), width: x1 - x0, height: self.line_height(line) });
        }
        out
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
    /// (x of the cell start, y of the line top, width in cells, is a tab).
    pub fn whitespace_marks(&self, model: &TextModel) -> Vec<(f32, f32, usize, bool)> {
        let rope = model.rope();
        let tab = self.tab_size.max(1);
        let cw = self.metrics.char_width;
        let gutter = self.gutter_width(rope);
        let mut out = Vec::new();
        for line in self.shown(self.visible_lines(rope)) {
            let y = self.y_of(line);
            let mut cells = 0;
            for c in rope.line(line).chars().take(line_len(rope, line).min(MAX_MARKED_COLUMNS)) {
                let w = cell_width(c, cells, tab);
                if c == ' ' || c == '\t' {
                    out.push((gutter + cells as f32 * cw - self.scroll_left, y, w, c == '\t'));
                }
                cells += w;
            }
        }
        out
    }
}

/// Width of the fold marker column at the right of the gutter, in cells.
const FOLD_MARKER_CELLS: f32 = 2.0;

/// A fold marker in the gutter: the line heads a region that can be folded, or is folded.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FoldMarker {
    pub line: usize,
    pub y: f32,
    pub height: f32,
    pub folded: bool,
}

/// Moves foldable regions past an edit that replaced lines `a..=a + old` with `a..=a + new`.
/// Regions headed inside the edit are dropped until the provider recomputes them.
fn shift_foldable(foldable: &mut Vec<(usize, usize)>, a: usize, old: usize, new: usize) {
    let shift = |l: usize| if l > a + old { l + new - old } else { l.min(a + new) };
    foldable.retain_mut(|(s, e)| {
        if *s > a && *s <= a + old {
            return false;
        }
        *s = shift(*s);
        *e = shift(*e);
        *e > *s
    });
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

/// Foldable regions from indentation: a non-blank line followed by more deeply indented
/// lines heads a region ending at the last of them (trailing blank lines excluded). Used
/// when no syntax tree gives better ranges, and for languages like Python where indentation
/// is the structure.
pub fn indent_fold_ranges(rope: &Rope, tab: usize) -> Vec<(usize, usize)> {
    let tab = tab.max(1);
    let total = rope.len_lines();
    let mut out = Vec::new();
    // Open headers as (line, indent cells); closed when a line at or below their indent appears.
    let mut open: Vec<(usize, usize)> = Vec::new();
    let mut last_nonblank = 0;
    for line in 0..total {
        let Some(cells) = indent_cells(rope, line, tab) else { continue };
        while let Some(&(h, ind)) = open.last() {
            if cells > ind {
                break;
            }
            open.pop();
            if last_nonblank > h {
                out.push((h, last_nonblank));
            }
        }
        open.push((line, cells));
        last_nonblank = line;
    }
    while let Some((h, _)) = open.pop() {
        if last_nonblank > h {
            out.push((h, last_nonblank));
        }
    }
    out.sort_unstable();
    out
}

fn indent_cells(rope: &Rope, line: usize, tab: usize) -> Option<usize> {
    let mut cells = 0;
    for c in rope.line(line).chars() {
        match c {
            ' ' => cells += 1,
            '\t' => cells += tab - cells % tab,
            '\n' | '\r' => return None,
            _ => return Some(cells),
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
    fn indentation_gives_fold_ranges() {
        let rope = Rope::from_str("a\n  b\n\n  c\n    d\ne\n  f\n");
        assert_eq!(indent_fold_ranges(&rope, 4), vec![(0, 4), (3, 4), (5, 6)]);
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
            let mut fresh = HeightMap::new(lines, 20.0);
            for r in &v.hidden {
                fresh.fill(r.clone(), 0.0);
            }
            for l in 0..lines {
                assert_eq!(v.heights.height(l), fresh.height(l), "line {l} at step {step}");
            }
        }
    }
}
