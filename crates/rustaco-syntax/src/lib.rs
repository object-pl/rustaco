//! Syntax highlighting for the Rustaco editor.
//!
//! [`Syntax`] keeps a tree-sitter tree in step with a [`rustaco_core::TextModel`] by
//! replaying the model's edit events, and highlights only the lines a view asks for.
//! Highlights carry style ids; a [`Theme`] built from VS Code theme rules turns the ids
//! into colours.

mod catalog;
mod default_theme;
#[cfg(feature = "engine")]
mod highlight;
#[cfg(feature = "engine")]
mod injection;
#[cfg(feature = "engine")]
mod language;
mod names;
#[cfg(any(feature = "engine", feature = "textmate"))]
mod spans;
#[cfg(feature = "textmate")]
mod textmate;
mod theme;

pub use catalog::{LanguageConfig, LanguageInfo, catalog, detect, info};
#[cfg(feature = "engine")]
pub use highlight::Syntax;
#[cfg(feature = "engine")]
pub use injection::{MAX_INJECTION_BYTES, resolve as resolve_injection_language};
#[cfg(feature = "engine")]
pub use language::{LanguageDef, by_id, languages};
pub use names::{HIGHLIGHT_NAMES, name_of_style, style_for_capture};
#[cfg(any(feature = "engine", feature = "textmate"))]
pub use spans::{Highlights, MAX_HIGHLIGHT_COLUMNS, TextContext};
#[cfg(feature = "textmate")]
pub use textmate::{Grammar, TextMateSyntax, grammar as textmate_grammar, textmate_languages};
pub use theme::{Colors, Style, Theme, ThemeRule};

/// Themes bundled with the editor.
pub mod themes {
    pub use crate::default_theme::{dark, high_contrast, light};
}
