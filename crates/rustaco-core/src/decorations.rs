//! Decorations: ranges of text with a kind (search match, diagnostic, diff hunk, highlight)
//! that move with edits. They live in the model, so every editor showing it sees them.
//!
//! Stored sorted by start. Ranges longer than [`LONG`] chars are kept apart and scanned
//! linearly, so looking up the decorations of the visible lines is a binary search plus the
//! few long ones (folding regions, multi-line diagnostics).

use std::ops::Range;

use crate::change::{Assoc, ChangeSet};

const LONG: usize = 4096;

/// How a decoration's edges behave when text is typed exactly at them (Monaco's names).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stickiness {
    /// Text typed at either edge becomes part of the decoration.
    AlwaysGrows,
    /// Text typed at an edge stays outside.
    #[default]
    NeverGrows,
    /// Text typed at the start is inside, at the end outside.
    GrowsBefore,
    /// Text typed at the end is inside, at the start outside.
    GrowsAfter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DecorationId(pub u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoration {
    pub id: DecorationId,
    pub range: Range<usize>,
    /// What the decoration means; frontends map kinds to styles.
    pub kind: u32,
    pub stickiness: Stickiness,
    /// Decorations added together share an owner and can be replaced together.
    pub owner: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Decorations {
    short: Vec<Decoration>,
    long: Vec<Decoration>,
    next_id: u64,
    /// Bumped by every change, so views can cache what they derive from decorations.
    generation: u64,
}

impl Decorations {
    pub fn len(&self) -> usize {
        self.short.len() + self.long.len()
    }

    /// Changes whenever any decoration is added, removed or moved.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Replaces all decorations of `owner` with `ranges`, returning their ids.
    pub fn set(
        &mut self,
        owner: u32,
        kind: u32,
        stickiness: Stickiness,
        ranges: impl IntoIterator<Item = Range<usize>>,
    ) -> Vec<DecorationId> {
        self.set_each(owner, stickiness, ranges.into_iter().map(|r| (r, kind)))
    }

    /// Replaces all decorations of `owner` with `items`, each a range with its own kind.
    pub fn set_each(
        &mut self,
        owner: u32,
        stickiness: Stickiness,
        items: impl IntoIterator<Item = (Range<usize>, u32)>,
    ) -> Vec<DecorationId> {
        self.clear(owner);
        self.generation += 1;
        let mut ids = Vec::new();
        for (range, kind) in items {
            self.next_id += 1;
            let d = Decoration { id: DecorationId(self.next_id), range, kind, stickiness, owner };
            ids.push(d.id);
            if d.range.len() > LONG { self.long.push(d) } else { self.short.push(d) }
        }
        self.short.sort_by_key(|d| (d.range.start, d.id));
        ids
    }

    pub fn clear(&mut self, owner: u32) {
        let before = self.len();
        self.short.retain(|d| d.owner != owner);
        self.long.retain(|d| d.owner != owner);
        if self.len() != before {
            self.generation += 1;
        }
    }

    pub fn get(&self, id: DecorationId) -> Option<&Decoration> {
        self.short.iter().chain(&self.long).find(|d| d.id == id)
    }

    /// Decorations overlapping `range` (a collapsed decoration at `range.start` counts), in
    /// order of start.
    pub fn in_range(&self, range: Range<usize>) -> Vec<&Decoration> {
        let from = self.short.partition_point(|d| d.range.start + LONG < range.start);
        let mut out: Vec<&Decoration> = self.short[from..]
            .iter()
            .take_while(|d| d.range.start <= range.end)
            .filter(|d| overlaps(&d.range, &range))
            .chain(self.long.iter().filter(|d| overlaps(&d.range, &range)))
            .collect();
        out.sort_by_key(|d| (d.range.start, d.id));
        out
    }

    /// Every decoration, in no particular order.
    pub fn iter(&self) -> impl Iterator<Item = &Decoration> {
        self.short.iter().chain(&self.long)
    }

    /// All decorations of `owner`, in order of start.
    pub fn of_owner(&self, owner: u32) -> Vec<&Decoration> {
        let mut out: Vec<&Decoration> = self.short.iter().chain(&self.long).filter(|d| d.owner == owner).collect();
        out.sort_by_key(|d| (d.range.start, d.id));
        out
    }

    /// Moves every decoration through an applied change set.
    pub(crate) fn apply(&mut self, changes: &ChangeSet) {
        if self.is_empty() || changes.is_empty() {
            return;
        }
        self.generation += 1;
        let edits = changes.edits();
        let first = edits[0].start;
        let last = edits[edits.len() - 1].end;
        let delta: isize = edits.iter().map(|e| e.text.chars().count() as isize - (e.end - e.start) as isize).sum();
        let map = |d: &mut Decoration| {
            let (start_assoc, end_assoc) = match d.stickiness {
                Stickiness::AlwaysGrows => (Assoc::Before, Assoc::After),
                Stickiness::NeverGrows => (Assoc::After, Assoc::Before),
                Stickiness::GrowsBefore => (Assoc::Before, Assoc::Before),
                Stickiness::GrowsAfter => (Assoc::After, Assoc::After),
            };
            let start = changes.map(d.range.start, start_assoc);
            let end = changes.map(d.range.end, end_assoc).max(start);
            d.range = start..end;
        };
        // Short decorations ending before the first edit stay; those starting after the last
        // one only shift. Only the ones in between go through the change set, so an edit
        // costs the decorations near it, not all of them.
        let lo = self.short.partition_point(|d| d.range.start + LONG < first);
        let hi = self.short.partition_point(|d| d.range.start <= last);
        for d in &mut self.short[hi..] {
            d.range = d.range.start.wrapping_add_signed(delta)..d.range.end.wrapping_add_signed(delta);
        }
        self.short[lo..hi].iter_mut().for_each(map);
        self.long.iter_mut().for_each(map);
        // Ranges that changed length may belong to the other list now.
        if self.short[lo..hi].iter().any(|d| d.range.len() > LONG) || self.long.iter().any(|d| d.range.len() <= LONG) {
            let (now_long, short): (Vec<_>, Vec<_>) =
                std::mem::take(&mut self.short).into_iter().partition(|d| d.range.len() > LONG);
            let (now_short, long): (Vec<_>, Vec<_>) =
                std::mem::take(&mut self.long).into_iter().partition(|d| d.range.len() <= LONG);
            self.short = short;
            self.short.extend(now_short);
            self.long = long;
            self.long.extend(now_long);
        }
        if !self.short.is_sorted_by_key(|d| (d.range.start, d.id)) {
            self.short.sort_by_key(|d| (d.range.start, d.id));
        }
    }
}

fn overlaps(a: &Range<usize>, b: &Range<usize>) -> bool {
    if a.is_empty() { b.start <= a.start && a.start <= b.end } else { a.start < b.end && b.start < a.end }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::change::Edit;

    #[test]
    fn mapping_only_nearby_decorations_matches_mapping_all() {
        let mut seed = 0x1234_5678_u64;
        let mut rand = move |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n.max(1) as u64) as usize
        };
        let mut len = 50_000;
        let mut fast = Decorations::default();
        let ranges: Vec<Range<usize>> = (0..2000)
            .map(|_| {
                let a = rand(len);
                a..(a + if rand(10) == 0 { rand(10_000) } else { rand(30) }).min(len)
            })
            .collect();
        fast.set(1, 0, Stickiness::NeverGrows, ranges.clone());
        let mut naive: Vec<(DecorationId, Range<usize>)> = fast.iter().map(|d| (d.id, d.range.clone())).collect();
        for _ in 0..200 {
            let mut edits = Vec::new();
            let mut at = 0;
            for _ in 0..1 + rand(3) {
                let start = (at + rand(len / 3)).min(len);
                let end = (start + rand(20)).min(len);
                edits.push(Edit::new(start, end, "x".repeat(rand(5))));
                at = end + 1;
                if at >= len {
                    break;
                }
            }
            let changes = ChangeSet::new(edits);
            len = (len as isize
                + changes.edits().iter().map(|e| e.text.len() as isize - (e.end - e.start) as isize).sum::<isize>())
                as usize;
            fast.apply(&changes);
            for (_, r) in &mut naive {
                let start = changes.map(r.start, Assoc::After);
                *r = start..changes.map(r.end, Assoc::Before).max(start);
            }
            let mut got: Vec<(DecorationId, Range<usize>)> = fast.iter().map(|d| (d.id, d.range.clone())).collect();
            got.sort_by_key(|x| x.0);
            naive.sort_by_key(|x| x.0);
            assert_eq!(got, naive);
            assert!(fast.short.is_sorted_by_key(|d| (d.range.start, d.id)));
        }
    }

    #[test]
    fn decorations_follow_edits_by_stickiness() {
        let mut d = Decorations::default();
        let ids = d.set(1, 0, Stickiness::NeverGrows, std::iter::once(2..5));
        let grows = d.set(2, 0, Stickiness::AlwaysGrows, std::iter::once(2..5));
        d.apply(&ChangeSet::new(vec![Edit::insert(0, "xx"), Edit::insert(5, "yy")]));
        assert_eq!(d.get(ids[0]).unwrap().range, 4..7, "typing at the end stays outside");
        assert_eq!(d.get(grows[0]).unwrap().range, 4..9, "typing at the end grows it");
        d.apply(&ChangeSet::new(vec![Edit::delete(3, 20)]));
        assert_eq!(d.get(ids[0]).unwrap().range, 3..3, "deleted text collapses the decoration");
    }

    #[test]
    fn lookup_by_range_includes_long_decorations() {
        let mut d = Decorations::default();
        d.set(1, 0, Stickiness::NeverGrows, (0..1000).map(|i| i * 10..i * 10 + 3));
        d.set(2, 9, Stickiness::NeverGrows, std::iter::once(0..50_000));
        let found = d.in_range(500..520);
        assert_eq!(found.iter().filter(|x| x.owner == 1).count(), 2);
        assert!(found.iter().any(|x| x.owner == 2));
        d.clear(1);
        assert_eq!(d.len(), 1);
    }
}
