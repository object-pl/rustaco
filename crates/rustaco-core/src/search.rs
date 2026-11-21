//! Find and replace: literal or regular-expression search over a model, with case and
//! whole-word options and capture groups in replacements.
//!
//! Regular expressions use `regex-lite`, which keeps the browser bundle small; it supports
//! the usual syntax but not Unicode classes such as `\p{L}` (`\w` and `\b` are ASCII).

use std::ops::Range;

use regex_lite::{Regex, RegexBuilder};
use ropey::Rope;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchQuery {
    pub pattern: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
}

/// A compiled query.
pub struct Searcher {
    regex: Regex,
}

impl Searcher {
    pub fn new(query: &SearchQuery) -> Result<Searcher, String> {
        if query.pattern.is_empty() {
            return Err("empty search".into());
        }
        let mut pattern = if query.regex { query.pattern.clone() } else { regex_lite::escape(&query.pattern) };
        if query.whole_word {
            pattern = format!(r"\b(?:{pattern})\b");
        }
        let regex = RegexBuilder::new(&pattern)
            .case_insensitive(!query.case_sensitive)
            .multi_line(true)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Searcher { regex })
    }

    /// Every match in `rope` (or inside `within`), as char ranges. Empty matches are skipped.
    pub fn find_all(&self, rope: &Rope, within: Option<Range<usize>>) -> Vec<Range<usize>> {
        let text = rope.to_string();
        let (base_char, slice) = match within {
            Some(r) => {
                let (a, b) =
                    (rope.char_to_byte(r.start.min(rope.len_chars())), rope.char_to_byte(r.end.min(rope.len_chars())));
                (r.start, &text[a..b])
            }
            None => (0, text.as_str()),
        };
        let mut out = Vec::new();
        let mut chars_before = 0;
        let mut last_byte = 0;
        for m in self.regex.find_iter(slice) {
            if m.start() == m.end() {
                continue;
            }
            chars_before += slice[last_byte..m.start()].chars().count();
            let len = m.as_str().chars().count();
            out.push(base_char + chars_before..base_char + chars_before + len);
            chars_before += len;
            last_byte = m.end();
        }
        out
    }

    /// The replacement text for the match at `range`: `$1`, `${name}` refer to capture groups
    /// in regex mode; a literal query inserts `replacement` as is.
    pub fn replacement_for(&self, rope: &Rope, range: Range<usize>, replacement: &str, regex_mode: bool) -> String {
        if !regex_mode {
            return replacement.to_owned();
        }
        let text = rope.slice(range).to_string();
        match self.regex.captures(&text) {
            Some(caps) => {
                let mut out = String::new();
                caps.expand(replacement, &mut out);
                out
            }
            None => replacement.to_owned(),
        }
    }
}

/// The match after (or before) `from` in a sorted list of matches, wrapping around.
pub fn next_match(matches: &[Range<usize>], from: usize, forward: bool) -> Option<Range<usize>> {
    if matches.is_empty() {
        return None;
    }
    if forward {
        let i = matches.partition_point(|m| m.start < from);
        Some(matches.get(i).unwrap_or(&matches[0]).clone())
    } else {
        let i = matches.partition_point(|m| m.end < from);
        let i = if i == 0 { matches.len() - 1 } else { i - 1 };
        Some(matches[i].clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(text: &str, q: SearchQuery) -> Vec<Range<usize>> {
        Searcher::new(&q).unwrap().find_all(&Rope::from_str(text), None)
    }

    #[test]
    fn literal_case_and_whole_word() {
        let q = |p: &str, case, word| SearchQuery {
            pattern: p.into(),
            regex: false,
            case_sensitive: case,
            whole_word: word,
        };
        assert_eq!(find("Foo foo food", q("foo", false, false)), [0..3, 4..7, 8..11]);
        assert_eq!(find("Foo foo food", q("foo", true, false)), [4..7, 8..11]);
        assert_eq!(find("Foo foo food", q("foo", false, true)), [0..3, 4..7]);
        let literal = find("a.b axb", q("a.b", false, false));
        assert_eq!((literal.len(), literal[0].clone()), (1, 0..3), "literal mode escapes regex syntax");
    }

    #[test]
    fn regex_positions_count_chars_not_bytes() {
        let q = SearchQuery { pattern: r"\d+".into(), regex: true, case_sensitive: true, whole_word: false };
        assert_eq!(find("é1 😀22", q), [1..2, 4..6]);
    }

    #[test]
    fn replacement_with_groups_and_wrapping_navigation() {
        let rope = Rope::from_str("key=value");
        let q = SearchQuery { pattern: r"(\w+)=(\w+)".into(), regex: true, case_sensitive: true, whole_word: false };
        let s = Searcher::new(&q).unwrap();
        assert_eq!(s.replacement_for(&rope, 0..9, "$2=$1", true), "value=key");
        let ms = vec![2..4, 10..12];
        assert_eq!(next_match(&ms, 5, true), Some(10..12));
        assert_eq!(next_match(&ms, 11, true), Some(2..4), "wraps to the first match");
        assert_eq!(next_match(&ms, 3, false), Some(10..12), "wraps to the last match");
        assert!(Searcher::new(&SearchQuery { pattern: "(".into(), regex: true, ..Default::default() }).is_err());
    }
}
