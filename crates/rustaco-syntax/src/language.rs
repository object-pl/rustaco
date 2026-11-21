//! Grammars and highlight queries of the languages compiled into this build.

use std::sync::OnceLock;

use tree_sitter::{Language, Query};

use crate::catalog::{LanguageInfo, info};
use crate::injection::CompiledInjections;
use crate::names::style_for_capture;

/// A highlight query compiled for one language, with the style id of each capture.
pub(crate) struct CompiledQuery {
    pub(crate) query: Query,
    pub(crate) capture_styles: Vec<u32>,
    /// Per pattern: how much context it requires, counted as the nodes and predicates in its
    /// source (each opens a parenthesis). When several patterns capture the same node, the
    /// most specific one wins: `(call function: (attribute attribute: (identifier)))` over
    /// `(attribute attribute: (identifier))`, and that over a bare `(identifier)`.
    pub(crate) specificity: Vec<u32>,
}

type Compiled = OnceLock<Result<CompiledQuery, String>>;
type CompiledInj = OnceLock<Result<CompiledInjections, String>>;

pub struct LanguageDef {
    pub id: &'static str,
    grammar: fn() -> Language,
    /// Query sources joined in order; TypeScript builds on the JavaScript query, C++ on C.
    highlights: &'static [&'static str],
    injections: &'static [&'static str],
    compiled: &'static Compiled,
    compiled_injections: &'static CompiledInj,
}

impl LanguageDef {
    pub fn grammar(&self) -> Language {
        (self.grammar)()
    }

    /// Detection rules and editing configuration from the catalog.
    pub fn info(&self) -> &'static LanguageInfo {
        info(self.id).expect("every compiled language has a catalog entry")
    }

    /// Compiles the highlight query on first use.
    pub(crate) fn highlight_query(&self) -> Result<&CompiledQuery, &str> {
        self.compiled
            .get_or_init(|| {
                let source = self.highlights.join("\n");
                let query = Query::new(&self.grammar(), &source).map_err(|e| format!("{}: {e}", self.id))?;
                let capture_styles = query.capture_names().iter().map(|n| style_for_capture(n)).collect();
                let specificity = (0..query.pattern_count())
                    .map(|i| {
                        let text = &source[query.start_byte_for_pattern(i)..query.end_byte_for_pattern(i)];
                        pattern_specificity(text)
                    })
                    .collect();
                Ok(CompiledQuery { query, capture_styles, specificity })
            })
            .as_ref()
            .map_err(String::as_str)
    }

    /// Compiles the injection query on first use; `None` for languages without one.
    pub(crate) fn injection_query(&self) -> Option<Result<&CompiledInjections, &str>> {
        if self.injections.is_empty() {
            return None;
        }
        let compiled = self.compiled_injections.get_or_init(|| {
            let source = self.injections.join("\n");
            Query::new(&self.grammar(), &source)
                .map(CompiledInjections::new)
                .map_err(|e| format!("{} injections: {e}", self.id))
        });
        Some(compiled.as_ref().map_err(String::as_str))
    }
}

#[allow(unused_macros)] // unused when the build enables no language
/// Counts the parentheses that open nodes and predicates in a query pattern, skipping
/// string literals and comments.
fn pattern_specificity(pattern: &str) -> u32 {
    let mut count = 0;
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                while let Some(c) = chars.next() {
                    match c {
                        '\\' => {
                            chars.next();
                        }
                        '"' => break,
                        _ => {}
                    }
                }
            }
            ';' => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '(' => count += 1,
            _ => {}
        }
    }
    count
}

macro_rules! language {
    ($id:literal, $grammar:expr, [$($query:expr),+ $(,)?] $(, injections: [$($inj:expr),* $(,)?])?) => {
        LanguageDef {
            id: $id,
            grammar: || $grammar.into(),
            highlights: &[$($query),+],
            injections: &[$($($inj),*)?],
            compiled: {
                static COMPILED: Compiled = OnceLock::new();
                &COMPILED
            },
            compiled_injections: {
                static COMPILED: CompiledInj = OnceLock::new();
                &COMPILED
            },
        }
    };
}

