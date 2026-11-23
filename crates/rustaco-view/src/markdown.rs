//! The little markdown that hover cards and completion documentation show: ``` fences as
//! code blocks, and inside text `code`, **bold** and *italic*. Anything else stays as
//! written. Frontends turn the blocks into their own widgets (DOM in the browser).

/// A piece of inline text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inline {
    Text(String),
    Code(String),
    Bold(String),
    Italic(String),
}

/// A block: a code block from a fence, or a paragraph of inline pieces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Code(String),
    Paragraph(Vec<Inline>),
}

/// Splits `text` into blocks. Empty parts are skipped; a paragraph loses its leading and
/// trailing line breaks, a code block its last one.
pub fn parse(text: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut rest = text;
    let mut in_code = false;
    loop {
        let (part, next) = match rest.find("```") {
            Some(at) => {
                // The fence's language name and line break belong to the fence.
                let after = &rest[at + 3..];
                let name = after.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(after.len());
                let after = &after[name..];
                (&rest[..at], Some(after.strip_prefix('\n').unwrap_or(after)))
            }
            None => (rest, None),
        };
        if !part.is_empty() {
            if in_code {
                blocks.push(Block::Code(part.strip_suffix('\n').unwrap_or(part).to_owned()));
            } else {
                let trimmed = part.trim_matches('\n');
                if !trimmed.is_empty() {
                    blocks.push(Block::Paragraph(inline(trimmed)));
                }
            }
        }
        match next {
            Some(n) => {
                rest = n;
                in_code = !in_code;
            }
            None => return blocks,
        }
    }
}

/// `code`, **bold** and *italic* (an italic span starts with a char that is neither `*` nor
/// a space); the rest is text.
fn inline(text: &str) -> Vec<Inline> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut plain = String::new();
    let mut i = 0;
    let find = |from: usize, c: char| chars[from..].iter().position(|&x| x == c).map(|p| from + p);
    while i < chars.len() {
        let span = match chars[i] {
            '`' => {
                find(i + 1, '`').filter(|&j| j > i + 1).map(|j| (j + 1, Inline::Code(chars[i + 1..j].iter().collect())))
            }
            '*' if chars.get(i + 1) == Some(&'*') => find(i + 2, '*')
                .filter(|&j| j > i + 2 && chars.get(j + 1) == Some(&'*'))
                .map(|j| (j + 2, Inline::Bold(chars[i + 2..j].iter().collect()))),
            '*' if chars.get(i + 1).is_some_and(|c| !c.is_whitespace()) => {
                find(i + 1, '*').map(|j| (j + 1, Inline::Italic(chars[i + 1..j].iter().collect())))
            }
            _ => None,
        };
        match span {
            Some((end, piece)) => {
                if !plain.is_empty() {
                    out.push(Inline::Text(std::mem::take(&mut plain)));
                }
                out.push(piece);
                i = end;
            }
            None => {
                plain.push(chars[i]);
                i += 1;
            }
        }
    }
    if !plain.is_empty() {
        out.push(Inline::Text(plain));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Inline {
        Inline::Text(s.into())
    }

    #[test]
    fn fences_and_inline_spans() {
        let blocks = parse("```rust\nfn main() {}\n```\nCalls `run` **twice**, *then* stops.\n");
        assert_eq!(
            blocks,
            vec![
                Block::Code("fn main() {}".into()),
                Block::Paragraph(vec![
                    text("Calls "),
                    Inline::Code("run".into()),
                    text(" "),
                    Inline::Bold("twice".into()),
                    text(", "),
                    Inline::Italic("then".into()),
                    text(" stops."),
                ]),
            ]
        );
    }

    #[test]
    fn unmatched_markers_stay_text() {
        assert_eq!(parse("a * b and `x and **y"), vec![Block::Paragraph(vec![text("a * b and `x and **y")])]);
        assert_eq!(parse("2 ** 3 = 8"), vec![Block::Paragraph(vec![text("2 ** 3 = 8")])]);
        assert_eq!(parse("``\n\n"), vec![Block::Paragraph(vec![text("``")])]);
        assert!(parse("").is_empty());
        // An unclosed fence: the rest is code.
        assert_eq!(parse("see\n```\nx = 1"), vec![Block::Paragraph(vec![text("see")]), Block::Code("x = 1".into())]);
    }
}
