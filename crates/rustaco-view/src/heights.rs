//! Per-line heights with fast prefix sums.
//!
//! [`HeightMap`] stores the pixel height of every model line (0 for a line hidden inside a
//! fold, several rows for a wrapped line, extra pixels for a block between lines) and
//! answers "where does line N start" and "which line is at y" in O(log n + chunk). Lines
//! are kept in chunks, so inserting or deleting lines costs one chunk, not the file.

/// Lines per chunk after a split. Lookups scan at most `2 * CHUNK` lines.
use std::cell::RefCell;

const CHUNK: usize = 512;

#[derive(Clone, Debug)]
struct Chunk {
    heights: Vec<f32>,
    sum: f64,
}

impl Chunk {
    fn new(heights: Vec<f32>) -> Self {
        let sum = heights.iter().map(|&h| h as f64).sum();
        Self { heights, sum }
    }
}

/// `prefix[i]` is the height of the chunks before chunk `i` and `starts[i]` the first line
/// of chunk `i`; both are valid for chunks before `valid`. Rebuilt lazily, so a batch of
/// changes costs one rebuild.
#[derive(Clone, Debug, Default)]
struct Index {
    prefix: Vec<f64>,
    starts: Vec<usize>,
    valid: usize,
}

#[derive(Clone, Debug)]
pub struct HeightMap {
    chunks: Vec<Chunk>,
    index: RefCell<Index>,
    lines: usize,
}

impl HeightMap {
    /// `lines` lines of height `height` each.
    pub fn new(lines: usize, height: f32) -> Self {
        let mut map = Self { chunks: Vec::new(), index: RefCell::default(), lines: 0 };
        map.splice(0, 0, std::iter::repeat_n(height, lines));
        map
    }

    pub fn len(&self) -> usize {
        self.lines
    }

    pub fn is_empty(&self) -> bool {
        self.lines == 0
    }

    pub fn height(&self, line: usize) -> f32 {
        let (c, i) = self.locate(line);
        self.chunks[c].heights[i]
    }

    /// Sets the height of `line`; returns the change in total height.
    pub fn set(&mut self, line: usize, height: f32) -> f64 {
        let (c, i) = self.locate(line);
        let chunk = &mut self.chunks[c];
        let old = std::mem::replace(&mut chunk.heights[i], height);
        if old == height {
            return 0.0;
        }
        let delta = height as f64 - old as f64;
        chunk.sum += delta;
        self.invalidate(c + 1);
        delta
    }

    /// Sets every line in `lines` to `height`, a chunk at a time.
    pub fn fill(&mut self, lines: std::ops::Range<usize>, height: f32) {
        let end = lines.end.min(self.lines);
        if lines.start >= end {
            return;
        }
        let (mut c, mut i) = self.locate(lines.start);
        self.invalidate(c + 1);
        let mut left = end - lines.start;
        while left > 0 {
            let chunk = &mut self.chunks[c];
            let take = (chunk.heights.len() - i).min(left);
            chunk.heights[i..i + take].fill(height);
            chunk.sum = chunk.heights.iter().map(|&h| h as f64).sum();
            left -= take;
            c += 1;
            i = 0;
        }
    }

    /// Replaces `removed` lines starting at `line` with `inserted`.
    pub fn splice(&mut self, line: usize, removed: usize, inserted: impl IntoIterator<Item = f32>) {
        assert!(line + removed <= self.lines, "splice past the end");
        let inserted: Vec<f32> = inserted.into_iter().collect();
        if self.chunks.is_empty() {
            self.lines = inserted.len();
            self.chunks = inserted.chunks(CHUNK).map(|c| Chunk::new(c.to_vec())).collect();
            self.invalidate(0);
            return;
        }
        // Gather the affected chunks into one vector, edit it, and cut it back into chunks.
        let (first, offset) = if line == self.lines { (self.chunks.len() - 1, usize::MAX) } else { self.locate(line) };
        let offset = if offset == usize::MAX { self.chunks[first].heights.len() } else { offset };
        let mut last = first;
        let mut covered = self.chunks[first].heights.len() - offset;
        while covered < removed {
            last += 1;
            covered += self.chunks[last].heights.len();
        }
        let mut merged: Vec<f32> = Vec::with_capacity(covered + offset + inserted.len());
        for c in &self.chunks[first..=last] {
            merged.extend_from_slice(&c.heights);
        }
        merged.splice(offset..offset + removed, inserted.iter().copied());
        self.lines = self.lines - removed + inserted.len();
        let replacement: Vec<Chunk> = if merged.len() <= 2 * CHUNK {
            if merged.is_empty() { Vec::new() } else { vec![Chunk::new(merged)] }
        } else {
            merged.chunks(CHUNK).map(|c| Chunk::new(c.to_vec())).collect()
        };
        self.chunks.splice(first..=last, replacement);
        self.invalidate(first);
    }

    /// Total height of all lines.
    pub fn total(&self) -> f64 {
        let index = self.index();
        match self.chunks.last() {
            Some(c) => index.prefix[self.chunks.len() - 1] + c.sum,
            None => 0.0,
        }
    }

