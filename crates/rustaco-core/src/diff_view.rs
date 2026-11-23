//! What a diff view needs from a [`Diff`](crate::Diff), independent of how it draws: the
//! ranges to mark, the filler blocks that keep two panes aligned, stepping between changes,
//! the edit that reverts a change, and the unchanged stretches worth collapsing. Offsets are
//! chars, lines are 0-based.

use std::ops::Range;

use ropey::Rope;

use crate::diff::LineChange;

/// What a marked range is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffMark {
    /// A whole line of the original that was removed or replaced.
    RemovedLine,
    /// A whole line of the modified text that was inserted or replaces others.
    InsertedLine,
    /// Chars inside a changed line of the original.
    RemovedText,
    /// Chars inside a changed line of the modified text.
    InsertedText,
}

/// From the start of `lines`' first line to the end of its last line, without the line
/// break.
fn line_span(rope: &Rope, lines: &Range<usize>) -> Range<usize> {
    let last_line = rope.len_lines().saturating_sub(1);
    let first = lines.start.min(last_line);
    let last = lines.end.saturating_sub(1).max(lines.start).min(last_line);
    rope.line_to_char(first)..rope.line_to_char(last) + crate::line_len(rope, last)
}

/// The ranges to mark: the original's in `old`, the modified text's in `new`. With
/// `inline` the original is not shown, so it gets no marks.
pub fn marks(old: &Rope, new: &Rope, changes: &[LineChange], inline: bool) -> Vec<(DiffMark, Range<usize>)> {
    let mut out = Vec::new();
    for c in changes {
        if !c.old.is_empty() && !inline {
            out.push((DiffMark::RemovedLine, line_span(old, &c.old)));
        }
        if !c.new.is_empty() {
            out.push((DiffMark::InsertedLine, line_span(new, &c.new)));
        }
        for i in &c.inner {
            if !i.old.is_empty() && !inline {
                out.push((DiffMark::RemovedText, i.old.clone()));
            }
            if !i.new.is_empty() {
                out.push((DiffMark::InsertedText, i.new.clone()));
            }
        }
    }
    out
}

/// A block of empty lines in one pane, so the panes stay aligned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Filler {
    /// True for the modified pane, false for the original.
    pub in_modified: bool,
    /// The line the block follows (-1: before the first line).
    pub after_line: isize,
    pub lines: usize,
}

/// Side by side, the shorter side of each change gets the missing lines.
pub fn fillers(changes: &[LineChange]) -> Vec<Filler> {
    changes
        .iter()
        .filter_map(|c| {
            let (old, new) = (c.old.len() as isize, c.new.len() as isize);
            match old.cmp(&new) {
                std::cmp::Ordering::Greater => {
                    Some(Filler { in_modified: true, after_line: c.new.end as isize - 1, lines: (old - new) as usize })
                }
                std::cmp::Ordering::Less => {
                    Some(Filler { in_modified: false, after_line: c.old.end as isize - 1, lines: (new - old) as usize })
                }
                std::cmp::Ordering::Equal => None,
            }
        })
        .collect()
}

/// The first line of the next change after `line` in the modified text (or the previous one
/// before it), wrapping around; None without changes.
pub fn next_change(changes: &[LineChange], line: usize, forward: bool) -> Option<usize> {
    let starts = changes.iter().map(|c| c.new.start);
    if forward {
        starts.clone().find(|&s| s > line).or_else(|| starts.clone().next())
    } else {
        starts.clone().rev().find(|&s| s < line).or_else(|| starts.clone().next_back())
    }
}

/// From the start of `lines` to the start of the line after them (the end of the text for
/// the last lines).
fn block_span(rope: &Rope, lines: &Range<usize>) -> Range<usize> {
    let line_count = rope.len_lines();
    let at = |line: usize| if line >= line_count { rope.len_chars() } else { rope.line_to_char(line) };
    let start = at(lines.start);
    start..at(lines.end).max(start)
}

/// True when the text does not end with a line break.
fn open_ended(rope: &Rope) -> bool {
    rope.len_chars() > 0 && rope.char(rope.len_chars() - 1) != '\n'
}

