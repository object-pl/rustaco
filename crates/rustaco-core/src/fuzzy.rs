//! Fuzzy matching for completion lists, and collecting the words of a text.

use std::collections::HashSet;

use crate::Rope;

/// A fuzzy match: higher scores rank first; `matches` are the char indices of `word` that
/// matched, for highlighting.
#[derive(Clone, Debug, PartialEq)]
pub struct FuzzyMatch {
    pub score: f64,
    pub matches: Vec<usize>,
}

fn is_boundary(word: &[char], i: usize) -> bool {
    i == 0
        || matches!(word[i - 1], '_' | '-' | '.' | ' ' | '/' | ':')
        || (word[i].is_uppercase() && !word[i - 1].is_uppercase())
}

/// Scores `word` against what was typed, the way VS Code's completion filter does: every
/// char of `pattern` in order, case-insensitively, with the first one at the start of the
/// word or at a word boundary (after `_`, `-`, `.`, space, `/`, `:`, or a lower-to-upper
/// case change). Matches at the start, consecutive chars, boundaries and the same case
/// score higher. An empty pattern matches everything with score 0. `None` when it does not
/// match.
pub fn fuzzy_score(pattern: &str, word: &str) -> Option<FuzzyMatch> {
    if pattern.is_empty() {
        return Some(FuzzyMatch { score: 0.0, matches: Vec::new() });
    }
    let pat: Vec<char> = pattern.chars().collect();
    let wrd: Vec<char> = word.chars().collect();
    let lower = |c: char| c.to_lowercase().next().unwrap_or(c);
    let p: Vec<char> = pat.iter().map(|&c| lower(c)).collect();
    let w: Vec<char> = wrd.iter().map(|&c| lower(c)).collect();
    let mut best: Option<FuzzyMatch> = None;
    for first in 0..w.len() {
        if w[first] != p[0] || !is_boundary(&wrd, first) {
            continue;
        }
        let mut matches = vec![first];
        let mut score = (if first == 0 { 8.0 } else { 5.0 }) + if wrd[first] == pat[0] { 1.0 } else { 0.0 };
        let mut wi = first + 1;
        let mut ok = true;
        for pi in 1..p.len() {
            while wi < w.len() && w[wi] != p[pi] {
                wi += 1;
            }
            if wi >= w.len() {
                ok = false;
                break;
            }
            let consecutive = matches.last() == Some(&(wi - 1));
            score += 1.0
                + if consecutive { 3.0 } else { 0.0 }
                + if is_boundary(&wrd, wi) { 4.0 } else { 0.0 }
                + if wrd[wi] == pat[pi] { 1.0 } else { 0.0 };
            matches.push(wi);
            wi += 1;
        }
        if ok && best.as_ref().is_none_or(|b| score - first as f64 * 0.1 > b.score) {
            best = Some(FuzzyMatch { score: score - first as f64 * 0.1, matches });
        }
        if first == 0 && best.is_some() {
            break;
        }
    }
    best
}

/// Filters and ranks completion candidates. `texts` are the strings matched against
/// (filter text or label), `sort_texts` break ties. Returns indices into `texts` with their
/// matches, best first; with `keep_order` (snippet choices) in the given order.
pub fn rank_completions(
    pattern: &str,
    texts: &[String],
    sort_texts: &[String],
    keep_order: bool,
) -> Vec<(usize, FuzzyMatch)> {
    let mut scored: Vec<(usize, FuzzyMatch)> =
        texts.iter().enumerate().filter_map(|(i, t)| fuzzy_score(pattern, t).map(|m| (i, m))).collect();
    if !keep_order {
        scored.sort_by(|a, b| {
            b.1.score
                .partial_cmp(&a.1.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| sort_texts[a.0].cmp(&sort_texts[b.0]))
        });
    }
    scored
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The distinct words (a letter or `_`, then letters, digits or `_`) of `lines`, in order of
/// first appearance, at most `max`.
pub fn collect_words(rope: &Rope, lines: std::ops::Range<usize>, max: usize) -> Vec<String> {
    let end = lines.end.min(rope.len_lines());
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    let mut word = String::new();
    let mut flush = |word: &mut String, out: &mut Vec<String>| {
        if !word.is_empty() {
            if !seen.contains(word.as_str()) {
                seen.insert(word.clone());
                out.push(std::mem::take(word));
            } else {
                word.clear();
            }
        }
    };
    for line in lines.start.min(end)..end {
        for c in rope.line(line).chars() {
            if is_word_char(c) && (!word.is_empty() || !c.is_numeric()) {
                word.push(c);
            } else {
                flush(&mut word, &mut out);
            }
            if out.len() >= max {
                return out;
            }
        }
        flush(&mut word, &mut out);
    }
    out.truncate(max);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_matches_prefixes_and_boundaries() {
        let m = fuzzy_score("gv", "getValue").unwrap();
        assert_eq!(m.matches, vec![0, 3]);
        assert!(fuzzy_score("val", "getValue").is_some());
        // The first char must sit at a boundary.
        assert!(fuzzy_score("al", "getValue").is_none());
        assert!(fuzzy_score("x", "getValue").is_none());
        // A prefix beats a match further in; the same case scores a little higher.
        let prefix = fuzzy_score("get", "getValue").unwrap().score;
        let inner = fuzzy_score("get", "my_get").unwrap().score;
        assert!(prefix > inner);
        assert!(fuzzy_score("Get", "Get").unwrap().score > fuzzy_score("get", "Get").unwrap().score);
        assert_eq!(fuzzy_score("", "anything").unwrap().score, 0.0);
    }

    #[test]
    fn ranking_sorts_by_score_then_sort_text() {
        let texts: Vec<String> = ["my_get", "getValue", "getter"].map(String::from).to_vec();
        let ranked = rank_completions("get", &texts, &texts, false);
        let order: Vec<usize> = ranked.iter().map(|r| r.0).collect();
        assert_eq!(order[2], 0);
        let kept = rank_completions("", &texts, &texts, true);
        assert_eq!(kept.iter().map(|r| r.0).collect::<Vec<_>>(), vec![0, 1, 2]);
    }

    #[test]
    fn words_are_distinct_and_skip_numbers() {
        let rope = Rope::from_str("let a1 = b_2 + a1;\nfn 42x zé\n");
        assert_eq!(collect_words(&rope, 0..3, 100), vec!["let", "a1", "b_2", "fn", "x", "zé"]);
        assert_eq!(collect_words(&rope, 0..3, 2), vec!["let", "a1"]);
    }
}
