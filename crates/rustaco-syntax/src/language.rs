//! Grammars and highlight queries of the languages compiled into this build.

use std::sync::OnceLock;

use tree_sitter::{Language, Query};

use crate::catalog::{LanguageInfo, info};
use crate::names::style_for_capture;

/// A highlight query compiled for one language, with the style id of each capture.
pub(crate) struct CompiledQuery {
    pub(crate) query: Query,
    pub(crate) capture_styles: Vec<u32>,
}

type Compiled = OnceLock<Result<CompiledQuery, String>>;

pub struct LanguageDef {
    pub id: &'static str,
    grammar: fn() -> Language,
    /// Query sources joined in order; TypeScript builds on the JavaScript query, C++ on C.
    highlights: &'static [&'static str],
    compiled: &'static Compiled,
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
                Ok(CompiledQuery { query, capture_styles })
            })
            .as_ref()
            .map_err(String::as_str)
    }
}

#[allow(unused_macros)] // unused when the build enables no language
macro_rules! language {
    ($id:literal, $grammar:expr, [$($query:expr),+ $(,)?]) => {
        LanguageDef {
            id: $id,
            grammar: || $grammar.into(),
            highlights: &[$($query),+],
            compiled: {
                static COMPILED: Compiled = OnceLock::new();
                &COMPILED
            },
        }
    };
}

static LANGUAGES: &[LanguageDef] = &[
    #[cfg(feature = "rust")]
    language!("rust", tree_sitter_rust::LANGUAGE, [tree_sitter_rust::HIGHLIGHTS_QUERY]),
    #[cfg(feature = "javascript")]
    language!(
        "javascript",
        tree_sitter_javascript::LANGUAGE,
        [tree_sitter_javascript::HIGHLIGHT_QUERY, tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,]
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
    language!("html", tree_sitter_html::LANGUAGE, [tree_sitter_html::HIGHLIGHTS_QUERY]),
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
    language!("markdown", tree_sitter_md::LANGUAGE, [tree_sitter_md::HIGHLIGHT_QUERY_BLOCK]),
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
    fn every_highlight_query_compiles() {
        for lang in languages() {
            let q = lang.highlight_query().unwrap_or_else(|e| panic!("{e}"));
            assert!(q.capture_styles.iter().any(|&s| s != 0), "{} maps no capture to a style", lang.id);
            assert_eq!(lang.info().id, lang.id);
        }
    }
}
