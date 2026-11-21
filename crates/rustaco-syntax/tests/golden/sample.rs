use std::collections::HashMap;

/// Counts words.
pub fn count(text: &str) -> HashMap<&str, usize> {
    let mut map = HashMap::new();
    for w in text.split_whitespace() {
        *map.entry(w).or_insert(0) += 1; // tally
    }
    map
}
