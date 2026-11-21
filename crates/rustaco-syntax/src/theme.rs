//! Themes in the TextMate scope model that VS Code themes use.
//!
//! A frontend parses a theme file (the browser uses `JSON.parse` on a VS Code theme) into
//! [`ThemeRule`]s and calls [`Theme::style_table`] to get one [`Style`] per highlight name.

use crate::names::HIGHLIGHT_NAMES;

/// Colours are `0xRRGGBBAA`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub foreground: Option<u32>,
    pub background: Option<u32>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
}

/// One scope selector with the style it sets. A theme entry with a comma-separated
/// selector list becomes one rule per selector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeRule {
    pub selector: String,
    pub style: Style,
}

/// Colours of the editor surface, `0xRRGGBBAA`. Named after the VS Code theme `colors`
/// keys they are read from (see [`Colors::set`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Colors {
    pub background: u32,
    pub foreground: u32,
    pub selection: u32,
    pub current_line: u32,
    pub caret: u32,
    pub line_number: u32,
    pub line_number_active: u32,
    pub gutter: u32,
    pub scrollbar: u32,
    pub find_match: u32,
}

impl Colors {
    pub const DARK: Colors = Colors {
        background: 0x1e1e1eff,
        foreground: 0xd4d4d4ff,
        selection: 0x264f78ff,
        current_line: 0x2a2d2eff,
        caret: 0xaeafadff,
        line_number: 0x6e7681ff,
        line_number_active: 0xccccccff,
        gutter: 0x1e1e1eff,
        scrollbar: 0x79797966,
        find_match: 0xea5c0055,
    };

    pub const LIGHT: Colors = Colors {
        background: 0xffffffff,
        foreground: 0x000000ff,
        selection: 0xadd6ffff,
        current_line: 0xf0f0f0ff,
        caret: 0x000000ff,
        line_number: 0x6e7681ff,
        line_number_active: 0x171184ff,
        gutter: 0xffffffff,
        scrollbar: 0x64646466,
        find_match: 0xea5c0055,
    };

    /// Black background, white text, and a selection and current line that stand out
    /// without hiding the text under them.
    pub const HIGH_CONTRAST: Colors = Colors {
        background: 0x000000ff,
        foreground: 0xffffffff,
        selection: 0xffffff59,
        current_line: 0xffffff1f,
        caret: 0xffffffff,
        line_number: 0xffffffff,
        line_number_active: 0xf38518ff,
        gutter: 0x000000ff,
        scrollbar: 0x6fc3dfcc,
        find_match: 0xf3851866,
    };

    /// Sets the colour a VS Code theme `colors` key names. Returns false for keys the editor
    /// does not use.
    pub fn set(&mut self, key: &str, rgba: u32) -> bool {
        let field = match key {
            "editor.background" => &mut self.background,
            "editor.foreground" => &mut self.foreground,
            "editor.selectionBackground" => &mut self.selection,
            "editor.lineHighlightBackground" => &mut self.current_line,
            "editorCursor.foreground" => &mut self.caret,
            "editorLineNumber.foreground" => &mut self.line_number,
            "editorLineNumber.activeForeground" => &mut self.line_number_active,
            "editorGutter.background" => &mut self.gutter,
            "scrollbarSlider.background" => &mut self.scrollbar,
            "editor.findMatchHighlightBackground" => &mut self.find_match,
            _ => return false,
        };
        *field = rgba;
        true
    }
}

impl Default for Colors {
    fn default() -> Self {
        Colors::DARK
    }
}

#[derive(Clone, Debug, Default)]
pub struct Theme {
    pub rules: Vec<ThemeRule>,
    pub colors: Colors,
}

impl Theme {
    /// Style for the first scope in `scopes` that any rule matches, each property taken
    /// from the most specific rule that sets it. Selectors with descendant parts
    /// (`meta.function entity.name`) or exclusions (`- comment`) are skipped: tree-sitter
    /// highlights carry a single scope, not a scope stack.
    pub fn resolve(&self, scopes: &[&str]) -> Style {
        for scope in scopes {
            let mut matching: Vec<&ThemeRule> = self
                .rules
                .iter()
                // Descendant selectors and exclusions both contain spaces; hyphens alone are
                // part of ordinary scope names such as `attribute-name`.
                .filter(|r| !r.selector.trim().contains([' ', ',']))
                .filter(|r| selector_matches(&r.selector, scope))
                .collect();
            if matching.is_empty() {
                continue;
            }
            // Most specific (longest) selector last, so it overrides.
            matching.sort_by_key(|r| r.selector.len());
            let mut style = Style::default();
            for r in matching {
                let s = r.style;
                style.foreground = s.foreground.or(style.foreground);
                style.background = s.background.or(style.background);
                style.bold = s.bold.or(style.bold);
                style.italic = s.italic.or(style.italic);
                style.underline = s.underline.or(style.underline);
            }
            return style;
        }
        Style::default()
    }

    /// One style per style id: index 0 is plain text, index `i` is `HIGHLIGHT_NAMES[i - 1]`.
    pub fn style_table(&self) -> Vec<Style> {
        std::iter::once(Style::default())
            .chain(HIGHLIGHT_NAMES.iter().map(|(_, scopes)| self.resolve(scopes)))
            .collect()
    }
}

/// TextMate matching: `keyword.control` matches `keyword.control` and `keyword.control.rust`,
/// not `keyword.controlx`.
fn selector_matches(selector: &str, scope: &str) -> bool {
    let selector = selector.trim();
    scope == selector || (scope.starts_with(selector) && scope.as_bytes().get(selector.len()) == Some(&b'.'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::style_for_capture;

    fn rule(selector: &str, fg: u32) -> ThemeRule {
        ThemeRule { selector: selector.into(), style: Style { foreground: Some(fg), ..Style::default() } }
    }

    #[test]
    fn most_specific_selector_wins_per_property() {
        let mut italic = rule("comment", 0x6a9955ff);
        italic.style.italic = Some(true);
        let theme = Theme {
            rules: vec![italic, rule("comment.block.documentation", 0x608b4eff), rule("keyword", 0x569cd6ff)],
            colors: Colors::DARK,
        };
        let doc = theme.resolve(&["comment.block.documentation", "comment"]);
        assert_eq!(doc.foreground, Some(0x608b4eff));
        assert_eq!(doc.italic, Some(true), "italic inherited from the less specific rule");
        assert_eq!(theme.resolve(&["keyword.control"]).foreground, Some(0x569cd6ff));
        assert_eq!(theme.resolve(&["keywordx"]), Style::default());
    }

    #[test]
    fn falls_back_to_the_next_scope() {
        let theme = Theme { rules: vec![rule("entity.name.function", 0xdcdcaaff)], colors: Colors::DARK };
        let table = theme.style_table();
        let method = style_for_capture("function.method") as usize;
        assert_eq!(table[method].foreground, Some(0xdcdcaaff));
        assert_eq!(table[0], Style::default());
    }
}
