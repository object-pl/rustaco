//! Selections and multi-cursor sets.

/// A selection between `anchor` (fixed end) and `head` (the caret), in char offsets.
/// An empty selection is a plain cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
    /// Column the caret tries to return to when moving vertically through shorter lines.
    pub goal_column: Option<usize>,
}

impl Selection {
    pub const fn new(anchor: usize, head: usize) -> Self {
        Self { anchor, head, goal_column: None }
    }

    pub const fn cursor(pos: usize) -> Self {
        Self::new(pos, pos)
    }

    pub fn start(&self) -> usize {
        self.anchor.min(self.head)
    }

    pub fn end(&self) -> usize {
        self.anchor.max(self.head)
    }

    pub fn len(&self) -> usize {
        self.end() - self.start()
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    /// True when the caret sits before the anchor (selection made right to left).
    pub fn is_backward(&self) -> bool {
        self.head < self.anchor
    }

    fn merge(self, other: Selection) -> Selection {
        let (start, end) = (self.start().min(other.start()), self.end().max(other.end()));
        let backward = if self.is_empty() { other.is_backward() } else { self.is_backward() };
        if backward { Selection::new(end, start) } else { Selection::new(start, end) }
    }

    fn clamp(self, max: usize) -> Selection {
        Selection { anchor: self.anchor.min(max), head: self.head.min(max), ..self }
    }
}

/// A non-empty set of selections, sorted by position, never overlapping.
/// One of them is primary: it scrolls into view and receives single-cursor commands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selections {
    ranges: Vec<Selection>,
    primary: usize,
}

impl Selections {
    pub fn single(sel: Selection) -> Self {
        Self { ranges: vec![sel], primary: 0 }
    }

    /// Builds a normalized set. Overlapping selections, and a cursor touching
    /// another selection, are merged.
    ///
    /// # Panics
    /// If `ranges` is empty or `primary` is out of bounds.
    pub fn new(ranges: Vec<Selection>, primary: usize) -> Self {
        assert!(primary < ranges.len(), "primary selection out of bounds");
        let mut s = Self { ranges, primary };
        s.normalize();
        s
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Selection> {
        self.ranges.iter()
    }

    pub fn as_slice(&self) -> &[Selection] {
        &self.ranges
    }

    pub fn len(&self) -> usize {
        self.ranges.len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn primary(&self) -> Selection {
        self.ranges[self.primary]
    }

    pub fn primary_index(&self) -> usize {
        self.primary
    }

    /// Applies `f` to every selection, keeping the primary index, and renormalizes.
    pub fn map(&self, f: impl FnMut(&Selection) -> Selection) -> Self {
        Self::new(self.ranges.iter().map(f).collect(), self.primary)
    }

    pub(crate) fn clamp(&self, max: usize) -> Self {
        self.map(|s| s.clamp(max))
    }

    fn normalize(&mut self) {
        if self.ranges.len() == 1 {
            return;
        }
        let mut indexed: Vec<(usize, Selection)> = self.ranges.drain(..).enumerate().collect();
        indexed.sort_by_key(|(_, s)| (s.start(), s.end()));
        let mut primary = 0;
        for (i, sel) in indexed {
            if let Some(last) = self.ranges.last_mut() {
                let touches = sel.start() == last.end() && (sel.is_empty() || last.is_empty());
                if sel.start() < last.end() || touches {
                    *last = last.merge(sel);
                    if i == self.primary {
                        primary = self.ranges.len() - 1;
                    }
                    continue;
                }
            }
            self.ranges.push(sel);
            if i == self.primary {
                primary = self.ranges.len() - 1;
            }
        }
        self.primary = primary;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_and_tracks_primary() {
        let s = Selections::new(vec![Selection::cursor(9), Selection::cursor(2)], 0);
        assert_eq!(s.as_slice(), [Selection::cursor(2), Selection::cursor(9)]);
        assert_eq!(s.primary(), Selection::cursor(9));
    }

    #[test]
    fn merges_overlap_and_touching_cursor() {
        let s = Selections::new(
            vec![Selection::new(0, 4), Selection::new(6, 2), Selection::cursor(6), Selection::new(7, 9)],
            3,
        );
        assert_eq!(s.as_slice(), [Selection::new(0, 6), Selection::new(7, 9)]);
        assert_eq!(s.primary_index(), 1);
    }

    #[test]
    fn keeps_adjacent_non_empty_selections_apart() {
        let s = Selections::new(vec![Selection::new(0, 2), Selection::new(2, 4)], 0);
        assert_eq!(s.len(), 2);
    }
}
