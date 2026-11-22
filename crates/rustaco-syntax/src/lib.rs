//! Syntax highlighting for the Rustaco editor.
//!
//! [`Syntax`] keeps a tree-sitter tree in step with a [`rustaco_core::TextModel`] by
//! replaying the model's edit events, and highlights only the lines a view asks for.
//! Highlights carry style ids; a [`Theme`] built from VS Code theme rules turns the ids
//! into colours.
//!
//! [`Engine`] puts tree-sitter, TextMate and the Mermaid tokenizer behind one interface in
//! char offsets, and [`SyntaxService`] (feature `service`) serves the bundled languages as a
//! language server: highlighting, folding, symbols, and completion, definition, references
//! and rename within a file.

#[cfg(all(feature = "engine", not(target_arch = "wasm32")))]
mod background;
mod catalog;
mod default_theme;
#[cfg(feature = "highlighting")]
mod engines;
#[cfg(feature = "engine")]
mod highlight;
#[cfg(feature = "engine")]
mod injection;
#[cfg(feature = "engine")]
mod language;
#[cfg(feature = "engine")]
mod locals;
#[cfg(feature = "highlighting")]
pub mod mermaid;
mod names;
#[cfg(feature = "service")]
mod service;
#[cfg(feature = "highlighting")]
mod spans;
#[cfg(feature = "textmate")]
mod textmate;
mod theme;

#[cfg(all(feature = "engine", not(target_arch = "wasm32")))]
pub use background::{BackgroundSyntax, HighlightReply};
pub use catalog::{LanguageConfig, LanguageInfo, catalog, detect, info, language_editing};
#[cfg(feature = "highlighting")]
pub use engines::{CharSymbol, Engine};
#[cfg(feature = "engine")]
pub use highlight::{Symbol, Syntax};
#[cfg(feature = "engine")]
pub use injection::{MAX_INJECTION_BYTES, resolve as resolve_injection_language};
#[cfg(feature = "engine")]
pub use language::{LanguageDef, by_id, languages};
pub use names::{BRACKET_COLORS, BRACKET_STYLE_BASE, HIGHLIGHT_NAMES, name_of_style, style_for_capture};
#[cfg(feature = "service")]
pub use service::SyntaxService;
#[cfg(feature = "highlighting")]
pub use spans::{Highlights, MAX_HIGHLIGHT_COLUMNS, TextContext};
#[cfg(feature = "textmate")]
pub use textmate::{Grammar, TextMateSyntax, grammar as textmate_grammar, textmate_languages};
pub use theme::{Colors, Style, Theme, ThemeRule};

/// Themes bundled with the editor.
pub mod themes {
    pub use crate::default_theme::{dark, high_contrast, light};
}
