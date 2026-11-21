//! The bundled themes, written for this project in the VS Code theme rule format (MIT).

use crate::theme::{Style, Theme, ThemeRule};

const fn rgb(c: u32) -> u32 {
    (c << 8) | 0xff
}

/// (selector, foreground 0xRRGGBB, italic)
const DARK: &[(&str, u32, bool)] = &[
    ("comment", 0x6a9955, true),
    ("comment.block.documentation", 0x6a9955, true),
    ("keyword", 0x569cd6, false),
    ("storage.type", 0x569cd6, false),
    ("keyword.control", 0xc586c0, false),
    ("keyword.operator", 0xd4d4d4, false),
    ("string", 0xce9178, false),
    ("string.regexp", 0xd16969, false),
    ("constant.character.escape", 0xd7ba7d, false),
    ("constant.numeric", 0xb5cea8, false),
    ("constant.language", 0x569cd6, false),
    ("variable.other.constant", 0x4fc1ff, false),
    ("entity.name.function", 0xdcdcaa, false),
    ("entity.name.function.macro", 0x4ec9b0, false),
    ("support.function", 0xdcdcaa, false),
    ("entity.name.type", 0x4ec9b0, false),
    ("support.type", 0x4ec9b0, false),
    ("support.type.primitive", 0x569cd6, false),
    ("entity.name.namespace", 0x4ec9b0, false),
    ("entity.name.tag", 0x569cd6, false),
    ("entity.other.attribute-name", 0x9cdcfe, false),
    ("entity.name.label", 0xc8c8c8, false),
    ("variable", 0x9cdcfe, false),
    ("variable.language", 0x569cd6, false),
    ("variable.parameter", 0x9cdcfe, false),
    ("variable.other.property", 0x9cdcfe, false),
    ("support.type.property-name", 0x9cdcfe, false),
    ("punctuation", 0xd4d4d4, false),
    ("punctuation.bracket", 0xffd700, false),
    ("markup.heading", 0x569cd6, false),
    ("markup.inline.raw", 0xce9178, false),
    ("markup.underline.link", 0x3794ff, false),
    ("markup.quote", 0x6a9955, false),
    ("keyword.control.at-rule", 0xc586c0, false),
];

const LIGHT: &[(&str, u32, bool)] = &[
    ("comment", 0x008000, true),
    ("keyword", 0x0000ff, false),
    ("keyword.control", 0xaf00db, false),
    ("keyword.operator", 0x000000, false),
    ("string", 0xa31515, false),
    ("string.regexp", 0x811f3f, false),
    ("constant.character.escape", 0xee0000, false),
    ("constant.numeric", 0x098658, false),
    ("constant.language", 0x0000ff, false),
    ("variable.other.constant", 0x0070c1, false),
    ("entity.name.function", 0x795e26, false),
    ("support.function", 0x795e26, false),
    ("entity.name.type", 0x267f99, false),
    ("support.type", 0x267f99, false),
    ("support.type.primitive", 0x0000ff, false),
    ("entity.name.namespace", 0x267f99, false),
    ("entity.name.tag", 0x800000, false),
    ("entity.other.attribute-name", 0xe50000, false),
    ("variable", 0x001080, false),
    ("variable.language", 0x0000ff, false),
    ("variable.parameter", 0x001080, false),
    ("variable.other.property", 0x001080, false),
    ("support.type.property-name", 0x0451a5, false),
    ("punctuation", 0x000000, false),
    ("markup.heading", 0x800000, false),
    ("markup.inline.raw", 0xa31515, false),
    ("markup.underline.link", 0x0000ff, false),
    ("markup.quote", 0x008000, false),
    ("keyword.control.at-rule", 0xaf00db, false),
];

/// Font-only rules shared by every bundled theme: (selector, bold, italic, underline).
const FONT: &[(&str, bool, bool, bool)] = &[
    ("markup.heading", true, false, false),
    ("markup.bold", true, false, false),
    ("markup.italic", false, true, false),
    ("markup.underline.link", false, false, true),
    ("markup.strikethrough", false, false, false),
];

fn build(rules: &[(&str, u32, bool)]) -> Theme {
    let colours = rules.iter().map(|&(selector, fg, italic)| ThemeRule {
        selector: selector.to_owned(),
        style: Style { foreground: Some(rgb(fg)), italic: italic.then_some(true), ..Style::default() },
    });
    let fonts = FONT.iter().map(|&(selector, bold, italic, underline)| ThemeRule {
        selector: selector.to_owned(),
        style: Style {
            bold: bold.then_some(true),
            italic: italic.then_some(true),
            underline: underline.then_some(true),
            ..Style::default()
        },
    });
    Theme { rules: colours.chain(fonts).collect() }
}

pub fn dark() -> Theme {
    build(DARK)
}

pub fn light() -> Theme {
    build(LIGHT)
}
