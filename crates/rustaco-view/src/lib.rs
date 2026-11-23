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
pub mod markdown;
mod wrap;

pub use folding::{FoldMarker, indent_fold_ranges};
pub use heights::HeightMap;
pub use wrap::{LineRows, wrap_line, wrap_slice};

use std::ops::Range;

use folding::{Fold, Hidden, shift_foldable, shift_lines};
use rustaco_core::{Assoc, Rope, Selections, TextModel, char_to_position, line_end, line_len};
use unicode_width::UnicodeWidthChar;

/// Font measurements supplied by the frontend. `#[non_exhaustive]`: measuring proportional
/// text may add to it later, so build it with [`Metrics::new`].
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct Metrics {
    /// Advance of one monospace cell.
    pub char_width: f32,
    pub line_height: f32,
}

impl Metrics {
    pub fn new(char_width: f32, line_height: f32) -> Self {
        Self { char_width, line_height }
    }
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

/// One laid-out frame. Grows with features (minimap, sticky lines, zones): read its fields,
/// do not build it.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
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
    /// The minimap, when it is on.
    pub minimap: Option<MinimapFrame>,
    /// Headers of the scopes the top of the viewport is inside, pinned over the text, the
    /// outermost first. Drawn last, over everything else in the text area.
    pub sticky: Vec<FrameLine>,
    /// Visible view zones: the id the frontend gave each and the space it occupies.
    pub zones: Vec<(u32, Rect)>,
}

/// A view zone: empty space of a given height after a line (or above the first), for the
/// frontend to fill (diff filler, inline widgets, code lens). The anchor is a char offset
/// at the end of the line, so the zone follows edits.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Zone {
    id: u32,
    anchor: Option<usize>,
    /// The line the zone's height is counted in, as of the last sync.
    line: Option<usize>,
    height: f32,
}

/// The minimap: the document at a small scale beside the text, each char a block of
/// colour, with a slider over the part the viewport shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MinimapFrame {
    /// Left edge and width of the minimap column.
    pub x: f32,
    pub width: f32,
    /// Height of one row of text.
    pub row_height: f32,
    pub blocks: Vec<MinimapBlock>,
    pub slider: Rect,
}

/// A run of non-blank chars with one style, in the minimap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MinimapBlock {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub style: u32,
}

/// Minimap scale: pixels per text row and per cell.
const MINIMAP_ROW: f32 = 2.0;
const MINIMAP_CELL: f32 = 1.0;

/// Overview ruler marks with what they were computed for: decoration generation, view
/// geometry and track height.
type OverviewCache = ((u64, u64, u32), Vec<(f32, f32, u32)>);

