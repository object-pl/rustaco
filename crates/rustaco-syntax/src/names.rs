//! Highlight names: the capture names tree-sitter queries use, and the TextMate scopes
//! each one maps to, so VS Code themes can colour tree-sitter highlights.

/// Recognised capture names with their TextMate scopes, most specific scope first.
/// A capture resolves to the longest entry that is a dot-separated prefix of it:
/// `function.method.call` resolves to `function.method`. Style id = index + 1; 0 is plain text.
pub const HIGHLIGHT_NAMES: &[(&str, &[&str])] = &[
    ("attribute", &["entity.other.attribute-name"]),
    ("boolean", &["constant.language.boolean", "constant.language"]),
    ("character", &["constant.character"]),
    ("charset", &["keyword.control.at-rule", "keyword"]),
    ("comment", &["comment"]),
    ("comment.documentation", &["comment.block.documentation", "comment"]),
    ("constant", &["variable.other.constant", "constant", "constant.other"]),
    ("constant.builtin", &["constant.language"]),
    ("constructor", &["entity.name.function.constructor", "entity.name.type"]),
    ("embedded", &["meta.embedded"]),
    ("escape", &["constant.character.escape"]),
    ("function", &["entity.name.function"]),
    ("function.builtin", &["support.function"]),
    ("function.macro", &["entity.name.function.macro", "entity.name.function"]),
    ("function.method", &["entity.name.function.member", "entity.name.function"]),
    ("import", &["keyword.control.at-rule", "keyword"]),
    // TextMate grammars put declaration keywords (`public`, `data`, `static`) under `storage`.
    ("keyword", &["keyword", "storage.modifier", "storage"]),
    ("keyword.conditional", &["keyword.control.conditional", "keyword.control"]),
    ("keyword.control", &["keyword.control"]),
    ("keyword.exception", &["keyword.control.exception", "keyword.control"]),
    ("keyword.function", &["storage.type.function", "keyword"]),
    ("keyword.import", &["keyword.control.import", "keyword.control"]),
    ("keyword.operator", &["keyword.operator"]),
    ("keyword.repeat", &["keyword.control.loop", "keyword.control"]),
    ("keyword.return", &["keyword.control.flow", "keyword.control"]),
    ("keyword.storage", &["storage.type", "keyword"]),
    ("keyword.type", &["storage.type", "keyword"]),
    ("keyframes", &["keyword.control.at-rule", "keyword"]),
    ("label", &["entity.name.label"]),
    ("markup.bold", &["markup.bold"]),
    ("markup.heading", &["markup.heading"]),
    ("markup.italic", &["markup.italic"]),
    ("markup.link", &["markup.underline.link"]),
    ("markup.list", &["markup.list", "punctuation"]),
    ("markup.quote", &["markup.quote"]),
    ("markup.raw", &["markup.inline.raw", "string"]),
    ("markup.strikethrough", &["markup.strikethrough"]),
    ("media", &["keyword.control.at-rule", "keyword"]),
    ("module", &["entity.name.namespace"]),
    ("namespace", &["entity.name.namespace"]),
    ("number", &["constant.numeric"]),
    ("operator", &["keyword.operator"]),
    ("property", &["variable.other.property"]),
    ("punctuation", &["punctuation"]),
    ("punctuation.bracket", &["punctuation.bracket", "punctuation"]),
    // `punctuation.seperator` is a misspelling some grammars (Kotlin) ship.
    (
        "punctuation.delimiter",
        &[
            "punctuation.separator",
            "punctuation",
            "punctuation.seperator",
            "punctuation.accessor",
            "punctuation.terminator",
        ],
    ),
    ("punctuation.special", &["punctuation.special", "punctuation"]),
    ("string", &["string"]),
    ("string.escape", &["constant.character.escape"]),
    ("string.regexp", &["string.regexp", "string"]),
    ("string.special", &["string.other", "string"]),
    ("string.special.key", &["support.type.property-name", "string"]),
    ("supports", &["keyword.control.at-rule", "keyword"]),
    ("tag", &["entity.name.tag"]),
    // Older capture names for markup, used by the Markdown grammar.
    ("text.emphasis", &["markup.italic"]),
    ("text.literal", &["markup.inline.raw", "string"]),
    ("text.reference", &["markup.underline.link"]),
    ("text.strong", &["markup.bold"]),
    ("text.title", &["markup.heading"]),
    ("text.uri", &["markup.underline.link"]),
    (
        "type",
        &[
            "entity.name.type",
            "support.type",
            "entity.name.class",
            "entity.name.struct",
            "entity.name.enum",
            "entity.name.interface",
            "entity.name.trait",
            "support.class",
        ],
    ),
    ("type.builtin", &["support.type.primitive", "support.type"]),
    ("variable", &["variable"]),
    ("variable.builtin", &["variable.language"]),
    ("variable.parameter", &["variable.parameter"]),
];

/// Style id for a capture name, or 0 when no entry covers it.
pub fn style_for_capture(capture: &str) -> u32 {
    let mut best: Option<(usize, usize)> = None;
    for (i, (name, _)) in HIGHLIGHT_NAMES.iter().enumerate() {
        let covers =
            capture == *name || (capture.starts_with(name) && capture.as_bytes().get(name.len()) == Some(&b'.'));
        if covers && best.is_none_or(|(_, len)| name.len() > len) {
            best = Some((i, name.len()));
        }
    }
    best.map_or(0, |(i, _)| i as u32 + 1)
}

/// The highlight name behind a style id.
pub fn name_of_style(style: u32) -> Option<&'static str> {
    style.checked_sub(1).and_then(|i| HIGHLIGHT_NAMES.get(i as usize)).map(|(n, _)| *n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_longest_prefix() {
        assert_eq!(name_of_style(style_for_capture("function.method.call")), Some("function.method"));
        assert_eq!(name_of_style(style_for_capture("function")), Some("function"));
        assert_eq!(name_of_style(style_for_capture("keyword.return")), Some("keyword.return"));
        assert_eq!(name_of_style(style_for_capture("keyword.coroutine")), Some("keyword"));
        assert_eq!(style_for_capture("functionality"), 0);
        assert_eq!(style_for_capture("local.definition"), 0);
    }
}
