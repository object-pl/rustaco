//! Editing commands beyond typing and cursor movement: line operations, indentation,
//! comments, bracket pairs, occurrence selection, case changes.

use std::ops::Range;

use ropey::Rope;
use unicode_segmentation::UnicodeSegmentation;

use crate::change::{Assoc, ChangeSet, Edit};
use crate::editor::{Editor, word_at};
use crate::history::EditKind;
use crate::position::{char_to_position, line_end, line_len};
use crate::search::Searcher;
use crate::selection::{Selection, Selections};

/// Chars on each side of a position examined to find grapheme boundaries. Clusters longer
/// than this (long emoji ZWJ sequences, stacked combining marks) are split at the window edge.
const GRAPHEME_WINDOW: usize = 64;

/// The grapheme cluster boundary before `pos` (`\r\n` and emoji sequences count as one).
pub(crate) fn prev_grapheme(rope: &Rope, pos: usize) -> usize {
    if pos == 0 {
        return 0;
    }
    let start = pos.saturating_sub(GRAPHEME_WINDOW);
    let window = rope.slice(start..pos).to_string();
    let last = window.grapheme_indices(true).next_back().map_or(0, |(i, _)| i);
    start + window[..last].chars().count()
}

/// The grapheme cluster boundary after `pos`.
pub(crate) fn next_grapheme(rope: &Rope, pos: usize) -> usize {
    let len = rope.len_chars();
    if pos >= len {
        return len;
    }
    let end = (pos + GRAPHEME_WINDOW).min(len);
    let window = rope.slice(pos..end).to_string();
    let first = window.graphemes(true).next().map_or(0, |g| g.chars().count());
    pos + first.max(1)
}

/// Lines touched by the selections, merged into ranges. A selection ending at the start of a
/// line does not include that line, unless it is empty.
fn selected_line_ranges(rope: &Rope, selections: &Selections) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for s in selections.iter() {
        let first = rope.char_to_line(s.start());
        let mut last = rope.char_to_line(s.end());
        if !s.is_empty() && last > first && s.end() == rope.line_to_char(last) {
            last -= 1;
        }
        match ranges.last_mut() {
            Some(r) if first <= r.end => r.end = r.end.max(last + 1),
            _ => ranges.push(first..last + 1),
        }
    }
    ranges
}

fn indent_of(rope: &Rope, line: usize) -> usize {
    rope.line(line).chars().take(line_len(rope, line)).take_while(|c| *c == ' ' || *c == '\t').count()
}

fn is_blank(rope: &Rope, line: usize) -> bool {
    indent_of(rope, line) == line_len(rope, line)
}

/// Maps selections through a change, keeping a selection edge at the start of a line there
/// (so a whole-line selection keeps covering the whole line) and moving other edges past
/// inserted text.
fn map_keeping_line_starts(rope_before: &Rope, selections: &Selections, changes: &ChangeSet) -> Selections {
    let map = |p: usize| {
        let at_line_start = rope_before.line_to_char(rope_before.char_to_line(p)) == p;
        changes.map(p, if at_line_start { Assoc::Before } else { Assoc::After })
    };
    selections.map(|s| Selection { anchor: map(s.anchor), head: map(s.head), goal_column: None })
}