/// Style id of inlay hint runs; frontends draw them muted.
pub const INLAY_STYLE: u32 = u32::MAX - 1;

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
    /// Width of the minimap column at the right; 0 when it is off.
    minimap_width: f32,
    /// Most scope headers sticky scroll pins; 0 turns it off.
    sticky_max: usize,
    zones: Vec<Zone>,
    /// Inlay hints: text shown before the char at an offset, sorted by offset.
    inlays: Vec<(usize, String)>,
    /// Height of the zones above the first line.
    top_padding: f64,
    /// Bumped whenever line positions may have changed (sync, folds, zones).
    geometry: u64,
    /// Overview ruler marks for (decoration generation, geometry, track height).
    overview_cache: std::cell::RefCell<Option<OverviewCache>>,
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
            minimap_width: 0.0,
            sticky_max: 0,
            zones: Vec::new(),
            inlays: Vec::new(),
            top_padding: 0.0,
            geometry: 0,
            overview_cache: std::cell::RefCell::new(None),
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

    /// Shows a minimap `width` pixels wide at the right (0 hides it). The text area
    /// narrows by the same amount.
    pub fn set_minimap(&mut self, width: f32) {
        self.minimap_width = width.max(0.0);
    }

    pub fn minimap_width(&self) -> f32 {
        self.minimap_width
    }

    /// Pins up to `max` headers of the scopes (foldable regions) the viewport's top is
    /// inside; 0 turns sticky scroll off.
    pub fn set_sticky_scroll(&mut self, max: usize) {
        self.sticky_max = max;
    }

    /// The scope headers to pin, outermost first, each with its viewport y. The innermost
    /// one slides up as the end of its scope scrolls past, so the next line can take its
    /// place.
    pub fn sticky_lines(&self, rope: &Rope) -> Vec<(usize, f32)> {
        if self.sticky_max == 0 || self.foldable.is_empty() {
            return Vec::new();
        }
        let lh = self.metrics.line_height;
        let mut stack: Vec<(usize, usize)> = Vec::new();
        loop {
            // The line just under the pinned headers.
            let probe = self.line_at_y(rope, self.scroll_top + (stack.len() as f32 * lh) as f64);
            let next = self
                .foldable
                .iter()
                .take_while(|&&(h, _)| h < probe)
                .filter(|&&(h, e)| e >= probe && !self.is_hidden(h) && stack.last().is_none_or(|&(sh, _)| h > sh))
                .min_by_key(|&&(h, _)| h)
                .copied();
            match next {
                Some(region) if stack.len() < self.sticky_max => stack.push(region),
                _ => break,
            }
        }
        let mut out = Vec::with_capacity(stack.len());
        for (i, &(header, end)) in stack.iter().enumerate() {
            let natural = i as f32 * lh;
            // The scope's last line must stay above its header's pinned row.
            let end_bottom = (self.line_top(end) - self.scroll_top) as f32 + self.line_height(end);
            out.push((header, natural.min(end_bottom - lh)));
        }
        out
    }

    /// Width of the text area: the viewport less the gutter and the minimap.
    fn text_width(&self, rope: &Rope) -> f32 {
        self.width - self.gutter_width(rope) - self.minimap_width
    }

    pub fn gutter_width(&self, rope: &Rope) -> f32 {
        let digits = rope.len_lines().to_string().len().max(2);
        (digits as f32 + 3.0 + FOLD_MARKER_CELLS) * self.metrics.char_width
    }

    /// Wrapping width in cells, when wrapping.
    fn wrap_cells(&self, rope: &Rope) -> Option<usize> {
        self.wrap.then(|| {
            let text = self.text_width(rope) - if self.minimap_width > 0.0 { 0.0 } else { SCROLLBAR_WIDTH };
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
            self.remove_zone_heights();
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
                // Text typed at the end of the line goes before the zone; a new line after it.
                for z in &mut self.zones {
                    z.anchor = z.anchor.map(|a| c.map(a, Assoc::Before));
                }
                for (offset, _) in &mut self.inlays {
                    *offset = c.map(*offset, Assoc::Before);
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
            for z in &mut self.zones {
                z.line = None;
            }
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
        self.add_zone_heights(rope);
        self.apply_folds(rope);
    }

    /// Takes the zones' heights out of the lines they are counted in.
    fn remove_zone_heights(&mut self) {
        for z in &mut self.zones {
            if let Some(l) = z.line.take()
                && l < self.heights.len()
            {
                let h = self.heights.height(l);
                self.heights.set(l, (h - z.height).max(0.0));
            }
        }
        self.top_padding = 0.0;
    }

    /// Adds the zones' heights to the lines they now follow.
    fn add_zone_heights(&mut self, rope: &Rope) {
        let len = rope.len_chars();
        self.top_padding = 0.0;
        for z in &mut self.zones {
            match z.anchor {
                None => self.top_padding += z.height as f64,
                Some(a) => {
                    let l = rope.char_to_line(a.min(len));
                    let h = self.heights.height(l);
                    self.heights.set(l, h + z.height);
                    z.line = Some(l);
                }
            }
        }
    }

    /// Adds a view zone after `after` (None: above the first line), `height` pixels tall.
    /// Other zones keep the positions edits gave them.
    pub fn add_zone(&mut self, model: &TextModel, id: u32, after: Option<usize>, height: f32) {
        self.sync(model);
        self.remove_zone_heights();
        let rope = model.rope();
        let anchor = after.map(|l| line_end(rope, l.min(rope.len_lines() - 1)));
        self.zones.push(Zone { id, anchor, line: None, height: height.max(0.0) });
        self.add_zone_heights(rope);
        self.apply_folds(rope);
    }

    /// Removes the view zone `id`. Returns false when there is none.
    pub fn remove_zone(&mut self, model: &TextModel, id: u32) -> bool {
        self.sync(model);
        self.remove_zone_heights();
        let before = self.zones.len();
        self.zones.retain(|z| z.id != id);
        let rope = model.rope();
        self.add_zone_heights(rope);
        self.apply_folds(rope);
        self.zones.len() != before
    }

    /// Replaces the view zones: (id, line the zone follows or None for above the first line,
    /// height in pixels).
    pub fn set_zones(&mut self, model: &TextModel, zones: impl IntoIterator<Item = (u32, Option<usize>, f32)>) {
        self.sync(model);
        self.remove_zone_heights();
        let rope = model.rope();
        let last = rope.len_lines() - 1;
        self.zones = zones
            .into_iter()
            .map(|(id, after, height)| Zone {
                id,
                anchor: after.map(|l| line_end(rope, l.min(last))),
                line: None,
                height: height.max(0.0),
            })
            .collect();
        self.add_zone_heights(rope);
        self.apply_folds(rope);
    }
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
        self.geometry += 1;
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
            self.top_padding + self.hidden.top(&self.heights, line)
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
            self.hidden.line_at(&self.heights, y - self.top_padding)
        };
        line.min(rope.len_lines() - 1)
    }

    /// The line and row at document y.
    fn row_at_y(&self, rope: &Rope, y: f64) -> (usize, usize) {
        let line = self.line_at_y(rope, y);
        let row = ((y - self.line_top(line)) / self.metrics.line_height as f64).max(0.0) as usize;
        let rows = self.line_rows(rope, line).count();
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

    /// Shows inlay hints: `label` drawn before the char at each offset, pushing the text
    /// after it right. A caret at that offset stays before the hint.
    pub fn set_inlays(&mut self, model: &TextModel, mut hints: Vec<(usize, String)>) {
        self.sync(model);
        hints.retain(|(_, label)| !label.is_empty());
        hints.sort_by_key(|h| h.0);
        self.inlays = hints;
    }

    /// Inlay hints on `line`: (column, width in cells, label), by column.
    fn inlays_on(&self, rope: &Rope, line: usize) -> Vec<(usize, usize, &str)> {
        if self.inlays.is_empty() {
            return Vec::new();
        }
        let start = rope.line_to_char(line);
        let end = start + line_len(rope, line);
        let from = self.inlays.partition_point(|h| h.0 < start);
        self.inlays[from..]
            .iter()
            .take_while(|h| h.0 <= end)
            .map(|(o, label)| (o - start, label.chars().map(|c| c.width().unwrap_or(1)).sum(), label.as_str()))
            .collect()
    }

    /// Row and x (cells from the text area's left edge, before scrolling) of char `column`
    /// on `line`. Inlay hints before the column on its row count; one at the column does not.
    fn locate(&self, rope: &Rope, rows: &LineRows, line: usize, column: usize) -> (usize, usize) {
        let row = rows.row_of(column);
        let tab = self.tab_size;
        let start_cells = if row == 0 { 0 } else { visual_column(rope, line, rows.starts[row], tab) };
        let hints: usize =
            self.inlays_on(rope, line).iter().filter(|h| h.0 >= rows.starts[row] && h.0 < column).map(|h| h.1).sum();
        (row, rows.indent_of(row) + visual_column(rope, line, column, tab) - start_cells + hints)
    }

    /// Char column on `row` of `line` nearest to x `cells` (as [`View::locate`] measures).
    /// Past the end of a row that wraps, the column stays before the row's last char.
    fn column_in_row(&self, rope: &Rope, rows: &LineRows, line: usize, row: usize, cells: f32) -> usize {
        if !self.inlays_on(rope, line).is_empty() {
            // Hints shift the text unevenly: pick the nearest caret position by measuring.
            let start = rows.starts[row];
            let end = rows.starts.get(row + 1).map_or(line_len(rope, line), |&n| n - 1);
            let mut best = (start, f32::MAX);
            for col in start..=end {
                let d = (self.locate(rope, rows, line, col).1 as f32 - cells).abs();
                if d < best.1 {
                    best = (col, d);
                }
            }
            return best.0;
        }
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
        let max_left = (self.content_width(rope) - self.width + self.minimap_width).max(0.0);
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
        let text_width = self.text_width(rope);
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
            let inlays = self.inlays_on(rope, line);
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
                let (runs, cells) = layout_line(rope, line, cols, &spans, window, tab, &x_of, &inlays);
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
        frame.minimap = self.minimap(rope, styler);
        frame.zones = self.zone_rects(rope, visible.clone());
        for (line, y) in self.sticky_lines(rope) {
            let spans = styler.spans(line);
            let x_of = |cells: usize| gutter + cells as f32 * cw - self.scroll_left;
            let len = line_len(rope, line);
            let (runs, _) = layout_line(rope, line, 0..len, &spans, first_cell..last_cell, tab, &x_of, &[]);
            frame.sticky.push(FrameLine { line, row: 0, y, height: lh, runs });
        }
        frame
    }

    /// Viewport rectangles of the zones on the visible lines.
    fn zone_rects(&self, rope: &Rope, visible: Range<usize>) -> Vec<(u32, Rect)> {
        let gutter = self.gutter_width(rope);
        let width = self.width - gutter - self.minimap_width;
        let lh = self.metrics.line_height;
        let mut below: Vec<(usize, f32)> = Vec::new(); // (line, zone height already placed)
        let mut top = -(self.scroll_top as f32);
        let mut out = Vec::new();
        for z in &self.zones {
            match z.line {
                None if z.anchor.is_none() => {
                    out.push((z.id, Rect { x: gutter, y: top, width, height: z.height }));
                    top += z.height;
                }
                Some(l) if visible.contains(&l) && !self.is_hidden(l) => {
                    let placed = match below.iter_mut().find(|(line, _)| *line == l) {
                        Some((_, h)) => {
                            let at = *h;
                            *h += z.height;
                            at
                        }
                        None => {
                            below.push((l, z.height));
                            0.0
                        }
                    };
                    let rows = self.line_rows(rope, l).count() as f32;
                    let y = self.y_of(l) + rows * lh + placed;
                    out.push((z.id, Rect { x: gutter, y, width, height: z.height }));
                }
                _ => {}
            }
        }
        out.sort_by(|a, b| a.1.y.total_cmp(&b.1.y));
        out
    }

    /// Document height (with the scroll room past the last line) and the scale and
    /// offset of the minimap: minimap y = (document y - offset) * scale.
    fn minimap_geometry(&self, rope: &Rope) -> (f64, f64, f64) {
        let lh = self.metrics.line_height as f64;
        let scale = MINIMAP_ROW as f64 / lh;
        let max_scroll = (self.content_height(rope) - self.height as f64).max(0.0);
        // Scrolling goes past the last line by a viewport, and so does the minimap.
        let document = max_scroll + self.height as f64;
        let fits = self.height as f64 / scale;
        // A document taller than the minimap scrolls through it in step with the viewport.
        let offset =
            if document <= fits || max_scroll == 0.0 { 0.0 } else { self.scroll_top / max_scroll * (document - fits) };
        (document, scale, offset)
    }

    /// Lines the minimap shows, folded ones included; a frontend highlights them too.
    pub fn minimap_lines(&self, rope: &Rope) -> Range<usize> {
        if self.minimap_width <= 0.0 {
            return 0..0;
        }
        let (_, scale, offset) = self.minimap_geometry(rope);
        let first = self.line_at_y(rope, offset);
        let last = self.line_at_y(rope, offset + self.height as f64 / scale);
        first..last + 1
    }

    fn minimap(&self, rope: &Rope, styler: &dyn Styler) -> Option<MinimapFrame> {
        if self.minimap_width <= 0.0 {
            return None;
        }
        let (_, scale, offset) = self.minimap_geometry(rope);
        let x0 = self.width - self.minimap_width;
        let max_cells = (self.minimap_width / MINIMAP_CELL) as usize;
        let lh = self.metrics.line_height as f64;
        let tab = self.tab_size;
        let mut blocks = Vec::new();
        for line in self.shown(self.minimap_lines(rope)) {
            let spans = styler.spans(line);
            let rows = self.line_rows(rope, line);
            let top = self.line_top(line);
            let mut row = 0;
            let mut cells = 0usize; // cells from the line start
            let mut row_shift = 0isize; // added to `cells` to get the x within the row
            let mut span_idx = 0;
            let mut run: Option<MinimapBlock> = None;
            let y_of_row = |row: usize| ((top + row as f64 * lh - offset) * scale) as f32;
            for (col, c) in rope.line(line).chars().take(line_len(rope, line)).enumerate() {
                if rows.starts.get(row + 1) == Some(&col) {
                    blocks.extend(run.take());
                    row += 1;
                    row_shift = rows.indent as isize - cells as isize;
                }
                let w = cell_width(c, cells, tab);
                let x = (cells as isize + row_shift) as usize;
                cells += w;
                if x >= max_cells {
                    continue;
                }
                if c.is_whitespace() {
                    blocks.extend(run.take());
                    continue;
                }
                while span_idx < spans.len() && spans[span_idx].end <= col {
                    span_idx += 1;
                }
                let style = spans.get(span_idx).filter(|s| s.start <= col).map_or(0, |s| s.style);
                match &mut run {
                    Some(r) if r.style == style => r.width += w as f32 * MINIMAP_CELL,
                    _ => {
                        blocks.extend(run.take());
                        run = Some(MinimapBlock {
                            x: x0 + x as f32 * MINIMAP_CELL,
                            y: y_of_row(row),
                            width: w as f32 * MINIMAP_CELL,
                            style,
                        });
                    }
                }
            }
            blocks.extend(run);
        }
        let slider = self.minimap_slider(rope);
        Some(MinimapFrame { x: x0, width: self.minimap_width, row_height: MINIMAP_ROW, blocks, slider })
    }

    /// The minimap's slider: the part of the minimap the viewport shows.
    pub fn minimap_slider(&self, rope: &Rope) -> Rect {
        let (document, scale, offset) = self.minimap_geometry(rope);
        Rect {
            x: self.width - self.minimap_width,
            y: ((self.scroll_top - offset) * scale) as f32,
            width: self.minimap_width,
            height: (self.height as f64 * scale).min(document * scale) as f32,
        }
    }

    /// Whether viewport x falls on the minimap.
    pub fn in_minimap(&self, x: f32) -> bool {
        self.minimap_width > 0.0 && x >= self.width - self.minimap_width
    }

    /// Scroll position that puts the document y under minimap y `y` in the middle of the
    /// viewport, for a click on the minimap.
    pub fn minimap_scroll_target(&self, rope: &Rope, y: f32) -> f64 {
        let (_, scale, offset) = self.minimap_geometry(rope);
        offset + y as f64 / scale - self.height as f64 / 2.0
    }

    /// Scroll distance per pixel of slider drag.
    pub fn minimap_drag_factor(&self, rope: &Rope) -> f64 {
        let (document, scale, _) = self.minimap_geometry(rope);
        let fits = self.height as f64 / scale;
        let max_scroll = (self.content_height(rope) - self.height as f64).max(0.0);
        if document > fits {
            let slider = self.height as f64 * scale;
            max_scroll / (self.height as f64 - slider).max(1.0)
        } else {
            1.0 / scale
        }
    }

    /// Where a caret at char `pos` would be drawn, if its line is visible: for placing
    /// widgets next to text.
    pub fn position_rect(&self, rope: &Rope, pos: usize) -> Option<Rect> {
        self.caret_rect(rope, pos.min(rope.len_chars()), self.gutter_width(rope))
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
        self.range_rects_in(rope, range, gutter, &self.visible_lines(rope))
    }

    /// [`View::range_rects`] with the visible lines computed once by the caller.
    fn range_rects_in(
        &self,
        rope: &Rope,
        range: std::ops::Range<usize>,
        gutter: f32,
        visible: &Range<usize>,
    ) -> Vec<Rect> {
        let cw = self.metrics.char_width;
        let lh = self.metrics.line_height;
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
        let hints: usize = self
            .inlays_on(rope, line)
            .iter()
            .filter(|h| h.0 >= rows.starts[row] && h.0 < rows.starts[row + 1])
            .map(|h| h.1)
            .sum();
        (row, rows.indent_of(row) + end_cells - start_cells + hints)
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
            .flat_map(|d| {
                self.range_rects_in(rope, d.range.clone(), gutter, &visible).into_iter().map(move |r| (r, d.kind))
            })
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
    /// Cached until decorations or line positions change, since a search can leave tens of
    /// thousands of decorations and the marks do not depend on the scroll position.
    pub fn overview_marks(&self, model: &TextModel, track: f32, accept: impl Fn(u32) -> bool) -> Vec<(f32, f32, u32)> {
        let key = (model.decorations.generation(), self.geometry, track.to_bits());
        if let Some((k, marks)) = self.overview_cache.borrow().as_ref()
            && *k == key
        {
            return marks.clone();
        }
        let marks = self.compute_overview_marks(model, track, accept);
        *self.overview_cache.borrow_mut() = Some((key, marks.clone()));
        marks
    }

    fn compute_overview_marks(
        &self,
        model: &TextModel,
        track: f32,
        accept: impl Fn(u32) -> bool,
    ) -> Vec<(f32, f32, u32)> {
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
        out.sort_by(|x, y| x.2.cmp(&y.2).then(x.0.total_cmp(&y.0)));
        // Marks of one kind that touch merge, so thousands of matches draw as a few bars.
        let mut merged: Vec<(f32, f32, u32)> = Vec::with_capacity(out.len().min(4096));
        for m in out {
            match merged.last_mut() {
                Some(last) if last.2 == m.2 && m.0 <= last.0 + last.1 + 0.5 => {
                    last.1 = last.1.max(m.0 + m.1 - last.0);
                }
                _ => merged.push(m),
            }
        }
        merged
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
        // Blank lines take the smaller level of the nearest non-blank lines around them; one
        // pass each way over the screen, plus a bounded look past its edges.
        let raw: Vec<Option<usize>> = lines.iter().map(|&l| indent_level(rope, l, tab)).collect();
        let first = lines.first().copied().unwrap_or(0);
        let last = lines.last().copied().unwrap_or(0);
        let above =
            (first.saturating_sub(GUIDE_REACH)..first).rev().find_map(|l| indent_level(rope, l, tab)).unwrap_or(0);
        let below = (last + 1..(last + GUIDE_REACH).min(total)).find_map(|l| indent_level(rope, l, tab)).unwrap_or(0);
        let mut levels = vec![0; raw.len()];
        let mut prev = above;
        for (i, r) in raw.iter().enumerate() {
            if let Some(l) = r {
                prev = *l;
            }
            levels[i] = prev;
        }
        let mut next = below;
        for (i, r) in raw.iter().enumerate().rev() {
            match r {
                Some(l) => next = *l,
                None => levels[i] = levels[i].min(next),
            }
        }
        let active_level = guide_level(rope, cursor_line, tab).max(1);

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
            let inlays = self.inlays_on(rope, line);
            let mut cells = 0;
            let mut row = 0;
            let mut row_shift = 0isize;
            for (col, c) in rope.line(line).chars().take(line_len(rope, line).min(MAX_MARKED_COLUMNS)).enumerate() {
                if rows.starts.get(row + 1) == Some(&col) {
                    row += 1;
                    row_shift = rows.indent as isize - cells as isize;
                }
                for h in inlays.iter().filter(|h| h.0 == col) {
                    row_shift += h.1 as isize;
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
#[allow(clippy::too_many_arguments)]
fn layout_line(
    rope: &Rope,
    line: usize,
    cols: Range<usize>,
    spans: &[StyleSpan],
    window: Range<usize>,
    tab: usize,
    x_of: &dyn Fn(usize) -> f32,
    inlays: &[(usize, usize, &str)],
) -> (Vec<TextRun>, usize) {
    let mut runs: Vec<TextRun> = Vec::new();
    let mut span_idx = 0;
    let mut acc = 0usize;
    // Cells added by inlay hints drawn so far on this row.
    let mut shift = 0usize;
    let mut prev_off_grid = false;
    let hint = |col: usize, acc: usize, shift: &mut usize, runs: &mut Vec<TextRun>| {
        for &(_, cells, label) in inlays.iter().filter(|h| h.0 == col && cols.contains(&col)) {
            runs.push(TextRun { x: x_of(acc + *shift), text: label.to_owned(), style: INLAY_STYLE });
            *shift += cells;
        }
    };
    let line_end = line_len(rope, line);
    for (col, c) in rope.line(line).chars().take(cols.end).enumerate() {
        let w = cell_width(c, acc, tab);
        if acc >= window.end {
            // Past the visible area: the rest of the line only matters for its width,
            // which is approximated by the char count to keep long lines cheap.
            return (runs, acc + shift + (cols.end - col));
        }
        if col >= cols.start {
            hint(col, acc, &mut shift, &mut runs);
        }
        if col >= cols.start && acc + w > window.start {
            while span_idx < spans.len() && spans[span_idx].end <= col {
                span_idx += 1;
            }
            let style = spans.get(span_idx).filter(|s| s.start <= col).map_or(0, |s| s.style);
            // A char outside ASCII may come from a fallback font whose advance is not a whole
            // number of cells (CJK, emoji), so it gets a run of its own, placed on the grid,
            // and the text after it starts on the grid again instead of drifting.
            let off_grid = !c.is_ascii();
            let start_new =
                off_grid || prev_off_grid || runs.last().is_none_or(|r| r.style != style || r.style == INLAY_STYLE);
            prev_off_grid = off_grid;
            if start_new {
                runs.push(TextRun { x: x_of(acc + shift), text: String::new(), style });
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
    // A hint at the end of the line, after its last char.
    if cols.end == line_end {
        let mut cols_end = cols.clone();
        cols_end.end += 1;
        for &(_, cells, label) in inlays.iter().filter(|h| h.0 == line_end && cols_end.contains(&h.0)) {
            runs.push(TextRun { x: x_of(acc + shift), text: label.to_owned(), style: INLAY_STYLE });
            shift += cells;
        }
    }
    (runs, acc + shift)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustaco_core::{Document, Selection, Selections};

    fn view() -> View {
        View::new(Metrics::new(10.0, 20.0), 400.0, 100.0)
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
        let mut v = View::new(Metrics::new(10.0, 20.0), 400.0, 400.0);
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
        let mut v = View::new(Metrics::new(10.0, 20.0), 400.0, 400.0);
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
        let mut v = View::new(Metrics::new(10.0, 20.0), 400.0, 400.0);
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
        let mut v = View::new(Metrics::new(10.0, 20.0), 284.0, 200.0);
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

#[cfg(test)]
mod minimap_tests {
    use super::*;
    use rustaco_core::Document;

    #[test]
    fn minimap_draws_blocks_and_a_slider() {
        let text: String = (0..500).map(|i| format!("let x{i} = {i};\n")).collect();
        let doc = Document::new(&text);
        let mut v = View::new(Metrics::new(10.0, 20.0), 600.0, 200.0);
        v.set_minimap(80.0);
        let frame = v.layout(doc.model(), doc.selections(), &Plain);
        let m = frame.minimap.expect("the minimap is on");
        assert_eq!((m.x, m.width), (520.0, 80.0));
        // "let", "x0", "=", "0;" on the first row.
        let first_row: Vec<&MinimapBlock> = m.blocks.iter().filter(|b| b.y == 0.0).collect();
        assert_eq!(first_row.len(), 4);
        assert_eq!((first_row[0].x, first_row[0].width), (520.0, 3.0));
        assert_eq!(m.slider.y, 0.0);
        assert_eq!(m.slider.height, 20.0, "the viewport's 10 rows at 2 px each");
        // At the bottom the slider reaches the bottom of the minimap.
        v.scroll_to(doc.rope(), 1e9, 0.0);
        let m = v.layout(doc.model(), doc.selections(), &Plain).minimap.unwrap();
        assert!((m.slider.y + m.slider.height - 200.0).abs() < 1.0, "slider at {:?}", m.slider);
    }
}

#[cfg(test)]
mod sticky_tests {
    use super::*;
    use rustaco_core::Document;

    #[test]
    fn headers_of_enclosing_scopes_stay_pinned() {
        // impl at 0, fn at 1, body lines 2..=31, closing braces at 32 and 33.
        let mut text = String::from("impl A {\n    fn f() {\n");
        for i in 0..30 {
            text.push_str(&format!("        line{i};\n"));
        }
        text.push_str("    }\n}\nafter\n");
        let doc = Document::new(&text);
        let mut v = View::new(Metrics::new(10.0, 20.0), 400.0, 200.0);
        v.sync(doc.model());
        v.set_foldable(indent_fold_ranges(doc.rope(), 4));
        v.set_sticky_scroll(5);
        v.scroll_to(doc.rope(), 10.0 * 20.0, 0.0);
        let frame = v.layout(doc.model(), doc.selections(), &Plain);
        let pinned: Vec<(usize, f32)> = frame.sticky.iter().map(|l| (l.line, l.y)).collect();
        assert_eq!(pinned, vec![(0, 0.0), (1, 20.0)]);
        assert_eq!(frame.sticky[1].runs[0].text.trim(), "fn f() {");
        // Near the end of the fn the fn header slides up under the impl header.
        v.scroll_to(doc.rope(), 31.0 * 20.0 - 15.0, 0.0);
        let pinned = v.sticky_lines(doc.rope());
        assert_eq!(pinned.len(), 2);
        assert!(pinned[1].1 < 20.0, "pushed up: {pinned:?}");
        // At the top nothing is pinned.
        v.scroll_to(doc.rope(), 0.0, 0.0);
        assert!(v.sticky_lines(doc.rope()).is_empty());
    }
}

#[cfg(test)]
mod zone_tests {
    use super::*;
    use rustaco_core::{Document, Edit};

    fn view() -> View {
        View::new(Metrics::new(10.0, 20.0), 400.0, 400.0)
    }

    #[test]
    fn zones_add_space_between_lines_and_follow_edits() {
        let mut doc = Document::new("a\nb\nc\nd");
        let mut v = view();
        v.set_zones(doc.model(), [(7, Some(1), 50.0), (8, None, 10.0)]);
        let frame = v.layout(doc.model(), doc.selections(), &Plain);
        let tops: Vec<(usize, f32)> = frame.lines.iter().map(|l| (l.line, l.y)).collect();
        assert_eq!(tops, vec![(0, 10.0), (1, 30.0), (2, 100.0), (3, 120.0)]);
        assert_eq!(
            frame.zones,
            vec![
                (8, Rect { x: frame.gutter_width, y: 0.0, width: 400.0 - frame.gutter_width, height: 10.0 }),
                (7, Rect { x: frame.gutter_width, y: 50.0, width: 400.0 - frame.gutter_width, height: 50.0 }),
            ]
        );
        // A click inside the zone lands on the line above it, not below.
        assert_eq!(doc.rope().char_to_line(v.hit_test(doc.rope(), 100.0, 80.0)), 1);
        // A line inserted above moves the zone down with its line.
        doc.apply_edits(vec![Edit::insert(0, "new\n")]);
        let frame = v.layout(doc.model(), doc.selections(), &Plain);
        assert_eq!(frame.zones[1].1.y, 70.0);
        assert_eq!(v.line_top(3), 120.0);
        // Typing at the end of the zone's line keeps the zone after it.
        let end_of_b = doc.rope().line_to_char(2) + 1;
        doc.apply_edits(vec![Edit::insert(end_of_b, "\nmore")]);
        v.sync(doc.model());
        assert_eq!(v.line_top(3), 10.0 + 3.0 * 20.0 + 50.0, "the zone stays after line 2, before the new line");
        v.set_zones(doc.model(), []);
        assert_eq!(v.line_top(3), 60.0);
    }
}

#[cfg(test)]
mod grid_tests {
    use super::*;
    use rustaco_core::Document;

    #[test]
    fn chars_outside_ascii_sit_on_the_grid() {
        let doc = Document::new("a\u{4E2D}b\u{e9}cd");
        let mut v = View::new(Metrics::new(10.0, 20.0), 400.0, 100.0);
        let frame = v.layout(doc.model(), doc.selections(), &Plain);
        let g = frame.gutter_width;
        let runs: Vec<(&str, f32)> = frame.lines[0].runs.iter().map(|r| (r.text.as_str(), r.x - g)).collect();
        assert_eq!(runs, vec![("a", 0.0), ("\u{4E2D}", 10.0), ("b", 30.0), ("\u{e9}", 40.0), ("cd", 50.0)]);
    }
}

#[cfg(test)]
mod inlay_tests {
    use super::*;
    use rustaco_core::{Document, Edit, Selection, Selections};

    #[test]
    fn hints_push_text_right_and_carets_stay_before_them() {
        let mut doc = Document::new("let x = 1;");
        let mut v = View::new(Metrics::new(10.0, 20.0), 400.0, 100.0);
        v.set_inlays(doc.model(), vec![(5, ": i32".into())]);
        doc.set_selections(Selections::single(Selection::cursor(5)));
        let frame = v.layout(doc.model(), doc.selections(), &Plain);
        let g = frame.gutter_width;
        let runs: Vec<(&str, f32, bool)> =
            frame.lines[0].runs.iter().map(|r| (r.text.as_str(), r.x - g, r.style == INLAY_STYLE)).collect();
        assert_eq!(runs, vec![("let x", 0.0, false), (": i32", 50.0, true), (" = 1;", 100.0, false)]);
        assert_eq!(frame.carets[0].x - g, 50.0, "the caret at the hint stays before it");
        // A click on the hint lands at its offset; after it, on the text.
        assert_eq!(v.hit_test(doc.rope(), g + 70.0, 5.0), 5);
        assert_eq!(v.hit_test(doc.rope(), g + 110.0, 5.0), 6);
        // The hint follows edits before it.
        doc.apply_edits(vec![Edit::insert(0, "  ")]);
        let frame = v.layout(doc.model(), doc.selections(), &Plain);
        assert!(frame.lines[0].runs.iter().any(|r| r.style == INLAY_STYLE && r.x - g == 70.0));
    }
}