/// The edit to the modified text that puts change `c`'s original lines back.
pub fn revert_edit(old: &Rope, new: &Rope, c: &LineChange) -> (Range<usize>, String) {
    let from = block_span(old, &c.old);
    let mut text = old.slice(from.clone()).to_string();
    let mut to = block_span(new, &c.new);
    // At the end of a text without a final line break, the break between the last two
    // lines belongs to the block: reverting an addition takes it away, reverting a deletion
    // brings it back before the lines.
    if from.is_empty()
        && from.start == old.len_chars()
        && open_ended(old)
        && to.start > 0
        && new.char(to.start - 1) == '\n'
    {
        to.start -= 1;
    } else if to.is_empty() && to.start == new.len_chars() && open_ended(new) {
        if text.ends_with('\n') {
            text.pop();
        }
        text.insert(0, '\n');
    }
    (to, text)
}

/// Unchanged stretches longer than `2 * context` lines on one side (`in_modified` picks it),
/// as (first, last) lines to fold, keeping `context` lines around each change.
pub fn unchanged_regions(
    changes: &[LineChange],
    in_modified: bool,
    line_count: usize,
    context: usize,
) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut at = 0;
    for c in changes {
        let lines = if in_modified { &c.new } else { &c.old };
        if lines.start.saturating_sub(at) > 2 * context {
            // At the top of the file there is no change above to keep context for.
            let first = if at == 0 { at } else { at + context };
            out.push((first, lines.start - context - 1));
        }
        at = lines.end;
    }
    let last = line_count.saturating_sub(1);
    if last.saturating_sub(at) > 2 * context {
        out.push((at + context, last));
    }
    out.retain(|(a, b)| b > a);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DiffOptions, diff};

    fn changes(a: &str, b: &str) -> (Rope, Rope, Vec<LineChange>) {
        let (a, b) = (Rope::from_str(a), Rope::from_str(b));
        let d = diff(&a, &b, DiffOptions::default(), &mut || true);
        (a, b, d.changes)
    }

    #[test]
    fn marks_fillers_and_steps() {
        let (a, b, c) = changes("one\ntwo\nthree\nfour\n", "one\n2\nthree\nfour\nfive\nsix\n");
        let m = marks(&a, &b, &c, false);
        assert!(m.contains(&(DiffMark::RemovedLine, 4..7)));
        assert!(m.contains(&(DiffMark::InsertedLine, 4..5)));
        assert!(
            marks(&a, &b, &c, true).iter().all(|(k, _)| matches!(k, DiffMark::InsertedLine | DiffMark::InsertedText))
        );
        // Two lines were added at the end: the original pane gets a two-line filler.
        assert!(fillers(&c).contains(&Filler { in_modified: false, after_line: 3, lines: 2 }));
        assert_eq!(next_change(&c, 0, true), Some(1));
        assert_eq!(next_change(&c, 1, true), Some(4));
        assert_eq!(next_change(&c, 4, true), Some(1));
        assert_eq!(next_change(&c, 1, false), Some(4));
        assert_eq!(next_change(&[], 1, false), None);
    }

    #[test]
    fn revert_puts_the_original_back_in_chars() {
        // An emoji before the change: offsets are chars, not UTF-16 units.
        let (a, b, c) = changes("😀\nold\nend", "😀\nnew\nend");
        let (range, text) = revert_edit(&a, &b, &c[0]);
        assert_eq!((range, text.as_str()), (2..6, "old\n"));
        // Every change reverted gives the original back, also at the end of texts with and
        // without a final line break.
        for (before, after) in [
            ("a\nb", "a\nb\nc"),
            ("a\nb\nc", "a\nb"),
            ("a\nb\n", "a\nb\nc\n"),
            ("a\nb\nc\n", "a\nb\n"),
            ("a\nb", "a\nB\nc"),
            ("x", ""),
            ("", "x\ny"),
        ] {
            let (a, b, c) = changes(before, after);
            let mut reverted = b.clone();
            for change in c.iter().rev() {
                let (range, text) = revert_edit(&a, &reverted, change);
                reverted.remove(range.clone());
                reverted.insert(range.start, &text);
            }
            assert_eq!(reverted.to_string(), before, "reverting {after:?}");
        }
    }

    #[test]
    fn collapsible_stretches_keep_context() {
        let a: String = (0..30).map(|i| format!("{i}\n")).collect();
        let b = a.replace("15\n", "fifteen\n");
        let (_, _, c) = changes(&a, &b);
        assert_eq!(unchanged_regions(&c, true, 31, 3), vec![(0, 11), (19, 30)]);
    }
}