impl Editor<'_> {
    fn eol(&self) -> &'static str {
        self.model.line_ending.as_str()
    }

    fn indent_unit(&self) -> String {
        let o = self.model.options;
        if o.insert_spaces { " ".repeat(o.tab_size) } else { "\t".to_owned() }
    }

    /// Tab: indents the selected lines when a selection spans lines, otherwise inserts
    /// indentation at each cursor.
    pub fn tab(&mut self) {
        let rope = &self.model.rope;
        let multi_line =
            self.cursors.selections.iter().any(|s| rope.char_to_line(s.start()) != rope.char_to_line(s.end()));
        if multi_line { self.indent_lines() } else { self.indent() }
    }

    /// Adds one indentation unit at the start of every selected non-blank line.
    pub fn indent_lines(&mut self) {
        let rope = self.model.rope.clone();
        let unit = self.indent_unit();
        let edits = selected_line_ranges(&rope, &self.cursors.selections)
            .into_iter()
            .flatten()
            .filter(|&l| !is_blank(&rope, l))
            .map(|l| Edit::insert(rope.line_to_char(l), unit.clone()))
            .collect();
        let changes = ChangeSet::new(edits);
        let after = map_keeping_line_starts(&rope, &self.cursors.selections, &changes);
        self.commit(changes, after, EditKind::Other);
    }

    /// Shift+Tab: removes up to one indentation unit from the start of every selected line.
    pub fn outdent_lines(&mut self) {
        let rope = self.model.rope.clone();
        let tab = self.model.options.tab_size;
        let edits = selected_line_ranges(&rope, &self.cursors.selections)
            .into_iter()
            .flatten()
            .filter_map(|l| {
                let start = rope.line_to_char(l);
                let mut n = 0;
                for c in rope.line(l).chars().take(tab) {
                    match c {
                        ' ' => n += 1,
                        '\t' => {
                            n += 1;
                            break;
                        }
                        _ => break,
                    }
                }
                (n > 0).then(|| Edit::delete(start, start + n))
            })
            .collect();
        let changes = ChangeSet::new(edits);
        let after = self.cursors.selections.map(|s| Selection {
            anchor: changes.map(s.anchor, Assoc::Before),
            head: changes.map(s.head, Assoc::Before),
            goal_column: None,
        });
        self.commit(changes, after, EditKind::Other);
    }

    /// Alt+Up / Alt+Down: moves the selected lines past the line above or below.
    pub fn move_lines(&mut self, up: bool) {
        let rope = self.model.rope.clone();
        let total = rope.len_lines();
        let eol = self.eol();
        let mut edits = Vec::new();
        let mut moves: Vec<(Range<usize>, isize, Range<usize>, isize)> = Vec::new();
        for block in selected_line_ranges(&rope, &self.cursors.selections) {
            let (region, other) = if up {
                if block.start == 0 {
                    continue;
                }
                (block.start - 1..block.end, block.start - 1)
            } else {
                if block.end >= total {
                    continue;
                }
                (block.start..block.end + 1, block.end)
            };
            let text = |l: usize| rope.line(l).slice(..line_len(&rope, l)).to_string();
            let block_text: Vec<String> = block.clone().map(text).collect();
            let other_text = text(other);
            let mut lines = block_text.clone();
            if up {
                lines.push(other_text.clone())
            } else {
                lines.insert(0, other_text.clone())
            }
            let start = rope.line_to_char(region.start);
            let end = line_end(&rope, region.end - 1);
            edits.push(Edit::new(start, end, lines.join(eol)));
            // Chars inside the block shift by the other line's length (plus a line break),
            // chars of the other line by the block's length.
            let eol_len = eol.chars().count() as isize;
            let other_len = other_text.chars().count() as isize + eol_len;
            let block_len = block_text.iter().map(|l| l.chars().count() as isize + eol_len).sum::<isize>();
            let block_chars = rope.line_to_char(block.start)..line_end(&rope, block.end - 1);
            let other_chars = rope.line_to_char(other)..line_end(&rope, other);
            if up {
                moves.push((block_chars, -other_len, other_chars, block_len));
            } else {
                moves.push((block_chars, other_len, other_chars, -block_len));
            }
        }
        if edits.is_empty() {
            return;
        }
        let shift = |p: usize| {
            for (block, db, other, dother) in &moves {
                if block.contains(&p) || p == block.end {
                    return p.wrapping_add_signed(*db);
                }
                if other.contains(&p) || p == other.end {
                    return p.wrapping_add_signed(*dother);
                }
            }
            p
        };
        let after = self.cursors.selections.map(|s| Selection {
            anchor: shift(s.anchor),
            head: shift(s.head),
            goal_column: s.goal_column,
        });
        self.commit(ChangeSet::new(edits), after, EditKind::Other);
    }

    /// Shift+Alt+Up / Shift+Alt+Down: duplicates the selected lines above or below; the
    /// selections end up on the copy.
    pub fn copy_lines(&mut self, up: bool) {
        let rope = self.model.rope.clone();
        let eol = self.eol();
        let eol_len = eol.chars().count();
        let mut edits = Vec::new();
        let mut shifts: Vec<(Range<usize>, usize)> = Vec::new();
        for block in selected_line_ranges(&rope, &self.cursors.selections) {
            let start = rope.line_to_char(block.start);
            let end = line_end(&rope, block.end - 1);
            let text = rope.slice(start..end).to_string();
            if up {
                edits.push(Edit::insert(start, format!("{text}{eol}")));
            } else {
                shifts.push((start..end + 1, end - start + eol_len));
                edits.push(Edit::insert(end, format!("{eol}{text}")));
            }
        }
        let changes = ChangeSet::new(edits);
        let after = self.cursors.selections.map(|s| {
            let map = |p: usize| {
                let base = changes.map(p, Assoc::Before);
                shifts.iter().find(|(r, _)| r.contains(&p)).map_or(base, |(_, d)| base + d)
            };
            Selection { anchor: map(s.anchor), head: map(s.head), goal_column: None }
        });
        self.commit(changes, after, EditKind::Other);
    }

    /// Ctrl+Shift+K: deletes the selected lines; cursors keep their column on the next line.
    pub fn delete_lines(&mut self) {
        let rope = self.model.rope.clone();
        let total = rope.len_lines();
        let mut edits = Vec::new();
        for block in selected_line_ranges(&rope, &self.cursors.selections) {
            let (start, end) = if block.end < total {
                (rope.line_to_char(block.start), rope.line_to_char(block.end))
            } else if block.start > 0 {
                (line_end(&rope, block.start - 1), rope.len_chars())
            } else {
                (0, rope.len_chars())
            };
            edits.push(Edit::delete(start, end));
        }
        let changes = ChangeSet::new(edits);
        let columns: Vec<usize> =
            self.cursors.selections.iter().map(|s| char_to_position(&rope, s.head).column).collect();
        let mut after_rope = rope.clone();
        changes.apply(&mut after_rope);
        let mut i = 0;
        let after = self.cursors.selections.map(|s| {
            let p = changes.map(s.head, Assoc::Before);
            let line = after_rope.char_to_line(p.min(after_rope.len_chars()));
            let col = columns[i].min(line_len(&after_rope, line));
            i += 1;
            Selection::cursor(after_rope.line_to_char(line) + col)
        });
        self.commit(changes, after, EditKind::Other);
    }

    /// Ctrl+Enter (below) / Ctrl+Shift+Enter (above): opens a new line with the current
    /// line's indentation and moves the cursor onto it.
    pub fn insert_line(&mut self, below: bool) {
        let rope = self.model.rope.clone();
        let eol = self.eol();
        let mut edits = Vec::new();
        let mut carets = Vec::new();
        let mut delta = 0isize;
        let mut lines: Vec<usize> = self.cursors.selections.iter().map(|s| rope.char_to_line(s.head)).collect();
        lines.dedup();
        for line in lines {
            let indent: String = rope.line(line).chars().take(indent_of(&rope, line)).collect();
            let indent_len = indent.chars().count();
            let eol_len = eol.chars().count();
            if below {
                let at = line_end(&rope, line);
                edits.push(Edit::insert(at, format!("{eol}{indent}")));
                carets.push(Selection::cursor(at.wrapping_add_signed(delta) + eol_len + indent_len));
            } else {
                let at = rope.line_to_char(line);
                edits.push(Edit::insert(at, format!("{indent}{eol}")));
                carets.push(Selection::cursor(at.wrapping_add_signed(delta) + indent_len));
            }
            delta += (indent_len + eol_len) as isize;
        }
        let after = Selections::new(carets, 0);
        self.commit(ChangeSet::new(edits), after, EditKind::Other);
    }

    /// Ctrl+/: comments out the selected lines with the language's line comment token, or
    /// uncomments them when every non-blank one is already commented. Falls back to a block
    /// comment for languages without line comments.
    pub fn toggle_line_comment(&mut self) {
        let Some(token) = self.model.editing.line_comment.clone() else {
            self.toggle_block_comment();
            return;
        };
        let rope = self.model.rope.clone();
        let lines: Vec<usize> = selected_line_ranges(&rope, &self.cursors.selections)
            .into_iter()
            .flatten()
            .filter(|&l| !is_blank(&rope, l))
            .collect();
        if lines.is_empty() {
            return;
        }
        let token_len = token.chars().count();
        let starts_with_token = |l: usize| {
            let start = rope.line_to_char(l) + indent_of(&rope, l);
            start + token_len <= rope.len_chars() && rope.slice(start..start + token_len) == token.as_str()
        };
        let edits = if lines.iter().all(|&l| starts_with_token(l)) {
            lines
                .iter()
                .map(|&l| {
                    let start = rope.line_to_char(l) + indent_of(&rope, l);
                    let space = rope.get_char(start + token_len) == Some(' ');
                    Edit::delete(start, start + token_len + usize::from(space))
                })
                .collect()
        } else {
            let column = lines.iter().map(|&l| indent_of(&rope, l)).min().unwrap_or(0);
            lines.iter().map(|&l| Edit::insert(rope.line_to_char(l) + column, format!("{token} "))).collect()
        };
        let changes = ChangeSet::new(edits);
        let after = map_keeping_line_starts(&rope, &self.cursors.selections, &changes);
        self.commit(changes, after, EditKind::Other);
    }

    /// Shift+Alt+A: wraps each selection in the language's block comment tokens, keeping the
    /// selection on the wrapped text, or removes the tokens when they sit just inside or just
    /// outside the selection.
    pub fn toggle_block_comment(&mut self) {
        let Some((open, close)) = self.model.editing.block_comment.clone() else { return };
        let rope = self.model.rope.clone();
        let (open_len, close_len) = (open.chars().count(), close.chars().count());
        let len = rope.len_chars();
        let matches = |at: usize, token: &str, n: usize| at + n <= len && rope.slice(at..at + n) == token;
        let mut edits = Vec::new();
        let mut wrapped = Vec::new();
        for s in self.cursors.selections.iter() {
            // Tokens just outside the selection, optionally separated by one space.
            let before = |gap: usize| {
                s.start()
                    .checked_sub(open_len + gap)
                    .filter(|&at| matches(at, &open, open_len) && (gap == 0 || rope.char(s.start() - 1) == ' '))
            };
            let after = |gap: usize| {
                Some(s.end() + gap)
                    .filter(|&at| matches(at, &close, close_len) && (gap == 0 || rope.get_char(s.end()) == Some(' ')))
            };
            let outside = before(1)
                .zip(after(1))
                .map(|(o, c)| (o, open_len + 1, c - 1, close_len + 1))
                .or_else(|| before(0).zip(after(0)).map(|(o, c)| (o, open_len, c, close_len)));
            // Tokens just inside it.
            let text = rope.slice(s.start()..s.end()).to_string();
            let lead = text.chars().take_while(|c| c.is_whitespace()).count();
            let trail = text.chars().rev().take_while(|c| c.is_whitespace()).count();
            let inside = (text.trim().chars().count() >= open_len + close_len
                && matches(s.start() + lead, &open, open_len)
                && matches(s.end() - trail - close_len, &close, close_len))
            .then(|| {
                let open_at = s.start() + lead;
                let close_at = s.end() - trail - close_len;
                let space_open = rope.get_char(open_at + open_len) == Some(' ') && open_at + open_len < close_at;
                let space_close = close_at > open_at + open_len && rope.get_char(close_at - 1) == Some(' ');
                (
                    open_at,
                    open_len + usize::from(space_open),
                    close_at - usize::from(space_close),
                    close_len + usize::from(space_close),
                )
            });
            match outside.or(inside) {
                Some((open_at, open_n, close_at, close_n)) => {
                    edits.push(Edit::delete(open_at, open_at + open_n));
                    edits.push(Edit::delete(close_at, close_at + close_n));
                    wrapped.push(false);
                }
                None => {
                    edits.push(Edit::insert(s.start(), format!("{open} ")));
                    edits.push(Edit::insert(s.end(), format!(" {close}")));
                    wrapped.push(true);
                }
            }
        }
        let changes = ChangeSet::new(edits);
        let mut i = 0;
        let after = self.cursors.selections.map(|s| {
            let wrap = wrapped[i];
            i += 1;
            // After wrapping, the selection stays on the wrapped text, inside the tokens.
            let (start_assoc, end_assoc) =
                if wrap { (Assoc::After, Assoc::Before) } else { (Assoc::Before, Assoc::Before) };
            if s.is_empty() && wrap {
                // Both tokens went in at one point and merged into one edit; land between them.
                Selection::cursor(changes.map(s.head, Assoc::Before) + open_len + 1)
            } else if s.is_empty() {
                Selection::cursor(changes.map(s.head, start_assoc))
            } else {
                let (a, b) = (changes.map(s.start(), start_assoc), changes.map(s.end(), end_assoc));
                if s.is_backward() { Selection::new(b, a) } else { Selection::new(a, b) }
            }
        });
        self.commit(changes, after, EditKind::Other);
    }

    /// Types one char with bracket handling: an opening char of an auto-closing pair inserts
    /// the pair (or wraps a selection in it), a closing char steps over the same char after
    /// the cursor. Other chars are typed as usual.
    pub fn type_char(&mut self, c: char) {
        let pairs = self.model.editing.auto_closing.clone();
        let mut buf = [0u8; 4];
        let typed: &str = c.encode_utf8(&mut buf);
        let open_pair = pairs.iter().find(|(o, _)| o == typed).cloned();
        let closes = pairs.iter().any(|(_, cl)| cl == typed);
        if open_pair.is_none() && !closes {
            self.insert(typed);
            return;
        }
        let rope = self.model.rope.clone();
        let mut edits = Vec::new();
        let mut after = Vec::new();
        let mut delta = 0isize;
        for s in self.cursors.selections.iter() {
            let next = rope.get_char(s.head);
            let prev = s.head.checked_sub(1).and_then(|i| rope.get_char(i));
            let at = |p: usize, d: isize| p.wrapping_add_signed(d);
            if s.is_empty() && closes && next == Some(c) {
                // Step over the closing char that is already there.
                after.push(Selection::cursor(at(s.head, delta) + 1));
                continue;
            }
            match &open_pair {
                Some((open, close)) if !s.is_empty() => {
                    edits.push(Edit::insert(s.start(), open.clone()));
                    edits.push(Edit::insert(s.end(), close.clone()));
                    let (a, b) = (at(s.start(), delta) + 1, at(s.end(), delta) + 1);
                    after.push(if s.is_backward() { Selection::new(b, a) } else { Selection::new(a, b) });
                    delta += 2;
                }
                Some((open, close)) => {
                    let quote = open == close;
                    let next_ok = next.is_none_or(|n| {
                        n.is_whitespace() || pairs.iter().any(|(_, cl)| cl.starts_with(n)) || ",;".contains(n)
                    });
                    let prev_ok = !quote || prev.is_none_or(|p| !(p.is_alphanumeric() || p == '_' || p == c));
                    if next_ok && prev_ok {
                        edits.push(Edit::insert(s.head, format!("{open}{close}")));
                        after.push(Selection::cursor(at(s.head, delta) + 1));
                        delta += 2;
                    } else {
                        edits.push(Edit::insert(s.head, open.clone()));
                        after.push(Selection::cursor(at(s.head, delta) + 1));
                        delta += 1;
                    }
                }
                None => {
                    edits.push(Edit::new(s.start(), s.end(), typed));
                    after.push(Selection::cursor(at(s.start(), delta) + 1));
                    delta += 1 - s.len() as isize;
                }
            }
        }
        let after = Selections::new(after, self.cursors.selections.primary_index());
        self.commit(ChangeSet::new(edits), after, EditKind::Typing);
    }

    /// Backspace that also removes the closing half of an empty auto-closing pair.
    pub fn delete_backward_pair(&mut self) {
        let pairs = self.model.editing.auto_closing.clone();
        let rope = &self.model.rope;
        let in_empty_pair = |s: &Selection| {
            s.is_empty()
                && s.head > 0
                && pairs.iter().any(|(o, c)| {
                    let (o, c) = (o.chars().collect::<Vec<_>>(), c.chars().collect::<Vec<_>>());
                    o.len() == 1
                        && c.len() == 1
                        && rope.get_char(s.head - 1) == Some(o[0])
                        && rope.get_char(s.head) == Some(c[0])
                })
        };
        if !self.cursors.selections.iter().any(in_empty_pair) {
            self.delete_backward();
            return;
        }
        let flags: Vec<bool> = self.cursors.selections.iter().map(in_empty_pair).collect();
        let mut i = 0;
        self.delete_each_pub(EditKind::Deleting, |rope, s| {
            let pair = flags[i];
            i += 1;
            if !s.is_empty() {
                (s.start(), s.end())
            } else if pair {
                (s.head - 1, s.head + 1)
            } else {
                (prev_grapheme(rope, s.head), s.head)
            }
        });
    }

    /// Ctrl+D: selects the word at the cursor, or adds the next occurrence of the primary
    /// selection's text as a new selection (wrapping at the end of the document).
    pub fn add_next_occurrence(&mut self) {
        let rope = self.model.rope.clone();
        let primary = self.cursors.selections.primary();
        if primary.is_empty() {
            let (a, b) = word_at(&rope, primary.head);
            if a < b {
                let mut ranges = self.cursors.selections.as_slice().to_vec();
                ranges[self.cursors.selections.primary_index()] = Selection::new(a, b);
                self.set_selections(Selections::new(ranges, self.cursors.selections.primary_index()));
            }
            return;
        }
        let needle: String = rope.slice(primary.start()..primary.end()).to_string();
        let text = rope.to_string();
        let taken: Vec<(usize, usize)> = self.cursors.selections.iter().map(|s| (s.start(), s.end())).collect();
        let last_end = taken.iter().map(|&(_, e)| e).max().unwrap_or(0);
        let byte_of = |c: usize| rope.char_to_byte(c);
        let from = byte_of(last_end);
        let candidates = text[from..]
            .match_indices(&needle)
            .map(|(i, _)| i + from)
            .chain(text[..from].match_indices(&needle).map(|(i, _)| i));
        for b in candidates {
            let start = rope.byte_to_char(b);
            let end = start + primary.len();
            if !taken.iter().any(|&(s, e)| s < end && start < e) {
                let mut ranges = self.cursors.selections.as_slice().to_vec();
                ranges.push(Selection::new(start, end));
                let primary_index = ranges.len() - 1;
                self.set_selections(Selections::new(ranges, primary_index));
                return;
            }
        }
    }

    /// Ctrl+Shift+L: selects every occurrence of the primary selection's text (or of the word
    /// at the cursor).
    pub fn select_all_occurrences(&mut self) {
        let rope = self.model.rope.clone();
        let primary = self.cursors.selections.primary();
        let (a, b) = if primary.is_empty() { word_at(&rope, primary.head) } else { (primary.start(), primary.end()) };
        if a == b {
            return;
        }
        let needle: String = rope.slice(a..b).to_string();
        let text = rope.to_string();
        let len = b - a;
        let ranges: Vec<Selection> = text
            .match_indices(&needle)
            .map(|(i, _)| {
                let start = rope.byte_to_char(i);
                Selection::new(start, start + len)
            })
            .collect();
        let primary_index = ranges.iter().position(|s| s.start() == a).unwrap_or(0);
        self.set_selections(Selections::new(ranges, primary_index));
    }

    /// Converts the selected text (or the word at each cursor) to upper or lower case.
    pub fn transform_case(&mut self, upper: bool) {
        let rope = self.model.rope.clone();
        let mut edits = Vec::new();
        let mut after = Vec::new();
        let mut delta = 0isize;
        for s in self.cursors.selections.iter() {
            let (a, b) = if s.is_empty() { word_at(&rope, s.head) } else { (s.start(), s.end()) };
            let text = rope.slice(a..b).to_string();
            let new = if upper { text.to_uppercase() } else { text.to_lowercase() };
            let new_len = new.chars().count();
            let (na, nb) = (a.wrapping_add_signed(delta), a.wrapping_add_signed(delta) + new_len);
            after.push(if s.is_empty() {
                Selection::cursor(s.head.wrapping_add_signed(delta).min(nb))
            } else if s.is_backward() {
                Selection::new(nb, na)
            } else {
                Selection::new(na, nb)
            });
            delta += new_len as isize - (b - a) as isize;
            edits.push(Edit::new(a, b, new));
        }
        let after = Selections::new(after, self.cursors.selections.primary_index());
        self.commit(ChangeSet::new(edits), after, EditKind::Other);
    }

    /// Removes spaces and tabs at the end of every line.
    pub fn trim_trailing_whitespace(&mut self) {
        let rope = self.model.rope.clone();
        let edits = (0..rope.len_lines())
            .filter_map(|l| {
                let end = line_end(&rope, l);
                let start = rope.line_to_char(l);
                let mut from = end;
                while from > start && matches!(rope.char(from - 1), ' ' | '\t') {
                    from -= 1;
                }
                (from < end).then(|| Edit::delete(from, end))
            })
            .collect();
        let changes = ChangeSet::new(edits);
        let after = self.cursors.selections.map(|s| Selection {
            anchor: changes.map(s.anchor, Assoc::Before),
            head: changes.map(s.head, Assoc::Before),
            goal_column: None,
        });
        self.commit(changes, after, EditKind::Other);
    }
}