    /// Distance from the top of the document to the top of `line`. `line == len()` gives the
    /// total height.
    pub fn top(&self, line: usize) -> f64 {
        if line >= self.lines {
            return self.total();
        }
        let (c, i) = self.locate(line);
        self.index().prefix[c] + self.chunks[c].heights[..i].iter().map(|&h| h as f64).sum::<f64>()
    }

    /// The line at document height `y`: the last line whose top is at or above `y`, skipping
    /// lines of zero height. Clamped into the document.
    pub fn line_at(&self, y: f64) -> usize {
        if self.lines == 0 {
            return 0;
        }
        let index = self.index();
        let n = self.chunks.len();
        // Last chunk whose start is at or above y.
        let c = index.prefix[..n].partition_point(|&p| p <= y).saturating_sub(1);
        let mut acc = index.prefix[c];
        let mut found = None;
        let mut chunk = c;
        // A chunk of zero-height lines may hold y's top; continue into later chunks if needed.
        'outer: while chunk < n {
            for (i, &h) in self.chunks[chunk].heights.iter().enumerate() {
                if h > 0.0 {
                    if acc + h as f64 > y {
                        found = Some(index.starts[chunk] + i);
                        break 'outer;
                    }
                    found = Some(index.starts[chunk] + i);
                }
                acc += h as f64;
            }
            chunk += 1;
        }
        found.unwrap_or(self.lines - 1)
    }

    /// Chunk and index inside it of `line`.
    fn locate(&self, line: usize) -> (usize, usize) {
        assert!(line < self.lines, "line {line} out of {}", self.lines);
        let index = self.index();
        let c = index.starts.partition_point(|&s| s <= line) - 1;
        (c, line - index.starts[c])
    }

    fn invalidate(&mut self, chunk: usize) {
        let index = self.index.get_mut();
        index.valid = index.valid.min(chunk);
    }

    /// The index, brought up to date.
    fn index(&self) -> std::cell::Ref<'_, Index> {
        {
            let mut index = self.index.borrow_mut();
            let n = self.chunks.len();
            if index.valid < n || index.prefix.len() != n {
                index.prefix.resize(n, 0.0);
                index.starts.resize(n, 0);
                for i in index.valid.min(n)..n {
                    if i == 0 {
                        index.prefix[0] = 0.0;
                        index.starts[0] = 0;
                    } else {
                        index.prefix[i] = index.prefix[i - 1] + self.chunks[i - 1].sum;
                        index.starts[i] = index.starts[i - 1] + self.chunks[i - 1].heights.len();
                    }
                }
                index.valid = n;
            }
        }
        self.index.borrow()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same map kept as a plain vector.
    fn check(map: &HeightMap, plain: &[f32]) {
        assert_eq!(map.len(), plain.len());
        let mut top = 0.0f64;
        for (line, &h) in plain.iter().enumerate() {
            assert_eq!(map.height(line), h, "height of {line}");
            assert!((map.top(line) - top).abs() < 1e-6, "top of {line}");
            if h > 0.0 {
                assert_eq!(map.line_at(top), line, "line at top of {line}");
                assert_eq!(map.line_at(top + h as f64 * 0.99), line, "line near bottom of {line}");
            }
            top += h as f64;
        }
        assert!((map.total() - top).abs() < 1e-6);
    }

    #[test]
    fn uniform_lines() {
        let map = HeightMap::new(2000, 20.0);
        assert_eq!(map.top(1500), 30000.0);
        assert_eq!(map.line_at(30010.0), 1500);
        assert_eq!(map.line_at(-5.0), 0);
        assert_eq!(map.line_at(1e9), 1999);
    }

    #[test]
    fn hidden_lines_are_skipped() {
        let mut map = HeightMap::new(10, 20.0);
        for l in 3..7 {
            map.set(l, 0.0);
        }
        assert_eq!(map.top(7), 60.0);
        assert_eq!(map.line_at(59.0), 2);
        assert_eq!(map.line_at(60.0), 7);
    }

    #[test]
    fn matches_a_plain_vector_under_random_edits() {
        let mut seed = 0x2545_f491_u64;
        let mut rand = move |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n.max(1) as u64) as usize
        };
        let mut plain = vec![20.0f32; 3000];
        let mut map = HeightMap::new(plain.len(), 20.0);
        for _ in 0..300 {
            match rand(3) {
                0 => {
                    let l = rand(plain.len());
                    let h = [0.0, 20.0, 40.0, 60.0][rand(4)];
                    plain[l] = h;
                    map.set(l, h);
                }
                _ => {
                    let at = rand(plain.len() + 1);
                    let removed = rand((plain.len() - at).min(1500) + 1);
                    let inserted: Vec<f32> = (0..rand(1500)).map(|i| if i % 3 == 0 { 40.0 } else { 20.0 }).collect();
                    plain.splice(at..at + removed, inserted.iter().copied());
                    map.splice(at, removed, inserted);
                }
            }
            if plain.is_empty() {
                plain.push(20.0);
                map.splice(0, 0, [20.0]);
            }
        }
        check(&map, &plain);
    }
}
