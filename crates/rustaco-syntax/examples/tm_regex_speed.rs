//! Times fancy-regex searches for TextMate-style keyword patterns with Unicode and ASCII
//! word boundaries: `cargo run --release -p rustaco-syntax --example tm_regex_speed`.
use std::time::Instant;

fn time(label: &str, pattern: &str, line: &str) {
    let re = fancy_regex::Regex::new(pattern).expect("pattern");
    let t = Instant::now();
    let mut found = 0;
    for _ in 0..20_000 {
        let mut pos = 0;
        while let Ok(Some(m)) = re.find_from_pos(line, pos) {
            found += 1;
            pos = m.end().max(pos + 1);
        }
    }
    println!("{label:<28} {:>8.2} µs per line scan ({found} matches)", t.elapsed().as_secs_f64() * 1e6 / 20_000.0);
}

fn main() {
    let words = "select|insert|update|delete|from|where|group|order|by|having|join|inner|outer|left|right|on|as|and|or|not|null|is|in|like|between|exists|union|all|distinct|limit|offset|case|when|then|else|end|create|table|index|view|drop|alter|add|column|primary|key|foreign|references";
    let line = "SELECT a.id, b.name, COUNT(*) AS n FROM orders a JOIN users b ON a.user_id = b.id WHERE a.created > '2026-01-01' AND b.flag = 7 GROUP BY 1, 2; -- row 7";
    time("unicode \\b", &format!(r"(?i)\b({words})\b"), line);
    time("ascii (?-u:\\b)", &format!(r"(?i)(?-u:\b)({words})(?-u:\b)"), line);
    time("unicode \\b + lookbehind", &format!(r"(?i)(?<!@)\b({words})\b"), line);
}
