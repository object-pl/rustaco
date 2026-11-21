//! Folded regions and the regions that can be folded.

use std::ops::Range;

use rustaco_core::Rope;

use crate::HeightMap;

/// A collapsed region: the lines after the header line down to the line holding `end` are
/// hidden. Both ends are char offsets (the end of the header line and the end of the last
/// hidden line), so the fold follows edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Fold {
    pub start: usize,
    pub end: usize,
}

/// A fold marker in the gutter: the line heads a region that can be folded, or is folded.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FoldMarker {
    pub line: usize,
    pub y: f32,
    pub height: f32,
    pub folded: bool,
}

/// Lines hidden by folds, with what is needed to convert between document heights with
/// and without them. The height map keeps every line's own height; hidden lines are taken
/// out here, so folding never rewrites the map.
#[derive(Clone, Debug, Default)]
pub(crate) struct Hidden {
    /// Sorted, disjoint runs of hidden lines.
    pub ranges: Vec<Range<usize>>,
    /// Shown y of the top of each run (where the run would start).
    shown_top: Vec<f64>,
    /// Height hidden by runs `0..=i`.
    hidden_through: Vec<f64>,
}

impl Hidden {
    /// Hidden runs as the union of `ranges`, measured against `heights`.
    pub fn new(mut ranges: Vec<Range<usize>>, heights: &HeightMap) -> Self {
        ranges.sort_by_key(|r| r.start);
        let mut merged: Vec<Range<usize>> = Vec::new();
        for r in ranges {
            match merged.last_mut() {
                Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
                _ => merged.push(r),
            }
        }
        let mut hidden = Self { ranges: Vec::new(), shown_top: Vec::new(), hidden_through: Vec::new() };
        hidden.ranges = merged;
        hidden.measure(heights);
        hidden
    }

    /// Recomputes the measurements after line heights changed.
    pub fn measure(&mut self, heights: &HeightMap) {
        let len = heights.len();
        self.ranges.retain(|r| r.start < len);
        for r in &mut self.ranges {
            r.end = r.end.min(len);
        }
        self.shown_top.clear();
        self.hidden_through.clear();
        let mut acc = 0.0;
        for r in &self.ranges {
            let top = heights.top(r.start);
            self.shown_top.push(top - acc);
            acc += heights.top(r.end) - top;
            self.hidden_through.push(acc);
        }
    }

    /// The run holding `line`, as its index.
    pub fn run_of(&self, line: usize) -> Option<usize> {
        let i = self.ranges.partition_point(|r| r.end <= line);
        self.ranges.get(i).filter(|r| r.start <= line).map(|_| i)
    }

    /// Shown y of the top of `line`; a hidden line sits where its run starts.
    pub fn top(&self, heights: &HeightMap, line: usize) -> f64 {
        let i = self.ranges.partition_point(|r| r.end <= line);
        if self.ranges.get(i).is_some_and(|r| r.start <= line) {
            return self.shown_top[i];
        }
        heights.top(line) - if i == 0 { 0.0 } else { self.hidden_through[i - 1] }
    }

    /// The shown line at shown height `y`.
    pub fn line_at(&self, heights: &HeightMap, y: f64) -> usize {
        // Runs that start at or above y lie wholly above it, since they take no space.
        let i = self.shown_top.partition_point(|&t| t <= y);
        let natural = y + if i == 0 { 0.0 } else { self.hidden_through[i - 1] };
        heights.line_at(natural)
    }

    /// Total shown height.
    pub fn total(&self, heights: &HeightMap) -> f64 {
        heights.total() - self.hidden_through.last().copied().unwrap_or(0.0)
    }
}

/// Moves foldable regions past an edit that replaced lines `a..=a + old` with `a..=a + new`.
/// Regions headed inside the edit are dropped until the provider recomputes them.
pub(crate) fn shift_foldable(foldable: &mut Vec<(usize, usize)>, a: usize, old: usize, new: usize) {
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

/// Moves a range of touched lines past a later edit, as [`shift_foldable`] does.
pub(crate) fn shift_lines(r: &mut Range<usize>, a: usize, old: usize, new: usize) {
    let shift = |l: usize| if l > a + old { l + new - old } else { l.min(a + new) };
    r.start = shift(r.start);
    r.end = shift(r.end.saturating_sub(1)) + 1;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_runs_take_no_space() {
        let heights = HeightMap::new(10, 20.0);
        let hidden = Hidden::new(vec![3..5, 4..7], &heights);
        assert_eq!(hidden.ranges, vec![3..7]);
        assert_eq!(hidden.top(&heights, 2), 40.0);
        assert_eq!(hidden.top(&heights, 5), 60.0);
        assert_eq!(hidden.top(&heights, 7), 60.0);
        assert_eq!(hidden.top(&heights, 8), 80.0);
        assert_eq!(hidden.line_at(&heights, 59.0), 2);
        assert_eq!(hidden.line_at(&heights, 60.0), 7);
        assert_eq!(hidden.total(&heights), 120.0);
    }

    #[test]
    fn indentation_gives_fold_ranges() {
        let rope = Rope::from_str("a\n  b\n\n  c\n    d\ne\n  f\n");
        assert_eq!(indent_fold_ranges(&rope, 4), vec![(0, 4), (3, 4), (5, 6)]);
    }
}