impl Editor<'_> {
    /// Replaces every match of `searcher` with `replacement` (capture groups in regex mode)
    /// as one undo step. Returns the number of replacements.
    pub fn replace_all(&mut self, searcher: &Searcher, replacement: &str, regex_mode: bool) -> usize {
        let rope = self.model.rope.clone();
        let matches = searcher.find_all(&rope, None);
        let edits: Vec<Edit> = matches
            .iter()
            .map(|m| Edit::new(m.start, m.end, searcher.replacement_for(&rope, m.clone(), replacement, regex_mode)))
            .collect();
        let count = edits.len();
        let changes = ChangeSet::new(edits);
        let after = self.cursors.selections.map(|s| Selection::cursor(changes.map(s.head, Assoc::Before)));
        self.commit(changes, after, EditKind::Other);
        count
    }

    /// Selects every match of `searcher` (Alt+Enter in the find widget).
    pub fn select_matches(&mut self, searcher: &Searcher) -> usize {
        let matches = searcher.find_all(&self.model.rope, None);
        if !matches.is_empty() {
            let ranges = matches.iter().map(|m| Selection::new(m.start, m.end)).collect();
            self.set_selections(Selections::new(ranges, 0));
        }
        matches.len()
    }
}

impl Editor<'_> {
    /// Runs a command by its id (VS Code's names). Returns false for ids the core does not
    /// handle; frontends run those themselves (page movement needs the view, clipboard
    /// commands need the platform clipboard).
    pub fn run(&mut self, command: &str) -> bool {
        match command {
            "cursorLeft" => self.move_left(false),
            "cursorLeftSelect" => self.move_left(true),
            "cursorRight" => self.move_right(false),
            "cursorRightSelect" => self.move_right(true),
            "cursorUp" => self.move_up(false),
            "cursorUpSelect" => self.move_up(true),
            "cursorDown" => self.move_down(false),
            "cursorDownSelect" => self.move_down(true),
            "cursorWordLeft" => self.move_word_left(false),
            "cursorWordLeftSelect" => self.move_word_left(true),
            "cursorWordRight" => self.move_word_right(false),
            "cursorWordRightSelect" => self.move_word_right(true),
            "cursorHome" => self.move_line_start(false),
            "cursorHomeSelect" => self.move_line_start(true),
            "cursorEnd" => self.move_line_end(false),
            "cursorEndSelect" => self.move_line_end(true),
            "cursorTop" => self.move_document_start(false),
            "cursorTopSelect" => self.move_document_start(true),
            "cursorBottom" => self.move_document_end(false),
            "cursorBottomSelect" => self.move_document_end(true),
            "deleteLeft" => self.delete_backward_pair(),
            "deleteRight" => self.delete_forward(),
            "deleteWordLeft" => self.delete_word_backward(),
            "deleteWordRight" => self.delete_word_forward(),
            "editor.action.newline" => self.newline(),
            "tab" => self.tab(),
            "outdent" | "editor.action.outdentLines" => self.outdent_lines(),
            "editor.action.indentLines" => self.indent_lines(),
            "editor.action.selectAll" => self.select_all(),
            "undo" => {
                self.undo();
            }
            "redo" => {
                self.redo();
            }
            "removeSecondaryCursors" => {
                let primary = self.cursors.selections.primary();
                self.set_selections(Selections::single(primary));
            }
            "cancelSelection" => {
                let primary = self.cursors.selections.primary();
                self.set_selections(Selections::single(Selection::cursor(primary.head)));
            }
            "cursorUndo" => {
                self.cursor_undo();
            }
            "editor.action.insertCursorAbove" => self.add_cursor_vertical(false),
            "editor.action.insertCursorBelow" => self.add_cursor_vertical(true),
            "editor.action.moveLinesUpAction" => self.move_lines(true),
            "editor.action.moveLinesDownAction" => self.move_lines(false),
            "editor.action.copyLinesUpAction" => self.copy_lines(true),
            "editor.action.copyLinesDownAction" => self.copy_lines(false),
            "editor.action.deleteLines" => self.delete_lines(),
            "editor.action.insertLineAfter" => self.insert_line(true),
            "editor.action.insertLineBefore" => self.insert_line(false),
            "editor.action.commentLine" => self.toggle_line_comment(),
            "editor.action.blockComment" => self.toggle_block_comment(),
            "editor.action.addSelectionToNextFindMatch" => self.add_next_occurrence(),
            "editor.action.selectHighlights" => self.select_all_occurrences(),
            "editor.action.transformToUppercase" => self.transform_case(true),
            "editor.action.transformToLowercase" => self.transform_case(false),
            "editor.action.trimTrailingWhitespace" => self.trim_trailing_whitespace(),
            _ => return false,
        }
        true
    }
}
