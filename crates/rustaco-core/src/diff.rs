//! Line diff with a char-level pass inside changed lines.
//!
//! Lines are compared as interned ids (optionally ignoring whitespace) with Myers' O(ND)
//! algorithm in linear space (bisection at the middle snake, as diff-match-patch does), after
//! trimming the common prefix and suffix. When `keep_going` says to stop, the rest of the
//! input is reported as one replaced block, so a huge input still yields a usable, coarser
//! diff. Inside each changed block both sides are diffed again by chars, and deleted blocks
//! that reappear unchanged elsewhere are paired up as moves.

use std::collections::HashMap;
use std::ops::Range;

use ropey::Rope;

/// One changed block: lines `old` were replaced by lines `new` (either may be empty).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineChange {
    pub old: Range<usize>,
    pub new: Range<usize>,
    /// Char ranges (offsets into each text) that differ inside the block.
    pub inner: Vec<CharChange>,
    /// The index of the change this block moved to or from, when its lines reappear
    /// unchanged elsewhere.
    pub moved: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CharChange {
    pub old: Range<usize>,
    pub new: Range<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Diff {
    pub changes: Vec<LineChange>,
    /// False when the time budget ran out and part of the result is coarse.
    pub complete: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DiffOptions {
    /// Lines that differ only in whitespace count as equal.
    pub ignore_whitespace: bool,
}

/// Blocks with more chars than this on a side skip the char-level pass.
const MAX_INNER_CHARS: usize = 20_000;

/// Moved blocks must have at least this many lines.
const MIN_MOVED_LINES: usize = 3;

/// Diffs two texts by lines, then by chars inside changed blocks. `keep_going` is asked
/// now and then; once it returns false the remaining work is done coarsely.
pub fn diff(old: &Rope, new: &Rope, options: DiffOptions, keep_going: &mut dyn FnMut() -> bool) -> Diff {
    let mut ids: HashMap<String, u32> = HashMap::new();
    let mut intern = |rope: &Rope| -> Vec<u32> {
        rope.lines()
            .map(|line| {
                let mut s: String = line.chars().filter(|c| *c != '\n' && *c != '\r').collect();
                if options.ignore_whitespace {
                    s = s.split_whitespace().collect::<Vec<_>>().join(" ");
                }
                let next = ids.len() as u32;
                *ids.entry(s).or_insert(next)
            })
            .collect()
    };
    let a = intern(old);
    let b = intern(new);
    let mut ops = Vec::new();
    let mut complete = true;
    myers(&a, &b, 0, 0, &mut ops, keep_going, &mut complete);
    let mut changes = blocks(&ops, a.len(), b.len());
    for c in &mut changes {
        if !c.old.is_empty() && !c.new.is_empty() {
            c.inner = inner(old, new, c, keep_going, &mut complete);
        }
    }
    pair_moves(&mut changes, &a, &b);
    Diff { changes, complete }
}

/// An edit op on line indexes: (old start, old len, new start, new len) of a replaced run.
type Op = (usize, usize, usize, usize);

fn myers(
    a: &[u32],
    b: &[u32],
    ao: usize,
    bo: usize,
    ops: &mut Vec<Op>,
    keep_going: &mut dyn FnMut() -> bool,
    complete: &mut bool,
) {
    // Common prefix and suffix.
    let prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let (a, b) = (&a[prefix..], &b[prefix..]);
    let (ao, bo) = (ao + prefix, bo + prefix);
    let suffix = a.iter().rev().zip(b.iter().rev()).take_while(|(x, y)| x == y).count();
    let (a, b) = (&a[..a.len() - suffix], &b[..b.len() - suffix]);
    if a.is_empty() && b.is_empty() {
        return;
    }
    if a.is_empty() || b.is_empty() {
        ops.push((ao, a.len(), bo, b.len()));
        return;
    }
    if !*complete || !keep_going() {
        *complete = false;
        ops.push((ao, a.len(), bo, b.len()));
        return;
    }
    match bisect(a, b, keep_going) {
        Some((x, y)) => {
            myers(&a[..x], &b[..y], ao, bo, ops, keep_going, complete);
            myers(&a[x..], &b[y..], ao + x, bo + y, ops, keep_going, complete);
        }
        None => ops.push((ao, a.len(), bo, b.len())),
    }
}

/// The middle snake of the edit graph: a split point (x, y) such that diffing the halves
/// separately gives a shortest edit script. None when the sides share nothing (or time ran
/// out), which means "replace everything".
fn bisect(a: &[u32], b: &[u32], keep_going: &mut dyn FnMut() -> bool) -> Option<(usize, usize)> {
    let (n, m) = (a.len() as isize, b.len() as isize);
    let max_d = (n + m + 1) / 2;
    let offset = max_d;
    let len = (2 * max_d + 2) as usize;
    let mut v1 = vec![-1isize; len];
    let mut v2 = vec![-1isize; len];
    v1[(offset + 1) as usize] = 0;
    v2[(offset + 1) as usize] = 0;
    let delta = n - m;
    let front = delta % 2 != 0;
    let (mut k1start, mut k1end, mut k2start, mut k2end) = (0, 0, 0, 0);
    for d in 0..max_d {
        if d % 64 == 63 && !keep_going() {
            return None;
        }
        let mut k1 = -d + k1start;
        while k1 <= d - k1end {
            let k1o = (offset + k1) as usize;
            let mut x1 = if k1 == -d || (k1 != d && v1[k1o - 1] < v1[k1o + 1]) { v1[k1o + 1] } else { v1[k1o - 1] + 1 };
            let mut y1 = x1 - k1;
            while x1 < n && y1 < m && a[x1 as usize] == b[y1 as usize] {
                x1 += 1;
                y1 += 1;
            }
            v1[k1o] = x1;
            if x1 > n {
                k1end += 2;
            } else if y1 > m {
                k1start += 2;
            } else if front {
                let k2o = offset + delta - k1;
                if k2o >= 0 && (k2o as usize) < len && v2[k2o as usize] != -1 {
                    let x2 = n - v2[k2o as usize];
                    if x1 >= x2 {
                        return Some((x1 as usize, y1 as usize));
                    }
                }
            }
            k1 += 2;
        }
        let mut k2 = -d + k2start;
        while k2 <= d - k2end {
            let k2o = (offset + k2) as usize;
            let mut x2 = if k2 == -d || (k2 != d && v2[k2o - 1] < v2[k2o + 1]) { v2[k2o + 1] } else { v2[k2o - 1] + 1 };
            let mut y2 = x2 - k2;
            while x2 < n && y2 < m && a[(n - x2 - 1) as usize] == b[(m - y2 - 1) as usize] {
                x2 += 1;
                y2 += 1;
            }
            v2[k2o] = x2;
            if x2 > n {
                k2end += 2;
            } else if y2 > m {
                k2start += 2;
            } else if !front {
                let k1o = offset + delta - k2;
                if k1o >= 0 && (k1o as usize) < len && v1[k1o as usize] != -1 {
                    let x1 = v1[k1o as usize];
                    let y1 = offset + x1 - k1o;
                    if x1 >= n - x2 {
                        return Some((x1 as usize, y1 as usize));
                    }
                }
            }
            k2 += 2;
        }
    }
    None
}

/// Merges adjacent ops into changed blocks, sorted.
fn blocks(ops: &[Op], _old_len: usize, _new_len: usize) -> Vec<LineChange> {
    let mut sorted = ops.to_vec();
    sorted.sort_by_key(|o| (o.0, o.2));
    let mut out: Vec<LineChange> = Vec::new();
    for (ao, al, bo, bl) in sorted {
        match out.last_mut() {
            Some(last) if last.old.end == ao && last.new.end == bo => {
                last.old.end = ao + al;
                last.new.end = bo + bl;
            }
            _ => out.push(LineChange { old: ao..ao + al, new: bo..bo + bl, inner: Vec::new(), moved: None }),
        }
    }
    out
}

/// The char-level differences inside a changed block.
fn inner(
    old: &Rope,
    new: &Rope,
    c: &LineChange,
    keep_going: &mut dyn FnMut() -> bool,
    complete: &mut bool,
) -> Vec<CharChange> {
    let span = |rope: &Rope, lines: &Range<usize>| {
        rope.line_to_char(lines.start)..rope.line_to_char(lines.end.min(rope.len_lines()))
    };
    let (os, ns) = (span(old, &c.old), span(new, &c.new));
    if os.len() > MAX_INNER_CHARS || ns.len() > MAX_INNER_CHARS {
        return vec![CharChange { old: os, new: ns }];
    }
    let a: Vec<u32> = old.slice(os.clone()).chars().map(|ch| ch as u32).collect();
    let b: Vec<u32> = new.slice(ns.clone()).chars().map(|ch| ch as u32).collect();
    let mut ops = Vec::new();
    myers(&a, &b, 0, 0, &mut ops, keep_going, complete);
    let mut merged = blocks(&ops, a.len(), b.len());
    // Equal runs of one or two chars between changes read as noise: join across them.
    let mut joined: Vec<LineChange> = Vec::new();
    for ch in merged.drain(..) {
        match joined.last_mut() {
            Some(last) if ch.old.start - last.old.end <= 2 && ch.new.start - last.new.end <= 2 => {
                last.old.end = ch.old.end;
                last.new.end = ch.new.end;
            }
            _ => joined.push(ch),
        }
    }
    joined
        .into_iter()
        .map(|ch| CharChange {
            old: os.start + ch.old.start..os.start + ch.old.end,
            new: ns.start + ch.new.start..ns.start + ch.new.end,
        })
        .collect()
}

/// Pairs pure deletions with pure insertions of the same lines (moves).
fn pair_moves(changes: &mut [LineChange], a: &[u32], b: &[u32]) {
    let mut inserted: HashMap<&[u32], usize> = HashMap::new();
    for (i, c) in changes.iter().enumerate() {
        if c.old.is_empty() && c.new.len() >= MIN_MOVED_LINES {
            inserted.entry(&b[c.new.clone()]).or_insert(i);
        }
    }
    let mut pairs = Vec::new();
    for (i, c) in changes.iter().enumerate() {
        if c.new.is_empty()
            && c.old.len() >= MIN_MOVED_LINES
            && let Some(&j) = inserted.get(&a[c.old.clone()])
        {
            pairs.push((i, j));
            inserted.remove(&a[c.old.clone()]);
        }
    }
    for (i, j) in pairs {
        changes[i].moved = Some(j);
        changes[j].moved = Some(i);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(a: &str, b: &str) -> Diff {
        diff(&Rope::from_str(a), &Rope::from_str(b), DiffOptions::default(), &mut || true)
    }

    /// Applies the line changes to `a` and checks the result is `b`.
    fn check(a: &str, b: &str) {
        let diff = d(a, b);
        apply(a, b, &diff);
    }

    fn apply(a: &str, b: &str, diff: &Diff) {
        let al: Vec<&str> = a.split_inclusive('\n').collect();
        let bl: Vec<&str> = b.split_inclusive('\n').collect();
        let mut out: Vec<&str> = Vec::new();
        let mut at = 0;
        for c in &diff.changes {
            out.extend(&al[at..c.old.start.min(al.len())]);
            out.extend(&bl[c.new.start.min(bl.len())..c.new.end.min(bl.len())]);
            at = c.old.end.min(al.len());
        }
        out.extend(&al[at.min(al.len())..]);
        assert_eq!(out.concat(), b, "diff of {a:?} -> {b:?}: {:?}", diff.changes);
    }

    #[test]
    fn line_changes_rebuild_the_new_text() {
        check("a\nb\nc\n", "a\nx\nc\n");
        check("a\nb\nc\n", "a\nc\n");
        check("a\nc\n", "a\nb\nc\n");
        check("", "x\ny\n");
        check("x\ny\n", "");
        check("1\n2\n3\n4\n5\n6\n", "0\n2\n3\nx\n5\n6\n7\n");
    }

    #[test]
    fn random_texts_round_trip() {
        let mut seed = 0x7a3c_51d2_u64;
        let mut rand = move |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n as u64) as usize
        };
        for _ in 0..200 {
            let a: String = (0..rand(30)).map(|_| format!("{}\n", rand(6))).collect();
            let b: String = (0..rand(30)).map(|_| format!("{}\n", rand(6))).collect();
            check(&a, &b);
        }
    }

    #[test]
    fn chars_inside_a_changed_line_and_whitespace() {
        let lines = d("let value = 1;\n", "let total = 1;\n");
        assert_eq!(lines.changes.len(), 1);
        assert_eq!(lines.changes[0].inner, vec![CharChange { old: 4..9, new: 4..9 }]);
        let ws = diff(
            &Rope::from_str("a  b\n"),
            &Rope::from_str("a b\n"),
            DiffOptions { ignore_whitespace: true },
            &mut || true,
        );
        assert!(ws.changes.is_empty());
    }

    #[test]
    fn moved_blocks_are_paired() {
        let a = "m1\nm2\nm3\nv\nw\nx\ny\nz\n";
        let b = "v\nw\nx\ny\nz\nm1\nm2\nm3\n";
        let diff = d(a, b);
        let moved: Vec<Option<usize>> = diff.changes.iter().map(|c| c.moved).collect();
        assert!(moved.iter().all(Option::is_some), "{:?}", diff.changes);
    }

    #[test]
    fn a_stop_gives_a_coarse_but_correct_diff() {
        let a: String = (0..2000).map(|i| format!("{i}\n")).collect();
        let b: String = (0..2000).map(|i| if i % 7 == 0 { "x\n".to_owned() } else { format!("{i}\n") }).collect();
        let mut calls = 0;
        let coarse = diff(&Rope::from_str(&a), &Rope::from_str(&b), DiffOptions::default(), &mut || {
            calls += 1;
            calls < 3
        });
        assert!(!coarse.complete);
        // Coarser, but it still turns one text into the other.
        apply(&a, &b, &coarse);
    }
}
