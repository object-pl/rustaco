//! The bundled themes, written for this project in the VS Code theme rule format (MIT).

use crate::theme::{Colors, Style, Theme, ThemeRule};

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

/// Colours chosen for contrast on black (each above 7:1, WCAG AAA for text).
const HIGH_CONTRAST: &[(&str, u32, bool)] = &[
    ("comment", 0x7ca668, true),
    ("keyword", 0x569cd6, false),
    ("storage.type", 0x569cd6, false),
    ("keyword.control", 0xc586c0, false),
    ("keyword.operator", 0xffffff, false),
    ("string", 0xce9178, false),
    ("string.regexp", 0xd98181, false),
    ("constant.character.escape", 0xd7ba7d, false),
    ("constant.numeric", 0xb5cea8, false),
    ("constant.language", 0x569cd6, false),
    ("variable.other.constant", 0x4fc1ff, false),
    ("entity.name.function", 0xdcdcaa, false),
    ("support.function", 0xdcdcaa, false),
    ("entity.name.type", 0x4ec9b0, false),
    ("support.type", 0x4ec9b0, false),
    ("entity.name.namespace", 0x4ec9b0, false),
    ("entity.name.tag", 0x569cd6, false),
    ("entity.other.attribute-name", 0x9cdcfe, false),
    ("variable", 0x9cdcfe, false),
    ("variable.language", 0x569cd6, false),
    ("variable.other.property", 0x9cdcfe, false),
    ("support.type.property-name", 0x9cdcfe, false),
    ("punctuation", 0xffffff, false),
    ("markup.heading", 0x6796e6, false),
    ("markup.inline.raw", 0xce9178, false),
    ("markup.underline.link", 0x489dff, false),
    ("keyword.control.at-rule", 0xc586c0, false),
];

/// Font-only rules shared by every bundled theme: (selector, bold, italic, underline).
const FONT: &[(&str, bool, bool, bool)] = &[
    ("markup.heading", true, false, false),
    ("markup.bold", true, false, false),
    ("markup.italic", false, true, false),
    ("markup.underline.link", false, false, true),
    ("markup.strikethrough", false, false, false),
];

fn build(rules: &[(&str, u32, bool)], colors: Colors) -> Theme {
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
    Theme { rules: colours.chain(fonts).collect(), colors }
}

pub fn dark() -> Theme {
    build(DARK, Colors::DARK)
}

pub fn light() -> Theme {
    build(LIGHT, Colors::LIGHT)
}

pub fn high_contrast() -> Theme {
    build(HIGH_CONTRAST, Colors::HIGH_CONTRAST)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG relative luminance contrast between two 0xRRGGBBAA colours (alpha ignored).
    fn contrast(a: u32, b: u32) -> f64 {
        let lum = |c: u32| {
            let ch = |shift: u32| {
                let v = ((c >> shift) & 0xff) as f64 / 255.0;
                if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * ch(24) + 0.7152 * ch(16) + 0.0722 * ch(8)
        };
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn token_colours_are_readable_on_their_background() {
        for (name, theme, minimum) in
            [("dark", dark(), 3.0), ("light", light(), 3.0), ("high contrast", high_contrast(), 7.0)]
        {
            for rule in &theme.rules {
                if let Some(fg) = rule.style.foreground {
                    let ratio = contrast(fg, theme.colors.background);
                    assert!(ratio >= minimum, "{name}: {} has contrast {ratio:.2}", rule.selector);
                }
            }
        }
    }
}