static LANGUAGES: &[LanguageDef] = &[
    #[cfg(feature = "rust")]
    language!("rust", tree_sitter_rust::LANGUAGE, [tree_sitter_rust::HIGHLIGHTS_QUERY],
        injections: [tree_sitter_rust::INJECTIONS_QUERY]),
    #[cfg(feature = "javascript")]
    language!(
        "javascript",
        tree_sitter_javascript::LANGUAGE,
        [tree_sitter_javascript::HIGHLIGHT_QUERY, tree_sitter_javascript::JSX_HIGHLIGHT_QUERY],
        injections: [tree_sitter_javascript::INJECTIONS_QUERY]
    ),
    #[cfg(feature = "typescript")]
    language!(
        "typescript",
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
        [tree_sitter_javascript::HIGHLIGHT_QUERY, tree_sitter_typescript::HIGHLIGHTS_QUERY,]
    ),
    #[cfg(feature = "typescript")]
    language!(
        "tsx",
        tree_sitter_typescript::LANGUAGE_TSX,
        [
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
            tree_sitter_typescript::HIGHLIGHTS_QUERY,
        ]
    ),
    #[cfg(feature = "json")]
    language!("json", tree_sitter_json::LANGUAGE, [tree_sitter_json::HIGHLIGHTS_QUERY]),
    #[cfg(feature = "python")]
    language!("python", tree_sitter_python::LANGUAGE, [tree_sitter_python::HIGHLIGHTS_QUERY]),
    #[cfg(feature = "c")]
    language!("c", tree_sitter_c::LANGUAGE, [tree_sitter_c::HIGHLIGHT_QUERY]),
    #[cfg(feature = "cpp")]
    language!("cpp", tree_sitter_cpp::LANGUAGE, [tree_sitter_c::HIGHLIGHT_QUERY, tree_sitter_cpp::HIGHLIGHT_QUERY]),
    #[cfg(feature = "go")]
    language!("go", tree_sitter_go::LANGUAGE, [tree_sitter_go::HIGHLIGHTS_QUERY]),
    #[cfg(feature = "java")]
    language!("java", tree_sitter_java::LANGUAGE, [tree_sitter_java::HIGHLIGHTS_QUERY]),
    #[cfg(feature = "csharp")]
    language!("csharp", tree_sitter_c_sharp::LANGUAGE, [tree_sitter_c_sharp::HIGHLIGHTS_QUERY]),
    #[cfg(feature = "html")]
    language!("html", tree_sitter_html::LANGUAGE, [tree_sitter_html::HIGHLIGHTS_QUERY],
        injections: [tree_sitter_html::INJECTIONS_QUERY]),
    #[cfg(feature = "css")]
    language!("css", tree_sitter_css::LANGUAGE, [tree_sitter_css::HIGHLIGHTS_QUERY]),
    #[cfg(feature = "bash")]
    language!("bash", tree_sitter_bash::LANGUAGE, [tree_sitter_bash::HIGHLIGHT_QUERY]),
    #[cfg(feature = "toml")]
    language!("toml", tree_sitter_toml_ng::LANGUAGE, [tree_sitter_toml_ng::HIGHLIGHTS_QUERY]),
    #[cfg(feature = "yaml")]
    language!("yaml", tree_sitter_yaml::LANGUAGE, [tree_sitter_yaml::HIGHLIGHTS_QUERY]),
    #[cfg(feature = "sql")]
    language!("sql", tree_sitter_sequel::LANGUAGE, [tree_sitter_sequel::HIGHLIGHTS_QUERY]),
    #[cfg(feature = "markdown")]
    language!("markdown", tree_sitter_md::LANGUAGE, [tree_sitter_md::HIGHLIGHT_QUERY_BLOCK],
        injections: [tree_sitter_md::INJECTION_QUERY_BLOCK]),
    #[cfg(feature = "markdown")]
    language!("markdown_inline", tree_sitter_md::INLINE_LANGUAGE, [tree_sitter_md::HIGHLIGHT_QUERY_INLINE],
        injections: [tree_sitter_md::INJECTION_QUERY_INLINE]),
];

/// The languages compiled into this build.
pub fn languages() -> &'static [LanguageDef] {
    LANGUAGES
}

pub fn by_id(id: &str) -> Option<&'static LanguageDef> {
    LANGUAGES.iter().find(|l| l.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specificity_counts_nodes_and_predicates() {
        assert_eq!(pattern_specificity("(identifier) @variable"), 1);
        assert_eq!(pattern_specificity("((identifier) @constructor (#match? @constructor \"^[A-Z(]\"))"), 3);
        assert_eq!(pattern_specificity("(call function: (attribute attribute: (identifier) @m))"), 3);
        assert_eq!(pattern_specificity("[\"fn\" \"let\"] @keyword ; (comment)"), 0);
    }

    #[test]
    fn every_highlight_query_compiles() {
        for lang in languages() {
            let q = lang.highlight_query().unwrap_or_else(|e| panic!("{e}"));
            assert!(q.capture_styles.iter().any(|&s| s != 0), "{} maps no capture to a style", lang.id);
            assert_eq!(lang.info().id, lang.id);
            assert_eq!(q.specificity.len(), q.query.pattern_count());
            if let Some(inj) = lang.injection_query() {
                inj.unwrap_or_else(|e| panic!("{e}"));
            }
        }
    }
}
